use serde::{Deserialize, Serialize};

use super::AggregationInput;
use crate::activity::{ActivityKind, ActivitySegment, ActivitySource};
use crate::interval::{overlay_by_priority, Interval, IntervalSet, OverlayItem};

/// What a timeline block represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TimelineBlockKind {
    Locked,
    Idle,
    BrowserPage,
    Application,
    /// Active input that no application could be attributed to.
    Input,
    Break,
    Untracked,
}

/// One block of the primary timeline (spec §73, §117).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineBlock {
    pub start_ms: i64,
    pub end_ms: i64,
    pub duration_ms: i64,
    pub block_kind: TimelineBlockKind,
    pub label: String,
    pub segment_id: Option<String>,
    pub source: Option<ActivitySource>,
    pub kind: Option<ActivityKind>,
    pub app_name: Option<String>,
    pub process_name: Option<String>,
    pub window_title: Option<String>,
    pub browser: Option<String>,
    pub domain: Option<String>,
    pub url: Option<String>,
    pub page_title: Option<String>,
    pub category_id: Option<String>,
    pub project_id: Option<String>,
}

impl TimelineBlock {
    fn from_segment(segment: &ActivitySegment, interval: Interval) -> Self {
        let block_kind = match segment.kind {
            ActivityKind::Idle => TimelineBlockKind::Idle,
            ActivityKind::Locked => TimelineBlockKind::Locked,
            ActivityKind::BrowserPage => TimelineBlockKind::BrowserPage,
            ActivityKind::Input => TimelineBlockKind::Input,
            _ => TimelineBlockKind::Application,
        };
        Self {
            start_ms: interval.start_ms,
            end_ms: interval.end_ms,
            duration_ms: interval.duration_ms(),
            block_kind,
            label: segment.label(),
            segment_id: Some(segment.id.clone()),
            source: Some(segment.source),
            kind: Some(segment.kind),
            app_name: segment.app_name.clone(),
            process_name: segment.process_name.clone(),
            window_title: segment.window_title.clone(),
            browser: segment.browser.clone(),
            domain: segment.domain.clone(),
            url: segment.url.clone(),
            page_title: segment.page_title.clone(),
            category_id: segment.category_id.clone(),
            project_id: segment.project_id.clone(),
        }
    }

    fn synthetic(kind: TimelineBlockKind, label: &str, interval: Interval) -> Self {
        Self {
            start_ms: interval.start_ms,
            end_ms: interval.end_ms,
            duration_ms: interval.duration_ms(),
            block_kind: kind,
            label: label.to_string(),
            segment_id: None,
            source: None,
            kind: None,
            app_name: None,
            process_name: None,
            window_title: None,
            browser: None,
            domain: None,
            url: None,
            page_title: None,
            category_id: None,
            project_id: None,
        }
    }
}

/// Build the primary timeline (spec §117).
///
/// Priority: system lock / AFK → browser page → desktop application →
/// untracked. Browser detail replaces the generic "Chrome" application block so
/// the timeline reads `GitHub, ChatGPT, Gmail` instead of `Chrome + GitHub`
/// (spec §42), while no millisecond is ever counted twice.
pub fn build_timeline(input: &AggregationInput) -> Vec<TimelineBlock> {
    let window = input.window;

    let mut inactive: Vec<OverlayItem<ActivitySegment>> = Vec::new();
    let mut browser: Vec<OverlayItem<ActivitySegment>> = Vec::new();
    let mut desktop: Vec<OverlayItem<ActivitySegment>> = Vec::new();
    // Input with no window detail is the weakest evidence, so it only fills
    // time nothing better covers.
    let mut unattributed: Vec<OverlayItem<ActivitySegment>> = Vec::new();

    for segment in &input.segments {
        let Some(clipped) = segment.interval().clip(&window) else {
            continue;
        };
        if clipped.is_empty() {
            continue;
        }
        let item = OverlayItem::new(clipped, segment.clone());
        match segment.kind {
            ActivityKind::Idle | ActivityKind::Locked => inactive.push(item),
            ActivityKind::BrowserPage => browser.push(item),
            ActivityKind::Input => unattributed.push(item),
            ActivityKind::Interaction => {}
            _ => desktop.push(item),
        }
    }

    let overlaid = overlay_by_priority(vec![inactive, browser, desktop, unattributed]);

    // A break is never activity: it wins over every observed segment, so a
    // collector that kept running during a break cannot fill it with work.
    let break_intervals = input.break_intervals();
    let mut blocks: Vec<TimelineBlock> = Vec::with_capacity(overlaid.len());
    for item in overlaid {
        for piece in break_intervals.subtract_interval(item.interval) {
            blocks.push(TimelineBlock::from_segment(&item.payload, piece));
        }
    }

    let covered =
        IntervalSet::from_intervals(blocks.iter().map(|b| Interval::new(b.start_ms, b.end_ms)));
    for brk in break_intervals.iter() {
        blocks.push(TimelineBlock::synthetic(
            TimelineBlockKind::Break,
            "Break",
            *brk,
        ));
    }

    // Untracked = time inside work that no collector covered (spec §17).
    let work = input.work_intervals();
    let untracked = work.subtract(&covered).subtract(&break_intervals);
    for gap in untracked.iter() {
        blocks.push(TimelineBlock::synthetic(
            TimelineBlockKind::Untracked,
            "Untracked",
            *gap,
        ));
    }

    blocks.sort_by_key(|b| (b.start_ms, b.end_ms));
    blocks
}

/// Merge consecutive timeline blocks that carry the same label and kind, so the
/// UI does not render hundreds of one-second slivers.
pub fn compact_timeline(blocks: Vec<TimelineBlock>, max_gap_ms: i64) -> Vec<TimelineBlock> {
    let mut out: Vec<TimelineBlock> = Vec::with_capacity(blocks.len());
    for block in blocks {
        match out.last_mut() {
            Some(last)
                if last.block_kind == block.block_kind
                    && last.label == block.label
                    && last.url == block.url
                    && last.window_title == block.window_title
                    && block.start_ms - last.end_ms <= max_gap_ms =>
            {
                last.end_ms = last.end_ms.max(block.end_ms);
                last.duration_ms = last.end_ms - last.start_ms;
            }
            _ => out.push(block),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::SegmentKey;
    use crate::sessions::WorkSession;

    const MIN: i64 = 60_000;

    fn seg(key: SegmentKey, start: i64, end: i64) -> ActivitySegment {
        ActivitySegment::from_key(&key, start, end, 0)
    }

    fn desktop(app: &str, start: i64, end: i64) -> ActivitySegment {
        seg(
            SegmentKey::desktop(
                Some(app.into()),
                Some(format!("{app}.exe")),
                Some("w".into()),
            ),
            start,
            end,
        )
    }

    fn page(domain: &str, start: i64, end: i64) -> ActivitySegment {
        seg(
            SegmentKey::browser_page(
                Some("chrome".into()),
                Some(domain.into()),
                Some(format!("https://{domain}/x")),
                Some(domain.into()),
            ),
            start,
            end,
        )
    }

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

    #[test]
    fn browser_detail_replaces_chrome_block() {
        // Spec §42.
        let mut input = AggregationInput::new(Interval::new(0, 30 * MIN), 30 * MIN);
        input.sessions.push(session(0, 30 * MIN));
        input.segments = vec![
            desktop("Chrome", 0, 30 * MIN),
            page("github.com", 0, 12 * MIN),
            page("chatgpt.com", 12 * MIN, 20 * MIN),
            page("gmail.com", 20 * MIN, 30 * MIN),
        ];
        let blocks = build_timeline(&input);
        let labels: Vec<&str> = blocks.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, vec!["github.com", "chatgpt.com", "gmail.com"]);
        let total: i64 = blocks.iter().map(|b| b.duration_ms).sum();
        assert_eq!(total, 30 * MIN, "no double counting");
    }

    #[test]
    fn idle_overrides_everything() {
        let mut input = AggregationInput::new(Interval::new(0, 20 * MIN), 20 * MIN);
        input.sessions.push(session(0, 20 * MIN));
        input.segments = vec![
            desktop("Chrome", 0, 20 * MIN),
            page("github.com", 0, 20 * MIN),
            seg(SegmentKey::idle(), 10 * MIN, 15 * MIN),
        ];
        let blocks = build_timeline(&input);
        let idle: Vec<&TimelineBlock> = blocks
            .iter()
            .filter(|b| b.block_kind == TimelineBlockKind::Idle)
            .collect();
        assert_eq!(idle.len(), 1);
        assert_eq!(idle[0].duration_ms, 5 * MIN);
        let total: i64 = blocks.iter().map(|b| b.duration_ms).sum();
        assert_eq!(total, 20 * MIN);
    }

    #[test]
    fn gaps_inside_work_become_untracked() {
        let mut input = AggregationInput::new(Interval::new(0, 60 * MIN), 60 * MIN);
        input.sessions.push(session(0, 60 * MIN));
        input.segments = vec![
            desktop("Code", 0, 20 * MIN),
            desktop("Code", 40 * MIN, 60 * MIN),
        ];
        let blocks = build_timeline(&input);
        let untracked: i64 = blocks
            .iter()
            .filter(|b| b.block_kind == TimelineBlockKind::Untracked)
            .map(|b| b.duration_ms)
            .sum();
        assert_eq!(untracked, 20 * MIN);
    }

    #[test]
    fn compaction_joins_identical_neighbours() {
        let blocks = vec![
            TimelineBlock::synthetic(
                TimelineBlockKind::Untracked,
                "Untracked",
                Interval::new(0, 10),
            ),
            TimelineBlock::synthetic(
                TimelineBlockKind::Untracked,
                "Untracked",
                Interval::new(10, 20),
            ),
        ];
        let out = compact_timeline(blocks, 0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].duration_ms, 20);
    }
}
