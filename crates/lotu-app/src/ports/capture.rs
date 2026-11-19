//! Ports for native-resolution PNG capture (M3).

use async_trait::async_trait;
use lotu_domain::Serial;

/// A PNG image as produced by `adb shell screencap -p`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PngImage {
    pub bytes: Vec<u8>,
    /// Native width read from the PNG IHDR chunk.
    pub width: u32,
    /// Native height read from the PNG IHDR chunk.
    pub height: u32,
}

/// Failure modes of a screenshot capture.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    #[error("screenshot timed out")]
    Timeout,
    #[error("invalid screenshot: {0}")]
    Invalid(String),
    #[error("screenshot failed: {0}")]
    Failed(String),
}

/// Captures the screen of one connected device at its native resolution.
#[async_trait]
pub trait ScreenshotCapture: Send + Sync {
    async fn capture(&self, serial: &Serial) -> Result<PngImage, CaptureError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_image_keeps_native_dimensions() {
        let image = PngImage {
            bytes: vec![0x89, b'P', b'N', b'G'],
            width: 1080,
            height: 2400,
        };
        assert_eq!((image.width, image.height), (1080, 2400));
        assert_eq!(image.bytes.len(), 4);
    }
}
