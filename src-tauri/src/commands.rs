use crate::catalog::CatalogRepository;
use crate::generated::types::ProjectCommandReceipt;
use crate::harness::{
    diagnose_knowledge_event, grounded_finding, plan_support, registry_digest_map,
    resolve_project_scope, BoundedCapabilityExecutor, CapabilityCallEnvelope, CapabilityRegistry,
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
use crate::repositories::ops::{chapter_page, single_chapter, volume_page, SearchService};
use crate::runtime::{pending_decision_count, DurableJobService, JobSpec};
use crate::stable_id::{StableIdKind, StableIdentityFactory};
use serde_json::{json, Value};
use std::collections::BTreeSet;
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
        facts_truncated || knowledge_truncated,
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

fn context_input_from_authority(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
    revision: i64,
) -> Result<crate::narrative::ContextCompilerInput, String> {
    let state = narrative_state_from_request(connection, request, project_id)?;
    let threads = continuity_from_request(connection, request, project_id)?;
    let goal_contract_refs =
        crate::repositories::narrative::NarrativeRepository::goal_contract_refs(
            connection, project_id, 100,
        )
        .map_err(|e| e.to_string())?;
    let (task_states, task_truncated) =
        crate::repositories::narrative::NarrativeRepository::task_states_bounded(
            connection, project_id, 100,
        )
        .map_err(|e| e.to_string())?;
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
    let future_reservation_refs =
        crate::repositories::narrative::NarrativeRepository::latest_artifact_payload(
            connection, project_id, "BookPlan",
        )
        .map_err(|e| e.to_string())?
        .and_then(|payload| {
            payload
                .get("futureReservations")
                .and_then(Value::as_array)
                .cloned()
        })
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            item.get("reservationId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect();
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
    state.truncated = state.truncated || task_truncated || evidence_truncated;
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
            let threads = continuity_from_request(connection, &request, project_id)
                .map_err(ManagerError::ProjectCommand)?;
            serde_json::to_value(
                json!({"projectId":project_id,"projectRevision":revision,"threads":threads}),
            )
            .map_err(|e| ManagerError::ProjectCommand(e.to_string()))
        })
        .map_err(|e| e.to_string())
}

pub fn query_volume_progress_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let volume_id = required_string(&request, "volumeId")?;
    let revision = state
        .manager
        .project_revision(project_id)
        .map_err(|e| e.to_string())?;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
            let route = crate::repositories::narrative::NarrativeRepository::volume_plan_route(
                connection, project_id, volume_id,
            )
            .map_err(|e| ManagerError::ProjectCommand(e.to_string()))?
            .ok_or_else(|| ManagerError::ProjectCommand("VOLUME_PLAN_NOT_FOUND".into()))?;
            let plan =
                crate::repositories::narrative::NarrativeRepository::latest_artifact_payload(
                    connection,
                    project_id,
                    "VolumePlan",
                )
                .map_err(|e| ManagerError::ProjectCommand(e.to_string()))?
                .unwrap_or(Value::Null);
            let target_chapters = plan
                .get("capacityBudget")
                .and_then(|budget| budget.get("maxChapters"))
                .and_then(Value::as_i64)
                .unwrap_or_else(|| {
                    request
                        .get("currentChapter")
                        .and_then(Value::as_i64)
                        .unwrap_or(0)
                });
            let (tasks, tasks_truncated) =
                crate::repositories::narrative::NarrativeRepository::task_states_bounded(
                    connection, project_id, 100,
                )
                .map_err(|e| ManagerError::ProjectCommand(e.to_string()))?;
            let mut result = serde_json::to_value(build_volume_progress(VolumeProgressInput {
                volume_id: volume_id.into(),
                target_chapters,
                current_chapter: request
                    .get("currentChapter")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                tasks,
                generated_revision: revision,
            }))
            .map_err(|e| ManagerError::ProjectCommand(e.to_string()))?;
            result["routePlanRef"] = serde_json::to_value(route)
                .map_err(|e| ManagerError::ProjectCommand(e.to_string()))?;
            result["truncated"] = json!(tasks_truncated);
            Ok(result)
        })
        .map_err(|e| e.to_string())
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
            let status = evaluate_plan_stale(
                compiled_revision,
                compiled_digest,
                current_revision,
                &current_digest,
            );
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
                serde_json::to_value(
                    crate::repositories::narrative::NarrativeRepository::canon_events(
                        self.connection,
                        self.project_id,
                        chapter,
                        bounds.max_results as i64,
                    )
                    .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?
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
            "query_continuity" => serde_json::to_value(continuity_from_request(
                self.connection,
                input,
                self.project_id,
            )?)
            .map_err(|e| e.to_string())?,
            "query_context_manifest" => serde_json::to_value(compile_context_manifest(
                crate::narrative::ContextCompilerInput {
                    execution_mode: "LONG_FORM".into(),
                    project_revision: input
                        .get("projectRevision")
                        .and_then(Value::as_i64)
                        .unwrap_or(0),
                    goal_contract_refs: Vec::new(),
                    state: narrative_state_from_request(self.connection, input, self.project_id)?,
                    continuity_threads: continuity_from_request(
                        self.connection,
                        input,
                        self.project_id,
                    )?,
                    active_task_refs: Vec::new(),
                    future_reservation_refs: Vec::new(),
                    selected_evidence: Vec::new(),
                    omitted_soft_sources: Vec::new(),
                    hard_tokens: 1024,
                    soft_tokens: 0,
                },
            )?)
            .map_err(|e| e.to_string())?,
            "query_project_meta" => serde_json::to_value(
                crate::repositories::narrative::NarrativeRepository::goal_contract_refs(
                    self.connection,
                    self.project_id,
                    bounds.max_results as i64,
                )
                .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?,
            "query_canon"
            | "query_relations"
            | "query_obligations"
            | "query_foreshadows"
            | "query_mysteries"
            | "query_task_states"
            | "query_dependency_edges"
            | "query_model_runs"
            | "query_commit_receipts"
            | "query_integrity_status"
            | "query_volume_progress"
            | "query_plan_stale"
            | "diagnose_canon_evidence"
            | "diagnose_knowledge_event"
            | "diagnose_thread_evidence"
            | "diagnose_task_evidence"
            | "diagnose_projection_authority" => json!([]),
            _ => return Err("CAPABILITY_UNSUPPORTED_BY_ADAPTER".into()),
        };
        let source_refs = match &value {
            Value::Array(items) => items
                .iter()
                .filter_map(|item| {
                    item.get("factId")
                        .or_else(|| item.get("eventId"))
                        .or_else(|| item.get("evidenceId"))
                        .or_else(|| item.get("threadId"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .collect(),
            _ => Vec::new(),
        };
        Ok((value, source_refs, truncated))
    }
}

pub fn support_session_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?;
    let question = required_string(&request, "question")?;
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
    let mut plan = plan_support(&scope, question, "1970-01-01T00:00:00Z")?;
    let registry = CapabilityRegistry::read_only_r1();
    let executor = BoundedCapabilityExecutor {
        registry: registry.clone(),
    };
    let input = request.clone();
    let mut calls = Vec::new();
    let mut evidence_ledger = Vec::new();
    let mut source_refs = BTreeSet::new();
    let mut result_digests = Vec::new();
    let mut evidence_found = false;
    state
        .manager
        .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
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
                        max_tokens: 4096,
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
                    started_at: "1970-01-01T00:00:00Z".into(),
                    finished_at: "1970-01-01T00:00:00Z".into(),
                    status: result.status.clone(),
                });
                calls.push(result);
                step.status = crate::harness::SupportStepStatus::DONE;
            }
            Ok::<(), ManagerError>(())
        })
        .map_err(|e| e.to_string())?;
    let requires_evidence = question.contains("知道") || question.contains("承诺");
    let diagnostic_finding = if question.contains("知道") {
        state
            .manager
            .with_read_connection(project_id, "1970-01-01T00:00:00Z", |connection| {
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
                let event = request
                    .get("chapterId")
                    .and_then(Value::as_str)
                    .map(|chapter| {
                        crate::repositories::narrative::NarrativeRepository::canon_events(
                            connection, project_id, chapter, 1,
                        )
                    })
                    .transpose()
                    .map_err(|e| ManagerError::ProjectCommand(e.to_string()))?
                    .and_then(|mut events| events.pop());
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
                Ok::<_, ManagerError>(knowledge.map(|item| {
                    diagnose_knowledge_event(
                        &scope,
                        question,
                        &item,
                        event.as_ref(),
                        evidence.as_ref(),
                    )
                }))
            })
            .map_err(|e| e.to_string())?
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
        pending_authorization_ref: None,
        created_at: "1970-01-01T00:00:00Z".into(),
    };
    let handle = state
        .manager
        .open(project_id, "", "1970-01-01T00:00:00Z")
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
        started_at: "1970-01-01T00:00:00Z".into(),
        finished_at: "1970-01-01T00:00:00Z".into(),
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
        result["affectedQuery"] =
            query_narrative_state_impl(state, json!({"projectId":project_id,"scope":"PROJECT"}))?;
        let scope = crate::harness::ProjectScope {
            project_id: project_id.into(),
            project_revision: current_revision,
            question_digest: checkpoint.question_digest.clone(),
            requested_mode: "DIAGNOSE".into(),
            requested_scope: "PROJECT".into(),
        };
        let plan = crate::harness::plan_support(
            &scope,
            "checkpoint affected facts requery",
            "1970-01-01T00:00:00Z",
        )?;
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

pub fn execute_command_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let command_type = required_string(&request, "type")?;
    let command_id = required_string(&request, "commandId")?;
    let idempotency_key = required_string(&request, "idempotencyKey")?;
    let requested_at = required_string(&request, "requestedAt")?;
    let project_id = request.get("projectId").and_then(Value::as_str);
    let expected_revision = expected_revision(&request)?;
    let digest = request_digest(&request)?;
    match command_type {
        "production_start" | "production_pause" | "production_resume" | "production_cancel" => {
            let id = project_id.ok_or_else(|| "IPC_INVALID:projectId".to_string())?;
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
        "project_create_from_idea" | "project_create_from_market" => {
            let title = request
                .pointer("/payload/title")
                .and_then(Value::as_str)
                .unwrap_or("未命名作品");
            let id = stable_project_id(command_id)?;
            if let Some(replayed) = replayed_project_command(state, &id, idempotency_key, &digest)?
            {
                return Ok(replayed);
            }
            match state.manager.overview(&id) {
                Ok((existing_title, revision, _)) if existing_title == title && revision == 0 => {
                    // Recover a crash after atomic create/catalog registration but
                    // before the command receipt was persisted.
                }
                Ok(_) => return Err("IDEMPOTENCY_CONFLICT".to_string()),
                Err(crate::project_db::ManagerError::ProjectNotFound) => {
                    drop(
                        state
                            .manager
                            .create(&id, title, requested_at)
                            .map_err(|error| error.to_string())?,
                    );
                }
                Err(error) => return Err(error.to_string()),
            }
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
        search_fulltext,
        query_volume_list,
        query_reader_volumes,
        query_chapter_list,
        query_chapter,
        query_chapter_final,
        query_narrative_state,
        query_continuity,
        query_volume_progress,
        query_context_manifest,
        query_plan_stale,
        support_session,
        support_checkpoint_resume,
        execute_command,
        query_global_settings,
        test_provider,
        query_market_opportunities,
    ]
}
