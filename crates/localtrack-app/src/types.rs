//! Types crossing the Rust ↔ TypeScript boundary.
//!
//! All of them are `camelCase` in JSON so the frontend never has to translate.

use localtrack_collector_common::collector::CollectorStatus;
use localtrack_collector_common::pipeline::TrackingDecision;
use localtrack_core::activity::ActivitySegment;
use localtrack_core::aggregation::{DaySummary, ReportSet, Summary, TimelineBlock};
use localtrack_core::sessions::{ClockState, WorkBreak, WorkSession};
use localtrack_core::settings::Settings;
use serde::{Deserialize, Serialize};

/// The status card at the top of the dashboard, the tray and the popup
/// (spec §72, §96, §100).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentStatus {
    pub state: ClockState,
    pub tracking: TrackingDecision,
    pub session: Option<WorkSession>,
    pub open_break: Option<WorkBreak>,
    pub session_duration_ms: i64,
    pub break_duration_ms: i64,
    pub current_activity: Option<CurrentActivity>,
    pub today: Summary,
    pub collectors: Vec<CollectorStatus>,
    pub chrome_connected: bool,
    pub stale_session_warning: bool,
    pub app_version: String,
    pub schema_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentActivity {
    pub label: String,
    pub app_name: Option<String>,
    pub domain: Option<String>,
    pub title: Option<String>,
    pub since_ms: i64,
    pub category_name: Option<String>,
    pub project_name: Option<String>,
}

/// A time range plus filters, used by every report query.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RangeQuery {
    pub from_ms: i64,
    pub to_ms: i64,
    pub filter: localtrack_storage::ActivityFilter,
}

/// A session row with its computed metrics (spec §93, §114).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub session: WorkSession,
    pub breaks: Vec<WorkBreak>,
    pub summary: Summary,
}

/// One page of the activity feed (spec §80, §121).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityPage {
    pub rows: Vec<ActivityRow>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityRow {
    #[serde(flatten)]
    pub segment: ActivitySegment,
    pub label: String,
    pub duration_ms: i64,
    pub category_name: Option<String>,
    pub project_name: Option<String>,
}

/// Everything the Today page needs in one round trip (spec §71).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayDashboard {
    pub summary: Summary,
    pub timeline: Vec<TimelineBlock>,
    pub applications: ReportSet,
    pub websites: ReportSet,
    /// What the window titles said was being worked on (spec §75, extended).
    pub workspaces: ReportSet,
    pub categories: ReportSet,
    pub projects: ReportSet,
}

/// All reports for a period (spec §75–§79).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportBundle {
    pub summary: Summary,
    pub daily: Vec<DaySummary>,
    pub applications: ReportSet,
    pub websites: ReportSet,
    pub pages: ReportSet,
    /// What the window titles said was being worked on.
    pub workspaces: ReportSet,
    pub categories: ReportSet,
    pub projects: ReportSet,
}

/// Period-over-period comparison (spec §84).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonResult {
    pub current: ReportBundle,
    pub previous: ReportBundle,
    pub clocked_delta_ms: i64,
    pub work_delta_ms: i64,
    pub active_delta_ms: i64,
    pub idle_delta_ms: i64,
    pub break_delta_ms: i64,
}

/// Values available for filter dropdowns (spec §81).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterOptions {
    pub applications: Vec<String>,
    pub processes: Vec<String>,
    pub browsers: Vec<String>,
    pub domains: Vec<String>,
}

/// Settings plus the values the UI needs to explain them (spec §110, §128).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub settings: Settings,
    pub data_directory: String,
    pub database_path: String,
    pub database_size_bytes: i64,
    pub schema_version: i64,
    pub app_version: String,
}

/// What a data deletion would affect, so confirmation is informed (spec §111).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionPreview {
    pub segments: i64,
    pub sessions: i64,
}
