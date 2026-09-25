use std::collections::BTreeMap;

use localtrack_core::settings::Settings;
use rusqlite::{params, Connection, Transaction};

use crate::error::Result;

pub fn get_all(conn: &Connection) -> Result<BTreeMap<String, serde_json::Value>> {
    let mut stmt = conn.prepare("SELECT key, value_json FROM settings")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = BTreeMap::new();
    for row in rows {
        let (key, raw) = row?;
        match serde_json::from_str(&raw) {
            Ok(value) => {
                out.insert(key, value);
            }
            Err(err) => tracing::warn!(key = %key, error = %err, "ignoring unparseable setting"),
        }
    }
    Ok(out)
}

pub fn load(conn: &Connection) -> Result<Settings> {
    Ok(Settings::from_map(&get_all(conn)?))
}

pub fn get(conn: &Connection, key: &str) -> Result<Option<serde_json::Value>> {
    let mut stmt = conn.prepare("SELECT value_json FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query([key])?;
    match rows.next()? {
        Some(row) => {
            let raw: String = row.get(0)?;
            Ok(serde_json::from_str(&raw).ok())
        }
        None => Ok(None),
    }
}

pub fn set(tx: &Transaction<'_>, key: &str, value: &serde_json::Value, now_ms: i64) -> Result<()> {
    tx.execute(
        "INSERT INTO settings(key, value_json, updated_at_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
                                        updated_at_ms = excluded.updated_at_ms",
        params![key, serde_json::to_string(value)?, now_ms],
    )?;
    Ok(())
}

pub fn save(tx: &Transaction<'_>, settings: &Settings, now_ms: i64) -> Result<()> {
    for (key, value) in settings.to_map() {
        set(tx, &key, &value, now_ms)?;
    }
    Ok(())
}
