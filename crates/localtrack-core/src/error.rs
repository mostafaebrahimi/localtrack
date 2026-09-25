use thiserror::Error;

/// Errors produced by the domain core.
///
/// The core never panics for expected runtime failures; every fallible
/// operation returns [`Result`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    #[error("invalid clock transition: {0}")]
    InvalidClockTransition(String),

    #[error("invalid time range: start {start_ms} is not before end {end_ms}")]
    InvalidTimeRange { start_ms: i64, end_ms: i64 },

    #[error("validation failed: {0}")]
    Validation(String),

    #[error("invalid rule pattern: {0}")]
    InvalidPattern(String),

    #[error("unsupported protocol version: {0}")]
    UnsupportedProtocolVersion(u32),

    #[error("value too long: field {field} has {len} characters, maximum is {max}")]
    TooLong {
        field: &'static str,
        len: usize,
        max: usize,
    },
}

pub type Result<T> = std::result::Result<T, CoreError>;
