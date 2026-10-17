use serde::{Serialize, Serializer};

use crate::connectors::ConnectorError;
use crate::llm::provider::LlmError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("Storage error: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("Analytics engine error: {0}")]
    Analytics(#[from] duckdb::Error),
    #[error("{0}")]
    Connector(#[from] ConnectorError),
    #[error("{0}")]
    Llm(#[from] LlmError),
    #[error("File system error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid data: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Keychain error: {0}")]
    Secret(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl AppError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// Runs blocking storage/analytics work off the async runtime so the UI thread never stalls.
pub async fn blocking<T, F>(work: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?
}
