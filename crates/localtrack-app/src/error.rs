use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Storage(#[from] localtrack_storage::StorageError),

    #[error(transparent)]
    Core(#[from] localtrack_core::CoreError),

    #[error(transparent)]
    Export(#[from] localtrack_export::ExportError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, AppError>;

/// Error shape handed to the frontend: actionable, never a stack trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiError {
    pub code: String,
    pub message: String,
}

impl From<&AppError> for UiError {
    fn from(err: &AppError) -> Self {
        let code = match err {
            AppError::Storage(localtrack_storage::StorageError::NotFound(_)) => "NOT_FOUND",
            AppError::Storage(localtrack_storage::StorageError::Conflict(_)) => "CONFLICT",
            AppError::Storage(localtrack_storage::StorageError::Invalid(_)) => "INVALID",
            AppError::Core(localtrack_core::CoreError::InvalidClockTransition(_)) => {
                "INVALID_CLOCK_TRANSITION"
            }
            AppError::Core(_) => "INVALID",
            AppError::Invalid(_) => "INVALID",
            AppError::Export(_) => "EXPORT_FAILED",
            AppError::Io(_) => "IO_ERROR",
            _ => "ERROR",
        };
        UiError {
            code: code.to_string(),
            message: err.to_string(),
        }
    }
}

impl From<AppError> for String {
    fn from(err: AppError) -> Self {
        serde_json::to_string(&UiError::from(&err))
            .unwrap_or_else(|_| "{\"code\":\"ERROR\",\"message\":\"unknown error\"}".into())
    }
}
