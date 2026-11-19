//! `start_replay` command: serves the bundled fixture over the internal
//! WebSocket (M0b).
//!
//! Replay is a development aid, so the command is compiled to a stub that fails
//! with `replay_unavailable` in release builds. Everything else lives behind
//! `#[cfg(debug_assertions)]` to keep release builds free of the fixture and
//! server wiring.

use lotu_app::dto::ReplaySessionDto;
use tauri::State;

use crate::commands::error::{ErrorCode, ErrorDto};
use crate::state::AppState;

/// Fixture served when `LOTU_REPLAY_FIXTURE` is not set.
///
/// Resolved from the crate manifest (`src-tauri`) so the command works from any
/// checkout path and from any process working directory.
#[cfg(debug_assertions)]
const DEFAULT_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../fixtures/replay/pixel-portrait.lotureplay"
);

/// Environment variable overriding the fixture path (`LOTU_REPLAY_FIXTURE`).
#[cfg(debug_assertions)]
const FIXTURE_ENV: &str = "LOTU_REPLAY_FIXTURE";

/// Starts (or replaces) the replay session and returns its WebSocket URL.
///
/// The URL already carries the per-session token; the device comes from the
/// fixture metadata (`docs/m0-contract.md`, 6.3).
#[tauri::command]
pub async fn start_replay(state: State<'_, AppState>) -> Result<ReplaySessionDto, ErrorDto> {
    #[cfg(not(debug_assertions))]
    {
        let _ = state;
        Err(replay_unavailable())
    }
    #[cfg(debug_assertions)]
    {
        start_replay_with_fixture(state.inner(), fixture_path()).await
    }
}

#[cfg(not(debug_assertions))]
fn replay_unavailable() -> ErrorDto {
    ErrorDto::new(
        ErrorCode::ReplayUnavailable,
        "replay is only available in development builds",
    )
}

/// Fixture path: `LOTU_REPLAY_FIXTURE` when set, the bundled fixture otherwise.
#[cfg(debug_assertions)]
fn fixture_path() -> std::path::PathBuf {
    fixture_path_from(std::env::var_os(FIXTURE_ENV))
}

#[cfg(debug_assertions)]
fn fixture_path_from(override_path: Option<std::ffi::OsString>) -> std::path::PathBuf {
    override_path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_FIXTURE))
}

/// Loads `path` and swaps the replay actor for a new one.
///
/// The fixture is loaded before touching the running actor, so an invalid
/// fixture leaves the current session untouched. A previous actor is stopped
/// and awaited before the new listener binds: at most one listener, source and
/// socket exist at any time.
#[cfg(debug_assertions)]
async fn start_replay_with_fixture(
    state: &AppState,
    path: std::path::PathBuf,
) -> Result<ReplaySessionDto, ErrorDto> {
    use lotu_adapters::replay::ReplayFrameSource;

    use crate::state::ReplayHandle;
    use crate::stream_ws::ReplayServer;

    let source = ReplayFrameSource::load(&path).map_err(ErrorDto::from)?;
    let hello = source.hello().to_owned();
    let device = source.device().clone();

    // One session per application: a live session must be stopped before the
    // replay actor binds its listener (`docs/m1-contract.md` 4.6).
    state.abort_replay_and_wait().await;
    state.stop_session_and_wait().await;

    let server = ReplayServer::bind(hello, Box::new(source))
        .await
        .map_err(|error| ErrorDto::new(ErrorCode::Internal, error.to_string()))?;
    let ws_url = server.ws_url();
    let task = tauri::async_runtime::spawn(server.run());

    state.set_replay(ReplayHandle {
        task,
        ws_url: ws_url.clone(),
        device: device.clone(),
    });

    Ok(ReplaySessionDto { ws_url, device })
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use async_trait::async_trait;
    use futures_util::StreamExt;
    use lotu_adapters::media::Mp4Recorder;
    use lotu_app::ports::{CatalogError, DeviceCatalog, ScreenshotCapture, VideoSource};
    use lotu_app::services::{DeviceService, RecordingService, SessionService};
    use lotu_domain::{Device, DeviceState};
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    use super::*;

    struct EmptyCatalog;

    #[async_trait]
    impl DeviceCatalog for EmptyCatalog {
        async fn list(&self) -> Result<Vec<Device>, CatalogError> {
            Ok(Vec::new())
        }
    }

    /// The replay tests never open a live session; the port only has to exist.
    struct UnusedVideoSource;

    #[async_trait]
    impl VideoSource for UnusedVideoSource {
        async fn open(
            &self,
            _serial: &lotu_domain::Serial,
            _config: &lotu_domain::VideoConfig,
        ) -> Result<lotu_app::ports::LiveStream, lotu_app::ports::SessionError> {
            Err(lotu_app::ports::SessionError::Failed(
                "live sessions are not used by the replay tests".to_owned(),
            ))
        }
    }

    struct UnusedCapture;

    #[async_trait]
    impl ScreenshotCapture for UnusedCapture {
        async fn capture(
            &self,
            _serial: &lotu_domain::Serial,
        ) -> Result<lotu_app::ports::PngImage, lotu_app::ports::CaptureError> {
            Err(lotu_app::ports::CaptureError::Failed("unused".to_owned()))
        }
    }

    fn app_state() -> AppState {
        let catalog: Arc<dyn DeviceCatalog> = Arc::new(EmptyCatalog);
        let video: Arc<dyn VideoSource> = Arc::new(UnusedVideoSource);
        let capture: Arc<dyn ScreenshotCapture> = Arc::new(UnusedCapture);

        AppState::new(
            DeviceService::new(Arc::clone(&catalog)),
            SessionService::new(catalog, video),
            capture,
            Arc::new(RecordingService::new(Box::new(Mp4Recorder::new()))),
            crate::test_support::settings_service(),
            crate::test_support::fingerprint_source(),
        )
    }

    /// The committed fixture, resolved from the crate manifest.
    fn committed_fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("fixtures")
            .join("replay")
            .join("pixel-portrait.lotureplay")
    }

    #[test]
    fn default_fixture_is_the_committed_pixel_portrait() {
        assert_eq!(
            Path::new(DEFAULT_FIXTURE)
                .canonicalize()
                .expect("the bundled fixture must exist"),
            committed_fixture()
                .canonicalize()
                .expect("the committed fixture must exist")
        );
    }

    #[test]
    fn fixture_env_override_wins_over_the_bundled_fixture() {
        let override_path = Path::new("C:/fixtures/custom.lotureplay");
        let resolved = fixture_path_from(Some(override_path.as_os_str().to_owned()));
        assert_eq!(resolved, override_path);

        assert_eq!(fixture_path_from(None), Path::new(DEFAULT_FIXTURE));
    }

    #[tokio::test]
    async fn serves_the_fixture_device_and_a_loopback_url() {
        let state = app_state();

        let session = start_replay_with_fixture(&state, committed_fixture())
            .await
            .expect("the committed fixture should start a session");

        assert!(
            session.ws_url.starts_with("ws://127.0.0.1:"),
            "{}",
            session.ws_url
        );
        assert!(
            session.ws_url.contains("/stream?token="),
            "{}",
            session.ws_url
        );
        assert_eq!(session.device.serial, "REPLAY0001");
        assert_eq!(session.device.name, "Pixel 8 Pro (replay)");
        assert_eq!(session.device.state, DeviceState::Connected);
        assert_eq!(
            session.device.resolution.map(|r| (r.width, r.height)),
            Some((1080, 2400))
        );

        // The returned URL is usable and the hello comes first.
        let (mut ws, _response) = connect_async(session.ws_url.as_str())
            .await
            .expect("the session URL should accept a client");
        let hello = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next())
            .await
            .expect("hello should arrive")
            .expect("the socket should stay open")
            .expect("the message should be valid");
        assert!(
            matches!(hello, Message::Text(ref text) if text.as_str().contains("\"type\":\"hello\"")),
            "{hello:?}"
        );

        state.abort_replay_and_wait().await;
    }

    #[tokio::test]
    async fn a_second_call_replaces_the_previous_actor() {
        let state = app_state();

        let first = start_replay_with_fixture(&state, committed_fixture())
            .await
            .expect("the first session should start");
        let second = start_replay_with_fixture(&state, committed_fixture())
            .await
            .expect("the second session should start");

        assert_ne!(first.ws_url, second.ws_url);

        // The previous listener is gone: its port no longer accepts clients.
        assert!(
            connect_async(first.ws_url.as_str()).await.is_err(),
            "the replaced actor must stop listening"
        );

        // The new session serves, so exactly one actor remains.
        let (_ws, _response) = connect_async(second.ws_url.as_str())
            .await
            .expect("the replacement session should accept a client");

        state.abort_replay_and_wait().await;
    }

    #[tokio::test]
    async fn an_invalid_fixture_reports_fixture_invalid_and_keeps_the_actor() {
        let state = app_state();
        let session = start_replay_with_fixture(&state, committed_fixture())
            .await
            .expect("the committed fixture should start a session");

        let missing = committed_fixture().with_file_name("missing.lotureplay");
        let error = start_replay_with_fixture(&state, missing)
            .await
            .expect_err("a missing fixture must fail");
        assert_eq!(error.code, ErrorCode::FixtureInvalid);

        // The previous actor is untouched and still serving.
        let (_ws, _response) = connect_async(session.ws_url.as_str())
            .await
            .expect("the previous session should still accept clients");

        state.abort_replay_and_wait().await;
    }
}
