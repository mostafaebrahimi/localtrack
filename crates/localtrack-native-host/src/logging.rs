//! Rotating local logs (spec §108).
//!
//! Operational events only: no URLs, page titles, window titles, interaction
//! labels or user notes ever reach a log file.

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

pub fn init(component: &str) -> Option<WorkerGuard> {
    let dir = localtrack_storage::paths::logs_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }

    let appender = tracing_appender::rolling::daily(&dir, format!("{component}.log"));
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let filter =
        EnvFilter::try_from_env("LOCALTRACK_LOG").unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false)
        .finish();

    // stdout belongs to the native messaging protocol, so logs never go there.
    let _ = tracing::subscriber::set_global_default(subscriber);
    Some(guard)
}
