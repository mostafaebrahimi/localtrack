//! Typed settings with defaults (spec §18, §19, §26, §43, §45, §112, §128).
//!
//! Settings are persisted as `key -> value_json` rows so new keys never require
//! a schema migration, but they are always read through this typed struct.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::activity::MergeConfig;
use crate::privacy::UrlPolicy;

/// Where activity may be recorded (spec §18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TrackingScope {
    #[default]
    WorkSessionsOnly,
    Always,
}

impl TrackingScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrackingScope::WorkSessionsOnly => "WORK_SESSIONS_ONLY",
            TrackingScope::Always => "ALWAYS",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "WORK_SESSIONS_ONLY" => Some(TrackingScope::WorkSessionsOnly),
            "ALWAYS" => Some(TrackingScope::Always),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// The interface language.
///
/// `System` follows the desktop's own locale: someone who has already told
/// their machine they read Catalan should not have to say it again here.
/// Persian is written right to left, which the interface mirrors for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    System,
    En,
    Es,
    Ca,
    Fa,
}

/// All application settings with their spec-mandated defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    // General
    pub tracking_scope: TrackingScope,
    pub pause_tracking_during_break: bool,
    pub tracking_paused: bool,
    pub afk_threshold_seconds: i64,
    /// Stop the clock automatically after this many minutes with no input, and
    /// backdate the clock-out to the last real activity. 0 turns it off.
    ///
    /// Without this, a machine left on overnight reports a twelve-hour day.
    pub auto_clock_out_idle_minutes: i64,
    /// Close the running session at local midnight and start a fresh one, so a
    /// day's tracking always begins at zero and each day is its own record.
    pub split_sessions_at_midnight: bool,
    pub launch_at_startup: bool,
    pub close_to_tray: bool,
    pub start_minimized: bool,
    /// A small always-on-top window showing the clock state, the elapsed time
    /// and the current activity. It is also the visible sign that tracking is
    /// running, so it is on by default.
    pub show_mini_timer: bool,
    /// Where the timer bar was last parked, so it comes back where it was left.
    /// `None` on a fresh installation, which means "the default corner".
    pub mini_timer_x: Option<i64>,
    pub mini_timer_y: Option<i64>,
    pub theme: Theme,
    pub language: Language,
    pub onboarding_completed: bool,

    /// Send the workspaces read from window titles with the daily report, so
    /// the organization's own agent can group the day into projects.
    ///
    /// Off unless the organization turns it on through policy: a workspace name
    /// is a project folder, a repository or a task, which says more about the
    /// work than an application name does.
    pub share_workspace_context: bool,

    // Privacy
    pub url_policy: UrlPolicy,
    pub track_incognito: bool,
    pub detailed_interactions: bool,
    pub record_excluded_duration: bool,

    // Data
    /// 0 means "forever" (spec §112 default).
    pub retention_days: i64,
    pub retention_delete_sessions: bool,
    pub last_retention_run_ms: i64,

    // Engine tuning
    pub merge_gap_seconds: i64,
    pub heartbeat_interval_seconds: i64,
    pub heartbeat_tolerance_seconds: i64,
    pub poll_interval_seconds: i64,
    /// How long a new window or page title must persist before it starts a new
    /// segment. Animated titles (spinners, progress counters, unread badges)
    /// would otherwise shatter one activity into hundreds of slivers.
    pub title_stability_seconds: i64,

    // Analysis
    pub context_switch_noise_seconds: i64,
    pub focus_interruption_tolerance_seconds: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            tracking_scope: TrackingScope::WorkSessionsOnly,
            pause_tracking_during_break: true,
            tracking_paused: false,
            afk_threshold_seconds: 180,
            auto_clock_out_idle_minutes: 30,
            split_sessions_at_midnight: true,
            launch_at_startup: true,
            close_to_tray: true,
            start_minimized: false,
            show_mini_timer: true,
            mini_timer_x: None,
            mini_timer_y: None,
            theme: Theme::System,
            language: Language::System,
            onboarding_completed: false,
            share_workspace_context: false,

            url_policy: UrlPolicy::PathWithoutQuery,
            track_incognito: false,
            detailed_interactions: false,
            record_excluded_duration: false,

            retention_days: 0,
            retention_delete_sessions: false,
            last_retention_run_ms: 0,

            merge_gap_seconds: 20,
            heartbeat_interval_seconds: 15,
            heartbeat_tolerance_seconds: 30,
            poll_interval_seconds: 1,
            title_stability_seconds: 5,

            context_switch_noise_seconds: 15,
            focus_interruption_tolerance_seconds: 120,
        }
    }
}

/// Setting keys as stored in the `settings` table.
pub mod keys {
    pub const TRACKING_SCOPE: &str = "tracking_scope";
    pub const PAUSE_TRACKING_DURING_BREAK: &str = "pause_tracking_during_break";
    pub const TRACKING_PAUSED: &str = "tracking_paused";
    pub const AFK_THRESHOLD_SECONDS: &str = "afk_threshold_seconds";
    pub const AUTO_CLOCK_OUT_IDLE_MINUTES: &str = "auto_clock_out_idle_minutes";
    pub const SPLIT_SESSIONS_AT_MIDNIGHT: &str = "split_sessions_at_midnight";
    pub const LAUNCH_AT_STARTUP: &str = "launch_at_startup";
    pub const CLOSE_TO_TRAY: &str = "close_to_tray";
    pub const START_MINIMIZED: &str = "start_minimized";
    pub const SHOW_MINI_TIMER: &str = "show_mini_timer";
    pub const MINI_TIMER_X: &str = "mini_timer_x";
    pub const MINI_TIMER_Y: &str = "mini_timer_y";
    pub const SHARE_WORKSPACE_CONTEXT: &str = "share_workspace_context";
    pub const THEME: &str = "theme";
    pub const LANGUAGE: &str = "language";
    pub const ONBOARDING_COMPLETED: &str = "onboarding_completed";
    pub const URL_POLICY: &str = "url_policy";
    pub const TRACK_INCOGNITO: &str = "track_incognito";
    pub const DETAILED_INTERACTIONS: &str = "detailed_interactions";
    pub const RECORD_EXCLUDED_DURATION: &str = "record_excluded_duration";
    pub const RETENTION_DAYS: &str = "retention_days";
    pub const RETENTION_DELETE_SESSIONS: &str = "retention_delete_sessions";
    pub const LAST_RETENTION_RUN_MS: &str = "last_retention_run_ms";
    pub const MERGE_GAP_SECONDS: &str = "merge_gap_seconds";
    pub const HEARTBEAT_INTERVAL_SECONDS: &str = "heartbeat_interval_seconds";
    pub const HEARTBEAT_TOLERANCE_SECONDS: &str = "heartbeat_tolerance_seconds";
    pub const POLL_INTERVAL_SECONDS: &str = "poll_interval_seconds";
    pub const TITLE_STABILITY_SECONDS: &str = "title_stability_seconds";
    pub const CONTEXT_SWITCH_NOISE_SECONDS: &str = "context_switch_noise_seconds";
    pub const FOCUS_INTERRUPTION_TOLERANCE_SECONDS: &str = "focus_interruption_tolerance_seconds";

    pub const ALL: &[&str] = &[
        TRACKING_SCOPE,
        PAUSE_TRACKING_DURING_BREAK,
        TRACKING_PAUSED,
        AFK_THRESHOLD_SECONDS,
        AUTO_CLOCK_OUT_IDLE_MINUTES,
        SPLIT_SESSIONS_AT_MIDNIGHT,
        LAUNCH_AT_STARTUP,
        CLOSE_TO_TRAY,
        START_MINIMIZED,
        SHOW_MINI_TIMER,
        MINI_TIMER_X,
        MINI_TIMER_Y,
        SHARE_WORKSPACE_CONTEXT,
        THEME,
        LANGUAGE,
        ONBOARDING_COMPLETED,
        URL_POLICY,
        TRACK_INCOGNITO,
        DETAILED_INTERACTIONS,
        RECORD_EXCLUDED_DURATION,
        RETENTION_DAYS,
        RETENTION_DELETE_SESSIONS,
        LAST_RETENTION_RUN_MS,
        MERGE_GAP_SECONDS,
        HEARTBEAT_INTERVAL_SECONDS,
        HEARTBEAT_TOLERANCE_SECONDS,
        POLL_INTERVAL_SECONDS,
        TITLE_STABILITY_SECONDS,
        CONTEXT_SWITCH_NOISE_SECONDS,
        FOCUS_INTERRUPTION_TOLERANCE_SECONDS,
    ];
}

impl Settings {
    /// Flatten into `key -> JSON value` rows.
    pub fn to_map(&self) -> BTreeMap<String, serde_json::Value> {
        use serde_json::json;
        let mut map = BTreeMap::new();
        map.insert(
            keys::TRACKING_SCOPE.into(),
            json!(self.tracking_scope.as_str()),
        );
        map.insert(
            keys::PAUSE_TRACKING_DURING_BREAK.into(),
            json!(self.pause_tracking_during_break),
        );
        map.insert(keys::TRACKING_PAUSED.into(), json!(self.tracking_paused));
        map.insert(
            keys::AFK_THRESHOLD_SECONDS.into(),
            json!(self.afk_threshold_seconds),
        );
        map.insert(
            keys::AUTO_CLOCK_OUT_IDLE_MINUTES.into(),
            json!(self.auto_clock_out_idle_minutes),
        );
        map.insert(
            keys::SPLIT_SESSIONS_AT_MIDNIGHT.into(),
            json!(self.split_sessions_at_midnight),
        );
        map.insert(
            keys::LAUNCH_AT_STARTUP.into(),
            json!(self.launch_at_startup),
        );
        map.insert(keys::CLOSE_TO_TRAY.into(), json!(self.close_to_tray));
        map.insert(keys::START_MINIMIZED.into(), json!(self.start_minimized));
        map.insert(keys::SHOW_MINI_TIMER.into(), json!(self.show_mini_timer));
        map.insert(keys::MINI_TIMER_X.into(), json!(self.mini_timer_x));
        map.insert(keys::MINI_TIMER_Y.into(), json!(self.mini_timer_y));
        map.insert(
            keys::SHARE_WORKSPACE_CONTEXT.into(),
            json!(self.share_workspace_context),
        );
        map.insert(keys::THEME.into(), json!(self.theme));
        map.insert(keys::LANGUAGE.into(), json!(self.language));
        map.insert(
            keys::ONBOARDING_COMPLETED.into(),
            json!(self.onboarding_completed),
        );
        map.insert(keys::URL_POLICY.into(), json!(self.url_policy.as_str()));
        map.insert(keys::TRACK_INCOGNITO.into(), json!(self.track_incognito));
        map.insert(
            keys::DETAILED_INTERACTIONS.into(),
            json!(self.detailed_interactions),
        );
        map.insert(
            keys::RECORD_EXCLUDED_DURATION.into(),
            json!(self.record_excluded_duration),
        );
        map.insert(keys::RETENTION_DAYS.into(), json!(self.retention_days));
        map.insert(
            keys::RETENTION_DELETE_SESSIONS.into(),
            json!(self.retention_delete_sessions),
        );
        map.insert(
            keys::LAST_RETENTION_RUN_MS.into(),
            json!(self.last_retention_run_ms),
        );
        map.insert(
            keys::MERGE_GAP_SECONDS.into(),
            json!(self.merge_gap_seconds),
        );
        map.insert(
            keys::HEARTBEAT_INTERVAL_SECONDS.into(),
            json!(self.heartbeat_interval_seconds),
        );
        map.insert(
            keys::HEARTBEAT_TOLERANCE_SECONDS.into(),
            json!(self.heartbeat_tolerance_seconds),
        );
        map.insert(
            keys::POLL_INTERVAL_SECONDS.into(),
            json!(self.poll_interval_seconds),
        );
        map.insert(
            keys::CONTEXT_SWITCH_NOISE_SECONDS.into(),
            json!(self.context_switch_noise_seconds),
        );
        map.insert(
            keys::FOCUS_INTERRUPTION_TOLERANCE_SECONDS.into(),
            json!(self.focus_interruption_tolerance_seconds),
        );
        map
    }

    /// Read settings from stored rows, falling back to defaults for anything
    /// missing or unparseable. Unknown keys are ignored.
    pub fn from_map(map: &BTreeMap<String, serde_json::Value>) -> Self {
        let mut s = Settings::default();
        let bool_of = |v: &serde_json::Value| v.as_bool();
        let int_of = |v: &serde_json::Value| v.as_i64();
        let str_of = |v: &serde_json::Value| v.as_str().map(|s| s.to_string());

        for (key, value) in map {
            match key.as_str() {
                keys::TRACKING_SCOPE => {
                    if let Some(v) = str_of(value).as_deref().and_then(TrackingScope::parse) {
                        s.tracking_scope = v;
                    }
                }
                keys::PAUSE_TRACKING_DURING_BREAK => {
                    if let Some(v) = bool_of(value) {
                        s.pause_tracking_during_break = v;
                    }
                }
                keys::TRACKING_PAUSED => {
                    if let Some(v) = bool_of(value) {
                        s.tracking_paused = v;
                    }
                }
                keys::AFK_THRESHOLD_SECONDS => {
                    if let Some(v) = int_of(value) {
                        s.afk_threshold_seconds = v.clamp(10, 24 * 3600);
                    }
                }
                keys::AUTO_CLOCK_OUT_IDLE_MINUTES => {
                    if let Some(v) = int_of(value) {
                        s.auto_clock_out_idle_minutes = v.clamp(0, 24 * 60);
                    }
                }
                keys::SPLIT_SESSIONS_AT_MIDNIGHT => {
                    if let Some(v) = bool_of(value) {
                        s.split_sessions_at_midnight = v;
                    }
                }
                keys::LAUNCH_AT_STARTUP => {
                    if let Some(v) = bool_of(value) {
                        s.launch_at_startup = v;
                    }
                }
                keys::CLOSE_TO_TRAY => {
                    if let Some(v) = bool_of(value) {
                        s.close_to_tray = v;
                    }
                }
                keys::START_MINIMIZED => {
                    if let Some(v) = bool_of(value) {
                        s.start_minimized = v;
                    }
                }
                keys::SHOW_MINI_TIMER => {
                    if let Some(v) = bool_of(value) {
                        s.show_mini_timer = v;
                    }
                }
                keys::MINI_TIMER_X => {
                    s.mini_timer_x = value.as_i64();
                }
                keys::MINI_TIMER_Y => {
                    s.mini_timer_y = value.as_i64();
                }
                keys::SHARE_WORKSPACE_CONTEXT => {
                    if let Some(v) = bool_of(value) {
                        s.share_workspace_context = v;
                    }
                }
                keys::THEME => {
                    if let Ok(v) = serde_json::from_value::<Theme>(value.clone()) {
                        s.theme = v;
                    }
                }
                keys::LANGUAGE => {
                    if let Ok(v) = serde_json::from_value::<Language>(value.clone()) {
                        s.language = v;
                    }
                }
                keys::ONBOARDING_COMPLETED => {
                    if let Some(v) = bool_of(value) {
                        s.onboarding_completed = v;
                    }
                }
                keys::URL_POLICY => {
                    if let Some(v) = str_of(value).as_deref().and_then(UrlPolicy::parse) {
                        s.url_policy = v;
                    }
                }
                keys::TRACK_INCOGNITO => {
                    if let Some(v) = bool_of(value) {
                        s.track_incognito = v;
                    }
                }
                keys::DETAILED_INTERACTIONS => {
                    if let Some(v) = bool_of(value) {
                        s.detailed_interactions = v;
                    }
                }
                keys::RECORD_EXCLUDED_DURATION => {
                    if let Some(v) = bool_of(value) {
                        s.record_excluded_duration = v;
                    }
                }
                keys::RETENTION_DAYS => {
                    if let Some(v) = int_of(value) {
                        s.retention_days = v.max(0);
                    }
                }
                keys::RETENTION_DELETE_SESSIONS => {
                    if let Some(v) = bool_of(value) {
                        s.retention_delete_sessions = v;
                    }
                }
                keys::LAST_RETENTION_RUN_MS => {
                    if let Some(v) = int_of(value) {
                        s.last_retention_run_ms = v.max(0);
                    }
                }
                keys::MERGE_GAP_SECONDS => {
                    if let Some(v) = int_of(value) {
                        s.merge_gap_seconds = v.clamp(1, 600);
                    }
                }
                keys::HEARTBEAT_INTERVAL_SECONDS => {
                    if let Some(v) = int_of(value) {
                        s.heartbeat_interval_seconds = v.clamp(5, 300);
                    }
                }
                keys::HEARTBEAT_TOLERANCE_SECONDS => {
                    if let Some(v) = int_of(value) {
                        s.heartbeat_tolerance_seconds = v.clamp(5, 600);
                    }
                }
                keys::POLL_INTERVAL_SECONDS => {
                    if let Some(v) = int_of(value) {
                        s.poll_interval_seconds = v.clamp(1, 60);
                    }
                }
                keys::TITLE_STABILITY_SECONDS => {
                    if let Some(v) = int_of(value) {
                        s.title_stability_seconds = v.clamp(0, 120);
                    }
                }
                keys::CONTEXT_SWITCH_NOISE_SECONDS => {
                    if let Some(v) = int_of(value) {
                        s.context_switch_noise_seconds = v.clamp(0, 3600);
                    }
                }
                keys::FOCUS_INTERRUPTION_TOLERANCE_SECONDS => {
                    if let Some(v) = int_of(value) {
                        s.focus_interruption_tolerance_seconds = v.clamp(0, 3600);
                    }
                }
                _ => {}
            }
        }
        s
    }

    pub fn afk_threshold_ms(&self) -> i64 {
        self.afk_threshold_seconds * 1000
    }

    /// Idle time after which the clock stops on its own, or `None` when off.
    pub fn auto_clock_out_idle_ms(&self) -> Option<i64> {
        if self.auto_clock_out_idle_minutes <= 0 {
            None
        } else {
            Some(self.auto_clock_out_idle_minutes * 60_000)
        }
    }

    pub fn title_stability_ms(&self) -> i64 {
        self.title_stability_seconds * 1000
    }

    pub fn merge_config(&self) -> MergeConfig {
        MergeConfig {
            merge_gap_ms: self.merge_gap_seconds * 1000,
            heartbeat_tolerance_ms: self.heartbeat_tolerance_seconds * 1000,
            ..MergeConfig::default()
        }
    }

    pub fn retention_cutoff_ms(&self, now_ms: i64) -> Option<i64> {
        if self.retention_days <= 0 {
            None
        } else {
            Some(now_ms - self.retention_days * crate::time::DAY_MS)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_spec() {
        let s = Settings::default();
        assert_eq!(s.tracking_scope, TrackingScope::WorkSessionsOnly);
        assert!(s.pause_tracking_during_break);
        assert_eq!(s.afk_threshold_seconds, 180);
        assert_eq!(s.auto_clock_out_idle_minutes, 30);
        assert!(s.split_sessions_at_midnight);
        assert_eq!(s.url_policy, UrlPolicy::PathWithoutQuery);
        assert!(!s.track_incognito);
        assert!(!s.detailed_interactions);
        assert!(!s.record_excluded_duration);
        assert_eq!(s.retention_days, 0);
        assert_eq!(s.merge_gap_seconds, 20);
        assert_eq!(s.title_stability_seconds, 5);
        assert_eq!(s.heartbeat_interval_seconds, 15);
        assert!(s.launch_at_startup);
    }

    #[test]
    fn map_round_trip() {
        let s = Settings {
            url_policy: UrlPolicy::FullUrl,
            retention_days: 90,
            theme: Theme::Dark,
            ..Settings::default()
        };
        let restored = Settings::from_map(&s.to_map());
        assert_eq!(restored, s);
    }

    #[test]
    fn unknown_and_bad_values_fall_back() {
        let mut map = Settings::default().to_map();
        map.insert("nonsense".into(), serde_json::json!(1));
        map.insert(
            keys::AFK_THRESHOLD_SECONDS.into(),
            serde_json::json!("not a number"),
        );
        let s = Settings::from_map(&map);
        assert_eq!(s.afk_threshold_seconds, 180);
        assert_eq!(s.auto_clock_out_idle_minutes, 30);
        assert!(s.split_sessions_at_midnight);
    }

    #[test]
    fn retention_cutoff() {
        let mut s = Settings::default();
        assert_eq!(s.retention_cutoff_ms(1_000_000), None);
        s.retention_days = 30;
        assert_eq!(s.retention_cutoff_ms(30 * crate::time::DAY_MS), Some(0));
    }
}
