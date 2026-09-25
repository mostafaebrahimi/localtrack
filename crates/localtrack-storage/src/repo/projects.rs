use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};

use crate::error::{Result, StorageError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub archived: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

pub fn list(conn: &Connection, include_archived: bool) -> Result<Vec<Project>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, archived, created_at_ms, updated_at_ms FROM projects
         WHERE (?1 = 1 OR archived = 0)
         ORDER BY archived ASC, name ASC",
    )?;
    let rows = stmt.query_map([i64::from(include_archived)], |row| {
        Ok(Project {
            id: row.get(0)?,
            name: row.get(1)?,
            archived: row.get::<_, i64>(2)? != 0,
            created_at_ms: row.get(3)?,
            updated_at_ms: row.get(4)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn create(tx: &Transaction<'_>, id: &str, name: &str, now_ms: i64) -> Result<Project> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StorageError::Invalid(
            "project name must not be empty".into(),
        ));
    }
    tx.execute(
        "INSERT INTO projects(id, name, archived, created_at_ms, updated_at_ms)
         VALUES (?1, ?2, 0, ?3, ?3)",
        params![id, name, now_ms],
    )
    .map_err(|e| match &e {
        rusqlite::Error::SqliteFailure(err, _)
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            StorageError::Conflict(format!("a project named {name} already exists"))
        }
        _ => StorageError::Sqlite(e),
    })?;
    Ok(Project {
        id: id.to_string(),
        name: name.to_string(),
        archived: false,
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
    })
}

pub fn update(
    tx: &Transaction<'_>,
    id: &str,
    name: &str,
    archived: bool,
    now_ms: i64,
) -> Result<()> {
    let changed = tx.execute(
        "UPDATE projects SET name = ?2, archived = ?3, updated_at_ms = ?4 WHERE id = ?1",
        params![id, name.trim(), i64::from(archived), now_ms],
    )?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("project {id}")));
    }
    Ok(())
}

pub fn delete(tx: &Transaction<'_>, id: &str) -> Result<()> {
    let changed = tx.execute("DELETE FROM projects WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("project {id}")));
    }
    Ok(())
}
