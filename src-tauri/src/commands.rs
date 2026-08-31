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
use crate::repositories::ops::{chapter_page, single_chapter, volume_page, SearchService};
use crate::runtime::{pending_decision_count, DurableJobService, JobSpec};
use crate::stable_id::{StableIdKind, StableIdentityFactory};
use libsql_rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;
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
            paths,
            production_gateway: Arc::new(crate::production::UnavailableProductionGateway),
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

/// B08 production projection.  This is a bounded read projection assembled
/// from the authoritative job/artifact/runtime rows; it never reconstructs a
/// candidate as a committed chapter and never trusts Vue-local state.
pub fn query_production_center_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let project_id = required_string(&request, "projectId")?.to_string();
    state
        .manager
        .with_read_connection(&project_id, "1970-01-01T00:00:00Z", |connection| {
            let runtime: (String, i64, i64, i64) = connection
                .query_row(
                    "SELECT COALESCE(run_state,'IDLE'), COALESCE(committed_chapters,0), COALESCE(total_cost_cents,0), COALESCE(candidate_chapters,0) FROM project_runtime_state WHERE project_id=?1",
                    [&project_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap_or_else(|_| ("IDLE".into(), 0, 0, 0));
            let job: Option<(String, String, Option<String>, String)> = connection
                .query_row(
                    "SELECT job_id, current_stage, chapter_id, status FROM jobs WHERE project_id=?1 ORDER BY updated_at DESC, job_id DESC LIMIT 1",
                    [&project_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()?;
            let current_stage = job.as_ref().map(|(_, stage, _, _)| stage.clone()).unwrap_or_else(|| "IDLE".into());
            let active_detail = job
                .as_ref()
                .map(|(id, _, chapter, status)| format!("{status} · {id} · {}", chapter.as_deref().unwrap_or("未绑定章节")))
                .unwrap_or_else(|| "暂无生产任务".into());
            let stages = ["CONTEXT", "PLAN", "RENDER", "CANDIDATE", "AUDIT", "GATE", "COMMIT", "RECONCILE", "COMPLETED"];
            let stage_order = stages.iter().position(|stage| *stage == current_stage).unwrap_or(usize::MAX);
            let timeline = stages.iter().enumerate().map(|(order, stage)| json!({
                "id": format!("b08:{stage}"), "order": order + 1, "title": stage,
                "description": "B08 Chapter Production Runtime", "status": if order < stage_order { "done" } else if order == stage_order { "active" } else { "pending" }
            })).collect::<Vec<_>>();
            let mut artifacts = connection.prepare("SELECT artifact_id, artifact_type, artifact_status, payload_hash, version FROM artifacts WHERE project_id=?1 ORDER BY created_at DESC, artifact_id DESC LIMIT 20")?;
            let artifact_items = artifacts.query_map([&project_id], |row| Ok(json!({
                "id": row.get::<_, String>(0)?, "title": row.get::<_, String>(1)?,
                "summary": format!("v{} · {} · {}", row.get::<_, i64>(4)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?),
                "status": if row.get::<_, String>(2)? == "COMMITTED" { "official" } else { "candidate" }
            })))?.collect::<Result<Vec<_>, _>>()?;
            let task = job.as_ref().map(|(id, _, chapter, status)| json!({"id":id,"title":"Chapter Production","description":format!("{} · {}", chapter.as_deref().unwrap_or(""), status),"progress":if status=="COMPLETED" {100} else {50}})).into_iter().collect::<Vec<_>>();
            let next_chapter: Option<(String, i64, String, String)> = connection.query_row(
                "SELECT c.chapter_id,c.chapter_no,c.title,c.volume_id FROM chapters c WHERE c.project_id=?1 AND c.status!='COMMITTED' ORDER BY c.chapter_no,c.chapter_id LIMIT 1",
                [&project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            ).optional()?;
            let active_chapter_id = job.as_ref().and_then(|(_,_,chapter,_)| chapter.clone())
                .or_else(|| next_chapter.as_ref().map(|row| row.0.clone()));
            let plan_row: Option<(String,i64,String,String)> = active_chapter_id.as_ref().and_then(|chapter_id| {
                connection.query_row(
                    "SELECT h.artifact_id,h.version,h.payload_hash,p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan' AND h.scope_level='CHAPTER' AND h.scope_key=?2",
                    libsql_rusqlite::params![project_id, json!({"level":"CHAPTER","chapterId":chapter_id}).to_string()],
                    |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
                ).optional().ok().flatten()
            });
            let plan_projection = plan_row.as_ref().and_then(|(id,version,hash,raw)| {
                serde_json::from_str::<Value>(raw).ok().map(|payload| json!({
                    "ref":{"artifactId":id,"version":version,"payloadHash":hash},
                    "chapterGoal":payload.get("chapterGoal"),
                    "contextManifestRef":payload.get("contextManifestRef"),
                    "activeTaskRefs":payload.get("activeTaskRefs").cloned().unwrap_or_else(|| json!([])),
                    "continuityActions":payload.get("continuityActions").cloned().unwrap_or_else(|| json!([])),
                    "compiledFromProjectRevision":payload.get("compiledFromProjectRevision"),
                    "compiledFromStateDigest":payload.get("compiledFromStateDigest")
                }))
            });
            let current_revision: i64 = connection.query_row(
                "SELECT revision FROM project_meta WHERE project_id=?1", [&project_id], |row| row.get(0)
            )?;
            let stale_status = plan_projection.as_ref().map(|plan| {
                if plan.get("compiledFromProjectRevision").and_then(Value::as_i64).is_some_and(|revision| revision > current_revision) {
                    "PLAN_STALE"
                } else { "VALID_OR_REQUIRES_RELEVANT_RECHECK" }
            }).unwrap_or("NO_PLAN");
            let candidate_ref: Option<Value> = active_chapter_id.as_ref().and_then(|chapter_id| connection.query_row(
                "SELECT json_object('artifactId',a.artifact_id,'version',a.version,'payloadHash',a.payload_hash) FROM artifacts a WHERE a.project_id=?1 AND a.artifact_type='ChapterCandidate' AND a.scope_level='CHAPTER' AND a.scope_key=?2 ORDER BY a.created_at DESC,a.version DESC LIMIT 1",
                libsql_rusqlite::params![project_id, json!({"level":"CHAPTER","chapterId":chapter_id}).to_string()],
                |row| row.get::<_,String>(0),
            ).optional().ok().flatten().and_then(|raw| serde_json::from_str(&raw).ok()));
            let final_ref: Option<Value> = active_chapter_id.as_ref().and_then(|chapter_id| connection.query_row(
                "SELECT json_object('artifactId',h.artifact_id,'version',h.version,'payloadHash',h.payload_hash) FROM canonical_heads h WHERE h.project_id=?1 AND h.artifact_type='FinalChapter' AND h.scope_level='CHAPTER' AND h.scope_key=?2",
                libsql_rusqlite::params![project_id, json!({"level":"CHAPTER","chapterId":chapter_id}).to_string()],
                |row| row.get::<_,String>(0),
            ).optional().ok().flatten().and_then(|raw| serde_json::from_str(&raw).ok()));
            let model_runs = if let Some((job_id,_,_,_)) = &job {
                let mut statement = connection.prepare("SELECT model_run_id,capability,provider,model,status FROM model_runs WHERE project_id=?1 AND job_id=?2 AND capability IN ('PLAN_STRATEGY','RENDER_CHAPTER','AUDIT_EXTRACT') ORDER BY created_at,model_run_id")?;
                let rows = statement.query_map(libsql_rusqlite::params![project_id,job_id], |row| Ok(json!({
                    "modelRunId":row.get::<_,String>(0)?,"capability":row.get::<_,String>(1)?,
                    "provider":row.get::<_,String>(2)?,"model":row.get::<_,String>(3)?,"status":row.get::<_,String>(4)?
                })))?.collect::<Result<Vec<_>,_>>()?;
                rows
            } else { Vec::new() };
            let latest_receipt: Option<String> = active_chapter_id.as_ref().and_then(|chapter_id| connection.query_row(
                "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ProductionReceipt' AND h.scope_level='CHAPTER' AND h.scope_key=?2",
                libsql_rusqlite::params![project_id, json!({"level":"CHAPTER","chapterId":chapter_id}).to_string()],
                |row| row.get(0),
            ).optional().ok().flatten());
            let reconciliation = latest_receipt.as_deref().and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                .and_then(|value| value.get("reconciliation").cloned()).unwrap_or(Value::Null);
            let repair: Option<Value> = connection.query_row(
                "SELECT rp.repair_plan_id,rp.based_on_project_revision,rp.diagnosis_json,rp.finding_refs_json,rp.target_authority_owner,rp.proposed_operations_json,rp.affected_artifacts_json,rp.affected_scopes_json,rp.risk_level,rp.requires_backup,rp.expected_postconditions_json,rp.operation_digest,rp.status,ra.authorization_id,ra.consumed_at,rr.receipt_ref,rr.status,rr.postconditions_json FROM repair_plans rp LEFT JOIN repair_authorizations ra ON ra.project_id=rp.project_id AND ra.repair_plan_id=rp.repair_plan_id LEFT JOIN repair_execution_receipts rr ON rr.project_id=rp.project_id AND rr.repair_plan_id=rp.repair_plan_id WHERE rp.project_id=?1 ORDER BY rp.created_at DESC,ra.issued_at DESC LIMIT 1",
                [&project_id],
                |row| {
                    let receipt_ref = row.get::<_,Option<String>>(15)?;
                    let receipt_status = row.get::<_,Option<String>>(16)?;
                    let receipt_postconditions = row.get::<_,Option<String>>(17)?;
                    let execution_receipt = receipt_ref.map(|receipt_ref| json!({
                        "receiptRef":receipt_ref,
                        "status":receipt_status.unwrap_or_default(),
                        "postconditions":serde_json::from_str::<Value>(&receipt_postconditions.unwrap_or_else(|| "[]".into())).unwrap_or_else(|_| json!([]))
                    }));
                    Ok(json!({
                    "repairPlanId":row.get::<_,String>(0)?,"basedOnProjectRevision":row.get::<_,i64>(1)?,
                    "diagnosis":serde_json::from_str::<Value>(&row.get::<_,String>(2)?).unwrap_or(Value::Null),
                    "findingRefs":serde_json::from_str::<Value>(&row.get::<_,String>(3)?).unwrap_or_else(|_| json!([])),
                    "targetAuthorityOwner":row.get::<_,String>(4)?,
                    "proposedOperations":serde_json::from_str::<Value>(&row.get::<_,String>(5)?).unwrap_or_else(|_| json!([])),
                    "affectedArtifacts":serde_json::from_str::<Value>(&row.get::<_,String>(6)?).unwrap_or_else(|_| json!([])),
                    "affectedScopes":serde_json::from_str::<Value>(&row.get::<_,String>(7)?).unwrap_or_else(|_| json!([])),
                    "riskLevel":row.get::<_,String>(8)?,"requiresBackup":row.get::<_,i64>(9)? != 0,
                    "expectedPostconditions":serde_json::from_str::<Value>(&row.get::<_,String>(10)?).unwrap_or_else(|_| json!([])),
                    "operationDigest":row.get::<_,String>(11)?,"status":row.get::<_,String>(12)?,
                    "authorizationId":row.get::<_,Option<String>>(13)?,"consumedAt":row.get::<_,Option<String>>(14)?,
                    "executionReceipt":execution_receipt
                }))},
            ).optional()?;
            let next_chapter_projection = next_chapter.map(|(id,number,title,volume_id)| json!({
                "id":id,"number":number,"title":title,"volumeId":volume_id,
                "executionMode":if plan_projection.is_some() {"LONG_FORM"} else {"OPENING_BOOTSTRAP"}
            }));
            Ok(json!({
                "timeline": timeline, "tasks": task,
                "artifacts":{"items":artifact_items,"nextCursor":null,"total":artifact_items.len()},
                "signing":{"status":"NOT_STARTED","verdict":"B08 production gate is authoritative"},
                "activeWork":{"label":current_stage,"detail":active_detail},
                "productionStage":current_stage,
                "nextChapter":next_chapter_projection,
                "chapterPlan":plan_projection,
                "triggeredContinuity":plan_projection.as_ref().and_then(|plan| plan.get("continuityActions")).cloned().unwrap_or_else(|| json!([])),
                "gate":{"status":if final_ref.is_some() {"PASS"} else if stale_status=="PLAN_STALE" {"BLOCKED"} else {"PENDING"},"reason":if stale_status=="PLAN_STALE" {"PLAN_STALE / RECOMPILE_REQUIRED"} else {""}},
                "stale":{"status":stale_status,"reason":if stale_status=="PLAN_STALE" {"Relevant authority changed"} else {""}},
                "modelRuns":model_runs,"candidateRef":candidate_ref,"finalChapterRef":final_ref,
                "reconciliation":reconciliation,"repair":repair,
                "costSummary":{"currentVolume":0,"projectTotal":runtime.2 as f64 / 100.0,"budgetRemaining":0},
                "committedChapters":runtime.1,"candidateChapters":runtime.3
            }))
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
        json!({"projectId":project_id,"projectRevision":revision,"volumeId":volume_id,"volumeIntentRef":volume_intent_ref,"volumePlanRef":route_ref,"routeVersion":route.get("routeVersion").cloned().unwrap_or(Value::Null),"tasks":tasks,"routeInvariants":route.get("routeInvariants").cloned().unwrap_or_else(|| json!([])),"requiredExitConditions":route.get("requiredExitConditions").cloned().unwrap_or_else(|| json!([])),"capacityBudget":route.get("capacityBudget").cloned().unwrap_or_else(|| json!({})),"taskReality":progress.get("tasks").cloned().unwrap_or_else(|| json!([])),"planningPressure":progress.get("planningPressure").cloned().unwrap_or_else(|| json!({})),"continuityThreads":continuity_threads,"futureReservations":future_reservations,"currentChapterPlan":current_chapter_plan,"horizonPreview":horizon_preview,"owner":"B07::PlanningRoute","truncated":truncated}),
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
                let revision: i64 = self
                    .connection
                    .query_row(
                        "SELECT revision FROM project_meta WHERE project_id=?1",
                        [self.project_id],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
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
            "replan_volume_route" => Err("H03_CAPABILITY_UNAVAILABLE_BEFORE_B09".into()),
            "execute_change_request" => Err("H03_CHANGE_REQUEST_PROPOSAL_ONLY".into()),
            _ => Err("H03_OWNER_OPERATION_UNKNOWN".into()),
        }
    }

    fn verify(&self, operation: &crate::repair::RepairOperation) -> Result<Value, String> {
        match operation.operation.as_str() {
            "refresh_runtime_projection" => {
                let updated_at: String = self
                    .connection
                    .query_row(
                        "SELECT updated_at FROM project_runtime_state WHERE project_id=?1",
                        [self.project_id],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                Ok(
                    json!({"operation":operation.operation,"status":if updated_at == self.now {"PASS"} else {"FAIL"},"observedUpdatedAt":updated_at}),
                )
            }
            "rebuild_projection" | "rebuild_fts" => {
                let healthy = SearchService::projection_health(self.connection, self.project_id)
                    .map_err(|error| error.to_string())?;
                Ok(
                    json!({"operation":operation.operation,"status":if healthy {"PASS"} else {"FAIL"},"projectionHealthy":healthy}),
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
                let raw: String = self.connection.query_row(
                    "SELECT p.payload_json FROM canonical_heads h JOIN artifacts a ON a.project_id=h.project_id AND a.artifact_id=h.artifact_id AND a.version=h.version JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan' AND h.scope_level='CHAPTER' AND h.scope_key=?2 AND json_extract(a.provenance_json,'$.createdBy')='B07::PlanningOwner'",
                    libsql_rusqlite::params![self.project_id, json!({"level":"CHAPTER","chapterId":chapter_id}).to_string()],
                    |row| row.get(0),
                ).map_err(|error| error.to_string())?;
                let actual: Value =
                    serde_json::from_str(&raw).map_err(|error| error.to_string())?;
                let revision: i64 = self
                    .connection
                    .query_row(
                        "SELECT revision FROM project_meta WHERE project_id=?1",
                        [self.project_id],
                        |row| row.get(0),
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
                let authority = json!({
                    "projectId":self.project_id,"chapterId":chapter_id,"volumeId":volume_id,
                    "executionMode":"LONG_FORM","entityIds":operation.params.get("entityIds").cloned().unwrap_or_else(|| json!([])),
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
                Ok(
                    json!({"operation":operation.operation,"status":if actual == *expected && matches!(stale, PlanStaleStatus::Valid) {"PASS"} else {"FAIL"},"owner":"B07::PlanningOwner","stale":if matches!(stale, PlanStaleStatus::Valid) {"VALID"} else {"PLAN_STALE"}}),
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
                    json!({"operation":operation.operation,"status":if exists {"PASS"} else {"FAIL"},"backupReadable":exists}),
                )
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

pub fn execute_command_impl(state: &KernelState, request: Value) -> Result<Value, String> {
    let command_type = required_string(&request, "type")?;
    let command_id = required_string(&request, "commandId")?;
    let idempotency_key = required_string(&request, "idempotencyKey")?;
    let requested_at = required_string(&request, "requestedAt")?;
    let project_id = request.get("projectId").and_then(Value::as_str);
    let expected_revision = expected_revision(&request)?;
    let digest = request_digest(&request)?;
    match command_type {
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
        query_production_center,
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
        query_plan_route,
        support_session,
        support_checkpoint_resume,
        execute_command,
        query_global_settings,
        test_provider,
        query_market_opportunities,
    ]
}
