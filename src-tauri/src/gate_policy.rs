use crate::commit::CommitService;
use crate::generated::types::{ArtifactEnvelope, ArtifactRef, CommitBundle, CommitReceipt};
use crate::hash::sha256_canonical;
use crate::project_db::{ProjectHandle, ProjectManager};
use crate::repositories::artifacts::ArtifactRepository;
use crate::repositories::narrative::NarrativeRepository;
use crate::stable_id::{StableIdKind, StableIdentityFactory};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const DEFAULT_GATE_POLICY_RESOURCE_VERSION: &str = "gate-policy-default.r1";
const DEFAULT_POLICY_BYTES: &[u8] =
    include_bytes!("../../contracts/resources/gate-policy-default.r1/policy.json");
const DEFAULT_POLICY_MANIFEST: &str =
    include_str!("../../contracts/resources/gate-policy-default.r1/resource-manifest.json");
const CANON_SCAN_LIMIT: usize = 10_000;

fn current_policy(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
) -> Result<Option<(ArtifactRef, Value)>, String> {
    let Some(reference) =
        NarrativeRepository::canonical_project_head_ref(connection, project_id, "GatePolicy")
            .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let payload = NarrativeRepository::artifact_payload_by_ref(connection, project_id, &reference)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "B08_GATE_POLICY_REQUIRED".to_string())?;
    Ok(Some((reference, payload)))
}

fn default_policy_payload() -> Result<Value, String> {
    let manifest: Value = serde_json::from_str(DEFAULT_POLICY_MANIFEST)
        .map_err(|error| format!("GATE_POLICY_RESOURCE_INVALID:{error}"))?;
    let expected_digest = manifest
        .pointer("/files/policy.json")
        .and_then(Value::as_str)
        .ok_or_else(|| "GATE_POLICY_RESOURCE_DIGEST_MISSING".to_string())?;
    let actual_digest = hex::encode(Sha256::digest(DEFAULT_POLICY_BYTES));
    if actual_digest != expected_digest
        || manifest.get("resourceId").and_then(Value::as_str)
            != Some(DEFAULT_GATE_POLICY_RESOURCE_VERSION)
        || manifest.get("digestAlgorithm").and_then(Value::as_str) != Some("sha256-raw")
    {
        return Err("GATE_POLICY_RESOURCE_DIGEST_MISMATCH".into());
    }
    let mut policy: Value = serde_json::from_slice(DEFAULT_POLICY_BYTES)
        .map_err(|error| format!("GATE_POLICY_RESOURCE_INVALID:{error}"))?;
    policy["resourceRef"] = json!({
        "version":DEFAULT_GATE_POLICY_RESOURCE_VERSION,
        "sha256":actual_digest
    });
    validate_policy(&policy)?;
    Ok(policy)
}

pub(crate) fn default_policy_artifact(
    project_id: &str,
    artifact_id: String,
    version: i64,
    command_id: &str,
    now: &str,
) -> Result<ArtifactEnvelope, String> {
    let payload = default_policy_payload()?;
    let payload_hash = sha256_canonical(&payload).map_err(|error| error.to_string())?;
    serde_json::from_value(json!({
        "artifactId":artifact_id,
        "projectId":project_id,
        "artifactType":"GatePolicy",
        "scope":{"level":"PROJECT"},
        "version":version,
        "schemaVersion":"m0a.artifact.v1",
        "artifactStatus":"CANDIDATE",
        "semanticLifecycle":"MATERIALIZED",
        "payload":payload,
        "payloadHash":payload_hash,
        "dependencyRefs":[],
        "provenance":{"createdBy":"B08::GatePolicyOwner","commandId":command_id},
        "createdAt":now
    }))
    .map_err(|error| format!("GATE_POLICY_ARTIFACT_INVALID:{error}"))
}

pub(crate) fn ensure_default_policy(
    connection: &ProjectHandle,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    now: &str,
) -> Result<Option<CommitReceipt>, String> {
    if current_policy(connection, project_id)?.is_some() {
        return Ok(None);
    }
    let payload = default_policy_payload()?;
    let expected_revision = NarrativeRepository::project_revision(connection, project_id)
        .map_err(|error| error.to_string())?;
    commit_policy_payload(
        connection,
        project_id,
        command_id,
        idempotency_key,
        expected_revision,
        now,
        payload,
        None,
        "GATE_POLICY_DEFAULT_INITIALIZED",
        "GATE_POLICY_DEFAULT_INITIALIZED",
        false,
    )
    .map(Some)
}

pub(crate) fn require_long_form_policy_supported(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
) -> Result<(), String> {
    let (_, policy) = current_policy(connection, project_id)?
        .ok_or_else(|| "B08_GATE_POLICY_REQUIRED".to_string())?;
    if policy.get("policyId").and_then(Value::as_str) == Some("b11-opening") {
        return Err("GATE_POLICY_UPGRADE_REQUIRED".into());
    }
    validate_policy(&policy)
}

#[allow(clippy::too_many_arguments)]
pub fn execute_update(
    manager: &ProjectManager,
    request: &Value,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: Option<i64>,
    request_digest: &str,
    now: &str,
) -> Result<Value, String> {
    let expected_revision = expected_revision
        .ok_or_else(|| "IPC_INVALID:expectedRevision required for GatePolicy update".to_string())?;
    let action = request
        .pointer("/payload/action")
        .and_then(Value::as_str)
        .ok_or_else(|| "IPC_INVALID:payload.action".to_string())?;
    let connection = manager
        .open(project_id, "", now)
        .map_err(|error| error.to_string())?;
    if let Some(previous) = CommitService::find_receipt(&connection, project_id, idempotency_key)
        .map_err(|error| error.to_string())?
    {
        let previous_payload = previous
            .committed_artifact_refs
            .iter()
            .find(|reference| {
                previous.head_changes.iter().any(|head| {
                    head.artifact_type == crate::generated::types::ArtifactType::GatePolicy
                        && head.artifact_id == reference.artifact_id
                })
            })
            .and_then(|reference| {
                NarrativeRepository::artifact_payload_by_ref(&connection, project_id, reference)
                    .ok()
                    .flatten()
            });
        if previous_payload
            .as_ref()
            .and_then(|payload| payload.get("requestDigest"))
            .and_then(Value::as_str)
            == Some(request_digest)
        {
            return Ok(json!({
                "commandId":previous.command_id,
                "projectId":project_id,
                "finalStatus":"committed",
                "message":"GatePolicy 正式版本已找到。",
                "receiptId":previous.receipt_id,
                "beforeRevision":previous.before_revision,
                "afterRevision":previous.after_revision,
                "replayed":true,
                "receipt":previous
            }));
        }
        return Err("IDEMPOTENCY_CONFLICT".into());
    }
    let current = current_policy(&connection, project_id)?;
    let (next_payload, decision_type, outcome) = match action {
        "ADOPT_DEFAULT" => {
            let Some((_, current_payload)) = current.as_ref() else {
                return Err("B08_GATE_POLICY_REQUIRED".into());
            };
            if current_payload.get("policyId").and_then(Value::as_str) != Some("b11-opening") {
                return Err("GATE_POLICY_NOT_LEGACY".into());
            }
            (
                default_policy_payload()?,
                "GATE_POLICY_DEFAULT_ADOPTED",
                "DEFAULT_ADOPTED",
            )
        }
        "UPSERT_RULE" | "DISABLE_RULE" => {
            let Some((_, current_payload)) = current.as_ref() else {
                return Err("B08_GATE_POLICY_REQUIRED".into());
            };
            if current_payload.get("policyId").and_then(Value::as_str) == Some("b11-opening") {
                return Err("GATE_POLICY_UPGRADE_REQUIRED".into());
            }
            let mut payload = current_payload.clone();
            let rules = payload
                .pointer_mut("/canonSemantics/rules")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| "GATE_POLICY_RULE_INVALID".to_string())?;
            match action {
                "UPSERT_RULE" => {
                    let rule = request
                        .pointer("/payload/rule")
                        .filter(|rule| validate_rule_shape(rule).is_ok())
                        .cloned()
                        .ok_or_else(|| "GATE_POLICY_RULE_INVALID".to_string())?;
                    let predicate = rule
                        .get("predicate")
                        .and_then(Value::as_str)
                        .ok_or_else(|| "GATE_POLICY_RULE_INVALID".to_string())?;
                    if let Some(existing) = rules.iter_mut().find(|existing| {
                        existing.get("predicate").and_then(Value::as_str) == Some(predicate)
                    }) {
                        *existing = rule;
                    } else {
                        rules.push(rule);
                    }
                }
                "DISABLE_RULE" => {
                    let predicate = request
                        .pointer("/payload/predicate")
                        .and_then(Value::as_str)
                        .filter(|predicate| !predicate.trim().is_empty())
                        .ok_or_else(|| "IPC_INVALID:payload.predicate".to_string())?;
                    let before = rules.len();
                    rules.retain(|rule| {
                        rule.get("predicate").and_then(Value::as_str) != Some(predicate)
                    });
                    if rules.len() == before {
                        return Err("GATE_POLICY_RULE_NOT_FOUND".into());
                    }
                }
                _ => unreachable!("action was matched above"),
            }
            payload["policyId"] = json!("project-gate-policy");
            payload["canonSemantics"]["policyVersion"] =
                json!(format!("canon-semantics.project.{}", request_digest));
            validate_policy(&payload)?;
            (
                payload,
                if action == "UPSERT_RULE" {
                    "GATE_POLICY_RULE_UPSERTED"
                } else {
                    "GATE_POLICY_RULE_DISABLED"
                },
                action,
            )
        }
        _ => return Err("IPC_INVALID:payload.action".into()),
    };
    if let Some((_, old_payload)) = current.as_ref() {
        validate_no_new_canon_conflicts(&connection, project_id, old_payload, &next_payload)?;
    }
    let mut next_payload = next_payload;
    if let Some((previous_ref, _)) = current.as_ref() {
        next_payload["previousPolicyRef"] =
            serde_json::to_value(previous_ref).map_err(|error| error.to_string())?;
    }
    next_payload["requestDigest"] = json!(request_digest);
    next_payload["updatedAt"] = json!(now);
    let receipt = commit_policy_payload(
        &connection,
        project_id,
        command_id,
        idempotency_key,
        expected_revision,
        now,
        next_payload,
        current.as_ref().map(|(reference, _)| reference),
        decision_type,
        outcome,
        true,
    )?;
    Ok(json!({
        "commandId":command_id,
        "projectId":project_id,
        "finalStatus":"committed",
        "message":"GatePolicy 已生成新的正式版本。",
        "receiptId":receipt.receipt_id,
        "beforeRevision":receipt.before_revision,
        "afterRevision":receipt.after_revision,
        "replayed":false,
        "receipt":receipt
    }))
}

#[allow(clippy::too_many_arguments)]
fn commit_policy_payload(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    now: &str,
    payload: Value,
    previous_ref: Option<&ArtifactRef>,
    decision_type: &str,
    outcome: &str,
    advance_revision: bool,
) -> Result<CommitReceipt, String> {
    let artifact_id = previous_ref
        .map(|reference| reference.artifact_id.clone())
        .unwrap_or_else(|| format!("gate-policy:{project_id}"));
    let version = ArtifactRepository::next_version(connection, &artifact_id)
        .map_err(|error| error.to_string())?;
    let payload_hash = sha256_canonical(&payload).map_err(|error| error.to_string())?;
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId":artifact_id,
        "projectId":project_id,
        "artifactType":"GatePolicy",
        "scope":{"level":"PROJECT"},
        "version":version,
        "schemaVersion":"m0a.artifact.v1",
        "artifactStatus":"CANDIDATE",
        "semanticLifecycle":"MATERIALIZED",
        "payload":payload,
        "payloadHash":payload_hash,
        "dependencyRefs":previous_ref.into_iter().collect::<Vec<_>>(),
        "provenance":{"createdBy":"B08::GatePolicyOwner","commandId":command_id},
        "createdAt":now
    }))
    .map_err(|error| format!("GATE_POLICY_ARTIFACT_INVALID:{error}"))?;
    let artifact_ref = ArtifactRef {
        artifact_id: artifact.artifact_id.clone(),
        version,
        payload_hash: artifact.payload_hash.clone(),
    };
    let revision = NarrativeRepository::project_revision(connection, project_id)
        .map_err(|error| error.to_string())?;
    if revision != expected_revision {
        return Err("REVISION_CONFLICT".into());
    }
    let decision_id = StableIdentityFactory::from_command(
        StableIdKind::Decision,
        &format!("gate-policy:{command_id}"),
    )
    .map_err(|error| error.to_string())?;
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":format!("commit:{command_id}"),
        "projectId":project_id,
        "commandId":command_id,
        "idempotencyKey":idempotency_key,
        "expectedProjectRevision":expected_revision,
        "contractVersion":"m0a.v1",
        "createdAt":now,
        "artifacts":[artifact],
        "headUpdates":[{"artifactType":"GatePolicy","scope":{"level":"PROJECT"},"artifactId":artifact_ref.artifact_id,"version":artifact_ref.version,"payloadHash":artifact_ref.payload_hash}],
        "decisionUpdates":[{"decisionId":decision_id,"decisionType":decision_type,"outcome":outcome,"reason":"用户通过正式 GatePolicy 命令更新规则。","sourceRef":artifact_ref.artifact_id,"payloadJson":{"gatePolicyRef":artifact_ref}}],
        "receiptDraft":{"domain":"B08_GATE_POLICY","artifactRefs":[artifact_ref],"payload":{"decisionType":decision_type,"outcome":outcome}}
    }))
    .map_err(|error| format!("GATE_POLICY_BUNDLE_INVALID:{error}"))?;
    if advance_revision {
        CommitService::commit(connection, &bundle, now).map_err(|error| error.to_string())
    } else {
        CommitService::commit_without_revision(connection, &bundle, now)
            .map_err(|error| error.to_string())
    }
}

fn validate_policy(payload: &Value) -> Result<(), String> {
    crate::canon_semantics::CanonSemanticPolicy::from_gate_policy(payload)
        .map(|_| ())
        .map_err(|_| "GATE_POLICY_RULE_INVALID".to_string())
}

fn validate_rule_shape(rule: &Value) -> Result<(), ()> {
    let object = rule.as_object().ok_or(())?;
    let allowed = [
        "predicate",
        "cardinality",
        "scope",
        "timeBasis",
        "mutualExclusions",
        "polarityField",
        "equivalenceSets",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    if object.keys().any(|key| !allowed.contains(key.as_str())) {
        return Err(());
    }
    let predicate = object
        .get("predicate")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(())?;
    let cardinality = object
        .get("cardinality")
        .and_then(Value::as_str)
        .ok_or(())?;
    if !matches!(cardinality, "SINGLE" | "MULTI") {
        return Err(());
    }
    let scope = object.get("scope").and_then(Value::as_object).ok_or(())?;
    match scope.get("kind").and_then(Value::as_str) {
        Some("FIXED")
            if scope
                .get("value")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
                && scope.len() == 2 => {}
        Some("OBJECT_FIELD")
            if scope
                .get("field")
                .and_then(Value::as_str)
                .is_some_and(|field| !field.trim().is_empty())
                && scope.len() == 2 => {}
        _ => return Err(()),
    }
    let time_basis = object.get("timeBasis").and_then(Value::as_str);
    if time_basis.is_some_and(|basis| !matches!(basis, "WORLD_TIME" | "VALIDITY_CHAPTER")) {
        return Err(());
    }
    let mutual_exclusions = object
        .get("mutualExclusions")
        .map(|value| value.as_array().ok_or(()))
        .transpose()?
        .cloned()
        .unwrap_or_default();
    for exclusion in &mutual_exclusions {
        let pair = exclusion.as_object().ok_or(())?;
        if pair.len() != 2
            || pair.get("left").is_none_or(Value::is_null)
            || pair.get("right").is_none_or(Value::is_null)
        {
            return Err(());
        }
    }
    if let Some(sets) = object.get("equivalenceSets") {
        let sets = sets.as_array().ok_or(())?;
        if sets.iter().any(|set| {
            set.as_array()
                .is_none_or(|items| items.len() < 2 || items.iter().any(Value::is_null))
        }) {
            return Err(());
        }
    }
    if object
        .get("polarityField")
        .is_some_and(|value| value.as_str().is_none_or(str::is_empty))
    {
        return Err(());
    }
    let time_required = cardinality == "SINGLE"
        || !mutual_exclusions.is_empty()
        || object.contains_key("polarityField");
    if time_required && time_basis.is_none() {
        return Err(());
    }
    if predicate.len() > 120 {
        return Err(());
    }
    Ok(())
}

fn validate_no_new_canon_conflicts(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    old_payload: &Value,
    new_payload: &Value,
) -> Result<(), String> {
    use crate::canon_semantics::{CandidateSemanticInput, CanonSemanticPolicy, SemanticOutcome};

    let new_policy = CanonSemanticPolicy::from_gate_policy(new_payload)
        .map_err(|_| "GATE_POLICY_RULE_INVALID".to_string())?;
    let old_policy = CanonSemanticPolicy::from_gate_policy(old_payload).ok();
    let predicates = new_payload
        .pointer("/canonSemantics/rules")
        .and_then(Value::as_array)
        .ok_or_else(|| "GATE_POLICY_RULE_INVALID".to_string())?
        .iter()
        .filter_map(|rule| {
            rule.get("predicate")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect::<Vec<_>>();
    let (groups, truncated) = NarrativeRepository::current_canon_semantic_groups_bounded(
        connection,
        project_id,
        &predicates,
        CANON_SCAN_LIMIT,
    )
    .map_err(|error| error.to_string())?;
    if truncated {
        return Err("GATE_POLICY_CANON_SCAN_INCOMPLETE".into());
    }
    for group in groups {
        for left_index in 0..group.facts.len() {
            for right_index in (left_index + 1)..group.facts.len() {
                let current = &group.facts[left_index];
                let candidate = &group.facts[right_index];
                let candidate_input = CandidateSemanticInput {
                    object_json: &candidate.object_json,
                    story_time: candidate.story_time_from.as_deref(),
                    valid_from_chapter_no: candidate.valid_from_chapter_no,
                    valid_to_chapter_no: candidate.valid_to_chapter_no,
                };
                if matches!(
                    new_policy.evaluate(&group.predicate, current, candidate_input.clone()),
                    SemanticOutcome::Conflict(_)
                ) && !old_policy.as_ref().is_some_and(|policy| {
                    matches!(
                        policy.evaluate(&group.predicate, current, candidate_input),
                        SemanticOutcome::Conflict(_)
                    )
                }) {
                    return Err("GATE_POLICY_CANON_CONFLICT".into());
                }
            }
        }
    }
    Ok(())
}
