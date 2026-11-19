//! Port for writing the live H.264 stream into an MP4 file (M3).
//!
//! The recorder is deliberately synchronous: it appends small samples to a file
//! and is called from the session actor, which must never block on a second
//! runtime. Implementations use interior mutability so a single recorder can be
//! shared between Tauri commands (start/stop) and the frame tap.

use std::path::PathBuf;

use crate::ports::StreamMessage;

/// Outcome of a finished recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RecordingStats {
    pub frames: u64,
    /// Wall-clock span of the recorded samples, in microseconds.
    pub duration_us: u64,
}

/// Failure modes of [`Recorder`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordingError {
    #[error("a recording is already active")]
    AlreadyActive,
    #[error("no recording is active")]
    Inactive,
    #[error("invalid recording request: {0}")]
    Invalid(String),
    #[error("recording failed: {0}")]
    Failed(String),
}

/// Appends normalized stream messages to a video-only MP4 file.
pub trait Recorder: Send + Sync {
    /// Starts a new file. `seed` is the last decoder configuration seen on the
    /// stream, so recording can begin in the middle of a live session.
    fn start(&self, path: PathBuf, seed: Option<&StreamMessage>) -> Result<(), RecordingError>;

    /// Appends one configuration or frame message.
    fn write(&self, message: &StreamMessage) -> Result<(), RecordingError>;

    /// Finalizes the file and returns its statistics.
    fn finish(&self) -> Result<RecordingStats, RecordingError>;

    fn is_active(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_variants_are_stable() {
        assert_eq!(
            RecordingError::AlreadyActive.to_string(),
            "a recording is already active"
        );
        assert_eq!(
            RecordingError::Invalid("bad path".to_owned()).to_string(),
            "invalid recording request: bad path"
        );
    }
}
