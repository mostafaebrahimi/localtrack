use std::sync::mpsc::Sender;

use localtrack_core::activity::Observation;
use serde::{Deserialize, Serialize};

/// Channel a collector publishes observations on.
pub type ObservationSender = Sender<Observation>;

/// Health of a collector, surfaced in Settings → Diagnostics (spec §109).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorStatus {
    pub name: String,
    /// False when the collector failed and is being retried (spec §145).
    pub healthy: bool,
    /// False when this platform cannot support the collector at all
    /// (e.g. active-window tracking on some Wayland compositors, spec §25).
    pub available: bool,
    pub last_event_ms: Option<i64>,
    pub error: Option<String>,
    /// Short human explanation shown in the UI when unavailable.
    pub detail: Option<String>,
}

impl CollectorStatus {
    pub fn healthy(name: &str, last_event_ms: Option<i64>) -> Self {
        Self {
            name: name.to_string(),
            healthy: true,
            available: true,
            last_event_ms,
            error: None,
            detail: None,
        }
    }

    pub fn unavailable(name: &str, detail: &str) -> Self {
        Self {
            name: name.to_string(),
            healthy: true,
            available: false,
            last_event_ms: None,
            error: None,
            detail: Some(detail.to_string()),
        }
    }

    pub fn failed(name: &str, error: &str) -> Self {
        Self {
            name: name.to_string(),
            healthy: false,
            available: true,
            last_event_ms: None,
            error: Some(error.to_string()),
            detail: None,
        }
    }
}

/// The common collector interface (spec §8).
///
/// The concrete implementations run on their own OS thread and publish
/// observations through a channel, so a failing collector can never take down
/// the dashboard (spec §145).
pub trait ActivityCollector: Send {
    fn name(&self) -> &'static str;

    /// Start publishing observations. Must return promptly.
    fn start(&mut self, sender: ObservationSender) -> Result<(), CollectorError>;

    /// Stop publishing and release OS resources.
    fn stop(&mut self) -> Result<(), CollectorError>;

    fn status(&self) -> CollectorStatus;
}

#[derive(Debug, thiserror::Error)]
pub enum CollectorError {
    #[error("collector unavailable on this platform: {0}")]
    Unavailable(String),

    #[error("collector failed: {0}")]
    Failed(String),

    #[error("collector already running")]
    AlreadyRunning,
}

/// A running collector plus the flag used to ask it to stop.
pub struct CollectorHandle {
    pub name: &'static str,
    pub thread: Option<std::thread::JoinHandle<()>>,
    pub stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl CollectorHandle {
    pub fn stop(&mut self) {
        self.stop_flag
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}
