use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};

use crate::error::{Result, StorageError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

pub fn list(conn: &Connection) -> Result<Vec<Category>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, created_at_ms, updated_at_ms FROM categories ORDER BY name ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Category {
            id: row.get(0)?,
            name: row.get(1)?,
            created_at_ms: row.get(2)?,
            updated_at_ms: row.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn find_by_name(conn: &Connection, name: &str) -> Result<Option<Category>> {
    let mut stmt = conn
        .prepare("SELECT id, name, created_at_ms, updated_at_ms FROM categories WHERE name = ?1")?;
    let mut rows = stmt.query([name])?;
    match rows.next()? {
        Some(row) => Ok(Some(Category {
            id: row.get(0)?,
            name: row.get(1)?,
            created_at_ms: row.get(2)?,
            updated_at_ms: row.get(3)?,
        })),
        None => Ok(None),
    }
}

pub fn create(tx: &Transaction<'_>, id: &str, name: &str, now_ms: i64) -> Result<Category> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StorageError::Invalid(
            "category name must not be empty".into(),
        ));
    }
    tx.execute(
        "INSERT INTO categories(id, name, created_at_ms, updated_at_ms) VALUES (?1, ?2, ?3, ?3)",
        params![id, name, now_ms],
    )
    .map_err(|e| duplicate_name(e, name))?;
    Ok(Category {
        id: id.to_string(),
        name: name.to_string(),
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
    })
}

pub fn rename(tx: &Transaction<'_>, id: &str, name: &str, now_ms: i64) -> Result<()> {
    let changed = tx
        .execute(
            "UPDATE categories SET name = ?2, updated_at_ms = ?3 WHERE id = ?1",
            params![id, name.trim(), now_ms],
        )
        .map_err(|e| duplicate_name(e, name))?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("category {id}")));
    }
    Ok(())
}

pub fn delete(tx: &Transaction<'_>, id: &str) -> Result<()> {
    let changed = tx.execute("DELETE FROM categories WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("category {id}")));
    }
    Ok(())
}

fn duplicate_name(err: rusqlite::Error, name: &str) -> StorageError {
    match &err {
        rusqlite::Error::SqliteFailure(e, _)
            if e.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            StorageError::Conflict(format!("a category named {name} already exists"))
        }
        _ => StorageError::Sqlite(err),
    }
}
