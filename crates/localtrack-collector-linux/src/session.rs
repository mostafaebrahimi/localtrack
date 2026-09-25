use serde::{Deserialize, Serialize};

/// Which display server the current session uses (spec §25).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    X11,
    Wayland,
    Unknown,
}

/// Detect the session type from the environment.
pub fn detect_session() -> SessionKind {
    detect_session_from(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("DISPLAY").ok().as_deref(),
    )
}

pub(crate) fn detect_session_from(
    xdg_session_type: Option<&str>,
    wayland_display: Option<&str>,
    display: Option<&str>,
) -> SessionKind {
    match xdg_session_type.map(|v| v.to_ascii_lowercase()).as_deref() {
        Some("wayland") => SessionKind::Wayland,
        Some("x11") => SessionKind::X11,
        _ => {
            if wayland_display.is_some_and(|v| !v.is_empty()) {
                SessionKind::Wayland
            } else if display.is_some_and(|v| !v.is_empty()) {
                SessionKind::X11
            } else {
                SessionKind::Unknown
            }
        }
    }
}

/// The desktop environment / compositor name, used only for diagnostics.
pub fn desktop_environment() -> Option<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_type_wins_over_display_variables() {
        assert_eq!(
            detect_session_from(Some("wayland"), None, Some(":0")),
            SessionKind::Wayland
        );
        assert_eq!(
            detect_session_from(Some("x11"), Some("wayland-0"), None),
            SessionKind::X11
        );
    }

    #[test]
    fn falls_back_to_display_variables() {
        assert_eq!(
            detect_session_from(None, Some("wayland-0"), None),
            SessionKind::Wayland
        );
        assert_eq!(
            detect_session_from(None, None, Some(":0")),
            SessionKind::X11
        );
        assert_eq!(
            detect_session_from(None, Some(""), Some("")),
            SessionKind::Unknown
        );
        assert_eq!(detect_session_from(None, None, None), SessionKind::Unknown);
    }
}
