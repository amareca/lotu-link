use std::sync::Arc;

use crate::dto::DeviceDto;
use crate::ports::{CatalogError, DeviceCatalog};

/// Application use case for listing devices.
pub struct DeviceService {
    catalog: Arc<dyn DeviceCatalog>,
}

impl DeviceService {
    pub fn new(catalog: Arc<dyn DeviceCatalog>) -> Self {
        Self { catalog }
    }

    /// Returns one DTO per device reported by the catalog.
    ///
    /// An empty list is a valid result. Ordering is left untouched: the service
    /// does not add presentation rules.
    pub async fn list_devices(&self) -> Result<Vec<DeviceDto>, CatalogError> {
        let devices = self.catalog.list().await?;
        Ok(devices.into_iter().map(DeviceDto::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use lotu_domain::{Device, DeviceState, Resolution, Serial};

    #[derive(Clone)]
    struct FakeCatalog {
        result: Result<Vec<Device>, CatalogError>,
    }

    #[async_trait]
    impl DeviceCatalog for FakeCatalog {
        async fn list(&self) -> Result<Vec<Device>, CatalogError> {
            self.result.clone()
        }
    }

    fn service(result: Result<Vec<Device>, CatalogError>) -> DeviceService {
        DeviceService::new(Arc::new(FakeCatalog { result }))
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

    #[tokio::test]
    async fn maps_devices_to_dtos() {
        let service = service(Ok(vec![
            device("R58M12345", DeviceState::Connected),
            device("1234567890ABC", DeviceState::Unauthorized),
        ]));

        let devices = service
            .list_devices()
            .await
            .expect("listing should succeed");

        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].serial, "R58M12345");
        assert_eq!(devices[0].state, DeviceState::Connected);
        assert_eq!(devices[0].android_version.as_deref(), Some("15"));
        assert_eq!(devices[0].resolution.expect("resolution").width, 1080);
        assert_eq!(devices[0].battery_percent, Some(85));
        assert_eq!(devices[0].transport, "usb");
        assert_eq!(devices[1].state, DeviceState::Unauthorized);
        assert_eq!(devices[1].transport, "usb");
    }

    #[tokio::test]
    async fn preserves_catalog_order() {
        let service = service(Ok(vec![
            device("B", DeviceState::Offline),
            device("A", DeviceState::Connected),
        ]));

        let devices = service
            .list_devices()
            .await
            .expect("listing should succeed");

        assert_eq!(
            devices
                .iter()
                .map(|device| device.serial.as_str())
                .collect::<Vec<_>>(),
            vec!["B", "A"]
        );
    }

    #[tokio::test]
    async fn empty_list_is_valid() {
        let service = service(Ok(Vec::new()));

        let devices = service
            .list_devices()
            .await
            .expect("listing should succeed");

        assert!(devices.is_empty());
    }

    #[tokio::test]
    async fn propagates_unavailable_errors() {
        let service = service(Err(CatalogError::Unavailable("adb not found".to_owned())));

        let error = service
            .list_devices()
            .await
            .expect_err("listing should fail");

        assert_eq!(error, CatalogError::Unavailable("adb not found".to_owned()));
    }

    #[tokio::test]
    async fn propagates_timeout_errors() {
        let service = service(Err(CatalogError::Timeout));

        let error = service
            .list_devices()
            .await
            .expect_err("listing should fail");

        assert_eq!(error, CatalogError::Timeout);
    }

    #[tokio::test]
    async fn propagates_failed_errors() {
        let service = service(Err(CatalogError::Failed("unexpected output".to_owned())));

        let error = service
            .list_devices()
            .await
            .expect_err("listing should fail");

        assert_eq!(error, CatalogError::Failed("unexpected output".to_owned()));
    }
}
