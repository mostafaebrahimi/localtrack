//! Local export to XLSX and CSV (spec §88–§95).
//!
//! Everything happens on the machine: no upload, no external service, no
//! network access of any kind.

pub mod csv_export;
pub mod model;
pub mod xlsx;

pub use model::{ExportData, ExportOptions, ExportUrlPrivacy, SessionExportRow};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("xlsx error: {0}")]
    Xlsx(#[from] rust_xlsxwriter::XlsxError),

    #[error("csv error: {0}")]
    Csv(#[from] csv::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ExportError>;
