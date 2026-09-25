//! Narrow, typed commands exposed to the dashboard (spec §120).
//!
//! There is deliberately no `execute_sql` command: the frontend can never run
//! arbitrary queries against the database.

use localtrack_app::diagnostics::Diagnostics;
use localtrack_app::exporting::{ExportRequest, ExportResult};
use localtrack_app::managed::{ManagedStatus, SentReport, SessionSyncSummary};
use localtrack_app::service::ReclassifyScope;
use localtrack_app::{
    ActivityPage, ComparisonResult, CurrentStatus, DeletionPreview, FilterOptions, RangeQuery,
    ReportBundle, SessionDetail, SettingsView, TodayDashboard,
};
use localtrack_core::activity::ActivitySegment;
use localtrack_core::aggregation::{ReportSet, Summary, TimelineBlock};
use localtrack_core::classification::ClassificationRule;
use localtrack_core::privacy::ExclusionRule;
use localtrack_core::sessions::{WorkBreak, WorkSession};
use localtrack_core::settings::Settings;
use localtrack_storage::filters::{ActivityFilter, Page, SortOrder};
use localtrack_storage::repo::categories::Category;
use localtrack_storage::repo::maintenance::RetentionReport;
use localtrack_storage::repo::projects::Project;
use tauri::State;

use crate::AppState;

/// Every command returns a JSON-encoded [`localtrack_app::error::UiError`] on
/// failure, so the UI can show an actionable message (spec §147).
type CmdResult<T> = Result<T, String>;

fn map<T>(result: localtrack_app::Result<T>) -> CmdResult<T> {
    result.map_err(Into::into)
}

// ------------------------------------------------------------------- status

#[tauri::command]
pub fn get_current_status(state: State<'_, AppState>) -> CmdResult<CurrentStatus> {
    map(state.service.current_status())
}

#[tauri::command]
pub fn clock_in(state: State<'_, AppState>, note: Option<String>) -> CmdResult<CurrentStatus> {
    map(state.service.clock_in_with_note(note))
}

#[tauri::command]
pub fn set_session_note(
    state: State<'_, AppState>,
    session_id: String,
    note: Option<String>,
) -> CmdResult<()> {
    map(state.service.set_session_note(&session_id, note))
}

/// Record work done away from this computer.
#[tauri::command]
pub fn add_manual_entry(
    state: State<'_, AppState>,
    started_at_ms: i64,
    ended_at_ms: i64,
    note: Option<String>,
) -> CmdResult<WorkSession> {
    map(state
        .service
        .add_manual_entry(started_at_ms, ended_at_ms, note))
}

// ------------------------------------------------------------- employee mode

#[tauri::command]
pub fn get_managed_status(state: State<'_, AppState>) -> CmdResult<ManagedStatus> {
    map(state.service.managed_status())
}

#[tauri::command]
pub fn enroll_device(
    state: State<'_, AppState>,
    server_url: String,
    code: String,
    device_name: String,
) -> CmdResult<ManagedStatus> {
    map(state.service.enroll(&server_url, &code, &device_name))
}

#[tauri::command]
pub fn unenroll_device(state: State<'_, AppState>) -> CmdResult<()> {
    map(state.service.unenroll())
}

#[tauri::command]
pub fn sync_now(state: State<'_, AppState>) -> CmdResult<SessionSyncSummary> {
    map(state.service.sync_now())
}

/// Everything already sent to the server, so it can be read here.
#[tauri::command]
pub fn list_sent_reports(state: State<'_, AppState>) -> CmdResult<Vec<SentReport>> {
    map(state.service.sent_reports(60))
}

#[tauri::command]
pub fn list_queued_reports(state: State<'_, AppState>) -> CmdResult<Vec<SentReport>> {
    map(state.service.queued_reports(60))
}

#[tauri::command]
pub fn clock_out(state: State<'_, AppState>) -> CmdResult<CurrentStatus> {
    map(state.service.clock_out())
}

#[tauri::command]
pub fn start_break(state: State<'_, AppState>) -> CmdResult<CurrentStatus> {
    map(state.service.start_break())
}

#[tauri::command]
pub fn end_break(state: State<'_, AppState>) -> CmdResult<CurrentStatus> {
    map(state.service.end_break())
}

#[tauri::command]
pub fn set_tracking_paused(state: State<'_, AppState>, paused: bool) -> CmdResult<CurrentStatus> {
    map(state.service.set_paused(paused))
}

// ------------------------------------------------------------------ reports

#[tauri::command]
pub fn get_today_summary(state: State<'_, AppState>) -> CmdResult<TodayDashboard> {
    map(state.service.today())
}

#[tauri::command]
pub fn get_summary(state: State<'_, AppState>, query: RangeQuery) -> CmdResult<Summary> {
    map(state.service.summary(&query))
}

#[tauri::command]
pub fn get_timeline(
    state: State<'_, AppState>,
    query: RangeQuery,
) -> CmdResult<Vec<TimelineBlock>> {
    map(state.service.timeline(&query))
}

#[tauri::command]
pub fn get_reports(state: State<'_, AppState>, query: RangeQuery) -> CmdResult<ReportBundle> {
    map(state.service.reports(&query))
}

/// Week-by-week aggregation (spec §84 comparison, extended to weeks).
#[tauri::command]
pub fn get_weekly_report(
    state: State<'_, AppState>,
    query: RangeQuery,
) -> CmdResult<Vec<localtrack_core::aggregation::WeekSummary>> {
    map(state.service.weekly(&query))
}

#[tauri::command]
pub fn get_page_report(
    state: State<'_, AppState>,
    query: RangeQuery,
    domain: String,
) -> CmdResult<ReportSet> {
    map(state.service.pages_for_domain(&query, &domain))
}

#[tauri::command]
pub fn get_comparison(
    state: State<'_, AppState>,
    query: RangeQuery,
) -> CmdResult<ComparisonResult> {
    map(state.service.compare(&query))
}

#[tauri::command]
pub fn get_activity_page(
    state: State<'_, AppState>,
    filter: ActivityFilter,
    limit: Option<i64>,
    offset: Option<i64>,
    descending: Option<bool>,
) -> CmdResult<ActivityPage> {
    let page = Page {
        limit: limit.unwrap_or(localtrack_storage::filters::DEFAULT_PAGE_SIZE),
        offset: offset.unwrap_or(0),
    };
    let order = if descending.unwrap_or(false) {
        SortOrder::Desc
    } else {
        SortOrder::Asc
    };
    map(state.service.activity_page(&filter, page, order))
}

#[tauri::command]
pub fn get_filter_options(state: State<'_, AppState>) -> CmdResult<FilterOptions> {
    map(state.service.filter_options())
}

// ----------------------------------------------------------------- sessions

#[tauri::command]
pub fn list_sessions(
    state: State<'_, AppState>,
    from_ms: i64,
    to_ms: i64,
) -> CmdResult<Vec<SessionDetail>> {
    map(state.service.sessions(from_ms, to_ms))
}

#[tauri::command]
pub fn update_session(
    state: State<'_, AppState>,
    session_id: String,
    started_at_ms: i64,
    ended_at_ms: Option<i64>,
    note: Option<String>,
) -> CmdResult<SessionDetail> {
    map(state
        .service
        .update_session(&session_id, started_at_ms, ended_at_ms, note))
}

#[tauri::command]
pub fn create_manual_session(
    state: State<'_, AppState>,
    started_at_ms: i64,
    ended_at_ms: i64,
    note: Option<String>,
) -> CmdResult<WorkSession> {
    map(state
        .service
        .create_manual_session(started_at_ms, ended_at_ms, note))
}

#[tauri::command]
pub fn delete_session(state: State<'_, AppState>, session_id: String) -> CmdResult<()> {
    map(state.service.delete_session(&session_id))
}

#[tauri::command]
pub fn add_break(
    state: State<'_, AppState>,
    session_id: String,
    started_at_ms: i64,
    ended_at_ms: Option<i64>,
) -> CmdResult<WorkBreak> {
    map(state
        .service
        .add_break(&session_id, started_at_ms, ended_at_ms))
}

#[tauri::command]
pub fn update_break(
    state: State<'_, AppState>,
    break_id: String,
    started_at_ms: i64,
    ended_at_ms: Option<i64>,
    note: Option<String>,
) -> CmdResult<()> {
    map(state
        .service
        .update_break(&break_id, started_at_ms, ended_at_ms, note))
}

#[tauri::command]
pub fn delete_break(state: State<'_, AppState>, break_id: String) -> CmdResult<()> {
    map(state.service.delete_break(&break_id))
}

// ---------------------------------------------------------------- activity

#[tauri::command]
pub fn classify_segment(
    state: State<'_, AppState>,
    segment_id: String,
    category_id: Option<String>,
    project_id: Option<String>,
) -> CmdResult<ActivitySegment> {
    map(state
        .service
        .classify_segment(&segment_id, category_id, project_id))
}

#[tauri::command]
pub fn annotate_segment(
    state: State<'_, AppState>,
    segment_id: String,
    note: Option<String>,
) -> CmdResult<ActivitySegment> {
    map(state.service.annotate_segment(&segment_id, note))
}

#[tauri::command]
pub fn split_segment(
    state: State<'_, AppState>,
    segment_id: String,
    at_ms: i64,
) -> CmdResult<(String, String)> {
    map(state.service.split_segment(&segment_id, at_ms))
}

#[tauri::command]
pub fn delete_segment(state: State<'_, AppState>, segment_id: String) -> CmdResult<()> {
    map(state.service.delete_segment(&segment_id))
}

// ------------------------------------------------- categories and projects

#[tauri::command]
pub fn list_categories(state: State<'_, AppState>) -> CmdResult<Vec<Category>> {
    map(state.service.categories())
}

#[tauri::command]
pub fn create_category(state: State<'_, AppState>, name: String) -> CmdResult<Category> {
    map(state.service.create_category(&name))
}

#[tauri::command]
pub fn rename_category(state: State<'_, AppState>, id: String, name: String) -> CmdResult<()> {
    map(state.service.rename_category(&id, &name))
}

#[tauri::command]
pub fn delete_category(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    map(state.service.delete_category(&id))
}

#[tauri::command]
pub fn list_projects(
    state: State<'_, AppState>,
    include_archived: Option<bool>,
) -> CmdResult<Vec<Project>> {
    map(state.service.projects(include_archived.unwrap_or(false)))
}

#[tauri::command]
pub fn create_project(state: State<'_, AppState>, name: String) -> CmdResult<Project> {
    map(state.service.create_project(&name))
}

#[tauri::command]
pub fn update_project(
    state: State<'_, AppState>,
    id: String,
    name: String,
    archived: bool,
) -> CmdResult<()> {
    map(state.service.update_project(&id, &name, archived))
}

#[tauri::command]
pub fn delete_project(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    map(state.service.delete_project(&id))
}

// -------------------------------------------------------------------- rules

#[tauri::command]
pub fn list_rules(state: State<'_, AppState>) -> CmdResult<Vec<ClassificationRule>> {
    map(state.service.rules())
}

#[tauri::command]
pub fn save_rule(
    state: State<'_, AppState>,
    rule: ClassificationRule,
    apply_to_existing: Option<bool>,
) -> CmdResult<usize> {
    let scope = if apply_to_existing.unwrap_or(false) {
        ReclassifyScope::ExistingActivity
    } else {
        ReclassifyScope::NewActivityOnly
    };
    map(state.service.save_rule(rule, scope))
}

#[tauri::command]
pub fn delete_rule(
    state: State<'_, AppState>,
    id: String,
    apply_to_existing: Option<bool>,
) -> CmdResult<usize> {
    let scope = if apply_to_existing.unwrap_or(false) {
        ReclassifyScope::ExistingActivity
    } else {
        ReclassifyScope::NewActivityOnly
    };
    map(state.service.delete_rule(&id, scope))
}

#[tauri::command]
pub fn reclassify(
    state: State<'_, AppState>,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> CmdResult<usize> {
    map(state.service.reclassify(from_ms, to_ms))
}

// --------------------------------------------------------------- exclusions

#[tauri::command]
pub fn list_exclusions(state: State<'_, AppState>) -> CmdResult<Vec<ExclusionRule>> {
    map(state.service.exclusions())
}

#[tauri::command]
pub fn save_exclusion(state: State<'_, AppState>, rule: ExclusionRule) -> CmdResult<ExclusionRule> {
    map(state.service.save_exclusion(rule))
}

#[tauri::command]
pub fn delete_exclusion(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    map(state.service.delete_exclusion(&id))
}

// ----------------------------------------------------------------- settings

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> CmdResult<SettingsView> {
    map(state.service.settings_view())
}

#[tauri::command]
pub fn update_setting(
    state: State<'_, AppState>,
    key: String,
    value: serde_json::Value,
) -> CmdResult<Settings> {
    map(state.service.update_setting(&key, value))
}

/// Bring up the dashboard from the timer bar.
///
/// The window is destroyed while it sits in the tray, so the frontend cannot
/// just look it up and show it.
#[tauri::command]
pub fn open_dashboard(app: tauri::AppHandle) {
    crate::dashboard::open(&app);
}

// ------------------------------------------------------------- data control

#[tauri::command]
pub fn preview_deletion(
    state: State<'_, AppState>,
    from_ms: i64,
    to_ms: i64,
) -> CmdResult<DeletionPreview> {
    map(state.service.preview_deletion(from_ms, to_ms))
}

#[tauri::command]
pub fn delete_range(
    state: State<'_, AppState>,
    from_ms: i64,
    to_ms: i64,
    include_sessions: bool,
) -> CmdResult<RetentionReport> {
    map(state.service.delete_range(from_ms, to_ms, include_sessions))
}

#[tauri::command]
pub fn delete_last_minutes(
    state: State<'_, AppState>,
    minutes: i64,
    include_sessions: bool,
) -> CmdResult<RetentionReport> {
    map(state.service.delete_last_minutes(minutes, include_sessions))
}

#[tauri::command]
pub fn delete_all_activity(
    state: State<'_, AppState>,
    include_sessions: bool,
) -> CmdResult<RetentionReport> {
    map(state.service.delete_all(include_sessions))
}

#[tauri::command]
pub fn run_retention(state: State<'_, AppState>) -> CmdResult<Option<RetentionReport>> {
    map(state.service.run_retention_now())
}

#[tauri::command]
pub fn backup_database(
    state: State<'_, AppState>,
    destination: Option<String>,
) -> CmdResult<String> {
    let path = map(state.service.backup(destination.map(Into::into)))?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn restore_database(state: State<'_, AppState>, source: String) -> CmdResult<String> {
    let safety = map(state.service.restore(std::path::Path::new(&source)))?;
    Ok(safety.display().to_string())
}

// ------------------------------------------------------------------- export

#[tauri::command]
pub fn export_report(
    state: State<'_, AppState>,
    request: ExportRequest,
) -> CmdResult<ExportResult> {
    map(state.service.export(&request))
}

#[tauri::command]
pub fn full_urls_available(
    state: State<'_, AppState>,
    from_ms: i64,
    to_ms: i64,
) -> CmdResult<bool> {
    map(state.service.full_urls_available(from_ms, to_ms))
}

// -------------------------------------------------------------- diagnostics

#[tauri::command]
pub fn get_diagnostics(state: State<'_, AppState>) -> CmdResult<Diagnostics> {
    map(state.service.diagnostics())
}

#[tauri::command]
pub fn get_diagnostics_text(state: State<'_, AppState>) -> CmdResult<String> {
    Ok(map(state.service.diagnostics())?.to_plain_text())
}

/// The full command surface handed to Tauri.
///
/// `generate_handler!` is expanded at the call site in `lib.rs`, which is how
/// Tauri expects it to be used.
#[macro_export]
macro_rules! localtrack_handlers {
    () => {
        ::tauri::generate_handler![
            $crate::commands::get_current_status,
            $crate::commands::clock_in,
            $crate::commands::set_session_note,
            $crate::commands::add_manual_entry,
            $crate::commands::get_managed_status,
            $crate::commands::enroll_device,
            $crate::commands::unenroll_device,
            $crate::commands::sync_now,
            $crate::commands::list_sent_reports,
            $crate::commands::list_queued_reports,
            $crate::commands::clock_out,
            $crate::commands::start_break,
            $crate::commands::end_break,
            $crate::commands::set_tracking_paused,
            $crate::commands::get_today_summary,
            $crate::commands::get_summary,
            $crate::commands::get_timeline,
            $crate::commands::get_reports,
            $crate::commands::get_weekly_report,
            $crate::commands::get_page_report,
            $crate::commands::get_comparison,
            $crate::commands::get_activity_page,
            $crate::commands::get_filter_options,
            $crate::commands::list_sessions,
            $crate::commands::update_session,
            $crate::commands::create_manual_session,
            $crate::commands::delete_session,
            $crate::commands::add_break,
            $crate::commands::update_break,
            $crate::commands::delete_break,
            $crate::commands::classify_segment,
            $crate::commands::annotate_segment,
            $crate::commands::split_segment,
            $crate::commands::delete_segment,
            $crate::commands::list_categories,
            $crate::commands::create_category,
            $crate::commands::rename_category,
            $crate::commands::delete_category,
            $crate::commands::list_projects,
            $crate::commands::create_project,
            $crate::commands::update_project,
            $crate::commands::delete_project,
            $crate::commands::list_rules,
            $crate::commands::save_rule,
            $crate::commands::delete_rule,
            $crate::commands::reclassify,
            $crate::commands::list_exclusions,
            $crate::commands::save_exclusion,
            $crate::commands::delete_exclusion,
            $crate::commands::get_settings,
            $crate::commands::update_setting,
            $crate::commands::open_dashboard,
            $crate::commands::preview_deletion,
            $crate::commands::delete_range,
            $crate::commands::delete_last_minutes,
            $crate::commands::delete_all_activity,
            $crate::commands::run_retention,
            $crate::commands::backup_database,
            $crate::commands::restore_database,
            $crate::commands::export_report,
            $crate::commands::full_urls_available,
            $crate::commands::get_diagnostics,
            $crate::commands::get_diagnostics_text,
        ]
    };
}
