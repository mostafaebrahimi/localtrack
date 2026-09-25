use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{backup::Backup, Connection, OpenFlags, Transaction};
use serde::{Deserialize, Serialize};

use crate::error::{Result, StorageError};
use crate::migrations;

/// Retry budget for `SQLITE_BUSY` beyond the busy timeout (spec §146).
const BUSY_RETRIES: u32 = 3;
const BUSY_RETRY_DELAY_MS: u64 = 50;

/// A SQLite database handle.
///
/// The desktop application and the Chrome native host are separate processes
/// that both open this file, which is why WAL mode and a busy timeout are
/// mandatory (spec §5).
pub struct Database {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.path)
            .finish()
    }
}

impl Database {
    /// Open (creating if needed) and migrate a database file.
    pub fn open<P: AsRef<Path>>(path: P, now_ms: i64) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        apply_pragmas(&conn)?;
        migrations::migrate(&mut conn, now_ms)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path,
        })
    }

    /// An in-memory database, used by tests.
    pub fn open_in_memory(now_ms: i64) -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        apply_pragmas(&conn)?;
        migrations::migrate(&mut conn, now_ms)?;
        Ok(Self {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Run a read-only closure against the connection.
    pub fn read<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let guard = self
            .conn
            .lock()
            .map_err(|_| StorageError::Conflict("database mutex poisoned".to_string()))?;
        f(&guard)
    }

    /// Run a closure inside a transaction, retrying briefly when the database
    /// is busy. Never spins continuously (spec §146).
    pub fn write<T, F>(&self, f: F) -> Result<T>
    where
        F: Fn(&Transaction<'_>) -> Result<T>,
    {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| StorageError::Conflict("database mutex poisoned".to_string()))?;

        let mut attempt = 0;
        loop {
            let tx = guard.transaction()?;
            match f(&tx) {
                Ok(value) => {
                    tx.commit()?;
                    return Ok(value);
                }
                Err(err) => {
                    drop(tx);
                    if attempt < BUSY_RETRIES && is_busy(&err) {
                        attempt += 1;
                        std::thread::sleep(std::time::Duration::from_millis(
                            BUSY_RETRY_DELAY_MS * attempt as u64,
                        ));
                        tracing::warn!(attempt, "database busy, retrying write");
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }

    /// Truncate the WAL so the main database file is complete (spec §124).
    pub fn checkpoint(&self) -> Result<()> {
        self.read(|conn| {
            conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
            Ok(())
        })
    }

    /// VACUUM. Never run on every startup (spec §124).
    pub fn vacuum(&self) -> Result<()> {
        self.read(|conn| {
            conn.execute_batch("VACUUM")?;
            Ok(())
        })
    }

    pub fn integrity_check(&self) -> Result<bool> {
        self.read(|conn| {
            let result: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
            Ok(result == "ok")
        })
    }

    pub fn health(&self) -> Result<DatabaseHealth> {
        let size_bytes = std::fs::metadata(&self.path)
            .map(|m| m.len() as i64)
            .unwrap_or(0);
        self.read(|conn| {
            let schema_version = migrations::current_version(conn)?;
            let segment_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM activity_segments", [], |r| r.get(0))?;
            let session_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM work_sessions", [], |r| r.get(0))?;
            let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
            let oldest_segment_ms: Option<i64> = conn.query_row(
                "SELECT MIN(started_at_ms) FROM activity_segments",
                [],
                |r| r.get(0),
            )?;
            let newest_segment_ms: Option<i64> =
                conn.query_row("SELECT MAX(ended_at_ms) FROM activity_segments", [], |r| {
                    r.get(0)
                })?;
            Ok(DatabaseHealth {
                path: self.path.display().to_string(),
                size_bytes,
                schema_version,
                segment_count,
                session_count,
                journal_mode,
                oldest_segment_ms,
                newest_segment_ms,
            })
        })
    }

    /// SQLite-safe online backup (spec §125): never a raw file copy of a live
    /// WAL database.
    pub fn backup_to<P: AsRef<Path>>(&self, destination: P) -> Result<PathBuf> {
        let destination = destination.as_ref().to_path_buf();
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.read(|conn| {
            let mut dest = Connection::open(&destination)?;
            {
                let backup = Backup::new(conn, &mut dest)?;
                backup.run_to_completion(64, std::time::Duration::from_millis(50), None)?;
            }
            dest.close().map_err(|(_, e)| StorageError::Sqlite(e))?;
            Ok(())
        })?;
        Ok(destination)
    }

    /// Validate a candidate database file before it may replace the live one
    /// (spec §126). The current database is never destroyed first.
    pub fn validate_backup_file<P: AsRef<Path>>(path: P) -> Result<i64> {
        let conn = Connection::open_with_flags(
            path.as_ref(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let integrity: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if integrity != "ok" {
            return Err(StorageError::InvalidDatabase(format!(
                "integrity check failed: {integrity}"
            )));
        }
        migrations::verify_schema(&conn)
    }

    /// Replace the contents of this database with a validated file.
    ///
    /// The current contents are copied aside first; if anything fails the
    /// caller still has that safety copy.
    pub fn restore_from<P: AsRef<Path>>(&self, source: P, safety_copy: P) -> Result<()> {
        Self::validate_backup_file(source.as_ref())?;
        self.backup_to(safety_copy.as_ref())?;

        let source_conn = Connection::open_with_flags(
            source.as_ref(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| StorageError::Conflict("database mutex poisoned".to_string()))?;
        {
            let backup = Backup::new(&source_conn, &mut guard)?;
            backup.run_to_completion(64, std::time::Duration::from_millis(50), None)?;
        }
        apply_pragmas(&guard)?;
        Ok(())
    }
}

fn is_busy(err: &StorageError) -> bool {
    matches!(
        err,
        StorageError::Sqlite(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::DatabaseBusy
                || e.code == rusqlite::ErrorCode::DatabaseLocked
    )
}

/// PRAGMAs required by the spec (§5).
pub fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

/// Diagnostics about the database (spec §109).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseHealth {
    pub path: String,
    pub size_bytes: i64,
    pub schema_version: i64,
    pub segment_count: i64,
    pub session_count: i64,
    pub journal_mode: String,
    pub oldest_segment_ms: Option<i64>,
    pub newest_segment_ms: Option<i64>,
}
