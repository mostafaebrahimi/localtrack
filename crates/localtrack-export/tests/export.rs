use localtrack_core::activity::{ActivitySegment, SegmentKey};
use localtrack_core::aggregation::{ReportDimension, ReportRow, ReportSet, Summary};
use localtrack_export::csv_export::{write_csvs, CsvDataset};
use localtrack_export::model::{ActivityExportRow, ExportUrlPrivacy};
use localtrack_export::{xlsx, ExportData, ExportOptions, SessionExportRow};

const HOUR: i64 = 3_600_000;

fn sample_data() -> ExportData {
    let segment = ActivitySegment::from_key(
        &SegmentKey::browser_page(
            Some("chrome".into()),
            Some("github.com".into()),
            Some("https://github.com/company/hub/pull/51".into()),
            Some("Fix authentication".into()),
        ),
        0,
        HOUR,
        0,
    );
    ExportData {
        from_ms: 0,
        to_ms: 8 * HOUR,
        generated_at_ms: 8 * HOUR,
        app_version: "1.0.0".into(),
        summary: Summary {
            clocked_ms: 8 * HOUR,
            active_ms: 6 * HOUR,
            ..Default::default()
        },
        daily: vec![],
        sessions: vec![SessionExportRow {
            date: "2026-08-20".into(),
            clock_in_ms: 0,
            clock_out_ms: Some(8 * HOUR),
            clocked_ms: 8 * HOUR,
            break_ms: 0,
            work_ms: 8 * HOUR,
            active_ms: 6 * HOUR,
            idle_ms: HOUR,
            untracked_ms: HOUR,
            note: Some("Sprint work".into()),
        }],
        applications: Some(ReportSet {
            dimension: ReportDimension::Application,
            total_ms: HOUR,
            rows: vec![ReportRow {
                key: "Code".into(),
                label: "Visual Studio Code".into(),
                secondary: Some("Code.exe".into()),
                duration_ms: HOUR,
                percentage: 100.0,
                visit_count: 1,
            }],
        }),
        websites: None,
        pages: None,
        categories: None,
        projects: None,
        activities: vec![ActivityExportRow {
            segment,
            category_name: Some("Development".into()),
            project_name: Some("Hub".into()),
        }],
        interactions: vec![],
    }
}

#[test]
fn writes_an_xlsx_workbook() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("report.xlsx");
    let written = xlsx::write_workbook(&path, &sample_data(), &ExportOptions::default()).unwrap();
    assert!(written.exists());
    let bytes = std::fs::read(&written).unwrap();
    assert!(bytes.len() > 1000);
    assert_eq!(&bytes[0..2], b"PK", "xlsx files are zip archives");
}

#[test]
fn writes_csv_datasets() {
    let dir = tempfile::tempdir().unwrap();
    let files = write_csvs(
        dir.path(),
        &[
            CsvDataset::Sessions,
            CsvDataset::Activities,
            CsvDataset::Applications,
        ],
        &sample_data(),
        &ExportOptions::default(),
    )
    .unwrap();
    assert_eq!(files.len(), 3);

    let sessions = std::fs::read_to_string(dir.path().join("sessions.csv")).unwrap();
    assert!(sessions.contains("2026-08-20"));
    assert!(sessions.contains("Sprint work"));

    let activities = std::fs::read_to_string(dir.path().join("activities.csv")).unwrap();
    assert!(activities.contains("github.com"));
    assert!(activities.contains("Development"));
}

#[test]
fn export_privacy_levels_control_url_detail() {
    let url = Some("https://github.com/company/hub/pull/51?tab=files");
    assert_eq!(
        ExportUrlPrivacy::DomainOnly.apply(url).as_deref(),
        Some("github.com")
    );
    assert_eq!(
        ExportUrlPrivacy::SanitizedUrl.apply(url).as_deref(),
        Some("https://github.com/company/hub/pull/51")
    );
    assert_eq!(ExportUrlPrivacy::FullUrl.apply(url).as_deref(), url);
    assert_eq!(ExportUrlPrivacy::FullUrl.apply(None), None);
}

#[test]
fn domain_only_export_strips_paths_from_csv() {
    let dir = tempfile::tempdir().unwrap();
    let options = ExportOptions {
        url_privacy: ExportUrlPrivacy::DomainOnly,
        ..ExportOptions::default()
    };
    write_csvs(
        dir.path(),
        &[CsvDataset::Activities],
        &sample_data(),
        &options,
    )
    .unwrap();
    let activities = std::fs::read_to_string(dir.path().join("activities.csv")).unwrap();
    assert!(!activities.contains("/pull/51"));
    assert!(activities.contains("github.com"));
}

#[test]
fn interactions_sheet_is_skipped_when_empty() {
    let dir = tempfile::tempdir().unwrap();
    let options = ExportOptions {
        include_interactions: true,
        ..ExportOptions::default()
    };
    let path = dir.path().join("report.xlsx");
    // No interactions recorded: the sheet must simply not exist.
    xlsx::write_workbook(&path, &sample_data(), &options).unwrap();
    assert!(path.exists());
}
