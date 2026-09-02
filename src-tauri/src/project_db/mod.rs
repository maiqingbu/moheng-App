pub mod connection;
pub mod paths;
pub mod read_pool;

pub use read_pool::ReadPool;

use crate::catalog::CatalogRepository;
use crate::commit::{CommitError, CommitService};
pub use crate::commit::{ProjectCommandOutcome, ProjectCommandSpec};
use crate::generated::types::ProjectCommandReceipt;
use crate::hash::sha256_canonical;
use crate::repositories::ops::record_runtime_profile;
pub use connection::DatabaseError;
use connection::{migrate_project, open_connection, project_needs_migration, quick_check};
use libsql_rusqlite::{Connection, OptionalExtension};
use paths::{AppPaths, PathError};
use std::collections::HashSet;
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Fixed read-time timestamp used when a read does not carry a caller-supplied
/// wall-clock value. Reading the catalog's authoritative `project_path` does not
/// depend on the wall clock, so a constant is safe and keeps reads deterministic.
const DEFAULT_NOW: &str = "1970-01-01T00:00:00Z";

/// Rebuildable runtime-state projection read back from a project's own DB.
/// Derived from project_runtime_state; never a second authority.
pub struct LibraryRuntimeProjection {
    pub current_phase: String,
    pub run_state: String,
    pub block_reason: Option<String>,
    pub signing_status: String,
    pub committed_chapters: i64,
    pub candidate_chapters: i64,
    pub total_words: i64,
    pub total_cost_cents: i64,
    pub progress: f64,
    pub generated_revision: i64,
}

impl Default for LibraryRuntimeProjection {
    fn default() -> Self {
        Self {
            current_phase: "SETUP".to_string(),
            run_state: "IDLE".to_string(),
            block_reason: None,
            signing_status: "NOT_STARTED".to_string(),
            committed_chapters: 0,
            candidate_chapters: 0,
            total_words: 0,
            total_cost_cents: 0,
            progress: 0.0,
            generated_revision: 0,
        }
    }
}

/// Controlled, non-catalog-derived projection that backs the project library.
/// `revision` is the project's authoritative revision; `runtime` is None when the
/// project has not yet produced a runtime-state row (fresh M0B project).
#[derive(Default)]
pub struct LibraryProjection {
    pub revision: i64,
    pub runtime: Option<LibraryRuntimeProjection>,
}

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
    maintenance: Arc<Mutex<HashSet<String>>>,
    restore_test_hook: Arc<Mutex<Option<RestoreTestHook>>>,
}

pub type RestoreTestHook = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;

pub struct ProjectMaintenanceGuard {
    maintenance: Arc<Mutex<HashSet<String>>>,
    project_id: String,
}

impl Drop for ProjectMaintenanceGuard {
    fn drop(&mut self) {
        if let Ok(mut projects) = self.maintenance.lock() {
            projects.remove(&self.project_id);
        }
    }
}

impl ProjectManager {
    pub fn begin_maintenance(
        &self,
        project_id: &str,
    ) -> Result<ProjectMaintenanceGuard, ManagerError> {
        let mut maintenance = self
            .maintenance
            .lock()
            .map_err(|_| ManagerError::WriterLockPoisoned)?;
        if !maintenance.insert(project_id.to_string()) {
            return Err(ManagerError::ProjectCommand(
                "RESTORE_ALREADY_IN_PROGRESS".into(),
            ));
        }
        Ok(ProjectMaintenanceGuard {
            maintenance: Arc::clone(&self.maintenance),
            project_id: project_id.to_string(),
        })
    }
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            writers: Arc::new(Mutex::new(HashSet::new())),
            read_pool: Arc::new(Mutex::new(read_pool::ReadPool::new(16, 4))),
            maintenance: Arc::new(Mutex::new(HashSet::new())),
            restore_test_hook: Arc::new(Mutex::new(None)),
        }
    }

    #[doc(hidden)]
    pub fn set_restore_test_hook(&self, hook: Option<RestoreTestHook>) -> Result<(), ManagerError> {
        *self
            .restore_test_hook
            .lock()
            .map_err(|_| ManagerError::WriterLockPoisoned)? = hook;
        Ok(())
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
                record_runtime_profile(&connection, project_id, now)
                    .map_err(|error| DatabaseError::Integrity(error.to_string()))?;
                crate::repositories::ops::SearchService::ensure_projection(
                    &connection,
                    project_id,
                    now,
                )
                .map_err(|error| DatabaseError::Integrity(error.to_string()))?;
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
        self.open_internal(project_id, title, now, true)
    }

    fn open_internal(
        &self,
        project_id: &str,
        title: &str,
        now: &str,
        ensure_projection: bool,
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
            // Normal open is a metadata/projection-health path. A full
            // quick_check belongs to explicit recovery/integrity commands; doing
            // it here would scan a 10M-word project on every UI reopen.
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
            if ensure_projection {
                record_runtime_profile(&connection, project_id, now).map_err(|error| {
                    ManagerError::Database(DatabaseError::Integrity(error.to_string()))
                })?;
                crate::repositories::ops::SearchService::ensure_projection(
                    &connection,
                    project_id,
                    now,
                )
                .map_err(|error| {
                    ManagerError::Database(DatabaseError::Integrity(error.to_string()))
                })?;
                let catalog = CatalogRepository::open(&self.paths, now)?;
                CatalogRepository::touch_opened(&catalog, project_id, now)?;
                CatalogRepository::set_compatibility(&catalog, project_id, "COMPATIBLE", now)?;
            }
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
        let maintenance = self
            .maintenance
            .lock()
            .map_err(|_| ManagerError::WriterLockPoisoned)?;
        if maintenance.contains(project_id) {
            return Err(ManagerError::ProjectCommand(
                "RETRYABLE_MAINTENANCE_ERROR: project reader gate is closed".into(),
            ));
        }
        let mut pool = self
            .read_pool
            .lock()
            .map_err(|_| ManagerError::WriterLockPoisoned)?;
        let connection = pool.acquire(project_id, &database)?;
        let maintenance_state: String = connection
            .query_row(
                "SELECT maintenance_state FROM project_meta WHERE project_id=?1",
                [project_id],
                |row| row.get(0),
            )
            .map_err(ManagerError::Sqlite)?;
        if maintenance_state != "READY" {
            pool.release(project_id, connection);
            return Err(ManagerError::ProjectCommand(
                "RETRYABLE_MAINTENANCE_ERROR: project database is not ready".into(),
            ));
        }
        drop(maintenance);
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
        CommitService::find_project_command_receipt(&connection, project_id, idempotency_key)
            .map_err(manager_error_from_commit)
    }

    /// Runs a project command mutation and its receipt write in one project DB
    /// transaction. Catalog projection updates are performed by the caller only
    /// after this authoritative transaction succeeds.
    pub fn commit_project_command<T>(
        &self,
        spec: ProjectCommandSpec<'_>,
        apply: impl FnOnce(&Connection) -> Result<T, ManagerError>,
    ) -> Result<ProjectCommandOutcome<T>, ManagerError> {
        let handle = if matches!(
            spec.command_type,
            "change_request_create"
                | "impact_preflight"
                | "impact_expand"
                | "impact_resume"
                | "impact_cancel"
                | "change_request_decide"
                | "retcon_plan_create"
                | "retcon_apply"
        ) {
            self.open_internal(spec.project_id, "", spec.now, false)?
        } else {
            self.open(spec.project_id, "", spec.now)?
        };
        let connection: &Connection = &handle;
        CommitService::commit_project_command(connection, spec, |connection| {
            apply(connection).map_err(|error| CommitError::ProjectCommandApply(error.to_string()))
        })
        .map_err(manager_error_from_commit)
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
            let report = crate::repositories::ops::deep_integrity_check(
                connection,
                spec.project_id,
                spec.now,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
            if !report.ok {
                return Err(ManagerError::ProjectCommand(report.failures.join(",")));
            }
            Ok(())
        })
    }

    pub fn commit_backup(
        &self,
        spec: ProjectCommandSpec<'_>,
        backup_path: &Path,
    ) -> Result<ProjectCommandOutcome<()>, ManagerError> {
        let handle = self.open(spec.project_id, "", spec.now)?;
        let manifest = crate::repositories::ops::BackupService::create_snapshot(
            &handle,
            backup_path,
            spec.project_id,
            spec.now,
        )
        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
        let result = CommitService::commit_project_command(&handle, spec, |connection| {
            crate::repositories::ops::BackupService::persist_manifest(connection, &manifest)
                .map_err(|error| CommitError::ProjectCommandApply(error.to_string()))?;
            Ok(())
        })
        .map_err(manager_error_from_commit);
        drop(handle);
        result
    }

    /// Performs a formal, checksum-gated project restore. The project enters
    /// MAINTENANCE before the old writer is closed, the staged database must
    /// pass quick/FK/deep integrity plus identity checks, and only then is the
    /// staged file atomically promoted and stamped with a project receipt.
    pub fn restore_backup(
        &self,
        spec: ProjectCommandSpec<'_>,
        backup_path: &Path,
        expected_checksum: &str,
    ) -> Result<ProjectCommandOutcome<crate::repositories::ops::RestoreReport>, ManagerError> {
        self.acquire_writer(spec.project_id)?;
        let result = (|| {
            let _maintenance_guard = self.begin_maintenance(spec.project_id)?;
            let directory = self.resolve_registered_project_path(spec.project_id)?;
            let database = directory.join("project.db");
            let staging = directory.join("project.db.restore.tmp");
            let mark_ready = || -> Result<(), ManagerError> {
                let connection = open_connection(&database, false)?;
                connection.execute(
                    "UPDATE project_meta SET maintenance_state='READY', updated_at=?1 WHERE project_id=?2",
                    libsql_rusqlite::params![spec.now, spec.project_id],
                )?;
                Ok(())
            };
            {
                let connection = open_connection(&database, false)?;
                connection.execute(
                    "UPDATE project_meta SET maintenance_state='MAINTENANCE', updated_at=?1 WHERE project_id=?2",
                    libsql_rusqlite::params![spec.now, spec.project_id],
                )?;
            }
            let restore_test_hook = self
                .restore_test_hook
                .lock()
                .map_err(|_| ManagerError::WriterLockPoisoned)?
                .clone();
            if let Some(hook) = restore_test_hook {
                if let Err(error) = hook() {
                    let _ = mark_ready();
                    return Err(ManagerError::ProjectCommand(format!(
                        "RESTORE_TEST_HOOK: {error}"
                    )));
                }
            }
            {
                let pool = self
                    .read_pool
                    .lock()
                    .map_err(|_| ManagerError::WriterLockPoisoned)?;
                if pool.checked_out_len(spec.project_id) != 0 {
                    let _ = mark_ready();
                    return Err(ManagerError::ProjectCommand(
                        "READ_POOL_BUSY: restore requires all read leases to drain".into(),
                    ));
                }
            }
            self.read_pool
                .lock()
                .map_err(|_| ManagerError::WriterLockPoisoned)?
                .invalidate_project(spec.project_id);
            if staging.exists() {
                std::fs::remove_file(&staging)?;
            }
            let report = match crate::repositories::ops::BackupService::restore_verified(
                backup_path,
                &staging,
                expected_checksum,
                spec.project_id,
                spec.now,
            ) {
                Ok(report) => report,
                Err(error) => {
                    let _ = mark_ready();
                    let _ = std::fs::remove_file(&staging);
                    return Err(ManagerError::ProjectCommand(error.to_string()));
                }
            };
            let outcome = (|| -> Result<_, ManagerError> {
                let connection = open_connection(&staging, false)?;
                connection.execute(
                    "UPDATE project_meta SET maintenance_state='READY', updated_at=?1 WHERE project_id=?2",
                    libsql_rusqlite::params![spec.now, spec.project_id],
                )?;
                let outcome =
                    CommitService::commit_project_command(
                        &connection,
                        spec,
                        |_| Ok(report.clone()),
                    )
                    .map_err(manager_error_from_commit)?;
                let _ = connection
                    .execute_batch("PRAGMA wal_checkpoint(FULL); PRAGMA journal_mode = DELETE;");
                drop(connection);
                std::fs::rename(&database, database.with_extension("db.pre-restore"))?;
                if let Err(error) = std::fs::rename(&staging, &database) {
                    let _ = std::fs::rename(database.with_extension("db.pre-restore"), &database);
                    return Err(error.into());
                }
                let ready = open_connection(&database, false)?;
                ready.execute(
                    "UPDATE project_meta SET maintenance_state='READY', updated_at=?1 WHERE project_id=?2",
                    libsql_rusqlite::params![spec.now, spec.project_id],
                )?;
                let state: String = ready.query_row(
                    "SELECT maintenance_state FROM project_meta WHERE project_id=?1",
                    [spec.project_id],
                    |row| row.get(0),
                )?;
                if state != "READY" {
                    return Err(ManagerError::ProjectCommand(
                        "RESTORE_NOT_READY_AFTER_REPLACE".into(),
                    ));
                }
                let _ = std::fs::remove_file(database.with_extension("db.pre-restore"));
                Ok(outcome)
            })();
            if outcome.is_err() {
                let _ = std::fs::remove_file(&staging);
                let _ = mark_ready();
            }
            outcome
        })();
        self.release_writer(spec.project_id);
        result
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

    /// Reads the authoritative revision plus the rebuildable runtime-state summary
    /// for a single project, through the bounded read-only pool. This is the
    /// controlled, non-catalog-derived projection that backs the project library
    /// (B01-T07), so a library row carries the project's *actual* revision instead
    /// of a hard-coded 0. The runtime_state table is a derived projection; when it
    /// has no row yet the caller receives conservative defaults rather than a
    /// fabricated value.
    pub fn library_projection(&self, project_id: &str) -> Result<LibraryProjection, ManagerError> {
        self.with_read_connection(project_id, DEFAULT_NOW, |connection| {
            let revision: i64 = connection
                .query_row(
                    "SELECT revision FROM project_meta WHERE project_id = ?1",
                    [project_id],
                    |row| row.get(0),
                )
                .map_err(ManagerError::Sqlite)?;
            // runtime_state is optional (a fresh M0B project may not have one yet).
            let runtime = connection
                .query_row(
                    "SELECT current_phase, run_state, block_reason, signing_status, committed_chapters, candidate_chapters, total_words, total_cost_cents, progress, generated_revision
                     FROM project_runtime_state WHERE project_id = ?1",
                    [project_id],
                    |row| {
                        Ok(LibraryRuntimeProjection {
                            current_phase: row.get(0)?,
                            run_state: row.get(1)?,
                            block_reason: row.get(2)?,
                            signing_status: row.get(3)?,
                            committed_chapters: row.get(4)?,
                            candidate_chapters: row.get(5)?,
                            total_words: row.get(6)?,
                            total_cost_cents: row.get(7)?,
                            progress: row.get(8)?,
                            generated_revision: row.get(9)?,
                        })
                    },
                )
                .optional()
                .map_err(ManagerError::Sqlite)?;
            Ok(LibraryProjection { revision, runtime })
        })
    }

    pub fn has_valid_copy_receipt(
        &self,
        source_id: &str,
        target_id: &str,
    ) -> Result<bool, ManagerError> {
        let directory = match self.resolve_registered_project_path(target_id) {
            Ok(directory) => directory,
            Err(ManagerError::ProjectNotFound) => return Ok(false),
            Err(error) => return Err(error),
        };
        let connection = open_connection(&directory.join("project.db"), true)?;
        let row: Option<(String, String)> = connection
            .query_row(
                "SELECT receipt_json, receipt_digest FROM copy_receipts WHERE source_project_id = ?1 AND target_project_id = ?2",
                [source_id, target_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((receipt_json, stored_digest)) = row else {
            return Ok(false);
        };
        let receipt: serde_json::Value = serde_json::from_str(&receipt_json)?;
        let expected = copy_receipt_digest(
            receipt["receiptId"].as_str().unwrap_or_default(),
            receipt["sourceProjectId"].as_str().unwrap_or_default(),
            receipt["targetProjectId"].as_str().unwrap_or_default(),
            receipt["copiedAt"].as_str().unwrap_or_default(),
        )?;
        Ok(receipt["sourceProjectId"] == source_id
            && receipt["targetProjectId"] == target_id
            && receipt["receiptDigest"] == expected
            && stored_digest == expected)
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
        let lifecycle = CatalogRepository::lifecycle(&catalog, project_id)?;
        match lifecycle.as_deref() {
            Some("ACTIVE") | Some("ARCHIVED") | Some("DELETING") => {}
            _ => return Err(ManagerError::ProjectNotFound),
        }
        if lifecycle.as_deref() == Some("DELETING") {
            if let Ok(path) = self.resolve_registered_project_path(project_id) {
                if path.starts_with(self.paths.trash_dir()) && path.join("project.db").is_file() {
                    return Ok(());
                }
            }
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
    ///   gc receipts, leases, jobs, model telemetry, audit ledgers) rather than
    ///   copying immutable receipts that belong to the source identity;
    /// - rewrites `project_id` on every authoritative long-term table
    ///   (project_meta, artifacts, canonical_heads, books, volumes, arcs,
    ///   chapters, canon, obligations, foreshadows, reader desires, typed
    ///   dependency edges, change/retcon, model budget, projections) to `new_id`;
    /// - resets derived/runtime state (project_runtime_state, leases, checkpoints,
    ///   jobs, model runs) rather than copying a live run that belongs to the
    ///   source identity;
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
            // 1. Drop the source's runtime/audit artifacts, which are identity-
            //    bound and never copied. (Leases/jobs/model telemetry are runtime
            //    state that must not be carried into a copy.)
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
            // 2. Apply the Copy-Project policy for every registered table from the
            //    single-authority copy_policy_registry: REMAP rewrites the copy's
            //    project_id to the new id; CLEAR deletes the source's rows (runtime,
            //    lease, telemetry, and derived projections that must be rebuilt).
            //    Tables the registry does not cover (M0A audit tables) are either
            //    handled explicitly above or carry no project_id, so they are skipped.
            let mut stmt = connection.prepare(
                "SELECT table_name, copy_policy FROM copy_policy_registry ORDER BY table_name",
            )?;
            let policy_rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            drop(stmt);
            for (table, policy) in policy_rows {
                match policy.as_str() {
                    "COPY_REMAP" => {
                        connection.execute(
                            &format!("UPDATE {table} SET project_id = ?1 WHERE project_id = ?2"),
                            [new_id, source_id],
                        )?;
                    }
                    "COPY_CLEAR" => {
                        connection.execute(
                            &format!("DELETE FROM {table} WHERE project_id = ?1"),
                            [source_id],
                        )?;
                    }
                    _ => {}
                }
            }
            connection.execute("UPDATE project_meta SET project_id = ?1, title = ?2, revision = 0, maintenance_state = 'READY', updated_at = ?3 WHERE project_id = ?4", libsql_rusqlite::params![new_id, format!("{source_id} 副本"), now, source_id])?;
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

/// Computes the canonical digest over a copy receipt's stable fields. Both the
/// producer (`write_copy_receipt`) and the verifier (`has_valid_copy_receipt`)
/// must agree on this field set; centralizing it prevents the two sides drifting
/// if the receipt gains a field.
fn copy_receipt_digest(
    receipt_id: &str,
    source_id: &str,
    target_id: &str,
    now: &str,
) -> Result<String, ManagerError> {
    Ok(sha256_canonical(&serde_json::json!({
        "receiptId": receipt_id,
        "sourceProjectId": source_id,
        "targetProjectId": target_id,
        "copiedAt": now,
    }))?)
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
    let receipt_digest = copy_receipt_digest(&receipt_id, source_id, new_id, now)?;
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

fn manager_error_from_commit(error: CommitError) -> ManagerError {
    match error {
        CommitError::ProjectNotFound => ManagerError::ProjectNotFound,
        CommitError::RevisionConflict => ManagerError::RevisionConflict,
        CommitError::IdempotencyConflict => ManagerError::IdempotencyConflict,
        CommitError::ProjectCommandReceiptDigestMismatch => {
            ManagerError::ProjectCommandReceiptDigestMismatch
        }
        CommitError::Sqlite(error) => ManagerError::Sqlite(error),
        CommitError::Json(error) => ManagerError::Json(error),
        other => ManagerError::ProjectCommand(other.to_string()),
    }
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
    #[error("project command: {0}")]
    ProjectCommand(String),
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
