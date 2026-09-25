//! Retention and data deletion (spec §111, §112).

use rusqlite::{params, Transaction};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionReport {
    pub segments_deleted: i64,
    pub sessions_deleted: i64,
    pub cutoff_ms: i64,
}

/// Delete activity older than the cutoff.
///
/// Work sessions are only removed when the user explicitly opted in
/// (spec §112): losing clocked history silently would be unacceptable.
pub fn apply_retention(
    tx: &Transaction<'_>,
    cutoff_ms: i64,
    delete_sessions: bool,
) -> Result<RetentionReport> {
    let segments_deleted = tx.execute(
        "DELETE FROM activity_segments WHERE ended_at_ms < ?1",
        params![cutoff_ms],
    )? as i64;

    let sessions_deleted = if delete_sessions {
        tx.execute(
            "DELETE FROM work_sessions WHERE ended_at_ms IS NOT NULL AND ended_at_ms < ?1",
            params![cutoff_ms],
        )? as i64
    } else {
        0
    };

    Ok(RetentionReport {
        segments_deleted,
        sessions_deleted,
        cutoff_ms,
    })
}

/// Delete everything inside a range (spec §111).
pub fn delete_range(
    tx: &Transaction<'_>,
    from_ms: i64,
    to_ms: i64,
    include_sessions: bool,
    now_ms: i64,
) -> Result<RetentionReport> {
    let segments_deleted = super::segments::delete_range(tx, from_ms, to_ms, now_ms)? as i64;

    let sessions_deleted = if include_sessions {
        let sessions = tx.execute(
            "DELETE FROM work_sessions
             WHERE started_at_ms >= ?1 AND COALESCE(ended_at_ms, ?1) <= ?2",
            params![from_ms, to_ms],
        )? as i64;
        tx.execute(
            "DELETE FROM work_breaks WHERE started_at_ms >= ?1 AND COALESCE(ended_at_ms, ?1) <= ?2",
            params![from_ms, to_ms],
        )?;
        sessions
    } else {
        0
    };

    Ok(RetentionReport {
        segments_deleted,
        sessions_deleted,
        cutoff_ms: to_ms,
    })
}

/// Delete all activity, optionally including sessions (spec §111).
pub fn delete_all(tx: &Transaction<'_>, include_sessions: bool) -> Result<RetentionReport> {
    let segments_deleted = tx.execute("DELETE FROM activity_segments", [])? as i64;
    let sessions_deleted = if include_sessions {
        tx.execute("DELETE FROM work_breaks", [])?;
        tx.execute("DELETE FROM work_sessions", [])? as i64
    } else {
        0
    };
    Ok(RetentionReport {
        segments_deleted,
        sessions_deleted,
        cutoff_ms: 0,
    })
}

/// How many sessions/segments would be affected by a range deletion.
pub fn preview_range(tx: &Transaction<'_>, from_ms: i64, to_ms: i64) -> Result<(i64, i64)> {
    let segments: i64 = tx.query_row(
        "SELECT COUNT(*) FROM activity_segments WHERE started_at_ms < ?1 AND ended_at_ms > ?2",
        params![to_ms, from_ms],
        |row| row.get(0),
    )?;
    let sessions: i64 = tx.query_row(
        "SELECT COUNT(*) FROM work_sessions
         WHERE started_at_ms < ?1 AND COALESCE(ended_at_ms, ?1) > ?2",
        params![to_ms, from_ms],
        |row| row.get(0),
    )?;
    Ok((segments, sessions))
}
