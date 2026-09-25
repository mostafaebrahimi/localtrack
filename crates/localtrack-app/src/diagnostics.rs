//! Settings → Diagnostics (spec §109, §154).
//!
//! Copied diagnostics contain no activity data: no URLs, titles or notes.

use std::sync::Arc;

use localtrack_collector_common::collector::CollectorStatus;
use localtrack_collector_common::pipeline::{PipelineStats, TrackingDecision};
use localtrack_storage::{paths, Database, DatabaseHealth};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::tracking::DesktopSupport;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub app_version: String,
    pub schema_version: i64,
    pub protocol_version: u32,
    /// Reported by the native host the last time Chrome connected (spec §154).
    pub native_host_version: Option<String>,
    pub chrome_extension_origin: Option<String>,
    pub chrome_connected_at_ms: Option<i64>,
    pub platform: String,
    pub desktop: DesktopSupport,
    pub collectors: Vec<CollectorStatus>,
    pub tracking: TrackingDecision,
    pub database: DatabaseHealth,
    pub database_integrity_ok: bool,
    pub data_directory: String,
    pub stats: PipelineStats,
    pub generated_at_ms: i64,
}

impl Diagnostics {
    /// A plain-text block for the "Copy Diagnostics" button.
    pub fn to_plain_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("LocalTrack {}\n", self.app_version));
        out.push_str(&format!("Database schema {}\n", self.schema_version));
        out.push_str(&format!("Native protocol {}\n", self.protocol_version));
        if let Some(version) = &self.native_host_version {
            out.push_str(&format!("Native host {version}\n"));
        }
        out.push_str(&format!("Platform: {}\n", self.platform));
        out.push_str(&format!(
            "Desktop adapter: {} (active window: {}, afk: {}, lock: {})\n",
            self.desktop.adapter, self.desktop.active_window, self.desktop.afk, self.desktop.lock
        ));
        for collector in &self.collectors {
            out.push_str(&format!(
                "Collector {}: {}\n",
                collector.name,
                if !collector.available {
                    "unavailable"
                } else if collector.healthy {
                    "healthy"
                } else {
                    "unhealthy"
                }
            ));
        }
        out.push_str(&format!("Tracking: {:?}\n", self.tracking));
        out.push_str(&format!(
            "Database size: {} bytes\n",
            self.database.size_bytes
        ));
        out.push_str(&format!("Segments: {}\n", self.database.segment_count));
        out.push_str(&format!("Sessions: {}\n", self.database.session_count));
        out.push_str(&format!("Journal mode: {}\n", self.database.journal_mode));
        out.push_str(&format!("Integrity: {}\n", self.database_integrity_ok));
        out.push_str(&format!(
            "Observations: {} received, {} written\n",
            self.stats.observations_received, self.stats.segments_written
        ));
        out
    }
}

pub fn collect(
    db: &Arc<Database>,
    collectors: Vec<CollectorStatus>,
    tracking: TrackingDecision,
    stats: PipelineStats,
    now_ms: i64,
) -> Result<Diagnostics> {
    let (native_host_version, chrome_extension_origin, chrome_connected_at_ms) =
        db.read(|conn| {
            Ok((
                localtrack_storage::repo::settings::get(conn, "native_host_version")?
                    .and_then(|value| value.as_str().map(|v| v.to_string())),
                localtrack_storage::repo::settings::get(conn, "chrome_extension_origin")?
                    .and_then(|value| value.as_str().map(|v| v.to_string())),
                localtrack_storage::repo::settings::get(conn, "chrome_connected_at_ms")?
                    .and_then(|value| value.as_i64()),
            ))
        })?;

    Ok(Diagnostics {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version: localtrack_storage::SCHEMA_VERSION,
        protocol_version: localtrack_core::NATIVE_PROTOCOL_VERSION,
        native_host_version,
        chrome_extension_origin,
        chrome_connected_at_ms,
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        desktop: crate::tracking::desktop_capabilities(),
        collectors,
        tracking,
        database: db.health()?,
        database_integrity_ok: db.integrity_check()?,
        data_directory: paths::data_dir().display().to_string(),
        stats,
        generated_at_ms: now_ms,
    })
}
