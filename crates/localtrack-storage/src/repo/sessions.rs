use localtrack_core::sessions::{ClockSnapshot, WorkBreak, WorkSession};
use rusqlite::{params, Connection, Row, Transaction};

use crate::error::{Result, StorageError};

const SESSION_COLUMNS: &str = "id, started_at_ms, ended_at_ms, start_timezone_offset_min, \
     end_timezone_offset_min, note, created_manually, edited_manually, created_at_ms, updated_at_ms";

const BREAK_COLUMNS: &str =
    "id, work_session_id, started_at_ms, ended_at_ms, note, created_at_ms, updated_at_ms";

fn map_session(row: &Row<'_>) -> rusqlite::Result<WorkSession> {
    Ok(WorkSession {
        id: row.get(0)?,
        started_at_ms: row.get(1)?,
        ended_at_ms: row.get(2)?,
        start_timezone_offset_min: row.get(3)?,
        end_timezone_offset_min: row.get(4)?,
        note: row.get(5)?,
        created_manually: row.get::<_, i64>(6)? != 0,
        edited_manually: row.get::<_, i64>(7)? != 0,
        created_at_ms: row.get(8)?,
        updated_at_ms: row.get(9)?,
    })
}

fn map_break(row: &Row<'_>) -> rusqlite::Result<WorkBreak> {
    Ok(WorkBreak {
        id: row.get(0)?,
        work_session_id: row.get(1)?,
        started_at_ms: row.get(2)?,
        ended_at_ms: row.get(3)?,
        note: row.get(4)?,
        created_at_ms: row.get(5)?,
        updated_at_ms: row.get(6)?,
    })
}

/// Flag a session as changed here and not yet sent to the server.
///
/// Harmless when the device is not enrolled: nothing ever reads the flag.
pub fn mark_dirty(tx: &Transaction<'_>, session_id: &str) -> Result<()> {
    tx.execute(
        "UPDATE work_sessions SET sync_dirty = 1 WHERE id = ?1",
        [session_id],
    )?;
    Ok(())
}

pub fn insert_session(tx: &Transaction<'_>, session: &WorkSession) -> Result<()> {
    tx.execute(
        "INSERT INTO work_sessions (
            id, started_at_ms, ended_at_ms, start_timezone_offset_min, end_timezone_offset_min,
            note, created_manually, edited_manually, created_at_ms, updated_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            session.id,
            session.started_at_ms,
            session.ended_at_ms,
            session.start_timezone_offset_min,
            session.end_timezone_offset_min,
            session.note,
            i64::from(session.created_manually),
            i64::from(session.edited_manually),
            session.created_at_ms,
            session.updated_at_ms,
        ],
    )?;
    Ok(())
}

pub fn update_session(tx: &Transaction<'_>, session: &WorkSession) -> Result<()> {
    let changed = tx.execute(
        "UPDATE work_sessions SET
            started_at_ms = ?2, ended_at_ms = ?3, start_timezone_offset_min = ?4,
            end_timezone_offset_min = ?5, note = ?6, created_manually = ?7,
            edited_manually = ?8, updated_at_ms = ?9
         WHERE id = ?1",
        params![
            session.id,
            session.started_at_ms,
            session.ended_at_ms,
            session.start_timezone_offset_min,
            session.end_timezone_offset_min,
            session.note,
            i64::from(session.created_manually),
            i64::from(session.edited_manually),
            session.updated_at_ms,
        ],
    )?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("session {}", session.id)));
    }
    mark_dirty(tx, &session.id)?;
    Ok(())
}

pub fn get_session(conn: &Connection, id: &str) -> Result<WorkSession> {
    let sql = format!("SELECT {SESSION_COLUMNS} FROM work_sessions WHERE id = ?1");
    conn.query_row(&sql, [id], map_session)
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => StorageError::NotFound(format!("session {id}")),
            other => StorageError::Sqlite(other),
        })
}

/// The single open session, if any (spec §15).
pub fn open_session(conn: &Connection) -> Result<Option<WorkSession>> {
    let sql = format!(
        "SELECT {SESSION_COLUMNS} FROM work_sessions
         WHERE ended_at_ms IS NULL
         ORDER BY started_at_ms DESC LIMIT 1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    match rows.next()? {
        Some(row) => Ok(Some(map_session(row)?)),
        None => Ok(None),
    }
}

/// Guard against more than one open session existing at once (spec §15).
pub fn count_open_sessions(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM work_sessions WHERE ended_at_ms IS NULL",
        [],
        |row| row.get(0),
    )?)
}

pub fn list_sessions(conn: &Connection, from_ms: i64, to_ms: i64) -> Result<Vec<WorkSession>> {
    let sql = format!(
        "SELECT {SESSION_COLUMNS} FROM work_sessions
         WHERE started_at_ms < ?1 AND COALESCE(ended_at_ms, ?2) > ?3
         ORDER BY started_at_ms ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![to_ms, i64::MAX, from_ms], map_session)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn delete_session(tx: &Transaction<'_>, id: &str) -> Result<()> {
    // Leave a tombstone so the deletion reaches the server instead of the
    // session reappearing at the next pull.
    tx.execute(
        "INSERT INTO session_tombstones (client_id, remote_id, deleted_at_ms, synced)
         SELECT id, remote_id, ?2, 0 FROM work_sessions WHERE id = ?1
         ON CONFLICT(client_id) DO UPDATE SET deleted_at_ms = excluded.deleted_at_ms, synced = 0",
        rusqlite::params![id, crate::now_ms()],
    )?;
    let changed = tx.execute("DELETE FROM work_sessions WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("session {id}")));
    }
    Ok(())
}

pub fn insert_break(tx: &Transaction<'_>, work_break: &WorkBreak) -> Result<()> {
    mark_dirty(tx, &work_break.work_session_id)?;
    tx.execute(
        "INSERT INTO work_breaks (
            id, work_session_id, started_at_ms, ended_at_ms, note, created_at_ms, updated_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            work_break.id,
            work_break.work_session_id,
            work_break.started_at_ms,
            work_break.ended_at_ms,
            work_break.note,
            work_break.created_at_ms,
            work_break.updated_at_ms,
        ],
    )?;
    Ok(())
}

pub fn update_break(tx: &Transaction<'_>, work_break: &WorkBreak) -> Result<()> {
    mark_dirty(tx, &work_break.work_session_id)?;
    let changed = tx.execute(
        "UPDATE work_breaks SET
            started_at_ms = ?2, ended_at_ms = ?3, note = ?4, updated_at_ms = ?5
         WHERE id = ?1",
        params![
            work_break.id,
            work_break.started_at_ms,
            work_break.ended_at_ms,
            work_break.note,
            work_break.updated_at_ms,
        ],
    )?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("break {}", work_break.id)));
    }
    Ok(())
}

pub fn delete_break(tx: &Transaction<'_>, id: &str) -> Result<()> {
    tx.execute(
        "UPDATE work_sessions SET sync_dirty = 1
         WHERE id = (SELECT work_session_id FROM work_breaks WHERE id = ?1)",
        [id],
    )?;
    let changed = tx.execute("DELETE FROM work_breaks WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("break {id}")));
    }
    Ok(())
}

pub fn breaks_for_session(conn: &Connection, session_id: &str) -> Result<Vec<WorkBreak>> {
    let sql = format!(
        "SELECT {BREAK_COLUMNS} FROM work_breaks WHERE work_session_id = ?1
         ORDER BY started_at_ms ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([session_id], map_break)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn breaks_in_range(conn: &Connection, from_ms: i64, to_ms: i64) -> Result<Vec<WorkBreak>> {
    let sql = format!(
        "SELECT {BREAK_COLUMNS} FROM work_breaks
         WHERE started_at_ms < ?1 AND COALESCE(ended_at_ms, ?2) > ?3
         ORDER BY started_at_ms ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![to_ms, i64::MAX, from_ms], map_break)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn open_break(conn: &Connection, session_id: &str) -> Result<Option<WorkBreak>> {
    let sql = format!(
        "SELECT {BREAK_COLUMNS} FROM work_breaks
         WHERE work_session_id = ?1 AND ended_at_ms IS NULL
         ORDER BY started_at_ms DESC LIMIT 1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([session_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(map_break(row)?)),
        None => Ok(None),
    }
}

/// Rebuild the clock snapshot from persisted rows — the crash-recovery path
/// (spec §20).
pub fn clock_snapshot(conn: &Connection) -> Result<ClockSnapshot> {
    match open_session(conn)? {
        Some(session) => {
            let breaks = breaks_for_session(conn, &session.id)?;
            Ok(ClockSnapshot::derive(Some(session), breaks))
        }
        None => Ok(ClockSnapshot::clocked_out()),
    }
}
