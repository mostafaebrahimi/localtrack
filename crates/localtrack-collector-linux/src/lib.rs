//! Linux desktop collectors (spec §23–§25).
//!
//! Linux support is built from adapters because there is no universal
//! foreground-window API: X11 is supported fully, and Wayland capability is
//! detected and reported honestly rather than faked.

pub mod adapter;
pub mod collector;
pub mod session;

#[cfg(target_os = "linux")]
pub mod x11;

pub use adapter::{DesktopCapabilities, WindowAdapter, WindowSnapshot};
pub use collector::LinuxDesktopCollector;
pub use session::{detect_session, SessionKind};

#[cfg(target_os = "linux")]
pub(crate) fn now_ms() -> i64 {
    localtrack_core::time::now_ms()
}
