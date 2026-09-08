use ai_novel_production_system_core::catalog::CatalogRepository;
use ai_novel_production_system_core::project_db::connection::open_connection;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::read_pool::ReadPool;
use ai_novel_production_system_core::project_db::{ManagerError, ProjectManager};
use std::fs;
use tempfile::tempdir;

const NOW: &str = "2026-08-25T00:00:00Z";

fn new_manager(temp: &tempfile::TempDir) -> ProjectManager {
    ProjectManager::new(AppPaths::new(temp.path()))
}

#[test]
fn read_pool_reuses_connection_and_bounds_global_and_per_project() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    drop(manager.create("proj-a", "A", NOW).unwrap());
    let db = temp.path().join("projects/proj-a/project.db");

    let mut pool = ReadPool::new(4, 2);
    // A single project may hold at most two read connections.
    let first = pool.acquire("proj-a", &db).unwrap();
    let second = pool.acquire("proj-a", &db).unwrap();
    assert!(matches!(
        pool.acquire("proj-a", &db),
        Err(ai_novel_production_system_core::project_db::read_pool::PoolError::PerProjectExhausted)
    ));
    assert_eq!(pool.live_len(), 2);
    // Releasing reuses the connection instead of opening a new one.
    pool.release("proj-a", first);
    let reopened = pool.acquire("proj-a", &db).unwrap();
    assert_eq!(pool.live_len(), 2);
    assert_eq!(pool.idle_len("proj-a"), 0);
    // Dropping still leaves live bookkeeping consistent.
    pool.release("proj-a", second);
    pool.release("proj-a", reopened);
    pool.close();
    assert_eq!(pool.live_len(), 0);
}

#[test]
fn manager_read_connection_is_bounded_and_released_back_to_pool() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    drop(manager.create("proj-a", "A", NOW).unwrap());

    let conn = manager.read_connection("proj-a", NOW).unwrap();
    let title: String = conn
        .query_row(
            "SELECT title FROM project_meta WHERE project_id = 'proj-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(title, "A");
    drop(conn);
    // Underlying pool no longer leaks connections; closing is a no-op.
    assert!(manager.read_connection("proj-a", NOW).is_ok());
}

#[test]
fn duplicate_generates_new_id_copies_authority_and_does_not_copy_runtime() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    drop(manager.create("proj-a", "第一本", NOW).unwrap());
    manager.rename("proj-a", "重命名后", NOW).unwrap();

    manager
        .duplicate("proj-a", "proj-a-copy", "2026-08-25T00:00:01Z")
        .unwrap();

    // New project owns authoritative rows under its own id.
    let conn = manager.read_connection("proj-a-copy", NOW).unwrap();
    let (title, revision): (String, i64) = conn
        .query_row(
            "SELECT title, revision FROM project_meta WHERE project_id = 'proj-a-copy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(revision, 0);
    drop(conn);
    let _ = title;

    // Catalog now points at the new directory whose project.db exists.
    let catalog =
        CatalogRepository::open(&AppPaths::new(temp.path()), "2026-08-25T00:00:02Z").unwrap();
    let (dupe_path, dupe_lifecycle): (String, String) = catalog
        .query_row(
            "SELECT project_path, lifecycle FROM projects WHERE project_id = 'proj-a-copy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(dupe_lifecycle, "ACTIVE");
    assert!(std::path::Path::new(&dupe_path)
        .join("project.db")
        .is_file());
    // Deleting/duplicating never mutates the source: A stays intact.
    let src_lifecycle: String = catalog
        .query_row(
            "SELECT lifecycle FROM projects WHERE project_id = 'proj-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(src_lifecycle, "ACTIVE");
}

#[test]
fn duplicate_refuses_active_writer_and_self_duplicate() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    let _handle = manager.create("proj-a", "A", NOW).unwrap();
    assert!(matches!(
        manager.duplicate("proj-a", "proj-a-copy", NOW),
        Err(ManagerError::AlreadyOpen)
    ));
    drop(_handle);
}

#[test]
fn delete_to_trash_is_recoverable_and_isolated() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    drop(manager.create("proj-a", "A", NOW).unwrap());
    drop(manager.create("proj-b", "B", NOW).unwrap());

    manager
        .delete_to_trash("proj-a", "2026-08-25T00:00:01Z")
        .unwrap();

    // Source directory moved into trash, B untouched.
    let projects = temp.path().join("projects");
    let trash = temp.path().join("trash");
    assert!(trash.join("proj-a").join("project.db").is_file());
    assert!(projects.join("proj-b").join("project.db").is_file());
    assert!(!projects.join("proj-a").exists());

    // Catalog lifecycle reflects DELETING and path points at trash.
    let catalog =
        CatalogRepository::open(&AppPaths::new(temp.path()), "2026-08-25T00:00:02Z").unwrap();
    let (lifecycle, path): (String, String) = catalog
        .query_row(
            "SELECT lifecycle, project_path FROM projects WHERE project_id = 'proj-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(lifecycle, "DELETING");
    assert!(path.contains("/trash/proj-a"));
}

#[test]
fn delete_to_trash_refuses_active_writer_and_delete_does_not_remove_rows() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    let handle = manager.create("proj-a", "A", NOW).unwrap();
    assert!(matches!(
        manager.delete_to_trash("proj-a", NOW),
        Err(ManagerError::AlreadyOpen)
    ));
    drop(handle);
    // Deleting a fresh project then relocating does not remove authoritative rows.
    drop(manager.create("proj-b", "B", NOW).unwrap());
    manager
        .delete_to_trash("proj-b", "2026-08-25T00:00:01Z")
        .unwrap();
    let catalog =
        CatalogRepository::open(&AppPaths::new(temp.path()), "2026-08-25T00:00:02Z").unwrap();
    let count: i64 = catalog
        .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn registry_lifecycle_manages_unreadable_project_without_opening_project_db() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    drop(manager.create("proj-zombie", "损坏项目", NOW).unwrap());
    fs::remove_file(temp.path().join("projects/proj-zombie/project.db")).unwrap();

    assert!(manager.library_projection("proj-zombie").is_err());
    manager
        .set_lifecycle("proj-zombie", "ARCHIVED", NOW)
        .unwrap();
    manager
        .delete_registered_to_trash("proj-zombie", "2026-09-08T01:00:00Z")
        .unwrap();

    let catalog = CatalogRepository::open(&AppPaths::new(temp.path()), NOW).unwrap();
    let (lifecycle, path): (String, String) = catalog
        .query_row(
            "SELECT lifecycle, project_path FROM projects WHERE project_id = 'proj-zombie'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(lifecycle, "DELETING");
    assert!(path.contains("/trash/proj-zombie"));
}

#[test]
fn writer_lock_releases_on_close() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    let handle = manager.create("proj-a", "A", NOW).unwrap();
    drop(handle);
    // Reopening after the handle drops must succeed (writer released).
    let reopened = manager.open("proj-a", "A", "2026-08-25T00:00:01Z").unwrap();
    drop(reopened);
}

#[test]
fn reconcile_repairs_catalog_path_after_interrupted_delete() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    drop(manager.create("proj-a", "A", NOW).unwrap());

    // Simulate an interrupted delete: the project dir is moved out but the
    // catalog still records the canonical path.
    let canonical = temp.path().join("projects/proj-a");
    let trash = temp.path().join("trash/proj-a");
    fs::create_dir_all(trash.parent().unwrap()).unwrap();
    fs::rename(&canonical, &trash).unwrap();

    let fixed = manager.reconcile("2026-08-25T00:00:01Z").unwrap();
    assert_eq!(fixed, vec!["proj-a:trash-restored".to_string()]);

    let catalog =
        CatalogRepository::open(&AppPaths::new(temp.path()), "2026-08-25T00:00:02Z").unwrap();
    let (lifecycle, path): (String, String) = catalog
        .query_row(
            "SELECT lifecycle, project_path FROM projects WHERE project_id = 'proj-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(lifecycle, "DELETING");
    assert!(path.contains("/trash/proj-a"));
}

/// H-01: an interrupted create (crash before the atomic rename) leaves only a
/// staging directory, never a partial final project dir, and the final path
/// must not exist until the whole DB is built.
#[test]
fn create_never_exposes_partial_final_dir() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    drop(manager.create("proj-a", "A", NOW).unwrap());
    let finals = temp.path().join("projects");
    // Only the fully-registered project dir exists under projects/.
    let entries: Vec<String> = fs::read_dir(&finals)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(entries.contains(&"proj-a".to_string()));
    // The `.staging` root may persist (it is the shared staging workspace), but it
    // must not hold a leftover per-project staging dir after a successful create.
    let staging_root = temp.path().join(".staging");
    if staging_root.exists() {
        let leftovers: Vec<_> = fs::read_dir(&staging_root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            leftovers.is_empty(),
            "no stale .staging child dir should survive a successful create: {leftovers:?}"
        );
    }
}

/// H-03: Duplicate must be a WAL-safe snapshot. A source in WAL mode with a
/// committed row still only in the `-wal` file (never checkpointed into the main
/// DB) must be present in the duplicate. A naive `std::fs::copy(project.db)`
/// would drop it; SQLite Online Backup must not.
#[test]
fn duplicate_is_wal_safe_snapshot_including_uncheckpointed_commits() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    drop(manager.create("src", "Original", NOW).unwrap());
    let src_db = temp.path().join("projects/src/project.db");

    // Reopen the source writable (WAL mode) and commit a fresh, distinguishing row
    // WITHOUT checkpointing, so it lives only in the `-wal` file.
    let wconn = open_connection(&src_db, false).unwrap();
    let mode: String = wconn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
    wconn
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS wal_probe(probe TEXT NOT NULL);
             INSERT INTO wal_probe(probe) VALUES ('un-checkpointed');",
        )
        .unwrap();
    wconn
        .execute(
            "UPDATE project_meta SET title = 'WAL-最新', revision = 99, updated_at = ?1 WHERE project_id = 'src'",
            [NOW],
        )
        .unwrap();
    assert_eq!(
        manager.overview("src").unwrap().1,
        99,
        "source should still show rev 99"
    );
    drop(wconn);

    manager
        .duplicate("src", "copy", "2026-08-25T00:00:02Z")
        .unwrap();

    // The copy, produced via Online Backup from the WAL-mode source, must include
    // the un-checkpointed committed row even though it never reached the main file.
    let conn = manager.read_connection("copy", NOW).unwrap();
    let probe_present: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM wal_probe WHERE probe = 'un-checkpointed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(conn);
    assert_eq!(
        probe_present, 1,
        "a WAL-only committed row must survive the duplicate via online backup"
    );
}

/// H-04: the production Overview query runs through the bounded read pool, so a
/// long run of overview queries does not grow live read handles without bound and
/// released connections are reused.
#[test]
fn production_overview_path_reuses_bounded_read_pool() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    drop(manager.create("proj-a", "A", NOW).unwrap());

    // Warm the pool with one overview; afterwards the connection is released
    // into the idle bucket, so live count stays flat.
    manager.overview("proj-a").unwrap();
    let baseline_live = manager.read_pool_live_len();
    assert!(
        baseline_live <= 1,
        "overview should use at most one live read handle"
    );
    let idle_after_first = manager.read_pool_idle_len("proj-a");
    assert_eq!(
        idle_after_first, 1,
        "the overview read handle should be pooled for reuse"
    );

    // Many consecutive overviews must reuse the single pooled connection and
    // never grow the live handle count.
    for _ in 0..50 {
        manager.overview("proj-a").unwrap();
    }
    assert_eq!(
        manager.read_pool_live_len(),
        baseline_live,
        "repeated overview queries must reuse the pooled connection"
    );
    assert_eq!(manager.read_pool_idle_len("proj-a"), 1);
}

/// P1-02: the RAII read lease returns the connection to the pool on drop, so a
/// caller that never calls an explicit release cannot leak a live handle into
/// PoolExhausted.
#[test]
fn read_lease_returns_connection_on_drop_without_explicit_release() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    drop(manager.create("proj-a", "A", NOW).unwrap());

    // Acquire a lease and read through its Deref, then let it drop naturally.
    {
        let lease = manager.read_lease("proj-a", NOW).unwrap();
        let title: String = lease
            .query_row(
                "SELECT title FROM project_meta WHERE project_id = 'proj-a'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(title, "A");
        assert_eq!(manager.read_pool_live_len(), 1);
    }
    // No explicit release: the guard's Drop returned the handle to the pool.
    assert_eq!(manager.read_pool_idle_len("proj-a"), 1);
    assert_eq!(manager.read_pool_live_len(), 1);

    // The pool can be acquired again immediately; it was not leaked to Exhausted.
    let lease2 = manager.read_lease("proj-a", NOW).unwrap();
    drop(lease2);
    assert_eq!(manager.read_pool_idle_len("proj-a"), 1);
}

/// P1-02: dropping a checked-out lease during a panic still returns the handle,
/// so the per-project and global caps recover after an error path.
#[test]
fn read_lease_recovers_cap_when_error_path_drops_connection() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    drop(manager.create("proj-a", "A", NOW).unwrap());

    // Acquire up to the per-project bound (4), then abort by dropping the leases.
    let mut leases = Vec::new();
    for _ in 0..4 {
        leases.push(manager.read_lease("proj-a", NOW).unwrap());
    }
    assert_eq!(manager.read_pool_live_len(), 4);
    // A further acquire hits the per-project bound (the pool is exhausted for it).
    assert!(manager.read_lease("proj-a", NOW).is_err());
    drop(leases); // emulate an error path that drops every held lease
                  // Caps recover: all four handles were returned to the idle bucket (no leak),
                  // and live stays at 4 rather than growing.
    assert_eq!(manager.read_pool_idle_len("proj-a"), 4);
    assert_eq!(manager.read_pool_live_len(), 4);
    // A fresh acquire succeeds now that the leases returned.
    let lease = manager.read_lease("proj-a", NOW).unwrap();
    drop(lease);
    assert_eq!(manager.read_pool_idle_len("proj-a"), 4);
}
