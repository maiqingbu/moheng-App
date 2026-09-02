//! B07 deterministic planning authority.  It validates plan payloads against
//! existing R1 contracts and never mutates Canon, Knowledge, task reality, or
//! continuity lifecycle.
use crate::commit::CommitService;
use crate::generated::types::ArtifactRef;
use crate::generated::types::{ArtifactEnvelope, CommitBundle, CommitReceipt};
use crate::generated::validators::{
    validate_book_plan_r1, validate_chapter_plan_r1, validate_volume_plan_r1,
};
use crate::hash::sha256_canonical;
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub fn approve_book_plan(
    seed: &Value,
    signing_pass: bool,
    opening_canon_refs: &[String],
    signing_evidence_refs: &[String],
) -> Result<Value, String> {
    if !signing_pass || opening_canon_refs.len() < 3 || signing_evidence_refs.is_empty() {
        return Err("BOOK_PLAN_SIGNING_REQUIRED".into());
    }
    if seed.get("planningMaturity").and_then(Value::as_str) != Some("SEED") {
        return Err("BOOK_PLAN_SEED_REQUIRED".into());
    }
    let mut approved = seed.clone();
    let object = approved.as_object_mut().ok_or("BOOK_PLAN_INVALID")?;
    object.insert("planningMaturity".into(), json!("APPROVED"));
    for key in [
        "mustAchieve",
        "relationshipMacroDirections",
        "majorMysteryGoals",
        "futureReservations",
        "volumeIntentSeeds",
    ] {
        if !object.contains_key(key) {
            return Err(format!("BOOK_PLAN_APPROVAL_MISSING:{key}"));
        }
    }
    if object.contains_key("chapters") || object.contains_key("futureVolumeTimeline") {
        return Err("BOOK_PLAN_STATIC_OUTLINE_FORBIDDEN".into());
    }
    validate_book_plan_r1(&approved).map_err(|error| format!("BOOK_PLAN_CONTRACT:{error}"))?;
    Ok(approved)
}

pub fn validate_volume_route(plan: &Value) -> Result<(), String> {
    if plan.get("chapters").is_some() {
        return Err("VOLUME_PLAN_STATIC_CHAPTERS_FORBIDDEN".into());
    }
    if plan.pointer("/tasks/0/status").is_some()
        || plan
            .get("tasks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|task| task.get("status").is_some())
    {
        return Err("VOLUME_PLAN_TASK_REALITY_FORBIDDEN".into());
    }
    validate_volume_plan_r1(plan).map_err(|error| format!("VOLUME_PLAN_CONTRACT:{error}"))
}

pub fn validate_volume_route_reference(plan: &Value, intent: &ArtifactRef) -> Result<(), String> {
    let supplied = plan
        .get("volumeIntentRef")
        .ok_or("VOLUME_PLAN_INTENT_REF_REQUIRED")?;
    let supplied: ArtifactRef = serde_json::from_value(supplied.clone())
        .map_err(|error| format!("VOLUME_PLAN_INTENT_REF_INVALID:{error}"))?;
    if &supplied != intent {
        return Err("VOLUME_PLAN_INTENT_REF_MISMATCH".into());
    }
    Ok(())
}

pub fn validate_volume_intent(plan: &Value) -> Result<(), String> {
    crate::generated::validators::validate_volume_intent_r1(plan)
        .map_err(|error| format!("VOLUME_INTENT_CONTRACT:{error}"))
}

/// B07's semantic boundary for a Change target.  B12 may assemble the outer
/// CommitBundle, but it cannot turn a JSON patch into a planning authority
/// without this owner validation having accepted the candidate.
pub fn validate_change_target_owner(
    _connection: &Connection,
    _project_id: &str,
    artifact_type: &str,
    candidate: &Value,
    scope: &Value,
) -> Result<(), String> {
    match artifact_type {
        "BookPlan" => {
            validate_book_plan_r1(candidate)
                .map_err(|error| format!("BOOK_PLAN_CONTRACT:{error}"))?;
            if !matches!(
                candidate.get("planningMaturity").and_then(Value::as_str),
                Some("SEED" | "APPROVED")
            ) {
                return Err("BOOK_PLAN_MATURITY_INVALID".into());
            }
        }
        "ChapterPlan" => {
            validate_chapter_plan_r1(candidate)
                .map_err(|error| format!("CHAPTER_PLAN_CONTRACT:{error}"))?;
            let scope_chapter = scope
                .get("chapterId")
                .and_then(Value::as_str)
                .ok_or("CHAPTER_PLAN_SCOPE_REQUIRED")?;
            if candidate.get("chapterId").and_then(Value::as_str) != Some(scope_chapter) {
                return Err("CHAPTER_PLAN_CHAPTER_MISMATCH".into());
            }
        }
        _ => return Err(format!("B12_CHANGE_TARGET_UNSUPPORTED:{artifact_type}")),
    }
    Ok(())
}

pub fn validate_task_reality(
    chapter_plan: &Value,
    route_plan: &Value,
    task_states: &[(&str, &str)],
) -> Result<(), String> {
    let tasks = route_plan
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or("VOLUME_PLAN_TASKS_REQUIRED")?;
    let states = task_states
        .iter()
        .copied()
        .collect::<std::collections::BTreeMap<_, _>>();
    for task_ref in chapter_plan
        .get("activeTaskRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let task = tasks
            .iter()
            .find(|item| item.get("taskId").and_then(Value::as_str) == Some(task_ref))
            .ok_or("TASK_SCOPE_VIOLATION")?;
        if task.get("class").and_then(Value::as_str) == Some("RESERVED") {
            return Err("TASK_SCOPE_VIOLATION".into());
        }
        match states.get(task_ref).copied() {
            Some("INVALIDATED") => return Err("TASK_SCOPE_VIOLATION".into()),
            Some("BLOCKED") => return Err("TASK_DEPENDENCY_UNMET".into()),
            _ => {}
        }
        for dependency in task
            .get("dependsOnTaskIds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if states.get(dependency).copied() != Some("SATISFIED") {
                return Err("TASK_DEPENDENCY_UNMET".into());
            }
        }
    }
    Ok(())
}

pub fn validate_reservation_actions(
    plan: &Value,
    reservations: &[Value],
    current_volume_no: i64,
) -> Result<(), String> {
    let reservation_map = reservations
        .iter()
        .filter_map(|item| {
            item.get("reservationId")
                .and_then(Value::as_str)
                .map(|id| (id, item))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for reference in plan
        .get("reservationRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !reservation_map.contains_key(reference) {
            return Err("FUTURE_RESERVATION_VIOLATION".into());
        }
    }
    for intent in plan
        .get("sceneExecution")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let action = intent
            .get("action")
            .or_else(|| intent.get("intent"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let reference = intent
            .get("reservationRef")
            .and_then(Value::as_str)
            .or_else(|| intent.get("reservationId").and_then(Value::as_str));
        let Some(reference) = reference else { continue };
        let Some(reservation) = reservation_map.get(reference) else {
            return Err("FUTURE_RESERVATION_VIOLATION".into());
        };
        let before_reserved_volume = reservation
            .get("reservedForVolumeNo")
            .and_then(Value::as_i64)
            .is_some_and(|volume| current_volume_no < volume);
        let before_not_before = reservation
            .get("notBeforeVolumeNo")
            .and_then(Value::as_i64)
            .is_some_and(|volume| current_volume_no < volume);
        let allowed = match action {
            "FORESHADOW" => reservation.get("mayForeshadow").and_then(Value::as_bool) == Some(true),
            "REVEAL" => {
                reservation.get("mayReveal").and_then(Value::as_bool) == Some(true)
                    && !before_reserved_volume
                    && !before_not_before
            }
            _ => true,
        };
        if !allowed {
            return Err("FUTURE_RESERVATION_VIOLATION".into());
        }
    }
    Ok(())
}

pub fn planning_pressure(
    current_chapter: i64,
    max_chapters: i64,
    required_remaining: i64,
    blocked_required: i64,
    exit_gap: i64,
    overdue_threads: i64,
    reservations: i64,
) -> Value {
    let remaining = (max_chapters - current_chapter).max(0);
    let score = required_remaining.saturating_mul(4)
        + blocked_required.saturating_mul(6)
        + exit_gap.saturating_mul(5)
        + overdue_threads.saturating_mul(3)
        + reservations.saturating_mul(2)
        + if remaining <= 8 { 8 } else { 0 };
    json!({"remainingChapterCapacity":remaining,"requiredTasksRemaining":required_remaining,"blockedRequiredTasks":blocked_required,"exitConditionGap":exit_gap,"overdueThreadPressure":overdue_threads,"reservationPressure":reservations,"priorityScore":score})
}

pub fn validate_current_chapter_plan(
    plan: &Value,
    current_chapter_id: &str,
    route_task_ids: &[String],
    triggered_threads: &[String],
) -> Result<(), String> {
    validate_chapter_plan_r1(plan).map_err(|error| format!("CHAPTER_PLAN_CONTRACT:{error}"))?;
    if plan.get("chapterId").and_then(Value::as_str) != Some(current_chapter_id) {
        return Err("CHAPTER_PLAN_CURRENT_ONLY".into());
    }
    let horizon = plan
        .get("horizonPreview")
        .and_then(Value::as_array)
        .ok_or("CHAPTER_PLAN_HORIZON_REQUIRED")?;
    if !(3..=8).contains(&horizon.len())
        || horizon
            .iter()
            .any(|slot| slot.get("tentative").and_then(Value::as_bool) != Some(true))
    {
        return Err("ROLLING_HORIZON_TENTATIVE_REQUIRED".into());
    }
    let route = route_task_ids.iter().collect::<BTreeSet<_>>();
    if plan
        .get("activeTaskRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|task| !route.contains(&task.to_string()))
    {
        return Err("TASK_SCOPE_VIOLATION".into());
    }
    let dispositions = plan
        .get("continuityActions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|action| {
            action
                .get("threadRef")
                .and_then(Value::as_str)
                .map(|thread| (thread, action))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for thread in triggered_threads {
        let action = dispositions
            .get(thread.as_str())
            .ok_or("CONTINUITY_BRIDGE_REQUIRED")?;
        if action.get("disposition").and_then(Value::as_str) == Some("DEFER_WITH_BRIDGE")
            && action
                .get("bridge")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        {
            return Err("CONTINUITY_BRIDGE_REQUIRED".into());
        }
    }
    Ok(())
}

pub fn classify_plan_reality(
    stale: bool,
    required_blocked: bool,
    capacity_exhausted: bool,
    change_requested: bool,
) -> &'static str {
    if change_requested {
        "CHANGE_REQUEST"
    } else if capacity_exhausted {
        "REPLAN_VOLUME_ROUTE"
    } else if required_blocked {
        "RECOMPILE_ROLLING_WINDOW"
    } else if stale {
        "RECOMPILE_NEXT_CHAPTER"
    } else {
        "NO_ACTION"
    }
}

#[allow(clippy::too_many_arguments)]
pub fn recompile_chapter_plan_owner(
    connection: &Connection,
    project_id: &str,
    chapter_id: &str,
    plan: &Value,
    authority_request: &Value,
    execution_mode: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    model_run_id: Option<&str>,
) -> Result<CommitReceipt, String> {
    validate_chapter_plan_r1(plan).map_err(|error| format!("CHAPTER_PLAN_CONTRACT:{error}"))?;
    if plan.get("chapterId").and_then(Value::as_str) != Some(chapter_id) {
        return Err("CHAPTER_PLAN_CHAPTER_MISMATCH".into());
    }
    let context_ref: ArtifactRef = serde_json::from_value(
        plan.get("contextManifestRef")
            .cloned()
            .ok_or_else(|| "CONTEXT_MANIFEST_REF_REQUIRED".to_string())?,
    )
    .map_err(|error| format!("CONTEXT_MANIFEST_REF_INVALID:{error}"))?;
    if crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
        connection,
        project_id,
        &context_ref,
    )
    .map_err(|error| error.to_string())?
    .is_none()
    {
        return Err("CONTEXT_MANIFEST_NOT_FOUND".into());
    }
    if crate::repositories::change_retcon::has_pending_stale_for_refs(
        connection,
        project_id,
        &[context_ref.artifact_id.as_str()],
    )
    .map_err(|error| error.to_string())?
    {
        return Err("PLAN_STALE".into());
    }
    let volume_id = crate::repositories::narrative::NarrativeRepository::chapter_volume_id(
        connection, project_id, chapter_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "CHAPTER_NOT_FOUND".to_string())?;
    let context = crate::commands::context_input_from_authority(
        connection,
        authority_request,
        project_id,
        expected_revision,
    )?;
    let current_digest = crate::narrative::relevant_state_digest(&context)?;
    if matches!(
        crate::narrative::evaluate_plan_stale(
            plan.get("compiledFromProjectRevision")
                .and_then(Value::as_i64)
                .ok_or_else(|| "CHAPTER_PLAN_REVISION_REQUIRED".to_string())?,
            plan.get("compiledFromStateDigest")
                .and_then(Value::as_str)
                .ok_or_else(|| "CHAPTER_PLAN_STATE_DIGEST_REQUIRED".to_string())?,
            expected_revision,
            &current_digest,
        ),
        crate::narrative::PlanStaleStatus::PlanStale
    ) {
        return Err("PLAN_STALE".into());
    }
    let triggered_threads = context
        .continuity_threads
        .iter()
        .filter(|thread| matches!(thread.authority_level.as_str(), "A" | "B"))
        .map(|thread| thread.thread_id.clone())
        .collect::<Vec<_>>();
    let (route_task_ids, route_payload) = if execution_mode == "LONG_FORM" {
        let route_ref = crate::repositories::narrative::NarrativeRepository::volume_plan_route(
            connection, project_id, &volume_id,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VOLUME_PLAN_NOT_FOUND".to_string())?;
        let route = crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
            connection, project_id, &route_ref,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VOLUME_PLAN_PAYLOAD_NOT_FOUND".to_string())?;
        if crate::repositories::change_retcon::has_pending_stale_for_refs(
            connection,
            project_id,
            &[route_ref.artifact_id.as_str()],
        )
        .map_err(|error| error.to_string())?
        {
            return Err("PLAN_STALE".into());
        }
        let task_ids = route
            .get("tasks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|task| task.get("taskId").and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        (task_ids, Some(route))
    } else {
        (context.active_task_refs.clone(), None)
    };
    validate_current_chapter_plan(plan, chapter_id, &route_task_ids, &triggered_threads)?;
    if let Some(route) = route_payload.as_ref() {
        let states = crate::repositories::narrative::NarrativeRepository::task_states_by_ids(
            connection,
            project_id,
            &route_task_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            100,
        )
        .map_err(|error| error.to_string())?;
        let states = states
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
        validate_task_reality(plan, route, &states)?;
    }
    let current_volume_no = crate::repositories::narrative::NarrativeRepository::volume_number(
        connection, project_id, &volume_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "VOLUME_NOT_FOUND".to_string())?;
    let reservations = [
        crate::repositories::narrative::NarrativeRepository::latest_artifact_payload(
            connection, project_id, "BookPlan",
        )
        .map_err(|error| error.to_string())?,
        crate::repositories::narrative::NarrativeRepository::artifact_payload(
            connection,
            project_id,
            "VolumeIntent",
            &volume_id,
        )
        .map_err(|error| error.to_string())?,
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
    validate_reservation_actions(plan, &reservations, current_volume_no)?;
    let artifact_id = format!("plan:chapterplan:{chapter_id}");
    let version = crate::repositories::narrative::NarrativeRepository::next_artifact_version(
        connection,
        project_id,
        &artifact_id,
    )
    .map_err(|error| error.to_string())?;
    let hash = sha256_canonical(plan).map_err(|error| error.to_string())?;
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId":artifact_id,"projectId":project_id,"artifactType":"ChapterPlan",
        "scope":{"level":"CHAPTER","chapterId":chapter_id},"version":version,
        "schemaVersion":"m0a.artifact.v1","artifactStatus":"CANDIDATE","payload":plan,
        "payloadHash":hash,"dependencyRefs":[context_ref],
        "provenance":{"createdBy":"B07::PlanningOwner","commandId":command_id,"contextManifestRef":context_ref.artifact_id,"modelRunId":model_run_id},
        "createdAt":now
    }))
    .map_err(|error| error.to_string())?;
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":format!("commit:{command_id}"),"projectId":project_id,"commandId":command_id,
        "idempotencyKey":idempotency_key,"expectedProjectRevision":expected_revision,
        "contractVersion":"m0a.v1","createdAt":now,"artifacts":[artifact],
        "headUpdates":[{"artifactType":"ChapterPlan","scope":{"level":"CHAPTER","chapterId":chapter_id},
            "artifactId":artifact_id,"version":version,"payloadHash":hash}]
    }))
    .map_err(|error| error.to_string())?;
    CommitService::commit(connection, &bundle, now).map_err(|error| error.to_string())
}

/// B07 owner entry used by B11's opening bootstrap. The caller supplies the
/// context reference produced by B06; this owner builds only the current
/// ChapterPlan and routes it through the same validation/commit function used
/// by ordinary planning and stale recompile paths.
#[allow(clippy::too_many_arguments)]
pub fn commit_opening_chapter_plan_owner(
    connection: &Connection,
    project_id: &str,
    chapter_id: &str,
    context_ref: &ArtifactRef,
    authority_request: &Value,
    expected_revision: i64,
    command_id: &str,
    idempotency_key: &str,
    now: &str,
) -> Result<CommitReceipt, String> {
    let context_payload =
        crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
            connection,
            project_id,
            context_ref,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "CONTEXT_MANIFEST_NOT_FOUND".to_string())?;
    let authority = {
        let mut request = authority_request.clone();
        request["executionMode"] = json!("OPENING_BOOTSTRAP");
        request["chapterId"] = json!(chapter_id);
        request
    };
    let context_input = crate::commands::context_input_from_authority(
        connection,
        &authority,
        project_id,
        expected_revision,
    )?;
    let current_digest = crate::narrative::relevant_state_digest(&context_input)?;
    let chapter_goal = authority
        .get("openingChapterGoal")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("让主角面对一个必须主动回应的异常");
    let continuity_actions = context_input
        .continuity_threads
        .iter()
        .map(|thread| {
            json!({
                "threadRef":thread.thread_id,
                "disposition":"KEEP_OPEN",
            })
        })
        .collect::<Vec<_>>();
    let plan = json!({
        "payloadSchema":"chapter-plan.r1",
        "chapterId":chapter_id,
        "compiledFromProjectRevision":expected_revision,
        "compiledFromStateDigest":current_digest,
        "contextManifestRef":context_ref,
        "chapterGoal":chapter_goal,
        "activeTaskRefs":context_input.active_task_refs,
        "continuityActions":continuity_actions,
        "requiredStateOutcomes":[],
        "forbiddenTransitions":[],
        "knowledgeBoundaries":[],
        "reservationRefs":[],
        "sceneExecution":[],
        "horizonPreview":[
            {"slotNo":1,"objective":"展示当前异常并迫使主角选择","tentative":true},
            {"slotNo":2,"objective":"给出局部回报并留下代价","tentative":true},
            {"slotNo":3,"objective":"把问题推向首卷主线","tentative":true}
        ]
    });
    if context_payload
        .get("compiledFromStateDigest")
        .and_then(Value::as_str)
        .is_none()
    {
        return Err("CONTEXT_MANIFEST_STATE_DIGEST_MISSING".into());
    }
    recompile_chapter_plan_owner(
        connection,
        project_id,
        chapter_id,
        &plan,
        &authority,
        "OPENING_BOOTSTRAP",
        command_id,
        idempotency_key,
        expected_revision,
        now,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter_plan() -> Value {
        json!({"payloadSchema":"chapter-plan.r1","chapterId":"chapter:1","compiledFromProjectRevision":1,"compiledFromStateDigest":"a".repeat(64),"contextManifestRef":{"artifactId":"context:1","version":1,"payloadHash":"b".repeat(64)},"chapterGoal":"advance","activeTaskRefs":["task:required"],"continuityActions":[{"threadRef":"thread:promise","disposition":"DEFER_WITH_BRIDGE","bridge":"interrupted meeting"}],"requiredStateOutcomes":[],"forbiddenTransitions":[],"knowledgeBoundaries":[],"reservationRefs":[],"sceneExecution":[],"horizonPreview":[{"slotNo":1,"objective":"one","tentative":true},{"slotNo":2,"objective":"two","tentative":true},{"slotNo":3,"objective":"three","tentative":true}]})
    }

    #[test]
    fn chapter_plan_rejects_reserved_reveal_and_out_of_route_task() {
        let mut plan = chapter_plan();
        plan["sceneExecution"] = json!([{ "action":"REVEAL", "reservationRef":"secret:v5" }]);
        assert_eq!(
            validate_reservation_actions(
                &plan,
                &[json!({"reservationId":"secret:v5","mayForeshadow":true,"mayReveal":false})],
                3
            ),
            Err("FUTURE_RESERVATION_VIOLATION".into())
        );
        plan["sceneExecution"] = json!([]);
        plan["activeTaskRefs"] = json!(["task:future"]);
        assert_eq!(
            validate_current_chapter_plan(
                &plan,
                "chapter:1",
                &["task:required".into()],
                &["thread:promise".into()]
            ),
            Err("TASK_SCOPE_VIOLATION".into())
        );
    }

    #[test]
    fn chapter_plan_requires_bridge_and_tentative_horizon() {
        let mut plan = chapter_plan();
        plan["continuityActions"][0]["bridge"] = Value::Null;
        assert_eq!(
            validate_current_chapter_plan(
                &plan,
                "chapter:1",
                &["task:required".into()],
                &["thread:promise".into()]
            ),
            Err("CONTINUITY_BRIDGE_REQUIRED".into())
        );
        plan["continuityActions"][0]["bridge"] = json!("bridge");
        plan["horizonPreview"][0]["tentative"] = json!(false);
        assert!(validate_current_chapter_plan(
            &plan,
            "chapter:1",
            &["task:required".into()],
            &["thread:promise".into()]
        )
        .is_err());
    }

    #[test]
    fn book_approval_and_volume_route_fail_closed_on_static_truth() {
        let seed = json!({"payloadSchema":"book-plan.r1","planningMaturity":"SEED","storyPromise":"promise","protagonistMacroDirection":"direction","endingBoundary":"end","mustNotBreak":[],"openingHypotheses":[],"globalLengthBudget":{"preferredWords":1000}});
        assert_eq!(
            approve_book_plan(&seed, false, &[], &[]),
            Err("BOOK_PLAN_SIGNING_REQUIRED".into())
        );
        let route = json!({"payloadSchema":"volume-plan.r1","volumeIntentRef":{"artifactId":"intent:1","version":1,"payloadHash":"a".repeat(64)},"routeVersion":1,"tasks":[],"routeInvariants":[],"requiredExitConditions":[],"capacityBudget":{"minChapters":1,"preferredChapters":2,"maxChapters":3},"chapters":[]});
        assert_eq!(
            validate_volume_route(&route),
            Err("VOLUME_PLAN_STATIC_CHAPTERS_FORBIDDEN".into())
        );
    }

    #[test]
    fn pressure_rises_for_required_blocked_and_exit_constraints() {
        let calm = planning_pressure(10, 40, 1, 0, 0, 0, 0);
        let pressured = planning_pressure(35, 40, 3, 1, 2, 1, 1);
        assert!(
            pressured["priorityScore"].as_i64().unwrap() > calm["priorityScore"].as_i64().unwrap()
        );
        assert_eq!(pressured["remainingChapterCapacity"], 5);
    }

    #[test]
    fn plan_reality_classifier_keeps_route_replan_as_recommendation() {
        assert_eq!(
            classify_plan_reality(false, false, false, false),
            "NO_ACTION"
        );
        assert_eq!(
            classify_plan_reality(true, false, false, false),
            "RECOMPILE_NEXT_CHAPTER"
        );
        assert_eq!(
            classify_plan_reality(false, true, false, false),
            "RECOMPILE_ROLLING_WINDOW"
        );
        assert_eq!(
            classify_plan_reality(false, false, true, false),
            "REPLAN_VOLUME_ROUTE"
        );
        assert_eq!(
            classify_plan_reality(false, false, false, true),
            "CHANGE_REQUEST"
        );
    }

    #[test]
    fn task_reality_rejects_unmet_dependency_blocked_invalidated_and_reserved() {
        let chapter = json!({"activeTaskRefs":["task:b"]});
        let route = json!({"tasks":[{"taskId":"task:a","class":"REQUIRED","dependsOnTaskIds":[],"objective":"a"},{"taskId":"task:b","class":"REQUIRED","dependsOnTaskIds":["task:a"],"objective":"b"}]});
        assert_eq!(
            validate_task_reality(
                &chapter,
                &route,
                &[("task:a", "ACTIVE"), ("task:b", "ACTIVE")]
            ),
            Err("TASK_DEPENDENCY_UNMET".into())
        );
        assert!(validate_task_reality(
            &chapter,
            &route,
            &[("task:a", "SATISFIED"), ("task:b", "ACTIVE")]
        )
        .is_ok());
        assert_eq!(
            validate_task_reality(
                &chapter,
                &route,
                &[("task:a", "SATISFIED"), ("task:b", "INVALIDATED")]
            ),
            Err("TASK_SCOPE_VIOLATION".into())
        );
        let reserved_route = json!({"tasks":[{"taskId":"task:b","class":"RESERVED","dependsOnTaskIds":[],"objective":"reserved"}]});
        assert_eq!(
            validate_task_reality(&chapter, &reserved_route, &[("task:b", "ACTIVE")]),
            Err("TASK_SCOPE_VIOLATION".into())
        );
    }
}
