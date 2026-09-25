//! Data deletion, retention, backup and restore (spec §111, §112, §124–§126).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use localtrack_core::settings::{keys, Settings};
use localtrack_core::time::{format_local_date, now_ms, DAY_MS};
use localtrack_storage::repo::maintenance::RetentionReport;
use localtrack_storage::{paths, repo, Database};

use crate::error::{AppError, Result};
use crate::types::DeletionPreview;

pub fn preview_deletion(db: &Arc<Database>, from_ms: i64, to_ms: i64) -> Result<DeletionPreview> {
    let (segments, sessions) =
        db.write(|tx| repo::maintenance::preview_range(tx, from_ms, to_ms))?;
    Ok(DeletionPreview { segments, sessions })
}

pub fn delete_range(
    db: &Arc<Database>,
    from_ms: i64,
    to_ms: i64,
    include_sessions: bool,
) -> Result<RetentionReport> {
    if to_ms <= from_ms {
        return Err(AppError::Invalid("the deletion range is empty".into()));
    }
    let now = now_ms();
    db.write(|tx| repo::maintenance::delete_range(tx, from_ms, to_ms, include_sessions, now))
        .map_err(Into::into)
}

pub fn delete_last(
    db: &Arc<Database>,
    minutes: i64,
    include_sessions: bool,
) -> Result<RetentionReport> {
    let now = now_ms();
    delete_range(db, now - minutes * 60_000, now, include_sessions)
}

pub fn delete_all(db: &Arc<Database>, include_sessions: bool) -> Result<RetentionReport> {
    db.write(|tx| repo::maintenance::delete_all(tx, include_sessions))
        .map_err(Into::into)
}

/// Run retention at most once a day (spec §112).
pub fn run_retention_if_due(
    db: &Arc<Database>,
    settings: &Settings,
) -> Result<Option<RetentionReport>> {
    let now = now_ms();
    let Some(cutoff) = settings.retention_cutoff_ms(now) else {
        return Ok(None);
    };
    if now - settings.last_retention_run_ms < DAY_MS {
        return Ok(None);
    }

    let report = db.write(|tx| {
        let report =
            repo::maintenance::apply_retention(tx, cutoff, settings.retention_delete_sessions)?;
        repo::uptime::prune(tx, cutoff)?;
        repo::settings::set(
            tx,
            keys::LAST_RETENTION_RUN_MS,
            &serde_json::json!(now),
            now,
        )?;
        Ok(report)
    })?;

    tracing::info!(
        segments = report.segments_deleted,
        sessions = report.sessions_deleted,
        "retention applied"
    );
    Ok(Some(report))
}

/// Write a timestamped backup (spec §125).
pub fn backup(db: &Arc<Database>, destination: Option<&Path>) -> Result<PathBuf> {
    let path = match destination {
        Some(path) => path.to_path_buf(),
        None => paths::backups_dir().join(format!(
            "localtrack-backup-{}.db",
            format_local_date(now_ms())
        )),
    };
    db.checkpoint()?;
    let written = db.backup_to(&path)?;
    Ok(written)
}

/// Validate, keep a safety copy, then restore (spec §126).
pub fn restore(db: &Arc<Database>, source: &Path) -> Result<PathBuf> {
    Database::validate_backup_file(source)?;
    let safety = paths::backups_dir().join(format!("localtrack-pre-restore-{}.db", now_ms()));
    std::fs::create_dir_all(paths::backups_dir())?;
    db.restore_from(source, safety.as_path())?;
    Ok(safety)
}

pub fn checkpoint(db: &Arc<Database>) -> Result<()> {
    db.checkpoint().map_err(Into::into)
}

pub fn vacuum(db: &Arc<Database>) -> Result<()> {
    db.vacuum().map_err(Into::into)
}
