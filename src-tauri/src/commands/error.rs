use lotu_adapters::replay::FixtureError;
use lotu_app::ports::device_catalog::CatalogError;
use lotu_app::ports::{CaptureError, InputError, RecordingError, SessionError};
use serde::Serialize;

/// Stable error codes consumed by the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    AdbUnavailable,
    AdbTimeout,
    AdbFailed,
    ReplayUnavailable,
    FixtureInvalid,
    DeviceNotFound,
    DeviceNotConnected,
    VendorMissing,
    SessionTimeout,
    SessionFailed,
    SessionInactive,
    InputInvalid,
    InputFailed,
    CaptureTimeout,
    CaptureInvalid,
    CaptureFailed,
    RecordingAlreadyActive,
    RecordingInactive,
    RecordingFailed,
    UserCancelled,
    IoFailed,
    Internal,
}

/// Error payload returned by every Tauri command.
///
/// `message` is English diagnostics; the UI localizes using `code`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorDto {
    pub code: ErrorCode,
    pub message: String,
}

impl ErrorDto {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl From<CatalogError> for ErrorDto {
    fn from(error: CatalogError) -> Self {
        match error {
            CatalogError::Unavailable(message) => Self::new(ErrorCode::AdbUnavailable, message),
            CatalogError::Timeout => Self::new(
                ErrorCode::AdbTimeout,
                "adb did not respond within the timeout",
            ),
            CatalogError::Failed(message) => Self::new(ErrorCode::AdbFailed, message),
        }
    }
}

/// Maps the live session failures of `docs/m1-contract.md` 4.7.
///
/// ADB failures keep the M0 codes: the cause is the same and the frontend
/// already localizes them.
impl From<SessionError> for ErrorDto {
    fn from(error: SessionError) -> Self {
        match error {
            SessionError::DeviceNotFound => {
                Self::new(ErrorCode::DeviceNotFound, "device is not in the catalog")
            }
            SessionError::DeviceNotConnected => {
                Self::new(ErrorCode::DeviceNotConnected, "device is not connected")
            }
            SessionError::Adb(error) => Self::from(error),
            SessionError::VendorMissing(message) => Self::new(ErrorCode::VendorMissing, message),
            SessionError::Timeout => Self::new(
                ErrorCode::SessionTimeout,
                "the session did not start within the 15 s budget",
            ),
            SessionError::Failed(message) => Self::new(ErrorCode::SessionFailed, message),
        }
    }
}

/// Input validation and transport failures of M2.
impl From<InputError> for ErrorDto {
    fn from(error: InputError) -> Self {
        match error {
            InputError::Invalid(message) => Self::new(ErrorCode::InputInvalid, message),
            InputError::Failed(message) => Self::new(ErrorCode::InputFailed, message),
        }
    }
}

/// Screenshot failures of M3.
impl From<CaptureError> for ErrorDto {
    fn from(error: CaptureError) -> Self {
        match error {
            CaptureError::Timeout => Self::new(ErrorCode::CaptureTimeout, error.to_string()),
            CaptureError::Invalid(_) => Self::new(ErrorCode::CaptureInvalid, error.to_string()),
            CaptureError::Failed(_) => Self::new(ErrorCode::CaptureFailed, error.to_string()),
        }
    }
}

/// Recording failures of M3.
impl From<RecordingError> for ErrorDto {
    fn from(error: RecordingError) -> Self {
        match error {
            RecordingError::AlreadyActive => {
                Self::new(ErrorCode::RecordingAlreadyActive, error.to_string())
            }
            RecordingError::Inactive => Self::new(ErrorCode::RecordingInactive, error.to_string()),
            RecordingError::Invalid(_) | RecordingError::Failed(_) => {
                Self::new(ErrorCode::RecordingFailed, error.to_string())
            }
        }
    }
}

/// Every fixture problem (missing file, bad container, bad metadata) is a
/// development-time configuration error with a single stable code.
impl From<FixtureError> for ErrorDto {
    fn from(error: FixtureError) -> Self {
        Self::new(ErrorCode::FixtureInvalid, error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_fixture_errors_to_fixture_invalid() {
        let error = ErrorDto::from(FixtureError::InvalidMagic);
        assert_eq!(error.code, ErrorCode::FixtureInvalid);
        assert!(error.message.contains("magic"), "{}", error.message);
    }

    #[test]
    fn maps_catalog_errors_to_stable_codes() {
        let error = ErrorDto::from(CatalogError::Unavailable("adb not found".to_owned()));
        assert_eq!(error.code, ErrorCode::AdbUnavailable);

        let error = ErrorDto::from(CatalogError::Timeout);
        assert_eq!(error.code, ErrorCode::AdbTimeout);

        let error = ErrorDto::from(CatalogError::Failed("unexpected output".to_owned()));
        assert_eq!(error.code, ErrorCode::AdbFailed);
    }

    #[test]
    fn serializes_codes_as_snake_case() {
        let cases = [
            (ErrorCode::AdbUnavailable, "adb_unavailable"),
            (ErrorCode::AdbTimeout, "adb_timeout"),
            (ErrorCode::AdbFailed, "adb_failed"),
            (ErrorCode::ReplayUnavailable, "replay_unavailable"),
            (ErrorCode::FixtureInvalid, "fixture_invalid"),
            (ErrorCode::DeviceNotFound, "device_not_found"),
            (ErrorCode::DeviceNotConnected, "device_not_connected"),
            (ErrorCode::VendorMissing, "vendor_missing"),
            (ErrorCode::SessionTimeout, "session_timeout"),
            (ErrorCode::SessionFailed, "session_failed"),
            (ErrorCode::SessionInactive, "session_inactive"),
            (ErrorCode::InputInvalid, "input_invalid"),
            (ErrorCode::InputFailed, "input_failed"),
            (ErrorCode::CaptureTimeout, "capture_timeout"),
            (ErrorCode::CaptureInvalid, "capture_invalid"),
            (ErrorCode::CaptureFailed, "capture_failed"),
            (
                ErrorCode::RecordingAlreadyActive,
                "recording_already_active",
            ),
            (ErrorCode::RecordingInactive, "recording_inactive"),
            (ErrorCode::RecordingFailed, "recording_failed"),
            (ErrorCode::UserCancelled, "user_cancelled"),
            (ErrorCode::IoFailed, "io_failed"),
            (ErrorCode::Internal, "internal"),
        ];

        for (code, expected) in cases {
            let value = serde_json::to_value(ErrorDto::new(code, "diagnostic"))
                .expect("error should serialize");
            assert_eq!(value["code"], expected);
            assert_eq!(value["message"], "diagnostic");
        }
    }

    #[test]
    fn maps_capture_and_recording_errors_to_stable_codes() {
        assert_eq!(
            ErrorDto::from(CaptureError::Timeout).code,
            ErrorCode::CaptureTimeout
        );
        assert_eq!(
            ErrorDto::from(CaptureError::Invalid("bad png".to_owned())).code,
            ErrorCode::CaptureInvalid
        );
        assert_eq!(
            ErrorDto::from(RecordingError::AlreadyActive).code,
            ErrorCode::RecordingAlreadyActive
        );
        assert_eq!(
            ErrorDto::from(RecordingError::Inactive).code,
            ErrorCode::RecordingInactive
        );
        assert_eq!(
            ErrorDto::from(RecordingError::Failed("io".to_owned())).code,
            ErrorCode::RecordingFailed
        );
    }

    #[test]
    fn maps_input_errors_to_stable_codes() {
        let invalid = ErrorDto::from(InputError::Invalid("bad range".to_owned()));
        assert_eq!(invalid.code, ErrorCode::InputInvalid);

        let failed = ErrorDto::from(InputError::Failed("writer closed".to_owned()));
        assert_eq!(failed.code, ErrorCode::InputFailed);
    }

    #[test]
    fn maps_session_errors_to_stable_codes() {
        let cases = [
            (SessionError::DeviceNotFound, ErrorCode::DeviceNotFound),
            (
                SessionError::DeviceNotConnected,
                ErrorCode::DeviceNotConnected,
            ),
            (
                SessionError::VendorMissing("missing jar".to_owned()),
                ErrorCode::VendorMissing,
            ),
            (SessionError::Timeout, ErrorCode::SessionTimeout),
            (
                SessionError::Failed("protocol error".to_owned()),
                ErrorCode::SessionFailed,
            ),
            (
                SessionError::Adb(CatalogError::Unavailable("adb not found".to_owned())),
                ErrorCode::AdbUnavailable,
            ),
            (
                SessionError::Adb(CatalogError::Timeout),
                ErrorCode::AdbTimeout,
            ),
            (
                SessionError::Adb(CatalogError::Failed("unexpected output".to_owned())),
                ErrorCode::AdbFailed,
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(ErrorDto::from(error).code, expected);
        }
    }
}
