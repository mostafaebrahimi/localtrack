//! Managed (employee) mode: enrollment, server policy and the report payloads.
//!
//! This module is pure data and rules. It performs no I/O, so the shape of what
//! leaves the machine can be reviewed — and tested — in one place.
//!
//! Two properties are enforced here rather than left to the transport:
//!
//! * a daily report contains **aggregates only** — no URLs, page titles, window
//!   titles, notes or per-segment detail ever enter the payload;
//! * the policy is the only inbound channel, and it can lock settings but can
//!   never clock the user in or out, delete data, or run anything.

pub mod policy;
pub mod report;
pub mod sync;

pub use policy::{LockState, ManagedCategory, ManagedPolicy, ManagedRule, ReportingConfig};
pub use report::{
    ApplicationTotal, CategoryTotal, DailyReport, Heartbeat, ProjectTotal, ReportFocus,
    ReportSession, ReportTotals, REPORT_SCHEMA,
};
pub use sync::{
    merge_batch, merge_incoming, resolve_open_sessions, BreakSyncItem, LocalRecord, MergeAction,
    MergeOutcome, SessionSyncItem,
};

use serde::{Deserialize, Serialize};

/// How this installation is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppMode {
    /// Everything stays on this machine. The default, and the only mode until
    /// somebody deliberately enrolls the device.
    #[default]
    Personal,
    /// Enrolled with an organization's server: reduced interface, policy-locked
    /// settings, daily aggregate reports.
    Employee,
}

impl AppMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AppMode::Personal => "PERSONAL",
            AppMode::Employee => "EMPLOYEE",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "PERSONAL" => Some(AppMode::Personal),
            "EMPLOYEE" => Some(AppMode::Employee),
            _ => None,
        }
    }

    pub fn is_managed(&self) -> bool {
        matches!(self, AppMode::Employee)
    }
}

/// The device's enrollment with a server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Enrollment {
    pub server_url: String,
    pub device_id: String,
    /// Bearer token issued at enrollment. Never leaves the machine except as an
    /// Authorization header to the enrolled server.
    #[serde(skip_serializing)]
    pub device_token: String,
    pub organization: Option<String>,
    pub employee_ref: Option<String>,
    pub enrolled_at_ms: i64,
    pub last_policy_at_ms: Option<i64>,
    pub last_report_at_ms: Option<i64>,
    pub last_heartbeat_at_ms: Option<i64>,
    pub last_error: Option<String>,
}

impl Enrollment {
    /// The host shown to the user, so "where does my data go" is answerable
    /// without digging.
    pub fn server_host(&self) -> String {
        url::Url::parse(&self.server_url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
            .unwrap_or_else(|| self.server_url.clone())
    }
}

/// Validate a server URL before it is stored.
///
/// Plain HTTP is refused for anything but localhost: a daily report is an
/// employee's working day and must not travel in clear text.
pub fn validate_server_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("Enter the server address.".into());
    }
    let parsed =
        url::Url::parse(trimmed).map_err(|_| "That is not a valid address.".to_string())?;
    match parsed.scheme() {
        "https" => {}
        "http" => {
            let host = parsed.host_str().unwrap_or_default();
            let local = host == "localhost" || host == "127.0.0.1" || host == "::1";
            if !local {
                return Err("Use https:// so reports cannot be read in transit.".into());
            }
        }
        _ => return Err("The address must start with https://".into()),
    }
    if parsed.host_str().is_none() {
        return Err("The address needs a host name.".into());
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_round_trip() {
        for mode in [AppMode::Personal, AppMode::Employee] {
            assert_eq!(AppMode::parse(mode.as_str()), Some(mode));
        }
        assert!(!AppMode::Personal.is_managed());
        assert!(AppMode::Employee.is_managed());
    }

    #[test]
    fn server_urls_must_be_encrypted_off_localhost() {
        assert_eq!(
            validate_server_url("https://track.example.com/"),
            Ok("https://track.example.com".to_string())
        );
        assert!(validate_server_url("http://track.example.com").is_err());
        assert!(validate_server_url("http://localhost:8080").is_ok());
        assert!(validate_server_url("ftp://example.com").is_err());
        assert!(validate_server_url("   ").is_err());
        assert!(validate_server_url("not a url").is_err());
    }

    #[test]
    fn the_token_never_serializes() {
        let enrollment = Enrollment {
            server_url: "https://track.example.com".into(),
            device_id: "device-1".into(),
            device_token: "super-secret-token".into(),
            organization: Some("Acme".into()),
            employee_ref: Some("jdoe".into()),
            enrolled_at_ms: 0,
            last_policy_at_ms: None,
            last_report_at_ms: None,
            last_heartbeat_at_ms: None,
            last_error: None,
        };
        let json = serde_json::to_string(&enrollment).unwrap();
        assert!(!json.contains("super-secret-token"));
        assert_eq!(enrollment.server_host(), "track.example.com");
    }
}
