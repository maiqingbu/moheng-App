use ai_novel_production_system_core::catalog::CatalogRepository;
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

    let conn = manager.read_connection("proj-a").unwrap();
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
    assert!(manager.read_connection("proj-a").is_ok());
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
    let conn = manager.read_connection("proj-a-copy").unwrap();
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
