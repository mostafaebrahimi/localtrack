use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::timeline::{build_timeline, TimelineBlockKind};
use super::AggregationInput;
use crate::activity::{ActivityKind, ActivitySegment};
use crate::interval::{overlay_by_priority, Interval, IntervalSet, OverlayItem};

/// Report dimensions (spec §75–§79).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReportDimension {
    Application,
    Website,
    Page,
    Category,
    Project,
    /// What the window titles said was being worked on.
    Workspace,
}

/// One row of a report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportRow {
    /// Stable key (app name, domain, url, category id, project id).
    pub key: String,
    /// Human label.
    pub label: String,
    /// Optional secondary label, e.g. the domain of a page row.
    pub secondary: Option<String>,
    pub duration_ms: i64,
    /// Share of the report total, 0.0–100.0.
    pub percentage: f64,
    /// Number of separate visits/sessions of this item.
    pub visit_count: i64,
}

/// A complete report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSet {
    pub dimension: ReportDimension,
    pub total_ms: i64,
    pub rows: Vec<ReportRow>,
}

/// Drop the scheme so a list of addresses reads as addresses.
fn readable_address(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

struct Item {
    key: String,
    label: String,
    secondary: Option<String>,
    interval: Interval,
}

fn build_rows(dimension: ReportDimension, items: Vec<Item>) -> ReportSet {
    let mut buckets: BTreeMap<String, (String, Option<String>, Vec<Interval>)> = BTreeMap::new();
    for item in items {
        let entry = buckets
            .entry(item.key)
            .or_insert_with(|| (item.label.clone(), item.secondary.clone(), Vec::new()));
        entry.2.push(item.interval);
    }

    let mut rows: Vec<ReportRow> = buckets
        .into_iter()
        .map(|(key, (label, secondary, intervals))| {
            let set = IntervalSet::from_intervals(intervals);
            // Visits: separate appearances, joined when less than a minute apart.
            let visits = set.merge_with_tolerance(60_000).len() as i64;
            ReportRow {
                key,
                label,
                secondary,
                duration_ms: set.duration_ms(),
                percentage: 0.0,
                visit_count: visits,
            }
        })
        .filter(|row| row.duration_ms > 0)
        .collect();

    let total_ms: i64 = rows.iter().map(|r| r.duration_ms).sum();
    for row in &mut rows {
        row.percentage = if total_ms > 0 {
            (row.duration_ms as f64 / total_ms as f64) * 100.0
        } else {
            0.0
        };
    }
    rows.sort_by(|a, b| {
        b.duration_ms
            .cmp(&a.duration_ms)
            .then(a.label.cmp(&b.label))
    });

    ReportSet {
        dimension,
        total_ms,
        rows,
    }
}

/// Deduplicate a set of segments so overlapping observations in the same
/// dimension can never be counted twice (spec §118).
fn deduplicated(segments: Vec<(ActivitySegment, Interval)>) -> Vec<(ActivitySegment, Interval)> {
    let layer: Vec<OverlayItem<ActivitySegment>> = segments
        .into_iter()
        .map(|(segment, interval)| OverlayItem::new(interval, segment))
        .collect();
    overlay_by_priority(vec![layer])
        .into_iter()
        .map(|item| (item.payload, item.interval))
        .collect()
}

fn clipped(input: &AggregationInput, kinds: &[ActivityKind]) -> Vec<(ActivitySegment, Interval)> {
    let active = input.active_window();
    let mut out = Vec::new();
    for segment in &input.segments {
        if !kinds.contains(&segment.kind) {
            continue;
        }
        for piece in active.clip_interval(segment.interval()) {
            out.push((segment.clone(), piece));
        }
    }
    deduplicated(out)
}

/// Applications report (spec §75).
///
/// Desktop window segments are authoritative. Browser segments only contribute
/// where no desktop segment covers the time — that keeps the report correct on
/// platforms without an active-window API (e.g. some Wayland compositors)
/// without ever double counting.
pub fn application_report(input: &AggregationInput) -> ReportSet {
    let desktop = clipped(input, &[ActivityKind::Window, ActivityKind::Manual]);
    let desktop_cover = IntervalSet::from_intervals(desktop.iter().map(|(_, i)| *i));

    let mut items: Vec<Item> = desktop
        .into_iter()
        .map(|(segment, interval)| Item {
            key: segment
                .app_name
                .clone()
                .or_else(|| segment.process_name.clone())
                .unwrap_or_else(|| "Unknown".into()),
            label: segment
                .app_name
                .clone()
                .or_else(|| segment.process_name.clone())
                .unwrap_or_else(|| "Unknown".into()),
            secondary: segment.process_name.clone(),
            interval,
        })
        .collect();

    // Input activity with no window: reported under its own name so the
    // application report still adds up to active time.
    for (_segment, interval) in clipped(input, &[ActivityKind::Input]) {
        for piece in desktop_cover.subtract_interval(interval) {
            items.push(Item {
                key: "Unattributed activity".into(),
                label: "Unattributed activity".into(),
                secondary: None,
                interval: piece,
            });
        }
    }

    for (segment, interval) in clipped(input, &[ActivityKind::BrowserPage]) {
        for piece in desktop_cover.subtract_interval(interval) {
            let name = segment
                .browser
                .clone()
                .map(|b| {
                    let mut chars = b.chars();
                    match chars.next() {
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                        None => b.clone(),
                    }
                })
                .unwrap_or_else(|| "Browser".into());
            items.push(Item {
                key: name.clone(),
                label: name,
                secondary: None,
                interval: piece,
            });
        }
    }

    build_rows(ReportDimension::Application, items)
}

/// What the day was spent working *on*, read from window titles.
///
/// The applications report says "Code, 1h 28m"; this one says "helpdesk-v2,
/// 1h 12m" by reading the project out of the editor's title, the directory or
/// task out of the terminal's, and the repository or ticket out of the page's.
/// Time whose title said nothing is kept rather than dropped, so the report
/// still adds up to the time that was tracked. It is grouped per application:
/// the title said nothing about the work, but which application it was spent
/// in is still worth reporting.
pub fn workspace_report(input: &AggregationInput) -> ReportSet {
    let mut items = Vec::new();
    let mut unknown = Vec::new();

    for (segment, interval) in clipped(
        input,
        &[
            ActivityKind::Window,
            ActivityKind::BrowserPage,
            ActivityKind::Manual,
        ],
    ) {
        let context = crate::context::work_context(&segment);
        match context.workspace {
            Some(workspace) => items.push(Item {
                // The kind is part of the key so an editor project and a chat
                // that happen to share a name stay separate rows.
                key: format!("{}:{}", context.kind.as_str(), workspace),
                label: workspace,
                secondary: segment
                    .app_name
                    .clone()
                    .or_else(|| segment.process_name.clone()),
                interval,
            }),
            None => unknown.push((
                segment
                    .app_name
                    .clone()
                    .or_else(|| segment.process_name.clone()),
                interval,
            )),
        }
    }

    // The application is part of the key, so unidentified time in two
    // applications reads as two rows instead of one anonymous lump. Anything
    // that treats this time as the agent's own business matches the
    // `UNKNOWN:` prefix, never the whole key.
    for (app, interval) in unknown {
        items.push(Item {
            key: format!("UNKNOWN:{}", app.as_deref().unwrap_or_default()),
            label: "Not identified".into(),
            secondary: app,
            interval,
        });
    }

    build_rows(ReportDimension::Workspace, items)
}

/// Websites report (spec §76).
pub fn website_report(input: &AggregationInput) -> ReportSet {
    let items = clipped(input, &[ActivityKind::BrowserPage])
        .into_iter()
        .filter_map(|(segment, interval)| {
            let domain = segment.domain.clone()?;
            Some(Item {
                key: domain.clone(),
                label: domain,
                secondary: segment.browser.clone(),
                interval,
            })
        })
        .collect();
    build_rows(ReportDimension::Website, items)
}

/// Pages report (spec §77). Optionally restricted to one domain.
pub fn page_report(input: &AggregationInput, domain_filter: Option<&str>) -> ReportSet {
    let items = clipped(input, &[ActivityKind::BrowserPage])
        .into_iter()
        .filter(|(segment, _)| match domain_filter {
            Some(domain) => segment.domain.as_deref() == Some(domain),
            None => true,
        })
        .filter_map(|(segment, interval)| {
            let key = segment
                .url
                .clone()
                .or_else(|| segment.domain.clone())
                .or_else(|| segment.page_title.clone())?;
            // The address is the subject of this report, so it is the label;
            // the page title explains it underneath.
            let label = readable_address(&key);
            let secondary = segment
                .page_title
                .clone()
                .or_else(|| segment.domain.clone());
            Some(Item {
                key,
                label,
                secondary,
                interval,
            })
        })
        .collect();
    build_rows(ReportDimension::Page, items)
}

/// Category report (spec §78), computed over the primary timeline so browser
/// and desktop time are never added together.
pub fn category_report(input: &AggregationInput, names: &BTreeMap<String, String>) -> ReportSet {
    let items = build_timeline(input)
        .into_iter()
        .filter(|b| {
            matches!(
                b.block_kind,
                TimelineBlockKind::Application
                    | TimelineBlockKind::BrowserPage
                    | TimelineBlockKind::Input
            )
        })
        .map(|block| {
            let key = block
                .category_id
                .clone()
                .unwrap_or_else(|| "uncategorized".into());
            let label = names
                .get(&key)
                .cloned()
                .unwrap_or_else(|| "Uncategorized".into());
            Item {
                key,
                label,
                secondary: None,
                interval: Interval::new(block.start_ms, block.end_ms),
            }
        })
        .collect();
    build_rows(ReportDimension::Category, items)
}

/// Project report (spec §79). Unassigned time is reported explicitly.
pub fn project_report(input: &AggregationInput, names: &BTreeMap<String, String>) -> ReportSet {
    let items = build_timeline(input)
        .into_iter()
        .filter(|b| {
            matches!(
                b.block_kind,
                TimelineBlockKind::Application
                    | TimelineBlockKind::BrowserPage
                    | TimelineBlockKind::Input
            )
        })
        .map(|block| {
            let key = block
                .project_id
                .clone()
                .unwrap_or_else(|| "unassigned".into());
            let label = names
                .get(&key)
                .cloned()
                .unwrap_or_else(|| "Unassigned".into());
            Item {
                key,
                label,
                secondary: None,
                interval: Interval::new(block.start_ms, block.end_ms),
            }
        })
        .collect();
    build_rows(ReportDimension::Project, items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::SegmentKey;
    use crate::sessions::{WorkBreak, WorkSession};

    const MIN: i64 = 60_000;
    const HOUR: i64 = 60 * MIN;

    fn session(start: i64, end: i64) -> WorkSession {
        WorkSession {
            id: "s".into(),
            started_at_ms: start,
            ended_at_ms: Some(end),
            start_timezone_offset_min: 0,
            end_timezone_offset_min: Some(0),
            note: None,
            created_manually: false,
            edited_manually: false,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }

    fn brk(start: i64, end: i64) -> WorkBreak {
        WorkBreak {
            id: "b".into(),
            work_session_id: "s".into(),
            started_at_ms: start,
            ended_at_ms: Some(end),
            note: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }

    fn desktop(app: &str, start: i64, end: i64) -> ActivitySegment {
        ActivitySegment::from_key(
            &SegmentKey::desktop(
                Some(app.into()),
                Some(format!("{app}.exe")),
                Some("w".into()),
            ),
            start,
            end,
            0,
        )
    }

    fn desktop_titled(app: &str, title: &str, start: i64, end: i64) -> ActivitySegment {
        ActivitySegment::from_key(
            &SegmentKey::desktop(
                Some(app.into()),
                Some(format!("{app}.exe")),
                Some(title.into()),
            ),
            start,
            end,
            0,
        )
    }

    fn page(domain: &str, path: &str, start: i64, end: i64) -> ActivitySegment {
        ActivitySegment::from_key(
            &SegmentKey::browser_page(
                Some("chrome".into()),
                Some(domain.into()),
                Some(format!("https://{domain}{path}")),
                Some(format!("{domain} page")),
            ),
            start,
            end,
            0,
        )
    }

    fn acceptance_input() -> AggregationInput {
        let mut input = AggregationInput::new(Interval::new(0, 3 * HOUR), 3 * HOUR);
        input.sessions.push(session(0, 3 * HOUR));
        input.breaks.push(brk(2 * HOUR, 2 * HOUR + 15 * MIN));
        input.segments = vec![
            desktop("VS Code", 0, HOUR),
            desktop("Chrome", HOUR, HOUR + 30 * MIN),
            page("github.com", "/a", HOUR, HOUR + 30 * MIN),
            ActivitySegment::from_key(&SegmentKey::idle(), HOUR + 30 * MIN, HOUR + 40 * MIN, 0),
            desktop("Chrome", HOUR + 40 * MIN, 2 * HOUR),
            page("chatgpt.com", "/b", HOUR + 40 * MIN, 2 * HOUR),
            desktop("VS Code", 2 * HOUR + 15 * MIN, 3 * HOUR),
        ];
        input
    }

    #[test]
    fn spec_163_application_report() {
        let report = application_report(&acceptance_input());
        let by_key = |k: &str| {
            report
                .rows
                .iter()
                .find(|r| r.key == k)
                .map(|r| r.duration_ms)
                .unwrap_or(0)
        };
        assert_eq!(by_key("VS Code"), HOUR + 45 * MIN);
        assert_eq!(by_key("Chrome"), 50 * MIN);
        assert_eq!(report.total_ms, 2 * HOUR + 35 * MIN);
    }

    #[test]
    fn spec_163_website_report() {
        let report = website_report(&acceptance_input());
        let by_key = |k: &str| {
            report
                .rows
                .iter()
                .find(|r| r.key == k)
                .map(|r| r.duration_ms)
                .unwrap_or(0)
        };
        assert_eq!(by_key("github.com"), 30 * MIN);
        assert_eq!(by_key("chatgpt.com"), 20 * MIN);
        assert_eq!(report.total_ms, 50 * MIN);
    }

    #[test]
    fn percentages_sum_to_one_hundred() {
        let report = application_report(&acceptance_input());
        let sum: f64 = report.rows.iter().map(|r| r.percentage).sum();
        assert!((sum - 100.0).abs() < 0.001, "sum={sum}");
    }

    #[test]
    fn browser_time_counts_as_app_when_no_desktop_collector() {
        // Wayland-style: no desktop window segments at all.
        let mut input = AggregationInput::new(Interval::new(0, HOUR), HOUR);
        input.sessions.push(session(0, HOUR));
        input.segments = vec![page("github.com", "/a", 0, HOUR)];
        let report = application_report(&input);
        assert_eq!(report.rows.len(), 1);
        assert_eq!(report.rows[0].label, "Chrome");
        assert_eq!(report.rows[0].duration_ms, HOUR);
    }

    #[test]
    fn page_report_can_filter_by_domain() {
        let input = acceptance_input();
        let all = page_report(&input, None);
        assert_eq!(all.rows.len(), 2);
        let filtered = page_report(&input, Some("github.com"));
        assert_eq!(filtered.rows.len(), 1);
        assert_eq!(filtered.rows[0].key, "https://github.com/a");
        assert_eq!(
            filtered.rows[0].label, "github.com/a",
            "the address is the subject"
        );
        assert_eq!(
            filtered.rows[0].secondary.as_deref(),
            Some("github.com page")
        );
        assert_eq!(filtered.total_ms, 30 * MIN);
    }

    #[test]
    fn category_and_project_reports_use_the_primary_timeline() {
        let mut input = acceptance_input();
        for segment in &mut input.segments {
            if segment.kind == ActivityKind::BrowserPage {
                segment.category_id = Some("research".into());
                segment.project_id = Some("hub".into());
            } else if segment.app_name.as_deref() == Some("VS Code") {
                segment.category_id = Some("development".into());
                segment.project_id = Some("hub".into());
            }
        }
        let mut names = BTreeMap::new();
        names.insert("research".to_string(), "Research".to_string());
        names.insert("development".to_string(), "Development".to_string());
        names.insert("hub".to_string(), "Hub".to_string());

        let cats = category_report(&input, &names);
        assert_eq!(cats.total_ms, 2 * HOUR + 35 * MIN);
        let dev = cats.rows.iter().find(|r| r.key == "development").unwrap();
        assert_eq!(dev.duration_ms, HOUR + 45 * MIN);
        let research = cats.rows.iter().find(|r| r.key == "research").unwrap();
        assert_eq!(research.duration_ms, 50 * MIN);

        let projects = project_report(&input, &names);
        let hub = projects.rows.iter().find(|r| r.key == "hub").unwrap();
        assert_eq!(hub.duration_ms, 2 * HOUR + 35 * MIN);
        assert_eq!(hub.label, "Hub");
    }

    #[test]
    fn unassigned_project_time_is_visible() {
        let input = acceptance_input();
        let projects = project_report(&input, &BTreeMap::new());
        assert_eq!(projects.rows.len(), 1);
        assert_eq!(projects.rows[0].key, "unassigned");
        assert_eq!(projects.rows[0].label, "Unassigned");
    }

    #[test]
    fn visit_counts_separate_appearances() {
        let mut input = AggregationInput::new(Interval::new(0, 3 * HOUR), 3 * HOUR);
        input.sessions.push(session(0, 3 * HOUR));
        input.segments = vec![
            desktop("Code", 0, 30 * MIN),
            desktop("Slack", 30 * MIN, 40 * MIN),
            desktop("Code", 40 * MIN, HOUR),
        ];
        let report = application_report(&input);
        let code = report.rows.iter().find(|r| r.key == "Code").unwrap();
        assert_eq!(code.visit_count, 2);
    }

    /// A chat application sitting on its own window title says nothing about
    /// the work, but it still says which application the time went to. The
    /// row stays honestly labelled; the application moves into the key, so two
    /// applications no longer collapse into one anonymous row.
    #[test]
    fn unidentified_time_is_grouped_by_application() {
        let mut input = AggregationInput::new(Interval::new(0, HOUR), HOUR);
        input.sessions.push(session(0, HOUR));
        input.segments = vec![
            desktop_titled("Telegram Desktop", "Telegram", 0, 10 * MIN),
            desktop_titled("Telegram Desktop", "Telegram (4)", 10 * MIN, 20 * MIN),
            desktop_titled("Discord", "Discord", 20 * MIN, 25 * MIN),
        ];

        let report = workspace_report(&input);
        let unknown: Vec<_> = report
            .rows
            .iter()
            .filter(|row| row.key.starts_with("UNKNOWN:"))
            .collect();

        assert_eq!(unknown.len(), 2);
        assert!(unknown.iter().all(|row| row.label == "Not identified"));

        let telegram = unknown
            .iter()
            .find(|row| row.key == "UNKNOWN:Telegram Desktop")
            .expect("the Telegram row");
        assert_eq!(telegram.secondary.as_deref(), Some("Telegram Desktop"));
        assert_eq!(telegram.duration_ms, 20 * MIN);

        let discord = unknown
            .iter()
            .find(|row| row.key == "UNKNOWN:Discord")
            .expect("the Discord row");
        assert_eq!(discord.duration_ms, 5 * MIN);

        // The unidentified time is still all there: nothing was dropped to
        // make the rows read better.
        assert_eq!(report.total_ms, 25 * MIN);
    }
}
