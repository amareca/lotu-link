use async_trait::async_trait;
use lotu_domain::Device;

/// Source of Android devices known to the system (ADB in production).
#[async_trait]
pub trait DeviceCatalog: Send + Sync {
    async fn list(&self) -> Result<Vec<Device>, CatalogError>;
}

/// Failure modes of a device catalog, mapped to stable frontend error codes by
/// the Tauri delivery layer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CatalogError {
    #[error("device catalog is unavailable: {0}")]
    Unavailable(String),
    #[error("device catalog timed out")]
    Timeout,
    #[error("device catalog failed: {0}")]
    Failed(String),
}
