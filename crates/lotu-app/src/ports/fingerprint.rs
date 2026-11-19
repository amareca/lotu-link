//! Port for reading this machine's ADB RSA fingerprint (M4 onboarding).

use async_trait::async_trait;

/// Failure to obtain the fingerprint. Messages are safe to show in the UI.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FingerprintError {
    #[error("the ADB key is not available yet; run a device scan first")]
    KeyUnavailable,
    #[error("the ADB public key could not be read")]
    Unreadable,
}

/// Reads the fingerprint that Android shows in its USB authorization dialog.
#[async_trait]
pub trait FingerprintSource: Send + Sync {
    async fn current(&self) -> Result<String, FingerprintError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_stable() {
        assert_eq!(
            FingerprintError::KeyUnavailable.to_string(),
            "the ADB key is not available yet; run a device scan first"
        );
    }
}
