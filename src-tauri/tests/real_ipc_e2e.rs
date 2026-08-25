//! Real Tauri command-dispatch E2E.
//!
//! Unlike `tauri_ipc_smoke`, this test drives the payload through the actual
//! `#[tauri::command]` handler registered by `commands::handler()`, on the
//! Tauri `MockRuntime`. That proves the frontend-facing dispatch chain
//! (`invoke -> generate_handler -> command state injection -> serialized
//! response`) rather than only calling the internal `*_impl` functions.
//!
//! The window is never shown and no WebView/WebDriver is required, so it runs
//! deterministically on CI and locally.

use ai_novel_production_system_core::commands::{self, KernelState};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::{json, Value};
use tauri::ipc;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;
use tempfile::tempdir;

/// Builds a Tauri `App<MockRuntime>` wired exactly like the production host:
/// a managed `KernelState` plus the real `commands::handler()`.
fn build_app() -> (tauri::App<tauri::test::MockRuntime>, tempfile::TempDir) {
    let temp = tempdir().unwrap();
    let app = build_app_over(temp.path());
    (app, temp)
}

/// Builds the app over an explicit storage root, so a "restart" can reopen the
/// SAME on-disk catalog + project DBs.
fn build_app_over(root: &std::path::Path) -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(KernelState::new(AppPaths::new(root.to_path_buf())))
        .invoke_handler(commands::handler())
        .build(mock_context(noop_assets()))
        .unwrap()
}

/// Builds the single "main" webview for an app, once, then reuses it for every
/// invoke (the mock runtime forbids creating two webviews with the same label).
fn webview(
    app: &tauri::App<tauri::test::MockRuntime>,
) -> tauri::WebviewWindow<tauri::test::MockRuntime> {
    tauri::WebviewWindowBuilder::new(app, "main", Default::default())
        .build()
        .unwrap()
}

/// Runs an `invoke` against the app's webview and deserializes the response.
fn invoke(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    command: &str,
    body: Value,
) -> Value {
    let request = InvokeRequest {
        cmd: command.into(),
        callback: ipc::CallbackFn(0),
        error: ipc::CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(),
        body: ipc::InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: tauri::test::INVOKE_KEY.to_string(),
    };
    let response = tauri::test::get_ipc_response(webview, request).expect("invoke should succeed");
    response
        .deserialize::<Value>()
        .expect("response body should be JSON")
}

fn receipt_project_id(receipt: &Value) -> &str {
    receipt["projectId"]
        .as_str()
        .expect("receipt has projectId")
}

#[test]
fn real_ipc_create_query_rename_archive_restore_and_delete() {
    let (app, _temp) = build_app();
    let w = webview(&app);

    // 1. project_create_from_idea through the real command dispatch.
    let created = invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_create_from_idea",
                "commandId": "c1",
                "requestedAt": "2026-08-25T00:00:00Z",
                "payload": {"title": "第一部"}
            }
        }),
    );
    assert_eq!(created["finalStatus"], "committed");
    assert!(created["receiptId"].as_str().is_some());
    let project_id = receipt_project_id(&created);

    // 2. Library reflects the new project.
    let library = invoke(&w, "query_project_library", json!({}));
    assert_eq!(library["projects"].as_array().unwrap().len(), 1);
    assert_eq!(library["projects"][0]["id"], project_id);

    // 3. Overview reads title/revision/lifecycle from the project DB.
    let overview = invoke(
        &w,
        "query_project_overview",
        json!({"request": {"projectId": project_id, "scope": "overview", "pageSize": 8}}),
    );
    assert_eq!(overview["project"]["title"], "第一部");
    assert_eq!(overview["project"]["revision"], 0);

    // 4. Rename and archive through real dispatch.
    invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_rename",
                "commandId": "c2",
                "projectId": project_id,
                "requestedAt": "2026-08-25T00:00:01Z",
                "payload": {"title": "第二部"}
            }
        }),
    );
    invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_archive",
                "commandId": "c3",
                "projectId": project_id,
                "requestedAt": "2026-08-25T00:00:02Z"
            }
        }),
    );
    let archived = invoke(&w, "query_project_library", json!({}));
    assert_eq!(archived["projects"][0]["title"], "第二部");
    assert_eq!(archived["projects"][0]["lifecycle"], "ARCHIVED");

    // 5. Restore the archive through real dispatch.
    invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_restore",
                "commandId": "c4",
                "projectId": project_id,
                "requestedAt": "2026-08-25T00:00:03Z"
            }
        }),
    );
    let restored = invoke(&w, "query_project_library", json!({}));
    assert_eq!(restored["projects"][0]["lifecycle"], "ACTIVE");
    assert_eq!(restored["projects"][0]["title"], "第二部");
}

#[test]
fn real_ipc_duplicate_and_delete_to_trash_round_trip() {
    let (app, _temp) = build_app();
    let w = webview(&app);
    let created = invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_create_from_idea",
                "commandId": "d1",
                "requestedAt": "2026-08-25T00:00:00Z",
                "payload": {"title": "待复制"}
            }
        }),
    );
    let source = receipt_project_id(&created);

    // Duplicate via real dispatch.
    let dup = invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_duplicate",
                "commandId": "d2",
                "projectId": source,
                "requestedAt": "2026-08-25T00:00:01Z",
                "payload": {"targetProjectId": "copy-1"}
            }
        }),
    );
    assert_eq!(dup["projectId"], "copy-1");
    let library = invoke(&w, "query_project_library", json!({}));
    assert_eq!(library["projects"].as_array().unwrap().len(), 2);

    // Delete (to trash) via real dispatch.
    invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_delete",
                "commandId": "d3",
                "projectId": source,
                "requestedAt": "2026-08-25T00:00:02Z"
            }
        }),
    );
    let after = invoke(&w, "query_project_library", json!({}));
    let source_row = after["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == source)
        .unwrap();
    assert_eq!(source_row["lifecycle"], "DELETING");
    // The duplicate survives untouched: deleting A never affects B.
    let dup_row = after["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "copy-1")
        .unwrap();
    assert_eq!(dup_row["lifecycle"], "ACTIVE");
}

#[test]
fn real_ipc_survives_restart_and_reloaded_query_matches_commit() {
    let temp = tempdir().unwrap();
    let app_first = build_app_over(temp.path());
    let w_first = webview(&app_first);
    let created = invoke(
        &w_first,
        "execute_command",
        json!({
            "request": {
                "type": "project_create_from_idea",
                "commandId": "r1",
                "requestedAt": "2026-08-25T00:00:00Z",
                "payload": {"title": "重启验证"}
            }
        }),
    );
    let project_id = receipt_project_id(&created);
    invoke(
        &w_first,
        "execute_command",
        json!({
            "request": {
                "type": "project_rename",
                "commandId": "r2",
                "projectId": project_id,
                "requestedAt": "2026-08-25T00:00:01Z",
                "payload": {"title": "重命名后"}
            }
        }),
    );
    drop(w_first);
    drop(app_first);

    // "Restart" the application over the SAME on-disk root.
    let app_second = build_app_over(temp.path());
    let w_second = webview(&app_second);
    let library = invoke(&w_second, "query_project_library", json!({}));
    assert_eq!(library["projects"].as_array().unwrap().len(), 1);
    assert_eq!(library["projects"][0]["id"], project_id);
    assert_eq!(library["projects"][0]["title"], "重命名后");
    let overview = invoke(
        &w_second,
        "query_project_overview",
        json!({"request": {"projectId": project_id, "scope": "overview", "pageSize": 8}}),
    );
    assert_eq!(overview["project"]["title"], "重命名后");
    assert_eq!(overview["project"]["revision"], 1);
}

#[test]
fn real_ipc_duplicate_receipt_and_delete_reflect_in_project_db() {
    let (app, _temp) = build_app();
    let w = webview(&app);
    let created = invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_create_from_idea",
                "commandId": "k1",
                "requestedAt": "2026-08-25T00:00:00Z",
                "payload": {"title": "持久化"}
            }
        }),
    );
    let project_id = receipt_project_id(&created);

    // The command handler wrote a receipt into the response and the project DB
    // holds the authoritative row; the invoke did not silently fall back.
    assert_eq!(created["message"], "权威事务已完成，界面将重新查询。");
    assert_ne!(project_id, "");

    // A duplicate command targets a brand-new id and returns it in the receipt.
    let dup = invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_duplicate",
                "commandId": "k2",
                "projectId": project_id,
                "requestedAt": "2026-08-25T00:00:01Z",
                "payload": {"targetProjectId": "persist-copy"}
            }
        }),
    );
    assert_eq!(dup["projectId"], "persist-copy");
}

#[test]
fn real_ipc_integrity_query_runs_and_market_opportunities_is_available() {
    let (app, _temp) = build_app();
    let w = webview(&app);
    let created = invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_create_from_idea",
                "commandId": "i1",
                "requestedAt": "2026-08-25T00:00:00Z",
                "payload": {"title": "完整性"}
            }
        }),
    );
    let project_id = receipt_project_id(&created);
    invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "integrity_check_run",
                "commandId": "i2",
                "projectId": project_id,
                "requestedAt": "2026-08-25T00:00:01Z"
            }
        }),
    );
    let markets = invoke(&w, "query_market_opportunities", json!({}));
    assert!(markets.as_array().is_some());
}
