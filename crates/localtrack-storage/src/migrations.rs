//! Schema migrations (spec §151).
//!
//! Migrations are embedded in the binary and applied in order inside a
//! transaction. A shipped migration is never edited; new schema changes always
//! get a new version.

use rusqlite::{Connection, Transaction};

use crate::error::{Result, StorageError};

pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "001_initial",
        sql: include_str!("../migrations/001_initial.sql"),
    },
    Migration {
        version: 2,
        name: "002_projects",
        sql: include_str!("../migrations/002_projects.sql"),
    },
    Migration {
        version: 3,
        name: "003_exclusions",
        sql: include_str!("../migrations/003_exclusions.sql"),
    },
    Migration {
        version: 4,
        name: "004_managed",
        sql: include_str!("../migrations/004_managed.sql"),
    },
    Migration {
        version: 5,
        name: "005_session_sync",
        sql: include_str!("../migrations/005_session_sync.sql"),
    },
    Migration {
        version: 6,
        name: "006_uptime",
        sql: include_str!("../migrations/006_uptime.sql"),
    },
    Migration {
        version: 7,
        name: "007_status_indexes",
        sql: include_str!("../migrations/007_status_indexes.sql"),
    },
];

pub const LATEST_VERSION: i64 = 7;

/// Default categories seeded on first run (spec §56, §155).
pub const DEFAULT_CATEGORIES: &[&str] = &[
    "Development",
    "Communication",
    "Research",
    "Meetings",
    "Design",
    "Administration",
    "Entertainment",
    "Personal",
    "Other",
    "Uncategorized",
];

/// The category used when nothing matches.
pub const UNCATEGORIZED: &str = "Uncategorized";

pub fn current_version(conn: &Connection) -> Result<i64> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at_ms INTEGER NOT NULL
        );",
    )?;
    let version: Option<i64> =
        conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })?;
    Ok(version.unwrap_or(0))
}

/// Apply every pending migration. Safe to call on every start.
pub fn migrate(conn: &mut Connection, now_ms: i64) -> Result<i64> {
    let mut version = current_version(conn)?;

    for migration in MIGRATIONS {
        if migration.version <= version {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)
            .map_err(|e| StorageError::Migration {
                version: migration.version,
                message: format!("{}: {e}", migration.name),
            })?;
        tx.execute(
            "INSERT INTO schema_migrations(version, applied_at_ms) VALUES (?1, ?2)",
            rusqlite::params![migration.version, now_ms],
        )?;
        if migration.version == 1 {
            seed_defaults(&tx, now_ms)?;
        }
        tx.commit()?;
        version = migration.version;
        tracing::info!(
            version = migration.version,
            name = migration.name,
            "migration applied"
        );
    }

    Ok(version)
}

fn seed_defaults(tx: &Transaction<'_>, now_ms: i64) -> Result<()> {
    for name in DEFAULT_CATEGORIES {
        tx.execute(
            "INSERT OR IGNORE INTO categories(id, name, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?3)",
            rusqlite::params![uuid::Uuid::new_v4().to_string(), name, now_ms],
        )?;
    }
    Ok(())
}

/// Verify a database file looks like a LocalTrack database (spec §126).
pub fn verify_schema(conn: &Connection) -> Result<i64> {
    let required = [
        "schema_migrations",
        "settings",
        "work_sessions",
        "work_breaks",
        "categories",
        "activity_segments",
        "classification_rules",
    ];
    for table in required {
        let found: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |row| row.get(0),
        )?;
        if found == 0 {
            return Err(StorageError::InvalidDatabase(format!(
                "missing table {table}"
            )));
        }
    }
    let version = current_version(conn)?;
    if version > LATEST_VERSION {
        return Err(StorageError::InvalidDatabase(format!(
            "database schema version {version} is newer than this build ({LATEST_VERSION})"
        )));
    }
    Ok(version)
}
