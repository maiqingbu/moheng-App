//! B07 deterministic planning authority.  It validates plan payloads against
//! existing R1 contracts and never mutates Canon, Knowledge, task reality, or
//! continuity lifecycle.
use crate::commit::CommitService;
use crate::generated::types::ArtifactRef;
use crate::generated::types::{ArtifactEnvelope, CommitBundle, CommitReceipt, HeadChange};
use crate::generated::validators::{
    validate_book_plan_r1, validate_chapter_plan_r1, validate_volume_plan_r1,
};
use crate::hash::sha256_canonical;
use crate::repositories::narrative::OpeningActualState;
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub fn normalize_chapter_plan_model_output(
    mut plan: Value,
    triggered_thread_refs: &[String],
    authoritative_bindings: &Value,
) -> Value {
    if let Some(object) = plan.as_object_mut() {
        object.retain(|key, _| {
            matches!(
                key.as_str(),
                "payloadSchema"
                    | "chapterId"
                    | "compiledFromProjectRevision"
                    | "compiledFromStateDigest"
                    | "contextManifestRef"
                    | "chapterGoal"
                    | "activeTaskRefs"
                    | "continuityActions"
                    | "requiredStateOutcomes"
                    | "forbiddenTransitions"
                    | "knowledgeBoundaries"
                    | "reservationRefs"
                    | "sceneExecution"
                    | "horizonPreview"
            )
        });
    }
    plan["payloadSchema"] = json!("chapter-plan.r1");
    for field in [
        "chapterId",
        "compiledFromProjectRevision",
        "compiledFromStateDigest",
        "contextManifestRef",
    ] {
        if let Some(value) = authoritative_bindings.get(field) {
            plan[field] = value.clone();
        }
    }
    if let Some(items) = plan
        .get_mut("knowledgeBoundaries")
        .and_then(Value::as_array_mut)
    {
        for item in items {
            if item.is_string() {
                continue;
            }
            if let Some(object) = item.as_object() {
                let mut parts = Vec::new();
                for key in ["entityId", "factId", "state", "constraint", "description"] {
                    if let Some(value) = object.get(key).and_then(Value::as_str) {
                        if !value.trim().is_empty() {
                            parts.push(format!("{key}={value}"));
                        }
                    }
                }
                *item = Value::String(if parts.is_empty() {
                    Value::Object(object.clone()).to_string()
                } else {
                    parts.join(" · ")
                });
            } else {
                *item = Value::String(item.to_string());
            }
        }
    }
    for field in ["requiredStateOutcomes", "forbiddenTransitions"] {
        if let Some(items) = plan.get_mut(field).and_then(Value::as_array_mut) {
            for item in items {
                let normalized = if let Some(statement) = item
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    Some(json!({"kind":"MODEL_STATED","value":statement}))
                } else {
                    item.as_object().map(|object| {
                        let kind = object
                            .get("kind")
                            .or_else(|| object.get("stateKey"))
                            .or_else(|| object.get("outcomeId"))
                            .or_else(|| object.get("transitionId"))
                            .or_else(|| object.get("type"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty());
                        let value = object
                            .get("value")
                            .cloned()
                            .or_else(|| object.get("description").cloned())
                            .unwrap_or_else(|| Value::Object(object.clone()));
                        json!({
                            "kind":kind.unwrap_or("MODEL_STATED"),
                            "value":value
                        })
                    })
                };
                if let Some(normalized) = normalized {
                    *item = normalized;
                }
            }
        }
    }
    if triggered_thread_refs.is_empty() {
        if let Some(items) = plan
            .get_mut("continuityActions")
            .and_then(Value::as_array_mut)
        {
            items.retain_mut(|item| {
                let Some(object) = item.as_object_mut() else {
                    return false;
                };
                let thread_ref = object
                    .get("threadRef")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let disposition = object
                    .get("disposition")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| {
                        matches!(
                            *value,
                            "ADVANCE"
                                | "RESOLVE"
                                | "DEFER_WITH_BRIDGE"
                                | "NO_LONGER_APPLICABLE_CHANGE_REQUEST"
                        )
                    });
                thread_ref.is_some() && disposition.is_some()
            });
        }
    }
    if let Some(items) = plan.get_mut("horizonPreview").and_then(Value::as_array_mut) {
        let mut normalized_items = Vec::new();
        for item in items.drain(..) {
            let objective = item
                .as_str()
                .or_else(|| item.get("objective").and_then(Value::as_str))
                .or_else(|| item.get("description").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if let Some(objective) = objective {
                normalized_items.push(json!({"slotNo":normalized_items.len() + 1,"objective":objective,"tentative":true}));
            }
        }
        *items = normalized_items;
        items.truncate(8);
        let fallbacks = [
            "根据本章结果重新评估下一步行动。",
            "保持长期悬念，并为后续冲突留出空间。",
            "让新的选择产生可验证的叙事后果。",
        ];
        while items.len() < 3 {
            let index = items.len();
            items.push(json!({
                "slotNo":index + 1,
                "objective":fallbacks[index],
                "tentative":true
            }));
        }
    }
    if triggered_thread_refs.is_empty()
        && plan
            .get("continuityActions")
            .and_then(Value::as_array)
            .is_some_and(|items| items.iter().any(|item| !item.is_object()))
    {
        plan["continuityActions"] = json!([]);
    }
    plan
}

pub fn approve_book_plan(
    seed: &Value,
    signing_pass: bool,
    opening_canon_refs: &[String],
    signing_evidence_refs: &[String],
    actual_state: &OpeningActualState,
) -> Result<Value, String> {
    if !signing_pass || opening_canon_refs.len() < 3 || signing_evidence_refs.is_empty() {
        return Err("BOOK_PLAN_SIGNING_REQUIRED".into());
    }
    if seed.get("planningMaturity").and_then(Value::as_str) != Some("SEED") {
        return Err("BOOK_PLAN_SEED_REQUIRED".into());
    }
    let mut approved = seed.clone();
    let object = approved.as_object_mut().ok_or("BOOK_PLAN_INVALID")?;
    // Older entry payloads carried a timestamp for display.  It is not part
    // of the frozen BookPlan R1 schema, so strip it from the new authority
    // while retaining the historical SEED artifact unchanged.
    object.remove("createdAt");
    object.insert("planningMaturity".into(), json!("APPROVED"));
    enrich_book_plan_from_opening(
        object,
        opening_canon_refs,
        signing_evidence_refs,
        actual_state,
    )?;
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

/// Seeds the post-signing goal fields from actual opening references when the
/// pre-signing seed did not provide them.  These are deliberately structural
/// continuations of observed Canon, not a new commercial scoring product or a
/// fabricated chapter outline.  The deterministic digest keeps generated IDs
/// stable for the same real opening evidence.
fn append_book_plan_item(object: &mut serde_json::Map<String, Value>, key: &str, item: Value) {
    let items = object
        .entry(key.to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("book plan array field");
    items.push(item);
}

fn enrich_book_plan_from_opening(
    object: &mut serde_json::Map<String, Value>,
    opening_canon_refs: &[String],
    signing_evidence_refs: &[String],
    actual_state: &OpeningActualState,
) -> Result<(), String> {
    if opening_canon_refs.is_empty() || actual_state.evidence.is_empty() {
        return Err("BOOK_PLAN_OPENING_CANON_REQUIRED".into());
    }
    let digest = actual_state.semantic_digest.as_str();
    if digest.len() != 64 {
        return Err("BOOK_PLAN_OPENING_STATE_DIGEST_REQUIRED".into());
    }
    let short_digest = &digest[..16];
    let fact_text = actual_state
        .facts
        .iter()
        .take(8)
        .map(|fact| {
            format!(
                "{}{}={}({})",
                fact.subject_name.as_deref().unwrap_or(&fact.subject_id),
                fact.predicate,
                fact.object_json,
                fact.fact_id
            )
        })
        .collect::<Vec<_>>();
    let relation_text = actual_state
        .relations
        .iter()
        .take(8)
        .map(|relation| {
            format!(
                "{}{}{}({})",
                relation
                    .subject_name
                    .as_deref()
                    .unwrap_or(&relation.subject_id),
                relation.predicate,
                relation
                    .object_name
                    .as_deref()
                    .unwrap_or(&relation.object_id),
                relation.relation_id
            )
        })
        .collect::<Vec<_>>();
    let knowledge_text = actual_state
        .knowledge
        .iter()
        .take(8)
        .map(|knowledge| {
            format!(
                "{}对{}的认知为{}({})",
                knowledge.knower_id,
                knowledge.fact_id.as_deref().unwrap_or("未知事实"),
                knowledge.epistemic_state,
                knowledge.knowledge_id
            )
        })
        .collect::<Vec<_>>();
    let event_text = actual_state
        .events
        .iter()
        .take(8)
        .map(|event| {
            format!(
                "{}:{}({})",
                event.event_type, event.object_json, event.event_id
            )
        })
        .collect::<Vec<_>>();
    let evidence_text = actual_state
        .evidence
        .iter()
        .take(8)
        .filter_map(|evidence| evidence.quote.as_deref())
        .collect::<Vec<_>>()
        .join("；");
    let facts = if fact_text.is_empty() {
        "无".into()
    } else {
        fact_text.join("；")
    };
    let relations = if relation_text.is_empty() {
        "无".into()
    } else {
        relation_text.join("；")
    };
    let knowledge = if knowledge_text.is_empty() {
        "无".into()
    } else {
        knowledge_text.join("；")
    };
    let events = if event_text.is_empty() {
        "无".into()
    } else {
        event_text.join("；")
    };
    let evidence = if evidence_text.is_empty() {
        "无".into()
    } else {
        evidence_text
    };
    let actual_refs = opening_canon_refs.join(", ");
    let semantic_evidence_refs = actual_state
        .evidence
        .iter()
        .map(|item| item.evidence_id.clone())
        .collect::<Vec<_>>();
    object.insert("openingCanonRefs".into(), json!(opening_canon_refs));
    object.insert("signingEvidenceRefs".into(), json!(semantic_evidence_refs));
    object.insert(
        "openingActualStateDigest".into(),
        json!(actual_state.semantic_digest),
    );
    append_book_plan_item(
        object,
        "mustAchieve",
        json!({
            "id":format!("goal:opening-reality:{short_digest}"),
            "description":format!("基于开篇事实 {facts} 与事件 {events}，继续让真实选择产生可核对后果；来源 {actual_refs}。")
        }),
    );
    append_book_plan_item(
        object,
        "mustNotBreak",
        json!({
            "id":format!("rule:opening-reality:{short_digest}"),
            "description":format!("不得违背开篇事实 {facts}、关系 {relations}、知识边界 {knowledge}；逐章证据：{evidence}。")
        }),
    );
    if actual_state.relations.is_empty() {
        append_book_plan_item(
            object,
            "relationshipMacroDirections",
            json!({
                "id":format!("relationship:opening-facts:{short_digest}"),
                "description":format!("根据开篇事实中的人物变化继续保持关系可追溯：{facts}。")
            }),
        );
    } else {
        for relation in actual_state.relations.iter().take(8) {
            append_book_plan_item(
                object,
                "relationshipMacroDirections",
                json!({
                    "id":format!("relationship:opening:{}", relation.relation_id),
                    "description":format!("{}与{}的关系“{}”必须沿真实开篇变化继续可追溯。", relation.subject_name.as_deref().unwrap_or(&relation.subject_id), relation.object_name.as_deref().unwrap_or(&relation.object_id), relation.predicate)
                }),
            );
        }
    }
    append_book_plan_item(
        object,
        "majorMysteryGoals",
        json!({
            "id":format!("mystery:opening-reality:{short_digest}"),
            "description":format!("围绕开篇实际事件 {events} 与知识边界 {knowledge} 推进未解问题，证据线索为 {evidence}。")
        }),
    );
    append_book_plan_item(
        object,
        "futureReservations",
        json!({
            "reservationId":format!("reservation:opening-reality:{short_digest}"),
            "description":format!("保留由开篇事实 {facts}、关系 {relations} 与事件 {events} 提出的后续责任。"),
            "reservedForVolumeNo":2,
            "notBeforeVolumeNo":2,
            "prerequisites":opening_canon_refs.iter().cloned().chain(signing_evidence_refs.iter().cloned()).collect::<Vec<_>>(),
            "mayForeshadow":true,
            "mayReveal":true,
            "authorityLevel":"B"
        }),
    );
    append_book_plan_item(
        object,
        "volumeIntentSeeds",
        json!({
            "seedId":format!("volume-intent:opening-reality:{short_digest}"),
            "description":format!("下一卷承接事实 {facts}、关系 {relations}、知识 {knowledge} 和事件 {events}，并以证据 {evidence} 校验推进。")
        }),
    );
    Ok(())
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

/// Finite B03 predicate vocabulary.  The task route is a plan, so these
/// predicates describe a future, inspectable condition; they never encode a
/// task's own `SATISFIED` state as its completion rule.
pub const B03_COMPLETION_PREDICATE_KINDS: &[&str] = &[
    "TASK_EVIDENCE_RECORDED",
    "CANON_FACT_MATCH",
    "CANON_EVENT_EXISTS",
    "RELATION_STATE_MATCH",
    "KNOWLEDGE_STATE_MATCH",
    "FUTURE_RESERVATION_STATUS",
    "OBLIGATION_STATUS",
];

pub fn is_b03_completion_predicate_kind(kind: &str) -> bool {
    B03_COMPLETION_PREDICATE_KINDS.contains(&kind)
}

fn predicate_object<'a>(predicate: &'a Value, key: &str) -> Result<&'a Value, String> {
    predicate
        .get("value")
        .and_then(Value::as_object)
        .and_then(|object| object.get(key))
        .filter(|value| !value.is_null())
        .ok_or_else(|| format!("VOLUME_PLAN_COMPLETION_PREDICATE_{key}_REQUIRED"))
}

pub fn validate_b03_completion_predicate(predicate: &Value, task_id: &str) -> Result<(), String> {
    let kind = predicate
        .get("kind")
        .and_then(Value::as_str)
        .filter(|kind| !kind.trim().is_empty())
        .ok_or("VOLUME_PLAN_COMPLETION_PREDICATE_REQUIRED")?;
    if kind == "TASK_SATISFIED" {
        return Err("VOLUME_PLAN_COMPLETION_PREDICATE_SELF_REFERENCE".into());
    }
    if !is_b03_completion_predicate_kind(kind) {
        return Err("VOLUME_PLAN_COMPLETION_PREDICATE_UNKNOWN".into());
    }
    if predicate.get("value").is_none_or(Value::is_null) {
        return Err("VOLUME_PLAN_COMPLETION_PREDICATE_VALUE_REQUIRED".into());
    }
    match kind {
        "TASK_EVIDENCE_RECORDED" => {
            let value = predicate_object(predicate, "taskId")?.as_str();
            if value != Some(task_id) {
                return Err("VOLUME_PLAN_COMPLETION_PREDICATE_TASK_MISMATCH".into());
            }
            if predicate_object(predicate, "evidenceType")?
                .as_str()
                .is_none()
            {
                return Err("VOLUME_PLAN_COMPLETION_PREDICATE_EVIDENCE_TYPE_REQUIRED".into());
            }
            if predicate_object(predicate, "requiresNewEvidence")? != &Value::Bool(true) {
                return Err("VOLUME_PLAN_COMPLETION_PREDICATE_FUTURE_EVIDENCE_REQUIRED".into());
            }
        }
        "CANON_FACT_MATCH" => {
            predicate_object(predicate, "factId")?;
            predicate_object(predicate, "expected")?;
        }
        "CANON_EVENT_EXISTS" => {
            if predicate_object(predicate, "eventId").is_err()
                && predicate_object(predicate, "eventType").is_err()
            {
                return Err("VOLUME_PLAN_COMPLETION_PREDICATE_EVENT_REQUIRED".into());
            }
            if predicate_object(predicate, "afterPlanCommit")? != &Value::Bool(true) {
                return Err("VOLUME_PLAN_COMPLETION_PREDICATE_FUTURE_EVENT_REQUIRED".into());
            }
        }
        "RELATION_STATE_MATCH" => {
            predicate_object(predicate, "relationId")?;
            predicate_object(predicate, "expectedTransition")?;
        }
        "KNOWLEDGE_STATE_MATCH" => {
            predicate_object(predicate, "knowledgeId")?;
            predicate_object(predicate, "epistemicState")?;
        }
        "FUTURE_RESERVATION_STATUS" => {
            predicate_object(predicate, "reservationId")?;
            predicate_object(predicate, "requiredStatus")?;
        }
        "OBLIGATION_STATUS" => {
            predicate_object(predicate, "obligationId")?;
            predicate_object(predicate, "requiredStatus")?;
        }
        _ => unreachable!("predicate kind is checked above"),
    }
    Ok(())
}

/// Evaluates the part of a predicate that can be decided from Task Reality.
/// Canon/Relation/Event/Knowledge/Reservation owners provide the referenced
/// state; this function intentionally never infers completion from Plan data.
pub fn evaluate_b03_completion_predicate(
    predicate: &Value,
    task_id: &str,
    reality: &Value,
) -> Result<bool, String> {
    validate_b03_completion_predicate(predicate, task_id)?;
    let status = reality.get("status").and_then(Value::as_str);
    match predicate.get("kind").and_then(Value::as_str) {
        Some("TASK_EVIDENCE_RECORDED") => Ok(status == Some("SATISFIED")
            && reality
                .get("evidenceRefs")
                .and_then(Value::as_array)
                .is_some_and(|refs| !refs.is_empty())
            && reality
                .get("lastAdvancedChapterId")
                .and_then(Value::as_str)
                .is_some_and(|chapter| !chapter.trim().is_empty())),
        Some(
            "CANON_FACT_MATCH"
            | "CANON_EVENT_EXISTS"
            | "RELATION_STATE_MATCH"
            | "KNOWLEDGE_STATE_MATCH"
            | "FUTURE_RESERVATION_STATUS"
            | "OBLIGATION_STATUS",
        ) => Ok(reality
            .get("predicateSatisfied")
            .and_then(Value::as_bool)
            .unwrap_or(false)),
        Some(_) | None => unreachable!("predicate kind is checked above"),
    }
}

fn non_empty_array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    let items = value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{key} must be an array"))?;
    if items.is_empty() {
        return Err(format!("{key} must not be empty"));
    }
    Ok(items)
}

pub fn validate_volume_intent_semantics(intent: &Value) -> Result<(), String> {
    validate_volume_intent(intent)?;
    if intent
        .get("mustAchieve")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err("VOLUME_INTENT_MUST_ACHIEVE_REQUIRED".into());
    }
    if intent
        .get("mustNotBreak")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err("VOLUME_INTENT_MUST_NOT_BREAK_REQUIRED".into());
    }
    if intent
        .get("requiredExitConditions")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err("VOLUME_INTENT_EXIT_CONDITIONS_REQUIRED".into());
    }
    if intent
        .get("sourceBookPlanRef")
        .and_then(Value::as_object)
        .is_none()
    {
        return Err("VOLUME_INTENT_BOOK_PLAN_REF_REQUIRED".into());
    }
    if intent
        .get("compiledFromProjectRevision")
        .and_then(Value::as_i64)
        .is_none()
    {
        return Err("VOLUME_INTENT_COMPILED_REVISION_REQUIRED".into());
    }
    if intent
        .get("sourceEvidenceRefs")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err("VOLUME_INTENT_SOURCE_EVIDENCE_REQUIRED".into());
    }
    Ok(())
}

pub fn validate_volume_intent_binding(
    intent: &Value,
    book_plan_ref: &ArtifactRef,
    project_revision: i64,
) -> Result<(), String> {
    validate_volume_intent_semantics(intent)?;
    let supplied = serde_json::from_value::<ArtifactRef>(
        intent
            .get("sourceBookPlanRef")
            .cloned()
            .ok_or("VOLUME_INTENT_BOOK_PLAN_REF_REQUIRED")?,
    )
    .map_err(|error| format!("VOLUME_INTENT_BOOK_PLAN_REF_INVALID:{error}"))?;
    if &supplied != book_plan_ref {
        return Err("VOLUME_INTENT_BOOK_PLAN_REF_MISMATCH".into());
    }
    let compiled_revision = intent
        .get("compiledFromProjectRevision")
        .and_then(Value::as_i64)
        .ok_or("VOLUME_INTENT_COMPILED_REVISION_REQUIRED")?;
    if compiled_revision != project_revision {
        return Err("VOLUME_INTENT_COMPILED_REVISION_MISMATCH".into());
    }
    Ok(())
}

fn push_unique_goal(items: &mut Vec<Value>, goal: Value) {
    let id = goal.get("id").and_then(Value::as_str);
    if id.is_some_and(|id| {
        items
            .iter()
            .any(|item| item.get("id").and_then(Value::as_str) == Some(id))
    }) {
        return;
    }
    items.push(goal);
}

fn source_strings(sources: &Value, key: &str, nested_key: &str) -> Vec<String> {
    sources
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get(nested_key).and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

fn seed_matches_volume(seed_id: &str, volume_no: i64) -> bool {
    let marker = format!("volume-{volume_no}");
    seed_id == marker
        || seed_id.ends_with(&format!(":{marker}"))
        || seed_id.ends_with(&format!("/{marker}"))
        || seed_id.ends_with(&format!("-{volume_no}"))
}

fn goal_matches_seed(goal: &Value, seed_description: &str) -> bool {
    let Some(goal_description) = goal.get("description").and_then(Value::as_str) else {
        return false;
    };
    let goal_tokens = goal_description
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<BTreeSet<_>>();
    if seed_description
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .any(|token| goal_tokens.contains(token))
    {
        return true;
    }
    let compact_seed = seed_description
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<Vec<_>>();
    (4..=8.min(compact_seed.len())).any(|size| {
        compact_seed.windows(size).any(|window| {
            let fragment = window.iter().collect::<String>();
            fragment.len() >= 4 && goal_description.contains(&fragment)
        })
    })
}

#[allow(clippy::too_many_arguments)]
pub fn compile_volume_intent_from_authority(
    volume_id: &str,
    volume_no: i64,
    chapter_start: i64,
    chapter_end: i64,
    target_words: i64,
    book_plan_ref: &ArtifactRef,
    book_plan: &Value,
    sources: &Value,
    project_revision: i64,
) -> Result<Value, String> {
    if book_plan.get("planningMaturity").and_then(Value::as_str) != Some("APPROVED") {
        return Err("GOAL_CONTRACT_BOOK_PLAN_APPROVED_REQUIRED".into());
    }
    if project_revision < 0 {
        return Err("VOLUME_INTENT_COMPILED_REVISION_REQUIRED".into());
    }
    let scoped_seeds = book_plan
        .get("volumeIntentSeeds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|seed| {
            seed.get("seedId")
                .and_then(Value::as_str)
                .is_some_and(|id| seed_matches_volume(id, volume_no))
        })
        .collect::<Vec<_>>();
    let global_goals = book_plan
        .get("mustAchieve")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let scoped_seed_descriptions = scoped_seeds
        .iter()
        .filter_map(|seed| seed.get("description").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let mut must_achieve = if scoped_seeds.is_empty() && volume_no == 1 {
        global_goals
    } else if !scoped_seed_descriptions.is_empty() {
        global_goals
            .into_iter()
            .filter(|goal| {
                scoped_seed_descriptions
                    .iter()
                    .any(|seed| goal_matches_seed(goal, seed))
            })
            .collect()
    } else {
        Vec::new()
    };
    for seed in &scoped_seeds {
        if let (Some(seed_id), Some(description)) = (
            seed.get("seedId").and_then(Value::as_str),
            seed.get("description").and_then(Value::as_str),
        ) {
            push_unique_goal(
                &mut must_achieve,
                json!({
                    "id":format!("goal:{volume_id}:seed:{seed_id}"),
                    "description":description
                }),
            );
        }
    }
    for responsibility in sources
        .get("responsibilities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(id), Some(description)) = (
            responsibility.get("obligationId").and_then(Value::as_str),
            responsibility.get("description").and_then(Value::as_str),
        ) {
            push_unique_goal(
                &mut must_achieve,
                json!({
                    "id":format!("goal:{volume_id}:responsibility:{id}"),
                    "description":format!("推进跨卷责任：{description}")
                }),
            );
        }
    }
    for event in sources
        .get("events")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(8)
    {
        if let (Some(id), Some(event_type)) = (
            event.get("eventId").and_then(Value::as_str),
            event.get("eventType").and_then(Value::as_str),
        ) {
            let object = event
                .get("objectJson")
                .and_then(|value| value.get("text"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| {
                    event
                        .get("objectJson")
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "未知对象".into())
                });
            push_unique_goal(
                &mut must_achieve,
                json!({
                    "id":format!("goal:{volume_id}:event:{id}"),
                    "description":format!("承接已发生事件 {event_type}：{object} 并给出可核对后果")
                }),
            );
        }
    }
    if must_achieve.is_empty() {
        for fact in sources
            .get("facts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(8)
        {
            if let (Some(id), Some(predicate), Some(object)) = (
                fact.get("factId").and_then(Value::as_str),
                fact.get("predicate").and_then(Value::as_str),
                fact.get("objectJson"),
            ) {
                push_unique_goal(
                    &mut must_achieve,
                    json!({
                        "id":format!("goal:{volume_id}:fact:{id}"),
                        "description":format!("围绕已确认事实 {predicate}={object} 推进本卷责任")
                    }),
                );
            }
        }
    }
    if must_achieve.is_empty() {
        for event in sources
            .get("events")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(8)
        {
            if let (Some(id), Some(event_type)) = (
                event.get("eventId").and_then(Value::as_str),
                event.get("eventType").and_then(Value::as_str),
            ) {
                push_unique_goal(
                    &mut must_achieve,
                    json!({
                        "id":format!("goal:{volume_id}:event:{id}"),
                        "description":format!("承接已发生事件 {event_type} 并给出可核对后果")
                    }),
                );
            }
        }
    }
    if must_achieve.is_empty() {
        return Err("VOLUME_INTENT_MUST_ACHIEVE_REQUIRED".into());
    }

    let mut must_not_break = book_plan
        .get("mustNotBreak")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for fact in sources
        .get("facts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(8)
    {
        if let (Some(id), Some(predicate), Some(object)) = (
            fact.get("factId").and_then(Value::as_str),
            fact.get("predicate").and_then(Value::as_str),
            fact.get("objectJson"),
        ) {
            push_unique_goal(
                &mut must_not_break,
                json!({"id":format!("rule:{volume_id}:fact:{id}"),"description":format!("不得违背已确认 Canon：{predicate}={object}")}),
            );
        }
    }
    for knowledge in sources
        .get("knowledge")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(8)
    {
        if let (Some(id), Some(state)) = (
            knowledge.get("knowledgeId").and_then(Value::as_str),
            knowledge.get("epistemicState").and_then(Value::as_str),
        ) {
            push_unique_goal(
                &mut must_not_break,
                json!({"id":format!("rule:{volume_id}:knowledge:{id}"),"description":format!("保持知识边界状态 {state}，不得让角色无证据获得未知信息")}),
            );
        }
    }
    for relation in sources
        .get("relations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(8)
    {
        if let (Some(id), Some(predicate), Some(subject), Some(object)) = (
            relation.get("relationId").and_then(Value::as_str),
            relation.get("predicate").and_then(Value::as_str),
            relation.get("subjectId").and_then(Value::as_str),
            relation.get("objectId").and_then(Value::as_str),
        ) {
            push_unique_goal(
                &mut must_not_break,
                json!({
                    "id":format!("rule:{volume_id}:relation:{id}"),
                    "description":format!("保持已确认关系 {subject} {predicate} {object}，不得无证据跳变")
                }),
            );
        }
    }
    if must_not_break.is_empty() {
        return Err("VOLUME_INTENT_MUST_NOT_BREAK_REQUIRED".into());
    }

    let future_reservation_refs = book_plan
        .get("futureReservations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|reservation| {
            let volume_match = reservation
                .get("reservedForVolumeNo")
                .and_then(Value::as_i64)
                .is_some_and(|number| number == volume_no)
                || reservation
                    .get("reservedForVolumeId")
                    .and_then(Value::as_str)
                    .is_some_and(|id| id == volume_id);
            let unscoped = reservation.get("reservedForVolumeNo").is_none()
                && reservation.get("reservedForVolumeId").is_none();
            volume_match || unscoped
        })
        .filter_map(|reservation| {
            reservation
                .get("reservationId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect::<Vec<_>>();
    let source_canon_refs = [
        source_strings(sources, "facts", "factId"),
        source_strings(sources, "relations", "relationId"),
        source_strings(sources, "events", "eventId"),
    ]
    .into_iter()
    .flatten()
    .collect::<BTreeSet<_>>()
    .into_iter()
    .collect::<Vec<_>>();
    let source_evidence_refs = source_strings(sources, "evidence", "evidenceId");
    if source_evidence_refs.is_empty() {
        return Err("VOLUME_INTENT_SOURCE_EVIDENCE_REQUIRED".into());
    }
    let responsibilities = sources
        .get("responsibilities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("obligationId").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let required_exit_conditions = must_achieve
        .iter()
        .filter_map(|goal| goal.get("id").and_then(Value::as_str))
        .map(|id| json!({"kind":"GOAL_ACHIEVED","value":id}))
        .collect::<Vec<_>>();
    let preferred_chapters = (chapter_end - chapter_start + 1).max(1);
    let target_words = target_words
        .max(preferred_chapters.saturating_mul(1000))
        .max(1);
    let semantic_digest = sha256_canonical(&json!({
        "volumeId":volume_id,
        "volumeNo":volume_no,
        "bookPlan":book_plan,
        "sources":sources,
        "revision":project_revision,
    }))
    .map_err(|error| error.to_string())?;
    let seed_description = book_plan
        .get("volumeIntentSeeds")
        .and_then(Value::as_array)
        .and_then(|items| {
            items.iter().find(|item| {
                item.get("seedId")
                    .and_then(Value::as_str)
                    .is_some_and(|id| seed_matches_volume(id, volume_no))
            })
        })
        .and_then(|item| item.get("description"))
        .and_then(Value::as_str)
        .unwrap_or("承接全书战略并推进当前卷目标");
    let payload = json!({
        "payloadSchema":"volume-intent.r1",
        "volumeFunction":format!("{seed_description}（第{volume_no}卷，基于当前真实 Canon 与责任）"),
        "entryConditions":[
            {"kind":"BOOK_PLAN_APPROVED","value":book_plan_ref},
            {"kind":"VOLUME_EXISTS","value":volume_id},
            {"kind":"PROJECT_REVISION","value":project_revision}
        ],
        "mustAchieve":must_achieve,
        "mustNotBreak":must_not_break,
        "requiredExitConditions":required_exit_conditions,
        "futureReservationRefs":future_reservation_refs,
        "crossVolumeResponsibilityRefs":responsibilities,
        "targetChapterRange":{"min":preferred_chapters.min(1),"preferred":preferred_chapters,"max":preferred_chapters},
        "targetWordRange":{"preferred":target_words},
        "sourceBookPlanRef":book_plan_ref,
        "compiledFromProjectRevision":project_revision,
        "sourceCanonRefs":source_canon_refs,
        "sourceEvidenceRefs":source_evidence_refs,
        "semanticDigest":semantic_digest
    });
    validate_volume_intent_semantics(&payload)?;
    Ok(payload)
}

#[allow(clippy::too_many_arguments)]
pub fn compile_goal_contract(
    story_constitution_ref: &ArtifactRef,
    story_constitution: &Value,
    main_contract_ref: &ArtifactRef,
    main_contract: &Value,
    book_plan_ref: &ArtifactRef,
    book_plan: &Value,
    volume_intent_ref: &ArtifactRef,
    volume_intent: &Value,
) -> Result<Value, String> {
    if story_constitution.as_object().is_none() {
        return Err("GOAL_CONTRACT_STORY_CONSTITUTION_REQUIRED".into());
    }
    if main_contract.as_object().is_none() {
        return Err("GOAL_CONTRACT_MAIN_CONTRACT_REQUIRED".into());
    }
    if book_plan.get("planningMaturity").and_then(Value::as_str) != Some("APPROVED") {
        return Err("GOAL_CONTRACT_BOOK_PLAN_APPROVED_REQUIRED".into());
    }
    validate_volume_intent_semantics(volume_intent)
        .map_err(|_| "GOAL_CONTRACT_VOLUME_INTENT_REQUIRED".to_string())?;
    Ok(json!({
        "contractType":"GoalContractR1",
        "hard":true,
        "refs":{"storyConstitution":story_constitution_ref,"mainContract":main_contract_ref,"bookPlan":book_plan_ref,"volumeIntent":volume_intent_ref},
        "storyConstitution":story_constitution,
        "mainContract":main_contract,
        "bookPlan":book_plan,
        "volumeIntent":volume_intent
    }))
}

fn task_id(task: &Value) -> Result<&str, String> {
    task.get("taskId")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| "VOLUME_PLAN_TASK_ID_REQUIRED".into())
}

pub fn validate_volume_plan_semantics(plan: &Value) -> Result<(), String> {
    validate_volume_route(plan)?;
    let tasks =
        non_empty_array(plan, "tasks").map_err(|_| "VOLUME_PLAN_TASKS_REQUIRED".to_string())?;
    if plan
        .get("routeInvariants")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err("VOLUME_PLAN_ROUTE_INVARIANTS_REQUIRED".into());
    }
    if plan
        .get("requiredExitConditions")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err("VOLUME_PLAN_EXIT_CONDITIONS_REQUIRED".into());
    }
    let ids = tasks
        .iter()
        .map(task_id)
        .collect::<Result<BTreeSet<_>, _>>()?;
    if ids.len() != tasks.len() {
        return Err("VOLUME_PLAN_TASK_ID_DUPLICATE".into());
    }
    for task in tasks {
        let id = task_id(task)?;
        let predicate = task
            .get("completionPredicate")
            .and_then(Value::as_object)
            .ok_or("VOLUME_PLAN_COMPLETION_PREDICATE_REQUIRED")?;
        validate_b03_completion_predicate(&Value::Object(predicate.clone()), id)?;
        for dependency in task
            .get("dependsOnTaskIds")
            .and_then(Value::as_array)
            .ok_or("VOLUME_PLAN_DEPENDENCIES_REQUIRED")?
        {
            let dependency = dependency
                .as_str()
                .ok_or("VOLUME_PLAN_DEPENDENCY_INVALID")?;
            if !ids.contains(dependency) {
                return Err(format!("VOLUME_PLAN_DANGLING_DEPENDENCY:{dependency}"));
            }
            if dependency == id {
                return Err("VOLUME_PLAN_DEPENDENCY_CYCLE".into());
            }
        }
    }
    let mut unresolved = ids.clone();
    loop {
        let ready = tasks
            .iter()
            .filter(|task| {
                task_id(task)
                    .map(|id| {
                        unresolved.contains(id)
                            && task
                                .get("dependsOnTaskIds")
                                .and_then(Value::as_array)
                                .is_some_and(|dependencies| {
                                    dependencies.iter().all(|dependency| {
                                        dependency.as_str().is_none_or(|dependency| {
                                            !unresolved.contains(dependency)
                                        })
                                    })
                                })
                    })
                    .unwrap_or(false)
            })
            .filter_map(|task| task_id(task).ok())
            .collect::<Vec<_>>();
        if ready.is_empty() {
            break;
        }
        for task_id in ready {
            unresolved.remove(task_id);
        }
    }
    if !unresolved.is_empty() {
        return Err("VOLUME_PLAN_DEPENDENCY_CYCLE".into());
    }
    let budget = plan
        .get("capacityBudget")
        .and_then(Value::as_object)
        .ok_or("VOLUME_PLAN_CAPACITY_REQUIRED")?;
    let min = budget
        .get("minChapters")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let preferred = budget
        .get("preferredChapters")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let max = budget
        .get("maxChapters")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if min < 1 || min > preferred || preferred > max {
        return Err("VOLUME_PLAN_CAPACITY_INVALID".into());
    }
    if !tasks
        .iter()
        .any(|task| task.get("class").and_then(Value::as_str) == Some("REQUIRED"))
    {
        return Err("VOLUME_PLAN_REQUIRED_TASK_MISSING".into());
    }
    Ok(())
}

pub fn compile_volume_plan_from_authority(
    volume_intent_ref: &ArtifactRef,
    volume_intent: &Value,
    goal_contract: &Value,
    project_revision: i64,
) -> Result<Value, String> {
    validate_volume_intent_semantics(volume_intent)
        .map_err(|_| "GOAL_CONTRACT_VOLUME_INTENT_REQUIRED".to_string())?;
    if project_revision < 0 {
        return Err("VOLUME_PLAN_COMPILED_REVISION_REQUIRED".into());
    }
    let source_evidence_refs = volume_intent
        .get("sourceEvidenceRefs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let continuity_refs = volume_intent
        .get("crossVolumeResponsibilityRefs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut tasks = Vec::new();
    let mut previous_required: Option<String> = None;
    for goal in volume_intent
        .get("mustAchieve")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let goal_id = goal
            .get("id")
            .and_then(Value::as_str)
            .ok_or("VOLUME_PLAN_GOAL_ID_REQUIRED")?;
        let task_id = format!("task:required:{goal_id}");
        let predicate = json!({
            "kind":"TASK_EVIDENCE_RECORDED",
            "value":{"taskId":task_id,"goalId":goal_id,"evidenceType":"FINAL_CHAPTER","requiresNewEvidence":true}
        });
        let dependencies = previous_required
            .iter()
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>();
        tasks.push(json!({
            "taskId":task_id,
            "class":"REQUIRED",
            "objective":goal.get("description").cloned().unwrap_or_else(|| json!("完成卷目标")),
            "dependsOnTaskIds":dependencies,
            "continuityThreadRefs":continuity_refs,
            "reservationRefs":[],
            "completionPredicate":predicate,
            "estimatedChapterCost":{"preferred":1}
        }));
        previous_required = Some(task_id);
    }
    let book_plan = goal_contract
        .get("bookPlan")
        .ok_or("GOAL_CONTRACT_BOOK_PLAN_REQUIRED")?;
    if let Some(items) = book_plan
        .get("relationshipMacroDirections")
        .and_then(Value::as_array)
    {
        for item in items.iter().take(8) {
            let id = item.get("id").and_then(Value::as_str).unwrap_or("relation");
            tasks.push(json!({
                "taskId":format!("task:optional:{id}"),
                "class":"OPTIONAL",
                "objective":item.get("description").cloned().unwrap_or_else(|| json!("观察关系变化")),
                "dependsOnTaskIds":[],
                "continuityThreadRefs":continuity_refs,
                "reservationRefs":[],
                "completionPredicate":{"kind":"RELATION_STATE_MATCH","value":{"relationId":id,"expectedTransition":"ADVANCED","requiresNewEvidence":true}},
                "estimatedChapterCost":{"preferred":1}
            }));
        }
    }
    let mut climax_task: Option<String> = None;
    if let Some(items) = book_plan.get("majorMysteryGoals").and_then(Value::as_array) {
        for item in items.iter().take(1) {
            let id = item.get("id").and_then(Value::as_str).unwrap_or("mystery");
            let task_id = format!("task:climax:{id}");
            tasks.push(json!({
                "taskId":task_id,
                "class":"CLIMAX",
                "objective":item.get("description").cloned().unwrap_or_else(|| json!("完成本卷关键回报")),
                "dependsOnTaskIds":previous_required.iter().cloned().map(Value::String).collect::<Vec<_>>(),
                "continuityThreadRefs":continuity_refs,
                "reservationRefs":[],
                "completionPredicate":{"kind":"CANON_EVENT_EXISTS","value":{"eventType":"MYSTERY_RESOLVED","mysteryId":id,"afterPlanCommit":true}},
                "estimatedChapterCost":{"preferred":2}
            }));
            climax_task = Some(task_id);
        }
    }
    let reservations = volume_intent
        .get("futureReservationRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    for reservation in reservations {
        tasks.push(json!({
            "taskId":format!("task:reserved:{reservation}"),
            "class":"RESERVED",
            "objective":format!("保护并在授权窗口处理保留项 {reservation}"),
            "dependsOnTaskIds":previous_required.iter().cloned().map(Value::String).collect::<Vec<_>>(),
            "continuityThreadRefs":continuity_refs,
            "reservationRefs":[reservation],
            "completionPredicate":{"kind":"FUTURE_RESERVATION_STATUS","value":{"reservationId":reservation,"requiredStatus":"FULFILLED"}},
            "estimatedChapterCost":{"preferred":1}
        }));
    }
    if tasks.is_empty() {
        return Err("VOLUME_PLAN_TASKS_REQUIRED".into());
    }
    let required_exit_conditions = tasks
        .iter()
        .filter(|task| {
            matches!(
                task.get("class").and_then(Value::as_str),
                Some("REQUIRED" | "CLIMAX" | "RESERVED")
            )
        })
        .filter_map(|task| task.get("taskId").and_then(Value::as_str))
        .map(|id| json!({"kind":"TASK_SATISFIED","value":id}))
        .collect::<Vec<_>>();
    if required_exit_conditions.is_empty() {
        return Err("VOLUME_PLAN_EXIT_CONDITIONS_REQUIRED".into());
    }
    let mut route_invariants = volume_intent
        .get("mustNotBreak")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("description").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    route_invariants.push("所有任务必须保持 Goal Contract 的 authority 依赖".into());
    route_invariants.push("Task Reality 只能从运行时状态投影读取，不写入本计划".into());
    let chapter_range = volume_intent
        .get("targetChapterRange")
        .cloned()
        .ok_or("VOLUME_PLAN_CAPACITY_REQUIRED")?;
    let min_chapters = chapter_range
        .get("min")
        .and_then(Value::as_i64)
        .unwrap_or(1)
        .max(1);
    let preferred_chapters = chapter_range
        .get("preferred")
        .and_then(Value::as_i64)
        .unwrap_or(min_chapters)
        .max(min_chapters);
    let max_chapters = chapter_range
        .get("max")
        .and_then(Value::as_i64)
        .unwrap_or(preferred_chapters)
        .max(preferred_chapters);
    let semantic_digest = sha256_canonical(&json!({"intent":volume_intent,"goalContract":goal_contract,"tasks":tasks,"revision":project_revision})).map_err(|error| error.to_string())?;
    let payload = json!({
        "payloadSchema":"volume-plan.r1",
        "volumeIntentRef":volume_intent_ref,
        "routeVersion":1,
        "tasks":tasks,
        "routeInvariants":route_invariants,
        "requiredExitConditions":required_exit_conditions,
        "capacityBudget":{"minChapters":min_chapters,"preferredChapters":preferred_chapters,"maxChapters":max_chapters},
        "goalContractRefs":[goal_contract["refs"]["storyConstitution"].clone(),goal_contract["refs"]["mainContract"].clone(),goal_contract["refs"]["bookPlan"].clone(),goal_contract["refs"]["volumeIntent"].clone()],
        "compiledFromProjectRevision":project_revision,
        "sourceEvidenceRefs":source_evidence_refs,
        "semanticDigest":semantic_digest,
        "climaxReservation":climax_task.map(|task_id| json!({"taskId":task_id,"minRemainingCapacity":1}))
    });
    validate_volume_plan_semantics(&payload)?;
    Ok(payload)
}

/// Normalizes an explicitly supplied route against the current Goal Contract.
/// The product path normally supplies no plan and uses the deterministic
/// compiler above; this compatibility normalizer keeps older callers bounded
/// while still preventing empty invariants, exit conditions, or predicates.
pub fn normalize_volume_plan_candidate(
    mut candidate: Value,
    volume_intent_ref: &ArtifactRef,
    volume_intent: &Value,
    goal_contract: &Value,
    project_revision: i64,
) -> Result<Value, String> {
    if !candidate.is_object() {
        return Err("VOLUME_PLAN_REQUIRED".into());
    }
    candidate["payloadSchema"] = json!("volume-plan.r1");
    candidate["volumeIntentRef"] = json!(volume_intent_ref);
    if candidate
        .get("routeVersion")
        .and_then(Value::as_i64)
        .is_none_or(|version| version < 1)
    {
        candidate["routeVersion"] = json!(1);
    }
    {
        let tasks = candidate
            .get_mut("tasks")
            .and_then(Value::as_array_mut)
            .ok_or("VOLUME_PLAN_TASKS_REQUIRED")?;
        for task in tasks.iter_mut() {
            let id = task_id(task)?.to_owned();
            let predicate_kind = task
                .get("completionPredicate")
                .and_then(Value::as_object)
                .and_then(|predicate| predicate.get("kind"))
                .and_then(Value::as_str);
            if predicate_kind == Some("DONE") || predicate_kind.is_none() {
                task["completionPredicate"] = json!({
                    "kind":"TASK_EVIDENCE_RECORDED",
                    "value":{"taskId":id,"evidenceType":"FINAL_CHAPTER","requiresNewEvidence":true}
                });
            }
            if task.get("continuityThreadRefs").is_none() {
                task["continuityThreadRefs"] = json!([]);
            }
            if task.get("reservationRefs").is_none() {
                task["reservationRefs"] = json!([]);
            }
            if task.get("dependsOnTaskIds").is_none() {
                task["dependsOnTaskIds"] = json!([]);
            }
            if task.get("estimatedChapterCost").is_none() {
                task["estimatedChapterCost"] = json!({"preferred":1});
            }
        }
    }
    if candidate
        .get("routeInvariants")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        candidate["routeInvariants"] = json!([
            "所有任务必须遵循当前 Goal Contract 的硬约束",
            "Task Reality 由正式运行状态投影提供，不写入 VolumePlan"
        ]);
        if let Some(items) = volume_intent.get("mustNotBreak").and_then(Value::as_array) {
            for item in items.iter().take(8) {
                if let Some(description) = item.get("description").and_then(Value::as_str) {
                    candidate["routeInvariants"]
                        .as_array_mut()
                        .expect("route invariants array")
                        .push(json!(description));
                }
            }
        }
    }
    if candidate
        .get("requiredExitConditions")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        let task_ids = candidate
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
            .filter_map(|task| task.get("taskId").and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        candidate["requiredExitConditions"] = json!(task_ids
            .iter()
            .map(|id| json!({"kind":"TASK_SATISFIED","value":id}))
            .collect::<Vec<_>>());
    }
    if candidate.get("capacityBudget").is_none() {
        candidate["capacityBudget"] = volume_intent
            .get("targetChapterRange")
            .cloned()
            .map(|range| {
                json!({
                    "minChapters":range.get("min").and_then(Value::as_i64).unwrap_or(1).max(1),
                    "preferredChapters":range.get("preferred").and_then(Value::as_i64).unwrap_or(1).max(1),
                    "maxChapters":range.get("max").and_then(Value::as_i64).unwrap_or(1).max(1)
                })
            })
            .unwrap_or_else(|| json!({"minChapters":1,"preferredChapters":1,"maxChapters":1}));
    }
    let goal_contract_refs = [
        "storyConstitution",
        "mainContract",
        "bookPlan",
        "volumeIntent",
    ]
    .into_iter()
    .filter_map(|key| goal_contract.get("refs")?.get(key).cloned())
    .filter(|reference| !reference.is_null())
    .collect::<Vec<_>>();
    candidate["goalContractRefs"] = json!(goal_contract_refs);
    candidate["compiledFromProjectRevision"] = json!(project_revision);
    candidate["sourceEvidenceRefs"] = volume_intent
        .get("sourceEvidenceRefs")
        .cloned()
        .unwrap_or_else(|| json!([]));
    candidate["semanticDigest"] =
        json!(sha256_canonical(&candidate).map_err(|error| error.to_string())?);
    validate_volume_plan_semantics(&candidate)?;
    Ok(candidate)
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

/// The typed planning mutation returned to an outer B12 transaction.  The
/// owner performs all current-head and plan semantics here; B12 only embeds
/// this exact result in its CommitBundle.
#[derive(Debug, Clone)]
pub struct PlanningChangeMutation {
    pub artifact: ArtifactEnvelope,
    pub head: HeadChange,
    pub owner_receipt: Value,
}

pub struct PlanningOwner;

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

#[derive(Debug, Clone)]
struct ChapterPlanChangeSemantics {
    route_task_refs_checked: usize,
    triggered_continuity_checked: usize,
    reservation_refs_checked: usize,
    stale_refs_checked: usize,
}

fn validate_chapter_plan_change_semantics(
    connection: &Connection,
    project_id: &str,
    candidate: &Value,
    expected_scope: &Value,
) -> Result<ChapterPlanChangeSemantics, String> {
    let chapter_id = expected_scope
        .get("chapterId")
        .and_then(Value::as_str)
        .ok_or("CHAPTER_PLAN_SCOPE_REQUIRED")?;
    let context_ref: ArtifactRef = serde_json::from_value(
        candidate
            .get("contextManifestRef")
            .cloned()
            .ok_or("CONTEXT_MANIFEST_REF_REQUIRED")?,
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
    let volume_id = crate::repositories::narrative::NarrativeRepository::chapter_volume_id(
        connection, project_id, chapter_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or("CHAPTER_NOT_FOUND")?;
    let route_ref = crate::repositories::narrative::NarrativeRepository::volume_plan_route(
        connection, project_id, &volume_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or("VOLUME_PLAN_NOT_FOUND")?;
    if crate::repositories::change_retcon::has_pending_stale_for_refs(
        connection,
        project_id,
        &[
            context_ref.artifact_id.as_str(),
            route_ref.artifact_id.as_str(),
        ],
    )
    .map_err(|error| error.to_string())?
    {
        return Err("PLAN_STALE".into());
    }
    let route = crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
        connection, project_id, &route_ref,
    )
    .map_err(|error| error.to_string())?
    .ok_or("VOLUME_PLAN_PAYLOAD_NOT_FOUND")?;
    let route_task_ids = route
        .get("tasks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|task| task.get("taskId").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let project_revision = crate::repositories::narrative::NarrativeRepository::project_revision(
        connection, project_id,
    )
    .map_err(|error| error.to_string())?;
    let current_chapter_no = crate::repositories::narrative::NarrativeRepository::chapter_number(
        connection, project_id, chapter_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or("CHAPTER_NOT_FOUND")?;
    let context = crate::commands::context_input_from_authority(
        connection,
        &json!({
            "chapterId":chapter_id,
            "volumeId":volume_id,
            "currentVolumeId":volume_id,
            "currentChapter":current_chapter_no,
            "executionMode":"LONG_FORM"
        }),
        project_id,
        project_revision,
    )?;
    let triggered_threads = context
        .continuity_threads
        .iter()
        .filter(|thread| matches!(thread.authority_level.as_str(), "A" | "B"))
        .map(|thread| thread.thread_id.clone())
        .collect::<Vec<_>>();
    validate_current_chapter_plan(candidate, chapter_id, &route_task_ids, &triggered_threads)?;
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
    validate_task_reality(candidate, &route, &states)?;
    let current_volume_no = crate::repositories::narrative::NarrativeRepository::volume_number(
        connection, project_id, &volume_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or("VOLUME_NOT_FOUND")?;
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
    validate_reservation_actions(candidate, &reservations, current_volume_no)?;
    Ok(ChapterPlanChangeSemantics {
        route_task_refs_checked: route_task_ids.len(),
        triggered_continuity_checked: triggered_threads.len(),
        reservation_refs_checked: reservations.len(),
        stale_refs_checked: 2,
    })
}

#[allow(clippy::too_many_arguments)]
impl PlanningOwner {
    /// Applies one approved typed plan operation without committing it.  The
    /// caller must later put the returned artifact and head into the same
    /// CommitBundle as the surrounding B12 lifecycle changes.
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
    ) -> Result<PlanningChangeMutation, String> {
        if !matches!(artifact_type, "BookPlan" | "ChapterPlan") {
            return Err(format!("B12_CHANGE_TARGET_UNSUPPORTED:{artifact_type}"));
        }
        if !patch.is_object() {
            return Err("B12_CHANGE_TARGET_PATCH_REQUIRED".into());
        }
        let current = crate::repositories::change_retcon::load_authority_head(
            connection,
            project_id,
            Some(artifact_type),
            Some(artifact_id),
            None,
            None,
        )
        .map_err(|error| format!("B07_DB:{error}"))?
        .ok_or_else(|| "B07_CHANGE_TARGET_NOT_FOUND".to_string())?;
        let current_scope: Value = serde_json::from_str(&current.scope_key)
            .map_err(|error| format!("B07_CHANGE_SCOPE_INVALID:{error}"))?;
        if sha256_canonical(&current_scope).map_err(|error| error.to_string())?
            != sha256_canonical(expected_scope).map_err(|error| error.to_string())?
        {
            return Err("B07_CHANGE_SCOPE_STALE".into());
        }
        if current.version != expected_version || current.payload_hash != expected_payload_hash {
            return Err("B07_CHANGE_TARGET_STALE".into());
        }
        let current_hash = sha256_canonical(&current.payload).map_err(|error| error.to_string())?;
        if current_hash != current.payload_hash {
            return Err("B07_CHANGE_HEAD_PAYLOAD_MISMATCH".into());
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
        let chapter_semantics = (artifact_type == "ChapterPlan")
            .then(|| {
                validate_chapter_plan_change_semantics(
                    connection,
                    project_id,
                    &candidate,
                    expected_scope,
                )
            })
            .transpose()?;
        let payload_hash = sha256_canonical(&candidate).map_err(|error| error.to_string())?;
        let artifact: ArtifactEnvelope = serde_json::from_value(json!({
            "artifactId":artifact_id,
            "projectId":project_id,
            "artifactType":artifact_type,
            "scope":expected_scope,
            "version":current.version + 1,
            "schemaVersion":"m0a.artifact.v1",
            "artifactStatus":"CANDIDATE",
            "semanticLifecycle":"APPROVED_PLAN",
            "authorityLevel":"B",
            "payload":candidate,
            "payloadHash":payload_hash,
            "dependencyRefs":[],
            "provenance":{"createdBy":"B07::PlanningOwner","commandId":command_id,"decisionRefs":[decision_id]},
            "createdAt":now
        }))
        .map_err(|error| format!("B07_ARTIFACT:{error}"))?;
        let head: HeadChange = serde_json::from_value(json!({
            "artifactType":artifact_type,
            "scope":expected_scope,
            "artifactId":artifact.artifact_id,
            "version":artifact.version,
            "payloadHash":artifact.payload_hash
        }))
        .map_err(|error| format!("B07_HEAD:{error}"))?;
        let owner_receipt = json!({
            "owner":"B07::PlanningOwner",
            "operation":"apply_typed_change",
            "artifactType":artifact_type,
            "artifactId":artifact_id,
            "sourceHead":{"version":current.version,"payloadHash":current.payload_hash},
            "resultHead":{"version":artifact.version,"payloadHash":artifact.payload_hash},
            "invocationCount":1,
            "directB12HeadMutationCount":0,
            "semanticValidation":"PASS",
            "routeTaskRefsChecked":chapter_semantics.as_ref().map(|value| value.route_task_refs_checked).unwrap_or(0),
            "triggeredContinuityChecked":chapter_semantics.as_ref().map(|value| value.triggered_continuity_checked).unwrap_or(0),
            "reservationRefsChecked":chapter_semantics.as_ref().map(|value| value.reservation_refs_checked).unwrap_or(0),
            "taskRealityChecked":chapter_semantics.is_some(),
            "staleRefsChecked":chapter_semantics.as_ref().map(|value| value.stale_refs_checked).unwrap_or(0)
        });
        Ok(PlanningChangeMutation {
            artifact,
            head,
            owner_receipt,
        })
    }
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
    let readiness_gap = required_remaining
        .saturating_mul(40)
        .saturating_add(blocked_required.saturating_mul(40))
        .saturating_add(exit_gap.saturating_mul(20));
    let climax_readiness = 100_i64.saturating_sub(readiness_gap.min(100));
    let continuity_aging = overdue_threads.saturating_mul(current_chapter.max(1));
    json!({
        "remainingChapterCapacity":remaining,
        "requiredTasksRemaining":required_remaining,
        "blockedRequiredTasks":blocked_required,
        "exitConditionGap":exit_gap,
        "overdueThreadPressure":overdue_threads,
        "continuityAging":continuity_aging,
        "climaxReadiness":climax_readiness,
        "reservationPressure":reservations,
        "priorityScore":score
    })
}

/// Inputs for the production planning pressure projection. Every field is
/// sourced from the authoritative route/reality/continuity readers; no plan
/// task status or shorthand `threadRef` is accepted as a substitute.
pub struct PlanningPressureInput<'a> {
    pub current_chapter: i64,
    pub max_chapters: i64,
    pub route_tasks: &'a [Value],
    pub task_reality: &'a [Value],
    pub continuity_threads: &'a [Value],
    pub reservations: &'a [Value],
    pub exit_conditions: &'a [Value],
}

/// Resolve the structured FutureReservation authority used by pressure and
/// chapter planning. A route's `reservationRefs` are only identifiers; they
/// are never treated as pressure by themselves.
pub fn future_reservation_authority(book_plan: &Value, intent: &Value) -> Vec<Value> {
    let allowed = intent
        .get("futureReservationRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.as_str()
                .or_else(|| item.get("reservationId").and_then(Value::as_str))
        })
        .collect::<BTreeSet<_>>();
    book_plan
        .get("futureReservations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|reservation| {
            reservation
                .get("reservationId")
                .and_then(Value::as_str)
                .is_some_and(|id| allowed.is_empty() || allowed.contains(id))
        })
        .map(|reservation| {
            let mut value = reservation.clone();
            if value.get("status").is_none() && value.get("lifecycle").is_none() {
                value["status"] = json!("DECLARED");
            }
            value
        })
        .collect()
}

/// Build one pressure projection for every production caller. Route task
/// refs and intent refs are merged here, then resolved against the supplied
/// authority projections, so VolumeProgress and VolumeLifecycle cannot drift.
pub fn compile_planning_pressure(
    current_chapter: i64,
    max_chapters: i64,
    route: &Value,
    intent: &Value,
    task_reality: &[Value],
    continuity_authority: &[Value],
    reservation_authority: &[Value],
) -> Value {
    let route_tasks = route
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let continuity_refs = intent
        .get("crossVolumeResponsibilityRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .chain(route_tasks.iter().flat_map(|task| {
            task.get("continuityThreadRefs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
        }))
        .collect::<BTreeSet<_>>();
    let continuity = continuity_authority
        .iter()
        .filter(|thread| {
            thread
                .get("threadId")
                .or_else(|| thread.get("threadRef"))
                .and_then(Value::as_str)
                .is_some_and(|id| continuity_refs.contains(id))
        })
        .cloned()
        .collect::<Vec<_>>();
    let reservation_refs = route_tasks
        .iter()
        .flat_map(|task| {
            task.get("reservationRefs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
        })
        .chain(
            intent
                .get("futureReservationRefs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
        .collect::<BTreeSet<_>>();
    let reservations = reservation_authority
        .iter()
        .filter(|reservation| {
            reservation
                .get("reservationId")
                .and_then(Value::as_str)
                .is_some_and(|id| reservation_refs.is_empty() || reservation_refs.contains(id))
        })
        .cloned()
        .collect::<Vec<_>>();
    planning_pressure_from_authority(PlanningPressureInput {
        current_chapter,
        max_chapters,
        route_tasks: &route_tasks,
        task_reality,
        continuity_threads: &continuity,
        reservations: &reservations,
        exit_conditions: route
            .get("requiredExitConditions")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    })
}

/// Calculate pressure from the actual B07/B06 authority inputs. The legacy
/// `planning_pressure` helper remains for frozen compatibility tests, while
/// product projections call this function so CLIMAX readiness, thread age,
/// deadlines, and task status all have explicit sources.
pub fn planning_pressure_from_authority(input: PlanningPressureInput<'_>) -> Value {
    let remaining_capacity = (input.max_chapters - input.current_chapter).max(0);
    let status_by_task = input
        .task_reality
        .iter()
        .filter_map(|state| {
            let id = state
                .get("taskId")
                .or_else(|| state.get("stateId"))
                .and_then(Value::as_str)?;
            let status = state
                .get("status")
                .or_else(|| state.pointer("/value/status"))
                .and_then(Value::as_str)
                .unwrap_or("NOT_STARTED");
            Some((id.to_string(), status.to_string()))
        })
        .collect::<BTreeMap<_, _>>();
    let task_status = |task_id: &str| {
        status_by_task
            .get(task_id)
            .map(String::as_str)
            .unwrap_or("NOT_STARTED")
    };
    let is_required = |task: &Value| {
        matches!(
            task.get("class").and_then(Value::as_str),
            Some("REQUIRED" | "CLIMAX" | "RESERVED")
        )
    };
    let required_remaining = input
        .route_tasks
        .iter()
        .filter(|task| is_required(task))
        .filter(|task| {
            task.get("taskId")
                .and_then(Value::as_str)
                .is_none_or(|id| task_status(id) != "SATISFIED")
        })
        .count() as i64;
    let blocked_required = input
        .route_tasks
        .iter()
        .filter(|task| is_required(task))
        .filter(|task| {
            task.get("taskId")
                .and_then(Value::as_str)
                .is_some_and(|id| task_status(id) == "BLOCKED")
        })
        .count() as i64;
    let exit_gap = input
        .exit_conditions
        .iter()
        .filter(|condition| {
            if condition.get("kind").and_then(Value::as_str) == Some("TASK_SATISFIED") {
                return condition
                    .get("value")
                    .and_then(Value::as_str)
                    .is_none_or(|task_id| task_status(task_id) != "SATISFIED");
            }
            true
        })
        .count() as i64;

    let climax_tasks = input
        .route_tasks
        .iter()
        .filter(|task| task.get("class").and_then(Value::as_str) == Some("CLIMAX"))
        .collect::<Vec<_>>();
    let climax_blocked_prerequisites = climax_tasks
        .iter()
        .map(|task| {
            task.get("dependsOnTaskIds")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|task_id| task_status(task_id) != "SATISFIED")
                .count() as i64
        })
        .sum::<i64>();
    let climax_remaining_cost = climax_tasks
        .iter()
        .filter(|task| {
            task.get("taskId")
                .and_then(Value::as_str)
                .is_none_or(|id| task_status(id) != "SATISFIED")
        })
        .map(|task| {
            task.pointer("/estimatedChapterCost/preferred")
                .and_then(Value::as_i64)
                .unwrap_or(1)
        })
        .sum::<i64>();
    let climax_readiness = if climax_tasks.is_empty() {
        Value::Null
    } else if climax_blocked_prerequisites > 0 || climax_remaining_cost > remaining_capacity {
        json!(0)
    } else if climax_remaining_cost == 0 {
        json!(100)
    } else {
        json!(75)
    };

    let mut overdue_threads = 0_i64;
    let mut continuity_aging = 0_i64;
    let mut aged_threads = 0_i64;
    for thread in input.continuity_threads {
        let status = thread
            .get("logicalStatus")
            .or_else(|| thread.get("status"))
            .or_else(|| thread.get("lifecycle"))
            .and_then(Value::as_str)
            .unwrap_or("OPEN");
        if matches!(status, "RESOLVED" | "ABANDONED" | "SUPERSEDED") {
            continue;
        }
        let deadline = thread
            .get("mustResolveBeforeChapter")
            .or_else(|| thread.get("deadline"))
            .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()));
        if deadline.is_some_and(|value| value < input.current_chapter) {
            overdue_threads += 1;
        }
        if let Some(last_touched) = thread
            .get("lastTouchedChapter")
            .or_else(|| thread.get("lastTouchedChapterNo"))
            .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
        {
            continuity_aging += (input.current_chapter - last_touched).max(0);
            aged_threads += 1;
        }
    }
    let reservation_pressure = input
        .reservations
        .iter()
        .filter(|reservation| reservation.is_object())
        .filter(|reservation| {
            reservation
                .get("status")
                .or_else(|| reservation.get("lifecycle"))
                .and_then(Value::as_str)
                .is_none_or(|status| !matches!(status, "RESOLVED" | "SATISFIED" | "ABANDONED"))
        })
        .count() as i64;
    let priority_score = required_remaining.saturating_mul(4)
        + blocked_required.saturating_mul(6)
        + exit_gap.saturating_mul(5)
        + overdue_threads.saturating_mul(3)
        + reservation_pressure.saturating_mul(2)
        + if remaining_capacity <= 8 { 8 } else { 0 };
    json!({
        "remainingChapterCapacity": remaining_capacity,
        "requiredTasksRemaining": required_remaining,
        "blockedRequiredTasks": blocked_required,
        "exitConditionGap": exit_gap,
        "overdueThreadPressure": overdue_threads,
        "continuityAging": continuity_aging,
        "continuityAgedThreads": aged_threads,
        "climaxReadiness": climax_readiness,
        "climaxTaskCount": climax_tasks.len() as i64,
        "climaxBlockedPrerequisites": climax_blocked_prerequisites,
        "climaxRemainingChapterCost": climax_remaining_cost,
        "reservationPressure": reservation_pressure,
        "priorityScore": priority_score
    })
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
    let mut bound_plan = plan.clone();
    let hard_scope = crate::narrative::context_hard_scope(&context);
    bound_plan["activeTaskRefs"] = hard_scope["taskRefs"].clone();
    bound_plan["entityRefs"] = hard_scope["entityRefs"].clone();
    bound_plan["reservationRefs"] = hard_scope["reservationRefs"].clone();
    bound_plan["hardScope"] = hard_scope;
    validate_current_chapter_plan(&bound_plan, chapter_id, &route_task_ids, &triggered_threads)?;
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
        validate_task_reality(&bound_plan, route, &states)?;
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
    validate_reservation_actions(&bound_plan, &reservations, current_volume_no)?;
    let artifact_id = format!("plan:chapterplan:{chapter_id}");
    let version = crate::repositories::narrative::NarrativeRepository::next_artifact_version(
        connection,
        project_id,
        &artifact_id,
    )
    .map_err(|error| error.to_string())?;
    let hash = sha256_canonical(&bound_plan).map_err(|error| error.to_string())?;
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId":artifact_id,"projectId":project_id,"artifactType":"ChapterPlan",
        "scope":{"level":"CHAPTER","chapterId":chapter_id},"version":version,
        "schemaVersion":"m0a.artifact.v1","artifactStatus":"CANDIDATE","payload":bound_plan,
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
    fn model_plan_normalization_strips_contract_extras() {
        let normalized = normalize_chapter_plan_model_output(
            json!({
                "chapterGoal":"推进",
                "activeTaskRefs":[],
                "continuityActions":[],
                "requiredStateOutcomes":[],
                "forbiddenTransitions":[],
                "knowledgeBoundaries":[],
                "reservationRefs":[],
                "sceneExecution":[],
                "horizonPreview":[],
                "from":"hallucinated"
            }),
            &[],
            &json!({
                "chapterId":"chapter:bound",
                "compiledFromProjectRevision":7,
                "compiledFromStateDigest":"a".repeat(64),
                "contextManifestRef":{"artifactId":"context:bound","version":1,"payloadHash":"b".repeat(64)}
            }),
        );
        assert!(normalized.get("from").is_none());
    }

    #[test]
    fn model_plan_normalization_repairs_only_typed_container_shapes() {
        let plan = json!({
            "requiredStateOutcomes":["完成首次委托"],
            "forbiddenTransitions":["不得揭晓幕后者"],
            "continuityActions":["继续追查"],
            "horizonPreview":["发现代价","主动追查","锁定线索"]
        });
        let bindings = json!({
            "chapterId":"chapter:bound",
            "compiledFromProjectRevision":7,
            "compiledFromStateDigest":"a".repeat(64),
            "contextManifestRef":{"artifactId":"context:bound","version":1,"payloadHash":"b".repeat(64)}
        });
        let normalized = normalize_chapter_plan_model_output(plan, &[], &bindings);
        assert_eq!(normalized["payloadSchema"], "chapter-plan.r1");
        assert_eq!(normalized["chapterId"], "chapter:bound");
        assert_eq!(normalized["compiledFromProjectRevision"], 7);
        assert_eq!(
            normalized["requiredStateOutcomes"][0]["kind"],
            "MODEL_STATED"
        );
        assert_eq!(
            normalized["forbiddenTransitions"][0]["value"],
            "不得揭晓幕后者"
        );
        assert_eq!(normalized["continuityActions"], json!([]));
        assert_eq!(normalized["horizonPreview"][0]["slotNo"], 1);
        assert_eq!(normalized["horizonPreview"][0]["tentative"], true);

        let knowledge_boundary = normalize_chapter_plan_model_output(
            json!({"knowledgeBoundaries":[{"entityId":"entity:hero","factId":"fact:secret","state":"BELIEVES_TRUE","constraint":"不得越权知晓"}]}),
            &[],
            &bindings,
        );
        assert!(knowledge_boundary["knowledgeBoundaries"][0].is_string());
        assert!(knowledge_boundary["knowledgeBoundaries"][0]
            .as_str()
            .unwrap()
            .contains("fact:secret"));

        let with_required_thread = normalize_chapter_plan_model_output(
            json!({"continuityActions":["不能推断对应线程"]}),
            &["thread:required".into()],
            &bindings,
        );
        assert_eq!(
            with_required_thread["continuityActions"][0],
            "不能推断对应线程"
        );

        let aliased = normalize_chapter_plan_model_output(
            json!({
                "requiredStateOutcomes":[{"outcomeId":"FIRST_CASE","stateKey":"case.status","value":"DONE","description":"完成委托"}],
                "forbiddenTransitions":[{"transitionId":"NO_REVEAL","description":"不得揭晓"}],
                "horizonPreview":[{"horizonId":"H1","description":"追查线索","refs":[]}]
            }),
            &[],
            &bindings,
        );
        let typed = normalize_chapter_plan_model_output(
            json!({"requiredStateOutcomes":[{"type":"FACT_VERIFICATION","description":"必须有新证据"}]}),
            &[],
            &bindings,
        );
        assert_eq!(
            typed["requiredStateOutcomes"][0]["kind"],
            "FACT_VERIFICATION"
        );
        assert_eq!(typed["requiredStateOutcomes"][0]["value"], "必须有新证据");
        assert_eq!(
            aliased["requiredStateOutcomes"][0],
            json!({"kind":"case.status","value":"DONE"})
        );
        assert_eq!(
            aliased["forbiddenTransitions"][0],
            json!({"kind":"NO_REVEAL","value":"不得揭晓"})
        );
        assert_eq!(
            aliased["horizonPreview"][0],
            json!({"slotNo":1,"objective":"追查线索","tentative":true})
        );
        assert_eq!(aliased["horizonPreview"].as_array().unwrap().len(), 3);
        assert!(aliased["horizonPreview"]
            .as_array()
            .unwrap()
            .iter()
            .all(|slot| slot["tentative"] == true));
    }

    #[test]
    fn model_plan_normalization_drops_unbound_continuity_and_horizon_items_without_threads() {
        let normalized = normalize_chapter_plan_model_output(
            json!({
                "continuityActions":[{"type":"STATE_INHERITANCE","description":"继承状态"}],
                "horizonPreview":[{"type":"LOCATION_SHIFT","target":"未知地点"}]
            }),
            &[],
            &json!({
                "chapterId":"chapter:bound",
                "compiledFromProjectRevision":7,
                "compiledFromStateDigest":"a".repeat(64),
                "contextManifestRef":{"artifactId":"context:bound","version":1,"payloadHash":"b".repeat(64)}
            }),
        );
        assert_eq!(normalized["continuityActions"], json!([]));
        assert_eq!(normalized["horizonPreview"].as_array().unwrap().len(), 3);
        assert!(normalized["horizonPreview"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["tentative"] == true && item["objective"].is_string()));
    }

    #[test]
    fn model_plan_normalization_wraps_untyped_outcome_objects_as_model_stated() {
        let normalized = normalize_chapter_plan_model_output(
            json!({
                "requiredStateOutcomes":[{"factId":"fact:new","predicate":"OBSERVED","objectJson":{"ok":true}}],
                "forbiddenTransitions":[{"target":"entity:unknown","reason":"不得越权"}]
            }),
            &[],
            &json!({
                "chapterId":"chapter:bound",
                "compiledFromProjectRevision":7,
                "compiledFromStateDigest":"a".repeat(64),
                "contextManifestRef":{"artifactId":"context:bound","version":1,"payloadHash":"b".repeat(64)}
            }),
        );
        assert_eq!(
            normalized["requiredStateOutcomes"][0]["kind"],
            "MODEL_STATED"
        );
        assert!(normalized["requiredStateOutcomes"][0]["value"]["factId"].is_string());
        assert_eq!(
            normalized["forbiddenTransitions"][0]["kind"],
            "MODEL_STATED"
        );
        assert!(normalized["forbiddenTransitions"][0]["value"]["reason"].is_string());
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
            approve_book_plan(&seed, false, &[], &[], &OpeningActualState::default()),
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
        assert_eq!(calm["climaxReadiness"], 60);
        assert_eq!(calm["continuityAging"], 0);
    }

    #[test]
    fn authority_pressure_reads_climax_reality_and_thread_timing() {
        let tasks = vec![
            json!({"taskId":"task:climax","class":"CLIMAX","dependsOnTaskIds":["task:prereq"],"estimatedChapterCost":{"preferred":2}}),
            json!({"taskId":"task:prereq","class":"REQUIRED","dependsOnTaskIds":[],"estimatedChapterCost":{"preferred":1}}),
        ];
        let reality = vec![
            json!({"stateId":"task:climax","value":{"status":"NOT_STARTED"}}),
            json!({"stateId":"task:prereq","value":{"status":"BLOCKED"}}),
        ];
        let threads = vec![json!({
            "threadId":"thread:promise",
            "lifecycle":"PROGRESSING",
            "mustResolveBeforeChapter":3,
            "lastTouchedChapterNo":1
        })];
        let pressure = planning_pressure_from_authority(PlanningPressureInput {
            current_chapter: 4,
            max_chapters: 8,
            route_tasks: &tasks,
            task_reality: &reality,
            continuity_threads: &threads,
            reservations: &[],
            exit_conditions: &[],
        });
        assert_eq!(pressure["climaxReadiness"], 0);
        assert_eq!(pressure["climaxBlockedPrerequisites"], 1);
        assert_eq!(pressure["overdueThreadPressure"], 1);
        assert_eq!(pressure["continuityAging"], 3);
    }

    #[test]
    fn authority_pressure_without_climax_is_schema_neutral() {
        let pressure = planning_pressure_from_authority(PlanningPressureInput {
            current_chapter: 1,
            max_chapters: 10,
            route_tasks: &[],
            task_reality: &[],
            continuity_threads: &[],
            reservations: &[],
            exit_conditions: &[],
        });
        assert!(pressure["climaxReadiness"].is_null());
        assert_eq!(pressure["climaxTaskCount"], 0);
    }

    #[test]
    fn pressure_does_not_count_ref_only_reservations() {
        let route = json!({"tasks":[],"requiredExitConditions":[]});
        let intent = json!({"futureReservationRefs":["reservation:future"]});
        let ref_only = vec![json!("reservation:future")];
        let object = vec![
            json!({"reservationId":"reservation:future","status":"DECLARED","mayReveal":false}),
        ];
        assert_eq!(
            compile_planning_pressure(1, 10, &route, &intent, &[], &[], &ref_only)
                ["reservationPressure"],
            0
        );
        assert_eq!(
            compile_planning_pressure(1, 10, &route, &intent, &[], &[], &object)
                ["reservationPressure"],
            1
        );
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
