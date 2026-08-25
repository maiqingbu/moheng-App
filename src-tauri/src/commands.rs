use crate::catalog::CatalogRepository;
use crate::generated::types::ProjectCommandReceipt;
use crate::hash::sha256_canonical;
use crate::project_db::paths::AppPaths;
use crate::project_db::{ProjectCommandOutcome, ProjectCommandSpec, ProjectManager};
use serde_json::{json, Value};
use tauri::State;

#[derive(Clone)]
pub struct KernelState {
    pub manager: ProjectManager,
    pub paths: AppPaths,
}

impl KernelState {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            manager: ProjectManager::new(paths.clone()),
            paths,
        }
    }
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("IPC_INVALID:{field}"))
}

fn safe_project_id(command_id: &str) -> String {
    let suffix: String = command_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '-'
            }
        })
        .collect();
    format!("project-{suffix}")
}

fn project_snapshot(project_id: &str, title: &str, revision: i64, updated_at: &str) -> Value {
    json!({
        "id": project_id,
        "title": title,
        "subtitle": "本地权威项目",
        "platform": "未设置",
        "genre": "未设置",
        "mode": "DIRECTOR",
        "phase": "SETUP",
        "runState": "IDLE",
        "blockReason": null,
        "signingStatus": "NOT_STARTED",
        "progress": 0,
        "totalCost": 0,
        "totalElapsed": "0 分钟",
        "committedChapters": 0,
        "candidateChapters": 0,
        "latestReceiptId": "",
        "revision": revision,
        "updatedAt": updated_at
    })
}

fn request_digest(request: &Value) -> Result<String, String> {
    sha256_canonical(request).map_err(|error| format!("IPC_INVALID:request:{error}"))
}

fn expected_revision(request: &Value) -> Result<Option<i64>, String> {
    request
        .get("expectedRevision")
        .map(|value| {
            value
                .as_i64()
                .filter(|revision| *revision >= 0)
                .ok_or_else(|| "IPC_INVALID:expectedRevision".to_string())
        })
        .transpose()
}

fn project_command_response(receipt: &ProjectCommandReceipt) -> Value {
    json!({
        "commandId": receipt.command_id,
        "projectId": receipt.result_project_id,
        "acceptedAt": receipt.committed_at,
        "finalStatus": "committed",
        "message": "权威事务已完成，界面将重新查询。",
        "receiptId": receipt.receipt_id,
        "beforeRevision": receipt.before_revision,
        "afterRevision": receipt.after_revision,
        "requestDigest": receipt.request_digest,
    })
}

fn outcome_receipt<T>(outcome: ProjectCommandOutcome<T>) -> ProjectCommandReceipt {
    match outcome {
        ProjectCommandOutcome::Replayed(receipt)
        | ProjectCommandOutcome::Committed { receipt, .. } => receipt,
    }
}

#[allow(clippy::too_many_arguments)]
fn project_command_spec<'a>(
    project_id: &'a str,
    result_project_id: &'a str,
    command_id: &'a str,
    idempotency_key: &'a str,
    command_type: &'a str,
    request_digest: &'a str,
    expected_revision: Option<i64>,
    now: &'a str,
    advance_revision: bool,
) -> ProjectCommandSpec<'a> {
    ProjectCommandSpec {
        project_id,
        result_project_id,
        command_id,
        idempotency_key,
        command_type,
        request_digest,
        expected_revision,
        now,
        advance_revision,
    }
}

fn replayed_project_command(
    state: &KernelState,
    project_id: &str,
    idempotency_key: &str,
    digest: &str,
) -> Result<Option<Value>, String> {
    let receipt = state
        .manager
        .find_project_command_receipt(project_id, idempotency_key)
        .map_err(|error| error.to_string())?;
    match receipt {
        None => Ok(None),
        Some(receipt) if receipt.request_digest == digest => {
            Ok(Some(project_command_response(&receipt)))
        }
        Some(_) => Err("IDEMPOTENCY_CONFLICT".to_string()),
    }
}

pub fn query_project_library_impl(state: &KernelState) -> Result<Value, String> {
    let catalog = CatalogRepository::open(&state.paths, "1970-01-01T00:00:00Z")
        .map_err(|error| error.to_string())?;
    let rows = CatalogRepository::list_projects(&catalog).map_err(|error| error.to_string())?;
    let projects: Vec<Value> = rows
        .into_iter()
        .map(|row| {
            json!({
                "id": row.project_id,
                "title": row.title,
                "subtitle": "本地权威项目",
                "platform": "未设置",
                "genre": "未设置",
                "mode": "DIRECTOR",
                "phase": "SETUP",
                "runState": "IDLE",
                "blockReason": null,
                "signingStatus": "NOT_STARTED",
                "lifecycle": row.lifecycle,
                "progress": 0,
                "totalCost": 0,
                "totalWords": 0,
                "committedChapters": 0,
                "candidateChapters": 0,
                "pendingDecisions": 0,
                "revision": 0,
                "createdAt": row.created_at,
                "updatedAt": row.updated_at,
                "lastOpenedAt": row.last_opened_at.unwrap_or_default()
            })
        })
        .collect();
    let active = projects
        .iter()
        .filter(|project| project["lifecycle"] == "ACTIVE")
        .count();
    let archived = projects.len() - active;
    Ok(json!({
        "projects": projects,
        "totals": {
            "all": active + archived,
            "active": active,
            "archived": archived,
            "producing": 0,
            "waitingDecision": 0,
            "completed": 0,
            "totalCost": 0
        }
    }))
}

pub fn query_project_overview_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let (title, revision, updated_at) = state
        .manager
        .overview(project_id)
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "project": project_snapshot(project_id, &title, revision, &updated_at),
        "pendingDecisionCount": 0,
        "recentChapters": [],
        "recentArtifacts": [],
        "healthSummary": {
            "level": "healthy",
            "label": "权威项目库可用",
            "note": "M0A 存储与完整性检查已通过"
        }
    }))
}

pub fn execute_command_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let command_type = required_string(&request, "type")?;
    let command_id = required_string(&request, "commandId")?;
    let idempotency_key = required_string(&request, "idempotencyKey")?;
    let requested_at = required_string(&request, "requestedAt")?;
    let project_id = request.get("projectId").and_then(Value::as_str);
    let expected_revision = expected_revision(&request)?;
    let digest = request_digest(&request)?;
    match command_type {
        "project_create_from_idea" | "project_create_from_market" => {
            let title = request
                .pointer("/payload/title")
                .and_then(Value::as_str)
                .unwrap_or("未命名作品");
            let id = safe_project_id(command_id);
            if let Some(replayed) = replayed_project_command(state, &id, idempotency_key, &digest)?
            {
                return Ok(replayed);
            }
            drop(
                state
                    .manager
                    .create(&id, title, requested_at)
                    .map_err(|error| error.to_string())?,
            );
            let receipt = outcome_receipt(
                state
                    .manager
                    .commit_project_command(
                        project_command_spec(
                            &id,
                            &id,
                            command_id,
                            idempotency_key,
                            command_type,
                            &digest,
                            expected_revision,
                            requested_at,
                            false,
                        ),
                        |_| Ok(()),
                    )
                    .map_err(|error| error.to_string())?,
            );
            Ok(project_command_response(&receipt))
        }
        "project_rename" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            let title = request
                .pointer("/payload/title")
                .and_then(Value::as_str)
                .ok_or_else(|| "IPC_INVALID:payload.title".to_string())?;
            let outcome = state
                .manager
                .commit_project_rename(
                    project_command_spec(
                        id,
                        id,
                        command_id,
                        idempotency_key,
                        command_type,
                        &digest,
                        expected_revision,
                        requested_at,
                        true,
                    ),
                    title,
                )
                .map_err(|error| error.to_string())?;
            let receipt = match outcome {
                ProjectCommandOutcome::Replayed(receipt) => receipt,
                ProjectCommandOutcome::Committed { receipt, .. } => {
                    let catalog = CatalogRepository::open(&state.paths, requested_at)
                        .map_err(|error| error.to_string())?;
                    CatalogRepository::rename(&catalog, id, title, requested_at)
                        .map_err(|error| error.to_string())?;
                    receipt
                }
            };
            Ok(project_command_response(&receipt))
        }
        "project_archive" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            let outcome = state
                .manager
                .commit_project_command(
                    project_command_spec(
                        id,
                        id,
                        command_id,
                        idempotency_key,
                        command_type,
                        &digest,
                        expected_revision,
                        requested_at,
                        true,
                    ),
                    |_| Ok(()),
                )
                .map_err(|error| error.to_string())?;
            let receipt = match outcome {
                ProjectCommandOutcome::Replayed(receipt) => receipt,
                ProjectCommandOutcome::Committed { receipt, .. } => {
                    state
                        .manager
                        .set_lifecycle(id, "ARCHIVED", requested_at)
                        .map_err(|error| error.to_string())?;
                    receipt
                }
            };
            Ok(project_command_response(&receipt))
        }
        "project_restore" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            let outcome = state
                .manager
                .commit_project_command(
                    project_command_spec(
                        id,
                        id,
                        command_id,
                        idempotency_key,
                        command_type,
                        &digest,
                        expected_revision,
                        requested_at,
                        true,
                    ),
                    |_| Ok(()),
                )
                .map_err(|error| error.to_string())?;
            let receipt = match outcome {
                ProjectCommandOutcome::Replayed(receipt) => receipt,
                ProjectCommandOutcome::Committed { receipt, .. } => {
                    state
                        .manager
                        .set_lifecycle(id, "ACTIVE", requested_at)
                        .map_err(|error| error.to_string())?;
                    receipt
                }
            };
            Ok(project_command_response(&receipt))
        }
        "project_duplicate" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            let target = request
                .pointer("/payload/targetProjectId")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(|value| value.to_string())
                .ok_or_else(|| "IPC_INVALID:payload.targetProjectId".to_string())?;
            if let Some(replayed) = replayed_project_command(state, id, idempotency_key, &digest)? {
                return Ok(replayed);
            }
            if let Some(expected) = expected_revision {
                let current = state
                    .manager
                    .project_revision(id)
                    .map_err(|error| error.to_string())?;
                if current != expected {
                    return Err("REVISION_CONFLICT".to_string());
                }
            }
            state
                .manager
                .duplicate(id, &target, requested_at)
                .map_err(|error| error.to_string())?;
            let result = state.manager.commit_project_command(
                project_command_spec(
                    id,
                    &target,
                    command_id,
                    idempotency_key,
                    command_type,
                    &digest,
                    expected_revision,
                    requested_at,
                    true,
                ),
                |_| Ok(()),
            );
            let receipt = match result {
                Ok(outcome) => outcome_receipt(outcome),
                Err(error) => {
                    let _ = state.manager.delete_to_trash(&target, requested_at);
                    return Err(error.to_string());
                }
            };
            Ok(project_command_response(&receipt))
        }
        "project_delete" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            if let Some(replayed) = replayed_project_command(state, id, idempotency_key, &digest)? {
                return Ok(replayed);
            }
            let outcome = state
                .manager
                .commit_project_command(
                    project_command_spec(
                        id,
                        id,
                        command_id,
                        idempotency_key,
                        command_type,
                        &digest,
                        expected_revision,
                        requested_at,
                        true,
                    ),
                    |_| Ok(()),
                )
                .map_err(|error| error.to_string())?;
            let receipt = match outcome {
                ProjectCommandOutcome::Replayed(receipt) => {
                    return Ok(project_command_response(&receipt));
                }
                ProjectCommandOutcome::Committed { receipt, .. } => receipt,
            };
            // A new delete has to move the now-receipted project after its
            // authoritative project transaction releases the writer.
            state
                .manager
                .delete_to_trash(id, requested_at)
                .map_err(|error| error.to_string())?;
            Ok(project_command_response(&receipt))
        }
        "integrity_check_run" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            let outcome = state
                .manager
                .commit_project_integrity(project_command_spec(
                    id,
                    id,
                    command_id,
                    idempotency_key,
                    command_type,
                    &digest,
                    expected_revision,
                    requested_at,
                    false,
                ))
                .map_err(|error| error.to_string())?;
            let receipt = outcome_receipt(outcome);
            Ok(project_command_response(&receipt))
        }
        _ => Err(format!("IPC_UNSUPPORTED:{command_type}")),
    }
}

#[tauri::command]
pub fn query_project_library(state: State<'_, KernelState>) -> Result<Value, String> {
    query_project_library_impl(&state)
}

#[tauri::command]
pub fn query_project_overview(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_project_overview_impl(&state, request)
}

#[tauri::command]
pub fn execute_command(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    execute_command_impl(&state, request)
}

#[tauri::command]
pub fn query_global_settings() -> Result<Value, String> {
    Ok(json!({
        "provider": "",
        "model": "",
        "endpoint": "",
        "monthlyBudget": 0,
        "chapterBudget": 0,
        "pauseBeforeOverrun": true,
        "privacyMode": "local-first",
        "sendDraftOnly": true,
        "exportFormat": "DOCX",
        "platform": "",
        "theme": "system"
    }))
}

#[tauri::command]
pub fn test_provider(_settings: Value) -> Result<Value, String> {
    Err("IPC_UNSUPPORTED:test_provider".into())
}

#[tauri::command]
pub fn query_market_opportunities() -> Result<Value, String> {
    Ok(json!([]))
}

pub fn handler<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool {
    tauri::generate_handler![
        query_project_library,
        query_project_overview,
        execute_command,
        query_global_settings,
        test_provider,
        query_market_opportunities,
    ]
}
