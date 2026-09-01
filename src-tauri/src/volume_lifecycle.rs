//! B09 volume lifecycle owner.
//!
//! B07 remains the route validator and B08 remains the chapter production root.
//! This module only owns the cross-volume lifecycle boundary: rebuildable
//! planning state, an authorized route replan, atomic volume close, and the
//! next-volume handoff.

use crate::commit::{CommitService, ProjectCommandOutcome, ProjectCommandSpec};
use crate::generated::types::{
    ArtifactEnvelope, ArtifactEnvelopeProvenance, ArtifactRef, ArtifactScope, ArtifactScopeLevel,
    ArtifactStatus, ArtifactType, AuthorityLevel, SemanticLifecycle,
};
use crate::hash::sha256_canonical;
use crate::planning;
use crate::repositories::artifacts::{scope_key, ArtifactRepository};
use crate::repositories::narrative::NarrativeRepository;
use crate::repositories::volume_lifecycle::VolumeLifecycleRepository;
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct RouteReplanRequest {
    pub project_id: String,
    pub volume_id: String,
    pub expected_revision: i64,
    pub command_id: String,
    pub idempotency_key: String,
    pub trigger: String,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub expected_reality_digest: Option<String>,
    pub acknowledged_responsibility_refs: Vec<String>,
    pub now: String,
}

#[derive(Debug, Clone)]
pub struct VolumeCloseRequest {
    pub project_id: String,
    pub volume_id: String,
    pub expected_revision: i64,
    pub command_id: String,
    pub idempotency_key: String,
    pub reason: String,
    pub fault_injection: Option<String>,
    pub now: String,
}

#[derive(Debug, Clone)]
pub struct NextVolumeHandoffRequest {
    pub project_id: String,
    pub current_volume_id: String,
    pub next_volume_id: String,
    pub expected_revision: i64,
    pub command_id: String,
    pub idempotency_key: String,
    pub title: String,
    pub chapter_start: i64,
    pub chapter_end: i64,
    pub target_words: i64,
    pub now: String,
}

pub struct VolumeLifecycleOwner;

impl VolumeLifecycleOwner {
    pub fn rebuild_planning_state(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        now: &str,
    ) -> Result<Value, String> {
        let projection = build_planning_state(connection, project_id, volume_id, now, None)?;
        persist_planning_state(connection, project_id, volume_id, &projection, now)?;
        Ok(projection)
    }

    pub fn replan_volume_route(
        connection: &Connection,
        request: RouteReplanRequest,
    ) -> Result<Value, String> {
        if request.reason.trim().is_empty() {
            return Err("REPLAN_REASON_REQUIRED".into());
        }
        if request.evidence_refs.is_empty() {
            return Err("REPLAN_EVIDENCE_REQUIRED".into());
        }
        for evidence_ref in &request.evidence_refs {
            if !VolumeLifecycleRepository::has_evidence_ref(
                connection,
                &request.project_id,
                evidence_ref,
            )
            .map_err(|error| error.to_string())?
            {
                return Err(format!("REPLAN_EVIDENCE_NOT_FOUND:{evidence_ref}"));
            }
        }

        let request_digest = sha256_canonical(&json!({
            "projectId": request.project_id,
            "volumeId": request.volume_id,
            "expectedRevision": request.expected_revision,
            "commandId": request.command_id,
            "idempotencyKey": request.idempotency_key,
            "trigger": request.trigger,
            "reason": request.reason,
            "evidenceRefs": request.evidence_refs,
            "expectedRealityDigest": request.expected_reality_digest,
            "acknowledgedResponsibilityRefs": request.acknowledged_responsibility_refs,
        }))
        .map_err(|error| error.to_string())?;
        let spec = ProjectCommandSpec {
            project_id: &request.project_id,
            result_project_id: &request.project_id,
            command_id: &request.command_id,
            idempotency_key: &request.idempotency_key,
            command_type: "replan_volume_route",
            request_digest: &request_digest,
            expected_revision: Some(request.expected_revision),
            now: &request.now,
            advance_revision: true,
        };

        let outcome = CommitService::commit_project_command(connection, spec, |connection| {
            apply_route_replan(connection, &request)
                .map_err(crate::commit::CommitError::ProjectCommandApply)
        })
        .map_err(|error| error.to_string())?;

        match outcome {
            ProjectCommandOutcome::Committed { value, .. } => Ok(value),
            ProjectCommandOutcome::Replayed(receipt) => {
                let route = current_route(connection, &request.project_id, &request.volume_id)?;
                Ok(json!({
                    "replayed": true,
                    "routeVersion": route.1.get("routeVersion").cloned().unwrap_or(Value::Null),
                    "projectRevision": receipt.after_revision,
                    "receiptId": receipt.receipt_id,
                }))
            }
        }
    }

    pub fn close_volume(
        connection: &Connection,
        request: VolumeCloseRequest,
    ) -> Result<Value, String> {
        if request.reason.trim().is_empty() {
            return Err("VOLUME_CLOSE_REASON_REQUIRED".into());
        }
        let request_digest = sha256_canonical(&json!({
            "projectId": request.project_id,
            "volumeId": request.volume_id,
            "expectedRevision": request.expected_revision,
            "commandId": request.command_id,
            "idempotencyKey": request.idempotency_key,
            "reason": request.reason,
        }))
        .map_err(|error| error.to_string())?;
        let spec = ProjectCommandSpec {
            project_id: &request.project_id,
            result_project_id: &request.project_id,
            command_id: &request.command_id,
            idempotency_key: &request.idempotency_key,
            command_type: "volume_close",
            request_digest: &request_digest,
            expected_revision: Some(request.expected_revision),
            now: &request.now,
            advance_revision: true,
        };
        let outcome = CommitService::commit_project_command(connection, spec, |connection| {
            apply_volume_close(connection, &request)
                .map_err(crate::commit::CommitError::ProjectCommandApply)
        })
        .map_err(|error| error.to_string())?;

        match outcome {
            ProjectCommandOutcome::Committed { value, .. } => Ok(value),
            ProjectCommandOutcome::Replayed(receipt) => {
                let eligibility = VolumeLifecycleRepository::close_eligibility(
                    connection,
                    &request.project_id,
                    &request.volume_id,
                )
                .map_err(|error| error.to_string())?
                .unwrap_or_else(|| "BLOCKED".into());
                Ok(json!({
                    "replayed": true,
                    "nextVolumeEligibility": eligibility,
                    "projectRevision": receipt.after_revision,
                    "receiptId": receipt.receipt_id,
                }))
            }
        }
    }

    pub fn handoff_next_volume(
        connection: &Connection,
        request: NextVolumeHandoffRequest,
    ) -> Result<Value, String> {
        if request.title.trim().is_empty() {
            return Err("NEXT_VOLUME_TITLE_REQUIRED".into());
        }
        if request.chapter_start <= 0 || request.chapter_end < request.chapter_start {
            return Err("NEXT_VOLUME_CHAPTER_RANGE_INVALID".into());
        }
        let request_digest = sha256_canonical(&json!({
            "projectId": request.project_id,
            "currentVolumeId": request.current_volume_id,
            "nextVolumeId": request.next_volume_id,
            "expectedRevision": request.expected_revision,
            "commandId": request.command_id,
            "idempotencyKey": request.idempotency_key,
            "title": request.title,
            "chapterStart": request.chapter_start,
            "chapterEnd": request.chapter_end,
            "targetWords": request.target_words,
        }))
        .map_err(|error| error.to_string())?;
        let spec = ProjectCommandSpec {
            project_id: &request.project_id,
            result_project_id: &request.project_id,
            command_id: &request.command_id,
            idempotency_key: &request.idempotency_key,
            command_type: "volume_handoff",
            request_digest: &request_digest,
            expected_revision: Some(request.expected_revision),
            now: &request.now,
            advance_revision: true,
        };
        let outcome = CommitService::commit_project_command(connection, spec, |connection| {
            apply_next_volume_handoff(connection, &request)
                .map_err(crate::commit::CommitError::ProjectCommandApply)
        })
        .map_err(|error| error.to_string())?;

        match outcome {
            ProjectCommandOutcome::Committed { value, .. } => Ok(value),
            ProjectCommandOutcome::Replayed(receipt) => Ok(json!({
                "replayed": true,
                "eligibility": "ELIGIBLE",
                "projectRevision": receipt.after_revision,
                "receiptId": receipt.receipt_id,
            })),
        }
    }
}

fn current_route(
    connection: &Connection,
    project_id: &str,
    volume_id: &str,
) -> Result<(ArtifactRef, Value), String> {
    let route_ref = NarrativeRepository::volume_plan_route(connection, project_id, volume_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VOLUME_PLAN_NOT_FOUND".to_string())?;
    let route = NarrativeRepository::artifact_payload_by_ref(connection, project_id, &route_ref)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VOLUME_PLAN_PAYLOAD_NOT_FOUND".to_string())?;
    Ok((route_ref, route))
}

fn current_intent(
    connection: &Connection,
    project_id: &str,
    route: &Value,
) -> Result<(ArtifactRef, Value), String> {
    let intent_ref: ArtifactRef = serde_json::from_value(
        route
            .get("volumeIntentRef")
            .cloned()
            .ok_or_else(|| "VOLUME_PLAN_INTENT_REF_REQUIRED".to_string())?,
    )
    .map_err(|error| format!("VOLUME_PLAN_INTENT_REF_INVALID:{error}"))?;
    let intent = NarrativeRepository::artifact_payload_by_ref(connection, project_id, &intent_ref)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VOLUME_INTENT_NOT_FOUND".to_string())?;
    planning::validate_volume_intent(&intent)?;
    Ok((intent_ref, intent))
}

fn validate_route_reservations(
    connection: &Connection,
    project_id: &str,
    volume_id: &str,
    route: &Value,
    intent: &Value,
) -> Result<(), String> {
    let declared = intent
        .get("futureReservationRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let route_refs = route
        .get("tasks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|task| task.get("reservationRefs").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    if route_refs
        .iter()
        .any(|reference| !declared.contains(reference))
    {
        return Err("FUTURE_RESERVATION_VIOLATION".into());
    }
    let Some(book_plan) =
        NarrativeRepository::latest_artifact_payload(connection, project_id, "BookPlan")
            .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let reservations = book_plan
        .get("futureReservations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if reservations.is_empty() || route_refs.is_empty() {
        return Ok(());
    }
    let current_volume_no = NarrativeRepository::volume_number(connection, project_id, volume_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VOLUME_NOT_FOUND".to_string())?;
    let actions = json!({
        "reservationRefs": route_refs.into_iter().collect::<Vec<_>>(),
        "sceneExecution": []
    });
    planning::validate_reservation_actions(&actions, &reservations, current_volume_no)
}

fn compile_route_replan_input(
    connection: &Connection,
    request: &RouteReplanRequest,
) -> Result<Value, String> {
    let goal_contract_refs = NarrativeRepository::goal_contract_refs_for_volume(
        connection,
        &request.project_id,
        &request.volume_id,
        100,
    )
    .map_err(|error| error.to_string())?;
    if goal_contract_refs.is_empty() {
        return Err("REPLAN_GOAL_REQUIRED".into());
    }
    let current_revision = NarrativeRepository::project_revision(connection, &request.project_id)
        .map_err(|error| error.to_string())?;
    let (route_ref, route) = current_route(connection, &request.project_id, &request.volume_id)?;
    planning::validate_volume_route(&route)?;
    let (intent_ref, intent) = current_intent(connection, &request.project_id, &route)?;
    validate_route_reservations(
        connection,
        &request.project_id,
        &request.volume_id,
        &route,
        &intent,
    )?;
    let planning_state = build_planning_state(
        connection,
        &request.project_id,
        &request.volume_id,
        &request.now,
        Some(current_revision),
    )?;
    let actual_reality_digest = planning_state
        .get("stateDigest")
        .and_then(Value::as_str)
        .ok_or("REPLAN_REALITY_DIGEST_MISSING")?;
    if request
        .expected_reality_digest
        .as_deref()
        .is_some_and(|expected| expected != actual_reality_digest)
    {
        return Err("REPLAN_REALITY_STALE".into());
    }
    let responsibilities = planning_state
        .get("unresolvedResponsibilities")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let mut required_responsibilities = responsibilities
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("threadRef").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut acknowledged = request.acknowledged_responsibility_refs.clone();
    required_responsibilities.sort();
    acknowledged.sort();
    if required_responsibilities != acknowledged {
        return Err("REPLAN_RESPONSIBILITY_REQUIRED".into());
    }
    let evidence_refs = request
        .evidence_refs
        .iter()
        .map(|reference| json!({"ref":reference,"resolved":true}))
        .collect::<Vec<_>>();
    let compiled_input = json!({
        "inputSchema": "b09.route-replan-input.r1",
        "projectId": request.project_id,
        "volumeId": request.volume_id,
        "projectRevision": current_revision,
        "currentGoal": goal_contract_refs,
        "currentRoute": {"ref": route_ref, "payload": route},
        "actualReality": {
            "stateDigest": actual_reality_digest,
            "taskReality": planning_state.get("taskReality").cloned().unwrap_or_else(|| json!([])),
        },
        "planningState": planning_state,
        "unresolvedResponsibilities": responsibilities,
        "evidenceRefs": evidence_refs,
        "acknowledgedResponsibilityRefs": acknowledged,
        "candidatePolicy": "DERIVE_FROM_COMPILED_INPUT_ONLY",
    });
    let compiled_input_digest =
        sha256_canonical(&compiled_input).map_err(|error| error.to_string())?;
    Ok(json!({
        "compiledInput": compiled_input,
        "compiledInputDigest": compiled_input_digest,
        "previousRoute": route_ref,
        "intent": intent_ref,
    }))
}

fn derive_route_candidate(compiled: &Value) -> Result<(Value, i64), String> {
    let mut candidate = compiled
        .pointer("/compiledInput/currentRoute/payload")
        .cloned()
        .ok_or("REPLAN_COMPILED_ROUTE_MISSING")?;
    let previous_version = candidate
        .get("routeVersion")
        .and_then(Value::as_i64)
        .ok_or("VOLUME_PLAN_ROUTE_VERSION_REQUIRED")?;
    let route_version = previous_version + 1;
    candidate["routeVersion"] = json!(route_version);
    let mut invariants = candidate
        .get("routeInvariants")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    invariants.push(json!("B09_REPLAN_DERIVED_FROM_COMPILED_INPUT"));
    candidate["routeInvariants"] = Value::Array(invariants);
    planning::validate_volume_route(&candidate)?;
    let intent_ref: ArtifactRef = serde_json::from_value(
        compiled
            .get("intent")
            .cloned()
            .ok_or("REPLAN_INTENT_REF_MISSING")?,
    )
    .map_err(|error| error.to_string())?;
    planning::validate_volume_route_reference(&candidate, &intent_ref)?;
    Ok((candidate, route_version))
}

fn build_planning_state(
    connection: &Connection,
    project_id: &str,
    volume_id: &str,
    now: &str,
    revision_override: Option<i64>,
) -> Result<Value, String> {
    let metadata = VolumeLifecycleRepository::volume_metadata(connection, project_id, volume_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VOLUME_NOT_FOUND".to_string())?;
    let revision = revision_override.unwrap_or(
        NarrativeRepository::project_revision(connection, project_id)
            .map_err(|error| error.to_string())?,
    );
    let (route_ref, route) = current_route(connection, project_id, volume_id)?;
    planning::validate_volume_route(&route)?;
    let (intent_ref, intent) = current_intent(connection, project_id, &route)?;
    validate_route_reservations(connection, project_id, volume_id, &route, &intent)?;
    let tasks = route
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let task_ids = tasks
        .iter()
        .filter_map(|task| task.get("taskId").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let states = NarrativeRepository::task_states_by_ids(connection, project_id, &task_ids, 500)
        .map_err(|error| error.to_string())?;
    let state_map = states
        .into_iter()
        .map(|state| {
            (
                state.state_id,
                state
                    .value
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("NOT_STARTED")
                    .to_string(),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut active_task_ids = Vec::new();
    let mut blocked_task_ids = Vec::new();
    let mut required_task_ids = Vec::new();
    let mut remaining_task_ids = Vec::new();
    for task in &tasks {
        let Some(task_id) = task.get("taskId").and_then(Value::as_str) else {
            continue;
        };
        let status = state_map
            .get(task_id)
            .map(String::as_str)
            .unwrap_or("NOT_STARTED");
        if status != "SATISFIED" {
            remaining_task_ids.push(task_id.to_string());
            active_task_ids.push(task_id.to_string());
        }
        if status == "BLOCKED" {
            blocked_task_ids.push(task_id.to_string());
        }
        if task.get("class").and_then(Value::as_str) == Some("REQUIRED") {
            required_task_ids.push(task_id.to_string());
        }
    }
    let required_tasks_remaining = required_task_ids
        .iter()
        .filter(|task_id| {
            state_map
                .get(task_id.as_str())
                .map(String::as_str)
                .unwrap_or("NOT_STARTED")
                != "SATISFIED"
        })
        .count() as i64;
    let blocked_required_tasks = required_task_ids
        .iter()
        .filter(|task_id| {
            state_map
                .get(task_id.as_str())
                .map(String::as_str)
                .unwrap_or("NOT_STARTED")
                == "BLOCKED"
        })
        .count() as i64;
    let exit_condition_gap = route
        .get("requiredExitConditions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|condition| {
            if condition.get("kind").and_then(Value::as_str) == Some("TASK_SATISFIED") {
                return condition
                    .get("value")
                    .and_then(Value::as_str)
                    .map(|task_id| state_map.get(task_id).map(String::as_str) != Some("SATISFIED"))
                    .unwrap_or(true);
            }
            true
        })
        .count() as i64;
    let committed_chapters =
        VolumeLifecycleRepository::committed_chapters(connection, project_id, volume_id)
            .map_err(|error| error.to_string())?;
    let max_chapters = route
        .pointer("/capacityBudget/maxChapters")
        .and_then(Value::as_i64)
        .unwrap_or((metadata.chapter_end - metadata.chapter_start + 1).max(1));
    let remaining_chapter_capacity = (max_chapters - committed_chapters).max(0);
    let future_reservation_refs = intent
        .get("futureReservationRefs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let unresolved_thread_refs = intent
        .get("crossVolumeResponsibilityRefs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let unresolved_responsibilities = unresolved_thread_refs
        .iter()
        .filter_map(Value::as_str)
        .map(|thread_ref| {
            json!({
                "threadRef": thread_ref,
                "status": "OPEN",
                "responsibility": true,
            })
        })
        .collect::<Vec<_>>();
    let task_reality = tasks
        .iter()
        .filter_map(|task| task.get("taskId").and_then(Value::as_str))
        .map(|task_id| {
            json!({
                "taskId": task_id,
                "status": state_map.get(task_id).map(String::as_str).unwrap_or("NOT_STARTED"),
            })
        })
        .collect::<Vec<_>>();
    let reservation_pressure = future_reservation_refs.len() as i64
        + tasks
            .iter()
            .filter_map(|task| task.get("reservationRefs").and_then(Value::as_array))
            .map(Vec::len)
            .sum::<usize>() as i64;
    let planning_pressure = planning::planning_pressure(
        committed_chapters,
        max_chapters,
        required_tasks_remaining,
        blocked_required_tasks,
        exit_condition_gap,
        unresolved_thread_refs.len() as i64,
        reservation_pressure,
    );
    let state_input = json!({
        "projectId": project_id,
        "volumeId": volume_id,
        "routeRef": route_ref,
        "intentRef": intent_ref,
        "volumeStatus": metadata.status,
        "committedChapters": committed_chapters,
        "remainingTaskIds": remaining_task_ids,
        "blockedTaskIds": blocked_task_ids,
        "taskReality": task_reality,
        "requiredExitConditionGap": exit_condition_gap,
        "futureReservationRefs": future_reservation_refs,
        "unresolvedThreadRefs": unresolved_thread_refs,
        "unresolvedResponsibilities": unresolved_responsibilities,
    });
    let state_digest = sha256_canonical(&state_input).map_err(|error| error.to_string())?;
    let close_readiness = if required_tasks_remaining == 0
        && blocked_required_tasks == 0
        && exit_condition_gap == 0
    {
        "READY"
    } else {
        "BLOCKED"
    };
    let mut projection = json!({
        "projectId": project_id,
        "volumeId": volume_id,
        "generatedRevision": revision,
        "volumeStatus": metadata.status,
        "routeRef": route_ref,
        "intentRef": intent_ref,
        "routeVersion": route.get("routeVersion").cloned().unwrap_or(Value::Null),
        "activeTaskIds": active_task_ids,
        "blockedTaskIds": blocked_task_ids,
        "requiredTaskIds": required_task_ids,
        "remainingTaskIds": remaining_task_ids,
        "requiredTasksRemaining": required_tasks_remaining,
        "blockedRequiredTasks": blocked_required_tasks,
        "remainingChapterCapacity": remaining_chapter_capacity,
        "planningPressure": planning_pressure,
        "exitConditionGap": exit_condition_gap,
        "taskReality": task_reality,
        "unresolvedThreadRefs": unresolved_thread_refs,
        "unresolvedResponsibilities": unresolved_responsibilities,
        "futureReservationRefs": future_reservation_refs,
        "closeReadiness": close_readiness,
        "stateDigest": state_digest,
        "updatedAt": now,
    });
    let projection_digest = sha256_canonical(&projection).map_err(|error| error.to_string())?;
    projection["projectionDigest"] = json!(projection_digest);
    Ok(projection)
}

fn persist_planning_state(
    connection: &Connection,
    project_id: &str,
    volume_id: &str,
    projection: &Value,
    now: &str,
) -> Result<(), String> {
    let state_digest = projection
        .get("stateDigest")
        .and_then(Value::as_str)
        .ok_or("PLANNING_STATE_DIGEST_MISSING")?;
    let projection_digest = projection
        .get("projectionDigest")
        .and_then(Value::as_str)
        .ok_or("PLANNING_PROJECTION_DIGEST_MISSING")?;
    let revision = projection
        .get("generatedRevision")
        .and_then(Value::as_i64)
        .ok_or("PLANNING_PROJECTION_REVISION_MISSING")?;
    VolumeLifecycleRepository::upsert_planning_projection(
        connection,
        project_id,
        volume_id,
        revision,
        state_digest,
        projection_digest,
        &projection.to_string(),
        now,
    )
    .map_err(|error| error.to_string())
}

fn apply_route_replan(
    connection: &Connection,
    request: &RouteReplanRequest,
) -> Result<Value, String> {
    let compiled = compile_route_replan_input(connection, request)?;
    let (candidate, route_version) = derive_route_candidate(&compiled)?;
    let previous_ref: ArtifactRef = serde_json::from_value(
        compiled
            .get("previousRoute")
            .cloned()
            .ok_or("REPLAN_PREVIOUS_ROUTE_REF_MISSING")?,
    )
    .map_err(|error| error.to_string())?;
    let intent_ref: ArtifactRef = serde_json::from_value(
        compiled
            .get("intent")
            .cloned()
            .ok_or("REPLAN_INTENT_REF_MISSING")?,
    )
    .map_err(|error| error.to_string())?;
    let artifact = make_artifact(
        &request.project_id,
        &previous_ref.artifact_id,
        ArtifactType::VolumePlan,
        volume_scope(&request.volume_id),
        previous_ref.version + 1,
        candidate.clone(),
        vec![intent_ref.clone()],
        &request.command_id,
        "B09::VolumeLifecycleOwner",
        SemanticLifecycle::ApprovedPlan,
        AuthorityLevel::B,
        &request.now,
    )?;
    ArtifactRepository::insert_candidate(connection, &request.project_id, &artifact)
        .map_err(|error| error.to_string())?;
    ArtifactRepository::commit_candidate(
        connection,
        &artifact.artifact_id,
        artifact.version,
        &request.now,
    )
    .map_err(|error| error.to_string())?;
    let new_route_ref = artifact_ref(&artifact);
    let scope_key_value = scope_key(&artifact.scope).map_err(|error| error.to_string())?;
    ArtifactRepository::promote_head(
        connection,
        &request.project_id,
        &new_route_ref,
        "VolumePlan",
        "VOLUME",
        &scope_key_value,
    )
    .map_err(|error| error.to_string())?;
    let target_chapters = candidate
        .pointer("/capacityBudget/maxChapters")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    VolumeLifecycleRepository::update_plan_index(
        connection,
        &request.project_id,
        &request.volume_id,
        &artifact.artifact_id,
        artifact.version,
        target_chapters,
        &request.now,
    )
    .map_err(|error| error.to_string())?;
    let input_refs = json!({
        "candidateSource": "B09::VolumeLifecycleOwner::compiled_input",
        "compiledInput": compiled["compiledInput"],
        "compiledInputDigest": compiled["compiledInputDigest"],
        "currentGoal": compiled["compiledInput"]["currentGoal"],
        "currentRoute": compiled["compiledInput"]["currentRoute"],
        "actualReality": compiled["compiledInput"]["actualReality"],
        "planningState": compiled["compiledInput"]["planningState"],
        "unresolvedResponsibilities": compiled["compiledInput"]["unresolvedResponsibilities"],
        "evidenceRefs": compiled["compiledInput"]["evidenceRefs"],
        "previousRoute": previous_ref,
        "intent": intent_ref,
    });
    VolumeLifecycleRepository::record_route_replan(
        connection,
        &request.command_id,
        &request.project_id,
        &request.volume_id,
        Some(&previous_ref.artifact_id),
        Some(previous_ref.version),
        &artifact.artifact_id,
        artifact.version,
        &request.trigger,
        &request.reason,
        request.expected_revision,
        &input_refs.to_string(),
        &format!("project-receipt:{}", request.command_id),
        &request.now,
    )
    .map_err(|error| error.to_string())?;
    let projection = build_planning_state(
        connection,
        &request.project_id,
        &request.volume_id,
        &request.now,
        Some(request.expected_revision + 1),
    )?;
    persist_planning_state(
        connection,
        &request.project_id,
        &request.volume_id,
        &projection,
        &request.now,
    )?;
    Ok(json!({
        "replayed": false,
        "routeVersion": route_version,
        "projectRevision": request.expected_revision + 1,
        "receiptId": format!("project-receipt:{}", request.command_id),
        "routeRef": new_route_ref,
        "reason": request.reason,
        "evidenceRefs": request.evidence_refs,
        "compiledInputDigest": compiled["compiledInputDigest"],
        "compiledInput": compiled["compiledInput"],
    }))
}

fn apply_volume_close(
    connection: &Connection,
    request: &VolumeCloseRequest,
) -> Result<Value, String> {
    let metadata = VolumeLifecycleRepository::volume_metadata(
        connection,
        &request.project_id,
        &request.volume_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "VOLUME_NOT_FOUND".to_string())?;
    if metadata.status == "CLOSED" {
        return Err("VOLUME_ALREADY_CLOSED".into());
    }
    let projection = build_planning_state(
        connection,
        &request.project_id,
        &request.volume_id,
        &request.now,
        Some(request.expected_revision),
    )?;
    if projection.get("closeReadiness").and_then(Value::as_str) != Some("READY") {
        return Err("VOLUME_CLOSE_BLOCKED".into());
    }
    let (route_ref, route) = current_route(connection, &request.project_id, &request.volume_id)?;
    let (intent_ref, _) = current_intent(connection, &request.project_id, &route)?;
    let handoff_evidence = VolumeLifecycleRepository::handoff_evidence(
        connection,
        &request.project_id,
        &request.volume_id,
        20,
    )
    .map_err(|error| error.to_string())?;
    if handoff_evidence.is_empty() {
        return Err("VOLUME_CLOSE_EVIDENCE_REQUIRED".into());
    }
    let exit_artifact_id = format!("exit:{}", request.volume_id);
    let exit_version = ArtifactRepository::next_version(connection, &exit_artifact_id)
        .map_err(|error| error.to_string())?;
    let exit_payload = json!({
        "payloadSchema": "volume-exit-state.r1",
        "volumeId": request.volume_id,
        "closedAt": request.now,
        "stateDigest": projection["stateDigest"],
        "remainingTaskIds": projection["remainingTaskIds"],
        "unresolvedThreadRefs": projection["unresolvedThreadRefs"],
        "nextVolumeEligibility": "ELIGIBLE",
        "handoffEvidenceRefs": handoff_evidence,
    });
    let exit_artifact = make_artifact(
        &request.project_id,
        &exit_artifact_id,
        ArtifactType::VolumeExitState,
        volume_scope(&request.volume_id),
        exit_version,
        exit_payload,
        vec![intent_ref, route_ref],
        &request.command_id,
        "B09::VolumeLifecycleOwner",
        SemanticLifecycle::Materialized,
        AuthorityLevel::B,
        &request.now,
    )?;
    ArtifactRepository::insert_candidate(connection, &request.project_id, &exit_artifact)
        .map_err(|error| error.to_string())?;
    if request.fault_injection.as_deref() == Some("after_exit_artifact") {
        return Err("VOLUME_CLOSE_FAULT_INJECTED".into());
    }
    ArtifactRepository::commit_candidate(
        connection,
        &exit_artifact.artifact_id,
        exit_artifact.version,
        &request.now,
    )
    .map_err(|error| error.to_string())?;
    let exit_ref = artifact_ref(&exit_artifact);
    let exit_scope_key = scope_key(&exit_artifact.scope).map_err(|error| error.to_string())?;
    ArtifactRepository::promote_head(
        connection,
        &request.project_id,
        &exit_ref,
        "VolumeExitState",
        "VOLUME",
        &exit_scope_key,
    )
    .map_err(|error| error.to_string())?;
    if !VolumeLifecycleRepository::mark_closed(
        connection,
        &request.project_id,
        &request.volume_id,
        &request.now,
    )
    .map_err(|error| error.to_string())?
    {
        return Err("VOLUME_CLOSE_STATUS_UPDATE_FAILED".into());
    }
    VolumeLifecycleRepository::update_planning_ledger(
        connection,
        &request.project_id,
        &request.volume_id,
        "COMPLETE",
        "NONE",
        &request.now,
    )
    .map_err(|error| error.to_string())?;
    VolumeLifecycleRepository::record_volume_close(
        connection,
        &request.command_id,
        &request.project_id,
        &request.volume_id,
        &exit_artifact.artifact_id,
        exit_artifact.version,
        request.expected_revision,
        "ELIGIBLE",
        "[]",
        &exit_artifact
            .payload
            .get("handoffEvidenceRefs")
            .cloned()
            .unwrap_or_else(|| json!([]))
            .to_string(),
        &format!("project-receipt:{}", request.command_id),
        &request.now,
    )
    .map_err(|error| error.to_string())?;
    let closed_projection = build_planning_state(
        connection,
        &request.project_id,
        &request.volume_id,
        &request.now,
        Some(request.expected_revision + 1),
    )?;
    persist_planning_state(
        connection,
        &request.project_id,
        &request.volume_id,
        &closed_projection,
        &request.now,
    )?;
    Ok(json!({
        "replayed": false,
        "volumeId": request.volume_id,
        "exitRef": exit_ref,
        "nextVolumeEligibility": "ELIGIBLE",
        "projectRevision": request.expected_revision + 1,
        "receiptId": format!("project-receipt:{}", request.command_id),
    }))
}

fn apply_next_volume_handoff(
    connection: &Connection,
    request: &NextVolumeHandoffRequest,
) -> Result<Value, String> {
    let current = VolumeLifecycleRepository::volume_metadata(
        connection,
        &request.project_id,
        &request.current_volume_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "CURRENT_VOLUME_NOT_FOUND".to_string())?;
    if current.status != "CLOSED" {
        return Err("NEXT_VOLUME_BLOCKED_CURRENT_NOT_CLOSED".into());
    }
    if VolumeLifecycleRepository::close_eligibility(
        connection,
        &request.project_id,
        &request.current_volume_id,
    )
    .map_err(|error| error.to_string())?
    .as_deref()
        != Some("ELIGIBLE")
    {
        return Err("NEXT_VOLUME_BLOCKED_ELIGIBILITY".into());
    }
    if VolumeLifecycleRepository::volume_metadata(
        connection,
        &request.project_id,
        &request.next_volume_id,
    )
    .map_err(|error| error.to_string())?
    .is_some()
    {
        return Err("NEXT_VOLUME_ALREADY_EXISTS".into());
    }
    let (current_route_ref, current_route) =
        current_route(connection, &request.project_id, &request.current_volume_id)?;
    let (current_intent_ref, current_intent_payload) =
        current_intent(connection, &request.project_id, &current_route)?;
    validate_route_reservations(
        connection,
        &request.project_id,
        &request.current_volume_id,
        &current_route,
        &current_intent_payload,
    )?;
    let current_planning_state = build_planning_state(
        connection,
        &request.project_id,
        &request.current_volume_id,
        &request.now,
        Some(request.expected_revision),
    )?;
    let goal_contract_refs = NarrativeRepository::goal_contract_refs_for_volume(
        connection,
        &request.project_id,
        &request.current_volume_id,
        100,
    )
    .map_err(|error| error.to_string())?;
    let canon_facts =
        VolumeLifecycleRepository::current_canon_facts(connection, &request.project_id, 100)
            .map_err(|error| error.to_string())?;
    let exit_scope_key =
        scope_key(&volume_scope(&request.current_volume_id)).map_err(|error| error.to_string())?;
    let current_exit_ref = NarrativeRepository::canonical_head_ref(
        connection,
        &request.project_id,
        "VolumeExitState",
        &exit_scope_key,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "VOLUME_EXIT_STATE_NOT_FOUND".to_string())?;
    let current_exit_payload = NarrativeRepository::artifact_payload_by_ref(
        connection,
        &request.project_id,
        &current_exit_ref,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "VOLUME_EXIT_STATE_PAYLOAD_NOT_FOUND".to_string())?;
    let compiled_input = json!({
        "inputSchema": "b09.next-volume-input.r1",
        "projectId": request.project_id,
        "fromVolumeId": request.current_volume_id,
        "toVolumeId": request.next_volume_id,
        "projectRevision": request.expected_revision,
        "currentGoal": goal_contract_refs.clone(),
        "currentRoute": current_route_ref.clone(),
        "canonFacts": canon_facts.clone(),
        "actualReality": current_planning_state.clone(),
        "capacity": current_planning_state.get("planningPressure").cloned().unwrap_or_else(|| json!({})),
        "unresolvedResponsibilities": current_intent_payload.get("crossVolumeResponsibilityRefs").cloned().unwrap_or_else(|| json!([])),
        "futureReservations": current_intent_payload.get("futureReservationRefs").cloned().unwrap_or_else(|| json!([])),
        "volumeExitState": {"ref": current_exit_ref.clone(), "payload": current_exit_payload.clone()},
        "exactEvidenceRefs": current_exit_payload.get("handoffEvidenceRefs").cloned().unwrap_or_else(|| json!([])),
    });
    let compiled_input_digest =
        sha256_canonical(&compiled_input).map_err(|error| error.to_string())?;
    let next_intent_payload = compile_next_volume_intent(
        &request.next_volume_id,
        &request.title,
        &request.current_volume_id,
        request.chapter_start,
        request.chapter_end,
        request.target_words,
        &current_intent_payload,
    )?;
    let next_task_ids = next_intent_payload
        .get("mustAchieve")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .map(|id| format!("task:{id}"))
        .collect::<Vec<_>>();
    let next_route_payload = compile_next_volume_route(
        &next_intent_payload,
        &next_task_ids,
        request.chapter_start,
        request.chapter_end,
        request.target_words,
    )?;
    VolumeLifecycleRepository::insert_next_volume(
        connection,
        &request.project_id,
        &request.next_volume_id,
        &current.book_id,
        current.volume_no + 1,
        &request.title,
        request.chapter_start,
        request.chapter_end,
        request.target_words,
        &request.now,
    )
    .map_err(|error| error.to_string())?;
    let next_intent_id = format!("intent:{}", request.next_volume_id);
    let next_intent = make_artifact(
        &request.project_id,
        &next_intent_id,
        ArtifactType::VolumeIntent,
        volume_scope(&request.next_volume_id),
        1,
        next_intent_payload.clone(),
        vec![current_intent_ref.clone(), current_exit_ref.clone()],
        &request.command_id,
        "B09::VolumeLifecycleOwner",
        SemanticLifecycle::ApprovedPlan,
        AuthorityLevel::B,
        &request.now,
    )?;
    ArtifactRepository::insert_candidate(connection, &request.project_id, &next_intent)
        .map_err(|error| error.to_string())?;
    ArtifactRepository::commit_candidate(
        connection,
        &next_intent.artifact_id,
        next_intent.version,
        &request.now,
    )
    .map_err(|error| error.to_string())?;
    promote_volume_head(
        connection,
        &request.project_id,
        &next_intent,
        "VolumeIntent",
    )?;

    let next_intent_ref = artifact_ref(&next_intent);
    let mut next_route_payload = next_route_payload;
    next_route_payload["volumeIntentRef"] =
        serde_json::to_value(&next_intent_ref).map_err(|error| error.to_string())?;
    planning::validate_volume_route(&next_route_payload)?;
    planning::validate_volume_route_reference(&next_route_payload, &next_intent_ref)?;
    let next_plan_id = format!("plan:{}", request.next_volume_id);
    let next_plan = make_artifact(
        &request.project_id,
        &next_plan_id,
        ArtifactType::VolumePlan,
        volume_scope(&request.next_volume_id),
        1,
        next_route_payload.clone(),
        vec![
            next_intent_ref.clone(),
            current_route_ref.clone(),
            current_exit_ref.clone(),
        ],
        &request.command_id,
        "B09::VolumeLifecycleOwner",
        SemanticLifecycle::ApprovedPlan,
        AuthorityLevel::B,
        &request.now,
    )?;
    ArtifactRepository::insert_candidate(connection, &request.project_id, &next_plan)
        .map_err(|error| error.to_string())?;
    ArtifactRepository::commit_candidate(
        connection,
        &next_plan.artifact_id,
        next_plan.version,
        &request.now,
    )
    .map_err(|error| error.to_string())?;
    promote_volume_head(connection, &request.project_id, &next_plan, "VolumePlan")?;
    let target_chapters = next_route_payload
        .pointer("/capacityBudget/maxChapters")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    VolumeLifecycleRepository::update_plan_index(
        connection,
        &request.project_id,
        &request.next_volume_id,
        &next_plan.artifact_id,
        next_plan.version,
        target_chapters,
        &request.now,
    )
    .map_err(|error| error.to_string())?;
    VolumeLifecycleRepository::update_planning_ledger(
        connection,
        &request.project_id,
        &request.next_volume_id,
        "INCOMPLETE",
        "NONE",
        &request.now,
    )
    .map_err(|error| error.to_string())?;
    let next_projection = build_planning_state(
        connection,
        &request.project_id,
        &request.next_volume_id,
        &request.now,
        Some(request.expected_revision + 1),
    )?;
    persist_planning_state(
        connection,
        &request.project_id,
        &request.next_volume_id,
        &next_projection,
        &request.now,
    )?;
    let next_plan_ref = artifact_ref(&next_plan);
    let input_refs = json!({
        "candidateSource": "B09::VolumeLifecycleOwner::next_volume_compiler",
        "compiledInput": compiled_input,
        "compiledInputDigest": compiled_input_digest,
        "goalContractRefs": goal_contract_refs,
        "canonFacts": canon_facts,
        "currentPlanningState": current_planning_state,
        "inheritedRefs": {
            "currentIntent": current_intent_ref,
            "currentRoute": current_route_ref,
            "currentExit": current_exit_ref,
        },
        "newIntent": next_intent_ref,
        "newRoute": next_plan_ref,
        "continuityRefs": next_intent_payload.get("crossVolumeResponsibilityRefs").cloned().unwrap_or_else(|| json!([])),
        "futureReservationRefs": next_intent_payload.get("futureReservationRefs").cloned().unwrap_or_else(|| json!([])),
    });
    let handoff_id = request.command_id.clone();
    VolumeLifecycleRepository::record_handoff(
        connection,
        &handoff_id,
        &request.project_id,
        &request.current_volume_id,
        &request.next_volume_id,
        &current_exit_ref.artifact_id,
        &input_refs.to_string(),
        &format!("project-receipt:{}", request.command_id),
        &request.now,
    )
    .map_err(|error| error.to_string())?;
    Ok(json!({
        "replayed": false,
        "eligibility": "ELIGIBLE",
        "fromVolumeId": request.current_volume_id,
        "toVolumeId": request.next_volume_id,
        "projectRevision": request.expected_revision + 1,
        "receiptId": format!("project-receipt:{}", request.command_id),
        "compiledInputDigest": compiled_input_digest,
        "inputRefs": input_refs,
    }))
}

fn compile_next_volume_intent(
    next_volume_id: &str,
    title: &str,
    previous_volume_id: &str,
    chapter_start: i64,
    chapter_end: i64,
    target_words: i64,
    current_intent: &Value,
) -> Result<Value, String> {
    let responsibilities = current_intent
        .get("crossVolumeResponsibilityRefs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let must_achieve = if responsibilities.is_empty() {
        vec![json!({
            "id": format!("goal:{next_volume_id}:entry"),
            "description": "建立下一卷入口并推进当前主线",
        })]
    } else {
        responsibilities
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                item.as_str().map(|responsibility| {
                    json!({
                        "id": format!("goal:{next_volume_id}:responsibility:{index}"),
                        "description": format!("在下一卷继续推进跨卷责任 {responsibility}"),
                    })
                })
            })
            .collect()
    };
    let task_ids = must_achieve
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .map(|id| format!("task:{id}"))
        .collect::<Vec<_>>();
    let required_exit_conditions = task_ids
        .iter()
        .map(|task_id| json!({"kind":"TASK_SATISFIED","value":task_id}))
        .collect::<Vec<_>>();
    let payload = json!({
        "payloadSchema": "volume-intent.r1",
        "volumeFunction": format!("{title}：新入口与跨卷责任推进"),
        "entryConditions": [{"kind":"PREVIOUS_VOLUME_CLOSED","value":previous_volume_id}],
        "mustAchieve": must_achieve,
        "mustNotBreak": current_intent.get("mustNotBreak").cloned().unwrap_or_else(|| json!([])),
        "requiredExitConditions": required_exit_conditions,
        "futureReservationRefs": current_intent.get("futureReservationRefs").cloned().unwrap_or_else(|| json!([])),
        "crossVolumeResponsibilityRefs": current_intent.get("crossVolumeResponsibilityRefs").cloned().unwrap_or_else(|| json!([])),
        "targetChapterRange": {"min":1,"preferred":(chapter_end - chapter_start + 1).max(1),"max":(chapter_end - chapter_start + 1).max(1)},
        "targetWordRange": {"preferred":target_words.max(1)},
    });
    planning::validate_volume_intent(&payload)?;
    Ok(payload)
}

fn compile_next_volume_route(
    intent: &Value,
    task_ids: &[String],
    chapter_start: i64,
    chapter_end: i64,
    _target_words: i64,
) -> Result<Value, String> {
    let goals = intent
        .get("mustAchieve")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let responsibilities = intent
        .get("crossVolumeResponsibilityRefs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tasks = task_ids
        .iter()
        .enumerate()
        .map(|(index, task_id)| {
            let objective = goals
                .get(index)
                .and_then(|item| item.get("description"))
                .and_then(Value::as_str)
                .unwrap_or("推进下一卷入口");
            let continuity_refs = responsibilities
                .get(index)
                .cloned()
                .map(|value| vec![value])
                .unwrap_or_default();
            json!({
                "taskId": task_id,
                "class": "REQUIRED",
                "objective": objective,
                "dependsOnTaskIds": [],
                "continuityThreadRefs": continuity_refs,
                "reservationRefs": [],
                "completionPredicate": {"kind":"TASK_SATISFIED","value":task_id},
                "estimatedChapterCost": {"preferred":1},
            })
        })
        .collect::<Vec<_>>();
    let required_exit_conditions = task_ids
        .iter()
        .map(|task_id| json!({"kind":"TASK_SATISFIED","value":task_id}))
        .collect::<Vec<_>>();
    let span = (chapter_end - chapter_start + 1).max(1);
    let payload = json!({
        "payloadSchema": "volume-plan.r1",
        "volumeIntentRef": Value::Null,
        "routeVersion": 1,
        "tasks": tasks,
        "routeInvariants": ["B09_NEXT_VOLUME_ROUTE_DERIVED"],
        "requiredExitConditions": required_exit_conditions,
        "capacityBudget": {"minChapters":1,"preferredChapters":span,"maxChapters":span},
    });
    Ok(payload)
}

#[allow(clippy::too_many_arguments)]
fn make_artifact(
    project_id: &str,
    artifact_id: &str,
    artifact_type: ArtifactType,
    scope: ArtifactScope,
    version: i64,
    payload: Value,
    dependency_refs: Vec<ArtifactRef>,
    command_id: &str,
    created_by: &str,
    semantic_lifecycle: SemanticLifecycle,
    authority_level: AuthorityLevel,
    now: &str,
) -> Result<ArtifactEnvelope, String> {
    let payload_hash = sha256_canonical(&payload).map_err(|error| error.to_string())?;
    Ok(ArtifactEnvelope {
        artifact_id: artifact_id.to_string(),
        project_id: project_id.to_string(),
        artifact_type,
        scope,
        version,
        schema_version: "m0a.artifact.v1".into(),
        artifact_status: ArtifactStatus::Candidate,
        semantic_lifecycle: Some(semantic_lifecycle),
        authority_level: Some(authority_level),
        payload,
        payload_hash,
        dependency_refs,
        provenance: ArtifactEnvelopeProvenance {
            created_by: created_by.to_string(),
            command_id: command_id.to_string(),
            job_id: None,
            step_id: None,
            model_run_id: None,
            prompt_ref: None,
            context_manifest_ref: None,
            decision_refs: None,
            gate_certificate_refs: None,
        },
        created_at: now.to_string(),
        committed_at: None,
    })
}

fn volume_scope(volume_id: &str) -> ArtifactScope {
    ArtifactScope {
        level: ArtifactScopeLevel::Volume,
        book_id: None,
        volume_id: Some(volume_id.to_string()),
        arc_id: None,
        chapter_id: None,
        scene_id: None,
        entity_id: None,
        platform_id: None,
    }
}

fn artifact_ref(artifact: &ArtifactEnvelope) -> ArtifactRef {
    ArtifactRef {
        artifact_id: artifact.artifact_id.clone(),
        version: artifact.version,
        payload_hash: artifact.payload_hash.clone(),
    }
}

fn promote_volume_head(
    connection: &Connection,
    project_id: &str,
    artifact: &ArtifactEnvelope,
    artifact_type: &str,
) -> Result<(), String> {
    let reference = artifact_ref(artifact);
    let scope_key_value = scope_key(&artifact.scope).map_err(|error| error.to_string())?;
    ArtifactRepository::promote_head(
        connection,
        project_id,
        &reference,
        artifact_type,
        "VOLUME",
        &scope_key_value,
    )
    .map_err(|error| error.to_string())
}
