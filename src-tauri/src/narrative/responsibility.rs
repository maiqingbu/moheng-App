use crate::commit::CommitService;
use crate::generated::types::{ArtifactEnvelope, ArtifactRef, CommitBundle};
use crate::hash::sha256_canonical;
use crate::project_db::ProjectManager;
use crate::repositories::narrative::NarrativeRepository;
use crate::stable_id::{StableIdKind, StableIdentityFactory};
use serde_json::{json, Value};

pub struct AdjustmentCommand<'a> {
    pub request: &'a Value,
    pub project_id: &'a str,
    pub command_id: &'a str,
    pub idempotency_key: &'a str,
    pub expected_revision: Option<i64>,
    pub request_digest: &'a str,
    pub requested_at: &'a str,
}

pub fn execute_adjustment(
    manager: &ProjectManager,
    command: AdjustmentCommand<'_>,
) -> Result<Value, String> {
    let AdjustmentCommand {
        request,
        project_id,
        command_id,
        idempotency_key,
        expected_revision,
        request_digest,
        requested_at,
    } = command;
    let expected_revision = expected_revision.ok_or_else(|| {
        "IPC_INVALID:expectedRevision required for responsibility adjustment".to_string()
    })?;
    let payload = request
        .get("payload")
        .and_then(Value::as_object)
        .ok_or_else(|| "IPC_INVALID:payload".to_string())?;
    let thread_ref = payload
        .get("threadRef")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "IPC_INVALID:threadRef".to_string())?;
    let obligation_id = thread_ref
        .strip_prefix("thread:")
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "IPC_INVALID:threadRef".to_string())?;
    let action = payload
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| "IPC_INVALID:action".to_string())?;
    let artifact_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("narrative-obligation-set:{project_id}:{obligation_id}"),
    )
    .map_err(|error| error.to_string())?;
    let change_request_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("responsibility-change-request:{project_id}:{command_id}"),
    )
    .map_err(|error| error.to_string())?;
    let connection = manager
        .open(project_id, "", requested_at)
        .map_err(|error| error.to_string())?;

    let mut previous_receipt =
        CommitService::find_receipt(&connection, project_id, idempotency_key)
            .map_err(|error| error.to_string())?;
    if previous_receipt.is_none() && action == "DELETE" {
        let previous_key = NarrativeRepository::commit_receipt_idempotency_key_by_command(
            &connection,
            project_id,
            command_id,
        )
        .map_err(|error| error.to_string())?;
        if let Some(previous_key) = previous_key {
            previous_receipt = CommitService::find_receipt(&connection, project_id, &previous_key)
                .map_err(|error| error.to_string())?;
        }
    }
    if let Some(previous) = previous_receipt {
        let previous_proposal = previous
            .committed_artifact_refs
            .iter()
            .find(|reference| reference.artifact_id == change_request_id)
            .and_then(|reference| {
                NarrativeRepository::artifact_payload_by_ref(&connection, project_id, reference)
                    .ok()
                    .flatten()
            });
        if previous_proposal
            .as_ref()
            .and_then(|proposal| proposal.get("requestDigest"))
            .and_then(Value::as_str)
            == Some(request_digest)
        {
            return Ok(json!({
                "commandId":previous.command_id,
                "projectId":project_id,
                "finalStatus":"committed",
                "actionStatus":"CHANGE_REQUEST_SUBMITTED",
                "message":"B08_RESPONSIBILITY_CHANGE_REQUEST_CREATED",
                "changeRequestId":change_request_id,
                "receiptId":previous.receipt_id,
                "beforeRevision":previous.before_revision,
                "afterRevision":previous.after_revision,
                "replayed":true,
                "receipt":previous
            }));
        }
        let previous_snapshot = previous
            .committed_artifact_refs
            .iter()
            .find(|reference| reference.artifact_id == artifact_id)
            .and_then(|reference| {
                NarrativeRepository::artifact_payload_by_ref(&connection, project_id, reference)
                    .ok()
                    .flatten()
            });
        if previous_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.get("requestDigest"))
            .and_then(Value::as_str)
            == Some(request_digest)
        {
            return Ok(json!({
                "commandId":previous.command_id,
                "projectId":project_id,
                "finalStatus":"committed",
                "message":"责任调整正式回执已找到。",
                "receiptId":previous.receipt_id,
                "beforeRevision":previous.before_revision,
                "afterRevision":previous.after_revision,
                "replayed":true,
                "receipt":previous
            }));
        }
        return Err("IDEMPOTENCY_CONFLICT".into());
    }

    let record = NarrativeRepository::responsibility_adjustment_record(
        &connection,
        project_id,
        obligation_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "B08_RESPONSIBILITY_NOT_FOUND".to_string())?;
    if matches!(
        record.lifecycle.as_str(),
        "RESOLVED" | "ABANDONED" | "SUPERSEDED"
    ) {
        return Err("B08_RESPONSIBILITY_NOT_ACTIVE".into());
    }
    if matches!(action, "DELETE" | "DOWNGRADE")
        && !NarrativeRepository::responsibility_adjustment_is_user_editable(
            &connection,
            project_id,
            obligation_id,
        )
        .map_err(|error| error.to_string())?
    {
        return Err("B08_RESPONSIBILITY_CHANGE_REQUEST_REQUIRED".into());
    }
    let target_level = match action {
        "DOWNGRADE" => {
            if record.authority_level != "B" {
                return Err("B08_RESPONSIBILITY_DOWNGRADE_INVALID".into());
            }
            "C"
        }
        "UPGRADE" => {
            if record.authority_level != "C" {
                return Err("B08_RESPONSIBILITY_UPGRADE_INVALID".into());
            }
            match payload.get("authorityLevel").and_then(Value::as_str) {
                Some("A") => "A",
                Some("B") => "B",
                _ => return Err("IPC_INVALID:authorityLevel".into()),
            }
        }
        "DELETE" => record.authority_level.as_str(),
        _ => return Err("IPC_INVALID:action".into()),
    };
    if action == "DELETE" && matches!(record.authority_level.as_str(), "A" | "B") {
        return create_responsibility_change_request(
            &connection,
            project_id,
            command_id,
            idempotency_key,
            expected_revision,
            request_digest,
            requested_at,
            thread_ref,
            &record,
            &change_request_id,
        );
    }
    let fulfillment_object = if action == "UPGRADE" {
        payload
            .get("fulfillmentObject")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| record.fulfillment_object.clone())
            .ok_or_else(|| "B08_RESPONSIBILITY_UPGRADE_OBJECT_REQUIRED".to_string())?
    } else {
        record.fulfillment_object.clone().unwrap_or_default()
    };
    let resume_condition = if action == "UPGRADE" {
        let condition = payload
            .get("resumeCondition")
            .cloned()
            .unwrap_or_else(|| record.resume_condition.clone());
        crate::narrative::runtime::validate_resume_condition(&condition)
            .map_err(|_| "B08_RESPONSIBILITY_UPGRADE_RESUME_CONDITION_REQUIRED")?;
        condition
    } else {
        record.resume_condition.clone()
    };
    let mut source_evidence_refs = record.evidence_refs.clone();
    if action == "UPGRADE"
        && !source_evidence_refs.iter().any(|evidence| {
            evidence.range_start.is_some()
                && evidence.range_end.is_some()
                && evidence
                    .quote
                    .as_deref()
                    .is_some_and(|quote| !quote.trim().is_empty())
        })
    {
        let supplied = payload.get("evidence").and_then(Value::as_object);
        let has_start = supplied.is_some_and(|evidence| evidence.contains_key("start"));
        let has_end = supplied.is_some_and(|evidence| evidence.contains_key("end"));
        let (start, end, quote) = match supplied {
            Some(evidence) => (
                evidence
                    .get("start")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize),
                evidence
                    .get("end")
                    .and_then(Value::as_u64)
                    .map(|value| value as usize),
                evidence.get("quote").and_then(Value::as_str),
            ),
            None => (None, None, None),
        };
        let Some(quote) = quote else {
            return Err("B08_RESPONSIBILITY_UPGRADE_EVIDENCE_REQUIRED".into());
        };
        let body = NarrativeRepository::final_chapter_body_for_ref(
            &connection,
            project_id,
            &record.source_artifact_id,
            record.source_version,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "B08_RESPONSIBILITY_UPGRADE_EVIDENCE_REQUIRED".to_string())?;
        let (start, end) = match (has_start, has_end) {
            (true, true) => match (start, end) {
                (Some(start), Some(end)) => (start, end),
                _ => return Err("B08_RESPONSIBILITY_UPGRADE_EVIDENCE_REQUIRED".into()),
            },
            (false, false) => unique_quote_span(&body, quote)
                .ok_or_else(|| "EVIDENCE_QUOTE_NOT_UNIQUE".to_string())?,
            _ => return Err("B08_RESPONSIBILITY_UPGRADE_EVIDENCE_REQUIRED".into()),
        };
        crate::production::exact_quote_span(&body, start, end, quote)
            .map_err(|_| "B08_RESPONSIBILITY_UPGRADE_EVIDENCE_INVALID".to_string())?;
        source_evidence_refs.push(crate::generated::types::EvidenceRef {
            artifact_id: record.source_artifact_id.clone(),
            version: record.source_version,
            range_start: Some(start as i64),
            range_end: Some(end as i64),
            quote: Some(quote.to_owned()),
        });
    }
    let lifecycle = if action == "DELETE" {
        "ABANDONED"
    } else if action == "UPGRADE" && record.lifecycle == "PROPOSED" {
        "PROGRESSING"
    } else {
        record.lifecycle.as_str()
    };
    let project_scope_key = json!({"level":"PROJECT"}).to_string();
    let previous_ref = NarrativeRepository::canonical_head_ref(
        &connection,
        project_id,
        "NarrativeObligationSet",
        &project_scope_key,
    )
    .map_err(|error| error.to_string())?;
    let version =
        crate::repositories::artifacts::ArtifactRepository::next_version(&connection, &artifact_id)
            .map_err(|error| error.to_string())?;
    let snapshot_payload = json!({
        "payloadSchema":"narrative-obligation-adjustment.r1",
        "obligationId":record.obligation_id,
        "threadRef":thread_ref,
        "action":action,
        "previousAuthorityLevel":record.authority_level,
        "authorityLevel":target_level,
        "previousLifecycle":record.lifecycle,
        "lifecycle":lifecycle,
        "description":record.description,
        "fulfillmentObject":fulfillment_object,
        "resumeCondition":resume_condition,
        "sourceRef":{"artifactId":record.source_artifact_id,"version":record.source_version},
        "sourceEvidenceRefs":source_evidence_refs,
        "previousAdjustmentRef":previous_ref,
        "actor":"USER",
        "operator":"USER",
        "changedAt":requested_at,
        "requestDigest":request_digest
    });
    let payload_hash = sha256_canonical(&snapshot_payload).map_err(|error| error.to_string())?;
    let artifact_ref = ArtifactRef {
        artifact_id: artifact_id.clone(),
        version,
        payload_hash: payload_hash.clone(),
    };
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId":artifact_id,
        "projectId":project_id,
        "artifactType":"NarrativeObligationSet",
        "scope":{"level":"PROJECT"},
        "version":version,
        "schemaVersion":"m0a.artifact.v1",
        "artifactStatus":"CANDIDATE",
        "semanticLifecycle":"MATERIALIZED",
        "payload":snapshot_payload,
        "payloadHash":payload_hash,
        "dependencyRefs":[],
        "provenance":{"createdBy":"B08::ResponsibilityAdjustmentOwner","commandId":command_id},
        "createdAt":requested_at
    }))
    .map_err(|error| error.to_string())?;
    let reason = match action {
        "DOWNGRADE" => "用户将责任从 B 级降为 C 级。",
        "UPGRADE" => "用户手动升级责任等级。",
        "DELETE" => "用户撤销这项责任；原审计记录与证据保留。",
        _ => unreachable!("action was validated above"),
    };
    let decision_id = StableIdentityFactory::from_command(
        StableIdKind::Decision,
        &format!("thread-adjustment:{command_id}"),
    )
    .map_err(|error| error.to_string())?;
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":format!("commit:{command_id}"),
        "projectId":project_id,
        "commandId":command_id,
        "idempotencyKey":idempotency_key,
        "expectedProjectRevision":expected_revision,
        "contractVersion":"m0a.v1",
        "createdAt":requested_at,
        "artifacts":[artifact],
        "headUpdates":[{"artifactType":"NarrativeObligationSet","scope":{"level":"PROJECT"},"artifactId":artifact_ref.artifact_id,"version":artifact_ref.version,"payloadHash":artifact_ref.payload_hash}],
        "obligationDelta":[{
            "obligationId":record.obligation_id,
            "scope":record.scope,
            "obligationType":record.obligation_type,
            "authorityLevel":target_level,
            "lifecycle":lifecycle,
            "description":record.description,
            "mustResolveBeforeChapter":record.must_resolve_before_chapter,
        "prerequisites":record.prerequisites,
            "decisionRef":decision_id,
            "payloadJson":{"sourceArtifactId":artifact_ref.artifact_id,"sourceVersion":artifact_ref.version,"resumeCondition":resume_condition,"fulfillmentObject":fulfillment_object,"action":action}
        }],
        "decisionUpdates":[{
            "decisionId":decision_id,
            "decisionType":"USER_THREAD_RESPONSIBILITY_ADJUSTMENT",
            "outcome":action,
            "reason":reason,
            "sourceRef":thread_ref,
            "payloadJson":{"previousAuthorityLevel":record.authority_level,"authorityLevel":target_level,"lifecycle":lifecycle,"fulfillmentObject":fulfillment_object,"resumeCondition":resume_condition,"sourceEvidenceRefs":source_evidence_refs,"actor":"USER","changedAt":requested_at,"narrativeObligationSetRef":artifact_ref}
        }],
        "receiptDraft":{"domain":"B08_THREAD_RESPONSIBILITY_ADJUSTMENT","payload":{"threadRef":thread_ref,"action":action,"authorityLevel":target_level,"lifecycle":lifecycle,"narrativeObligationSetRef":artifact_ref}}
    }))
    .map_err(|error| error.to_string())?;
    let receipt = CommitService::commit(&connection, &bundle, requested_at)
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "commandId":command_id,
        "projectId":project_id,
        "finalStatus":"committed",
        "message":"责任调整已正式提交，历史版本与原始证据均已保留。",
        "receiptId":receipt.receipt_id,
        "beforeRevision":receipt.before_revision,
        "afterRevision":receipt.after_revision,
        "replayed":false,
        "receipt":receipt
    }))
}

#[allow(clippy::too_many_arguments)]
fn create_responsibility_change_request(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    expected_revision: i64,
    request_digest: &str,
    requested_at: &str,
    thread_ref: &str,
    record: &crate::repositories::narrative::ResponsibilityAdjustmentRecord,
    change_request_id: &str,
) -> Result<Value, String> {
    let source_ref = NarrativeRepository::artifact_ref_for_version(
        connection,
        project_id,
        &record.source_artifact_id,
        record.source_version,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "B08_RESPONSIBILITY_SOURCE_UNAVAILABLE".to_string())?;
    let payload = json!({
        "payloadSchema":"b12-change-request.r1",
        "changeRequestId":change_request_id,
        "changeType":"CHANGE",
        "status":"PROPOSED",
        "summary":format!("申请撤销{}级责任：{}", record.authority_level, record.description),
        "requestedChange":{"kind":"WITHDRAW_RESPONSIBILITY","responsibilityRef":thread_ref},
        "responsibilityRef":thread_ref,
        "basedOnRevision":expected_revision,
        "sourceRef":source_ref,
        "sourceEvidenceRefs":record.evidence_refs,
        "requestDigest":request_digest
    });
    let payload_hash = sha256_canonical(&payload).map_err(|error| error.to_string())?;
    let version = crate::repositories::artifacts::ArtifactRepository::next_version(
        connection,
        change_request_id,
    )
    .map_err(|error| error.to_string())?;
    let artifact_ref = ArtifactRef {
        artifact_id: change_request_id.to_owned(),
        version,
        payload_hash: payload_hash.clone(),
    };
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId":change_request_id,
        "projectId":project_id,
        "artifactType":"ChangeRequest",
        "scope":{"level":"PROJECT"},
        "version":version,
        "schemaVersion":"m0a.artifact.v1",
        "artifactStatus":"CANDIDATE",
        "semanticLifecycle":"MATERIALIZED",
        "payload":payload,
        "payloadHash":payload_hash,
        "dependencyRefs":[source_ref],
        "provenance":{"createdBy":"B08::ResponsibilityChangeRequestOwner","commandId":command_id},
        "createdAt":requested_at
    }))
    .map_err(|error| error.to_string())?;
    let decision_id = StableIdentityFactory::from_command(
        StableIdKind::Decision,
        &format!("responsibility-change-request:{project_id}:{command_id}"),
    )
    .map_err(|error| error.to_string())?;
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":format!("commit:{command_id}"),
        "projectId":project_id,
        "commandId":command_id,
        "idempotencyKey":idempotency_key,
        "expectedProjectRevision":expected_revision,
        "contractVersion":"m0a.v1",
        "createdAt":requested_at,
        "artifacts":[artifact],
        "headUpdates":[{"artifactType":"ChangeRequest","scope":{"level":"PROJECT"},"artifactId":change_request_id,"version":version,"payloadHash":payload_hash}],
        "decisionUpdates":[{"decisionId":decision_id,"decisionType":"NO_LONGER_APPLICABLE_CHANGE_REQUEST","outcome":"PROPOSED","reason":"用户请求撤销 A/B 级责任，等待正式变更流程确认。","sourceRef":thread_ref,"payloadJson":{"changeRequestRef":artifact_ref,"responsibilityRef":thread_ref}}],
        "receiptDraft":{"domain":"B08_RESPONSIBILITY_CHANGE_REQUEST","payload":{"changeRequestRef":artifact_ref,"responsibilityRef":thread_ref,"status":"PROPOSED"}}
    }))
    .map_err(|error| error.to_string())?;
    let receipt = CommitService::commit(connection, &bundle, requested_at)
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "commandId":command_id,
        "projectId":project_id,
        "finalStatus":"committed",
        "actionStatus":"CHANGE_REQUEST_SUBMITTED",
        "message":"B08_RESPONSIBILITY_CHANGE_REQUEST_CREATED",
        "changeRequestId":change_request_id,
        "receiptId":receipt.receipt_id,
        "beforeRevision":receipt.before_revision,
        "afterRevision":receipt.after_revision,
        "replayed":false,
        "receipt":receipt
    }))
}

fn unique_quote_span(body: &str, quote: &str) -> Option<(usize, usize)> {
    if quote.is_empty() {
        return None;
    }

    let mut cursor = 0;
    let mut match_start = None;
    while cursor <= body.len() {
        let Some(relative_start) = body.get(cursor..)?.find(quote) else {
            break;
        };
        let byte_start = cursor + relative_start;
        if match_start.is_some() {
            return None;
        }
        match_start = Some(byte_start);
        let next_char_len = body.get(byte_start..)?.chars().next()?.len_utf8();
        cursor = byte_start + next_char_len;
    }

    let byte_start = match_start?;
    let start = body.get(..byte_start)?.chars().count();
    let end = start + quote.chars().count();
    Some((start, end))
}

/// FX-12: decides a pending WITHDRAW_RESPONSIBILITY ChangeRequest (from the B08
/// audit or a user DELETE of an A/B responsibility). One CommitService commit
/// updates the responsibility, the request and its proposal decision together.
pub fn execute_withdraw_decision(
    manager: &ProjectManager,
    command: AdjustmentCommand<'_>,
) -> Result<Value, String> {
    let AdjustmentCommand {
        request,
        project_id,
        command_id,
        idempotency_key,
        expected_revision,
        requested_at,
        ..
    } = command;
    let expected_revision = expected_revision
        .ok_or_else(|| "IPC_INVALID:expectedRevision required for withdraw decision".to_string())?;
    let payload = request
        .get("payload")
        .ok_or_else(|| "IPC_INVALID:payload".to_string())?;
    let change_request_id = payload
        .get("changeRequestId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "IPC_INVALID:changeRequestId".to_string())?;
    let approve = match payload.get("decision").and_then(Value::as_str) {
        Some("APPROVE") => true,
        Some("REJECT") => false,
        _ => return Err("IPC_INVALID:decision".into()),
    };
    let action_status = if approve {
        "WITHDRAW_APPROVED"
    } else {
        "WITHDRAW_REJECTED"
    };
    let connection = manager
        .open(project_id, "", requested_at)
        .map_err(|error| error.to_string())?;
    let response = |receipt: crate::generated::types::CommitReceipt, replayed: bool| {
        json!({
            "commandId":command_id,
            "projectId":project_id,
            "finalStatus":"committed",
            "actionStatus":action_status,
            "changeRequestId":change_request_id,
            "receiptId":receipt.receipt_id,
            "beforeRevision":receipt.before_revision,
            "afterRevision":receipt.after_revision,
            "replayed":replayed,
            "receipt":receipt
        })
    };
    if let Some(previous) = CommitService::find_receipt(&connection, project_id, idempotency_key)
        .map_err(|error| error.to_string())?
    {
        return Ok(response(previous, true));
    }

    let (request_ref, mut request_payload, request_scope) =
        NarrativeRepository::latest_committed_artifact(
            &connection,
            project_id,
            change_request_id,
            "ChangeRequest",
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "RESPONSIBILITY_WITHDRAW_NOT_FOUND".to_string())?;
    if request_payload
        .pointer("/requestedChange/kind")
        .and_then(Value::as_str)
        != Some("WITHDRAW_RESPONSIBILITY")
    {
        return Err("RESPONSIBILITY_WITHDRAW_NOT_FOUND".into());
    }
    if request_payload.get("status").and_then(Value::as_str) != Some("PROPOSED") {
        return Err("RESPONSIBILITY_WITHDRAW_NOT_PENDING".into());
    }
    let thread_ref = request_payload
        .get("responsibilityRef")
        .and_then(Value::as_str)
        .ok_or_else(|| "RESPONSIBILITY_WITHDRAW_NOT_FOUND".to_string())?
        .to_owned();
    let authority_id = thread_ref.strip_prefix("thread:").unwrap_or(&thread_ref);
    let decision_id = NarrativeRepository::withdraw_request_decision_id(
        &connection,
        project_id,
        change_request_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "RESPONSIBILITY_WITHDRAW_NOT_FOUND".to_string())?;

    let mut obligation_delta = Vec::new();
    let mut foreshadow_delta = Vec::new();
    if approve {
        if let Some(record) = NarrativeRepository::responsibility_adjustment_record(
            &connection,
            project_id,
            authority_id,
        )
        .map_err(|error| error.to_string())?
        {
            if matches!(
                record.lifecycle.as_str(),
                "RESOLVED" | "ABANDONED" | "SUPERSEDED"
            ) {
                return Err("B08_RESPONSIBILITY_NOT_ACTIVE".into());
            }
            obligation_delta.push(json!({
                "obligationId":record.obligation_id,
                "scope":record.scope,
                "obligationType":record.obligation_type,
                "authorityLevel":record.authority_level,
                "lifecycle":"ABANDONED",
                "description":record.description,
                "mustResolveBeforeChapter":record.must_resolve_before_chapter,
                "prerequisites":record.prerequisites,
                "decisionRef":decision_id,
                // Explicit source: the bundle's first artifact (the request) must
                // not replace the responsibility's original authority source.
                "payloadJson":{"sourceArtifactId":record.source_artifact_id,"sourceVersion":record.source_version,"resumeCondition":record.resume_condition,"fulfillmentObject":record.fulfillment_object,"action":"WITHDRAW","changeRequestRef":request_ref}
            }));
        } else if let Some(foreshadow) =
            NarrativeRepository::foreshadow_authority(&connection, project_id, authority_id)
                .map_err(|error| error.to_string())?
        {
            if matches!(foreshadow.status.as_str(), "RECOVERED" | "ABANDONED") {
                return Err("B08_RESPONSIBILITY_NOT_ACTIVE".into());
            }
            foreshadow_delta.push(json!({
                "foreshadowId":authority_id,
                "intent":foreshadow.intent,
                "status":"ABANDONED",
                "plantChapterId":foreshadow.plant_chapter_id,
                "recoverWindowEndChapter":foreshadow.recover_window_end_chapter,
                "forbiddenRevealBeforeChapter":foreshadow.forbidden_reveal_before_chapter,
                "secretPayloadRef":foreshadow.secret_payload_ref,
                "payloadJson":{"sourceAuthority":foreshadow.source_authority,"changeRequestRef":request_ref}
            }));
        } else {
            return Err("B08_RESPONSIBILITY_NOT_FOUND".into());
        }
    }

    let outcome = if approve { "APPROVED" } else { "REJECTED" };
    request_payload["status"] = json!(outcome);
    request_payload["decidedAt"] = json!(requested_at);
    request_payload["decisionCommandId"] = json!(command_id);
    let payload_hash = sha256_canonical(&request_payload).map_err(|error| error.to_string())?;
    let version = crate::repositories::artifacts::ArtifactRepository::next_version(
        &connection,
        change_request_id,
    )
    .map_err(|error| error.to_string())?;
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId":change_request_id,
        "projectId":project_id,
        "artifactType":"ChangeRequest",
        "scope":request_scope,
        "version":version,
        "schemaVersion":"m0a.artifact.v1",
        "artifactStatus":"CANDIDATE",
        "semanticLifecycle":"MATERIALIZED",
        "payload":request_payload,
        "payloadHash":payload_hash,
        "dependencyRefs":[request_ref],
        "provenance":{"createdBy":"B08::ResponsibilityWithdrawDecisionOwner","commandId":command_id},
        "createdAt":requested_at
    }))
    .map_err(|error| error.to_string())?;
    let bundle = json!({
        "commitId":format!("commit:{command_id}"),
        "projectId":project_id,
        "commandId":command_id,
        "idempotencyKey":idempotency_key,
        "expectedProjectRevision":expected_revision,
        "contractVersion":"m0a.v1",
        "createdAt":requested_at,
        "artifacts":[artifact],
        "headUpdates":[{"artifactType":"ChangeRequest","scope":request_scope,"artifactId":change_request_id,"version":version,"payloadHash":payload_hash}],
        "obligationDelta":obligation_delta,
        "foreshadowDelta":foreshadow_delta,
        "decisionUpdates":[{
            "decisionId":decision_id,
            "decisionType":"NO_LONGER_APPLICABLE_CHANGE_REQUEST",
            "outcome":outcome,
            "reason":if approve { "用户批准撤销这项责任；原审计记录与证据保留。" } else { "用户拒绝撤销，这项责任保持有效。" },
            "sourceRef":thread_ref,
            "payloadJson":{"threadRef":thread_ref,"changeRequestRef":{"artifactId":change_request_id,"version":version,"payloadHash":payload_hash},"previousChangeRequestRef":request_ref,"actor":"USER","decidedAt":requested_at}
        }],
        "receiptDraft":{"domain":"B08_RESPONSIBILITY_WITHDRAW_DECISION","payload":{"threadRef":thread_ref,"changeRequestId":change_request_id,"outcome":outcome}}
    });
    let bundle: CommitBundle = serde_json::from_value(bundle).map_err(|error| error.to_string())?;
    let receipt = CommitService::commit(&connection, &bundle, requested_at)
        .map_err(|error| error.to_string())?;
    Ok(response(receipt, false))
}
