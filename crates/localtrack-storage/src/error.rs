use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("core error: {0}")]
    Core(#[from] localtrack_core::CoreError),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("migration failed at version {version}: {message}")]
    Migration { version: i64, message: String },

    #[error("database is not a valid LocalTrack database: {0}")]
    InvalidDatabase(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;
