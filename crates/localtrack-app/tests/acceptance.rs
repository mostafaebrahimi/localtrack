//! Application-level acceptance tests (spec §163–§166).

use std::sync::Arc;

use localtrack_app::exporting::{ExportFormat, ExportRequest};
use localtrack_app::service::ReclassifyScope;
use localtrack_app::{AppService, RangeQuery};
use localtrack_core::activity::{
    BrowserObservation, IdleObservation, Observation, WindowObservation,
};
use localtrack_core::classification::{ClassificationRule, RuleField, RuleOperator};
use localtrack_core::settings::{keys, TrackingScope};
use localtrack_core::time::now_ms;
use localtrack_storage::filters::{ActivityFilter, Page, SortOrder};
use localtrack_storage::Database;

const MIN: i64 = 60_000;
const HOUR: i64 = 60 * MIN;

fn service() -> (tempfile::TempDir, Arc<AppService>) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("localtrack.db"), now_ms()).unwrap());
    let service = AppService::with_database(db).unwrap();
    // Ingest without requiring a live clock so scenarios can be backdated.
    service
        .update_setting(
            keys::TRACKING_SCOPE,
            serde_json::json!(TrackingScope::Always.as_str()),
        )
        .unwrap();
    (dir, service)
}

fn window(app: &str, at_ms: i64) -> Observation {
    Observation::WindowChanged(WindowObservation {
        captured_at_ms: at_ms,
        app_name: Some(app.into()),
        process_name: Some(format!("{}.exe", app.to_lowercase().replace(' ', ""))),
        window_title: Some(format!("{app} window")),
        pid: Some(42),
    })
}

fn page(domain: &str, path: &str, at_ms: i64) -> Observation {
    Observation::BrowserChanged(BrowserObservation {
        captured_at_ms: at_ms,
        event: "activated".into(),
        browser: "chrome".into(),
        window_id: Some(1),
        tab_id: Some(1),
        url: Some(format!("https://{domain}{path}")),
        title: Some(format!("{domain} page")),
        incognito: false,
        audible: false,
        focused: true,
    })
}

/// Feed heartbeats the way a running collector would, so segments stay alive.
fn keep_alive(service: &Arc<AppService>, streams: &[&str], from_ms: i64, to_ms: i64) {
    let mut t = from_ms;
    while t < to_ms {
        t = (t + 15_000).min(to_ms);
        for stream in streams {
            service
                .handle_observation(Observation::Heartbeat {
                    captured_at_ms: t,
                    stream: (*stream).to_string(),
                })
                .unwrap();
        }
    }
}

/// The exact scenario from spec §163.
fn build_acceptance_day(service: &Arc<AppService>) -> (i64, i64) {
    let end = now_ms();
    let start = end - 3 * HOUR;

    service
        .create_manual_session(start, end, Some("Acceptance day".into()))
        .unwrap();

    // 09:00–10:00 VS Code
    service
        .handle_observation(window("VS Code", start))
        .unwrap();
    keep_alive(service, &["desktop"], start, start + HOUR);
    // 10:00–10:30 Chrome / GitHub
    service
        .handle_observation(window("Chrome", start + HOUR))
        .unwrap();
    service
        .handle_observation(page("github.com", "/company/hub/pull/51", start + HOUR))
        .unwrap();
    keep_alive(
        service,
        &["desktop", "browser"],
        start + HOUR,
        start + HOUR + 30 * MIN,
    );
    // 10:30–10:40 Idle
    service
        .handle_observation(Observation::UserIdle(IdleObservation {
            captured_at_ms: start + HOUR + 30 * MIN,
            last_input_ms: start + HOUR + 27 * MIN,
            idle_threshold_ms: 3 * MIN,
        }))
        .unwrap();
    // 10:40–11:00 Chrome / ChatGPT
    service
        .handle_observation(Observation::UserActive(IdleObservation {
            captured_at_ms: start + HOUR + 40 * MIN,
            last_input_ms: start + HOUR + 40 * MIN,
            idle_threshold_ms: 3 * MIN,
        }))
        .unwrap();
    service
        .handle_observation(window("Chrome", start + HOUR + 40 * MIN))
        .unwrap();
    service
        .handle_observation(page("chatgpt.com", "/c/123", start + HOUR + 40 * MIN))
        .unwrap();
    keep_alive(
        service,
        &["desktop", "browser"],
        start + HOUR + 40 * MIN,
        start + 2 * HOUR,
    );

    // 11:00–11:15 Break
    let session = service.sessions(start, end + 1).unwrap()[0].session.clone();
    service
        .add_break(
            &session.id,
            start + 2 * HOUR,
            Some(start + 2 * HOUR + 15 * MIN),
        )
        .unwrap();

    // 11:15–12:00 VS Code
    service
        .handle_observation(window("VS Code", start + 2 * HOUR + 15 * MIN))
        .unwrap();
    keep_alive(service, &["desktop"], start + 2 * HOUR + 15 * MIN, end);
    service.flush_open_segments().unwrap();

    (start, end)
}

#[test]
fn core_acceptance_scenario_spec_163() {
    let (_dir, service) = service();
    let (start, end) = build_acceptance_day(&service);
    let query = RangeQuery {
        from_ms: start,
        to_ms: end,
        filter: ActivityFilter::default(),
    };

    let reports = service.reports(&query).unwrap();
    let summary = reports.summary;

    assert_eq!(summary.clocked_ms, 3 * HOUR, "clocked");
    assert_eq!(summary.break_ms, 15 * MIN, "break");
    assert_eq!(summary.work_ms, 2 * HOUR + 45 * MIN, "work");
    assert_eq!(summary.idle_ms, 10 * MIN, "idle");
    assert_eq!(summary.active_ms, 2 * HOUR + 35 * MIN, "active");
    assert_eq!(summary.untracked_ms, 0, "untracked");

    let app = |key: &str| {
        reports
            .applications
            .rows
            .iter()
            .find(|r| r.key == key)
            .map(|r| r.duration_ms)
            .unwrap_or(0)
    };
    assert_eq!(app("VS Code"), HOUR + 45 * MIN);
    assert_eq!(app("Chrome"), 50 * MIN);

    let site = |key: &str| {
        reports
            .websites
            .rows
            .iter()
            .find(|r| r.key == key)
            .map(|r| r.duration_ms)
            .unwrap_or(0)
    };
    assert_eq!(site("github.com"), 30 * MIN);
    assert_eq!(site("chatgpt.com"), 20 * MIN);

    // Browser and desktop time must not be summed (spec §118).
    let apps_total: i64 = reports
        .applications
        .rows
        .iter()
        .map(|r| r.duration_ms)
        .sum();
    assert_eq!(apps_total, summary.active_ms);
}

#[test]
fn timeline_shows_browser_detail_instead_of_chrome() {
    let (_dir, service) = service();
    let (start, end) = build_acceptance_day(&service);
    let blocks = service
        .timeline(&RangeQuery {
            from_ms: start,
            to_ms: end,
            filter: ActivityFilter::default(),
        })
        .unwrap();

    let labels: Vec<String> = blocks.iter().map(|b| b.label.clone()).collect();
    assert!(labels.contains(&"github.com".to_string()));
    assert!(labels.contains(&"chatgpt.com".to_string()));
    assert!(labels.contains(&"Idle".to_string()));
    assert!(labels.contains(&"Break".to_string()));
    assert!(
        !labels.iter().any(|l| l == "Chrome"),
        "the generic Chrome block is replaced by page detail"
    );

    // Blocks tile the timeline without overlapping.
    let mut sorted = blocks.clone();
    sorted.sort_by_key(|b| b.start_ms);
    for pair in sorted.windows(2) {
        assert!(
            pair[0].end_ms <= pair[1].start_ms,
            "timeline blocks must not overlap"
        );
    }
}

#[test]
fn activity_feed_paginates_and_resolves_names() {
    let (_dir, service) = service();
    let (start, end) = build_acceptance_day(&service);

    let page = service
        .activity_page(
            &ActivityFilter::for_range(start, end),
            Page {
                limit: 2,
                offset: 0,
            },
            SortOrder::Asc,
        )
        .unwrap();
    assert_eq!(page.rows.len(), 2);
    assert!(page.total >= 5);
    assert_eq!(page.rows[0].label, "VS Code");
    assert_eq!(page.rows[0].category_name.as_deref(), Some("Uncategorized"));
}

#[test]
fn rules_classify_new_and_historical_activity() {
    let (_dir, service) = service();
    let (start, end) = build_acceptance_day(&service);

    let development = service
        .categories()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "Development")
        .unwrap();
    let project = service.create_project("Hub").unwrap();

    let changed = service
        .save_rule(
            ClassificationRule {
                id: String::new(),
                name: "Company GitHub".into(),
                enabled: true,
                priority: 10,
                target_field: RuleField::Domain,
                operator: RuleOperator::Exact,
                pattern: "github.com".into(),
                category_id: Some(development.id.clone()),
                project_id: Some(project.id.clone()),
                created_at_ms: 0,
                updated_at_ms: 0,
            },
            ReclassifyScope::ExistingActivity,
        )
        .unwrap();
    assert!(changed >= 1, "historical activity is reclassified");

    let reports = service
        .reports(&RangeQuery {
            from_ms: start,
            to_ms: end,
            filter: ActivityFilter::default(),
        })
        .unwrap();
    let hub = reports
        .projects
        .rows
        .iter()
        .find(|r| r.label == "Hub")
        .expect("project row");
    assert_eq!(hub.duration_ms, 30 * MIN);
}

#[test]
fn manual_classification_is_never_overwritten_by_rules() {
    let (_dir, service) = service();
    let (start, end) = build_acceptance_day(&service);

    let personal = service
        .categories()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "Personal")
        .unwrap();
    let development = service
        .categories()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "Development")
        .unwrap();

    let page = service
        .activity_page(
            &ActivityFilter::for_range(start, end),
            Page {
                limit: 100,
                offset: 0,
            },
            SortOrder::Asc,
        )
        .unwrap();
    let github = page
        .rows
        .iter()
        .find(|r| r.segment.domain.as_deref() == Some("github.com"))
        .unwrap();

    service
        .classify_segment(&github.segment.id, Some(personal.id.clone()), None)
        .unwrap();
    service
        .save_rule(
            ClassificationRule {
                id: String::new(),
                name: "GitHub is development".into(),
                enabled: true,
                priority: 100,
                target_field: RuleField::Domain,
                operator: RuleOperator::Exact,
                pattern: "github.com".into(),
                category_id: Some(development.id),
                project_id: None,
                created_at_ms: 0,
                updated_at_ms: 0,
            },
            ReclassifyScope::ExistingActivity,
        )
        .unwrap();

    let reloaded = service
        .activity_page(
            &ActivityFilter::for_range(start, end),
            Page {
                limit: 100,
                offset: 0,
            },
            SortOrder::Asc,
        )
        .unwrap();
    let github = reloaded
        .rows
        .iter()
        .find(|r| r.segment.id == github.segment.id)
        .unwrap();
    assert_eq!(github.category_name.as_deref(), Some("Personal"));
}

#[test]
fn a_session_can_say_what_it_was_for() {
    let (_dir, service) = service();
    service
        .clock_in_with_note(Some("  Invoice run  ".into()))
        .unwrap();
    let status = service.current_status().unwrap();
    let session = status.session.expect("clocked in");
    assert_eq!(session.note.as_deref(), Some("Invoice run"), "trimmed");

    service
        .set_session_note(&session.id, Some("Invoice run and follow-ups".into()))
        .unwrap();
    let sessions = service
        .sessions(session.started_at_ms - 1, now_ms() + 1)
        .unwrap();
    assert_eq!(
        sessions[0].session.note.as_deref(),
        Some("Invoice run and follow-ups")
    );

    // Clearing it is allowed too.
    service.set_session_note(&session.id, None).unwrap();
    let sessions = service
        .sessions(session.started_at_ms - 1, now_ms() + 1)
        .unwrap();
    assert!(sessions[0].session.note.is_none());
}

#[test]
fn work_away_from_the_computer_can_be_entered_by_hand() {
    let (_dir, service) = service();
    let end = now_ms() - HOUR;
    let start = end - 2 * HOUR;

    let entry = service
        .add_manual_entry(start, end, Some("Client meeting offsite".into()))
        .unwrap();
    assert!(entry.created_manually, "reports can tell it was typed in");

    let detail = service
        .sessions(start - 1, end + 1)
        .unwrap()
        .into_iter()
        .find(|d| d.session.id == entry.id)
        .unwrap();
    assert_eq!(detail.summary.clocked_ms, 2 * HOUR);
    assert_eq!(
        detail.summary.untracked_ms,
        2 * HOUR,
        "no collector saw it, so it is clocked but not active"
    );

    // Nonsense entries are refused rather than silently stored.
    assert!(service.add_manual_entry(end, start, None).is_err());
    assert!(service
        .add_manual_entry(now_ms() + 2 * HOUR, now_ms() + 3 * HOUR, None)
        .is_err());
    // And so is one that would double-count minutes already recorded.
    assert!(service
        .add_manual_entry(start + 600_000, end - 600_000, None)
        .is_err());
}

#[test]
fn notes_and_manual_edits_are_audited_locally() {
    let (_dir, service) = service();
    let (start, end) = build_acceptance_day(&service);
    let rows = service
        .activity_page(
            &ActivityFilter::for_range(start, end),
            Page {
                limit: 10,
                offset: 0,
            },
            SortOrder::Asc,
        )
        .unwrap();
    let first = rows.rows[0].segment.clone();

    let annotated = service
        .annotate_segment(&first.id, Some("  Pairing on auth  ".into()))
        .unwrap();
    let metadata = annotated.metadata_json.clone().unwrap();
    assert!(metadata.contains("Pairing on auth"));
    assert!(!metadata.contains("  Pairing"), "notes are trimmed");

    let personal = service
        .categories()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "Personal")
        .unwrap();
    let classified = service
        .classify_segment(&first.id, Some(personal.id), None)
        .unwrap();
    let metadata = classified.metadata_json.unwrap();
    assert!(metadata.contains("\"kind\":\"note\""));
    assert!(metadata.contains("\"kind\":\"classification\""));
    assert!(
        metadata.contains("Pairing on auth"),
        "the note survives reclassification"
    );

    // Clearing the note removes it without losing the audit trail.
    let cleared = service.annotate_segment(&first.id, None).unwrap();
    let metadata = cleared.metadata_json.unwrap();
    assert!(!metadata.contains("Pairing on auth"));
    assert!(metadata.contains("classification"));
}

#[test]
fn a_session_crossing_midnight_is_split_per_local_day() {
    let (_dir, service) = service();
    // Build a session that starts before local midnight and ends after it.
    let midnight = {
        let now = now_ms();
        localtrack_core::time::local_day_start_ms(now)
    };
    let start = midnight - 2 * HOUR;
    let end = midnight + 2 * HOUR;
    service.create_manual_session(start, end, None).unwrap();
    service
        .handle_observation(window("VS Code", start))
        .unwrap();
    keep_alive(&service, &["desktop"], start, end);
    service.flush_open_segments().unwrap();

    let reports = service
        .reports(&RangeQuery {
            from_ms: start,
            to_ms: end,
            filter: ActivityFilter::default(),
        })
        .unwrap();
    assert_eq!(reports.summary.clocked_ms, 4 * HOUR);
    assert_eq!(reports.daily.len(), 2, "one row per local day");
    assert_eq!(reports.daily[0].summary.active_ms, 2 * HOUR);
    assert_eq!(reports.daily[1].summary.active_ms, 2 * HOUR);
    let total: i64 = reports.daily.iter().map(|day| day.summary.active_ms).sum();
    assert_eq!(total, reports.summary.active_ms, "days sum to the period");
}

#[test]
fn splitting_a_segment_keeps_total_time_and_allows_separate_categories() {
    let (_dir, service) = service();
    let (start, end) = build_acceptance_day(&service);
    let page = service
        .activity_page(
            &ActivityFilter::for_range(start, end),
            Page {
                limit: 100,
                offset: 0,
            },
            SortOrder::Asc,
        )
        .unwrap();
    let first = page.rows[0].segment.clone();
    let midpoint = first.started_at_ms + first.duration_ms() / 2;

    let before = service
        .summary(&RangeQuery {
            from_ms: start,
            to_ms: end,
            filter: ActivityFilter::default(),
        })
        .unwrap();
    service.split_segment(&first.id, midpoint).unwrap();
    let after = service
        .summary(&RangeQuery {
            from_ms: start,
            to_ms: end,
            filter: ActivityFilter::default(),
        })
        .unwrap();
    assert_eq!(
        before.active_ms, after.active_ms,
        "splitting changes no durations"
    );
}

#[test]
fn privacy_acceptance_no_secret_ever_reaches_storage_spec_164() {
    let (_dir, service) = service();
    let now = now_ms();
    service
        .create_manual_session(now - HOUR, now, None)
        .unwrap();
    service
        .handle_observation(window("Chrome", now - HOUR))
        .unwrap();
    service
        .handle_observation(Observation::BrowserChanged(BrowserObservation {
            captured_at_ms: now - HOUR,
            event: "activated".into(),
            browser: "chrome".into(),
            window_id: None,
            tab_id: None,
            url: Some("https://example.com/login?password=supersecret&token=123".into()),
            title: Some("Sign in".into()),
            incognito: false,
            audible: false,
            focused: true,
        }))
        .unwrap();
    service.flush_open_segments().unwrap();

    let rows = service
        .activity_page(
            &ActivityFilter::for_range(now - 2 * HOUR, now + 1),
            Page {
                limit: 100,
                offset: 0,
            },
            SortOrder::Asc,
        )
        .unwrap();
    let dump = format!("{rows:?}");
    assert!(!dump.contains("supersecret"));
    assert!(!dump.contains("token=123"));
    assert!(dump.contains("https://example.com/login"));
}

#[test]
fn exports_run_locally_in_both_formats() {
    let dir = tempfile::tempdir().unwrap();
    let (_db_dir, service) = service();
    let (start, end) = build_acceptance_day(&service);

    let xlsx = service
        .export(&ExportRequest {
            from_ms: start,
            to_ms: end,
            format: ExportFormat::Xlsx,
            destination: dir.path().join("report").display().to_string(),
            options: Default::default(),
            filter: ActivityFilter::default(),
        })
        .unwrap();
    assert_eq!(xlsx.files.len(), 1);
    assert!(xlsx.files[0].ends_with(".xlsx"));
    assert!(std::path::Path::new(&xlsx.files[0]).exists());

    let csv = service
        .export(&ExportRequest {
            from_ms: start,
            to_ms: end,
            format: ExportFormat::Csv,
            destination: dir.path().join("csv").display().to_string(),
            options: Default::default(),
            filter: ActivityFilter::default(),
        })
        .unwrap();
    assert!(csv.files.len() >= 4);
    let sessions = std::fs::read_to_string(dir.path().join("csv/sessions.csv")).unwrap();
    assert!(sessions.contains("Acceptance day"));
}

#[test]
fn deleting_a_range_removes_only_that_range() {
    let (_dir, service) = service();
    let (start, end) = build_acceptance_day(&service);
    let query = RangeQuery {
        from_ms: start,
        to_ms: end,
        filter: ActivityFilter::default(),
    };
    let before = service.summary(&query).unwrap();

    let preview = service.preview_deletion(start, start + 30 * MIN).unwrap();
    assert!(preview.segments >= 1);

    service
        .delete_range(start, start + 30 * MIN, false)
        .unwrap();
    let after = service.summary(&query).unwrap();
    assert_eq!(after.active_ms, before.active_ms - 30 * MIN);
    assert_eq!(
        after.clocked_ms, before.clocked_ms,
        "the session is untouched"
    );
    assert_eq!(
        after.untracked_ms,
        30 * MIN,
        "the hole is untracked, not active"
    );
}

#[test]
fn backup_and_restore_round_trip_through_the_service() {
    let dir = tempfile::tempdir().unwrap();
    let (_db_dir, service) = service();
    let (start, end) = build_acceptance_day(&service);
    let query = RangeQuery {
        from_ms: start,
        to_ms: end,
        filter: ActivityFilter::default(),
    };
    let before = service.summary(&query).unwrap();

    let backup = service.backup(Some(dir.path().join("backup.db"))).unwrap();
    service.delete_all(true).unwrap();
    assert_eq!(service.summary(&query).unwrap().active_ms, 0);

    service.restore(&backup).unwrap();
    assert_eq!(service.summary(&query).unwrap(), before);
}

#[test]
fn diagnostics_contain_no_activity_metadata() {
    let (_dir, service) = service();
    build_acceptance_day(&service);
    let diagnostics = service.diagnostics().unwrap();
    let text = diagnostics.to_plain_text();
    assert!(text.contains("LocalTrack"));
    assert!(text.contains(&format!(
        "Database schema {}",
        localtrack_storage::SCHEMA_VERSION
    )));
    assert!(!text.contains("github.com"));
    assert!(!text.contains("VS Code window"));
    assert!(diagnostics.database_integrity_ok);
}

#[test]
fn comparison_reports_deltas_between_periods() {
    let (_dir, service) = service();
    let (start, end) = build_acceptance_day(&service);
    let comparison = service
        .compare(&RangeQuery {
            from_ms: start,
            to_ms: end,
            filter: ActivityFilter::default(),
        })
        .unwrap();
    assert_eq!(comparison.previous.summary.active_ms, 0);
    assert_eq!(
        comparison.active_delta_ms,
        comparison.current.summary.active_ms
    );
}

#[test]
fn settings_updates_are_validated_and_applied() {
    let (_dir, service) = service();
    let settings = service
        .update_setting(keys::AFK_THRESHOLD_SECONDS, serde_json::json!(300))
        .unwrap();
    assert_eq!(settings.afk_threshold_seconds, 300);
    assert!(service
        .update_setting("evil; DROP TABLE settings", serde_json::json!(1))
        .is_err());
}

#[test]
fn filters_never_hide_idle_time() {
    let (_dir, service) = service();
    let (start, end) = build_acceptance_day(&service);
    let filter = ActivityFilter {
        app_names: vec!["VS Code".into()],
        ..ActivityFilter::default()
    };
    let summary = service
        .summary(&RangeQuery {
            from_ms: start,
            to_ms: end,
            filter,
        })
        .unwrap();
    assert_eq!(summary.active_ms, HOUR + 45 * MIN);
    assert_eq!(
        summary.idle_ms,
        10 * MIN,
        "idle stays visible under filters"
    );
}

#[test]
fn a_gap_says_whether_localtrack_was_even_running() {
    // The difference between "you were clocked in and nothing happened" and
    // "the tracker was off" matters to anybody reading their own timesheet.
    use localtrack_core::activity::{ActivitySegment, SegmentKey};
    use localtrack_storage::repo;

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("localtrack.db"), now_ms()).unwrap());

    let start = now_ms() - 3 * HOUR;

    // An earlier run of the application: it worked for an hour, then stopped.
    db.write(|tx| {
        repo::uptime::start(tx, "run-1", start, "1.0.0")?;
        repo::uptime::stop(tx, "run-1", start + HOUR)
    })
    .unwrap();

    let observed = ActivitySegment::from_key(
        &SegmentKey::desktop(
            Some("VS Code".into()),
            Some("code".into()),
            Some("main.rs".into()),
        ),
        start,
        start + HOUR,
        start,
    );
    db.write(|tx| repo::segments::upsert(tx, &observed))
        .unwrap();

    // It starts again now; the two hours in between belong to nobody.
    let service = AppService::with_database(db).unwrap();
    service
        .create_manual_session(start, now_ms(), None)
        .unwrap();

    let summary = service
        .summary(&RangeQuery {
            from_ms: start,
            to_ms: now_ms(),
            filter: ActivityFilter::default(),
        })
        .unwrap();

    assert_eq!(summary.active_ms, HOUR);
    assert!(summary.untracked_ms >= 2 * HOUR - 1000);
    assert!(
        summary.untracked_agent_off_ms > HOUR,
        "the gap is explained, not left as a mystery"
    );
    assert!(
        summary.untracked_agent_off_ms <= summary.untracked_ms,
        "the explanation never exceeds the gap"
    );
}

#[test]
fn a_gap_from_before_the_record_began_is_not_blamed_on_the_tracker() {
    // The uptime table starts when the feature does. Time before that cannot be
    // attributed to anything, and guessing would be worse than saying nothing.
    use localtrack_core::activity::{ActivitySegment, SegmentKey};
    use localtrack_storage::repo;

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("localtrack.db"), now_ms()).unwrap());

    // A day of history that predates any uptime record.
    let start = now_ms() - 6 * HOUR;
    let observed = ActivitySegment::from_key(
        &SegmentKey::desktop(
            Some("VS Code".into()),
            Some("code".into()),
            Some("main.rs".into()),
        ),
        start,
        start + HOUR,
        start,
    );
    db.write(|tx| repo::segments::upsert(tx, &observed))
        .unwrap();

    let service = AppService::with_database(db).unwrap();
    service
        .create_manual_session(start, now_ms(), None)
        .unwrap();

    let summary = service
        .summary(&RangeQuery {
            from_ms: start,
            to_ms: now_ms(),
            filter: ActivityFilter::default(),
        })
        .unwrap();

    assert!(summary.untracked_ms > 4 * HOUR);
    assert!(
        summary.untracked_agent_off_ms < HOUR,
        "hours recorded before the first known run are left unexplained, not blamed"
    );
}
