pub mod connection;
pub mod paths;
pub mod read_pool;

pub use read_pool::ReadPool;

use crate::catalog::CatalogRepository;
use crate::generated::types::ProjectCommandReceipt;
use crate::hash::sha256_canonical;
pub use connection::DatabaseError;
use connection::{migrate_project, open_connection, project_needs_migration, quick_check};
use libsql_rusqlite::{Connection, OptionalExtension};
use paths::{AppPaths, PathError};
use serde_json::json;
use std::collections::HashSet;
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Fixed read-time timestamp used when a read does not carry a caller-supplied
/// wall-clock value. Reading the catalog's authoritative `project_path` does not
/// depend on the wall clock, so a constant is safe and keeps reads deterministic.
const DEFAULT_NOW: &str = "1970-01-01T00:00:00Z";

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

/// RAII guard over a pooled read-only connection. Obtained from
/// [`ProjectManager::read_lease`]; the connection is returned to the pool when
/// the guard is dropped, so a production caller that forgets an explicit release
/// can never leak a live handle and drive the pool to `Exhausted`.
pub struct ReadLease {
    project_id: String,
    connection: Option<Connection>,
    pool: Arc<Mutex<read_pool::ReadPool>>,
}

pub enum ProjectCommandOutcome<T> {
    Replayed(ProjectCommandReceipt),
    Committed {
        value: T,
        receipt: ProjectCommandReceipt,
    },
}

#[derive(Clone, Copy)]
pub struct ProjectCommandSpec<'a> {
    pub project_id: &'a str,
    pub result_project_id: &'a str,
    pub command_id: &'a str,
    pub idempotency_key: &'a str,
    pub command_type: &'a str,
    pub request_digest: &'a str,
    pub expected_revision: Option<i64>,
    pub now: &'a str,
    pub advance_revision: bool,
}

impl std::ops::Deref for ReadLease {
    type Target = Connection;
    fn deref(&self) -> &Self::Target {
        self.connection.as_ref().expect("lease connection present")
    }
}

impl Drop for ReadLease {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            if let Ok(mut pool) = self.pool.lock() {
                pool.release(&self.project_id, connection);
            }
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

    /// Creates a project using a frozen, atomic staging flow.
    ///
    /// ```text
    /// validate -> temp staging dir -> open/create/migrate/seed project.db
    ///          -> quick_check -> fsync/close -> atomic rename temp -> final
    ///          -> catalog transaction register
    /// ```
    ///
    /// - Catalog registration failure removes the just-finalized directory
    ///   (or leaves a recoverable orphan if removal fails).
    /// - A crash at any point before the atomic rename leaves only a `.staging`
    ///   directory, which [`reconcile`](Self::reconcile) can clean up; a crash
    ///   right after the rename but before register is detectable as a
    ///   catalog-missing create orphan.
    pub fn create(
        &self,
        project_id: &str,
        title: &str,
        now: &str,
    ) -> Result<ProjectHandle, ManagerError> {
        self.acquire_writer(project_id)?;
        let result = (|| {
            let final_dir = self.paths.project_dir(project_id)?;
            let staging_root = self.paths.staging_dir();
            std::fs::create_dir_all(&staging_root)?;
            // Ensure the projects root exists so the final rename has a parent dir.
            if let Some(parent) = final_dir.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let staging_dir =
                staging_root.join(format!("{project_id}.tmp-{}", now.replace(':', "-")));
            if staging_dir.exists() {
                // A stale staging dir from an interrupted run: remove it before retry.
                std::fs::remove_dir_all(&staging_dir)?;
            }
            std::fs::create_dir_all(&staging_dir)?;
            let database = staging_dir.join("project.db");
            // Build the authoritative DB fully inside staging first. Any failure
            // here (open/create, migrate, quick_check) must abort the create
            // BEFORE the atomic rename, so no partial project dir is ever exposed
            // under its final name.
            (|| -> Result<(), ManagerError> {
                let connection = open_connection(&database, false)?;
                migrate_project(&connection, project_id, title, now)?;
                quick_check(&connection)?;
                // Flush WAL into the main DB and close so rename yields a
                // self-contained DB snapshot (no lingering -wal to migrate).
                let _ = connection
                    .execute_batch("PRAGMA wal_checkpoint(FULL); PRAGMA journal_mode = DELETE;");
                drop(connection);
                Ok(())
            })()?;
            // Atomic rename of the fully-initialized directory to its final location.
            std::fs::rename(&staging_dir, &final_dir)?;
            // Register in the catalog inside a guard: if the catalog open/insert
            // fails after the rename, the final dir would otherwise be an orphan.
            // Remove it so no unregistered final project leaks; if removal also
            // fails, reconcile() detects and re-adopts the create orphan.
            if let Err(error) = (|| -> Result<(), ManagerError> {
                let catalog = CatalogRepository::open(&self.paths, now)?;
                CatalogRepository::register_project(
                    &catalog,
                    project_id,
                    &final_dir.to_string_lossy(),
                    title,
                    now,
                )?;
                Ok(())
            })() {
                let _ = std::fs::remove_dir_all(&final_dir);
                return Err(error);
            }
            let connection = open_connection(&final_dir.join("project.db"), false)?;
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
            // Open-policy gate (M0-03): a DELETING project is no longer
            // producible, so it may be read (UI shows the trashed state) but
            // cannot be opened for writing/migration. ACTIVE and ARCHIVED remain
            // openable. Read-only queries go through read_connection and are not
            // gated here.
            let catalog_path = self.paths.catalog_db();
            if !catalog_path.is_file() {
                return Err(ManagerError::ProjectNotFound);
            }
            let catalog = open_connection(&catalog_path, true)?;
            match CatalogRepository::lifecycle(&catalog, project_id)?.as_deref() {
                Some("ACTIVE") | Some("ARCHIVED") => {}
                _ => return Err(ManagerError::ProjectNotFound),
            }
            drop(catalog);
            // Resolve the authoritative directory from the catalog so the open
            // path can never diverge from what is registered (e.g. after a
            // delete-to-trash relocation or an interrupted duplicate).
            let directory = self.resolve_registered_project_path(project_id)?;
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

    fn resolve_registered_project_path(
        &self,
        project_id: &str,
    ) -> Result<std::path::PathBuf, ManagerError> {
        // Resolve from the catalog using a read-only connection that does NOT
        // run the catalog migration. Path resolution is a read-only lookup, so
        // re-migrating the catalog (a write transaction + full schema replay) on
        // every read would put a write on the hot query path — the very cost the
        // bounded read pool is meant to avoid. A cleanly-initialized catalog has
        // its tables already; an absent catalog (never created) simply has no
        // registered project, which is reported as ProjectNotFound and — being a
        // read-only open — never materializes a catalog db on disk.
        let catalog_path = self.paths.catalog_db();
        if !catalog_path.is_file() {
            return Err(ManagerError::ProjectNotFound);
        }
        let catalog = open_connection(&catalog_path, true)?;
        let registered = CatalogRepository::project_path(&catalog, project_id)?
            .ok_or(ManagerError::ProjectNotFound)?;
        let allowed_roots = [self.paths.projects_dir(), self.paths.trash_dir()];
        let path = Path::new(&registered);
        if !allowed_roots
            .iter()
            .any(|root| path.starts_with(root) && path.join("project.db").is_file())
        {
            return Err(ManagerError::PathOutsideRoot);
        }
        Ok(path.to_path_buf())
    }

    /// Lends a pooled read-only connection behind a RAII guard. The guard returns
    /// the connection to the pool on drop, so production reads never leak a live
    /// handle. This is the pool read entry; use the ergonomic
    /// [`with_read_connection`](Self::with_read_connection) for a closure form.
    pub fn read_connection(&self, project_id: &str, _now: &str) -> Result<ReadLease, ManagerError> {
        self.read_lease(project_id, _now)
    }

    /// Releases a pooled read connection back to the project's idle bucket.
    pub fn release_read(&self, project_id: &str, connection: Connection) {
        if let Ok(mut pool) = self.read_pool.lock() {
            pool.release(project_id, connection);
        }
    }

    /// Lends a pooled read-only connection behind a RAII guard. The guard returns
    /// the connection to the pool on drop, so production reads never leak a live
    /// handle. Prefer this over [`read_connection`](Self::read_connection) for any
    /// code path the Kernel drives.
    pub fn read_lease(&self, project_id: &str, _now: &str) -> Result<ReadLease, ManagerError> {
        let directory = self.resolve_registered_project_path(project_id)?;
        let database = directory.join("project.db");
        let mut pool = self
            .read_pool
            .lock()
            .map_err(|_| ManagerError::WriterLockPoisoned)?;
        let connection = pool.acquire(project_id, &database)?;
        Ok(ReadLease {
            project_id: project_id.to_string(),
            connection: Some(connection),
            pool: Arc::clone(&self.read_pool),
        })
    }

    /// Live read connections across the whole pool (idle + checked out). Exposed
    /// so callers/tests can observe that the production query path reuses a
    /// bounded pool instead of growing without bound.
    pub fn read_pool_live_len(&self) -> usize {
        self.read_pool
            .lock()
            .map(|pool| pool.live_len())
            .unwrap_or(0)
    }

    /// Idle pooled read connections available for reuse for a given project.
    pub fn read_pool_idle_len(&self, project_id: &str) -> usize {
        self.read_pool
            .lock()
            .map(|pool| pool.idle_len(project_id))
            .unwrap_or(0)
    }

    /// Runs `op` with a pooled read-only connection, releasing it back to the
    /// bounded pool regardless of whether `op` succeeds. This is the RAII form
    /// of [`read_lease`](Self::read_lease) so production query paths never leak a
    /// read handle, and is the single entry point the Kernel read queries use.
    pub fn with_read_connection<T>(
        &self,
        project_id: &str,
        now: &str,
        op: impl FnOnce(&Connection) -> Result<T, ManagerError>,
    ) -> Result<T, ManagerError> {
        let lease = self.read_lease(project_id, now)?;
        op(&lease)
    }

    /// Reads the authoritative receipt for a project lifecycle command without
    /// opening a writer. This also works for a project already moved to trash,
    /// which is required for safe retries of delete commands.
    pub fn find_project_command_receipt(
        &self,
        project_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<ProjectCommandReceipt>, ManagerError> {
        let directory = match self.resolve_registered_project_path(project_id) {
            Ok(directory) => directory,
            Err(ManagerError::ProjectNotFound) => return Ok(None),
            Err(error) => return Err(error),
        };
        let connection = open_connection(&directory.join("project.db"), true)?;
        Self::read_project_command_receipt(&connection, project_id, idempotency_key)
    }

    /// Runs a project command mutation and its receipt write in one project DB
    /// transaction. Catalog projection updates are performed by the caller only
    /// after this authoritative transaction succeeds.
    pub fn commit_project_command<T>(
        &self,
        spec: ProjectCommandSpec<'_>,
        apply: impl FnOnce(&Connection) -> Result<T, ManagerError>,
    ) -> Result<ProjectCommandOutcome<T>, ManagerError> {
        let handle = self.open(spec.project_id, "", spec.now)?;
        let connection: &Connection = &handle;
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            if let Some(previous) = Self::read_project_command_receipt(
                connection,
                spec.project_id,
                spec.idempotency_key,
            )? {
                if previous.request_digest == spec.request_digest {
                    return Ok(ProjectCommandOutcome::Replayed(previous));
                }
                return Err(ManagerError::IdempotencyConflict);
            }

            let before: i64 = connection
                .query_row(
                    "SELECT revision FROM project_meta WHERE project_id = ?1 AND maintenance_state = 'READY'",
                    [spec.project_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or(ManagerError::ProjectNotFound)?;
            if let Some(expected) = spec.expected_revision {
                if expected != before {
                    return Err(ManagerError::RevisionConflict);
                }
            }

            let value = apply(connection)?;
            let after = if spec.advance_revision {
                before + 1
            } else {
                before
            };
            if spec.advance_revision {
                let updated = connection.execute(
                    "UPDATE project_meta SET revision = ?1, updated_at = ?2 WHERE project_id = ?3 AND revision = ?4",
                    libsql_rusqlite::params![after, spec.now, spec.project_id, before],
                )?;
                if updated != 1 {
                    return Err(ManagerError::RevisionConflict);
                }
            }

            let mut receipt = ProjectCommandReceipt {
                receipt_id: format!("project-receipt:{}", spec.command_id),
                project_id: spec.project_id.to_string(),
                result_project_id: spec.result_project_id.to_string(),
                command_id: spec.command_id.to_string(),
                idempotency_key: spec.idempotency_key.to_string(),
                command_type: spec.command_type.to_string(),
                before_revision: before,
                after_revision: after,
                request_digest: spec.request_digest.to_string(),
                committed_at: spec.now.to_string(),
                receipt_digest: String::new(),
            };
            let mut digest_input = serde_json::to_value(&receipt)?;
            digest_input["receiptDigest"] = json!("");
            receipt.receipt_digest = sha256_canonical(&digest_input)?;
            let receipt_json = serde_json::to_string(&receipt)?;
            connection.execute(
                "INSERT INTO project_command_receipts(receipt_id, project_id, result_project_id, command_id, idempotency_key, command_type, before_revision, after_revision, request_digest, receipt_json, receipt_digest, committed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                libsql_rusqlite::params![
                    receipt.receipt_id,
                    receipt.project_id,
                    receipt.result_project_id,
                    receipt.command_id,
                    receipt.idempotency_key,
                    receipt.command_type,
                    receipt.before_revision,
                    receipt.after_revision,
                    receipt.request_digest,
                    receipt_json,
                    receipt.receipt_digest,
                    receipt.committed_at,
                ],
            )?;
            Ok(ProjectCommandOutcome::Committed { value, receipt })
        })();

        match result {
            Ok(outcome) => {
                connection.execute_batch("COMMIT")?;
                Ok(outcome)
            }
            Err(error) => {
                let _ = connection.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub fn commit_project_rename(
        &self,
        spec: ProjectCommandSpec<'_>,
        title: &str,
    ) -> Result<ProjectCommandOutcome<()>, ManagerError> {
        if title.trim().is_empty() {
            return Err(ManagerError::InvalidTitle);
        }
        self.commit_project_command(spec, |connection| {
            connection.execute(
                "UPDATE project_meta SET title = ?1 WHERE project_id = ?2",
                libsql_rusqlite::params![title, spec.project_id],
            )?;
            Ok(())
        })
    }

    pub fn commit_project_integrity(
        &self,
        spec: ProjectCommandSpec<'_>,
    ) -> Result<ProjectCommandOutcome<()>, ManagerError> {
        self.commit_project_command(spec, |connection| {
            quick_check(connection)?;
            Ok(())
        })
    }

    fn read_project_command_receipt(
        connection: &Connection,
        project_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<ProjectCommandReceipt>, ManagerError> {
        let row: Option<(String, String)> = connection
            .query_row(
                "SELECT receipt_json, receipt_digest FROM project_command_receipts WHERE project_id = ?1 AND idempotency_key = ?2",
                [project_id, idempotency_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(receipt_json, stored_digest)| {
            let receipt: ProjectCommandReceipt = serde_json::from_str(&receipt_json)?;
            let mut digest_input = serde_json::to_value(&receipt)?;
            digest_input["receiptDigest"] = json!("");
            let expected = sha256_canonical(&digest_input)?;
            if receipt.receipt_digest != expected || stored_digest != expected {
                return Err(ManagerError::ProjectCommandReceiptDigestMismatch);
            }
            Ok(receipt)
        })
        .transpose()
    }

    pub fn overview(&self, project_id: &str) -> Result<(String, i64, String), ManagerError> {
        // Route the production Overview query through the bounded reusable read
        // pool instead of opening a fresh read-only connection each call, so a
        // busy UI maintains a flat live handle count rather than unbounded growth.
        self.with_read_connection(project_id, DEFAULT_NOW, |connection| {
            connection
                .query_row(
                    "SELECT title, revision, updated_at FROM project_meta WHERE project_id = ?1",
                    [project_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(ManagerError::Sqlite)
        })
    }

    pub fn project_revision(&self, project_id: &str) -> Result<i64, ManagerError> {
        self.with_read_connection(project_id, DEFAULT_NOW, |connection| {
            connection
                .query_row(
                    "SELECT revision FROM project_meta WHERE project_id = ?1",
                    [project_id],
                    |row| row.get(0),
                )
                .map_err(ManagerError::Sqlite)
        })
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
        let source_dir = self.resolve_registered_project_path(source_id)?;
        let source_db = source_dir.join("project.db");
        if !source_db.is_file() {
            return Err(ManagerError::ProjectNotFound);
        }
        let target_dir = self.paths.project_dir(new_id)?;
        let staging_root = self.paths.staging_dir();
        std::fs::create_dir_all(&staging_root)?;
        if let Some(parent) = target_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let staging_dir = staging_root.join(format!("{new_id}.dupe-{}", now.replace(':', "-")));
        if staging_dir.exists() {
            std::fs::remove_dir_all(&staging_dir)?;
        }
        std::fs::create_dir_all(&staging_dir)?;
        let staging_db = staging_dir.join("project.db");

        // Use SQLite Online Backup so a WAL-mode source with un-checkpointed
        // pages yields a consistent snapshot (never assume a checkpoint ran).
        let source_connection = open_connection(&source_db, true)?;
        source_connection
            .backup(libsql_rusqlite::DatabaseName::Main, &staging_db, None)
            .map_err(ManagerError::Sqlite)?;

        let connection = open_connection(&staging_db, false)?;
        // Authoritative identity remap for the Copy Project contract (P0-02).
        // The source DB is a full Online Backup snapshot, so every authoritative
        // table carrying `project_id` must be rewritten to the new project, while
        // the source's runtime/audit artifacts (commands, commit receipts, gc
        // receipts) are NOT copied — they are immutable, belong to the source
        // identity, and instead we write a CopyReceipt to record this duplication.
        Self::remap_copy(&connection, source_id, new_id, now)?;
        // Reset the WAL checkpoint so the relocated duplicate is self-contained.
        let _ =
            connection.execute_batch("PRAGMA wal_checkpoint(FULL); PRAGMA journal_mode = DELETE;");
        quick_check(&connection)?;
        drop(connection);

        // Atomic finalize: rename the fully-built staging dir into place.
        std::fs::rename(&staging_dir, &target_dir)?;
        let catalog = CatalogRepository::open(&self.paths, now)?;
        if let Err(error) = CatalogRepository::register_project(
            &catalog,
            new_id,
            &target_dir.to_string_lossy(),
            &format!("{source_id} 副本"),
            now,
        ) {
            let _ = std::fs::remove_dir_all(&target_dir);
            return Err(error.into());
        }
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
        for row in &rows {
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
        // Detect create orphans: a final project directory that exists on disk
        // but has no catalog entry (crash between atomic rename and register),
        // and stale `.staging` directories from an interrupted create.
        let projects_root = self.paths.projects_dir();
        if projects_root.is_dir() {
            for entry in std::fs::read_dir(&projects_root)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let dir = entry.path();
                let db = dir.join("project.db");
                let in_catalog = rows.iter().any(|row| row.project_id == name);
                if !in_catalog && db.is_file() {
                    // Catalog-missing create orphan: adopt it back into the
                    // catalog so its authoritative data is not lost.
                    let title = project_meta_title(&db, &name)?;
                    CatalogRepository::register_project(
                        &catalog,
                        &name,
                        &dir.to_string_lossy(),
                        &title,
                        now,
                    )?;
                    repaired.push(format!("{name}:create-orphan-restored"));
                }
            }
        }
        // Remove stale staging directories left by a crash before atomic rename.
        let staging_root = self.paths.staging_dir();
        if staging_root.is_dir() {
            for entry in std::fs::read_dir(&staging_root)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    let _ = std::fs::remove_dir_all(entry.path());
                    repaired.push(format!(
                        "staging:{}:cleaned",
                        entry.file_name().to_string_lossy()
                    ));
                }
            }
        }
        Ok(repaired)
    }

    pub fn close(&self, handle: ProjectHandle) {
        drop(handle);
    }

    /// Rewrites a source project's authoritative rows to a new project id inside a
    /// copy staging DB, applying the frozen Copy Project identity-remap contract
    /// (P0-02). It:
    ///
    /// - clears the source's runtime/audit artifacts (commands, commit receipts,
    ///   gc receipts) rather than copying immutable receipts that belong to the
    ///   source identity;
    /// - rewrites `project_id` on every authoritative table (project_meta,
    ///   artifacts, canonical_heads) to `new_id`;
    /// - re-points the CATALOG-independent `provenance_json.refs` only for
    ///   mutable/candidate rows; committed provenance remains immutable source
    ///   evidence;
    /// - writes a `copy_receipts` row to record the duplication;
    /// - validates `foreign_key_check` is empty and `quick_check = ok`.
    ///
    /// The caller (duplicate) owns the surrounding staging DB; this runs as part
    /// of that staging transaction.
    fn remap_copy(
        connection: &Connection,
        source_id: &str,
        new_id: &str,
        now: &str,
    ) -> Result<(), ManagerError> {
        // Re-map under a relaxed-FK window: artifacts/canonical_heads reference
        // project_meta, so we rewrite them before re-enabling FK checking.
        connection.execute_batch("PRAGMA foreign_keys = OFF;")?;
        let result = (|| -> Result<(), ManagerError> {
            // 1. Drop the source's runtime/audit artifacts from the copy.
            connection.execute("DELETE FROM commands WHERE project_id = ?1", [source_id])?;
            connection.execute(
                "DELETE FROM commit_receipts WHERE project_id = ?1",
                [source_id],
            )?;
            connection.execute("DELETE FROM gc_receipts WHERE project_id = ?1", [source_id])?;
            connection.execute(
                "DELETE FROM project_command_receipts WHERE project_id = ?1",
                [source_id],
            )?;
            connection.execute("DELETE FROM copy_receipts", [])?;
            // 2. Rewrite project_id on each authoritative table.
            connection.execute("UPDATE project_meta SET project_id = ?1, title = ?2, revision = 0, maintenance_state = 'READY', updated_at = ?3 WHERE project_id = ?4", libsql_rusqlite::params![new_id, format!("{source_id} 副本"), now, source_id])?;
            connection.execute(
                "UPDATE artifacts SET project_id = ?1 WHERE project_id = ?2",
                [new_id, source_id],
            )?;
            connection.execute(
                "UPDATE canonical_heads SET project_id = ?1 WHERE project_id = ?2",
                [new_id, source_id],
            )?;
            // 3. Re-point inner project references inside serialized provenance.
            rewrite_provenance_project(connection, source_id, new_id)?;
            // 4. Record the copy action.
            write_copy_receipt(connection, source_id, new_id, now)?;
            Ok::<_, ManagerError>(())
        })();
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        result?;
        // 5. Validate: the copy must be FK-clean and intact before finalizing.
        let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
        let fk_rows = statement.query_map([], |_row| Ok(()))?.count();
        if fk_rows != 0 {
            return Err(ManagerError::Io(std::io::Error::other(format!(
                "copy project left {fk_rows} foreign-key violations"
            ))));
        }
        quick_check(connection)?;
        Ok(())
    }
}

/// Reads the authoritative `title` for `project_id` from its on-disk database,
/// used when re-adopting a catalog-missing create orphan during reconcile.
/// The id is passed in directly because the orphan is not yet registered, so
/// there is no catalog row to resolve it from.
fn project_meta_title(database: &Path, project_id: &str) -> Result<String, ManagerError> {
    let connection = open_connection(database, true)?;
    connection
        .query_row(
            "SELECT title FROM project_meta WHERE project_id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .map_err(ManagerError::Sqlite)
}

/// Rewrites any in-string reference to the source project id inside mutable
/// artifact provenance values. Committed artifact metadata is immutable and its
/// source provenance must remain unchanged in a copy.
fn rewrite_provenance_project(
    connection: &Connection,
    source_id: &str,
    new_id: &str,
) -> Result<(), ManagerError> {
    let mut statement = connection.prepare(
        "SELECT artifact_id, version, provenance_json FROM artifacts WHERE project_id = ?1 AND artifact_status <> 'COMMITTED'",
    )?;
    let rows: Vec<(String, i64, String)> = statement
        .query_map([new_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;
    drop(statement);
    for (artifact_id, version, provenance) in rows {
        if provenance.contains(source_id) {
            let rewritten = provenance.replace(source_id, new_id);
            connection.execute(
                "UPDATE artifacts SET provenance_json = ?1 WHERE artifact_id = ?2 AND version = ?3",
                libsql_rusqlite::params![rewritten, artifact_id, version],
            )?;
        }
    }
    Ok(())
}

/// Writes a CopyReceipt recording that `source_id` was duplicated into `new_id`.
///
/// The receipt's digest is computed over the stable copy fields and validated by
/// the same digest-verification helper used for project-command receipts.
fn write_copy_receipt(
    connection: &Connection,
    source_id: &str,
    new_id: &str,
    now: &str,
) -> Result<(), ManagerError> {
    let receipt_id = format!("copy:{source_id}:{now}");
    let digest_input = serde_json::json!({
        "receiptId": receipt_id,
        "sourceProjectId": source_id,
        "targetProjectId": new_id,
        "copiedAt": now,
    });
    let receipt_digest = sha256_canonical(&digest_input)?;
    let receipt_json = serde_json::to_string(&serde_json::json!({
        "receiptId": receipt_id,
        "sourceProjectId": source_id,
        "targetProjectId": new_id,
        "copiedAt": now,
        "receiptDigest": receipt_digest,
    }))?;
    connection.execute("INSERT OR IGNORE INTO copy_receipts(receipt_id, source_project_id, target_project_id, copied_at, receipt_json, receipt_digest) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", libsql_rusqlite::params![receipt_id, source_id, new_id, now, receipt_json, receipt_digest])?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("path: {0}")]
    Path(#[from] PathError),
    #[error("project not found")]
    ProjectNotFound,
    #[error("path outside allowed root")]
    PathOutsideRoot,
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
    #[error("REVISION_CONFLICT")]
    RevisionConflict,
    #[error("IDEMPOTENCY_CONFLICT")]
    IdempotencyConflict,
    #[error("PROJECT_COMMAND_RECEIPT_DIGEST_MISMATCH")]
    ProjectCommandReceiptDigestMismatch,
    #[error("database: {0}")]
    Database(#[from] DatabaseError),
    #[error("read pool: {0}")]
    ReadPool(#[from] read_pool::PoolError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}
