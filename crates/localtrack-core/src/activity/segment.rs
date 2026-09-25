use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::interval::Interval;
use crate::limits;

/// Where an observation came from (spec §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ActivitySource {
    Desktop,
    Chrome,
    System,
    Manual,
}

impl ActivitySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActivitySource::Desktop => "DESKTOP",
            ActivitySource::Chrome => "CHROME",
            ActivitySource::System => "SYSTEM",
            ActivitySource::Manual => "MANUAL",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "DESKTOP" => Some(ActivitySource::Desktop),
            "CHROME" => Some(ActivitySource::Chrome),
            "SYSTEM" => Some(ActivitySource::System),
            "MANUAL" => Some(ActivitySource::Manual),
            _ => None,
        }
    }
}

/// What kind of activity a segment represents (spec §12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ActivityKind {
    Window,
    BrowserPage,
    Idle,
    Locked,
    Interaction,
    Manual,
    /// The user was demonstrably at the keyboard or mouse, but no foreground
    /// window could be attributed — an unsupported Wayland compositor, a
    /// full-screen client the window manager hides, or a collector hiccup.
    ///
    /// This is evidence of real input, not an assumption, so it counts as
    /// active time instead of leaving a hole in the day.
    Input,
}

impl ActivityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActivityKind::Window => "WINDOW",
            ActivityKind::BrowserPage => "BROWSER_PAGE",
            ActivityKind::Idle => "IDLE",
            ActivityKind::Locked => "LOCKED",
            ActivityKind::Interaction => "INTERACTION",
            ActivityKind::Manual => "MANUAL",
            ActivityKind::Input => "INPUT",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "WINDOW" => Some(ActivityKind::Window),
            "BROWSER_PAGE" => Some(ActivityKind::BrowserPage),
            "IDLE" => Some(ActivityKind::Idle),
            "LOCKED" => Some(ActivityKind::Locked),
            "INTERACTION" => Some(ActivityKind::Interaction),
            "MANUAL" => Some(ActivityKind::Manual),
            "INPUT" => Some(ActivityKind::Input),
            _ => None,
        }
    }

    /// Idle and Locked never count as active time.
    pub fn is_inactive(&self) -> bool {
        matches!(self, ActivityKind::Idle | ActivityKind::Locked)
    }
}

/// How a segment received its category/project (spec §66).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ClassificationSource {
    Manual,
    Rule,
    Default,
}

impl ClassificationSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClassificationSource::Manual => "MANUAL",
            ClassificationSource::Rule => "RULE",
            ClassificationSource::Default => "DEFAULT",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "MANUAL" => Some(ClassificationSource::Manual),
            "RULE" => Some(ClassificationSource::Rule),
            "DEFAULT" => Some(ClassificationSource::Default),
            _ => None,
        }
    }
}

/// The metadata that decides whether two observations belong to the same
/// segment (spec §29). Everything here is already privacy-sanitized.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SegmentKey {
    pub source: Option<String>,
    pub kind: Option<String>,

    pub app_name: Option<String>,
    pub process_name: Option<String>,
    pub window_title: Option<String>,

    pub browser: Option<String>,
    pub domain: Option<String>,
    pub url: Option<String>,
    pub page_title: Option<String>,

    pub interaction_type: Option<String>,
    pub is_afk: bool,
}

impl SegmentKey {
    pub fn desktop(
        app_name: Option<String>,
        process_name: Option<String>,
        window_title: Option<String>,
    ) -> Self {
        Self {
            source: Some(ActivitySource::Desktop.as_str().to_string()),
            kind: Some(ActivityKind::Window.as_str().to_string()),
            app_name: app_name.map(|v| limits::truncate(&v, limits::APP_NAME_MAX)),
            process_name: process_name.map(|v| limits::truncate(&v, limits::PROCESS_NAME_MAX)),
            window_title: window_title.map(|v| limits::truncate(&v, limits::WINDOW_TITLE_MAX)),
            ..Default::default()
        }
    }

    pub fn browser_page(
        browser: Option<String>,
        domain: Option<String>,
        url: Option<String>,
        page_title: Option<String>,
    ) -> Self {
        Self {
            source: Some(ActivitySource::Chrome.as_str().to_string()),
            kind: Some(ActivityKind::BrowserPage.as_str().to_string()),
            browser,
            domain: domain.map(|v| limits::truncate(&v, limits::DOMAIN_MAX)),
            url: url.map(|v| limits::truncate(&v, limits::URL_MAX)),
            page_title: page_title.map(|v| limits::truncate(&v, limits::PAGE_TITLE_MAX)),
            ..Default::default()
        }
    }

    /// Input activity with no attributable window.
    pub fn input() -> Self {
        Self {
            source: Some(ActivitySource::Desktop.as_str().to_string()),
            kind: Some(ActivityKind::Input.as_str().to_string()),
            ..Default::default()
        }
    }

    pub fn idle() -> Self {
        Self {
            source: Some(ActivitySource::System.as_str().to_string()),
            kind: Some(ActivityKind::Idle.as_str().to_string()),
            is_afk: true,
            ..Default::default()
        }
    }

    pub fn locked() -> Self {
        Self {
            source: Some(ActivitySource::System.as_str().to_string()),
            kind: Some(ActivityKind::Locked.as_str().to_string()),
            is_afk: true,
            ..Default::default()
        }
    }

    /// True when two keys describe the same activity and differ only in the
    /// window or page title.
    ///
    /// Titles animate: terminal spinners, "(3) Inbox", progress counters and
    /// video timestamps all change every second while the user keeps doing the
    /// same thing.
    pub fn differs_only_by_title(&self, other: &SegmentKey) -> bool {
        if self == other {
            return false;
        }
        let same_identity = self.source == other.source
            && self.kind == other.kind
            && self.app_name == other.app_name
            && self.process_name == other.process_name
            && self.browser == other.browser
            && self.domain == other.domain
            && self.url == other.url
            && self.interaction_type == other.interaction_type
            && self.is_afk == other.is_afk;
        same_identity
            && (self.window_title != other.window_title || self.page_title != other.page_title)
    }

    pub fn source_enum(&self) -> ActivitySource {
        self.source
            .as_deref()
            .and_then(ActivitySource::parse)
            .unwrap_or(ActivitySource::Desktop)
    }

    pub fn kind_enum(&self) -> ActivityKind {
        self.kind
            .as_deref()
            .and_then(ActivityKind::parse)
            .unwrap_or(ActivityKind::Window)
    }
}

/// A persisted, normalized activity segment (spec §10, §58).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySegment {
    pub id: String,

    pub source: ActivitySource,
    pub kind: ActivityKind,

    pub started_at_ms: i64,
    pub ended_at_ms: i64,

    pub timezone_offset_min: Option<i32>,

    pub app_name: Option<String>,
    pub process_name: Option<String>,
    pub window_title: Option<String>,

    pub browser: Option<String>,
    pub domain: Option<String>,
    pub url: Option<String>,
    pub page_title: Option<String>,

    pub interaction_type: Option<String>,

    pub category_id: Option<String>,
    pub project_id: Option<String>,

    pub classification_source: Option<ClassificationSource>,

    pub is_afk: bool,

    pub metadata_json: Option<String>,

    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl ActivitySegment {
    /// Build a segment from a key and a time range.
    pub fn from_key(key: &SegmentKey, started_at_ms: i64, ended_at_ms: i64, now_ms: i64) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            source: key.source_enum(),
            kind: key.kind_enum(),
            started_at_ms,
            ended_at_ms,
            timezone_offset_min: Some(crate::time::offset_minutes_at(started_at_ms)),
            app_name: key.app_name.clone(),
            process_name: key.process_name.clone(),
            window_title: key.window_title.clone(),
            browser: key.browser.clone(),
            domain: key.domain.clone(),
            url: key.url.clone(),
            page_title: key.page_title.clone(),
            interaction_type: key.interaction_type.clone(),
            category_id: None,
            project_id: None,
            classification_source: None,
            is_afk: key.is_afk || key.kind_enum().is_inactive(),
            metadata_json: None,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }

    pub fn interval(&self) -> Interval {
        Interval::new(self.started_at_ms, self.ended_at_ms)
    }

    pub fn duration_ms(&self) -> i64 {
        self.interval().duration_ms()
    }

    pub fn key(&self) -> SegmentKey {
        SegmentKey {
            source: Some(self.source.as_str().to_string()),
            kind: Some(self.kind.as_str().to_string()),
            app_name: self.app_name.clone(),
            process_name: self.process_name.clone(),
            window_title: self.window_title.clone(),
            browser: self.browser.clone(),
            domain: self.domain.clone(),
            url: self.url.clone(),
            page_title: self.page_title.clone(),
            interaction_type: self.interaction_type.clone(),
            is_afk: self.is_afk,
        }
    }

    /// Human label used by the timeline and activity feed.
    pub fn label(&self) -> String {
        match self.kind {
            ActivityKind::Idle => "Idle".to_string(),
            ActivityKind::Locked => "Locked".to_string(),
            ActivityKind::Input => "Active (no window detail)".to_string(),
            ActivityKind::BrowserPage => self
                .domain
                .clone()
                .or_else(|| self.page_title.clone())
                .unwrap_or_else(|| "Browser".to_string()),
            _ => self
                .app_name
                .clone()
                .or_else(|| self.process_name.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_round_trips() {
        for s in [
            ActivitySource::Desktop,
            ActivitySource::Chrome,
            ActivitySource::System,
            ActivitySource::Manual,
        ] {
            assert_eq!(ActivitySource::parse(s.as_str()), Some(s));
        }
        for k in [
            ActivityKind::Window,
            ActivityKind::BrowserPage,
            ActivityKind::Idle,
            ActivityKind::Locked,
            ActivityKind::Interaction,
            ActivityKind::Manual,
            ActivityKind::Input,
        ] {
            assert_eq!(ActivityKind::parse(k.as_str()), Some(k));
        }
    }

    #[test]
    fn keys_truncate_oversized_metadata() {
        let long = "x".repeat(5000);
        let key = SegmentKey::desktop(None, None, Some(long));
        assert_eq!(
            key.window_title.unwrap().chars().count(),
            limits::WINDOW_TITLE_MAX
        );
    }

    #[test]
    fn title_only_changes_are_recognised() {
        let a = SegmentKey::desktop(
            Some("Terminal".into()),
            Some("gnome-terminal".into()),
            Some("\u{25d0} building".into()),
        );
        let b = SegmentKey::desktop(
            Some("Terminal".into()),
            Some("gnome-terminal".into()),
            Some("\u{25d1} building".into()),
        );
        assert!(a.differs_only_by_title(&b));
        assert!(!a.differs_only_by_title(&a));

        let other_app = SegmentKey::desktop(
            Some("Code".into()),
            Some("code".into()),
            Some("\u{25d1} building".into()),
        );
        assert!(!a.differs_only_by_title(&other_app));

        let page_a = SegmentKey::browser_page(
            Some("chrome".into()),
            Some("mail.example.com".into()),
            Some("https://mail.example.com/inbox".into()),
            Some("(1) Inbox".into()),
        );
        let page_b = SegmentKey::browser_page(
            Some("chrome".into()),
            Some("mail.example.com".into()),
            Some("https://mail.example.com/inbox".into()),
            Some("(2) Inbox".into()),
        );
        assert!(page_a.differs_only_by_title(&page_b));
    }

    #[test]
    fn idle_segments_are_afk() {
        let seg = ActivitySegment::from_key(&SegmentKey::idle(), 0, 10, 10);
        assert!(seg.is_afk);
        assert_eq!(seg.label(), "Idle");
    }
}
