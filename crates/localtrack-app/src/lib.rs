//! Desktop application services.
//!
//! Everything the dashboard can ask for lives here as a plain Rust API, so the
//! Tauri command layer stays a thin, typed wrapper (spec §120) and the logic
//! remains testable without a window.

pub mod diagnostics;
pub mod error;
pub mod exporting;
pub mod logging;
pub mod maintenance;
pub mod managed;
pub mod queries;
pub mod service;
pub mod tracking;
pub mod types;

pub use error::{AppError, Result};
pub use service::AppService;
pub use types::*;
