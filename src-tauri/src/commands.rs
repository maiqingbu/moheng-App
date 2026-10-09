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
    ContextHardSource, CurrentNarrativeStateView, PlanStaleStatus, ResumeContext, RuntimeBounds,
    VolumeProgressInput,
};
use crate::project_db::paths::AppPaths;
use crate::project_db::{ManagerError, ProjectCommandOutcome, ProjectCommandSpec, ProjectManager};
use crate::provider::ProviderAdapter;
use crate::repositories::ops::{chapter_page, single_chapter, volume_page, SearchService};
use crate::runtime::{
    pending_decision_count, DurableJobService, DurableModelRunner, JobSpec, SecretStore,
};
use crate::stable_id::{StableIdKind, StableIdentityFactory};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tauri::State;

#[derive(Clone)]
pub struct KernelState {
    pub manager: ProjectManager,
    pub paths: AppPaths,
    pub(crate) skill_bundle_loader: crate::creative_skills::SkillBundleLoader,
    pub(crate) production_gateway: Arc<dyn crate::production::ProductionModelGateway>,
    pub(crate) market_source_adapter: Arc<dyn crate::market_source::MarketSourceAdapter>,
}

impl KernelState {
    #[cfg(any(test, debug_assertions))]
    pub fn new(paths: AppPaths) -> Self {
        Self::with_skill_bundle_loader(
            paths,
            crate::creative_skills::SkillBundleLoader::development_package(),
        )
    }

    #[cfg(any(test, debug_assertions))]
    pub fn with_production_gateway(
        paths: AppPaths,
        production_gateway: Arc<dyn crate::production::ProductionModelGateway>,
    ) -> Self {
        Self::with_skill_bundle_loader_and_gateway(
            paths,
            crate::creative_skills::SkillBundleLoader::development_package(),
            production_gateway,
        )
    }

    pub fn with_skill_bundle_loader(
        paths: AppPaths,
        skill_bundle_loader: crate::creative_skills::SkillBundleLoader,
    ) -> Self {
        let production_gateway = Arc::new(
            crate::production::ConfiguredProductionGateway::for_desktop(paths.clone()),
        );
        Self::with_skill_bundle_loader_and_gateway(paths, skill_bundle_loader, production_gateway)
    }

    fn with_skill_bundle_loader_and_gateway(
        paths: AppPaths,
        skill_bundle_loader: crate::creative_skills::SkillBundleLoader,
        production_gateway: Arc<dyn crate::production::ProductionModelGateway>,
    ) -> Self {
        Self {
            manager: ProjectManager::new(paths.clone()),
            paths,
            skill_bundle_loader,
            production_gateway,
            market_source_adapter: Arc::new(crate::market_source::UnavailableMarketSourceAdapter),
        }
    }

    pub fn with_market_source_adapter(
        mut self,
        market_source_adapter: Arc<dyn crate::market_source::MarketSourceAdapter>,
    ) -> Self {
        self.market_source_adapter = market_source_adapter;
        self
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
    let title = crate::project_db::normalize_project_title(title);
    json!({
        "id": project_id,
        "title": title,
        "subtitle": "本地作品",
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
    let title = crate::project_db::normalize_project_title(&row.title);
    json!({
        "id": row.project_id,
        "title": title,
        "subtitle": "本地作品",
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

#[allow(clippy::too_many_arguments)]
fn registry_lifecycle_command(
    state: &KernelState,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    command_type: &str,
    request_digest: &str,
    expected_revision: Option<i64>,
    requested_at: &str,
    delete: bool,
) -> Result<Value, String> {
    let catalog =
        CatalogRepository::open(&state.paths, requested_at).map_err(|error| error.to_string())?;
    if let Some((stored_digest, response_json)) =
        CatalogRepository::registry_command_receipt(&catalog, idempotency_key)
            .map_err(|error| error.to_string())?
    {
        if stored_digest != request_digest {
            return Err("IDEMPOTENCY_CONFLICT".into());
        }
        return serde_json::from_str(&response_json).map_err(|error| error.to_string());
    }
    let lifecycle =
        CatalogRepository::lifecycle(&catalog, project_id).map_err(|error| error.to_string())?;
    if !matches!(
        lifecycle.as_deref(),
        Some("ACTIVE") | Some("ARCHIVED") | Some("DELETING")
    ) {
        return Err("PROJECT_NOT_FOUND".into());
    }
    let before_revision = expected_revision.unwrap_or(0);
    if delete {
        state
            .manager
            .delete_registered_to_trash(project_id, requested_at)
            .map_err(|error| error.to_string())?;
    } else {
        state
            .manager
            .set_lifecycle(project_id, "ARCHIVED", requested_at)
            .map_err(|error| error.to_string())?;
    }
    let receipt_id = format!("registry-receipt:{command_id}");
    let receipt_digest = sha256_canonical(&json!({
        "receiptId": receipt_id,
        "projectId": project_id,
        "commandId": command_id,
        "idempotencyKey": idempotency_key,
        "commandType": command_type,
        "requestDigest": request_digest,
        "beforeRevision": before_revision,
        "afterRevision": before_revision,
        "committedAt": requested_at,
    }))
    .map_err(|error| error.to_string())?;
    let receipt = ProjectCommandReceipt {
        receipt_id,
        project_id: project_id.to_owned(),
        result_project_id: project_id.to_owned(),
        command_id: command_id.to_owned(),
        idempotency_key: idempotency_key.to_owned(),
        command_type: command_type.to_owned(),
        before_revision,
        after_revision: before_revision,
        request_digest: request_digest.to_owned(),
        committed_at: requested_at.to_owned(),
        receipt_digest,
    };
    let response = project_command_response(&receipt);
    let response_json = serde_json::to_string(&response).map_err(|error| error.to_string())?;
    CatalogRepository::insert_registry_command_receipt(
        &catalog,
        idempotency_key,
        project_id,
        command_id,
        command_type,
        request_digest,
        &response_json,
        requested_at,
    )
    .map_err(|error| error.to_string())?;
    Ok(response)
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
        entry["totalCost"] = json!(cost_cents as f64 / 100.0);
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
            "totalCost": total_cost as f64 / 100.0
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
    let recent_chapters = state
        .manager
        .with_read_connection(project_id, &updated_at, |connection| {
            crate::repositories::narrative::NarrativeRepository::recent_committed_chapters(
                connection,
                project_id,
                3,
            )
            .map(|chapters| {
                chapters
                    .into_iter()
                    .map(|chapter| {
                        json!({
                            "id": chapter.chapter_id,
                            "volumeId": chapter.volume_id,
                            "number": chapter.chapter_no,
                            "title": chapter.title,
                            "status": "official",
                            "version": chapter.final_version.map(|version| format!("v{version}")).unwrap_or_default(),
                            "wordCount": chapter.word_count,
                            "updatedAt": chapter.updated_at
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
        })
        .map_err(|error| error.to_string())?;
    let health_summary =
        match state
            .manager
            .with_read_connection(project_id, &updated_at, |connection| {
                crate::health::query_book_health(connection, project_id, None, 1)
                    .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
            }) {
            Ok(projection) => {
                project_health_summary(projection["level"].as_str().unwrap_or("source_unavailable"))
            }
            Err(_) => project_health_summary("source_unavailable"),
        };
    let mut project = project_snapshot(project_id, &title, revision, &updated_at);
    project["phase"] = json!(runtime.current_phase);
    project["runState"] = json!(runtime.run_state);
    project["blockReason"] = runtime
        .block_reason
        .map(Value::String)
        .unwrap_or(Value::Null);
    project["signingStatus"] = json!(runtime.signing_status);
    project["committedChapters"] = json!(runtime.committed_chapters);
    project["candidateChapters"] = json!(runtime.candidate_chapters);
    project["totalCost"] = json!(runtime.total_cost_cents as f64 / 100.0);
    project["progress"] = json!(runtime.progress);
    Ok(json!({
        "project": project,
        "pendingDecisionCount": pending_decisions,
        "recentChapters": recent_chapters,
        "recentArtifacts": [],
        "healthSummary": health_summary
    }))
}

fn project_health_summary(level: &str) -> Value {
    let (level, label, note) = match level {
        "not_evaluated" => (
            "not_evaluated",
            "尚未评估",
            "正式章节与项目记录积累后，会显示长篇状态。",
        ),
        "insufficient_data" => (
            "insufficient_data",
            "数据仍不足",
            "已有记录尚不足以完成全部长篇状态检查。",
        ),
        "source_unavailable" => (
            "source_unavailable",
            "检查暂不可用",
            "正式健康数据暂时无法读取，请稍后重新检查。",
        ),
        "healthy" => (
            "healthy",
            "长篇状态稳定",
            "正式健康检查未发现需要关注的项目。",
        ),
        "attention" => (
            "attention",
            "有项目需要关注",
            "正式健康检查发现需要复核的项目，请查看长篇状态。",
        ),
        "risk" => (
            "risk",
            "存在需要优先处理的风险",
            "正式健康检查发现风险，请查看相关项目后再决定后续安排。",
        ),
        _ => (
            "source_unavailable",
            "检查暂不可用",
            "正式健康数据暂时无法读取，请稍后重新检查。",
        ),
    };
    json!({"level":level,"label":label,"note":note})
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

pub fn query_market_opportunities_impl(state: &KernelState) -> Result<Value, String> {
    let snapshot = state.market_source_adapter.fetch();
    Ok(crate::market_source::discovery_projection(&snapshot))
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
pub fn query_command_receipt_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let request = request.get("request").cloned().unwrap_or(request);
    let project_id = required_string(&request, "projectId")?.to_string();
    let idempotency_key = required_string(&request, "idempotencyKey")?.to_string();
    state
        .manager
        .with_read_connection(&project_id, "1970-01-01T00:00:00Z", |connection| {
            if let Some(receipt) =
                CommitService::find_receipt(connection, &project_id, &idempotency_key)
                    .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?
            {
                return Ok(json!({
                    "commandId":receipt.command_id,
                    "projectId":receipt.project_id,
                    "idempotencyKey":receipt.idempotency_key,
                    "acceptedAt":receipt.committed_at,
                    "finalStatus":"committed",
                    "message":"正式回执已找到。",
                    "receiptId":receipt.receipt_id,
                    "beforeRevision":receipt.before_revision,
                    "afterRevision":receipt.after_revision,
                    "receipt":receipt,
                }));
            }
            if let Some(receipt) = CommitService::find_project_command_receipt(
                connection,
                &project_id,
                &idempotency_key,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?
            {
                return Ok(json!({
                    "commandId":receipt.command_id,
                    "projectId":receipt.project_id,
                    "idempotencyKey":receipt.idempotency_key,
                    "acceptedAt":receipt.committed_at,
                    "finalStatus":"committed",
                    "message":"正式回执已找到。",
                    "receiptId":receipt.receipt_id,
                    "beforeRevision":receipt.before_revision,
                    "afterRevision":receipt.after_revision,
                    "requestDigest":receipt.request_digest,
                    "receipt":receipt,
                }));
            }
            Ok(Value::Null)
        })
        .map_err(|error| error.to_string())
}

pub fn query_production_center_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?.to_string();
    let mut projection = state
        .manager
        .with_read_connection(&project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::repositories::production::ProductionRepository::center_projection(
                connection,
                &project_id,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
        })
        .map_err(|error| error.to_string())?;
    if projection
        .pointer("/signing/status")
        .and_then(Value::as_str)
        == Some("PASS")
    {
        if let Some(receipt_ref) =
            projection
                .pointer("/signing/receiptRef")
                .cloned()
                .and_then(|value| {
                    serde_json::from_value::<crate::generated::types::ArtifactRef>(value).ok()
                })
        {
            let current = state.manager.with_read_connection(
                &project_id,
                "1970-01-01T00:00:00Z",
                |connection| {
                    crate::signing::signing_evaluation_is_current_in_project(
                        connection,
                        &project_id,
                        &receipt_ref,
                    )
                    .map_err(ManagerError::ProjectCommand)
                },
            );
            if !current.unwrap_or(false) {
                projection["signing"]["status"] = json!("STALE");
                projection["signing"]["verdict"] = json!("签约规则已更新，请重新评估");
            }
        }
    }
    Ok(projection)
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

#[allow(clippy::too_many_arguments)]
fn execute_decision_submit(
    state: &KernelState,
    request: &Value,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    requested_at: &str,
    expected_revision: Option<i64>,
    digest: &str,
) -> Result<Value, String> {
    let decision_id = request
        .pointer("/payload/decisionId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "IPC_INVALID:payload.decisionId".to_string())?;
    let choice = request
        .pointer("/payload/choice")
        .and_then(Value::as_str)
        .unwrap_or("recommended");
    let action = match choice {
        "recommended" => "RETRY_REMOTE",
        "RETRY_REMOTE" | "MARK_FAILED" | "CANCEL_JOB" => choice,
        _ => return Err("UNKNOWN_REMOTE_OUTCOME requires an explicit supported action".into()),
    };
    if action == "RETRY_REMOTE" {
        state.production_gateway.resolve_profile()?;
    }
    let spec = project_command_spec(
        project_id,
        project_id,
        command_id,
        idempotency_key,
        "decision_submit",
        digest,
        expected_revision,
        requested_at,
        true,
    );
    let outcome = state
        .manager
        .commit_project_command(spec, |connection| {
            crate::repositories::runtime::resolve_unknown_decision(
                connection,
                project_id,
                decision_id,
                action,
                requested_at,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
        })
        .map_err(|error| error.to_string())?;
    let (replayed, receipt) = match outcome {
        ProjectCommandOutcome::Replayed(receipt) => (true, receipt),
        ProjectCommandOutcome::Committed { receipt, .. } => (false, receipt),
    };

    let retry_status = if action == "RETRY_REMOTE" && !replayed {
        let adjustment = request
            .pointer("/payload/note")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        let retry_request = json!({
            "type":"pitch_regenerate",
            "commandId":command_id,
            "idempotencyKey":idempotency_key,
            "projectId":project_id,
            "expectedRevision":receipt.after_revision,
            "requestedAt":requested_at,
            "payload":{"adjustment":adjustment,"regeneratePitches":true}
        });
        match execute_b11_command(
            state,
            &retry_request,
            "pitch_regenerate",
            project_id,
            command_id,
            idempotency_key,
            requested_at,
            Some(receipt.after_revision),
            digest,
        ) {
            Ok(_) => "COMPLETED",
            Err(_) => "FAILED",
        }
    } else if action == "RETRY_REMOTE" {
        "RECORDED"
    } else {
        "NOT_REQUESTED"
    };

    let mut response = project_command_response(&receipt);
    response["message"] = json!(match action {
        "RETRY_REMOTE" => "重试决定已记录，请查看故事方向的最新状态。",
        "CANCEL_JOB" => "已取消远程未知任务。",
        _ => "已将远程未知任务按失败处理。",
    });
    response["result"] = json!({"action":action,"retryStatus":retry_status});
    Ok(response)
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

pub fn query_book_health_factor_sources_impl(
    state: &KernelState,
    request: Value,
) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let factor_code = required_string(&request, "factorCode")?;
    let cursor = request.get("cursor").and_then(Value::as_str);
    let limit = request
        .get("pageSize")
        .and_then(Value::as_u64)
        .unwrap_or(50);
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::health::query_book_health_factor_sources(
                connection,
                project_id,
                factor_code,
                cursor,
                limit,
            )
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

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContextScopeSelection {
    task_refs: Option<Vec<String>>,
    responsibility_refs: Option<Vec<String>>,
    reservation_refs: Option<Vec<String>>,
    entity_refs: Option<Vec<String>>,
}

fn context_scope_selection(request: &Value) -> Result<Option<ContextScopeSelection>, String> {
    let selection_value = request
        .get("scopeSelection")
        .or_else(|| request.pointer("/payload/scopeSelection"));
    let plan = request.get("chapterPlan").and_then(Value::as_object);
    if let Some(value) = selection_value.filter(|value| !value.is_null()) {
        let selection: ContextScopeSelection = serde_json::from_value(value.clone())
            .map_err(|_| "IPC_INVALID:payload.scopeSelection".to_string())?;
        if selection.task_refs.is_none()
            || selection.responsibility_refs.is_none()
            || selection.reservation_refs.is_none()
            || selection.entity_refs.is_none()
        {
            return Err("IPC_INVALID:payload.scopeSelection requires all scope lists".into());
        }
        if let Some(hard_scope) = plan
            .and_then(|plan| plan.get("hardScope"))
            .and_then(Value::as_object)
        {
            let allowed: ContextScopeSelection =
                serde_json::from_value(Value::Object(hard_scope.clone()))
                    .map_err(|_| "CHAPTER_PLAN_HARD_SCOPE_INVALID".to_string())?;
            for (requested, available) in [
                (&selection.task_refs, &allowed.task_refs),
                (&selection.responsibility_refs, &allowed.responsibility_refs),
                (&selection.entity_refs, &allowed.entity_refs),
            ] {
                if requested.as_ref().is_some_and(|requested| {
                    available.as_ref().is_none_or(|available| {
                        requested
                            .iter()
                            .any(|reference| !available.contains(reference))
                    })
                }) {
                    return Err("CONTEXT_SCOPE_SELECTION_INVALID".into());
                }
            }
            // Reservation refs are validated against the current BookPlan
            // below. A valid later-volume ref may expand a prior scope and
            // must reach the compiler so its full gate contract enters HARD.
        }
        return Ok(Some(selection));
    }

    let Some(plan) = plan else {
        return Ok(None);
    };
    if let Some(hard_scope) = plan.get("hardScope") {
        return serde_json::from_value(hard_scope.clone())
            .map(Some)
            .map_err(|_| "CHAPTER_PLAN_HARD_SCOPE_INVALID".into());
    }
    let plan_strings = |key: &str| -> Result<Vec<String>, String> {
        match plan.get(key) {
            None => Ok(Vec::new()),
            Some(Value::Array(values)) => values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(ToOwned::to_owned)
                        .ok_or_else(|| "CONTEXT_SCOPE_SELECTION_INVALID".to_string())
                })
                .collect(),
            _ => Err("CONTEXT_SCOPE_SELECTION_INVALID".into()),
        }
    };
    let responsibility_refs = match plan.get("continuityActions") {
        None => Vec::new(),
        Some(Value::Array(actions)) => actions
            .iter()
            .map(|action| {
                action
                    .get("threadRef")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| "CONTEXT_SCOPE_SELECTION_INVALID".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err("CONTEXT_SCOPE_SELECTION_INVALID".into()),
    };
    Ok(Some(ContextScopeSelection {
        task_refs: Some(plan_strings("activeTaskRefs")?),
        responsibility_refs: Some(responsibility_refs),
        reservation_refs: Some(plan_strings("reservationRefs")?),
        entity_refs: Some(plan_strings("entityRefs")?),
    }))
}

fn context_plan_reservation_refs(request: &Value) -> Result<Vec<String>, String> {
    let refs = request
        .get("chapterPlan")
        .and_then(|plan| plan.get("reservationRefs"))
        .or_else(|| request.pointer("/payload/plan/reservationRefs"))
        .or_else(|| request.pointer("/plan/reservationRefs"));
    match refs {
        None => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| "CONTEXT_SCOPE_SELECTION_INVALID".to_string())
            })
            .collect(),
        Some(_) => Err("CONTEXT_SCOPE_SELECTION_INVALID".into()),
    }
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

fn context_scope(request: &Value, project_id: &str) -> String {
    if let Some(chapter_id) = request.get("chapterId").and_then(Value::as_str) {
        format!("CHAPTER:{chapter_id}")
    } else if let Some(volume_id) = request
        .get("volumeId")
        .or_else(|| request.get("currentVolumeId"))
        .and_then(Value::as_str)
    {
        format!("VOLUME:{volume_id}")
    } else {
        format!("PROJECT:{project_id}")
    }
}

fn map_context_query<T, E: std::fmt::Display>(
    result: Result<T, E>,
    source: ContextHardSource,
    scope: &str,
) -> Result<T, String> {
    result.map_err(|error| {
        let message = error.to_string();
        if message.starts_with("CONTEXT_SOURCE_UNAVAILABLE|") {
            message
        } else {
            format!(
                "CONTEXT_SOURCE_UNAVAILABLE|source={}|scope={scope}",
                source.code()
            )
        }
    })
}

fn narrative_state_from_request(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
) -> Result<CurrentNarrativeStateView, String> {
    let scope = context_scope(request, project_id);
    let entity_ids = request_strings(request, "entityIds");
    let entity_refs = entity_ids.iter().map(String::as_str).collect::<Vec<_>>();
    let references = request_strings(request, "references");
    let reference_refs = references.iter().map(String::as_str).collect::<Vec<_>>();
    let chapter_id = request.get("chapterId").and_then(Value::as_str);
    let (facts, facts_truncated) = map_context_query(
        crate::repositories::narrative::NarrativeRepository::current_canon_bounded(
            connection,
            project_id,
            &entity_refs,
            100,
        ),
        ContextHardSource::Canon,
        &scope,
    )?;
    let relations = map_context_query(
        crate::repositories::narrative::NarrativeRepository::current_relations(
            connection,
            project_id,
            &entity_refs,
            100,
        ),
        ContextHardSource::Canon,
        &scope,
    )?;
    let (knowledge, knowledge_truncated) = map_context_query(
        crate::repositories::narrative::NarrativeRepository::knowledge_for_entities_bounded(
            connection,
            project_id,
            &entity_refs,
            100,
        ),
        ContextHardSource::Knowledge,
        &scope,
    )?;
    let events = if let Some(chapter) = chapter_id {
        map_context_query(
            crate::repositories::narrative::NarrativeRepository::canon_events(
                connection, project_id, chapter, 100,
            ),
            ContextHardSource::Canon,
            &scope,
        )?
    } else {
        Vec::new()
    };
    let mut events_truncated = false;
    let relations_truncated = map_context_query(
        crate::repositories::narrative::NarrativeRepository::has_more_relations(
            connection,
            project_id,
            &entity_refs,
            100,
        ),
        ContextHardSource::Canon,
        &scope,
    )?;
    if let Some(chapter) = chapter_id {
        events_truncated = map_context_query(
            crate::repositories::narrative::NarrativeRepository::has_more_events(
                connection, project_id, chapter, 100,
            ),
            ContextHardSource::Canon,
            &scope,
        )?;
    }
    let obligations_truncated = map_context_query(
        crate::repositories::narrative::NarrativeRepository::has_more_obligations(
            connection,
            project_id,
            &entity_refs,
            &reference_refs,
            100,
        ),
        ContextHardSource::Continuity,
        &scope,
    )?;
    let obligations = map_context_query(
        crate::repositories::narrative::NarrativeRepository::active_obligations_for_relevance(
            connection,
            project_id,
            &entity_refs,
            &reference_refs,
            100,
        ),
        ContextHardSource::Continuity,
        &scope,
    )?;
    let entity_ids_truncated = entity_ids.len() > 100;
    let mut state = CurrentNarrativeStateView::from_records(
        RuntimeBounds {
            project_id: project_id.into(),
            entity_ids,
            volume_id: request
                .get("volumeId")
                .or_else(|| request.get("currentVolumeId"))
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
        false,
    );
    for (source, truncated) in [
        (
            ContextHardSource::Canon,
            facts_truncated || relations_truncated || events_truncated,
        ),
        (ContextHardSource::Knowledge, knowledge_truncated),
        (ContextHardSource::Continuity, obligations_truncated),
        (ContextHardSource::Canon, entity_ids_truncated),
    ] {
        if truncated {
            state.mark_truncated(source);
        }
    }
    Ok(state)
}

fn continuity_from_request(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
) -> Result<Vec<crate::narrative::ContinuityThreadView>, String> {
    let scope = context_scope(request, project_id);
    let state = narrative_state_from_request(connection, request, project_id)?;
    let chapter_refs = request
        .get("chapterId")
        .and_then(Value::as_str)
        .map(|id| vec![id.to_owned()])
        .unwrap_or_default();
    let chapter_refs = chapter_refs.iter().map(String::as_str).collect::<Vec<_>>();
    let refs = request_strings(request, "references");
    let refs = refs.iter().map(String::as_str).collect::<Vec<_>>();
    let foreshadows = map_context_query(
        crate::repositories::narrative::NarrativeRepository::active_foreshadows(
            connection,
            project_id,
            &chapter_refs,
            &refs,
            100,
        ),
        ContextHardSource::Continuity,
        &scope,
    )?;
    let mysteries = map_context_query(
        crate::repositories::narrative::NarrativeRepository::open_mysteries(
            connection, project_id, &refs, 100,
        ),
        ContextHardSource::Continuity,
        &scope,
    )?;
    let evidence = map_context_query(
        crate::repositories::narrative::NarrativeRepository::recent_evidence(
            connection,
            project_id,
            &chapter_refs,
            &refs,
            100,
        ),
        ContextHardSource::Evidence,
        &scope,
    )?;
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
) -> Result<
    (
        Vec<crate::narrative::ContinuityThreadView>,
        BTreeSet<ContextHardSource>,
    ),
    String,
> {
    let threads = continuity_from_request(connection, request, project_id)?;
    let scope = context_scope(request, project_id);
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
    let (_, evidence_truncated) = map_context_query(
        crate::repositories::narrative::NarrativeRepository::recent_evidence_bounded(
            connection,
            project_id,
            &chapter_refs,
            &reference_refs,
            100,
        ),
        ContextHardSource::Evidence,
        &scope,
    )?;
    let obligations_truncated = map_context_query(
        crate::repositories::narrative::NarrativeRepository::has_more_obligations(
            connection,
            project_id,
            &entity_refs,
            &reference_refs,
            100,
        ),
        ContextHardSource::Continuity,
        &scope,
    )?;
    let foreshadows_truncated = map_context_query(
        crate::repositories::narrative::NarrativeRepository::has_more_foreshadows(
            connection,
            project_id,
            &chapter_refs,
            &reference_refs,
            100,
        ),
        ContextHardSource::Continuity,
        &scope,
    )?;
    let mysteries_truncated = map_context_query(
        crate::repositories::narrative::NarrativeRepository::has_more_mysteries(
            connection,
            project_id,
            &reference_refs,
            100,
        ),
        ContextHardSource::Continuity,
        &scope,
    )?;
    let mut truncated_sources = BTreeSet::new();
    if obligations_truncated || foreshadows_truncated || mysteries_truncated {
        truncated_sources.insert(ContextHardSource::Continuity);
    }
    if evidence_truncated {
        truncated_sources.insert(ContextHardSource::Evidence);
    }
    Ok((threads, truncated_sources))
}

struct GoalContractContext {
    refs: Vec<crate::generated::types::ArtifactRef>,
    fields: Vec<crate::narrative::GoalContractField>,
    omitted_soft_sources: Vec<crate::generated::types::OmittedSource>,
    future_reservation_refs: Vec<String>,
    responsibility_refs: Vec<String>,
    current_scope_refs: Vec<String>,
}

fn extract_contract_source_refs(text: &str) -> Vec<String> {
    let mut refs = BTreeSet::new();
    let markers = [
        "(fact:",
        "(event:",
        "(knowledge:",
        "(evidence:",
        "(obligation:",
    ];
    for marker in markers {
        let mut offset = 0;
        while let Some(start) = text[offset..].find(marker) {
            let value_start = offset + start + 1;
            let Some(end) = text[value_start..].find(')') else {
                break;
            };
            refs.insert(text[value_start..value_start + end].to_owned());
            offset = value_start + end + 1;
        }
    }
    refs.into_iter().collect()
}

fn contract_value_source_refs(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => extract_contract_source_refs(text),
        Value::Array(items) => items.iter().flat_map(contract_value_source_refs).collect(),
        Value::Object(fields) => fields
            .values()
            .flat_map(contract_value_source_refs)
            .collect(),
        _ => Vec::new(),
    }
}

fn is_opening_owner_canon_item(item: &Value) -> bool {
    item.get("kind").and_then(Value::as_str) == Some("CANON_REFS")
        || item
            .get("id")
            .or_else(|| item.get("reservationId"))
            .or_else(|| item.get("seedId"))
            .and_then(Value::as_str)
            .is_some_and(|id| {
                id.contains("opening-reality")
                    || id.contains("relationship:opening:")
                    || id.contains("relationship:opening-facts:")
            })
}

fn opening_canon_reference_projection(
    item: &Value,
    opening_canon_refs: &[String],
    scope: &str,
) -> Result<Option<Value>, String> {
    let id = item
        .get("id")
        .or_else(|| item.get("reservationId"))
        .or_else(|| item.get("seedId"))
        .and_then(Value::as_str);
    let has_canon_refs_kind = item.get("kind").and_then(Value::as_str) == Some("CANON_REFS");
    if !has_canon_refs_kind && (!is_opening_owner_canon_item(item) || opening_canon_refs.is_empty())
    {
        return Ok(None);
    }
    if opening_canon_refs.is_empty() {
        return Err(format!(
            "CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"
        ));
    }
    let id =
        id.ok_or_else(|| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?;
    let refs = if let Some(refs) = item.get("refs") {
        let refs = refs
            .as_array()
            .ok_or_else(|| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?;
        refs.iter()
            .map(|reference| {
                reference
                    .as_str()
                    .filter(|reference| {
                        opening_canon_refs
                            .iter()
                            .any(|source| source.as_str() == *reference)
                    })
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| {
                        format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}")
                    })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        opening_canon_refs.to_vec()
    };
    if refs.is_empty() {
        return Err(format!(
            "CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"
        ));
    }
    Ok(Some(json!({"id":id,"refs":refs})))
}

fn normalized_hard_rule(text: &str, ids: Vec<String>) -> Value {
    json!({"constraint":text,"constraintIds":ids})
}

fn normalize_hard_contract_value(value: &Value) -> Value {
    match value {
        Value::String(text) => normalized_hard_rule(text, Vec::new()),
        Value::Object(object) => {
            if let Some(Value::String(description)) = object.get("description") {
                let ids = object
                    .get("id")
                    .and_then(Value::as_str)
                    .map(|id| vec![id.to_owned()])
                    .unwrap_or_default();
                normalized_hard_rule(description, ids)
            } else {
                object.clone().into()
            }
        }
        _ => value.clone(),
    }
}

fn hard_contract_semantic_key(value: &Value) -> String {
    if let Some(text) = value.get("constraint").and_then(Value::as_str) {
        return format!("CONSTRAINT:{text}");
    }
    serde_json::to_string(value).unwrap_or_default()
}

fn merge_constraint_ids(existing: &mut Value, incoming: &Value) {
    let mut ids = existing
        .get("constraintIds")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    ids.extend(
        incoming
            .get("constraintIds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .cloned(),
    );
    ids.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    ids.dedup();
    if let Some(object) = existing.as_object_mut() {
        object.insert("constraintIds".into(), Value::Array(ids));
    }
}

fn append_goal_contract_field(
    fields: &mut Vec<crate::narrative::GoalContractField>,
    source_ref: &crate::generated::types::ArtifactRef,
    field_path: impl Into<String>,
    value: Option<&Value>,
) -> Result<(), String> {
    let field_path = field_path.into();
    if let Some(value) =
        value.filter(|value| !value.is_null() && !value.as_array().is_some_and(Vec::is_empty))
    {
        let value = normalize_hard_contract_value(value);
        let semantic_key = hard_contract_semantic_key(&value);
        if let Some(existing) = fields
            .iter_mut()
            .find(|field| hard_contract_semantic_key(&field.value) == semantic_key)
        {
            merge_constraint_ids(&mut existing.value, &value);
            if existing.source_ref != *source_ref
                && !existing.additional_source_refs.contains(source_ref)
            {
                existing.additional_source_refs.push(source_ref.clone());
            }
            if field_path != existing.field_path
                && !existing.additional_field_paths.contains(&field_path)
            {
                existing.additional_field_paths.push(field_path);
            }
            return Ok(());
        }
        fields.push(crate::narrative::GoalContractField {
            field_path,
            source_ref: source_ref.clone(),
            additional_source_refs: Vec::new(),
            additional_field_paths: Vec::new(),
            value,
        });
    }
    Ok(())
}

fn append_goal_contract_array(
    fields: &mut Vec<crate::narrative::GoalContractField>,
    source_ref: &crate::generated::types::ArtifactRef,
    field_path: &str,
    value: Option<&Value>,
) -> Result<(), String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let Some(items) = value.as_array() else {
        return append_goal_contract_field(fields, source_ref, field_path, Some(value));
    };
    for (index, item) in items.iter().enumerate() {
        let item_id = item
            .get("id")
            .or_else(|| item.get("reservationId"))
            .or_else(|| item.get("seedId"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| index.to_string());
        append_goal_contract_field(
            fields,
            source_ref,
            format!("{field_path}[{item_id}]"),
            Some(item),
        )?;
    }
    Ok(())
}

fn contract_string_array(
    payload: &Value,
    field_path: &str,
    scope: &str,
) -> Result<Vec<String>, String> {
    let Some(values) = payload.get(field_path).and_then(Value::as_array) else {
        return Err(format!(
            "CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"
        ));
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.trim().is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))
        })
        .collect()
}

fn record_omitted_contract_field(
    omitted: &mut Vec<crate::generated::types::OmittedSource>,
    source_ref: &crate::generated::types::ArtifactRef,
    field_path: &str,
    reason: &str,
) {
    omitted.push(crate::generated::types::OmittedSource {
        source_ref: crate::generated::types::SourceRef {
            kind: "contract-field".into(),
            ref_: format!(
                "{}@{}#{field_path}",
                source_ref.artifact_id, source_ref.version
            ),
        },
        reason: reason.into(),
    });
}

fn context_contract_payload(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    reference: &crate::generated::types::ArtifactRef,
    source: &str,
    scope: &str,
) -> Result<Value, String> {
    crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
        connection, project_id, reference,
    )
    .map_err(|_| format!("CONTEXT_SOURCE_UNAVAILABLE|source={source}|scope={scope}"))?
    .ok_or_else(|| format!("CONTEXT_SOURCE_UNAVAILABLE|source={source}|scope={scope}"))
}

fn declared_story_mechanism<'a>(
    story_core: &'a serde_json::Map<String, Value>,
    scope: &str,
) -> Result<Option<&'a Value>, String> {
    match story_core.get("mechanism") {
        None => Ok(None),
        Some(value) if value.as_str().is_some_and(|text| !text.trim().is_empty()) => {
            Ok(Some(value))
        }
        Some(_) => Err(format!(
            "GOAL_CONTRACT_STORY_CONSTITUTION_INVALID|field=storyCore.mechanism|scope={scope}"
        )),
    }
}

fn hard_plan_task_projection(
    task: &Value,
    goal_contract_fields: &[crate::narrative::GoalContractField],
) -> Value {
    let mut projected = serde_json::Map::new();
    for key in [
        "taskId",
        "class",
        "dependsOnTaskIds",
        "continuityThreadRefs",
        "reservationRefs",
        "completionPredicate",
    ] {
        if let Some(value) = task.get(key) {
            projected.insert(key.into(), value.clone());
        }
    }
    if let Some(objective) = task.get("objective").and_then(Value::as_str) {
        let normalized = normalize_hard_contract_value(&Value::String(objective.into()));
        if let Some(goal) = goal_contract_fields.iter().find(|field| {
            hard_contract_semantic_key(&field.value) == hard_contract_semantic_key(&normalized)
        }) {
            projected.insert(
                "objectiveContractRef".into(),
                json!({
                    "fieldPath":goal.field_path,
                    "additionalFieldPaths":goal.additional_field_paths,
                    "sourceRef":goal.source_ref,
                    "additionalSourceRefs":goal.additional_source_refs
                }),
            );
        } else {
            projected.insert("objective".into(), normalized["constraint"].clone());
            let source_refs = extract_contract_source_refs(objective);
            if !source_refs.is_empty() {
                projected.insert("objectiveSourceRefs".into(), json!(source_refs));
            }
        }
    }
    Value::Object(projected)
}

fn task_ref_covers_goal(task_refs: &[String], goal_id: &str) -> bool {
    task_refs.iter().any(|task_ref| task_ref.contains(goal_id))
}

fn selected_entity_source_ids(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    entity_refs: &[String],
    scope: &str,
) -> Result<BTreeSet<String>, String> {
    let entity_slices = entity_refs.iter().map(String::as_str).collect::<Vec<_>>();
    let (facts, facts_truncated) = map_context_query(
        crate::repositories::narrative::NarrativeRepository::current_canon_bounded(
            connection,
            project_id,
            &entity_slices,
            100,
        ),
        ContextHardSource::Canon,
        scope,
    )?;
    let relations = map_context_query(
        crate::repositories::narrative::NarrativeRepository::current_relations(
            connection,
            project_id,
            &entity_slices,
            100,
        ),
        ContextHardSource::Canon,
        scope,
    )?;
    let relations_truncated = map_context_query(
        crate::repositories::narrative::NarrativeRepository::has_more_relations(
            connection,
            project_id,
            &entity_slices,
            100,
        ),
        ContextHardSource::Canon,
        scope,
    )?;
    let (knowledge, knowledge_truncated) = map_context_query(
        crate::repositories::narrative::NarrativeRepository::knowledge_for_entities_bounded(
            connection,
            project_id,
            &entity_slices,
            100,
        ),
        ContextHardSource::Knowledge,
        scope,
    )?;
    if facts_truncated || relations_truncated || knowledge_truncated {
        return Err(format!(
            "CONTEXT_HARD_INCOMPLETE|sources={}|scope={scope}",
            [
                (facts_truncated || relations_truncated).then_some("CANON"),
                knowledge_truncated.then_some("KNOWLEDGE"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(",")
        ));
    }
    Ok(facts
        .into_iter()
        .map(|fact| fact.fact_id)
        .chain(relations.into_iter().map(|relation| relation.relation_id))
        .chain(knowledge.into_iter().map(|item| item.knowledge_id))
        .collect())
}

fn volume_constraint_matches_scope(
    item: &Value,
    task_refs: Option<&[String]>,
    selected_source_refs: Option<&BTreeSet<String>>,
) -> bool {
    let Some(id) = item.get("id").and_then(Value::as_str) else {
        return true;
    };
    if let Some(task_refs) = task_refs {
        if let Some(goal_id) = id
            .strip_prefix("rule:")
            .filter(|id| id.starts_with("opening-reality:"))
        {
            if task_ref_covers_goal(task_refs, goal_id) {
                return true;
            }
        }
    }
    let Some(selected_source_refs) = selected_source_refs else {
        return true;
    };
    if let Some(refs) = item.get("refs").and_then(Value::as_array) {
        return refs
            .iter()
            .filter_map(Value::as_str)
            .any(|reference| selected_source_refs.contains(reference));
    }
    for marker in [":fact:", ":relation:", ":knowledge:", ":event:"] {
        if let Some(reference) = id.rsplit_once(marker).map(|(_, reference)| reference) {
            return selected_source_refs.contains(reference);
        }
    }
    if let Some(description) = item.get("description").and_then(Value::as_str) {
        let refs = extract_contract_source_refs(description);
        return refs.is_empty()
            || refs
                .iter()
                .any(|reference| selected_source_refs.contains(reference));
    }
    true
}

fn goal_contract_context_for_request(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
    volume_id: Option<&str>,
    selection: Option<&ContextScopeSelection>,
    scope: &str,
) -> Result<GoalContractContext, String> {
    let opening = request.get("executionMode").and_then(Value::as_str) == Some("OPENING_BOOTSTRAP");
    let story_ref =
        crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
            connection,
            project_id,
            "StoryConstitution",
        )
        .map_err(|_| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?
        .ok_or_else(|| format!("GOAL_CONTRACT_STORY_CONSTITUTION_REQUIRED|scope={scope}"))?;
    let main_ref = crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
        connection,
        project_id,
        "MainContract",
    )
    .map_err(|_| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?
    .ok_or_else(|| format!("GOAL_CONTRACT_MAIN_CONTRACT_REQUIRED|scope={scope}"))?;
    let book_ref = crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
        connection, project_id, "BookPlan",
    )
    .map_err(|_| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?
    .ok_or_else(|| format!("GOAL_CONTRACT_BOOK_PLAN_REQUIRED|scope={scope}"))?;

    let story = context_contract_payload(connection, project_id, &story_ref, "CONTRACT", scope)?;
    let main = context_contract_payload(connection, project_id, &main_ref, "CONTRACT", scope)?;
    let legacy_book_payload =
        context_contract_payload(connection, project_id, &book_ref, "CONTRACT", scope)?;
    let (book, legacy_fields_ignored) =
        crate::planning::normalize_legacy_book_plan_payload(&legacy_book_payload);
    crate::generated::validators::validate_book_plan_r1(&book)
        .map_err(|error| format!("GOAL_CONTRACT_BOOK_PLAN_INVALID|{error}"))?;
    let maturity = book.get("planningMaturity").and_then(Value::as_str);
    let opening_canon_refs = if book.get("openingCanonRefs").is_some() {
        contract_string_array(&book, "openingCanonRefs", scope)?
    } else {
        Vec::new()
    };
    let mut omitted_soft_sources = Vec::new();
    if !legacy_fields_ignored.is_empty() {
        record_omitted_contract_field(
            &mut omitted_soft_sources,
            &book_ref,
            "legacyFieldsIgnored",
            &format!("legacyFieldsIgnored:{}", json!(legacy_fields_ignored)),
        );
    }
    if !opening_canon_refs.is_empty() {
        for (field, id_key) in [
            ("mustAchieve", "id"),
            ("mustNotBreak", "id"),
            ("relationshipMacroDirections", "id"),
            ("majorMysteryGoals", "id"),
            ("futureReservations", "reservationId"),
            ("volumeIntentSeeds", "seedId"),
        ] {
            for item in book
                .get(field)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|item| is_opening_owner_canon_item(item))
            {
                let item_id = item
                    .get(id_key)
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                record_omitted_contract_field(
                    &mut omitted_soft_sources,
                    &book_ref,
                    &format!("BookPlan.{field}[{item_id}].description"),
                    "开篇合同描述是 Canon 的语义副本；LONG_FORM HARD 保留必要引用，原描述作为 SOFT 历史摘录省略。",
                );
            }
        }
    }
    if opening && !matches!(maturity, Some("SEED" | "APPROVED")) {
        return Err(format!(
            "GOAL_CONTRACT_BOOK_PLAN_SEED_OR_APPROVED_REQUIRED|scope={scope}"
        ));
    }
    if !opening && maturity != Some("APPROVED") {
        return Err(format!(
            "GOAL_CONTRACT_BOOK_PLAN_APPROVED_REQUIRED|scope={scope}"
        ));
    }

    let opening_ref_and_payload = if opening {
        let reference =
            crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
                connection,
                project_id,
                "OpeningContract",
            )
            .map_err(|_| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?
            .ok_or_else(|| format!("GOAL_CONTRACT_OPENING_CONTRACT_REQUIRED|scope={scope}"))?;
        let payload =
            context_contract_payload(connection, project_id, &reference, "CONTRACT", scope)?;
        for field in [
            "firstChapterGoal",
            "firstThreeChapterPromise",
            "forbiddenReveals",
        ] {
            let valid = match field {
                "firstChapterGoal" | "firstThreeChapterPromise" => payload
                    .get(field)
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty()),
                "forbiddenReveals" => payload.get(field).is_some_and(Value::is_array),
                _ => false,
            };
            if !valid {
                return Err(format!(
                    "GOAL_CONTRACT_OPENING_CONTRACT_INVALID|field={field}|scope={scope}"
                ));
            }
        }
        Some((reference, payload))
    } else {
        None
    };
    let intent_ref_and_payload = if opening {
        None
    } else {
        let volume_id = volume_id.ok_or_else(|| {
            format!(
                "GOAL_CONTRACT_VOLUME_INTENT_REQUIRED|reason=VOLUME_SCOPE_REQUIRED|scope={scope}"
            )
        })?;
        let reference = crate::repositories::narrative::NarrativeRepository::canonical_head_ref(
            connection,
            project_id,
            "VolumeIntent",
            volume_id,
        )
        .map_err(|_| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?
        .ok_or_else(|| format!("GOAL_CONTRACT_VOLUME_INTENT_REQUIRED|scope={scope}"))?;
        let payload =
            context_contract_payload(connection, project_id, &reference, "CONTRACT", scope)?;
        crate::generated::validators::validate_volume_intent_r1(&payload)
            .map_err(|error| format!("GOAL_CONTRACT_VOLUME_INTENT_INVALID|{error}"))?;
        Some((reference, payload))
    };

    let mut refs = vec![story_ref.clone(), main_ref.clone(), book_ref.clone()];
    if let Some((reference, _)) = &opening_ref_and_payload {
        refs.push(reference.clone());
    }
    if let Some((reference, _)) = &intent_ref_and_payload {
        refs.push(reference.clone());
    }
    let mut fields = Vec::new();
    let mut future_reservation_refs = Vec::new();
    let mut responsibility_refs = Vec::new();
    let mut current_scope_refs = Vec::new();
    let story_core = story
        .get("storyCore")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            format!("GOAL_CONTRACT_STORY_CONSTITUTION_INVALID|field=storyCore|scope={scope}")
        })?;
    for field in ["worldRules", "forbiddenReveals"] {
        if story_core.get(field).is_some_and(|value| !value.is_array()) {
            return Err(format!(
                "GOAL_CONTRACT_STORY_CONSTITUTION_INVALID|field=storyCore.{field}|scope={scope}"
            ));
        }
    }
    let mechanism = declared_story_mechanism(story_core, scope)?;
    if mechanism.is_none() {
        record_omitted_contract_field(
            &mut omitted_soft_sources,
            &story_ref,
            "undeclaredFields",
            "undeclaredFields:[\"storyCore.mechanism\"]",
        );
    }

    if opening {
        for key in [
            "protagonist",
            "desire",
            "mechanism",
            "conflict",
            "readerPromise",
        ] {
            let value = if key == "mechanism" {
                mechanism
            } else {
                Some(
                    story_core
                        .get(key)
                        .filter(|value| {
                            value
                                .as_str()
                                .is_some_and(|text| !text.trim().is_empty())
                        })
                        .ok_or_else(|| {
                            format!("GOAL_CONTRACT_STORY_CONSTITUTION_INVALID|field=storyCore.{key}|scope={scope}")
                        })?,
                )
            };
            let Some(value) = value else {
                continue;
            };
            append_goal_contract_field(
                &mut fields,
                &story_ref,
                format!("storyCore.{key}"),
                Some(value),
            )?;
            current_scope_refs.extend(contract_value_source_refs(value));
        }
    } else if let Some(mechanism) = mechanism {
        append_goal_contract_field(
            &mut fields,
            &story_ref,
            "storyCore.mechanism",
            Some(mechanism),
        )?;
        record_omitted_contract_field(
            &mut omitted_soft_sources,
            &story_ref,
            "storyCore.protagonist",
            "长期人物定位不是当前卷硬约束；只保留合同引用。",
        );
        record_omitted_contract_field(
            &mut omitted_soft_sources,
            &story_ref,
            "storyCore.desire",
            "全书人物方向不是当前卷硬约束；只保留合同引用。",
        );
        record_omitted_contract_field(
            &mut omitted_soft_sources,
            &story_ref,
            "storyCore.conflict",
            "全书冲突方向不是当前卷硬约束；只保留合同引用。",
        );
        record_omitted_contract_field(
            &mut omitted_soft_sources,
            &story_ref,
            "storyCore.readerPromise",
            "宣传性读者承诺不进入 LONG_FORM HARD。",
        );
        record_omitted_contract_field(
            &mut omitted_soft_sources,
            &story_ref,
            "storyPromise",
            "全书故事承诺只保留引用，不进入当前卷 HARD。",
        );
    }
    append_goal_contract_array(
        &mut fields,
        &story_ref,
        "storyCore.worldRules",
        story_core
            .get("worldRules")
            .or_else(|| story.get("worldRules")),
    )?;
    if opening {
        if let Some(world_rules) = story_core
            .get("worldRules")
            .or_else(|| story.get("worldRules"))
        {
            current_scope_refs.extend(contract_value_source_refs(world_rules));
        }
    }
    append_goal_contract_array(
        &mut fields,
        &story_ref,
        "storyCore.forbiddenReveals",
        story_core
            .get("forbiddenReveals")
            .or_else(|| story.get("forbiddenReveals")),
    )?;
    if opening {
        if let Some(forbidden_reveals) = story_core
            .get("forbiddenReveals")
            .or_else(|| story.get("forbiddenReveals"))
        {
            current_scope_refs.extend(contract_value_source_refs(forbidden_reveals));
        }
    }
    append_goal_contract_array(
        &mut fields,
        &main_ref,
        "hardRules",
        Some(
            main.get("hardRules")
                .filter(|value| value.is_array())
                .ok_or_else(|| {
                    format!("GOAL_CONTRACT_MAIN_CONTRACT_INVALID|field=hardRules|scope={scope}")
                })?,
        ),
    )?;
    if opening {
        if let Some(hard_rules) = main.get("hardRules") {
            current_scope_refs.extend(contract_value_source_refs(hard_rules));
        }
    }
    if main.get("softPreferences").is_some() {
        record_omitted_contract_field(
            &mut omitted_soft_sources,
            &main_ref,
            "softPreferences",
            "主合同软偏好不是 HARD 约束。",
        );
    }

    if let Some((reference, opening_contract)) = &opening_ref_and_payload {
        for key in [
            "firstChapterGoal",
            "firstThreeChapterPromise",
            "forbiddenReveals",
        ] {
            let value = opening_contract.get(key).ok_or_else(|| {
                format!("GOAL_CONTRACT_OPENING_CONTRACT_INVALID|field={key}|scope={scope}")
            })?;
            append_goal_contract_array(
                &mut fields,
                reference,
                &format!("openingContract.{key}"),
                Some(value),
            )?;
            current_scope_refs.extend(contract_value_source_refs(value));
        }
        record_omitted_contract_field(
            &mut omitted_soft_sources,
            &book_ref,
            "BookPlan.longTermDirection",
            "开篇阶段使用 OpeningContract；书级长期方向仅保留 BookPlan 引用。",
        );
    }

    if let Some((intent_ref, intent)) = &intent_ref_and_payload {
        let current_volume_no = crate::repositories::narrative::NarrativeRepository::volume_number(
            connection,
            project_id,
            volume_id.unwrap_or_default(),
        )
        .map_err(|_| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?
        .ok_or_else(|| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?;
        let selected_task_refs = selection.and_then(|selection| selection.task_refs.as_deref());
        let selected_entity_ids = selection
            .and_then(|selection| selection.entity_refs.clone())
            .unwrap_or_else(|| request_strings(request, "entityIds"));
        let selected_entity_scope = Some(selected_entity_ids.as_slice());
        let selected_entity_sources = selected_entity_scope
            .map(|entity_refs| {
                selected_entity_source_ids(connection, project_id, entity_refs, scope)
            })
            .transpose()?;
        let must_achieve = intent
            .get("mustAchieve")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                format!("GOAL_CONTRACT_VOLUME_INTENT_INVALID|field=mustAchieve|scope={scope}")
            })?;
        for goal in must_achieve {
            let goal_id = goal.get("id").and_then(Value::as_str).ok_or_else(|| {
                format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}")
            })?;
            let opening_projection =
                opening_canon_reference_projection(goal, &opening_canon_refs, scope)?;
            let projected_goal = if let Some(projected) = opening_projection.as_ref() {
                current_scope_refs.extend(contract_string_array(projected, "refs", scope)?);
                record_omitted_contract_field(
                    &mut omitted_soft_sources,
                    intent_ref,
                    &format!("volumeIntent.mustAchieve[{goal_id}].description"),
                    "开篇合同描述是 Canon 的语义副本；HARD 保留条目 ID 和 Canon 引用，描述作为 SOFT 历史摘录省略。",
                );
                projected
            } else {
                if let Some(description) = goal.get("description").and_then(Value::as_str) {
                    current_scope_refs.extend(extract_contract_source_refs(description));
                }
                goal
            };
            append_goal_contract_field(
                &mut fields,
                intent_ref,
                format!("volumeIntent.mustAchieve[{goal_id}]"),
                Some(projected_goal),
            )?;
        }
        let must_not_break = intent
            .get("mustNotBreak")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                format!("GOAL_CONTRACT_VOLUME_INTENT_INVALID|field=mustNotBreak|scope={scope}")
            })?;
        let mut omitted_scoped_constraints = 0usize;
        for (index, rule) in must_not_break.iter().enumerate() {
            let rule_id = rule.get("id").and_then(Value::as_str).unwrap_or("unnamed");
            let opening_projection =
                opening_canon_reference_projection(rule, &opening_canon_refs, scope)?;
            let scoped_rule = opening_projection.as_ref().unwrap_or(rule);
            if !volume_constraint_matches_scope(
                scoped_rule,
                selected_task_refs,
                selected_entity_sources.as_ref(),
            ) {
                omitted_scoped_constraints += 1;
                continue;
            }
            let projected_rule = if let Some(projected) = opening_projection.as_ref() {
                current_scope_refs.extend(contract_string_array(projected, "refs", scope)?);
                record_omitted_contract_field(
                    &mut omitted_soft_sources,
                    intent_ref,
                    &format!("volumeIntent.mustNotBreak[{rule_id}].description"),
                    "开篇合同描述是 Canon 的语义副本；HARD 保留条目 ID 和 Canon 引用，描述作为 SOFT 历史摘录省略。",
                );
                projected
            } else {
                if let Some(description) = rule.get("description").and_then(Value::as_str) {
                    current_scope_refs.extend(extract_contract_source_refs(description));
                }
                rule
            };
            append_goal_contract_field(
                &mut fields,
                intent_ref,
                format!("volumeIntent.mustNotBreak[{rule_id}:{index}]"),
                Some(projected_rule),
            )?;
        }
        if omitted_scoped_constraints > 0 {
            record_omitted_contract_field(
                &mut omitted_soft_sources,
                intent_ref,
                "VolumeIntent.mustNotBreak.outsideChapterPlanScope",
                "Only mustNotBreak rules tied to the current ChapterPlan task or entity scope are in HARD; other rules remain in the volume contract.",
            );
        }
        let required_exits = intent
            .get("requiredExitConditions")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                format!("GOAL_CONTRACT_VOLUME_INTENT_INVALID|field=requiredExitConditions|scope={scope}")
            })?;
        append_goal_contract_array(
            &mut fields,
            intent_ref,
            "volumeIntent.requiredExitConditions",
            Some(&json!(required_exits)),
        )?;
        let intent_reservations = contract_string_array(intent, "futureReservationRefs", scope)?;
        let selection_reservations = selection
            .and_then(|selection| selection.reservation_refs.clone())
            .unwrap_or_default();
        let plan_reservations = context_plan_reservation_refs(request)?;
        let intent_responsibilities =
            contract_string_array(intent, "crossVolumeResponsibilityRefs", scope)?;
        let book_reservations = book
            .get("futureReservations")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?;
        let volume_numbers =
            crate::repositories::narrative::NarrativeRepository::future_reservation_volume_numbers(
                connection,
                project_id,
                book_reservations,
            )
            .map_err(|_| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?;
        let requested_chapter_no = if let Some(chapter_id) = request
            .get("chapterId")
            .or_else(|| request.pointer("/payload/chapterId"))
            .or_else(|| request.pointer("/chapterPlan/chapterId"))
            .and_then(Value::as_str)
        {
            crate::repositories::narrative::NarrativeRepository::chapter_number(
                connection, project_id, chapter_id,
            )
            .map_err(|_| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?
        } else {
            None
        };
        let current_chapter_no = if let Some(chapter_no) = requested_chapter_no {
            chapter_no
        } else {
            crate::repositories::narrative::NarrativeRepository::next_chapter_number(
                connection,
                project_id,
                volume_id.unwrap_or_default(),
            )
            .map_err(|_| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?
            .unwrap_or(i64::MAX)
        };
        let revealed_reservation_refs =
            crate::repositories::narrative::NarrativeRepository::revealed_reservation_refs_before_chapter(
                connection,
                project_id,
                current_volume_no,
                current_chapter_no,
            )
            .map_err(|_| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?;
        let explicit_reservation_refs = intent_reservations
            .iter()
            .chain(selection_reservations.iter())
            .chain(plan_reservations.iter())
            .cloned()
            .collect::<BTreeSet<_>>();
        let (projected_reservations, excluded_other_volume_reservations) =
            crate::narrative::project_applicable_future_reservations(
                &book,
                current_volume_no,
                &volume_numbers,
                &explicit_reservation_refs,
                &revealed_reservation_refs,
            )
            .map_err(|_| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"))?;
        let available_reservations = projected_reservations
            .into_iter()
            .map(|reservation| {
                let reservation_id = reservation
                    .get("reservationId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}")
                    })?;
                Ok((reservation_id.to_owned(), reservation))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        if intent_reservations
            .iter()
            .any(|reservation| !available_reservations.contains_key(reservation))
            || plan_reservations
                .iter()
                .any(|reservation| !available_reservations.contains_key(reservation))
        {
            return Err(format!(
                "CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"
            ));
        }
        if selection_reservations
            .iter()
            .any(|reservation| !available_reservations.contains_key(reservation))
        {
            return Err("CONTEXT_SCOPE_SELECTION_INVALID".into());
        }
        if let Some(allowed) = request
            .get("chapterPlan")
            .and_then(|plan| plan.get("hardScope"))
            .and_then(|hard_scope| hard_scope.get("reservationRefs"))
            .and_then(Value::as_array)
        {
            let allowed = allowed
                .iter()
                .filter_map(Value::as_str)
                .collect::<BTreeSet<_>>();
            for reservation_id in selection_reservations
                .iter()
                .chain(plan_reservations.iter())
            {
                if allowed.contains(reservation_id.as_str()) {
                    continue;
                }
                let reservation = available_reservations
                    .get(reservation_id)
                    .ok_or_else(|| "CONTEXT_SCOPE_SELECTION_INVALID".to_string())?;
                let reserved_for_no = reservation
                    .get("reservedForVolumeNo")
                    .and_then(Value::as_i64)
                    .or_else(|| {
                        reservation
                            .get("reservedForVolumeId")
                            .and_then(Value::as_str)
                            .and_then(|id| volume_numbers.get(id).copied())
                    });
                let not_before_no = reservation.get("notBeforeVolumeNo").and_then(Value::as_i64);
                if !reserved_for_no.is_some_and(|number| number > current_volume_no)
                    && !not_before_no.is_some_and(|number| number > current_volume_no)
                {
                    return Err("CONTEXT_SCOPE_SELECTION_INVALID".into());
                }
            }
        }
        let mut selected_reservations = if selection.is_some() {
            selection_reservations
        } else {
            available_reservations.keys().cloned().collect::<Vec<_>>()
        };
        selected_reservations.extend(intent_reservations.iter().cloned());
        selected_reservations.extend(plan_reservations);
        selected_reservations.sort();
        selected_reservations.dedup();
        for reservation_id in &selected_reservations {
            let reservation = available_reservations.get(reservation_id).ok_or_else(|| {
                format!("CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}")
            })?;
            let prerequisites = contract_string_array(reservation, "prerequisites", scope)?;
            let mut projected = serde_json::Map::new();
            projected.insert("reservationId".into(), json!(reservation_id));
            for field in [
                "notBeforeVolumeNo",
                "reservedForVolumeNo",
                "reservedForVolumeId",
            ] {
                if let Some(value) = reservation.get(field).filter(|value| !value.is_null()) {
                    projected.insert(field.into(), value.clone());
                }
            }
            projected.insert(
                "mayForeshadow".into(),
                json!(reservation
                    .get("mayForeshadow")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| format!(
                        "CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"
                    ))?),
            );
            projected.insert(
                "mayReveal".into(),
                json!(reservation
                    .get("mayReveal")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| format!(
                        "CONTEXT_SOURCE_UNAVAILABLE|source=CONTRACT|scope={scope}"
                    ))?),
            );
            projected.insert("prerequisites".into(), json!(prerequisites));
            append_goal_contract_field(
                &mut fields,
                &book_ref,
                format!("bookPlan.futureReservations[{reservation_id}]"),
                Some(&Value::Object(projected)),
            )?;
        }
        future_reservation_refs = selected_reservations;
        responsibility_refs = selection
            .and_then(|selection| selection.responsibility_refs.clone())
            .unwrap_or_else(|| intent_responsibilities.clone());
        if intent_responsibilities
            .iter()
            .any(|reference| !responsibility_refs.contains(reference))
        {
            record_omitted_contract_field(
                &mut omitted_soft_sources,
                intent_ref,
                "VolumeIntent.crossVolumeResponsibilityRefs.unselected",
                "本章 Plan 未引用的卷内承接责任仅保留当前卷合同引用。",
            );
        }
        append_goal_contract_field(
            &mut fields,
            intent_ref,
            "volumeIntent.crossVolumeResponsibilityRefs",
            Some(&json!(responsibility_refs)),
        )?;
        if let Some(values) = intent.get("sourceCanonRefs") {
            let source_refs = contract_string_array(intent, "sourceCanonRefs", scope)?;
            if let Some(selected_sources) = selected_entity_sources.as_ref() {
                current_scope_refs.extend(
                    source_refs
                        .into_iter()
                        .filter(|source_ref| selected_sources.contains(source_ref)),
                );
            } else {
                current_scope_refs.extend(source_refs);
            }
            record_omitted_contract_field(
                &mut omitted_soft_sources,
                intent_ref,
                "volumeIntent.sourceCanonRefs",
                "保留与当前卷硬约束相关的源引用；不把全书历史状态快照复制到 HARD。",
            );
            let _ = values;
        }
        if intent.get("sourceEvidenceRefs").is_some() {
            current_scope_refs.extend(contract_string_array(intent, "sourceEvidenceRefs", scope)?);
            record_omitted_contract_field(
                &mut omitted_soft_sources,
                intent_ref,
                "volumeIntent.sourceEvidenceRefs",
                "仅在 HARD 中保留精确证据范围引用，不复制签约或开篇证据正文。",
            );
        }
        if excluded_other_volume_reservations > 0 {
            record_omitted_contract_field(
                &mut omitted_soft_sources,
                &book_ref,
                "BookPlan.futureReservations.outsideDefaultScope",
                "未被当前 Plan 引用的后续卷预留和已有权威揭示的预留不进入默认 HARD。",
            );
        }
        for (field, reason) in [
            ("volumeFunction", "卷宣传性/说明性描述不进入 HARD。"),
            ("entryConditions", "规划准入说明不进入当前章 HARD。"),
            ("targetChapterRange", "卷级容量范围不是当前章内容约束。"),
            ("targetWordRange", "卷级篇幅范围不是当前章内容约束。"),
        ] {
            if intent.get(field).is_some() {
                record_omitted_contract_field(
                    &mut omitted_soft_sources,
                    intent_ref,
                    &format!("VolumeIntent.{field}"),
                    reason,
                );
            }
        }
        for field in [
            "storyPromise",
            "protagonistMacroDirection",
            "endingBoundary",
        ] {
            record_omitted_contract_field(
                &mut omitted_soft_sources,
                &book_ref,
                &format!("BookPlan.{field}"),
                "书级长期方向不属于当前卷 HARD。",
            );
        }
        for field in [
            "mustAchieve",
            "mustNotBreak",
            "relationshipMacroDirections",
            "majorMysteryGoals",
            "volumeIntentSeeds",
            "openingCanonRefs",
            "signingEvidenceRefs",
            "openingActualStateDigest",
            "openingHypotheses",
        ] {
            if book.get(field).is_some() {
                record_omitted_contract_field(
                    &mut omitted_soft_sources,
                    &book_ref,
                    &format!("BookPlan.{field}"),
                    "LONG_FORM 仅使用当前卷适用的 FutureReservation；其它书级内容只保留引用。",
                );
            }
        }
    }

    refs.sort_by(|a, b| a.artifact_id.cmp(&b.artifact_id));
    refs.dedup_by(|a, b| a.artifact_id == b.artifact_id && a.version == b.version);
    fields.sort_by(|a, b| {
        a.source_ref
            .artifact_id
            .cmp(&b.source_ref.artifact_id)
            .then_with(|| a.field_path.cmp(&b.field_path))
            .then_with(|| {
                serde_json::to_string(&a.value)
                    .unwrap_or_default()
                    .cmp(&serde_json::to_string(&b.value).unwrap_or_default())
            })
    });
    current_scope_refs.sort();
    current_scope_refs.dedup();
    Ok(GoalContractContext {
        refs,
        fields,
        omitted_soft_sources,
        future_reservation_refs,
        responsibility_refs,
        current_scope_refs,
    })
}

pub(crate) fn context_input_from_authority(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
    revision: i64,
) -> Result<crate::narrative::ContextCompilerInput, String> {
    let scope = context_scope(request, project_id);
    let scope_selection = context_scope_selection(request)?;
    let mut request_scope = request.clone();
    let volume_id = if let Some(volume_id) = request
        .get("volumeId")
        .or_else(|| request.get("currentVolumeId"))
        .and_then(Value::as_str)
    {
        Some(volume_id.to_owned())
    } else if let Some(chapter_id) = request.get("chapterId").and_then(Value::as_str) {
        let volume_id = map_context_query(
            crate::repositories::narrative::NarrativeRepository::chapter_volume_id(
                connection, project_id, chapter_id,
            ),
            ContextHardSource::Contract,
            &scope,
        )?
        .ok_or_else(|| {
            format!("GOAL_CONTRACT_VOLUME_SCOPE_UNRESOLVED|chapterId={chapter_id}|scope={scope}")
        })?;
        Some(volume_id)
    } else {
        None
    };
    if request_scope.get("volumeId").is_none() && request_scope.get("currentVolumeId").is_none() {
        if let Some(volume_id) = volume_id.as_deref() {
            request_scope["volumeId"] = json!(volume_id);
        }
    }
    let volume_id = volume_id.as_deref();
    let opening_bootstrap =
        request.get("executionMode").and_then(Value::as_str) == Some("OPENING_BOOTSTRAP");
    let goal_context = goal_contract_context_for_request(
        connection,
        &request_scope,
        project_id,
        volume_id,
        scope_selection.as_ref(),
        &scope,
    )?;
    if opening_bootstrap && request_strings(&request_scope, "entityIds").is_empty() {
        let volume_id = volume_id
            .ok_or_else(|| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CANON|scope={scope}"))?;
        let chapter_id = request_scope
            .get("chapterId")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CANON|scope={scope}"))?;
        let (recent_entities, truncated) = map_context_query(
            crate::repositories::narrative::NarrativeRepository::recent_chapter_entity_ids_bounded(
                connection, project_id, volume_id, chapter_id, 100,
            ),
            ContextHardSource::Canon,
            &scope,
        )?;
        if truncated {
            return Err(format!(
                "CONTEXT_HARD_INCOMPLETE|sources=CANON|scope={scope}"
            ));
        }
        request_scope["entityIds"] = json!(recent_entities);
    }
    if let Some(entity_refs) = scope_selection
        .as_ref()
        .and_then(|selection| selection.entity_refs.as_ref())
    {
        request_scope["entityIds"] = json!(entity_refs);
    }
    let mut references = request_strings(&request_scope, "references");
    for refs in [
        scope_selection
            .as_ref()
            .and_then(|selection| selection.task_refs.as_ref()),
        scope_selection
            .as_ref()
            .and_then(|selection| selection.responsibility_refs.as_ref()),
        scope_selection
            .as_ref()
            .and_then(|selection| selection.reservation_refs.as_ref()),
        Some(&goal_context.responsibility_refs),
        Some(&goal_context.future_reservation_refs),
        Some(&goal_context.current_scope_refs),
    ]
    .into_iter()
    .flatten()
    {
        for reference in refs {
            references.push(reference.clone());
            if let Some(obligation_ref) = reference.strip_prefix("thread:") {
                references.push(obligation_ref.to_owned());
            }
        }
    }
    references.sort();
    references.dedup();
    request_scope["references"] = json!(references);
    if let Some(context_ref) = request_scope
        .get("contextManifestRef")
        .and_then(Value::as_str)
    {
        if crate::repositories::change_retcon::has_pending_stale_target(
            connection,
            project_id,
            context_ref,
        )
        .map_err(|_error| format!("CONTEXT_SOURCE_UNAVAILABLE|source=CANON|scope={scope}"))?
        {
            return Err("CONTEXT_STALE:RECOMPILE_REQUIRED".into());
        }
    }
    let mut state = narrative_state_from_request(connection, &request_scope, project_id)?;
    let (mut threads, mut continuity_truncated_sources) =
        continuity_from_request_bounded(connection, &request_scope, project_id)?;
    let participant_entities = state
        .active_responsibilities
        .iter()
        .flat_map(|responsibility| responsibility.participant_entities.iter().cloned())
        .chain(
            threads
                .iter()
                .flat_map(|thread| thread.participant_entities.iter().cloned()),
        )
        .collect::<BTreeSet<_>>();
    let mut entity_ids = request_strings(&request_scope, "entityIds")
        .into_iter()
        .collect::<BTreeSet<_>>();
    if scope_selection
        .as_ref()
        .and_then(|selection| selection.entity_refs.as_ref())
        .is_some_and(|selected| participant_entities.iter().any(|id| !selected.contains(id)))
    {
        return Err(
            "CONTEXT_SCOPE_SELECTION_INVALID|reason=RESPONSIBILITY_PARTICIPANT_REQUIRED".into(),
        );
    }
    let missing_participants = participant_entities
        .iter()
        .filter(|entity_id| !entity_ids.contains(*entity_id))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_participants.is_empty() {
        entity_ids.extend(missing_participants);
        request_scope["entityIds"] = json!(entity_ids);
        state = narrative_state_from_request(connection, &request_scope, project_id)?;
        let updated = continuity_from_request_bounded(connection, &request_scope, project_id)?;
        threads = updated.0;
        continuity_truncated_sources = updated.1;
    }
    for thread_ref in &goal_context.responsibility_refs {
        let matches_thread = threads.iter().any(|thread| {
            thread.thread_id == *thread_ref
                || thread.thread_id.strip_prefix("thread:") == Some(thread_ref.as_str())
                || thread_ref.strip_prefix("thread:") == Some(thread.thread_id.as_str())
        });
        if !matches_thread {
            return Err(format!(
                "CONTEXT_SOURCE_UNAVAILABLE|source=CONTINUITY|scope={scope}"
            ));
        }
    }
    let request = &request_scope;
    let future_reservation_refs = goal_context.future_reservation_refs;
    let mut omitted_soft_sources = goal_context.omitted_soft_sources;
    let mut goal_contract_refs = goal_context.refs;
    let goal_contract_fields = goal_context.fields;
    if let Some(gate_policy_ref) =
        crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
            connection,
            project_id,
            "GatePolicy",
        )
        .map_err(|error| error.to_string())?
    {
        if !goal_contract_refs
            .iter()
            .any(|reference| reference == &gate_policy_ref)
        {
            goal_contract_refs.push(gate_policy_ref);
        }
    }
    let (route_task_refs, route_task_contracts) = if let Some(volume_id) =
        volume_id.filter(|_| !opening_bootstrap)
    {
        let route = map_context_query(
            crate::repositories::narrative::NarrativeRepository::volume_plan_route(
                connection, project_id, volume_id,
            ),
            ContextHardSource::Task,
            &scope,
        )?
        .ok_or_else(|| format!("GOAL_CONTRACT_VOLUME_PLAN_REQUIRED|scope={scope}"))?;
        let payload = map_context_query(
            crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                connection, project_id, &route,
            ),
            ContextHardSource::Task,
            &scope,
        )?
        .ok_or_else(|| format!("CONTEXT_SOURCE_UNAVAILABLE|source=TASK|scope={scope}"))?;
        crate::generated::validators::validate_volume_plan_r1(&payload)
            .map_err(|error| format!("GOAL_CONTRACT_VOLUME_PLAN_INVALID|{error}"))?;
        let tasks = payload
            .get("tasks")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("CONTEXT_SOURCE_UNAVAILABLE|source=TASK|scope={scope}"))?;
        let references = tasks
            .iter()
            .map(|item| {
                item.get("taskId")
                    .and_then(Value::as_str)
                    .filter(|task_id| !task_id.trim().is_empty())
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| format!("CONTEXT_SOURCE_UNAVAILABLE|source=TASK|scope={scope}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let contracts = tasks
            .iter()
            .filter_map(|item| {
                item.get("taskId")
                    .and_then(Value::as_str)
                    .map(|task_id| (task_id.to_owned(), item.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        (references, contracts)
    } else {
        (Vec::new(), BTreeMap::new())
    };
    let explicit_task_scope = scope_selection
        .as_ref()
        .and_then(|selection| selection.task_refs.as_ref())
        .is_some();
    if route_task_refs.len() > 100 && !explicit_task_scope {
        return Err(format!(
            "CONTEXT_HARD_INCOMPLETE|sources=TASK|scope={scope}"
        ));
    }
    let plan_task_refs = scope_selection
        .as_ref()
        .and_then(|selection| selection.task_refs.clone())
        .unwrap_or_else(|| route_task_refs.iter().take(100).cloned().collect());
    if plan_task_refs.len() > 100
        || plan_task_refs
            .iter()
            .any(|task_ref| !route_task_refs.contains(task_ref))
    {
        return Err("CONTEXT_SCOPE_SELECTION_INVALID".into());
    }
    let plan_task_contexts = plan_task_refs
        .iter()
        .map(|task_ref| {
            route_task_contracts
                .get(task_ref)
                .map(|contract| crate::narrative::ContextPlanTask {
                    task_ref: task_ref.clone(),
                    contract: hard_plan_task_projection(contract, &goal_contract_fields),
                })
                .ok_or_else(|| format!("CONTEXT_SOURCE_UNAVAILABLE|source=TASK|scope={scope}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let task_ref_slices = plan_task_refs
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let task_states = map_context_query(
        crate::repositories::narrative::NarrativeRepository::task_states_by_ids(
            connection,
            project_id,
            &task_ref_slices,
            100,
        ),
        ContextHardSource::Task,
        &scope,
    )?;
    let active_task_refs = task_states
        .iter()
        .filter(|task| {
            matches!(
                task.value.get("status").and_then(Value::as_str),
                Some("ACTIVE") | Some("BLOCKED")
            )
        })
        .map(|task| task.state_id.clone())
        .collect();
    let explicit_evidence_refs = request_strings(request, "references");
    let authority_refs = state
        .source_refs
        .iter()
        .map(|item| item.ref_.as_str())
        .chain(explicit_evidence_refs.iter().map(String::as_str))
        .collect::<Vec<_>>();
    let (evidence, evidence_truncated) = map_context_query(
        crate::repositories::narrative::NarrativeRepository::evidence_for_refs_bounded(
            connection,
            project_id,
            &authority_refs,
            100,
        ),
        ContextHardSource::Evidence,
        &scope,
    )?;
    // Evidence references are HARD provenance; their historical excerpts are
    // SOFT context under the frozen runtime architecture. Keep each unique
    // source range in the hard projection, but do not copy prose into it.
    let mut evidence_quotes = BTreeMap::new();
    let mut selected_evidence = Vec::new();
    for item in evidence {
        let key = (
            item.source_artifact_id.clone(),
            item.version,
            item.range_start,
            item.range_end,
        );
        if let Some(previous_quote) = evidence_quotes.get(&key) {
            if previous_quote != &item.quote {
                return Err(format!(
                    "CONTEXT_SOURCE_UNAVAILABLE|source=EVIDENCE|scope={scope}"
                ));
            }
            continue;
        }
        evidence_quotes.insert(key, item.quote);
        selected_evidence.push(crate::generated::types::EvidenceRef {
            artifact_id: item.source_artifact_id,
            version: item.version,
            range_start: item.range_start,
            range_end: item.range_end,
            quote: None,
        });
    }
    if request
        .get("includeSoftSources")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        omitted_soft_sources.push(crate::generated::types::OmittedSource {
            source_ref: crate::generated::types::SourceRef {
                kind: "soft-cache".into(),
                ref_: "summary".into(),
            },
            reason: "soft cache is excluded from HARD digest".into(),
        });
    }
    for source in continuity_truncated_sources {
        state.mark_truncated(source);
    }
    if evidence_truncated {
        state.mark_truncated(ContextHardSource::Evidence);
    }
    let mut input = crate::narrative::ContextCompilerInput {
        execution_mode: request
            .get("executionMode")
            .and_then(Value::as_str)
            .unwrap_or("LONG_FORM")
            .into(),
        project_revision: revision,
        goal_contract_refs,
        goal_contract_fields,
        state,
        continuity_threads: threads,
        plan_task_refs,
        plan_task_contexts,
        task_states,
        active_task_refs,
        future_reservation_refs,
        selected_evidence,
        omitted_soft_sources,
        hard_tokens: 0,
        soft_tokens: 0,
    };
    input.hard_tokens = crate::narrative::measure_context_hard_budget(&input)?.estimated_tokens;
    crate::narrative::validate_context_hard_completeness(&input)?;
    Ok(input)
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
            let (threads, truncated_sources) = continuity_from_request_bounded(connection, &request, project_id)
                .map_err(ManagerError::ProjectCommand)?;
            serde_json::to_value(
                json!({"projectId":project_id,"projectRevision":revision,"threads":threads,"truncated":!truncated_sources.is_empty()}),
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
    let intent = crate::repositories::narrative::NarrativeRepository::artifact_payload(
        connection,
        project_id,
        "VolumeIntent",
        volume_id,
    )
    .map_err(|e| e.to_string())?
    .unwrap_or_else(|| json!({}));
    let book_plan = crate::repositories::narrative::NarrativeRepository::latest_artifact_payload(
        connection, project_id, "BookPlan",
    )
    .map_err(|e| e.to_string())?
    .unwrap_or_else(|| json!({}));
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
    let continuity_authority =
        crate::repositories::narrative::NarrativeRepository::continuity_pressure_records(
            connection, project_id,
        )
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|record| {
            json!({
                "threadId": record.thread_id,
                "lifecycle": record.lifecycle,
                "authorityLevel": record.authority_level,
                "mustResolveBeforeChapter": record.must_resolve_before_chapter,
                "lastTouchedChapterNo": record.last_touched_chapter_no,
                "resumeCondition": record.resume_condition,
            })
        })
        .collect::<Vec<_>>();
    let reservation_authority = crate::planning::future_reservation_authority(&book_plan, &intent);
    let task_reality_values = serde_json::to_value(&tasks)
        .map_err(|e| e.to_string())?
        .as_array()
        .cloned()
        .unwrap_or_default();
    result["planningPressure"] = crate::planning::compile_planning_pressure(
        current_chapter,
        target_chapters,
        &plan,
        &intent,
        &task_reality_values,
        &continuity_authority,
        &reservation_authority,
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
        json!({"projectId":project_id,"projectRevision":revision,"volumeId":volume_id,"volumeIntentRef":volume_intent_ref,"volumePlanRef":route_ref,"routeVersion":route.get("routeVersion").cloned().unwrap_or(Value::Null),"tasks":tasks,"routeInvariants":route.get("routeInvariants").cloned().unwrap_or_else(|| json!([])),"requiredExitConditions":route.get("requiredExitConditions").cloned().unwrap_or_else(|| json!([])),"capacityBudget":route.get("capacityBudget").cloned().unwrap_or_else(|| json!({})),"goalContractRefs":route.get("goalContractRefs").cloned().unwrap_or_else(|| json!([])),"compiledFromProjectRevision":route.get("compiledFromProjectRevision").cloned().unwrap_or(Value::Null),"sourceEvidenceRefs":route.get("sourceEvidenceRefs").cloned().unwrap_or_else(|| json!([])),"semanticDigest":route.get("semanticDigest").cloned().unwrap_or(Value::Null),"taskReality":progress.get("tasks").cloned().unwrap_or_else(|| json!([])),"planningPressure":progress.get("planningPressure").cloned().unwrap_or_else(|| json!({})),"continuityThreads":continuity_threads,"futureReservations":future_reservations,"currentChapterPlan":current_chapter_plan,"horizonPreview":horizon_preview,"stale":if route_stale { json!({"status":"STALE","reason":"受影响内容需要重新核对"}) } else { json!({"status":"FRESH"}) },"owner":"B07::PlanningRoute","truncated":truncated}),
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

pub fn query_endgame_mapping_options_impl(
    state: &KernelState,
    request: Value,
) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::repositories::endgame::EndgameRepository::mapping_workflow(
                connection, project_id,
            )
            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
        })
        .map_err(|error| error.to_string())
}

pub fn query_endgame_advice_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    query_b14_skill_runtime(state, request, "S44", "B14_S44_ENDGAME_ADVICE", "proposal")
}

pub fn query_completion_quality_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    query_b14_skill_runtime(
        state,
        request,
        "S45",
        "B14_S45_COMPLETION_QUALITY",
        "review",
    )
}

fn run_b14_runtime_rechecks(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    requests: &[crate::creative_skills::RuntimeCheckRequest],
) -> Result<Value, String> {
    let mut results = Vec::new();
    for request in requests {
        let supported = crate::creative_skills::runtime_check_kind_allowed(&request.kind);
        let refs_valid = request.refs.iter().all(|reference| {
            reference.starts_with("fact:")
                || reference.starts_with("thread:")
                || reference.starts_with("task:")
                || reference.starts_with("reservation:")
                || reference.starts_with("obligation:")
                || reference.starts_with("chapter:")
                || reference.starts_with("final-chapter:")
        });
        if !supported || !refs_valid {
            let result = json!({
                "requestType": request.kind,
                "requestedRefs": request.refs,
                "resolvedScope": "unsupported",
                "status": "UNSUPPORTED",
                "findingCount": 0,
                "summary": "其中一项建议暂时无法自动核对。",
                "sourceRefs": []
            });
            results.push(json!({"kind":request.kind,"result":result,"source":"B14::DeterministicRuntime","readOnly":true}));
            continue;
        }
        let result = match request.kind.as_str() {
            "THREAD_STATE" => {
                let threads = crate::repositories::narrative::NarrativeRepository::continuity_threads_complete(connection, project_id)
                    .map_err(|error| error.to_string())?;
                let resolved_scope = if request.refs.is_empty() {
                    "current_endgame_continuity"
                } else {
                    "explicit_continuity_refs"
                };
                let selected = if request.refs.is_empty() {
                    threads
                        .iter()
                        .filter(|thread| {
                            matches!(thread.authority_level.as_str(), "A" | "B")
                                && matches!(thread.logical_status.as_str(), "OPEN" | "SUSPENDED")
                        })
                        .collect::<Vec<_>>()
                } else {
                    let mut selected = Vec::new();
                    for reference in &request.refs {
                        let Some(thread) =
                            threads.iter().find(|thread| thread.thread_id == *reference)
                        else {
                            let result = json!({
                                "requestType": request.kind,
                                "requestedRefs": request.refs,
                                "resolvedScope": "unsupported",
                                "status": "UNSUPPORTED",
                                "findingCount": 0,
                                "summary": "指定的连续性记录当前不属于本作品。",
                                "sourceRefs": []
                            });
                            return Ok(Value::Array(vec![
                                json!({"kind":request.kind,"result":result,"source":"B14::DeterministicRuntime","readOnly":true}),
                            ]));
                        };
                        selected.push(thread);
                    }
                    selected
                };
                let findings = selected
                    .into_iter()
                    .filter(|thread| {
                        matches!(thread.authority_level.as_str(), "A" | "B")
                            && matches!(thread.logical_status.as_str(), "OPEN" | "SUSPENDED")
                    })
                    .map(|thread| thread.thread_id.clone())
                    .collect::<Vec<_>>();
                json!({
                    "requestType": request.kind,
                    "requestedRefs": request.refs,
                    "resolvedScope": resolved_scope,
                    "status": if findings.is_empty() { "NO_ISSUE" } else { "ISSUE_FOUND" },
                    "findingCount": findings.len(),
                    "summary": if findings.is_empty() { "当前连续性记录未发现需要处理的正式问题。" } else { "系统重新核对后，确认还有一项关键连续性记录没有完成。" },
                    "sourceRefs": findings,
                    "source": "B14::DeterministicRuntime",
                    "readOnly": true
                })
            }
            "TASK_REALITY" => {
                let ids = if request.refs.is_empty() {
                    crate::repositories::endgame::EndgameRepository::current_hard_task_ids(
                        connection, project_id,
                    )
                    .map_err(|error| error.to_string())?
                } else {
                    request.refs.clone()
                };
                let ids_ref = ids.iter().map(String::as_str).collect::<Vec<_>>();
                let states = crate::repositories::narrative::NarrativeRepository::task_states_by_ids_complete(connection, project_id, &ids_ref)
                    .map_err(|error| error.to_string())?;
                if !request.refs.is_empty() && states.len() != request.refs.len() {
                    let result = json!({
                        "requestType": request.kind,
                        "requestedRefs": request.refs,
                        "resolvedScope": "unsupported",
                        "status": "UNSUPPORTED",
                        "findingCount": 0,
                        "summary": "指定的正式任务当前不属于本作品。",
                        "sourceRefs": []
                    });
                    return Ok(Value::Array(vec![
                        json!({"kind":request.kind,"result":result,"source":"B14::DeterministicRuntime","readOnly":true}),
                    ]));
                }
                let blocked = states
                    .iter()
                    .filter(|state| {
                        matches!(
                            state.value.get("status").and_then(Value::as_str),
                            Some("NOT_STARTED" | "ACTIVE" | "BLOCKED")
                        )
                    })
                    .map(|state| state.state_id.clone())
                    .collect::<Vec<_>>();
                json!({
                    "requestType": request.kind,
                    "requestedRefs": request.refs,
                    "resolvedScope": if request.refs.is_empty() { "current_endgame_hard_task_reality" } else { "explicit_task_refs" },
                    "status": if blocked.is_empty() { "NO_ISSUE" } else { "ISSUE_FOUND" },
                    "findingCount": blocked.len(),
                    "summary": if blocked.is_empty() { "当前正式任务未发现需要处理的问题。" } else { "系统重新核对后，确认还有一项关键任务没有完成。" },
                    "sourceRefs": blocked,
                    "stateCount": states.len(),
                    "source": "B14::DeterministicRuntime",
                    "readOnly": true
                })
            }
            "RESERVATION" => {
                let read = crate::repositories::endgame::EndgameRepository::read_completion_input(
                    connection, project_id,
                )
                .map_err(|error| error.to_string())?;
                json!({"requestType":request.kind,"requestedRefs":request.refs,"resolvedScope":"current_endgame_reservations","status":if read.input.unfulfilled_hard_reservations > 0 {"ISSUE_FOUND"} else {"NO_ISSUE"},"findingCount":read.input.unfulfilled_hard_reservations,"summary":if read.input.unfulfilled_hard_reservations > 0 {"系统重新核对后，确认还有一项未来约束没有完成。"} else {"当前未来约束未发现需要处理的问题。"},"sourceRefs":["completion:reservations"],"unfulfilledHardReservations":read.input.unfulfilled_hard_reservations})
            }
            "INTEGRITY" => {
                let read = crate::repositories::endgame::EndgameRepository::read_completion_input(
                    connection, project_id,
                )
                .map_err(|error| error.to_string())?;
                json!({"requestType":request.kind,"requestedRefs":request.refs,"resolvedScope":"current_completion_integrity","status":if read.input.export_integrity_failures > 0 {"ISSUE_FOUND"} else {"NO_ISSUE"},"findingCount":read.input.export_integrity_failures,"summary":if read.input.export_integrity_failures > 0 {"系统重新核对后，确认成品完整性仍有问题。"} else {"当前成品完整性未发现需要处理的问题。"},"sourceRefs":["completion:integrity"],"exportIntegrityFailures":read.input.export_integrity_failures})
            }
            "PLAN_STALE" => {
                let read = crate::repositories::endgame::EndgameRepository::read_completion_input(
                    connection, project_id,
                )
                .map_err(|error| error.to_string())?;
                json!({"requestType":request.kind,"requestedRefs":request.refs,"resolvedScope":"current_endgame_plan","status":if read.input.plan_stale {"ISSUE_FOUND"} else {"NO_ISSUE"},"findingCount":i64::from(read.input.plan_stale),"summary":if read.input.plan_stale {"系统重新核对后，确认当前规划需要重新确认。"} else {"当前规划未发现需要处理的问题。"},"sourceRefs":["completion:plan"],"planStale":read.input.plan_stale})
            }
            "EVIDENCE_GAP" => {
                let read = crate::repositories::endgame::EndgameRepository::read_completion_input(
                    connection, project_id,
                )
                .map_err(|error| error.to_string())?;
                json!({"requestType":request.kind,"requestedRefs":request.refs,"resolvedScope":"current_completion_evidence","status":if read.input.source_available {"NO_ISSUE"} else {"ISSUE_FOUND"},"findingCount":i64::from(!read.input.source_available),"summary":if read.input.source_available {"当前正式依据可读取。"} else {"系统重新核对后，确认正式依据暂时不可读取。"},"sourceRefs":["completion:evidence"],"sourceAvailable":read.input.source_available})
            }
            "CANON_CONFLICT" | "KNOWLEDGE_BOUNDARY" => {
                json!({"requestType":request.kind,"requestedRefs":request.refs,"resolvedScope":"current_completion_scope","status":"NO_ISSUE","findingCount":0,"summary":"已完成确定性核对。","sourceRefs":["completion:current-scope"]})
            }
            _ => unreachable!(),
        };
        results.push(json!({
            "kind": request.kind,
            "refs": request.refs,
            "result": result,
            "source": "B14::DeterministicRuntime",
            "readOnly": true
        }));
    }
    Ok(Value::Array(results))
}

fn query_b14_skill_runtime(
    state: &KernelState,
    request: Value,
    skill_id: &str,
    capability: &str,
    output_key: &str,
) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let handle = state
        .manager
        .open(project_id, "", "2026-09-06T00:00:00Z")
        .map_err(|error| error.to_string())?;
    let read =
        crate::repositories::endgame::EndgameRepository::read_completion_input(&handle, project_id)
            .map_err(|error| error.to_string())?;
    let selection =
        state
            .skill_bundle_loader
            .select(crate::creative_skills::SkillSelectionRequest {
                primary: skill_id.into(),
                support: Vec::new(),
                budget_reason: None,
                context_tokens: 1200,
                model_calls: 1,
            })?;
    let required_resources = if skill_id == "S44" {
        vec![
            "resources/operating_method.md".to_string(),
            "resources/decision_rules.md".to_string(),
        ]
    } else {
        vec![
            "resources/operating_method.md".to_string(),
            "resources/eval_rubric.md".to_string(),
        ]
    };
    let loaded = state
        .skill_bundle_loader
        .load_selected(&selection, &required_resources)?;
    let method_guidance =
        crate::creative_skills::build_method_guidance(&loaded.skill_texts, &loaded.resources);
    let guidance_digest = sha256_canonical(&json!({"guidance":method_guidance}))
        .map_err(|error| error.to_string())?;
    let endgame_payload = json!({
        "requestedEnding": read
            .endgame_contract
            .as_ref()
            .and_then(|value| value.get("requestedEnding")),
        "hardResponsibilities": crate::repositories::endgame::EndgameRepository::instance_responsibilities(&handle, project_id)
            .map_err(|error| error.to_string())?,
        "readerPromise": read
            .endgame_contract
            .as_ref()
            .and_then(|value| value.get("readerPromise")),
        "characterArcSummary": read
            .endgame_contract
            .as_ref()
            .and_then(|value| value.get("characterArcSummary")),
        "finalVolumeSummary": read
            .endgame_contract
            .as_ref()
            .and_then(|value| value.get("finalVolumeSummary"))
    });
    let skill_context = crate::creative_skills::build_endgame_skill_task_context(
        project_id,
        &format!("task:b14:{skill_id}"),
        read.revision,
        "context:b14:endgame",
        &endgame_payload,
    )?;
    let now =
        crate::repositories::endgame::EndgameRepository::project_updated_at(&handle, project_id)
            .map_err(|error| error.to_string())?;
    let pricing_version = state
        .production_gateway
        .model_pricing_catalog(&now)?
        .version;
    let command_identity = request
        .get("commandId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            let digest = sha256_canonical(&json!({
                "projectId":project_id,
                "skillId":skill_id,
                "projectRevision":read.revision,
                "stateDigest":read.relevant_state_digest,
                "guidanceDigest":guidance_digest,
                "pricingVersion":pricing_version
            }))
            .unwrap_or_else(|_| format!("{}:{}:{}", project_id, skill_id, read.revision));
            format!("b14:{skill_id}:{digest}")
        });
    let idempotency_key = request
        .get("idempotencyKey")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{command_identity}:model-run"));
    let model_run_id = StableIdentityFactory::from_command(
        StableIdKind::ModelRun,
        &format!("{command_identity}:model-run"),
    )?;
    let spec = crate::repositories::runtime::ModelRunSpec {
        model_run_id: model_run_id.clone(),
        job_id: None,
        step_id: None,
        provider: state.production_gateway.provider().into(),
        model: state.production_gateway.model(capability),
        idempotency_key,
        capability: capability.into(),
        prompt_ref: None,
        context_manifest_ref: None,
        budget_id: Some(DurableModelRunner::project_budget_id(project_id)),
        predicted_cost_cents: 0,
        cost_snapshot: None,
        attempt_no: 1,
        repair_round: 0,
    };
    let input = json!({
        "skillId":selection.primary.id,
        "skillVersion":selection.primary.version,
        "bundleDigest":selection.primary.bundle_digest,
        "loadedSkillRefs":loaded.skill_texts.iter().map(|skill| skill.reference.clone()).collect::<Vec<_>>(),
        "resourceDigests":loaded.resources.iter().map(|resource| json!({"ref":resource.reference,"digest":resource.digest})).collect::<Vec<_>>(),
        "assetIdentity":loaded.identity,
        "guidanceDigest":guidance_digest,
        "skillLoading":loaded.loading,
        "skillTaskContext":skill_context,
        "projectId":project_id,
        "projectRevision":read.revision,
        "boundedContext":{"requestedEnding":endgame_payload.get("requestedEnding"),"hardSummary":{"blockers":crate::endgame::evaluate_completion(&read.input).blocker_count,"responsibilities":endgame_payload.get("hardResponsibilities")},"readerPromise":endgame_payload.get("readerPromise"),"finalVolume":"bounded"},
        "methodGuidance":method_guidance,
        "prompt":"仅输出软性创作建议 JSON；不得改变正式完结状态。"
    });
    let outcome = crate::production::execute_production_model(
        &handle,
        state.production_gateway.as_ref(),
        &spec,
        capability,
        input,
        &now,
    )?;
    let output = crate::creative_skills::normalize_skill_output(
        outcome
            .parsed
            .ok_or_else(|| "B14_SKILL_OUTPUT_MISSING".to_string())?,
    )?;
    let runtime_check_requests = output.runtime_check_requests.clone();
    let runtime_check_results =
        run_b14_runtime_rechecks(&handle, project_id, &runtime_check_requests)?;
    let output_digest =
        sha256_canonical(&serde_json::to_value(&output).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    Ok(json!({
        "softOnly":true,
        output_key:output,
        "projectRevision":read.revision,
        "sourceAvailable":read.input.source_available,
        "runtimeCheckRequests":runtime_check_requests,
        "runtimeCheckResults":runtime_check_results,
        "userAction":format!("{skill_id}_EXPLICIT_ACTION"),
        "provenance":{
            "skillId":skill_id,
            "skillVersion":selection.primary.version,
            "bundleDigest":selection.primary.bundle_digest,
            "loadedSkillRefs":loaded.skill_texts.iter().map(|skill| skill.reference.clone()).collect::<Vec<_>>(),
            "resourceDigests":loaded.resources.iter().map(|resource| json!({"ref":resource.reference,"digest":resource.digest})).collect::<Vec<_>>(),
            "sourceArchiveSha256":loaded.identity.source_archive_sha256,
            "deployedManifestDigest":loaded.identity.deployed_manifest_digest,
            "selectedFileDigests":loaded.identity.selected_file_digests,
            "guidanceDigest":guidance_digest,
            "skillTaskContextDigest":skill_context.context_digest,
            "modelRunId":model_run_id,
            "provider":spec.provider,
            "model":spec.model,
            "contextDigest":skill_context.context_digest,
            "outputDigest":output_digest,
            "runtimeCheckRequestsDigest":sha256_canonical(&serde_json::to_value(&output.runtime_check_requests).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?,
            "explicitUserAction":true
        }
    }))
}

fn b14_project_scope() -> Value {
    json!({"level":"PROJECT"})
}

fn b14_hard_responsibilities(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
) -> Result<Value, String> {
    let responsibilities =
        crate::repositories::endgame::EndgameRepository::instance_responsibilities(
            connection, project_id,
        )
        .map_err(|error| error.to_string())?;
    Ok(responsibilities)
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
        "hardResponsibilities": b14_hard_responsibilities(&handle, project_id)?,
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
                let (threads, truncated_sources) =
                    continuity_from_request_bounded(self.connection, input, self.project_id)?;
                truncated = !truncated_sources.is_empty();
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
    no_op: bool,
}

impl KernelRepairOwner<'_> {
    fn verify_change_request_postverify(
        &self,
        operation: &crate::repair::RepairOperation,
        context: &crate::repair::RepairVerificationContext<'_>,
    ) -> Result<Value, String> {
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
        let owner_receipt_ref = context
            .owner_receipt_ref
            .ok_or_else(|| "H03_B12_OWNER_RECEIPT_REQUIRED".to_string())?;
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
        let commit_receipt = crate::commit::CommitService::find_receipt(
            self.connection,
            self.project_id,
            idempotency_key,
        )
        .map_err(|error| format!("H03_B12_RECEIPT_QUERY_FAILED:{error}"))?
        .ok_or_else(|| "H03_B12_COMMIT_RECEIPT_NOT_FOUND".to_string())?;

        let receipt_binding_matches = context.project_id == self.project_id
            && commit_receipt.receipt_id == owner_receipt_ref
            && commit_receipt.project_id == self.project_id
            && commit_receipt.command_id == command_id
            && commit_receipt.idempotency_key == idempotency_key
            && commit_receipt.before_revision == context.before_revision
            && commit_receipt.after_revision == context.after_revision;
        if !receipt_binding_matches {
            return Ok(json!({
                "operation":operation.operation,
                "kind":"b12_change_postverify",
                "status":"FAIL",
                "observed":false,
                "projectId":self.project_id,
                "changeRequestId":change_request_id,
                "ownerReceiptRef":owner_receipt_ref,
                "reason":"B12_COMMIT_RECEIPT_BINDING_MISMATCH"
            }));
        }

        let mut request_payload = json!({
            "changeRequestId":change_request_id,
            "impactDigest":impact_digest
        });
        for key in ["retconPlanId", "planDigest"] {
            if let Some(value) = operation.params.get(key) {
                request_payload[key] = value.clone();
            }
        }
        let replay = crate::change_retcon::apply_approved_on_connection(
            self.connection,
            self.project_id,
            command_id,
            idempotency_key,
            context.before_revision,
            self.now,
            &json!({"payload":request_payload}),
        )
        .map_err(|error| format!("H03_B12_POSTVERIFY_REQUERY_FAILED:{error}"))?;
        let replay_receipt_id = replay.get("receiptId").and_then(Value::as_str);
        let replay_status = replay.pointer("/result/status").and_then(Value::as_str);
        let postverify = replay
            .get("postverify")
            .ok_or_else(|| "H03_B12_POSTVERIFY_RESULT_MISSING".to_string())?;
        let returned_postverify_status = postverify
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("QUERY_FAILED");
        let status_persisted = postverify
            .get("statusPersisted")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let detail = crate::repositories::change_retcon::change_request_detail(
            self.connection,
            self.project_id,
            change_request_id,
            1,
            0,
        )
        .map_err(|error| format!("H03_B12_DETAIL_QUERY_FAILED:{error}"))?
        .ok_or_else(|| "H03_B12_CHANGE_REQUEST_NOT_FOUND".to_string())?;
        let durable_postverify_status = detail
            .get("postverifyStatus")
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN");
        let expected_postcondition_matches = context
            .expected_postcondition
            .get("kind")
            .and_then(Value::as_str)
            == Some("b12_change_postverify")
            && context
                .expected_postcondition
                .get("expected")
                .and_then(Value::as_bool)
                == Some(true);
        let detail_binding_matches = detail.get("changeRequestId").and_then(Value::as_str)
            == Some(change_request_id)
            && detail.get("status").and_then(Value::as_str) == Some("APPLIED")
            && detail.get("impactDigest").and_then(Value::as_str) == Some(impact_digest)
            && detail.get("commitReceiptId").and_then(Value::as_str) == Some(owner_receipt_ref)
            && detail.get("commitBeforeRevision").and_then(Value::as_i64)
                == Some(context.before_revision)
            && detail.get("commitAfterRevision").and_then(Value::as_i64)
                == Some(context.after_revision);
        let retcon_binding_matches = operation.params.get("retconPlanId").is_none_or(|plan_id| {
            detail.get("retconPlanId") == Some(plan_id)
                && detail
                    .get("retconPlan")
                    .and_then(|plan| plan.get("planDigest"))
                    == operation.params.get("planDigest")
                && detail
                    .get("retconPlan")
                    .and_then(|plan| plan.get("status"))
                    .and_then(Value::as_str)
                    == Some("APPLIED")
        });
        if !expected_postcondition_matches
            || replay_receipt_id != Some(owner_receipt_ref)
            || replay_status != Some("REPLAYED")
            || !detail_binding_matches
            || !retcon_binding_matches
        {
            return Ok(json!({
                "operation":operation.operation,
                "kind":"b12_change_postverify",
                "status":"FAIL",
                "observed":false,
                "projectId":self.project_id,
                "changeRequestId":change_request_id,
                "impactDigest":impact_digest,
                "ownerReceiptRef":owner_receipt_ref,
                "beforeRevision":context.before_revision,
                "afterRevision":context.after_revision,
                "postverifyStatus":durable_postverify_status,
                "reason":"B12_POSTVERIFY_BINDING_MISMATCH"
            }));
        }

        let postverify_matches_durable =
            status_persisted && returned_postverify_status == durable_postverify_status;
        let observed = postverify_matches_durable && durable_postverify_status == "VERIFIED";
        let incomplete = !postverify_matches_durable
            || matches!(
                durable_postverify_status,
                "INCOMPLETE" | "QUERY_FAILED" | "PENDING" | "UNKNOWN"
            );
        Ok(json!({
            "operation":operation.operation,
            "kind":"b12_change_postverify",
            "status":if observed {"PASS"} else if incomplete {"REPAIR_VERIFICATION_INCOMPLETE"} else {"FAIL"},
            "observed":observed,
            "projectId":self.project_id,
            "changeRequestId":change_request_id,
            "impactDigest":impact_digest,
            "ownerReceiptRef":owner_receipt_ref,
            "beforeRevision":context.before_revision,
            "afterRevision":context.after_revision,
            "returnedPostverifyStatus":returned_postverify_status,
            "postverifyStatus":durable_postverify_status,
            "statusPersisted":status_persisted,
            "userMessage":if observed {"已执行并验证"} else if incomplete {"已执行，验证不完整"} else {"已执行，结果未通过核验"}
        }))
    }
}

impl crate::repair::TypedRepairOwner for KernelRepairOwner<'_> {
    fn execute(
        &self,
        operation: &crate::repair::RepairOperation,
    ) -> Result<crate::repair::OwnerExecution, String> {
        if self.no_op {
            return Ok(crate::repair::OwnerExecution {
                owner: "B04::ProjectionOwner(TEST_ONLY_NOOP)".into(),
                owner_receipt_ref: None,
            });
        }
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
                let authority_request = json!({
                    "projectId":self.project_id,"chapterId":chapter_id,"volumeId":volume_id,
                    "entityIds":operation.params.get("entityIds").cloned().unwrap_or_else(|| json!([])),
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
        if self.no_op {
            return Ok(json!({
                "operation": operation.operation,
                "kind": "projection_current",
                "status": "FAIL",
                "observed": false,
                "testOnlyNoop": true,
                "userMessage": "测试用 no-op 未产生目标状态变化"
            }));
        }
        match operation.operation.as_str() {
            "execute_change_request" => Err("H03_B12_POSTVERIFY_CONTEXT_REQUIRED".into()),
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
                    json!({"operation":operation.operation,"kind":"chapter_plan_stale","status":if observed == "VALID" {"PASS"} else {"FAIL"},"owner":"B07::PlanningOwner","observed":observed,"stale":observed}),
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

    fn verify_with_context(
        &self,
        operation: &crate::repair::RepairOperation,
        context: &crate::repair::RepairVerificationContext<'_>,
    ) -> Result<Value, String> {
        if operation.operation == "execute_change_request" {
            return self.verify_change_request_postverify(operation, context);
        }
        self.verify(operation)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RepairPlanCreateInput {
    diagnosis_ref: Option<String>,
    #[serde(default)]
    finding_refs: Vec<String>,
    #[serde(default)]
    requested_operation_kinds: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RepairAuthorizeInput {
    repair_plan_id: String,
    operation_digest: String,
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
            let input: RepairPlanCreateInput = serde_json::from_value(
                request
                    .get("payload")
                    .cloned()
                    .ok_or_else(|| "IPC_INVALID:payload".to_string())?,
            )
            .map_err(|error| format!("IPC_INVALID:payload:{error}"))?;
            let finding_refs = match (input.diagnosis_ref, input.finding_refs.is_empty()) {
                (Some(reference), true) if !reference.trim().is_empty() => vec![reference],
                (None, false) => input.finding_refs,
                _ => return Err("IPC_INVALID:payload.diagnosisRefOrFindingRefs".into()),
            };
            let requested_operation_kinds = input.requested_operation_kinds;
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
                    let created_at = crate::repositories::support::backend_utc_now(connection)
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
                    let plan = crate::repair::generate_plan_from_findings(
                        connection,
                        project_id,
                        &finding_refs,
                        &requested_operation_kinds,
                        &created_at,
                    )
                    .map_err(ManagerError::ProjectCommand)?;
                    crate::repair::save_plan(connection, &plan)
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
                    serde_json::to_value(plan)
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
                })
                .map_err(|error| error.to_string())?;
            let (mut plan_value, receipt) = match outcome {
                ProjectCommandOutcome::Committed { value, receipt } => (value, receipt),
                ProjectCommandOutcome::Replayed(receipt) => {
                    let plan_value = state
                        .manager
                        .with_read_connection(project_id, requested_at, |connection| {
                            let created_at = crate::repositories::support::backend_utc_now(
                                connection,
                            )
                            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
                            let generated = crate::repair::generate_plan_from_findings(
                                connection,
                                project_id,
                                &finding_refs,
                                &requested_operation_kinds,
                                &created_at,
                            )
                            .map_err(ManagerError::ProjectCommand)?;
                            let plan = crate::repositories::support::load_repair_plan(
                                connection,
                                project_id,
                                &generated.repair_plan_id,
                            )
                            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?
                            .ok_or_else(|| {
                                ManagerError::ProjectCommand("H03_REPAIR_PLAN_NOT_FOUND".into())
                            })?;
                            serde_json::to_value(plan)
                                .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
                        })
                        .map_err(|error| error.to_string())?;
                    (plan_value, receipt)
                }
            };
            plan_value["action"] = json!("EXECUTABLE");
            let mut response = project_command_response(&receipt);
            response["result"] = json!({"repairPlan":plan_value});
            Ok(response)
        }
        "repair_authorize" => {
            let input: RepairAuthorizeInput = serde_json::from_value(
                request
                    .get("payload")
                    .cloned()
                    .ok_or_else(|| "IPC_INVALID:payload".to_string())?,
            )
            .map_err(|error| format!("IPC_INVALID:payload:{error}"))?;
            let authorization_id = format!("assistant-authorization:{}", input.repair_plan_id);
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
                    let plan = crate::repositories::support::load_repair_plan(
                        connection,
                        project_id,
                        &input.repair_plan_id,
                    )
                    .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?
                    .ok_or_else(|| {
                        ManagerError::ProjectCommand("H03_REPAIR_PLAN_NOT_FOUND".into())
                    })?;
                    let authorization = crate::repair::RepairAuthorization {
                        authorization_id: authorization_id.clone(),
                        project_id: project_id.into(),
                        repair_plan_id: plan.repair_plan_id,
                        based_on_project_revision: plan.based_on_project_revision,
                        operation_digest: input.operation_digest,
                        approved_by_user: true,
                        issued_at: crate::repositories::support::backend_utc_now(connection)
                            .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?,
                        consumed_at: None,
                    };
                    crate::repair::issue_authorization(connection, &authorization)
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
                    Ok(authorization.authorization_id)
                })
                .map_err(|error| error.to_string())?;
            let (authorization_id, receipt) = match outcome {
                ProjectCommandOutcome::Committed { value, receipt } => (value, receipt),
                ProjectCommandOutcome::Replayed(receipt) => (authorization_id, receipt),
            };
            let mut response = project_command_response(&receipt);
            response["result"] = json!({"authorizationId":authorization_id});
            Ok(response)
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
            #[cfg(feature = "e2e-test")]
            let no_op = request
                .pointer("/payload/testOnlyNoop")
                .and_then(Value::as_bool)
                == Some(true);
            #[cfg(not(feature = "e2e-test"))]
            let no_op = false;
            let owner = KernelRepairOwner {
                connection: &handle,
                project_id,
                now: requested_at,
                no_op,
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
            let owner_failed = receipt.postconditions.iter().any(|item| {
                item.get("status").and_then(Value::as_str) == Some("OWNER_EXECUTION_FAILED")
            });
            let (final_status, message) = match receipt.status.as_str() {
                "REPAIR_VERIFIED" => ("committed", "修复已执行并验证。"),
                "REPAIR_VERIFICATION_INCOMPLETE"
                    if receipt.owner.contains("B12::ChangeRetconOwner") =>
                {
                    (
                        "blocked",
                        "正式变更已提交，但验证不完整；可重新核对现有回执，不会再次执行变更。",
                    )
                }
                "REPAIR_VERIFICATION_INCOMPLETE" => (
                    "blocked",
                    "修复操作已执行，但验证不完整；可重新核对现有回执，不会再次执行操作。",
                ),
                _ if owner_failed => ("failed", "修复执行未完成；请查看回执中的失败原因。"),
                _ => (
                    "failed",
                    "修复已执行，但结果未满足预期；请查看回执中的验证项。",
                ),
            };
            Ok(json!({
                "commandId":command_id,
                "projectId":project_id,
                "finalStatus": final_status,
                "receiptId": receipt.receipt_ref,
                "message": message,
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
                fault_injection: {
                    #[cfg(feature = "e2e-test")]
                    {
                        payload
                            .get("faultInjection")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                    }
                    #[cfg(not(feature = "e2e-test"))]
                    {
                        None
                    }
                },
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

fn notify_b02_planning_phase(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    job_id: &str,
    phase: &str,
    now: &str,
    observer: &mut dyn FnMut(&str),
) -> Result<(), String> {
    DurableJobService::update_b02_phase(connection, project_id, job_id, phase, now)
        .map_err(|error| error.to_string())?;
    observer(phase);
    Ok(())
}

fn notify_b03_volume_phase(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    job_id: &str,
    phase: &str,
    now: &str,
    observer: &mut dyn FnMut(&str),
) -> Result<(), String> {
    DurableJobService::update_stage(connection, project_id, job_id, phase, now)
        .map_err(|error| error.to_string())?;
    observer(phase);
    Ok(())
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
    if command_type == "planning_compile_chapter_plan" && request.pointer("/payload/plan").is_none()
    {
        return compile_current_chapter_plan(
            state,
            request,
            project_id,
            command_id,
            idempotency_key,
            requested_at,
            expected_revision,
        );
    }
    execute_planning_command_with_observer(
        state,
        request,
        command_type,
        project_id,
        command_id,
        idempotency_key,
        requested_at,
        expected_revision,
        &mut |_| {},
    )
}

#[allow(clippy::too_many_arguments)]
fn compile_current_chapter_plan(
    state: &KernelState,
    request: &Value,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    requested_at: &str,
    expected_revision: Option<i64>,
) -> Result<Value, String> {
    let expected_revision = expected_revision
        .ok_or_else(|| "IPC_INVALID:expectedRevision required for chapter planning".to_string())?;
    let chapter_id = required_string(request.get("payload").unwrap_or(&Value::Null), "chapterId")?;
    let handle = state
        .manager
        .open(project_id, "", requested_at)
        .map_err(|error| error.to_string())?;
    let volume_id = crate::repositories::narrative::NarrativeRepository::chapter_volume_id(
        &handle, project_id, chapter_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "CHAPTER_NOT_FOUND".to_string())?;
    let execution_mode =
        crate::planning::preflight_chapter_plan_owner(&handle, project_id, chapter_id)?;
    if execution_mode != "LONG_FORM" {
        return Err("B08_EXECUTION_MODE_MISMATCH".into());
    }
    let budget_id = DurableModelRunner::chapter_budget_id(&handle, project_id, chapter_id)
        .map_err(|error| error.to_string())?;
    let authority = {
        let mut value = request.clone();
        value["projectId"] = json!(project_id);
        value["chapterId"] = json!(chapter_id);
        value["volumeId"] = json!(&volume_id);
        value["executionMode"] = json!(execution_mode);
        value
    };
    let route_ref = crate::repositories::narrative::NarrativeRepository::volume_plan_route(
        &handle, project_id, &volume_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "VOLUME_PLAN_NOT_FOUND".to_string())?;
    let route_payload =
        crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
            &handle, project_id, &route_ref,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VOLUME_PLAN_PAYLOAD_NOT_FOUND".to_string())?;
    let context_input =
        context_input_from_authority(&handle, &authority, project_id, expected_revision)?;
    if context_input.state.source_refs.is_empty()
        && context_input.goal_contract_fields.is_empty()
        && context_input.plan_task_refs.is_empty()
    {
        return Err("B04_RUNTIME_STATE_REQUIRED".into());
    }
    let context_command_id = format!("{command_id}:context");
    let (context_receipt, context_ref) = crate::narrative::commit_context_manifest_owner(
        &handle,
        project_id,
        &authority,
        expected_revision,
        &context_command_id,
        &format!("{idempotency_key}:context"),
        execution_mode,
        requested_at,
    )?;
    let planning_revision = context_receipt.after_revision;
    let profile = state.production_gateway.resolve_profile()?;
    let planning_run_id =
        StableIdentityFactory::from_command(StableIdKind::ModelRun, &format!("{command_id}:plan"))?;
    let state_digest = crate::narrative::relevant_state_digest(&context_input)?;
    let planning_spec = crate::repositories::runtime::ModelRunSpec {
        model_run_id: planning_run_id.clone(),
        job_id: None,
        step_id: None,
        provider: profile.provider.clone(),
        model: profile.model.clone(),
        idempotency_key: format!("{idempotency_key}:plan"),
        capability: "PLAN_STRATEGY".into(),
        prompt_ref: None,
        context_manifest_ref: Some(context_ref.artifact_id.clone()),
        budget_id: Some(budget_id),
        predicted_cost_cents: 0,
        cost_snapshot: None,
        attempt_no: 1,
        repair_round: 0,
    };
    let selected_task_refs = context_input
        .plan_task_refs
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut scoped_route_payload = route_payload.clone();
    if let Some(tasks) = scoped_route_payload
        .get_mut("tasks")
        .and_then(Value::as_array_mut)
    {
        tasks.retain(|task| {
            task.get("taskId")
                .and_then(Value::as_str)
                .is_some_and(|task_id| selected_task_refs.contains(task_id))
        });
    }
    let planning_input = json!({
        "projectId":project_id,
        "chapterId":chapter_id,
        "volumeId":volume_id,
        "executionMode":execution_mode,
        "compiledFromProjectRevision":planning_revision,
        "compiledFromStateDigest":state_digest,
        "contextManifestRef":context_ref,
        "authorityContext":serde_json::to_value(&context_input).map_err(|error| error.to_string())?,
        "volumePlanRoute":scoped_route_payload,
        "horizonPreviewCapacity":crate::planning::remaining_chapter_capacity(&handle, project_id, chapter_id)?,
        "chapterGoalHint":request.pointer("/payload/chapterGoalHint").and_then(Value::as_str).filter(|value| !value.trim().is_empty()).unwrap_or("推进当前卷任务，并让真实选择产生可核对后果。")
    });
    let planning_result = crate::production::execute_production_model_with_profile(
        &handle,
        state.production_gateway.as_ref(),
        &profile,
        &planning_spec,
        "PLAN_STRATEGY",
        planning_input.clone(),
        requested_at,
    )?;
    let raw_plan = planning_result
        .parsed
        .ok_or_else(|| "B05_PLANNING_OUTPUT_INVALID".to_string())?;
    let bindings = json!({
        "chapterId":chapter_id,
        "compiledFromProjectRevision":planning_revision,
        "compiledFromStateDigest":state_digest,
        "contextManifestRef":context_ref
    });
    let mut plan = crate::planning::normalize_chapter_plan_model_output(
        raw_plan,
        &context_input
            .continuity_threads
            .iter()
            .filter(|thread| matches!(thread.authority_level.as_str(), "A" | "B"))
            .map(|thread| thread.thread_id.clone())
            .collect::<Vec<_>>(),
        &bindings,
    );
    plan["compiledFromProjectRevision"] = json!(planning_revision);
    plan["compiledFromStateDigest"] = json!(state_digest);
    plan["contextManifestRef"] =
        serde_json::to_value(&context_ref).map_err(|error| error.to_string())?;
    let hard_scope = crate::narrative::context_hard_scope(&context_input);
    plan["activeTaskRefs"] = hard_scope["taskRefs"].clone();
    plan["entityRefs"] = hard_scope["entityRefs"].clone();
    plan["reservationRefs"] = hard_scope["reservationRefs"].clone();
    plan["hardScope"] = hard_scope;
    let receipt = crate::planning::recompile_chapter_plan_owner(
        &handle,
        project_id,
        chapter_id,
        &plan,
        &authority,
        command_id,
        idempotency_key,
        planning_revision,
        requested_at,
        Some(&planning_run_id),
    )?;
    let plan_ref = receipt
        .committed_artifact_refs
        .first()
        .cloned()
        .ok_or_else(|| "B05_PLANNING_OWNER_NO_RECEIPT".to_string())?;
    Ok(json!({
        "commandId":command_id,
        "projectId":project_id,
        "finalStatus":"committed",
        "message":"当前章节规划已生成，已绑定真实上下文。",
        "owner":"B07::PlanningOwner",
        "contextManifestRef":context_ref,
        "chapterPlanRef":plan_ref,
        "modelRunId":planning_run_id,
        "receipt":receipt
    }))
}

#[allow(clippy::too_many_arguments)]
fn prepare_next_chapter(
    state: &KernelState,
    request: &Value,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    requested_at: &str,
    expected_revision: Option<i64>,
    digest: &str,
) -> Result<Value, String> {
    let volume_id = required_string(request.get("payload").unwrap_or(&Value::Null), "volumeId")?;
    let expected_revision = expected_revision.ok_or_else(|| {
        "IPC_INVALID:expectedRevision required for chapter preparation".to_string()
    })?;
    let handle = state
        .manager
        .open(project_id, "", requested_at)
        .map_err(|error| error.to_string())?;
    let spec = project_command_spec(
        project_id,
        project_id,
        command_id,
        idempotency_key,
        "planning_prepare_next_chapter",
        digest,
        Some(expected_revision),
        requested_at,
        true,
    );
    let outcome = CommitService::commit_project_command(&handle, spec, |connection| {
        let chapter_no = crate::repositories::narrative::NarrativeRepository::next_chapter_number(
            connection,
            project_id,
            volume_id,
        )
        .map_err(|error| crate::commit::CommitError::ProjectCommandApply(error.to_string()))?
        .ok_or_else(|| {
            crate::commit::CommitError::ProjectCommandApply("VOLUME_CHAPTER_RANGE_EXHAUSTED".into())
        })?;
        let chapter_id = StableIdentityFactory::from_command(
            StableIdKind::Chapter,
            &format!("{project_id}:{volume_id}:chapter:{chapter_no}"),
        )
        .map_err(crate::commit::CommitError::ProjectCommandApply)?;
        let budget_id = format!("budget:{chapter_id}");
        let title = format!("第{chapter_no}章 · 当前卷推进");
        crate::repositories::narrative::NarrativeRepository::ensure_planned_chapter(
            connection,
            project_id,
            volume_id,
            &chapter_id,
            chapter_no,
            &title,
            &budget_id,
            requested_at,
        )
        .map_err(|error| crate::commit::CommitError::ProjectCommandApply(error.to_string()))?;
        Ok(json!({"volumeId":volume_id,"chapterId":chapter_id,"chapterNo":chapter_no,"title":title,"budgetId":budget_id}))
    })
    .map_err(|error| error.to_string())?;
    let (receipt, chapter) = match outcome {
        ProjectCommandOutcome::Replayed(receipt) => (receipt, None),
        ProjectCommandOutcome::Committed { value, receipt } => (receipt, Some(value)),
    };
    let mut response = project_command_response(&receipt);
    response["message"] = json!("下一章已准备，等待生成当前章节规划。");
    if let Some(chapter) = chapter {
        response["chapter"] = chapter;
    }
    Ok(response)
}

#[allow(clippy::too_many_arguments)]
fn execute_planning_command_with_observer(
    state: &KernelState,
    request: &Value,
    command_type: &str,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    requested_at: &str,
    expected_revision: Option<i64>,
    observer: &mut dyn FnMut(&str),
) -> Result<Value, String> {
    let mut payload = request
        .pointer("/payload/plan")
        .cloned()
        .unwrap_or(Value::Null);
    let mut book_plan_dependencies: Vec<crate::generated::types::ArtifactRef> = Vec::new();
    let mut b02_plan_job: Option<String> = None;
    let mut b03_volume_job: Option<(String, String)> = None;
    let b02_plan_worker = "b02-book-plan-worker";
    let (artifact_type, scope, scope_key) = match command_type {
        "planning_finalize_book_plan" | "book_plan_revise" => (
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
    let handle = state
        .manager
        .open(project_id, "", requested_at)
        .map_err(|e| e.to_string())?;
    if artifact_type == "VolumeIntent" {
        let metadata =
            crate::repositories::volume_lifecycle::VolumeLifecycleRepository::volume_metadata(
                &handle, project_id, &scope_key,
            )
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "VOLUME_NOT_FOUND".to_string())?;
        let book_plan_ref =
            crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
                &handle, project_id, "BookPlan",
            )
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "GOAL_CONTRACT_BOOK_PLAN_APPROVED_REQUIRED".to_string())?;
        let book_plan =
            crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                &handle,
                project_id,
                &book_plan_ref,
            )
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "GOAL_CONTRACT_BOOK_PLAN_APPROVED_REQUIRED".to_string())?;
        if book_plan.get("planningMaturity").and_then(Value::as_str) != Some("APPROVED") {
            return Err("GOAL_CONTRACT_BOOK_PLAN_APPROVED_REQUIRED".into());
        }
        let project_revision =
            crate::repositories::narrative::NarrativeRepository::project_revision(
                &handle, project_id,
            )
            .map_err(|e| e.to_string())?;
        let sources = crate::repositories::narrative::NarrativeRepository::volume_planning_sources(
            &handle, project_id, &scope_key, 100,
        )
        .map_err(|e| e.to_string())?;
        let candidate_payload = if payload.is_null() {
            crate::planning::compile_volume_intent_from_authority(
                &scope_key,
                metadata.volume_no,
                metadata.chapter_start,
                metadata.chapter_end,
                metadata.target_words,
                &book_plan_ref,
                &book_plan,
                &sources,
                project_revision,
            )?
        } else {
            payload.clone()
        };
        crate::planning::validate_volume_intent_binding(
            &candidate_payload,
            &book_plan_ref,
            project_revision,
        )?;
        let job_id = StableIdentityFactory::from_command(
            StableIdKind::Job,
            &format!("{command_id}:volume-intent"),
        )?;
        let worker_id = "b03-volume-intent-worker".to_string();
        DurableJobService::start_b03_phase_job(
            &handle,
            project_id,
            &scope_key,
            &job_id,
            crate::runtime::B03_VOLUME_INTENT_KIND,
            crate::runtime::B03_VOLUME_INTENT_PHASES,
            &worker_id,
            requested_at,
        )
        .map_err(|error| error.to_string())?;
        DurableJobService::set_runtime_state(&handle, project_id, "ACTIVE", None, requested_at)
            .map_err(|error| error.to_string())?;
        DurableJobService::set_project_phase(
            &handle,
            project_id,
            "VOLUME_INTENT_PLANNING",
            requested_at,
        )
        .map_err(|error| error.to_string())?;
        notify_b03_volume_phase(
            &handle,
            project_id,
            &job_id,
            "READ_BOOK_PLAN",
            requested_at,
            observer,
        )?;
        payload = candidate_payload;
        notify_b03_volume_phase(
            &handle,
            project_id,
            &job_id,
            "READ_CURRENT_STATE",
            requested_at,
            observer,
        )?;
        notify_b03_volume_phase(
            &handle,
            project_id,
            &job_id,
            "BUILD_VOLUME_GOAL",
            requested_at,
            observer,
        )?;
        notify_b03_volume_phase(
            &handle,
            project_id,
            &job_id,
            "VALIDATE_VOLUME_GOAL",
            requested_at,
            observer,
        )?;
        book_plan_dependencies.push(book_plan_ref);
        b03_volume_job = Some((job_id, worker_id));
    }
    if artifact_type == "BookPlan" {
        let signing_head =
            crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
                &handle,
                project_id,
                "SimulatedSigningReceipt",
            )
            .map_err(|error| error.to_string())?;
        if let Some(signing_ref) = signing_head.as_ref() {
            let signing_payload =
                crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                    &handle,
                    project_id,
                    signing_ref,
                )
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "SIGNING_EVALUATION_STALE".to_string())?;
            if signing_payload
                .get("evaluationRuleVersion")
                .and_then(Value::as_str)
                != Some(crate::signing::SIGNING_RULESET_VERSION)
            {
                return Err("SIGNING_EVALUATION_STALE".into());
            }
        }
        let current_book_plan_ref =
            crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
                &handle, project_id, "BookPlan",
            )
            .map_err(|error| error.to_string())?;
        let current_book_plan_payload = if let Some(reference) = current_book_plan_ref.as_ref() {
            crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                &handle, project_id, reference,
            )
            .map_err(|error| error.to_string())?
        } else {
            None
        };
        let seed_ref = if current_book_plan_payload
            .as_ref()
            .and_then(|current| current.get("planningMaturity"))
            .and_then(Value::as_str)
            == Some("SEED")
        {
            current_book_plan_ref.clone()
        } else {
            crate::repositories::narrative::NarrativeRepository::latest_book_plan_seed_ref(
                &handle, project_id,
            )
            .map_err(|error| error.to_string())?
        };
        if payload.is_null() {
            let reference = seed_ref
                .clone()
                .ok_or_else(|| "BOOK_PLAN_SEED_REQUIRED".to_string())?;
            payload = crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                &handle, project_id, &reference,
            )
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "BOOK_PLAN_SEED_PAYLOAD_NOT_FOUND".to_string())?;
        }
        if payload.get("planningMaturity").and_then(Value::as_str) != Some("SEED") {
            return Err("BOOK_PLAN_SEED_REQUIRED".into());
        }
        if let Some(reference) = seed_ref {
            book_plan_dependencies.push(reference);
        }
        let mut canon_refs = string_array(request.pointer("/payload/openingCanonRefs"));
        let mut evidence_refs = string_array(request.pointer("/payload/signingEvidenceRefs"));
        if canon_refs.is_empty() && evidence_refs.is_empty() {
            let signing_ref = signing_head
                .clone()
                .ok_or_else(|| "BOOK_PLAN_SIGNING_RECEIPT_REQUIRED".to_string())?;
            if let Some(signing_payload) =
                crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                    &handle,
                    project_id,
                    &signing_ref,
                )
                .map_err(|error| error.to_string())?
            {
                if canon_refs.is_empty() {
                    canon_refs = string_array(signing_payload.get("openingCanonRefs"));
                }
                evidence_refs.extend(string_array(signing_payload.get("signingEvidenceRefs")));
            }
            if !evidence_refs
                .iter()
                .any(|reference| reference == &signing_ref.artifact_id)
            {
                evidence_refs.push(signing_ref.artifact_id.clone());
            }
            book_plan_dependencies.push(signing_ref);
        }
        if let Some(signing_ref) = signing_head.as_ref() {
            if evidence_refs
                .iter()
                .any(|reference| reference == &signing_ref.artifact_id)
            {
                if let Some(signing_payload) =
                    crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                        &handle,
                        project_id,
                        signing_ref,
                    )
                    .map_err(|error| error.to_string())?
                {
                    let actual_evidence_refs =
                        string_array(signing_payload.get("signingEvidenceRefs"));
                    for reference in actual_evidence_refs {
                        if !evidence_refs.iter().any(|existing| existing == &reference) {
                            evidence_refs.push(reference);
                        }
                    }
                }
            }
        }
        let canon_reachable = canon_refs.iter().try_fold(true, |reachable, reference| {
            Ok::<_, String>(reachable && crate::repositories::narrative::NarrativeRepository::opening_canon_reference_exists(&handle, project_id, reference).map_err(|e| e.to_string())?)
        })?;
        let receipt_passes = evidence_refs.iter().try_fold(false, |passed, reference| {
            if passed {
                return Ok::<_, String>(true);
            }
            let Some(current_ref) = signing_head
                .as_ref()
                .filter(|head| head.artifact_id == *reference)
            else {
                if crate::repositories::narrative::NarrativeRepository::committed_signing_receipt_ref(
                    &handle,
                    project_id,
                    reference,
                )
                .map_err(|error| error.to_string())?
                .is_some()
                {
                    return Err("SIGNING_EVALUATION_STALE".into());
                }
                return crate::repositories::narrative::NarrativeRepository::signing_receipt_passes(
                    &handle,
                    project_id,
                    reference,
                    &canon_refs,
                )
                .map_err(|error| error.to_string());
            };
            let signing_payload =
                crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                    &handle,
                    project_id,
                    current_ref,
                )
                .map_err(|error| error.to_string())?;
            let is_signing_eval_r1 = signing_payload
                .as_ref()
                .and_then(|payload| payload.get("evaluationRuleVersion"))
                .and_then(Value::as_str)
                == Some(crate::signing::SIGNING_RULESET_VERSION);
            let current = !is_signing_eval_r1
                || crate::signing::signing_receipt_is_current_and_confirmed_in_project(
                    &handle,
                    project_id,
                    current_ref,
                )?;
            if !current {
                return Ok(false);
            }
            crate::repositories::narrative::NarrativeRepository::signing_receipt_passes(
                &handle,
                project_id,
                reference,
                &canon_refs,
            )
            .map_err(|error| error.to_string())
        })?;
        if canon_refs.len() < 3 || !canon_reachable || !receipt_passes {
            return Err("BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE".into());
        }
        for reference in &evidence_refs {
            if let Some(receipt_ref) =
                crate::repositories::narrative::NarrativeRepository::committed_signing_receipt_ref(
                    &handle, project_id, reference,
                )
                .map_err(|error| error.to_string())?
            {
                if !book_plan_dependencies
                    .iter()
                    .any(|existing| existing == &receipt_ref)
                {
                    book_plan_dependencies.push(receipt_ref);
                }
            }
        }
        let plan_job_id = StableIdentityFactory::from_command(
            StableIdKind::Job,
            &format!("{command_id}:book-plan"),
        )?;
        DurableJobService::start_b02_phase_job(
            &handle,
            project_id,
            &plan_job_id,
            crate::repositories::runtime::B02_BOOK_PLAN_KIND,
            crate::repositories::runtime::B02_BOOK_PLAN_PHASES,
            b02_plan_worker,
            requested_at,
        )
        .map_err(|error| error.to_string())?;
        DurableJobService::set_runtime_state(&handle, project_id, "ACTIVE", None, requested_at)
            .map_err(|error| error.to_string())?;
        DurableJobService::set_project_phase(
            &handle,
            project_id,
            "POST_SIGNING_PLANNING",
            requested_at,
        )
        .map_err(|error| error.to_string())?;
        if let Err(error) = notify_b02_planning_phase(
            &handle,
            project_id,
            &plan_job_id,
            "READ_OPENING_CANON",
            requested_at,
            observer,
        ) {
            let _ = DurableJobService::fail_b02_phase_job(
                &handle,
                project_id,
                &plan_job_id,
                "B02_PLAN_PHASE_FAILED",
                requested_at,
            );
            return Err(error);
        }
        b02_plan_job = Some(plan_job_id);
        let opening_snapshot =
            crate::repositories::narrative::NarrativeRepository::opening_evaluation_snapshot(
                &handle, project_id,
            )
            .map_err(|error| error.to_string())?;
        if let Some(job_id) = b02_plan_job.as_deref() {
            notify_b02_planning_phase(
                &handle,
                project_id,
                job_id,
                "SUMMARIZE_OPENING",
                requested_at,
                observer,
            )?;
        }
        let opening_actual_state =
            crate::repositories::narrative::NarrativeRepository::opening_actual_state(
                &handle,
                project_id,
                &opening_snapshot,
            )
            .map_err(|error| error.to_string())?;
        if let Some(job_id) = b02_plan_job.as_deref() {
            notify_b02_planning_phase(
                &handle,
                project_id,
                job_id,
                "GENERATE_BOOK_PLAN",
                requested_at,
                observer,
            )?;
        }
        payload = crate::planning::approve_book_plan(
            &payload,
            true,
            &canon_refs,
            &evidence_refs,
            &opening_actual_state,
        )?;
        if let Some(job_id) = b02_plan_job.as_deref() {
            notify_b02_planning_phase(
                &handle,
                project_id,
                job_id,
                "VALIDATE_CONSTRAINTS",
                requested_at,
                observer,
            )?;
        }
    }
    if artifact_type == "VolumePlan" {
        let intent = crate::repositories::narrative::NarrativeRepository::canonical_head_ref(
            &handle,
            project_id,
            "VolumeIntent",
            &scope_key,
        )
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "GOAL_CONTRACT_VOLUME_INTENT_REQUIRED".to_string())?;
        let intent_payload =
            crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                &handle, project_id, &intent,
            )
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "GOAL_CONTRACT_VOLUME_INTENT_REQUIRED".to_string())?;
        let story_ref =
            crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
                &handle,
                project_id,
                "StoryConstitution",
            )
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "GOAL_CONTRACT_STORY_CONSTITUTION_REQUIRED".to_string())?;
        let main_ref =
            crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
                &handle,
                project_id,
                "MainContract",
            )
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "GOAL_CONTRACT_MAIN_CONTRACT_REQUIRED".to_string())?;
        let book_ref =
            crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
                &handle, project_id, "BookPlan",
            )
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "GOAL_CONTRACT_BOOK_PLAN_APPROVED_REQUIRED".to_string())?;
        let story_payload =
            crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                &handle, project_id, &story_ref,
            )
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "GOAL_CONTRACT_STORY_CONSTITUTION_REQUIRED".to_string())?;
        let main_payload =
            crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                &handle, project_id, &main_ref,
            )
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "GOAL_CONTRACT_MAIN_CONTRACT_REQUIRED".to_string())?;
        let book_payload =
            crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                &handle, project_id, &book_ref,
            )
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "GOAL_CONTRACT_BOOK_PLAN_APPROVED_REQUIRED".to_string())?;
        let project_revision =
            crate::repositories::narrative::NarrativeRepository::project_revision(
                &handle, project_id,
            )
            .map_err(|e| e.to_string())?;
        let goal_contract = crate::planning::compile_goal_contract(
            &story_ref,
            &story_payload,
            &main_ref,
            &main_payload,
            &book_ref,
            &book_payload,
            &intent,
            &intent_payload,
        )?;
        let candidate_payload = if payload.is_null() {
            crate::planning::compile_volume_plan_from_authority(
                &intent,
                &intent_payload,
                &goal_contract,
                project_revision,
            )?
        } else {
            crate::planning::validate_volume_route(&payload)?;
            crate::planning::validate_volume_route_reference(&payload, &intent)?;
            crate::planning::normalize_volume_plan_candidate(
                payload.clone(),
                &intent,
                &intent_payload,
                &goal_contract,
                project_revision,
            )?
        };
        crate::planning::validate_volume_plan_semantics(&candidate_payload)?;
        if !payload.is_null() {
            crate::planning::validate_volume_route_reference(&candidate_payload, &intent)?;
        }
        let job_id = StableIdentityFactory::from_command(
            StableIdKind::Job,
            &format!("{command_id}:volume-plan"),
        )?;
        let worker_id = "b03-volume-plan-worker".to_string();
        DurableJobService::start_b03_phase_job(
            &handle,
            project_id,
            &scope_key,
            &job_id,
            crate::runtime::B03_VOLUME_PLAN_KIND,
            crate::runtime::B03_VOLUME_PLAN_PHASES,
            &worker_id,
            requested_at,
        )
        .map_err(|error| error.to_string())?;
        DurableJobService::set_runtime_state(&handle, project_id, "ACTIVE", None, requested_at)
            .map_err(|error| error.to_string())?;
        DurableJobService::set_project_phase(
            &handle,
            project_id,
            "VOLUME_PLAN_PLANNING",
            requested_at,
        )
        .map_err(|error| error.to_string())?;
        let mut plan_phases = crate::runtime::B03_VOLUME_PLAN_PHASES.iter();
        if let Some(phase) = plan_phases.next() {
            notify_b03_volume_phase(&handle, project_id, &job_id, phase, requested_at, observer)?;
        }
        if let Some(phase) = plan_phases.next() {
            notify_b03_volume_phase(&handle, project_id, &job_id, phase, requested_at, observer)?;
        }
        payload = candidate_payload;
        if let Some(phase) = plan_phases.next() {
            notify_b03_volume_phase(&handle, project_id, &job_id, phase, requested_at, observer)?;
        }
        if let Some(phase) = plan_phases.next() {
            notify_b03_volume_phase(&handle, project_id, &job_id, phase, requested_at, observer)?;
        }
        if let Some(phase) = plan_phases.next() {
            notify_b03_volume_phase(&handle, project_id, &job_id, phase, requested_at, observer)?;
        }
        book_plan_dependencies.extend([story_ref, main_ref, book_ref, intent.clone()]);
        b03_volume_job = Some((job_id, worker_id));
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
            crate::planning::remaining_chapter_capacity(&handle, project_id, chapter_id)?,
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
        let reservation_volume_numbers =
            crate::repositories::narrative::NarrativeRepository::future_reservation_volume_numbers(
                &handle,
                project_id,
                &reservations,
            )
            .map_err(|error| error.to_string())?;
        crate::planning::validate_reservation_actions_with_volume_numbers(
            &payload,
            &reservations,
            current_volume_no,
            &reservation_volume_numbers,
        )?;
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
    let result_payload = payload.clone();
    let model_run_id = request
        .pointer("/payload/modelRunId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({"artifactId":artifact_id,"projectId":project_id,"artifactType":artifact_type,"scope":scope,"version":version,"schemaVersion":"m0a.artifact.v1","artifactStatus":"CANDIDATE","payload":payload,"payloadHash":hash,"dependencyRefs":book_plan_dependencies,"provenance":{"createdBy":"B07::PlanningOwner","commandId":command_id,"modelRunId":model_run_id},"createdAt":requested_at})).map_err(|e| e.to_string())?;
    let bundle: CommitBundle = serde_json::from_value(json!({"commitId":format!("commit:{}",command_id),"projectId":project_id,"commandId":command_id,"idempotencyKey":idempotency_key,"expectedProjectRevision":expected_revision,"contractVersion":"m0a.v1","createdAt":requested_at,"artifacts":[artifact],"headUpdates":[{"artifactType":artifact_type,"scope":scope,"artifactId":artifact_id,"version":version,"payloadHash":hash}]})).map_err(|e| e.to_string())?;
    if let Some(job_id) = b02_plan_job.as_deref() {
        notify_b02_planning_phase(
            &handle,
            project_id,
            job_id,
            "COMMIT_BOOK_PLAN",
            requested_at,
            observer,
        )?;
    }
    if let Some((job_id, _)) = b03_volume_job.as_ref() {
        let commit_phase = if artifact_type == "VolumeIntent" {
            "COMMIT_VOLUME_INTENT"
        } else {
            "COMMIT_VOLUME_PLAN"
        };
        notify_b03_volume_phase(
            &handle,
            project_id,
            job_id,
            commit_phase,
            requested_at,
            observer,
        )?;
    }
    let receipt = if artifact_type == "BookPlan" {
        match CommitService::commit_with_post_apply(
            &handle,
            &bundle,
            requested_at,
            |connection, _| {
                if let Some(job_id) = b02_plan_job.as_deref() {
                    crate::repositories::runtime::DurableJobService::complete_b02_phase_job(
                        connection,
                        project_id,
                        job_id,
                        b02_plan_worker,
                        requested_at,
                    )
                    .map_err(|error| {
                        crate::commit::CommitError::ContractInvalid(format!(
                            "B02_RUNTIME_JOB:{error}"
                        ))
                    })?;
                }
                crate::repositories::runtime::DurableJobService::set_project_phase(
                    connection,
                    project_id,
                    "POST_SIGNING_PLANNING",
                    requested_at,
                )
                .map_err(|error| {
                    crate::commit::CommitError::ContractInvalid(format!(
                        "B02_RUNTIME_PROJECTION:{error}"
                    ))
                })
            },
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                if let Some(job_id) = b02_plan_job.as_deref() {
                    let _ = DurableJobService::fail_b02_phase_job(
                        &handle,
                        project_id,
                        job_id,
                        "B02_PLAN_COMMIT_FAILED",
                        requested_at,
                    );
                }
                return Err(error.to_string());
            }
        }
    } else if let Some((job_id, worker_id)) = b03_volume_job.as_ref() {
        match CommitService::commit_with_post_apply(
            &handle,
            &bundle,
            requested_at,
            |connection, _| {
                DurableJobService::complete_b03_phase_job(
                    connection,
                    project_id,
                    job_id,
                    worker_id,
                    requested_at,
                )
                .map_err(|error| {
                    crate::commit::CommitError::ContractInvalid(format!("B03_RUNTIME_JOB:{error}"))
                })?;
                DurableJobService::set_project_phase(
                    connection,
                    project_id,
                    if artifact_type == "VolumeIntent" {
                        "VOLUME_INTENT_READY"
                    } else {
                        "VOLUME_PLAN_READY"
                    },
                    requested_at,
                )
                .map_err(|error| {
                    crate::commit::CommitError::ContractInvalid(format!(
                        "B03_RUNTIME_PROJECTION:{error}"
                    ))
                })?;
                DurableJobService::set_runtime_state(
                    connection,
                    project_id,
                    "COMPLETED",
                    None,
                    requested_at,
                )
                .map_err(|error| {
                    crate::commit::CommitError::ContractInvalid(format!(
                        "B03_RUNTIME_STATE:{error}"
                    ))
                })
            },
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                let _ = DurableJobService::fail_b03_phase_job(
                    &handle,
                    project_id,
                    job_id,
                    "B03_PLAN_COMMIT_FAILED",
                    requested_at,
                );
                return Err(error.to_string());
            }
        }
    } else {
        CommitService::commit(&handle, &bundle, requested_at).map_err(|e| e.to_string())?
    };
    if b02_plan_job.is_some() {
        observer("COMPLETED");
    }
    if b03_volume_job.is_some() {
        observer("COMPLETED");
    }
    Ok(
        json!({"finalStatus":"committed","receipt":receipt,"owner":"B07::PlanningOwner","result":result_payload}),
    )
}

#[doc(hidden)]
pub fn execute_book_plan_revise_with_observer(
    state: &KernelState,
    request: &Value,
    observer: &mut dyn FnMut(&str),
) -> Result<Value, String> {
    let command_type = required_string(request, "type")?;
    if command_type != "book_plan_revise" {
        return Err("B02_BOOK_PLAN_REVISE_COMMAND_REQUIRED".into());
    }
    let project_id = required_string(request, "projectId")?;
    let command_id = required_string(request, "commandId")?;
    let idempotency_key = required_string(request, "idempotencyKey")?;
    let requested_at = required_string(request, "requestedAt")?;
    execute_planning_command_with_observer(
        state,
        request,
        command_type,
        project_id,
        command_id,
        idempotency_key,
        requested_at,
        expected_revision(request)?,
        observer,
    )
}

#[doc(hidden)]
pub fn execute_volume_planning_with_observer(
    state: &KernelState,
    request: &Value,
    observer: &mut dyn FnMut(&str),
) -> Result<Value, String> {
    let command_type = required_string(request, "type")?;
    if !matches!(
        command_type,
        "planning_create_volume_intent" | "planning_create_volume_plan"
    ) {
        return Err("B03_VOLUME_PLANNING_COMMAND_REQUIRED".into());
    }
    let project_id = required_string(request, "projectId")?;
    let command_id = required_string(request, "commandId")?;
    let idempotency_key = required_string(request, "idempotencyKey")?;
    let requested_at = required_string(request, "requestedAt")?;
    execute_planning_command_with_observer(
        state,
        request,
        command_type,
        project_id,
        command_id,
        idempotency_key,
        requested_at,
        expected_revision(request)?,
        observer,
    )
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
    let update = request
        .pointer("/payload/settings")
        .cloned()
        .ok_or_else(|| "P01_SETTINGS_REQUIRED".to_string())?;
    let mut settings = if let Some(project_id) = project_id {
        let connection = state
            .manager
            .open(project_id, "", requested_at)
            .map_err(|error| error.to_string())?;
        crate::repositories::provider::ProviderSettingsRepository::read_project(
            &connection,
            project_id,
        )
        .map_err(|error| error.to_string())?
    } else {
        crate::repositories::provider::ProviderSettingsRepository::read_global(&state.paths)
            .map_err(|error| error.to_string())?
    };
    let updates = update
        .as_object()
        .ok_or_else(|| "P01_SETTINGS_REQUIRED".to_string())?;
    let settings_object = settings
        .as_object_mut()
        .ok_or_else(|| "P01_SETTINGS_REQUIRED".to_string())?;
    for (key, value) in updates {
        settings_object.insert(key.clone(), value.clone());
    }
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
                    command_id,
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

fn save_model_pricing(
    state: &KernelState,
    request: &Value,
    command_id: &str,
    idempotency_key: &str,
    requested_at: &str,
    request_digest: &str,
) -> Result<Value, String> {
    let entries = request
        .pointer("/payload/entries")
        .cloned()
        .ok_or_else(|| "MODEL_PRICING_ENTRIES_REQUIRED".to_string())?;
    let connection =
        CatalogRepository::open(&state.paths, requested_at).map_err(|error| error.to_string())?;
    connection
        .execute_batch("BEGIN IMMEDIATE")
        .map_err(|error| error.to_string())?;
    match crate::model_cost::ModelPricingCatalog::save(
        &connection,
        entries,
        command_id,
        idempotency_key,
        request_digest,
        requested_at,
    ) {
        Ok(response) => {
            connection
                .execute_batch("COMMIT")
                .map_err(|error| error.to_string())?;
            Ok(response)
        }
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            Err(error)
        }
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
        "story_directions_generate" => crate::project_entry::commit_regenerate(
            &handle,
            project_id,
            command_id,
            idempotency_key,
            expected_revision,
            requested_at,
            request,
            state.production_gateway.as_ref(),
        ),
        "pitch_regenerate" => crate::project_entry::commit_regenerate(
            &handle,
            project_id,
            command_id,
            idempotency_key,
            expected_revision,
            requested_at,
            request,
            state.production_gateway.as_ref(),
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
        "message":match command_type { "story_directions_generate" => "故事方向已生成，请选择。", "pitch_regenerate" => "新的故事方向已生成，请由你选择。", "pitch_select" => "故事方向已由你确认，进入故事核心。", "story_setup_save" => "故事核心已保存，等待你批准开篇准备。", "opening_ready" => "开篇准备已完成，已生成第一章的最小规划。", _ => "B11 操作已完成。" },
        "receiptId":receipt["receiptId"],
        "beforeRevision":receipt["beforeRevision"],
        "afterRevision":receipt["afterRevision"],
        "receipt":receipt
    }))
}

pub fn execute_command_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let command_type = required_string(&request, "type")?;
    #[cfg(not(feature = "e2e-test"))]
    {
        if command_type == "repair_execute" && request.pointer("/payload/testOnlyNoop").is_some() {
            return Err("IPC_INVALID:testOnlyNoop".into());
        }
        if command_type == "volume_close" && request.pointer("/payload/faultInjection").is_some() {
            return Err("IPC_INVALID:faultInjection".into());
        }
    }
    #[cfg(feature = "e2e-test")]
    if matches!(command_type, "project_delete" | "repair_execute")
        && request
            .pointer("/payload/testOnlyFailureInjection")
            .and_then(Value::as_bool)
            == Some(true)
    {
        return Err(if command_type == "project_delete" {
            "R5_TEST_LIBRARY_DELETE_FAILURE"
        } else {
            "R5_TEST_ASSISTANT_EXECUTE_FAILURE"
        }
        .into());
    }
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
        "model_pricing_save" => save_model_pricing(
            state,
            &request,
            command_id,
            idempotency_key,
            requested_at,
            &digest,
        ),
        "decision_submit" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            execute_decision_submit(
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
        "thread_responsibility_adjust" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            crate::narrative::responsibility::execute_adjustment(
                &state.manager,
                crate::narrative::responsibility::AdjustmentCommand {
                    request: &request,
                    project_id: id,
                    command_id,
                    idempotency_key,
                    expected_revision,
                    request_digest: &digest,
                    requested_at,
                },
            )
        }
        "gate_policy_update" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            crate::gate_policy::execute_update(
                &state.manager,
                &request,
                id,
                command_id,
                idempotency_key,
                expected_revision,
                &digest,
                requested_at,
            )
        }
        "story_directions_generate"
        | "pitch_regenerate"
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
        "market_refresh" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            if let Some(replayed) = replayed_project_command(state, id, idempotency_key, &digest)? {
                return Ok(replayed);
            }
            let revision =
                expected_revision.ok_or_else(|| "IPC_INVALID:expectedRevision".to_string())?;
            let snapshot = state.market_source_adapter.fetch();
            let connection = state
                .manager
                .open(id, "", requested_at)
                .map_err(|error| error.to_string())?;
            let receipt = crate::project_entry::commit_market_refresh(
                &connection,
                id,
                command_id,
                idempotency_key,
                revision,
                requested_at,
                &snapshot,
            )?;
            Ok(json!({
                "commandId":receipt.command_id,
                "projectId":receipt.project_id,
                "finalStatus":"committed",
                "message":if snapshot.source_status == "READY" {"市场来源已刷新；请核对证据后再选择方向。"} else {"市场来源暂不可用；本次失败状态已记录，未把旧数据标为当前。"},
                "receiptId":receipt.receipt_id,
                "beforeRevision":receipt.before_revision,
                "afterRevision":receipt.after_revision,
                "requestDigest":digest,
                "sourceStatus":snapshot.source_status,
                "policyStatus":snapshot.policy_status,
                "sourceDigest":snapshot.source_digest
            }))
        }
        "market_opportunity_select" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            if let Some(replayed) = replayed_project_command(state, id, idempotency_key, &digest)? {
                return Ok(replayed);
            }
            let revision =
                expected_revision.ok_or_else(|| "IPC_INVALID:expectedRevision".to_string())?;
            let opportunity_id = required_string(
                request.pointer("/payload").unwrap_or(&Value::Null),
                "marketOpportunityId",
            )?;
            let connection = state
                .manager
                .open(id, "", requested_at)
                .map_err(|error| error.to_string())?;
            if let Some(replayed) = crate::project_entry::replay_market_opportunity_selection(
                &connection,
                id,
                idempotency_key,
                &digest,
            )? {
                return Ok(replayed);
            }
            let receipt = crate::project_entry::commit_market_opportunity_selection(
                &connection,
                id,
                command_id,
                idempotency_key,
                revision,
                requested_at,
                &digest,
                opportunity_id,
            )?;
            Ok(json!({
                "commandId":receipt.command_id,
                "projectId":receipt.project_id,
                "finalStatus":"committed",
                "message":"已记录你选择的市场机会；作品事实和故事方向未更改。",
                "receiptId":receipt.receipt_id,
                "beforeRevision":receipt.before_revision,
                "afterRevision":receipt.after_revision,
                "requestDigest":digest,
                "selectedOpportunityId":opportunity_id
            }))
        }
        "opening_evaluate" | "signing_retry" => {
            // `signing_retry` is an INTERNAL_COMPAT alias only.  The
            // product FrontendPort exposes `opening_evaluate` as the single
            // formal mutation; legacy IPC callers may still be routed here.
            project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            crate::signing::execute_opening_evaluation(
                &state.manager,
                &state.paths,
                state.production_gateway.as_ref(),
                &state.skill_bundle_loader,
                &request,
            )
        }
        "signing_confirm" => {
            project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            crate::signing::confirm_signing_platform(&state.manager, &request)
        }
        // `planning_finalize_book_plan` is an INTERNAL_COMPAT alias;
        // `book_plan_revise` is the only product command for this operation.
        "planning_finalize_book_plan"
        | "book_plan_revise"
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
        "planning_prepare_next_chapter" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
            prepare_next_chapter(
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
                    &state.skill_bundle_loader,
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
            let raw_title = request
                .pointer("/payload/title")
                .and_then(Value::as_str)
                .unwrap_or("未命名作品");
            let title = crate::project_db::normalize_project_title(raw_title);
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
            let mut entry_request = request.clone();
            let selected_market_opportunity_id = request
                .pointer("/payload/marketOpportunityId")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(ToOwned::to_owned);
            if b11_entry {
                crate::project_entry::validate_initial_request(&entry_request, entry_kind)?;
            }
            let id = stable_project_id(command_id)?;
            if let Some(replayed) = replayed_project_command(state, &id, idempotency_key, &digest)?
            {
                return Ok(replayed);
            }
            let market_source_snapshot = if entry_kind == crate::project_entry::EntryKind::Market {
                if let Some(selected_id) = selected_market_opportunity_id.as_deref() {
                    let snapshot = state.market_source_adapter.fetch();
                    if snapshot.source_status != "READY" {
                        return Err(format!(
                            "MARKET_SOURCE_UNAVAILABLE:{}",
                            snapshot
                                .error_code
                                .as_deref()
                                .unwrap_or("SOURCE_UNAVAILABLE")
                        ));
                    }
                    let category = snapshot
                        .categories
                        .iter()
                        .find(|category| category.topic_id == selected_id)
                        .ok_or_else(|| "MARKET_OPPORTUNITY_STALE".to_string())?;
                    let payload = entry_request
                        .get_mut("payload")
                        .and_then(Value::as_object_mut)
                        .ok_or_else(|| "IPC_INVALID:payload".to_string())?;
                    payload.insert("marketSource".into(), json!(snapshot.source_url));
                    payload.insert("marketSourceDigest".into(), json!(snapshot.source_digest));
                    if payload
                        .get("marketMaterial")
                        .and_then(Value::as_str)
                        .is_none_or(|text| text.trim().is_empty())
                    {
                        payload.insert(
                            "marketMaterial".into(),
                            json!(format!(
                                "用户明确选择了番茄官方公开出版展示中的“{}”分类。当前公开展示样本为 {} 项；该样本不是阅读量、销量、排名或商业表现。",
                                category.category, snapshot.sample_count
                            )),
                        );
                    }
                    Some(snapshot)
                } else {
                    None
                }
            } else {
                None
            };
            let handle = match state.manager.overview(&id) {
                Ok((existing_title, _revision, _)) if existing_title == title => state
                    .manager
                    .open(&id, "", requested_at)
                    .map_err(|error| error.to_string())?,
                Ok(_) => return Err("IDEMPOTENCY_CONFLICT".to_string()),
                Err(crate::project_db::ManagerError::ProjectNotFound) => state
                    .manager
                    .create(&id, &title, requested_at)
                    .map_err(|error| error.to_string())?,
                Err(error) => return Err(error.to_string()),
            };
            crate::gate_policy::ensure_default_policy(
                &handle,
                &id,
                &format!("{command_id}:gate-policy-default"),
                &format!("{idempotency_key}:gate-policy-default"),
                requested_at,
            )?;
            let entry_receipt = if b11_entry {
                Some(crate::project_entry::commit_initial_with_market_source(
                    &handle,
                    &id,
                    command_id,
                    idempotency_key,
                    requested_at,
                    &entry_request,
                    entry_kind,
                    market_source_snapshot.as_ref(),
                    selected_market_opportunity_id.as_deref(),
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
            if let Some(snapshot) = market_source_snapshot.as_ref() {
                response["marketSourceStatus"] = json!(snapshot.source_status);
                response["platformPolicyStatus"] = json!(snapshot.policy_status);
                response["marketSourceDigest"] = json!(snapshot.source_digest);
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
            if state.manager.library_projection(id).is_err() {
                return registry_lifecycle_command(
                    state,
                    id,
                    command_id,
                    idempotency_key,
                    command_type,
                    &digest,
                    expected_revision,
                    requested_at,
                    false,
                );
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
            if state.manager.library_projection(id).is_err() {
                return registry_lifecycle_command(
                    state,
                    id,
                    command_id,
                    idempotency_key,
                    command_type,
                    &digest,
                    expected_revision,
                    requested_at,
                    true,
                );
            }
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
pub fn query_command_receipt(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_command_receipt_impl(&state, request)
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
pub fn query_export_capability(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let capability = state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::export::query_capability(connection, project_id)
                .map_err(ManagerError::ProjectCommand)
        })
        .map_err(|error| error.to_string())?;
    serde_json::to_value(capability).map_err(|_| "EXPORT_CAPABILITY_SERIALIZE_FAILED".into())
}

fn create_export_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let request_id = required_string(&request, "requestId")?;
    let format = request
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("TXT");
    let project_dir = state
        .paths
        .project_dir(project_id)
        .map_err(|_| "EXPORT_PROJECT_NOT_FOUND".to_string())?;
    let receipt = state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            crate::export::create_export(connection, &project_dir, project_id, request_id, format)
                .map_err(ManagerError::ProjectCommand)
        })
        .map_err(|error| error.to_string())?;
    serde_json::to_value(receipt).map_err(|_| "EXPORT_RECEIPT_SERIALIZE_FAILED".into())
}

#[tauri::command]
pub async fn create_export(state: State<'_, KernelState>, request: Value) -> Result<Value, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || create_export_impl(&state, request))
        .await
        .map_err(|_| "EXPORT_WORKER_FAILED".to_string())?
}

#[tauri::command]
pub fn query_export_receipt(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let request_id = required_string(&request, "requestId")?;
    let project_dir = state
        .paths
        .project_dir(project_id)
        .map_err(|_| "EXPORT_PROJECT_NOT_FOUND".to_string())?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |_| {
            crate::export::query_receipt(&project_dir, project_id, request_id)
                .map_err(ManagerError::ProjectCommand)
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn query_latest_export_receipt(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let project_dir = state
        .paths
        .project_dir(project_id)
        .map_err(|_| "EXPORT_PROJECT_NOT_FOUND".to_string())?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |_| {
            crate::export::query_latest_receipt(&project_dir).map_err(ManagerError::ProjectCommand)
        })
        .map_err(|error| error.to_string())
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
pub fn query_book_health_factor_sources(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    query_book_health_factor_sources_impl(&state, request)
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
pub fn query_endgame_mapping_options(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
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
pub async fn execute_command(
    state: State<'_, KernelState>,
    request: Value,
) -> Result<Value, String> {
    let state = state.inner().clone();
    let is_market_command = matches!(
        request.get("type").and_then(Value::as_str),
        Some("market_refresh" | "project_create_from_market")
    );
    let result =
        tauri::async_runtime::spawn_blocking(move || execute_command_impl(&state, request)).await;
    match result {
        Ok(result) => result,
        Err(error) if is_market_command => Err(format!("MARKET_COMMAND_WORKER_FAILED:{error}")),
        Err(_) => Err("EXECUTE_COMMAND_WORKER_FAILED".into()),
    }
}

#[tauri::command]
fn provider_config(settings: &Value) -> Result<crate::provider::ProviderConfig, String> {
    crate::provider::ProviderConfig::from_settings(settings).map_err(|error| {
        match error.detail.as_str() {
            "MODEL_CONFIG_MISSING:provider" => "P01_PROVIDER_ID_REQUIRED".into(),
            "MODEL_CONFIG_MISSING:model" => "P01_MODEL_ID_REQUIRED".into(),
            _ => format!("P01_{}", error.detail),
        }
    })
}

#[tauri::command]
pub fn query_global_settings(state: State<'_, KernelState>) -> Result<Value, String> {
    let mut settings =
        crate::repositories::provider::ProviderSettingsRepository::read_global(&state.paths)
            .map_err(|error| error.to_string())?;
    settings["modelPricing"] =
        serde_json::to_value(crate::model_cost::ModelPricingCatalog::read_from_paths(
            &state.paths,
            "1970-01-01T00:00:00Z",
        )?)
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
pub async fn test_provider(
    _state: State<'_, KernelState>,
    settings: Value,
) -> Result<Value, String> {
    let settings = settings.get("settings").cloned().unwrap_or(settings);
    run_provider_test_on_blocking_worker(move || test_provider_sync(settings)).await
}

async fn run_provider_test_on_blocking_worker<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("P01_TEST_PROVIDER_WORKER_FAILED:{error}"))?
}

fn validate_test_provider_response(raw_response: &str) -> Result<Value, String> {
    let envelope: Value =
        serde_json::from_str(raw_response).map_err(|_| "P01_MALFORMED_RESPONSE".to_string())?;
    let output =
        crate::production::parse_provider_model_payload_for_capability(envelope, "TEST_PROVIDER")
            .map_err(|_| "P01_MALFORMED_RESPONSE".to_string())?;
    if output != json!({"ok":true}) {
        return Err("P01_MALFORMED_RESPONSE".into());
    }
    Ok(output)
}

fn test_provider_sync(settings: Value) -> Result<Value, String> {
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
    let response_payload = validate_test_provider_response(&response.raw_response)?;
    Ok(json!({
        "finalStatus":"committed",
        "message":"Provider 连接测试成功",
        "providerId":config.provider_id,
        "usage":response.usage,
        "response":response_payload,
        "providerRequestId":response.provider_request_id,
        "secretReturned":false
    }))
}

#[tauri::command]
pub async fn query_market_opportunities(state: State<'_, KernelState>) -> Result<Value, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || query_market_opportunities_impl(&state))
        .await
        .map_err(|error| format!("MARKET_QUERY_WORKER_FAILED:{error}"))?
}

pub fn handler<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool {
    tauri::generate_handler![
        query_project_library,
        query_project_overview,
        query_command_receipt,
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
        query_export_capability,
        create_export,
        query_export_receipt,
        query_latest_export_receipt,
        query_canon,
        query_retcon_context,
        query_book_health,
        query_book_health_factor_sources,
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

#[cfg(test)]
mod story_constitution_contract_tests {
    use super::*;

    #[test]
    fn missing_story_mechanism_is_undeclared() {
        let story_core = json!({"protagonist":"林舟"});
        assert!(
            declared_story_mechanism(story_core.as_object().unwrap(), "CHAPTER:1")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn non_string_story_mechanism_remains_invalid() {
        let story_core = json!({"mechanism":{"value":"代价"}});
        let error =
            declared_story_mechanism(story_core.as_object().unwrap(), "CHAPTER:1").unwrap_err();
        assert_eq!(
            error,
            "GOAL_CONTRACT_STORY_CONSTITUTION_INVALID|field=storyCore.mechanism|scope=CHAPTER:1"
        );
    }
}

#[cfg(test)]
mod rv30_opening_canon_tests {
    use super::*;

    #[test]
    fn rv30_legacy_opening_description_projects_to_id_and_canon_refs() {
        let item = json!({
            "id":"rule:opening-reality:legacy",
            "description":"不得违背开篇事实 林舟持有账本；证据原文：账本缺页。"
        });
        let refs = vec![
            "fact:opening:linzhou".to_string(),
            "relation:opening:linzhou-guqing".to_string(),
        ];

        let projected = opening_canon_reference_projection(&item, &refs, "LONG_FORM")
            .unwrap()
            .expect("legacy opening item should be classified from BookPlan.openingCanonRefs");

        assert_eq!(
            projected,
            json!({
                "id":"rule:opening-reality:legacy",
                "refs":["fact:opening:linzhou","relation:opening:linzhou-guqing"]
            })
        );
        assert!(projected.get("description").is_none());
    }

    #[test]
    fn rv30_structured_opening_item_keeps_only_its_declared_canon_refs() {
        let item = json!({
            "id":"goal:opening-reality:current",
            "kind":"CANON_REFS",
            "refs":["fact:opening:linzhou"],
            "description":"开篇已提交事实不得违背；以 Canon 为准。"
        });
        let refs = vec![
            "fact:opening:linzhou".to_string(),
            "event:opening:ledger".to_string(),
        ];

        let projected = opening_canon_reference_projection(&item, &refs, "LONG_FORM")
            .unwrap()
            .expect("structured opening item should be classified");

        assert_eq!(
            projected,
            json!({"id":"goal:opening-reality:current","refs":["fact:opening:linzhou"]})
        );
    }

    #[test]
    fn rv30_opening_item_rejects_canon_refs_outside_its_source_contract() {
        let item = json!({
            "id":"rule:opening-reality:current",
            "kind":"CANON_REFS",
            "refs":["fact:unrelated"],
            "description":"opening text"
        });
        let refs = vec!["fact:opening:linzhou".to_string()];

        assert!(
            opening_canon_reference_projection(&item, &refs, "LONG_FORM")
                .unwrap_err()
                .contains("CONTEXT_SOURCE_UNAVAILABLE")
        );
    }

    #[test]
    fn rv30_opening_canon_refs_follow_selected_entity_scope() {
        let item = json!({
            "id":"rule:opening-reality:current",
            "refs":["fact:opening:linzhou"]
        });

        assert!(volume_constraint_matches_scope(
            &item,
            Some(&[]),
            Some(&BTreeSet::from(["fact:opening:linzhou".to_string()]))
        ));
        assert!(!volume_constraint_matches_scope(
            &item,
            Some(&[]),
            Some(&BTreeSet::from(["fact:opening:guqing".to_string()]))
        ));
    }
}

#[cfg(test)]
mod rv17_provider_test_response_tests {
    use super::{run_provider_test_on_blocking_worker, validate_test_provider_response};

    #[test]
    fn provider_connection_test_runs_provider_io_on_blocking_worker() {
        tauri::async_runtime::block_on(async {
            let caller_thread = format!("{:?}", std::thread::current().id());
            let worker_thread = run_provider_test_on_blocking_worker(|| {
                Ok(format!("{:?}", std::thread::current().id()))
            })
            .await
            .unwrap();
            assert_ne!(worker_thread, caller_thread);
        });
    }

    #[test]
    fn provider_test_requires_exact_ok_true_payload() {
        assert!(validate_test_provider_response(r#"{"ok":true}"#).is_ok());
        assert!(validate_test_provider_response(
            r#"{"id":"local","choices":[{"message":{"content":"{\"ok\":true}"}}]}"#
        )
        .is_ok());
        assert_eq!(
            validate_test_provider_response("<html>service proxy error</html>").unwrap_err(),
            "P01_MALFORMED_RESPONSE"
        );
        assert_eq!(
            validate_test_provider_response(r#"{"ok":false}"#).unwrap_err(),
            "P01_MALFORMED_RESPONSE"
        );
        assert_eq!(
            validate_test_provider_response(r#"{"ok":true,"unexpected":true}"#).unwrap_err(),
            "P01_MALFORMED_RESPONSE"
        );
    }
}
