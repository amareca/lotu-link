//! Test-only doubles for building `AppState` without disk I/O.

use std::sync::{Arc, Mutex};

use lotu_adapters::media::Mp4Recorder;
use lotu_app::ports::{ScreenshotCapture, SettingsData, SettingsError, SettingsStore};
use lotu_app::services::{DeviceService, RecordingService, SessionService, SettingsService};

use crate::state::AppState;

#[derive(Default)]
struct MemorySettingsStore {
    data: Mutex<SettingsData>,
}

impl SettingsStore for MemorySettingsStore {
    fn load(&self) -> Result<SettingsData, SettingsError> {
        Ok(*self.data.lock().expect("settings lock"))
    }

    fn save(&self, data: &SettingsData) -> Result<(), SettingsError> {
        *self.data.lock().expect("settings lock") = *data;
        Ok(())
    }
}

struct NoCapture;

#[async_trait::async_trait]
impl ScreenshotCapture for NoCapture {
    async fn capture(
        &self,
        _serial: &lotu_domain::Serial,
    ) -> Result<lotu_app::ports::PngImage, lotu_app::ports::CaptureError> {
        Ok(lotu_app::ports::PngImage {
            bytes: Vec::new(),
            width: 1080,
            height: 2400,
        })
    }
}

/// A settings service backed by memory, for tests that provide their own
/// catalog/video doubles.
pub fn settings_service() -> Arc<SettingsService> {
    Arc::new(SettingsService::new(Box::new(
        MemorySettingsStore::default(),
    )))
}

/// Builds an `AppState` with empty catalog/video doubles and memory settings.
pub fn app_state() -> AppState {
    let devices: Arc<dyn lotu_app::ports::DeviceCatalog> = Arc::new(EmptyCatalog);
    let video: Arc<dyn lotu_app::ports::VideoSource> = Arc::new(EmptyVideo);
    let capture: Arc<dyn ScreenshotCapture> = Arc::new(NoCapture);
    let recording = Arc::new(RecordingService::new(Box::new(Mp4Recorder::new())));
    let settings = settings_service();

    AppState::new(
        DeviceService::new(Arc::clone(&devices)),
        SessionService::new(devices, video),
        capture,
        recording,
        settings,
    )
}

struct EmptyCatalog;

#[async_trait::async_trait]
impl lotu_app::ports::DeviceCatalog for EmptyCatalog {
    async fn list(&self) -> Result<Vec<lotu_domain::Device>, lotu_app::ports::CatalogError> {
        Ok(Vec::new())
    }
}

struct EmptyVideo;

#[async_trait::async_trait]
impl lotu_app::ports::VideoSource for EmptyVideo {
    async fn open(
        &self,
        _serial: &lotu_domain::Serial,
        _config: &lotu_domain::VideoConfig,
    ) -> Result<lotu_app::ports::LiveStream, lotu_app::ports::SessionError> {
        Err(lotu_app::ports::SessionError::Failed(
            "test video source is not a stream".to_owned(),
        ))
    }
}
