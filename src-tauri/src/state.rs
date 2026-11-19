use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use lotu_app::dto::DeviceDto;
use lotu_app::ports::{InputSink, ScreenshotCapture};
use lotu_app::services::{DeviceService, RecordingService, SessionService, SettingsService};
use tauri::async_runtime::JoinHandle;
use tokio::time::timeout;

/// Budget of [`AppState::stop_session_and_wait`] (`docs/m1-contract.md` 4.6).
pub const SESSION_STOP_BUDGET: Duration = Duration::from_secs(8);

/// Running replay actor and the session data handed to the frontend.
///
/// Only one replay actor exists per application: installing a new handle always
/// invalidates the previous one.
pub struct ReplayHandle {
    pub task: JoinHandle<()>,
    pub ws_url: String,
    pub device: DeviceDto,
}

/// Running live session actor and the session data handed to the frontend.
///
/// The actor owns the live [`lotu_app::ports::SessionStream`], so aborting the
/// task drops the stream and its `Drop` releases the remote resources.
pub struct SessionHandle {
    pub task: JoinHandle<()>,
    pub ws_url: String,
    pub device: DeviceDto,
    /// Sink for M2 input on the live session's control socket.
    pub input: Arc<dyn InputSink>,
}

/// State shared by Tauri commands.
pub struct AppState {
    pub devices: DeviceService,
    pub session_service: SessionService,
    /// M3: native PNG capture of the active device.
    pub capture: Arc<dyn ScreenshotCapture>,
    /// M3: the single recording slot and its frame tap.
    pub recording: Arc<RecordingService>,
    /// M4: persisted video settings.
    pub settings: Arc<SettingsService>,
    replay: Mutex<Option<ReplayHandle>>,
    session: Mutex<Option<SessionHandle>>,
}

impl AppState {
    pub fn new(
        devices: DeviceService,
        session_service: SessionService,
        capture: Arc<dyn ScreenshotCapture>,
        recording: Arc<RecordingService>,
        settings: Arc<SettingsService>,
    ) -> Self {
        Self {
            devices,
            session_service,
            capture,
            recording,
            settings,
            replay: Mutex::new(None),
            session: Mutex::new(None),
        }
    }

    /// Installs the running replay actor, aborting any previous one.
    ///
    /// The caller is expected to have awaited the previous actor before binding
    /// a new listener; this is the last-resort guard against duplicates.
    pub fn set_replay(&self, handle: ReplayHandle) {
        let previous = self.replay().replace(handle);
        if let Some(previous) = previous {
            previous.task.abort();
        }
    }

    /// Aborts the replay actor, if any, and waits for its task to finish.
    ///
    /// Used when replacing the actor: the previous listener must be gone before
    /// the new one binds.
    pub async fn abort_replay_and_wait(&self) {
        if let Some(handle) = self.take_replay() {
            handle.task.abort();
            // The task aborts on its next await point; the join result only
            // reports the cancellation, so it carries no useful value.
            let _ = handle.task.await;
        }
    }

    /// Aborts the replay actor without waiting, for shutdown paths that cannot
    /// await (window close, app exit).
    pub fn abort_replay(&self) {
        if let Some(handle) = self.take_replay() {
            handle.task.abort();
        }
    }

    /// Installs the running live session actor, aborting any previous one.
    ///
    /// Same contract as [`set_replay`](Self::set_replay): callers stop the
    /// previous session first; this is the guard against duplicates.
    pub fn set_session(&self, handle: SessionHandle) {
        let previous = self.session().replace(handle);
        if let Some(previous) = previous {
            previous.task.abort();
        }
        // A recording belongs to one session: forget the old decoder config.
        let _ = self.recording.reset_stream();
    }

    /// Aborts the live session actor, if any, and waits for it to finish.
    ///
    /// The wait is bounded by [`SESSION_STOP_BUDGET`]: the remote cleanup runs
    /// from `Drop`, which must never block the command (`docs/m1-contract.md`
    /// 4.6). Idempotent: no live session is not an error.
    pub async fn stop_session_and_wait(&self) {
        let Some(handle) = self.take_session() else {
            return;
        };

        // Finalize the file before the actor stops feeding it. `stop` is
        // idempotent, so a session without recording is unaffected.
        let _ = self.recording.stop();
        handle.task.abort();
        // The task aborts on its next await point; the join result only reports
        // the cancellation, so it carries no useful value.
        let _ = timeout(SESSION_STOP_BUDGET, handle.task).await;
    }

    /// Aborts the live session actor without waiting, for shutdown paths that
    /// cannot await (window close, app exit).
    ///
    /// The stream `Drop` kills the server process and schedules the best-effort
    /// remote cleanup (`docs/m1-contract.md` 4.5, D8).
    pub fn abort_session(&self) {
        if let Some(handle) = self.take_session() {
            let _ = self.recording.stop();
            handle.task.abort();
        }
    }

    /// Serial of the active live session, needed by screenshot capture.
    pub fn session_serial(&self) -> Option<String> {
        self.session()
            .as_ref()
            .map(|handle| handle.device.serial.clone())
    }

    /// Clones the input sink of the live session, if one is active.
    ///
    /// Holding the `Arc` only for the duration of a command keeps the lock short
    /// and guarantees the sink cannot be dropped mid-send.
    pub fn session_input(&self) -> Option<Arc<dyn InputSink>> {
        self.session()
            .as_ref()
            .map(|handle| Arc::clone(&handle.input))
    }

    fn take_replay(&self) -> Option<ReplayHandle> {
        self.replay().take()
    }

    fn take_session(&self) -> Option<SessionHandle> {
        self.session().take()
    }

    /// Recovers a poisoned lock: the slot only ever holds an actor handle, so
    /// the guarded data stays usable after a panic in another command.
    fn replay(&self) -> MutexGuard<'_, Option<ReplayHandle>> {
        self.replay
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Recovers a poisoned lock, same reasoning as [`replay`](Self::replay).
    fn session(&self) -> MutexGuard<'_, Option<SessionHandle>> {
        self.session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
