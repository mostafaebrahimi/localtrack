//! Collector supervision (spec §7, §145).

use std::sync::mpsc::{channel, Receiver, Sender};

use localtrack_collector_common::collector::{ActivityCollector, CollectorStatus};
use localtrack_core::activity::Observation;

/// Build the desktop collector for this platform.
pub fn build_desktop_collector(
    poll_interval_ms: u64,
    afk_threshold_ms: i64,
) -> Option<Box<dyn ActivityCollector>> {
    #[cfg(target_os = "linux")]
    {
        return Some(Box::new(
            localtrack_collector_linux::LinuxDesktopCollector::new(
                poll_interval_ms,
                afk_threshold_ms,
            ),
        ));
    }

    #[cfg(windows)]
    {
        return Some(Box::new(
            localtrack_collector_windows::WindowsDesktopCollector::new(
                poll_interval_ms,
                afk_threshold_ms,
            ),
        ));
    }

    #[allow(unreachable_code)]
    {
        let _ = (poll_interval_ms, afk_threshold_ms);
        None
    }
}

/// Whether this platform can report the foreground window at all (spec §25).
pub fn desktop_capabilities() -> DesktopSupport {
    #[cfg(target_os = "linux")]
    {
        let capabilities = localtrack_collector_linux::collector::detect_capabilities();
        return DesktopSupport {
            adapter: capabilities.adapter,
            active_window: capabilities.active_window,
            window_title: capabilities.window_title,
            afk: capabilities.afk,
            lock: capabilities.lock,
            detail: capabilities.detail,
        };
    }

    #[cfg(windows)]
    {
        let capabilities = localtrack_collector_windows::WindowsCapabilities::default();
        return DesktopSupport {
            adapter: "windows".into(),
            active_window: capabilities.active_window,
            window_title: capabilities.window_title,
            afk: capabilities.afk,
            lock: capabilities.lock,
            detail: capabilities.detail,
        };
    }

    #[allow(unreachable_code)]
    DesktopSupport {
        adapter: "unsupported".into(),
        active_window: false,
        window_title: false,
        afk: false,
        lock: false,
        detail: Some("Desktop tracking is not supported on this platform.".into()),
    }
}

/// Platform capability summary shown in Diagnostics and the setup flow.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSupport {
    pub adapter: String,
    pub active_window: bool,
    pub window_title: bool,
    pub afk: bool,
    pub lock: bool,
    pub detail: Option<String>,
}

/// The observation channel shared by collectors and the ingest worker.
pub struct ObservationBus {
    pub sender: Sender<Observation>,
    pub receiver: Option<Receiver<Observation>>,
}

impl Default for ObservationBus {
    fn default() -> Self {
        Self::new()
    }
}

impl ObservationBus {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        Self {
            sender,
            receiver: Some(receiver),
        }
    }
}

/// Status of the Chrome connection, derived from how recently the native host
/// wrote browser activity (spec §109).
pub fn chrome_status(last_browser_event_ms: Option<i64>, now_ms: i64) -> CollectorStatus {
    match last_browser_event_ms {
        Some(last) if now_ms - last < 120_000 => CollectorStatus::healthy("chrome", Some(last)),
        Some(last) => CollectorStatus {
            name: "chrome".into(),
            healthy: false,
            available: true,
            last_event_ms: Some(last),
            error: None,
            detail: Some("No browser activity received recently.".into()),
        },
        None => CollectorStatus {
            name: "chrome".into(),
            healthy: false,
            available: true,
            last_event_ms: None,
            error: None,
            detail: Some("The Chrome extension has not connected yet.".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_is_healthy_only_when_recently_active() {
        assert!(chrome_status(Some(1_000), 30_000).healthy);
        assert!(!chrome_status(Some(1_000), 500_000).healthy);
        assert!(!chrome_status(None, 500_000).healthy);
    }
}
