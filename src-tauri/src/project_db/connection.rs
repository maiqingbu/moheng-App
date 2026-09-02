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
const PROJECT_LONGTERM_MIGRATION: &str = include_str!("../../migrations/0004_project_longterm.sql");
const M0C_COMMIT_DELTA_MIGRATION: &str = include_str!("../../migrations/0005_m0c_commit_delta.sql");
const M0E_OPS_MIGRATION: &str = include_str!("../../migrations/0006_m0e_ops.sql");
const B05_NARRATIVE_MIGRATION: &str =
    include_str!("../../migrations/0007_b05_narrative_foundation.sql");
const H01_SUPPORT_MIGRATION: &str =
    include_str!("../../migrations/0008_h01_support_checkpoint.sql");
const H03_REPAIR_MIGRATION: &str = include_str!("../../migrations/0009_h03_repair_workflow.sql");
const B09_H04_VOLUME_LIFECYCLE_MIGRATION: &str =
    include_str!("../../migrations/0010_b09_h04_volume_lifecycle.sql");
const B12_CHANGE_RETCON_MIGRATION: &str =
    include_str!("../../migrations/0011_b12_change_retcon_runtime.sql");

pub fn open_connection(path: &Path, read_only: bool) -> Result<Connection, DatabaseError> {
    let existed = path.is_file();
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
        // WAL is persistent per database. Reissuing the mode switch on every
        // writer open makes a large 3m-edge project pay the transition cost on
        // each B12 preflight. Only migrate an existing database when it is not
        // already in WAL mode; new databases receive the mode once here.
        let journal_mode: String =
            connection.query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        if !existed || !journal_mode.eq_ignore_ascii_case("wal") {
            connection.execute_batch("PRAGMA journal_mode = WAL;")?;
        }
        connection.execute_batch(
            "PRAGMA synchronous = FULL; PRAGMA wal_autocheckpoint = 1000; PRAGMA mmap_size = 268435456;",
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
    // A project is up to date only when every numbered project migration has
    // been recorded. The SQL is replay-safe, but the history is the authority
    // used by ProjectManager to decide whether an upgrade backup is required.
    let applied_v3: Option<i64> = connection
        .query_row(
            "SELECT version FROM migration_history WHERE version = 3",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let applied_v4: Option<i64> = connection
        .query_row(
            "SELECT version FROM migration_history WHERE version = 4",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let applied_v5: Option<i64> = connection
        .query_row(
            "SELECT version FROM migration_history WHERE version = 5",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let applied_v6: Option<i64> = connection
        .query_row(
            "SELECT version FROM migration_history WHERE version = 6",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let applied_v7: Option<i64> = connection
        .query_row(
            "SELECT version FROM migration_history WHERE version = 7",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let applied_v8: Option<i64> = connection
        .query_row(
            "SELECT version FROM migration_history WHERE version = 8",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let applied_v10: Option<i64> = connection
        .query_row(
            "SELECT version FROM migration_history WHERE version = 10",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let applied_v11: Option<i64> = connection
        .query_row(
            "SELECT version FROM migration_history WHERE version = 11",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(applied.is_none()
        || applied_v3.is_none()
        || applied_v4.is_none()
        || applied_v5.is_none()
        || applied_v6.is_none()
        || applied_v7.is_none()
        || applied_v8.is_none()
        || applied_v10.is_none()
        || applied_v11.is_none())
}

fn migration_recorded(connection: &Connection, version: i64) -> Result<bool, DatabaseError> {
    let history_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='migration_history')",
        [],
        |row| row.get(0),
    )?;
    if !history_exists {
        return Ok(false);
    }
    let recorded: Option<i64> = connection
        .query_row(
            "SELECT version FROM migration_history WHERE version=?1",
            [version],
            |row| row.get(0),
        )
        .optional()?;
    Ok(recorded.is_some())
}

fn apply_numbered_migration(
    connection: &Connection,
    version: i64,
    sql: &str,
    now: &str,
) -> Result<bool, DatabaseError> {
    if migration_recorded(connection, version)? {
        return Ok(false);
    }
    connection.execute_batch(sql)?;
    connection.execute(
        "INSERT OR IGNORE INTO migration_history(version, applied_at) VALUES (?1, ?2)",
        libsql_rusqlite::params![version, now],
    )?;
    Ok(true)
}

pub fn migrate_project(
    connection: &Connection,
    project_id: &str,
    title: &str,
    now: &str,
) -> Result<(), DatabaseError> {
    // Opening a completed project must not replay every historical CREATE
    // INDEX/CREATE TABLE statement. SQLite's `IF NOT EXISTS` is safe but a
    // replayed index declaration on a 3m-edge project still spends tens of
    // seconds on the hot open path. Migration history is the authority for the
    // numbered migrations; the two compatibility additions below are checked
    // independently and only created when absent.
    let needs_full_migration = project_needs_migration(connection)?;
    let h03_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='repair_plans')",
        [],
        |row| row.get(0),
    )?;
    let b04_index_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_b04_dependency_project_source_edge')",
        [],
        |row| row.get(0),
    )?;
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        if needs_full_migration {
            apply_numbered_migration(connection, 2, PROJECT_MIGRATION, now)?;
            apply_numbered_migration(connection, 3, PROJECT_COMMAND_MIGRATION, now)?;
            apply_numbered_migration(connection, 4, PROJECT_LONGTERM_MIGRATION, now)?;
            apply_numbered_migration(connection, 5, M0C_COMMIT_DELTA_MIGRATION, now)?;
            apply_numbered_migration(connection, 6, M0E_OPS_MIGRATION, now)?;
            apply_numbered_migration(connection, 7, B05_NARRATIVE_MIGRATION, now)?;
            apply_numbered_migration(connection, 8, H01_SUPPORT_MIGRATION, now)?;
            apply_numbered_migration(connection, 10, B09_H04_VOLUME_LIFECYCLE_MIGRATION, now)?;
        }
        // H03 is deliberately support-only and replay-safe. It is not added to
        // numbered migration history; only a missing support table needs the
        // compatibility SQL on an already-migrated project.
        if !h03_exists {
            connection.execute_batch(H03_REPAIR_MIGRATION)?;
        }
        // Additive B04 query-coverage index. Do not replay its declaration on
        // every open of a large dependency graph.
        if !b04_index_exists {
            connection.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_b04_dependency_project_source_edge ON dependency_edges(project_id, source_ref, edge_id);",
            )?;
        }
        apply_numbered_migration(connection, 11, B12_CHANGE_RETCON_MIGRATION, now)?;
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
