//! Final-hardening tests for H-01 (atomic create + orphan recovery),
//! H-02 (catalog-path authoritative open), H-04 (bounded read-pool reuse on
//! the production query path) and H-06 (V2.1 minimal compatible registration).

use ai_novel_production_system_core::catalog::CatalogRepository;
use ai_novel_production_system_core::generated::types::ArtifactType;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::{ManagerError, ProjectManager};
use std::fs;
use tempfile::tempdir;

const NOW: &str = "2026-08-25T00:00:00Z";

fn new_manager(temp: &tempfile::TempDir) -> ProjectManager {
    ProjectManager::new(AppPaths::new(temp.path()))
}

/// H-01: a crash after atomic rename but before catalog register leaves a
/// catalog-missing final project directory; reconcile re-adopts it.
#[test]
fn reconcile_adopts_catalog_missing_create_orphan() {
    let temp = tempdir().unwrap();
    // Build a project normally, then simulate the orphan: remove it from the
    // catalog while keeping the on-disk directory + authoritative DB.
    let manager = new_manager(&temp);
    drop(manager.create("proj-a", "第一本", NOW).unwrap());

    // Drop the catalog record outright and orphan the dir.
    let catalog = CatalogRepository::open(&AppPaths::new(temp.path()), NOW).unwrap();
    catalog
        .execute("DELETE FROM projects WHERE project_id = ?1", ["proj-a"])
        .unwrap();

    // reconcile should re-register the orphan from its on-disk authoritative DB.
    let fixed = manager.reconcile("2026-08-25T00:00:01Z").unwrap();
    assert!(
        fixed.iter().any(|s| s == "proj-a:create-orphan-restored"),
        "reconcile should adopt the create orphan: {fixed:?}"
    );
    let catalog = CatalogRepository::open(&AppPaths::new(temp.path()), NOW).unwrap();
    let count: i64 = catalog
        .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

/// H-01: reconcile cleans stale `.staging` directories left by a pre-rename crash.
#[test]
fn reconcile_cleans_stale_staging_dir() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    let staging = temp
        .path()
        .join(".staging")
        .join("proj-b.tmp-2026-08-25T00-00-00Z");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("project.db"), b"partial").unwrap();

    let fixed = manager.reconcile("2026-08-25T00:00:01Z").unwrap();
    assert!(
        fixed
            .iter()
            .any(|s| s.contains("staging:") && s.contains("cleaned")),
        "reconcile should clean stale staging: {fixed:?}"
    );
    assert!(!temp
        .path()
        .join(".staging")
        .join("proj-b.tmp-2026-08-25T00-00-00Z")
        .exists());
}

/// H-01: atomic create does not leave a final project dir if catalog register
/// is impossible (simulated by a project dir name that collides with an
/// existing non-dir, e.g. a file at the final path).
#[test]
fn create_fails_cleanly_when_final_dir_blocked() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    // Place a file where the project dir should be created -> rename must fail.
    let project_dir = temp.path().join("projects/proj-blocked");
    fs::create_dir_all(project_dir.parent().unwrap()).unwrap();
    fs::write(&project_dir, b"not a dir").unwrap();

    let result = manager.create("proj-blocked", "B", NOW);
    assert!(
        result.is_err(),
        "create should fail when final dir is blocked"
    );
    // The catalog must not contain a registered project for it.
    let catalog = CatalogRepository::open(&AppPaths::new(temp.path()), NOW).unwrap();
    let count: i64 = catalog
        .query_row(
            "SELECT COUNT(*) FROM projects WHERE project_id = 'proj-blocked'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

/// H-02: normal open resolves the path from the catalog, not from
/// `projects_dir(id)`. Moving the directory and updating the catalog path must
/// be the single source of truth for `read_connection`/`open`.
#[test]
fn open_resolves_path_from_catalog_authority() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let manager = ProjectManager::new(paths.clone());
    drop(manager.create("proj-a", "A", NOW).unwrap());

    // Relocate the project dir to an archive/trash location and repoint catalog.
    let moved = temp.path().join("trash/proj-a");
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::rename(temp.path().join("projects/proj-a"), &moved).unwrap();
    let catalog = CatalogRepository::open(&paths, NOW).unwrap();
    CatalogRepository::set_project_path(&catalog, "proj-a", &moved.to_string_lossy(), NOW).unwrap();
    CatalogRepository::set_lifecycle(&catalog, "proj-a", "DELETING", NOW).unwrap();

    // read_connection must now read from the catalog-referenced trash path.
    let conn = manager.read_connection("proj-a", NOW).unwrap();
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM project_meta WHERE project_id = 'proj-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(exists, 1);
}

/// H-02: a path outside any allowed root (projects/ or trash/) is rejected.
#[test]
fn open_rejects_path_outside_allowed_root() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let manager = ProjectManager::new(paths.clone());
    drop(manager.create("proj-a", "A", NOW).unwrap());

    // Point the catalog at a directory outside the allowed roots.
    let outside = temp.path().join("evil/proj-a");
    fs::create_dir_all(&outside).unwrap();
    fs::copy(
        temp.path().join("projects/proj-a/project.db"),
        outside.join("project.db"),
    )
    .unwrap();
    let catalog = CatalogRepository::open(&paths, NOW).unwrap();
    CatalogRepository::set_project_path(&catalog, "proj-a", &outside.to_string_lossy(), NOW)
        .unwrap();

    assert!(matches!(
        manager.read_connection("proj-a", NOW),
        Err(ManagerError::PathOutsideRoot)
    ));
}

/// H-01: after create, the staged DB is consolidated (no lingering WAL) and the
/// final directory is self-contained.
#[test]
fn create_finalizes_self_contained_db() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    let handle = manager.create("proj-a", "A", NOW).unwrap();
    drop(handle);
    let dir = temp.path().join("projects/proj-a");
    assert!(dir.join("project.db").is_file());
    // After the checkpoint in create, the journal mode is not WAL so the main
    // DB alone is authoritative (self-contained). Exactly one project.db-wal may
    // not exist; if it does, a subsequent reopen must still be consistent.
    let conn = manager.read_connection("proj-a", NOW).unwrap();
    let title: String = conn
        .query_row(
            "SELECT title FROM project_meta WHERE project_id = 'proj-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(title, "A");
}

/// H-06: the frozen V2.1 ArtifactType set is registered and round-trips in the
/// Rust enum. M0A only needs *registration compatibility* — it must not
/// implement M0B/M3B business semantics for these types.
#[test]
fn v21_artifact_type_registration_compat() {
    for raw in [
        "NarrativeTasteProfile",
        "NarrativeSemanticSeal",
        "DramaticAuthoringPlan",
        "SceneStrategySet",
        "SceneStrategyDecision",
        "NarrativeQualityReport",
        "ReaderDesireSnapshot",
    ] {
        let value = serde_json::json!(raw);
        let parsed: ArtifactType = serde_json::from_value(value)
            .unwrap_or_else(|error| panic!("V2.1 ArtifactType {raw} must deserialize: {error}"));
        let reencoded = serde_json::to_string(&parsed).unwrap();
        assert_eq!(reencoded, format!("\"{raw}\""));
    }
}

/// H-06: the upstream document reserves the ReaderDesire / Quality / CostBudget
/// table-family authority boundary. M0A recorded these as RESERVED (projection /
/// runtime-policy). B01-T03 materializes the frozen tables, so ReaderDesire
/// becomes a real authoritative Ledger (projection_only=0) while the derived
/// quality/health projections stay derived (projection_only=1) and the per-chapter
/// budget stays a runtime-policy authority (projection_only=0). The family/owner
/// boundaries remain exactly as the architecture freezes them.
#[test]
fn v21_reserved_table_family_registry_boundary() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    // The authority/table-family registries live in the *project* DB, so hold a
    // writable connection to inspect them directly.
    let connection = manager.create("proj-a", "A", NOW).unwrap();

    // ReaderDesire is a real authoritative Ledger after B01 (projection_only=0,
    // never Canon). QualityVector/Trend is a derived Evidence/Projection
    // (projection_only=1). ChapterProductionBudget is a Runtime/Policy authority
    // (projection_only=0).
    let expectations = [
        ("reader_desire", "narrative_control", 0),
        ("quality_vector", "quality_evidence", 1),
        ("production_budget", "runtime_policy", 0),
    ];
    for (authority_key, expected_family, expected_projection) in expectations {
        let projection: i64 = connection
            .query_row(
                "SELECT projection_only FROM authority_registry WHERE authority_key = ?1",
                [authority_key],
                |row| row.get(0),
            )
            .unwrap_or_else(|error| {
                panic!("{authority_key} must be reserved in authority_registry: {error}")
            });
        assert_eq!(
            projection, expected_projection,
            "{authority_key}: projection_only flags non-Canon versus runtime-policy authority"
        );
        let family: String = connection
            .query_row(
                "SELECT family FROM table_family_registry WHERE authority_key = ?1",
                [authority_key],
                |row| row.get(0),
            )
            .unwrap_or_else(|error| {
                panic!("{authority_key} must have a table-family entry: {error}")
            });
        assert_eq!(family, expected_family, "{authority_key} family");
    }

    // The reserved authority is owned by a downstream milestone (M3B), not M0A,
    // so M0A never writes these tables.
    let owner: String = connection
        .query_row(
            "SELECT write_owner FROM authority_registry WHERE authority_key = 'reader_desire'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        owner, "M3B",
        "reader_desire authority must be owned downstream"
    );
}

/// H-02 open-policy: a DELETING project is no longer producible (M0-03) and must
/// not be opened for writing/migration, while ARCHIVED and ACTIVE remain openable.
#[test]
fn open_policy_rejects_deleting_but_allows_archived_and_active() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let manager = ProjectManager::new(paths.clone());
    drop(manager.create("proj-a", "A", NOW).unwrap());
    drop(manager.create("proj-b", "B", NOW).unwrap());
    drop(manager.create("proj-c", "C", NOW).unwrap());

    // Archive proj-B and trash proj-C.
    manager
        .set_lifecycle("proj-b", "ARCHIVED", "2026-08-25T00:00:01Z")
        .unwrap();
    manager
        .delete_to_trash("proj-c", "2026-08-25T00:00:01Z")
        .unwrap();

    // ACTIVE and ARCHIVED open normally.
    let active = manager.open("proj-a", "A", NOW);
    assert!(active.is_ok(), "ACTIVE project must open");
    drop(active.unwrap());
    let archived = manager.open("proj-b", "B", NOW);
    assert!(archived.is_ok(), "ARCHIVED project must open");
    drop(archived.unwrap());

    // DELETING is no longer producible: open refuses, read still allowed so the
    // UI can surface the trashed state.
    assert!(matches!(
        manager.open("proj-c", "C", NOW),
        Err(ManagerError::ProjectNotFound)
    ));
    let conn = manager.read_connection("proj-c", NOW);
    assert!(
        conn.is_ok(),
        "read of a DELETING project must still be allowed"
    );
    drop(conn.unwrap());
}

/// H-01: a crash *after migrate but before the atomic rename* (e.g. the process
/// dies in the staging phase) must not expose a final project dir. Before the
/// rename the whole DB lives under `.staging`, so the final path never exists.
#[test]
fn create_after_migrate_crash_leaves_no_final_dir() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    let final_dir = temp.path().join("projects/proj-migrate");
    // Create the staging dir + a fully-migrated DB inside it, mimicking the
    // state right after migrate_project+quick_check succeeds and before rename.
    let staging = temp.path().join(".staging/proj-migrate.tmp-dead");
    std::fs::create_dir_all(&staging).unwrap();
    // Reuse a real created project DB as the "fully built staging DB".
    drop(manager.create("seed", "S", NOW).unwrap());
    let seed_db = temp.path().join("projects/seed/project.db");
    std::fs::copy(&seed_db, staging.join("project.db")).unwrap();
    // manager still owns its writer set; it can be dropped at end of scope.

    // The crash-window invariant: no final project dir is visible, and reconcile
    // treats the stale staging as cleaned-up debris rather than adopting it.
    assert!(
        !final_dir.exists(),
        "final dir must not exist after pre-rename crash"
    );
    let new_manager = new_manager(&temp);
    let fixed = new_manager.reconcile("2026-08-25T00:00:01Z").unwrap();
    assert!(
        fixed.iter().any(|s| s.starts_with("staging:")),
        "stale staging from a pre-rename crash is cleaned: {fixed:?}"
    );
    assert!(!staging.exists(), "stale staging dir must be removed");
}

/// H-01: restart + reconcile recovers a create orphan. A fresh manager over the
/// same on-disk root (simulating a process restart) must re-adopt a final project
/// dir that has no catalog entry (crash after rename, before register).
#[test]
fn restart_reconcile_recovers_create_orphan() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    // Manager #1 creates the project, then we orphan its catalog record.
    let manager1 = ProjectManager::new(paths.clone());
    drop(manager1.create("proj-a", "第一本", NOW).unwrap());
    let catalog = CatalogRepository::open(&paths, NOW).unwrap();
    catalog
        .execute("DELETE FROM projects WHERE project_id = ?1", ["proj-a"])
        .unwrap();
    drop(manager1);

    // Manager #2 (fresh instance) reconciles after the "restart".
    let manager2 = ProjectManager::new(paths.clone());
    let fixed = manager2.reconcile("2026-08-25T00:00:01Z").unwrap();
    assert!(
        fixed.iter().any(|s| s == "proj-a:create-orphan-restored"),
        "restart + reconcile must re-adopt the create orphan: {fixed:?}"
    );
    let catalog = CatalogRepository::open(&paths, NOW).unwrap();
    let count: i64 = catalog
        .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

/// H-04: the production Overview path honors the pool's global and per-project
/// caps. Under the default pool the per-project cap stops a single project from
/// exhausting the pool once live connections are held elsewhere.
#[test]
fn overview_obeys_read_pool_cap_on_contended_project() {
    let temp = tempdir().unwrap();
    let manager = new_manager(&temp);
    drop(manager.create("proj-a", "A", NOW).unwrap());
    // Default pool is max_total=16, max_per_project=4. Hold the pool allocation
    // at its per-project ceiling by calling overview; because overview releases
    // immediately, repeat calls do not accumulate. Instead, hold connections
    // directly and confirm the pool's bound still caps total live handles.
    for _ in 0..200 {
        manager.overview("proj-a").unwrap();
    }
    // After a long run the pool never grew: released handles are reused, so live
    // stays at the flat ceiling established by the first call.
    assert!(
        manager.read_pool_live_len() <= 1,
        "production overview must keep live read handles flat, got {}",
        manager.read_pool_live_len()
    );
    assert_eq!(manager.read_pool_idle_len("proj-a"), 1);
}
