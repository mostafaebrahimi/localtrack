//! Read paths for the dashboard. Every query is bounded by a time range and
//! aggregates in Rust/SQL, never by shipping the database to the frontend
//! (spec §120–§123).

use std::collections::BTreeMap;
use std::sync::Arc;

use localtrack_core::aggregation::{
    reports, summary as summary_mod, timeline, AggregationInput, ReportSet, Summary, TimelineBlock,
};
use localtrack_core::interval::Interval;
use localtrack_core::settings::Settings;
use localtrack_core::time::now_ms;
use localtrack_storage::filters::{ActivityFilter, Page, SortOrder};
use localtrack_storage::{repo, Database};

use crate::error::Result;
use crate::types::{
    ActivityPage, ActivityRow, ComparisonResult, FilterOptions, ReportBundle, SessionDetail,
    TodayDashboard,
};

/// Load everything the aggregation engine needs for a window.
pub fn load_input(
    db: &Arc<Database>,
    from_ms: i64,
    to_ms: i64,
    filter: &ActivityFilter,
    settings: &Settings,
) -> Result<AggregationInput> {
    let (segments, sessions, breaks) = db.read(|conn| {
        Ok((
            repo::segments::load_range(conn, from_ms, to_ms)?,
            repo::sessions::list_sessions(conn, from_ms, to_ms)?,
            repo::sessions::breaks_in_range(conn, from_ms, to_ms)?,
        ))
    })?;

    // Knowing when LocalTrack was running turns an unexplained gap into an
    // explained one — but only for the period the record covers. Before the
    // first recorded run, treat the time as known-good rather than blaming the
    // agent for a gap nobody can account for.
    let (mut agent_running, known_since) = db.read(|conn| {
        Ok((
            repo::uptime::running_intervals(conn, from_ms, to_ms)?,
            repo::uptime::known_since(conn)?,
        ))
    })?;
    let unknown_before = known_since.unwrap_or(to_ms).min(to_ms).max(from_ms);
    if unknown_before > from_ms {
        agent_running = agent_running.union(&localtrack_core::interval::IntervalSet::single(
            Interval::new(from_ms, unknown_before),
        ));
    }

    let mut input = AggregationInput::new(Interval::new(from_ms, to_ms), now_ms());
    input.segments = apply_filter(segments, filter);
    input.sessions = sessions;
    input.breaks = breaks;
    input.agent_running = Some(agent_running);
    input.focus = localtrack_core::aggregation::FocusOptions {
        context_switch_noise_ms: settings.context_switch_noise_seconds * 1000,
        interruption_tolerance_ms: settings.focus_interruption_tolerance_seconds * 1000,
    };
    Ok(input)
}

/// Apply the in-memory part of a filter.
///
/// Idle and lock segments are never filtered out: removing them would silently
/// turn idle time into active time.
fn apply_filter(
    segments: Vec<localtrack_core::activity::ActivitySegment>,
    filter: &ActivityFilter,
) -> Vec<localtrack_core::activity::ActivitySegment> {
    let has_filters = !filter.app_names.is_empty()
        || !filter.process_names.is_empty()
        || !filter.browsers.is_empty()
        || !filter.domains.is_empty()
        || !filter.category_ids.is_empty()
        || !filter.project_ids.is_empty()
        || filter.search.is_some()
        || filter.min_duration_ms.is_some()
        || filter.max_duration_ms.is_some()
        || filter.uncategorized_only
        || filter.unassigned_only;

    if !has_filters {
        return segments;
    }

    let needle = filter.search.as_ref().map(|s| s.to_lowercase());
    segments
        .into_iter()
        .filter(|segment| {
            if segment.kind.is_inactive() {
                return true;
            }
            let matches_list = |values: &Vec<String>, value: &Option<String>| {
                values.is_empty()
                    || value
                        .as_deref()
                        .map(|v| values.iter().any(|candidate| candidate == v))
                        .unwrap_or(false)
            };
            if !matches_list(&filter.app_names, &segment.app_name)
                || !matches_list(&filter.process_names, &segment.process_name)
                || !matches_list(&filter.browsers, &segment.browser)
                || !matches_list(&filter.domains, &segment.domain)
                || !matches_list(&filter.category_ids, &segment.category_id)
                || !matches_list(&filter.project_ids, &segment.project_id)
            {
                return false;
            }
            if filter.uncategorized_only && segment.category_id.is_some() {
                return false;
            }
            if filter.unassigned_only && segment.project_id.is_some() {
                return false;
            }
            if let Some(min) = filter.min_duration_ms {
                if segment.duration_ms() < min {
                    return false;
                }
            }
            if let Some(max) = filter.max_duration_ms {
                if segment.duration_ms() > max {
                    return false;
                }
            }
            if let Some(needle) = &needle {
                let haystack = [
                    segment.app_name.as_deref(),
                    segment.process_name.as_deref(),
                    segment.window_title.as_deref(),
                    segment.domain.as_deref(),
                    segment.page_title.as_deref(),
                    segment.url.as_deref(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
                if !haystack.contains(needle) {
                    return false;
                }
            }
            true
        })
        .collect()
}

pub fn category_names(db: &Arc<Database>) -> Result<BTreeMap<String, String>> {
    let categories = db.read(repo::categories::list)?;
    Ok(categories.into_iter().map(|c| (c.id, c.name)).collect())
}

pub fn project_names(db: &Arc<Database>) -> Result<BTreeMap<String, String>> {
    let projects = db.read(|conn| repo::projects::list(conn, true))?;
    Ok(projects.into_iter().map(|p| (p.id, p.name)).collect())
}

pub fn summary(
    db: &Arc<Database>,
    from_ms: i64,
    to_ms: i64,
    filter: &ActivityFilter,
    settings: &Settings,
) -> Result<Summary> {
    let input = load_input(db, from_ms, to_ms, filter, settings)?;
    Ok(summary_mod::compute_summary(&input))
}

pub fn timeline_blocks(
    db: &Arc<Database>,
    from_ms: i64,
    to_ms: i64,
    filter: &ActivityFilter,
    settings: &Settings,
) -> Result<Vec<TimelineBlock>> {
    let input = load_input(db, from_ms, to_ms, filter, settings)?;
    // Already normalized and overlaid: the frontend never re-implements
    // interval logic (spec §122).
    Ok(timeline::compact_timeline(
        timeline::build_timeline(&input),
        1_000,
    ))
}

pub fn today(db: &Arc<Database>, settings: &Settings) -> Result<TodayDashboard> {
    let now = now_ms();
    let from = localtrack_core::time::local_day_start_ms(now);
    let to = localtrack_core::time::local_day_end_ms(now).min(now.max(from + 1));
    let filter = ActivityFilter::default();
    let input = load_input(db, from, to, &filter, settings)?;
    let categories = category_names(db)?;
    let projects = project_names(db)?;

    Ok(TodayDashboard {
        summary: summary_mod::compute_summary(&input),
        timeline: timeline::compact_timeline(timeline::build_timeline(&input), 1_000),
        applications: reports::application_report(&input),
        websites: reports::website_report(&input),
        workspaces: reports::workspace_report(&input),
        categories: reports::category_report(&input, &categories),
        projects: reports::project_report(&input, &projects),
    })
}

/// Per-week totals with application, website and address usage.
pub fn weekly_report(
    db: &Arc<Database>,
    from_ms: i64,
    to_ms: i64,
    filter: &ActivityFilter,
    settings: &Settings,
) -> Result<Vec<localtrack_core::aggregation::WeekSummary>> {
    let input = load_input(db, from_ms, to_ms, filter, settings)?;
    Ok(summary_mod::compute_weekly_summaries(&input))
}

pub fn report_bundle(
    db: &Arc<Database>,
    from_ms: i64,
    to_ms: i64,
    filter: &ActivityFilter,
    settings: &Settings,
) -> Result<ReportBundle> {
    let input = load_input(db, from_ms, to_ms, filter, settings)?;
    let categories = category_names(db)?;
    let projects = project_names(db)?;
    Ok(ReportBundle {
        summary: summary_mod::compute_summary(&input),
        daily: summary_mod::compute_daily_summaries(&input),
        applications: reports::application_report(&input),
        websites: reports::website_report(&input),
        pages: reports::page_report(&input, None),
        workspaces: reports::workspace_report(&input),
        categories: reports::category_report(&input, &categories),
        projects: reports::project_report(&input, &projects),
    })
}

pub fn page_report_for_domain(
    db: &Arc<Database>,
    from_ms: i64,
    to_ms: i64,
    domain: &str,
    filter: &ActivityFilter,
    settings: &Settings,
) -> Result<ReportSet> {
    let input = load_input(db, from_ms, to_ms, filter, settings)?;
    Ok(reports::page_report(&input, Some(domain)))
}

/// Compare a period with the immediately preceding one of the same length.
pub fn compare(
    db: &Arc<Database>,
    from_ms: i64,
    to_ms: i64,
    filter: &ActivityFilter,
    settings: &Settings,
) -> Result<ComparisonResult> {
    let length = (to_ms - from_ms).max(0);
    let current = report_bundle(db, from_ms, to_ms, filter, settings)?;
    let previous = report_bundle(db, from_ms - length, from_ms, filter, settings)?;
    Ok(ComparisonResult {
        clocked_delta_ms: current.summary.clocked_ms - previous.summary.clocked_ms,
        work_delta_ms: current.summary.work_ms - previous.summary.work_ms,
        active_delta_ms: current.summary.active_ms - previous.summary.active_ms,
        idle_delta_ms: current.summary.idle_ms - previous.summary.idle_ms,
        break_delta_ms: current.summary.break_ms - previous.summary.break_ms,
        current,
        previous,
    })
}

pub fn activity_page(
    db: &Arc<Database>,
    filter: &ActivityFilter,
    page: Page,
    order: SortOrder,
) -> Result<ActivityPage> {
    let page = page.clamped();
    let (segments, total) = db.read(|conn| {
        Ok((
            repo::segments::list(conn, filter, page, order)?,
            repo::segments::count(conn, filter)?,
        ))
    })?;
    let categories = category_names(db)?;
    let projects = project_names(db)?;

    let rows = segments
        .into_iter()
        .map(|segment| ActivityRow {
            label: segment.label(),
            duration_ms: segment.duration_ms(),
            category_name: segment
                .category_id
                .as_ref()
                .and_then(|id| categories.get(id).cloned()),
            project_name: segment
                .project_id
                .as_ref()
                .and_then(|id| projects.get(id).cloned()),
            segment,
        })
        .collect();

    Ok(ActivityPage {
        rows,
        total,
        limit: page.limit,
        offset: page.offset,
    })
}

/// Sessions with their computed metrics (spec §93).
pub fn session_details(
    db: &Arc<Database>,
    from_ms: i64,
    to_ms: i64,
    settings: &Settings,
) -> Result<Vec<SessionDetail>> {
    let sessions = db.read(|conn| repo::sessions::list_sessions(conn, from_ms, to_ms))?;
    let mut out = Vec::with_capacity(sessions.len());
    for session in sessions {
        let breaks = db.read(|conn| repo::sessions::breaks_for_session(conn, &session.id))?;
        let start = session.started_at_ms;
        let end = session.ended_at_ms.unwrap_or_else(now_ms);
        let mut input = load_input(db, start, end, &ActivityFilter::default(), settings)?;
        input.sessions = vec![session.clone()];
        input.breaks = breaks.clone();
        out.push(SessionDetail {
            summary: summary_mod::compute_summary(&input),
            session,
            breaks,
        });
    }
    Ok(out)
}

pub fn filter_options(db: &Arc<Database>) -> Result<FilterOptions> {
    db.read(|conn| {
        Ok(FilterOptions {
            applications: repo::segments::distinct_values(conn, "app_name", 200)?,
            processes: repo::segments::distinct_values(conn, "process_name", 200)?,
            browsers: repo::segments::distinct_values(conn, "browser", 20)?,
            domains: repo::segments::distinct_values(conn, "domain", 500)?,
        })
    })
    .map_err(Into::into)
}
