use serde::{Deserialize, Serialize};

use crate::aggregation::{ReportSet, Summary};
use crate::sessions::WorkSession;

/// The daily summary sent to the server.
///
/// Aggregates only. There is no field here for a URL, a page title, a window
/// title, a note or an individual segment — the type itself is the guarantee,
/// so a future change that tried to include them would not compile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReport {
    pub schema: u32,
    pub device_id: String,
    /// Local calendar date, `YYYY-MM-DD`.
    pub date: String,
    pub timezone_offset_minutes: i32,
    pub generated_at_ms: i64,
    pub app_version: String,
    pub totals: ReportTotals,
    pub focus: ReportFocus,
    pub sessions: Vec<ReportSession>,
    pub applications: Vec<ApplicationTotal>,
    pub categories: Vec<CategoryTotal>,
    pub projects: Vec<ProjectTotal>,
    /// What the day was spent working on, read from window titles.
    ///
    /// Empty unless the organization's policy turns workspace sharing on. It is
    /// what lets an agent on the server group a day into projects and send back
    /// categories and rules; see `docs/managed-mode.md`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<WorkspaceTotal>,
}

/// Schema 2 added the optional `workspaces` list. A server written for
/// schema 1 can read a schema 2 report by ignoring the field.
pub const REPORT_SCHEMA: u32 = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportTotals {
    pub clocked_ms: i64,
    pub work_ms: i64,
    pub active_ms: i64,
    pub idle_ms: i64,
    pub break_ms: i64,
    pub untracked_ms: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportFocus {
    pub context_switches: i64,
    pub average_focus_ms: i64,
    pub longest_focus_ms: i64,
}

/// Clock times only: a session's note stays on the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSession {
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub break_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationTotal {
    pub name: String,
    pub ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryTotal {
    /// The server's own key when the category came from a policy.
    pub key: Option<String>,
    pub name: String,
    pub ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTotal {
    pub name: String,
    pub ms: i64,
}

/// A project, directory, repository or task read out of a window title.
///
/// The name and the application it was seen in, and nothing else: no titles, no
/// addresses, no file names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTotal {
    pub name: String,
    /// Where it was read from: `EDITOR`, `TERMINAL`, `BROWSER`, `CHAT`,
    /// `DOCUMENT`. Two workspaces with the same name and different kinds are
    /// different things.
    pub kind: String,
    /// The application it was seen in, when the report is unambiguous about it.
    pub app: Option<String>,
    pub ms: i64,
    pub visits: i64,
}

impl DailyReport {
    /// Build a report from already-computed aggregates.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        device_id: &str,
        date: &str,
        timezone_offset_minutes: i32,
        generated_at_ms: i64,
        app_version: &str,
        summary: &Summary,
        sessions: &[(WorkSession, i64)],
        applications: &ReportSet,
        categories: &ReportSet,
        projects: &ReportSet,
        category_keys: &std::collections::BTreeMap<String, String>,
        // `None` unless the organization's policy asked for workspaces.
        workspaces: Option<&ReportSet>,
    ) -> Self {
        Self {
            schema: REPORT_SCHEMA,
            device_id: device_id.to_string(),
            date: date.to_string(),
            timezone_offset_minutes,
            generated_at_ms,
            app_version: app_version.to_string(),
            totals: ReportTotals {
                clocked_ms: summary.clocked_ms,
                work_ms: summary.work_ms,
                active_ms: summary.active_ms,
                idle_ms: summary.idle_ms,
                break_ms: summary.break_ms,
                untracked_ms: summary.untracked_ms,
            },
            focus: ReportFocus {
                context_switches: summary.context_switches,
                average_focus_ms: summary.average_focus_ms,
                longest_focus_ms: summary.longest_focus_ms,
            },
            sessions: sessions
                .iter()
                .map(|(session, break_ms)| ReportSession {
                    started_at_ms: session.started_at_ms,
                    ended_at_ms: session.ended_at_ms,
                    break_ms: *break_ms,
                })
                .collect(),
            applications: applications
                .rows
                .iter()
                .map(|row| ApplicationTotal {
                    name: row.label.clone(),
                    ms: row.duration_ms,
                })
                .collect(),
            categories: categories
                .rows
                .iter()
                .map(|row| CategoryTotal {
                    key: category_keys.get(&row.key).cloned(),
                    name: row.label.clone(),
                    ms: row.duration_ms,
                })
                .collect(),
            projects: projects
                .rows
                .iter()
                .map(|row| ProjectTotal {
                    name: row.label.clone(),
                    ms: row.duration_ms,
                })
                .collect(),
            workspaces: workspaces
                .map(|set| {
                    set.rows
                        .iter()
                        // Time whose title said nothing is the agent's business,
                        // not the server's. The key carries the application
                        // after the prefix, so this has to match the prefix:
                        // an exact comparison would send the row and name the
                        // application it was spent in.
                        .filter(|row| !row.key.starts_with("UNKNOWN:"))
                        .map(|row| WorkspaceTotal {
                            name: row.label.clone(),
                            kind: row
                                .key
                                .split_once(':')
                                .map(|(kind, _)| kind.to_string())
                                .unwrap_or_else(|| "UNKNOWN".into()),
                            app: row.secondary.clone(),
                            ms: row.duration_ms,
                            visits: row.visit_count,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// A one-line description for the "what was sent" list in the interface.
    pub fn describe(&self) -> String {
        let workspaces = if self.workspaces.is_empty() {
            String::new()
        } else {
            format!(", {} workspaces", self.workspaces.len())
        };
        format!(
            "{}: {} tracked, {} applications, {} categories{workspaces}",
            self.date,
            crate::time::format_duration_hm(self.totals.active_ms),
            self.applications.len(),
            self.categories.len()
        )
    }
}

/// The periodic "this agent is alive" ping. Read-only from the server's side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Heartbeat {
    pub schema: u32,
    pub device_id: String,
    pub at_ms: i64,
    pub app_version: String,
    pub clock_state: String,
    pub today_active_ms: i64,
    pub tracking_healthy: bool,
    pub policy_revision: Option<i64>,
    pub pending_reports: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregation::{ReportDimension, ReportRow};

    fn row(key: &str, label: &str, ms: i64) -> ReportRow {
        ReportRow {
            key: key.into(),
            label: label.into(),
            secondary: Some("should never be sent".into()),
            duration_ms: ms,
            percentage: 50.0,
            visit_count: 3,
        }
    }

    fn set(rows: Vec<ReportRow>) -> ReportSet {
        ReportSet {
            dimension: ReportDimension::Application,
            total_ms: rows.iter().map(|r| r.duration_ms).sum(),
            rows,
        }
    }

    #[test]
    fn a_report_carries_aggregates_and_nothing_identifying() {
        let mut session = WorkSession {
            id: "s1".into(),
            started_at_ms: 1_000,
            ended_at_ms: Some(9_000),
            start_timezone_offset_min: 210,
            end_timezone_offset_min: Some(210),
            note: Some("standup then invoice for Acme".into()),
            created_manually: false,
            edited_manually: false,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        session.note = Some("private note".into());

        let summary = Summary {
            active_ms: 3_600_000,
            clocked_ms: 7_200_000,
            ..Default::default()
        };
        let report = DailyReport::build(
            "device-1",
            "2026-08-20",
            210,
            123,
            "1.0.0",
            &summary,
            &[(session, 600_000)],
            &set(vec![row("Code", "Visual Studio Code", 1_800_000)]),
            &set(vec![row("dev", "Development", 1_800_000)]),
            &set(vec![row("hub", "Hub", 1_800_000)]),
            &std::collections::BTreeMap::from([("dev".to_string(), "server-dev".to_string())]),
            None,
        );

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("Visual Studio Code"));
        assert!(json.contains("\"activeMs\":3600000"));
        assert_eq!(report.categories[0].key.as_deref(), Some("server-dev"));

        // Nothing about what was on screen, and no session notes.
        assert!(!json.contains("private note"));
        assert!(!json.contains("should never be sent"));
        assert!(!json.contains("http"));
        assert!(!json.contains("title"));
        assert!(!json.contains("url"));
        assert!(!json.contains("domain"));
    }

    fn workspace_set() -> ReportSet {
        let mut rows = vec![
            row("EDITOR:helpdesk-v2", "helpdesk-v2", 1_800_000),
            row(
                "TERMINAL:Kubernetes config review",
                "Kubernetes config review",
                900_000,
            ),
            row("UNKNOWN:Telegram Desktop", "Not identified", 300_000),
        ];
        rows[0].secondary = Some("Code".into());
        rows[2].secondary = Some("Telegram Desktop".into());
        set(rows)
    }

    #[test]
    fn workspaces_stay_home_unless_the_policy_asks_for_them() {
        let report = DailyReport::build(
            "device-1",
            "2026-08-20",
            0,
            0,
            "1.0.0",
            &Summary::default(),
            &[],
            &set(vec![row("Code", "Code", 1)]),
            &set(vec![]),
            &set(vec![]),
            &Default::default(),
            None,
        );
        assert!(report.workspaces.is_empty());
        // An empty list is not even a field on the wire.
        assert!(!serde_json::to_string(&report)
            .unwrap()
            .contains("workspaces"));
    }

    #[test]
    fn shared_workspaces_carry_names_and_totals_only() {
        let report = DailyReport::build(
            "device-1",
            "2026-08-20",
            0,
            0,
            "1.0.0",
            &Summary::default(),
            &[],
            &set(vec![row("Code", "Code", 1)]),
            &set(vec![]),
            &set(vec![]),
            &Default::default(),
            Some(&workspace_set()),
        );

        // Time the titles could not place is the agent's business, not the
        // server's, so it is left out rather than sent as a mystery row.
        assert_eq!(report.workspaces.len(), 2);
        assert_eq!(report.workspaces[0].name, "helpdesk-v2");
        assert_eq!(report.workspaces[0].kind, "EDITOR");
        assert_eq!(report.workspaces[0].app.as_deref(), Some("Code"));
        assert_eq!(report.workspaces[1].kind, "TERMINAL");

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("helpdesk-v2"));
        assert!(!json.contains("Not identified"));
        // The key now names the application; that must not reach the server
        // either, or the row would be excluded in name only.
        assert!(!json.contains("Telegram"));
        assert!(!json.contains("url"));
        assert!(!json.contains("title"));
    }

    #[test]
    fn reports_describe_themselves_for_the_disclosure_list() {
        let report = DailyReport::build(
            "device-1",
            "2026-08-20",
            0,
            0,
            "1.0.0",
            &Summary {
                active_ms: 5 * 3_600_000,
                ..Default::default()
            },
            &[],
            &set(vec![row("a", "A", 1)]),
            &set(vec![]),
            &set(vec![]),
            &Default::default(),
            None,
        );
        assert_eq!(
            report.describe(),
            "2026-08-20: 05h 00m tracked, 1 applications, 0 categories"
        );
    }
}
