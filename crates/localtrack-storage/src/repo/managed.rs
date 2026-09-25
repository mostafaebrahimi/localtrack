//! Employee-mode storage: enrollment, policy, and the outbox of reports
//! waiting for the server (spec-external; see `docs/managed-mode.md`).

use localtrack_core::managed::{Enrollment, ManagedPolicy};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use crate::error::{Result, StorageError};

// ------------------------------------------------------------- enrollment

pub fn get_enrollment(conn: &Connection) -> Result<Option<Enrollment>> {
    let mut stmt = conn.prepare(
        "SELECT server_url, device_id, device_token, organization, employee_ref,
                enrolled_at_ms, last_policy_at_ms, last_report_at_ms,
                last_heartbeat_at_ms, last_error
         FROM managed_enrollment WHERE id = 1",
    )?;
    let enrollment = stmt
        .query_row([], |row| {
            Ok(Enrollment {
                server_url: row.get(0)?,
                device_id: row.get(1)?,
                device_token: row.get(2)?,
                organization: row.get(3)?,
                employee_ref: row.get(4)?,
                enrolled_at_ms: row.get(5)?,
                last_policy_at_ms: row.get(6)?,
                last_report_at_ms: row.get(7)?,
                last_heartbeat_at_ms: row.get(8)?,
                last_error: row.get(9)?,
            })
        })
        .optional()?;
    Ok(enrollment)
}

pub fn save_enrollment(tx: &Transaction<'_>, enrollment: &Enrollment) -> Result<()> {
    tx.execute(
        "INSERT INTO managed_enrollment (
            id, server_url, device_id, device_token, organization, employee_ref,
            enrolled_at_ms, last_policy_at_ms, last_report_at_ms, last_heartbeat_at_ms, last_error
         ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET
            server_url = excluded.server_url,
            device_id = excluded.device_id,
            device_token = excluded.device_token,
            organization = excluded.organization,
            employee_ref = excluded.employee_ref,
            enrolled_at_ms = excluded.enrolled_at_ms,
            last_policy_at_ms = excluded.last_policy_at_ms,
            last_report_at_ms = excluded.last_report_at_ms,
            last_heartbeat_at_ms = excluded.last_heartbeat_at_ms,
            last_error = excluded.last_error",
        params![
            enrollment.server_url,
            enrollment.device_id,
            enrollment.device_token,
            enrollment.organization,
            enrollment.employee_ref,
            enrollment.enrolled_at_ms,
            enrollment.last_policy_at_ms,
            enrollment.last_report_at_ms,
            enrollment.last_heartbeat_at_ms,
            enrollment.last_error,
        ],
    )?;
    Ok(())
}

/// Record when the server was last reached, and why it was not.
pub fn touch_enrollment(
    tx: &Transaction<'_>,
    column: EnrollmentTimestamp,
    at_ms: i64,
    error: Option<&str>,
) -> Result<()> {
    let sql = match column {
        EnrollmentTimestamp::Policy => {
            "UPDATE managed_enrollment SET last_policy_at_ms = ?1, last_error = ?2 WHERE id = 1"
        }
        EnrollmentTimestamp::Report => {
            "UPDATE managed_enrollment SET last_report_at_ms = ?1, last_error = ?2 WHERE id = 1"
        }
        EnrollmentTimestamp::Heartbeat => {
            "UPDATE managed_enrollment SET last_heartbeat_at_ms = ?1, last_error = ?2 WHERE id = 1"
        }
    };
    tx.execute(sql, params![at_ms, error])?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollmentTimestamp {
    Policy,
    Report,
    Heartbeat,
}

/// Leaving managed mode removes the enrollment, the policy and anything still
/// queued for the server. Collected activity is untouched: it is the person's
/// own record of their time.
pub fn clear_enrollment(tx: &Transaction<'_>) -> Result<()> {
    tx.execute("DELETE FROM managed_enrollment", [])?;
    tx.execute("DELETE FROM managed_policy", [])?;
    tx.execute("DELETE FROM sync_outbox", [])?;
    tx.execute("UPDATE categories SET managed_key = NULL", [])?;
    tx.execute("UPDATE classification_rules SET managed_key = NULL", [])?;
    Ok(())
}

// ----------------------------------------------------------------- policy

pub fn get_policy(conn: &Connection) -> Result<Option<ManagedPolicy>> {
    let mut stmt = conn.prepare("SELECT document_json FROM managed_policy WHERE id = 1")?;
    let raw: Option<String> = stmt.query_row([], |row| row.get(0)).optional()?;
    match raw {
        Some(raw) => Ok(serde_json::from_str(&raw).ok()),
        None => Ok(None),
    }
}

pub fn save_policy(tx: &Transaction<'_>, policy: &ManagedPolicy, now_ms: i64) -> Result<()> {
    tx.execute(
        "INSERT INTO managed_policy (id, revision, document_json, received_at_ms)
         VALUES (1, ?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET
            revision = excluded.revision,
            document_json = excluded.document_json,
            received_at_ms = excluded.received_at_ms",
        params![policy.revision, serde_json::to_string(policy)?, now_ms],
    )?;
    Ok(())
}

// ----------------------------------------------------------------- outbox

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxEntry {
    pub id: String,
    pub kind: String,
    pub period_key: Option<String>,
    pub payload_json: String,
    pub created_at_ms: i64,
    pub attempts: i64,
    pub last_attempt_ms: Option<i64>,
    pub last_error: Option<String>,
    pub delivered_at_ms: Option<i64>,
}

pub const KIND_DAILY_REPORT: &str = "daily_report";

/// Queue a payload. A second report for the same day replaces the first.
pub fn enqueue(
    tx: &Transaction<'_>,
    id: &str,
    kind: &str,
    period_key: Option<&str>,
    payload_json: &str,
    now_ms: i64,
) -> Result<()> {
    tx.execute(
        "INSERT INTO sync_outbox (id, kind, period_key, payload_json, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(kind, period_key) WHERE period_key IS NOT NULL DO UPDATE SET
            payload_json = excluded.payload_json,
            created_at_ms = excluded.created_at_ms,
            attempts = 0,
            last_error = NULL,
            delivered_at_ms = NULL",
        params![id, kind, period_key, payload_json, now_ms],
    )?;
    Ok(())
}

pub fn pending(conn: &Connection, limit: i64) -> Result<Vec<OutboxEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, period_key, payload_json, created_at_ms, attempts,
                last_attempt_ms, last_error, delivered_at_ms
         FROM sync_outbox
         WHERE delivered_at_ms IS NULL
         ORDER BY created_at_ms ASC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |row| {
        Ok(OutboxEntry {
            id: row.get(0)?,
            kind: row.get(1)?,
            period_key: row.get(2)?,
            payload_json: row.get(3)?,
            created_at_ms: row.get(4)?,
            attempts: row.get(5)?,
            last_attempt_ms: row.get(6)?,
            last_error: row.get(7)?,
            delivered_at_ms: row.get(8)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Recently delivered payloads, for the "what has been sent" list.
pub fn delivered(conn: &Connection, limit: i64) -> Result<Vec<OutboxEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, period_key, payload_json, created_at_ms, attempts,
                last_attempt_ms, last_error, delivered_at_ms
         FROM sync_outbox
         WHERE delivered_at_ms IS NOT NULL
         ORDER BY delivered_at_ms DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |row| {
        Ok(OutboxEntry {
            id: row.get(0)?,
            kind: row.get(1)?,
            period_key: row.get(2)?,
            payload_json: row.get(3)?,
            created_at_ms: row.get(4)?,
            attempts: row.get(5)?,
            last_attempt_ms: row.get(6)?,
            last_error: row.get(7)?,
            delivered_at_ms: row.get(8)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn mark_delivered(tx: &Transaction<'_>, id: &str, now_ms: i64) -> Result<()> {
    tx.execute(
        "UPDATE sync_outbox
         SET delivered_at_ms = ?2, last_attempt_ms = ?2, last_error = NULL,
             attempts = attempts + 1
         WHERE id = ?1",
        params![id, now_ms],
    )?;
    Ok(())
}

pub fn mark_failed(tx: &Transaction<'_>, id: &str, now_ms: i64, error: &str) -> Result<()> {
    tx.execute(
        "UPDATE sync_outbox
         SET attempts = attempts + 1, last_attempt_ms = ?2, last_error = ?3
         WHERE id = ?1",
        params![id, now_ms, error],
    )?;
    Ok(())
}

/// Keep the delivered history bounded; it exists to be shown, not archived.
pub fn prune_delivered(tx: &Transaction<'_>, keep: i64) -> Result<usize> {
    Ok(tx.execute(
        "DELETE FROM sync_outbox
         WHERE delivered_at_ms IS NOT NULL
           AND id NOT IN (
             SELECT id FROM sync_outbox
             WHERE delivered_at_ms IS NOT NULL
             ORDER BY delivered_at_ms DESC LIMIT ?1
           )",
        [keep],
    )?)
}

pub fn pending_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM sync_outbox WHERE delivered_at_ms IS NULL",
        [],
        |row| row.get(0),
    )?)
}

// ------------------------------------------------- server-managed catalogue

/// Insert or update a category the server owns, matched on its server key.
pub fn upsert_managed_category(
    tx: &Transaction<'_>,
    managed_key: &str,
    name: &str,
    now_ms: i64,
) -> Result<String> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM categories WHERE managed_key = ?1",
            [managed_key],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(id) = existing {
        tx.execute(
            "UPDATE categories SET name = ?2, updated_at_ms = ?3 WHERE id = ?1",
            params![id, name, now_ms],
        )?;
        return Ok(id);
    }

    // Adopt a category the person already created under the same name rather
    // than ending up with two rows that mean the same thing.
    let by_name: Option<String> = tx
        .query_row(
            "SELECT id FROM categories WHERE name = ?1 COLLATE NOCASE",
            [name],
            |row| row.get(0),
        )
        .optional()?;

    let id = by_name.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    tx.execute(
        "INSERT INTO categories (id, name, created_at_ms, updated_at_ms, managed_key)
         VALUES (?1, ?2, ?3, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            managed_key = excluded.managed_key,
            updated_at_ms = excluded.updated_at_ms",
        params![id, name, now_ms, managed_key],
    )?;
    Ok(id)
}

/// Insert or update a rule the server owns.
pub fn upsert_managed_rule(
    tx: &Transaction<'_>,
    managed_key: &str,
    rule: &localtrack_core::classification::ClassificationRule,
) -> Result<()> {
    rule.validate()?;
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM classification_rules WHERE managed_key = ?1",
            [managed_key],
            |row| row.get(0),
        )
        .optional()?;
    let id = existing.unwrap_or_else(|| rule.id.clone());

    tx.execute(
        "INSERT INTO classification_rules (
            id, name, enabled, priority, target_field, operator, pattern,
            category_id, project_id, created_at_ms, updated_at_ms, managed_key
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            enabled = excluded.enabled,
            priority = excluded.priority,
            target_field = excluded.target_field,
            operator = excluded.operator,
            pattern = excluded.pattern,
            category_id = excluded.category_id,
            updated_at_ms = excluded.updated_at_ms,
            managed_key = excluded.managed_key",
        params![
            id,
            rule.name,
            i64::from(rule.enabled),
            rule.priority,
            rule.target_field.as_str(),
            rule.operator.as_str(),
            rule.pattern,
            rule.category_id,
            rule.project_id,
            rule.created_at_ms,
            rule.updated_at_ms,
            managed_key,
        ],
    )?;
    Ok(())
}

/// Remove managed rows the server no longer publishes, leaving local ones alone.
pub fn retain_managed(
    tx: &Transaction<'_>,
    category_keys: &[String],
    rule_keys: &[String],
) -> Result<()> {
    let keep_categories = serde_json::to_string(category_keys)?;
    let keep_rules = serde_json::to_string(rule_keys)?;
    tx.execute(
        "DELETE FROM classification_rules
         WHERE managed_key IS NOT NULL
           AND managed_key NOT IN (SELECT value FROM json_each(?1))",
        [keep_rules],
    )?;
    // Categories are only unlinked: activity may already point at them.
    tx.execute(
        "UPDATE categories SET managed_key = NULL
         WHERE managed_key IS NOT NULL
           AND managed_key NOT IN (SELECT value FROM json_each(?1))",
        [keep_categories],
    )?;
    Ok(())
}

/// Category id → server key, used to label totals in a report.
pub fn managed_category_keys(
    conn: &Connection,
) -> Result<std::collections::BTreeMap<String, String>> {
    let mut stmt =
        conn.prepare("SELECT id, managed_key FROM categories WHERE managed_key IS NOT NULL")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = std::collections::BTreeMap::new();
    for row in rows {
        let (id, key) = row?;
        out.insert(id, key);
    }
    Ok(out)
}

/// True when a row is server-managed and must not be edited locally.
pub fn is_managed_category(conn: &Connection, id: &str) -> Result<bool> {
    let managed: Option<Option<String>> = conn
        .query_row(
            "SELECT managed_key FROM categories WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    match managed {
        Some(key) => Ok(key.is_some()),
        None => Err(StorageError::NotFound(format!("category {id}"))),
    }
}

pub fn is_managed_rule(conn: &Connection, id: &str) -> Result<bool> {
    let managed: Option<Option<String>> = conn
        .query_row(
            "SELECT managed_key FROM classification_rules WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    match managed {
        Some(key) => Ok(key.is_some()),
        None => Err(StorageError::NotFound(format!("rule {id}"))),
    }
}

// ------------------------------------------------------ session synchronisation

/// Sessions changed on this device and not yet accepted by the server.
pub fn dirty_sessions(conn: &Connection, limit: i64) -> Result<Vec<(String, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT id, remote_id FROM work_sessions
         WHERE sync_dirty = 1
         ORDER BY started_at_ms ASC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn clear_dirty(tx: &Transaction<'_>, client_id: &str, remote_id: Option<&str>) -> Result<()> {
    tx.execute(
        "UPDATE work_sessions
         SET sync_dirty = 0, remote_id = COALESCE(?2, remote_id)
         WHERE id = ?1",
        params![client_id, remote_id],
    )?;
    Ok(())
}

/// Deletions waiting to reach the server.
pub fn pending_tombstones(conn: &Connection) -> Result<Vec<(String, Option<String>, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT client_id, remote_id, deleted_at_ms FROM session_tombstones WHERE synced = 0",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn mark_tombstone_synced(tx: &Transaction<'_>, client_id: &str) -> Result<()> {
    tx.execute(
        "UPDATE session_tombstones SET synced = 1 WHERE client_id = ?1",
        [client_id],
    )?;
    Ok(())
}

pub fn record_tombstone(
    tx: &Transaction<'_>,
    client_id: &str,
    remote_id: Option<&str>,
    deleted_at_ms: i64,
    synced: bool,
) -> Result<()> {
    tx.execute(
        "INSERT INTO session_tombstones (client_id, remote_id, deleted_at_ms, synced)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(client_id) DO UPDATE SET
            remote_id = COALESCE(excluded.remote_id, session_tombstones.remote_id),
            deleted_at_ms = excluded.deleted_at_ms,
            synced = excluded.synced",
        params![client_id, remote_id, deleted_at_ms, i64::from(synced)],
    )?;
    Ok(())
}

/// Has this device ever seen this session, and in what state?
pub fn local_record(
    conn: &Connection,
    client_id: &str,
    remote_id: Option<&str>,
) -> Result<Option<localtrack_core::managed::LocalRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, remote_id, updated_at_ms, sync_dirty FROM work_sessions
         WHERE id = ?1 OR (?2 IS NOT NULL AND remote_id = ?2)
         LIMIT 1",
    )?;
    let found = stmt
        .query_row(params![client_id, remote_id], |row| {
            Ok(localtrack_core::managed::LocalRecord {
                client_id: row.get(0)?,
                remote_id: row.get(1)?,
                updated_at_ms: row.get(2)?,
                dirty: row.get::<_, i64>(3)? != 0,
                deleted: false,
            })
        })
        .optional()?;
    if found.is_some() {
        return Ok(found);
    }

    // A deletion this device already knows about still counts as "seen".
    let mut stmt = conn.prepare(
        "SELECT client_id, remote_id, deleted_at_ms FROM session_tombstones
         WHERE client_id = ?1 OR (?2 IS NOT NULL AND remote_id = ?2)
         LIMIT 1",
    )?;
    let tombstone = stmt
        .query_row(params![client_id, remote_id], |row| {
            Ok(localtrack_core::managed::LocalRecord {
                client_id: row.get(0)?,
                remote_id: row.get(1)?,
                updated_at_ms: row.get(2)?,
                dirty: false,
                deleted: true,
            })
        })
        .optional()?;
    Ok(tombstone)
}

/// Write a session that arrived from the server, replacing its breaks.
pub fn apply_remote_session(
    tx: &Transaction<'_>,
    item: &localtrack_core::managed::SessionSyncItem,
    now_ms: i64,
) -> Result<()> {
    // Match on the server id first, then on the id this device generated.
    let existing: Option<String> = tx
        .query_row(
            "SELECT id FROM work_sessions WHERE (?1 IS NOT NULL AND remote_id = ?1) OR id = ?2",
            params![item.remote_id, item.client_id],
            |row| row.get(0),
        )
        .optional()?;
    let id = existing.unwrap_or_else(|| item.client_id.clone());

    tx.execute(
        "INSERT INTO work_sessions (
            id, started_at_ms, ended_at_ms, start_timezone_offset_min, end_timezone_offset_min,
            note, created_manually, edited_manually, created_at_ms, updated_at_ms,
            remote_id, sync_dirty
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, ?7, ?8, ?9, 0)
         ON CONFLICT(id) DO UPDATE SET
            started_at_ms = excluded.started_at_ms,
            ended_at_ms = excluded.ended_at_ms,
            end_timezone_offset_min = excluded.end_timezone_offset_min,
            note = excluded.note,
            updated_at_ms = excluded.updated_at_ms,
            remote_id = COALESCE(excluded.remote_id, work_sessions.remote_id),
            sync_dirty = 0",
        params![
            id,
            item.started_at_ms,
            item.ended_at_ms,
            localtrack_core::time::offset_minutes_at(item.started_at_ms),
            item.ended_at_ms
                .map(localtrack_core::time::offset_minutes_at),
            item.note,
            now_ms,
            item.updated_at_ms,
            item.remote_id,
        ],
    )?;

    // Breaks travel with their session; replacing them keeps the two sides
    // identical rather than merging two partial lists.
    tx.execute("DELETE FROM work_breaks WHERE work_session_id = ?1", [&id])?;
    for brk in &item.breaks {
        tx.execute(
            "INSERT INTO work_breaks (
                id, work_session_id, started_at_ms, ended_at_ms, note, created_at_ms, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?5)",
            params![
                uuid::Uuid::new_v4().to_string(),
                id,
                brk.started_at_ms,
                brk.ended_at_ms,
                now_ms,
            ],
        )?;
    }
    Ok(())
}

/// Delete a session because the server says it is gone.
pub fn apply_remote_deletion(
    tx: &Transaction<'_>,
    item: &localtrack_core::managed::SessionSyncItem,
) -> Result<()> {
    tx.execute(
        "DELETE FROM work_sessions WHERE (?1 IS NOT NULL AND remote_id = ?1) OR id = ?2",
        params![item.remote_id, item.client_id],
    )?;
    record_tombstone(
        tx,
        &item.client_id,
        item.remote_id.as_deref(),
        item.deleted_at_ms.unwrap_or_else(crate::now_ms),
        true,
    )
}

// -------------------------------------------------------------- sync cursor

pub fn get_sync_state(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT value FROM sync_state WHERE key = ?1")?;
    Ok(stmt.query_row([key], |row| row.get(0)).optional()?)
}

pub fn set_sync_state(tx: &Transaction<'_>, key: &str, value: &str, now_ms: i64) -> Result<()> {
    tx.execute(
        "INSERT INTO sync_state (key, value, updated_at_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at_ms = excluded.updated_at_ms",
        params![key, value, now_ms],
    )?;
    Ok(())
}

pub const CURSOR_SESSIONS: &str = "sessions_cursor";
