use crate::session::SessionState;

/// Errors raised when a domain invariant is violated.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("serial must not be empty or contain whitespace or control characters")]
    InvalidSerial,
    #[error("resolution width and height must be greater than zero")]
    InvalidResolution,
    #[error("battery percentage must be between 0 and 100")]
    InvalidBattery,
    #[error("device name must not be empty")]
    EmptyName,
    #[error("invalid session transition: {0} -> {1}")]
    InvalidSessionTransition(SessionState, SessionState),
    #[error("max_size must be 8192 or lower (0 keeps the native size)")]
    InvalidMaxSize,
    #[error("max_fps must be 240 or lower")]
    InvalidMaxFps,
    #[error("bit_rate must be greater than zero")]
    InvalidBitRate,
}
