//! B02 opening evaluation and simulated-signing owner.
//!
//! The owner evaluates only committed golden-three reality.  It does not
//! invent a commercial score, call a second model, or write a second Canon;
//! the formal SimulatedSigningReceipt is committed through CommitService and
//! the runtime signing row is only its rebuildable projection.

use crate::commit::CommitService;
use crate::generated::types::{ArtifactEnvelope, ArtifactRef, CommitBundle};
use crate::hash::sha256_canonical;
use crate::project_db::ProjectManager;
use crate::repositories::narrative::{NarrativeRepository, OpeningEvaluationSnapshot};
use crate::repositories::runtime::DurableJobService;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const EVALUATION_STEPS: &[(&str, &str)] = &[
    ("READ_OPENING_CANON", "读取三章事实"),
    ("SUMMARIZE_OPENING", "汇总开篇状态"),
    ("EVALUATE_SIGNING", "校验开篇评估"),
    ("COMMIT_RECEIPT", "提交正式签约回执"),
];

fn required(request: &Value, path: &str) -> Result<String, String> {
    request
        .pointer(path)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("IPC_INVALID:{path}"))
}

fn ref_value(reference: &ArtifactRef) -> Value {
    json!({
        "artifactId": reference.artifact_id,
        "version": reference.version,
        "payloadHash": reference.payload_hash
    })
}

fn dedupe_refs(refs: impl IntoIterator<Item = ArtifactRef>) -> Vec<ArtifactRef> {
    let mut seen = BTreeSet::new();
    refs.into_iter()
        .filter(|reference| {
            seen.insert((
                reference.artifact_id.clone(),
                reference.version,
                reference.payload_hash.clone(),
            ))
        })
        .collect()
}

fn status_from_snapshot(snapshot: &OpeningEvaluationSnapshot) -> (&'static str, &'static str) {
    let complete = snapshot.chapters.len() == 3
        && snapshot
            .chapters
            .iter()
            .enumerate()
            .all(|(index, chapter)| {
                chapter.chapter_no == index as i64 + 1
                    && chapter.final_ref.is_some()
                    && !chapter.evidence.is_empty()
                    && !chapter.canon_refs.is_empty()
            })
        && snapshot.opening_canon_refs.len() >= 3
        && snapshot.signing_evidence_refs.len() >= 3;
    if complete {
        (
            "PASS",
            "黄金三章均已正式提交，开篇 Canon 与逐章证据完整可追溯。",
        )
    } else {
        (
            "REVISE",
            "开篇评估需要修订：三章正式事实或逐章证据尚未完整。",
        )
    }
}

fn evaluation_steps() -> Value {
    Value::Array(
        EVALUATION_STEPS
            .iter()
            .map(|(id, label)| json!({"id":id,"label":label,"status":"done"}))
            .collect(),
    )
}

fn result_from_payload(payload: &Value, receipt_ref: &ArtifactRef) -> Value {
    json!({
        "status": payload.get("status").cloned().unwrap_or_else(|| json!("REVISE")),
        "verdict": payload.get("verdict").cloned().unwrap_or(Value::Null),
        "summary": payload.get("summary").cloned().unwrap_or(Value::Null),
        "receiptRef": ref_value(receipt_ref),
        "openingChapterRefs": payload.get("openingChapterRefs").cloned().unwrap_or_else(|| json!([])),
        "openingCanonRefs": payload.get("openingCanonRefs").cloned().unwrap_or_else(|| json!([])),
        "signingEvidenceRefs": payload.get("signingEvidenceRefs").cloned().unwrap_or_else(|| json!([])),
        "evaluation": payload.get("evaluation").cloned().unwrap_or_else(|| json!({}))
    })
}

fn command_response(
    command_id: &str,
    project_id: &str,
    receipt: &crate::generated::types::CommitReceipt,
    payload: &Value,
    receipt_ref: &ArtifactRef,
) -> Result<Value, String> {
    let result = result_from_payload(payload, receipt_ref);
    let status = result["status"].as_str().unwrap_or("REVISE");
    let message = if status == "PASS" {
        "开篇评估已完成，正式签约回执已提交。"
    } else {
        "开篇评估已完成，需要修订后再进入正式总纲。"
    };
    Ok(json!({
        "commandId":command_id,
        "projectId":project_id,
        "finalStatus":"committed",
        "message":message,
        "receiptId":receipt.receipt_id,
        "beforeRevision":receipt.before_revision,
        "afterRevision":receipt.after_revision,
        "result":result,
        "receipt":serde_json::to_value(receipt).map_err(|error| error.to_string())?
    }))
}

fn snapshot_payload(
    project_id: &str,
    now: &str,
    status: &str,
    verdict: &str,
    snapshot: &OpeningEvaluationSnapshot,
    source_refs: &[(String, ArtifactRef)],
) -> Result<Value, String> {
    let opening_chapter_refs = snapshot
        .chapters
        .iter()
        .map(|chapter| chapter.chapter_id.clone())
        .collect::<Vec<_>>();
    let final_chapter_refs = snapshot
        .chapters
        .iter()
        .filter_map(|chapter| chapter.final_ref.as_ref().map(ref_value))
        .collect::<Vec<_>>();
    let chapter_evidence = snapshot
        .chapters
        .iter()
        .map(|chapter| {
            json!({
                "chapterId":chapter.chapter_id,
                "chapterNo":chapter.chapter_no,
                "finalChapterRef":chapter.final_ref.as_ref().map(ref_value),
                "canonRefs":chapter.canon_refs,
                "evidenceRefs":chapter.evidence.iter().map(|item| item.evidence_id.clone()).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    let source_digest = sha256_canonical(&json!({
        "projectId":project_id,
        "openingChapterRefs":opening_chapter_refs,
        "finalChapterRefs":final_chapter_refs,
        "openingCanonRefs":snapshot.opening_canon_refs,
        "signingEvidenceRefs":snapshot.signing_evidence_refs,
        "sourceRefs":source_refs.iter().map(|(kind, reference)| json!({"kind":kind,"ref":ref_value(reference)})).collect::<Vec<_>>()
    }))
    .map_err(|error| error.to_string())?;
    let mut payload = json!({
        "payloadSchema":"simulated-signing-receipt.r1",
        "status":status,
        "verdict":verdict,
        "evaluatedAt":now,
        "openingChapterRefs":opening_chapter_refs,
        "openingCanonRefs":snapshot.opening_canon_refs,
        "signingEvidenceRefs":snapshot.signing_evidence_refs,
        "chapterEvidence":chapter_evidence,
        "sourceRefs":source_refs.iter().map(|(kind, reference)| (kind.clone(), ref_value(reference))).collect::<serde_json::Map<_,_>>(),
        "sourceDigest":source_digest,
        "evaluation":{"status":"COMPLETED","phase":"COMPLETED","steps":evaluation_steps(),"summary":"仅使用三章已提交正文、Canon 与证据进行结构化核验。"},
        "summary":{"chapters":snapshot.chapters.len(),"canonRefs":snapshot.opening_canon_refs.len(),"evidenceRefs":snapshot.signing_evidence_refs.len()},
        "receiptHash":""
    });
    let receipt_hash = sha256_canonical(&payload).map_err(|error| error.to_string())?;
    payload["receiptHash"] = json!(receipt_hash);
    Ok(payload)
}

/// Executes the one formal B02 production command.  `signing_retry` is kept as
/// a compatibility alias, but both names enter this same owner and commit
/// exactly the same artifact/lifecycle semantics.
pub fn execute_opening_evaluation(
    manager: &ProjectManager,
    request: &Value,
) -> Result<Value, String> {
    let project_id = required(request, "/projectId")?;
    let command_id = required(request, "/commandId")?;
    let idempotency_key = required(request, "/idempotencyKey")?;
    let now = required(request, "/requestedAt")?;
    let expected_revision = request.get("expectedRevision").and_then(Value::as_i64);
    let handle = manager
        .open(&project_id, "", &now)
        .map_err(|error| error.to_string())?;

    if let Some(previous) = CommitService::find_receipt(&handle, &project_id, &idempotency_key)
        .map_err(|error| error.to_string())?
    {
        let receipt_ref = previous
            .committed_artifact_refs
            .first()
            .cloned()
            .ok_or_else(|| "B02_SIGNING_RECEIPT_REF_MISSING".to_string())?;
        let payload =
            NarrativeRepository::artifact_payload_by_ref(&handle, &project_id, &receipt_ref)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "B02_SIGNING_RECEIPT_PAYLOAD_MISSING".to_string())?;
        return command_response(&command_id, &project_id, &previous, &payload, &receipt_ref);
    }

    let snapshot = NarrativeRepository::opening_evaluation_snapshot(&handle, &project_id)
        .map_err(|error| error.to_string())?;
    if snapshot.chapters.len() != 3
        || snapshot
            .chapters
            .iter()
            .enumerate()
            .any(|(index, chapter)| chapter.chapter_no != index as i64 + 1)
    {
        return Err("B02_OPENING_THREE_CHAPTERS_REQUIRED".into());
    }
    let source_kinds = [
        ("storyConstitutionRef", "StoryConstitution"),
        ("mainContractRef", "MainContract"),
        ("openingContractRef", "OpeningContract"),
        ("bookPlanSeedRef", "BookPlan"),
    ];
    let mut source_refs = Vec::new();
    for (kind, artifact_type) in source_kinds {
        let reference =
            NarrativeRepository::canonical_project_head_ref(&handle, &project_id, artifact_type)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("B02_OPENING_AUTHORITY_REQUIRED:{artifact_type}"))?;
        source_refs.push((kind.to_string(), reference));
    }
    let (status, verdict) = status_from_snapshot(&snapshot);
    let payload = snapshot_payload(&project_id, &now, status, verdict, &snapshot, &source_refs)?;
    let artifact_id = format!("signing-receipt:{project_id}");
    let version = NarrativeRepository::next_artifact_version(&handle, &project_id, &artifact_id)
        .map_err(|error| error.to_string())?;
    let payload_hash = sha256_canonical(&payload).map_err(|error| error.to_string())?;
    let receipt_ref = ArtifactRef {
        artifact_id: artifact_id.clone(),
        version,
        payload_hash: payload_hash.clone(),
    };
    let dependencies = dedupe_refs(
        source_refs
            .iter()
            .map(|(_, reference)| reference.clone())
            .chain(
                snapshot
                    .chapters
                    .iter()
                    .filter_map(|chapter| chapter.final_ref.clone()),
            ),
    );
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId":artifact_id,
        "projectId":project_id,
        "artifactType":"SimulatedSigningReceipt",
        "scope":{"level":"PROJECT"},
        "version":version,
        "schemaVersion":"m0a.artifact.v1",
        "artifactStatus":"CANDIDATE",
        "payload":payload,
        "payloadHash":payload_hash,
        "dependencyRefs":dependencies,
        "provenance":{"createdBy":"B02::OpeningEvaluationOwner","commandId":command_id},
        "createdAt":now
    }))
    .map_err(|error| format!("B02_SIGNING_ARTIFACT_INVALID:{error}"))?;
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":format!("commit:{command_id}"),
        "projectId":project_id,
        "commandId":command_id,
        "idempotencyKey":idempotency_key,
        "expectedProjectRevision":expected_revision,
        "contractVersion":"m0a.v1",
        "createdAt":now,
        "artifacts":[artifact],
        "headUpdates":[{"artifactType":"SimulatedSigningReceipt","scope":{"level":"PROJECT"},"artifactId":receipt_ref.artifact_id,"version":receipt_ref.version,"payloadHash":receipt_ref.payload_hash}],
        "receiptDraft":{"domain":"B02_OPENING_EVALUATION","artifactRefs":[receipt_ref.clone()],"payload":{"status":status,"openingChapterRefs":snapshot.chapters.iter().map(|chapter| chapter.chapter_id.clone()).collect::<Vec<_>>(),"openingCanonRefs":snapshot.opening_canon_refs,"signingEvidenceRefs":snapshot.signing_evidence_refs}}
    }))
    .map_err(|error| format!("B02_SIGNING_BUNDLE_INVALID:{error}"))?;
    let phase = "SIGNING";
    let run_state = if status == "PASS" { "IDLE" } else { "BLOCKED" };
    let block_reason = (status != "PASS").then_some("SIGNING_REQUIRED");
    let receipt = CommitService::commit_with_post_apply(&handle, &bundle, &now, |connection, _| {
        DurableJobService::set_signing_projection(
            connection,
            &project_id,
            phase,
            status,
            run_state,
            block_reason,
            &now,
        )
        .map_err(|error| {
            crate::commit::CommitError::ContractInvalid(format!("B02_RUNTIME_PROJECTION:{error}"))
        })
    })
    .map_err(|error| error.to_string())?;
    command_response(&command_id, &project_id, &receipt, &payload, &receipt_ref)
}
