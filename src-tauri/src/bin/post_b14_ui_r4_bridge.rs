#![cfg(feature = "e2e-test")]

use ai_novel_production_system_core::commands::{
    execute_command_impl, query_assistant_repair_plan_impl, query_book_health_impl,
    query_canon_impl, query_decisions_impl, query_opening_ready_impl, query_production_center_impl,
    query_project_library_impl, query_project_overview_impl, query_story_directions_impl,
    query_story_setup_impl, query_volume_list_impl, support_session_impl, KernelState,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repositories::ops::SearchService;
use serde_json::{json, Value};
use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

static R5_FAIL_NEXT_DELETE: AtomicBool = AtomicBool::new(false);
static R5_FAIL_NEXT_ASSISTANT_EXECUTE: AtomicBool = AtomicBool::new(false);
static R5_LAST_RECEIPT: Mutex<Option<Value>> = Mutex::new(None);
static R6_TRACE: Mutex<Vec<Value>> = Mutex::new(Vec::new());
static R6_FAIL_NEXT_NOOP: AtomicBool = AtomicBool::new(false);

fn r6_trace_event(
    event: &str,
    layer: &str,
    command: &str,
    capability: &str,
    owner: &str,
    result: &str,
) {
    if let Ok(mut trace) = R6_TRACE.lock() {
        let seq = trace.len() + 1;
        trace.push(json!({
            "seq": seq,
            "layer": layer,
            "event": event,
            "command": command,
            "capability": capability,
            "owner": owner,
            "result": result,
            "timestampOrMonotonicOrder": seq
        }));
    }
}

fn authority_snapshot(state: &KernelState, project_id: &str) -> Result<Value, String> {
    let revision = state
        .manager
        .project_revision(project_id)
        .map_err(|error| error.to_string())?;
    let canon = query_canon_impl(
        state,
        json!({"projectId": project_id, "scope": "canon", "pageSize": 100}),
    )?;
    let canon_count = canon
        .get("facts")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let revision_digest =
        sha256_canonical(&json!({"revision": revision})).map_err(|error| error.to_string())?;
    let canon_digest = sha256_canonical(&canon).map_err(|error| error.to_string())?;
    let projection_current = state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            SearchService::projection_health(connection, project_id).map_err(|error| {
                ai_novel_production_system_core::project_db::ManagerError::ProjectCommand(
                    error.to_string(),
                )
            })
        })
        .map_err(|error| error.to_string())?;
    let projection_digest = sha256_canonical(&json!({"projectionCurrent": projection_current}))
        .map_err(|error| error.to_string())?;
    let fts_settings = state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            SearchService::fts_projection_settings(connection, project_id).map_err(|error| {
                ai_novel_production_system_core::project_db::ManagerError::ProjectCommand(
                    error.to_string(),
                )
            })
        })
        .map_err(|error| error.to_string())?;
    let fts_settings_digest =
        sha256_canonical(&json!({"settings": fts_settings})).map_err(|error| error.to_string())?;
    Ok(json!({
        "domains": [
            {"name":"ProjectRevision","count":1,"digest":revision_digest},
            {"name":"Canon","count":canon_count,"digest":canon_digest},
            {"name":"Projection","count":1,"digest":projection_digest},
            {"name":"FTSProjectionSettings","count":fts_settings.len(),"digest":fts_settings_digest}
        ],
        "revision": revision,
        "source": "formal authority read owners"
    }))
}

fn catalog_snapshot(root: PathBuf) -> Result<Value, String> {
    let catalog_path = root.join("catalog.db");
    if !catalog_path.is_file() {
        let digest =
            sha256_canonical(&json!({"projects": []})).map_err(|error| error.to_string())?;
        return Ok(
            json!({"count": 0, "catalogDigest": digest, "profileExists": false, "readOnly": true}),
        );
    }
    let connection = ai_novel_production_system_core::project_db::connection::open_connection(
        &catalog_path,
        true,
    )
    .map_err(|error| error.to_string())?;
    let projects =
        ai_novel_production_system_core::catalog::CatalogRepository::list_projects(&connection)
            .map_err(|error| error.to_string())?;
    let redacted = projects
        .iter()
        .map(|project| {
            json!({
                "projectIdDigest": sha256_canonical(&json!(project.project_id)).unwrap_or_default(),
                "lifecycle": project.lifecycle,
                "createdAt": project.created_at,
                "updatedAt": project.updated_at
            })
        })
        .collect::<Vec<_>>();
    let digest =
        sha256_canonical(&json!({"projects": redacted})).map_err(|error| error.to_string())?;
    Ok(
        json!({"count": projects.len(), "catalogDigest": digest, "profileExists": true, "readOnly": true}),
    )
}

fn dispatch(state: &KernelState, command: &str, body: Value) -> Result<Value, String> {
    let request = body.get("request").cloned().unwrap_or(body.clone());
    if command == "r6_reset_trace" {
        R6_TRACE
            .lock()
            .map_err(|_| "R6_TRACE_LOCK_POISONED".to_string())?
            .clear();
        return Ok(json!({"reset": true, "testOnly": true}));
    }
    if command == "r6_record_ui_event" {
        let event = request
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or("UI_EVENT");
        r6_trace_event(
            event,
            "ui-test-observer",
            "ui",
            "assistant.repair",
            "ProjectAssistantDock",
            "observed",
        );
        return Ok(json!({"recorded": true, "testOnly": true}));
    }
    if command == "r6_query_trace" {
        return Ok(json!({
            "traceSource": "BRIDGE_RUNTIME_RECORDER",
            "sequence": R6_TRACE.lock().map_err(|_| "R6_TRACE_LOCK_POISONED".to_string())?.clone(),
            "testOnly": true
        }));
    }
    if command == "r6_arm_noop" {
        R6_FAIL_NEXT_NOOP.store(true, Ordering::SeqCst);
        return Ok(json!({"armed": true, "testOnly": true}));
    }
    if command == "r5_arm_library_failure" {
        R5_FAIL_NEXT_DELETE.store(true, Ordering::SeqCst);
        return Ok(json!({"armed": true, "testOnly": true}));
    }
    if command == "r5_arm_assistant_failure" {
        R5_FAIL_NEXT_ASSISTANT_EXECUTE.store(true, Ordering::SeqCst);
        return Ok(json!({"armed": true, "testOnly": true}));
    }
    if command == "query_r5_last_receipt" {
        return R5_LAST_RECEIPT
            .lock()
            .map_err(|_| "R5_RECEIPT_LOCK_POISONED".to_string())?
            .clone()
            .ok_or_else(|| "R5_RECEIPT_NOT_FOUND".to_string());
    }
    if command == "query_r5_authority_snapshot" {
        let project_id = request
            .get("projectId")
            .and_then(Value::as_str)
            .ok_or_else(|| "R5_PROJECT_REQUIRED".to_string())?;
        return authority_snapshot(state, project_id);
    }
    if command == "query_r5_data_isolation" {
        let production_root = env::var_os("MOHENG_R5_PRODUCTION_ROOT")
            .map(PathBuf::from)
            .ok_or_else(|| "R5_PRODUCTION_ROOT_REQUIRED".to_string())?;
        return Ok(json!({
            "test": query_project_library_impl(state)?,
            "production": catalog_snapshot(production_root)?
        }));
    }
    let mut request = request;
    if command == "execute_command"
        && request.get("type").and_then(Value::as_str) == Some("project_delete")
        && R5_FAIL_NEXT_DELETE.swap(false, Ordering::SeqCst)
    {
        request["payload"]["testOnlyFailureInjection"] = json!(true);
    }
    if command == "execute_command"
        && request.get("type").and_then(Value::as_str) == Some("repair_execute")
        && R5_FAIL_NEXT_ASSISTANT_EXECUTE.swap(false, Ordering::SeqCst)
    {
        request["payload"]["testOnlyFailureInjection"] = json!(true);
    }
    if command == "execute_command"
        && request.get("type").and_then(Value::as_str) == Some("repair_execute")
        && R6_FAIL_NEXT_NOOP.swap(false, Ordering::SeqCst)
    {
        request["payload"]["testOnlyNoop"] = json!(true);
    }
    if command == "execute_command"
        && matches!(
            request.get("type").and_then(Value::as_str),
            Some("repair_authorize" | "repair_execute")
        )
    {
        if request.get("type").and_then(Value::as_str) == Some("repair_authorize") {
            r6_trace_event(
                "STORE_AUTHORIZE",
                "store-observer",
                "repair_authorize",
                "assistant.repair.authorize",
                "ProjectStore",
                "observed",
            );
            r6_trace_event(
                "IPC_REPAIR_AUTHORIZE",
                "ipc",
                "repair_authorize",
                "assistant.repair.authorize",
                "H03::RepairAuthorizationOwner",
                "received",
            );
            r6_trace_event(
                "AUTHORIZATION_CAPABILITY_RESOLVED",
                "capability",
                "repair_authorize",
                "assistant.repair.authorize",
                "H03::RepairAuthorizationOwner",
                "resolved",
            );
        } else {
            r6_trace_event(
                "IPC_REPAIR_EXECUTE",
                "ipc",
                "repair_execute",
                "assistant.repair.execute",
                "H03::RepairOwner",
                "received",
            );
        }
        if let Some(delay) = env::var("MOHENG_R5_EXECUTION_DELAY_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
        {
            std::thread::sleep(Duration::from_millis(delay));
        }
    }
    if command == "query_project_overview" {
        if let Some(delay) = env::var("MOHENG_R5_POST_VERIFY_DELAY_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
        {
            std::thread::sleep(Duration::from_millis(delay));
        }
    }
    if matches!(command, "support_session" | "query_assistant_repair_plan") {
        let question = request
            .get("question")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if question.contains("[R4_UNAVAILABLE]") {
            return Err("IPC_UNSUPPORTED:query_assistant_repair_plan".into());
        }
        if question.contains("[R4_BACKEND_ERROR]") {
            return Err("R4_TEST_BACKEND_ERROR".into());
        }
    }
    match command {
        "query_project_library" => query_project_library_impl(state),
        "query_project_overview" => query_project_overview_impl(state, request),
        "query_story_directions" => query_story_directions_impl(state, request),
        "query_story_setup" => query_story_setup_impl(state, request),
        "query_opening_ready" => query_opening_ready_impl(state, request),
        "query_production_center" => query_production_center_impl(state, request),
        "query_decisions" => query_decisions_impl(state, request),
        "query_canon" => query_canon_impl(state, request),
        "query_volume_list" => query_volume_list_impl(state, request),
        "query_reader_volumes" => query_volume_list_impl(state, request),
        "query_book_health" => query_book_health_impl(state, request),
        "query_market_opportunities" => Ok(json!([])),
        "support_session" => support_session_impl(state, request),
        "query_assistant_repair_plan" => query_assistant_repair_plan_impl(state, request),
        "execute_command" => {
            let result = execute_command_impl(state, request.clone());
            if let Err(error) = &result {
                if request
                    .pointer("/payload/testOnlyFailureInjection")
                    .and_then(Value::as_bool)
                    == Some(true)
                {
                    let command_id = request
                        .get("commandId")
                        .and_then(Value::as_str)
                        .unwrap_or("R5_TEST_FAILURE")
                        .to_owned();
                    let project_id = request
                        .get("projectId")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                    return Ok(json!({
                        "commandId": command_id,
                        "projectId": project_id.clone(),
                        "resultProjectId": project_id,
                        "finalStatus": "failed",
                        "message": error,
                        "receiptId": format!("failed:{command_id}"),
                        "testOnlyFailureReceipt": true
                    }));
                }
            }
            if request.get("type").and_then(Value::as_str) == Some("repair_authorize") {
                let outcome = if result.is_ok() { "accepted" } else { "failed" };
                r6_trace_event(
                    "AUTHORIZATION_OWNER_CALLED",
                    "owner",
                    "repair_authorize",
                    "assistant.repair.authorize",
                    "H03::RepairAuthorizationOwner",
                    outcome,
                );
                if result.is_ok() {
                    r6_trace_event(
                        "AUTHORIZATION_ACCEPTED",
                        "owner",
                        "repair_authorize",
                        "assistant.repair.authorize",
                        "H03::RepairAuthorizationOwner",
                        "committed",
                    );
                }
            } else if request.get("type").and_then(Value::as_str) == Some("repair_execute") {
                r6_trace_event(
                    "REPAIR_OWNER_CALLED",
                    "owner",
                    "repair_execute",
                    "assistant.repair.execute",
                    "H03::RepairOwner",
                    if result.is_ok() {
                        "committed"
                    } else {
                        "failed"
                    },
                );
            }
            if let Ok(value) = &result {
                if matches!(
                    request.get("type").and_then(Value::as_str),
                    Some("repair_authorize" | "repair_execute")
                ) {
                    if let Ok(mut receipt) = R5_LAST_RECEIPT.lock() {
                        *receipt = Some(value.clone());
                    }
                }
            }
            result
        }
        other => Err(format!("IPC_UNSUPPORTED:{other}")),
    }
}

fn serve(state: &KernelState, ready: Value) -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string(&ready)?);
    io::stdout().flush()?;
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let envelope: Value = serde_json::from_str(&line)?;
        let command = envelope["command"].as_str().unwrap_or_default();
        let body = envelope.get("payload").cloned().unwrap_or(Value::Null);
        let response = match dispatch(state, command, body) {
            Ok(value) => json!({"ok":true,"value":value}),
            Err(error) => json!({"ok":false,"error":error}),
        };
        println!("{}", serde_json::to_string(&response)?);
        io::stdout().flush()?;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = env::var_os("MOHENG_R4_FIXTURE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join(format!("moheng-r4-{}", std::process::id())));
    std::fs::create_dir_all(&root)?;
    let state = KernelState::new(AppPaths::new(&root));
    let manifest_path = root.join("r5-fixture-ids.json");
    if manifest_path.is_file() {
        let ready: Value = serde_json::from_slice(&std::fs::read(manifest_path)?)?;
        return serve(&state, ready);
    }
    let seed = execute_command_impl(
        &state,
        json!({
            "type":"project_create_from_idea",
            "commandId":"r4-seed-project",
            "idempotencyKey":"r4-seed-project-key",
            "requestedAt":"2026-09-08T00:00:00Z",
            "payload":{"title":"R4 Production E2E Fixture","idea":"用于正式助手生产链路验收的隔离测试作品"}
        }),
    )?;
    let project_id = seed["projectId"].as_str().unwrap_or_default().to_owned();
    let directions = query_story_directions_impl(
        &state,
        json!({"projectId": project_id, "scope":"directions", "pageSize":10}),
    )?;
    let pitch_id = directions["pitches"][0]["id"].as_str().unwrap_or_default();
    execute_command_impl(
        &state,
        json!({
            "type":"pitch_select","commandId":"r4-seed-pitch","idempotencyKey":"r4-seed-pitch-key",
            "projectId":project_id,"expectedRevision":0,"requestedAt":"2026-09-08T00:00:01Z",
            "payload":{"pitchIds":[pitch_id],"mode":"single"}
        }),
    )?;
    let degraded = execute_command_impl(
        &state,
        json!({
            "type":"project_create_from_idea",
            "commandId":"r5-seed-degraded-project",
            "idempotencyKey":"r5-seed-degraded-project-key",
            "requestedAt":"2026-09-08T00:00:03Z",
            "payload":{"title":"R5 无法打开的归档作品","idea":"用于作品库归档生命周期隔离验收"}
        }),
    )?;
    let degraded_id = degraded["projectId"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let degraded_db = root.join("projects").join(&degraded_id).join("project.db");
    if degraded_db.exists() {
        std::fs::remove_file(degraded_db)?;
    }
    let degraded_delete = execute_command_impl(
        &state,
        json!({
            "type":"project_create_from_idea",
            "commandId":"r5-seed-degraded-delete-project",
            "idempotencyKey":"r5-seed-degraded-delete-project-key",
            "requestedAt":"2026-09-08T00:00:03Z",
            "payload":{"title":"R5 无法打开的删除作品","idea":"用于作品库删除与刷新隔离验收"}
        }),
    )?;
    let degraded_delete_id = degraded_delete["projectId"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let degraded_delete_db = root
        .join("projects")
        .join(&degraded_delete_id)
        .join("project.db");
    if degraded_delete_db.exists() {
        std::fs::remove_file(degraded_delete_db)?;
    }
    let failed = execute_command_impl(
        &state,
        json!({
            "type":"project_create_from_idea",
            "commandId":"r5-seed-failure-project",
            "idempotencyKey":"r5-seed-failure-project-key",
            "requestedAt":"2026-09-08T00:00:04Z",
            "payload":{"title":"R5 删除失败测试作品","idea":"用于验证失败不假成功"}
        }),
    )?;
    let failed_id = failed["projectId"].as_str().unwrap_or_default().to_owned();
    let ready = json!({"event":"ready","projectId":project_id,"degradedProjectId":degraded_id,"degradedDeleteProjectId":degraded_delete_id,"failureProjectId":failed_id});
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&ready)?)?;
    execute_command_impl(
        &state,
        json!({
            "type":"story_setup_save","commandId":"r4-seed-story","idempotencyKey":"r4-seed-story-key",
            "projectId":project_id,"expectedRevision":1,"requestedAt":"2026-09-08T00:00:02Z",
            "payload":{"storyCore":{"protagonist":"被逐出宗门的炼丹师","desire":"证明废丹的价值","mechanism":"废丹让妖兽进化并留下代价","conflict":"宗门封锁秘密","readerPromise":"每次升级都伴随选择与代价","forbiddenReveals":[]}}
        }),
    )?;
    serve(&state, ready)
}
