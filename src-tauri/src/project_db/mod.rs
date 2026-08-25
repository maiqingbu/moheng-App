pub mod connection;
pub mod paths;
pub mod read_pool;

pub use read_pool::ReadPool;

use crate::catalog::CatalogRepository;
pub use connection::DatabaseError;
use connection::{migrate_project, open_connection, project_needs_migration, quick_check};
use libsql_rusqlite::Connection;
use paths::{AppPaths, PathError};
use std::collections::HashSet;
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct ProjectHandle {
    connection: Connection,
    project_id: String,
    writers: Arc<Mutex<HashSet<String>>>,
}

impl Deref for ProjectHandle {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.connection
    }
}

impl Drop for ProjectHandle {
    fn drop(&mut self) {
        if let Ok(mut writers) = self.writers.lock() {
            writers.remove(&self.project_id);
        }
    }
}

#[derive(Clone)]
pub struct ProjectManager {
    paths: AppPaths,
    writers: Arc<Mutex<HashSet<String>>>,
    read_pool: Arc<Mutex<read_pool::ReadPool>>,
}

impl ProjectManager {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            writers: Arc::new(Mutex::new(HashSet::new())),
            read_pool: Arc::new(Mutex::new(read_pool::ReadPool::new(16, 4))),
        }
    }

    fn acquire_writer(&self, project_id: &str) -> Result<(), ManagerError> {
        let mut writers = self
            .writers
            .lock()
            .map_err(|_| ManagerError::WriterLockPoisoned)?;
        if !writers.insert(project_id.to_string()) {
            return Err(ManagerError::AlreadyOpen);
        }
        Ok(())
    }

    fn release_writer(&self, project_id: &str) {
        if let Ok(mut writers) = self.writers.lock() {
            writers.remove(project_id);
        }
    }

    pub fn create(
        &self,
        project_id: &str,
        title: &str,
        now: &str,
    ) -> Result<ProjectHandle, ManagerError> {
        self.acquire_writer(project_id)?;
        let result = (|| {
            let directory = self.paths.project_dir(project_id)?;
            let database = directory.join("project.db");
            let connection = open_connection(&database, false)?;
            migrate_project(&connection, project_id, title, now)?;
            quick_check(&connection)?;
            let catalog = CatalogRepository::open(&self.paths, now)?;
            CatalogRepository::register_project(
                &catalog,
                project_id,
                &directory.to_string_lossy(),
                title,
                now,
            )?;
            Ok(ProjectHandle {
                connection,
                project_id: project_id.to_string(),
                writers: Arc::clone(&self.writers),
            })
        })();
        if result.is_err() {
            self.release_writer(project_id);
        }
        result
    }

    pub fn open(
        &self,
        project_id: &str,
        title: &str,
        now: &str,
    ) -> Result<ProjectHandle, ManagerError> {
        self.acquire_writer(project_id)?;
        let result = (|| {
            let directory = self.paths.project_dir(project_id)?;
            let database = directory.join("project.db");
            if !database.is_file() {
                return Err(ManagerError::ProjectNotFound);
            }
            let connection = open_connection(&database, false)?;
            quick_check(&connection)?;
            if project_needs_migration(&connection)? {
                connection.execute_batch("PRAGMA wal_checkpoint(FULL)")?;
                let backups = directory.join("backups");
                std::fs::create_dir_all(&backups)?;
                let safe_now: String = now
                    .chars()
                    .map(|character| {
                        if character.is_ascii_alphanumeric() {
                            character
                        } else {
                            '-'
                        }
                    })
                    .collect();
                std::fs::copy(&database, backups.join(format!("migration-{safe_now}.db")))?;
            }
            if let Err(error) = migrate_project(&connection, project_id, title, now) {
                if let Ok(catalog) = CatalogRepository::open(&self.paths, now) {
                    let _ =
                        CatalogRepository::set_compatibility(&catalog, project_id, "RECOVERY", now);
                }
                return Err(error.into());
            }
            let catalog = CatalogRepository::open(&self.paths, now)?;
            CatalogRepository::touch_opened(&catalog, project_id, now)?;
            CatalogRepository::set_compatibility(&catalog, project_id, "COMPATIBLE", now)?;
            Ok(ProjectHandle {
                connection,
                project_id: project_id.to_string(),
                writers: Arc::clone(&self.writers),
            })
        })();
        if result.is_err() {
            self.release_writer(project_id);
        }
        result
    }

    pub fn open_read(&self, project_id: &str) -> Result<Connection, ManagerError> {
        let database = self.paths.project_dir(project_id)?.join("project.db");
        if !database.is_file() {
            return Err(ManagerError::ProjectNotFound);
        }
        let connection = open_connection(&database, true)?;
        quick_check(&connection)?;
        Ok(connection)
    }

    /// Returns a bounded, reusable read-only connection for `project_id`.
    ///
    /// The connection is borrowed from the shared [`ReadPool`]; callers should
    /// release it back via [`ProjectHandle::read_and_release`] or
    /// [`ProjectManager::release_read`] so it can be reused for later reads.
    pub fn read_connection(&self, project_id: &str) -> Result<Connection, ManagerError> {
        let database = self.paths.project_dir(project_id)?.join("project.db");
        if !database.is_file() {
            return Err(ManagerError::ProjectNotFound);
        }
        let mut pool = self
            .read_pool
            .lock()
            .map_err(|_| ManagerError::WriterLockPoisoned)?;
        let connection = pool.acquire(project_id, &database)?;
        Ok(connection)
    }

    /// Releases a pooled read connection back to the project's idle bucket.
    pub fn release_read(&self, project_id: &str, connection: Connection) {
        if let Ok(mut pool) = self.read_pool.lock() {
            pool.release(project_id, connection);
        }
    }

    pub fn overview(&self, project_id: &str) -> Result<(String, i64, String), ManagerError> {
        let connection = self.open_read(project_id)?;
        connection
            .query_row(
                "SELECT title, revision, updated_at FROM project_meta WHERE project_id = ?1",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(ManagerError::Sqlite)
    }

    pub fn rename(&self, project_id: &str, title: &str, now: &str) -> Result<(), ManagerError> {
        if title.trim().is_empty() {
            return Err(ManagerError::InvalidTitle);
        }
        let handle = self.open(project_id, title, now)?;
        handle.execute(
            "UPDATE project_meta SET title = ?1, revision = revision + 1, updated_at = ?2 WHERE project_id = ?3",
            libsql_rusqlite::params![title, now, project_id],
        )?;
        let catalog = CatalogRepository::open(&self.paths, now)?;
        CatalogRepository::rename(&catalog, project_id, title, now)?;
        Ok(())
    }

    pub fn set_lifecycle(
        &self,
        project_id: &str,
        lifecycle: &str,
        now: &str,
    ) -> Result<(), ManagerError> {
        if !matches!(lifecycle, "ACTIVE" | "ARCHIVED" | "DELETING") {
            return Err(ManagerError::InvalidLifecycle);
        }
        let catalog = CatalogRepository::open(&self.paths, now)?;
        CatalogRepository::set_lifecycle(&catalog, project_id, lifecycle, now)?;
        Ok(())
    }

    /// Copies an existing project's authoritative data to a fresh project id.
    ///
    /// - generates a new `project_id`;
    /// - copies the project database (authoritative rows);
    /// - does **not** copy an active writer lease/runtime job;
    /// - points the catalog at the new project directory.
    pub fn duplicate(&self, source_id: &str, new_id: &str, now: &str) -> Result<(), ManagerError> {
        if new_id.is_empty() {
            return Err(ManagerError::InvalidProjectId);
        }
        if new_id == source_id {
            return Err(ManagerError::InvalidProjectId);
        }
        if self
            .writers
            .lock()
            .map_err(|_| ManagerError::WriterLockPoisoned)?
            .contains(source_id)
        {
            return Err(ManagerError::AlreadyOpen);
        }
        let source_dir = self.paths.project_dir(source_id)?;
        let source_db = source_dir.join("project.db");
        if !source_db.is_file() {
            return Err(ManagerError::ProjectNotFound);
        }
        let target_dir = self.paths.project_dir(new_id)?;
        let target_db = target_dir.join("project.db");
        std::fs::create_dir_all(&target_dir)?;
        std::fs::copy(&source_db, &target_db)?;
        let connection = open_connection(&target_db, false)?;
        // Rewrite project identity to the new id so the duplicate is authoritative
        // under its own name, without inheriting source runtime state.
        connection.execute(
            "UPDATE project_meta SET project_id = ?1, title = ?2, revision = 0, maintenance_state = 'READY', updated_at = ?3 WHERE project_id = ?4",
            libsql_rusqlite::params![new_id, format!("{source_id} 副本"), now, source_id],
        )?;
        quick_check(&connection)?;
        drop(connection);
        let catalog = CatalogRepository::open(&self.paths, now)?;
        CatalogRepository::register_project(
            &catalog,
            new_id,
            &target_dir.to_string_lossy(),
            &format!("{source_id} 副本"),
            now,
        )?;
        CatalogRepository::set_lifecycle(&catalog, new_id, "ACTIVE", now)?;
        Ok(())
    }

    /// Moves a project to a recoverable trash directory instead of irreversibly
    /// deleting it, and refuses to run when the project is actively written.
    ///
    /// - lifecycle transitions to `DELETING` first;
    /// - refuses when a writer lease or active writer is present;
    /// - relocates the directory under a controlled trash root;
    /// - deleting A never mutates B.
    pub fn delete_to_trash(&self, project_id: &str, now: &str) -> Result<(), ManagerError> {
        let catalog = CatalogRepository::open(&self.paths, now)?;
        match CatalogRepository::lifecycle(&catalog, project_id)?.as_deref() {
            Some("ACTIVE") | Some("ARCHIVED") | Some("DELETING") => {}
            _ => return Err(ManagerError::ProjectNotFound),
        }
        if self
            .writers
            .lock()
            .map_err(|_| ManagerError::WriterLockPoisoned)?
            .contains(project_id)
        {
            return Err(ManagerError::AlreadyOpen);
        }
        let source_dir = self.paths.project_dir(project_id)?;
        if !source_dir.join("project.db").is_file() {
            return Err(ManagerError::ProjectNotFound);
        }
        CatalogRepository::set_lifecycle(&catalog, project_id, "DELETING", now)?;
        let trash_dir = self.paths.trash_dir();
        std::fs::create_dir_all(&trash_dir)?;
        let target_dir = trash_dir.join(project_id);
        if target_dir.exists() {
            return Err(ManagerError::AlreadyOpen);
        }
        std::fs::rename(&source_dir, &target_dir)?;
        CatalogRepository::set_project_path(
            &catalog,
            project_id,
            &target_dir.to_string_lossy(),
            now,
        )?;
        Ok(())
    }

    /// Reconciles the catalog against the on-disk project/trash directories after
    /// an interrupted duplicate/delete, restoring path consistency.
    pub fn reconcile(&self, now: &str) -> Result<Vec<String>, ManagerError> {
        let catalog = CatalogRepository::open(&self.paths, now)?;
        let rows = CatalogRepository::list_projects(&catalog)?;
        let mut repaired = Vec::new();
        for row in rows {
            let canonical = self.paths.project_dir(&row.project_id)?;
            let trash = self.paths.trash_dir().join(&row.project_id);
            let recorded_exists = Path::new(&row.project_path).join("project.db").is_file();
            let canonical_exists = canonical.join("project.db").is_file();
            let trash_exists = trash.join("project.db").is_file();
            if recorded_exists {
                continue;
            }
            // Recorded path is gone; repoint to whichever authoritative copy
            // actually survived the interrupted operation.
            if canonical_exists {
                CatalogRepository::set_project_path(
                    &catalog,
                    &row.project_id,
                    &canonical.to_string_lossy(),
                    now,
                )?;
                repaired.push(format!("{}:path-restored", row.project_id));
            } else if trash_exists {
                CatalogRepository::set_project_path(
                    &catalog,
                    &row.project_id,
                    &trash.to_string_lossy(),
                    now,
                )?;
                CatalogRepository::set_lifecycle(&catalog, &row.project_id, "DELETING", now)?;
                repaired.push(format!("{}:trash-restored", row.project_id));
            }
        }
        Ok(repaired)
    }

    pub fn close(&self, handle: ProjectHandle) {
        drop(handle);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("path: {0}")]
    Path(#[from] PathError),
    #[error("project not found")]
    ProjectNotFound,
    #[error("invalid project id")]
    InvalidProjectId,
    #[error("project writer already open")]
    AlreadyOpen,
    #[error("writer lock poisoned")]
    WriterLockPoisoned,
    #[error("invalid project title")]
    InvalidTitle,
    #[error("invalid project lifecycle")]
    InvalidLifecycle,
    #[error("database: {0}")]
    Database(#[from] DatabaseError),
    #[error("read pool: {0}")]
    ReadPool(#[from] read_pool::PoolError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
