//! Clock state machine, work sessions and breaks (spec §14–§20, §114).

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::interval::{Interval, IntervalSet};

/// The three allowed clock states (spec §14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClockState {
    ClockedOut,
    ClockedIn,
    OnBreak,
}

impl ClockState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClockState::ClockedOut => "CLOCKED_OUT",
            ClockState::ClockedIn => "CLOCKED_IN",
            ClockState::OnBreak => "ON_BREAK",
        }
    }

    /// Activity may be persisted in `WORK_SESSIONS_ONLY` scope only while
    /// clocked in and not on a break (spec §18).
    pub fn is_tracking_allowed(&self) -> bool {
        matches!(self, ClockState::ClockedIn)
    }
}

/// Transitions requested by the user or the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClockCommand {
    ClockIn,
    ClockOut,
    StartBreak,
    EndBreak,
}

impl ClockCommand {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClockCommand::ClockIn => "CLOCK_IN",
            ClockCommand::ClockOut => "CLOCK_OUT",
            ClockCommand::StartBreak => "START_BREAK",
            ClockCommand::EndBreak => "END_BREAK",
        }
    }
}

/// Apply a command to a state. Invalid transitions are rejected (spec §14).
pub fn transition(state: ClockState, command: ClockCommand) -> Result<ClockState> {
    use ClockCommand::*;
    use ClockState::*;
    match (state, command) {
        (ClockedOut, ClockIn) => Ok(ClockedIn),
        (ClockedIn, ClockOut) => Ok(ClockedOut),
        (ClockedIn, StartBreak) => Ok(OnBreak),
        (OnBreak, EndBreak) => Ok(ClockedIn),
        // Clocking out during a break is allowed: the break is closed first.
        (OnBreak, ClockOut) => Ok(ClockedOut),
        (current, cmd) => Err(CoreError::InvalidClockTransition(format!(
            "cannot {} while {}",
            cmd.as_str(),
            current.as_str()
        ))),
    }
}

/// A work session (spec §15).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkSession {
    pub id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub start_timezone_offset_min: i32,
    pub end_timezone_offset_min: Option<i32>,
    pub note: Option<String>,
    pub created_manually: bool,
    pub edited_manually: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl WorkSession {
    pub fn is_open(&self) -> bool {
        self.ended_at_ms.is_none()
    }

    /// Interval of the session, bounded by `now_ms` while still open.
    pub fn interval(&self, now_ms: i64) -> Interval {
        Interval::new(self.started_at_ms, self.ended_at_ms.unwrap_or(now_ms))
    }

    /// Clocked duration = clock out − clock in, breaks included (spec §17).
    pub fn clocked_duration_ms(&self, now_ms: i64) -> i64 {
        self.interval(now_ms).duration_ms()
    }
}

/// A break inside a session (spec §16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkBreak {
    pub id: String,
    pub work_session_id: String,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub note: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl WorkBreak {
    pub fn is_open(&self) -> bool {
        self.ended_at_ms.is_none()
    }

    pub fn interval(&self, now_ms: i64) -> Interval {
        Interval::new(self.started_at_ms, self.ended_at_ms.unwrap_or(now_ms))
    }
}

/// The complete clock state as shown by the dashboard, tray and popup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClockSnapshot {
    pub state: ClockState,
    pub session: Option<WorkSession>,
    pub open_break: Option<WorkBreak>,
    pub breaks: Vec<WorkBreak>,
}

impl ClockSnapshot {
    pub fn clocked_out() -> Self {
        Self {
            state: ClockState::ClockedOut,
            session: None,
            open_break: None,
            breaks: Vec::new(),
        }
    }

    /// Derive the state from persisted rows; this is how crash recovery
    /// restores the clock (spec §20).
    pub fn derive(session: Option<WorkSession>, breaks: Vec<WorkBreak>) -> Self {
        match session {
            Some(session) if session.is_open() => {
                let open_break = breaks.iter().find(|b| b.is_open()).cloned();
                let state = if open_break.is_some() {
                    ClockState::OnBreak
                } else {
                    ClockState::ClockedIn
                };
                Self {
                    state,
                    session: Some(session),
                    open_break,
                    breaks,
                }
            }
            _ => ClockSnapshot::clocked_out(),
        }
    }

    /// Intervals covered by breaks, clipped to the session.
    pub fn break_intervals(&self, now_ms: i64) -> IntervalSet {
        let Some(session) = &self.session else {
            return IntervalSet::empty();
        };
        let bounds = session.interval(now_ms);
        IntervalSet::from_intervals(self.breaks.iter().map(|b| b.interval(now_ms))).clip(&bounds)
    }

    /// Work intervals = session minus breaks (spec §17).
    pub fn work_intervals(&self, now_ms: i64) -> IntervalSet {
        let Some(session) = &self.session else {
            return IntervalSet::empty();
        };
        IntervalSet::single(session.interval(now_ms)).subtract(&self.break_intervals(now_ms))
    }
}

/// Validation for manual session editing (spec §114).
pub fn validate_session_edit(
    started_at_ms: i64,
    ended_at_ms: Option<i64>,
    breaks: &[(i64, Option<i64>)],
) -> Result<()> {
    if let Some(end) = ended_at_ms {
        if end <= started_at_ms {
            return Err(CoreError::Validation(
                "clock out must be after clock in".into(),
            ));
        }
    }
    let session_end = ended_at_ms.unwrap_or(i64::MAX);

    let mut sorted: Vec<(i64, i64)> = Vec::new();
    for (start, end) in breaks {
        let end_value = end.unwrap_or(session_end);
        if end.is_some() && end_value <= *start {
            return Err(CoreError::Validation(
                "break end must be after break start".into(),
            ));
        }
        if *start < started_at_ms || end_value > session_end {
            return Err(CoreError::Validation(
                "breaks must be inside the work session".into(),
            ));
        }
        sorted.push((*start, end_value));
    }

    sorted.sort_by_key(|(s, _)| *s);
    for pair in sorted.windows(2) {
        if pair[1].0 < pair[0].1 {
            return Err(CoreError::Validation("breaks must not overlap".into()));
        }
    }
    Ok(())
}

/// An open session older than this looks suspicious after a crash (spec §20).
pub const STALE_SESSION_WARNING_MS: i64 = 16 * 60 * 60 * 1000;

pub fn session_looks_stale(session: &WorkSession, now_ms: i64) -> bool {
    session.is_open() && now_ms - session.started_at_ms > STALE_SESSION_WARNING_MS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(start: i64, end: Option<i64>) -> WorkSession {
        WorkSession {
            id: "s1".into(),
            started_at_ms: start,
            ended_at_ms: end,
            start_timezone_offset_min: 0,
            end_timezone_offset_min: None,
            note: None,
            created_manually: false,
            edited_manually: false,
            created_at_ms: start,
            updated_at_ms: start,
        }
    }

    fn brk(start: i64, end: Option<i64>) -> WorkBreak {
        WorkBreak {
            id: format!("b{start}"),
            work_session_id: "s1".into(),
            started_at_ms: start,
            ended_at_ms: end,
            note: None,
            created_at_ms: start,
            updated_at_ms: start,
        }
    }

    #[test]
    fn valid_transitions() {
        use ClockCommand::*;
        use ClockState::*;
        assert_eq!(transition(ClockedOut, ClockIn).unwrap(), ClockedIn);
        assert_eq!(transition(ClockedIn, StartBreak).unwrap(), OnBreak);
        assert_eq!(transition(OnBreak, EndBreak).unwrap(), ClockedIn);
        assert_eq!(transition(ClockedIn, ClockOut).unwrap(), ClockedOut);
        assert_eq!(transition(OnBreak, ClockOut).unwrap(), ClockedOut);
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        use ClockCommand::*;
        use ClockState::*;
        assert!(transition(ClockedOut, ClockOut).is_err());
        assert!(transition(ClockedIn, ClockIn).is_err());
        assert!(transition(ClockedOut, StartBreak).is_err());
        assert!(transition(OnBreak, StartBreak).is_err());
        assert!(transition(ClockedIn, EndBreak).is_err());
        assert!(transition(ClockedOut, EndBreak).is_err());
    }

    #[test]
    fn crash_recovery_restores_clocked_in() {
        let snapshot = ClockSnapshot::derive(Some(session(0, None)), vec![]);
        assert_eq!(snapshot.state, ClockState::ClockedIn);
    }

    #[test]
    fn crash_recovery_restores_on_break() {
        let snapshot = ClockSnapshot::derive(Some(session(0, None)), vec![brk(100, None)]);
        assert_eq!(snapshot.state, ClockState::OnBreak);
        assert!(snapshot.open_break.is_some());
    }

    #[test]
    fn closed_session_means_clocked_out() {
        let snapshot = ClockSnapshot::derive(Some(session(0, Some(10))), vec![brk(2, Some(4))]);
        assert_eq!(snapshot.state, ClockState::ClockedOut);
        assert!(snapshot.session.is_none());
    }

    #[test]
    fn work_intervals_exclude_breaks() {
        let snapshot = ClockSnapshot::derive(Some(session(0, None)), vec![brk(100, Some(200))]);
        let work = snapshot.work_intervals(500);
        assert_eq!(work.duration_ms(), 400);
        assert_eq!(snapshot.break_intervals(500).duration_ms(), 100);
    }

    #[test]
    fn manual_edit_validation() {
        assert!(validate_session_edit(0, Some(100), &[(10, Some(20))]).is_ok());
        assert!(validate_session_edit(100, Some(10), &[]).is_err());
        assert!(validate_session_edit(0, Some(100), &[(10, Some(5))]).is_err());
        assert!(validate_session_edit(0, Some(100), &[(90, Some(120))]).is_err());
        assert!(validate_session_edit(0, Some(100), &[(10, Some(50)), (40, Some(60))]).is_err());
        assert!(validate_session_edit(0, Some(100), &[(10, Some(40)), (40, Some(60))]).is_ok());
    }

    #[test]
    fn stale_open_session_is_flagged() {
        let s = session(0, None);
        assert!(session_looks_stale(&s, 19 * 60 * 60 * 1000));
        assert!(!session_looks_stale(&s, 3 * 60 * 60 * 1000));
    }
}
