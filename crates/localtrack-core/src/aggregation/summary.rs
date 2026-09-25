use serde::{Deserialize, Serialize};

use super::{focus, AggregationInput};
use crate::interval::{Interval, IntervalSet};
use crate::time;

/// Duration metrics for a period (spec §17, §71, §91).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub from_ms: i64,
    pub to_ms: i64,

    /// Clock out − clock in, breaks included.
    pub clocked_ms: i64,
    /// Sum of break intervals.
    pub break_ms: i64,
    /// Clocked − break.
    pub work_ms: i64,
    /// Work time with a real foreground activity and the user present.
    pub active_ms: i64,
    /// Work time where the user was AFK or the machine was locked.
    pub idle_ms: i64,
    /// Work − active − idle. Gaps never silently become active time.
    pub untracked_ms: i64,
    /// The part of the untracked time when LocalTrack was not running at all.
    ///
    /// Reported separately because "the tracker was off" and "you were clocked
    /// in and nothing happened" are different facts about a day.
    pub untracked_agent_off_ms: i64,

    pub context_switches: i64,
    pub average_focus_ms: i64,
    pub longest_focus_ms: i64,
    pub median_focus_ms: i64,

    pub session_count: i64,
}

/// Compute every duration metric from the same interval algebra.
pub fn compute_summary(input: &AggregationInput) -> Summary {
    let running = input.agent_running.clone();
    compute_summary_with_uptime(input, running.as_ref())
}

/// Compute a summary, attributing untracked time to the agent being off where
/// its uptime is known.
pub fn compute_summary_with_uptime(
    input: &AggregationInput,
    agent_running: Option<&IntervalSet>,
) -> Summary {
    let clocked = input.clocked_intervals();
    let breaks = input.break_intervals();
    let work = input.work_intervals();
    let idle = input.inactive_intervals().intersection(&work);

    let active_window = input.active_window();
    let covered =
        IntervalSet::from_intervals(input.foreground_segments().iter().map(|c| c.interval))
            .intersection(&active_window);

    let active_ms = covered.duration_ms();
    let idle_ms = idle.duration_ms();
    let work_ms = work.duration_ms();
    let untracked = work.subtract(&covered).subtract(&idle);
    let untracked_ms = untracked.duration_ms();
    let untracked_agent_off_ms = agent_running
        .map(|running| untracked.subtract(running).duration_ms())
        .unwrap_or(0);

    let focus_metrics = focus::compute_focus(input);

    Summary {
        from_ms: input.window.start_ms,
        to_ms: input.window.end_ms,
        clocked_ms: clocked.duration_ms(),
        break_ms: breaks.duration_ms(),
        work_ms,
        active_ms,
        idle_ms,
        untracked_ms,
        untracked_agent_off_ms,
        context_switches: focus_metrics.context_switches,
        average_focus_ms: focus_metrics.average_focus_ms,
        longest_focus_ms: focus_metrics.longest_focus_ms,
        median_focus_ms: focus_metrics.median_focus_ms,
        session_count: input.sessions.len() as i64,
    }
}

/// One local calendar day of metrics (spec §92).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaySummary {
    pub date: String,
    pub first_clock_in_ms: Option<i64>,
    pub last_clock_out_ms: Option<i64>,
    #[serde(flatten)]
    pub summary: Summary,
}

/// Split a period into local days and summarize each one.
///
/// Local days are derived with DST-aware boundaries, so a day is never assumed
/// to be exactly 24 hours (spec §136).
pub fn compute_daily_summaries(input: &AggregationInput) -> Vec<DaySummary> {
    let mut out = Vec::new();
    for (date, day_start, day_end) in
        time::local_days_in_range(input.window.start_ms, input.window.end_ms)
    {
        let window = Interval::new(
            day_start.max(input.window.start_ms),
            day_end.min(input.window.end_ms),
        );
        if window.is_empty() {
            continue;
        }
        let day_input = input.clipped_to(window);

        let clocked = day_input.clocked_intervals();
        let first_clock_in_ms = clocked.iter().map(|i| i.start_ms).min();
        let last_clock_out_ms = clocked.iter().map(|i| i.end_ms).max();

        out.push(DaySummary {
            date,
            first_clock_in_ms,
            last_clock_out_ms,
            summary: compute_summary(&day_input),
        });
    }
    out
}

/// One local week of metrics, with the breakdowns that answer "where did the
/// week go" without opening each day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekSummary {
    /// ISO week, e.g. `2026-W34`.
    pub week: String,
    pub starts_at_ms: i64,
    pub ends_at_ms: i64,
    pub summary: Summary,
    pub days: Vec<DaySummary>,
    /// Time per application over the whole week.
    pub applications: super::ReportSet,
    /// Time per website over the whole week.
    pub websites: super::ReportSet,
    /// Time per page address over the whole week.
    pub pages: super::ReportSet,
}

/// Split a period into local weeks and summarize each one.
///
/// Weeks start on Monday and are whole local weeks, so a week containing a
/// daylight-saving change is still seven days rather than 168 hours.
pub fn compute_weekly_summaries(input: &AggregationInput) -> Vec<WeekSummary> {
    let mut out = Vec::new();
    for (week, week_start, week_end) in
        time::local_weeks_in_range(input.window.start_ms, input.window.end_ms)
    {
        let window = Interval::new(
            week_start.max(input.window.start_ms),
            week_end.min(input.window.end_ms),
        );
        if window.is_empty() {
            continue;
        }

        let week_input = input.clipped_to(window);
        out.push(WeekSummary {
            week,
            starts_at_ms: window.start_ms,
            ends_at_ms: window.end_ms,
            summary: compute_summary(&week_input),
            days: compute_daily_summaries(&week_input),
            applications: super::reports::application_report(&week_input),
            websites: super::reports::website_report(&week_input),
            pages: super::reports::page_report(&week_input, None),
        });
    }
    out
}

/// A period-over-period comparison (spec §84).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryComparison {
    pub current: Summary,
    pub previous: Summary,
    pub clocked_delta_ms: i64,
    pub work_delta_ms: i64,
    pub active_delta_ms: i64,
    pub idle_delta_ms: i64,
    pub break_delta_ms: i64,
}

pub fn compare_summaries(current: Summary, previous: Summary) -> SummaryComparison {
    SummaryComparison {
        clocked_delta_ms: current.clocked_ms - previous.clocked_ms,
        work_delta_ms: current.work_ms - previous.work_ms,
        active_delta_ms: current.active_ms - previous.active_ms,
        idle_delta_ms: current.idle_ms - previous.idle_ms,
        break_delta_ms: current.break_ms - previous.break_ms,
        current,
        previous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{ActivitySegment, SegmentKey};
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

    fn page(domain: &str, start: i64, end: i64) -> ActivitySegment {
        ActivitySegment::from_key(
            &SegmentKey::browser_page(
                Some("chrome".into()),
                Some(domain.into()),
                Some(format!("https://{domain}/x")),
                Some(domain.into()),
            ),
            start,
            end,
            0,
        )
    }

    /// Spec §163 core acceptance test.
    fn acceptance_input() -> AggregationInput {
        let mut input = AggregationInput::new(Interval::new(0, 3 * HOUR), 3 * HOUR);
        input.sessions.push(session(0, 3 * HOUR));
        input.breaks.push(brk(2 * HOUR, 2 * HOUR + 15 * MIN));
        input.segments = vec![
            desktop("VS Code", 0, HOUR),
            desktop("Chrome", HOUR, HOUR + 30 * MIN),
            page("github.com", HOUR, HOUR + 30 * MIN),
            ActivitySegment::from_key(&SegmentKey::idle(), HOUR + 30 * MIN, HOUR + 40 * MIN, 0),
            desktop("Chrome", HOUR + 40 * MIN, 2 * HOUR),
            page("chatgpt.com", HOUR + 40 * MIN, 2 * HOUR),
            desktop("VS Code", 2 * HOUR + 15 * MIN, 3 * HOUR),
        ];
        input
    }

    #[test]
    fn core_acceptance_metrics() {
        let summary = compute_summary(&acceptance_input());
        assert_eq!(summary.clocked_ms, 3 * HOUR, "clocked");
        assert_eq!(summary.break_ms, 15 * MIN, "break");
        assert_eq!(summary.work_ms, 2 * HOUR + 45 * MIN, "work");
        assert_eq!(summary.idle_ms, 10 * MIN, "idle");
        assert_eq!(summary.active_ms, 2 * HOUR + 35 * MIN, "active");
        assert_eq!(summary.untracked_ms, 0, "untracked");
        // Work must equal active + idle + untracked exactly.
        assert_eq!(
            summary.work_ms,
            summary.active_ms + summary.idle_ms + summary.untracked_ms
        );
    }

    #[test]
    fn overlapping_browser_and_desktop_never_double_count() {
        let summary = compute_summary(&acceptance_input());
        // Chrome desktop 50m + GitHub 30m + ChatGPT 20m would be 100m if summed.
        // Active time must stay at the real elapsed value.
        assert_eq!(summary.active_ms, 2 * HOUR + 35 * MIN);
    }

    #[test]
    fn untracked_time_says_whether_the_tracker_was_even_running() {
        // Clocked in all morning; LocalTrack was restarted and missed an hour.
        let mut input = AggregationInput::new(Interval::new(0, 4 * HOUR), 4 * HOUR);
        input.sessions.push(session(0, 4 * HOUR));
        input.segments = vec![desktop("VS Code", 0, HOUR)];
        input.agent_running = Some(
            [Interval::new(0, HOUR), Interval::new(2 * HOUR, 4 * HOUR)]
                .into_iter()
                .collect(),
        );

        let summary = compute_summary(&input);
        assert_eq!(summary.active_ms, HOUR);
        assert_eq!(summary.untracked_ms, 3 * HOUR);
        assert_eq!(
            summary.untracked_agent_off_ms, HOUR,
            "one hour of the gap is explained: LocalTrack was not running"
        );

        // Without uptime information the figure is zero rather than a guess.
        input.agent_running = None;
        assert_eq!(compute_summary(&input).untracked_agent_off_ms, 0);
    }

    #[test]
    fn crash_gap_is_untracked_not_active() {
        let mut input = AggregationInput::new(Interval::new(0, HOUR), HOUR);
        input.sessions.push(session(0, HOUR));
        input.segments = vec![desktop("VS Code", 0, 20 * MIN)];
        let summary = compute_summary(&input);
        assert_eq!(summary.active_ms, 20 * MIN);
        assert_eq!(summary.untracked_ms, 40 * MIN);
        assert_eq!(summary.idle_ms, 0);
    }

    #[test]
    fn activity_outside_session_is_not_counted_when_sessions_exist() {
        let mut input = AggregationInput::new(Interval::new(0, 2 * HOUR), 2 * HOUR);
        input.sessions.push(session(0, HOUR));
        input.segments = vec![desktop("VS Code", 0, 2 * HOUR)];
        let summary = compute_summary(&input);
        assert_eq!(summary.clocked_ms, HOUR);
        assert_eq!(summary.active_ms, HOUR);
    }

    #[test]
    fn open_session_is_bounded_by_now() {
        let mut input = AggregationInput::new(Interval::new(0, 10 * HOUR), 2 * HOUR);
        let mut open = session(0, 0);
        open.ended_at_ms = None;
        input.sessions.push(open);
        input.segments = vec![desktop("VS Code", 0, 2 * HOUR)];
        let summary = compute_summary(&input);
        assert_eq!(summary.clocked_ms, 2 * HOUR);
    }

    #[test]
    fn daily_split_produces_one_row_per_local_day() {
        let start = time::local_day_start_ms(time::now_ms());
        let mut input = AggregationInput::new(
            Interval::new(start, start + 2 * time::DAY_MS),
            start + 2 * time::DAY_MS,
        );
        input.sessions.push(session(start, start + HOUR));
        input.segments = vec![desktop("VS Code", start, start + HOUR)];
        let days = compute_daily_summaries(&input);
        assert!(days.len() >= 2);
        assert_eq!(days[0].summary.active_ms, HOUR);
        assert_eq!(days[1].summary.active_ms, 0);
        assert!(days[0].first_clock_in_ms.is_some());
    }

    #[test]
    fn a_week_aggregates_days_applications_and_addresses() {
        let week_start = time::local_week_start_ms(time::now_ms());
        let mut input = AggregationInput::new(
            Interval::new(week_start, week_start + 7 * time::DAY_MS),
            week_start + 7 * time::DAY_MS,
        );

        // Two working days in the same week.
        for day in [0_i64, 1] {
            let start = time::local_day_start_ms(week_start + day * time::DAY_MS + 12 * HOUR);
            input
                .sessions
                .push(session(start + 9 * HOUR, start + 12 * HOUR));
            input
                .segments
                .push(desktop("VS Code", start + 9 * HOUR, start + 10 * HOUR));
            input
                .segments
                .push(desktop("Chrome", start + 10 * HOUR, start + 12 * HOUR));
            input
                .segments
                .push(page("github.com", start + 10 * HOUR, start + 11 * HOUR));
            input
                .segments
                .push(page("chatgpt.com", start + 11 * HOUR, start + 12 * HOUR));
        }

        let weeks = compute_weekly_summaries(&input);
        assert_eq!(weeks.len(), 1, "one week");
        let week = &weeks[0];
        assert!(week.week.contains("-W"));
        assert_eq!(week.summary.active_ms, 6 * HOUR, "both days together");
        assert_eq!(week.days.len(), 7, "every day of the week is listed");

        let app = |key: &str| {
            week.applications
                .rows
                .iter()
                .find(|row| row.key == key)
                .map(|row| row.duration_ms)
                .unwrap_or(0)
        };
        assert_eq!(app("VS Code"), 2 * HOUR, "one hour on each of the two days");
        assert_eq!(app("Chrome"), 4 * HOUR, "two hours on each of the two days");
        assert_eq!(
            week.applications.total_ms, week.summary.active_ms,
            "applications account for the active time exactly once"
        );

        let site = |key: &str| {
            week.websites
                .rows
                .iter()
                .find(|row| row.key == key)
                .map(|row| row.duration_ms)
                .unwrap_or(0)
        };
        assert_eq!(site("github.com"), 2 * HOUR);
        assert_eq!(site("chatgpt.com"), 2 * HOUR);

        // Page addresses are aggregated for the week too.
        assert_eq!(week.pages.rows.len(), 2);
        assert!(week
            .pages
            .rows
            .iter()
            .all(|row| row.key.starts_with("https://")));
    }

    #[test]
    fn weeks_do_not_bleed_into_each_other() {
        let week_start = time::local_week_start_ms(time::now_ms());
        let previous = time::local_week_start_ms(week_start - time::DAY_MS);
        let mut input = AggregationInput::new(
            Interval::new(previous, week_start + 7 * time::DAY_MS),
            week_start + 7 * time::DAY_MS,
        );

        let last_week_day = time::local_day_start_ms(previous + 12 * HOUR);
        input
            .sessions
            .push(session(last_week_day + 9 * HOUR, last_week_day + 10 * HOUR));
        input.segments.push(desktop(
            "Slack",
            last_week_day + 9 * HOUR,
            last_week_day + 10 * HOUR,
        ));

        let this_week_day = time::local_day_start_ms(week_start + 12 * HOUR);
        input
            .sessions
            .push(session(this_week_day + 9 * HOUR, this_week_day + 11 * HOUR));
        input.segments.push(desktop(
            "VS Code",
            this_week_day + 9 * HOUR,
            this_week_day + 11 * HOUR,
        ));

        let weeks = compute_weekly_summaries(&input);
        assert_eq!(weeks.len(), 2);
        assert_eq!(weeks[0].summary.active_ms, HOUR);
        assert_eq!(weeks[1].summary.active_ms, 2 * HOUR);
        assert_eq!(weeks[0].applications.rows[0].key, "Slack");
        assert_eq!(weeks[1].applications.rows[0].key, "VS Code");
    }

    #[test]
    fn comparison_deltas() {
        let current = Summary {
            active_ms: 100,
            ..Default::default()
        };
        let previous = Summary {
            active_ms: 60,
            ..Default::default()
        };
        let cmp = compare_summaries(current, previous);
        assert_eq!(cmp.active_delta_ms, 40);
    }
}
