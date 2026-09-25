//! When LocalTrack itself was running.
//!
//! Reports use this to explain a gap: "nothing was observed" and "LocalTrack was
//! not running" look identical in the activity table but mean different things
//! to the person reading the report.

use localtrack_core::interval::{Interval, IntervalSet};
use rusqlite::{params, Connection, Transaction};

use crate::error::Result;

/// Record that this process has started.
pub fn start(tx: &Transaction<'_>, id: &str, now_ms: i64, app_version: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO agent_uptime (id, started_at_ms, last_seen_ms, stopped_at_ms, app_version)
         VALUES (?1, ?2, ?2, NULL, ?3)",
        params![id, now_ms, app_version],
    )?;
    Ok(())
}

/// Keep the running row fresh, so a crash still leaves an accurate end time.
pub fn heartbeat(tx: &Transaction<'_>, id: &str, now_ms: i64) -> Result<()> {
    tx.execute(
        "UPDATE agent_uptime SET last_seen_ms = ?2 WHERE id = ?1",
        params![id, now_ms],
    )?;
    Ok(())
}

pub fn stop(tx: &Transaction<'_>, id: &str, now_ms: i64) -> Result<()> {
    tx.execute(
        "UPDATE agent_uptime SET last_seen_ms = ?2, stopped_at_ms = ?2 WHERE id = ?1",
        params![id, now_ms],
    )?;
    Ok(())
}

/// The periods LocalTrack was running inside a window.
pub fn running_intervals(conn: &Connection, from_ms: i64, to_ms: i64) -> Result<IntervalSet> {
    let mut stmt = conn.prepare(
        "SELECT started_at_ms, COALESCE(stopped_at_ms, last_seen_ms)
         FROM agent_uptime
         WHERE started_at_ms < ?1 AND COALESCE(stopped_at_ms, last_seen_ms) > ?2
         ORDER BY started_at_ms",
    )?;
    let rows = stmt.query_map(params![to_ms, from_ms], |row| {
        Ok(Interval::new(row.get(0)?, row.get(1)?))
    })?;
    let mut intervals = Vec::new();
    for row in rows {
        intervals.push(row?);
    }
    // A restart leaves a moment between two rows; anything under the poll
    // interval is not a real absence.
    Ok(IntervalSet::from_intervals(intervals).merge_with_tolerance(5_000))
}

/// The first moment this table knows anything about.
///
/// Before it, nothing can be said about whether LocalTrack was running.
pub fn known_since(conn: &Connection) -> Result<Option<i64>> {
    let earliest: Option<i64> =
        conn.query_row("SELECT MIN(started_at_ms) FROM agent_uptime", [], |row| {
            row.get(0)
        })?;
    Ok(earliest)
}

/// Remove rows older than the cutoff, alongside activity retention.
pub fn prune(tx: &Transaction<'_>, cutoff_ms: i64) -> Result<usize> {
    Ok(tx.execute(
        "DELETE FROM agent_uptime WHERE COALESCE(stopped_at_ms, last_seen_ms) < ?1",
        [cutoff_ms],
    )?)
}
