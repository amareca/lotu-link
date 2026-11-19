//! Application layer for LOTU Link.
//!
//! Use cases and ports live here. The crate only depends on the domain (plus
//! `serde`/`thiserror` for DTOs and errors); it must not know about Tauri, ADB or
//! the operating system.

pub mod dto;
pub mod ports;
pub mod services;
