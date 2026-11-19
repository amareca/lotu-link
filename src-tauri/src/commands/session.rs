//! `start_session` / `stop_session`: live scrcpy session wired to the internal
//! WebSocket actor (M1).
//!
//! The command is thin: it validates the serial, resolves the device DTO from
//! the catalog, enforces one session per application (live or replay), asks the
//! application service to open the stream and hands the ownership to the
//! [`ReplayServer`] live actor. No scrcpy or ADB detail leaks here.

use lotu_app::dto::SessionDto;
use lotu_domain::{DeviceState, Serial, VideoConfig};
use tauri::State;

use crate::commands::error::{ErrorCode, ErrorDto};
use crate::state::{AppState, SessionHandle};
use crate::stream_ws::ReplayServer;

/// Hello frame of the internal protocol (`docs/stream-protocol.md`); identical
/// to the replay one, so the frontend does not distinguish the origin.
const HELLO: &str = r#"{"v":1,"type":"hello","codec":"h264"}"#;

/// Starts (or replaces) the live session of `serial` and returns its WebSocket
/// URL.
///
/// Mutual exclusion: the replay and any previous live session are stopped
/// before the new one starts, so at most one actor, source and listener exist
/// at any time.
#[tauri::command]
pub async fn start_session(
    serial: String,
    state: State<'_, AppState>,
) -> Result<SessionDto, ErrorDto> {
    start_session_impl(state.inner(), &serial).await
}

/// Stops the live session, if any. Idempotent.
#[tauri::command]
pub async fn stop_session(state: State<'_, AppState>) -> Result<(), ErrorDto> {
    state.stop_session_and_wait().await;
    Ok(())
}

async fn start_session_impl(state: &AppState, raw_serial: &str) -> Result<SessionDto, ErrorDto> {
    // 1. Validate the serial. An invalid one cannot name any device.
    let serial = Serial::new(raw_serial).map_err(|_| {
        ErrorDto::new(
            ErrorCode::DeviceNotFound,
            format!("invalid device serial {raw_serial:?}"),
        )
    })?;

    // 2. The DTO comes from the catalog, the single source of device data. The
    //    application service repeats the existence/state check when it opens
    //    the stream (race with an unplugged device).
    let device = state
        .devices
        .list_devices()
        .await
        .map_err(ErrorDto::from)?
        .into_iter()
        .find(|device| device.serial == serial.as_str())
        .ok_or_else(|| {
            ErrorDto::new(
                ErrorCode::DeviceNotFound,
                format!("device {serial} is not in the catalog"),
            )
        })?;

    if device.state != DeviceState::Connected {
        return Err(ErrorDto::new(
            ErrorCode::DeviceNotConnected,
            format!("device {serial} is not connected"),
        ));
    }

    // 3. One session per application: stop the replay and the previous live
    //    session before binding a new listener.
    state.abort_replay_and_wait().await;
    state.stop_session_and_wait().await;

    // 4. Open the live stream (15 s budget inside the adapter).
    let live = state
        .session_service
        .start(&serial, &VideoConfig::default())
        .await
        .map_err(ErrorDto::from)?;

    // 5. Serve it through the live policy of the WebSocket actor.
    let server = ReplayServer::bind_live(HELLO, live.stream)
        .await
        .map_err(|error| ErrorDto::new(ErrorCode::Internal, error.to_string()))?;
    let ws_url = server.ws_url();
    let task = tauri::async_runtime::spawn(server.run());

    state.set_session(SessionHandle {
        task,
        ws_url: ws_url.clone(),
        device: device.clone(),
        input: live.input,
    });

    Ok(SessionDto { ws_url, device })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use futures_util::StreamExt;
    use lotu_app::dto::DeviceDto;
    use lotu_app::ports::{
        CatalogError, DeviceCatalog, FrameSource, InputError, InputMessage, InputSink, LiveStream,
        SessionError, SessionStream, StreamMessage, VideoSource,
    };
    use lotu_app::services::{DeviceService, SessionService};
    use lotu_domain::{Device, DeviceState, Resolution, Serial, VideoConfig};
    use tokio_tungstenite::tungstenite::Message;

    use super::*;

    const SERIAL: &str = "R58M12345";
    const READ_TIMEOUT: Duration = Duration::from_secs(2);

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

    struct FakeCatalog {
        devices: Vec<Device>,
    }

    #[async_trait]
    impl DeviceCatalog for FakeCatalog {
        async fn list(&self) -> Result<Vec<Device>, CatalogError> {
            Ok(self.devices.clone())
        }
    }

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

    /// Video port double: records `open` calls and never ends on its own, so the
    /// actor stays alive until it is aborted.
    struct PendingVideoSource {
        opens: Arc<AtomicUsize>,
        closes: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl VideoSource for PendingVideoSource {
        async fn open(
            &self,
            _serial: &Serial,
            _config: &VideoConfig,
        ) -> Result<LiveStream, SessionError> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            Ok(LiveStream {
                device_name: "Pixel 8 Pro (fake)".to_owned(),
                stream: Box::new(PendingStream {
                    closes: Arc::clone(&self.closes),
                }),
                input: Arc::new(FakeInputSink),
            })
        }
    }

    struct PendingStream {
        closes: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl FrameSource for PendingStream {
        async fn next_message(&mut self) -> Option<StreamMessage> {
            std::future::pending().await
        }
    }

    #[async_trait]
    impl SessionStream for PendingStream {
        async fn close(self: Box<Self>) {
            self.closes.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Harness {
        state: AppState,
        opens: Arc<AtomicUsize>,
    }

    fn harness(devices: Vec<Device>) -> Harness {
        let opens = Arc::new(AtomicUsize::new(0));
        let closes = Arc::new(AtomicUsize::new(0));
        let catalog: Arc<dyn DeviceCatalog> = Arc::new(FakeCatalog { devices });
        let video: Arc<dyn VideoSource> = Arc::new(PendingVideoSource {
            opens: Arc::clone(&opens),
            closes,
        });

        let state = AppState::new(
            DeviceService::new(Arc::clone(&catalog)),
            SessionService::new(catalog, video),
        );

        Harness { state, opens }
    }

    fn connected() -> Vec<Device> {
        vec![device(SERIAL, DeviceState::Connected)]
    }

    #[tokio::test]
    async fn starts_a_live_session_and_serves_the_hello() {
        let harness = harness(connected());

        let session = start_session_impl(&harness.state, SERIAL)
            .await
            .expect("the session should start");

        assert!(
            session.ws_url.starts_with("ws://127.0.0.1:"),
            "{}",
            session.ws_url
        );
        assert_eq!(session.device.serial, SERIAL);
        assert_eq!(session.device.state, DeviceState::Connected);
        assert_eq!(harness.opens.load(Ordering::SeqCst), 1);

        let (mut ws, _response) = tokio_tungstenite::connect_async(session.ws_url.as_str())
            .await
            .expect("the session URL should accept a client");
        let hello = tokio::time::timeout(READ_TIMEOUT, ws.next())
            .await
            .expect("hello should arrive")
            .expect("the socket should stay open")
            .expect("the message should be valid");
        assert_eq!(hello, Message::text(HELLO));

        harness.state.stop_session_and_wait().await;

        // After stopping, the listener is gone and the driver was replaced.
        assert!(
            tokio_tungstenite::connect_async(session.ws_url.as_str())
                .await
                .is_err(),
            "the stopped session must stop listening"
        );
    }

    #[tokio::test]
    async fn invalid_or_unknown_devices_do_not_open_the_port() {
        let harness = harness(connected());

        let error = start_session_impl(&harness.state, "BAD SERIAL")
            .await
            .expect_err("a serial with whitespace is invalid");
        assert_eq!(error.code, ErrorCode::DeviceNotFound);

        let error = start_session_impl(&harness.state, "UNKNOWN0001")
            .await
            .expect_err("an unknown serial must fail");
        assert_eq!(error.code, ErrorCode::DeviceNotFound);
        assert_eq!(
            harness.opens.load(Ordering::SeqCst),
            0,
            "the video port must not be called"
        );
    }

    #[tokio::test]
    async fn unauthorized_devices_report_not_connected() {
        let harness = harness(vec![device(SERIAL, DeviceState::Unauthorized)]);

        let error = start_session_impl(&harness.state, SERIAL)
            .await
            .expect_err("an unauthorized device must fail");
        assert_eq!(error.code, ErrorCode::DeviceNotConnected);
        assert_eq!(harness.opens.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn stop_session_is_idempotent() {
        let harness = harness(connected());
        harness.state.stop_session_and_wait().await;

        start_session_impl(&harness.state, SERIAL)
            .await
            .expect("the session should start");
        harness.state.stop_session_and_wait().await;

        // Nothing left to stop: the second call must return successfully.
        harness.state.stop_session_and_wait().await;
    }

    #[tokio::test]
    async fn a_second_start_replaces_the_previous_listener() {
        let harness = harness(connected());

        let first = start_session_impl(&harness.state, SERIAL)
            .await
            .expect("the first session should start");
        let second = start_session_impl(&harness.state, SERIAL)
            .await
            .expect("the second session should start");

        assert_ne!(first.ws_url, second.ws_url);
        assert!(
            tokio_tungstenite::connect_async(first.ws_url.as_str())
                .await
                .is_err(),
            "the replaced session must stop listening"
        );

        harness.state.stop_session_and_wait().await;
    }

    #[tokio::test]
    async fn the_handle_keeps_the_device_dto() {
        let harness = harness(connected());
        let dto = DeviceDto::from(device(SERIAL, DeviceState::Connected));

        start_session_impl(&harness.state, SERIAL)
            .await
            .expect("the session should start");

        // Opening a second session while the first exists must not leak a
        // second listener.
        let session = start_session_impl(&harness.state, SERIAL)
            .await
            .expect("the replacement session should start");
        assert_eq!(session.device, dto);

        harness.state.stop_session_and_wait().await;
    }
}
