use crate::project_db::connection::{migrate_catalog, open_connection, DatabaseError};
use crate::project_db::paths::AppPaths;
use libsql_rusqlite::{Connection, OptionalExtension};

#[derive(Debug)]
pub struct ProjectCatalogRow {
    pub project_id: String,
    pub title: String,
    pub lifecycle: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_opened_at: Option<String>,
    pub project_path: String,
}

pub struct CatalogRepository;
impl CatalogRepository {
    pub fn open(paths: &AppPaths, now: &str) -> Result<Connection, DatabaseError> {
        let connection = open_connection(&paths.catalog_db(), false)?;
        migrate_catalog(&connection, now)?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS project_registry_command_receipts (
                idempotency_key TEXT PRIMARY KEY NOT NULL,
                project_id TEXT NOT NULL,
                command_id TEXT NOT NULL,
                command_type TEXT NOT NULL,
                request_digest TEXT NOT NULL,
                response_json TEXT NOT NULL,
                committed_at TEXT NOT NULL
            );",
        )?;
        Ok(connection)
    }

    pub fn register_project(
        connection: &Connection,
        project_id: &str,
        project_path: &str,
        title: &str,
        now: &str,
    ) -> Result<(), DatabaseError> {
        connection.execute("INSERT INTO projects(project_id, project_path, title, lifecycle, created_at, updated_at, last_opened_at, project_schema_version, compatibility_state) VALUES (?1, ?2, ?3, 'ACTIVE', ?4, ?4, ?4, 'm0a.project.v1', 'COMPATIBLE') ON CONFLICT(project_id) DO UPDATE SET project_path=excluded.project_path, title=excluded.title, last_opened_at=excluded.last_opened_at, updated_at=excluded.updated_at", libsql_rusqlite::params![project_id, project_path, title, now])?;
        Ok(())
    }

    pub fn list_projects(connection: &Connection) -> Result<Vec<ProjectCatalogRow>, DatabaseError> {
        let mut statement = connection.prepare(
            "SELECT project_id, title, lifecycle, created_at, updated_at, last_opened_at, project_path FROM projects ORDER BY COALESCE(last_opened_at, updated_at) DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ProjectCatalogRow {
                project_id: row.get(0)?,
                title: row.get(1)?,
                lifecycle: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
                last_opened_at: row.get(5)?,
                project_path: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(DatabaseError::Sqlite)
    }

    pub fn touch_opened(
        connection: &Connection,
        project_id: &str,
        now: &str,
    ) -> Result<(), DatabaseError> {
        connection.execute(
            "UPDATE projects SET last_opened_at = ?1, updated_at = ?1 WHERE project_id = ?2",
            [now, project_id],
        )?;
        Ok(())
    }

    pub fn rename(
        connection: &Connection,
        project_id: &str,
        title: &str,
        now: &str,
    ) -> Result<(), DatabaseError> {
        connection.execute(
            "UPDATE projects SET title = ?1, updated_at = ?2 WHERE project_id = ?3",
            libsql_rusqlite::params![title, now, project_id],
        )?;
        Ok(())
    }

    pub fn set_compatibility(
        connection: &Connection,
        project_id: &str,
        compatibility: &str,
        now: &str,
    ) -> Result<(), DatabaseError> {
        connection.execute(
            "UPDATE projects SET compatibility_state = ?1, updated_at = ?2 WHERE project_id = ?3",
            libsql_rusqlite::params![compatibility, now, project_id],
        )?;
        Ok(())
    }

    pub fn set_lifecycle(
        connection: &Connection,
        project_id: &str,
        lifecycle: &str,
        now: &str,
    ) -> Result<(), DatabaseError> {
        connection.execute(
            "UPDATE projects SET lifecycle = ?1, updated_at = ?2 WHERE project_id = ?3",
            libsql_rusqlite::params![lifecycle, now, project_id],
        )?;
        Ok(())
    }

    pub fn lifecycle(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Option<String>, DatabaseError> {
        let lifecycle: Option<String> = connection
            .query_row(
                "SELECT lifecycle FROM projects WHERE project_id = ?1",
                [project_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(lifecycle)
    }

    pub fn project_path(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Option<String>, DatabaseError> {
        let project_path: Option<String> = connection
            .query_row(
                "SELECT project_path FROM projects WHERE project_id = ?1",
                [project_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(project_path)
    }

    pub fn set_project_path(
        connection: &Connection,
        project_id: &str,
        project_path: &str,
        now: &str,
    ) -> Result<(), DatabaseError> {
        connection.execute(
            "UPDATE projects SET project_path = ?1, updated_at = ?2 WHERE project_id = ?3",
            libsql_rusqlite::params![project_path, now, project_id],
        )?;
        Ok(())
    }

    pub fn registry_command_receipt(
        connection: &Connection,
        idempotency_key: &str,
    ) -> Result<Option<(String, String)>, DatabaseError> {
        connection
            .query_row(
                "SELECT request_digest, response_json FROM project_registry_command_receipts WHERE idempotency_key = ?1",
                [idempotency_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(DatabaseError::Sqlite)
    }

    pub fn insert_registry_command_receipt(
        connection: &Connection,
        idempotency_key: &str,
        project_id: &str,
        command_id: &str,
        command_type: &str,
        request_digest: &str,
        response_json: &str,
        committed_at: &str,
    ) -> Result<(), DatabaseError> {
        connection.execute(
            "INSERT INTO project_registry_command_receipts(idempotency_key, project_id, command_id, command_type, request_digest, response_json, committed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            libsql_rusqlite::params![idempotency_key, project_id, command_id, command_type, request_digest, response_json, committed_at],
        )?;
        Ok(())
    }
}
