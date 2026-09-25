use localtrack_core::activity::ActivitySegment;
use localtrack_core::aggregation::{DaySummary, ReportSet, Summary};
use localtrack_core::privacy::{domain_of, sanitize_url, UrlPolicy};
use serde::{Deserialize, Serialize};

/// How much of a stored URL appears in the export (spec §89).
///
/// `FullUrl` is only meaningful when full URLs were actually stored; the UI
/// hides the option otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExportUrlPrivacy {
    DomainOnly,
    #[default]
    SanitizedUrl,
    FullUrl,
}

impl ExportUrlPrivacy {
    /// Apply the export privacy level to a stored URL.
    pub fn apply(&self, url: Option<&str>) -> Option<String> {
        let url = url?;
        match self {
            ExportUrlPrivacy::DomainOnly => domain_of(url),
            ExportUrlPrivacy::SanitizedUrl => {
                sanitize_url(url, UrlPolicy::PathWithoutQuery).or_else(|| Some(url.to_string()))
            }
            ExportUrlPrivacy::FullUrl => Some(url.to_string()),
        }
    }
}

/// Which datasets to include (spec §89).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    pub include_summary: bool,
    pub include_daily_summary: bool,
    pub include_sessions: bool,
    pub include_applications: bool,
    pub include_websites: bool,
    pub include_pages: bool,
    pub include_categories: bool,
    pub include_projects: bool,
    pub include_activity_detail: bool,
    /// Off by default, like detailed interaction tracking itself.
    pub include_interactions: bool,
    pub url_privacy: ExportUrlPrivacy,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            include_summary: true,
            include_daily_summary: true,
            include_sessions: true,
            include_applications: true,
            include_websites: true,
            include_pages: true,
            include_categories: true,
            include_projects: true,
            include_activity_detail: true,
            include_interactions: false,
            url_privacy: ExportUrlPrivacy::SanitizedUrl,
        }
    }
}

/// One exported work session (spec §93).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionExportRow {
    pub date: String,
    pub clock_in_ms: i64,
    pub clock_out_ms: Option<i64>,
    pub clocked_ms: i64,
    pub break_ms: i64,
    pub work_ms: i64,
    pub active_ms: i64,
    pub idle_ms: i64,
    pub untracked_ms: i64,
    pub note: Option<String>,
}

/// A fully resolved activity row (category and project names, not ids).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityExportRow {
    pub segment: ActivitySegment,
    pub category_name: Option<String>,
    pub project_name: Option<String>,
}

/// Everything an export needs; assembled by the application layer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportData {
    pub from_ms: i64,
    pub to_ms: i64,
    pub generated_at_ms: i64,
    pub app_version: String,

    pub summary: Summary,
    pub daily: Vec<DaySummary>,
    pub sessions: Vec<SessionExportRow>,

    pub applications: Option<ReportSet>,
    pub websites: Option<ReportSet>,
    pub pages: Option<ReportSet>,
    pub categories: Option<ReportSet>,
    pub projects: Option<ReportSet>,

    pub activities: Vec<ActivityExportRow>,
    pub interactions: Vec<ActivityExportRow>,
}
