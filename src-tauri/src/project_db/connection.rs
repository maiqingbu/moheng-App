use libsql_rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::fs;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("integrity check failed: {0}")]
    Integrity(String),
}

const CATALOG_MIGRATION: &str = include_str!("../../migrations/0001_catalog.sql");
const PROJECT_MIGRATION: &str = include_str!("../../migrations/0002_project_authority.sql");
const PROJECT_COMMAND_MIGRATION: &str = include_str!("../../migrations/0003_project_command.sql");

pub fn open_connection(path: &Path, read_only: bool) -> Result<Connection, DatabaseError> {
    if !read_only {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    };
    let connection = Connection::open_with_flags(path, flags)?;
    connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
    if read_only {
        connection.execute_batch("PRAGMA query_only = ON;")?;
    } else {
        connection.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL; PRAGMA wal_autocheckpoint = 1000;",
        )?;
    }
    Ok(connection)
}

pub fn quick_check(connection: &Connection) -> Result<(), DatabaseError> {
    let result: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if result != "ok" {
        return Err(DatabaseError::Integrity(result));
    }
    Ok(())
}

pub fn migrate_catalog(connection: &Connection, now: &str) -> Result<(), DatabaseError> {
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        connection.execute_batch(CATALOG_MIGRATION)?;
        connection.execute("INSERT OR IGNORE INTO catalog_meta(key, value) VALUES ('schema_version', 'm0a.catalog.v1')", [])?;
        connection.execute(
            "INSERT OR IGNORE INTO catalog_migration_history(version, applied_at) VALUES (1, ?1)",
            [now],
        )?;
        Ok::<_, DatabaseError>(())
    })();
    match result {
        Ok(()) => {
            connection.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

pub fn project_needs_migration(connection: &Connection) -> Result<bool, DatabaseError> {
    let table_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'migration_history')",
        [],
        |row| row.get(0),
    )?;
    if !table_exists {
        return Ok(true);
    }
    let applied: Option<i64> = connection
        .query_row(
            "SELECT version FROM migration_history WHERE version = 2",
            [],
            |row| row.get(0),
        )
        .optional()?;
    // A project is up to date only when both the authority migration and the
    // project-command migration (v3) have been applied.
    let applied_v3: Option<i64> = connection
        .query_row(
            "SELECT version FROM migration_history WHERE version = 3",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(applied.is_none() || applied_v3.is_none())
}

pub fn migrate_project(
    connection: &Connection,
    project_id: &str,
    title: &str,
    now: &str,
) -> Result<(), DatabaseError> {
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        connection.execute_batch(PROJECT_MIGRATION)?;
        connection.execute_batch(PROJECT_COMMAND_MIGRATION)?;
        connection.execute(
            "INSERT OR IGNORE INTO migration_history(version, applied_at) VALUES (2, ?1)",
            [now],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO migration_history(version, applied_at) VALUES (3, ?1)",
            [now],
        )?;
        connection.execute("INSERT OR IGNORE INTO project_meta(project_id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)", [project_id, title, now])?;
        Ok::<_, DatabaseError>(())
    })();
    match result {
        Ok(()) => {
            connection.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}
