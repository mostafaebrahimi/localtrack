use localtrack_core::classification::{ClassificationRule, RuleField, RuleOperator};
use rusqlite::{params, Connection, Row, Transaction};

use crate::error::{Result, StorageError};

const COLUMNS: &str = "id, name, enabled, priority, target_field, operator, pattern, \
     category_id, project_id, created_at_ms, updated_at_ms";

fn map_row(row: &Row<'_>) -> rusqlite::Result<ClassificationRule> {
    let field: String = row.get(4)?;
    let operator: String = row.get(5)?;
    Ok(ClassificationRule {
        id: row.get(0)?,
        name: row.get(1)?,
        enabled: row.get::<_, i64>(2)? != 0,
        priority: row.get(3)?,
        target_field: RuleField::parse(&field).unwrap_or(RuleField::AppName),
        operator: RuleOperator::parse(&operator).unwrap_or(RuleOperator::Contains),
        pattern: row.get(6)?,
        category_id: row.get(7)?,
        project_id: row.get(8)?,
        created_at_ms: row.get(9)?,
        updated_at_ms: row.get(10)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<ClassificationRule>> {
    let sql =
        format!("SELECT {COLUMNS} FROM classification_rules ORDER BY priority DESC, name ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn get(conn: &Connection, id: &str) -> Result<ClassificationRule> {
    let sql = format!("SELECT {COLUMNS} FROM classification_rules WHERE id = ?1");
    conn.query_row(&sql, [id], map_row).map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => StorageError::NotFound(format!("rule {id}")),
        other => StorageError::Sqlite(other),
    })
}

pub fn upsert(tx: &Transaction<'_>, rule: &ClassificationRule) -> Result<()> {
    rule.validate()?;
    tx.execute(
        "INSERT INTO classification_rules (
            id, name, enabled, priority, target_field, operator, pattern,
            category_id, project_id, created_at_ms, updated_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            enabled = excluded.enabled,
            priority = excluded.priority,
            target_field = excluded.target_field,
            operator = excluded.operator,
            pattern = excluded.pattern,
            category_id = excluded.category_id,
            project_id = excluded.project_id,
            updated_at_ms = excluded.updated_at_ms",
        params![
            rule.id,
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
        ],
    )?;
    Ok(())
}

pub fn delete(tx: &Transaction<'_>, id: &str) -> Result<()> {
    let changed = tx.execute("DELETE FROM classification_rules WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("rule {id}")));
    }
    Ok(())
}
