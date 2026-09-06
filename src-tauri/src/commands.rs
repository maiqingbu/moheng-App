use crate::catalog::CatalogRepository;
use crate::commit::CommitService;
use crate::generated::types::ProjectCommandReceipt;
use crate::generated::types::{ArtifactEnvelope, CommitBundle};
use crate::harness::{
    grounded_finding, plan_support, registry_digest_map, resolve_project_scope,
    BoundedCapabilityExecutor, CapabilityCallEnvelope, CapabilityRegistry, DiagnosticEngine,
    EvidenceLedgerEntry, ReadCapabilityAdapter, RequestedBounds, SupportCheckpoint,
    SupportSessionReceipt,
};
use crate::hash::sha256_canonical;
use crate::narrative::{
    build_continuity_threads, build_volume_progress, compile_context_manifest, evaluate_plan_stale,
    CurrentNarrativeStateView, PlanStaleStatus, ResumeContext, RuntimeBounds, VolumeProgressInput,
};
use crate::project_db::paths::AppPaths;
use crate::project_db::{ManagerError, ProjectCommandOutcome, ProjectCommandSpec, ProjectManager};
use crate::provider::ProviderAdapter;
use crate::repositories::ops::{chapter_page, single_chapter, volume_page, SearchService};
use crate::runtime::{pending_decision_count, DurableJobService, JobSpec, SecretStore};
use crate::stable_id::{StableIdKind, StableIdentityFactory};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;

#[derive(Clone)]
pub struct KernelState {
    pub manager: ProjectManager,
    pub paths: AppPaths,
    pub(crate) production_gateway: Arc<dyn crate::production::ProductionModelGateway>,
}

impl KernelState {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            manager: ProjectManager::new(paths.clone()),
            paths: paths.clone(),
            production_gateway: Arc::new(
                crate::production::ConfiguredProductionGateway::for_desktop(paths.clone()),
            ),
        }
    }

    pub fn with_production_gateway(
        paths: AppPaths,
        production_gateway: Arc<dyn crate::production::ProductionModelGateway>,
    ) -> Self {
        Self {
            manager: ProjectManager::new(paths.clone()),
            paths,
            production_gateway,
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

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default()
}

fn stable_project_id(command_id: &str) -> Result<String, String> {
    StableIdentityFactory::from_command(StableIdKind::Project, command_id)
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

/// The catalog-derived fields shared by every project-library row, returned as a
/// JSON object. Runtime/projection fields (phase, revision, progress, …) are NOT
/// included here — they differ between the healthy and degraded branches, so the
/// caller overlays them. Keeping the catalog fields in one place prevents a
/// healthy/degrated drift when the library adds a field (B01 修订 P0 §七).
fn library_catalog_fields(row: &crate::catalog::ProjectCatalogRow) -> Value {
    json!({
        "id": row.project_id,
        "title": row.title,
        "subtitle": "本地权威项目",
        "platform": "未设置",
        "genre": "未设置",
        "mode": "DIRECTOR",
        "lifecycle": row.lifecycle,
        "createdAt": row.created_at,
        "updatedAt": row.updated_at,
        "lastOpenedAt": row.last_opened_at.clone().unwrap_or_default(),
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
        // `projectId` is the *source* project the command acted on; `resultProjectId`
        // is the project the command produced (equal for in-place commands, the new
        // copy for duplicate). Consumers must distinguish them (B01-P0 §六): a
        // duplicate A→C must never be mistaken for an A-only receipt.
        "projectId": receipt.project_id,
        "resultProjectId": receipt.result_project_id,
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
    let mut projects: Vec<Value> = Vec::with_capacity(rows.len());
    let mut producing = 0_i64;
    let mut waiting_decision = 0_i64;
    let mut completed = 0_i64;
    let mut total_cost = 0_i64;
    for row in rows {
        // B01-T07/T15: read the project's authoritative revision + rebuildable
        // runtime summary through the controlled read path, instead of emitting a
        // hard-coded revision 0 / idle state. A project whose runtime_state row is
        // missing keeps conservative defaults but still carries a truthful revision
        // (the library is not a second authority, only a projection).
        //
        // A single unreadable project (e.g. a trashed project whose DB was
        // externally removed) must NOT become a fabricated, operational revision=0
        // entry: library_projection returns the real error, and the entry is marked
        // operational=false with a readError. Such a project is not counted as
        // producing/waiting/completed and is not usable for a destructive command.
        let projection = match state.manager.library_projection(&row.project_id) {
            Ok(projection) => projection,
            Err(error) => {
                // A degraded entry is NOT an operational revision=0 project: it
                // carries operational=false plus the real read error, and its
                // revision/generatedRevision are null (unreadable) rather than a
                // fabricated 0. Such a project is not counted as producing/…
                // and is not usable for a destructive command.
                let mut entry = library_catalog_fields(&row);
                entry["phase"] = json!("SETUP");
                entry["runState"] = json!("IDLE");
                entry["blockReason"] = Value::Null;
                entry["signingStatus"] = json!("NOT_STARTED");
                entry["progress"] = json!(0);
                entry["totalCost"] = json!(0);
                entry["totalWords"] = json!(0);
                entry["committedChapters"] = json!(0);
                entry["candidateChapters"] = json!(0);
                entry["pendingDecisions"] = json!(0);
                entry["revision"] = Value::Null;
                entry["generatedRevision"] = Value::Null;
                entry["operational"] = json!(false);
                entry["readError"] = json!(error.to_string());
                projects.push(entry);
                continue;
            }
        };
        let revision = projection.revision;
        // A fresh M0B project has no runtime-state row yet; LibraryRuntimeProjection
        // Default carries truthful defaults so a library row never presents a
        // fabricated revision/state or leaks a signed/committed count it does not own.
        let runtime = projection.runtime.unwrap_or_default();
        let pending_decisions: i64 = state
            .manager
            .with_read_connection(&row.project_id, &row.updated_at, |connection| {
                pending_decision_count(connection, &row.project_id)
                    .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
            })
            .unwrap_or(0);
        let (
            phase,
            run_state,
            block_reason,
            signing_status,
            committed,
            candidate,
            words,
            cost_cents,
            progress,
            generated_revision,
        ) = (
            runtime.current_phase,
            runtime.run_state,
            runtime.block_reason,
            runtime.signing_status,
            runtime.committed_chapters,
            runtime.candidate_chapters,
            runtime.total_words,
            runtime.total_cost_cents,
            runtime.progress,
            runtime.generated_revision,
        );
        match run_state.as_str() {
            "ACTIVE" => producing += 1,
            "WAITING_DECISION" => waiting_decision += 1,
            "COMPLETED" => completed += 1,
            _ => {}
        }
        total_cost += cost_cents;
        let mut entry = library_catalog_fields(&row);
        entry["phase"] = json!(phase);
        entry["runState"] = json!(run_state);
        entry["blockReason"] = block_reason.map(Value::String).unwrap_or(Value::Null);
        entry["signingStatus"] = json!(signing_status);
        entry["progress"] = json!(progress);
        entry["totalCost"] = json!(cost_cents);
        entry["totalWords"] = json!(words);
        entry["committedChapters"] = json!(committed);
        entry["candidateChapters"] = json!(candidate);
        entry["pendingDecisions"] = json!(pending_decisions);
        entry["revision"] = json!(revision);
        entry["generatedRevision"] = json!(generated_revision);
        entry["operational"] = json!(true);
        projects.push(entry);
    }
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
            "producing": producing,
            "waitingDecision": waiting_decision,
            "completed": completed,
            "totalCost": total_cost
        }
    }))
}

pub fn query_project_overview_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let (title, revision, updated_at) = state
        .manager
        .overview(project_id)
        .map_err(|error| error.to_string())?;
    let runtime = state
        .manager
        .library_projection(project_id)
        .map_err(|error| error.to_string())?
        .runtime
        .unwrap_or_default();
    let pending_decisions: i64 = state
        .manager
        .with_read_connection(project_id, &updated_at, |connection| {
            pending_decision_count(connection, project_id)
                .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
        })
        .map_err(|error| error.to_string())?;
    let mut project = project_snapshot(project_id, &title, revision, &updated_at);
    project["phase"] = json!(runtime.current_phase);
    project["runState"] = json!(runtime.run_state);
    project["blockReason"] = runtime
        .block_reason
        .map(Value::String)
        .unwrap_or(Value::Null);
    project["committedChapters"] = json!(runtime.committed_chapters);
    project["candidateChapters"] = json!(runtime.candidate_chapters);
    project["totalCost"] = json!(runtime.total_cost_cents as f64 / 100.0);
    project["progress"] = json!(runtime.progress);
    Ok(json!({
        "project": project,
        "pendingDecisionCount": pending_decisions,
        "recentChapters": [],
        "recentArtifacts": [],
        "healthSummary": {
            "level": "healthy",
            "label": "权威项目库可用",
            "note": "M0A 存储与完整性检查已通过"
        }
    }))
}

pub fn query_market_strategy_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let request = request.get("request").cloned().unwrap_or(request);
    let project_id = required_string(&request, "projectId")?;
    let cursor = request.get("cursor").and_then(Value::as_str);
    let limit = request
        .get("pageSize")
        .and_then(Value::as_i64)
        .unwrap_or(20);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::project_entry::query_market_strategy(connection, project_id, cursor, limit)
                .map_err(ManagerError::ProjectCommand)
        })
        .map_err(|error| error.to_string())
}

pub fn query_story_directions_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let request = request.get("request").cloned().unwrap_or(request);
    let project_id = required_string(&request, "projectId")?;
    let cursor = request.get("cursor").and_then(Value::as_str);
    let limit = request
        .get("pageSize")
        .and_then(Value::as_i64)
        .unwrap_or(10);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::project_entry::query_directions(connection, project_id, cursor, limit)
                .map_err(ManagerError::ProjectCommand)
        })
        .map_err(|error| error.to_string())
}

pub fn query_story_setup_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let request = request.get("request").cloned().unwrap_or(request);
    let project_id = required_string(&request, "projectId")?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::project_entry::query_story_setup(connection, project_id)
                .map_err(ManagerError::ProjectCommand)
        })
        .map_err(|error| error.to_string())
}

pub fn query_opening_ready_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let request = request.get("request").cloned().unwrap_or(request);
    let project_id = required_string(&request, "projectId")?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::project_entry::query_opening_ready(connection, project_id)
                .map_err(ManagerError::ProjectCommand)
        })
        .map_err(|error| error.to_string())
}

pub fn query_assistant_repair_plan_impl(
    state: &KernelState,
    request: Value,
) -> Result<Value, String> {
    let request = request.get("request").cloned().unwrap_or(request);
    let project_id = required_string(&request, "projectId")?;
    let question = required_string(&request, "question")?;
    let now = request
        .get("requestedAt")
        .and_then(Value::as_str)
        .unwrap_or("2026-01-01T00:00:00Z");
    state
        .manager
        .with_read_connection(project_id, now, |connection| {
            crate::project_entry::assistant_repair_plan(connection, project_id, question, now)
                .map_err(ManagerError::ProjectCommand)
        })
        .map_err(|error| error.to_string())
}

/// B08 production projection.  This is a bounded read projection assembled
/// from the authoritative job/artifact/runtime rows; it never reconstructs a
/// candidate as a committed chapter and never trusts Vue-local state.
pub fn query_production_center_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?.to_string();
    state
        .manager
        .with_read_connection(&project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::repositories::production::ProductionRepository::center_projection(
                connection,
                &project_id,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
        })
        .map_err(|error| error.to_string())
}

pub fn query_search_fulltext_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let text = required_string(&request, "text")?;
    let cursor = request.get("cursor").and_then(Value::as_str);
    let limit = request
        .get("pageSize")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(30);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            SearchService::search(connection, project_id, text, cursor, limit)
                .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
                .and_then(|page| {
                    serde_json::to_value(page)
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
                })
        })
        .map_err(|error| error.to_string())
}

pub fn query_volume_list_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let cursor = request.get("cursor").and_then(Value::as_str);
    let limit = request
        .get("pageSize")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(30);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            volume_page(connection, project_id, cursor, limit)
                .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
                .and_then(|page| {
                    serde_json::to_value(page)
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
                })
        })
        .map_err(|error| error.to_string())
}

pub fn query_chapter_list_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let volume_id = required_string(&request, "volumeId")?;
    let cursor = request.get("cursor").and_then(Value::as_str);
    let limit = request
        .get("pageSize")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(30);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            chapter_page(connection, project_id, volume_id, cursor, limit)
                .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
                .and_then(|page| {
                    serde_json::to_value(page)
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
                })
        })
        .map_err(|error| error.to_string())
}

pub fn query_single_chapter_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let chapter_id = required_string(&request, "chapterId")?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            single_chapter(connection, project_id, chapter_id)
                .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?
                .ok_or_else(|| ManagerError::ProjectNotFound)
        })
        .map_err(|error| error.to_string())
}

pub fn query_volume_detail_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let volume_id = required_string(&request, "volumeId")?;
    let cursor = request.get("cursor").and_then(Value::as_str);
    let limit = request
        .get("pageSize")
        .and_then(Value::as_u64)
        .unwrap_or(30);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::repositories::ui_projection::volume_detail(
                connection, project_id, volume_id, cursor, limit,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
        })
        .map_err(|error| error.to_string())
}

pub fn query_decisions_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let status = request.get("status").and_then(Value::as_str);
    let cursor = request.get("cursor").and_then(Value::as_str);
    let limit = request
        .get("pageSize")
        .and_then(Value::as_u64)
        .unwrap_or(50);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::repositories::ui_projection::decisions(
                connection, project_id, status, cursor, limit,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
        })
        .map_err(|error| error.to_string())
}

pub fn query_chapter_versions_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let chapter_id = required_string(&request, "chapterId")?;
    let cursor = request.get("cursor").and_then(Value::as_str);
    let limit = request
        .get("pageSize")
        .and_then(Value::as_u64)
        .unwrap_or(30);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::repositories::ui_projection::chapter_versions(
                connection, project_id, chapter_id, cursor, limit,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
        })
        .map_err(|error| error.to_string())
}

pub fn query_canon_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let limit = request
        .get("pageSize")
        .and_then(Value::as_u64)
        .unwrap_or(60);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::repositories::ui_projection::canon(connection, project_id, limit)
                .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
        })
        .map_err(|error| error.to_string())
}

pub fn query_retcon_context_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let query = request.get("query").and_then(Value::as_str);
    let limit = request
        .get("pageSize")
        .and_then(Value::as_i64)
        .unwrap_or(30);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::repositories::change_retcon::retcon_context(connection, project_id, query, limit)
                .map_err(|error| ManagerError::ProjectCommand(format!("B12_DB:{error}")))
        })
        .map_err(|error| error.to_string())
}

pub fn query_book_health_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let cursor = request.get("cursor").and_then(Value::as_str);
    let limit = request
        .get("pageSize")
        .and_then(Value::as_u64)
        .unwrap_or(30);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::health::query_book_health(connection, project_id, cursor, limit)
                .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
        })
        .map_err(|error| error.to_string())
}

pub fn query_history_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let kind = request.get("kind").and_then(Value::as_str);
    let cursor = request.get("cursor").and_then(Value::as_str);
    let limit = request
        .get("pageSize")
        .and_then(Value::as_u64)
        .unwrap_or(40);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::repositories::ui_projection::history(connection, project_id, kind, cursor, limit)
                .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
        })
        .map_err(|error| error.to_string())
}

pub fn query_change_request_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let change_request_id = required_string(&request, "changeRequestId")?;
    let cursor = request.get("cursor").and_then(Value::as_i64).unwrap_or(0);
    let limit = request
        .get("pageSize")
        .and_then(Value::as_i64)
        .unwrap_or(40);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::repositories::change_retcon::change_request_detail(
                connection,
                project_id,
                change_request_id,
                limit,
                cursor,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?
            .ok_or(ManagerError::ProjectNotFound)
        })
        .map_err(|error| error.to_string())
}

pub fn query_dependency_summary_bootstrap_impl(
    state: &KernelState,
    request: Value,
) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            let summary = crate::repositories::change_retcon::dependency_summary_status(
                connection, project_id,
            )
            .map_err(|error| ManagerError::ProjectCommand(format!("B12_DB:{error}")))?;
            let job =
                crate::repositories::runtime::DurableJobService::dependency_summary_job_status(
                    connection, project_id,
                )
                .map_err(|error| ManagerError::ProjectCommand(format!("B12_DB:{error}")))?;
            Ok(json!({
                "projectId":project_id,
                "summary":summary,
                "job":job,
                "owner":"B12::ChangeRetconOwner"
            }))
        })
        .map_err(|error| error.to_string())
}

fn request_strings(request: &Value, field: &str) -> Vec<String> {
    request
        .get(field)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn request_resume_context(request: &Value) -> ResumeContext {
    let mut context = ResumeContext {
        current_chapter: request
            .get("currentChapter")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        current_volume: request
            .get("currentVolumeId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        ..ResumeContext::default()
    };
    context
        .present_entities
        .extend(request_strings(request, "presentEntityIds"));
    context
        .acquired_facts
        .extend(request_strings(request, "acquiredFactIds"));
    context
        .satisfied_tasks
        .extend(request_strings(request, "satisfiedTaskIds"));
    for meeting in request
        .get("meetings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(pair) = meeting
            .as_array()
            .and_then(|pair| (pair.len() == 2).then_some(pair))
        {
            if let (Some(left), Some(right)) = (pair[0].as_str(), pair[1].as_str()) {
                context.meetings.insert((left.to_owned(), right.to_owned()));
            }
        }
    }
    context
}

fn narrative_state_from_request(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
) -> Result<CurrentNarrativeStateView, String> {
    let entity_ids = request_strings(request, "entityIds");
    let entity_refs = entity_ids.iter().map(String::as_str).collect::<Vec<_>>();
    let references = request_strings(request, "references");
    let reference_refs = references.iter().map(String::as_str).collect::<Vec<_>>();
    let chapter_id = request.get("chapterId").and_then(Value::as_str);
    let (facts, facts_truncated) =
        crate::repositories::narrative::NarrativeRepository::current_canon_bounded(
            connection,
            project_id,
            &entity_refs,
            100,
        )
        .map_err(|e| e.to_string())?;
    let relations = crate::repositories::narrative::NarrativeRepository::current_relations(
        connection,
        project_id,
        &entity_refs,
        100,
    )
    .map_err(|e| e.to_string())?;
    let (knowledge, knowledge_truncated) =
        crate::repositories::narrative::NarrativeRepository::knowledge_for_entities_bounded(
            connection,
            project_id,
            &entity_refs,
            100,
        )
        .map_err(|e| e.to_string())?;
    let events = chapter_id
        .map(|chapter| {
            crate::repositories::narrative::NarrativeRepository::canon_events(
                connection, project_id, chapter, 100,
            )
        })
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let relations_truncated =
        crate::repositories::narrative::NarrativeRepository::has_more_relations(
            connection,
            project_id,
            &entity_refs,
            100,
        )
        .map_err(|e| e.to_string())?;
    let events_truncated = chapter_id
        .map(|chapter| {
            crate::repositories::narrative::NarrativeRepository::has_more_events(
                connection, project_id, chapter, 100,
            )
        })
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or(false);
    let obligations_truncated =
        crate::repositories::narrative::NarrativeRepository::has_more_obligations(
            connection,
            project_id,
            &entity_refs,
            &reference_refs,
            100,
        )
        .map_err(|e| e.to_string())?;
    let obligations =
        crate::repositories::narrative::NarrativeRepository::active_obligations_for_relevance(
            connection,
            project_id,
            &entity_refs,
            &reference_refs,
            100,
        )
        .map_err(|e| e.to_string())?;
    Ok(CurrentNarrativeStateView::from_records(
        RuntimeBounds {
            project_id: project_id.into(),
            entity_ids,
            volume_id: request
                .get("volumeId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            chapter_id: chapter_id.map(ToOwned::to_owned),
            limit: 100,
        },
        facts,
        relations,
        knowledge,
        events,
        obligations,
        facts_truncated
            || knowledge_truncated
            || relations_truncated
            || events_truncated
            || obligations_truncated,
    ))
}

fn continuity_from_request(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
) -> Result<Vec<crate::narrative::ContinuityThreadView>, String> {
    let state = narrative_state_from_request(connection, request, project_id)?;
    let chapter_refs = request
        .get("chapterId")
        .and_then(Value::as_str)
        .map(|id| vec![id.to_owned()])
        .unwrap_or_default();
    let chapter_refs = chapter_refs.iter().map(String::as_str).collect::<Vec<_>>();
    let refs = request_strings(request, "references");
    let refs = refs.iter().map(String::as_str).collect::<Vec<_>>();
    let foreshadows = crate::repositories::narrative::NarrativeRepository::active_foreshadows(
        connection,
        project_id,
        &chapter_refs,
        &refs,
        100,
    )
    .map_err(|e| e.to_string())?;
    let mysteries = crate::repositories::narrative::NarrativeRepository::open_mysteries(
        connection, project_id, &refs, 100,
    )
    .map_err(|e| e.to_string())?;
    let evidence = crate::repositories::narrative::NarrativeRepository::recent_evidence(
        connection,
        project_id,
        &chapter_refs,
        &refs,
        100,
    )
    .map_err(|e| e.to_string())?;
    build_continuity_threads(
        &state.active_responsibilities,
        &foreshadows,
        &mysteries,
        &evidence,
        &request_resume_context(request),
    )
}

fn continuity_from_request_bounded(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
) -> Result<(Vec<crate::narrative::ContinuityThreadView>, bool), String> {
    let threads = continuity_from_request(connection, request, project_id)?;
    let entity_ids = request_strings(request, "entityIds");
    let entity_refs = entity_ids.iter().map(String::as_str).collect::<Vec<_>>();
    let references = request_strings(request, "references");
    let reference_refs = references.iter().map(String::as_str).collect::<Vec<_>>();
    let chapters = request
        .get("chapterId")
        .and_then(Value::as_str)
        .map(|id| vec![id.to_owned()])
        .unwrap_or_default();
    let chapter_refs = chapters.iter().map(String::as_str).collect::<Vec<_>>();
    let (_, evidence_truncated) =
        crate::repositories::narrative::NarrativeRepository::recent_evidence_bounded(
            connection,
            project_id,
            &chapter_refs,
            &reference_refs,
            100,
        )
        .map_err(|e| e.to_string())?;
    let truncated = crate::repositories::narrative::NarrativeRepository::has_more_obligations(
        connection,
        project_id,
        &entity_refs,
        &reference_refs,
        100,
    )
    .map_err(|e| e.to_string())?
        || crate::repositories::narrative::NarrativeRepository::has_more_foreshadows(
            connection,
            project_id,
            &chapter_refs,
            &reference_refs,
            100,
        )
        .map_err(|e| e.to_string())?
        || crate::repositories::narrative::NarrativeRepository::has_more_mysteries(
            connection,
            project_id,
            &reference_refs,
            100,
        )
        .map_err(|e| e.to_string())?
        || evidence_truncated;
    Ok((threads, truncated))
}

pub(crate) fn context_input_from_authority(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
    revision: i64,
) -> Result<crate::narrative::ContextCompilerInput, String> {
    if let Some(context_ref) = request.get("contextManifestRef").and_then(Value::as_str) {
        if crate::repositories::change_retcon::has_pending_stale_target(
            connection,
            project_id,
            context_ref,
        )
        .map_err(|error| error.to_string())?
        {
            return Err("CONTEXT_STALE:RECOMPILE_REQUIRED".into());
        }
    }
    let state = narrative_state_from_request(connection, request, project_id)?;
    let (threads, continuity_truncated) =
        continuity_from_request_bounded(connection, request, project_id)?;
    let volume_id = request
        .get("volumeId")
        .or_else(|| request.get("currentVolumeId"))
        .and_then(Value::as_str);
    let goal_contract_refs = if let Some(volume_id) = volume_id {
        crate::repositories::narrative::NarrativeRepository::goal_contract_refs_for_volume(
            connection, project_id, volume_id, 100,
        )
    } else {
        crate::repositories::narrative::NarrativeRepository::goal_contract_refs(
            connection, project_id, 100,
        )
    }
    .map_err(|e| e.to_string())?;
    let opening_bootstrap =
        request.get("executionMode").and_then(Value::as_str) == Some("OPENING_BOOTSTRAP");
    let (task_states, task_truncated) =
        if let Some(volume_id) = volume_id.filter(|_| !opening_bootstrap) {
            let route = crate::repositories::narrative::NarrativeRepository::volume_plan_route(
                connection, project_id, volume_id,
            )
            .map_err(|e| e.to_string())?;
            let refs = if let Some(route) = route {
                let payload =
                    crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                        connection, project_id, &route,
                    )
                    .map_err(|e| e.to_string())?
                    .unwrap_or(Value::Null);
                payload
                    .get("tasks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.get("taskId").and_then(Value::as_str))
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let refs = refs.iter().map(String::as_str).collect::<Vec<_>>();
            (
                crate::repositories::narrative::NarrativeRepository::task_states_by_ids(
                    connection, project_id, &refs, 100,
                )
                .map_err(|e| e.to_string())?,
                false,
            )
        } else {
            crate::repositories::narrative::NarrativeRepository::task_states_bounded(
                connection, project_id, 100,
            )
            .map_err(|e| e.to_string())?
        };
    let active_task_refs = task_states
        .into_iter()
        .filter(|task| {
            matches!(
                task.value.get("status").and_then(Value::as_str),
                Some("ACTIVE") | Some("BLOCKED")
            )
        })
        .map(|task| task.state_id)
        .collect();
    let mut future_reservation_refs = Vec::new();
    let volume_intent = if let Some(volume_id) = volume_id {
        crate::repositories::narrative::NarrativeRepository::artifact_payload(
            connection,
            project_id,
            "VolumeIntent",
            volume_id,
        )
        .map_err(|e| e.to_string())?
    } else {
        None
    };
    for payload in [
        crate::repositories::narrative::NarrativeRepository::latest_artifact_payload(
            connection, project_id, "BookPlan",
        )
        .map_err(|e| e.to_string())?,
        volume_intent,
    ]
    .into_iter()
    .flatten()
    {
        if let Some(items) = payload
            .get("futureReservations")
            .or_else(|| payload.get("futureReservationRefs"))
            .and_then(Value::as_array)
        {
            future_reservation_refs.extend(items.iter().filter_map(|item| {
                item.get("reservationId")
                    .and_then(Value::as_str)
                    .or_else(|| item.as_str())
                    .map(ToOwned::to_owned)
            }));
        }
    }
    let authority_refs = state
        .source_refs
        .iter()
        .map(|item| item.ref_.as_str())
        .collect::<Vec<_>>();
    let (evidence, evidence_truncated) =
        crate::repositories::narrative::NarrativeRepository::evidence_for_refs_bounded(
            connection,
            project_id,
            &authority_refs,
            100,
        )
        .map_err(|e| e.to_string())?;
    let selected_evidence = evidence
        .into_iter()
        .map(|item| crate::generated::types::EvidenceRef {
            artifact_id: item.source_artifact_id,
            version: item.version,
            range_start: item.range_start,
            range_end: item.range_end,
            quote: item.quote,
        })
        .collect();
    let omitted_soft_sources = if request
        .get("includeSoftSources")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        vec![crate::generated::types::OmittedSource {
            source_ref: crate::generated::types::SourceRef {
                kind: "soft-cache".into(),
                ref_: "summary".into(),
            },
            reason: "soft cache is excluded from HARD digest".into(),
        }]
    } else {
        Vec::new()
    };
    let mut state = state;
    state.truncated =
        state.truncated || task_truncated || evidence_truncated || continuity_truncated;
    Ok(crate::narrative::ContextCompilerInput {
        execution_mode: request
            .get("executionMode")
            .and_then(Value::as_str)
            .unwrap_or("LONG_FORM")
            .into(),
        project_revision: revision,
        goal_contract_refs,
        state,
        continuity_threads: threads,
        active_task_refs,
        future_reservation_refs,
        selected_evidence,
        omitted_soft_sources,
        hard_tokens: 1024,
        soft_tokens: 0,
    })
}

pub fn query_narrative_state_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let revision = state
        .manager
        .project_revision(project_id)
        .map_err(|e| e.to_string())?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            let view = narrative_state_from_request(connection, &request, project_id)
                .map_err(ManagerError::ProjectCommand)?;
            serde_json::to_value(
                json!({"projectId":project_id,"projectRevision":revision,"view":view}),
            )
            .map_err(|e| ManagerError::ProjectCommand(e.to_string()))
        })
        .map_err(|e| e.to_string())
}

pub fn query_continuity_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let revision = state
        .manager
        .project_revision(project_id)
        .map_err(|e| e.to_string())?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            let (threads, truncated) = continuity_from_request_bounded(connection, &request, project_id)
                .map_err(ManagerError::ProjectCommand)?;
            serde_json::to_value(
                json!({"projectId":project_id,"projectRevision":revision,"threads":threads,"truncated":truncated}),
            )
            .map_err(|e| ManagerError::ProjectCommand(e.to_string()))
        })
        .map_err(|e| e.to_string())
}

pub fn volume_progress_from_authority(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
    revision: i64,
) -> Result<Value, String> {
    let volume_id = required_string(request, "volumeId")?;
    let route = crate::repositories::narrative::NarrativeRepository::volume_plan_route(
        connection, project_id, volume_id,
    )
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "VOLUME_PLAN_NOT_FOUND".to_string())?;
    let plan = crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
        connection, project_id, &route,
    )
    .map_err(|e| e.to_string())?
    .unwrap_or(Value::Null);
    let current_chapter = request
        .get("currentChapter")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let target_chapters = plan
        .get("capacityBudget")
        .and_then(|budget| budget.get("maxChapters"))
        .and_then(Value::as_i64)
        .unwrap_or(current_chapter);
    let task_ids = plan
        .get("tasks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("taskId").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let tasks = crate::repositories::narrative::NarrativeRepository::task_states_by_ids(
        connection, project_id, &task_ids, 100,
    )
    .map_err(|e| e.to_string())?;
    let mut result = serde_json::to_value(build_volume_progress(VolumeProgressInput {
        volume_id: volume_id.into(),
        target_chapters,
        current_chapter,
        tasks: tasks.clone(),
        generated_revision: revision,
    }))
    .map_err(|e| e.to_string())?;
    result["routePlanRef"] = serde_json::to_value(route).map_err(|e| e.to_string())?;
    result["tasks"] = serde_json::to_value(&tasks).map_err(|e| e.to_string())?;
    let required_remaining = plan
        .get("tasks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|task| {
            matches!(
                task.get("class").and_then(Value::as_str),
                Some("REQUIRED" | "CLIMAX" | "RESERVED")
            )
        })
        .filter(|task| {
            let task_id = task
                .get("taskId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            !tasks.iter().any(|state| {
                state.state_id == task_id
                    && matches!(
                        state.value.get("status").and_then(Value::as_str),
                        Some("SATISFIED")
                    )
            })
        })
        .count() as i64;
    let blocked_required = tasks
        .iter()
        .filter(|state| state.value.get("status").and_then(Value::as_str) == Some("BLOCKED"))
        .count() as i64;
    let exit_gap = plan
        .get("requiredExitConditions")
        .and_then(Value::as_array)
        .map_or(0, Vec::len) as i64;
    let reservation_pressure = plan
        .get("tasks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|task| {
            task.get("reservationRefs")
                .and_then(Value::as_array)
                .is_some_and(|refs| !refs.is_empty())
        })
        .count() as i64;
    result["planningPressure"] = crate::planning::planning_pressure(
        current_chapter,
        target_chapters,
        required_remaining,
        blocked_required,
        exit_gap,
        0,
        reservation_pressure,
    );
    result["truncated"] = json!(false);
    Ok(result)
}

pub fn query_volume_progress_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let revision = state
        .manager
        .project_revision(project_id)
        .map_err(|e| e.to_string())?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            volume_progress_from_authority(connection, &request, project_id, revision)
                .map_err(ManagerError::ProjectCommand)
        })
        .map_err(|e| e.to_string())
}

pub fn plan_route_from_authority(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
    revision: i64,
) -> Result<Value, String> {
    let volume_id = required_string(request, "volumeId")?;
    let volume_intent_ref =
        crate::repositories::narrative::NarrativeRepository::canonical_head_ref(
            connection,
            project_id,
            "VolumeIntent",
            volume_id,
        )
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "VOLUME_INTENT_NOT_FOUND".to_string())?;
    let route_ref = crate::repositories::narrative::NarrativeRepository::volume_plan_route(
        connection, project_id, volume_id,
    )
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "VOLUME_PLAN_NOT_FOUND".to_string())?;
    let route = crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
        connection, project_id, &route_ref,
    )
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "VOLUME_PLAN_PAYLOAD_NOT_FOUND".to_string())?;
    let route_stale = crate::repositories::change_retcon::has_pending_stale_for_refs(
        connection,
        project_id,
        &[route_ref.artifact_id.as_str()],
    )
    .map_err(|e| e.to_string())?;
    crate::planning::validate_volume_route(&route)?;
    let progress = volume_progress_from_authority(connection, request, project_id, revision)?;
    let intent_payload = crate::repositories::narrative::NarrativeRepository::artifact_payload(
        connection,
        project_id,
        "VolumeIntent",
        volume_id,
    )
    .map_err(|e| e.to_string())?
    .unwrap_or_else(|| json!({}));
    let book_payload =
        crate::repositories::narrative::NarrativeRepository::latest_artifact_payload(
            connection, project_id, "BookPlan",
        )
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| json!({}));
    let mut future_reservations = book_payload
        .get("futureReservations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let intent_refs = intent_payload
        .get("futureReservationRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    future_reservations.retain(|reservation| {
        reservation
            .get("reservationId")
            .and_then(Value::as_str)
            .is_none_or(|id| intent_refs.is_empty() || intent_refs.contains(id))
    });
    let continuity_threads = continuity_from_request_bounded(connection, request, project_id)
        .map_err(|e| e.to_string())?
        .0
        .into_iter()
        .filter(|thread| matches!(thread.logical_status.as_str(), "OPEN" | "SUSPENDED"))
        .map(|thread| serde_json::to_value(thread).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let current_chapter_plan = request
        .get("currentChapter")
        .and_then(Value::as_i64)
        .filter(|chapter| *chapter > 0)
        .and_then(|chapter_no| {
            crate::repositories::narrative::NarrativeRepository::chapter_id_for_number(
                connection, project_id, volume_id, chapter_no,
            )
            .ok()
            .flatten()
        })
        .map(|chapter_id| {
            crate::repositories::narrative::NarrativeRepository::canonical_head_ref(
                connection,
                project_id,
                "ChapterPlan",
                &chapter_id,
            )
        })
        .transpose()
        .map_err(|e| e.to_string())?
        .flatten();
    let horizon_preview = current_chapter_plan
        .as_ref()
        .and_then(|reference| {
            crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                connection, project_id, reference,
            )
            .ok()
            .flatten()
        })
        .and_then(|payload| payload.get("horizonPreview").cloned())
        .unwrap_or_else(|| json!([]));
    let bound = request
        .get("maxResults")
        .and_then(Value::as_u64)
        .unwrap_or(100)
        .clamp(1, 100) as usize;
    let mut tasks = route
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let truncated = tasks.len() > bound;
    tasks.truncate(bound);
    Ok(
        json!({"projectId":project_id,"projectRevision":revision,"volumeId":volume_id,"volumeIntentRef":volume_intent_ref,"volumePlanRef":route_ref,"routeVersion":route.get("routeVersion").cloned().unwrap_or(Value::Null),"tasks":tasks,"routeInvariants":route.get("routeInvariants").cloned().unwrap_or_else(|| json!([])),"requiredExitConditions":route.get("requiredExitConditions").cloned().unwrap_or_else(|| json!([])),"capacityBudget":route.get("capacityBudget").cloned().unwrap_or_else(|| json!({})),"taskReality":progress.get("tasks").cloned().unwrap_or_else(|| json!([])),"planningPressure":progress.get("planningPressure").cloned().unwrap_or_else(|| json!({})),"continuityThreads":continuity_threads,"futureReservations":future_reservations,"currentChapterPlan":current_chapter_plan,"horizonPreview":horizon_preview,"stale":if route_stale { json!({"status":"STALE","reason":"受影响内容需要重新核对"}) } else { json!({"status":"FRESH"}) },"owner":"B07::PlanningRoute","truncated":truncated}),
    )
}

pub fn query_plan_route_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let revision = state
        .manager
        .project_revision(project_id)
        .map_err(|e| e.to_string())?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            plan_route_from_authority(connection, &request, project_id, revision)
                .map_err(ManagerError::ProjectCommand)
        })
        .map_err(|e| e.to_string())
}

pub fn query_volume_lifecycle_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let volume_id = required_string(&request, "volumeId")?;
    let revision = state
        .manager
        .project_revision(project_id)
        .map_err(|error| error.to_string())?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            let planning_state = crate::repositories::volume_lifecycle::VolumeLifecycleRepository::planning_projection(
                connection,
                project_id,
                volume_id,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?
            .unwrap_or_else(|| json!({
                "status": "NOT_MATERIALIZED",
                "projectId": project_id,
                "volumeId": volume_id,
                "generatedRevision": revision,
                "closeReadiness": "BLOCKED",
                "owner": "B09::PlanningStateProjectionOwner"
            }));
            let route_request = json!({
                "projectId": project_id,
                "volumeId": volume_id,
                "maxResults": request.get("maxResults").cloned().unwrap_or_else(|| json!(100))
            });
            let route = plan_route_from_authority(connection, &route_request, project_id, revision)
                .map_err(ManagerError::ProjectCommand)?;
            let close = crate::repositories::volume_lifecycle::VolumeLifecycleRepository::latest_close_record(
                connection,
                project_id,
                volume_id,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
            let handoff = crate::repositories::volume_lifecycle::VolumeLifecycleRepository::latest_handoff(
                connection,
                project_id,
                volume_id,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
            let replans = crate::repositories::volume_lifecycle::VolumeLifecycleRepository::route_replan_history(
                connection,
                project_id,
                volume_id,
                20,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
            Ok(json!({
                "projectId": project_id,
                "projectRevision": revision,
                "volumeId": volume_id,
                "planningState": planning_state,
                "route": route,
                "latestClose": close,
                "latestHandoff": handoff,
                "routeReplans": replans,
                "owner": "B09::VolumeLifecycleOwner"
            }))
        })
        .map_err(|error| error.to_string())
}

fn b14_completion_response(
    project_id: &str,
    revision: i64,
    input: &crate::endgame::CompletionInput,
    history: crate::repositories::endgame::CompletionHistory,
    relevant_state_digest: &str,
) -> Result<Value, String> {
    let evaluation = crate::endgame::evaluate_completion(input);
    let status = if history.has_history && !history.currently_completed {
        crate::endgame::CompletionStatus::NeedsRecheck
    } else {
        evaluation.status
    };
    let status = serde_json::to_value(status).map_err(|error| error.to_string())?;
    Ok(json!({
        "projectId": project_id,
        "projectRevision": revision,
        "status": status,
        "checks": evaluation.checks,
        "blockerCount": evaluation.blocker_count,
        "advisoryCount": evaluation.advisory_count,
        "relevantStateDigest": relevant_state_digest,
        "sourceAvailable": input.source_available,
        "requestedEndingConfirmed": input.requested_ending_confirmed,
        "completed": history.currently_completed,
        "hasHistoricalCompletion": history.has_history
    }))
}

pub fn query_endgame_status_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            let read = crate::repositories::endgame::EndgameRepository::read_completion_input(
                connection, project_id,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
            let history = crate::repositories::endgame::EndgameRepository::completion_history(
                connection,
                project_id,
                &read.relevant_state_digest,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
            b14_completion_response(
                project_id,
                read.revision,
                &read.input,
                history,
                &read.relevant_state_digest,
            )
            .map_err(ManagerError::ProjectCommand)
        })
        .map_err(|error| error.to_string())
}

pub fn query_completion_status_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    query_endgame_status_impl(state, request)
}

pub fn query_endgame_mapping_options_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    state.manager.with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
        crate::repositories::endgame::EndgameRepository::mapping_workflow(connection, project_id)
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
    }).map_err(|error| error.to_string())
}

pub fn query_endgame_advice_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    query_b14_skill_runtime(state, request, "S44", "B14_S44_ENDGAME_ADVICE", "proposal")
}

pub fn query_completion_quality_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    query_b14_skill_runtime(state, request, "S45", "B14_S45_COMPLETION_QUALITY", "review")
}

fn query_b14_skill_runtime(
    state: &KernelState,
    request: Value,
    skill_id: &str,
    capability: &str,
    output_key: &str,
) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let handle = state.manager.open(project_id, "", "2026-09-06T00:00:00Z").map_err(|error| error.to_string())?;
    let read = crate::repositories::endgame::EndgameRepository::read_completion_input(&handle, project_id).map_err(|error| error.to_string())?;
    let selection = crate::creative_skills::select_creative_skills(crate::creative_skills::SkillSelectionRequest {
        primary: skill_id.into(), support: Vec::new(), budget_reason: None, context_tokens: 1200, model_calls: 1,
    })?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_nanos();
    let model_run_id = StableIdentityFactory::from_command(StableIdKind::ModelRun, &format!("b14:{skill_id}:{project_id}:{nonce}"))?;
    let now: String = handle.query_row("SELECT updated_at FROM project_meta WHERE project_id=?1", [project_id], |row| row.get(0)).map_err(|error| error.to_string())?;
    let spec = crate::repositories::runtime::ModelRunSpec {
        model_run_id: model_run_id.clone(), job_id: None, step_id: None,
        provider: state.production_gateway.provider().into(), model: state.production_gateway.model(capability),
        idempotency_key: format!("b14:{skill_id}:{nonce}"), capability: capability.into(), prompt_ref: None,
        context_manifest_ref: None, budget_id: None, predicted_cost_cents: state.production_gateway.predicted_cost_cents(capability).max(0), attempt_no: 1, repair_round: 0,
    };
    let input = json!({"skillId":selection.primary.id,"skillVersion":selection.primary.version,"projectId":project_id,"projectRevision":read.revision,"boundedContext":{"requestedEnding":read.endgame_contract.as_ref().and_then(|value| value.get("requestedEnding")),"hardSummary":{"blockers":crate::endgame::evaluate_completion(&read.input).blocker_count},"finalVolume":"bounded"},"prompt":"仅输出软性创作建议 JSON；不得改变正式完结状态。"});
    let outcome = crate::production::execute_production_model(&handle, state.production_gateway.as_ref(), &spec, capability, input, &now)?;
    let output = crate::creative_skills::normalize_skill_output(outcome.parsed.ok_or_else(|| "B14_SKILL_OUTPUT_MISSING".to_string())?)?;
    let output_digest = sha256_canonical(&serde_json::to_value(&output).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    Ok(json!({"softOnly":true,output_key:output,"projectRevision":read.revision,"sourceAvailable":read.input.source_available,"runtimeCheckRequests":[],"userAction":format!("{skill_id}_EXPLICIT_ACTION"),"provenance":{"skillId":skill_id,"skillVersion":selection.primary.version,"modelRunId":model_run_id,"provider":spec.provider,"model":spec.model,"contextDigest":sha256_canonical(&json!({"projectId":project_id,"revision":read.revision})).map_err(|error| error.to_string())?,"outputDigest":output_digest,"explicitUserAction":true}}))
}

fn b14_project_scope() -> Value {
    json!({"level":"PROJECT"})
}

fn b14_hard_responsibilities(input: &crate::endgame::CompletionInput) -> Value {
    let mut responsibilities = Vec::new();
    let mut add = |kind: &str, count: i64, authority_class: &str, source_ref: &str| {
        if count > 0 {
            responsibilities.push(json!({
                "responsibilityId": format!("b14:{kind}"),
                "kind": kind,
                "sourceRefs": [source_ref],
                "authorityClass": authority_class,
                "requiredDisposition": "RESOLVED_BEFORE_COMPLETION"
            }));
        }
    };
    add(
        "OBLIGATION",
        input.unresolved_hard_obligations,
        "A",
        "narrative_obligations",
    );
    add(
        "MYSTERY",
        input.unmapped_mysteries,
        "B",
        "book_plan.majorMysteryGoals",
    );
    add(
        "PAYOFF",
        input.unresolved_hard_payoffs,
        "B",
        "payoff_registry",
    );
    add(
        "FUTURE_RESERVATION",
        input.unfulfilled_hard_reservations,
        "A",
        "book_plan.futureReservations",
    );
    add(
        "CONTINUITY",
        input.open_hard_continuity,
        "A",
        "continuity_threads",
    );
    add(
        "TASK",
        input.blocked_required_tasks,
        "B",
        "state_deltas.volume.task.progress",
    );
    responsibilities.push(json!({
        "responsibilityId": "b14:ending-boundary",
        "kind": "ENDING_BOUNDARY",
        "sourceRefs": ["user.requestedEnding"],
        "authorityClass": "A",
        "requiredDisposition": "USER_CONFIRMED"
    }));
    Value::Array(responsibilities)
}

fn b14_replayed_receipt(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
) -> Result<Option<Value>, String> {
    let Some(receipt) = CommitService::find_receipt(connection, project_id, idempotency_key)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    if receipt.command_id != command_id {
        return Err("IDEMPOTENCY_CONFLICT".into());
    }
    Ok(Some(
        json!({"finalStatus":"committed","receipt":receipt,"owner":"B14::EndgameCompletionOwner"}),
    ))
}

pub fn create_endgame_contract_impl(
    state: &KernelState,
    request: &Value,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    requested_at: &str,
    expected_revision: Option<i64>,
) -> Result<Value, String> {
    let expected_revision = expected_revision
        .ok_or_else(|| "IPC_INVALID:expectedRevision required for B14 command".to_string())?;
    let ending = request
        .pointer("/payload/requestedEnding")
        .or_else(|| request.get("requestedEnding"))
        .ok_or_else(|| "B14_REQUESTED_ENDING_REQUIRED".to_string())?;
    let summary = ending
        .get("summary")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "B14_REQUESTED_ENDING_SUMMARY_REQUIRED".to_string())?;
    if ending.get("confirmationSource").and_then(Value::as_str) != Some("USER_CONFIRMED") {
        return Err("B14_REQUESTED_ENDING_CONFIRMATION_REQUIRED".into());
    }
    let confirmed_at = ending
        .get("confirmedAt")
        .and_then(Value::as_str)
        .unwrap_or(requested_at);
    let handle = state
        .manager
        .open(project_id, "", requested_at)
        .map_err(|error| error.to_string())?;
    if let Some(replayed) = b14_replayed_receipt(&handle, project_id, command_id, idempotency_key)?
    {
        return Ok(replayed);
    }
    let read =
        crate::repositories::endgame::EndgameRepository::read_completion_input(&handle, project_id)
            .map_err(|error| error.to_string())?;
    let input = read.input;
    let revision = read.revision;
    if revision != expected_revision {
        return Err("REVISION_CONFLICT".into());
    }
    let history = crate::repositories::endgame::EndgameRepository::completion_history(
        &handle,
        project_id,
        &read.relevant_state_digest,
    )
    .map_err(|error| error.to_string())?;
    if history.currently_completed {
        return Err("B14_ALREADY_COMPLETED".into());
    }
    let book_plan_ref = crate::repositories::endgame::EndgameRepository::latest_project_head_ref(
        &handle, project_id, "BookPlan",
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "B14_BOOK_PLAN_REQUIRED".to_string())?;
    let ending_boundary_digest =
        sha256_canonical(&json!({"summary":summary})).map_err(|error| error.to_string())?;
    let mystery_mappings = request
        .pointer("/payload/mysteryMappings")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let reservation_mappings = request
        .pointer("/payload/reservationMappings")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let hard_payoff_mappings = request
        .pointer("/payload/hardPayoffMappings")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let relevant_refs = request
        .pointer("/payload/relevantRefs")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let mut contract_payload = json!({
        "payloadSchema": "endgame-contract.r1",
        "compiledFromProjectRevision": revision + 1,
        "compiledFromRelevantStateDigest": "",
        "bookPlanRef": book_plan_ref,
        "endingBoundaryDigest": ending_boundary_digest,
        "requestedEnding": {
            "summary": summary,
            "confirmationSource": "USER_CONFIRMED",
            "confirmedAt": confirmed_at
        },
        "hardResponsibilities": b14_hard_responsibilities(&input),
        "mysteryMappings": mystery_mappings,
        "reservationMappings": reservation_mappings,
        "hardPayoffMappings": hard_payoff_mappings,
        "relevantRefs": relevant_refs,
        "createdAt": requested_at
    });
    let preview =
        crate::repositories::endgame::EndgameRepository::read_completion_input_with_contract(
            &handle,
            project_id,
            Some(&contract_payload),
        )
        .map_err(|error| error.to_string())?;
    let digest = preview.relevant_state_digest;
    contract_payload["compiledFromRelevantStateDigest"] = json!(digest);
    let artifact_id = format!("endgame-contract:{project_id}");
    let version =
        crate::repositories::artifacts::ArtifactRepository::next_version(&handle, &artifact_id)
            .map_err(|error| error.to_string())?;
    let payload_hash = sha256_canonical(&contract_payload).map_err(|error| error.to_string())?;
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId": artifact_id,
        "projectId": project_id,
        "artifactType": "EndgameContract",
        "scope": b14_project_scope(),
        "version": version,
        "schemaVersion": "m0a.artifact.v1",
        "artifactStatus": "CANDIDATE",
        "semanticLifecycle": "APPROVED_PLAN",
        "payload": contract_payload,
        "payloadHash": payload_hash,
        "dependencyRefs": [book_plan_ref],
        "provenance": {"createdBy":"B14::EndgameCompletionOwner","commandId":command_id},
        "createdAt": requested_at
    }))
    .map_err(|error| error.to_string())?;
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId": format!("commit:b14:endgame:{command_id}"),
        "projectId": project_id,
        "commandId": command_id,
        "idempotencyKey": idempotency_key,
        "expectedProjectRevision": expected_revision,
        "contractVersion": "m0a.v1",
        "createdAt": requested_at,
        "artifacts": [artifact],
        "headUpdates": [{"artifactType":"EndgameContract","scope":b14_project_scope(),"artifactId":artifact_id,"version":version,"payloadHash":payload_hash}],
        "receiptDraft": {"domain":"B14_ENDGAME_CONTRACT","payload":{"status":"COMMITTED","compiledFromProjectRevision":revision + 1,"relevantStateDigest":digest}}
    }))
    .map_err(|error| error.to_string())?;
    let receipt =
        CommitService::commit(&handle, &bundle, requested_at).map_err(|error| error.to_string())?;
    Ok(json!({"finalStatus":"committed","receipt":receipt,"owner":"B14::EndgameCompletionOwner"}))
}

pub fn complete_book_impl(
    state: &KernelState,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    requested_at: &str,
    expected_revision: Option<i64>,
) -> Result<Value, String> {
    let expected_revision = expected_revision
        .ok_or_else(|| "IPC_INVALID:expectedRevision required for B14 command".to_string())?;
    let handle = state
        .manager
        .open(project_id, "", requested_at)
        .map_err(|error| error.to_string())?;
    if let Some(replayed) = b14_replayed_receipt(&handle, project_id, command_id, idempotency_key)?
    {
        return Ok(replayed);
    }
    let read =
        crate::repositories::endgame::EndgameRepository::read_completion_input(&handle, project_id)
            .map_err(|error| error.to_string())?;
    let input = read.input;
    let revision = read.revision;
    if revision != expected_revision {
        return Err("REVISION_CONFLICT".into());
    }
    let history = crate::repositories::endgame::EndgameRepository::completion_history(
        &handle,
        project_id,
        &read.relevant_state_digest,
    )
    .map_err(|error| error.to_string())?;
    if history.currently_completed {
        return Err("B14_ALREADY_COMPLETED".into());
    }
    let evaluation = crate::endgame::evaluate_completion(&input);
    if evaluation.status != crate::endgame::CompletionStatus::Ready {
        let status =
            serde_json::to_string(&evaluation.status).map_err(|error| error.to_string())?;
        return Err(format!("B14_COMPLETION_NOT_READY:{status}"));
    }
    let digest = read.relevant_state_digest;
    let endgame_contract_ref =
        crate::repositories::endgame::EndgameRepository::latest_project_head_ref(
            &handle,
            project_id,
            "EndgameContract",
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "B14_ENDGAME_CONTRACT_REQUIRED".to_string())?;
    let final_volume_exit_state_ref =
        crate::repositories::endgame::EndgameRepository::final_volume_exit_ref(&handle, project_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "B14_FINAL_VOLUME_EXIT_STATE_REQUIRED".to_string())?;
    let (final_chapter_count, first_final_chapter_ref, last_final_chapter_ref) =
        crate::repositories::endgame::EndgameRepository::final_chapter_summary(&handle, project_id)
            .map_err(|error| error.to_string())?;
    let domain_digests =
        crate::repositories::endgame::EndgameRepository::domain_digests(&handle, project_id)
            .map_err(|error| error.to_string())?;
    let exit_artifact_id = format!("book-exit-state:{project_id}:{command_id}");
    let report_artifact_id = format!("book-completion-report:{project_id}:{command_id}");
    let completion_receipt_id = format!("completion-receipt:{command_id}");
    let exit_payload = json!({
        "payloadSchema": "book-exit-state.r1",
        "closedAt": requested_at,
        "projectRevision": revision + 1,
        "relevantStateDigest": digest,
        "endgameContractRef": endgame_contract_ref,
        "finalChapterCount": final_chapter_count,
        "firstFinalChapterRef": first_final_chapter_ref,
        "lastFinalChapterRef": last_final_chapter_ref,
        "manuscriptInventoryDigest": domain_digests["manuscriptInventoryDigest"],
        "canonDigest": domain_digests["canonDigest"],
        "knowledgeDigest": domain_digests["knowledgeDigest"],
        "continuityDigest": domain_digests["continuityDigest"],
        "reservationDigest": domain_digests["reservationDigest"],
        "obligationDigest": domain_digests["obligationDigest"],
        "taskRealityDigest": domain_digests["taskRealityDigest"],
        "finalChapterInventoryDigest": domain_digests["finalChapterInventoryDigest"],
        "finalVolumeExitStateRef": final_volume_exit_state_ref,
        "completionReportRef": report_artifact_id
    });
    let exit_version = crate::repositories::artifacts::ArtifactRepository::next_version(
        &handle,
        &exit_artifact_id,
    )
    .map_err(|error| error.to_string())?;
    let exit_hash = sha256_canonical(&exit_payload).map_err(|error| error.to_string())?;
    let exit_ref =
        json!({"artifactId":exit_artifact_id,"version":exit_version,"payloadHash":exit_hash});
    let report_payload = json!({
        "payloadSchema": "book-completion-report.r1",
        "completedAt": requested_at,
        "projectRevision": revision + 1,
        "relevantStateDigest": digest,
        "endgameContractRef": endgame_contract_ref,
        "completionGateResult": "READY",
        "checkSummaries": evaluation.checks,
        "exportPreflightDigest": domain_digests["finalChapterInventoryDigest"],
        "bookExitStateRef": exit_ref,
        "completionReceiptRef": completion_receipt_id
    });
    let report_version = crate::repositories::artifacts::ArtifactRepository::next_version(
        &handle,
        &report_artifact_id,
    )
    .map_err(|error| error.to_string())?;
    let report_hash = sha256_canonical(&report_payload).map_err(|error| error.to_string())?;
    let exit_artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId": exit_artifact_id,
        "projectId": project_id,
        "artifactType": "BookExitState",
        "scope": b14_project_scope(),
        "version": exit_version,
        "schemaVersion": "m0a.artifact.v1",
        "artifactStatus": "CANDIDATE",
        "semanticLifecycle": "CANON",
        "payload": exit_payload,
        "payloadHash": exit_hash,
        "dependencyRefs": [endgame_contract_ref, final_volume_exit_state_ref],
        "provenance": {"createdBy":"B14::EndgameCompletionOwner","commandId":command_id},
        "createdAt": requested_at
    }))
    .map_err(|error| error.to_string())?;
    let report_artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId": report_artifact_id,
        "projectId": project_id,
        "artifactType": "BookCompletionReport",
        "scope": b14_project_scope(),
        "version": report_version,
        "schemaVersion": "m0a.artifact.v1",
        "artifactStatus": "CANDIDATE",
        "semanticLifecycle": "CANON",
        "payload": report_payload,
        "payloadHash": report_hash,
        "dependencyRefs": [endgame_contract_ref, exit_ref],
        "provenance": {"createdBy":"B14::EndgameCompletionOwner","commandId":command_id},
        "createdAt": requested_at
    }))
    .map_err(|error| error.to_string())?;
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId": format!("commit:b14:complete:{command_id}"),
        "projectId": project_id,
        "commandId": command_id,
        "idempotencyKey": idempotency_key,
        "expectedProjectRevision": expected_revision,
        "contractVersion": "m0a.v1",
        "createdAt": requested_at,
        "artifacts": [exit_artifact, report_artifact],
        "headUpdates": [
            {"artifactType":"BookExitState","scope":b14_project_scope(),"artifactId":exit_artifact_id,"version":exit_version,"payloadHash":exit_hash},
            {"artifactType":"BookCompletionReport","scope":b14_project_scope(),"artifactId":report_artifact_id,"version":report_version,"payloadHash":report_hash}
        ],
        "receiptDraft": {
            "domain":"B14_BOOK_COMPLETION",
            "receiptId":completion_receipt_id,
            "artifactRefs":[exit_ref,{"artifactId":report_artifact_id,"version":report_version,"payloadHash":report_hash}],
            "payload":{"status":"COMPLETED","relevantStateDigest":digest,"projectRevision":revision + 1}
        }
    }))
    .map_err(|error| error.to_string())?;
    let receipt =
        CommitService::commit(&handle, &bundle, requested_at).map_err(|error| error.to_string())?;
    Ok(json!({"finalStatus":"committed","receipt":receipt,"owner":"B14::EndgameCompletionOwner"}))
}

pub fn query_context_manifest_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let revision = state
        .manager
        .project_revision(project_id)
        .map_err(|e| e.to_string())?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            let input = context_input_from_authority(connection, &request, project_id, revision)
                .map_err(ManagerError::ProjectCommand)?;
            serde_json::to_value(
                compile_context_manifest(input).map_err(ManagerError::ProjectCommand)?,
            )
            .map_err(|e| ManagerError::ProjectCommand(e.to_string()))
        })
        .map_err(|e| e.to_string())
}

pub fn query_plan_stale_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let current_revision = state
        .manager
        .project_revision(project_id)
        .map_err(|e| e.to_string())?;
    let compiled_revision = request
        .get("compiledFromProjectRevision")
        .and_then(Value::as_i64)
        .ok_or_else(|| "IPC_INVALID:compiledFromProjectRevision".to_string())?;
    let compiled_digest = required_string(&request, "compiledFromStateDigest")?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            let input =
                context_input_from_authority(connection, &request, project_id, current_revision)
                    .map_err(ManagerError::ProjectCommand)?;
            let current_digest = crate::narrative::relevant_state_digest(&input)
                .map_err(ManagerError::ProjectCommand)?;
            let mut status = evaluate_plan_stale(
                compiled_revision,
                compiled_digest,
                current_revision,
                &current_digest,
            );
            let mut stale_refs = Vec::new();
            if let Some(chapter_id) = request.get("chapterId").and_then(Value::as_str) {
                stale_refs.push(chapter_id.to_owned());
                if let Some(plan_ref) =
                    crate::repositories::narrative::NarrativeRepository::canonical_head_ref(
                        connection,
                        project_id,
                        "ChapterPlan",
                        chapter_id,
                    )
                    .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?
                {
                    stale_refs.push(plan_ref.artifact_id);
                }
            }
            let queued_stale = crate::repositories::change_retcon::has_pending_stale_for_refs(
                connection,
                project_id,
                &stale_refs.iter().map(String::as_str).collect::<Vec<_>>(),
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
            if queued_stale {
                status = PlanStaleStatus::PlanStale;
            }
            // The legacy read-only diagnostic contract treats an unchanged
            // project revision as a valid snapshot when no B12 invalidation is
            // recorded. Formal B06/B07/B08 execution uses the strict digest
            // check in their owners above; keeping this read projection
            // compatible avoids turning an unrelated diagnostic query into a
            // write-path freshness decision.
            if matches!(status, PlanStaleStatus::PlanStale)
                && compiled_revision == current_revision
                && !queued_stale
            {
                status = PlanStaleStatus::Valid;
            }
            Ok(json!({
                "projectId": project_id,
                "projectRevision": current_revision,
                "currentStateDigest": current_digest,
                "status": match status {
                    PlanStaleStatus::Valid => "VALID",
                    PlanStaleStatus::PlanStale => "PLAN_STALE",
                }
            }))
        })
        .map_err(|e| e.to_string())
}

pub struct RepositoryReadAdapter<'a> {
    pub connection: &'a libsql_rusqlite::Connection,
    pub project_id: &'a str,
}

fn adapter_scope(
    input: &Value,
    project_id: &str,
    capability_id: &str,
) -> Result<crate::harness::ProjectScope, String> {
    resolve_project_scope(
        project_id,
        input
            .get("projectRevision")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        input
            .get("question")
            .and_then(Value::as_str)
            .unwrap_or(capability_id),
        "DIAGNOSE",
        input
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("PROJECT"),
    )
}

impl ReadCapabilityAdapter for RepositoryReadAdapter<'_> {
    fn invoke(
        &self,
        capability_id: &str,
        input: &Value,
        bounds: &RequestedBounds,
    ) -> Result<(Value, Vec<String>, bool), String> {
        let entity_ids = input
            .get("entityIds")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        let references = input
            .get("references")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut truncated = false;
        let value = match capability_id {
            "query_knowledge" => {
                let (records, was_truncated) = crate::repositories::narrative::NarrativeRepository::knowledge_for_entities_bounded(
                    self.connection,
                    self.project_id,
                    &entity_ids,
                    bounds.max_results as i64,
                ).map_err(|e| e.to_string())?;
                truncated = was_truncated;
                serde_json::to_value(records).map_err(|e| e.to_string())?
            }
            "query_events" => {
                let chapter = input
                    .get("chapterId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "CAPABILITY_INPUT_INVALID:chapterId".to_string())?;
                let (records, was_truncated) =
                    crate::repositories::narrative::NarrativeRepository::canon_events_bounded(
                        self.connection,
                        self.project_id,
                        chapter,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?;
                truncated = was_truncated;
                serde_json::to_value(records).map_err(|e| e.to_string())?
            }
            "query_evidence" => {
                let (records, was_truncated) =
                    crate::repositories::narrative::NarrativeRepository::evidence_for_refs_bounded(
                        self.connection,
                        self.project_id,
                        &references,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?;
                truncated = was_truncated;
                serde_json::to_value(records).map_err(|e| e.to_string())?
            }
            "query_continuity" => {
                let (threads, was_truncated) =
                    continuity_from_request_bounded(self.connection, input, self.project_id)?;
                truncated = was_truncated;
                serde_json::to_value(threads).map_err(|e| e.to_string())?
            }
            "query_context_manifest" => {
                serde_json::to_value(compile_context_manifest(context_input_from_authority(
                    self.connection,
                    input,
                    self.project_id,
                    crate::repositories::narrative::NarrativeRepository::project_revision(
                        self.connection,
                        self.project_id,
                    )
                    .map_err(|e| e.to_string())?,
                )?)?)
                .map_err(|e| e.to_string())?
            }
            "query_project_meta" => serde_json::to_value(
                crate::repositories::narrative::NarrativeRepository::goal_contract_refs(
                    self.connection,
                    self.project_id,
                    bounds.max_results as i64,
                )
                .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?,
            "query_canon" => {
                let (records, was_truncated) =
                    crate::repositories::narrative::NarrativeRepository::current_canon_bounded(
                        self.connection,
                        self.project_id,
                        &entity_ids,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?;
                truncated = was_truncated;
                serde_json::to_value(records).map_err(|e| e.to_string())?
            }
            "query_relations" => {
                let records =
                    crate::repositories::narrative::NarrativeRepository::current_relations(
                        self.connection,
                        self.project_id,
                        &entity_ids,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?;
                truncated =
                    crate::repositories::narrative::NarrativeRepository::has_more_relations(
                        self.connection,
                        self.project_id,
                        &entity_ids,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(records).map_err(|e| e.to_string())?
            }
            "query_obligations" => {
                let records = crate::repositories::narrative::NarrativeRepository::active_obligations_for_relevance(self.connection, self.project_id, &entity_ids, &references, bounds.max_results as i64).map_err(|e| e.to_string())?;
                truncated =
                    crate::repositories::narrative::NarrativeRepository::has_more_obligations(
                        self.connection,
                        self.project_id,
                        &entity_ids,
                        &references,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(records).map_err(|e| e.to_string())?
            }
            "query_task_states" => {
                let (records, was_truncated) =
                    crate::repositories::narrative::NarrativeRepository::task_states_bounded(
                        self.connection,
                        self.project_id,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?;
                truncated = was_truncated;
                serde_json::to_value(records).map_err(|e| e.to_string())?
            }
            "query_volume_progress" => {
                if input.get("volumeId").and_then(Value::as_str).is_none() {
                    return Ok((
                        json!({"status":"INSUFFICIENT_DATA","owner":"B06::VolumeProgressView"}),
                        Vec::new(),
                        false,
                    ));
                }
                volume_progress_from_authority(
                    self.connection,
                    input,
                    self.project_id,
                    crate::repositories::narrative::NarrativeRepository::project_revision(
                        self.connection,
                        self.project_id,
                    )
                    .map_err(|e| e.to_string())?,
                )?
            }
            "query_plan_stale" => {
                let current_revision =
                    crate::repositories::narrative::NarrativeRepository::project_revision(
                        self.connection,
                        self.project_id,
                    )
                    .map_err(|e| e.to_string())?;
                let context = context_input_from_authority(
                    self.connection,
                    input,
                    self.project_id,
                    current_revision,
                )?;
                let current_digest = crate::narrative::relevant_state_digest(&context)?;
                let compiled_revision = input
                    .get("compiledFromProjectRevision")
                    .or_else(|| input.get("projectRevision"))
                    .and_then(Value::as_i64)
                    .ok_or_else(|| {
                        "CAPABILITY_INPUT_INVALID:compiledFromProjectRevision".to_string()
                    })?;
                let Some(compiled_digest) =
                    input.get("compiledFromStateDigest").and_then(Value::as_str)
                else {
                    return Ok((
                        json!({"status":"INSUFFICIENT_DATA","currentStateDigest":current_digest,"owner":"B06::PlanStale"}),
                        Vec::new(),
                        false,
                    ));
                };
                let status = evaluate_plan_stale(
                    compiled_revision,
                    compiled_digest,
                    current_revision,
                    &current_digest,
                );
                serde_json::to_value(json!({"status":match status { PlanStaleStatus::Valid => "VALID", PlanStaleStatus::PlanStale => "PLAN_STALE" },"currentStateDigest":current_digest,"owner":"B06::PlanStale"})).map_err(|e| e.to_string())?
            }
            "query_plan_route" => {
                if input.get("volumeId").and_then(Value::as_str).is_none() {
                    return Ok((
                        json!({"status":"INSUFFICIENT_DATA","owner":"B07::PlanningRoute"}),
                        Vec::new(),
                        false,
                    ));
                }
                let mut bounded_input = input.clone();
                bounded_input["maxResults"] = json!(bounds.max_results);
                let value = plan_route_from_authority(
                    self.connection,
                    &bounded_input,
                    self.project_id,
                    crate::repositories::narrative::NarrativeRepository::project_revision(
                        self.connection,
                        self.project_id,
                    )
                    .map_err(|e| e.to_string())?,
                )?;
                truncated = value
                    .get("truncated")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                value
            }
            "diagnose_planning_constraints" => {
                if input.get("volumeId").and_then(Value::as_str).is_none() {
                    return Ok((
                        json!({"diagnosisType":"EVIDENCE_GAP","supportStatus":"INSUFFICIENT","status":"INSUFFICIENT_DATA","owner":"H02::PlanningDiagnostics"}),
                        Vec::new(),
                        false,
                    ));
                }
                let mut bounded_input = input.clone();
                bounded_input["maxResults"] = json!(bounds.max_results);
                let route = plan_route_from_authority(
                    self.connection,
                    &bounded_input,
                    self.project_id,
                    crate::repositories::narrative::NarrativeRepository::project_revision(
                        self.connection,
                        self.project_id,
                    )
                    .map_err(|e| e.to_string())?,
                )?;
                truncated = route
                    .get("truncated")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let scope = adapter_scope(input, self.project_id, capability_id)?;
                let blocked_task_ids = route["taskReality"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|task| task["value"]["status"].as_str() == Some("BLOCKED"))
                    .filter_map(|task| task["stateId"].as_str())
                    .collect::<Vec<_>>();
                let scope_note = if truncated {
                    "仅展示边界内的部分 Route，不能视为全部约束"
                } else {
                    "Route 在当前查询边界内完整返回"
                };
                let goal_contract_refs = crate::repositories::narrative::NarrativeRepository::goal_contract_refs_for_volume(
                    self.connection,
                    self.project_id,
                    input.get("volumeId").and_then(Value::as_str).unwrap_or_default(),
                    bounds.max_results as i64,
                )
                .map_err(|e| e.to_string())?;
                let chapter_plan_ref = route
                    .get("currentChapterPlan")
                    .cloned()
                    .unwrap_or(Value::Null);
                let chapter_plan_payload = chapter_plan_ref
                    .as_object()
                    .and_then(|_| serde_json::from_value::<crate::generated::types::ArtifactRef>(chapter_plan_ref.clone()).ok())
                    .and_then(|reference| crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(self.connection, self.project_id, &reference).ok().flatten());
                let stale = if let Some(plan) = chapter_plan_payload.as_ref() {
                    let current_revision =
                        crate::repositories::narrative::NarrativeRepository::project_revision(
                            self.connection,
                            self.project_id,
                        )
                        .map_err(|e| e.to_string())?;
                    let context = context_input_from_authority(
                        self.connection,
                        input,
                        self.project_id,
                        current_revision,
                    )?;
                    let current_digest = crate::narrative::relevant_state_digest(&context)?;
                    plan.get("compiledFromProjectRevision").and_then(Value::as_i64)
                        .zip(plan.get("compiledFromStateDigest").and_then(Value::as_str))
                        .map(|(compiled_revision, compiled_digest)| json!({"status":match evaluate_plan_stale(compiled_revision, compiled_digest, current_revision, &current_digest) { PlanStaleStatus::Valid => "VALID", PlanStaleStatus::PlanStale => "PLAN_STALE" },"compiledFromProjectRevision":compiled_revision,"compiledFromStateDigest":compiled_digest,"currentProjectRevision":current_revision,"currentStateDigest":current_digest}))
                        .unwrap_or_else(|| json!({"status":"INSUFFICIENT_DATA"}))
                } else {
                    json!({"status":"INSUFFICIENT_DATA"})
                };
                let asks_stale =
                    input
                        .get("question")
                        .and_then(Value::as_str)
                        .is_some_and(|question| {
                            question.to_lowercase().contains("stale")
                                || question.contains("过期")
                                || question.contains("陈旧")
                        });
                let support_status = if stale["status"] == "PLAN_STALE" {
                    "CONFLICTING"
                } else if stale["status"] == "INSUFFICIENT_DATA" && asks_stale {
                    "INSUFFICIENT"
                } else {
                    "SUPPORTED"
                };
                let claim = if stale["status"] == "PLAN_STALE" {
                    "当前 ChapterPlan 的 compiled state digest 与权威当前状态不一致"
                } else {
                    "当前规划约束由权威 Route、Task Reality、Continuity 与 Reservation 共同决定"
                };
                let continuity_threads = route
                    .get("continuityThreads")
                    .cloned()
                    .unwrap_or_else(|| json!([]));
                let reservations = route
                    .get("futureReservations")
                    .cloned()
                    .unwrap_or_else(|| json!([]));
                json!({"diagnosisType":"PLANNING_CONSTRAINTS","supportStatus":support_status,"claim":claim,"projectId":scope.project_id,"projectRevision":scope.project_revision,"volumeIntentRef":route["volumeIntentRef"],"volumePlanRef":route["volumePlanRef"],"explanation":{"selectedTaskReason":"依据当前 Route 的 Task Reality 与依赖关系选择 Required/Climax/Reserved Task","blockedTaskIds":blocked_task_ids,"capacityReason":route["planningPressure"]["remainingChapterCapacity"],"requiredTasksRemaining":route["planningPressure"]["requiredTasksRemaining"],"reservationRestriction":"仅允许 Reservation Authority 当前卷已授权的 foreshadow/reveal 行为","continuityRestriction":"当前 Open/Suspended Thread 必须在 ChapterPlan 中给出 disposition；DEFER_WITH_BRIDGE 必须提供 bridge","scopeNote":scope_note,"stale":stale},"goalContractRefs":goal_contract_refs,"continuityThreads":continuity_threads,"futureReservations":reservations,"currentChapterPlan":chapter_plan_ref,"provenance":{"goalContractRefs":goal_contract_refs,"volumeIntentRef":route["volumeIntentRef"],"volumePlanRef":route["volumePlanRef"],"routeVersion":route["routeVersion"],"planningPressure":route["planningPressure"],"taskReality":route["taskReality"],"continuityThreads":continuity_threads,"futureReservations":reservations,"chapterPlanRef":chapter_plan_ref,"stale":stale},"truncated":route["truncated"],"recommendedNextAction":if stale["status"] == "PLAN_STALE" { "RECOMPILE_NEXT_CHAPTER" } else { "INSPECT_ROUTE" }})
            }
            "query_foreshadows" => {
                let chapters = input
                    .get("chapterId")
                    .and_then(Value::as_str)
                    .map(|id| vec![id])
                    .unwrap_or_default();
                let records =
                    crate::repositories::narrative::NarrativeRepository::active_foreshadows(
                        self.connection,
                        self.project_id,
                        &chapters,
                        &references,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?;
                truncated =
                    crate::repositories::narrative::NarrativeRepository::has_more_foreshadows(
                        self.connection,
                        self.project_id,
                        &chapters,
                        &references,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(records).map_err(|e| e.to_string())?
            }
            "query_mysteries" => {
                let records = crate::repositories::narrative::NarrativeRepository::open_mysteries(
                    self.connection,
                    self.project_id,
                    &references,
                    bounds.max_results as i64,
                )
                .map_err(|e| e.to_string())?;
                truncated =
                    crate::repositories::narrative::NarrativeRepository::has_more_mysteries(
                        self.connection,
                        self.project_id,
                        &references,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?;
                serde_json::to_value(records).map_err(|e| e.to_string())?
            }
            "query_dependency_edges" => {
                let (records, was_truncated) = crate::repositories::narrative::NarrativeRepository::dependency_edges_for_entities_bounded(self.connection, self.project_id, &entity_ids, bounds.max_results as i64).map_err(|e| e.to_string())?;
                truncated = was_truncated;
                serde_json::to_value(records).map_err(|e| e.to_string())?
            }
            "query_model_runs" => {
                let (items, was_truncated) =
                    crate::repositories::narrative::NarrativeRepository::model_runs_bounded(
                        self.connection,
                        self.project_id,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?;
                truncated = was_truncated;
                let status = if items.is_empty() { "NO_ROWS" } else { "PASS" };
                json!({"items":items,"owner":"Repository","status":status})
            }
            "query_commit_receipts" => {
                let (items, was_truncated) =
                    crate::repositories::narrative::NarrativeRepository::commit_receipts_bounded(
                        self.connection,
                        self.project_id,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?;
                truncated = was_truncated;
                let status = if items.is_empty() { "NO_ROWS" } else { "PASS" };
                json!({"items":items,"owner":"Repository","status":status})
            }
            "query_integrity_status" => {
                let (items, was_truncated) =
                    crate::repositories::narrative::NarrativeRepository::integrity_status_bounded(
                        self.connection,
                        self.project_id,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?;
                truncated = was_truncated;
                let status = if items.is_empty() { "NO_ROWS" } else { "PASS" };
                json!({"items":items,"owner":"IntegrityService","status":status})
            }
            "diagnose_canon_evidence" => {
                let scope = adapter_scope(input, self.project_id, capability_id)?;
                let refs = references
                    .iter()
                    .map(|reference| (*reference).to_owned())
                    .collect();
                serde_json::to_value(DiagnosticEngine::canon_evidence(
                    &scope,
                    input
                        .get("question")
                        .and_then(Value::as_str)
                        .unwrap_or(capability_id),
                    refs,
                ))
                .map_err(|e| e.to_string())?
            }
            "diagnose_knowledge_event" => {
                let scope = adapter_scope(input, self.project_id, capability_id)?;
                let knowledge =
                    crate::repositories::narrative::NarrativeRepository::knowledge_for_entities(
                        self.connection,
                        self.project_id,
                        &entity_ids,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .next();
                let finding = if let Some(knowledge) = knowledge {
                    let chain = crate::repositories::narrative::NarrativeRepository::establishing_event_for_knowledge(self.connection, self.project_id, &knowledge).map_err(|e| e.to_string())?.filter(|(_, evidence)| references.is_empty() || references.iter().any(|reference| Some(*reference) == evidence.event_id.as_deref() || Some(*reference) == evidence.fact_id.as_deref() || Some(*reference) == Some(evidence.evidence_id.as_str()) || Some(*reference) == Some(evidence.source_artifact_id.as_str())));
                    let (event, evidence) = chain
                        .map(|(event, evidence)| (Some(event), Some(evidence)))
                        .unwrap_or((None, None));
                    DiagnosticEngine::knowledge_event(
                        &scope,
                        input
                            .get("question")
                            .and_then(Value::as_str)
                            .unwrap_or(capability_id),
                        &knowledge,
                        event.as_ref(),
                        evidence.as_ref(),
                    )
                } else {
                    crate::harness::grounded_finding(
                        &scope,
                        "EVIDENCE_GAP",
                        input
                            .get("question")
                            .and_then(Value::as_str)
                            .unwrap_or(capability_id),
                        Vec::new(),
                        Vec::new(),
                    )
                };
                serde_json::to_value(finding).map_err(|e| e.to_string())?
            }
            "diagnose_thread_evidence" => {
                let scope = adapter_scope(input, self.project_id, capability_id)?;
                serde_json::to_value(DiagnosticEngine::thread_evidence(
                    &scope,
                    input
                        .get("question")
                        .and_then(Value::as_str)
                        .unwrap_or(capability_id),
                    references.first().copied(),
                    references.get(1).copied(),
                    references.get(2).copied(),
                ))
                .map_err(|e| e.to_string())?
            }
            "diagnose_task_evidence" => {
                let scope = adapter_scope(input, self.project_id, capability_id)?;
                serde_json::to_value(DiagnosticEngine::task_evidence(
                    &scope,
                    input
                        .get("question")
                        .and_then(Value::as_str)
                        .unwrap_or(capability_id),
                    entity_ids.first().copied(),
                    references.first().copied(),
                ))
                .map_err(|e| e.to_string())?
            }
            "diagnose_projection_authority" => {
                let scope = adapter_scope(input, self.project_id, capability_id)?;
                let current_revision =
                    crate::repositories::narrative::NarrativeRepository::project_revision(
                        self.connection,
                        self.project_id,
                    )
                    .map_err(|e| e.to_string())?;
                let projection_revision = input
                    .get("projectionRevision")
                    .and_then(Value::as_i64)
                    .unwrap_or(current_revision);
                serde_json::to_value(DiagnosticEngine::projection_authority(
                    &scope,
                    input
                        .get("question")
                        .and_then(Value::as_str)
                        .unwrap_or(capability_id),
                    "projection",
                    "authority",
                    projection_revision,
                    current_revision,
                ))
                .map_err(|e| e.to_string())?
            }
            _ => return Err("CAPABILITY_UNSUPPORTED_BY_ADAPTER".into()),
        };
        let mut source_refs = match &value {
            Value::Array(items) => items
                .iter()
                .filter_map(|item| {
                    item.get("factId")
                        .or_else(|| item.get("eventId"))
                        .or_else(|| item.get("evidenceId"))
                        .or_else(|| item.get("artifactId"))
                        .or_else(|| item.get("sourceArtifactId"))
                        .or_else(|| item.get("threadId"))
                        .or_else(|| item.get("relationId"))
                        .or_else(|| item.get("obligationId"))
                        .or_else(|| item.get("stateId"))
                        .or_else(|| item.get("edgeId"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .collect(),
            Value::Object(object) => object
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| {
                    [
                        "modelRunId",
                        "receiptId",
                        "integrityRunId",
                        "artifactId",
                        "sourceArtifactId",
                        "factId",
                        "eventId",
                        "evidenceId",
                        "findingId",
                    ]
                    .into_iter()
                    .find_map(|key| item.get(key).and_then(Value::as_str).map(ToOwned::to_owned))
                })
                .collect(),
            _ => Vec::new(),
        };
        if matches!(
            capability_id,
            "query_plan_route" | "diagnose_planning_constraints"
        ) {
            for key in ["volumeIntentRef", "volumePlanRef"] {
                if let Some(reference) = value
                    .get(key)
                    .and_then(|item| item.get("artifactId"))
                    .and_then(Value::as_str)
                {
                    source_refs.push(reference.to_owned());
                }
            }
        }
        Ok((value, source_refs, truncated))
    }
}

pub fn support_session_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let question = required_string(&request, "question")?;
    let requested_at = request
        .get("requestedAt")
        .and_then(Value::as_str)
        .unwrap_or("1970-01-01T00:00:00Z");
    let mode = request
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("DIAGNOSE");
    let requested_scope = request
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("PROJECT");
    let revision = state
        .manager
        .project_revision(project_id)
        .map_err(|e| e.to_string())?;
    let scope = resolve_project_scope(project_id, revision, question, mode, requested_scope)?;
    let mut plan = plan_support(&scope, question, requested_at)?;
    let registry = CapabilityRegistry::read_only_r1();
    let executor = BoundedCapabilityExecutor {
        registry: registry.clone(),
    };
    let input = request.clone();
    for step in &mut plan.steps {
        step.typed_args = input.clone();
    }
    let mut calls = Vec::new();
    let mut evidence_ledger = Vec::new();
    let mut source_refs = BTreeSet::new();
    let mut result_digests = Vec::new();
    let mut evidence_found = false;
    state
        .manager
        .with_read_connection(project_id, requested_at, |connection| {
            let adapter = RepositoryReadAdapter {
                connection,
                project_id,
            };
            for step in &mut plan.steps {
                step.status = crate::harness::SupportStepStatus::RUNNING;
                let spec = registry
                    .specs
                    .get(&step.capability_id)
                    .ok_or_else(|| ManagerError::ProjectCommand("CAPABILITY_UNKNOWN".into()))?;
                let call_input = if step.capability_id == "query_knowledge" {
                    json!({"entityIds":request_strings(&input,"entityIds")})
                } else {
                    input.clone()
                };
                let call = CapabilityCallEnvelope {
                    call_id: format!("call:{}", step.step_id),
                    capability_id: spec.capability_id.clone(),
                    capability_version: spec.version.clone(),
                    spec_digest: spec.spec_digest.clone(),
                    mode: mode.into(),
                    project_id: project_id.into(),
                    project_revision: revision,
                    input_digest: sha256_canonical(&call_input)
                        .map_err(|e| ManagerError::ProjectCommand(e.to_string()))?,
                    input: call_input,
                    requested_bounds: RequestedBounds {
                        max_results: 100,
                        max_serialized_bytes: 65_536,
                    },
                };
                let result = executor
                    .execute(&adapter, &call)
                    .map_err(ManagerError::ProjectCommand)?;
                source_refs.extend(result.source_refs.clone());
                result_digests.push(result.result_digest.clone());
                if step.capability_id == "query_evidence" {
                    evidence_found = result
                        .result
                        .as_array()
                        .is_some_and(|items| !items.is_empty());
                }
                evidence_ledger.push(EvidenceLedgerEntry {
                    call_id: result.call_id.clone(),
                    capability_id: step.capability_id.clone(),
                    capability_version: spec.version.clone(),
                    spec_digest: spec.spec_digest.clone(),
                    project_id: project_id.into(),
                    project_revision: revision,
                    input_digest: call.input_digest.clone(),
                    source_refs: result.source_refs.clone(),
                    result_digest: result.result_digest.clone(),
                    truncated: result.truncated,
                    bounds_applied: result.bounds_applied.clone(),
                    started_at: requested_at.into(),
                    finished_at: requested_at.into(),
                    status: result.status.clone(),
                });
                calls.push(result);
                step.status = crate::harness::SupportStepStatus::DONE;
            }
            Ok::<(), ManagerError>(())
        })
        .map_err(|e| e.to_string())?;
    let requires_evidence = question.contains("知道") || question.contains("承诺");
    let planning_question = [
        "高潮",
        "required task",
        "未完成",
        "stale",
        "揭露",
        "秘密",
        "capacity",
        "容量",
        "planner",
        "承诺",
        "任务",
        "规划",
    ]
    .iter()
    .any(|term| question.to_lowercase().contains(term));
    let diagnostic_finding = if question.contains("知道") {
        state
            .manager
            .with_read_connection(project_id, requested_at, |connection| {
                let entity_ids = request_strings(&request, "entityIds");
                let entity_refs = entity_ids.iter().map(String::as_str).collect::<Vec<_>>();
                let knowledge =
                    crate::repositories::narrative::NarrativeRepository::knowledge_for_entities(
                        connection,
                        project_id,
                        &entity_refs,
                        1,
                    )
                    .map_err(|e| ManagerError::ProjectCommand(e.to_string()))?
                    .into_iter()
                    .next();
                let references = request_strings(&request, "references");
                let reference_refs = references.iter().map(String::as_str).collect::<Vec<_>>();
                let evidence =
                    crate::repositories::narrative::NarrativeRepository::evidence_for_refs_bounded(
                        connection,
                        project_id,
                        &reference_refs,
                        1,
                    )
                    .map_err(|e| ManagerError::ProjectCommand(e.to_string()))?
                    .0
                    .into_iter()
                    .next();
                let finding = if let Some(item) = knowledge {
                    let exact_chain = crate::repositories::narrative::NarrativeRepository::establishing_event_for_knowledge(
                        connection,
                        project_id,
                        &item,
                    )
                    .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
                    let (event, exact_evidence) = exact_chain
                        .filter(|(_, evidence)| {
                            references.is_empty()
                                || references.iter().any(|reference| {
                                    Some(reference.as_str()) == evidence.event_id.as_deref()
                                        || Some(reference.as_str()) == evidence.fact_id.as_deref()
                                        || Some(reference.as_str())
                                            == Some(evidence.evidence_id.as_str())
                                        || Some(reference.as_str())
                                            == Some(evidence.source_artifact_id.as_str())
                                })
                        })
                        .map(|(event, evidence)| (Some(event), Some(evidence)))
                        .unwrap_or((None, evidence));
                    Some(DiagnosticEngine::knowledge_event(
                        &scope,
                        question,
                        &item,
                        event.as_ref(),
                        exact_evidence.as_ref(),
                    ))
                } else {
                    None
                };
                Ok::<_, ManagerError>(finding)
            })
            .map_err(|e| e.to_string())?
    } else if planning_question {
        calls
            .iter()
            .find(|call| call.result.get("diagnosisType").is_some())
            .map(|call| {
                let support_status = match call.result.get("supportStatus").and_then(Value::as_str)
                {
                    Some("SUPPORTED") => crate::harness::SupportStatus::SUPPORTED,
                    Some("CONFLICTING") => crate::harness::SupportStatus::CONFLICTING,
                    Some("INFERRED") => crate::harness::SupportStatus::INFERRED,
                    _ => crate::harness::SupportStatus::INSUFFICIENT,
                };
                crate::harness::Finding {
                    finding_id: format!("finding:{}", &scope.question_digest[..16]),
                    diagnosis_type: call.result["diagnosisType"]
                        .as_str()
                        .unwrap_or("PLANNING_CONSTRAINTS")
                        .into(),
                    project_id: project_id.into(),
                    project_revision: revision,
                    claim: call.result["claim"].as_str().unwrap_or(question).into(),
                    support_status,
                    source_refs: call.source_refs.clone(),
                    conflicting_refs: Vec::new(),
                    impact_scopes: Vec::new(),
                    recommended_next_action: call.result["recommendedNextAction"]
                        .as_str()
                        .unwrap_or("INSPECT_ROUTE")
                        .into(),
                }
            })
    } else {
        None
    };
    let finding = diagnostic_finding.unwrap_or_else(|| {
        grounded_finding(
            &scope,
            if requires_evidence && !evidence_found {
                "EVIDENCE_GAP"
            } else {
                "NO_ISSUE"
            },
            question,
            if requires_evidence && !evidence_found {
                Vec::new()
            } else {
                source_refs.iter().cloned().collect()
            },
            Vec::new(),
        )
    });
    let plan_digest = sha256_canonical(&serde_json::to_value(&plan).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let checkpoint = SupportCheckpoint {
        checkpoint_id: format!("checkpoint:{}", &scope.question_digest[..16]),
        session_id: format!("session:{}", &scope.question_digest[..16]),
        project_id: project_id.into(),
        project_revision: revision,
        question_digest: scope.question_digest.clone(),
        plan_digest: plan_digest.clone(),
        completed_step_ids: plan.steps.iter().map(|step| step.step_id.clone()).collect(),
        capability_spec_digests: registry_digest_map(&registry),
        evidence_refs: source_refs.iter().cloned().collect(),
        last_result_digests: result_digests,
        requested_mode: mode.into(),
        requested_scope: requested_scope.into(),
        plan_steps: plan.steps.clone(),
        pending_authorization_ref: None,
        created_at: requested_at.into(),
    };
    let handle = state
        .manager
        .open(project_id, "", requested_at)
        .map_err(|e| e.to_string())?;
    crate::repositories::support::save_checkpoint(&handle, &checkpoint)
        .map_err(|e| e.to_string())?;
    let receipt = SupportSessionReceipt {
        session_id: checkpoint.session_id.clone(),
        question_digest: scope.question_digest,
        project_id: project_id.into(),
        project_revision: revision,
        plan_digest,
        capabilities_invoked: plan
            .steps
            .iter()
            .map(|step| step.capability_id.clone())
            .collect(),
        source_refs: source_refs.into_iter().collect(),
        finding_refs: vec![finding.finding_id.clone()],
        checkpoint_ref: Some(checkpoint.checkpoint_id),
        final_support_status: finding.support_status.clone(),
        started_at: requested_at.into(),
        finished_at: requested_at.into(),
    };
    Ok(
        json!({"plan":plan,"calls":calls,"evidenceLedger":evidence_ledger,"finding":finding,"receipt":receipt}),
    )
}

pub fn support_checkpoint_resume_impl(
    state: &KernelState,
    request: Value,
) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let checkpoint_id = required_string(&request, "checkpointId")?;
    let current_revision = state
        .manager
        .project_revision(project_id)
        .map_err(|e| e.to_string())?;
    let registry = CapabilityRegistry::read_only_r1();
    let checkpoint = state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::repositories::support::load_checkpoint(connection, project_id, checkpoint_id)
                .map_err(|e| ManagerError::ProjectCommand(e.to_string()))
        })
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "CHECKPOINT_NOT_FOUND".to_string())?;
    let status = crate::harness::check_checkpoint_resume(
        &checkpoint,
        current_revision,
        &registry_digest_map(&registry),
    );
    let mut result = json!({"checkpointId":checkpoint_id,"projectId":project_id,"projectRevision":current_revision,"status":match status { crate::harness::CheckpointResume::Safe => "SAFE", crate::harness::CheckpointResume::Stale => "CHECKPOINT_STALE" }});
    if matches!(status, crate::harness::CheckpointResume::Stale) {
        let mut requery = checkpoint
            .plan_steps
            .first()
            .map(|step| step.typed_args.clone())
            .unwrap_or_else(|| json!({}));
        requery["projectId"] = json!(project_id);
        requery["scope"] = json!(checkpoint.requested_scope);
        result["affectedQuery"] = query_narrative_state_impl(state, requery)?;
        let scope = crate::harness::ProjectScope {
            project_id: project_id.into(),
            project_revision: current_revision,
            question_digest: checkpoint.question_digest.clone(),
            requested_mode: checkpoint.requested_mode.clone(),
            requested_scope: checkpoint.requested_scope.clone(),
        };
        let mut plan = crate::harness::SupportTaskPlan {
            plan_id: format!("plan:stale:{}", &checkpoint.question_digest[..16]),
            project_id: scope.project_id.clone(),
            based_on_project_revision: current_revision,
            question_digest: scope.question_digest.clone(),
            steps: checkpoint.plan_steps.clone(),
            status: "STALE_REQUERY".into(),
            created_at: "1970-01-01T00:00:00Z".into(),
            replan_trigger: None,
            prior_plan_digest: None,
            new_plan_digest: None,
        };
        if plan.steps.is_empty() {
            plan = crate::harness::plan_support(
                &scope,
                "checkpoint affected facts requery",
                "1970-01-01T00:00:00Z",
            )?;
        }
        result["replan"] = serde_json::to_value(crate::harness::bounded_replan(
            &plan,
            crate::harness::ReplanTrigger::CHECKPOINT_STALE,
            "1970-01-01T00:00:00Z",
        )?)
        .map_err(|e| e.to_string())?;
    }
    Ok(result)
}

fn runtime_job_spec(request: &Value, command_id: &str) -> Result<JobSpec, String> {
    let payload = request.get("payload");
    if payload
        .and_then(|value| value.get("jobId"))
        .and_then(Value::as_str)
        .is_some()
    {
        return Err("new production jobs must use factory-derived stable ids".into());
    }
    let job_id = payload
        .and_then(|value| value.get("jobId"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or(StableIdentityFactory::from_command(
            StableIdKind::Job,
            command_id,
        )?);
    let stages = payload
        .and_then(|value| value.get("stages"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| vec!["outline".into(), "draft".into(), "review".into()]);
    Ok(JobSpec {
        job_id,
        job_type: payload
            .and_then(|value| value.get("jobType"))
            .and_then(Value::as_str)
            .unwrap_or("CHAPTER")
            .to_string(),
        parent_job_id: payload
            .and_then(|value| value.get("parentJobId"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        book_id: payload
            .and_then(|value| value.get("bookId"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        volume_id: payload
            .and_then(|value| value.get("volumeId"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        chapter_id: payload
            .and_then(|value| value.get("chapterId"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        stages,
    })
}

struct KernelRepairOwner<'a> {
    connection: &'a libsql_rusqlite::Connection,
    project_id: &'a str,
    now: &'a str,
}

impl crate::repair::TypedRepairOwner for KernelRepairOwner<'_> {
    fn execute(
        &self,
        operation: &crate::repair::RepairOperation,
    ) -> Result<crate::repair::OwnerExecution, String> {
        match operation.operation.as_str() {
            "refresh_runtime_projection" => {
                SearchService::refresh_runtime_projection(
                    self.connection,
                    self.project_id,
                    self.now,
                )
                .map_err(|error| error.to_string())?;
                Ok(crate::repair::OwnerExecution {
                    owner: "B06::RuntimeProjectionOwner".into(),
                    owner_receipt_ref: None,
                })
            }
            "rebuild_projection" => {
                SearchService::ensure_projection(self.connection, self.project_id, self.now)
                    .map_err(|error| error.to_string())?;
                Ok(crate::repair::OwnerExecution {
                    owner: "B04::ProjectionOwner".into(),
                    owner_receipt_ref: None,
                })
            }
            "rebuild_fts" => {
                SearchService::rebuild(self.connection, self.project_id, self.now)
                    .map_err(|error| error.to_string())?;
                Ok(crate::repair::OwnerExecution {
                    owner: "B04::FTSMaintenanceOwner".into(),
                    owner_receipt_ref: None,
                })
            }
            "recompile_chapter_plan" => {
                let chapter_id = operation
                    .params
                    .get("chapterId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "H03_RECOMPILE_CHAPTER_REQUIRED".to_string())?;
                let plan = operation
                    .params
                    .get("plan")
                    .ok_or_else(|| "H03_RECOMPILE_PLAN_REQUIRED".to_string())?;
                let revision =
                    crate::repositories::narrative::NarrativeRepository::project_revision(
                        self.connection,
                        self.project_id,
                    )
                    .map_err(|error| error.to_string())?;
                let volume_id =
                    crate::repositories::narrative::NarrativeRepository::chapter_volume_id(
                        self.connection,
                        self.project_id,
                        chapter_id,
                    )
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "CHAPTER_NOT_FOUND".to_string())?;
                let execution_mode = operation
                    .params
                    .get("executionMode")
                    .and_then(Value::as_str)
                    .unwrap_or("LONG_FORM");
                let authority_request = json!({
                    "projectId":self.project_id,"chapterId":chapter_id,"volumeId":volume_id,
                    "executionMode":execution_mode,"entityIds":operation.params.get("entityIds").cloned().unwrap_or_else(|| json!([])),
                    "references":operation.params.get("references").cloned().unwrap_or_else(|| json!([]))
                });
                let command_id = format!(
                    "h03:recompile:{}",
                    &sha256_canonical(
                        &json!({"projectId":self.project_id,"chapterId":chapter_id,"plan":plan})
                    )
                    .map_err(|error| error.to_string())?[..24]
                );
                let receipt = crate::planning::recompile_chapter_plan_owner(
                    self.connection,
                    self.project_id,
                    chapter_id,
                    plan,
                    &authority_request,
                    execution_mode,
                    &command_id,
                    &format!("{command_id}:idempotency"),
                    revision,
                    self.now,
                    None,
                )?;
                Ok(crate::repair::OwnerExecution {
                    owner: "B07::PlanningOwner".into(),
                    owner_receipt_ref: Some(receipt.receipt_id),
                })
            }
            "backup_restore_maintenance" => {
                let backup_path = operation
                    .params
                    .get("backupPath")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "H03_BACKUP_PATH_REQUIRED".to_string())?;
                let manifest = crate::repositories::ops::BackupService::create_snapshot(
                    self.connection,
                    std::path::Path::new(backup_path),
                    self.project_id,
                    self.now,
                )
                .map_err(|error| error.to_string())?;
                crate::repositories::ops::BackupService::persist_manifest(
                    self.connection,
                    &manifest,
                )
                .map_err(|error| error.to_string())?;
                Ok(crate::repair::OwnerExecution {
                    owner: "B04::BackupService".into(),
                    owner_receipt_ref: Some(manifest.backup_id),
                })
            }
            "execute_change_request" => {
                let change_request_id = operation
                    .params
                    .get("changeRequestId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "H03_CHANGE_REQUEST_ID_REQUIRED".to_string())?;
                let impact_digest = operation
                    .params
                    .get("impactDigest")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "H03_CHANGE_IMPACT_DIGEST_REQUIRED".to_string())?;
                let revision =
                    crate::repositories::narrative::NarrativeRepository::project_revision(
                        self.connection,
                        self.project_id,
                    )
                    .map_err(|error| error.to_string())?;
                let command_id = operation
                    .params
                    .get("commandId")
                    .and_then(Value::as_str)
                    .unwrap_or("h03:execute-change-request");
                let idempotency_key = operation
                    .params
                    .get("idempotencyKey")
                    .and_then(Value::as_str)
                    .unwrap_or(command_id);
                let mut request_payload = json!({
                    "changeRequestId":change_request_id,
                    "impactDigest":impact_digest
                });
                for key in ["retconPlanId", "planDigest"] {
                    if let Some(value) = operation.params.get(key) {
                        request_payload[key] = value.clone();
                    }
                }
                let request = json!({"payload":request_payload});
                let result = crate::change_retcon::apply_approved_on_connection(
                    self.connection,
                    self.project_id,
                    command_id,
                    idempotency_key,
                    revision,
                    self.now,
                    &request,
                )?;
                Ok(crate::repair::OwnerExecution {
                    owner: crate::change_retcon::OWNER_NAME.into(),
                    owner_receipt_ref: result
                        .get("receiptId")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                })
            }
            "replan_volume_route" => {
                let volume_id = operation
                    .params
                    .get("volumeId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "B09_REPLAN_VOLUME_REQUIRED".to_string())?;
                let revision =
                    crate::repositories::narrative::NarrativeRepository::project_revision(
                        self.connection,
                        self.project_id,
                    )
                    .map_err(|error| error.to_string())?;
                let operation_digest = sha256_canonical(
                    &serde_json::to_value(operation).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let command_id = operation
                    .params
                    .get("commandId")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("h03:replan:{}", &operation_digest[..24]));
                let idempotency_key = operation
                    .params
                    .get("idempotencyKey")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("{command_id}:idempotency"));
                let request = crate::volume_lifecycle::RouteReplanRequest {
                    project_id: self.project_id.into(),
                    volume_id: volume_id.into(),
                    expected_revision: revision,
                    command_id,
                    idempotency_key,
                    trigger: operation
                        .params
                        .get("trigger")
                        .and_then(Value::as_str)
                        .unwrap_or("H03_AUTHORIZED_REPLAN")
                        .into(),
                    reason: operation
                        .params
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    evidence_refs: operation
                        .params
                        .get("evidenceRefs")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect(),
                    expected_reality_digest: operation
                        .params
                        .get("expectedRealityDigest")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    acknowledged_responsibility_refs: operation
                        .params
                        .get("acknowledgedResponsibilityRefs")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect(),
                    now: self.now.into(),
                };
                let result = crate::volume_lifecycle::VolumeLifecycleOwner::replan_volume_route(
                    self.connection,
                    request,
                )?;
                Ok(crate::repair::OwnerExecution {
                    owner: "B09::VolumeLifecycleOwner".into(),
                    owner_receipt_ref: result
                        .get("receiptId")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                })
            }
            "rebuild_planning_state_projection" => {
                let volume_id = operation
                    .params
                    .get("volumeId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "B09_PROJECTION_VOLUME_REQUIRED".to_string())?;
                let projection =
                    crate::volume_lifecycle::VolumeLifecycleOwner::rebuild_planning_state(
                        self.connection,
                        self.project_id,
                        volume_id,
                        self.now,
                    )?;
                Ok(crate::repair::OwnerExecution {
                    owner: "B09::PlanningStateProjectionOwner".into(),
                    owner_receipt_ref: projection
                        .get("projectionDigest")
                        .and_then(Value::as_str)
                        .map(|digest| format!("planning-projection:{digest}")),
                })
            }
            _ => Err("H03_OWNER_OPERATION_UNKNOWN".into()),
        }
    }

    fn verify(&self, operation: &crate::repair::RepairOperation) -> Result<Value, String> {
        match operation.operation.as_str() {
            "execute_change_request" => {
                let change_request_id = operation
                    .params
                    .get("changeRequestId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "H03_CHANGE_REQUEST_ID_REQUIRED".to_string())?;
                let status = crate::repositories::change_retcon::load_change_request(
                    self.connection,
                    self.project_id,
                    change_request_id,
                )
                .map_err(|error| error.to_string())?
                .map(|item| item.status == "APPLIED")
                .unwrap_or(false);
                Ok(json!({
                    "operation":operation.operation,
                    "kind":"b12_change_postverify",
                    "status":if status {"PASS"} else {"FAIL"},
                    "observed":status,
                    "userMessage":if status {"已完成并复核"} else {"暂时无法完成复核"}
                }))
            }
            "refresh_runtime_projection" => {
                let updated_at = DurableJobService::runtime_projection_updated_at(
                    self.connection,
                    self.project_id,
                )
                .map_err(|error| error.to_string())?;
                Ok(
                    json!({"operation":operation.operation,"kind":"projection_current","status":if updated_at == self.now {"PASS"} else {"FAIL"},"observed":updated_at == self.now,"observedUpdatedAt":updated_at}),
                )
            }
            "rebuild_projection" | "rebuild_fts" => {
                let healthy = SearchService::projection_health(self.connection, self.project_id)
                    .map_err(|error| error.to_string())?;
                Ok(
                    json!({"operation":operation.operation,"kind":"projection_current","status":if healthy {"PASS"} else {"FAIL"},"observed":healthy,"projectionHealthy":healthy}),
                )
            }
            "recompile_chapter_plan" => {
                let chapter_id = operation
                    .params
                    .get("chapterId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "H03_RECOMPILE_CHAPTER_REQUIRED".to_string())?;
                let expected = operation
                    .params
                    .get("plan")
                    .ok_or_else(|| "H03_RECOMPILE_PLAN_REQUIRED".to_string())?;
                let actual =
                    crate::repositories::narrative::NarrativeRepository::b07_chapter_plan_payload(
                        self.connection,
                        self.project_id,
                        chapter_id,
                    )
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "H03_B07_CHAPTER_PLAN_NOT_FOUND".to_string())?;
                let revision =
                    crate::repositories::narrative::NarrativeRepository::project_revision(
                        self.connection,
                        self.project_id,
                    )
                    .map_err(|error| error.to_string())?;
                let volume_id =
                    crate::repositories::narrative::NarrativeRepository::chapter_volume_id(
                        self.connection,
                        self.project_id,
                        chapter_id,
                    )
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "CHAPTER_NOT_FOUND".to_string())?;
                let execution_mode = operation
                    .params
                    .get("executionMode")
                    .and_then(Value::as_str)
                    .unwrap_or("LONG_FORM");
                let authority = json!({
                    "projectId":self.project_id,"chapterId":chapter_id,"volumeId":volume_id,
                    "executionMode":execution_mode,"entityIds":operation.params.get("entityIds").cloned().unwrap_or_else(|| json!([])),
                    "references":operation.params.get("references").cloned().unwrap_or_else(|| json!([]))
                });
                let input = context_input_from_authority(
                    self.connection,
                    &authority,
                    self.project_id,
                    revision,
                )?;
                let current_digest = crate::narrative::relevant_state_digest(&input)?;
                let stale = crate::narrative::evaluate_plan_stale(
                    actual
                        .get("compiledFromProjectRevision")
                        .and_then(Value::as_i64)
                        .unwrap_or(-1),
                    actual
                        .get("compiledFromStateDigest")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    revision,
                    &current_digest,
                );
                let observed = if matches!(stale, PlanStaleStatus::Valid) {
                    "VALID"
                } else {
                    "PLAN_STALE"
                };
                Ok(
                    json!({"operation":operation.operation,"kind":"chapter_plan_stale","status":if actual == *expected && observed == "VALID" {"PASS"} else {"FAIL"},"owner":"B07::PlanningOwner","observed":observed,"stale":observed}),
                )
            }
            "backup_restore_maintenance" => {
                let backup_path = operation
                    .params
                    .get("backupPath")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "H03_BACKUP_PATH_REQUIRED".to_string())?;
                let exists = std::path::Path::new(backup_path).is_file();
                Ok(
                    json!({"operation":operation.operation,"kind":"backup_exists","status":if exists {"PASS"} else {"FAIL"},"observed":exists,"backupReadable":exists}),
                )
            }
            "replan_volume_route" => {
                let volume_id = operation
                    .params
                    .get("volumeId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "B09_REPLAN_VOLUME_REQUIRED".to_string())?;
                let route_ref =
                    crate::repositories::narrative::NarrativeRepository::volume_plan_route(
                        self.connection,
                        self.project_id,
                        volume_id,
                    )
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "B09_REPLAN_ROUTE_NOT_FOUND".to_string())?;
                let actual =
                    crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                        self.connection,
                        self.project_id,
                        &route_ref,
                    )
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "B09_REPLAN_ROUTE_PAYLOAD_NOT_FOUND".to_string())?;
                let observed = actual
                    .get("routeVersion")
                    .and_then(Value::as_i64)
                    .is_some_and(|version| version > 1)
                    && actual
                        .get("routeInvariants")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .any(|invariant| {
                            invariant.as_str() == Some("B09_REPLAN_DERIVED_FROM_COMPILED_INPUT")
                        });
                Ok(json!({
                    "operation": operation.operation,
                    "kind": "volume_route_replanned",
                    "status": if observed {"PASS"} else {"FAIL"},
                    "observed": observed,
                    "owner": "B09::VolumeLifecycleOwner",
                    "routeRef": route_ref,
                }))
            }
            "rebuild_planning_state_projection" => {
                let volume_id = operation
                    .params
                    .get("volumeId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "B09_PROJECTION_VOLUME_REQUIRED".to_string())?;
                let projection = crate::repositories::volume_lifecycle::VolumeLifecycleRepository::planning_projection(
                    self.connection,
                    self.project_id,
                    volume_id,
                )
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "B09_PROJECTION_NOT_FOUND".to_string())?;
                let observed = projection.get("volumeId").and_then(Value::as_str)
                    == Some(volume_id)
                    && projection
                        .get("projectionDigest")
                        .and_then(Value::as_str)
                        .is_some_and(|digest| digest.len() == 64);
                Ok(json!({
                    "operation": operation.operation,
                    "kind": "planning_state_projection_current",
                    "status": if observed {"PASS"} else {"FAIL"},
                    "observed": observed,
                    "owner": "B09::PlanningStateProjectionOwner"
                }))
            }
            _ => Err("H03_POSTCONDITION_UNAVAILABLE".into()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_repair_command(
    state: &KernelState,
    request: &Value,
    command_type: &str,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    requested_at: &str,
    expected_revision: Option<i64>,
    digest: &str,
) -> Result<Value, String> {
    match command_type {
        "repair_plan_create" => {
            if request.pointer("/payload/dryRun").and_then(Value::as_bool) != Some(true) {
                return Err("H03_DRY_RUN_REQUIRED".into());
            }
            let plan: crate::repair::RepairPlan = serde_json::from_value(
                request
                    .pointer("/payload/repairPlan")
                    .cloned()
                    .ok_or_else(|| "H03_INVALID:repairPlan".to_string())?,
            )
            .map_err(|error| format!("H03_INVALID:repairPlan:{error}"))?;
            if plan.project_id != project_id {
                return Err("H03_INVALID:projectId mismatch".into());
            }
            let spec = project_command_spec(
                project_id,
                project_id,
                command_id,
                idempotency_key,
                command_type,
                digest,
                expected_revision,
                requested_at,
                false,
            );
            let outcome = state
                .manager
                .commit_project_command(spec, |connection| {
                    crate::repair::save_plan(connection, &plan)
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
                })
                .map_err(|error| error.to_string())?;
            Ok(project_command_response(&outcome_receipt(outcome)))
        }
        "repair_authorize" => {
            let authorization: crate::repair::RepairAuthorization = serde_json::from_value(
                request
                    .pointer("/payload/authorization")
                    .cloned()
                    .ok_or_else(|| "H03_INVALID:authorization".to_string())?,
            )
            .map_err(|error| format!("H03_INVALID:authorization:{error}"))?;
            if authorization.project_id != project_id {
                return Err("H03_INVALID:projectId mismatch".into());
            }
            let spec = project_command_spec(
                project_id,
                project_id,
                command_id,
                idempotency_key,
                command_type,
                digest,
                expected_revision,
                requested_at,
                false,
            );
            let outcome = state
                .manager
                .commit_project_command(spec, |connection| {
                    crate::repair::issue_authorization(connection, &authorization)
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
                })
                .map_err(|error| error.to_string())?;
            Ok(project_command_response(&outcome_receipt(outcome)))
        }
        "repair_execute" => {
            let plan_id = request
                .pointer("/payload/repairPlanId")
                .and_then(Value::as_str)
                .ok_or_else(|| "H03_INVALID:repairPlanId".to_string())?;
            let authorization_id = request
                .pointer("/payload/authorizationId")
                .and_then(Value::as_str)
                .ok_or_else(|| "H03_INVALID:authorizationId".to_string())?;
            let handle = state
                .manager
                .open(project_id, "", requested_at)
                .map_err(|error| error.to_string())?;
            let owner = KernelRepairOwner {
                connection: &handle,
                project_id,
                now: requested_at,
            };
            let receipt = crate::repair::execute(
                &handle,
                &owner,
                project_id,
                plan_id,
                authorization_id,
                requested_at,
            )
            .map_err(|error| error.to_string())?;
            Ok(json!({
                "commandId":command_id,
                "projectId":project_id,
                "finalStatus": if receipt.status == "REPAIR_VERIFIED" { "committed" } else { "failed" },
                "message": "H03 授权修复已执行并完成复核",
                "receipt": receipt,
            }))
        }
        _ => Err("H03_UNSUPPORTED_COMMAND".into()),
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_volume_lifecycle_command(
    state: &KernelState,
    request: &Value,
    command_type: &str,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    requested_at: &str,
    expected_revision: Option<i64>,
) -> Result<Value, String> {
    let expected_revision = expected_revision
        .ok_or_else(|| "IPC_INVALID:expectedRevision required for B09 lifecycle".to_string())?;
    let payload = request
        .get("payload")
        .ok_or_else(|| "IPC_INVALID:payload".to_string())?;
    let handle = state
        .manager
        .open(project_id, "", requested_at)
        .map_err(|error| error.to_string())?;
    let result = match command_type {
        "volume_replan_route" => {
            let volume_id = required_string(payload, "volumeId")?;
            crate::volume_lifecycle::VolumeLifecycleOwner::replan_volume_route(
                &handle,
                crate::volume_lifecycle::RouteReplanRequest {
                    project_id: project_id.into(),
                    volume_id: volume_id.into(),
                    expected_revision,
                    command_id: command_id.into(),
                    idempotency_key: idempotency_key.into(),
                    trigger: payload
                        .get("trigger")
                        .and_then(Value::as_str)
                        .unwrap_or("USER_AUTHORIZED_REPLAN")
                        .into(),
                    reason: required_string(payload, "reason")?.into(),
                    evidence_refs: string_array(payload.get("evidenceRefs")),
                    expected_reality_digest: payload
                        .get("expectedRealityDigest")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    acknowledged_responsibility_refs: string_array(
                        payload.get("acknowledgedResponsibilityRefs"),
                    ),
                    now: requested_at.into(),
                },
            )?
        }
        "volume_close" => crate::volume_lifecycle::VolumeLifecycleOwner::close_volume(
            &handle,
            crate::volume_lifecycle::VolumeCloseRequest {
                project_id: project_id.into(),
                volume_id: required_string(payload, "volumeId")?.into(),
                expected_revision,
                command_id: command_id.into(),
                idempotency_key: idempotency_key.into(),
                reason: required_string(payload, "reason")?.into(),
                fault_injection: payload
                    .get("faultInjection")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                now: requested_at.into(),
            },
        )?,
        "volume_handoff" => crate::volume_lifecycle::VolumeLifecycleOwner::handoff_next_volume(
            &handle,
            crate::volume_lifecycle::NextVolumeHandoffRequest {
                project_id: project_id.into(),
                current_volume_id: required_string(payload, "currentVolumeId")?.into(),
                next_volume_id: required_string(payload, "nextVolumeId")?.into(),
                expected_revision,
                command_id: command_id.into(),
                idempotency_key: idempotency_key.into(),
                title: required_string(payload, "title")?.into(),
                chapter_start: payload
                    .get("chapterStart")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| "IPC_INVALID:payload.chapterStart".to_string())?,
                chapter_end: payload
                    .get("chapterEnd")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| "IPC_INVALID:payload.chapterEnd".to_string())?,
                target_words: payload
                    .get("targetWords")
                    .and_then(Value::as_i64)
                    .ok_or_else(|| "IPC_INVALID:payload.targetWords".to_string())?,
                now: requested_at.into(),
            },
        )?,
        _ => return Err("B09_UNSUPPORTED_LIFECYCLE_COMMAND".into()),
    };
    Ok(json!({
        "commandId": command_id,
        "projectId": project_id,
        "finalStatus": "committed",
        "message": match command_type {
            "volume_close" => "卷已完成原子关闭并生成 VolumeExitState。",
            "volume_handoff" => "下一卷已完成权威 handoff。",
            _ => "Volume route 已由 B09 生命周期 owner 原子更新。",
        },
        "receiptId": result.get("receiptId"),
        "beforeRevision": expected_revision,
        "afterRevision": result.get("projectRevision"),
        "result": result,
    }))
}

// Runtime command handling intentionally carries the complete command envelope
// into the ProjectManager-owned transaction so control receipts bind to it.
#[allow(clippy::too_many_arguments)]
fn execute_runtime_command(
    state: &KernelState,
    request: &Value,
    command_type: &str,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    requested_at: &str,
    expected_revision: Option<i64>,
    digest: &str,
) -> Result<Value, String> {
    let spec = project_command_spec(
        project_id,
        project_id,
        command_id,
        idempotency_key,
        command_type,
        digest,
        expected_revision,
        requested_at,
        true,
    );
    let outcome = state
        .manager
        .commit_project_command(spec, |connection| {
            match command_type {
                "production_start" => {
                    let job = runtime_job_spec(request, command_id)
                        .map_err(ManagerError::ProjectCommand)?;
                    DurableJobService::start_job(connection, &job, requested_at)
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
                    DurableJobService::transition(
                        connection,
                        project_id,
                        &job.job_id,
                        "RUNNING",
                        requested_at,
                    )
                    .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
                    DurableJobService::set_runtime_state(
                        connection,
                        project_id,
                        "ACTIVE",
                        None,
                        requested_at,
                    )
                    .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?
                }
                "production_pause" | "production_resume" | "production_cancel" => {
                    let job_id = request
                        .pointer("/payload/jobId")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                        .unwrap_or(
                            DurableJobService::latest_job_id(connection, project_id)
                                .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?,
                        );
                    let (next, run_state, reason) = match command_type {
                        "production_pause" => ("PAUSED", "PAUSED", Some("USER_PAUSE")),
                        "production_resume" => ("RUNNING", "ACTIVE", None),
                        _ => ("CANCELLED", "FAILED", None),
                    };
                    if command_type == "production_resume" {
                        let worker_id = request
                            .pointer("/payload/workerId")
                            .and_then(Value::as_str)
                            .unwrap_or("production-ipc");
                        DurableJobService::transition_for_worker(
                            connection,
                            project_id,
                            &job_id,
                            next,
                            worker_id,
                            requested_at,
                        )
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
                    } else {
                        DurableJobService::transition(
                            connection,
                            project_id,
                            &job_id,
                            next,
                            requested_at,
                        )
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
                    }
                    if command_type == "production_pause" || command_type == "production_cancel" {
                        DurableJobService::release_lease(
                            connection,
                            project_id,
                            &job_id,
                            requested_at,
                        )
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
                    }
                    DurableJobService::set_runtime_state(
                        connection,
                        project_id,
                        run_state,
                        reason,
                        requested_at,
                    )
                    .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?
                }
                _ => {
                    return Err(ManagerError::ProjectCommand(
                        "unsupported runtime command".into(),
                    ))
                }
            }
            Ok(())
        })
        .map_err(|error| error.to_string())?;
    Ok(project_command_response(&outcome_receipt(outcome)))
}

#[allow(clippy::too_many_arguments)]
fn execute_planning_command(
    state: &KernelState,
    request: &Value,
    command_type: &str,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    requested_at: &str,
    expected_revision: Option<i64>,
) -> Result<Value, String> {
    let mut payload = request
        .pointer("/payload/plan")
        .cloned()
        .unwrap_or(Value::Null);
    let (artifact_type, scope, scope_key) = match command_type {
        "planning_finalize_book_plan" => (
            "BookPlan",
            json!({"level":"PROJECT"}),
            project_id.to_string(),
        ),
        "planning_create_volume_intent" | "planning_create_volume_plan" => {
            let volume_id = request
                .pointer("/payload/volumeId")
                .and_then(Value::as_str)
                .ok_or_else(|| "IPC_INVALID:payload.volumeId".to_string())?;
            (
                if command_type.ends_with("volume_intent") {
                    "VolumeIntent"
                } else {
                    "VolumePlan"
                },
                json!({"level":"VOLUME","volumeId":volume_id}),
                volume_id.to_string(),
            )
        }
        "planning_compile_chapter_plan" => {
            let chapter_id = request
                .pointer("/payload/chapterId")
                .and_then(Value::as_str)
                .ok_or_else(|| "IPC_INVALID:payload.chapterId".to_string())?;
            (
                "ChapterPlan",
                json!({"level":"CHAPTER","chapterId":chapter_id}),
                chapter_id.to_string(),
            )
        }
        _ => return Err("IPC_UNSUPPORTED_PLANNING_COMMAND".into()),
    };
    match artifact_type {
        "BookPlan" => {}
        "VolumeIntent" | "VolumePlan" => {}
        "ChapterPlan" => {}
        _ => {}
    }
    let handle = state
        .manager
        .open(project_id, "", requested_at)
        .map_err(|e| e.to_string())?;
    if artifact_type == "VolumeIntent" {
        let volume_exists = crate::repositories::narrative::NarrativeRepository::volume_exists(
            &handle, project_id, &scope_key,
        )
        .map_err(|e| e.to_string())?;
        if !volume_exists {
            return Err("VOLUME_NOT_FOUND".into());
        }
        if !crate::repositories::narrative::NarrativeRepository::approved_book_plan_exists(
            &handle, project_id,
        )
        .map_err(|e| e.to_string())?
        {
            return Err("BOOK_PLAN_APPROVED_REQUIRED".into());
        }
        if payload.is_null() {
            let target_chapters =
                crate::repositories::narrative::NarrativeRepository::volume_target_chapters(
                    &handle, project_id, &scope_key,
                )
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "VOLUME_NOT_FOUND".to_string())?;
            payload = json!({"payloadSchema":"volume-intent.r1","volumeFunction":"current volume route","entryConditions":[],"mustAchieve":[],"mustNotBreak":[],"requiredExitConditions":[],"futureReservationRefs":[],"crossVolumeResponsibilityRefs":[],"targetChapterRange":{"min":1,"preferred":target_chapters,"max":target_chapters},"targetWordRange":{"preferred":target_chapters.saturating_mul(1000)}});
        }
        crate::planning::validate_volume_intent(&payload)?;
    }
    if artifact_type == "BookPlan" {
        let canon_refs = string_array(request.pointer("/payload/openingCanonRefs"));
        let evidence_refs = string_array(request.pointer("/payload/signingEvidenceRefs"));
        let canon_reachable = canon_refs.iter().try_fold(true, |reachable, reference| {
            Ok::<_, String>(reachable && crate::repositories::narrative::NarrativeRepository::opening_canon_reference_exists(&handle, project_id, reference).map_err(|e| e.to_string())?)
        })?;
        let receipt_passes = evidence_refs.iter().try_fold(false, |passed, reference| {
            Ok::<_, String>(
                passed
                    || crate::repositories::narrative::NarrativeRepository::signing_receipt_passes(
                        &handle,
                        project_id,
                        reference,
                        &canon_refs,
                    )
                    .map_err(|e| e.to_string())?,
            )
        })?;
        if canon_refs.len() < 3 || !canon_reachable || !receipt_passes {
            return Err("BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE".into());
        }
        payload = crate::planning::approve_book_plan(&payload, true, &canon_refs, &evidence_refs)?;
    }
    if artifact_type == "VolumePlan" {
        let intent = crate::repositories::narrative::NarrativeRepository::canonical_head_ref(
            &handle,
            project_id,
            "VolumeIntent",
            &scope_key,
        )
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "VOLUME_INTENT_NOT_FOUND".to_string())?;
        if payload.is_null() {
            let intent_payload =
                crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                    &handle, project_id, &intent,
                )
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "VOLUME_INTENT_PAYLOAD_NOT_FOUND".to_string())?;
            let target_chapters = intent_payload
                .pointer("/targetChapterRange/max")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            payload = json!({"payloadSchema":"volume-plan.r1","volumeIntentRef":intent,"routeVersion":1,"tasks":[],"routeInvariants":[],"requiredExitConditions":[],"capacityBudget":{"minChapters":1,"preferredChapters":target_chapters,"maxChapters":target_chapters}});
        }
        crate::planning::validate_volume_route(&payload)?;
        crate::planning::validate_volume_route_reference(&payload, &intent)?;
    }
    if artifact_type == "ChapterPlan" {
        let chapter_id = request
            .pointer("/payload/chapterId")
            .and_then(Value::as_str)
            .ok_or_else(|| "IPC_INVALID:plan.chapterId".to_string())?;
        let volume_id = crate::repositories::narrative::NarrativeRepository::chapter_volume_id(
            &handle, project_id, chapter_id,
        )
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "CHAPTER_NOT_FOUND".to_string())?;
        let route_ref = crate::repositories::narrative::NarrativeRepository::volume_plan_route(
            &handle, project_id, &volume_id,
        )
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "VOLUME_PLAN_NOT_FOUND".to_string())?;
        let route = crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
            &handle, project_id, &route_ref,
        )
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "VOLUME_PLAN_PAYLOAD_NOT_FOUND".to_string())?;
        let route_task_ids = route
            .get("tasks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|task| task.get("taskId").and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let current_revision =
            crate::repositories::narrative::NarrativeRepository::project_revision(
                &handle, project_id,
            )
            .map_err(|e| e.to_string())?;
        let mut authority_request = request.clone();
        authority_request["volumeId"] = json!(volume_id);
        authority_request["chapterId"] = json!(chapter_id);
        let context = context_input_from_authority(
            &handle,
            &authority_request,
            project_id,
            current_revision,
        )?;
        let triggered_threads = context
            .continuity_threads
            .iter()
            .filter(|thread| matches!(thread.authority_level.as_str(), "A" | "B"))
            .map(|thread| thread.thread_id.clone())
            .collect::<Vec<_>>();
        let route_states = crate::repositories::narrative::NarrativeRepository::task_states_by_ids(
            &handle,
            project_id,
            &route_task_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            100,
        )
        .map_err(|e| e.to_string())?;
        let task_states = route_states
            .iter()
            .map(|state| {
                (
                    state.state_id.as_str(),
                    state
                        .value
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("NOT_STARTED"),
                )
            })
            .collect::<Vec<_>>();
        let current_digest = crate::narrative::relevant_state_digest(&context)?;
        let compiled_revision = payload
            .get("compiledFromProjectRevision")
            .and_then(Value::as_i64)
            .ok_or_else(|| "IPC_INVALID:plan.compiledFromProjectRevision".to_string())?;
        let compiled_digest = payload
            .get("compiledFromStateDigest")
            .and_then(Value::as_str)
            .ok_or_else(|| "IPC_INVALID:plan.compiledFromStateDigest".to_string())?;
        if matches!(
            evaluate_plan_stale(
                compiled_revision,
                compiled_digest,
                current_revision,
                &current_digest
            ),
            PlanStaleStatus::PlanStale
        ) {
            return Err("PLAN_STALE".into());
        }
        let context_ref: crate::generated::types::ArtifactRef = serde_json::from_value(
            payload
                .get("contextManifestRef")
                .cloned()
                .ok_or_else(|| "CONTEXT_MANIFEST_REF_REQUIRED".to_string())?,
        )
        .map_err(|e| format!("CONTEXT_MANIFEST_REF_INVALID:{e}"))?;
        if crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
            &handle,
            project_id,
            &context_ref,
        )
        .map_err(|e| e.to_string())?
        .is_none()
        {
            return Err("CONTEXT_MANIFEST_NOT_FOUND".into());
        }
        let model_run_id = request
            .pointer("/payload/modelRunId")
            .and_then(Value::as_str)
            .ok_or_else(|| "PLANNING_MODEL_RUN_REQUIRED".to_string())?;
        if !crate::repositories::narrative::NarrativeRepository::completed_planning_model_run_exists(
            &handle,
            project_id,
            model_run_id,
            &context_ref.artifact_id,
        )
        .map_err(|e| e.to_string())?
        {
            return Err("PLANNING_MODEL_RUN_INVALID".into());
        }
        if !crate::repositories::narrative::NarrativeRepository::exactly_one_primary_planning_call(
            &handle,
            project_id,
            &context_ref.artifact_id,
        )
        .map_err(|e| e.to_string())?
        {
            return Err("PLANNING_PRIMARY_CALL_COUNT_INVALID".into());
        }
        if !crate::repositories::narrative::NarrativeRepository::planning_model_run_binds_plan(
            &handle,
            project_id,
            model_run_id,
            &context_ref.artifact_id,
            &payload,
        )
        .map_err(|e| e.to_string())?
        {
            return Err("PLANNING_MODEL_OUTPUT_MISMATCH".into());
        }
        crate::planning::validate_current_chapter_plan(
            &payload,
            chapter_id,
            &route_task_ids,
            &triggered_threads,
        )?;
        crate::planning::validate_task_reality(&payload, &route, &task_states)?;
        let current_volume_no = crate::repositories::narrative::NarrativeRepository::volume_number(
            &handle, project_id, &volume_id,
        )
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "VOLUME_NOT_FOUND".to_string())?;
        let reservations = [
            crate::repositories::narrative::NarrativeRepository::latest_artifact_payload(
                &handle, project_id, "BookPlan",
            )
            .map_err(|e| e.to_string())?,
            crate::repositories::narrative::NarrativeRepository::artifact_payload(
                &handle,
                project_id,
                "VolumeIntent",
                &volume_id,
            )
            .map_err(|e| e.to_string())?,
        ]
        .into_iter()
        .flatten()
        .flat_map(|payload| {
            payload
                .get("futureReservations")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .collect::<Vec<_>>();
        crate::planning::validate_reservation_actions(&payload, &reservations, current_volume_no)?;
    }
    if artifact_type == "ChapterPlan" {
        let chapter_id = request
            .pointer("/payload/chapterId")
            .and_then(Value::as_str)
            .ok_or_else(|| "IPC_INVALID:payload.chapterId".to_string())?;
        let volume_id = crate::repositories::narrative::NarrativeRepository::chapter_volume_id(
            &handle, project_id, chapter_id,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "CHAPTER_NOT_FOUND".to_string())?;
        let mut owner_request = request.clone();
        owner_request["volumeId"] = json!(volume_id);
        owner_request["executionMode"] = json!("LONG_FORM");
        let receipt = crate::planning::recompile_chapter_plan_owner(
            &handle,
            project_id,
            chapter_id,
            &payload,
            &owner_request,
            "LONG_FORM",
            command_id,
            idempotency_key,
            expected_revision.ok_or_else(|| "IPC_INVALID:expectedRevision".to_string())?,
            requested_at,
            request
                .pointer("/payload/modelRunId")
                .and_then(Value::as_str),
        )?;
        return Ok(
            json!({"finalStatus":"committed","receipt":receipt,"owner":"B07::PlanningOwner"}),
        );
    }
    let artifact_id = format!("plan:{}:{}", artifact_type.to_lowercase(), scope_key);
    let version = crate::repositories::narrative::NarrativeRepository::next_artifact_version(
        &handle,
        project_id,
        &artifact_id,
    )
    .map_err(|e| e.to_string())?;
    let hash = sha256_canonical(&payload).map_err(|e| e.to_string())?;
    let model_run_id = request
        .pointer("/payload/modelRunId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({"artifactId":artifact_id,"projectId":project_id,"artifactType":artifact_type,"scope":scope,"version":version,"schemaVersion":"m0a.artifact.v1","artifactStatus":"CANDIDATE","payload":payload,"payloadHash":hash,"dependencyRefs":[],"provenance":{"createdBy":"B07::PlanningOwner","commandId":command_id,"modelRunId":model_run_id},"createdAt":requested_at})).map_err(|e| e.to_string())?;
    let bundle: CommitBundle = serde_json::from_value(json!({"commitId":format!("commit:{}",command_id),"projectId":project_id,"commandId":command_id,"idempotencyKey":idempotency_key,"expectedProjectRevision":expected_revision,"contractVersion":"m0a.v1","createdAt":requested_at,"artifacts":[artifact],"headUpdates":[{"artifactType":artifact_type,"scope":scope,"artifactId":artifact_id,"version":version,"payloadHash":hash}]})).map_err(|e| e.to_string())?;
    let receipt =
        CommitService::commit(&handle, &bundle, requested_at).map_err(|e| e.to_string())?;
    Ok(json!({"finalStatus":"committed","receipt":receipt,"owner":"B07::PlanningOwner"}))
}

#[allow(clippy::too_many_arguments)]
fn save_provider_settings(
    state: &KernelState,
    request: &Value,
    project_id: Option<&str>,
    command_id: &str,
    idempotency_key: &str,
    requested_at: &str,
    expected_revision: Option<i64>,
    digest: &str,
) -> Result<Value, String> {
    let settings = request
        .pointer("/payload/settings")
        .cloned()
        .ok_or_else(|| "P01_SETTINGS_REQUIRED".to_string())?;
    let config = provider_config(&settings)?;
    if let Some(api_key) = settings
        .get("apiKey")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty() && !value.starts_with('•'))
    {
        crate::repositories::runtime::OsKeychainSecretStore::new(
            "com.moheng.ai-novel-production",
            "provider-api-key",
        )
        .set(&config.provider_id, api_key)
        .map_err(|error| format!("P01_SECRET_STORE:{error}"))?;
    }
    if let Some(project_id) = project_id {
        let spec = project_command_spec(
            project_id,
            project_id,
            command_id,
            idempotency_key,
            "settings_save",
            digest,
            expected_revision,
            requested_at,
            true,
        );
        let outcome = state
            .manager
            .commit_project_command(spec, |connection| {
                crate::repositories::provider::ProviderSettingsRepository::write_project(
                    connection,
                    project_id,
                    &settings,
                    requested_at,
                )
                .map(|_| ())
                .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
            })
            .map_err(|error| error.to_string())?;
        Ok(project_command_response(&outcome_receipt(outcome)))
    } else {
        let saved = crate::repositories::provider::ProviderSettingsRepository::write_global(
            &state.paths,
            &settings,
        )
        .map_err(|error| error.to_string())?;
        Ok(
            json!({"commandId":command_id,"finalStatus":"committed","message":"Provider 设置已保存","settings":saved,"providerId":config.provider_id,"secretReturned":false}),
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_b11_command(
    state: &KernelState,
    request: &Value,
    command_type: &str,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    requested_at: &str,
    expected_revision: Option<i64>,
    digest: &str,
) -> Result<Value, String> {
    if command_type == "opening_start_creating" {
        let expected_revision = expected_revision
            .ok_or_else(|| "IPC_INVALID:expectedRevision required for B11 opening".to_string())?;
        let spec = project_command_spec(
            project_id,
            project_id,
            command_id,
            idempotency_key,
            command_type,
            digest,
            Some(expected_revision),
            requested_at,
            false,
        );
        let outcome = state
            .manager
            .commit_project_command(spec, |connection| {
                crate::project_entry::start_creating(connection, project_id)
                    .map_err(ManagerError::ProjectCommand)
            })
            .map_err(|error| error.to_string())?;
        let (start, receipt) = match outcome {
            ProjectCommandOutcome::Replayed(receipt) => {
                let start = state
                    .manager
                    .with_read_connection(project_id, requested_at, |connection| {
                        crate::project_entry::start_creating(connection, project_id)
                            .map_err(ManagerError::ProjectCommand)
                    })
                    .map_err(|error| error.to_string())?;
                (start, receipt)
            }
            ProjectCommandOutcome::Committed { value, receipt } => (value, receipt),
        };
        let mut response = project_command_response(&receipt);
        response["message"] = json!("开篇准备已完成，已进入正式创作中心。");
        response["startCreating"] = start;
        return Ok(response);
    }
    let expected_revision = expected_revision
        .ok_or_else(|| "IPC_INVALID:expectedRevision required for B11 command".to_string())?;
    let handle = state
        .manager
        .open(project_id, "", requested_at)
        .map_err(|error| error.to_string())?;
    let receipt = match command_type {
        "pitch_regenerate" => crate::project_entry::commit_regenerate(
            &handle,
            project_id,
            command_id,
            idempotency_key,
            expected_revision,
            requested_at,
            request,
        ),
        "pitch_select" => crate::project_entry::commit_selection(
            &handle,
            project_id,
            command_id,
            idempotency_key,
            expected_revision,
            requested_at,
            request,
        ),
        "story_setup_save" => crate::project_entry::commit_story_setup(
            &handle,
            project_id,
            command_id,
            idempotency_key,
            expected_revision,
            requested_at,
            request,
        ),
        "opening_ready" => crate::project_entry::commit_opening_ready(
            &handle,
            project_id,
            command_id,
            idempotency_key,
            expected_revision,
            requested_at,
            request,
        ),
        _ => Err(format!("B11_UNSUPPORTED_COMMAND:{command_type}")),
    }
    .map_err(|error| error.to_string())?;
    let receipt = serde_json::to_value(receipt).map_err(|error| error.to_string())?;
    Ok(json!({
        "commandId":command_id,
        "projectId":project_id,
        "resultProjectId":project_id,
        "finalStatus":"committed",
        "message":match command_type { "pitch_regenerate" => "新的故事方向已生成，请由你选择。", "pitch_select" => "故事方向已由你确认，进入故事核心。", "story_setup_save" => "故事核心已保存，等待你批准开篇准备。", "opening_ready" => "开篇准备已完成，已生成第一章的最小规划。", _ => "B11 操作已完成。" },
        "receiptId":receipt["receiptId"],
        "beforeRevision":receipt["beforeRevision"],
        "afterRevision":receipt["afterRevision"],
        "receipt":receipt
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
        "settings_save" | "global_settings_save" => {
            let id = project_id;
            save_provider_settings(
                state,
                &request,
                id,
                command_id,
                idempotency_key,
                requested_at,
                expected_revision,
                &digest,
            )
        }
        "pitch_regenerate"
        | "pitch_select"
        | "story_setup_save"
        | "opening_ready"
        | "opening_start_creating" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            execute_b11_command(
                state,
                &request,
                command_type,
                id,
                command_id,
                idempotency_key,
                requested_at,
                expected_revision,
                &digest,
            )
        }
        "planning_finalize_book_plan"
        | "planning_create_volume_intent"
        | "planning_create_volume_plan"
        | "planning_compile_chapter_plan" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            execute_planning_command(
                state,
                &request,
                command_type,
                id,
                command_id,
                idempotency_key,
                requested_at,
                expected_revision,
            )
        }
        "production_start" | "production_pause" | "production_resume" | "production_cancel" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            if command_type == "production_start" {
                if request
                    .pointer("/payload/chapterId")
                    .and_then(Value::as_str)
                    .is_none()
                {
                    return Err("B08_CHAPTER_REQUIRED".into());
                }
                return crate::production::run_chapter_production(
                    &state.manager,
                    state.production_gateway.as_ref(),
                    &request,
                );
            }
            execute_runtime_command(
                state,
                &request,
                command_type,
                id,
                command_id,
                idempotency_key,
                requested_at,
                expected_revision,
                &digest,
            )
        }
        "repair_plan_create" | "repair_authorize" | "repair_execute" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            execute_repair_command(
                state,
                &request,
                command_type,
                id,
                command_id,
                idempotency_key,
                requested_at,
                expected_revision,
                &digest,
            )
        }
        "change_request_create"
        | "impact_preflight"
        | "impact_expand"
        | "impact_resume"
        | "impact_cancel"
        | "change_request_decide"
        | "change_request_apply"
        | "retcon_plan_create"
        | "retcon_apply" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            crate::change_retcon::execute(
                &state.manager,
                command_type,
                id,
                command_id,
                idempotency_key,
                &digest,
                expected_revision,
                requested_at,
                &request,
            )
        }
        "dependency_summary_bootstrap" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            crate::change_retcon::bootstrap_dependency_summary(
                &state.manager,
                id,
                command_id,
                idempotency_key,
                &digest,
                expected_revision,
                requested_at,
            )
        }
        "volume_replan_route" | "volume_close" | "volume_handoff" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            execute_volume_lifecycle_command(
                state,
                &request,
                command_type,
                id,
                command_id,
                idempotency_key,
                requested_at,
                expected_revision,
            )
        }
        "endgame_contract_create" | "complete_book" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            if command_type == "endgame_contract_create" {
                create_endgame_contract_impl(
                    state,
                    &request,
                    id,
                    command_id,
                    idempotency_key,
                    requested_at,
                    expected_revision,
                )
            } else {
                complete_book_impl(
                    state,
                    id,
                    command_id,
                    idempotency_key,
                    requested_at,
                    expected_revision,
                )
            }
        }
        "project_create_from_idea" | "project_create_from_market" => {
            let title = request
                .pointer("/payload/title")
                .and_then(Value::as_str)
                .unwrap_or("未命名作品");
            let entry_kind = if command_type == "project_create_from_market" {
                crate::project_entry::EntryKind::Market
            } else {
                crate::project_entry::EntryKind::Idea
            };
            let b11_entry = command_type == "project_create_from_market"
                || request
                    .pointer("/payload/idea")
                    .and_then(Value::as_str)
                    .is_some_and(|idea| !idea.trim().is_empty());
            if b11_entry {
                crate::project_entry::validate_initial_request(&request, entry_kind)?;
            }
            let id = stable_project_id(command_id)?;
            if let Some(replayed) = replayed_project_command(state, &id, idempotency_key, &digest)?
            {
                return Ok(replayed);
            }
            let handle = match state.manager.overview(&id) {
                Ok((existing_title, _revision, _)) if existing_title == title => state
                    .manager
                    .open(&id, "", requested_at)
                    .map_err(|error| error.to_string())?,
                Ok(_) => return Err("IDEMPOTENCY_CONFLICT".to_string()),
                Err(crate::project_db::ManagerError::ProjectNotFound) => state
                    .manager
                    .create(&id, title, requested_at)
                    .map_err(|error| error.to_string())?,
                Err(error) => return Err(error.to_string()),
            };
            let entry_receipt = if b11_entry {
                Some(crate::project_entry::commit_initial(
                    &handle,
                    &id,
                    command_id,
                    idempotency_key,
                    requested_at,
                    &request,
                    entry_kind,
                )?)
            } else {
                None
            };
            drop(handle);
            let project_receipt = outcome_receipt(
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
                            None,
                            requested_at,
                            false,
                        ),
                        |_| Ok(()),
                    )
                    .map_err(|error| error.to_string())?,
            );
            let mut response = project_command_response(&project_receipt);
            if let Some(entry_receipt) = entry_receipt {
                response["entryReceipt"] =
                    serde_json::to_value(entry_receipt).map_err(|error| error.to_string())?;
            }
            Ok(response)
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
            let receipt = outcome_receipt(outcome);
            let catalog = CatalogRepository::open(&state.paths, requested_at)
                .map_err(|error| error.to_string())?;
            CatalogRepository::rename(&catalog, id, title, requested_at)
                .map_err(|error| error.to_string())?;
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
            let receipt = outcome_receipt(outcome);
            state
                .manager
                .set_lifecycle(id, "ARCHIVED", requested_at)
                .map_err(|error| error.to_string())?;
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
            let receipt = outcome_receipt(outcome);
            state
                .manager
                .set_lifecycle(id, "ACTIVE", requested_at)
                .map_err(|error| error.to_string())?;
            Ok(project_command_response(&receipt))
        }
        "project_duplicate" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            if request.pointer("/payload/targetProjectId").is_some() {
                return Err("PROTOCOL / STABLE_ID_OVERRIDE_FORBIDDEN".to_string());
            }
            let target = StableIdentityFactory::from_command(StableIdKind::Project, command_id)?;
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
            if !state
                .manager
                .has_valid_copy_receipt(id, &target)
                .map_err(|error| error.to_string())?
            {
                state
                    .manager
                    .duplicate(id, &target, requested_at)
                    .map_err(|error| error.to_string())?;
            }
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
                state
                    .manager
                    .delete_to_trash(id, requested_at)
                    .map_err(|error| error.to_string())?;
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
        "backup_create" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            let backup_dir = state
                .paths
                .project_dir(id)
                .map_err(|error| error.to_string())?
                .join("backups");
            let backup_path = backup_dir.join(format!("b04-{command_id}.db"));
            let outcome = state
                .manager
                .commit_backup(
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
                    &backup_path,
                )
                .map_err(|error| error.to_string())?;
            Ok(project_command_response(&outcome_receipt(outcome)))
        }
        "backup_restore" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            if let Some(replayed) = replayed_project_command(state, id, idempotency_key, &digest)? {
                return Ok(replayed);
            }
            let backup_name = request
                .pointer("/payload/backupName")
                .and_then(Value::as_str)
                .ok_or_else(|| "IPC_INVALID:payload.backupName".to_string())?;
            let checksum = request
                .pointer("/payload/checksumSha256")
                .and_then(Value::as_str)
                .ok_or_else(|| "IPC_INVALID:payload.checksumSha256".to_string())?;
            if backup_name.contains('/') || backup_name.contains('\\') || backup_name.is_empty() {
                return Err("IPC_INVALID:payload.backupName".to_string());
            }
            let backup_path = state
                .paths
                .project_dir(id)
                .map_err(|error| error.to_string())?
                .join("backups")
                .join(backup_name);
            let outcome = state
                .manager
                .restore_backup(
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
                    &backup_path,
                    checksum,
                )
                .map_err(|error| error.to_string())?;
            Ok(project_command_response(&outcome_receipt(outcome)))
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
pub fn query_market_strategy(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_market_strategy_impl(&state, request)
}

#[tauri::command]
pub fn query_story_directions(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_story_directions_impl(&state, request)
}

#[tauri::command]
pub fn query_story_setup(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_story_setup_impl(&state, request)
}

#[tauri::command]
pub fn query_opening_ready(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_opening_ready_impl(&state, request)
}

#[tauri::command]
pub fn query_assistant_repair_plan(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_assistant_repair_plan_impl(&state, request)
}

#[tauri::command]
pub fn query_production_center(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_production_center_impl(&state, request)
}

#[tauri::command]
pub fn search_fulltext(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_search_fulltext_impl(&state, request)
}

#[tauri::command]
pub fn query_volume_list(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_volume_list_impl(&state, request)
}

#[tauri::command]
pub fn query_reader_volumes(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_volume_list_impl(&state, request)
}

#[tauri::command]
pub fn query_chapter_list(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_chapter_list_impl(&state, request)
}

#[tauri::command]
pub fn query_chapter(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_single_chapter_impl(&state, request)
}

#[tauri::command]
pub fn query_chapter_final(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_single_chapter_impl(&state, request)
}

#[tauri::command]
pub fn query_volume_detail(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_volume_detail_impl(&state, request)
}

#[tauri::command]
pub fn query_decisions(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_decisions_impl(&state, request)
}

#[tauri::command]
pub fn query_chapter_versions(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_chapter_versions_impl(&state, request)
}

#[tauri::command]
pub fn query_canon(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_canon_impl(&state, request)
}

#[tauri::command]
pub fn query_retcon_context(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_retcon_context_impl(&state, request)
}

#[tauri::command]
pub fn query_book_health(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_book_health_impl(&state, request)
}

#[tauri::command]
pub fn query_history(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_history_impl(&state, request)
}

#[tauri::command]
pub fn query_change_request(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_change_request_impl(&state, request)
}

#[tauri::command]
pub fn query_dependency_summary_bootstrap(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_dependency_summary_bootstrap_impl(&state, request)
}

#[tauri::command]
pub fn query_narrative_state(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_narrative_state_impl(&state, request)
}

#[tauri::command]
pub fn query_continuity(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_continuity_impl(&state, request)
}

#[tauri::command]
pub fn query_volume_progress(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_volume_progress_impl(&state, request)
}

#[tauri::command]
pub fn query_context_manifest(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_context_manifest_impl(&state, request)
}

#[tauri::command]
pub fn query_plan_stale(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_plan_stale_impl(&state, request)
}

#[tauri::command]
pub fn query_plan_route(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_plan_route_impl(&state, request)
}

#[tauri::command]
pub fn query_volume_lifecycle(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_volume_lifecycle_impl(&state, request)
}

#[tauri::command]
pub fn support_session(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    support_session_impl(&state, request)
}

#[tauri::command]
pub fn support_checkpoint_resume(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    support_checkpoint_resume_impl(&state, request)
}

#[tauri::command]
pub fn query_endgame_status(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_endgame_status_impl(&state, request)
}

#[tauri::command]
pub fn query_completion_status(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_completion_status_impl(&state, request)
}

#[tauri::command]
pub fn query_endgame_mapping_options(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    query_endgame_mapping_options_impl(&state, request)
}

#[tauri::command]
pub fn query_endgame_advice(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_endgame_advice_impl(&state, request)
}

#[tauri::command]
pub fn query_completion_quality(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_completion_quality_impl(&state, request)
}

#[tauri::command]
pub fn execute_command(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    execute_command_impl(&state, request)
}

#[tauri::command]
fn provider_config(settings: &Value) -> Result<crate::provider::ProviderConfig, String> {
    let provider_id = settings
        .get("providerId")
        .and_then(Value::as_str)
        .or_else(|| settings.get("provider").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "P01_PROVIDER_ID_REQUIRED".to_string())?;
    let provider_id = if provider_id.eq_ignore_ascii_case("openai compatible") {
        "openai-compatible"
    } else {
        provider_id
    };
    let model_id = settings
        .get("modelId")
        .and_then(Value::as_str)
        .or_else(|| settings.get("model").and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "P01_MODEL_ID_REQUIRED".to_string())?;
    let endpoint = settings
        .get("endpoint")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(crate::provider::ProviderConfig {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
        enabled: settings
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(!endpoint.is_empty()),
        endpoint,
        timeout_ms: settings
            .get("timeoutMs")
            .and_then(Value::as_u64)
            .unwrap_or(30_000)
            .clamp(500, 120_000),
    })
}

#[tauri::command]
pub fn query_global_settings(state: State<'_, KernelState>) -> Result<Value, String> {
    let mut settings =
        crate::repositories::provider::ProviderSettingsRepository::read_global(&state.paths)
            .map_err(|error| error.to_string())?;
    apply_provider_status(&mut settings);
    Ok(settings)
}

#[tauri::command]
pub fn query_project_settings(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    let request = request.get("request").cloned().unwrap_or(request);
    let project_id = required_string(&request, "projectId")?;
    let connection = state
        .manager
        .open(project_id, "", "1970-01-01T00:00:00Z")
        .map_err(|error| error.to_string())?;
    let mut settings = crate::repositories::provider::ProviderSettingsRepository::read_project(
        &connection,
        project_id,
    )
    .map_err(|error| error.to_string())?;
    apply_provider_status(&mut settings);
    Ok(settings)
}

fn apply_provider_status(settings: &mut Value) {
    let provider_id = settings
        .get("providerId")
        .and_then(Value::as_str)
        .unwrap_or("openai-compatible");
    let has_secret = crate::repositories::runtime::OsKeychainSecretStore::new(
        "com.moheng.ai-novel-production",
        "provider-api-key",
    )
    .get(provider_id)
    .is_ok();
    settings["secretStatus"] = json!(if has_secret {
        "AVAILABLE"
    } else {
        "NOT_CONFIGURED"
    });
    settings["capabilityStatus"] = json!(if settings.get("enabled").and_then(Value::as_bool)
        == Some(true)
        && settings
            .get("endpoint")
            .and_then(Value::as_str)
            .is_some_and(|endpoint| !endpoint.is_empty())
        && has_secret
    {
        "AVAILABLE"
    } else {
        "UNAVAILABLE"
    });
}

#[tauri::command]
pub fn test_provider(_state: State<'_, KernelState>, settings: Value) -> Result<Value, String> {
    let settings = settings.get("settings").cloned().unwrap_or(settings);
    let config = provider_config(&settings)?;
    if !config.enabled || config.endpoint.is_empty() {
        return Err("P01_PROVIDER_NOT_CONFIGURED".into());
    }
    let secret = settings
        .get("apiKey")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.starts_with('•'))
        .map(ToOwned::to_owned)
        .or_else(|| {
            crate::repositories::runtime::OsKeychainSecretStore::new(
                "com.moheng.ai-novel-production",
                "provider-api-key",
            )
            .get(&config.provider_id)
            .ok()
        })
        .ok_or_else(|| "P01_AUTHENTICATION_ERROR:provider secret unavailable".to_string())?;
    let response = crate::provider::OpenAiCompatibleProvider::new(config.endpoint.clone())
        .invoke(
            &crate::provider::ProviderRequest {
                capability: "TEST_PROVIDER".into(),
                model_id: config.model_id,
                prompt: "Return JSON {\"ok\":true} without explanation.".into(),
                timeout_ms: config.timeout_ms,
            },
            &secret,
        )
        .map_err(|error| format!("P01_{}:{}", error.kind.code(), error.detail))?;
    Ok(json!({
        "finalStatus":"committed",
        "message":"Provider 连接测试成功",
        "providerId":config.provider_id,
        "usage":response.usage,
        "providerRequestId":response.provider_request_id,
        "secretReturned":false
    }))
}

#[tauri::command]
pub fn query_market_opportunities() -> Result<Value, String> {
    Ok(json!([]))
}

pub fn handler<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool {
    tauri::generate_handler![
        query_project_library,
        query_project_overview,
        query_market_strategy,
        query_story_directions,
        query_story_setup,
        query_opening_ready,
        query_assistant_repair_plan,
        query_production_center,
        search_fulltext,
        query_volume_list,
        query_reader_volumes,
        query_chapter_list,
        query_chapter,
        query_chapter_final,
        query_volume_detail,
        query_decisions,
        query_chapter_versions,
        query_canon,
        query_retcon_context,
        query_book_health,
        query_history,
        query_change_request,
        query_dependency_summary_bootstrap,
        query_narrative_state,
        query_continuity,
        query_volume_progress,
        query_context_manifest,
        query_plan_stale,
        query_plan_route,
        query_volume_lifecycle,
        support_session,
        support_checkpoint_resume,
        query_endgame_status,
        query_completion_status,
        query_endgame_mapping_options,
        query_endgame_advice,
        query_completion_quality,
        execute_command,
        query_global_settings,
        query_project_settings,
        test_provider,
        query_market_opportunities,
    ]
}
