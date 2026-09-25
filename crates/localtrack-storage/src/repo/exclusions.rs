use localtrack_core::privacy::{ExclusionAction, ExclusionRule, ExclusionTarget};
use rusqlite::{params, Connection, Row, Transaction};

use crate::error::{Result, StorageError};

const COLUMNS: &str = "id, enabled, target, pattern, action, created_at_ms, updated_at_ms";

fn map_row(row: &Row<'_>) -> rusqlite::Result<ExclusionRule> {
    let target: String = row.get(2)?;
    let action: String = row.get(4)?;
    Ok(ExclusionRule {
        id: row.get(0)?,
        enabled: row.get::<_, i64>(1)? != 0,
        target: ExclusionTarget::parse(&target).unwrap_or(ExclusionTarget::Domain),
        pattern: row.get(3)?,
        action: ExclusionAction::parse(&action).unwrap_or(ExclusionAction::Ignore),
        created_at_ms: row.get(5)?,
        updated_at_ms: row.get(6)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<ExclusionRule>> {
    let sql = format!("SELECT {COLUMNS} FROM exclusion_rules ORDER BY target ASC, pattern ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn upsert(tx: &Transaction<'_>, rule: &ExclusionRule) -> Result<()> {
    if rule.pattern.trim().is_empty() {
        return Err(StorageError::Invalid(
            "exclusion pattern must not be empty".into(),
        ));
    }
    tx.execute(
        "INSERT INTO exclusion_rules (id, enabled, target, pattern, action, created_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            enabled = excluded.enabled,
            target = excluded.target,
            pattern = excluded.pattern,
            action = excluded.action,
            updated_at_ms = excluded.updated_at_ms",
        params![
            rule.id,
            i64::from(rule.enabled),
            rule.target.as_str(),
            rule.pattern.trim(),
            rule.action.as_str(),
            rule.created_at_ms,
            rule.updated_at_ms,
        ],
    )?;
    Ok(())
}

pub fn delete(tx: &Transaction<'_>, id: &str) -> Result<()> {
    let changed = tx.execute("DELETE FROM exclusion_rules WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("exclusion {id}")));
    }
    Ok(())
}
