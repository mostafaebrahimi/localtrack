//! The application service: one object the Tauri commands (and the tray) call.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::Duration;

use localtrack_collector_common::collector::{ActivityCollector, CollectorStatus};
use localtrack_collector_common::pipeline::{IngestPipeline, TrackingDecision};
use localtrack_core::activity::ActivitySegment;
use localtrack_core::classification::{ClassificationRule, Classifier};
use localtrack_core::privacy::ExclusionRule;
use localtrack_core::sessions::{
    session_looks_stale, validate_session_edit, ClockState, WorkBreak, WorkSession,
};
use localtrack_core::settings::Settings;
use localtrack_core::time::now_ms;
use localtrack_storage::filters::{ActivityFilter, Page, SortOrder};
use localtrack_storage::repo::categories::Category;
use localtrack_storage::repo::maintenance::RetentionReport;
use localtrack_storage::repo::projects::Project;
use localtrack_storage::{paths, repo, Database};
use parking_lot::Mutex;
use rusqlite::OptionalExtension;

use crate::diagnostics::Diagnostics;
use crate::error::{AppError, Result};
use crate::exporting::{ExportRequest, ExportResult};
use crate::managed::{ManagedService, SyncTransport};
use crate::tracking::{
    build_desktop_collector, chrome_status, desktop_capabilities, ObservationBus,
};
use crate::types::*;
use crate::{diagnostics, exporting, maintenance, queries};

/// Local metadata attached to a segment: an optional note plus an audit trail
/// of manual edits. It never leaves the machine and holds no captured content.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edits: Vec<SegmentEdit>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentEdit {
    pub kind: String,
    pub at_ms: i64,
}

impl SegmentMetadata {
    pub fn with_note(mut self, note: Option<String>) -> Self {
        self.note = note
            .map(|value| value.trim().to_string())
            .filter(|v| !v.is_empty());
        self
    }

    pub fn with_edit(mut self, kind: &str, at_ms: i64) -> Self {
        self.edits.push(SegmentEdit {
            kind: kind.to_string(),
            at_ms,
        });
        // Keep the audit trail bounded; the most recent edits are the useful ones.
        if self.edits.len() > 20 {
            let excess = self.edits.len() - 20;
            self.edits.drain(0..excess);
        }
        self
    }

    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }
}

fn segment_metadata(segment: &ActivitySegment) -> SegmentMetadata {
    segment
        .metadata_json
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default()
}

/// Which segments a rule change should be applied to (spec §65).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReclassifyScope {
    NewActivityOnly,
    ExistingActivity,
}

pub struct AppService {
    db: Arc<Database>,
    /// Identifies this run in the uptime table.
    run_id: String,
    /// Present only when the build carries a transport; `None` keeps the
    /// application entirely local, which is still the default.
    managed: Option<Arc<ManagedService>>,
    pipeline: Arc<Mutex<IngestPipeline>>,
    collector: Mutex<Option<Box<dyn ActivityCollector>>>,
    bus: Mutex<Option<ObservationBus>>,
    worker_stop: Arc<AtomicBool>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Today's figures, recomputed at most every few seconds.
    today_cache: Mutex<Option<(i64, localtrack_core::aggregation::Summary)>>,
}

/// How long a cached "today" summary stays good enough for the status card.
const TODAY_CACHE_MS: i64 = 5_000;

impl AppService {
    /// Open the database at the standard location and prepare the pipeline.
    pub fn new() -> Result<Arc<Self>> {
        paths::ensure_dirs()?;
        Self::with_database_path(paths::db_path())
    }

    pub fn with_database_path<P: AsRef<Path>>(path: P) -> Result<Arc<Self>> {
        let db = Arc::new(Database::open(path, now_ms())?);
        Self::with_database(db)
    }

    pub fn with_database(db: Arc<Database>) -> Result<Arc<Self>> {
        Self::with_transport(db, None)
    }

    /// Build a service that can talk to an organization's server.
    ///
    /// Without a transport the application stays entirely local, which remains
    /// the default for anyone who never enrolls a device.
    pub fn with_transport(
        db: Arc<Database>,
        transport: Option<Arc<dyn SyncTransport>>,
    ) -> Result<Arc<Self>> {
        let desktop_available = desktop_capabilities().active_window;
        let pipeline = Arc::new(Mutex::new(IngestPipeline::new(
            db.clone(),
            desktop_available,
        )?));
        let managed =
            transport.map(|transport| Arc::new(ManagedService::new(db.clone(), transport)));
        // Record that this run started, so gaps can be explained later.
        let run_id = uuid::Uuid::new_v4().to_string();
        let started = now_ms();
        let recorded_id = run_id.clone();
        db.write(move |tx| {
            repo::uptime::start(tx, &recorded_id, started, env!("CARGO_PKG_VERSION"))
        })?;

        Ok(Arc::new(Self {
            managed,
            run_id,
            db,
            pipeline,
            collector: Mutex::new(None),
            bus: Mutex::new(Some(ObservationBus::new())),
            worker_stop: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
            today_cache: Mutex::new(None),
        }))
    }

    pub fn database(&self) -> Arc<Database> {
        self.db.clone()
    }

    /// Employee-mode services, when this build has a transport.
    pub fn managed(&self) -> Option<Arc<ManagedService>> {
        self.managed.clone()
    }

    /// Enroll this device with an organization's server.
    pub fn enroll(
        &self,
        server_url: &str,
        code: &str,
        device_name: &str,
    ) -> Result<crate::managed::ManagedStatus> {
        let managed = self.require_managed()?;
        let status = managed.enroll(server_url, code, device_name)?;

        // Enrolling *is* the setup, and the welcome text no longer applies.
        let now = now_ms();
        self.db.write(|tx| {
            repo::settings::set(
                tx,
                localtrack_core::settings::keys::ONBOARDING_COMPLETED,
                &serde_json::Value::Bool(true),
                now,
            )
        })?;

        // A policy may have pinned settings and added categories and rules.
        self.pipeline.lock().reload()?;
        Ok(status)
    }

    /// Ask the server for a newer policy and apply it.
    pub fn poll_policy(&self) -> Result<bool> {
        let managed = self.require_managed()?;
        let changed = managed.poll_policy()?;
        if changed {
            self.pipeline.lock().reload()?;
        }
        Ok(changed)
    }

    pub fn unenroll(&self) -> Result<()> {
        let managed = self.require_managed()?;
        managed.unenroll()?;
        self.pipeline.lock().reload()?;
        Ok(())
    }

    pub fn managed_status(&self) -> Result<crate::managed::ManagedStatus> {
        match &self.managed {
            Some(managed) => managed.status(),
            None => Ok(crate::managed::ManagedStatus::personal()),
        }
    }

    /// Exchange sessions with the server and deliver anything queued.
    pub fn sync_now(&self) -> Result<crate::managed::SessionSyncSummary> {
        let managed = self.require_managed()?;
        let summary = managed.sync_sessions()?;

        // Queue any finished day that has not been reported, so pressing this
        // does what a person expects rather than waiting for the hourly pass.
        let settings = self.settings();
        if let Err(err) = managed.queue_due_reports(&settings, now_ms()) {
            tracing::debug!(error = %err, "queueing daily reports failed");
        }
        managed.flush_outbox()?;

        // Pressing "Sync now" should exchange in both directions: the server
        // may have answered an earlier report with new categories and rules,
        // and waiting for the next scheduled poll to see them looks broken.
        if let Err(err) = managed.poll_policy() {
            tracing::debug!(error = %err, "policy poll during sync failed");
        }

        self.pipeline.lock().reload()?;
        Ok(summary)
    }

    pub fn sent_reports(&self, limit: i64) -> Result<Vec<crate::managed::SentReport>> {
        match &self.managed {
            Some(managed) => managed.sent_reports(limit),
            None => Ok(Vec::new()),
        }
    }

    pub fn queued_reports(&self, limit: i64) -> Result<Vec<crate::managed::SentReport>> {
        match &self.managed {
            Some(managed) => managed.queued_reports(limit),
            None => Ok(Vec::new()),
        }
    }

    fn require_managed(&self) -> Result<Arc<ManagedService>> {
        self.managed.clone().ok_or_else(|| {
            AppError::Invalid("This build of LocalTrack has no server connection.".into())
        })
    }

    /// Settings frozen by an administrator's policy.
    pub fn locked_settings(&self) -> Vec<String> {
        self.managed
            .as_ref()
            .map(|managed| managed.locked_settings())
            .unwrap_or_default()
    }

    pub fn settings(&self) -> Settings {
        self.pipeline.lock().settings().clone()
    }

    // ------------------------------------------------------------ lifecycle

    /// Start the ingest worker and the platform collector.
    pub fn start(self: &Arc<Self>) -> Result<()> {
        self.start_worker();
        self.start_collector()?;
        self.start_managed_worker();

        // A session left open across a shutdown or an overnight absence is
        // closed at the last activity actually recorded, never at "now".
        if let Err(err) = self.recover_stale_session() {
            tracing::warn!(error = %err, "could not close a stale session");
        }

        // Retention runs at most once a day, in the background (spec §112).
        let settings = self.settings();
        if let Err(err) = maintenance::run_retention_if_due(&self.db, &settings) {
            tracing::warn!(error = %err, "retention run failed");
        }
        Ok(())
    }

    /// Background work for an enrolled device: policy, sessions, heartbeat and
    /// the daily report. Everything is best-effort — a server that is down must
    /// never disturb local tracking.
    /// Close a session that was left running before a long gap.
    ///
    /// The clock-out is backdated to the last recorded activity, so a machine
    /// left on overnight does not report a twelve-hour day.
    fn recover_stale_session(&self) -> Result<Option<i64>> {
        let settings = self.settings();
        let Some(limit) = settings.auto_clock_out_idle_ms() else {
            return Ok(None);
        };
        let Some(session) = self.db.read(repo::sessions::open_session)? else {
            return Ok(None);
        };

        let now = now_ms();
        let last_activity = self.db.read(|conn| {
            let segments = repo::segments::load_range(conn, session.started_at_ms, now)?;
            Ok(segments
                .iter()
                .filter(|segment| !segment.kind.is_inactive())
                .map(|segment| segment.ended_at_ms)
                .max())
        })?;

        let stopped_at = last_activity.unwrap_or(session.started_at_ms);
        if now - stopped_at < limit {
            return Ok(None);
        }

        let ended_at = stopped_at.max(session.started_at_ms);
        self.pipeline.lock().clock_out(ended_at)?;
        tracing::info!("closed a session that had been left open");
        Ok(Some(ended_at))
    }

    fn start_managed_worker(self: &Arc<Self>) {
        let Some(managed) = self.managed.clone() else {
            return;
        };
        let service = self.clone();
        let stop = self.worker_stop.clone();

        let _ = std::thread::Builder::new()
            .name("localtrack-managed".into())
            .spawn(move || {
                let mut last_policy = 0i64;
                let mut last_sync = 0i64;
                let mut last_heartbeat = 0i64;
                let mut last_report_check = 0i64;

                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_secs(20));
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    if managed.enrollment().ok().flatten().is_none() {
                        continue;
                    }

                    let now = now_ms();
                    let reporting = managed
                        .policy()
                        .ok()
                        .flatten()
                        .map(|policy| policy.reporting)
                        .unwrap_or_default();

                    if now - last_policy >= reporting.policy_poll_interval_ms() {
                        last_policy = now;
                        if let Err(err) = service.poll_policy() {
                            tracing::debug!(error = %err, "policy poll failed");
                        }
                    }

                    // Two-way session sync: what changed here goes up, what
                    // changed elsewhere comes down.
                    if now - last_sync >= 120_000 {
                        last_sync = now;
                        if let Err(err) = service.sync_now() {
                            tracing::debug!(error = %err, "session sync failed");
                        }
                    }

                    if now - last_heartbeat >= reporting.heartbeat_interval_ms() {
                        last_heartbeat = now;
                        let (state, active, healthy) = match service.current_status() {
                            Ok(status) => (
                                status.state.as_str().to_string(),
                                status.today.active_ms,
                                status.collectors.iter().any(|c| c.healthy && c.available),
                            ),
                            Err(_) => ("UNKNOWN".to_string(), 0, false),
                        };
                        if let Err(err) = managed.send_heartbeat(&state, active, healthy) {
                            tracing::debug!(error = %err, "heartbeat failed");
                        }
                    }

                    // Finished days are queued once an hour and delivered as
                    // soon as the server is reachable.
                    if now - last_report_check >= 3_600_000 {
                        last_report_check = now;
                        let settings = service.settings();
                        if let Err(err) = managed.queue_due_reports(&settings, now) {
                            tracing::debug!(error = %err, "queueing daily reports failed");
                        }
                        if let Err(err) = managed.flush_outbox() {
                            tracing::debug!(error = %err, "sending daily reports failed");
                        }
                    }
                }
            });
    }

    fn start_worker(self: &Arc<Self>) {
        let mut worker = self.worker.lock();
        if worker.is_some() {
            return;
        }
        let receiver = match self.bus.lock().as_mut().and_then(|bus| bus.receiver.take()) {
            Some(receiver) => receiver,
            None => return,
        };

        self.worker_stop.store(false, Ordering::Relaxed);
        let stop = self.worker_stop.clone();
        let pipeline = self.pipeline.clone();
        let db = self.db.clone();
        let run_id = self.run_id.clone();
        let mut last_uptime = 0i64;

        let handle = std::thread::Builder::new()
            .name("localtrack-ingest".into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match receiver.recv_timeout(Duration::from_millis(1000)) {
                        Ok(observation) => {
                            let mut guard = pipeline.lock();
                            if let Err(err) = guard.handle(observation) {
                                tracing::warn!(error = %err, "observation could not be stored");
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }

                    let mut guard = pipeline.lock();
                    if let Err(err) = guard.tick(now_ms()) {
                        tracing::warn!(error = %err, "checkpoint failed");
                    }
                    drop(guard);

                    // Refresh the uptime row about as often as segments are
                    // checkpointed; a crash then loses at most that much.
                    let now = now_ms();
                    if now - last_uptime >= 15_000 {
                        last_uptime = now;
                        let id = run_id.clone();
                        if let Err(err) = db.write(move |tx| repo::uptime::heartbeat(tx, &id, now))
                        {
                            tracing::debug!(error = %err, "could not record uptime");
                        }
                    }
                }
            })
            .ok();
        *worker = handle;
    }

    fn start_collector(self: &Arc<Self>) -> Result<()> {
        let settings = self.settings();
        let sender = match self.bus.lock().as_ref() {
            Some(bus) => bus.sender.clone(),
            None => return Ok(()),
        };

        let mut collector = self.collector.lock();
        if let Some(existing) = collector.as_mut() {
            let _ = existing.stop();
        }
        let mut new_collector = match build_desktop_collector(
            (settings.poll_interval_seconds * 1000) as u64,
            settings.afk_threshold_ms(),
        ) {
            Some(collector) => collector,
            None => {
                tracing::warn!("no desktop collector is available on this platform");
                *collector = None;
                return Ok(());
            }
        };

        match new_collector.start(sender) {
            Ok(()) => {
                *collector = Some(new_collector);
                Ok(())
            }
            Err(err) => {
                // A failing collector never takes down the application (spec §145).
                tracing::warn!(error = %err, "desktop collector could not start");
                *collector = None;
                Ok(())
            }
        }
    }

    /// Stop collectors, flush open segments and close the WAL cleanly.
    pub fn shutdown(&self) {
        if let Some(collector) = self.collector.lock().as_mut() {
            let _ = collector.stop();
        }
        self.worker_stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.worker.lock().take() {
            let _ = handle.join();
        }
        {
            let mut pipeline = self.pipeline.lock();
            if let Err(err) = pipeline.close_all_streams(now_ms()) {
                tracing::warn!(error = %err, "could not flush open segments on shutdown");
            }
        }
        let id = self.run_id.clone();
        let now = now_ms();
        if let Err(err) = self.db.write(move |tx| repo::uptime::stop(tx, &id, now)) {
            tracing::warn!(error = %err, "could not record the shutdown");
        }
        if let Err(err) = self.db.checkpoint() {
            tracing::warn!(error = %err, "WAL checkpoint failed on shutdown");
        }
    }

    // ---------------------------------------------------------------- clock

    pub fn clock_in(&self) -> Result<CurrentStatus> {
        self.clock_in_with_note(None)
    }

    /// Start the clock, optionally recording what the person is working on.
    pub fn clock_in_with_note(&self, note: Option<String>) -> Result<CurrentStatus> {
        {
            let mut pipeline = self.pipeline.lock();
            pipeline.clock_in(now_ms())?;
        }
        if let Some(note) = note.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
            // Read the session in its own scope: an `if let` holds the guard for
            // the whole block, and the reload below takes the same lock.
            let session = {
                let pipeline = self.pipeline.lock();
                pipeline.clock().session.clone()
            };
            if let Some(mut session) = session {
                session.note = Some(note);
                session.updated_at_ms = now_ms();
                self.db
                    .write(|tx| repo::sessions::update_session(tx, &session))?;
                self.pipeline.lock().reload()?;
            }
        }
        self.current_status()
    }

    /// Change what the running session is about, without stopping the clock.
    pub fn set_session_note(&self, session_id: &str, note: Option<String>) -> Result<()> {
        let mut session = self
            .db
            .read(|conn| repo::sessions::get_session(conn, session_id))?;
        session.note = note.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
        session.updated_at_ms = now_ms();
        session.edited_manually = true;
        self.db
            .write(|tx| repo::sessions::update_session(tx, &session))?;
        self.pipeline.lock().reload()?;
        Ok(())
    }

    /// Record work done away from this computer (a meeting, a call, travel).
    ///
    /// The entry is marked as created by hand, so a report can always tell
    /// observed time from time somebody typed in.
    pub fn add_manual_entry(
        &self,
        started_at_ms: i64,
        ended_at_ms: i64,
        note: Option<String>,
    ) -> Result<WorkSession> {
        if ended_at_ms <= started_at_ms {
            return Err(AppError::Invalid("The end must be after the start.".into()));
        }
        if started_at_ms > now_ms() + 60_000 {
            return Err(AppError::Invalid("That entry starts in the future.".into()));
        }

        // Overlapping a session would double-count the same minutes.
        let neighbours = self
            .db
            .read(|conn| repo::sessions::list_sessions(conn, started_at_ms, ended_at_ms))?;
        if let Some(clash) = neighbours.first() {
            return Err(AppError::Invalid(format!(
                "That overlaps a session already recorded at {}.",
                localtrack_core::time::format_local_time(clash.started_at_ms)
            )));
        }

        self.create_manual_session(started_at_ms, ended_at_ms, note)
    }

    pub fn clock_out(&self) -> Result<CurrentStatus> {
        self.pipeline.lock().clock_out(now_ms())?;
        self.current_status()
    }

    pub fn start_break(&self) -> Result<CurrentStatus> {
        self.pipeline.lock().start_break(now_ms())?;
        self.current_status()
    }

    pub fn end_break(&self) -> Result<CurrentStatus> {
        self.pipeline.lock().end_break(now_ms())?;
        self.current_status()
    }

    pub fn set_paused(&self, paused: bool) -> Result<CurrentStatus> {
        self.pipeline.lock().set_paused(paused, now_ms())?;
        self.current_status()
    }

    pub fn current_status(&self) -> Result<CurrentStatus> {
        let now = now_ms();
        let (clock, tracking, stats) = {
            let mut pipeline = self.pipeline.lock();
            // The Chrome popup can clock in or out too (spec §100).
            pipeline.refresh_clock_if_stale(1_000)?;
            (
                pipeline.clock().clone(),
                pipeline.tracking_decision(),
                pipeline.stats(),
            )
        };

        let settings = self.settings();
        let today = self.today_summary_cached(now, &settings)?;
        let open_activity = self.pipeline.lock().open_activity(now);

        let session_duration_ms = clock
            .session
            .as_ref()
            .map(|s| s.clocked_duration_ms(now))
            .unwrap_or(0);
        let break_duration_ms = clock.break_intervals(now).duration_ms();

        let mut collectors = Vec::new();
        let collector_status = {
            let guard = self.collector.lock();
            guard.as_ref().map(|collector| collector.status())
        };
        if let Some(mut status) = collector_status {
            // The pipeline sees every observation, so it is the better source
            // for "last heard from" if the collector has not recorded one.
            if status.last_event_ms.is_none() {
                status.last_event_ms = stats.last_desktop_event_ms;
            }
            collectors.push(status);
        } else {
            let capabilities = desktop_capabilities();
            collectors.push(CollectorStatus {
                name: "desktop".into(),
                healthy: true,
                available: capabilities.active_window,
                last_event_ms: stats.last_desktop_event_ms,
                error: None,
                detail: capabilities.detail,
            });
        }
        let chrome = chrome_status(self.last_browser_activity_ms()?, now);
        let chrome_connected = chrome.healthy;
        collectors.push(chrome);

        Ok(CurrentStatus {
            state: clock.state,
            tracking,
            current_activity: self.current_activity(&clock.state, open_activity)?,
            session_duration_ms,
            break_duration_ms,
            open_break: clock.open_break.clone(),
            stale_session_warning: clock
                .session
                .as_ref()
                .map(|s| session_looks_stale(s, now))
                .unwrap_or(false),
            session: clock.session.clone(),
            today,
            collectors,
            chrome_connected,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            schema_version: localtrack_storage::SCHEMA_VERSION,
        })
    }

    /// Today's totals for the status card, tray and floating timer.
    ///
    /// This is read once a second by three consumers, so it computes only the
    /// summary — not the timeline and the four report sets the Today page needs
    /// — and holds the result briefly.
    fn today_summary_cached(
        &self,
        now: i64,
        settings: &Settings,
    ) -> Result<localtrack_core::aggregation::Summary> {
        if let Some((computed_at, summary)) = self.today_cache.lock().as_ref() {
            if now - computed_at < TODAY_CACHE_MS {
                return Ok(*summary);
            }
        }

        let from = localtrack_core::time::local_day_start_ms(now);
        let to = localtrack_core::time::local_day_end_ms(now).min(now.max(from + 1));
        let summary = queries::summary(&self.db, from, to, &ActivityFilter::default(), settings)?;
        *self.today_cache.lock() = Some((now, summary));
        Ok(summary)
    }

    fn last_browser_activity_ms(&self) -> Result<Option<i64>> {
        let now = now_ms();
        // Only the newest timestamp matters here, and this runs once a second
        // for as long as LocalTrack is open: ask SQLite for the maximum rather
        // than loading ten minutes of segments and folding over them. With
        // idx_segments_source_ended the index answers without touching a row.
        //
        // The upper bound on started_at_ms is not redundant. A backwards clock
        // jump can leave a segment dated entirely in the future, and reading one
        // as "Chrome just reported" would light the collector up as healthy on
        // the strength of a timestamp that has not happened yet.
        let latest = self.db.read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT MAX(ended_at_ms) FROM activity_segments
                 WHERE source = ?1 AND ended_at_ms > ?2 AND started_at_ms < ?3",
            )?;
            let latest: Option<i64> = stmt.query_row(
                rusqlite::params![
                    localtrack_core::activity::ActivitySource::Chrome.as_str(),
                    now - 10 * 60_000,
                    now
                ],
                |row| row.get(0),
            )?;
            Ok(latest)
        })?;
        Ok(latest)
    }

    /// What the user is doing right now, for the status card, tray and timer.
    ///
    /// The in-memory open segment is authoritative; storage is only consulted
    /// for activity this process did not collect itself, such as a browser page
    /// recorded by the Chrome native host.
    fn current_activity(
        &self,
        state: &ClockState,
        open_activity: Option<ActivitySegment>,
    ) -> Result<Option<CurrentActivity>> {
        if *state == ClockState::ClockedOut {
            return Ok(None);
        }
        let now = now_ms();
        // Only the most recent row matters here, so let SQLite pick it rather
        // than loading and scanning five minutes of activity every second.
        let newest_stored = self.db.read(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT id FROM activity_segments
                 WHERE ended_at_ms > ?1 AND kind NOT IN ('IDLE', 'LOCKED')
                 ORDER BY ended_at_ms DESC LIMIT 1",
            )?;
            let id: Option<String> = stmt
                .query_row([now - 5 * 60_000], |row| row.get(0))
                .optional()?;
            match id {
                Some(id) => Ok(Some(repo::segments::get(conn, &id)?)),
                None => Ok(None),
            }
        })?;

        let candidate = match (open_activity, newest_stored) {
            (Some(open), Some(stored)) => {
                // Whichever describes the more recent moment wins.
                if open.ended_at_ms >= stored.ended_at_ms {
                    Some(open)
                } else {
                    Some(stored)
                }
            }
            (Some(open), None) => Some(open),
            (None, stored) => stored,
        };

        let Some(latest) = candidate else {
            return Ok(None);
        };

        let categories = queries::category_names(&self.db)?;
        let projects = queries::project_names(&self.db)?;
        Ok(Some(CurrentActivity {
            label: latest.label(),
            app_name: latest.app_name.clone(),
            domain: latest.domain.clone(),
            title: latest
                .page_title
                .clone()
                .or_else(|| latest.window_title.clone()),
            since_ms: latest.started_at_ms,
            category_name: latest
                .category_id
                .as_ref()
                .and_then(|id| categories.get(id).cloned()),
            project_name: latest
                .project_id
                .as_ref()
                .and_then(|id| projects.get(id).cloned()),
        }))
    }

    // -------------------------------------------------------------- queries

    pub fn today(&self) -> Result<TodayDashboard> {
        queries::today(&self.db, &self.settings())
    }

    pub fn timeline(
        &self,
        query: &RangeQuery,
    ) -> Result<Vec<localtrack_core::aggregation::TimelineBlock>> {
        queries::timeline_blocks(
            &self.db,
            query.from_ms,
            query.to_ms,
            &query.filter,
            &self.settings(),
        )
    }

    pub fn summary(&self, query: &RangeQuery) -> Result<localtrack_core::aggregation::Summary> {
        queries::summary(
            &self.db,
            query.from_ms,
            query.to_ms,
            &query.filter,
            &self.settings(),
        )
    }

    pub fn reports(&self, query: &RangeQuery) -> Result<ReportBundle> {
        queries::report_bundle(
            &self.db,
            query.from_ms,
            query.to_ms,
            &query.filter,
            &self.settings(),
        )
    }

    /// Week-by-week totals, with the applications and addresses behind them.
    pub fn weekly(
        &self,
        query: &RangeQuery,
    ) -> Result<Vec<localtrack_core::aggregation::WeekSummary>> {
        queries::weekly_report(
            &self.db,
            query.from_ms,
            query.to_ms,
            &query.filter,
            &self.settings(),
        )
    }

    pub fn pages_for_domain(
        &self,
        query: &RangeQuery,
        domain: &str,
    ) -> Result<localtrack_core::aggregation::ReportSet> {
        queries::page_report_for_domain(
            &self.db,
            query.from_ms,
            query.to_ms,
            domain,
            &query.filter,
            &self.settings(),
        )
    }

    pub fn compare(&self, query: &RangeQuery) -> Result<ComparisonResult> {
        queries::compare(
            &self.db,
            query.from_ms,
            query.to_ms,
            &query.filter,
            &self.settings(),
        )
    }

    pub fn activity_page(
        &self,
        filter: &ActivityFilter,
        page: Page,
        order: SortOrder,
    ) -> Result<ActivityPage> {
        queries::activity_page(&self.db, filter, page, order)
    }

    pub fn sessions(&self, from_ms: i64, to_ms: i64) -> Result<Vec<SessionDetail>> {
        queries::session_details(&self.db, from_ms, to_ms, &self.settings())
    }

    pub fn filter_options(&self) -> Result<FilterOptions> {
        queries::filter_options(&self.db)
    }

    // ------------------------------------------------------- manual editing

    /// Edit a session and its breaks (spec §114).
    pub fn update_session(
        &self,
        session_id: &str,
        started_at_ms: i64,
        ended_at_ms: Option<i64>,
        note: Option<String>,
    ) -> Result<SessionDetail> {
        let breaks = self
            .db
            .read(|conn| repo::sessions::breaks_for_session(conn, session_id))?;
        let pairs: Vec<(i64, Option<i64>)> = breaks
            .iter()
            .map(|b| (b.started_at_ms, b.ended_at_ms))
            .collect();
        validate_session_edit(started_at_ms, ended_at_ms, &pairs)?;

        let mut session = self
            .db
            .read(|conn| repo::sessions::get_session(conn, session_id))?;
        session.started_at_ms = started_at_ms;
        session.ended_at_ms = ended_at_ms;
        session.end_timezone_offset_min = ended_at_ms.map(localtrack_core::time::offset_minutes_at);
        session.note = note;
        session.edited_manually = true;
        session.updated_at_ms = now_ms();
        self.db
            .write(|tx| repo::sessions::update_session(tx, &session))?;
        self.pipeline.lock().reload()?;

        self.sessions(
            session.started_at_ms,
            session.ended_at_ms.unwrap_or_else(now_ms) + 1,
        )?
        .into_iter()
        .find(|detail| detail.session.id == session_id)
        .ok_or_else(|| AppError::Invalid("session not found after update".into()))
    }

    pub fn add_break(
        &self,
        session_id: &str,
        started_at_ms: i64,
        ended_at_ms: Option<i64>,
    ) -> Result<WorkBreak> {
        let session = self
            .db
            .read(|conn| repo::sessions::get_session(conn, session_id))?;
        let mut breaks = self
            .db
            .read(|conn| repo::sessions::breaks_for_session(conn, session_id))?;
        breaks.push(WorkBreak {
            id: uuid::Uuid::new_v4().to_string(),
            work_session_id: session_id.to_string(),
            started_at_ms,
            ended_at_ms,
            note: None,
            created_at_ms: now_ms(),
            updated_at_ms: now_ms(),
        });
        let pairs: Vec<(i64, Option<i64>)> = breaks
            .iter()
            .map(|b| (b.started_at_ms, b.ended_at_ms))
            .collect();
        validate_session_edit(session.started_at_ms, session.ended_at_ms, &pairs)?;

        let created = breaks.pop().expect("break was just pushed");
        self.db
            .write(|tx| repo::sessions::insert_break(tx, &created))?;
        self.pipeline.lock().reload()?;
        Ok(created)
    }

    pub fn update_break(
        &self,
        break_id: &str,
        started_at_ms: i64,
        ended_at_ms: Option<i64>,
        note: Option<String>,
    ) -> Result<()> {
        let session_id = self.db.read(|conn| {
            let id: String = conn.query_row(
                "SELECT work_session_id FROM work_breaks WHERE id = ?1",
                [break_id],
                |row| row.get(0),
            )?;
            Ok(id)
        })?;
        let session = self
            .db
            .read(|conn| repo::sessions::get_session(conn, &session_id))?;
        let breaks = self
            .db
            .read(|conn| repo::sessions::breaks_for_session(conn, &session_id))?;
        let pairs: Vec<(i64, Option<i64>)> = breaks
            .iter()
            .map(|b| {
                if b.id == break_id {
                    (started_at_ms, ended_at_ms)
                } else {
                    (b.started_at_ms, b.ended_at_ms)
                }
            })
            .collect();
        validate_session_edit(session.started_at_ms, session.ended_at_ms, &pairs)?;

        let mut updated = breaks
            .into_iter()
            .find(|b| b.id == break_id)
            .ok_or_else(|| AppError::Invalid("break not found".into()))?;
        updated.started_at_ms = started_at_ms;
        updated.ended_at_ms = ended_at_ms;
        updated.note = note;
        updated.updated_at_ms = now_ms();
        self.db
            .write(|tx| repo::sessions::update_break(tx, &updated))?;
        self.pipeline.lock().reload()?;
        Ok(())
    }

    pub fn delete_break(&self, break_id: &str) -> Result<()> {
        self.db
            .write(|tx| repo::sessions::delete_break(tx, break_id))?;
        self.pipeline.lock().reload()?;
        Ok(())
    }

    pub fn create_manual_session(
        &self,
        started_at_ms: i64,
        ended_at_ms: i64,
        note: Option<String>,
    ) -> Result<WorkSession> {
        validate_session_edit(started_at_ms, Some(ended_at_ms), &[])?;
        let now = now_ms();
        let session = WorkSession {
            id: uuid::Uuid::new_v4().to_string(),
            started_at_ms,
            ended_at_ms: Some(ended_at_ms),
            start_timezone_offset_min: localtrack_core::time::offset_minutes_at(started_at_ms),
            end_timezone_offset_min: Some(localtrack_core::time::offset_minutes_at(ended_at_ms)),
            note,
            created_manually: true,
            edited_manually: false,
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.db
            .write(|tx| repo::sessions::insert_session(tx, &session))?;
        Ok(session)
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        self.db
            .write(|tx| repo::sessions::delete_session(tx, session_id))?;
        self.pipeline.lock().reload()?;
        Ok(())
    }

    /// Manual category/project assignment (spec §115). Never overwritten later.
    pub fn classify_segment(
        &self,
        segment_id: &str,
        category_id: Option<String>,
        project_id: Option<String>,
    ) -> Result<ActivitySegment> {
        let now = now_ms();
        let segment = self.db.read(|conn| repo::segments::get(conn, segment_id))?;
        // Manual edits are audited locally in the segment metadata (spec §74).
        let metadata = segment_metadata(&segment)
            .with_edit("classification", now)
            .encode();
        self.db.write(|tx| {
            repo::segments::set_classification(
                tx,
                segment_id,
                category_id.as_deref(),
                project_id.as_deref(),
                now,
            )?;
            repo::segments::set_metadata(tx, segment_id, Some(metadata.as_str()), now)
        })?;
        self.db
            .read(|conn| repo::segments::get(conn, segment_id))
            .map_err(Into::into)
    }

    /// Attach a note to one activity (spec §74).
    pub fn annotate_segment(
        &self,
        segment_id: &str,
        note: Option<String>,
    ) -> Result<ActivitySegment> {
        let segment = self.db.read(|conn| repo::segments::get(conn, segment_id))?;
        let metadata = segment_metadata(&segment)
            .with_note(note)
            .with_edit("note", now_ms());
        let encoded = metadata.encode();
        self.db.write(|tx| {
            repo::segments::set_metadata(tx, segment_id, Some(encoded.as_str()), now_ms())
        })?;
        self.db
            .read(|conn| repo::segments::get(conn, segment_id))
            .map_err(Into::into)
    }

    pub fn split_segment(&self, segment_id: &str, at_ms: i64) -> Result<(String, String)> {
        let new_id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        self.db
            .write(|tx| repo::segments::split(tx, segment_id, at_ms, &new_id, now))
            .map_err(Into::into)
    }

    pub fn delete_segment(&self, segment_id: &str) -> Result<()> {
        self.db
            .write(|tx| repo::segments::delete(tx, segment_id))
            .map_err(Into::into)
    }

    // ------------------------------------------------ categories & projects

    pub fn categories(&self) -> Result<Vec<Category>> {
        self.db.read(repo::categories::list).map_err(Into::into)
    }

    pub fn create_category(&self, name: &str) -> Result<Category> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        let created = self
            .db
            .write(|tx| repo::categories::create(tx, &id, name, now))?;
        self.pipeline.lock().reload()?;
        Ok(created)
    }

    /// Refuse edits to a row the server owns, so a policy stays authoritative.
    fn guard_managed_category(&self, id: &str) -> Result<()> {
        if self.managed.is_none() {
            return Ok(());
        }
        if self
            .db
            .read(|conn| repo::managed::is_managed_category(conn, id))?
        {
            return Err(AppError::Invalid(
                "This category comes from your organization and cannot be changed here.".into(),
            ));
        }
        Ok(())
    }

    fn guard_managed_rule(&self, id: &str) -> Result<()> {
        if self.managed.is_none() {
            return Ok(());
        }
        if self
            .db
            .read(|conn| repo::managed::is_managed_rule(conn, id))?
        {
            return Err(AppError::Invalid(
                "This rule comes from your organization and cannot be changed here.".into(),
            ));
        }
        Ok(())
    }

    pub fn rename_category(&self, id: &str, name: &str) -> Result<()> {
        self.guard_managed_category(id)?;
        self.db
            .write(|tx| repo::categories::rename(tx, id, name, now_ms()))?;
        self.pipeline.lock().reload()?;
        Ok(())
    }

    pub fn delete_category(&self, id: &str) -> Result<()> {
        self.guard_managed_category(id)?;
        self.db.write(|tx| repo::categories::delete(tx, id))?;
        self.pipeline.lock().reload()?;
        Ok(())
    }

    pub fn projects(&self, include_archived: bool) -> Result<Vec<Project>> {
        self.db
            .read(|conn| repo::projects::list(conn, include_archived))
            .map_err(Into::into)
    }

    pub fn create_project(&self, name: &str) -> Result<Project> {
        let id = uuid::Uuid::new_v4().to_string();
        let created = self
            .db
            .write(|tx| repo::projects::create(tx, &id, name, now_ms()))?;
        self.pipeline.lock().reload()?;
        Ok(created)
    }

    pub fn update_project(&self, id: &str, name: &str, archived: bool) -> Result<()> {
        self.db
            .write(|tx| repo::projects::update(tx, id, name, archived, now_ms()))?;
        self.pipeline.lock().reload()?;
        Ok(())
    }

    pub fn delete_project(&self, id: &str) -> Result<()> {
        self.db.write(|tx| repo::projects::delete(tx, id))?;
        self.pipeline.lock().reload()?;
        Ok(())
    }

    // ---------------------------------------------------------------- rules

    pub fn rules(&self) -> Result<Vec<ClassificationRule>> {
        self.db.read(repo::rules::list).map_err(Into::into)
    }

    pub fn save_rule(&self, mut rule: ClassificationRule, scope: ReclassifyScope) -> Result<usize> {
        let now = now_ms();
        if rule.id.trim().is_empty() {
            rule.id = uuid::Uuid::new_v4().to_string();
            rule.created_at_ms = now;
        }
        rule.updated_at_ms = now;
        rule.validate()?;
        if !rule.id.is_empty() {
            self.guard_managed_rule(&rule.id).or_else(|err| {
                // A rule id that does not exist yet is simply a new rule.
                match err {
                    AppError::Storage(localtrack_storage::StorageError::NotFound(_)) => Ok(()),
                    other => Err(other),
                }
            })?;
        }
        self.db.write(|tx| repo::rules::upsert(tx, &rule))?;
        self.pipeline.lock().reload()?;

        match scope {
            ReclassifyScope::NewActivityOnly => Ok(0),
            ReclassifyScope::ExistingActivity => self.reclassify(None, None),
        }
    }

    pub fn delete_rule(&self, id: &str, scope: ReclassifyScope) -> Result<usize> {
        self.guard_managed_rule(id)?;
        self.db.write(|tx| repo::rules::delete(tx, id))?;
        self.pipeline.lock().reload()?;
        match scope {
            ReclassifyScope::NewActivityOnly => Ok(0),
            ReclassifyScope::ExistingActivity => self.reclassify(None, None),
        }
    }

    /// Re-apply rules to historical activity, preserving manual overrides
    /// (spec §65, §66).
    pub fn reclassify(&self, from_ms: Option<i64>, to_ms: Option<i64>) -> Result<usize> {
        let rules = self.db.read(repo::rules::list)?;
        let categories = self.db.read(repo::categories::list)?;
        let default_category_id = categories
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case("Uncategorized"))
            .map(|c| c.id.clone());
        let classifier = Classifier::new(rules, default_category_id);

        let segments = self
            .db
            .read(|conn| repo::segments::load_reclassifiable(conn, from_ms, to_ms))?;
        let now = now_ms();

        let mut updates: Vec<(String, Option<String>, Option<String>)> = Vec::new();
        for segment in &segments {
            let classification = classifier.classify(segment);
            if classification.category_id != segment.category_id
                || classification.project_id != segment.project_id
            {
                updates.push((
                    segment.id.clone(),
                    classification.category_id,
                    classification.project_id,
                ));
            }
        }

        // One transaction keeps historical reclassification atomic (spec §65).
        let changed = self.db.write(|tx| {
            let mut changed = 0usize;
            for (id, category_id, project_id) in &updates {
                if repo::segments::apply_rule_classification(
                    tx,
                    id,
                    category_id.as_deref(),
                    project_id.as_deref(),
                    now,
                )? {
                    changed += 1;
                }
            }
            Ok(changed)
        })?;
        Ok(changed)
    }

    // ----------------------------------------------------------- exclusions

    pub fn exclusions(&self) -> Result<Vec<ExclusionRule>> {
        self.db.read(repo::exclusions::list).map_err(Into::into)
    }

    pub fn save_exclusion(&self, mut rule: ExclusionRule) -> Result<ExclusionRule> {
        let now = now_ms();
        if rule.id.trim().is_empty() {
            rule.id = uuid::Uuid::new_v4().to_string();
            rule.created_at_ms = now;
        }
        rule.updated_at_ms = now;
        self.db.write(|tx| repo::exclusions::upsert(tx, &rule))?;
        self.pipeline.lock().reload()?;
        Ok(rule)
    }

    pub fn delete_exclusion(&self, id: &str) -> Result<()> {
        self.db.write(|tx| repo::exclusions::delete(tx, id))?;
        self.pipeline.lock().reload()?;
        Ok(())
    }

    // ------------------------------------------------------------- settings

    pub fn settings_view(&self) -> Result<SettingsView> {
        let health = self.db.health()?;
        Ok(SettingsView {
            settings: self.settings(),
            data_directory: paths::data_dir().display().to_string(),
            database_path: health.path.clone(),
            database_size_bytes: health.size_bytes,
            schema_version: health.schema_version,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        })
    }

    /// Update one setting and apply its side effects immediately.
    pub fn update_setting(
        self: &Arc<Self>,
        key: &str,
        value: serde_json::Value,
    ) -> Result<Settings> {
        if !localtrack_core::settings::keys::ALL.contains(&key) {
            return Err(AppError::Invalid(format!("unknown setting {key}")));
        }
        // A locked setting belongs to the administrator, not the operator.
        if let Some(managed) = &self.managed {
            if managed.is_locked(key) {
                return Err(AppError::Invalid(format!(
                    "{key} is set by your organization and cannot be changed here."
                )));
            }
        }
        let now = now_ms();
        self.db
            .write(|tx| repo::settings::set(tx, key, &value, now))?;

        let restart_collector = matches!(
            key,
            localtrack_core::settings::keys::AFK_THRESHOLD_SECONDS
                | localtrack_core::settings::keys::POLL_INTERVAL_SECONDS
        );

        {
            let mut pipeline = self.pipeline.lock();
            pipeline.reload()?;
        }
        if restart_collector {
            self.start_collector()?;
        }
        Ok(self.settings())
    }

    // ------------------------------------------------------- data lifecycle

    pub fn preview_deletion(&self, from_ms: i64, to_ms: i64) -> Result<DeletionPreview> {
        maintenance::preview_deletion(&self.db, from_ms, to_ms)
    }

    pub fn delete_range(
        &self,
        from_ms: i64,
        to_ms: i64,
        include_sessions: bool,
    ) -> Result<RetentionReport> {
        let report = maintenance::delete_range(&self.db, from_ms, to_ms, include_sessions)?;
        self.pipeline.lock().reload()?;
        Ok(report)
    }

    pub fn delete_last_minutes(
        &self,
        minutes: i64,
        include_sessions: bool,
    ) -> Result<RetentionReport> {
        let report = maintenance::delete_last(&self.db, minutes, include_sessions)?;
        self.pipeline.lock().reload()?;
        Ok(report)
    }

    pub fn delete_all(&self, include_sessions: bool) -> Result<RetentionReport> {
        let report = maintenance::delete_all(&self.db, include_sessions)?;
        self.pipeline.lock().reload()?;
        Ok(report)
    }

    pub fn backup(&self, destination: Option<PathBuf>) -> Result<PathBuf> {
        maintenance::backup(&self.db, destination.as_deref())
    }

    pub fn restore(&self, source: &Path) -> Result<PathBuf> {
        // Collectors stop first so nothing writes during the swap (spec §126).
        if let Some(collector) = self.collector.lock().as_mut() {
            let _ = collector.stop();
        }
        let safety = maintenance::restore(&self.db, source)?;
        self.pipeline.lock().reload()?;
        Ok(safety)
    }

    pub fn run_retention_now(&self) -> Result<Option<RetentionReport>> {
        let mut settings = self.settings();
        settings.last_retention_run_ms = 0;
        maintenance::run_retention_if_due(&self.db, &settings)
    }

    // --------------------------------------------------------------- export

    pub fn export(&self, request: &ExportRequest) -> Result<ExportResult> {
        exporting::run_export(&self.db, request, &self.settings())
    }

    pub fn full_urls_available(&self, from_ms: i64, to_ms: i64) -> Result<bool> {
        exporting::full_urls_available(&self.db, from_ms, to_ms)
    }

    // ---------------------------------------------------------- diagnostics

    pub fn diagnostics(&self) -> Result<Diagnostics> {
        let (tracking, stats) = {
            let pipeline = self.pipeline.lock();
            (pipeline.tracking_decision(), pipeline.stats())
        };
        let mut collectors = Vec::new();
        if let Some(collector) = self.collector.lock().as_ref() {
            collectors.push(collector.status());
        }
        collectors.push(chrome_status(self.last_browser_activity_ms()?, now_ms()));
        diagnostics::collect(&self.db, collectors, tracking, stats, now_ms())
    }

    pub fn tracking_decision(&self) -> TrackingDecision {
        self.pipeline.lock().tracking_decision()
    }

    /// Used by tests and by the native-host-less code paths.
    pub fn handle_observation(
        &self,
        observation: localtrack_core::activity::Observation,
    ) -> Result<()> {
        self.pipeline.lock().handle(observation).map_err(Into::into)
    }

    pub fn flush_open_segments(&self) -> Result<()> {
        self.pipeline
            .lock()
            .close_all_streams(now_ms())
            .map_err(Into::into)
    }
}
