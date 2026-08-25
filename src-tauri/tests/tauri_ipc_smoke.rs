use ai_novel_production_system_core::commands::{
    execute_command_impl, query_project_library_impl, query_project_overview_impl, KernelState,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn tauri_service_path_creates_queries_renames_and_archives_project() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));

    let created = execute_command_impl(
        &state,
        json!({
            "type": "project_create_from_idea",
            "commandId": "create-1",
            "requestedAt": "2026-08-24T00:00:00Z",
            "payload": {"title": "第一部"}
        }),
    )
    .unwrap();
    let project_id = created["projectId"].as_str().unwrap();
    assert_eq!(created["finalStatus"], "committed");

    let library = query_project_library_impl(&state).unwrap();
    assert_eq!(library["projects"].as_array().unwrap().len(), 1);
    assert_eq!(library["projects"][0]["id"], project_id);

    let overview = query_project_overview_impl(
        &state,
        json!({"projectId": project_id, "scope": "overview", "pageSize": 8}),
    )
    .unwrap();
    assert_eq!(overview["project"]["title"], "第一部");
    assert_eq!(overview["project"]["revision"], 0);

    execute_command_impl(
        &state,
        json!({
            "type": "project_rename",
            "commandId": "rename-1",
            "projectId": project_id,
            "requestedAt": "2026-08-24T00:00:01Z",
            "payload": {"title": "第二部"}
        }),
    )
    .unwrap();
    execute_command_impl(
        &state,
        json!({
            "type": "project_archive",
            "commandId": "archive-1",
            "projectId": project_id,
            "requestedAt": "2026-08-24T00:00:02Z"
        }),
    )
    .unwrap();

    let library = query_project_library_impl(&state).unwrap();
    assert_eq!(library["projects"][0]["title"], "第二部");
    assert_eq!(library["projects"][0]["lifecycle"], "ARCHIVED");
}
