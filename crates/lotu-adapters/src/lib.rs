//! Infrastructure adapters for LOTU Link.
//!
//! ADB, media normalization (from M1) and the stream replay fixture live here.
//! Nothing in this crate leaks into the domain or application layers.

pub mod adb;
pub mod media;
pub mod platform;
pub mod replay;
pub mod scrcpy;
