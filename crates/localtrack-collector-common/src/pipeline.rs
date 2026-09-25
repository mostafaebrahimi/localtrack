use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use localtrack_core::activity::merge::CloseReason;
use localtrack_core::activity::{
    ActivityKind, ActivitySegment, BrowserInteractionObservation, BrowserObservation, Observation,
    SegmentKey, SegmentMerger, StreamId, WindowObservation,
};
use localtrack_core::classification::Classifier;
use localtrack_core::privacy::{domain_of, sanitize_url, ExclusionMatcher};
use localtrack_core::sessions::{
    transition, ClockCommand, ClockSnapshot, ClockState, WorkBreak, WorkSession,
};
use localtrack_core::settings::{Settings, TrackingScope};
use localtrack_core::time::now_ms as wall_now_ms;
use localtrack_core::{limits, CoreError};
use localtrack_storage::{repo, Database, Result as StorageResult, StorageError};
use serde::{Deserialize, Serialize};

/// Why activity is or is not being recorded right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TrackingDecision {
    /// Observations are stored.
    Recording,
    /// The user paused tracking (spec §113).
    Paused,
    /// Scope is WORK_SESSIONS_ONLY and the user is clocked out (spec §18).
    NotClockedIn,
    /// On a break with `pause_tracking_during_break` (spec §19).
    OnBreak,
}

impl TrackingDecision {
    pub fn is_recording(&self) -> bool {
        matches!(self, TrackingDecision::Recording)
    }
}

/// Counters shown in diagnostics; never contains activity metadata (spec §108).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineStats {
    pub observations_received: u64,
    pub observations_dropped_privacy: u64,
    pub observations_dropped_state: u64,
    pub segments_written: u64,
    pub last_desktop_event_ms: Option<i64>,
    pub last_browser_event_ms: Option<i64>,
    pub last_write_error_ms: Option<i64>,
}

/// A title change that has not yet lasted long enough to be believed.
#[derive(Debug, Clone)]
struct PendingTitle {
    key: SegmentKey,
    first_seen_ms: i64,
}

/// How often a cached clock state is re-read from the shared database.
const CLOCK_REFRESH_INTERVAL_MS: i64 = 3_000;

/// LocalTrack's own floating timer. It is always on top and gets focus when
/// clicked, but looking at the timer is not an activity worth recording.
const OWN_TIMER_TITLE: &str = "LocalTrack Timer";

/// Executable names that mean "this foreground window is a browser".
const BROWSER_PROCESS_HINTS: &[&str] = &[
    "chrome",
    "chromium",
    "google-chrome",
    "brave",
    "msedge",
    "edge",
    "vivaldi",
    "opera",
    "firefox",
    "librewolf",
    "waterfox",
    "zen",
    // Firefox's X11 window class is "Navigator".
    "navigator",
];

/// The event processing pipeline (spec §68).
///
/// ```text
/// Collector → Validate → Normalize → Tracking state → Privacy rules
///           → URL sanitization → Segment merge → Classification → Persist
/// ```
///
/// Privacy filtering always happens before anything durable is written.
pub struct IngestPipeline {
    db: Arc<Database>,
    merger: SegmentMerger,
    settings: Settings,
    exclusions: ExclusionMatcher,
    classifier: Classifier,
    clock: ClockSnapshot,
    default_category_id: Option<String>,
    /// True when the platform can report the foreground window.
    desktop_available: bool,
    /// True when the desktop foreground window belongs to a browser (spec §41).
    desktop_is_browser: bool,
    /// When the clock state was last re-read from the database.
    clock_refreshed_at_ms: i64,
    /// A title change waiting to prove it is real, per stream.
    pending_titles: HashMap<StreamId, PendingTitle>,
    /// The last moment there was evidence somebody was at the machine.
    ///
    /// Window changes, page changes, idle transitions and the collector's
    /// heartbeats all count: a heartbeat only arrives while the collector
    /// considers the user present, so working in one window for an hour keeps
    /// this fresh.
    last_input_ms: Option<i64>,
    stats: PipelineStats,
}

impl std::fmt::Debug for IngestPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IngestPipeline")
            .field("clock", &self.clock.state)
            .field("stats", &self.stats)
            .finish()
    }
}

impl IngestPipeline {
    pub fn new(db: Arc<Database>, desktop_available: bool) -> StorageResult<Self> {
        let mut pipeline = Self {
            db,
            merger: SegmentMerger::default(),
            settings: Settings::default(),
            exclusions: ExclusionMatcher::default(),
            classifier: Classifier::default(),
            clock: ClockSnapshot::clocked_out(),
            default_category_id: None,
            desktop_available,
            desktop_is_browser: false,
            clock_refreshed_at_ms: 0,
            pending_titles: HashMap::new(),
            last_input_ms: None,
            stats: PipelineStats::default(),
        };
        pipeline.reload()?;
        Ok(pipeline)
    }

    /// Re-read settings, rules, exclusions and clock state from the database.
    pub fn reload(&mut self) -> StorageResult<()> {
        let (settings, rules, exclusions, categories, clock) = self.db.read(|conn| {
            Ok((
                repo::settings::load(conn)?,
                repo::rules::list(conn)?,
                repo::exclusions::list(conn)?,
                repo::categories::list(conn)?,
                repo::sessions::clock_snapshot(conn)?,
            ))
        })?;

        self.default_category_id = categories
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case("Uncategorized"))
            .map(|c| c.id.clone());
        self.merger.set_config(settings.merge_config());
        self.classifier = Classifier::new(rules, self.default_category_id.clone());
        self.exclusions = ExclusionMatcher::new(exclusions);
        self.settings = settings;
        self.clock = clock;
        self.clock_refreshed_at_ms = wall_now_ms();
        Ok(())
    }

    /// Re-read only the clock state.
    ///
    /// The desktop application and the Chrome native host are separate
    /// processes sharing one database, so either can clock in or out. This is a
    /// single indexed query, cheap enough to run on a timer.
    pub fn refresh_clock(&mut self) -> StorageResult<()> {
        self.clock = self.db.read(repo::sessions::clock_snapshot)?;
        self.clock_refreshed_at_ms = wall_now_ms();
        Ok(())
    }

    /// Refresh the clock when the cached value is older than `max_age_ms`.
    pub fn refresh_clock_if_stale(&mut self, max_age_ms: i64) -> StorageResult<()> {
        if wall_now_ms() - self.clock_refreshed_at_ms >= max_age_ms {
            self.refresh_clock()?;
        }
        Ok(())
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn clock(&self) -> &ClockSnapshot {
        &self.clock
    }

    /// The activity currently open in memory.
    ///
    /// The dashboard and the floating timer read this rather than the database,
    /// because an open segment is only checkpointed every heartbeat — reading
    /// storage would show what the user was doing up to 15 seconds ago.
    pub fn open_activity(&self, now_ms: i64) -> Option<ActivitySegment> {
        // Idle and lock win: they describe the user, not a window.
        if let Some(system) = self.merger.snapshot(StreamId::System, now_ms) {
            return Some(system);
        }
        // Browser detail beats the generic browser window (spec §42).
        if self.desktop_is_browser {
            if let Some(browser) = self.merger.snapshot(StreamId::Browser, now_ms) {
                return Some(browser);
            }
        }
        self.merger
            .snapshot(StreamId::Desktop, now_ms)
            .or_else(|| self.merger.snapshot(StreamId::Browser, now_ms))
    }

    pub fn stats(&self) -> PipelineStats {
        self.stats
    }

    pub fn set_desktop_available(&mut self, available: bool) {
        self.desktop_available = available;
    }

    /// Whether activity may currently be persisted (spec §18, §19, §113).
    pub fn tracking_decision(&self) -> TrackingDecision {
        if self.settings.tracking_paused {
            return TrackingDecision::Paused;
        }
        match self.clock.state {
            ClockState::OnBreak if self.settings.pause_tracking_during_break => {
                TrackingDecision::OnBreak
            }
            ClockState::ClockedOut
                if self.settings.tracking_scope == TrackingScope::WorkSessionsOnly =>
            {
                TrackingDecision::NotClockedIn
            }
            _ => TrackingDecision::Recording,
        }
    }

    // ---------------------------------------------------------------- clock

    pub fn clock_in(&mut self, at_ms: i64) -> StorageResult<ClockSnapshot> {
        self.apply_clock_command(ClockCommand::ClockIn, at_ms)
    }

    pub fn clock_out(&mut self, at_ms: i64) -> StorageResult<ClockSnapshot> {
        self.apply_clock_command(ClockCommand::ClockOut, at_ms)
    }

    pub fn start_break(&mut self, at_ms: i64) -> StorageResult<ClockSnapshot> {
        self.apply_clock_command(ClockCommand::StartBreak, at_ms)
    }

    pub fn end_break(&mut self, at_ms: i64) -> StorageResult<ClockSnapshot> {
        self.apply_clock_command(ClockCommand::EndBreak, at_ms)
    }

    fn apply_clock_command(
        &mut self,
        command: ClockCommand,
        at_ms: i64,
    ) -> StorageResult<ClockSnapshot> {
        let next = transition(self.clock.state, command).map_err(StorageError::Core)?;

        // A clock boundary always closes open segments: nothing merges across it.
        self.close_all_streams(at_ms)?;

        let now = wall_now_ms();
        match command {
            ClockCommand::ClockIn => {
                let open = self.db.read(repo::sessions::count_open_sessions)?;
                if open > 0 {
                    return Err(StorageError::Conflict(
                        "another work session is already open".into(),
                    ));
                }
                let session = WorkSession {
                    id: uuid::Uuid::new_v4().to_string(),
                    started_at_ms: at_ms,
                    ended_at_ms: None,
                    start_timezone_offset_min: localtrack_core::time::offset_minutes_at(at_ms),
                    end_timezone_offset_min: None,
                    note: None,
                    created_manually: false,
                    edited_manually: false,
                    created_at_ms: now,
                    updated_at_ms: now,
                };
                self.db
                    .write(|tx| repo::sessions::insert_session(tx, &session))?;
            }
            ClockCommand::StartBreak => {
                let session = self.require_session()?;
                let work_break = WorkBreak {
                    id: uuid::Uuid::new_v4().to_string(),
                    work_session_id: session.id.clone(),
                    started_at_ms: at_ms,
                    ended_at_ms: None,
                    note: None,
                    created_at_ms: now,
                    updated_at_ms: now,
                };
                self.db
                    .write(|tx| repo::sessions::insert_break(tx, &work_break))?;
            }
            ClockCommand::EndBreak => {
                let session = self.require_session()?;
                let open_break = self
                    .db
                    .read(|conn| repo::sessions::open_break(conn, &session.id))?
                    .ok_or_else(|| {
                        StorageError::Core(CoreError::InvalidClockTransition(
                            "no open break to end".into(),
                        ))
                    })?;
                let mut closed = open_break;
                closed.ended_at_ms = Some(at_ms);
                closed.updated_at_ms = now;
                self.db
                    .write(|tx| repo::sessions::update_break(tx, &closed))?;
            }
            ClockCommand::ClockOut => {
                let session = self.require_session()?;
                if let Some(open_break) = self
                    .db
                    .read(|conn| repo::sessions::open_break(conn, &session.id))?
                {
                    let mut closed = open_break;
                    closed.ended_at_ms = Some(at_ms);
                    closed.updated_at_ms = now;
                    self.db
                        .write(|tx| repo::sessions::update_break(tx, &closed))?;
                }
                let mut closed = session;
                closed.ended_at_ms = Some(at_ms);
                closed.end_timezone_offset_min =
                    Some(localtrack_core::time::offset_minutes_at(at_ms));
                closed.updated_at_ms = now;
                self.db
                    .write(|tx| repo::sessions::update_session(tx, &closed))?;
            }
        }

        self.clock = self.db.read(repo::sessions::clock_snapshot)?;
        debug_assert_eq!(self.clock.state, next);
        Ok(self.clock.clone())
    }

    fn require_session(&self) -> StorageResult<WorkSession> {
        self.clock
            .session
            .clone()
            .ok_or_else(|| StorageError::NotFound("no open work session".into()))
    }

    /// Pause or resume all collection (spec §113). The clock is untouched.
    pub fn set_paused(&mut self, paused: bool, at_ms: i64) -> StorageResult<()> {
        if paused {
            self.close_all_streams(at_ms)?;
        }
        self.settings.tracking_paused = paused;
        let now = wall_now_ms();
        self.db.write(|tx| {
            repo::settings::set(
                tx,
                localtrack_core::settings::keys::TRACKING_PAUSED,
                &serde_json::Value::Bool(paused),
                now,
            )
        })
    }

    // ------------------------------------------------------------- ingestion

    /// Process one observation through the full pipeline.
    pub fn handle(&mut self, observation: Observation) -> StorageResult<()> {
        self.stats.observations_received += 1;
        // Timestamps from untrusted sources are validated in the native host
        // before they reach the pipeline; here a negative instant is simply
        // nonsense and is dropped.
        let at_ms = observation.captured_at_ms();
        if at_ms < 0 {
            return Ok(());
        }

        match observation {
            Observation::WindowChanged(window) => self.handle_window(window),
            Observation::BrowserChanged(browser) => self.handle_browser(browser),
            Observation::UserIdle(idle) => {
                // The collector knows exactly when input stopped; the threshold
                // is only how long we waited before believing it.
                self.last_input_ms = Some(idle.last_input_ms);
                let started = idle.afk_started_at_ms().min(idle.captured_at_ms);
                self.close_activity_streams(started)?;
                self.observe(StreamId::System, SegmentKey::idle(), started)
            }
            Observation::UserActive(idle) => {
                self.last_input_ms = Some(idle.captured_at_ms);
                self.close_stream(StreamId::System, idle.captured_at_ms)
            }
            Observation::SystemLocked(lock) => {
                // Lock overrides the idle threshold entirely (spec §28).
                self.close_activity_streams(lock.captured_at_ms)?;
                self.close_stream(StreamId::System, lock.captured_at_ms)?;
                self.observe(StreamId::System, SegmentKey::locked(), lock.captured_at_ms)
            }
            Observation::SystemUnlocked(lock) => {
                self.close_stream(StreamId::System, lock.captured_at_ms)
            }
            Observation::InputActivity { captured_at_ms } => {
                // Real input with no window to attribute it to: record it as
                // active rather than leaving the day with a hole.
                self.stats.last_desktop_event_ms = Some(captured_at_ms);
                self.observe(StreamId::Desktop, SegmentKey::input(), captured_at_ms)
            }
            Observation::Interaction(interaction) => self.handle_interaction(interaction),
            Observation::Heartbeat {
                captured_at_ms,
                stream,
            } => {
                let stream = match stream.as_str() {
                    "browser" => StreamId::Browser,
                    "system" => StreamId::System,
                    _ => StreamId::Desktop,
                };
                if stream == StreamId::Browser {
                    self.stats.last_browser_event_ms = Some(captured_at_ms);
                } else if stream == StreamId::Desktop {
                    self.stats.last_desktop_event_ms = Some(captured_at_ms);
                    // The desktop collector stops heartbeating once it decides
                    // the user is away, so this one means they are still here.
                    self.last_input_ms = Some(captured_at_ms);
                }

                if self.merger.heartbeat(stream, captured_at_ms) {
                    return Ok(());
                }

                // A system heartbeat only arrives while the user is away, and
                // AFK is reported on transitions. After a boundary closed the
                // idle segment — a clock-in, a midnight split, a restart — the
                // next transition may be hours away, so being away has to be
                // able to resume here or the time would fall into untracked.
                if stream == StreamId::System {
                    return self.observe(StreamId::System, SegmentKey::idle(), captured_at_ms);
                }
                Ok(())
            }
        }
    }

    fn handle_window(&mut self, window: WindowObservation) -> StorageResult<()> {
        self.stats.last_desktop_event_ms = Some(window.captured_at_ms);
        self.last_input_ms = Some(window.captured_at_ms);
        self.desktop_available = true;

        if window.window_title.as_deref() == Some(OWN_TIMER_TITLE) {
            // Keep the previous activity open rather than billing time to the
            // tracker's own always-on-top bar. With nothing open, the person is
            // still demonstrably at the machine, so record that rather than
            // leaving a hole in the day.
            if self
                .merger
                .heartbeat(StreamId::Desktop, window.captured_at_ms)
            {
                return Ok(());
            }
            return self.observe(
                StreamId::Desktop,
                SegmentKey::input(),
                window.captured_at_ms,
            );
        }

        let process = window
            .process_name
            .clone()
            .unwrap_or_default()
            .to_lowercase();
        let app = window.app_name.clone().unwrap_or_default().to_lowercase();
        let was_browser = self.desktop_is_browser;
        self.desktop_is_browser = BROWSER_PROCESS_HINTS
            .iter()
            .any(|hint| process.contains(hint) || app.contains(hint));

        // Chrome lost the foreground: browser page time must stop (spec §41).
        if was_browser && !self.desktop_is_browser {
            self.close_stream(StreamId::Browser, window.captured_at_ms)?;
        }

        let key = SegmentKey::desktop(
            window.app_name.map(|v| limits::normalize_whitespace(&v)),
            window.process_name,
            window
                .window_title
                .map(|v| limits::normalize_whitespace(&v)),
        );
        self.observe(StreamId::Desktop, key, window.captured_at_ms)
    }

    fn handle_browser(&mut self, browser: BrowserObservation) -> StorageResult<()> {
        self.stats.last_browser_event_ms = Some(browser.captured_at_ms);
        if browser.focused {
            self.last_input_ms = Some(browser.captured_at_ms);
        }

        // Incognito is never tracked by default (spec §43).
        if browser.incognito && !self.settings.track_incognito {
            self.stats.observations_dropped_privacy += 1;
            return self.close_stream(StreamId::Browser, browser.captured_at_ms);
        }

        // The browser window itself, or Chrome as a whole, is not focused.
        if !browser.focused
            || browser.event == "blurred"
            || browser.event == "closed"
            || (self.desktop_available && !self.desktop_is_browser)
        {
            self.stats.observations_dropped_state += 1;
            return self.close_stream(StreamId::Browser, browser.captured_at_ms);
        }

        let Some(raw_url) = browser.url.as_deref() else {
            return self.close_stream(StreamId::Browser, browser.captured_at_ms);
        };

        // URL sanitization happens before anything is stored (spec §44).
        let domain = domain_of(raw_url);
        let url = sanitize_url(raw_url, self.settings.url_policy);
        if domain.is_none() || url.is_none() {
            // chrome://, file://, about: pages are not activity.
            self.stats.observations_dropped_privacy += 1;
            return self.close_stream(StreamId::Browser, browser.captured_at_ms);
        }

        let key = SegmentKey::browser_page(
            Some(browser.browser.clone()),
            domain,
            url,
            browser.title.map(|t| limits::normalize_whitespace(&t)),
        );
        self.observe(StreamId::Browser, key, browser.captured_at_ms)
    }

    fn handle_interaction(
        &mut self,
        interaction: BrowserInteractionObservation,
    ) -> StorageResult<()> {
        // Off by default; nothing is stored unless the user turned it on.
        if !self.settings.detailed_interactions {
            self.stats.observations_dropped_privacy += 1;
            return Ok(());
        }
        if !self.tracking_decision().is_recording() {
            self.stats.observations_dropped_state += 1;
            return Ok(());
        }

        let domain = interaction
            .domain
            .clone()
            .or_else(|| interaction.url.as_deref().and_then(domain_of));
        let url = interaction
            .url
            .as_deref()
            .and_then(|u| sanitize_url(u, self.settings.url_policy));

        // The interaction payload carries no browser name, and guessing one
        // would mislabel Firefox as Chrome.
        let mut key = SegmentKey::browser_page(None, domain, url, None);
        key.kind = Some(ActivityKind::Interaction.as_str().to_string());
        key.interaction_type = Some(interaction.interaction.as_str().to_string());

        let Some(key) = self
            .exclusions
            .apply(key, self.settings.record_excluded_duration)
        else {
            self.stats.observations_dropped_privacy += 1;
            return Ok(());
        };

        let now = wall_now_ms();
        let mut segment = ActivitySegment::from_key(
            &key,
            interaction.captured_at_ms,
            interaction.captured_at_ms,
            now,
        );
        // Only element kind and a short accessible label may be stored (spec §49).
        let label = interaction.label.as_deref().map(|l| {
            limits::truncate(
                &limits::normalize_whitespace(l),
                limits::INTERACTION_LABEL_MAX,
            )
        });
        segment.metadata_json = Some(
            serde_json::json!({
                "element": interaction.element,
                "label": label,
            })
            .to_string(),
        );
        self.persist(vec![segment])
    }

    /// Validate → privacy → merge → classify → persist for one keyed activity.
    ///
    /// A change that only affects the window or page title has to persist for
    /// `title_stability_seconds` before it starts a new segment. Without that,
    /// an animated title — a terminal spinner, an unread-count badge, a video
    /// timestamp — would shatter one activity into a segment per second.
    fn observe(&mut self, stream: StreamId, key: SegmentKey, at_ms: i64) -> StorageResult<()> {
        let stability_ms = self.settings.title_stability_ms();
        if stability_ms > 0 {
            if let Some(open) = self.merger.open_segment(stream) {
                if open.key.differs_only_by_title(&key) {
                    let pending = match self.pending_titles.get(&stream) {
                        Some(pending) if pending.key == key => pending.clone(),
                        _ => {
                            let pending = PendingTitle {
                                key: key.clone(),
                                first_seen_ms: at_ms,
                            };
                            self.pending_titles.insert(stream, pending.clone());
                            pending
                        }
                    };

                    if at_ms - pending.first_seen_ms < stability_ms {
                        // Keep the current segment alive; the title may flip back.
                        self.merger.heartbeat(stream, at_ms);
                        return Ok(());
                    }

                    // The new title stuck: start the segment where it changed.
                    self.pending_titles.remove(&stream);
                    let started_at = pending.first_seen_ms;
                    self.commit_observation(stream, key, started_at)?;
                    self.merger.heartbeat(stream, at_ms);
                    return Ok(());
                }
            }
            // Anything other than a title change resolves the pending title.
            self.pending_titles.remove(&stream);
        }

        self.commit_observation(stream, key, at_ms)
    }

    fn commit_observation(
        &mut self,
        stream: StreamId,
        key: SegmentKey,
        at_ms: i64,
    ) -> StorageResult<()> {
        if !self.tracking_decision().is_recording() {
            self.stats.observations_dropped_state += 1;
            // Nothing is recorded, and any open segment is closed at the boundary.
            return self.close_all_streams(at_ms);
        }

        // Idle and lock are system state, not user content: no exclusion rules.
        let key = if key.kind_enum().is_inactive() {
            Some(key)
        } else {
            self.exclusions
                .apply(key, self.settings.record_excluded_duration)
        };

        let Some(key) = key else {
            self.stats.observations_dropped_privacy += 1;
            // Excluded activity is a hard boundary: never merge across it.
            return self.close_stream(stream, at_ms);
        };

        let now = wall_now_ms();
        if let Some(closed) = self.merger.observe(stream, key, at_ms, now) {
            self.persist(vec![closed])?;
        }
        Ok(())
    }

    fn close_stream(&mut self, stream: StreamId, at_ms: i64) -> StorageResult<()> {
        self.pending_titles.remove(&stream);
        let now = wall_now_ms();
        if let Some(segment) = self.merger.close(stream, at_ms, now, CloseReason::Boundary) {
            self.persist(vec![segment])?;
        }
        Ok(())
    }

    /// Close desktop and browser streams but leave idle/lock running.
    fn close_activity_streams(&mut self, at_ms: i64) -> StorageResult<()> {
        self.close_stream(StreamId::Desktop, at_ms)?;
        self.close_stream(StreamId::Browser, at_ms)
    }

    pub fn close_all_streams(&mut self, at_ms: i64) -> StorageResult<()> {
        self.pending_titles.clear();
        let now = wall_now_ms();
        let closed = self.merger.close_all(at_ms, now, CloseReason::Boundary);
        if closed.is_empty() {
            return Ok(());
        }
        self.persist(closed)
    }

    /// Close the running session at local midnight and start a fresh one.
    ///
    /// Each day then has its own record and today's figures start at zero,
    /// rather than a session begun on Monday still running on Wednesday.
    /// Returns the boundary it split at, when it split one.
    pub fn split_session_at_midnight(&mut self, now_ms: i64) -> StorageResult<Option<i64>> {
        if !self.settings.split_sessions_at_midnight {
            return Ok(None);
        }
        if !matches!(
            self.clock.state,
            ClockState::ClockedIn | ClockState::OnBreak
        ) {
            return Ok(None);
        }
        let Some(session) = self.clock.session.clone() else {
            return Ok(None);
        };

        // The first midnight after the session started.
        let boundary = localtrack_core::time::local_day_end_ms(session.started_at_ms);
        if now_ms < boundary {
            return Ok(None);
        }

        let note = session.note.clone();
        let was_on_break = matches!(self.clock.state, ClockState::OnBreak);

        self.clock_out(boundary)?;
        self.clock_in(boundary)?;

        // Carry the description across so the new day continues the same work.
        if let Some(note) = note {
            if let Some(mut fresh) = self.clock.session.clone() {
                fresh.note = Some(note);
                fresh.updated_at_ms = wall_now_ms();
                self.db
                    .write(|tx| repo::sessions::update_session(tx, &fresh))?;
                self.refresh_clock()?;
            }
        }

        // Someone on a break at midnight is still on a break afterwards.
        if was_on_break {
            self.start_break(boundary)?;
        }

        tracing::info!("session split at the day boundary");
        Ok(Some(boundary))
    }

    /// Stop the clock when the machine has been idle long enough, backdating
    /// the clock-out to the moment activity actually stopped.
    ///
    /// Returns the instant the session was closed at, when it closed one.
    pub fn auto_clock_out_if_idle(&mut self, now_ms: i64) -> StorageResult<Option<i64>> {
        let Some(limit) = self.settings.auto_clock_out_idle_ms() else {
            return Ok(None);
        };
        if !matches!(self.clock.state, ClockState::ClockedIn) {
            return Ok(None);
        }

        // The idle stream knows when the user stopped touching the machine.
        let idle_since = self
            .merger
            .open_segment(StreamId::System)
            .filter(|open| open.key.kind_enum().is_inactive())
            .map(|open| open.started_at_ms);

        // Every source of presence counts, and the most recent one wins.
        // Taking the first available instead would let a stale value — the last
        // time the foreground *window changed*, say — look like half an hour of
        // absence while somebody worked steadily in a single window.
        let browser_presence = if self.desktop_available {
            // The desktop collector is authoritative for presence; a focused
            // browser can heartbeat with nobody in the room.
            None
        } else {
            self.stats.last_browser_event_ms
        };
        // The start of an idle period is a boundary, not evidence of presence:
        // including it here would backdate a clock-out to when idleness was
        // confirmed rather than to when the work actually stopped.
        let last_activity = [
            self.last_input_ms,
            self.stats.last_desktop_event_ms,
            browser_presence,
        ]
        .into_iter()
        .flatten()
        .max()
        .or(idle_since);

        let stopped_at = match (idle_since, last_activity) {
            (Some(idle), Some(last)) => last.min(idle),
            (Some(idle), None) => idle,
            (None, Some(last)) if now_ms - last >= limit => last,
            _ => return Ok(None),
        };

        if now_ms - stopped_at < limit {
            return Ok(None);
        }

        // Never close before the session began.
        let session_start = self
            .clock
            .session
            .as_ref()
            .map(|session| session.started_at_ms)
            .unwrap_or(stopped_at);
        let ended_at = stopped_at.max(session_start);

        self.clock_out(ended_at)?;
        tracing::info!("clock stopped automatically after a long idle period");
        Ok(Some(ended_at))
    }

    /// Periodic maintenance: checkpoint open segments and drop stale streams.
    ///
    /// Called roughly every second by the desktop application.
    pub fn tick(&mut self, now_ms: i64) -> StorageResult<()> {
        // Another process may have clocked in or out since the last tick.
        if let Err(err) = self.refresh_clock_if_stale(CLOCK_REFRESH_INTERVAL_MS) {
            tracing::warn!(error = %err, "could not refresh the clock state");
        }

        if let Err(err) = self.auto_clock_out_if_idle(now_ms) {
            tracing::warn!(error = %err, "automatic clock out failed");
        }

        if let Err(err) = self.split_session_at_midnight(now_ms) {
            tracing::warn!(error = %err, "could not split the session at midnight");
        }

        let stale = self.merger.flush_stale(now_ms, now_ms);
        if !stale.is_empty() {
            self.persist(stale)?;
        }

        let interval = self.settings.heartbeat_interval_seconds * 1000;
        let due = self.merger.due_for_checkpoint(now_ms, interval);
        if due.is_empty() {
            return Ok(());
        }
        let mut snapshots = Vec::new();
        for stream in &due {
            if let Some(snapshot) = self.merger.snapshot(*stream, now_ms) {
                if snapshot.duration_ms() > 0 {
                    snapshots.push(snapshot);
                }
            }
        }
        if !snapshots.is_empty() {
            self.persist(snapshots)?;
        }
        for stream in due {
            self.merger.mark_checkpointed(stream, now_ms);
        }
        Ok(())
    }

    fn persist(&mut self, mut segments: Vec<ActivitySegment>) -> StorageResult<()> {
        if segments.is_empty() {
            return Ok(());
        }
        for segment in &mut segments {
            self.classifier.apply(segment);
        }
        let result = self
            .db
            .write(|tx| repo::segments::insert_many(tx, &segments).map(|_| ()));
        match result {
            Ok(()) => {
                self.stats.segments_written += segments.len() as u64;
                Ok(())
            }
            Err(err) => {
                self.stats.last_write_error_ms = Some(wall_now_ms());
                Err(err)
            }
        }
    }

    /// Set a session's description ("what are you working on").
    pub fn set_session_note(&mut self, session_id: &str, note: &str) -> StorageResult<()> {
        let mut session = self
            .db
            .read(|conn| repo::sessions::get_session(conn, session_id))?;
        session.note = Some(note.to_string());
        session.updated_at_ms = wall_now_ms();
        self.db
            .write(|tx| repo::sessions::update_session(tx, &session))?;
        self.refresh_clock()
    }

    /// Categories keyed by id, used for reports.
    pub fn category_names(&self) -> StorageResult<BTreeMap<String, String>> {
        let categories = self.db.read(repo::categories::list)?;
        Ok(categories.into_iter().map(|c| (c.id, c.name)).collect())
    }
}
