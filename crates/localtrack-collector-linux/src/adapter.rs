use serde::{Deserialize, Serialize};

/// What the current platform can actually observe (spec §25).
///
/// The dashboard shows this verbatim, so an unsupported compositor never
/// silently reports incorrect data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCapabilities {
    pub adapter: String,
    pub active_window: bool,
    pub window_title: bool,
    pub process_name: bool,
    pub afk: bool,
    pub lock: bool,
    pub detail: Option<String>,
}

impl DesktopCapabilities {
    pub fn unsupported(adapter: &str, detail: &str) -> Self {
        Self {
            adapter: adapter.to_string(),
            active_window: false,
            window_title: false,
            process_name: false,
            afk: false,
            lock: false,
            detail: Some(detail.to_string()),
        }
    }
}

/// A single foreground-window observation from a platform adapter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowSnapshot {
    pub app_name: Option<String>,
    pub process_name: Option<String>,
    pub window_title: Option<String>,
    pub pid: Option<u32>,
}

impl WindowSnapshot {
    pub fn is_empty(&self) -> bool {
        self.app_name.is_none() && self.process_name.is_none() && self.window_title.is_none()
    }
}

/// Platform adapter contract. X11 implements all of it; a Wayland compositor
/// adapter may implement only part.
pub trait WindowAdapter: Send {
    fn name(&self) -> &'static str;

    fn capabilities(&self) -> DesktopCapabilities;

    /// The current foreground window, or `None` when there is none.
    fn active_window(&mut self) -> Result<Option<WindowSnapshot>, String>;

    /// Milliseconds since the last user input.
    fn idle_time_ms(&mut self) -> Result<i64, String>;

    /// Whether the session is currently locked, when detectable.
    fn is_locked(&mut self) -> Result<Option<bool>, String>;
}

/// Adapter used when the platform cannot support active-window tracking.
pub struct UnsupportedAdapter {
    pub capabilities: DesktopCapabilities,
}

impl WindowAdapter for UnsupportedAdapter {
    fn name(&self) -> &'static str {
        "unsupported"
    }

    fn capabilities(&self) -> DesktopCapabilities {
        self.capabilities.clone()
    }

    fn active_window(&mut self) -> Result<Option<WindowSnapshot>, String> {
        Ok(None)
    }

    fn idle_time_ms(&mut self) -> Result<i64, String> {
        Err("idle detection unavailable".into())
    }

    fn is_locked(&mut self) -> Result<Option<bool>, String> {
        Ok(None)
    }
}
