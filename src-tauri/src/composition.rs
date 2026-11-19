use std::path::PathBuf;
use std::sync::Arc;

use lotu_adapters::adb::SystemAdb;
use lotu_adapters::media::{AdbScreenshotCapture, Mp4Recorder};
use lotu_adapters::platform;
use lotu_adapters::scrcpy::ScrcpyVideoSource;
use lotu_app::ports::{DeviceCatalog, ScreenshotCapture, VideoSource};
use lotu_app::services::{DeviceService, RecordingService, SessionService, SettingsService};

use crate::settings_store::FileSettingsStore;
use crate::state::AppState;

/// Builds the object graph for the application.
///
/// A single [`SystemAdb`] is shared by the catalog and by the live video source
/// (`docs/m1-contract.md` 4.6); the scrcpy server jar is resolved through
/// `platform` (2.5) and is only required when a session starts.
pub fn build_app_state(config_dir: PathBuf) -> AppState {
    let adb = Arc::new(SystemAdb::new(
        platform::adb_binary_path(),
        platform::adb_server_port(),
    ));

    let catalog: Arc<dyn DeviceCatalog> = adb.clone();
    let video: Arc<dyn VideoSource> = Arc::new(ScrcpyVideoSource::new(
        Arc::clone(&adb),
        platform::scrcpy_server_path(),
    ));
    let capture: Arc<dyn ScreenshotCapture> = Arc::new(AdbScreenshotCapture::new(Arc::clone(&adb)));
    let recording = Arc::new(RecordingService::new(Box::new(Mp4Recorder::new())));
    let settings = Arc::new(SettingsService::new(Box::new(FileSettingsStore::new(
        config_dir,
    ))));

    AppState::new(
        DeviceService::new(Arc::clone(&catalog)),
        SessionService::new(catalog, video),
        capture,
        recording,
        settings,
    )
}
