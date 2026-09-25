use std::path::{Path, PathBuf};

use localtrack_core::aggregation::ReportSet;
use localtrack_core::time::{format_local_date, format_local_time};

use crate::model::{ExportData, ExportOptions};
use crate::Result;

/// Which CSV datasets the user picked (spec §95).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsvDataset {
    Sessions,
    Activities,
    Applications,
    Websites,
    Pages,
    Categories,
    Projects,
    DailySummary,
}

impl CsvDataset {
    pub fn file_name(&self) -> &'static str {
        match self {
            CsvDataset::Sessions => "sessions.csv",
            CsvDataset::Activities => "activities.csv",
            CsvDataset::Applications => "applications.csv",
            CsvDataset::Websites => "websites.csv",
            CsvDataset::Pages => "pages.csv",
            CsvDataset::Categories => "categories.csv",
            CsvDataset::Projects => "projects.csv",
            CsvDataset::DailySummary => "daily-summary.csv",
        }
    }
}

fn seconds(ms: i64) -> String {
    format!("{:.0}", (ms as f64) / 1000.0)
}

fn write_report(path: &Path, report: &ReportSet) -> Result<()> {
    // UTF-8 output (spec §95).
    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record([
        "name",
        "detail",
        "duration_seconds",
        "share_percent",
        "visits",
    ])?;
    for row in &report.rows {
        writer.write_record([
            row.label.clone(),
            row.secondary.clone().unwrap_or_default(),
            seconds(row.duration_ms),
            format!("{:.2}", row.percentage),
            row.visit_count.to_string(),
        ])?;
    }
    writer.flush()?;
    Ok(())
}

/// Write the selected datasets into a directory, returning the files created.
pub fn write_csvs<P: AsRef<Path>>(
    directory: P,
    datasets: &[CsvDataset],
    data: &ExportData,
    options: &ExportOptions,
) -> Result<Vec<PathBuf>> {
    let directory = directory.as_ref();
    std::fs::create_dir_all(directory)?;
    let mut written = Vec::new();

    for dataset in datasets {
        let path = directory.join(dataset.file_name());
        match dataset {
            CsvDataset::Sessions => {
                let mut writer = csv::Writer::from_path(&path)?;
                writer.write_record([
                    "date",
                    "clock_in",
                    "clock_out",
                    "clocked_seconds",
                    "break_seconds",
                    "work_seconds",
                    "active_seconds",
                    "idle_seconds",
                    "untracked_seconds",
                    "note",
                ])?;
                for session in &data.sessions {
                    writer.write_record([
                        session.date.clone(),
                        format_local_time(session.clock_in_ms),
                        session
                            .clock_out_ms
                            .map(format_local_time)
                            .unwrap_or_default(),
                        seconds(session.clocked_ms),
                        seconds(session.break_ms),
                        seconds(session.work_ms),
                        seconds(session.active_ms),
                        seconds(session.idle_ms),
                        seconds(session.untracked_ms),
                        session.note.clone().unwrap_or_default(),
                    ])?;
                }
                writer.flush()?;
            }
            CsvDataset::Activities => {
                let mut writer = csv::Writer::from_path(&path)?;
                writer.write_record([
                    "date",
                    "start",
                    "end",
                    "duration_seconds",
                    "source",
                    "kind",
                    "application",
                    "process",
                    "window_title",
                    "browser",
                    "domain",
                    "page_title",
                    "url",
                    "category",
                    "project",
                    "interaction_type",
                ])?;
                for row in &data.activities {
                    let segment = &row.segment;
                    writer.write_record([
                        format_local_date(segment.started_at_ms),
                        format_local_time(segment.started_at_ms),
                        format_local_time(segment.ended_at_ms),
                        seconds(segment.duration_ms()),
                        segment.source.as_str().to_string(),
                        segment.kind.as_str().to_string(),
                        segment.app_name.clone().unwrap_or_default(),
                        segment.process_name.clone().unwrap_or_default(),
                        segment.window_title.clone().unwrap_or_default(),
                        segment.browser.clone().unwrap_or_default(),
                        segment.domain.clone().unwrap_or_default(),
                        segment.page_title.clone().unwrap_or_default(),
                        options
                            .url_privacy
                            .apply(segment.url.as_deref())
                            .unwrap_or_default(),
                        row.category_name.clone().unwrap_or_default(),
                        row.project_name.clone().unwrap_or_default(),
                        segment.interaction_type.clone().unwrap_or_default(),
                    ])?;
                }
                writer.flush()?;
            }
            CsvDataset::DailySummary => {
                let mut writer = csv::Writer::from_path(&path)?;
                writer.write_record([
                    "date",
                    "first_clock_in",
                    "last_clock_out",
                    "clocked_seconds",
                    "work_seconds",
                    "active_seconds",
                    "idle_seconds",
                    "break_seconds",
                    "untracked_seconds",
                ])?;
                for day in &data.daily {
                    writer.write_record([
                        day.date.clone(),
                        day.first_clock_in_ms
                            .map(format_local_time)
                            .unwrap_or_default(),
                        day.last_clock_out_ms
                            .map(format_local_time)
                            .unwrap_or_default(),
                        seconds(day.summary.clocked_ms),
                        seconds(day.summary.work_ms),
                        seconds(day.summary.active_ms),
                        seconds(day.summary.idle_ms),
                        seconds(day.summary.break_ms),
                        seconds(day.summary.untracked_ms),
                    ])?;
                }
                writer.flush()?;
            }
            CsvDataset::Applications => match &data.applications {
                Some(report) => write_report(&path, report)?,
                None => continue,
            },
            CsvDataset::Websites => match &data.websites {
                Some(report) => write_report(&path, report)?,
                None => continue,
            },
            CsvDataset::Pages => match &data.pages {
                Some(report) => write_report(&path, report)?,
                None => continue,
            },
            CsvDataset::Categories => match &data.categories {
                Some(report) => write_report(&path, report)?,
                None => continue,
            },
            CsvDataset::Projects => match &data.projects {
                Some(report) => write_report(&path, report)?,
                None => continue,
            },
        }
        written.push(path);
    }

    Ok(written)
}
