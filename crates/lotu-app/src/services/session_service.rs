use std::sync::Arc;

use lotu_domain::{DeviceState, Serial, VideoConfig};

use crate::ports::{DeviceCatalog, LiveStream, SessionError, VideoSource};

/// Application use case that opens the live session of one device.
///
/// The service only knows the catalog and the video port: no ADB, scrcpy or
/// sockets. The state machine ([`lotu_domain::SessionState`]) belongs to the
/// session actor; this service does not duplicate transitions.
pub struct SessionService {
    catalog: Arc<dyn DeviceCatalog>,
    video: Arc<dyn VideoSource>,
}

impl SessionService {
    pub fn new(catalog: Arc<dyn DeviceCatalog>, video: Arc<dyn VideoSource>) -> Self {
        Self { catalog, video }
    }

    /// Verifies that the device exists and is `connected`, then delegates the
    /// opening to the video port.
    ///
    /// A catalog failure is reported as [`SessionError::Adb`] so the delivery
    /// layer keeps the stable M0 error codes (`adb_unavailable`, `adb_timeout`,
    /// `adb_failed`).
    pub async fn start(
        &self,
        serial: &Serial,
        config: &VideoConfig,
    ) -> Result<LiveStream, SessionError> {
        let devices = self.catalog.list().await.map_err(SessionError::Adb)?;
        let device = devices
            .into_iter()
            .find(|device| device.serial == *serial)
            .ok_or(SessionError::DeviceNotFound)?;

        if device.state != DeviceState::Connected {
            return Err(SessionError::DeviceNotConnected);
        }

        self.video.open(serial, config).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use lotu_domain::{Device, Resolution};

    use super::*;
    use crate::ports::{
        CatalogError, FrameSource, InputError, InputMessage, InputSink, SessionStream,
        StreamMessage,
    };

    const SERIAL: &str = "R58M12345";

    struct FakeCatalog {
        devices: Result<Vec<Device>, CatalogError>,
    }

    #[async_trait]
    impl DeviceCatalog for FakeCatalog {
        async fn list(&self) -> Result<Vec<Device>, CatalogError> {
            self.devices.clone()
        }
    }

    /// Input sink double: live sessions always carry one, even in tests.
    struct FakeInputSink;

    #[async_trait]
    impl InputSink for FakeInputSink {
        async fn send(&self, _message: InputMessage) -> Result<(), InputError> {
            Ok(())
        }

        async fn get_clipboard(&self, _copy_key: u8) -> Result<String, InputError> {
            Ok(String::new())
        }
    }

    /// Video port double: records every call, can fail, and counts `close()`.
    struct FakeVideoSource {
        result: Result<(), SessionError>,
        opens: Mutex<Vec<(String, VideoConfig)>>,
        closes: Arc<Mutex<usize>>,
    }

    impl FakeVideoSource {
        fn shared(result: Result<(), SessionError>) -> Arc<Self> {
            Arc::new(Self {
                result,
                opens: Mutex::new(Vec::new()),
                closes: Arc::new(Mutex::new(0)),
            })
        }

        fn opens(&self) -> Vec<(String, VideoConfig)> {
            self.opens.lock().expect("opens lock").clone()
        }

        fn close_count(&self) -> usize {
            *self.closes.lock().expect("closes lock")
        }

        fn record_open(&self, serial: &Serial, config: &VideoConfig) {
            self.opens
                .lock()
                .expect("opens lock")
                .push((serial.to_string(), *config));
        }
    }

    #[async_trait]
    impl VideoSource for FakeVideoSource {
        async fn open(
            &self,
            serial: &Serial,
            config: &VideoConfig,
        ) -> Result<LiveStream, SessionError> {
            self.record_open(serial, config);

            if let Err(error) = &self.result {
                return Err(error.clone());
            }

            Ok(LiveStream {
                device_name: "Pixel 8 Pro (fake)".to_owned(),
                stream: Box::new(FakeSessionStream {
                    closes: Arc::clone(&self.closes),
                }),
                input: Arc::new(FakeInputSink),
            })
        }
    }

    struct FakeSessionStream {
        closes: Arc<Mutex<usize>>,
    }

    #[async_trait]
    impl FrameSource for FakeSessionStream {
        async fn next_message(&mut self) -> Option<StreamMessage> {
            None
        }
    }

    #[async_trait]
    impl SessionStream for FakeSessionStream {
        async fn close(self: Box<Self>) {
            *self.closes.lock().expect("closes lock") += 1;
        }
    }

    fn parse_serial(raw: &str) -> Serial {
        Serial::new(raw).expect("valid serial")
    }

    fn device(serial: &str, state: DeviceState) -> Device {
        Device::new(
            Serial::new(serial).expect("valid serial"),
            "Pixel 8 Pro",
            state,
            Some("15".to_owned()),
            Some(Resolution::new(1080, 2400).expect("valid resolution")),
            Some(85),
        )
        .expect("valid device")
    }

    fn connected_devices() -> Vec<Device> {
        vec![device(SERIAL, DeviceState::Connected)]
    }

    fn service(
        devices: Result<Vec<Device>, CatalogError>,
        video: &Arc<FakeVideoSource>,
    ) -> SessionService {
        let video = Arc::clone(video);
        let video: Arc<dyn VideoSource> = video;
        SessionService::new(Arc::new(FakeCatalog { devices }), video)
    }

    /// `Result::expect_err` needs `LiveStream: Debug`; a `SessionStream` trait
    /// object is not, so the tests extract the error explicitly.
    fn expect_error(result: Result<LiveStream, SessionError>) -> SessionError {
        match result {
            Ok(_) => panic!("the session must not start"),
            Err(error) => error,
        }
    }

    #[tokio::test]
    async fn starts_the_session_of_a_connected_device() {
        let video = FakeVideoSource::shared(Ok(()));
        let service = service(Ok(connected_devices()), &video);
        let serial = parse_serial(SERIAL);
        let config = VideoConfig::default();

        let live = service.start(&serial, &config).await.expect("start");

        assert_eq!(live.device_name, "Pixel 8 Pro (fake)");
        assert_eq!(video.opens(), vec![(SERIAL.to_owned(), config)]);
        assert_eq!(video.close_count(), 0);

        live.stream.close().await;
        assert_eq!(video.close_count(), 1, "close must run once");
    }

    #[tokio::test]
    async fn passes_the_serial_and_config_untouched() {
        let video = FakeVideoSource::shared(Ok(()));
        let service = service(Ok(connected_devices()), &video);
        let config = VideoConfig::new(1280, 30, 4_000_000).expect("valid config");

        let live = service.start(&parse_serial(SERIAL), &config).await;
        let live = live.expect("start");

        assert_eq!(video.opens(), vec![(SERIAL.to_owned(), config)]);
        live.stream.close().await;
    }

    #[tokio::test]
    async fn unknown_serial_is_not_found() {
        let video = FakeVideoSource::shared(Ok(()));
        let service = service(Ok(connected_devices()), &video);
        let serial = parse_serial("UNKNOWN0001");

        let result = service.start(&serial, &VideoConfig::default()).await;
        let error = expect_error(result);

        assert_eq!(error, SessionError::DeviceNotFound);
        assert!(video.opens().is_empty(), "the port must not be called");
    }

    #[tokio::test]
    async fn empty_catalog_is_not_found() {
        let video = FakeVideoSource::shared(Ok(()));
        let service = service(Ok(Vec::new()), &video);
        let serial = parse_serial(SERIAL);

        let result = service.start(&serial, &VideoConfig::default()).await;
        let error = expect_error(result);

        assert_eq!(error, SessionError::DeviceNotFound);
        assert!(video.opens().is_empty(), "the port must not be called");
    }

    #[tokio::test]
    async fn unauthorized_and_offline_devices_are_not_connected() {
        for state in [DeviceState::Unauthorized, DeviceState::Offline] {
            let video = FakeVideoSource::shared(Ok(()));
            let service = service(Ok(vec![device(SERIAL, state)]), &video);
            let serial = parse_serial(SERIAL);

            let result = service.start(&serial, &VideoConfig::default()).await;
            let error = expect_error(result);

            assert_eq!(error, SessionError::DeviceNotConnected, "{state:?}");
            assert!(video.opens().is_empty(), "the port must not be called");
        }
    }

    #[tokio::test]
    async fn catalog_errors_are_preserved_as_adb_errors() {
        for catalog_error in [
            CatalogError::Unavailable("adb not found".to_owned()),
            CatalogError::Timeout,
            CatalogError::Failed("unexpected output".to_owned()),
        ] {
            let video = FakeVideoSource::shared(Ok(()));
            let service = service(Err(catalog_error.clone()), &video);
            let serial = parse_serial(SERIAL);

            let result = service.start(&serial, &VideoConfig::default()).await;
            let error = expect_error(result);

            assert_eq!(error, SessionError::Adb(catalog_error));
            assert!(video.opens().is_empty(), "the port must not be called");
        }
    }

    #[tokio::test]
    async fn port_errors_are_propagated() {
        let cases = [
            SessionError::Failed("protocol error".to_owned()),
            SessionError::VendorMissing("scrcpy-server.jar".to_owned()),
            SessionError::Timeout,
        ];

        for expected in cases {
            let video = FakeVideoSource::shared(Err(expected.clone()));
            let service = service(Ok(connected_devices()), &video);
            let serial = parse_serial(SERIAL);

            let result = service.start(&serial, &VideoConfig::default()).await;
            let error = expect_error(result);

            assert_eq!(error, expected);
            assert_eq!(
                video.opens(),
                vec![(SERIAL.to_owned(), VideoConfig::default())],
                "the port must be called once"
            );
        }
    }
}
