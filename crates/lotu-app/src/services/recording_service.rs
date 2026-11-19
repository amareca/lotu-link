//! Application state of the active recording (M3).
//!
//! The service owns no files and no sockets: it remembers the last decoder
//! configuration seen on the stream, starts/stops the injected [`Recorder`] and
//! is the single frame tap called by the delivery actor. All rules live here so
//! Tauri commands and the WebSocket actor stay thin.

use std::path::PathBuf;
use std::sync::Mutex;

use crate::ports::{Recorder, RecordingError, RecordingStats, StreamMessage};

/// Application result of a start/stop command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingInfo {
    pub path: PathBuf,
    pub stats: RecordingStats,
}

struct State {
    active: bool,
    path: Option<PathBuf>,
    last_config: Option<StreamMessage>,
    /// Drop frames until the next IDR, so the MP4 always starts decodable.
    awaiting_keyframe: bool,
}

/// Single recording slot, shared by commands and the session actor.
pub struct RecordingService {
    recorder: Box<dyn Recorder>,
    state: Mutex<State>,
}

impl RecordingService {
    pub fn new(recorder: Box<dyn Recorder>) -> Self {
        Self {
            recorder,
            state: Mutex::new(State {
                active: false,
                path: None,
                last_config: None,
                awaiting_keyframe: false,
            }),
        }
    }

    /// Records to `path`, seeding the muxer with the last decoder configuration.
    pub fn start(&self, path: PathBuf) -> Result<(), RecordingError> {
        let mut state = self.lock()?;
        if state.active {
            return Err(RecordingError::AlreadyActive);
        }
        if path.as_os_str().is_empty() {
            return Err(RecordingError::Invalid("empty output path".to_owned()));
        }

        self.recorder
            .start(path.clone(), state.last_config.as_ref())?;
        state.active = true;
        state.path = Some(path);
        // The first sample must be an IDR; otherwise the MP4 opens with
        // undecodable P-frames (black screen with artefacts).
        state.awaiting_keyframe = true;
        Ok(())
    }

    /// Finalizes the file and clears the slot. Idempotent when inactive.
    pub fn stop(&self) -> Result<Option<RecordingInfo>, RecordingError> {
        let mut state = self.lock()?;
        if !state.active {
            return Ok(None);
        }

        // Clear the slot even when finalization fails: a failed short clip
        // must never block the next recording.
        state.active = false;
        state.awaiting_keyframe = false;
        let path = state.path.take().unwrap_or_default();
        let stats = self.recorder.finish()?;
        Ok(Some(RecordingInfo { path, stats }))
    }

    /// Called for every normalized stream message. Config messages are kept so
    /// recording can start mid-session; frames are appended only while active
    /// and after the next keyframe.
    pub fn observe(&self, message: &StreamMessage) -> Result<(), RecordingError> {
        let mut state = self.lock()?;
        if matches!(message, StreamMessage::Config { .. }) {
            state.last_config = Some(message.clone());
            if state.active {
                // The recorder may rotate to a new segment when the frame size
                // changes; after any config a fresh IDR is required again.
                state.awaiting_keyframe = true;
                return self.recorder.write(message);
            }
            return Ok(());
        }
        if !state.active {
            return Ok(());
        }
        if state.awaiting_keyframe {
            match message {
                StreamMessage::Frame { keyframe: true, .. } => {
                    state.awaiting_keyframe = false;
                }
                // Config is needed to seed the decoder; skip non-IDR frames.
                _ => return Ok(()),
            }
        }
        self.recorder.write(message)
    }

    /// Drops a stale configuration, e.g. when a new session starts.
    pub fn reset_stream(&self) -> Result<(), RecordingError> {
        let mut state = self.lock()?;
        state.last_config = None;
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.lock().map(|state| state.active).unwrap_or(false)
    }

    pub fn output_path(&self) -> Option<PathBuf> {
        self.lock().ok().and_then(|state| state.path.clone())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, RecordingError> {
        self.state
            .lock()
            .map_err(|_| RecordingError::Failed("recording state lock is poisoned".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    fn config() -> StreamMessage {
        StreamMessage::Config {
            width: 1080,
            height: 2400,
            avcc: vec![1, 0x42],
        }
    }

    fn frame(pts_us: i64) -> StreamMessage {
        StreamMessage::Frame {
            pts_us,
            keyframe: true,
            data: vec![0, 0, 0, 1, 0x65],
        }
    }

    fn p_frame(pts_us: i64) -> StreamMessage {
        StreamMessage::Frame {
            pts_us,
            keyframe: false,
            data: vec![0, 0, 0, 1, 0x41],
        }
    }

    #[derive(Default)]
    struct FakeRecorder {
        starts: Mutex<Vec<(PathBuf, Option<StreamMessage>)>>,
        writes: Mutex<Vec<StreamMessage>>,
        finishes: Mutex<usize>,
        fail_finish: Mutex<bool>,
    }

    impl FakeRecorder {
        fn fail_next_finish(&self) {
            *self.fail_finish.lock().expect("fail lock") = true;
        }
    }

    impl Recorder for FakeRecorder {
        fn start(&self, path: PathBuf, seed: Option<&StreamMessage>) -> Result<(), RecordingError> {
            self.starts
                .lock()
                .expect("starts lock")
                .push((path, seed.cloned()));
            Ok(())
        }

        fn write(&self, message: &StreamMessage) -> Result<(), RecordingError> {
            self.writes
                .lock()
                .expect("writes lock")
                .push(message.clone());
            Ok(())
        }

        fn finish(&self) -> Result<RecordingStats, RecordingError> {
            *self.finishes.lock().expect("finishes lock") += 1;
            if *self.fail_finish.lock().expect("fail lock") {
                *self.fail_finish.lock().expect("fail lock") = false;
                return Err(RecordingError::Invalid(
                    "no frames were recorded".to_owned(),
                ));
            }
            Ok(RecordingStats {
                frames: 1,
                duration_us: 16_000,
                segments: 1,
                paths: vec![PathBuf::from("clip.mp4")],
            })
        }

        fn is_active(&self) -> bool {
            false
        }
    }

    fn service(recorder: Arc<FakeRecorder>) -> RecordingService {
        // `FakeRecorder` lives behind an Arc so tests can inspect it; the
        // service takes ownership of a thin adapter forwarding to it.
        struct Shared(Arc<FakeRecorder>);
        impl Recorder for Shared {
            fn start(
                &self,
                path: PathBuf,
                seed: Option<&StreamMessage>,
            ) -> Result<(), RecordingError> {
                self.0.start(path, seed)
            }
            fn write(&self, message: &StreamMessage) -> Result<(), RecordingError> {
                self.0.write(message)
            }
            fn finish(&self) -> Result<RecordingStats, RecordingError> {
                self.0.finish()
            }
            fn is_active(&self) -> bool {
                self.0.is_active()
            }
        }
        RecordingService::new(Box::new(Shared(recorder)))
    }

    #[test]
    fn seeds_the_recorder_with_the_last_config() {
        let recorder = Arc::new(FakeRecorder::default());
        let service = service(Arc::clone(&recorder));
        service.observe(&config()).expect("observe");

        service
            .start(PathBuf::from("out.mp4"))
            .expect("start recording");

        let starts = recorder.starts.lock().expect("starts lock");
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0].0, PathBuf::from("out.mp4"));
        assert_eq!(starts[0].1, Some(config()));
    }

    #[test]
    fn only_writes_frames_while_active() {
        let recorder = Arc::new(FakeRecorder::default());
        let service = service(Arc::clone(&recorder));
        service.observe(&config()).expect("observe");
        service.observe(&frame(0)).expect("observe");
        assert!(recorder.writes.lock().expect("writes lock").is_empty());

        service.start(PathBuf::from("out.mp4")).expect("start");
        service.observe(&frame(16_000)).expect("observe");
        assert_eq!(recorder.writes.lock().expect("writes lock").len(), 1);

        let info = service.stop().expect("stop").expect("finished");
        assert_eq!(info.stats.frames, 1);
        assert_eq!(info.path, PathBuf::from("out.mp4"));
        assert_eq!(*recorder.finishes.lock().expect("finishes lock"), 1);
        assert!(service.stop().expect("idempotent stop").is_none());
    }

    #[test]
    fn waits_for_a_keyframe_before_writing_samples() {
        let recorder = Arc::new(FakeRecorder::default());
        let service = service(Arc::clone(&recorder));
        service.observe(&config()).expect("observe config");
        service.start(PathBuf::from("out.mp4")).expect("start");

        service.observe(&p_frame(0)).expect("drop P-frame");
        assert!(recorder.writes.lock().expect("writes lock").is_empty());

        service.observe(&frame(16_000)).expect("write IDR");
        assert_eq!(recorder.writes.lock().expect("writes lock").len(), 1);
        service.observe(&p_frame(33_000)).expect("write P-frame");
        assert_eq!(recorder.writes.lock().expect("writes lock").len(), 2);
    }

    #[test]
    fn a_failed_finalize_does_not_block_the_next_recording() {
        let recorder = Arc::new(FakeRecorder::default());
        let service = service(Arc::clone(&recorder));
        service.observe(&config()).expect("observe");
        service.start(PathBuf::from("a.mp4")).expect("start");

        recorder.fail_next_finish();
        assert!(service.stop().is_err());
        assert!(!service.is_active(), "the failed slot must be released");

        service.start(PathBuf::from("b.mp4")).expect("restart");
    }

    #[test]
    fn rejects_a_second_recording() {
        let recorder = Arc::new(FakeRecorder::default());
        let service = service(recorder);
        service.start(PathBuf::from("a.mp4")).expect("first start");
        assert_eq!(
            service.start(PathBuf::from("b.mp4")),
            Err(RecordingError::AlreadyActive)
        );
    }
}
