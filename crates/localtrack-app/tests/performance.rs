//! Performance check against a year-sized database (spec §123).
//!
//! Run explicitly, because generating a million segments takes a while:
//!
//! ```text
//! cargo test -p localtrack-app --test performance -- --ignored --nocapture
//! ```

use std::sync::Arc;
use std::time::Instant;

use localtrack_app::{AppService, RangeQuery};
use localtrack_core::activity::{ActivitySegment, SegmentKey};
use localtrack_core::time::now_ms;
use localtrack_storage::filters::{ActivityFilter, Page, SortOrder};
use localtrack_storage::{repo, Database};

const SEGMENTS: i64 = 1_000_000;
const SEGMENT_MS: i64 = 30_000;

#[test]
#[ignore = "generates a million segments; run explicitly"]
fn one_year_of_activity_stays_usable() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::open(dir.path().join("localtrack.db"), now_ms()).unwrap());

    let apps = ["VS Code", "Chrome", "Slack", "Terminal", "DBeaver"];
    let domains = [
        "github.com",
        "chatgpt.com",
        "gmail.com",
        "stackoverflow.com",
    ];

    let end = now_ms();
    let start = end - SEGMENTS * SEGMENT_MS;

    let generate = Instant::now();
    // 20 transactions of 50k rows keeps memory flat and mirrors real batching.
    for batch in 0..20 {
        let batch_start = start + batch * (SEGMENTS / 20) * SEGMENT_MS;
        let segments: Vec<ActivitySegment> = (0..(SEGMENTS / 20))
            .map(|index| {
                let at = batch_start + index * SEGMENT_MS;
                if index % 3 == 0 {
                    let domain = domains[(index as usize) % domains.len()];
                    ActivitySegment::from_key(
                        &SegmentKey::browser_page(
                            Some("chrome".into()),
                            Some(domain.into()),
                            Some(format!("https://{domain}/page/{index}")),
                            Some(format!("Page {index}")),
                        ),
                        at,
                        at + SEGMENT_MS,
                        at,
                    )
                } else {
                    let app = apps[(index as usize) % apps.len()];
                    ActivitySegment::from_key(
                        &SegmentKey::desktop(
                            Some(app.into()),
                            Some(format!("{app}.exe")),
                            Some(format!("file-{index}.rs")),
                        ),
                        at,
                        at + SEGMENT_MS,
                        at,
                    )
                }
            })
            .collect();
        db.write(|tx| repo::segments::insert_many(tx, &segments).map(|_| ()))
            .unwrap();
    }
    println!("generated {SEGMENTS} segments in {:?}", generate.elapsed());

    let service = AppService::with_database(db.clone()).unwrap();
    let count = db
        .read(|conn| repo::segments::count(conn, &ActivityFilter::default()))
        .unwrap();
    assert_eq!(count, SEGMENTS);

    // A day of reports out of a year of data.
    let day_start = end - 86_400_000;
    let query = RangeQuery {
        from_ms: day_start,
        to_ms: end,
        filter: ActivityFilter::default(),
    };

    let timer = Instant::now();
    let reports = service.reports(&query).unwrap();
    let reports_ms = timer.elapsed().as_millis();
    println!(
        "day reports in {reports_ms} ms, active {}",
        reports.summary.active_ms
    );
    assert!(reports_ms < 2_000, "day reports took {reports_ms} ms");

    let timer = Instant::now();
    let page = service
        .activity_page(
            &ActivityFilter::for_range(day_start, end),
            Page {
                limit: 100,
                offset: 0,
            },
            SortOrder::Asc,
        )
        .unwrap();
    let page_ms = timer.elapsed().as_millis();
    println!(
        "activity page in {page_ms} ms ({} of {})",
        page.rows.len(),
        page.total
    );
    assert_eq!(
        page.rows.len(),
        100,
        "pagination never loads the whole database"
    );
    assert!(page_ms < 1_000, "activity page took {page_ms} ms");

    let timer = Instant::now();
    let today = service.today().unwrap();
    let today_ms = timer.elapsed().as_millis();
    println!(
        "today dashboard in {today_ms} ms, timeline blocks {}",
        today.timeline.len()
    );
    assert!(today_ms < 2_000, "today dashboard took {today_ms} ms");

    let timer = Instant::now();
    let month = service
        .summary(&RangeQuery {
            from_ms: end - 30 * 86_400_000,
            to_ms: end,
            filter: ActivityFilter::default(),
        })
        .unwrap();
    let month_ms = timer.elapsed().as_millis();
    println!(
        "30-day summary in {month_ms} ms, active {}",
        month.active_ms
    );
    assert!(month_ms < 5_000, "30-day summary took {month_ms} ms");
}
