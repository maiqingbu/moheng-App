use crate::catalog::CatalogRepository;
use crate::generated::types::ProjectCommandReceipt;
use crate::hash::sha256_canonical;
use crate::project_db::paths::AppPaths;
use crate::project_db::{ManagerError, ProjectCommandOutcome, ProjectCommandSpec, ProjectManager};
use crate::runtime::{lease_expiry, DurableJobService, JobSpec};
use crate::stable_id::{StableIdKind, StableIdentityFactory};
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
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM decisions WHERE project_id=?1 AND outcome IN ('OPEN','PENDING')",
                        [&row.project_id],
                        |query| query.get(0),
                    )
                    .map_err(ManagerError::from)
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
            connection
                .query_row(
                    "SELECT COUNT(*) FROM decisions WHERE project_id=?1 AND outcome IN ('OPEN','PENDING')",
                    [project_id],
                    |row| row.get(0),
                )
                .map_err(ManagerError::from)
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
    if !job_id.starts_with("job:") {
        return Err("stable job id required".into());
    }
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
                    DurableJobService::acquire_lease(
                        connection,
                        project_id,
                        &job.job_id,
                        "production-ipc",
                        &lease_expiry(requested_at),
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
                    DurableJobService::transition(
                        connection,
                        project_id,
                        &job_id,
                        next,
                        requested_at,
                    )
                    .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
                    if command_type == "production_pause" || command_type == "production_cancel" {
                        DurableJobService::release_lease(
                            connection,
                            project_id,
                            &job_id,
                            requested_at,
                        )
                        .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?;
                    } else {
                        DurableJobService::acquire_lease(
                            connection,
                            project_id,
                            &job_id,
                            "production-ipc",
                            &lease_expiry(requested_at),
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
