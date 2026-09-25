//! Aggregation: primary timeline, duration metrics, reports and focus analysis
//! (spec §17, §42, §71, §75–§87, §117–§119).
//!
//! Everything is derived from the same interval algebra, so an application
//! report and a website report are alternative dimensions over the *same*
//! intervals and can never sum to more time than actually elapsed.

pub mod focus;
pub mod reports;
pub mod summary;
pub mod timeline;

pub use focus::{FocusMetrics, FocusOptions};
pub use reports::{ReportDimension, ReportRow, ReportSet};
pub use summary::{DaySummary, Summary, WeekSummary};
pub use timeline::{TimelineBlock, TimelineBlockKind};

use serde::{Deserialize, Serialize};

use crate::activity::{ActivityKind, ActivitySegment};
use crate::interval::{Interval, IntervalSet};
use crate::sessions::{WorkBreak, WorkSession};

/// Everything the aggregation engine needs. Callers load it once per query.
#[derive(Debug, Clone)]
pub struct AggregationInput {
    /// The reporting window (UTC ms, half-open).
    pub window: Interval,
    pub segments: Vec<ActivitySegment>,
    pub sessions: Vec<WorkSession>,
    pub breaks: Vec<WorkBreak>,
    /// "Now", used to bound still-open sessions and breaks.
    pub now_ms: i64,
    pub focus: FocusOptions,
    /// When LocalTrack itself was running, where that is known.
    pub agent_running: Option<IntervalSet>,
}

impl AggregationInput {
    pub fn new(window: Interval, now_ms: i64) -> Self {
        Self {
            window,
            segments: Vec::new(),
            sessions: Vec::new(),
            breaks: Vec::new(),
            now_ms,
            focus: FocusOptions::default(),
            agent_running: None,
        }
    }

    /// The same data narrowed to a sub-window, for per-day and per-week views.
    pub fn clipped_to(&self, window: Interval) -> AggregationInput {
        AggregationInput {
            window,
            segments: self
                .segments
                .iter()
                .filter(|segment| segment.interval().overlaps(&window))
                .cloned()
                .collect(),
            sessions: self
                .sessions
                .iter()
                .filter(|session| session.interval(self.now_ms).overlaps(&window))
                .cloned()
                .collect(),
            breaks: self
                .breaks
                .iter()
                .filter(|brk| brk.interval(self.now_ms).overlaps(&window))
                .cloned()
                .collect(),
            now_ms: self.now_ms,
            focus: self.focus,
            agent_running: self
                .agent_running
                .as_ref()
                .map(|running| running.clip(&window)),
        }
    }

    /// Session intervals clipped to the window (spec §17 clocked duration).
    pub fn clocked_intervals(&self) -> IntervalSet {
        IntervalSet::from_intervals(self.sessions.iter().map(|s| s.interval(self.now_ms)))
            .clip(&self.window)
    }

    /// Break intervals, clipped to sessions and the window.
    pub fn break_intervals(&self) -> IntervalSet {
        let breaks =
            IntervalSet::from_intervals(self.breaks.iter().map(|b| b.interval(self.now_ms)));
        breaks.intersection(&self.clocked_intervals())
    }

    /// Work intervals = clocked − breaks.
    pub fn work_intervals(&self) -> IntervalSet {
        self.clocked_intervals().subtract(&self.break_intervals())
    }

    /// Intervals where the user was idle or the machine was locked.
    pub fn inactive_intervals(&self) -> IntervalSet {
        IntervalSet::from_intervals(
            self.segments
                .iter()
                .filter(|s| s.kind.is_inactive())
                .map(|s| s.interval()),
        )
        .clip(&self.window)
    }

    /// The window in which foreground activity may be counted as active:
    /// inside work, not on break, not idle/locked.
    ///
    /// When no work session covers the window (tracking scope `ALWAYS` or a
    /// range with no sessions), the whole window is eligible so that recorded
    /// activity is still reported.
    pub fn active_window(&self) -> IntervalSet {
        let work = self.work_intervals();
        let base = if work.is_empty() && self.sessions.is_empty() {
            IntervalSet::single(self.window)
        } else {
            work
        };
        base.subtract(&self.inactive_intervals())
    }

    /// Foreground (non-idle) segments clipped to the active window.
    pub fn foreground_segments(&self) -> Vec<ClippedSegment> {
        let active = self.active_window();
        let mut out = Vec::new();
        for segment in &self.segments {
            if segment.kind.is_inactive() {
                continue;
            }
            if segment.kind == ActivityKind::Interaction {
                continue;
            }
            for piece in active.clip_interval(segment.interval()) {
                out.push(ClippedSegment {
                    segment: segment.clone(),
                    interval: piece,
                });
            }
        }
        out.sort_by_key(|c| (c.interval.start_ms, c.interval.end_ms));
        out
    }
}

/// A segment clipped into a reporting window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClippedSegment {
    pub segment: ActivitySegment,
    pub interval: Interval,
}

impl ClippedSegment {
    pub fn duration_ms(&self) -> i64 {
        self.interval.duration_ms()
    }
}
