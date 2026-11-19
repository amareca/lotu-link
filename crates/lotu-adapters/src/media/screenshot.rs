//! Native PNG screenshot adapter (M3).

use std::sync::Arc;

use async_trait::async_trait;
use lotu_app::ports::{CaptureError, PngImage, ScreenshotCapture};
use lotu_domain::Serial;

use crate::adb::SystemAdb;

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Captures the device screen through `adb exec-out screencap -p`.
pub struct AdbScreenshotCapture {
    adb: Arc<SystemAdb>,
}

impl AdbScreenshotCapture {
    pub fn new(adb: Arc<SystemAdb>) -> Self {
        Self { adb }
    }
}

#[async_trait]
impl ScreenshotCapture for AdbScreenshotCapture {
    async fn capture(&self, serial: &Serial) -> Result<PngImage, CaptureError> {
        let bytes = self
            .adb
            .screencap(serial)
            .await
            .map_err(|error| match error {
                lotu_app::ports::CatalogError::Timeout => CaptureError::Timeout,
                other => CaptureError::Failed(other.to_string()),
            })?;

        let (width, height) = parse_png_dimensions(&bytes).map_err(CaptureError::Invalid)?;
        Ok(PngImage {
            bytes,
            width,
            height,
        })
    }
}

/// Reads the native dimensions from the IHDR chunk, validating the PNG magic.
pub fn parse_png_dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.len() < 24 {
        return Err("PNG is shorter than its header".to_owned());
    }
    if bytes[..8] != PNG_MAGIC {
        return Err("missing PNG magic".to_owned());
    }
    let length = u32::from_be_bytes(bytes[8..12].try_into().expect("4 bytes"));
    if length != 13 || &bytes[12..16] != b"IHDR" {
        return Err("first PNG chunk is not a 13-byte IHDR".to_owned());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("4 bytes"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("4 bytes"));
    if width == 0 || height == 0 {
        return Err("PNG has zero dimensions".to_owned());
    }
    Ok((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = PNG_MAGIC.to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 5]); // bit depth, colour, compression, filter, interlace
        bytes.extend_from_slice(&[0u8; 4]); // CRC (not validated)
        bytes.extend_from_slice(b"trailing data must be ignored");
        bytes
    }

    #[test]
    fn reads_native_dimensions_from_ihdr() {
        assert_eq!(parse_png_dimensions(&png(1080, 2400)), Ok((1080, 2400)));
    }

    #[test]
    fn rejects_truncated_and_invalid_pngs() {
        assert!(parse_png_dimensions(&[0u8; 8]).is_err());
        let mut wrong_magic = png(10, 10);
        wrong_magic[1] = b'X';
        assert!(parse_png_dimensions(&wrong_magic).is_err());

        let mut short_ihdr = png(10, 10);
        short_ihdr[8] = 12;
        assert!(parse_png_dimensions(&short_ihdr).is_err());

        let mut zero = png(0, 10);
        assert!(parse_png_dimensions(&zero).is_err());
        zero = png(10, 0);
        assert!(parse_png_dimensions(&zero).is_err());
    }
}
