//! Media normalization and output shared by the stream adapters.
//!
//! H.264 Annex B → AVCC conversion, IDR detection and decoder configuration
//! records live here. The scrcpy parser feeds the live pipeline with these
//! helpers; the replay fixture tool uses the same code. M3 adds the PNG
//! screenshot and MP4 recording adapters.

pub mod h264;
pub mod mp4;
pub mod recorder;
pub mod screenshot;

pub use mp4::Mp4Writer;
pub use recorder::Mp4Recorder;
pub use screenshot::{parse_png_dimensions, AdbScreenshotCapture};
