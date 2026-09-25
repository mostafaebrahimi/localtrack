use serde::{Deserialize, Serialize};

use super::timeline::{build_timeline, TimelineBlockKind};
use super::AggregationInput;

/// Tuning for context-switch and focus-block analysis (spec §85, §86).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusOptions {
    /// Switches to an activity shorter than this are noise and ignored.
    pub context_switch_noise_ms: i64,
    /// Interruptions shorter than this do not break a focus block.
    pub interruption_tolerance_ms: i64,
}

impl Default for FocusOptions {
    fn default() -> Self {
        Self {
            context_switch_noise_ms: 15_000,
            interruption_tolerance_ms: 120_000,
        }
    }
}

/// Context-switch and focus metrics (spec §87).
///
/// Deliberately free of productivity scoring: LocalTrack reports facts, not
/// judgement (spec §87, §158).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusMetrics {
    pub context_switches: i64,
    pub focus_block_count: i64,
    pub average_focus_ms: i64,
    pub longest_focus_ms: i64,
    pub median_focus_ms: i64,
    pub focus_blocks: Vec<FocusBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusBlock {
    pub start_ms: i64,
    pub end_ms: i64,
    pub duration_ms: i64,
    pub label: String,
    pub category_id: Option<String>,
    pub project_id: Option<String>,
}

/// A focus key groups by project, then category, then activity label.
fn focus_key(project_id: &Option<String>, category_id: &Option<String>, label: &str) -> String {
    match (project_id, category_id) {
        (Some(p), _) => format!("project:{p}"),
        (None, Some(c)) => format!("category:{c}"),
        (None, None) => format!("label:{label}"),
    }
}

pub fn compute_focus(input: &AggregationInput) -> FocusMetrics {
    let blocks: Vec<_> = build_timeline(input)
        .into_iter()
        .filter(|b| {
            matches!(
                b.block_kind,
                TimelineBlockKind::Application
                    | TimelineBlockKind::BrowserPage
                    | TimelineBlockKind::Input
            )
        })
        .collect();

    if blocks.is_empty() {
        return FocusMetrics::default();
    }

    let noise = input.focus.context_switch_noise_ms;
    let tolerance = input.focus.interruption_tolerance_ms;

    // Context switches: a meaningful change of primary activity (spec §85).
    let mut context_switches = 0i64;
    let mut last_meaningful: Option<String> = None;
    for block in &blocks {
        if block.duration_ms < noise {
            continue;
        }
        match &last_meaningful {
            Some(prev) if prev == &block.label => {}
            Some(_) => {
                context_switches += 1;
                last_meaningful = Some(block.label.clone());
            }
            None => last_meaningful = Some(block.label.clone()),
        }
    }

    // Focus blocks: contiguous work on the same project/category, tolerating
    // short interruptions (spec §86).
    let mut focus_blocks: Vec<FocusBlock> = Vec::new();
    for block in &blocks {
        let key = focus_key(&block.project_id, &block.category_id, &block.label);
        match focus_blocks.last_mut() {
            Some(current) => {
                let current_key =
                    focus_key(&current.project_id, &current.category_id, &current.label);
                let gap = block.start_ms - current.end_ms;
                let same = current_key == key;
                let short_interruption = !same && block.duration_ms <= tolerance;
                if gap <= tolerance && (same || short_interruption) {
                    current.end_ms = current.end_ms.max(block.end_ms);
                    current.duration_ms = current.end_ms - current.start_ms;
                    continue;
                }
                focus_blocks.push(FocusBlock {
                    start_ms: block.start_ms,
                    end_ms: block.end_ms,
                    duration_ms: block.duration_ms,
                    label: block.label.clone(),
                    category_id: block.category_id.clone(),
                    project_id: block.project_id.clone(),
                });
            }
            None => focus_blocks.push(FocusBlock {
                start_ms: block.start_ms,
                end_ms: block.end_ms,
                duration_ms: block.duration_ms,
                label: block.label.clone(),
                category_id: block.category_id.clone(),
                project_id: block.project_id.clone(),
            }),
        }
    }

    let mut durations: Vec<i64> = focus_blocks.iter().map(|b| b.duration_ms).collect();
    durations.sort_unstable();
    let count = durations.len() as i64;
    let total: i64 = durations.iter().sum();
    let median = if durations.is_empty() {
        0
    } else if durations.len() % 2 == 1 {
        durations[durations.len() / 2]
    } else {
        (durations[durations.len() / 2 - 1] + durations[durations.len() / 2]) / 2
    };

    FocusMetrics {
        context_switches,
        focus_block_count: count,
        average_focus_ms: if count > 0 { total / count } else { 0 },
        longest_focus_ms: durations.last().copied().unwrap_or(0),
        median_focus_ms: median,
        focus_blocks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{ActivitySegment, SegmentKey};
    use crate::interval::Interval;
    use crate::sessions::WorkSession;

    const MIN: i64 = 60_000;
    const SEC: i64 = 1_000;

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

    fn desktop(app: &str, start: i64, end: i64) -> ActivitySegment {
        ActivitySegment::from_key(
            &SegmentKey::desktop(Some(app.into()), Some("p".into()), Some("w".into())),
            start,
            end,
            0,
        )
    }

    fn input_with(segments: Vec<ActivitySegment>, end: i64) -> AggregationInput {
        let mut input = AggregationInput::new(Interval::new(0, end), end);
        input.sessions.push(session(0, end));
        input.segments = segments;
        input
    }

    #[test]
    fn counts_meaningful_switches_only() {
        let input = input_with(
            vec![
                desktop("Code", 0, 30 * MIN),
                desktop("Slack", 30 * MIN, 30 * MIN + 5 * SEC), // noise
                desktop("Code", 30 * MIN + 5 * SEC, 60 * MIN),
                desktop("Chrome", 60 * MIN, 90 * MIN),
            ],
            90 * MIN,
        );
        let metrics = compute_focus(&input);
        assert_eq!(metrics.context_switches, 1, "5s Slack blip is noise");
    }

    #[test]
    fn short_interruption_does_not_break_focus() {
        let input = input_with(
            vec![
                desktop("Code", 0, 30 * MIN),
                desktop("Slack", 30 * MIN, 31 * MIN),
                desktop("Code", 31 * MIN, 60 * MIN),
            ],
            60 * MIN,
        );
        let metrics = compute_focus(&input);
        assert_eq!(metrics.focus_block_count, 1);
        assert_eq!(metrics.longest_focus_ms, 60 * MIN);
    }

    #[test]
    fn long_switch_starts_a_new_focus_block() {
        let input = input_with(
            vec![
                desktop("Code", 0, 30 * MIN),
                desktop("Slack", 30 * MIN, 50 * MIN),
                desktop("Code", 50 * MIN, 60 * MIN),
            ],
            60 * MIN,
        );
        let metrics = compute_focus(&input);
        assert_eq!(metrics.focus_block_count, 3);
        assert_eq!(metrics.longest_focus_ms, 30 * MIN);
        assert_eq!(metrics.median_focus_ms, 20 * MIN);
    }

    #[test]
    fn empty_input_is_zero() {
        let input = input_with(vec![], 60 * MIN);
        let metrics = compute_focus(&input);
        assert_eq!(metrics, FocusMetrics::default());
    }
}
