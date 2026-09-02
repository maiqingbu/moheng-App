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
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL; PRAGMA wal_autocheckpoint = 1000; PRAGMA mmap_size = 268435456;",
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
    Ok(applied.is_none()
        || applied_v3.is_none()
        || applied_v4.is_none()
        || applied_v5.is_none()
        || applied_v6.is_none()
        || applied_v7.is_none()
        || applied_v8.is_none()
        || applied_v10.is_none())
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
    let b12_insert_trigger_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='trigger' AND name='b12_dependency_summary_insert')",
        [],
        |row| row.get(0),
    )?;
    let b12_update_trigger_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='trigger' AND name='b12_dependency_summary_update')",
        [],
        |row| row.get(0),
    )?;
    let b12_delete_trigger_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='trigger' AND name='b12_dependency_summary_delete')",
        [],
        |row| row.get(0),
    )?;
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        let (needs_b05_migration, needs_h01_migration, needs_b09_migration) =
            if needs_full_migration {
                connection.execute_batch(PROJECT_MIGRATION)?;
                connection.execute_batch(PROJECT_COMMAND_MIGRATION)?;
                connection.execute_batch(PROJECT_LONGTERM_MIGRATION)?;
                connection.execute_batch(M0C_COMMIT_DELTA_MIGRATION)?;
                connection.execute_batch(M0E_OPS_MIGRATION)?;
                let needs_b05_migration: bool = connection.query_row(
                    "SELECT NOT EXISTS(SELECT 1 FROM migration_history WHERE version=7)",
                    [],
                    |row| row.get(0),
                )?;
                if needs_b05_migration {
                    connection.execute_batch(B05_NARRATIVE_MIGRATION)?;
                }
                let needs_h01_migration: bool = connection.query_row(
                    "SELECT NOT EXISTS(SELECT 1 FROM migration_history WHERE version=8)",
                    [],
                    |row| row.get(0),
                )?;
                if needs_h01_migration {
                    connection.execute_batch(H01_SUPPORT_MIGRATION)?;
                }
                let needs_b09_migration: bool = connection.query_row(
                    "SELECT NOT EXISTS(SELECT 1 FROM migration_history WHERE version=10)",
                    [],
                    |row| row.get(0),
                )?;
                if needs_b09_migration {
                    connection.execute_batch(B09_H04_VOLUME_LIFECYCLE_MIGRATION)?;
                }
                (
                    needs_b05_migration,
                    needs_h01_migration,
                    needs_b09_migration,
                )
            } else {
                (false, false, false)
            };
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
        if !b12_insert_trigger_exists {
            connection.execute_batch(
                "CREATE TRIGGER IF NOT EXISTS b12_dependency_summary_insert AFTER INSERT ON dependency_edges BEGIN UPDATE project_settings SET setting_value=json_set(setting_value, '$.mutationEpoch', COALESCE(json_extract(setting_value, '$.mutationEpoch'), 0) + 1) WHERE project_id=NEW.project_id AND setting_key='b12.dependencySummary.global'; END;",
            )?;
        }
        if !b12_update_trigger_exists {
            connection.execute_batch(
                "CREATE TRIGGER IF NOT EXISTS b12_dependency_summary_update AFTER UPDATE OF source_ref, target_ref, impact_class, source_hash ON dependency_edges BEGIN UPDATE project_settings SET setting_value=json_set(setting_value, '$.mutationEpoch', COALESCE(json_extract(setting_value, '$.mutationEpoch'), 0) + 1) WHERE project_id=NEW.project_id AND setting_key='b12.dependencySummary.global'; END;",
            )?;
        }
        if !b12_delete_trigger_exists {
            connection.execute_batch(
                "CREATE TRIGGER IF NOT EXISTS b12_dependency_summary_delete AFTER DELETE ON dependency_edges BEGIN UPDATE project_settings SET setting_value=json_set(setting_value, '$.mutationEpoch', COALESCE(json_extract(setting_value, '$.mutationEpoch'), 0) + 1) WHERE project_id=OLD.project_id AND setting_key='b12.dependencySummary.global'; END;",
            )?;
        }
        connection.execute(
            "INSERT OR IGNORE INTO migration_history(version, applied_at) VALUES (2, ?1)",
            [now],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO migration_history(version, applied_at) VALUES (3, ?1)",
            [now],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO migration_history(version, applied_at) VALUES (4, ?1)",
            [now],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO migration_history(version, applied_at) VALUES (5, ?1)",
            [now],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO migration_history(version, applied_at) VALUES (6, ?1)",
            [now],
        )?;
        if needs_b05_migration {
            connection.execute(
                "INSERT OR IGNORE INTO migration_history(version, applied_at) VALUES (7, ?1)",
                [now],
            )?;
        }
        if needs_h01_migration {
            connection.execute(
                "INSERT OR IGNORE INTO migration_history(version, applied_at) VALUES (8, ?1)",
                [now],
            )?;
        }
        if needs_b09_migration {
            connection.execute(
                "INSERT OR IGNORE INTO migration_history(version, applied_at) VALUES (10, ?1)",
                [now],
            )?;
        }
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
