use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::classification::{RuleField, RuleOperator};
use crate::settings::keys;

/// The policy document a server may push to an enrolled device.
///
/// This is the **only** inbound channel. It can lock settings and pin their
/// values, and it can supply shared categories and classification rules. It
/// deliberately cannot clock the user in or out, pause tracking, delete data,
/// request raw activity, or carry anything executable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedPolicy {
    /// Monotonic revision; the agent ignores anything not newer than what it has.
    pub revision: i64,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub lock: LockState,
    /// Settings the operator may not change while the policy is locked.
    #[serde(default)]
    pub locked_settings: Vec<String>,
    /// Values the server pins for the settings it locks.
    #[serde(default)]
    pub settings: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub reporting: ReportingConfig,
    #[serde(default)]
    pub categories: Vec<ManagedCategory>,
    #[serde(default)]
    pub rules: Vec<ManagedRule>,
    /// Shown to the employee so the arrangement is never a secret.
    #[serde(default)]
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LockState {
    /// The employee can still change their own settings.
    #[default]
    Unlocked,
    /// Settings named in `locked_settings` are read-only on the device.
    Locked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportingConfig {
    /// Local wall-clock time to send the daily summary, `HH:MM`.
    pub daily_report_local_time: String,
    /// How often to tell the server the agent is alive.
    pub heartbeat_minutes: i64,
    /// How often to re-read the policy.
    pub policy_poll_minutes: i64,
}

impl Default for ReportingConfig {
    fn default() -> Self {
        Self {
            daily_report_local_time: "23:30".to_string(),
            heartbeat_minutes: 5,
            policy_poll_minutes: 30,
        }
    }
}

impl ReportingConfig {
    /// Minutes past local midnight at which the daily report is due.
    pub fn daily_report_minute_of_day(&self) -> i64 {
        parse_hhmm(&self.daily_report_local_time).unwrap_or(23 * 60 + 30)
    }

    pub fn heartbeat_interval_ms(&self) -> i64 {
        self.heartbeat_minutes.clamp(1, 24 * 60) * 60_000
    }

    pub fn policy_poll_interval_ms(&self) -> i64 {
        self.policy_poll_minutes.clamp(5, 24 * 60) * 60_000
    }
}

fn parse_hhmm(value: &str) -> Option<i64> {
    let (hours, minutes) = value.split_once(':')?;
    let hours: i64 = hours.trim().parse().ok()?;
    let minutes: i64 = minutes.trim().parse().ok()?;
    if !(0..24).contains(&hours) || !(0..60).contains(&minutes) {
        return None;
    }
    Some(hours * 60 + minutes)
}

/// A category defined centrally so every employee reports into the same buckets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedCategory {
    /// Stable server-side identifier; the local row is matched on this.
    pub key: String,
    pub name: String,
}

/// A classification rule defined centrally.
///
/// This is what makes a new application appear already categorized: the server
/// adds a rule, every device picks it up on the next poll.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRule {
    pub key: String,
    pub name: String,
    pub target_field: RuleField,
    pub operator: RuleOperator,
    pub pattern: String,
    #[serde(default)]
    pub category_key: Option<String>,
    #[serde(default)]
    pub priority: i64,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Settings a policy is allowed to lock.
///
/// Everything that decides what is collected is lockable. Deliberately absent:
/// the theme and anything that would let a server hide the fact that tracking
/// is running.
pub const LOCKABLE_SETTINGS: &[&str] = &[
    keys::TRACKING_SCOPE,
    keys::PAUSE_TRACKING_DURING_BREAK,
    keys::TRACKING_PAUSED,
    keys::AFK_THRESHOLD_SECONDS,
    keys::LAUNCH_AT_STARTUP,
    keys::CLOSE_TO_TRAY,
    keys::START_MINIMIZED,
    keys::SHOW_MINI_TIMER,
    keys::URL_POLICY,
    keys::TRACK_INCOGNITO,
    keys::DETAILED_INTERACTIONS,
    keys::RECORD_EXCLUDED_DURATION,
    keys::RETENTION_DAYS,
    keys::RETENTION_DELETE_SESSIONS,
    keys::SHARE_WORKSPACE_CONTEXT,
];

impl ManagedPolicy {
    /// Settings this policy actually locks, ignoring anything not lockable.
    pub fn effective_locked_settings(&self) -> Vec<String> {
        if self.lock != LockState::Locked {
            return Vec::new();
        }
        self.locked_settings
            .iter()
            .filter(|key| LOCKABLE_SETTINGS.contains(&key.as_str()))
            .cloned()
            .collect()
    }

    pub fn is_locked(&self, key: &str) -> bool {
        self.lock == LockState::Locked
            && LOCKABLE_SETTINGS.contains(&key)
            && self.locked_settings.iter().any(|locked| locked == key)
    }

    /// Values the agent should apply, restricted to settings the policy locks.
    ///
    /// A server cannot quietly change a setting it has not also locked, which
    /// keeps "what can the server do to my machine" answerable from the
    /// lock list alone.
    pub fn pinned_settings(&self) -> BTreeMap<String, serde_json::Value> {
        self.settings
            .iter()
            .filter(|(key, _)| self.is_locked(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    /// Reject a policy that is not newer than the one already applied.
    pub fn supersedes(&self, current_revision: Option<i64>) -> bool {
        match current_revision {
            Some(current) => self.revision > current,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> ManagedPolicy {
        ManagedPolicy {
            revision: 3,
            organization: Some("Acme".into()),
            lock: LockState::Locked,
            locked_settings: vec![
                keys::TRACKING_SCOPE.to_string(),
                keys::URL_POLICY.to_string(),
                // Not lockable: must be ignored rather than honoured.
                "theme".to_string(),
                "nonsense_key".to_string(),
            ],
            settings: BTreeMap::from([
                (
                    keys::TRACKING_SCOPE.to_string(),
                    serde_json::json!("ALWAYS"),
                ),
                (
                    keys::URL_POLICY.to_string(),
                    serde_json::json!("DOMAIN_ONLY"),
                ),
                // Pinning a setting it did not lock must not take effect.
                (
                    keys::AFK_THRESHOLD_SECONDS.to_string(),
                    serde_json::json!(30),
                ),
                ("theme".to_string(), serde_json::json!("dark")),
            ]),
            reporting: ReportingConfig::default(),
            categories: vec![],
            rules: vec![],
            notice: None,
        }
    }

    #[test]
    fn only_lockable_settings_can_be_locked() {
        let policy = policy();
        let locked = policy.effective_locked_settings();
        assert_eq!(locked, vec![keys::TRACKING_SCOPE, keys::URL_POLICY]);
        assert!(policy.is_locked(keys::TRACKING_SCOPE));
        assert!(
            !policy.is_locked("theme"),
            "appearance stays with the employee"
        );
        assert!(!policy.is_locked("nonsense_key"));
    }

    #[test]
    fn a_policy_can_only_pin_what_it_locks() {
        let pinned = policy().pinned_settings();
        assert_eq!(pinned.len(), 2);
        assert_eq!(pinned[keys::TRACKING_SCOPE], serde_json::json!("ALWAYS"));
        assert!(!pinned.contains_key(keys::AFK_THRESHOLD_SECONDS));
        assert!(!pinned.contains_key("theme"));
    }

    #[test]
    fn an_unlocked_policy_locks_nothing() {
        let mut policy = policy();
        policy.lock = LockState::Unlocked;
        assert!(policy.effective_locked_settings().is_empty());
        assert!(policy.pinned_settings().is_empty());
        assert!(!policy.is_locked(keys::TRACKING_SCOPE));
    }

    #[test]
    fn older_revisions_are_refused() {
        let policy = policy();
        assert!(policy.supersedes(None));
        assert!(policy.supersedes(Some(2)));
        assert!(!policy.supersedes(Some(3)));
        assert!(!policy.supersedes(Some(9)));
    }

    #[test]
    fn reporting_schedule_parsing() {
        let config = ReportingConfig {
            daily_report_local_time: "23:30".into(),
            heartbeat_minutes: 5,
            policy_poll_minutes: 30,
        };
        assert_eq!(config.daily_report_minute_of_day(), 23 * 60 + 30);
        assert_eq!(config.heartbeat_interval_ms(), 300_000);

        let broken = ReportingConfig {
            daily_report_local_time: "not a time".into(),
            heartbeat_minutes: 0,
            policy_poll_minutes: 1,
        };
        assert_eq!(broken.daily_report_minute_of_day(), 23 * 60 + 30);
        assert_eq!(
            broken.heartbeat_interval_ms(),
            60_000,
            "clamped, never zero"
        );
        assert_eq!(broken.policy_poll_interval_ms(), 300_000);
    }
}
