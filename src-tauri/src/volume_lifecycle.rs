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

#[derive(Debug, Clone)]
pub struct VolumeChangeMutation {
    pub artifact: ArtifactEnvelope,
    pub head: crate::generated::types::HeadChange,
    pub owner_receipt: Value,
}

/// B09's semantic adapter for B12 planning targets.  This validates the
/// candidate against the current volume topology and its existing intent
/// owner before B12 materializes the versioned authority bundle.
pub fn validate_change_target_owner(
    connection: &Connection,
    project_id: &str,
    artifact_type: &str,
    candidate: &Value,
    scope: &Value,
) -> Result<(), String> {
    let volume_id = scope
        .get("volumeId")
        .and_then(Value::as_str)
        .ok_or("B09_VOLUME_SCOPE_REQUIRED")?;
    if !NarrativeRepository::volume_exists(connection, project_id, volume_id)
        .map_err(|error| error.to_string())?
    {
        return Err("VOLUME_NOT_FOUND".into());
    }
    match artifact_type {
        "VolumeIntent" => {
            if !NarrativeRepository::approved_book_plan_exists(connection, project_id)
                .map_err(|error| error.to_string())?
            {
                return Err("BOOK_PLAN_APPROVED_REQUIRED".into());
            }
            planning::validate_volume_intent(candidate)
        }
        "VolumePlan" => {
            planning::validate_volume_route(candidate)?;
            let intent = NarrativeRepository::canonical_head_ref(
                connection,
                project_id,
                "VolumeIntent",
                volume_id,
            )
            .map_err(|error| error.to_string())?
            .ok_or("VOLUME_INTENT_NOT_FOUND")?;
            planning::validate_volume_route_reference(candidate, &intent)
        }
        _ => Err(format!("B12_CHANGE_TARGET_UNSUPPORTED:{artifact_type}")),
    }
}

fn merge_change_patch(target: &mut Value, patch: &Value) {
    match (target.as_object_mut(), patch.as_object()) {
        (Some(target), Some(patch)) => {
            for (key, value) in patch {
                if value.is_null() {
                    target.remove(key);
                } else {
                    merge_change_patch(target.entry(key.clone()).or_insert(Value::Null), value);
                }
            }
        }
        _ => *target = patch.clone(),
    }
}

impl VolumeLifecycleOwner {
    /// Applies one approved typed VolumeIntent/VolumePlan operation without
    /// committing it. The returned mutation is consumed by the outer B12
    /// CommitBundle, while all volume topology and route semantics stay here.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_typed_change(
        connection: &Connection,
        project_id: &str,
        artifact_type: &str,
        artifact_id: &str,
        expected_version: i64,
        expected_payload_hash: &str,
        expected_scope: &Value,
        patch: &Value,
        decision_id: &str,
        command_id: &str,
        now: &str,
    ) -> Result<VolumeChangeMutation, String> {
        if !matches!(artifact_type, "VolumeIntent" | "VolumePlan") {
            return Err(format!("B12_CHANGE_TARGET_UNSUPPORTED:{artifact_type}"));
        }
        if !patch.is_object() {
            return Err("B12_CHANGE_TARGET_PATCH_REQUIRED".into());
        }
        let volume_id = expected_scope
            .get("volumeId")
            .and_then(Value::as_str)
            .ok_or("B09_VOLUME_SCOPE_REQUIRED")?;
        if !NarrativeRepository::volume_exists(connection, project_id, volume_id)
            .map_err(|error| error.to_string())?
        {
            return Err("VOLUME_NOT_FOUND".into());
        }
        let current = crate::repositories::change_retcon::load_authority_head(
            connection,
            project_id,
            Some(artifact_type),
            Some(artifact_id),
            None,
            None,
        )
        .map_err(|error| format!("B09_DB:{error}"))?
        .ok_or_else(|| "B09_CHANGE_TARGET_NOT_FOUND".to_string())?;
        let current_scope: Value = serde_json::from_str(&current.scope_key)
            .map_err(|error| format!("B09_CHANGE_SCOPE_INVALID:{error}"))?;
        if sha256_canonical(&current_scope).map_err(|error| error.to_string())?
            != sha256_canonical(expected_scope).map_err(|error| error.to_string())?
        {
            return Err("B09_CHANGE_SCOPE_STALE".into());
        }
        if current.version != expected_version || current.payload_hash != expected_payload_hash {
            return Err("B09_CHANGE_TARGET_STALE".into());
        }
        let current_hash = sha256_canonical(&current.payload).map_err(|error| error.to_string())?;
        if current_hash != current.payload_hash {
            return Err("B09_CHANGE_HEAD_PAYLOAD_MISMATCH".into());
        }
        let mut candidate = current.payload.clone();
        merge_change_patch(&mut candidate, patch);
        validate_change_target_owner(
            connection,
            project_id,
            artifact_type,
            &candidate,
            expected_scope,
        )?;
        let artifact_kind = match artifact_type {
            "VolumeIntent" => ArtifactType::VolumeIntent,
            "VolumePlan" => ArtifactType::VolumePlan,
            _ => unreachable!("artifact type checked above"),
        };
        let scope: ArtifactScope = serde_json::from_value(expected_scope.clone())
            .map_err(|error| format!("B09_CHANGE_SCOPE_INVALID:{error}"))?;
        let mut artifact = make_artifact(
            project_id,
            artifact_id,
            artifact_kind,
            scope.clone(),
            current.version + 1,
            candidate,
            Vec::new(),
            command_id,
            "B09::VolumeLifecycleOwner",
            SemanticLifecycle::ApprovedPlan,
            AuthorityLevel::B,
            now,
        )?;
        artifact.provenance.decision_refs = Some(vec![decision_id.to_string()]);
        let head: crate::generated::types::HeadChange = serde_json::from_value(json!({
            "artifactType":artifact_type,
            "scope":scope,
            "artifactId":artifact.artifact_id,
            "version":artifact.version,
            "payloadHash":artifact.payload_hash
        }))
        .map_err(|error| format!("B09_HEAD:{error}"))?;
        let owner_receipt = json!({
            "owner":"B09::VolumeLifecycleOwner",
            "operation":"apply_typed_change",
            "artifactType":artifact_type,
            "artifactId":artifact_id,
            "volumeId":volume_id,
            "sourceHead":{"version":current.version,"payloadHash":current.payload_hash},
            "resultHead":{"version":artifact.version,"payloadHash":artifact.payload_hash},
            "invocationCount":1,
            "routeIntentReferenceValidated":artifact_type == "VolumePlan",
            "topologyValidated":true,
            "semanticValidation":"PASS"
        });
        Ok(VolumeChangeMutation {
            artifact,
            head,
            owner_receipt,
        })
    }

    pub fn rebuild_planning_state(
        connection: &Connection,
        project_id: &str,
        volume_id: &str,
        now: &str,
    ) -> Result<Value, String> {
        VolumeLifecycleRepository::with_planning_health_refresh(connection, |connection| {
            let projection = build_planning_state(connection, project_id, volume_id, now, None)?;
            persist_planning_state(connection, project_id, volume_id, &projection, now)?;
            crate::repositories::health::HealthRepository::complete_b09_rebuild_pending(
                connection, project_id, volume_id,
            )
            .map_err(|error| error.to_string())?;
            crate::repositories::health::HealthRepository::flush_dirty(connection, project_id, now)
                .map_err(|error| error.to_string())?;
            Ok(projection)
        })
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

fn resolved_goal_contracts(
    connection: &Connection,
    project_id: &str,
    refs: &[ArtifactRef],
) -> Result<Vec<Value>, String> {
    refs.iter()
        .map(|reference| {
            let payload =
                NarrativeRepository::artifact_payload_by_ref(connection, project_id, reference)
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("REPLAN_GOAL_NOT_FOUND:{}", reference.artifact_id))?;
            Ok(json!({"ref": reference, "payload": payload}))
        })
        .collect()
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
    let goal_contracts =
        resolved_goal_contracts(connection, &request.project_id, &goal_contract_refs)?;
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
        "goalContracts": goal_contracts,
        "currentIntent": {"ref": intent_ref.clone(), "payload": intent},
        "currentRoute": {"ref": route_ref.clone(), "payload": route},
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

fn responsibility_refs_from_input(input: &Value) -> Vec<String> {
    let mut refs = BTreeSet::new();
    if let Some(items) = input
        .get("unresolvedResponsibilities")
        .and_then(Value::as_array)
    {
        for item in items {
            if let Some(reference) = item
                .as_str()
                .or_else(|| item.get("threadRef").and_then(Value::as_str))
            {
                refs.insert(reference.to_string());
            }
        }
    }
    refs.into_iter().collect()
}

fn compiled_responsibility_refs(compiled: &Value) -> Vec<String> {
    compiled
        .get("compiledInput")
        .map(responsibility_refs_from_input)
        .unwrap_or_else(|| responsibility_refs_from_input(compiled))
}

fn next_volume_goal_constraints(compiled_input: &Value) -> Vec<Value> {
    let current_intent_id = compiled_input
        .pointer("/currentIntent/ref/artifactId")
        .and_then(Value::as_str);
    let mut goals = Vec::new();
    if let Some(contracts) = compiled_input
        .get("goalContracts")
        .and_then(Value::as_array)
    {
        for contract in contracts {
            if contract.pointer("/ref/artifactId").and_then(Value::as_str) == current_intent_id {
                continue;
            }
            if let Some(items) = contract
                .get("payload")
                .and_then(|payload| payload.get("mustAchieve"))
                .and_then(Value::as_array)
            {
                goals.extend(items.iter().cloned());
            }
        }
    }
    goals
}

fn compiled_goal_descriptions(compiled: &Value) -> Vec<(String, String)> {
    let mut goals = Vec::new();
    if let Some(contracts) = compiled
        .pointer("/compiledInput/goalContracts")
        .and_then(Value::as_array)
    {
        for contract in contracts {
            if let Some(items) = contract
                .get("payload")
                .and_then(|payload| payload.get("mustAchieve"))
                .and_then(Value::as_array)
            {
                for item in items {
                    let Some(id) = item.get("id").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some(description) = item.get("description").and_then(Value::as_str) else {
                        continue;
                    };
                    goals.push((id.to_string(), description.to_string()));
                }
            }
        }
    }
    goals
}

fn compiled_task_status(compiled: &Value, task_id: &str) -> String {
    compiled
        .pointer("/compiledInput/actualReality/taskReality")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|task| task.get("taskId").and_then(Value::as_str) == Some(task_id))
        .and_then(|task| task.get("status").and_then(Value::as_str))
        .unwrap_or("NOT_STARTED")
        .to_string()
}

fn derived_route_capacity(current: &Value, compiled: &Value) -> Value {
    let Some(current_budget) = current.get("capacityBudget").and_then(Value::as_object) else {
        return json!({});
    };
    let current_min = current_budget
        .get("minChapters")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let current_preferred = current_budget
        .get("preferredChapters")
        .and_then(Value::as_i64)
        .unwrap_or(current_min);
    let current_max = current_budget
        .get("maxChapters")
        .and_then(Value::as_i64)
        .unwrap_or(current_preferred);
    let remaining = compiled
        .pointer("/compiledInput/planningState/remainingChapterCapacity")
        .and_then(Value::as_i64)
        .or_else(|| {
            compiled
                .pointer("/compiledInput/planningState/planningPressure/remainingChapterCapacity")
                .and_then(Value::as_i64)
        })
        .unwrap_or(current_max);
    let derived_max = current_max.min(remaining.saturating_add(1).max(1)).max(1);
    let derived_preferred = current_preferred.min(derived_max).max(1);
    let derived_min = current_min.min(derived_preferred).max(1);
    json!({
        "minChapters": derived_min,
        "preferredChapters": derived_preferred,
        "maxChapters": derived_max,
    })
}

fn route_semantics_equal(left: &Value, right: &Value) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    if let Some(object) = left.as_object_mut() {
        object.remove("routeVersion");
        object.remove("routeInvariants");
    }
    if let Some(object) = right.as_object_mut() {
        object.remove("routeVersion");
        object.remove("routeInvariants");
    }
    left == right
}

fn derive_route_candidate(compiled: &Value) -> Result<(Value, i64, bool), String> {
    let current_route = compiled
        .pointer("/compiledInput/currentRoute/payload")
        .cloned()
        .ok_or("REPLAN_COMPILED_ROUTE_MISSING")?;
    let previous_route = current_route.clone();
    let previous_version = current_route
        .get("routeVersion")
        .and_then(Value::as_i64)
        .ok_or("VOLUME_PLAN_ROUTE_VERSION_REQUIRED")?;
    let route_version = previous_version + 1;

    let responsibilities = compiled_responsibility_refs(compiled);
    let goals = compiled_goal_descriptions(compiled);
    let had_current_tasks = current_route
        .get("tasks")
        .and_then(Value::as_array)
        .is_some_and(|tasks| !tasks.is_empty());
    let mut candidate = json!({
        "payloadSchema": current_route
            .get("payloadSchema")
            .cloned()
            .unwrap_or_else(|| json!("volume-plan.r1")),
        "volumeIntentRef": current_route
            .get("volumeIntentRef")
            .cloned()
            .ok_or("VOLUME_PLAN_INTENT_REF_REQUIRED")?,
        "routeVersion": route_version,
        "tasks": [],
        "routeInvariants": current_route
            .get("routeInvariants")
            .cloned()
            .unwrap_or_else(|| json!([])),
        "requiredExitConditions": current_route
            .get("requiredExitConditions")
            .cloned()
            .unwrap_or_else(|| json!([])),
        "capacityBudget": current_route
            .get("capacityBudget")
            .cloned()
            .ok_or("VOLUME_PLAN_CAPACITY_REQUIRED")?,
    });
    if let Some(climax_reservation) = current_route.get("climaxReservation") {
        candidate["climaxReservation"] = climax_reservation.clone();
    }
    let mut tasks = Vec::new();
    for task in current_route
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let task_id = task
            .get("taskId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if compiled_task_status(compiled, &task_id) == "SATISFIED" {
            continue;
        }
        let mut task = task;
        let mut continuity = task
            .get("continuityThreadRefs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut continuity_refs = continuity
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect::<BTreeSet<_>>();
        for reference in &responsibilities {
            if continuity_refs.insert(reference.clone()) {
                continuity.push(json!(reference));
            }
        }
        task["continuityThreadRefs"] = Value::Array(continuity);
        let original_objective = task
            .get("objective")
            .and_then(Value::as_str)
            .unwrap_or("推进当前路线")
            .to_string();
        let status = compiled_task_status(compiled, &task_id);
        let mut objective = original_objective;
        if status == "BLOCKED" && !objective.starts_with("解除阻塞后：") {
            objective = format!("解除阻塞后：{objective}");
        }
        if let Some((_, description)) = goals.first() {
            if !objective.contains(description) {
                objective = format!("{objective}（目标：{description}）");
            }
        }
        task["objective"] = json!(objective);
        tasks.push(task);
    }
    if tasks.is_empty() && !had_current_tasks {
        for (id, description) in &goals {
            tasks.push(json!({
                "taskId": format!("task:replan:{id}"),
                "class": "REQUIRED",
                "objective": description,
                "dependsOnTaskIds": [],
                "continuityThreadRefs": responsibilities.clone(),
                "reservationRefs": [],
                "completionPredicate": {"kind":"TASK_SATISFIED","value":format!("task:replan:{id}")},
                "estimatedChapterCost": {"preferred":1},
            }));
        }
    }
    candidate["tasks"] = Value::Array(tasks.clone());
    let mut exit_conditions = candidate
        .get("requiredExitConditions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|condition| {
            condition.get("kind").and_then(Value::as_str) != Some("TASK_SATISFIED")
                || condition
                    .get("value")
                    .and_then(Value::as_str)
                    .is_none_or(|task_id| compiled_task_status(compiled, task_id) != "SATISFIED")
        })
        .collect::<Vec<_>>();
    let exit_task_ids = exit_conditions
        .iter()
        .filter_map(|condition| {
            (condition.get("kind").and_then(Value::as_str) == Some("TASK_SATISFIED"))
                .then(|| condition.get("value").and_then(Value::as_str))
                .flatten()
                .map(ToOwned::to_owned)
        })
        .collect::<BTreeSet<_>>();
    for task in &tasks {
        let Some(task_id) = task.get("taskId").and_then(Value::as_str) else {
            continue;
        };
        if !exit_task_ids.contains(task_id) {
            exit_conditions.push(json!({"kind":"TASK_SATISFIED","value":task_id}));
        }
    }
    candidate["requiredExitConditions"] = Value::Array(exit_conditions);
    candidate["capacityBudget"] = derived_route_capacity(&candidate, compiled);
    candidate["routeVersion"] = json!(route_version);
    let mut invariants = candidate
        .get("routeInvariants")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for invariant in [
        "B09_REPLAN_DERIVED_FROM_COMPILED_INPUT",
        "B09_REPLAN_INPUT_DRIVEN",
    ] {
        if !invariants
            .iter()
            .any(|item| item.as_str() == Some(invariant))
        {
            invariants.push(json!(invariant));
        }
    }
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
    Ok((
        candidate.clone(),
        route_version,
        !route_semantics_equal(&previous_route, &candidate),
    ))
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
    let compiled_state_digest =
        if let Some(digest) = route.get("compiledFromStateDigest").and_then(Value::as_str) {
            Some(digest.to_string())
        } else {
            VolumeLifecycleRepository::route_compiled_state_digest(
                connection,
                project_id,
                volume_id,
                &route_ref.artifact_id,
                route_ref.version,
            )
            .map_err(|error| error.to_string())?
        };
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
    let states =
        NarrativeRepository::task_states_by_ids_complete(connection, project_id, &task_ids)
            .map_err(|error| error.to_string())?;
    let mut state_map = BTreeMap::new();
    for state in states {
        if state
            .value
            .get("taskId")
            .and_then(Value::as_str)
            .is_some_and(|task_id| task_id != state.state_id)
        {
            return Err(format!(
                "PLANNING_TASK_STATE_ID_MISMATCH:{}",
                state.state_id
            ));
        }
        let status = state
            .value
            .get("status")
            .and_then(Value::as_str)
            .filter(|status| {
                matches!(
                    *status,
                    "NOT_STARTED" | "ACTIVE" | "BLOCKED" | "SATISFIED" | "INVALIDATED"
                )
            })
            .ok_or_else(|| format!("PLANNING_TASK_STATE_STATUS_UNAVAILABLE:{}", state.state_id))?;
        state_map.insert(state.state_id, status.to_string());
    }

    let mut active_task_ids = Vec::new();
    let mut blocked_task_ids = Vec::new();
    let mut required_task_ids = Vec::new();
    let mut health_required_task_ids = Vec::new();
    let mut health_optional_task_ids = Vec::new();
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
        match task.get("class").and_then(Value::as_str) {
            Some("REQUIRED") => {
                required_task_ids.push(task_id.to_string());
                health_required_task_ids.push(task_id.to_string());
            }
            Some("CLIMAX" | "RESERVED") => health_required_task_ids.push(task_id.to_string()),
            Some("OPTIONAL" | "FLEXIBLE") => health_optional_task_ids.push(task_id.to_string()),
            _ => {}
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
    let health_blocked_required_tasks = health_required_task_ids
        .iter()
        .filter(|task_id| state_map.get(task_id.as_str()).map(String::as_str) == Some("BLOCKED"))
        .count() as i64;
    let health_blocked_optional_tasks = health_optional_task_ids
        .iter()
        .filter(|task_id| state_map.get(task_id.as_str()).map(String::as_str) == Some("BLOCKED"))
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
    let continuity_pressure_records =
        NarrativeRepository::continuity_pressure_records(connection, project_id)
            .map_err(|error| error.to_string())?;
    let continuity_authority = continuity_pressure_records
        .iter()
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
    let book_plan =
        NarrativeRepository::latest_artifact_payload(connection, project_id, "BookPlan")
            .map_err(|error| error.to_string())?
            .unwrap_or_else(|| json!({}));
    let reservation_authority = planning::future_reservation_authority(&book_plan, &intent);
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
    let planning_pressure = planning::compile_planning_pressure(
        committed_chapters,
        max_chapters,
        &route,
        &intent,
        &task_reality,
        &continuity_authority,
        &reservation_authority,
    );
    let state_input = json!({
        "projectId": project_id,
        "volumeId": volume_id,
        "routeRef": route_ref,
        "compiledStateDigest": compiled_state_digest,
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
        "healthRequiredTaskIds": health_required_task_ids,
        "remainingTaskIds": remaining_task_ids,
        "requiredTasksRemaining": required_tasks_remaining,
        "blockedRequiredTasks": blocked_required_tasks,
        "healthBlockedRequiredTasks": health_blocked_required_tasks,
        "healthBlockedOptionalTasks": health_blocked_optional_tasks,
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
    let (candidate, route_version, substantive_change) = derive_route_candidate(&compiled)?;
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
    if !substantive_change {
        return Ok(json!({
            "replayed": false,
            "classification": "NO_ROUTE_CHANGE_REQUIRED",
            "newVersion": 0,
            "routeVersion": route_version - 1,
            "projectRevision": request.expected_revision + 1,
            "receiptId": format!("project-receipt:{}", request.command_id),
            "previousRoute": previous_ref,
            "intent": intent_ref,
            "compiledInputDigest": compiled["compiledInputDigest"],
            "compiledInput": compiled["compiledInput"],
        }));
    }
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
    let goal_contracts =
        resolved_goal_contracts(connection, &request.project_id, &goal_contract_refs)?;
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
        "goalContracts": goal_contracts,
        "currentIntent": {"ref": current_intent_ref.clone(), "payload": current_intent_payload.clone()},
        "currentRoute": current_route_ref.clone(),
        "canonFacts": canon_facts.clone(),
        "actualReality": current_planning_state.clone(),
        "capacity": current_planning_state.get("planningPressure").cloned().unwrap_or_else(|| json!({})),
        "unresolvedResponsibilities": current_planning_state.get("unresolvedResponsibilities").cloned().unwrap_or_else(|| json!([])),
        "futureReservations": current_intent_payload.get("futureReservationRefs").cloned().unwrap_or_else(|| json!([])),
        "volumeExitState": {"ref": current_exit_ref.clone(), "payload": current_exit_payload.clone()},
        "exactEvidenceRefs": current_exit_payload.get("handoffEvidenceRefs").cloned().unwrap_or_else(|| json!([])),
    });
    let compiled_input_digest =
        sha256_canonical(&compiled_input).map_err(|error| error.to_string())?;
    let next_intent_payload = compile_next_volume_intent(&compiled_input, request)?;
    let next_task_ids = next_intent_payload
        .get("mustAchieve")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .map(|id| format!("task:{id}"))
        .collect::<Vec<_>>();
    let next_route_payload = compile_next_volume_route(
        &compiled_input,
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
    compiled_input: &Value,
    request: &NextVolumeHandoffRequest,
) -> Result<Value, String> {
    let responsibilities = responsibility_refs_from_input(compiled_input);
    let mut must_achieve = next_volume_goal_constraints(compiled_input);
    if let Some(remaining_tasks) = compiled_input
        .pointer("/volumeExitState/payload/remainingTaskIds")
        .and_then(Value::as_array)
    {
        for (index, task) in remaining_tasks.iter().enumerate() {
            if let Some(task_id) = task.as_str() {
                must_achieve.push(json!({
                    "id": format!("goal:{}:exit:{index}", request.next_volume_id),
                    "description": format!("承接退出状态中的未完成任务 {task_id}"),
                }));
            }
        }
    }
    for (index, responsibility) in responsibilities.iter().enumerate() {
        must_achieve.push(json!({
            "id": format!("goal:{}:responsibility:{index}", request.next_volume_id),
            "description": format!("在下一卷继续推进跨卷责任 {responsibility}"),
        }));
    }
    if must_achieve.is_empty() {
        must_achieve.push(json!({
            "id": format!("goal:{}:entry", request.next_volume_id),
            "description": "建立下一卷入口并推进当前主线",
        }));
    }

    let current_must_not_break = compiled_input
        .pointer("/currentIntent/payload/mustNotBreak")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut must_not_break = current_must_not_break;
    if let Some(canon_facts) = compiled_input.get("canonFacts").and_then(Value::as_array) {
        for fact in canon_facts {
            let Some(fact_id) = fact.get("factId").and_then(Value::as_str) else {
                continue;
            };
            let predicate = fact
                .get("predicate")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let object = fact
                .get("object")
                .map(ToString::to_string)
                .unwrap_or_else(|| "null".into());
            must_not_break.push(json!({
                "id": format!("canon:{fact_id}"),
                "description": format!("保持 Canon {predicate} = {object}"),
            }));
        }
    }
    let future_reservations = compiled_input
        .get("futureReservations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let future_reservation_refs = future_reservations
        .iter()
        .filter_map(|item| {
            item.as_str().map(ToOwned::to_owned).or_else(|| {
                item.get("reservationId")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
        })
        .collect::<Vec<_>>();
    for reservation in &future_reservation_refs {
        must_not_break.push(json!({
            "id": format!("reservation:{reservation}"),
            "description": format!("保护未来保留项 {reservation}，不得提前消耗"),
        }));
    }
    let exact_evidence = compiled_input
        .get("exactEvidenceRefs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (index, evidence) in exact_evidence.iter().enumerate() {
        let label = evidence
            .get("artifactId")
            .and_then(Value::as_str)
            .or_else(|| evidence.get("ref").and_then(Value::as_str))
            .unwrap_or("unknown-evidence");
        must_not_break.push(json!({
            "id": format!("exit-evidence:{index}"),
            "description": format!("保留退出证据 {label} 供下一卷回溯"),
        }));
    }

    let exit_ref = compiled_input.pointer("/volumeExitState/ref").cloned();
    let reality_digest = compiled_input
        .pointer("/actualReality/stateDigest")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let semantic_digest = sha256_canonical(&json!({
        "goal": next_volume_goal_constraints(compiled_input),
        "canon": compiled_input.get("canonFacts"),
        "responsibilities": responsibilities.clone(),
        "reservations": future_reservation_refs.clone(),
        "exit": compiled_input.get("volumeExitState"),
        "evidence": exact_evidence.clone(),
    }))
    .map_err(|error| error.to_string())?;
    let mut entry_conditions = vec![
        json!({"kind":"PREVIOUS_VOLUME_CLOSED","value":request.current_volume_id}),
        json!({"kind":"PREVIOUS_VOLUME_EXIT_BOUND","value":exit_ref}),
    ];
    if !reality_digest.is_empty() {
        entry_conditions.push(json!({
            "kind":"PREVIOUS_VOLUME_REALITY_BOUND",
            "value":reality_digest,
        }));
    }
    for evidence in &exact_evidence {
        entry_conditions.push(json!({
            "kind":"EXIT_EVIDENCE_BOUND",
            "value":evidence,
        }));
    }
    let required_exit_conditions = must_achieve
        .iter()
        .filter_map(|goal| goal.get("id").and_then(Value::as_str))
        .map(|id| json!({"kind":"TASK_SATISFIED","value":format!("task:{id}")}))
        .collect::<Vec<_>>();
    let payload = json!({
        "payloadSchema": "volume-intent.r1",
        "volumeFunction": format!("{}：新入口与跨卷责任推进（依据 {semantic_digest}）", request.title),
        "entryConditions": entry_conditions,
        "mustAchieve": must_achieve,
        "mustNotBreak": must_not_break,
        "requiredExitConditions": required_exit_conditions,
        "futureReservationRefs": future_reservation_refs,
        "crossVolumeResponsibilityRefs": responsibilities,
        "targetChapterRange": {"min":1,"preferred":(request.chapter_end - request.chapter_start + 1).max(1),"max":(request.chapter_end - request.chapter_start + 1).max(1)},
        "targetWordRange": {"preferred":request.target_words.max(1)},
    });
    planning::validate_volume_intent(&payload)?;
    Ok(payload)
}

fn compile_next_volume_route(
    compiled_input: &Value,
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
    let responsibilities = responsibility_refs_from_input(compiled_input);
    let tasks = task_ids
        .iter()
        .enumerate()
        .map(|(index, task_id)| {
            let objective = goals
                .get(index)
                .and_then(|item| item.get("description"))
                .and_then(Value::as_str)
                .unwrap_or("推进下一卷入口");
            json!({
                "taskId": task_id,
                "class": "REQUIRED",
                "objective": objective,
                "dependsOnTaskIds": [],
                "continuityThreadRefs": responsibilities.clone(),
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
    let remaining_capacity = compiled_input
        .pointer("/actualReality/remainingChapterCapacity")
        .and_then(Value::as_i64)
        .unwrap_or(span);
    let max_chapters = span.min(remaining_capacity.saturating_add(1).max(1)).max(1);
    let preferred_chapters = span.min(max_chapters).max(1);
    let mut invariants = vec![json!("B09_NEXT_VOLUME_ROUTE_DERIVED")];
    if compiled_input
        .get("canonFacts")
        .and_then(Value::as_array)
        .is_some_and(|facts| !facts.is_empty())
    {
        invariants.push(json!("B09_NEXT_VOLUME_CANON_BOUND"));
    }
    if !next_volume_goal_constraints(compiled_input).is_empty() {
        invariants.push(json!("B09_NEXT_VOLUME_GOAL_BOUND"));
    }
    if !responsibilities.is_empty() {
        invariants.push(json!("B09_NEXT_VOLUME_RESPONSIBILITY_BOUND"));
    }
    if let Some(reservations) = compiled_input
        .get("futureReservations")
        .and_then(Value::as_array)
    {
        for reservation in reservations {
            if let Some(reference) = reservation
                .as_str()
                .or_else(|| reservation.get("reservationId").and_then(Value::as_str))
            {
                invariants.push(json!(format!(
                    "B09_FUTURE_RESERVATION_PROTECTED:{reference}"
                )));
            }
        }
    }
    if compiled_input
        .get("exactEvidenceRefs")
        .and_then(Value::as_array)
        .is_some_and(|evidence| !evidence.is_empty())
    {
        invariants.push(json!("B09_NEXT_VOLUME_EXIT_EVIDENCE_BOUND"));
        for (index, evidence) in compiled_input["exactEvidenceRefs"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let label = evidence
                .get("artifactId")
                .and_then(Value::as_str)
                .or_else(|| evidence.get("ref").and_then(Value::as_str))
                .unwrap_or("unknown-evidence");
            invariants.push(json!(format!("B09_EXIT_EVIDENCE_BOUND:{index}:{label}")));
        }
    }
    let payload = json!({
        "payloadSchema": "volume-plan.r1",
        "volumeIntentRef": Value::Null,
        "routeVersion": 1,
        "tasks": tasks,
        "routeInvariants": invariants,
        "requiredExitConditions": required_exit_conditions,
        "capacityBudget": {"minChapters":1,"preferredChapters":preferred_chapters,"maxChapters":max_chapters},
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
