//! Export orchestration (spec §88–§95). Everything runs locally.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use localtrack_core::settings::Settings;
use localtrack_core::time::{format_local_date, now_ms};
use localtrack_export::csv_export::{write_csvs, CsvDataset};
use localtrack_export::model::{ActivityExportRow, ExportData, ExportOptions};
use localtrack_export::{xlsx, SessionExportRow};
use localtrack_storage::filters::ActivityFilter;
use localtrack_storage::{repo, Database};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::queries;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Xlsx,
    Csv,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub from_ms: i64,
    pub to_ms: i64,
    pub format: ExportFormat,
    /// Target file (XLSX) or directory (CSV).
    pub destination: String,
    #[serde(default)]
    pub options: ExportOptions,
    #[serde(default)]
    pub filter: ActivityFilter,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub files: Vec<String>,
    pub row_counts: ExportRowCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportRowCounts {
    pub sessions: usize,
    pub activities: usize,
    pub interactions: usize,
}

/// Whether any full URL was actually stored, which decides if the export dialog
/// may offer the "Full URL" option (spec §89).
pub fn full_urls_available(db: &Arc<Database>, from_ms: i64, to_ms: i64) -> Result<bool> {
    let segments = db.read(|conn| repo::segments::load_range(conn, from_ms, to_ms))?;
    Ok(segments
        .iter()
        .any(|s| s.url.as_deref().is_some_and(|u| u.contains('?'))))
}

pub fn build_export_data(
    db: &Arc<Database>,
    request: &ExportRequest,
    settings: &Settings,
) -> Result<ExportData> {
    let bundle = queries::report_bundle(
        db,
        request.from_ms,
        request.to_ms,
        &request.filter,
        settings,
    )?;
    let sessions = queries::session_details(db, request.from_ms, request.to_ms, settings)?;
    let categories = queries::category_names(db)?;
    let projects = queries::project_names(db)?;

    let session_rows: Vec<SessionExportRow> = sessions
        .iter()
        .map(|detail| SessionExportRow {
            date: format_local_date(detail.session.started_at_ms),
            clock_in_ms: detail.session.started_at_ms,
            clock_out_ms: detail.session.ended_at_ms,
            clocked_ms: detail.summary.clocked_ms,
            break_ms: detail.summary.break_ms,
            work_ms: detail.summary.work_ms,
            active_ms: detail.summary.active_ms,
            idle_ms: detail.summary.idle_ms,
            untracked_ms: detail.summary.untracked_ms,
            note: detail.session.note.clone(),
        })
        .collect();

    let mut filter = request.filter.clone();
    filter.from_ms = Some(request.from_ms);
    filter.to_ms = Some(request.to_ms);
    let segments =
        db.read(|conn| repo::segments::load_range(conn, request.from_ms, request.to_ms))?;

    let to_row = |segment: localtrack_core::activity::ActivitySegment| ActivityExportRow {
        category_name: segment
            .category_id
            .as_ref()
            .and_then(|id| categories.get(id).cloned()),
        project_name: segment
            .project_id
            .as_ref()
            .and_then(|id| projects.get(id).cloned()),
        segment,
    };

    let (interactions, activities): (Vec<_>, Vec<_>) = segments
        .into_iter()
        .partition(|s| s.kind == localtrack_core::activity::ActivityKind::Interaction);

    Ok(ExportData {
        from_ms: request.from_ms,
        to_ms: request.to_ms,
        generated_at_ms: now_ms(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        summary: bundle.summary,
        daily: bundle.daily,
        sessions: session_rows,
        applications: request
            .options
            .include_applications
            .then_some(bundle.applications),
        websites: request.options.include_websites.then_some(bundle.websites),
        pages: request.options.include_pages.then_some(bundle.pages),
        categories: request
            .options
            .include_categories
            .then_some(bundle.categories),
        projects: request.options.include_projects.then_some(bundle.projects),
        activities: activities.into_iter().map(to_row).collect(),
        interactions: interactions.into_iter().map(to_row).collect(),
    })
}

pub fn run_export(
    db: &Arc<Database>,
    request: &ExportRequest,
    settings: &Settings,
) -> Result<ExportResult> {
    if request.to_ms <= request.from_ms {
        return Err(AppError::Invalid("the export range is empty".into()));
    }
    let data = build_export_data(db, request, settings)?;
    let counts = ExportRowCounts {
        sessions: data.sessions.len(),
        activities: data.activities.len(),
        interactions: data.interactions.len(),
    };

    let files = match request.format {
        ExportFormat::Xlsx => {
            let path = ensure_extension(Path::new(&request.destination), "xlsx");
            vec![xlsx::write_workbook(path, &data, &request.options)?]
        }
        ExportFormat::Csv => {
            let mut datasets = Vec::new();
            if request.options.include_sessions {
                datasets.push(CsvDataset::Sessions);
            }
            if request.options.include_activity_detail {
                datasets.push(CsvDataset::Activities);
            }
            if request.options.include_daily_summary {
                datasets.push(CsvDataset::DailySummary);
            }
            if request.options.include_applications {
                datasets.push(CsvDataset::Applications);
            }
            if request.options.include_websites {
                datasets.push(CsvDataset::Websites);
            }
            if request.options.include_pages {
                datasets.push(CsvDataset::Pages);
            }
            if request.options.include_categories {
                datasets.push(CsvDataset::Categories);
            }
            if request.options.include_projects {
                datasets.push(CsvDataset::Projects);
            }
            write_csvs(&request.destination, &datasets, &data, &request.options)?
        }
    };

    Ok(ExportResult {
        files: files.iter().map(|p| p.display().to_string()).collect(),
        row_counts: counts,
    })
}

fn ensure_extension(path: &Path, extension: &str) -> PathBuf {
    match path.extension() {
        Some(existing) if existing.eq_ignore_ascii_case(extension) => path.to_path_buf(),
        _ => path.with_extension(extension),
    }
}
