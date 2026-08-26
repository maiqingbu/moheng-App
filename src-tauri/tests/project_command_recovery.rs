use ai_novel_production_system_core::catalog::CatalogRepository;
use ai_novel_production_system_core::commands::{execute_command_impl, KernelState};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};
use serde_json::{json, Value};
use tempfile::tempdir;

const NOW: &str = "2026-08-26T00:00:00Z";

fn create_request(command_id: &str, key: &str, title: &str) -> Value {
    json!({
        "type": "project_create_from_idea",
        "commandId": command_id,
        "idempotencyKey": key,
        "expectedRevision": 0,
        "requestedAt": NOW,
        "payload": {"title": title}
    })
}

#[test]
fn create_retry_recovers_project_created_before_receipt() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let project_id =
        StableIdentityFactory::from_command(StableIdKind::Project, "create-recovery").unwrap();
    drop(state.manager.create(&project_id, "恢复创建", NOW).unwrap());

    let receipt = execute_command_impl(
        &state,
        create_request("create-recovery", "create-recovery-key", "恢复创建"),
    )
    .unwrap();
    assert_eq!(receipt["projectId"], project_id);
    assert!(receipt["receiptId"].as_str().is_some());
}

#[test]
fn archive_replay_repairs_catalog_projection() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let state = KernelState::new(paths.clone());
    let created =
        execute_command_impl(&state, create_request("archive-src", "create-key", "A")).unwrap();
    let project_id = created["projectId"].as_str().unwrap();
    let request = json!({
        "type": "project_archive",
        "commandId": "archive-command",
        "idempotencyKey": "archive-key",
        "projectId": project_id,
        "expectedRevision": 0,
        "requestedAt": "2026-08-26T00:00:01Z",
        "payload": {}
    });
    execute_command_impl(&state, request.clone()).unwrap();

    let catalog = CatalogRepository::open(&paths, "2026-08-26T00:00:02Z").unwrap();
    CatalogRepository::set_lifecycle(&catalog, project_id, "ACTIVE", "2026-08-26T00:00:02Z")
        .unwrap();
    drop(catalog);

    execute_command_impl(&state, request).unwrap();
    let catalog = CatalogRepository::open(&paths, "2026-08-26T00:00:03Z").unwrap();
    assert_eq!(
        CatalogRepository::lifecycle(&catalog, project_id)
            .unwrap()
            .as_deref(),
        Some("ARCHIVED")
    );
}

#[test]
fn duplicate_retry_adopts_copy_created_before_source_receipt() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let created =
        execute_command_impl(&state, create_request("dupe-src", "create-key", "A")).unwrap();
    let source_id = created["projectId"].as_str().unwrap();
    let copied_id =
        StableIdentityFactory::from_command(StableIdKind::Project, "duplicate-command").unwrap();
    state
        .manager
        .duplicate(source_id, &copied_id, "2026-08-26T00:00:01Z")
        .unwrap();

    let receipt = execute_command_impl(
        &state,
        json!({
            "type": "project_duplicate",
            "commandId": "duplicate-command",
            "idempotencyKey": "duplicate-key",
            "projectId": source_id,
            "expectedRevision": 0,
            "requestedAt": "2026-08-26T00:00:01Z"
        }),
    )
    .unwrap();
    // B01-P0 §六: the receipt must distinguish source from result. `projectId` is
    // the source (dupe-src) the command acted on; `resultProjectId` is the copy.
    assert_eq!(receipt["projectId"], source_id);
    assert_eq!(receipt["resultProjectId"], copied_id);
}

#[test]
fn delete_replay_completes_interrupted_filesystem_projection() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let state = KernelState::new(paths.clone());
    let created =
        execute_command_impl(&state, create_request("delete-src", "create-key", "A")).unwrap();
    let project_id = created["projectId"].as_str().unwrap();
    let request = json!({
        "type": "project_delete",
        "commandId": "delete-command",
        "idempotencyKey": "delete-key",
        "projectId": project_id,
        "expectedRevision": 0,
        "requestedAt": "2026-08-26T00:00:01Z",
        "payload": {}
    });
    execute_command_impl(&state, request.clone()).unwrap();

    let trash = paths.trash_dir().join(project_id);
    let canonical = paths.project_dir(project_id).unwrap();
    std::fs::create_dir_all(paths.projects_dir()).unwrap();
    std::fs::rename(&trash, &canonical).unwrap();
    let catalog = CatalogRepository::open(&paths, "2026-08-26T00:00:02Z").unwrap();
    CatalogRepository::set_project_path(
        &catalog,
        project_id,
        &canonical.to_string_lossy(),
        "2026-08-26T00:00:02Z",
    )
    .unwrap();
    drop(catalog);

    execute_command_impl(&state, request).unwrap();
    assert!(paths
        .trash_dir()
        .join(project_id)
        .join("project.db")
        .is_file());
    assert!(!canonical.exists());
}
