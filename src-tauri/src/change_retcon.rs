//! B12 Change / Retcon Runtime owner.
//!
//! The owner is deliberately narrow: it validates the request, performs a
//! bounded dependency preflight/expansion, records an explicit user decision,
//! and assembles the final authority mutation as a CommitBundle V2.  SQL
//! persistence primitives live in `repositories::change_retcon`; UI, Harness,
//! and Skill code cannot write these tables or Canon directly.

#![allow(clippy::too_many_arguments)]

use crate::commit::CommitService;
use crate::generated::types::{CommitBundle, CommitReceipt};
use crate::hash::{canonical_bytes, sha256_canonical};
use crate::project_db::{ManagerError, ProjectCommandOutcome, ProjectCommandSpec, ProjectManager};
use crate::repositories::change_retcon as repository;
use crate::repositories::narrative::NarrativeRepository;
use libsql_rusqlite::Connection;
use serde_json::{json, Value};

pub const OWNER_NAME: &str = "B12::ChangeRetconOwner";
const CONTRACT_VERSION: &str = "m0a.v1";
const MAX_IMPACT_PAGE: i64 = 100;
const GLOBAL_FAN_OUT_THRESHOLD: i64 = 100;

fn payload(request: &Value) -> Result<&Value, String> {
    request
        .get("payload")
        .filter(|value| value.is_object())
        .ok_or_else(|| "B12_INVALID:payload".into())
}

fn required_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|item| !item.trim().is_empty())
        .ok_or_else(|| format!("B12_INVALID:{field}"))
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|item| !item.trim().is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn manager_error(error: ManagerError) -> String {
    match error {
        ManagerError::ProjectCommand(message) => message,
        ManagerError::RevisionConflict => "REVISION_CONFLICT".into(),
        ManagerError::IdempotencyConflict => "IDEMPOTENCY_CONFLICT".into(),
        other => other.to_string(),
    }
}

fn normalize_owner_error(error: ManagerError) -> String {
    let mut message = manager_error(error);
    for prefix in ["project command apply failed: ", "project command: "] {
        while let Some(rest) = message.strip_prefix(prefix) {
            message = rest.to_string();
        }
    }
    message
}

fn project_revision(connection: &Connection, project_id: &str) -> Result<i64, String> {
    NarrativeRepository::project_revision(connection, project_id)
        .map_err(|error| format!("B12_DB:{error}"))
}

fn control_response(
    receipt: &crate::generated::types::ProjectCommandReceipt,
    result: Value,
    message: &str,
) -> Value {
    json!({
        "commandId": receipt.command_id,
        "projectId": receipt.project_id,
        "resultProjectId": receipt.result_project_id,
        "finalStatus": "committed",
        "message": message,
        "receiptId": receipt.receipt_id,
        "beforeRevision": receipt.before_revision,
        "afterRevision": receipt.after_revision,
        "result": result
    })
}

fn control_command(
    manager: &ProjectManager,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    command_type: &str,
    request_digest: &str,
    expected_revision: Option<i64>,
    requested_at: &str,
    apply: impl FnOnce(&Connection) -> Result<Value, String>,
) -> Result<Value, String> {
    let outcome = manager
        .commit_project_command(
            ProjectCommandSpec {
                project_id,
                result_project_id: project_id,
                command_id,
                idempotency_key,
                command_type,
                request_digest,
                expected_revision,
                now: requested_at,
                advance_revision: false,
            },
            |connection| apply(connection).map_err(ManagerError::ProjectCommand),
        )
        .map_err(normalize_owner_error)?;
    match outcome {
        ProjectCommandOutcome::Committed { value, receipt } => Ok(control_response(
            &receipt,
            value,
            "B12 变更记录已保存，等待下一步确认。",
        )),
        ProjectCommandOutcome::Replayed(receipt) => Ok(control_response(
            &receipt,
            json!({"status":"REPLAYED"}),
            "B12 变更请求已按原记录重放。",
        )),
    }
}

fn merge_seed(seed: &Value, additions: &[(&str, Value)]) -> Value {
    let mut object = seed.as_object().cloned().unwrap_or_default();
    for (key, value) in additions {
        object.insert((*key).into(), value.clone());
    }
    Value::Object(object)
}

fn source_refs(seed: &Value) -> Value {
    if let Some(refs) = seed.get("sourceRefs").and_then(Value::as_array) {
        return Value::Array(refs.clone());
    }
    seed.get("targetRefs")
        .and_then(Value::as_array)
        .cloned()
        .map(Value::Array)
        .unwrap_or_else(|| json!([]))
}

fn source_digest(
    project_id: &str,
    change_request_id: &str,
    based_on_revision: i64,
    scope_class: &str,
    direct_fan_out: i64,
    class_counts: &Value,
    dependency_fingerprint: &str,
) -> Result<String, String> {
    sha256_canonical(&json!({
        "projectId":project_id,
        "changeRequestId":change_request_id,
        "basedOnRevision":based_on_revision,
        "scopeClass":scope_class,
        "directFanOut":direct_fan_out,
        "classCounts":class_counts,
        "dependencyFingerprint":dependency_fingerprint,
    }))
    .map_err(|error| format!("B12_DIGEST:{error}"))
}

fn impact_result(
    impact: &repository::ImpactSetRecord,
    digest: &str,
    items: Value,
    has_more: bool,
) -> Value {
    json!({
        "impactSetId": impact.impact_set_id,
        "changeRequestId": impact.change_request_id,
        "scopeClass": impact.scope_class,
        "status": impact.status,
        "preflightState": impact.preflight_state,
        "totalEdges": impact.total_edges,
        "expandedEdges": impact.expanded_edges,
        "checkpointOffset": impact.checkpoint_offset,
        "hasMore": has_more,
        "impactDigest": digest,
        "items": items,
        "bounded": true,
        "paged": true
    })
}

fn build_artifact(
    project_id: &str,
    command_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    version: i64,
    payload: Value,
    lifecycle: &str,
    decision_refs: Vec<String>,
    now: &str,
) -> Result<crate::generated::types::ArtifactEnvelope, String> {
    build_artifact_with_scope(
        project_id,
        command_id,
        artifact_id,
        artifact_type,
        json!({"level":"PROJECT"}),
        version,
        payload,
        lifecycle,
        decision_refs,
        now,
    )
}

fn build_artifact_with_scope(
    project_id: &str,
    command_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    scope: Value,
    version: i64,
    payload: Value,
    lifecycle: &str,
    decision_refs: Vec<String>,
    now: &str,
) -> Result<crate::generated::types::ArtifactEnvelope, String> {
    let payload_hash = sha256_canonical(&payload).map_err(|error| error.to_string())?;
    serde_json::from_value(json!({
        "artifactId":artifact_id,
        "projectId":project_id,
        "artifactType":artifact_type,
        "scope":scope,
        "version":version,
        "schemaVersion":"m0a.artifact.v1",
        "artifactStatus":"CANDIDATE",
        "semanticLifecycle":lifecycle,
        "authorityLevel":"C",
        "payload":payload,
        "payloadHash":payload_hash,
        "dependencyRefs":[],
        "provenance":{"createdBy":OWNER_NAME,"commandId":command_id,"decisionRefs":decision_refs},
        "createdAt":now
    }))
    .map_err(|error| format!("B12_ARTIFACT:{error}"))
}

fn merge_json_patch(target: &mut Value, patch: &Value) {
    match (target.as_object_mut(), patch.as_object()) {
        (Some(target), Some(patch)) => {
            for (key, value) in patch {
                if value.is_null() {
                    target.remove(key);
                } else {
                    merge_json_patch(target.entry(key.clone()).or_insert(Value::Null), value);
                }
            }
        }
        _ => *target = patch.clone(),
    }
}

fn change_target_input<'a>(input: &'a Value, requested_change: &'a Value) -> Option<&'a Value> {
    input
        .get("targetAuthority")
        .or_else(|| requested_change.get("targetAuthority"))
        .filter(|value| !value.is_null())
}

fn target_patch(requested_change: &Value) -> Option<&Value> {
    requested_change
        .get("patch")
        .or_else(|| requested_change.get("changes"))
        .filter(|value| value.is_object())
}

fn target_scope_value(record: &repository::AuthorityHeadRecord) -> Result<Value, String> {
    serde_json::from_str(&record.scope_key)
        .map_err(|error| format!("B12_TARGET_SCOPE_INVALID:{error}"))
}

fn validate_typed_target_payload(artifact_type: &str, payload: &Value) -> Result<(), String> {
    match artifact_type {
        "BookPlan" => crate::generated::validators::validate_book_plan_r1(payload)
            .map_err(|error| format!("B12_TARGET_CONTRACT:{error}")),
        "VolumeIntent" => crate::generated::validators::validate_volume_intent_r1(payload)
            .map_err(|error| format!("B12_TARGET_CONTRACT:{error}")),
        "VolumePlan" => crate::generated::validators::validate_volume_plan_r1(payload)
            .map_err(|error| format!("B12_TARGET_CONTRACT:{error}")),
        "ChapterPlan" => crate::generated::validators::validate_chapter_plan_r1(payload)
            .map_err(|error| format!("B12_TARGET_CONTRACT:{error}")),
        "StoryConstitution" | "OpeningContract" => Ok(()),
        _ => Err(format!("B12_CHANGE_TARGET_UNSUPPORTED:{artifact_type}")),
    }
}

fn target_class(artifact_type: Option<&str>) -> &'static str {
    match artifact_type {
        Some("BookPlan" | "VolumeIntent" | "VolumePlan" | "ChapterPlan") => "PLAN_CHANGE",
        Some("StoryConstitution" | "OpeningContract") => "NON_CANON_AUTHORITY_CHANGE",
        Some(_) => "UNSUPPORTED",
        None => "PROJECTION_ONLY",
    }
}

fn retcon_plan_digest(plan: &repository::RetconPlanRecord) -> Result<String, String> {
    sha256_canonical(&plan.downstream_impact).map_err(|error| format!("B12_RETCON_DIGEST:{error}"))
}

fn signing_impact_for_fact(
    connection: &Connection,
    project_id: &str,
    fact: &Value,
) -> Result<bool, String> {
    let Some(chapter_id) = fact.get("sourceChapterId").and_then(Value::as_str) else {
        return Ok(false);
    };
    let chapter_number = crate::repositories::narrative::NarrativeRepository::chapter_number(
        connection, project_id, chapter_id,
    )
    .map_err(|error| format!("B12_DB:{error}"))?;
    Ok(chapter_number.is_some_and(|number| (1..=3).contains(&number)))
}

fn resolve_typed_target(
    connection: &Connection,
    project_id: &str,
    input: &Value,
    requested_change: &Value,
    change_seed: &Value,
) -> Result<Option<repository::AuthorityHeadRecord>, String> {
    let target = change_target_input(input, requested_change)
        .or_else(|| {
            change_seed
                .get("targetAuthority")
                .filter(|value| !value.is_null())
        })
        .or_else(|| {
            change_seed
                .get("requestedChange")
                .and_then(|value| change_target_input(value, value))
        });
    let target_ref = target
        .and_then(|value| value.get("artifactId").or_else(|| value.get("artifactRef")))
        .and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("artifactId").and_then(Value::as_str))
        })
        .or_else(|| input.get("targetRef").and_then(Value::as_str))
        .or_else(|| {
            change_seed
                .get("targetRefs")
                .and_then(Value::as_array)
                .and_then(|values| values.first())
                .and_then(Value::as_str)
        });
    let target_type = target
        .and_then(|value| value.get("artifactType").or_else(|| value.get("type")))
        .and_then(Value::as_str);
    let explicit_target = target.is_some() || input.get("targetRef").is_some();
    if target.is_none() && target_ref.is_none() && target_type.is_none() {
        return Ok(None);
    }
    if target_type.is_some_and(|value| value == "Canon" || value == "CanonFact")
        || target_ref.is_some_and(|value| value.starts_with("fact:"))
    {
        return Err("B12_RETCON_REQUIRED".into());
    }
    let scope = target
        .and_then(|value| value.get("scope"))
        .and_then(|value| {
            value
                .get("level")
                .and_then(Value::as_str)
                .map(|level| (level.to_owned(), value.to_owned()))
        });
    let (scope_level, scope_value) = scope
        .map(|(level, value)| {
            let key =
                String::from_utf8(canonical_bytes(&value).unwrap_or_default()).unwrap_or_default();
            (level, key)
        })
        .unzip();
    let record = repository::load_authority_head(
        connection,
        project_id,
        target_type,
        target_ref,
        scope_level.as_deref(),
        scope_value.as_deref(),
    )
    .map_err(|error| format!("B12_DB:{error}"))?;
    let Some(record) = record else {
        if explicit_target {
            return Err("B12_CHANGE_TARGET_UNSUPPORTED".into());
        }
        return Ok(None);
    };
    if let Some(target_type) = target_type {
        if target_type != record.artifact_type {
            return Err("B12_CHANGE_TARGET_UNSUPPORTED".into());
        }
    }
    if !matches!(
        record.artifact_type.as_str(),
        "BookPlan"
            | "VolumeIntent"
            | "VolumePlan"
            | "ChapterPlan"
            | "StoryConstitution"
            | "OpeningContract"
    ) {
        return Err(format!(
            "B12_CHANGE_TARGET_UNSUPPORTED:{}",
            record.artifact_type
        ));
    }
    Ok(Some(record))
}

fn state_delta(state_id: &str, state_type: &str, value: Value, payload: Value) -> Value {
    json!({
        "stateId":state_id,
        "stateType":state_type,
        "valueJson":value,
        "payloadJson":payload
    })
}

fn decision_update(decision_id: &str, change_request_id: &str, decision_payload: Value) -> Value {
    json!({
        "decisionId":decision_id,
        "decisionType":"B12_CHANGE_EXECUTION",
        "outcome":"APPROVED",
        "reason":"已通过 B12 显式用户决定",
        "sourceRef":change_request_id,
        "payloadJson":decision_payload
    })
}

fn build_bundle(
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    artifacts: Vec<crate::generated::types::ArtifactEnvelope>,
    canon_delta: Option<Value>,
    state_deltas: Vec<Value>,
    decision: Value,
    domain: &str,
) -> Result<CommitBundle, String> {
    let refs: Vec<Value> = artifacts
        .iter()
        .map(|artifact| {
            json!({
                "artifactId":artifact.artifact_id,
                "version":artifact.version,
                "payloadHash":artifact.payload_hash
            })
        })
        .collect();
    let mut value = json!({
        "commitId":format!("commit:b12:{command_id}"),
        "projectId":project_id,
        "commandId":command_id,
        "idempotencyKey":idempotency_key,
        "expectedProjectRevision":expected_revision,
        "contractVersion":CONTRACT_VERSION,
        "createdAt":now,
        "artifacts":artifacts,
        "stateDelta":state_deltas,
        "decisionUpdates":[decision],
        "receiptDraft":{
            "domain":domain,
            "artifactRefs":refs,
            "payload":{"owner":OWNER_NAME,"contract":"CommitBundle V2"}
        }
    });
    if let Some(canon_delta) = canon_delta {
        value["canonDelta"] = json!([canon_delta]);
    }
    serde_json::from_value(value).map_err(|error| format!("B12_BUNDLE:{error}"))
}

fn final_response(
    receipt: &CommitReceipt,
    result: Value,
    postverify: Value,
    message: &str,
) -> Value {
    let verified = postverify.get("status").and_then(Value::as_str) == Some("VERIFIED");
    json!({
        "commandId":receipt.command_id,
        "projectId":receipt.project_id,
        "resultProjectId":receipt.project_id,
        "finalStatus":if verified {"committed"} else {"failed"},
        "message":if verified {message} else {"变更已提交，但复核未达到预期。"},
        "receiptId":receipt.receipt_id,
        "beforeRevision":receipt.before_revision,
        "afterRevision":receipt.after_revision,
        "receipt":receipt,
        "result":result,
        "postverify":postverify
    })
}

fn preflight(
    manager: &ProjectManager,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    request_digest: &str,
    expected_revision: Option<i64>,
    now: &str,
    request: &Value,
) -> Result<Value, String> {
    let input = payload(request)?;
    let change_id = required_string(input, "changeRequestId")?.to_owned();
    control_command(
        manager,
        project_id,
        command_id,
        idempotency_key,
        "impact_preflight",
        request_digest,
        expected_revision,
        now,
        move |connection| {
            let change = repository::load_change_request(connection, project_id, &change_id)
                .map_err(|error| format!("B12_DB:{error}"))?
                .ok_or_else(|| "B12_CHANGE_REQUEST_NOT_FOUND".to_string())?;
            let current_revision = project_revision(connection, project_id)?;
            let based_revision = change
                .impact_seed
                .get("basedOnRevision")
                .and_then(Value::as_i64)
                .unwrap_or(current_revision);
            if current_revision != based_revision {
                return Err("B12_IMPACT_STALE".into());
            }
            let refs = source_refs(&change.impact_seed);
            let explicit_global = change.scope.eq_ignore_ascii_case("GLOBAL");
            let snapshot =
                repository::dependency_snapshot(connection, project_id, &refs, explicit_global)
                    .map_err(|error| format!("B12_DB:{error}"))?;
            if explicit_global && snapshot.summary_status != "READY" {
                return Ok(json!({
                    "status":"SUMMARY_BOOTSTRAP_REQUIRED",
                    "changeRequestId":change_id,
                    "summaryStatus":snapshot.summary_status,
                    "summaryBootstrapMode":"EXPLICIT_BACKGROUND_REQUIRED",
                    "fullGraphSynchronousScan":snapshot.full_graph_synchronous_scan,
                    "preflightOnly":true,
                    "bounded":true,
                    "userMessage":"全局影响索引正在准备，请完成一次后台索引后再查看影响。"
                }));
            }
            let direct = snapshot.total_edges;
            let scope_class = if explicit_global || direct > GLOBAL_FAN_OUT_THRESHOLD {
                "GLOBAL"
            } else if direct > 20 {
                "WIDE"
            } else {
                "LOCAL"
            };
            let query_global = explicit_global;
            let class_counts = snapshot.class_counts;
            let dependency_fingerprint = snapshot.fingerprint;
            let digest = source_digest(
                project_id,
                &change_id,
                based_revision,
                scope_class,
                direct,
                &class_counts,
                &dependency_fingerprint,
            )?;
            let estimate = json!({
                "basedOnRevision":based_revision,
                "sourceRefs":refs,
                "queryGlobal":query_global,
                "directFanOut":direct,
                "scopeClass":scope_class,
                "classCounts":class_counts,
                "dependencyFingerprint":dependency_fingerprint,
                "boundedSecondOrderEstimate":direct.saturating_mul(2).min(10_000),
                "estimatedAffectedPlans":direct,
                "estimatedRevalidate":class_counts.get("SOFT").and_then(Value::as_i64).unwrap_or(0),
                "estimatedReplan":class_counts.get("REVIEW_REQUIRED").and_then(Value::as_i64).unwrap_or(0),
                "estimatedRewrite":class_counts.get("HARD").and_then(Value::as_i64).unwrap_or(0),
                "impactDigest":digest,
                "preflightOnly":true
            });
            let impact = repository::load_impact_set(connection, project_id, &change_id)
                .map_err(|error| format!("B12_DB:{error}"))?;
            let needs_revalidation = impact
                .as_ref()
                .is_some_and(|item| item.preflight_state == "STALE");
            let impact_id = if needs_revalidation {
                format!("impact:{change_id}:revalidated:{}", &digest[..12])
            } else {
                impact
                    .as_ref()
                    .map(|item| item.impact_set_id.clone())
                    .unwrap_or_else(|| format!("impact:{change_id}:1"))
            };
            if impact.is_none() || needs_revalidation {
                repository::insert_impact_set(
                    connection,
                    project_id,
                    &impact_id,
                    &change_id,
                    scope_class,
                    &estimate,
                    direct,
                    if direct == 0 { "COMPLETED" } else { "PLANNING" },
                    now,
                )
                .map_err(|error| format!("B12_DB:{error}"))?;
                // A global preflight is intentionally summary-only.  When no
                // explicit source refs are supplied, seeding one frontier row
                // per source would scan the entire graph before the user has
                // confirmed expansion.  The durable frontier is initialized
                // by impact_expand/resume immediately before its first page.
                let defer_global_frontier =
                    query_global && refs.as_array().is_some_and(|items| items.is_empty());
                if !defer_global_frontier {
                    repository::initialize_impact_frontier(
                        connection,
                        project_id,
                        &impact_id,
                        &refs,
                        query_global,
                        now,
                    )
                    .map_err(|error| format!("B12_DB:{error}"))?;
                }
            }
            let seed = merge_seed(
                &change.impact_seed,
                &[
                    ("basedOnRevision", json!(based_revision)),
                    ("impactSetId", json!(impact_id)),
                    ("preflight", estimate.clone()),
                ],
            );
            repository::update_change_request(
                connection,
                project_id,
                &change_id,
                "IMPACT_READY",
                &seed,
                None,
                now,
            )
            .map_err(|error| format!("B12_DB:{error}"))?;
            Ok(json!({
                "status":"PREFLIGHTED",
                "changeRequestId":change_id,
                "impactSetId":impact_id,
                "scopeClass":scope_class,
                "directFanOut":direct,
                "expandedEdges":0,
                "totalEdges":direct,
                "impactDigest":digest,
                "estimate":estimate,
                "preflightOnly":true,
                "summaryStatus":snapshot.summary_status,
                "fullGraphSynchronousScan":snapshot.full_graph_synchronous_scan,
                "bounded":true
            }))
        },
    )
}

fn expand(
    manager: &ProjectManager,
    project_id: &str,
    command_type: &str,
    command_id: &str,
    idempotency_key: &str,
    request_digest: &str,
    expected_revision: Option<i64>,
    now: &str,
    request: &Value,
) -> Result<Value, String> {
    let input = payload(request)?;
    let change_id = required_string(input, "changeRequestId")?.to_owned();
    let continue_global = input
        .get("continueGlobal")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let resume = command_type == "impact_resume"
        || input.get("resume").and_then(Value::as_bool) == Some(true);
    let page_size = input.get("pageSize").and_then(Value::as_i64).unwrap_or(50);
    if !(1..=MAX_IMPACT_PAGE).contains(&page_size) {
        return Err("B12_IMPACT_PAGE_LIMIT".into());
    }
    control_command(
        manager,
        project_id,
        command_id,
        idempotency_key,
        command_type,
        request_digest,
        expected_revision,
        now,
        move |connection| {
            let change = repository::load_change_request(connection, project_id, &change_id)
                .map_err(|error| format!("B12_DB:{error}"))?
                .ok_or_else(|| "B12_CHANGE_REQUEST_NOT_FOUND".to_string())?;
            let mut impact = repository::load_impact_set(connection, project_id, &change_id)
                .map_err(|error| format!("B12_DB:{error}"))?
                .ok_or_else(|| "B12_IMPACT_PREFLIGHT_REQUIRED".to_string())?;
            if impact.status == "CANCELLED" && !resume {
                return Err("B12_IMPACT_CANCELLED".into());
            }
            if impact.scope_class == "GLOBAL" && !continue_global {
                return Err("B12_GLOBAL_EXPANSION_CONFIRMATION_REQUIRED".into());
            }
            let current_revision = project_revision(connection, project_id)?;
            let based_revision = impact
                .estimate
                .get("basedOnRevision")
                .and_then(Value::as_i64)
                .unwrap_or(-1);
            if current_revision != based_revision {
                let seed = merge_seed(
                    &change.impact_seed,
                    &[("staleReason", json!("REVISION_CHANGED"))],
                );
                repository::mark_impact_stale(
                    connection,
                    project_id,
                    &impact.impact_set_id,
                    &change_id,
                    &seed,
                    now,
                )
                .map_err(|error| format!("B12_DB:{error}"))?;
                return Err("B12_IMPACT_STALE".into());
            }
            let refs = source_refs(&change.impact_seed);
            let query_global = impact
                .estimate
                .get("queryGlobal")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let snapshot =
                repository::dependency_snapshot(connection, project_id, &refs, query_global)
                    .map_err(|error| format!("B12_DB:{error}"))?;
            if query_global && snapshot.summary_status != "READY" {
                return Err("B12_DEPENDENCY_SUMMARY_NOT_READY".into());
            }
            let direct = snapshot.total_edges;
            let class_counts = snapshot.class_counts;
            let dependency_fingerprint = snapshot.fingerprint;
            let digest = source_digest(
                project_id,
                &change_id,
                based_revision,
                &impact.scope_class,
                direct,
                &class_counts,
                &dependency_fingerprint,
            )?;
            if impact.estimate.get("impactDigest").and_then(Value::as_str) != Some(digest.as_str())
            {
                let seed = merge_seed(
                    &change.impact_seed,
                    &[("staleReason", json!("SOURCE_DIGEST_CHANGED"))],
                );
                repository::mark_impact_stale(
                    connection,
                    project_id,
                    &impact.impact_set_id,
                    &change_id,
                    &seed,
                    now,
                )
                .map_err(|error| format!("B12_DB:{error}"))?;
                return Err("B12_IMPACT_STALE".into());
            }
            if impact.status != "COMPLETED"
                && !repository::impact_frontier_exists(
                    connection,
                    project_id,
                    &impact.impact_set_id,
                )
                .map_err(|error| format!("B12_DB:{error}"))?
            {
                repository::initialize_impact_frontier(
                    connection,
                    project_id,
                    &impact.impact_set_id,
                    &refs,
                    query_global,
                    now,
                )
                .map_err(|error| format!("B12_DB:{error}"))?;
            }
            if impact.status == "CANCELLED" {
                repository::update_impact_set(
                    connection,
                    project_id,
                    &impact.impact_set_id,
                    &impact.processed_pages,
                    impact.expanded_edges,
                    impact.checkpoint_offset,
                    "RUNNING",
                    "RESUMED",
                    now,
                )
                .map_err(|error| format!("B12_DB:{error}"))?;
                impact.status = "RUNNING".into();
            }
            if impact.status == "COMPLETED" {
                return Ok(impact_result(&impact, &digest, json!([]), false));
            }
            let page = repository::expand_impact_batch(
                connection,
                project_id,
                &impact.impact_set_id,
                page_size,
                now,
            )
            .map_err(|error| format!("B12_DB:{error}"))?;
            let page_count = page.edges.len() as i64;
            let status = if page.frontier_pending == 0 {
                "COMPLETED"
            } else {
                "RUNNING"
            };
            let next_status = if status == "COMPLETED" {
                "PENDING_APPROVAL"
            } else {
                "IMPACT_READY"
            };
            let seed = merge_seed(
                &change.impact_seed,
                &[
                    ("impactSetId", json!(impact.impact_set_id)),
                    ("impactDigest", json!(digest)),
                ],
            );
            repository::update_change_request(
                connection,
                project_id,
                &change_id,
                next_status,
                &seed,
                None,
                now,
            )
            .map_err(|error| format!("B12_DB:{error}"))?;
            let page_value =
                serde_json::to_value(&page.edges).map_err(|error| error.to_string())?;
            impact.expanded_edges = page.expanded_count;
            impact.checkpoint_offset = page.checkpoint_offset;
            impact.total_edges = impact
                .total_edges
                .max(page.affected_count + page.frontier_pending);
            impact.status = status.into();
            impact.processed_pages = json!({
                "lastBatchEdges":page_count,
                "visitedCount":page.affected_count,
                "frontierPending":page.frontier_pending,
                "maxFrontierBatch":page.max_frontier_batch
            });
            Ok(impact_result(
                &impact,
                &digest,
                page_value,
                page.frontier_pending > 0,
            ))
        },
    )
}

fn decide(
    manager: &ProjectManager,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    request_digest: &str,
    expected_revision: Option<i64>,
    now: &str,
    request: &Value,
) -> Result<Value, String> {
    let input = payload(request)?;
    let change_id = required_string(input, "changeRequestId")?.to_owned();
    let decision = input
        .get("decision")
        .or_else(|| input.get("outcome"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_uppercase();
    if !matches!(decision.as_str(), "APPROVE" | "REJECT") {
        return Err("B12_DECISION_REQUIRED".into());
    }
    if input.get("scopeAcknowledged").and_then(Value::as_bool) != Some(true)
        || input.get("riskAcknowledged").and_then(Value::as_bool) != Some(true)
    {
        return Err("B12_DECISION_ACK_REQUIRED".into());
    }
    let supplied_digest = required_string(input, "impactDigest")?.to_owned();
    let decision_id = input
        .get("decisionId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("decision:{change_id}:1"));
    control_command(
        manager,
        project_id,
        command_id,
        idempotency_key,
        "change_request_decide",
        request_digest,
        expected_revision,
        now,
        move |connection| {
            let change = repository::load_change_request(connection, project_id, &change_id)
                .map_err(|error| format!("B12_DB:{error}"))?
                .ok_or_else(|| "B12_CHANGE_REQUEST_NOT_FOUND".to_string())?;
            let impact = repository::load_impact_set(connection, project_id, &change_id)
                .map_err(|error| format!("B12_DB:{error}"))?
                .ok_or_else(|| "B12_IMPACT_PREFLIGHT_REQUIRED".to_string())?;
            let expected_digest = impact
                .estimate
                .get("impactDigest")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if expected_digest != supplied_digest {
                return Err("B12_DECISION_STALE".into());
            }
            if impact.status != "COMPLETED" && impact.total_edges > 0 {
                return Err("B12_IMPACT_INCOMPLETE".into());
            }
            let current_revision = project_revision(connection, project_id)?;
            if impact
                .estimate
                .get("basedOnRevision")
                .and_then(Value::as_i64)
                != Some(current_revision)
            {
                return Err("B12_DECISION_STALE".into());
            }
            let retcon_plan = if change.change_type == "RETCON" {
                let plan_id = required_string(input, "retconPlanId")?.to_owned();
                let plan = repository::load_retcon_plan(connection, project_id, &plan_id)
                    .map_err(|error| format!("B12_DB:{error}"))?
                    .ok_or_else(|| "B12_RETCON_PLAN_NOT_FOUND".to_string())?;
                let digest = retcon_plan_digest(&plan)?;
                if input
                    .get("planDigest")
                    .and_then(Value::as_str)
                    .is_some_and(|supplied| supplied != digest)
                {
                    return Err("B12_DECISION_STALE".into());
                }
                Some((plan_id, digest))
            } else {
                None
            };
            let decision_payload = json!({
                "changeRequestId":change_id,
                "impactSetId":impact.impact_set_id,
                "impactDigest":supplied_digest,
                "basedOnRevision":current_revision,
                "scopeAcknowledged":true,
                "riskAcknowledged":true,
                "retconPlanId":retcon_plan.as_ref().map(|item| item.0.clone()),
                "planDigest":retcon_plan.as_ref().map(|item| item.1.clone())
            });
            repository::insert_decision(
                connection,
                project_id,
                &decision_id,
                "B12_CHANGE_APPROVAL",
                &decision,
                "用户明确决定",
                &change_id,
                &decision_payload,
                now,
            )
            .map_err(|error| format!("B12_DB:{error}"))?;
            let status = if decision == "APPROVE" {
                "APPROVED"
            } else {
                "REJECTED"
            };
            let seed = merge_seed(
                &change.impact_seed,
                &[
                    ("impactDigest", json!(supplied_digest)),
                    ("decisionId", json!(decision_id)),
                    ("decision", json!(decision)),
                ],
            );
            repository::update_change_request(
                connection,
                project_id,
                &change_id,
                status,
                &seed,
                Some(&decision_id),
                now,
            )
            .map_err(|error| format!("B12_DB:{error}"))?;
            if let Some((plan_id, _)) = retcon_plan.as_ref() {
                repository::update_retcon_plan(
                    connection,
                    project_id,
                    plan_id,
                    status,
                    Some(&decision_id),
                    now,
                )
                .map_err(|error| format!("B12_DB:{error}"))?;
            }
            Ok(json!({
                "status":status,
                "changeRequestId":change_id,
                "decisionId":decision_id,
                "impactDigest":supplied_digest,
                "basedOnRevision":current_revision,
                "retconPlanId":retcon_plan.as_ref().map(|item| item.0.clone()),
                "planDigest":retcon_plan.as_ref().map(|item| item.1.clone()),
                "explicit":true
            }))
        },
    )
}

fn postverify_normal(
    manager: &ProjectManager,
    project_id: &str,
    change_id: &str,
    impact_id: &str,
    receipt_id: &str,
    idempotency_key: &str,
    now: &str,
) -> Result<Value, String> {
    manager
        .with_read_connection(project_id, now, |connection| {
            let change = repository::load_change_request(connection, project_id, change_id)
                .map_err(|error| ManagerError::ProjectCommand(format!("B12_DB:{error}")))?;
            let impact = repository::load_impact_set(connection, project_id, change_id)
                .map_err(|error| ManagerError::ProjectCommand(format!("B12_DB:{error}")))?;
            let has_state = repository::state_exists(
                connection,
                project_id,
                &format!("change-state:{change_id}"),
            )
            .map_err(|error| ManagerError::ProjectCommand(format!("B12_DB:{error}")))?;
            let receipt_ok = CommitService::find_receipt(connection, project_id, idempotency_key)
                .map_err(|error| ManagerError::ProjectCommand(error.to_string()))?
                .is_some()
                && receipt_id.starts_with("receipt:");
            let observed = change.as_ref().is_some_and(|item| item.status == "APPLIED")
                && impact.as_ref().is_some_and(|item| {
                    item.impact_set_id == impact_id && item.status == "COMPLETED"
                })
                && has_state
                && receipt_ok;
            Ok(json!({
                "status":if observed {"VERIFIED"} else {"INCOMPLETE"},
                "userMessage":if observed {"已完成并复核"} else {"暂时无法完成复核"},
                "changeRequestId":change_id,
                "impactSetId":impact_id,
                "receiptId":receipt_id,
                "authorityChecked":observed,
                "staleChecked":observed,
                "typedRequery":true
            }))
        })
        .map_err(normalize_owner_error)
}

fn apply_change(
    manager: &ProjectManager,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: Option<i64>,
    now: &str,
    request: &Value,
    request_digest: &str,
) -> Result<Value, String> {
    let input = payload(request)?;
    let change_id = required_string(input, "changeRequestId")?.to_owned();
    let handle = manager.open(project_id, "", now).map_err(manager_error)?;
    if let Some(previous) = CommitService::find_receipt(&handle, project_id, idempotency_key)
        .map_err(|error| error.to_string())?
    {
        drop(handle);
        let postverify = postverify_normal(
            manager,
            project_id,
            &change_id,
            &format!("impact:{change_id}:1"),
            &previous.receipt_id,
            idempotency_key,
            now,
        )?;
        return Ok(final_response(
            &previous,
            json!({"status":"REPLAYED","changeRequestId":change_id}),
            postverify,
            "变更已按原回执重放并复核。",
        ));
    }
    let change = repository::load_change_request(&handle, project_id, &change_id)
        .map_err(|error| format!("B12_DB:{error}"))?
        .ok_or_else(|| "B12_CHANGE_REQUEST_NOT_FOUND".to_string())?;
    if change.status != "APPROVED" {
        return Err("B12_USER_DECISION_REQUIRED".into());
    }
    if change.change_type == "RETCON" {
        return Err("B12_RETCON_PLAN_REQUIRED".into());
    }
    if request
        .pointer("/payload/targetFactId")
        .and_then(Value::as_str)
        .is_some()
        || request
            .pointer("/payload/canonDelta")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
    {
        return Err("B12_RETCON_REQUIRED".into());
    }
    let impact = repository::load_impact_set(&handle, project_id, &change_id)
        .map_err(|error| format!("B12_DB:{error}"))?
        .ok_or_else(|| "B12_IMPACT_PREFLIGHT_REQUIRED".to_string())?;
    let supplied_digest = required_string(input, "impactDigest")?;
    if impact.estimate.get("impactDigest").and_then(Value::as_str) != Some(supplied_digest) {
        return Err("B12_DECISION_STALE".into());
    }
    let current_revision = project_revision(&handle, project_id)?;
    if let Some(expected) = expected_revision {
        if expected != current_revision {
            return Err("REVISION_CONFLICT".into());
        }
    }
    if change
        .impact_seed
        .get("basedOnRevision")
        .and_then(Value::as_i64)
        != Some(current_revision)
    {
        return Err("B12_DECISION_STALE".into());
    }
    let decision_id = change
        .decision_ref
        .clone()
        .ok_or_else(|| "B12_USER_DECISION_REQUIRED".to_string())?;
    let (decision_outcome, decision_payload) =
        repository::load_decision(&handle, project_id, &decision_id)
            .map_err(|error| format!("B12_DB:{error}"))?
            .ok_or_else(|| "B12_USER_DECISION_REQUIRED".to_string())?;
    if decision_outcome != "APPROVE"
        || decision_payload.get("impactDigest").and_then(Value::as_str) != Some(supplied_digest)
    {
        return Err("B12_DECISION_STALE".into());
    }
    let requested_change = input
        .get("requestedChange")
        .cloned()
        .or_else(|| change.impact_seed.get("requestedChange").cloned())
        .unwrap_or_else(|| json!({}));
    let typed_target = resolve_typed_target(
        &handle,
        project_id,
        input,
        &requested_change,
        &change.impact_seed,
    )?;
    let (target_artifact, target_head, target_type) = if let Some(target) = typed_target {
        let patch = target_patch(&requested_change)
            .ok_or_else(|| "B12_CHANGE_TARGET_PATCH_REQUIRED".to_string())?;
        let mut target_payload = target.payload.clone();
        merge_json_patch(&mut target_payload, patch);
        validate_typed_target_payload(&target.artifact_type, &target_payload)?;
        let scope = target_scope_value(&target)?;
        let artifact = build_artifact_with_scope(
            project_id,
            command_id,
            &target.artifact_id,
            &target.artifact_type,
            scope.clone(),
            target.version + 1,
            target_payload,
            "APPROVED_PLAN",
            vec![decision_id.clone()],
            now,
        )?;
        let head: crate::generated::types::HeadChange = serde_json::from_value(json!({
            "artifactType":target.artifact_type,
            "scope":scope,
            "artifactId":artifact.artifact_id,
            "version":artifact.version,
            "payloadHash":artifact.payload_hash
        }))
        .map_err(|error| format!("B12_TARGET_HEAD:{error}"))?;
        let artifact_type = target.artifact_type.clone();
        (Some(artifact), Some(head), Some(artifact_type))
    } else {
        (None, None, None)
    };
    let change_payload = json!({
        "payloadSchema":"b12-change-request.r1",
        "changeRequestId":change_id,
        "changeType":change.change_type,
        "status":"COMMITTED",
        "summary":change.summary,
        "requestedChange":requested_change,
        "basedOnRevision":current_revision,
        "impactSetId":impact.impact_set_id,
        "impactDigest":supplied_digest,
        "decisionId":decision_id,
        "authorityType":target_type.clone(),
        "targetClass":target_class(target_type.as_deref())
    });
    let artifact = build_artifact(
        project_id,
        command_id,
        &change_id,
        "ChangeRequest",
        1,
        change_payload,
        "MATERIALIZED",
        vec![decision_id.clone()],
        now,
    )?;
    let stale_state = state_delta(
        &format!("stale:{change_id}"),
        "b12.stale.activation",
        json!({"changeRequestId":change_id,"impactSetId":impact.impact_set_id,"scopeClass":impact.scope_class,"status":"STALE","expandedEdges":impact.expanded_edges}),
        json!({"source":"B12::ChangeRetconOwner","impactDigest":supplied_digest}),
    );
    let applied_state = state_delta(
        &format!("change-state:{change_id}"),
        "b12.change.applied",
        json!({"changeRequestId":change_id,"requestedChange":requested_change,"impactDigest":supplied_digest,"authorityStatus":"COMMITTED"}),
        json!({"owner":OWNER_NAME,"basedOnRevision":current_revision}),
    );
    let mut bundle = build_bundle(
        project_id,
        command_id,
        idempotency_key,
        current_revision,
        now,
        std::iter::once(artifact).chain(target_artifact).collect(),
        None,
        vec![stale_state, applied_state],
        decision_update(&decision_id, &change_id, decision_payload),
        "B12_CHANGE",
    )?;
    if let Some(head) = target_head {
        bundle.head_updates = Some(vec![head]);
    }
    let impact_id = impact.impact_set_id.clone();
    let seed = change.impact_seed.clone();
    let receipt =
        CommitService::commit_with_post_apply(&handle, &bundle, now, |connection, receipt| {
            repository::mark_applied(
                connection,
                project_id,
                &change_id,
                &impact_id,
                None,
                &seed,
                &receipt.receipt_id,
                now,
            )
            .map(|_| ())
            .map_err(|error| {
                crate::commit::CommitError::ProjectCommandApply(format!("B12_DB:{error}"))
            })
        })
        .map_err(|error| error.to_string())?;
    drop(handle);
    let postverify = postverify_normal(
        manager,
        project_id,
        &change_id,
        &impact_id,
        &receipt.receipt_id,
        idempotency_key,
        now,
    )?;
    Ok(final_response(
        &receipt,
        json!({"status":"APPLIED","changeRequestId":change_id,"impactSetId":impact_id,"requestDigest":request_digest,"authorityType":target_type.clone(),"targetClass":target_class(target_type.as_deref()),"authorityMutation":target_type.is_some()}),
        postverify,
        "变更已完成并复核。",
    ))
}

/// H03 adapter for the already-open repair connection.  It deliberately does
/// not open another writer: the repair owner has an active connection while it
/// claims the one-time P3 authorization, so nesting `ProjectManager::open` here
/// would create a second writer path and make the valid B12 capability appear
/// unavailable.  The final write still goes through the same CommitBundle V2
/// assembly and CommitService as the direct History workspace path.
#[allow(clippy::too_many_arguments)]
pub fn apply_approved_on_connection(
    connection: &Connection,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    request: &Value,
) -> Result<Value, String> {
    let input = payload(request)?;
    let change_id = required_string(input, "changeRequestId")?.to_owned();
    if let Some(previous) = CommitService::find_receipt(connection, project_id, idempotency_key)
        .map_err(|error| error.to_string())?
    {
        return Ok(
            json!({"receiptId":previous.receipt_id,"receipt":previous,"result":{"status":"REPLAYED","changeRequestId":change_id},"postverify":{"status":"VERIFIED","userMessage":"已完成并复核","typedRequery":true}}),
        );
    }
    let change = repository::load_change_request(connection, project_id, &change_id)
        .map_err(|error| format!("B12_DB:{error}"))?
        .ok_or_else(|| "B12_CHANGE_REQUEST_NOT_FOUND".to_string())?;
    if change.status != "APPROVED" {
        return Err("B12_USER_DECISION_REQUIRED".into());
    }
    if change.change_type == "RETCON" {
        return apply_retcon_on_connection(
            connection,
            project_id,
            command_id,
            idempotency_key,
            expected_revision,
            now,
            request,
        );
    }
    if input.get("targetFactId").is_some() || input.get("canonDelta").is_some() {
        return Err("B12_RETCON_REQUIRED".into());
    }
    let impact = repository::load_impact_set(connection, project_id, &change_id)
        .map_err(|error| format!("B12_DB:{error}"))?
        .ok_or_else(|| "B12_IMPACT_PREFLIGHT_REQUIRED".to_string())?;
    let digest = required_string(input, "impactDigest")?.to_owned();
    if impact.estimate.get("impactDigest").and_then(Value::as_str) != Some(digest.as_str()) {
        return Err("B12_DECISION_STALE".into());
    }
    if project_revision(connection, project_id)? != expected_revision {
        return Err("REVISION_CONFLICT".into());
    }
    let decision_id = change
        .decision_ref
        .clone()
        .ok_or_else(|| "B12_USER_DECISION_REQUIRED".to_string())?;
    let (outcome, decision_payload) =
        repository::load_decision(connection, project_id, &decision_id)
            .map_err(|error| format!("B12_DB:{error}"))?
            .ok_or_else(|| "B12_USER_DECISION_REQUIRED".to_string())?;
    if outcome != "APPROVE"
        || decision_payload.get("impactDigest").and_then(Value::as_str) != Some(digest.as_str())
    {
        return Err("B12_DECISION_STALE".into());
    }
    let requested_change = input
        .get("requestedChange")
        .cloned()
        .or_else(|| change.impact_seed.get("requestedChange").cloned())
        .unwrap_or_else(|| json!({}));
    let typed_target = resolve_typed_target(
        connection,
        project_id,
        input,
        &requested_change,
        &change.impact_seed,
    )?;
    let (target_artifact, target_head, target_type) = if let Some(target) = typed_target {
        let patch = target_patch(&requested_change)
            .ok_or_else(|| "B12_CHANGE_TARGET_PATCH_REQUIRED".to_string())?;
        let mut target_payload = target.payload.clone();
        merge_json_patch(&mut target_payload, patch);
        validate_typed_target_payload(&target.artifact_type, &target_payload)?;
        let scope = target_scope_value(&target)?;
        let artifact = build_artifact_with_scope(
            project_id,
            command_id,
            &target.artifact_id,
            &target.artifact_type,
            scope.clone(),
            target.version + 1,
            target_payload,
            "APPROVED_PLAN",
            vec![decision_id.clone()],
            now,
        )?;
        let head: crate::generated::types::HeadChange = serde_json::from_value(json!({
            "artifactType":target.artifact_type,
            "scope":scope,
            "artifactId":artifact.artifact_id,
            "version":artifact.version,
            "payloadHash":artifact.payload_hash
        }))
        .map_err(|error| format!("B12_TARGET_HEAD:{error}"))?;
        let artifact_type = target.artifact_type.clone();
        (Some(artifact), Some(head), Some(artifact_type))
    } else {
        (None, None, None)
    };
    let artifact = build_artifact(
        project_id,
        command_id,
        &change_id,
        "ChangeRequest",
        1,
        json!({"payloadSchema":"b12-change-request.r1","changeRequestId":change_id,"changeType":change.change_type,"status":"COMMITTED","summary":change.summary,"requestedChange":requested_change,"basedOnRevision":expected_revision,"impactSetId":impact.impact_set_id,"impactDigest":digest,"decisionId":decision_id,"authorityType":target_type.clone(),"targetClass":target_class(target_type.as_deref())}),
        "MATERIALIZED",
        vec![decision_id.clone()],
        now,
    )?;
    let stale = state_delta(
        &format!("stale:{change_id}"),
        "b12.stale.activation",
        json!({"changeRequestId":change_id,"impactSetId":impact.impact_set_id,"scopeClass":impact.scope_class,"status":"STALE","expandedEdges":impact.expanded_edges}),
        json!({"owner":OWNER_NAME,"impactDigest":digest}),
    );
    let applied = state_delta(
        &format!("change-state:{change_id}"),
        "b12.change.applied",
        json!({"changeRequestId":change_id,"requestedChange":requested_change,"impactDigest":digest,"authorityStatus":"COMMITTED"}),
        json!({"owner":OWNER_NAME,"basedOnRevision":expected_revision}),
    );
    let mut bundle = build_bundle(
        project_id,
        command_id,
        idempotency_key,
        expected_revision,
        now,
        std::iter::once(artifact).chain(target_artifact).collect(),
        None,
        vec![stale, applied],
        decision_update(&decision_id, &change_id, decision_payload),
        "B12_CHANGE",
    )?;
    if let Some(head) = target_head {
        bundle.head_updates = Some(vec![head]);
    }
    let impact_id = impact.impact_set_id.clone();
    let seed = change.impact_seed.clone();
    let receipt =
        CommitService::commit_with_post_apply(connection, &bundle, now, |connection, receipt| {
            repository::mark_applied(
                connection,
                project_id,
                &change_id,
                &impact_id,
                None,
                &seed,
                &receipt.receipt_id,
                now,
            )
            .map(|_| ())
            .map_err(|error| {
                crate::commit::CommitError::ProjectCommandApply(format!("B12_DB:{error}"))
            })
        })
        .map_err(|error| error.to_string())?;
    let verified = repository::load_change_request(connection, project_id, &change_id)
        .map_err(|error| format!("B12_DB:{error}"))?
        .is_some_and(|item| item.status == "APPLIED")
        && repository::state_exists(connection, project_id, &format!("change-state:{change_id}"))
            .map_err(|error| format!("B12_DB:{error}"))?;
    Ok(json!({
        "receiptId":receipt.receipt_id,
        "receipt":receipt,
        "result":{"status":"APPLIED","changeRequestId":change_id,"impactSetId":impact_id,"authorityType":target_type.clone(),"targetClass":target_class(target_type.as_deref())},
        "postverify":{"status":if verified {"VERIFIED"} else {"INCOMPLETE"},"userMessage":if verified {"已完成并复核"} else {"暂时无法完成复核"},"typedRequery":true}
    }))
}

fn create_change(
    manager: &ProjectManager,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    request_digest: &str,
    expected_revision: Option<i64>,
    now: &str,
    request: &Value,
) -> Result<Value, String> {
    let input = payload(request)?;
    let change_id = required_string(input, "changeRequestId")?.to_owned();
    let change_type = required_string(input, "changeType")?.to_uppercase();
    if !matches!(change_type.as_str(), "CHANGE" | "RETCON") {
        return Err("B12_INVALID:changeType".into());
    }
    let scope = required_string(input, "scope")?.to_uppercase();
    let summary = required_string(input, "summary")?.to_owned();
    let has_canon_target = input.get("targetFactId").and_then(Value::as_str).is_some()
        || string_array(input.get("targetRefs"))
            .iter()
            .any(|item| item.starts_with("fact:"));
    if has_canon_target && change_type != "RETCON" {
        return Err("B12_RETCON_REQUIRED".into());
    }
    control_command(
        manager,
        project_id,
        command_id,
        idempotency_key,
        "change_request_create",
        request_digest,
        expected_revision,
        now,
        move |connection| {
            let current = project_revision(connection, project_id)?;
            let based = input
                .get("basedOnRevision")
                .and_then(Value::as_i64)
                .unwrap_or(current);
            if based != current {
                return Err("B12_REVISION_STALE".into());
            }
            let target_refs = string_array(input.get("targetRefs"));
            let source_refs = string_array(input.get("sourceRefs"));
            let seed = json!({
                "basedOnRevision":based,
                "targetRefs":target_refs,
                "sourceRefs":source_refs,
                "targetFactId":input.get("targetFactId").cloned().unwrap_or(Value::Null),
                "targetAuthority":input.get("targetAuthority").cloned().unwrap_or(Value::Null),
                "requestedChange":input.get("requestedChange").cloned().unwrap_or_else(|| json!({})),
                "reason":input.get("reason").cloned().unwrap_or_else(|| json!(summary)),
                "requestDigest":request_digest,
                "createdAt":now
            });
            repository::insert_change_request(
                connection,
                project_id,
                &change_id,
                &change_type,
                &scope,
                &summary,
                &seed,
                now,
            )
            .map_err(|error| format!("B12_DB:{error}"))?;
            Ok(json!({
                "changeRequestId":change_id,
                "changeType":change_type,
                "scope":scope,
                "status":"DRAFT",
                "basedOnRevision":based,
                "proposalOnly":true,
                "authorityMutationCount":0
            }))
        },
    )
}

fn create_retcon_plan(
    manager: &ProjectManager,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    request_digest: &str,
    expected_revision: Option<i64>,
    now: &str,
    request: &Value,
) -> Result<Value, String> {
    let input = payload(request)?;
    let change_id = required_string(input, "changeRequestId")?.to_owned();
    let plan_id = required_string(input, "retconPlanId")?.to_owned();
    let fact_id = required_string(input, "targetFactId")?.to_owned();
    let reason = required_string(input, "reason")?.to_owned();
    let replacement = input
        .get("replacement")
        .cloned()
        .ok_or_else(|| "B12_INVALID:replacement".to_string())?;
    let evidence_refs = input
        .get("evidenceRefs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if evidence_refs.is_empty() {
        return Err("B12_RETCON_EVIDENCE_REQUIRED".into());
    }
    control_command(
        manager,
        project_id,
        command_id,
        idempotency_key,
        "retcon_plan_create",
        request_digest,
        expected_revision,
        now,
        move |connection| {
            let change = repository::load_change_request(connection, project_id, &change_id)
                .map_err(|error| format!("B12_DB:{error}"))?
                .ok_or_else(|| "B12_CHANGE_REQUEST_NOT_FOUND".to_string())?;
            if change.change_type != "RETCON" {
                return Err("B12_RETCON_TYPE_REQUIRED".into());
            }
            let current_revision = project_revision(connection, project_id)?;
            let based = change
                .impact_seed
                .get("basedOnRevision")
                .and_then(Value::as_i64)
                .unwrap_or(current_revision);
            if based != current_revision {
                return Err("B12_RETCON_STALE".into());
            }
            let impact = repository::load_impact_set(connection, project_id, &change_id)
                .map_err(|error| format!("B12_DB:{error}"))?
                .ok_or_else(|| "B12_IMPACT_PREFLIGHT_REQUIRED".to_string())?;
            let old = repository::current_fact(connection, project_id, &fact_id)
                .map_err(|error| format!("B12_DB:{error}"))?
                .ok_or_else(|| "B12_CANON_FACT_NOT_FOUND".to_string())?;
            if old
                .get("supersededBy")
                .is_some_and(|value| !value.is_null())
            {
                return Err("B12_CANON_FACT_NOT_CURRENT".into());
            }
            for evidence in &evidence_refs {
                let artifact_id = evidence.get("artifactId").and_then(Value::as_str);
                let version = evidence.get("version").and_then(Value::as_i64);
                let valid = match (artifact_id, version) {
                    (Some(artifact_id), Some(version)) => repository::artifact_reference_exists(
                        connection,
                        project_id,
                        artifact_id,
                        version,
                    )
                    .map_err(|error| format!("B12_DB:{error}"))?,
                    _ => false,
                };
                if !valid {
                    return Err("B12_RETCON_EVIDENCE_NOT_FOUND".into());
                }
            }
            let signing_impact = signing_impact_for_fact(connection, project_id, &old)?;
            let downstream = json!({
                "changeRequestId":change_id,
                "basedOnRevision":based,
                "impactSetId":impact.impact_set_id,
                "impactDigest":impact.estimate.get("impactDigest").cloned().unwrap_or(Value::Null),
                "evidenceRefs":evidence_refs,
                "oldCanon":old,
                "replacement":replacement,
                "signingImpact":signing_impact,
                "estimatedCostCents":0
            });
            let target_fact = json!({"old":downstream["oldCanon"].clone(),"replacement":downstream["replacement"].clone()});
            let fix_type = input
                .get("fixType")
                .and_then(Value::as_str)
                .unwrap_or("REWRITE");
            if !matches!(fix_type, "EXPLANATORY_PATCH" | "REWRITE" | "SUPPLEMENT") {
                return Err("B12_INVALID:fixType".into());
            }
            repository::insert_retcon_plan(
                connection,
                project_id,
                &plan_id,
                &target_fact,
                &reason,
                fix_type,
                &downstream,
                now,
            )
            .map_err(|error| format!("B12_DB:{error}"))?;
            let seed = merge_seed(
                &change.impact_seed,
                &[
                    ("retconPlanId", json!(plan_id)),
                    ("impactDigest", downstream["impactDigest"].clone()),
                ],
            );
            repository::update_change_request(
                connection,
                project_id,
                &change_id,
                "PENDING_APPROVAL",
                &seed,
                None,
                now,
            )
            .map_err(|error| format!("B12_DB:{error}"))?;
            let plan_digest = sha256_canonical(&downstream).map_err(|error| error.to_string())?;
            Ok(json!({
                "retconPlanId":plan_id,
                "changeRequestId":change_id,
                "status":"PENDING_APPROVAL",
                "basedOnRevision":based,
                "oldCanon":downstream["oldCanon"],
                "replacement":downstream["replacement"],
                "evidenceRefs":downstream["evidenceRefs"],
                "impactSetId":downstream["impactSetId"],
                "impactDigest":downstream["impactDigest"],
                "signingImpact":signing_impact,
                "planDigest":plan_digest
            }))
        },
    )
}

fn apply_retcon(
    manager: &ProjectManager,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: Option<i64>,
    now: &str,
    request: &Value,
) -> Result<Value, String> {
    let input = payload(request)?;
    let change_id = required_string(input, "changeRequestId")?.to_owned();
    let plan_id = required_string(input, "retconPlanId")?.to_owned();
    let supplied_digest = required_string(input, "impactDigest")?.to_owned();
    let handle = manager.open(project_id, "", now).map_err(manager_error)?;
    let change = repository::load_change_request(&handle, project_id, &change_id)
        .map_err(|error| format!("B12_DB:{error}"))?
        .ok_or_else(|| "B12_CHANGE_REQUEST_NOT_FOUND".to_string())?;
    let plan = repository::load_retcon_plan(&handle, project_id, &plan_id)
        .map_err(|error| format!("B12_DB:{error}"))?
        .ok_or_else(|| "B12_RETCON_PLAN_NOT_FOUND".to_string())?;
    if change.status != "APPROVED" || plan.status != "APPROVED" {
        return Err("B12_USER_DECISION_REQUIRED".into());
    }
    let current_plan_digest = retcon_plan_digest(&plan)?;
    if input
        .get("planDigest")
        .and_then(Value::as_str)
        .is_some_and(|digest| digest != current_plan_digest)
    {
        return Err("B12_DECISION_STALE".into());
    }
    if change
        .impact_seed
        .get("retconPlanId")
        .and_then(Value::as_str)
        != Some(plan_id.as_str())
    {
        return Err("B12_DECISION_STALE".into());
    }
    if plan
        .downstream_impact
        .get("impactDigest")
        .and_then(Value::as_str)
        != Some(supplied_digest.as_str())
    {
        return Err("B12_DECISION_STALE".into());
    }
    let current_revision = project_revision(&handle, project_id)?;
    if change
        .impact_seed
        .get("basedOnRevision")
        .and_then(Value::as_i64)
        != Some(current_revision)
    {
        return Err("B12_DECISION_STALE".into());
    }
    if let Some(expected) = expected_revision {
        if expected != current_revision {
            return Err("REVISION_CONFLICT".into());
        }
    }
    let decision_id = change
        .decision_ref
        .clone()
        .ok_or_else(|| "B12_USER_DECISION_REQUIRED".to_string())?;
    let decision = repository::load_decision(&handle, project_id, &decision_id)
        .map_err(|error| format!("B12_DB:{error}"))?
        .ok_or_else(|| "B12_USER_DECISION_REQUIRED".to_string())?;
    if decision.0 != "APPROVE"
        || decision.1.get("retconPlanId").and_then(Value::as_str) != Some(plan_id.as_str())
        || decision.1.get("planDigest").and_then(Value::as_str)
            != Some(current_plan_digest.as_str())
    {
        return Err("B12_DECISION_STALE".into());
    }
    let old = plan
        .target_fact
        .get("old")
        .cloned()
        .ok_or_else(|| "B12_RETCON_OLD_FACT_MISSING".to_string())?;
    let old_fact_id = required_string(&old, "factId")?.to_owned();
    let actual_old = repository::current_fact(&handle, project_id, &old_fact_id)
        .map_err(|error| format!("B12_DB:{error}"))?
        .ok_or_else(|| "B12_CANON_FACT_NOT_FOUND".to_string())?;
    if actual_old
        .get("supersededBy")
        .is_some_and(|value| !value.is_null())
    {
        return Err("B12_CANON_FACT_NOT_CURRENT".into());
    }
    let replacement = plan
        .target_fact
        .get("replacement")
        .cloned()
        .ok_or_else(|| "B12_RETCON_REPLACEMENT_MISSING".to_string())?;
    let new_fact_id = format!("{old_fact_id}:retcon:{plan_id}");
    let canon_delta = json!({
        "factId":new_fact_id,
        "subjectId":replacement.get("subjectId").and_then(Value::as_str).or_else(|| actual_old.get("subjectId").and_then(Value::as_str)).ok_or_else(|| "B12_RETCON_SUBJECT_REQUIRED".to_string())?,
        "predicate":replacement.get("predicate").and_then(Value::as_str).or_else(|| actual_old.get("predicate").and_then(Value::as_str)).ok_or_else(|| "B12_RETCON_PREDICATE_REQUIRED".to_string())?,
        "objectJson":replacement.get("objectJson").cloned().or_else(|| replacement.get("object").cloned()).ok_or_else(|| "B12_RETCON_OBJECT_REQUIRED".to_string())?,
        "validFromChapterId":actual_old.get("validFromChapterId").cloned().unwrap_or(Value::Null),
        "validToChapterId":actual_old.get("validToChapterId").cloned().unwrap_or(Value::Null),
        "authorityLevel":actual_old.get("authorityLevel").cloned().unwrap_or_else(|| json!("C")),
        "payloadJson":{
            "supersedesFactId":old_fact_id,
            "sourceArtifactId":plan_id,
            "sourceVersion":1,
            "retconPlanId":plan_id
        }
    });
    let change_payload = json!({
        "payloadSchema":"b12-retcon-commit.r1",
        "changeRequestId":change_id,
        "retconPlanId":plan_id,
        "status":"COMMITTED",
        "basedOnRevision":current_revision,
        "impactDigest":supplied_digest,
        "oldFactId":old_fact_id,
        "newFactId":new_fact_id,
        "signingImpact":plan.downstream_impact.get("signingImpact").cloned().unwrap_or(json!(false))
    });
    let artifact = build_artifact(
        project_id,
        command_id,
        &format!("retcon:{change_id}:{plan_id}"),
        "RetconPlan",
        1,
        change_payload,
        "MATERIALIZED",
        vec![change.decision_ref.clone().unwrap_or_default()],
        now,
    )?;
    let stale_state = state_delta(
        &format!("stale:{change_id}"),
        "b12.stale.activation",
        json!({"changeRequestId":change_id,"impactSetId":plan.downstream_impact["impactSetId"],"scopeClass":"RETCON","status":"STALE","reason":"CANON_VERSION_TRANSITION"}),
        json!({"owner":OWNER_NAME,"impactDigest":supplied_digest}),
    );
    let applied_state = state_delta(
        &format!("change-state:{change_id}"),
        "b12.retcon.applied",
        json!({"changeRequestId":change_id,"retconPlanId":plan_id,"oldFactId":old_fact_id,"newFactId":new_fact_id,"authorityStatus":"COMMITTED"}),
        json!({"owner":OWNER_NAME,"basedOnRevision":current_revision}),
    );
    let bundle = build_bundle(
        project_id,
        command_id,
        idempotency_key,
        current_revision,
        now,
        vec![artifact],
        Some(canon_delta),
        vec![stale_state, applied_state],
        decision_update(&decision_id, &change_id, decision.1),
        "B12_RETCON",
    )?;
    let impact_id = plan
        .downstream_impact
        .get("impactSetId")
        .and_then(Value::as_str)
        .ok_or_else(|| "B12_IMPACT_SET_REQUIRED".to_string())?
        .to_owned();
    let seed = change.impact_seed.clone();
    let receipt =
        CommitService::commit_with_post_apply(&handle, &bundle, now, |connection, receipt| {
            repository::mark_applied(
                connection,
                project_id,
                &change_id,
                &impact_id,
                Some(&plan_id),
                &seed,
                &receipt.receipt_id,
                now,
            )
            .map(|_| ())
            .map_err(|error| {
                crate::commit::CommitError::ProjectCommandApply(format!("B12_DB:{error}"))
            })
        })
        .map_err(|error| error.to_string())?;
    drop(handle);
    let postverify = manager
        .with_read_connection(project_id, now, |connection| {
            let current = repository::current_fact(connection, project_id, &old_fact_id)
                .map_err(|error| ManagerError::ProjectCommand(format!("B12_DB:{error}")))?;
            let new_current = repository::current_fact(connection, project_id, &new_fact_id)
                .map_err(|error| ManagerError::ProjectCommand(format!("B12_DB:{error}")))?;
            let observed = current
                .as_ref()
                .and_then(|item| item.get("supersededBy"))
                .and_then(Value::as_str)
                == Some(new_fact_id.as_str())
                && new_current.is_some()
                && repository::state_exists(
                    connection,
                    project_id,
                    &format!("change-state:{change_id}"),
                )
                .map_err(|error| ManagerError::ProjectCommand(format!("B12_DB:{error}")))?;
            Ok(json!({
                "status":if observed {"VERIFIED"} else {"INCOMPLETE"},
                "userMessage":if observed {"已完成并复核"} else {"暂时无法完成复核"},
                "oldFactPreserved":current.is_some(),
                "newFactCurrent":new_current.is_some(),
                "versionedAuthority":observed,
                "typedRequery":true
            }))
        })
        .map_err(manager_error)?;
    Ok(final_response(
        &receipt,
        json!({"status":"APPLIED","changeRequestId":change_id,"retconPlanId":plan_id,"oldFactId":old_fact_id,"newFactId":new_fact_id,"signingImpact":plan.downstream_impact.get("signingImpact").cloned().unwrap_or(json!(false))}),
        postverify,
        "事实修订已完成并复核。",
    ))
}

pub fn apply_retcon_on_connection(
    connection: &Connection,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    request: &Value,
) -> Result<Value, String> {
    let input = payload(request)?;
    let change_id = required_string(input, "changeRequestId")?.to_owned();
    let plan_id = required_string(input, "retconPlanId")?.to_owned();
    let supplied_digest = required_string(input, "impactDigest")?.to_owned();
    if let Some(previous) = CommitService::find_receipt(connection, project_id, idempotency_key)
        .map_err(|error| error.to_string())?
    {
        return Ok(json!({
            "receiptId":previous.receipt_id,
            "receipt":previous,
            "result":{"status":"REPLAYED","changeRequestId":change_id,"retconPlanId":plan_id},
            "postverify":{"status":"VERIFIED","userMessage":"已完成并复核","typedRequery":true}
        }));
    }
    let change = repository::load_change_request(connection, project_id, &change_id)
        .map_err(|error| format!("B12_DB:{error}"))?
        .ok_or_else(|| "B12_CHANGE_REQUEST_NOT_FOUND".to_string())?;
    let plan = repository::load_retcon_plan(connection, project_id, &plan_id)
        .map_err(|error| format!("B12_DB:{error}"))?
        .ok_or_else(|| "B12_RETCON_PLAN_NOT_FOUND".to_string())?;
    if change.status != "APPROVED" || plan.status != "APPROVED" {
        return Err("B12_USER_DECISION_REQUIRED".into());
    }
    let current_plan_digest = retcon_plan_digest(&plan)?;
    if input
        .get("planDigest")
        .and_then(Value::as_str)
        .is_some_and(|digest| digest != current_plan_digest)
    {
        return Err("B12_DECISION_STALE".into());
    }
    if change
        .impact_seed
        .get("retconPlanId")
        .and_then(Value::as_str)
        != Some(plan_id.as_str())
    {
        return Err("B12_DECISION_STALE".into());
    }
    if plan
        .downstream_impact
        .get("impactDigest")
        .and_then(Value::as_str)
        != Some(supplied_digest.as_str())
    {
        return Err("B12_DECISION_STALE".into());
    }
    if project_revision(connection, project_id)? != expected_revision {
        return Err("REVISION_CONFLICT".into());
    }
    let decision_id = change
        .decision_ref
        .clone()
        .ok_or_else(|| "B12_USER_DECISION_REQUIRED".to_string())?;
    let decision = repository::load_decision(connection, project_id, &decision_id)
        .map_err(|error| format!("B12_DB:{error}"))?
        .ok_or_else(|| "B12_USER_DECISION_REQUIRED".to_string())?;
    if decision.0 != "APPROVE"
        || decision.1.get("retconPlanId").and_then(Value::as_str) != Some(plan_id.as_str())
        || decision.1.get("planDigest").and_then(Value::as_str)
            != Some(current_plan_digest.as_str())
    {
        return Err("B12_DECISION_STALE".into());
    }
    let old = plan
        .target_fact
        .get("old")
        .cloned()
        .ok_or_else(|| "B12_RETCON_OLD_FACT_MISSING".to_string())?;
    let old_fact_id = required_string(&old, "factId")?.to_owned();
    let actual_old = repository::current_fact(connection, project_id, &old_fact_id)
        .map_err(|error| format!("B12_DB:{error}"))?
        .ok_or_else(|| "B12_CANON_FACT_NOT_FOUND".to_string())?;
    if actual_old
        .get("supersededBy")
        .is_some_and(|value| !value.is_null())
    {
        return Err("B12_CANON_FACT_NOT_CURRENT".into());
    }
    let replacement = plan
        .target_fact
        .get("replacement")
        .cloned()
        .ok_or_else(|| "B12_RETCON_REPLACEMENT_MISSING".to_string())?;
    let new_fact_id = format!("{old_fact_id}:retcon:{plan_id}");
    let canon_delta = json!({
        "factId":new_fact_id,
        "subjectId":replacement.get("subjectId").and_then(Value::as_str).or_else(|| actual_old.get("subjectId").and_then(Value::as_str)).ok_or_else(|| "B12_RETCON_SUBJECT_REQUIRED".to_string())?,
        "predicate":replacement.get("predicate").and_then(Value::as_str).or_else(|| actual_old.get("predicate").and_then(Value::as_str)).ok_or_else(|| "B12_RETCON_PREDICATE_REQUIRED".to_string())?,
        "objectJson":replacement.get("objectJson").cloned().or_else(|| replacement.get("object").cloned()).ok_or_else(|| "B12_RETCON_OBJECT_REQUIRED".to_string())?,
        "validFromChapterId":actual_old.get("validFromChapterId").cloned().unwrap_or(Value::Null),
        "validToChapterId":actual_old.get("validToChapterId").cloned().unwrap_or(Value::Null),
        "authorityLevel":actual_old.get("authorityLevel").cloned().unwrap_or_else(|| json!("C")),
        "payloadJson":{"supersedesFactId":old_fact_id,"sourceArtifactId":plan_id,"sourceVersion":1,"retconPlanId":plan_id}
    });
    let artifact = build_artifact(
        project_id,
        command_id,
        &format!("retcon:{change_id}:{plan_id}"),
        "RetconPlan",
        1,
        json!({
            "payloadSchema":"b12-retcon-commit.r1",
            "changeRequestId":change_id,
            "retconPlanId":plan_id,
            "status":"COMMITTED",
            "basedOnRevision":expected_revision,
            "impactDigest":supplied_digest,
            "oldFactId":old_fact_id,
            "newFactId":new_fact_id,
            "signingImpact":plan.downstream_impact.get("signingImpact").cloned().unwrap_or(json!(false))
        }),
        "MATERIALIZED",
        vec![decision_id.clone()],
        now,
    )?;
    let stale_state = state_delta(
        &format!("stale:{change_id}"),
        "b12.stale.activation",
        json!({"changeRequestId":change_id,"impactSetId":plan.downstream_impact["impactSetId"],"scopeClass":"RETCON","status":"STALE","reason":"CANON_VERSION_TRANSITION"}),
        json!({"owner":OWNER_NAME,"impactDigest":supplied_digest}),
    );
    let applied_state = state_delta(
        &format!("change-state:{change_id}"),
        "b12.retcon.applied",
        json!({"changeRequestId":change_id,"retconPlanId":plan_id,"oldFactId":old_fact_id,"newFactId":new_fact_id,"authorityStatus":"COMMITTED"}),
        json!({"owner":OWNER_NAME,"basedOnRevision":expected_revision}),
    );
    let bundle = build_bundle(
        project_id,
        command_id,
        idempotency_key,
        expected_revision,
        now,
        vec![artifact],
        Some(canon_delta),
        vec![stale_state, applied_state],
        decision_update(&decision_id, &change_id, decision.1),
        "B12_RETCON",
    )?;
    let impact_id = plan
        .downstream_impact
        .get("impactSetId")
        .and_then(Value::as_str)
        .ok_or_else(|| "B12_IMPACT_SET_REQUIRED".to_string())?
        .to_owned();
    let seed = change.impact_seed.clone();
    let receipt =
        CommitService::commit_with_post_apply(connection, &bundle, now, |connection, receipt| {
            repository::mark_applied(
                connection,
                project_id,
                &change_id,
                &impact_id,
                Some(&plan_id),
                &seed,
                &receipt.receipt_id,
                now,
            )
            .map(|_| ())
            .map_err(|error| {
                crate::commit::CommitError::ProjectCommandApply(format!("B12_DB:{error}"))
            })
        })
        .map_err(|error| error.to_string())?;
    let current = repository::current_fact(connection, project_id, &old_fact_id)
        .map_err(|error| format!("B12_DB:{error}"))?;
    let new_current = repository::current_fact(connection, project_id, &new_fact_id)
        .map_err(|error| format!("B12_DB:{error}"))?;
    let verified = current
        .as_ref()
        .and_then(|item| item.get("supersededBy"))
        .and_then(Value::as_str)
        == Some(new_fact_id.as_str())
        && new_current.is_some()
        && repository::state_exists(connection, project_id, &format!("change-state:{change_id}"))
            .map_err(|error| format!("B12_DB:{error}"))?;
    Ok(json!({
        "receiptId":receipt.receipt_id,
        "receipt":receipt,
        "result":{"status":"APPLIED","changeRequestId":change_id,"retconPlanId":plan_id,"oldFactId":old_fact_id,"newFactId":new_fact_id},
        "postverify":{"status":if verified {"VERIFIED"} else {"INCOMPLETE"},"userMessage":if verified {"已完成并复核"} else {"暂时无法完成复核"},"oldFactPreserved":current.is_some(),"newFactCurrent":new_current.is_some(),"versionedAuthority":verified,"typedRequery":true}
    }))
}

fn cancel_impact(
    manager: &ProjectManager,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    request_digest: &str,
    expected_revision: Option<i64>,
    now: &str,
    request: &Value,
) -> Result<Value, String> {
    let input = payload(request)?;
    let change_id = required_string(input, "changeRequestId")?.to_owned();
    control_command(
        manager,
        project_id,
        command_id,
        idempotency_key,
        "impact_cancel",
        request_digest,
        expected_revision,
        now,
        move |connection| {
            let impact = repository::load_impact_set(connection, project_id, &change_id)
                .map_err(|error| format!("B12_DB:{error}"))?
                .ok_or_else(|| "B12_IMPACT_PREFLIGHT_REQUIRED".to_string())?;
            repository::update_impact_set(
                connection,
                project_id,
                &impact.impact_set_id,
                &impact.processed_pages,
                impact.expanded_edges,
                impact.checkpoint_offset,
                "CANCELLED",
                "CANCELLED",
                now,
            )
            .map_err(|error| format!("B12_DB:{error}"))?;
            Ok(
                json!({"changeRequestId":change_id,"impactSetId":impact.impact_set_id,"status":"CANCELLED","authorityMutationCount":0}),
            )
        },
    )
}

/// Executes one B12 command. All callers use this entry point; there is no
/// alternate UI/Harness/Skill Change writer.
#[allow(clippy::too_many_arguments)]
pub fn execute(
    manager: &ProjectManager,
    command_type: &str,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    request_digest: &str,
    expected_revision: Option<i64>,
    requested_at: &str,
    request: &Value,
) -> Result<Value, String> {
    match command_type {
        "change_request_create" => create_change(
            manager,
            project_id,
            command_id,
            idempotency_key,
            request_digest,
            expected_revision,
            requested_at,
            request,
        ),
        "impact_preflight" => preflight(
            manager,
            project_id,
            command_id,
            idempotency_key,
            request_digest,
            expected_revision,
            requested_at,
            request,
        ),
        "impact_expand" | "impact_resume" => expand(
            manager,
            project_id,
            command_type,
            command_id,
            idempotency_key,
            request_digest,
            expected_revision,
            requested_at,
            request,
        ),
        "impact_cancel" => cancel_impact(
            manager,
            project_id,
            command_id,
            idempotency_key,
            request_digest,
            expected_revision,
            requested_at,
            request,
        ),
        "change_request_decide" => decide(
            manager,
            project_id,
            command_id,
            idempotency_key,
            request_digest,
            expected_revision,
            requested_at,
            request,
        ),
        "change_request_apply" => apply_change(
            manager,
            project_id,
            command_id,
            idempotency_key,
            expected_revision,
            requested_at,
            request,
            request_digest,
        ),
        "retcon_plan_create" => create_retcon_plan(
            manager,
            project_id,
            command_id,
            idempotency_key,
            request_digest,
            expected_revision,
            requested_at,
            request,
        ),
        "retcon_apply" => apply_retcon(
            manager,
            project_id,
            command_id,
            idempotency_key,
            expected_revision,
            requested_at,
            request,
        ),
        _ => Err(format!("B12_UNSUPPORTED_COMMAND:{command_type}")),
    }
}
