use serde::{Deserialize, Serialize};

/// Normalized observations published by collectors (spec §8).
///
/// Collectors never know anything about reporting, storage or the UI; they only
/// produce these values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Observation {
    WindowChanged(WindowObservation),
    BrowserChanged(BrowserObservation),
    UserIdle(IdleObservation),
    UserActive(IdleObservation),
    SystemLocked(SystemLockObservation),
    SystemUnlocked(SystemLockObservation),
    /// The user is active but the platform could not name the foreground
    /// window (spec §25 Wayland, or a transient failure).
    InputActivity {
        captured_at_ms: i64,
    },
    Interaction(BrowserInteractionObservation),
    /// Emitted periodically so an unchanged foreground stays alive (spec §29).
    Heartbeat {
        captured_at_ms: i64,
        stream: String,
    },
}

impl Observation {
    pub fn captured_at_ms(&self) -> i64 {
        match self {
            Observation::WindowChanged(o) => o.captured_at_ms,
            Observation::BrowserChanged(o) => o.captured_at_ms,
            Observation::UserIdle(o) | Observation::UserActive(o) => o.captured_at_ms,
            Observation::SystemLocked(o) | Observation::SystemUnlocked(o) => o.captured_at_ms,
            Observation::Interaction(o) => o.captured_at_ms,
            Observation::InputActivity { captured_at_ms } => *captured_at_ms,
            Observation::Heartbeat { captured_at_ms, .. } => *captured_at_ms,
        }
    }
}

/// Desktop foreground window observation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowObservation {
    pub captured_at_ms: i64,
    pub app_name: Option<String>,
    pub process_name: Option<String>,
    pub window_title: Option<String>,
    pub pid: Option<u32>,
}

/// Browser page observation coming from the Chrome extension.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserObservation {
    pub captured_at_ms: i64,
    pub event: String,
    pub browser: String,
    pub window_id: Option<i64>,
    pub tab_id: Option<i64>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub incognito: bool,
    pub audible: bool,
    /// False when the Chrome window itself lost focus (spec §41).
    pub focused: bool,
}

/// AFK / idle observation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdleObservation {
    pub captured_at_ms: i64,
    /// When the user last produced input; AFK logically starts at
    /// `last_input_ms + threshold`, not when the poll noticed (spec §26).
    pub last_input_ms: i64,
    pub idle_threshold_ms: i64,
}

impl IdleObservation {
    /// The logical start of the AFK period.
    pub fn afk_started_at_ms(&self) -> i64 {
        self.last_input_ms + self.idle_threshold_ms
    }
}

/// System lock/unlock observation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemLockObservation {
    pub captured_at_ms: i64,
    pub locked: bool,
}

/// Optional, off-by-default detailed browser interaction (spec §48).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInteractionObservation {
    pub captured_at_ms: i64,
    pub interaction: InteractionType,
    /// Element kind only: "button", "a", "form" — never a value.
    pub element: Option<String>,
    /// Accessible label, max 120 chars, never derived from inputs.
    pub label: Option<String>,
    pub url: Option<String>,
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionType {
    Navigation,
    ButtonClick,
    LinkClick,
    FormSubmit,
    ScrollActivity,
}

impl InteractionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            InteractionType::Navigation => "navigation",
            InteractionType::ButtonClick => "button_click",
            InteractionType::LinkClick => "link_click",
            InteractionType::FormSubmit => "form_submit",
            InteractionType::ScrollActivity => "scroll_activity",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "navigation" => Some(InteractionType::Navigation),
            "button_click" | "click" => Some(InteractionType::ButtonClick),
            "link_click" => Some(InteractionType::LinkClick),
            "form_submit" => Some(InteractionType::FormSubmit),
            "scroll_activity" => Some(InteractionType::ScrollActivity),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn afk_starts_at_last_input_plus_threshold() {
        // Spec §26: last input 10:00:00, threshold 3m, noticed 10:03:04
        // → AFK segment starts 10:03:00.
        let obs = IdleObservation {
            captured_at_ms: 184_000,
            last_input_ms: 0,
            idle_threshold_ms: 180_000,
        };
        assert_eq!(obs.afk_started_at_ms(), 180_000);
    }

    #[test]
    fn interaction_types_round_trip() {
        for t in [
            InteractionType::Navigation,
            InteractionType::ButtonClick,
            InteractionType::LinkClick,
            InteractionType::FormSubmit,
            InteractionType::ScrollActivity,
        ] {
            assert_eq!(InteractionType::parse(t.as_str()), Some(t));
        }
    }
}
