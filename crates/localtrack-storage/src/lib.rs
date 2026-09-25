//! Layer 3 — storage.
//!
//! Owns SQLite connections, transactions, queries, migrations, indexes,
//! retention, backup and database health. Nothing above this layer writes SQL,
//! and no SQL is ever built by concatenating browser-supplied values: every
//! query is parameterized (spec §107).

pub mod db;
pub mod error;
pub mod filters;
pub mod migrations;
pub mod paths;
pub mod repo;

pub use db::{Database, DatabaseHealth};
pub use error::{Result, StorageError};
pub use filters::{ActivityFilter, Page, SortOrder};

/// Current schema version shipped with this build (spec §154).
pub const SCHEMA_VERSION: i64 = migrations::LATEST_VERSION;

/// Wall-clock milliseconds, used where storage itself has to stamp a row.
pub(crate) fn now_ms() -> i64 {
    localtrack_core::time::now_ms()
}

/// Default database file name.
pub const DB_FILE_NAME: &str = "localtrack.db";
