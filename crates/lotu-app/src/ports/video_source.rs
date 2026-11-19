//! Port that opens live video streams for a connected device.

use std::sync::Arc;

use async_trait::async_trait;
use lotu_domain::{Serial, VideoConfig};

use crate::ports::{CatalogError, InputSink, SessionStream};

/// A live video session returned by a [`VideoSource`].
pub struct LiveStream {
    /// Device name reported by the video server; diagnostics only (the device
    /// shown to the UI comes from the catalog).
    pub device_name: String,
    /// Normalized video messages plus the deterministic cleanup of the session.
    pub stream: Box<dyn SessionStream>,
    /// Sink for input events on the session's control socket (M2).
    ///
    /// The delivery layer installs this in `SessionHandle`; replay sessions do
    /// not expose an input sink.
    pub input: Arc<dyn InputSink>,
}

/// Opens live video streams for one device.
///
/// The implementation owns every device-specific detail (ADB, scrcpy, sockets);
/// the application only sees the resulting [`LiveStream`].
#[async_trait]
pub trait VideoSource: Send + Sync {
    async fn open(&self, serial: &Serial, config: &VideoConfig)
        -> Result<LiveStream, SessionError>;
}

/// Failure modes of starting or keeping a live session.
///
/// The Tauri delivery layer maps each variant to a stable frontend error code
/// (`docs/m1-contract.md` 4.7).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    #[error("device is not in the catalog")]
    DeviceNotFound,
    #[error("device is not connected")]
    DeviceNotConnected,
    #[error("adb error: {0}")]
    Adb(#[from] CatalogError),
    #[error("scrcpy server is missing: {0}")]
    VendorMissing(String),
    #[error("session timed out")]
    Timeout,
    #[error("session failed: {0}")]
    Failed(String),
}
