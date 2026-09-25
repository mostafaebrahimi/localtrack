//! Employee mode: enrollment, policy application, and the daily report queue.
//!
//! The transport is abstracted so every rule here — what is queued, what a
//! policy may change, when a report is due — is testable without a server.

use std::collections::BTreeMap;
use std::sync::Arc;

use localtrack_core::classification::ClassificationRule;
use localtrack_core::managed::{
    merge_incoming, resolve_open_sessions, AppMode, DailyReport, Enrollment, Heartbeat,
    ManagedPolicy, MergeAction, SessionSyncItem, REPORT_SCHEMA,
};
use localtrack_core::settings::Settings;
use localtrack_core::time::{format_local_date, local_day_start_ms, now_ms, DAY_MS};
use localtrack_storage::repo::managed::{EnrollmentTimestamp, OutboxEntry, KIND_DAILY_REPORT};
use localtrack_storage::{repo, Database};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::queries;

/// What the agent asks the server for, and what it sends.
///
/// Implemented over HTTP by `localtrack-sync`; the desktop application only
/// ever talks to this trait.
pub trait SyncTransport: Send + Sync + 'static {
    /// Exchange an enrollment code for a device identity.
    fn enroll(&self, server_url: &str, request: &EnrollRequest) -> Result<EnrollResponse>;

    /// Fetch the current policy, if the server has a newer one.
    fn fetch_policy(&self, enrollment: &Enrollment) -> Result<Option<ManagedPolicy>>;

    /// Deliver one queued payload.
    fn send_report(&self, enrollment: &Enrollment, payload_json: &str) -> Result<()>;

    /// Tell the server the agent is alive.
    fn send_heartbeat(&self, enrollment: &Enrollment, heartbeat: &Heartbeat) -> Result<()>;

    /// Exchange local session changes for remote ones.
    ///
    /// Both sides may have edited the same day, so this is a two-way call: the
    /// device sends what changed here and receives what changed elsewhere.
    fn sync_sessions(
        &self,
        enrollment: &Enrollment,
        since: Option<&str>,
        local_changes: &[SessionSyncItem],
    ) -> Result<SessionSyncResult>;
}

/// What the server returns from a session exchange.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSyncResult {
    /// Opaque cursor to send back next time.
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub sessions: Vec<SessionSyncItem>,
    /// Server ids for sessions this device created offline, keyed by client id.
    #[serde(default)]
    pub assigned_ids: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollRequest {
    pub code: String,
    pub device_name: String,
    pub platform: String,
    pub app_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub employee_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollResponse {
    pub device_id: String,
    pub device_token: String,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub employee_ref: Option<String>,
    #[serde(default)]
    pub policy: Option<ManagedPolicy>,
}

/// What the interface shows about managed mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedStatus {
    pub mode: AppMode,
    pub enrolled: bool,
    pub organization: Option<String>,
    pub server_host: Option<String>,
    pub employee_ref: Option<String>,
    pub locked: bool,
    pub locked_settings: Vec<String>,
    pub policy_revision: Option<i64>,
    pub notice: Option<String>,
    pub last_report_at_ms: Option<i64>,
    pub last_heartbeat_at_ms: Option<i64>,
    pub last_policy_at_ms: Option<i64>,
    pub last_error: Option<String>,
    pub pending_reports: i64,
}

impl ManagedStatus {
    pub fn personal() -> Self {
        Self {
            mode: AppMode::Personal,
            enrolled: false,
            organization: None,
            server_host: None,
            employee_ref: None,
            locked: false,
            locked_settings: Vec::new(),
            policy_revision: None,
            notice: None,
            last_report_at_ms: None,
            last_heartbeat_at_ms: None,
            last_policy_at_ms: None,
            last_error: None,
            pending_reports: 0,
        }
    }
}

/// What one session exchange did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSyncSummary {
    pub pushed: usize,
    pub inserted: usize,
    pub updated: usize,
    pub deleted: usize,
    pub kept_local: usize,
    pub skipped: usize,
    pub closed_duplicates: usize,
}

/// One line in the "what has been sent" list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentReport {
    pub date: Option<String>,
    pub description: String,
    pub delivered_at_ms: Option<i64>,
    pub attempts: i64,
    pub last_error: Option<String>,
    /// The exact payload, so the employee can read what left their machine.
    pub payload_json: String,
}

pub struct ManagedService {
    db: Arc<Database>,
    transport: Arc<dyn SyncTransport>,
}

impl ManagedService {
    pub fn new(db: Arc<Database>, transport: Arc<dyn SyncTransport>) -> Self {
        Self { db, transport }
    }

    pub fn enrollment(&self) -> Result<Option<Enrollment>> {
        self.db
            .read(repo::managed::get_enrollment)
            .map_err(Into::into)
    }

    pub fn policy(&self) -> Result<Option<ManagedPolicy>> {
        self.db.read(repo::managed::get_policy).map_err(Into::into)
    }

    pub fn mode(&self) -> AppMode {
        match self.enrollment() {
            Ok(Some(_)) => AppMode::Employee,
            _ => AppMode::Personal,
        }
    }

    /// Settings the policy currently freezes.
    pub fn locked_settings(&self) -> Vec<String> {
        self.policy()
            .ok()
            .flatten()
            .map(|policy| policy.effective_locked_settings())
            .unwrap_or_default()
    }

    pub fn is_locked(&self, key: &str) -> bool {
        self.locked_settings().iter().any(|locked| locked == key)
    }

    pub fn status(&self) -> Result<ManagedStatus> {
        let Some(enrollment) = self.enrollment()? else {
            return Ok(ManagedStatus::personal());
        };
        let policy = self.policy()?;
        let pending = self.db.read(repo::managed::pending_count)?;

        Ok(ManagedStatus {
            mode: AppMode::Employee,
            enrolled: true,
            organization: enrollment
                .organization
                .clone()
                .or_else(|| policy.as_ref().and_then(|p| p.organization.clone())),
            server_host: Some(enrollment.server_host()),
            employee_ref: enrollment.employee_ref.clone(),
            locked: policy
                .as_ref()
                .map(|p| !p.effective_locked_settings().is_empty())
                .unwrap_or(false),
            locked_settings: policy
                .as_ref()
                .map(|p| p.effective_locked_settings())
                .unwrap_or_default(),
            policy_revision: policy.as_ref().map(|p| p.revision),
            notice: policy.as_ref().and_then(|p| p.notice.clone()),
            last_report_at_ms: enrollment.last_report_at_ms,
            last_heartbeat_at_ms: enrollment.last_heartbeat_at_ms,
            last_policy_at_ms: enrollment.last_policy_at_ms,
            last_error: enrollment.last_error,
            pending_reports: pending,
        })
    }

    // ------------------------------------------------------------- enrolling

    pub fn enroll(&self, server_url: &str, code: &str, device_name: &str) -> Result<ManagedStatus> {
        let server_url =
            localtrack_core::managed::validate_server_url(server_url).map_err(AppError::Invalid)?;
        if code.trim().is_empty() {
            return Err(AppError::Invalid("Enter the enrollment code.".into()));
        }

        let request = EnrollRequest {
            code: code.trim().to_string(),
            device_name: device_name.trim().to_string(),
            platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            employee_ref: None,
        };
        let response = self.transport.enroll(&server_url, &request)?;

        let now = now_ms();
        let enrollment = Enrollment {
            server_url,
            device_id: response.device_id,
            device_token: response.device_token,
            organization: response.organization,
            employee_ref: response.employee_ref,
            enrolled_at_ms: now,
            last_policy_at_ms: None,
            last_report_at_ms: None,
            last_heartbeat_at_ms: None,
            last_error: None,
        };
        self.db
            .write(|tx| repo::managed::save_enrollment(tx, &enrollment))?;

        if let Some(policy) = response.policy {
            self.apply_policy(&policy)?;
        }
        self.status()
    }

    /// Leave managed mode.
    ///
    /// Refused while a policy is locked: unenrolling is the one thing an admin
    /// lock has to survive, or it would not be a lock at all. Collected
    /// activity is never deleted by this.
    pub fn unenroll(&self) -> Result<()> {
        if !self.locked_settings().is_empty() {
            return Err(AppError::Invalid(
                "This device is managed by a policy. Ask your administrator to release it.".into(),
            ));
        }
        self.db.write(repo::managed::clear_enrollment)?;
        Ok(())
    }

    // --------------------------------------------------------------- policy

    /// Store a policy and apply everything it is allowed to change.
    pub fn apply_policy(&self, policy: &ManagedPolicy) -> Result<()> {
        let current = self.policy()?.map(|p| p.revision);
        if !policy.supersedes(current) {
            return Ok(());
        }

        let now = now_ms();
        let pinned = policy.pinned_settings();
        let categories = policy.categories.clone();
        let rules = policy.rules.clone();
        let policy_owned = policy.clone();

        self.db.write(move |tx| {
            repo::managed::save_policy(tx, &policy_owned, now)?;

            // Settings the policy pins, and only those.
            for (key, value) in &pinned {
                repo::settings::set(tx, key, value, now)?;
            }

            // Shared categories, so every device reports into the same buckets.
            let mut category_ids: BTreeMap<String, String> = BTreeMap::new();
            for category in &categories {
                let id =
                    repo::managed::upsert_managed_category(tx, &category.key, &category.name, now)?;
                category_ids.insert(category.key.clone(), id);
            }

            // Shared rules: a new application added centrally lands in the right
            // category on every device at the next poll.
            for rule in &rules {
                let category_id = rule
                    .category_key
                    .as_ref()
                    .and_then(|key| category_ids.get(key).cloned());
                let compiled = ClassificationRule {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: rule.name.clone(),
                    enabled: rule.enabled,
                    priority: rule.priority,
                    target_field: rule.target_field,
                    operator: rule.operator,
                    pattern: rule.pattern.clone(),
                    category_id,
                    project_id: None,
                    created_at_ms: now,
                    updated_at_ms: now,
                };
                repo::managed::upsert_managed_rule(tx, &rule.key, &compiled)?;
            }

            let category_keys: Vec<String> = categories.iter().map(|c| c.key.clone()).collect();
            let rule_keys: Vec<String> = rules.iter().map(|r| r.key.clone()).collect();
            repo::managed::retain_managed(tx, &category_keys, &rule_keys)?;
            repo::managed::touch_enrollment(tx, EnrollmentTimestamp::Policy, now, None)
        })?;

        Ok(())
    }

    pub fn poll_policy(&self) -> Result<bool> {
        let Some(enrollment) = self.enrollment()? else {
            return Ok(false);
        };
        match self.transport.fetch_policy(&enrollment) {
            Ok(Some(policy)) => {
                self.apply_policy(&policy)?;
                Ok(true)
            }
            Ok(None) => {
                let now = now_ms();
                self.db.write(|tx| {
                    repo::managed::touch_enrollment(tx, EnrollmentTimestamp::Policy, now, None)
                })?;
                Ok(false)
            }
            Err(err) => {
                self.record_error(&err.to_string())?;
                Err(err)
            }
        }
    }

    // -------------------------------------------------------------- reports

    /// Build and queue the report for one local day.
    pub fn queue_daily_report(
        &self,
        day_start_ms: i64,
        settings: &Settings,
    ) -> Result<DailyReport> {
        let Some(enrollment) = self.enrollment()? else {
            return Err(AppError::Invalid("This device is not enrolled.".into()));
        };

        let day_end = localtrack_core::time::local_day_end_ms(day_start_ms);
        let filter = localtrack_storage::ActivityFilter::default();
        let bundle = queries::report_bundle(&self.db, day_start_ms, day_end, &filter, settings)?;
        let sessions = queries::session_details(&self.db, day_start_ms, day_end, settings)?;
        let category_keys = self.db.read(repo::managed::managed_category_keys)?;

        let session_rows: Vec<_> = sessions
            .iter()
            .map(|detail| (detail.session.clone(), detail.summary.break_ms))
            .collect();

        let report = DailyReport::build(
            &enrollment.device_id,
            &format_local_date(day_start_ms),
            localtrack_core::time::offset_minutes_at(day_start_ms),
            now_ms(),
            env!("CARGO_PKG_VERSION"),
            &bundle.summary,
            &session_rows,
            &bundle.applications,
            &bundle.categories,
            &bundle.projects,
            &category_keys,
            // Workspaces travel only when the organization's policy asks for
            // them; see `Settings::share_workspace_context`.
            settings
                .share_workspace_context
                .then_some(&bundle.workspaces),
        );

        let payload = serde_json::to_string(&report).map_err(|err| {
            AppError::Invalid(format!("the report could not be serialized: {err}"))
        })?;
        let date = report.date.clone();
        let now = now_ms();
        self.db.write(move |tx| {
            repo::managed::enqueue(
                tx,
                &uuid::Uuid::new_v4().to_string(),
                KIND_DAILY_REPORT,
                Some(&date),
                &payload,
                now,
            )
        })?;

        Ok(report)
    }

    /// Try to deliver everything queued, oldest first.
    pub fn flush_outbox(&self) -> Result<usize> {
        let Some(enrollment) = self.enrollment()? else {
            return Ok(0);
        };
        let pending = self.db.read(|conn| repo::managed::pending(conn, 30))?;
        let mut delivered = 0usize;

        for entry in pending {
            match self.transport.send_report(&enrollment, &entry.payload_json) {
                Ok(()) => {
                    let now = now_ms();
                    let id = entry.id.clone();
                    self.db.write(move |tx| {
                        repo::managed::mark_delivered(tx, &id, now)?;
                        repo::managed::touch_enrollment(
                            tx,
                            EnrollmentTimestamp::Report,
                            now,
                            None,
                        )?;
                        repo::managed::prune_delivered(tx, 60).map(|_| ())
                    })?;
                    delivered += 1;
                }
                Err(err) => {
                    // Stop at the first failure: order matters and the server is
                    // probably unreachable for the rest too.
                    let now = now_ms();
                    let id = entry.id.clone();
                    let message = err.to_string();
                    self.db.write(move |tx| {
                        repo::managed::mark_failed(tx, &id, now, &message)?;
                        repo::managed::touch_enrollment(
                            tx,
                            EnrollmentTimestamp::Report,
                            now,
                            Some(&message),
                        )
                    })?;
                    break;
                }
            }
        }

        Ok(delivered)
    }

    pub fn send_heartbeat(
        &self,
        clock_state: &str,
        today_active_ms: i64,
        healthy: bool,
    ) -> Result<()> {
        let Some(enrollment) = self.enrollment()? else {
            return Ok(());
        };
        let pending = self.db.read(repo::managed::pending_count)?;
        let heartbeat = Heartbeat {
            schema: REPORT_SCHEMA,
            device_id: enrollment.device_id.clone(),
            at_ms: now_ms(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            clock_state: clock_state.to_string(),
            today_active_ms,
            tracking_healthy: healthy,
            policy_revision: self.policy()?.map(|p| p.revision),
            pending_reports: pending,
        };

        match self.transport.send_heartbeat(&enrollment, &heartbeat) {
            Ok(()) => {
                let now = now_ms();
                self.db.write(|tx| {
                    repo::managed::touch_enrollment(tx, EnrollmentTimestamp::Heartbeat, now, None)
                })?;
                Ok(())
            }
            Err(err) => {
                self.record_error(&err.to_string())?;
                Err(err)
            }
        }
    }

    /// Queue any finished day that has not been reported yet.
    ///
    /// Catches up after the machine was off, and never reports a day that is
    /// still in progress.
    pub fn queue_due_reports(&self, settings: &Settings, now: i64) -> Result<usize> {
        if self.enrollment()?.is_none() {
            return Ok(0);
        }
        let reported: Vec<String> = self.db.read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT period_key FROM sync_outbox WHERE kind = ?1 AND period_key IS NOT NULL",
            )?;
            let rows = stmt.query_map([KIND_DAILY_REPORT], |row| row.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })?;

        let today_start = local_day_start_ms(now);
        let mut queued = 0usize;
        // Up to a fortnight of catch-up, so a laptop that was off for a week
        // still delivers those days. Today is never sent: it is not over.
        for back in 1..=14 {
            let day_start = local_day_start_ms(today_start - back * DAY_MS + DAY_MS / 2);
            let date = format_local_date(day_start);
            if reported.contains(&date) {
                continue;
            }
            if !self.day_has_data(day_start)? {
                continue;
            }
            self.queue_daily_report(day_start, settings)?;
            queued += 1;
        }
        Ok(queued)
    }

    /// A day with nothing recorded is not worth a report.
    fn day_has_data(&self, day_start_ms: i64) -> Result<bool> {
        let day_end = localtrack_core::time::local_day_end_ms(day_start_ms);
        let counted = self.db.read(|conn| {
            let sessions = repo::sessions::list_sessions(conn, day_start_ms, day_end)?.len();
            let segments = repo::segments::count(
                conn,
                &localtrack_storage::ActivityFilter::for_range(day_start_ms, day_end),
            )?;
            Ok(sessions > 0 || segments > 0)
        })?;
        Ok(counted)
    }

    /// Payloads already delivered, newest first, for the disclosure page.
    pub fn sent_reports(&self, limit: i64) -> Result<Vec<SentReport>> {
        let entries = self.db.read(|conn| repo::managed::delivered(conn, limit))?;
        Ok(entries.into_iter().map(describe_entry).collect())
    }

    /// Payloads still waiting, so "what will be sent" is answerable too.
    pub fn queued_reports(&self, limit: i64) -> Result<Vec<SentReport>> {
        let entries = self.db.read(|conn| repo::managed::pending(conn, limit))?;
        Ok(entries.into_iter().map(describe_entry).collect())
    }

    // --------------------------------------------------- session synchronisation

    /// Exchange session changes with the server.
    ///
    /// Local edits that have not been accepted yet are sent, remote changes are
    /// merged in, and a conflict is resolved by the rules in
    /// [`localtrack_core::managed::sync`] rather than by whoever called last.
    pub fn sync_sessions(&self) -> Result<SessionSyncSummary> {
        let Some(enrollment) = self.enrollment()? else {
            return Ok(SessionSyncSummary::default());
        };

        let outgoing = self.collect_local_changes()?;
        let cursor = self
            .db
            .read(|conn| repo::managed::get_sync_state(conn, repo::managed::CURSOR_SESSIONS))?;

        let result = match self
            .transport
            .sync_sessions(&enrollment, cursor.as_deref(), &outgoing)
        {
            Ok(result) => result,
            Err(err) => {
                self.record_error(&err.to_string())?;
                return Err(err);
            }
        };

        let mut incoming = result.sessions.clone();
        // A server that thinks two timers are running must not be able to
        // corrupt this device's clock state (spec §15).
        let closed = resolve_open_sessions(&mut incoming);

        let now = now_ms();
        let mut summary = SessionSyncSummary {
            pushed: outgoing.len(),
            closed_duplicates: closed,
            ..Default::default()
        };

        for item in &incoming {
            let local = self.db.read(|conn| {
                repo::managed::local_record(conn, &item.client_id, item.remote_id.as_deref())
            })?;
            let outcome = merge_incoming(item, local.as_ref());
            let item = item.clone();
            match outcome.action {
                MergeAction::Insert | MergeAction::Update => {
                    self.db
                        .write(move |tx| repo::managed::apply_remote_session(tx, &item, now))?;
                    if outcome.action == MergeAction::Insert {
                        summary.inserted += 1;
                    } else {
                        summary.updated += 1;
                    }
                }
                MergeAction::Delete => {
                    self.db
                        .write(move |tx| repo::managed::apply_remote_deletion(tx, &item))?;
                    summary.deleted += 1;
                }
                MergeAction::KeepLocal => summary.kept_local += 1,
                MergeAction::Skip => summary.skipped += 1,
            }
        }

        // Everything that was sent is now the server's version too.
        let assigned = result.assigned_ids.clone();
        let sent: Vec<(String, Option<String>)> = outgoing
            .iter()
            .map(|item| {
                let remote = assigned
                    .get(&item.client_id)
                    .cloned()
                    .or_else(|| item.remote_id.clone());
                (item.client_id.clone(), remote)
            })
            .collect();
        let tombstones: Vec<String> = outgoing
            .iter()
            .filter(|item| item.is_tombstone())
            .map(|item| item.client_id.clone())
            .collect();
        let cursor_value = result.cursor.clone();

        self.db.write(move |tx| {
            for (client_id, remote_id) in &sent {
                repo::managed::clear_dirty(tx, client_id, remote_id.as_deref())?;
            }
            for client_id in &tombstones {
                repo::managed::mark_tombstone_synced(tx, client_id)?;
            }
            if let Some(cursor) = &cursor_value {
                repo::managed::set_sync_state(tx, repo::managed::CURSOR_SESSIONS, cursor, now)?;
            }
            Ok(())
        })?;

        Ok(summary)
    }

    /// Local sessions and deletions waiting to be sent.
    fn collect_local_changes(&self) -> Result<Vec<SessionSyncItem>> {
        let dirty = self
            .db
            .read(|conn| repo::managed::dirty_sessions(conn, 200))?;
        let mut out = Vec::with_capacity(dirty.len());

        for (client_id, remote_id) in dirty {
            let session = self
                .db
                .read(|conn| repo::sessions::get_session(conn, &client_id));
            let Ok(session) = session else {
                continue;
            };
            let breaks = self
                .db
                .read(|conn| repo::sessions::breaks_for_session(conn, &client_id))?;
            out.push(SessionSyncItem::from_local(&session, &breaks, remote_id));
        }

        for (client_id, remote_id, deleted_at_ms) in
            self.db.read(repo::managed::pending_tombstones)?
        {
            out.push(SessionSyncItem {
                remote_id,
                client_id,
                started_at_ms: 0,
                ended_at_ms: None,
                note: None,
                breaks: Vec::new(),
                updated_at_ms: deleted_at_ms,
                deleted_at_ms: Some(deleted_at_ms),
            });
        }

        Ok(out)
    }

    fn record_error(&self, message: &str) -> Result<()> {
        let message = message.to_string();
        self.db.write(move |tx| {
            tx.execute(
                "UPDATE managed_enrollment SET last_error = ?1 WHERE id = 1",
                [message.as_str()],
            )?;
            Ok(())
        })?;
        Ok(())
    }
}

fn describe_entry(entry: OutboxEntry) -> SentReport {
    let description = serde_json::from_str::<DailyReport>(&entry.payload_json)
        .map(|report| report.describe())
        .unwrap_or_else(|_| entry.kind.clone());
    SentReport {
        date: entry.period_key,
        description,
        delivered_at_ms: entry.delivered_at_ms,
        attempts: entry.attempts,
        last_error: entry.last_error,
        payload_json: entry.payload_json,
    }
}
