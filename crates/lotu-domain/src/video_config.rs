//! Video capture configuration negotiated with the device.

use crate::error::DomainError;

/// Video parameters of a live session.
///
/// Invariants (validated by [`VideoConfig::new`]):
///
/// - `bit_rate > 0` (bits per second).
/// - `max_fps <= 240`.
/// - `max_size <= 8192`; `0` means "native resolution" (no downscaling).
///
/// The fields are public to match the contract, so build values with
/// [`VideoConfig::new`] (or [`VideoConfig::default`]) instead of a struct
/// literal when the values come from user input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VideoConfig {
    /// Longest edge of the encoded frame in pixels; `0` keeps the native size.
    pub max_size: u32,
    /// Frame rate ceiling.
    pub max_fps: u32,
    /// Target bit rate in bits per second.
    pub bit_rate: u32,
}

/// Upper bound of `max_size` accepted by the domain.
pub const MAX_SIZE_LIMIT: u32 = 8192;
/// Upper bound of `max_fps` accepted by the domain.
pub const MAX_FPS_LIMIT: u32 = 240;

impl VideoConfig {
    /// Builds a configuration after validating the invariants.
    pub fn new(max_size: u32, max_fps: u32, bit_rate: u32) -> Result<Self, DomainError> {
        if max_size > MAX_SIZE_LIMIT {
            return Err(DomainError::InvalidMaxSize);
        }
        if max_fps > MAX_FPS_LIMIT {
            return Err(DomainError::InvalidMaxFps);
        }
        if bit_rate == 0 {
            return Err(DomainError::InvalidBitRate);
        }
        Ok(Self {
            max_size,
            max_fps,
            bit_rate,
        })
    }
}

impl Default for VideoConfig {
    /// M1 defaults: native resolution, 60 fps and 8 Mbps (scrcpy defaults).
    fn default() -> Self {
        Self {
            max_size: 0,
            max_fps: 60,
            bit_rate: 8_000_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_the_m1_configuration() {
        let config = VideoConfig::default();
        assert_eq!(config.max_size, 0);
        assert_eq!(config.max_fps, 60);
        assert_eq!(config.bit_rate, 8_000_000);
    }

    #[test]
    fn default_satisfies_the_invariants() {
        assert_eq!(
            VideoConfig::new(0, 60, 8_000_000),
            Ok(VideoConfig::default())
        );
    }

    #[test]
    fn accepts_boundary_values() {
        assert!(VideoConfig::new(0, 0, 1).is_ok(), "native size, 1 bps");
        assert!(
            VideoConfig::new(MAX_SIZE_LIMIT, MAX_FPS_LIMIT, u32::MAX).is_ok(),
            "upper bounds are inclusive"
        );
    }

    #[test]
    fn rejects_max_size_above_the_limit() {
        assert_eq!(
            VideoConfig::new(MAX_SIZE_LIMIT + 1, 60, 8_000_000),
            Err(DomainError::InvalidMaxSize)
        );
    }

    #[test]
    fn rejects_max_fps_above_the_limit() {
        assert_eq!(
            VideoConfig::new(0, MAX_FPS_LIMIT + 1, 8_000_000),
            Err(DomainError::InvalidMaxFps)
        );
    }

    #[test]
    fn rejects_zero_bit_rate() {
        assert_eq!(VideoConfig::new(0, 60, 0), Err(DomainError::InvalidBitRate));
    }

    #[test]
    fn keeps_the_given_values() {
        let config = VideoConfig::new(1280, 30, 4_000_000).expect("valid config");
        assert_eq!(config.max_size, 1280);
        assert_eq!(config.max_fps, 30);
        assert_eq!(config.bit_rate, 4_000_000);
    }
}
