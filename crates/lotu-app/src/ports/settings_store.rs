//! Port for persisting the application settings (M4).

use lotu_domain::VideoConfig;

/// Persistable application settings. Kept small: only video for this slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SettingsData {
    pub video: VideoConfig,
    /// Whether the USB authorization help has already been shown.
    pub onboarding_seen: bool,
}

/// Failure of the local settings store.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    #[error("settings I/O failed: {0}")]
    Io(String),
    #[error("stored settings are invalid: {0}")]
    Invalid(String),
}

/// Reads and writes settings from a local, durable location.
pub trait SettingsStore: Send + Sync {
    fn load(&self) -> Result<SettingsData, SettingsError>;
    fn save(&self, data: &SettingsData) -> Result<(), SettingsError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_m1_pipeline() {
        assert_eq!(SettingsData::default().video, VideoConfig::default());
    }
}
