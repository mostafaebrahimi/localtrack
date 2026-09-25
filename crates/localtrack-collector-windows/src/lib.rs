//! Windows desktop collector (spec §22).
//!
//! Uses the native Win32 APIs named in the specification:
//! `GetForegroundWindow`, `GetWindowTextW`, `GetWindowThreadProcessId`,
//! `QueryFullProcessImageNameW` and `GetLastInputInfo`, plus input-desktop
//! probing for lock/unlock. Nothing is polled faster than necessary.

pub mod collector;

#[cfg(windows)]
pub mod win32;

pub use collector::WindowsDesktopCollector;

use serde::{Deserialize, Serialize};

/// What this collector can observe on the running system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsCapabilities {
    pub active_window: bool,
    pub window_title: bool,
    pub process_name: bool,
    pub afk: bool,
    pub lock: bool,
    pub detail: Option<String>,
}

impl Default for WindowsCapabilities {
    fn default() -> Self {
        #[cfg(windows)]
        {
            Self {
                active_window: true,
                window_title: true,
                process_name: true,
                afk: true,
                lock: true,
                detail: None,
            }
        }
        #[cfg(not(windows))]
        {
            Self {
                active_window: false,
                window_title: false,
                process_name: false,
                afk: false,
                lock: false,
                detail: Some("The Windows collector only runs on Windows.".into()),
            }
        }
    }
}
