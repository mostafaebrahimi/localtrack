//! Employee mode end to end, against a fake server.

use std::sync::{Arc, Mutex};

use localtrack_app::managed::{EnrollRequest, EnrollResponse, SessionSyncResult, SyncTransport};
use localtrack_app::AppService;
use localtrack_core::classification::{RuleField, RuleOperator};
use localtrack_core::managed::policy::{LockState, ManagedCategory, ManagedRule, ReportingConfig};
use localtrack_core::managed::{
    DailyReport, Enrollment, Heartbeat, ManagedPolicy, SessionSyncItem,
};
use localtrack_core::settings::keys;
use localtrack_core::time::{local_day_start_ms, now_ms, DAY_MS};
use localtrack_storage::Database;

const HOUR: i64 = 3_600_000;

#[derive(Default)]
struct FakeServer {
    policy: Option<ManagedPolicy>,
    reports: Vec<String>,
    heartbeats: Vec<Heartbeat>,
    /// Sessions the server holds, keyed by its own id.
    sessions: Vec<SessionSyncItem>,
    received_sessions: Vec<SessionSyncItem>,
    /// When set, every call fails with this message.
    offline: Option<String>,
}

#[derive(Clone, Default)]
struct FakeTransport {
    server: Arc<Mutex<FakeServer>>,
}

impl FakeTransport {
    fn with_policy(policy: ManagedPolicy) -> Self {
        let transport = Self::default();
        transport.server.lock().unwrap().policy = Some(policy);
        transport
    }

    fn go_offline(&self, reason: &str) {
        self.server.lock().unwrap().offline = Some(reason.to_string());
    }

    fn come_online(&self) {
        self.server.lock().unwrap().offline = None;
    }

    fn reports(&self) -> Vec<String> {
        self.server.lock().unwrap().reports.clone()
    }
}

impl SyncTransport for FakeTransport {
    fn enroll(
        &self,
        _server_url: &str,
        request: &EnrollRequest,
    ) -> localtrack_app::Result<EnrollResponse> {
        let server = self.server.lock().unwrap();
        if let Some(reason) = &server.offline {
            return Err(localtrack_app::AppError::Invalid(reason.clone()));
        }
        Ok(EnrollResponse {
            device_id: format!("device-for-{}", request.code),
            device_token: "token-abc".into(),
            organization: Some("Acme".into()),
            employee_ref: Some("jdoe".into()),
            policy: server.policy.clone(),
        })
    }

    fn fetch_policy(
        &self,
        _enrollment: &Enrollment,
    ) -> localtrack_app::Result<Option<ManagedPolicy>> {
        let server = self.server.lock().unwrap();
        if let Some(reason) = &server.offline {
            return Err(localtrack_app::AppError::Invalid(reason.clone()));
        }
        Ok(server.policy.clone())
    }

    fn send_report(
        &self,
        _enrollment: &Enrollment,
        payload_json: &str,
    ) -> localtrack_app::Result<()> {
        let mut server = self.server.lock().unwrap();
        if let Some(reason) = &server.offline {
            return Err(localtrack_app::AppError::Invalid(reason.clone()));
        }
        server.reports.push(payload_json.to_string());
        Ok(())
    }

    fn send_heartbeat(
        &self,
        _enrollment: &Enrollment,
        heartbeat: &Heartbeat,
    ) -> localtrack_app::Result<()> {
        let mut server = self.server.lock().unwrap();
        if let Some(reason) = &server.offline {
            return Err(localtrack_app::AppError::Invalid(reason.clone()));
        }
        server.heartbeats.push(heartbeat.clone());
        Ok(())
    }

    fn sync_sessions(
        &self,
        _enrollment: &Enrollment,
        _since: Option<&str>,
        local_changes: &[SessionSyncItem],
    ) -> localtrack_app::Result<SessionSyncResult> {
        let mut server = self.server.lock().unwrap();
        if let Some(reason) = &server.offline {
            return Err(localtrack_app::AppError::Invalid(reason.clone()));
        }
        server
            .received_sessions
            .extend(local_changes.iter().cloned());

        // Behave like a real server: hand out an id for anything created offline.
        let mut assigned = std::collections::BTreeMap::new();
        for item in local_changes {
            if item.remote_id.is_none() && !item.is_tombstone() {
                assigned.insert(item.client_id.clone(), format!("srv-{}", item.client_id));
            }
        }

        let sessions = std::mem::take(&mut server.sessions);
        Ok(SessionSyncResult {
            cursor: Some("cursor-1".into()),
            sessions,
            assigned_ids: assigned,
        })
    }
}

fn policy(lock: LockState) -> ManagedPolicy {
    ManagedPolicy {
        revision: 4,
        organization: Some("Acme".into()),
        lock,
        locked_settings: vec![keys::TRACKING_SCOPE.into(), keys::URL_POLICY.into()],
        settings: std::collections::BTreeMap::from([
            (
                keys::TRACKING_SCOPE.to_string(),
                serde_json::json!("ALWAYS"),
            ),
            (
                keys::URL_POLICY.to_string(),
                serde_json::json!("DOMAIN_ONLY"),
            ),
        ]),
        reporting: ReportingConfig::default(),
        categories: vec![ManagedCategory {
            key: "dev".into(),
            name: "Development".into(),
        }],
        rules: vec![ManagedRule {
            key: "github".into(),
            name: "GitHub is development".into(),
            target_field: RuleField::Domain,
            operator: RuleOperator::Contains,
            pattern: "github.com".into(),
            category_key: Some("dev".into()),
            priority: 50,
            enabled: true,
        }],
        notice: Some("Daily summaries are shared with your team".into()),
    }
}

fn service(transport: FakeTransport) -> (tempfile::TempDir, Arc<AppService>, FakeTransport) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("localtrack.db"), now_ms()).unwrap());
    let service = AppService::with_transport(db, Some(Arc::new(transport.clone()))).unwrap();
    (dir, service, transport)
}

#[test]
fn a_device_starts_personal_and_local() {
    let (_dir, service, _) = service(FakeTransport::default());
    let managed = service.managed().unwrap();
    assert_eq!(managed.mode(), localtrack_core::managed::AppMode::Personal);
    assert!(!managed.status().unwrap().enrolled);
    assert!(service.locked_settings().is_empty());
    // Everything still works with no server involved at all.
    service.clock_in().unwrap();
    assert!(service.today().is_ok());
}

#[test]
fn enrolling_applies_the_policy_and_its_catalogue() {
    let (_dir, service, _) = service(FakeTransport::with_policy(policy(LockState::Locked)));

    let status = service
        .enroll("https://track.example.com", "TEAM-CODE", "Jane's laptop")
        .unwrap();
    assert!(status.enrolled);
    assert_eq!(status.organization.as_deref(), Some("Acme"));
    assert_eq!(status.server_host.as_deref(), Some("track.example.com"));
    assert_eq!(
        status.notice.as_deref(),
        Some("Daily summaries are shared with your team")
    );

    // The server's categories and rules arrive with it, so a new application is
    // categorized without the employee configuring anything.
    let categories = service.categories().unwrap();
    assert!(categories.iter().any(|c| c.name == "Development"));
    let rules = service.rules().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].pattern, "github.com");

    // Pinned settings are applied.
    let settings = service.settings_view().unwrap().settings;
    assert_eq!(settings.tracking_scope.as_str(), "ALWAYS");
    assert_eq!(settings.url_policy.as_str(), "DOMAIN_ONLY");
}

#[test]
fn enrolling_finishes_setup_and_stops_promising_local_only() {
    let (_dir, service, _) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    assert!(
        !service
            .settings_view()
            .unwrap()
            .settings
            .onboarding_completed,
        "a fresh install still needs setup"
    );

    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    assert!(
        service
            .settings_view()
            .unwrap()
            .settings
            .onboarding_completed,
        "an administrator-provisioned device does not greet the employee with a wizard"
    );
}

#[test]
fn syncing_by_hand_also_delivers_the_day_that_is_waiting() {
    let (_dir, service, transport) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    let yesterday = local_day_start_ms(now_ms() - DAY_MS);
    service
        .create_manual_session(yesterday + 9 * HOUR, yesterday + 17 * HOUR, None)
        .unwrap();

    service.sync_now().unwrap();

    assert_eq!(
        transport.reports().len(),
        1,
        "pressing sync does what a person expects, without waiting an hour"
    );
}

#[test]
fn a_locked_setting_cannot_be_changed_on_the_device() {
    let (_dir, service, _) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    let refused = service.update_setting(
        keys::TRACKING_SCOPE,
        serde_json::json!("WORK_SESSIONS_ONLY"),
    );
    assert!(refused.is_err());
    assert!(format!("{refused:?}").contains("organization"));

    // Anything the policy did not lock is still the employee's to change.
    assert!(service
        .update_setting(keys::THEME, serde_json::json!("dark"))
        .is_ok());
    assert!(service
        .update_setting(keys::AFK_THRESHOLD_SECONDS, serde_json::json!(300))
        .is_ok());
}

#[test]
fn server_owned_categories_and_rules_are_read_only() {
    let (_dir, service, _) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    let development = service
        .categories()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "Development")
        .unwrap();
    assert!(service.rename_category(&development.id, "Mine").is_err());
    assert!(service.delete_category(&development.id).is_err());

    let rule = service.rules().unwrap().remove(0);
    assert!(service
        .delete_rule(
            &rule.id,
            localtrack_app::service::ReclassifyScope::NewActivityOnly
        )
        .is_err());

    // The employee can still add their own alongside them.
    assert!(service.create_category("My own").is_ok());
}

#[test]
fn an_unlocked_policy_leaves_the_employee_in_control() {
    let (_dir, service, _) = service(FakeTransport::with_policy(policy(LockState::Unlocked)));
    let managed = service.managed().unwrap();
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    assert!(service.locked_settings().is_empty());
    assert!(service
        .update_setting(
            keys::TRACKING_SCOPE,
            serde_json::json!("WORK_SESSIONS_ONLY")
        )
        .is_ok());
    // And they may leave managed mode.
    assert!(service.unenroll().is_ok());
    assert_eq!(managed.mode(), localtrack_core::managed::AppMode::Personal);
}

#[test]
fn a_locked_device_cannot_unenrol_itself() {
    let (_dir, service, _) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    let refused = service.unenroll();
    assert!(refused.is_err());
    assert!(format!("{refused:?}").contains("administrator"));
}

#[test]
fn a_daily_report_is_queued_delivered_and_visible_to_the_employee() {
    let (_dir, service, transport) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    let managed = service.managed().unwrap();
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    // A day of work.
    let yesterday = local_day_start_ms(now_ms() - DAY_MS);
    service
        .create_manual_session(
            yesterday + 9 * HOUR,
            yesterday + 17 * HOUR,
            Some("Sprint".into()),
        )
        .unwrap();

    let report = managed
        .queue_daily_report(yesterday, &service.settings_view().unwrap().settings)
        .unwrap();
    assert_eq!(report.totals.clocked_ms, 8 * HOUR);

    assert_eq!(managed.queued_reports(10).unwrap().len(), 1);
    assert_eq!(managed.flush_outbox().unwrap(), 1);

    let sent = transport.reports();
    assert_eq!(sent.len(), 1);
    // Aggregates only: no session note, no titles, no URLs.
    assert!(!sent[0].contains("Sprint"));
    assert!(!sent[0].contains("url"));
    assert!(sent[0].contains("\"clockedMs\":28800000"));

    // The employee can see exactly what was sent.
    let visible = managed.sent_reports(10).unwrap();
    assert_eq!(visible.len(), 1);
    assert!(visible[0].description.contains("tracked"));
    let parsed: DailyReport = serde_json::from_str(&visible[0].payload_json).unwrap();
    assert_eq!(parsed.totals.clocked_ms, 8 * HOUR);
}

#[test]
fn a_daily_report_never_carries_a_timer_description() {
    // Timer descriptions travel with session sync, which the interface
    // discloses. A daily report is aggregates only.
    let (_dir, service, transport) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    let yesterday = local_day_start_ms(now_ms() - DAY_MS);
    service
        .create_manual_session(
            yesterday + 9 * HOUR,
            yesterday + 10 * HOUR,
            Some("Merger due diligence for Northwind".into()),
        )
        .unwrap();

    let managed = service.managed().unwrap();
    managed
        .queue_daily_report(yesterday, &service.settings_view().unwrap().settings)
        .unwrap();
    managed.flush_outbox().unwrap();

    let sent = transport.reports();
    assert_eq!(sent.len(), 1);
    assert!(!sent[0].contains("Northwind"));
    assert!(!sent[0].contains("due diligence"));
    assert!(sent[0].contains("\"clockedMs\""));
}

#[test]
fn an_offline_day_is_delivered_when_the_server_returns() {
    let (_dir, service, transport) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    let managed = service.managed().unwrap();
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    let yesterday = local_day_start_ms(now_ms() - DAY_MS);
    service
        .create_manual_session(yesterday + 9 * HOUR, yesterday + 12 * HOUR, None)
        .unwrap();
    managed
        .queue_daily_report(yesterday, &service.settings_view().unwrap().settings)
        .unwrap();

    transport.go_offline("The server could not be reached.");
    assert_eq!(managed.flush_outbox().unwrap(), 0);
    assert_eq!(
        managed.queued_reports(10).unwrap().len(),
        1,
        "kept, not dropped"
    );
    let status = managed.status().unwrap();
    assert_eq!(status.pending_reports, 1);
    assert!(status.last_error.is_some());

    transport.come_online();
    assert_eq!(managed.flush_outbox().unwrap(), 1);
    assert_eq!(managed.status().unwrap().pending_reports, 0);
    assert_eq!(transport.reports().len(), 1);
}

#[test]
fn queueing_a_day_twice_replaces_it_rather_than_duplicating() {
    let (_dir, service, _) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    let managed = service.managed().unwrap();
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    let yesterday = local_day_start_ms(now_ms() - DAY_MS);
    let settings = service.settings_view().unwrap().settings;
    managed.queue_daily_report(yesterday, &settings).unwrap();
    service
        .create_manual_session(yesterday + 9 * HOUR, yesterday + 10 * HOUR, None)
        .unwrap();
    managed.queue_daily_report(yesterday, &settings).unwrap();

    let queued = managed.queued_reports(10).unwrap();
    assert_eq!(queued.len(), 1, "one report per day");
    let parsed: DailyReport = serde_json::from_str(&queued[0].payload_json).unwrap();
    assert_eq!(parsed.totals.clocked_ms, HOUR, "the newer figures win");
}

#[test]
fn an_older_policy_revision_is_ignored() {
    let transport = FakeTransport::with_policy(policy(LockState::Locked));
    let (_dir, service, transport) = service(transport);
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    // The server rolls back to an older revision that unlocks everything.
    let mut stale = policy(LockState::Unlocked);
    stale.revision = 1;
    transport.server.lock().unwrap().policy = Some(stale);
    service.poll_policy().unwrap();

    assert_eq!(
        service.locked_settings().len(),
        2,
        "a replayed older policy cannot unlock the device"
    );
}

#[test]
fn heartbeats_report_status_without_activity_detail() {
    let (_dir, service, transport) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    let managed = service.managed().unwrap();
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();
    service.clock_in().unwrap();

    managed.send_heartbeat("CLOCKED_IN", 1_234, true).unwrap();
    let beats = transport.server.lock().unwrap().heartbeats.clone();
    assert_eq!(beats.len(), 1);
    assert_eq!(beats[0].clock_state, "CLOCKED_IN");
    assert_eq!(beats[0].today_active_ms, 1_234);

    let json = serde_json::to_string(&beats[0]).unwrap();
    assert!(!json.contains("url"));
    assert!(!json.contains("title"));
}

#[test]
fn missed_days_are_caught_up_but_today_is_never_reported_early() {
    let (_dir, service, _) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    let managed = service.managed().unwrap();
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    // Two earlier days of work, and some time today that is not over yet.
    for back in [1_i64, 2] {
        let day = local_day_start_ms(now_ms() - back * DAY_MS);
        service
            .create_manual_session(day + 9 * HOUR, day + 17 * HOUR, None)
            .unwrap();
    }
    service.clock_in().unwrap();

    let settings = service.settings_view().unwrap().settings;
    let queued = managed.queue_due_reports(&settings, now_ms()).unwrap();
    assert_eq!(queued, 2, "both finished days, and only those");

    let today = localtrack_core::time::format_local_date(now_ms());
    let dates: Vec<String> = managed
        .queued_reports(50)
        .unwrap()
        .into_iter()
        .filter_map(|r| r.date)
        .collect();
    assert!(
        !dates.contains(&today),
        "a day still in progress is not reported"
    );

    // Running it again does not queue the same days twice.
    assert_eq!(managed.queue_due_reports(&settings, now_ms()).unwrap(), 0);
}

// ------------------------------------------------- two-way session synchronisation

fn remote_session(client_id: &str, start: i64, end: Option<i64>, note: &str) -> SessionSyncItem {
    SessionSyncItem {
        remote_id: Some(format!("srv-{client_id}")),
        client_id: client_id.to_string(),
        started_at_ms: start,
        ended_at_ms: end,
        note: Some(note.to_string()),
        breaks: vec![],
        updated_at_ms: now_ms(),
        deleted_at_ms: None,
    }
}

#[test]
fn a_timer_started_on_the_web_arrives_on_the_device() {
    let (_dir, service, transport) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    let managed = service.managed().unwrap();
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    let start = now_ms() - HOUR;
    transport.server.lock().unwrap().sessions = vec![remote_session(
        "web-1",
        start,
        Some(now_ms()),
        "Client call",
    )];

    let summary = managed.sync_sessions().unwrap();
    assert_eq!(summary.inserted, 1);

    let sessions = service.sessions(start - HOUR, now_ms() + HOUR).unwrap();
    let arrived = sessions
        .iter()
        .find(|detail| detail.session.note.as_deref() == Some("Client call"))
        .expect("the web session is on the device");
    assert_eq!(arrived.session.started_at_ms, start);
}

#[test]
fn a_session_created_offline_is_pushed_and_gets_a_server_id() {
    let (_dir, service, transport) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    let managed = service.managed().unwrap();
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    let start = now_ms() - 2 * HOUR;
    service
        .create_manual_session(start, start + HOUR, Some("Offline work".into()))
        .unwrap();

    let summary = managed.sync_sessions().unwrap();
    assert!(summary.pushed >= 1);

    let received = transport.server.lock().unwrap().received_sessions.clone();
    let pushed = received
        .iter()
        .find(|item| item.note.as_deref() == Some("Offline work"))
        .expect("the offline session reached the server");
    assert!(pushed.remote_id.is_none(), "it had no server id yet");

    // The id the server assigned is remembered, and nothing stays dirty.
    let dirty = service
        .database()
        .read(|conn| localtrack_storage::repo::managed::dirty_sessions(conn, 10))
        .unwrap();
    assert!(dirty.is_empty(), "everything sent is now clean");

    // A second sync has nothing new to push.
    let again = managed.sync_sessions().unwrap();
    assert_eq!(again.pushed, 0);
}

#[test]
fn a_local_edit_is_never_lost_to_a_stale_server_copy() {
    let (_dir, service, transport) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    let managed = service.managed().unwrap();
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    let start = now_ms() - 3 * HOUR;
    let session = service
        .create_manual_session(start, start + HOUR, Some("Local truth".into()))
        .unwrap();

    // The server sends an older version of the same session.
    let mut stale = remote_session(&session.id, start, Some(start + HOUR), "Server version");
    stale.updated_at_ms = session.updated_at_ms - 60_000;
    transport.server.lock().unwrap().sessions = vec![stale];

    let summary = managed.sync_sessions().unwrap();
    assert_eq!(summary.kept_local, 1);

    let sessions = service.sessions(start - HOUR, now_ms()).unwrap();
    let kept = sessions
        .iter()
        .find(|d| d.session.id == session.id)
        .unwrap();
    assert_eq!(kept.session.note.as_deref(), Some("Local truth"));
}

#[test]
fn a_deletion_on_the_device_reaches_the_server_and_stays_deleted() {
    let (_dir, service, transport) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    let managed = service.managed().unwrap();
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    let start = now_ms() - 4 * HOUR;
    let session = service
        .create_manual_session(start, start + HOUR, Some("Mistake".into()))
        .unwrap();
    managed.sync_sessions().unwrap();
    service.delete_session(&session.id).unwrap();

    let summary = managed.sync_sessions().unwrap();
    assert!(summary.pushed >= 1);
    let received = transport.server.lock().unwrap().received_sessions.clone();
    assert!(
        received.iter().any(|item| item.is_tombstone()),
        "the deletion travelled as a tombstone"
    );

    // The server replaying the row as it was before the deletion must not
    // resurrect it. (A genuinely newer server edit still wins — that is a
    // re-creation, not a replay.)
    let mut replay = remote_session(&session.id, start, Some(start + HOUR), "Mistake");
    replay.updated_at_ms = session.updated_at_ms;
    transport.server.lock().unwrap().sessions = vec![replay];
    let summary = managed.sync_sessions().unwrap();
    assert_eq!(summary.skipped, 1);
    let sessions = service.sessions(start - HOUR, now_ms()).unwrap();
    assert!(sessions.iter().all(|d| d.session.id != session.id));
}

#[test]
fn two_running_timers_from_the_server_are_reconciled_to_one() {
    let (_dir, service, transport) = service(FakeTransport::with_policy(policy(LockState::Locked)));
    let managed = service.managed().unwrap();
    service
        .enroll("https://track.example.com", "CODE", "laptop")
        .unwrap();

    let start = now_ms() - 2 * HOUR;
    transport.server.lock().unwrap().sessions = vec![
        remote_session("open-a", start, None, "Left running on the web"),
        remote_session("open-b", start + HOUR, None, "Started on the phone"),
    ];

    let summary = managed.sync_sessions().unwrap();
    assert_eq!(summary.closed_duplicates, 1);

    let open: Vec<_> = service
        .sessions(start - HOUR, now_ms() + HOUR)
        .unwrap()
        .into_iter()
        .filter(|d| d.session.ended_at_ms.is_none())
        .collect();
    assert_eq!(open.len(), 1, "only one timer may be running");
}

#[test]
fn syncing_does_nothing_at_all_when_the_device_is_not_enrolled() {
    let (_dir, service, transport) = service(FakeTransport::default());
    let managed = service.managed().unwrap();

    let start = now_ms() - HOUR;
    service
        .create_manual_session(start, now_ms(), Some("Private".into()))
        .unwrap();

    let summary = managed.sync_sessions().unwrap();
    assert_eq!(summary, Default::default());
    assert!(transport
        .server
        .lock()
        .unwrap()
        .received_sessions
        .is_empty());
}
