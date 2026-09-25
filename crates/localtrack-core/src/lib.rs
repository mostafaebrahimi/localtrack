//! LocalTrack shared domain core.
//!
//! This crate is deliberately free of I/O: no SQLite, no OS APIs, no network.
//! Everything here is pure domain logic so it can be unit-tested exhaustively.
//!
//! Layer 2 of the architecture (see `docs/architecture.md`):
//! event normalization, segment merging, privacy sanitization, clock-state
//! logic, rule classification, project assignment, aggregation, duration
//! calculations and validation.

pub mod activity;
pub mod aggregation;
pub mod classification;
pub mod context;
pub mod error;
pub mod interval;
pub mod limits;
pub mod managed;
pub mod privacy;
pub mod sessions;
pub mod settings;
pub mod time;

pub use error::{CoreError, Result};

/// Protocol version spoken by the native messaging host.
pub const NATIVE_PROTOCOL_VERSION: u32 = 1;

/// Application version reported in diagnostics.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
