//! Domain entities and value objects for LOTU Link.
//!
//! This crate is deliberately free of Tauri, ADB, scrcpy and OS-specific
//! concerns. It only contains invariants and the vocabulary shared by the
//! application and adapter layers.

mod device;
mod error;
mod session;
mod video_config;

pub use device::{Device, DeviceState, Resolution, Serial};
pub use error::DomainError;
pub use session::SessionState;
pub use video_config::{VideoConfig, MAX_FPS_LIMIT, MAX_SIZE_LIMIT};
