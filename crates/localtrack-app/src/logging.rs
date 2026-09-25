//! Rotating local logs for the desktop application (spec §108).
//!
//! Operational events only. URLs, page titles, window titles, interaction
//! labels and notes are never logged.

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// Files kept by the rotating appender.
pub const MAX_LOG_FILES: usize = 5;

pub fn init(component: &str) -> Option<WorkerGuard> {
    let dir = localtrack_storage::paths::logs_dir();
    std::fs::create_dir_all(&dir).ok()?;
    prune_old_logs(&dir, component);

    let appender = tracing_appender::rolling::daily(&dir, format!("{component}.log"));
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let filter =
        EnvFilter::try_from_env("LOCALTRACK_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
    Some(guard)
}

/// Keep only the newest [`MAX_LOG_FILES`] files for a component.
pub fn prune_old_logs(dir: &std::path::Path, component: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<_> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{component}.log"))
        })
        .collect();
    if files.len() <= MAX_LOG_FILES {
        return;
    }
    files.sort_by_key(|entry| {
        entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    for entry in files.iter().take(files.len() - MAX_LOG_FILES) {
        let _ = std::fs::remove_file(entry.path());
    }
}
