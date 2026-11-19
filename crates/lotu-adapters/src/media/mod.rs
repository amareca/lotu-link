//! Media normalization shared by the stream adapters.
//!
//! H.264 Annex B → AVCC conversion, IDR detection and decoder configuration
//! records live here. The scrcpy parser feeds the live pipeline with these
//! helpers; the replay fixture tool uses the same code.

pub mod h264;
