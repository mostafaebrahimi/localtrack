//! Database integration tests (spec §134).

use localtrack_core::activity::{ActivitySegment, SegmentKey};
use localtrack_core::classification::{ClassificationRule, RuleField, RuleOperator};
use localtrack_core::privacy::{ExclusionAction, ExclusionRule, ExclusionTarget};
use localtrack_core::sessions::{WorkBreak, WorkSession};
use localtrack_core::settings::Settings;
use localtrack_storage::filters::{ActivityFilter, Page, SortOrder};
use localtrack_storage::repo;
use localtrack_storage::{Database, SCHEMA_VERSION};

const MIN: i64 = 60_000;
const HOUR: i64 = 60 * MIN;

fn temp_db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Database::open(dir.path().join("localtrack.db"), 0).expect("open db");
    (dir, db)
}

fn session(id: &str, start: i64, end: Option<i64>) -> WorkSession {
    WorkSession {
        id: id.into(),
        started_at_ms: start,
        ended_at_ms: end,
        start_timezone_offset_min: 0,
        end_timezone_offset_min: end.map(|_| 0),
        note: None,
        created_manually: false,
        edited_manually: false,
        created_at_ms: start,
        updated_at_ms: start,
    }
}

fn desktop_segment(id: &str, app: &str, start: i64, end: i64) -> ActivitySegment {
    let mut segment = ActivitySegment::from_key(
        &SegmentKey::desktop(
            Some(app.into()),
            Some(format!("{app}.exe")),
            Some("title".into()),
        ),
        start,
        end,
        start,
    );
    segment.id = id.into();
    segment
}

#[test]
fn migrations_apply_and_are_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("localtrack.db");
    let db = Database::open(&path, 0).unwrap();
    assert_eq!(db.health().unwrap().schema_version, SCHEMA_VERSION);
    drop(db);

    // Re-opening must not re-run migrations or fail.
    let db = Database::open(&path, 1).unwrap();
    assert_eq!(db.health().unwrap().schema_version, SCHEMA_VERSION);
    let categories = db.read(repo::categories::list).unwrap();
    assert_eq!(
        categories.len(),
        10,
        "default categories seeded exactly once"
    );
}

#[test]
fn wal_mode_and_pragmas_are_active() {
    let (_dir, db) = temp_db();
    let health = db.health().unwrap();
    assert_eq!(health.journal_mode.to_lowercase(), "wal");
    let foreign_keys: i64 = db
        .read(|conn| Ok(conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(foreign_keys, 1);
}

#[test]
fn session_and_break_crud() {
    let (_dir, db) = temp_db();
    db.write(|tx| repo::sessions::insert_session(tx, &session("s1", 0, None)))
        .unwrap();
    assert_eq!(db.read(repo::sessions::count_open_sessions).unwrap(), 1);

    db.write(|tx| {
        repo::sessions::insert_break(
            tx,
            &WorkBreak {
                id: "b1".into(),
                work_session_id: "s1".into(),
                started_at_ms: 10 * MIN,
                ended_at_ms: None,
                note: None,
                created_at_ms: 0,
                updated_at_ms: 0,
            },
        )
    })
    .unwrap();

    let snapshot = db.read(repo::sessions::clock_snapshot).unwrap();
    assert_eq!(
        snapshot.state,
        localtrack_core::sessions::ClockState::OnBreak,
        "an open break means ON_BREAK after restart"
    );

    let mut closed = session("s1", 0, Some(HOUR));
    closed.edited_manually = true;
    db.write(|tx| repo::sessions::update_session(tx, &closed))
        .unwrap();
    assert_eq!(db.read(repo::sessions::count_open_sessions).unwrap(), 0);

    // Deleting a session cascades to its breaks.
    db.write(|tx| repo::sessions::delete_session(tx, "s1"))
        .unwrap();
    let breaks = db
        .read(|c| repo::sessions::breaks_for_session(c, "s1"))
        .unwrap();
    assert!(breaks.is_empty(), "breaks cascade with their session");
}

#[test]
fn activity_crud_and_range_queries() {
    let (_dir, db) = temp_db();
    db.write(|tx| {
        repo::segments::insert_many(
            tx,
            &[
                desktop_segment("a", "Code", 0, 30 * MIN),
                desktop_segment("b", "Chrome", 30 * MIN, HOUR),
                desktop_segment("c", "Slack", 2 * HOUR, 3 * HOUR),
            ],
        )
    })
    .unwrap();

    let in_range = db.read(|c| repo::segments::load_range(c, 0, HOUR)).unwrap();
    assert_eq!(in_range.len(), 2);

    let filtered = db
        .read(|c| {
            repo::segments::list(
                c,
                &ActivityFilter {
                    app_names: vec!["Slack".into()],
                    ..Default::default()
                },
                Page::default(),
                SortOrder::Asc,
            )
        })
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "c");

    let total = db
        .read(|c| repo::segments::count(c, &ActivityFilter::default()))
        .unwrap();
    assert_eq!(total, 3);
}

#[test]
fn pagination_returns_stable_pages() {
    let (_dir, db) = temp_db();
    let segments: Vec<ActivitySegment> = (0..250)
        .map(|i| desktop_segment(&format!("s{i:03}"), "Code", i * MIN, i * MIN + MIN))
        .collect();
    db.write(|tx| repo::segments::insert_many(tx, &segments))
        .unwrap();

    let page1 = db
        .read(|c| {
            repo::segments::list(
                c,
                &ActivityFilter::default(),
                Page {
                    limit: 100,
                    offset: 0,
                },
                SortOrder::Asc,
            )
        })
        .unwrap();
    let page3 = db
        .read(|c| {
            repo::segments::list(
                c,
                &ActivityFilter::default(),
                Page {
                    limit: 100,
                    offset: 200,
                },
                SortOrder::Asc,
            )
        })
        .unwrap();
    assert_eq!(page1.len(), 100);
    assert_eq!(page3.len(), 50);
    assert_eq!(page1[0].id, "s000");
    assert_eq!(page3[0].id, "s200");
}

#[test]
fn checkpointing_an_open_segment_does_not_duplicate_rows() {
    let (_dir, db) = temp_db();
    let mut segment = desktop_segment("open", "Code", 0, 15_000);
    db.write(|tx| repo::segments::upsert(tx, &segment)).unwrap();
    for end in [30_000, 45_000, 60_000] {
        segment.ended_at_ms = end;
        segment.updated_at_ms = end;
        db.write(|tx| repo::segments::upsert(tx, &segment)).unwrap();
    }
    let all = db.read(|c| repo::segments::load_range(c, 0, HOUR)).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].ended_at_ms, 60_000);
}

#[test]
fn splitting_a_segment_preserves_total_duration() {
    let (_dir, db) = temp_db();
    db.write(|tx| repo::segments::upsert(tx, &desktop_segment("a", "Code", 0, HOUR)))
        .unwrap();
    db.write(|tx| repo::segments::split(tx, "a", 35 * MIN, "a2", HOUR).map(|_| ()))
        .unwrap();
    let all = db.read(|c| repo::segments::load_range(c, 0, HOUR)).unwrap();
    assert_eq!(all.len(), 2);
    let total: i64 = all.iter().map(|s| s.duration_ms()).sum();
    assert_eq!(total, HOUR);
    assert_eq!(all[0].ended_at_ms, 35 * MIN);
    assert_eq!(all[1].started_at_ms, 35 * MIN);

    // Splitting outside the segment is rejected.
    let err = db.write(|tx| repo::segments::split(tx, "a", 0, "a3", HOUR).map(|_| ()));
    assert!(err.is_err());
}

#[test]
fn deleting_a_range_trims_straddling_segments() {
    let (_dir, db) = temp_db();
    db.write(|tx| repo::segments::upsert(tx, &desktop_segment("a", "Code", 0, HOUR)))
        .unwrap();
    db.write(|tx| repo::segments::delete_range(tx, 20 * MIN, 30 * MIN, HOUR).map(|_| ()))
        .unwrap();
    let all = db.read(|c| repo::segments::load_range(c, 0, HOUR)).unwrap();
    let total: i64 = all.iter().map(|s| s.duration_ms()).sum();
    assert_eq!(total, 50 * MIN, "only the deleted range disappears");
    assert!(all
        .iter()
        .all(|s| s.started_at_ms >= 30 * MIN || s.ended_at_ms <= 20 * MIN));
}

#[test]
fn manual_classification_survives_rule_reclassification() {
    let (_dir, db) = temp_db();
    db.write(|tx| repo::segments::upsert(tx, &desktop_segment("a", "Code", 0, HOUR)))
        .unwrap();
    let personal = db
        .read(|c| repo::categories::find_by_name(c, "Personal"))
        .unwrap()
        .unwrap();
    let development = db
        .read(|c| repo::categories::find_by_name(c, "Development"))
        .unwrap()
        .unwrap();
    db.write(|tx| repo::segments::set_classification(tx, "a", Some(&personal.id), None, 1))
        .unwrap();

    let applied = db
        .write(|tx| {
            repo::segments::apply_rule_classification(tx, "a", Some(&development.id), None, 2)
        })
        .unwrap();
    assert!(!applied, "a manual override is never replaced by a rule");

    let segment = db.read(|c| repo::segments::get(c, "a")).unwrap();
    assert_eq!(segment.category_id.as_deref(), Some(personal.id.as_str()));

    let reclassifiable = db
        .read(|c| repo::segments::load_reclassifiable(c, None, None))
        .unwrap();
    assert!(reclassifiable.is_empty());
}

#[test]
fn rules_and_exclusions_round_trip() {
    let (_dir, db) = temp_db();
    let development = db
        .read(|c| repo::categories::find_by_name(c, "Development"))
        .unwrap()
        .unwrap();
    let rule = ClassificationRule {
        id: "r1".into(),
        name: "Company GitHub".into(),
        enabled: true,
        priority: 10,
        target_field: RuleField::Url,
        operator: RuleOperator::StartsWith,
        pattern: "https://github.com/company/".into(),
        category_id: Some(development.id.clone()),
        project_id: None,
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    db.write(|tx| repo::rules::upsert(tx, &rule)).unwrap();
    let loaded = db.read(repo::rules::list).unwrap();
    assert_eq!(loaded, vec![rule.clone()]);

    // An invalid regex is rejected before it reaches the database.
    let mut bad = rule.clone();
    bad.id = "r2".into();
    bad.operator = RuleOperator::Regex;
    bad.pattern = "(unclosed".into();
    assert!(db.write(|tx| repo::rules::upsert(tx, &bad)).is_err());

    let exclusion = ExclusionRule {
        id: "x1".into(),
        enabled: true,
        target: ExclusionTarget::Domain,
        pattern: "bank.example.com".into(),
        action: ExclusionAction::Ignore,
        created_at_ms: 0,
        updated_at_ms: 0,
    };
    db.write(|tx| repo::exclusions::upsert(tx, &exclusion))
        .unwrap();
    assert_eq!(db.read(repo::exclusions::list).unwrap(), vec![exclusion]);
}

#[test]
fn settings_round_trip_through_storage() {
    let (_dir, db) = temp_db();
    let settings = Settings {
        retention_days: 90,
        track_incognito: true,
        ..Settings::default()
    };
    db.write(|tx| repo::settings::save(tx, &settings, 0))
        .unwrap();
    let loaded = db.read(repo::settings::load).unwrap();
    assert_eq!(loaded, settings);
}

#[test]
fn retention_deletes_old_activity_but_keeps_sessions_by_default() {
    let (_dir, db) = temp_db();
    db.write(|tx| repo::sessions::insert_session(tx, &session("old", 0, Some(HOUR))))
        .unwrap();
    db.write(|tx| {
        repo::segments::insert_many(
            tx,
            &[
                desktop_segment("old", "Code", 0, HOUR),
                desktop_segment("new", "Code", 10 * HOUR, 11 * HOUR),
            ],
        )
    })
    .unwrap();

    let report = db
        .write(|tx| repo::maintenance::apply_retention(tx, 5 * HOUR, false))
        .unwrap();
    assert_eq!(report.segments_deleted, 1);
    assert_eq!(report.sessions_deleted, 0);
    assert_eq!(
        db.read(|c| repo::sessions::list_sessions(c, 0, HOUR))
            .unwrap()
            .len(),
        1
    );

    let report = db
        .write(|tx| repo::maintenance::apply_retention(tx, 5 * HOUR, true))
        .unwrap();
    assert_eq!(report.sessions_deleted, 1);
}

#[test]
fn backup_and_restore_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("localtrack.db"), 0).unwrap();
    db.write(|tx| repo::segments::upsert(tx, &desktop_segment("a", "Code", 0, HOUR)))
        .unwrap();

    let backup_path = dir.path().join("localtrack-backup-2026-08-20.db");
    db.backup_to(&backup_path).unwrap();
    assert!(backup_path.exists());
    assert_eq!(
        Database::validate_backup_file(&backup_path).unwrap(),
        SCHEMA_VERSION
    );

    // Change the live database, then restore.
    db.write(|tx| repo::segments::delete_all(tx).map(|_| ()))
        .unwrap();
    assert_eq!(
        db.read(|c| repo::segments::count(c, &ActivityFilter::default()))
            .unwrap(),
        0
    );

    let safety = dir.path().join("safety.db");
    db.restore_from(backup_path.as_path(), safety.as_path())
        .unwrap();
    assert_eq!(
        db.read(|c| repo::segments::count(c, &ActivityFilter::default()))
            .unwrap(),
        1
    );
    assert!(safety.exists(), "the pre-restore database is kept");
}

#[test]
fn restoring_an_invalid_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(dir.path().join("localtrack.db"), 0).unwrap();
    let bogus = dir.path().join("not-a-db.db");
    std::fs::write(&bogus, b"definitely not sqlite").unwrap();
    assert!(Database::validate_backup_file(&bogus).is_err());
    let safety = dir.path().join("safety.db");
    assert!(db.restore_from(bogus.as_path(), safety.as_path()).is_err());
}

#[test]
fn two_connections_can_write_concurrently_in_wal_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("localtrack.db");
    // Mirrors the desktop application and the Chrome native host.
    let desktop = Database::open(&path, 0).unwrap();
    let native_host = Database::open(&path, 0).unwrap();

    desktop
        .write(|tx| repo::segments::upsert(tx, &desktop_segment("desktop", "Code", 0, MIN)))
        .unwrap();
    native_host
        .write(|tx| repo::segments::upsert(tx, &desktop_segment("browser", "Chrome", MIN, 2 * MIN)))
        .unwrap();

    let count = desktop
        .read(|c| repo::segments::count(c, &ActivityFilter::default()))
        .unwrap();
    assert_eq!(count, 2, "both processes' writes are visible");
}

#[test]
fn integrity_check_passes() {
    let (_dir, db) = temp_db();
    assert!(db.integrity_check().unwrap());
}

#[test]
fn distinct_values_are_restricted_to_known_columns() {
    let (_dir, db) = temp_db();
    db.write(|tx| repo::segments::upsert(tx, &desktop_segment("a", "Code", 0, HOUR)))
        .unwrap();
    let apps = db
        .read(|c| repo::segments::distinct_values(c, "app_name", 10))
        .unwrap();
    assert_eq!(apps, vec!["Code".to_string()]);
    assert!(db
        .read(|c| repo::segments::distinct_values(c, "id; DROP TABLE activity_segments", 10))
        .is_err());
}
