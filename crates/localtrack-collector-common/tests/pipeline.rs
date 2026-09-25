//! Pipeline tests: privacy, tracking state, focus rules and boundaries.

use std::sync::Arc;

use localtrack_collector_common::pipeline::{IngestPipeline, TrackingDecision};
use localtrack_core::activity::{
    ActivityKind, BrowserObservation, IdleObservation, Observation, SystemLockObservation,
    WindowObservation,
};
use localtrack_core::privacy::{ExclusionAction, ExclusionRule, ExclusionTarget, UrlPolicy};
use localtrack_core::settings::{Settings, TrackingScope};
use localtrack_storage::{repo, Database};

const SEC: i64 = 1_000;
const MIN: i64 = 60 * SEC;

fn pipeline() -> (tempfile::TempDir, Arc<Database>, IngestPipeline) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("localtrack.db"), 0).unwrap());
    let pipeline = IngestPipeline::new(db.clone(), true).unwrap();
    (dir, db, pipeline)
}

fn window(app: &str, title: &str, at_ms: i64) -> Observation {
    Observation::WindowChanged(WindowObservation {
        captured_at_ms: at_ms,
        app_name: Some(app.into()),
        process_name: Some(format!("{}.exe", app.to_lowercase())),
        window_title: Some(title.into()),
        pid: Some(1),
    })
}

fn page(url: &str, title: &str, at_ms: i64) -> Observation {
    Observation::BrowserChanged(BrowserObservation {
        captured_at_ms: at_ms,
        event: "activated".into(),
        browser: "chrome".into(),
        window_id: Some(1),
        tab_id: Some(2),
        url: Some(url.into()),
        title: Some(title.into()),
        incognito: false,
        audible: false,
        focused: true,
    })
}

fn all_segments(db: &Database) -> Vec<localtrack_core::activity::ActivitySegment> {
    db.read(|conn| repo::segments::load_range(conn, 0, i64::MAX / 2))
        .unwrap()
}

#[test]
fn nothing_is_recorded_while_clocked_out_by_default() {
    let (_dir, db, mut pipeline) = pipeline();
    assert_eq!(pipeline.tracking_decision(), TrackingDecision::NotClockedIn);
    pipeline.handle(window("Code", "main.rs", SEC)).unwrap();
    pipeline.close_all_streams(60 * SEC).unwrap();
    assert!(all_segments(&db).is_empty());
}

#[test]
fn always_scope_records_without_a_session() {
    let (_dir, db, mut pipeline) = pipeline();
    let settings = Settings {
        tracking_scope: TrackingScope::Always,
        ..Settings::default()
    };
    db.write(|tx| repo::settings::save(tx, &settings, 0))
        .unwrap();
    pipeline.reload().unwrap();

    assert_eq!(pipeline.tracking_decision(), TrackingDecision::Recording);
    pipeline.handle(window("Code", "main.rs", SEC)).unwrap();
    pipeline.close_all_streams(60 * SEC).unwrap();
    assert_eq!(all_segments(&db).len(), 1);
}

#[test]
fn clock_flow_creates_session_and_break_rows() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    assert_eq!(pipeline.tracking_decision(), TrackingDecision::Recording);

    pipeline.start_break(30 * MIN).unwrap();
    assert_eq!(pipeline.tracking_decision(), TrackingDecision::OnBreak);
    pipeline.end_break(45 * MIN).unwrap();
    pipeline.clock_out(60 * MIN).unwrap();

    let sessions = db
        .read(|c| repo::sessions::list_sessions(c, 0, 2 * 60 * MIN))
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].ended_at_ms, Some(60 * MIN));
    let breaks = db
        .read(|c| repo::sessions::breaks_for_session(c, &sessions[0].id))
        .unwrap();
    assert_eq!(breaks.len(), 1);
    assert_eq!(breaks[0].ended_at_ms, Some(45 * MIN));
}

#[test]
fn invalid_clock_transitions_are_rejected() {
    let (_dir, _db, mut pipeline) = pipeline();
    assert!(pipeline.clock_out(0).is_err());
    assert!(pipeline.start_break(0).is_err());
    pipeline.clock_in(0).unwrap();
    assert!(pipeline.clock_in(SEC).is_err());
    assert!(pipeline.end_break(SEC).is_err());
}

#[test]
fn breaks_close_open_segments_and_stop_recording() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Code", "main.rs", 0)).unwrap();
    pipeline.start_break(10 * MIN).unwrap();

    let segments = all_segments(&db);
    assert_eq!(segments.len(), 1);
    assert_eq!(
        segments[0].ended_at_ms,
        10 * MIN,
        "segment closes at the break"
    );

    // Activity during a break is not stored.
    pipeline.handle(window("Chrome", "news", 12 * MIN)).unwrap();
    pipeline.end_break(15 * MIN).unwrap();
    assert_eq!(all_segments(&db).len(), 1);
}

#[test]
fn urls_are_sanitized_before_storage() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Chrome", "GitHub", 0)).unwrap();
    pipeline
        .handle(page(
            "https://example.com/login?password=secret&token=123#frag",
            "Login",
            SEC,
        ))
        .unwrap();
    pipeline.close_all_streams(5 * MIN).unwrap();

    let segments = all_segments(&db);
    let page_segment = segments
        .iter()
        .find(|s| s.kind == ActivityKind::BrowserPage)
        .expect("page segment stored");
    assert_eq!(
        page_segment.url.as_deref(),
        Some("https://example.com/login")
    );
    assert_eq!(page_segment.domain.as_deref(), Some("example.com"));

    // Nothing anywhere in the database contains the secrets.
    let dump = format!("{segments:?}");
    assert!(!dump.contains("secret"));
    assert!(!dump.contains("token"));
}

#[test]
fn incognito_pages_are_discarded_before_insertion() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Chrome", "Chrome", 0)).unwrap();
    let mut observation = page("https://example.com/secret", "Secret", SEC);
    if let Observation::BrowserChanged(ref mut browser) = observation {
        browser.incognito = true;
    }
    pipeline.handle(observation).unwrap();
    pipeline.close_all_streams(5 * MIN).unwrap();

    assert!(all_segments(&db)
        .iter()
        .all(|s| s.kind != ActivityKind::BrowserPage));
}

#[test]
fn excluded_domains_are_never_stored() {
    let (_dir, db, mut pipeline) = pipeline();
    db.write(|tx| {
        repo::exclusions::upsert(
            tx,
            &ExclusionRule {
                id: "x".into(),
                enabled: true,
                target: ExclusionTarget::Domain,
                pattern: "bank.example.com".into(),
                action: ExclusionAction::Ignore,
                created_at_ms: 0,
                updated_at_ms: 0,
            },
        )
    })
    .unwrap();
    pipeline.reload().unwrap();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Chrome", "Chrome", 0)).unwrap();
    pipeline
        .handle(page("https://bank.example.com/accounts", "Accounts", SEC))
        .unwrap();
    pipeline.close_all_streams(5 * MIN).unwrap();

    let dump = format!("{:?}", all_segments(&db));
    assert!(!dump.contains("bank.example.com"));
    assert!(!dump.contains("Accounts"));
}

#[test]
fn chrome_pages_do_not_count_when_another_app_is_foreground() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    // Chrome is foreground and a page is active.
    pipeline.handle(window("Chrome", "GitHub", 0)).unwrap();
    pipeline
        .handle(page("https://github.com/a", "A", 0))
        .unwrap();
    // The user switches to VS Code; the page keeps sending heartbeats.
    pipeline
        .handle(window("Code", "main.rs", 10 * MIN))
        .unwrap();
    pipeline
        .handle(page("https://github.com/a", "A", 12 * MIN))
        .unwrap();
    pipeline.close_all_streams(20 * MIN).unwrap();

    let segments = all_segments(&db);
    let page_time: i64 = segments
        .iter()
        .filter(|s| s.kind == ActivityKind::BrowserPage)
        .map(|s| s.duration_ms())
        .sum();
    assert_eq!(
        page_time,
        10 * MIN,
        "browser time stops when Chrome loses focus"
    );
}

#[test]
fn idle_closes_activity_and_starts_at_last_input_plus_threshold() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Code", "main.rs", 0)).unwrap();

    pipeline
        .handle(Observation::UserIdle(IdleObservation {
            captured_at_ms: 10 * MIN + 4 * SEC,
            last_input_ms: 7 * MIN,
            idle_threshold_ms: 3 * MIN,
        }))
        .unwrap();
    pipeline
        .handle(Observation::UserActive(IdleObservation {
            captured_at_ms: 15 * MIN,
            last_input_ms: 15 * MIN,
            idle_threshold_ms: 3 * MIN,
        }))
        .unwrap();
    pipeline.close_all_streams(20 * MIN).unwrap();

    let segments = all_segments(&db);
    let idle = segments
        .iter()
        .find(|s| s.kind == ActivityKind::Idle)
        .expect("idle segment");
    assert_eq!(
        idle.started_at_ms,
        10 * MIN,
        "AFK starts at last input + threshold"
    );
    assert_eq!(idle.ended_at_ms, 15 * MIN);
    assert!(idle.is_afk);

    let code = segments
        .iter()
        .find(|s| s.app_name.as_deref() == Some("Code"))
        .unwrap();
    assert_eq!(code.ended_at_ms, 10 * MIN, "activity ends when idle begins");
}

#[test]
fn a_night_away_from_the_desk_is_idle_not_untracked() {
    // The machine stays on, the person goes home. Idle has to keep accruing.
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Code", "main.rs", 0)).unwrap();

    pipeline
        .handle(Observation::UserIdle(IdleObservation {
            captured_at_ms: 10 * MIN,
            last_input_ms: 7 * MIN,
            idle_threshold_ms: 3 * MIN,
        }))
        .unwrap();

    // Four hours of the collector reporting "still nobody here".
    for tick in 1..=(4 * 60) {
        let at = 10 * MIN + tick * MIN;
        pipeline
            .handle(Observation::Heartbeat {
                captured_at_ms: at,
                stream: "system".into(),
            })
            .unwrap();
    }
    pipeline.close_all_streams(10 * MIN + 4 * 60 * MIN).unwrap();

    let idle: Vec<_> = all_segments(&db)
        .into_iter()
        .filter(|s| s.kind == ActivityKind::Idle)
        .collect();
    assert_eq!(idle.len(), 1, "one continuous idle period");
    assert_eq!(idle[0].started_at_ms, 10 * MIN);
    assert_eq!(
        idle[0].duration_ms(),
        4 * 60 * MIN,
        "the whole night is accounted for"
    );
}

#[test]
fn the_clock_stops_itself_after_a_long_idle_period() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Code", "main.rs", 0)).unwrap();

    // Work stops at 20 minutes; the machine is left on.
    pipeline
        .handle(Observation::UserIdle(IdleObservation {
            captured_at_ms: 23 * MIN,
            last_input_ms: 20 * MIN,
            idle_threshold_ms: 3 * MIN,
        }))
        .unwrap();
    for tick in 1..=90 {
        pipeline
            .handle(Observation::Heartbeat {
                captured_at_ms: 23 * MIN + tick * MIN,
                stream: "system".into(),
            })
            .unwrap();
    }
    pipeline.tick(23 * MIN + 90 * MIN).unwrap();

    assert_eq!(
        pipeline.clock().state,
        localtrack_core::sessions::ClockState::ClockedOut,
        "the clock stops on its own"
    );

    let sessions = db
        .read(|conn| repo::sessions::list_sessions(conn, 0, 24 * 60 * MIN))
        .unwrap();
    assert_eq!(
        sessions[0].ended_at_ms,
        Some(20 * MIN),
        "backdated to when the work actually stopped, not to now"
    );
    let _ = all_segments(&db);
}

#[test]
fn working_in_one_window_never_looks_like_absence() {
    // Reading a long document, watching a build, writing in one editor: the
    // foreground window does not change for an hour, but the person is there.
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Code", "main.rs", 0)).unwrap();

    for minute in 1..=90 {
        let at = minute * MIN;
        pipeline
            .handle(Observation::Heartbeat {
                captured_at_ms: at,
                stream: "desktop".into(),
            })
            .unwrap();
        pipeline.tick(at).unwrap();
    }

    assert_eq!(
        pipeline.clock().state,
        localtrack_core::sessions::ClockState::ClockedIn,
        "ninety minutes in one window is work, not absence"
    );

    pipeline.close_all_streams(90 * MIN).unwrap();
    let worked: i64 = all_segments(&db).iter().map(|s| s.duration_ms()).sum();
    assert_eq!(worked, 90 * MIN, "and all of it is recorded");
}

#[test]
fn the_clock_still_stops_when_the_collector_goes_quiet() {
    // No heartbeats at all — the machine slept, or the collector died. That is
    // absence, and the clock should stop at the last thing we saw.
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Code", "main.rs", 0)).unwrap();
    for minute in 1..=10 {
        pipeline
            .handle(Observation::Heartbeat {
                captured_at_ms: minute * MIN,
                stream: "desktop".into(),
            })
            .unwrap();
    }

    pipeline.tick(10 * MIN + 45 * MIN).unwrap();

    assert_eq!(
        pipeline.clock().state,
        localtrack_core::sessions::ClockState::ClockedOut
    );
    let sessions = db
        .read(|conn| repo::sessions::list_sessions(conn, 0, 24 * 60 * MIN))
        .unwrap();
    assert_eq!(
        sessions[0].ended_at_ms,
        Some(10 * MIN),
        "closed at the last sign of life, not at the moment we noticed"
    );
}

#[test]
fn automatic_clock_out_can_be_switched_off() {
    let (_dir, db, mut pipeline) = pipeline();
    let settings = Settings {
        auto_clock_out_idle_minutes: 0,
        ..Settings::default()
    };
    db.write(|tx| repo::settings::save(tx, &settings, 0))
        .unwrap();
    pipeline.reload().unwrap();

    pipeline.clock_in(0).unwrap();
    pipeline
        .handle(Observation::UserIdle(IdleObservation {
            captured_at_ms: 5 * MIN,
            last_input_ms: 2 * MIN,
            idle_threshold_ms: 3 * MIN,
        }))
        .unwrap();
    pipeline.tick(10 * 60 * MIN).unwrap();

    assert_eq!(
        pipeline.clock().state,
        localtrack_core::sessions::ClockState::ClockedIn,
        "the person asked to stay clocked in"
    );
}

#[test]
fn being_away_resumes_after_a_boundary_closes_the_idle_segment() {
    // Clocking in, a midnight split or a restart all close open segments. The
    // person may already be away, and AFK is only reported on transitions, so
    // the time must not silently become untracked.
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Code", "main.rs", 0)).unwrap();

    pipeline
        .handle(Observation::UserIdle(IdleObservation {
            captured_at_ms: 5 * MIN,
            last_input_ms: 2 * MIN,
            idle_threshold_ms: 3 * MIN,
        }))
        .unwrap();

    // A boundary closes everything while the user is still away.
    pipeline.close_all_streams(10 * MIN).unwrap();

    // The collector keeps saying "still nobody here".
    for minute in 11..=70 {
        pipeline
            .handle(Observation::Heartbeat {
                captured_at_ms: minute * MIN,
                stream: "system".into(),
            })
            .unwrap();
    }
    pipeline.close_all_streams(70 * MIN).unwrap();

    let idle: i64 = all_segments(&db)
        .iter()
        .filter(|s| s.kind == ActivityKind::Idle)
        .map(|s| s.duration_ms())
        .sum();
    assert_eq!(
        idle,
        5 * MIN + 59 * MIN,
        "the whole absence is accounted for"
    );
}

#[test]
fn lock_overrides_the_idle_threshold_immediately() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Code", "main.rs", 0)).unwrap();
    pipeline
        .handle(Observation::SystemLocked(SystemLockObservation {
            captured_at_ms: 5 * MIN,
            locked: true,
        }))
        .unwrap();
    pipeline
        .handle(Observation::SystemUnlocked(SystemLockObservation {
            captured_at_ms: 20 * MIN,
            locked: false,
        }))
        .unwrap();
    pipeline.close_all_streams(25 * MIN).unwrap();

    let segments = all_segments(&db);
    let locked = segments
        .iter()
        .find(|s| s.kind == ActivityKind::Locked)
        .expect("locked segment");
    assert_eq!(locked.started_at_ms, 5 * MIN);
    assert_eq!(locked.ended_at_ms, 20 * MIN);
}

#[test]
fn pausing_stops_collection_without_changing_the_clock() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Code", "main.rs", 0)).unwrap();
    pipeline.set_paused(true, 5 * MIN).unwrap();

    assert_eq!(pipeline.tracking_decision(), TrackingDecision::Paused);
    assert_eq!(
        pipeline.clock().state,
        localtrack_core::sessions::ClockState::ClockedIn,
        "pausing never clocks the user out"
    );

    pipeline.handle(window("Chrome", "news", 6 * MIN)).unwrap();
    pipeline.set_paused(false, 10 * MIN).unwrap();
    pipeline
        .handle(window("Code", "main.rs", 10 * MIN))
        .unwrap();
    pipeline.close_all_streams(15 * MIN).unwrap();

    let segments = all_segments(&db);
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].ended_at_ms, 5 * MIN);
    assert_eq!(segments[1].started_at_ms, 10 * MIN);
    let paused_gap: Vec<_> = segments
        .iter()
        .filter(|s| s.started_at_ms >= 5 * MIN && s.ended_at_ms <= 10 * MIN)
        .collect();
    assert!(paused_gap.is_empty(), "the paused window stays untracked");
}

#[test]
fn heartbeat_checkpoints_keep_open_segments_recoverable() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Code", "main.rs", 0)).unwrap();
    for t in (1..=40).map(|i| i * SEC) {
        pipeline
            .handle(Observation::Heartbeat {
                captured_at_ms: t,
                stream: "desktop".into(),
            })
            .unwrap();
        pipeline.tick(t).unwrap();
    }
    // A crash right now: the checkpointed row is already in the database.
    let segments = all_segments(&db);
    assert_eq!(segments.len(), 1);
    assert!(
        segments[0].ended_at_ms >= 30 * SEC,
        "at most one heartbeat interval of activity can be lost"
    );
}

#[test]
fn stale_browser_stream_is_closed_at_its_last_heartbeat() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Chrome", "GitHub", 0)).unwrap();
    pipeline
        .handle(page("https://github.com/a", "A", 0))
        .unwrap();
    pipeline
        .handle(Observation::Heartbeat {
            captured_at_ms: 15 * SEC,
            stream: "browser".into(),
        })
        .unwrap();

    // Chrome disappears; five minutes later the pipeline ticks.
    pipeline.tick(5 * MIN).unwrap();
    let page_segment = all_segments(&db)
        .into_iter()
        .find(|s| s.kind == ActivityKind::BrowserPage)
        .expect("page segment");
    assert_eq!(
        page_segment.ended_at_ms,
        15 * SEC,
        "a vanished browser does not keep accruing time"
    );
}

#[test]
fn interactions_are_off_by_default_and_never_store_values() {
    use localtrack_core::activity::{BrowserInteractionObservation, InteractionType};

    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    let interaction = BrowserInteractionObservation {
        captured_at_ms: SEC,
        interaction: InteractionType::ButtonClick,
        element: Some("button".into()),
        label: Some("Save Invoice".into()),
        url: Some("https://example.com/invoices/1?token=abc".into()),
        domain: Some("example.com".into()),
    };
    pipeline
        .handle(Observation::Interaction(interaction.clone()))
        .unwrap();
    assert!(all_segments(&db)
        .iter()
        .all(|s| s.kind != ActivityKind::Interaction));

    let settings = Settings {
        detailed_interactions: true,
        ..Settings::default()
    };
    db.write(|tx| repo::settings::save(tx, &settings, 0))
        .unwrap();
    pipeline.reload().unwrap();
    pipeline
        .handle(Observation::Interaction(interaction))
        .unwrap();

    let stored = all_segments(&db)
        .into_iter()
        .find(|s| s.kind == ActivityKind::Interaction)
        .expect("interaction stored when enabled");
    let metadata = stored.metadata_json.unwrap_or_default();
    assert!(metadata.contains("Save Invoice"));
    assert!(!stored.url.clone().unwrap_or_default().contains("token"));
}

#[test]
fn full_url_policy_is_honoured_when_the_user_opts_in() {
    let (_dir, db, mut pipeline) = pipeline();
    let settings = Settings {
        url_policy: UrlPolicy::FullUrl,
        ..Settings::default()
    };
    db.write(|tx| repo::settings::save(tx, &settings, 0))
        .unwrap();
    pipeline.reload().unwrap();

    pipeline.clock_in(0).unwrap();
    pipeline.handle(window("Chrome", "GitHub", 0)).unwrap();
    pipeline
        .handle(page("https://github.com/a?page=2", "A", SEC))
        .unwrap();
    pipeline.close_all_streams(5 * MIN).unwrap();

    let stored = all_segments(&db)
        .into_iter()
        .find(|s| s.kind == ActivityKind::BrowserPage)
        .unwrap();
    assert_eq!(stored.url.as_deref(), Some("https://github.com/a?page=2"));
}

fn window_titled(app: &str, title: &str, at_ms: i64) -> Observation {
    Observation::WindowChanged(WindowObservation {
        captured_at_ms: at_ms,
        app_name: Some(app.into()),
        process_name: Some(app.to_lowercase()),
        window_title: Some(title.into()),
        pid: Some(1),
    })
}

#[test]
fn an_animated_title_does_not_shatter_a_segment() {
    // A terminal spinner flips the window title every second while the user
    // keeps working in the same terminal.
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();

    for tick in 0..90 {
        let frame = if tick % 2 == 0 {
            "\u{25d0} building"
        } else {
            "\u{25d1} building"
        };
        pipeline
            .handle(window_titled("Terminal", frame, tick * SEC))
            .unwrap();
    }
    pipeline.close_all_streams(90 * SEC).unwrap();

    let segments = all_segments(&db);
    assert_eq!(
        segments.len(),
        1,
        "one activity, not one segment per second"
    );
    assert_eq!(segments[0].duration_ms(), 90 * SEC);
    assert_eq!(segments[0].app_name.as_deref(), Some("Terminal"));
}

#[test]
fn a_title_that_sticks_starts_a_new_segment_where_it_changed() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();

    for tick in 0..30 {
        pipeline
            .handle(window_titled("Code", "auth.service.ts", tick * SEC))
            .unwrap();
    }
    // At 30s the user opens another file and stays there.
    for tick in 30..90 {
        pipeline
            .handle(window_titled("Code", "billing.service.ts", tick * SEC))
            .unwrap();
    }
    pipeline.close_all_streams(90 * SEC).unwrap();

    let segments = all_segments(&db);
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].window_title.as_deref(), Some("auth.service.ts"));
    assert_eq!(
        segments[0].ended_at_ms,
        30 * SEC,
        "the boundary is when the title actually changed, not when it was confirmed"
    );
    assert_eq!(segments[1].started_at_ms, 30 * SEC);
    assert_eq!(
        segments[1].window_title.as_deref(),
        Some("billing.service.ts")
    );
}

#[test]
fn switching_application_is_never_debounced() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline
        .handle(window_titled("Code", "main.rs", 0))
        .unwrap();
    pipeline
        .handle(window_titled("Slack", "general", 2 * SEC))
        .unwrap();
    pipeline.close_all_streams(4 * SEC).unwrap();

    let segments = all_segments(&db);
    assert_eq!(
        segments.len(),
        2,
        "an app switch is real activity, not title noise"
    );
    assert_eq!(segments[0].ended_at_ms, 2 * SEC);
}

#[test]
fn a_title_that_flips_back_keeps_one_segment() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline.handle(window_titled("Chrome", "Docs", 0)).unwrap();
    // A transient notification title, then back to the original.
    pipeline
        .handle(window_titled("Chrome", "(1) Docs", 2 * SEC))
        .unwrap();
    pipeline
        .handle(window_titled("Chrome", "Docs", 4 * SEC))
        .unwrap();
    pipeline
        .handle(window_titled("Chrome", "Docs", 20 * SEC))
        .unwrap();
    pipeline.close_all_streams(30 * SEC).unwrap();

    let segments = all_segments(&db);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].duration_ms(), 30 * SEC);
}

#[test]
fn input_without_a_window_counts_as_active_not_untracked() {
    // A Wayland compositor with no active-window API, or a full-screen client
    // the shell hides: the user is clearly working, so the time is active.
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();

    pipeline
        .handle(window_titled("Code", "main.rs", 0))
        .unwrap();
    for tick in 10..40 {
        pipeline
            .handle(Observation::InputActivity {
                captured_at_ms: tick * SEC,
            })
            .unwrap();
    }
    pipeline.close_all_streams(40 * SEC).unwrap();

    let segments = all_segments(&db);
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[1].kind, ActivityKind::Input);
    assert_eq!(segments[1].started_at_ms, 10 * SEC);
    assert!(!segments[1].is_afk, "input is activity, not idleness");
    assert!(segments[1].app_name.is_none(), "no application is invented");

    // And it shows up as active time, under an honest label.
    use localtrack_core::aggregation::{reports, summary, AggregationInput};
    use localtrack_core::interval::Interval;
    let mut input = AggregationInput::new(Interval::new(0, 40 * SEC), 40 * SEC);
    input.segments = segments;
    input.sessions = db
        .read(|conn| repo::sessions::list_sessions(conn, 0, 40 * SEC))
        .unwrap();
    assert_eq!(summary::compute_summary(&input).active_ms, 40 * SEC);
    let report = reports::application_report(&input);
    let unattributed = report
        .rows
        .iter()
        .find(|row| row.key == "Unattributed activity")
        .expect("input time is reported, not hidden");
    assert_eq!(unattributed.duration_ms, 30 * SEC);
}

#[test]
fn a_known_window_always_beats_bare_input() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();
    pipeline
        .handle(Observation::InputActivity { captured_at_ms: 0 })
        .unwrap();
    pipeline
        .handle(window_titled("Code", "main.rs", 10 * SEC))
        .unwrap();
    pipeline.close_all_streams(20 * SEC).unwrap();

    let segments = all_segments(&db);
    assert_eq!(segments[0].kind, ActivityKind::Input);
    assert_eq!(segments[0].ended_at_ms, 10 * SEC);
    assert_eq!(segments[1].kind, ActivityKind::Window);
}

#[test]
fn the_trackers_own_timer_never_becomes_an_activity() {
    // The floating bar sits above every window; the pointer rests on it and it
    // must never be what the day is billed to.
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();

    for tick in 0..30 {
        pipeline
            .handle(window_titled("Localtrack", "LocalTrack Timer", tick * SEC))
            .unwrap();
    }
    pipeline.close_all_streams(30 * SEC).unwrap();

    let segments = all_segments(&db);
    assert!(
        segments
            .iter()
            .all(|s| s.window_title.as_deref() != Some("LocalTrack Timer")),
        "the timer window is never recorded"
    );
    // The person was clearly there, so the time is active, not a hole.
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].kind, ActivityKind::Input);
    assert_eq!(segments[0].duration_ms(), 30 * SEC);
}

#[test]
fn work_continues_uninterrupted_when_the_pointer_crosses_the_timer() {
    let (_dir, db, mut pipeline) = pipeline();
    pipeline.clock_in(0).unwrap();

    pipeline
        .handle(window_titled("Code", "main.rs", 0))
        .unwrap();
    // The pointer passes over the floating bar for a couple of seconds.
    pipeline
        .handle(window_titled("Localtrack", "LocalTrack Timer", 10 * SEC))
        .unwrap();
    pipeline
        .handle(window_titled("Localtrack", "LocalTrack Timer", 12 * SEC))
        .unwrap();
    pipeline
        .handle(window_titled("Code", "main.rs", 14 * SEC))
        .unwrap();
    pipeline.close_all_streams(20 * SEC).unwrap();

    let segments = all_segments(&db);
    assert_eq!(segments.len(), 1, "one unbroken stretch of work");
    assert_eq!(segments[0].app_name.as_deref(), Some("Code"));
    assert_eq!(segments[0].duration_ms(), 20 * SEC);
}

#[test]
fn a_session_is_split_at_midnight_so_each_day_starts_fresh() {
    use localtrack_core::time::{local_day_end_ms, local_day_start_ms, now_ms as wall_now};

    let (_dir, db, mut pipeline) = pipeline();

    // Clocked in yesterday evening and still running this morning.
    let yesterday_evening = local_day_start_ms(wall_now()) - 3 * 60 * MIN;
    pipeline.clock_in(yesterday_evening).unwrap();
    let session_id = pipeline.clock().session.as_ref().unwrap().id.clone();
    pipeline
        .set_session_note(&session_id, "Release checklist")
        .unwrap();

    let midnight = local_day_end_ms(yesterday_evening);
    pipeline.tick(midnight + MIN).unwrap();

    let sessions = db
        .read(|conn| repo::sessions::list_sessions(conn, yesterday_evening - MIN, wall_now() + MIN))
        .unwrap();
    assert_eq!(
        sessions.len(),
        2,
        "yesterday and today are separate records"
    );
    assert_eq!(
        sessions[0].ended_at_ms,
        Some(midnight),
        "yesterday ends at midnight"
    );
    assert_eq!(
        sessions[1].started_at_ms, midnight,
        "today starts at midnight"
    );
    assert!(sessions[1].ended_at_ms.is_none(), "and is still running");
    assert_eq!(
        sessions[1].note.as_deref(),
        Some("Release checklist"),
        "the same work carries over"
    );
    assert_eq!(
        pipeline.clock().state,
        localtrack_core::sessions::ClockState::ClockedIn
    );
}

#[test]
fn splitting_at_midnight_can_be_switched_off() {
    use localtrack_core::time::{local_day_end_ms, local_day_start_ms, now_ms as wall_now};

    let (_dir, db, mut pipeline) = pipeline();
    let settings = Settings {
        split_sessions_at_midnight: false,
        auto_clock_out_idle_minutes: 0,
        ..Settings::default()
    };
    db.write(|tx| repo::settings::save(tx, &settings, 0))
        .unwrap();
    pipeline.reload().unwrap();

    let yesterday_evening = local_day_start_ms(wall_now()) - 3 * 60 * MIN;
    pipeline.clock_in(yesterday_evening).unwrap();
    pipeline
        .tick(local_day_end_ms(yesterday_evening) + MIN)
        .unwrap();

    let sessions = db
        .read(|conn| repo::sessions::list_sessions(conn, yesterday_evening - MIN, wall_now() + MIN))
        .unwrap();
    assert_eq!(sessions.len(), 1, "one continuous session, as asked");
    assert!(sessions[0].ended_at_ms.is_none());
}

#[test]
fn a_clock_change_by_another_process_is_picked_up() {
    // The desktop application and the Chrome native host share one database;
    // either can clock in or out (spec §100).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("localtrack.db");
    let db = Arc::new(Database::open(&path, 0).unwrap());

    let mut desktop = IngestPipeline::new(db.clone(), true).unwrap();
    let mut native_host = IngestPipeline::new(db.clone(), false).unwrap();

    assert_eq!(desktop.tracking_decision(), TrackingDecision::NotClockedIn);
    assert_eq!(
        native_host.tracking_decision(),
        TrackingDecision::NotClockedIn
    );

    // The popup clocks in through the native host.
    native_host.clock_in(0).unwrap();
    assert_eq!(native_host.tracking_decision(), TrackingDecision::Recording);

    desktop.refresh_clock().unwrap();
    assert_eq!(
        desktop.tracking_decision(),
        TrackingDecision::Recording,
        "the desktop application sees the popup's clock in"
    );

    // And the other way around.
    desktop.clock_out(10 * MIN).unwrap();
    native_host.refresh_clock().unwrap();
    assert_eq!(
        native_host.tracking_decision(),
        TrackingDecision::NotClockedIn
    );
}

#[test]
fn crash_recovery_restores_the_open_session() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("localtrack.db");
    let db = Arc::new(Database::open(&path, 0).unwrap());
    {
        let mut pipeline = IngestPipeline::new(db.clone(), true).unwrap();
        pipeline.clock_in(0).unwrap();
        pipeline.handle(window("Code", "main.rs", 0)).unwrap();
        for t in (1..=30).map(|i| i * SEC) {
            pipeline
                .handle(Observation::Heartbeat {
                    captured_at_ms: t,
                    stream: "desktop".into(),
                })
                .unwrap();
            pipeline.tick(t).unwrap();
        }
        // Simulated crash: no clock out, no clean shutdown.
    }

    let restarted = IngestPipeline::new(db.clone(), true).unwrap();
    assert_eq!(
        restarted.clock().state,
        localtrack_core::sessions::ClockState::ClockedIn
    );
    let segments = all_segments(&db);
    assert_eq!(segments.len(), 1, "committed activity survives");
    assert!(
        segments[0].ended_at_ms <= 30 * SEC,
        "the offline gap is never fabricated"
    );
}
