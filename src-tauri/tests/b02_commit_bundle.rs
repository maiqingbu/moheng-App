use ai_novel_production_system_core::commit::{CommitError, CommitService};
use ai_novel_production_system_core::generated::types::{
    ArtifactEnvelope, ArtifactRef, ArtifactType, CommitBundle, DomainReceipt, GateCertificate,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::repositories::artifacts::ArtifactRepository;
use serde_json::{json, Value};
use tempfile::tempdir;

const NOW: &str = "2026-08-26T00:00:00Z";
const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const VOLUME_ID: &str = "vol:111111111111111111111111";

fn final_artifact(project_id: &str, command_id: &str) -> ArtifactEnvelope {
    let payload = json!({"title":"第一章","content":"最终正文"});
    serde_json::from_value(json!({
        "artifactId": "art:p1:chapter:1",
        "projectId": project_id,
        "artifactType": "FinalChapter",
        "scope": {"level":"CHAPTER","chapterId":"chapter-1"},
        "version": 1,
        "schemaVersion": "m0a.artifact.v1",
        "artifactStatus": "CANDIDATE",
        "payloadHash": sha256_canonical(&payload).unwrap(),
        "dependencyRefs": [],
        "provenance": {"createdBy":"b02-test","commandId":command_id},
        "payload": payload,
        "createdAt": NOW
    }))
    .unwrap()
}

fn gate_policy_artifact(project_id: &str, command_id: &str) -> ArtifactEnvelope {
    let payload = json!({"policy":"m0c-final-gate","version":1});
    serde_json::from_value(json!({
        "artifactId": "gate-policy:p1:final",
        "projectId": project_id,
        "artifactType": "GatePolicy",
        "scope": {"level":"PROJECT"},
        "version": 1,
        "schemaVersion": "m0a.artifact.v1",
        "artifactStatus": "CANDIDATE",
        "payloadHash": sha256_canonical(&payload).unwrap(),
        "dependencyRefs": [],
        "provenance": {"createdBy":"b02-test","commandId":command_id},
        "payload": payload,
        "createdAt": NOW
    }))
    .unwrap()
}

fn production_receipt_artifact(project_id: &str, command_id: &str) -> ArtifactEnvelope {
    let payload = json!({"chapterId":"chapter-1","receipt":"production"});
    serde_json::from_value(json!({
        "artifactId": "production-receipt:p1:chapter:1",
        "projectId": project_id,
        "artifactType": "ProductionReceipt",
        "scope": {"level":"CHAPTER","chapterId":"chapter-1"},
        "version": 1,
        "schemaVersion": "m0a.artifact.v1",
        "artifactStatus": "CANDIDATE",
        "payloadHash": sha256_canonical(&payload).unwrap(),
        "dependencyRefs": [],
        "provenance": {"createdBy":"b02-test","commandId":command_id},
        "payload": payload,
        "createdAt": NOW
    }))
    .unwrap()
}

fn volume_artifact(
    project_id: &str,
    command_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    payload: Value,
) -> ArtifactEnvelope {
    serde_json::from_value(json!({
        "artifactId": artifact_id,
        "projectId": project_id,
        "artifactType": artifact_type,
        "scope": {"level":"VOLUME","volumeId":VOLUME_ID},
        "version": 1,
        "schemaVersion": "m0a.artifact.v1",
        "artifactStatus": "CANDIDATE",
        "payloadHash": sha256_canonical(&payload).unwrap(),
        "dependencyRefs": [],
        "provenance": {"createdBy":"b02-test","commandId":command_id},
        "payload": payload,
        "createdAt": NOW
    }))
    .unwrap()
}

fn artifact_ref(artifact: &ArtifactEnvelope) -> Value {
    json!({
        "artifactId": artifact.artifact_id,
        "version": artifact.version,
        "payloadHash": artifact.payload_hash
    })
}

fn gate_certificate(
    policy: &ArtifactEnvelope,
    subject: &ArtifactEnvelope,
    certificate_id: &str,
) -> Value {
    let policy_ref = json!({
        "artifactId": policy.artifact_id,
        "version": policy.version,
        "payloadHash": policy.payload_hash
    });
    let subject_ref = artifact_ref(subject);
    let evidence_refs = json!([]);
    let source_digest = sha256_canonical(&json!({
        "gatePolicyRef": policy_ref,
        "subjectRef": subject_ref,
        "evidenceRefs": evidence_refs
    }))
    .unwrap();
    let mut certificate = json!({
        "certificateId": certificate_id,
        "projectId": "p1",
        "gatePolicyRef": policy_ref,
        "subjectRef": subject_ref,
        "evidenceRefs": [],
        "sourceDigest": source_digest,
        "passedAt": NOW,
        "certificateDigest": ""
    });
    certificate["certificateDigest"] = json!(sha256_canonical(&certificate).unwrap());
    certificate
}

fn refresh_certificate(certificate: &mut GateCertificate) {
    certificate.source_digest = sha256_canonical(&json!({
        "gatePolicyRef": certificate.gate_policy_ref,
        "subjectRef": certificate.subject_ref,
        "evidenceRefs": certificate.evidence_refs
    }))
    .unwrap();
    let mut value = serde_json::to_value(&*certificate).unwrap();
    value["certificateDigest"] = Value::String(String::new());
    certificate.certificate_digest = sha256_canonical(&value).unwrap();
}

fn volume_plan_bundle() -> CommitBundle {
    let command_id = "b02-volume-plan-command";
    let policy = gate_policy_artifact("p1", command_id);
    let plan = volume_artifact(
        "p1",
        command_id,
        "volume-plan:p1:1",
        "VolumePlan",
        json!({"volumeId":VOLUME_ID,"targetChapters":1,"state":"APPROVED_PLAN"}),
    );
    let contract = volume_artifact(
        "p1",
        command_id,
        "volume-contract:p1:1",
        "VolumeContract",
        json!({"volumeId":VOLUME_ID,"contract":"frozen"}),
    );
    let ledger = volume_artifact(
        "p1",
        command_id,
        "planning-ledger:p1:1",
        "PlanningStateLedger",
        json!({"volumeId":VOLUME_ID,"entryState":"OPEN","coverageState":"INCOMPLETE"}),
    );
    let production = volume_artifact(
        "p1",
        command_id,
        "production-receipt:p1:volume:1",
        "ProductionReceipt",
        json!({"volumeId":VOLUME_ID,"stage":"VOLUME_PLAN"}),
    );
    let mut bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":"commit:b02:volume-plan",
        "projectId":"p1",
        "commandId":command_id,
        "idempotencyKey":"b02-volume-plan-key",
        "expectedProjectRevision":0,
        "contractVersion":"m0a.v1",
        "createdAt":NOW,
        "artifacts":[plan,contract,ledger,production],
        "headUpdates":[
          {"artifactType":"VolumePlan","scope":{"level":"VOLUME","volumeId":VOLUME_ID},"artifactId":"volume-plan:p1:1","version":1,"payloadHash":HASH},
          {"artifactType":"VolumeContract","scope":{"level":"VOLUME","volumeId":VOLUME_ID},"artifactId":"volume-contract:p1:1","version":1,"payloadHash":HASH},
          {"artifactType":"PlanningStateLedger","scope":{"level":"VOLUME","volumeId":VOLUME_ID},"artifactId":"planning-ledger:p1:1","version":1,"payloadHash":HASH}
        ],
        "stateDelta":[{"stateId":"volume-state:p1:1","stateType":"volume.planning","valueJson":{"entryState":"OPEN"},"payloadJson":{"coverageState":"INCOMPLETE"}}],
        "dependencyEdges":[{"edgeId":"volume-edge:p1:1","sourceRef":"volume-plan:p1:1","targetRef":VOLUME_ID,"dependencyType":"HARD_CONTRACT","scope":"VOLUME","impactClass":"HARD","sourceHash":HASH}],
        "gateCertificates":[gate_certificate(&policy,&plan,"gate:p1:volume-plan")],
        "receiptDraft":{"domain":"volume-plan","artifactRefs":[],"payload":{"scope":"VOLUME"}}
    })).unwrap();
    let plan_hash = bundle.artifacts[0].payload_hash.clone();
    for head in bundle.head_updates.as_mut().unwrap() {
        head.payload_hash = bundle
            .artifacts
            .iter()
            .find(|artifact| artifact.artifact_id == head.artifact_id)
            .unwrap()
            .payload_hash
            .clone();
    }
    bundle.dependency_edges.as_mut().unwrap()[0].source_hash = plan_hash;
    bundle.receipt_draft.as_mut().unwrap().artifact_refs = Some(
        bundle
            .artifacts
            .iter()
            .map(artifact_ref)
            .map(|value| serde_json::from_value(value).unwrap())
            .collect(),
    );
    bundle
}

fn volume_close_bundle(plan_hash: &str, policy: &ArtifactEnvelope) -> CommitBundle {
    let command_id = "b02-volume-close-command";
    let exit_state = volume_artifact(
        "p1",
        command_id,
        "volume-exit:p1:1",
        "VolumeExitState",
        json!({"volumeId":VOLUME_ID,"state":"CLOSED","eligibleForNextVolume":true}),
    );
    let health = volume_artifact(
        "p1",
        command_id,
        "volume-health:p1:1",
        "VolumeHealthReport",
        json!({"volumeId":VOLUME_ID,"status":"HEALTHY","eligibleForNextVolume":true}),
    );
    let production = volume_artifact(
        "p1",
        command_id,
        "production-receipt:p1:volume-close:1",
        "ProductionReceipt",
        json!({"volumeId":VOLUME_ID,"stage":"VOLUME_CLOSE"}),
    );
    let mut bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":"commit:b02:volume-close",
        "projectId":"p1",
        "commandId":command_id,
        "idempotencyKey":"b02-volume-close-key",
        "expectedProjectRevision":1,
        "contractVersion":"m0a.v1",
        "createdAt":NOW,
        "artifacts":[exit_state,health,production],
        "headUpdates":[
          {"artifactType":"VolumeExitState","scope":{"level":"VOLUME","volumeId":VOLUME_ID},"artifactId":"volume-exit:p1:1","version":1,"payloadHash":serde_json::to_value(volume_artifact("p1", command_id, "volume-exit:p1:1", "VolumeExitState", json!({"volumeId":VOLUME_ID,"state":"CLOSED","eligibleForNextVolume":true}))).unwrap()["payloadHash"]},
          {"artifactType":"VolumeHealthReport","scope":{"level":"VOLUME","volumeId":VOLUME_ID},"artifactId":"volume-health:p1:1","version":1,"payloadHash":serde_json::to_value(volume_artifact("p1", command_id, "volume-health:p1:1", "VolumeHealthReport", json!({"volumeId":VOLUME_ID,"status":"HEALTHY","eligibleForNextVolume":true}))).unwrap()["payloadHash"]}
        ],
        "stateDelta":[{"stateId":"volume-close-state:p1:1","stateType":"volume.close","valueJson":{"ledgerState":"CLOSED"},"payloadJson":{"eligibleForNextVolume":true}}],
        "dependencyEdges":[{"edgeId":"volume-close-edge:p1:1","sourceRef":"volume-plan:p1:1","targetRef":"volume-exit:p1:1","dependencyType":"HARD_CONTRACT","scope":"VOLUME","impactClass":"HARD","sourceHash":plan_hash}],
        "gateCertificates":[gate_certificate(policy,&exit_state,"gate:p1:volume-close")],
        "receiptDraft":{"domain":"volume-close","artifactRefs":[],"payload":{"scope":"VOLUME"}}
    })).unwrap();
    bundle.receipt_draft.as_mut().unwrap().artifact_refs = Some(
        bundle
            .artifacts
            .iter()
            .map(artifact_ref)
            .map(|value| serde_json::from_value(value).unwrap())
            .collect(),
    );
    bundle
}

fn full_bundle() -> CommitBundle {
    let command_id = "b02-full-delta-command";
    let mut bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":"commit:b02:full",
        "projectId":"p1",
        "commandId":command_id,
        "idempotencyKey":"b02-full-delta-key",
        "expectedProjectRevision":0,
        "contractVersion":"m0a.v1",
        "createdAt":NOW,
        "artifacts":[
            serde_json::to_value(final_artifact("p1", command_id)).unwrap(),
            serde_json::to_value(production_receipt_artifact("p1", command_id)).unwrap()
        ],
        "headUpdates":[{"artifactType":"FinalChapter","scope":{"level":"CHAPTER","chapterId":"chapter-1"},"artifactId":"art:p1:chapter:1","version":1,"payloadHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}],
        "canonDelta":[{"entityId":"ent:p1:root:1","factId":"fact:p1:born:1","subjectId":"ent:p1:root:1","predicate":"cast","objectJson":{"name":"王五"},"authorityLevel":"C","eventType":"character_introduction","payloadJson":{"sourceArtifactId":"art:p1:chapter:1","sourceVersion":1,"chapterId":"chapter-1"}}],
        "obligationDelta":[{"obligationId":"obl:p1:1","scope":"VOLUME","obligationType":"payoff","authorityLevel":"B","lifecycle":"APPROVED","description":"pay off the planted mystery","payloadJson":{"sourceArtifactId":"art:p1:chapter:1","sourceVersion":1,"chapterId":"chapter-1"}}],
        "foreshadowDelta":[{"foreshadowId":"fs:p1:1","intent":"the same knife returns","status":"PLANTED","plantChapterId":"chapter-1","payloadJson":{"sourceAuthority":"C"}}],
        "knowledgeDelta":[{"knowledgeId":"kn:p1:1","knowerId":"ent:p1:root:1","factId":"fact:p1:born:1","epistemicState":"KNOWS","fromChapterId":"chapter-1","payloadJson":{"certainty":1}}],
        "stateDelta":[{"stateId":"st:p1:1","stateType":"mood","valueJson":{"valence":"grim"},"chapterId":"chapter-1","payloadJson":{"currentPhase":"OPENING"}}],
        "dependencyEdges":[{"edgeId":"edge:p1:1","sourceRef":"art:p1:chapter:1","targetRef":"chapter-1","dependencyType":"EVIDENCE","scope":"CHAPTER","impactClass":"HARD","sourceHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","payloadJson":{"createdBy":"b02-test"}}],
        "jobUpdates":[{"jobId":"job:p1:1","status":"COMPLETED","currentStage":"epilogue","attemptNo":1,"payloadJson":{"jobType":"CHAPTER"}}],
        "decisionUpdates":[{"decisionId":"dec:p1:1","decisionType":"cast","outcome":"approve","reason":"retains canon","sourceRef":"job:p1:1","payloadJson":{}}],
        "readerDesireDelta":[{"desireId":"desire:p1:1","desireType":"mystery","status":"OPEN","expectedPayoffChapter":"chapter-1","createdChapterId":"chapter-1","lastTouchedChapterId":"chapter-1","payloadJson":{"intensity":1}}],
        "gateCertificates":[{"certificateId":"gate:p1:1","projectId":"p1","gatePolicyRef":{"artifactId":"gate-policy:p1:final","version":1,"payloadHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"subjectRef":{"artifactId":"art:p1:chapter:1","version":1,"payloadHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"evidenceRefs":[],"sourceDigest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","passedAt":NOW,"certificateDigest":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}],
        "receiptDraft":{"domain":"chapter-production","receiptId":"domain-receipt:p1:chapter:1","artifactRefs":[],"payload":{"scope":"CHAPTER"}}
    })).unwrap();
    let policy_hash = gate_policy_artifact("p1", "b02-gate-policy-seed").payload_hash;
    let final_hash = bundle.artifacts[0].payload_hash.clone();
    bundle.head_updates.as_mut().unwrap()[0].payload_hash = final_hash.clone();
    bundle.dependency_edges.as_mut().unwrap()[0].source_hash = final_hash.clone();
    let certificates = bundle.gate_certificates.as_mut().unwrap();
    certificates[0].gate_policy_ref.payload_hash = policy_hash.clone();
    certificates[0].subject_ref = ArtifactRef {
        artifact_id: bundle.artifacts[0].artifact_id.clone(),
        version: bundle.artifacts[0].version,
        payload_hash: final_hash.clone(),
    };
    certificates[0].source_digest = sha256_canonical(&json!({
        "gatePolicyRef": certificates[0].gate_policy_ref,
        "subjectRef": certificates[0].subject_ref,
        "evidenceRefs": certificates[0].evidence_refs
    }))
    .unwrap();
    bundle.receipt_draft.as_mut().unwrap().artifact_refs = Some(
        bundle
            .artifacts
            .iter()
            .map(artifact_ref)
            .map(|value| serde_json::from_value(value).unwrap())
            .collect(),
    );
    let mut certificate_value = serde_json::to_value(&certificates[0]).unwrap();
    certificate_value["certificateDigest"] = Value::String(String::new());
    certificates[0].certificate_digest = sha256_canonical(&certificate_value).unwrap();
    bundle
}

fn seed_story_prerequisites(connection: &libsql_rusqlite::Connection) {
    connection.execute("INSERT INTO books(book_id, project_id, book_no, title, status, created_at, updated_at) VALUES ('book:p1:1','p1',1,'Book','ACTIVE',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id, project_id, book_id, volume_no, title, status, chapter_start, chapter_end, target_words, created_at, updated_at) VALUES (?1,'p1','book:p1:1',1,'Volume','PLANNING',1,1,1000,?2,?2)", [VOLUME_ID, NOW]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id, project_id, volume_id, chapter_no, title, importance_class, status, word_count, created_at, updated_at) VALUES ('chapter-1','p1',?1,1,'Chapter','NORMAL','PLANNED',0,?2,?2)", [VOLUME_ID, NOW]).unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id, project_id, canonical_name, entity_type, created_at, updated_at) VALUES ('ent:p1:root:1','p1','王五','CHARACTER',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO jobs(job_id, project_id, job_type, chapter_id, status, created_at, updated_at) VALUES ('job:p1:1','p1','CHAPTER','chapter-1','RUNNING',?1,?1)", [NOW]).unwrap();
}

fn seed_prerequisites(connection: &libsql_rusqlite::Connection) {
    seed_story_prerequisites(connection);
    let policy = gate_policy_artifact("p1", "b02-gate-policy-seed");
    ArtifactRepository::insert_candidate(connection, "p1", &policy).unwrap();
    connection
        .execute(
            "UPDATE artifacts SET artifact_status = 'COMMITTED', committed_at = ?1 WHERE artifact_id = ?2 AND version = ?3",
            libsql_rusqlite::params![NOW, policy.artifact_id, policy.version],
        )
        .unwrap();
    let (scope_level, scope_key, payload_hash): (String, String, String) = connection
        .query_row(
            "SELECT scope_level, scope_key, payload_hash FROM artifacts WHERE artifact_id = ?1 AND version = ?2",
            libsql_rusqlite::params![policy.artifact_id, policy.version],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO canonical_heads(project_id, artifact_type, scope_level, scope_key, artifact_id, version, payload_hash) VALUES ('p1', 'GatePolicy', ?1, ?2, ?3, ?4, ?5)",
            libsql_rusqlite::params![scope_level, scope_key, policy.artifact_id, policy.version, payload_hash],
        )
        .unwrap();
}

fn seed_over_limit_budget(connection: &libsql_rusqlite::Connection) {
    connection
        .execute(
            "INSERT INTO chapter_production_budgets(budget_id, project_id, chapter_id, importance_class, hard_max_model_runs, frozen_at) VALUES ('budget:p1:1', 'p1', 'chapter-1', 'NORMAL', 1, ?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO budget_reservations(reservation_id, project_id, budget_ref, reserved_cents, status, created_at) VALUES ('reservation:p1:1', 'p1', 'budget:p1:1', 1, 'OVER_LIMIT', ?1)",
            [NOW],
        )
        .unwrap();
}

#[test]
fn b02_full_delta_bundle_is_not_rejected_as_unsupported() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let receipt = CommitService::commit(&connection, &full_bundle(), NOW).unwrap();
    assert_eq!(receipt.after_revision, 1);
    assert_eq!(receipt.applied_delta_counts["canon"], 2);
    for channel in [
        "obligation",
        "foreshadow",
        "knowledge",
        "state",
        "dependency",
        "job",
        "decision",
        "readerDesire",
    ] {
        let expected = if channel == "job" { 0 } else { 1 };
        assert_eq!(
            receipt.applied_delta_counts[channel], expected,
            "channel {channel}"
        );
    }
    assert_eq!(receipt.gate_certificate_refs, vec!["gate:p1:1"]);
    assert_eq!(receipt.applied_delta_counts["gateCertificates"], 1);
    assert_eq!(
        receipt.bundle_digest,
        sha256_canonical(&serde_json::to_value(full_bundle()).unwrap()).unwrap()
    );
    assert!(
        CommitService::find_receipt(&connection, "p1", "b02-full-delta-key")
            .unwrap()
            .is_some()
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM canon_facts", [], |row| row.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM decisions", [], |row| row.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM gate_certificates", [], |row| row
                .get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM domain_receipts", [], |row| row.get(0))
            .unwrap(),
        1
    );
    let domain_json: String = connection
        .query_row(
            "SELECT receipt_json FROM domain_receipts WHERE receipt_id = 'domain-receipt:p1:chapter:1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let domain_receipt: DomainReceipt = serde_json::from_str(&domain_json).unwrap();
    assert_eq!(domain_receipt.artifact_refs.len(), 2);
    assert_eq!(domain_receipt.payload["scope"], "CHAPTER");
    let mut unsigned = serde_json::to_value(&domain_receipt).unwrap();
    unsigned["receiptDigest"] = Value::String(String::new());
    assert_eq!(
        domain_receipt.receipt_digest,
        sha256_canonical(&unsigned).unwrap()
    );
}

#[test]
fn b02_invalid_delta_still_fails_closed_without_revision_or_rows() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let mut bundle = full_bundle();
    bundle.dependency_edges.as_mut().unwrap()[0].source_hash = "not-a-sha".into();
    assert!(CommitService::commit(&connection, &bundle, NOW).is_err());
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT revision FROM project_meta", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM commit_receipts", [], |row| row.get(0))
            .unwrap(),
        0
    );
}

#[test]
fn b02_same_bundle_replays_100_times_with_one_business_side_effect() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let bundle = full_bundle();
    let first = CommitService::commit(&connection, &bundle, NOW).unwrap();
    for _ in 0..100 {
        assert_eq!(
            CommitService::commit(&connection, &bundle, NOW).unwrap(),
            first
        );
    }
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM commit_receipts", [], |row| row.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM commands", [], |row| row.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM canon_facts", [], |row| row.get(0))
            .unwrap(),
        1
    );
    assert!(
        CommitService::find_receipt(&connection, "p1", "b02-full-delta-key")
            .unwrap()
            .is_some()
    );
}

#[test]
fn b02_each_delta_channel_failure_rolls_back_the_entire_bundle() {
    let channels = [
        "canon",
        "obligation",
        "foreshadow",
        "knowledge",
        "state",
        "dependency",
        "job",
        "decision",
        "readerDesire",
        "gate",
    ];
    for channel in channels {
        let temp = tempdir().unwrap();
        let manager = ProjectManager::new(AppPaths::new(temp.path()));
        let connection = manager.create("p1", "A", NOW).unwrap();
        seed_prerequisites(&connection);
        let mut bundle = full_bundle();
        match channel {
            "canon" => {
                bundle.canon_delta.as_mut().unwrap()[0].subject_id = Some("missing-entity".into())
            }
            "obligation" => {
                bundle.obligation_delta.as_mut().unwrap()[0]
                    .payload_json
                    .as_mut()
                    .unwrap()
                    .insert(
                        "sourceArtifactId".into(),
                        Value::String("missing-artifact".into()),
                    );
            }
            "foreshadow" => {
                bundle.foreshadow_delta.as_mut().unwrap()[0].status = Some("INVALID".into())
            }
            "knowledge" => {
                bundle.knowledge_delta.as_mut().unwrap()[0].knower_id = "missing-entity".into()
            }
            "state" => bundle.state_delta.as_mut().unwrap()[0].state_type = String::new(),
            "dependency" => {
                bundle.dependency_edges.as_mut().unwrap()[0].source_hash = "invalid".into()
            }
            "job" => bundle.job_updates.as_mut().unwrap()[0].status = Some("INVALID".into()),
            "decision" => {
                bundle.decision_updates.as_mut().unwrap()[0].decision_type = String::new()
            }
            "readerDesire" => {
                bundle.reader_desire_delta.as_mut().unwrap()[0].desire_type = String::new()
            }
            "gate" => {
                bundle.gate_certificates.as_mut().unwrap()[0].certificate_digest =
                    "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".into()
            }
            _ => unreachable!(),
        }
        assert!(
            CommitService::commit(&connection, &bundle, NOW).is_err(),
            "channel {channel}"
        );
        println!("fault_channel={channel} rollback=PASS");
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT revision FROM project_meta", [], |row| row.get(0))
                .unwrap(),
            0,
            "revision {channel}"
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
                .unwrap(),
            1,
            "artifacts {channel}"
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM commit_receipts", [], |row| row
                    .get(0))
                .unwrap(),
            0,
            "receipts {channel}"
        );
    }
}

#[test]
fn b02_final_gate_rejects_wrong_project_and_budget_hard_limit_before_writes() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let mut wrong_project = full_bundle();
    wrong_project.gate_certificates.as_mut().unwrap()[0].project_id = "p2".into();
    assert!(matches!(
        CommitService::commit(&connection, &wrong_project, NOW),
        Err(CommitError::ContractInvalid(_))
    ));

    seed_over_limit_budget(&connection);
    let mut over_budget = full_bundle();
    over_budget.state_delta.as_mut().unwrap()[0]
        .payload_json
        .as_mut()
        .unwrap()
        .insert("hardLimitExceeded".into(), Value::Bool(true));
    assert!(matches!(
        CommitService::commit(&connection, &over_budget, NOW),
        Err(CommitError::BudgetHardLimit)
    ));
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT revision FROM project_meta", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM commit_receipts", [], |row| row.get(0))
            .unwrap(),
        0
    );
}

#[test]
fn b02_delta_repository_is_transaction_scoped_and_final_gate_is_commit_owned() {
    let deltas = include_str!("../src/commit/deltas.rs");
    let service = include_str!("../src/commit/mod.rs");
    assert!(deltas.contains("pub(crate) fn apply_all"));
    assert!(!deltas.contains("BEGIN IMMEDIATE"));
    assert!(service.contains("final_gate::validate"));
    assert!(!service.contains("has_reserved_delta"));
}

#[test]
fn b02_revision_conflict_performs_no_delta_writes() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let mut bundle = full_bundle();
    bundle.expected_project_revision = Some(1);
    assert!(matches!(
        CommitService::commit(&connection, &bundle, NOW),
        Err(CommitError::RevisionConflict)
    ));
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT revision FROM project_meta", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM commit_receipts", [], |row| row.get(0))
            .unwrap(),
        0
    );
}

#[test]
fn b02_final_gate_revalidates_gate_policy_type_and_dependency_source_hash() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);

    let mut bundle = full_bundle();
    bundle.gate_certificates.as_mut().unwrap()[0]
        .gate_policy_ref
        .artifact_id = "art:p1:chapter:1".into();
    bundle.gate_certificates.as_mut().unwrap()[0]
        .gate_policy_ref
        .payload_hash = bundle.artifacts[0].payload_hash.clone();
    assert!(matches!(
        CommitService::commit(&connection, &bundle, NOW),
        Err(CommitError::ContractInvalid(_))
    ));

    let mut dependency_bundle = full_bundle();
    dependency_bundle.dependency_edges.as_mut().unwrap()[0].source_ref = "fact:p1:existing".into();
    dependency_bundle.dependency_edges.as_mut().unwrap()[0].source_hash = HASH.into();
    connection
        .execute(
            "INSERT INTO canon_facts(fact_id, project_id, subject_id, predicate, object_json, authority_level, created_at, updated_at) VALUES ('fact:p1:existing', 'p1', 'ent:p1:root:1', 'status', '{}', 'C', ?1, ?1)",
            [NOW],
        )
        .unwrap();
    assert!(matches!(
        CommitService::commit(&connection, &dependency_bundle, NOW),
        Err(CommitError::ContractInvalid(_))
    ));
}

#[test]
fn b02_gate_policy_must_be_committed_before_the_final_commit() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_story_prerequisites(&connection);
    let policy = gate_policy_artifact("p1", "b02-gate-policy-command");
    let policy_hash = policy.payload_hash.clone();
    let policy_bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":"commit:b02:gate-policy",
        "projectId":"p1",
        "commandId":"b02-gate-policy-command",
        "idempotencyKey":"b02-gate-policy-key",
        "expectedProjectRevision":0,
        "contractVersion":"m0a.v1",
        "createdAt":NOW,
        "artifacts":[policy],
        "headUpdates":[{"artifactType":"GatePolicy","scope":{"level":"PROJECT"},"artifactId":"gate-policy:p1:final","version":1,"payloadHash":policy_hash}]
    }))
    .unwrap();
    let policy_receipt = CommitService::commit(&connection, &policy_bundle, NOW).unwrap();
    assert_eq!(policy_receipt.after_revision, 1);

    let mut final_bundle = full_bundle();
    final_bundle.expected_project_revision = Some(1);
    let final_receipt = CommitService::commit(&connection, &final_bundle, NOW).unwrap();
    assert_eq!(final_receipt.after_revision, 2);
}

#[test]
fn b02_same_bundle_or_uncommitted_gate_policy_is_rejected_before_writes() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_story_prerequisites(&connection);
    let mut bundle = full_bundle();
    bundle
        .artifacts
        .insert(0, gate_policy_artifact("p1", "b02-full-delta-command"));
    assert!(matches!(
        CommitService::commit(&connection, &bundle, NOW),
        Err(CommitError::ContractInvalid(_))
    ));
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT revision FROM project_meta", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
            .unwrap(),
        0
    );
}

#[test]
fn b02_final_gate_checks_knowledge_refs_and_authoritative_budget_state() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);

    let mut orphan_knowledge = full_bundle();
    orphan_knowledge.knowledge_delta.as_mut().unwrap()[0].fact_id = "fact:missing".into();
    assert!(matches!(
        CommitService::commit(&connection, &orphan_knowledge, NOW),
        Err(CommitError::ContractInvalid(_))
    ));

    seed_over_limit_budget(&connection);
    let mut over_budget = full_bundle();
    over_budget.state_delta.as_mut().unwrap()[0]
        .payload_json
        .as_mut()
        .unwrap()
        .remove("hardLimitExceeded");
    assert!(matches!(
        CommitService::commit(&connection, &over_budget, NOW),
        Err(CommitError::BudgetHardLimit)
    ));
}

#[test]
fn b02_candidate_cannot_become_a_canonical_head() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let mut bundle = full_bundle();
    bundle.artifacts[0].artifact_type = ArtifactType::ChapterCandidate;
    bundle.head_updates = Some(vec![serde_json::from_value(json!({
        "artifactType": "ChapterCandidate",
        "scope": {"level":"CHAPTER","chapterId":"chapter-1"},
        "artifactId": "art:p1:chapter:1",
        "version": 1,
        "payloadHash": bundle.artifacts[0].payload_hash
    }))
    .unwrap()]);
    assert!(matches!(
        CommitService::commit(&connection, &bundle, NOW),
        Err(CommitError::Artifact(_))
    ));
}

#[test]
fn b02_receipt_counts_are_net_rows_and_head_changes_are_read_back() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let before_entities: i64 = connection
        .query_row("SELECT COUNT(*) FROM canon_entities", [], |row| row.get(0))
        .unwrap();
    let before_facts: i64 = connection
        .query_row("SELECT COUNT(*) FROM canon_facts", [], |row| row.get(0))
        .unwrap();
    let before_events: i64 = connection
        .query_row("SELECT COUNT(*) FROM canon_events", [], |row| row.get(0))
        .unwrap();

    let mut bundle = full_bundle();
    bundle.head_updates = Some(vec![serde_json::from_value(json!({
        "artifactType": "FinalChapter",
        "scope": {"level":"CHAPTER","chapterId":"chapter-1"},
        "artifactId": "art:p1:chapter:1",
        "version": 1,
        "payloadHash": bundle.artifacts[0].payload_hash
    }))
    .unwrap()]);
    let receipt = CommitService::commit(&connection, &bundle, NOW).unwrap();
    let after_entities: i64 = connection
        .query_row("SELECT COUNT(*) FROM canon_entities", [], |row| row.get(0))
        .unwrap();
    let after_facts: i64 = connection
        .query_row("SELECT COUNT(*) FROM canon_facts", [], |row| row.get(0))
        .unwrap();
    let after_events: i64 = connection
        .query_row("SELECT COUNT(*) FROM canon_events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        receipt.applied_delta_counts["canon"],
        (after_entities - before_entities)
            + (after_facts - before_facts)
            + (after_events - before_events)
    );
    assert_eq!(receipt.head_changes[0].artifact_id, "art:p1:chapter:1");
    assert_eq!(
        receipt.head_changes[0].payload_hash,
        bundle.artifacts[0].payload_hash
    );
}

#[test]
fn b02_volume_plan_submission_is_atomic_and_receipted() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let receipt = CommitService::commit(&connection, &volume_plan_bundle(), NOW).unwrap();
    assert_eq!(receipt.after_revision, 1);
    assert_eq!(receipt.applied_delta_counts["artifacts"], 4);
    assert_eq!(receipt.applied_delta_counts["heads"], 3);
    assert_eq!(receipt.applied_delta_counts["state"], 1);
    assert_eq!(receipt.applied_delta_counts["dependency"], 1);
    assert_eq!(receipt.applied_delta_counts["gateCertificates"], 1);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM canonical_heads WHERE project_id = 'p1' AND scope_key LIKE '%111111111111111111111111%'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        3
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM state_deltas", [], |row| row.get(0))
            .unwrap(),
        1
    );
}

#[test]
fn b02_volume_close_records_exit_health_eligibility_and_receipt() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let plan = volume_plan_bundle();
    let plan_hash = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.artifact_id == "volume-plan:p1:1")
        .unwrap()
        .payload_hash
        .clone();
    let policy = gate_policy_artifact("p1", "b02-gate-policy-seed");
    CommitService::commit(&connection, &plan, NOW).unwrap();
    let receipt =
        CommitService::commit(&connection, &volume_close_bundle(&plan_hash, &policy), NOW).unwrap();
    assert_eq!(receipt.before_revision, 1);
    assert_eq!(receipt.after_revision, 2);
    assert_eq!(receipt.applied_delta_counts["artifacts"], 3);
    assert_eq!(receipt.applied_delta_counts["heads"], 2);
    assert_eq!(receipt.applied_delta_counts["state"], 1);
    assert_eq!(receipt.applied_delta_counts["dependency"], 1);
    assert!(receipt
        .head_changes
        .iter()
        .any(|head| head.artifact_type == ArtifactType::VolumeExitState));
    assert!(receipt
        .head_changes
        .iter()
        .any(|head| head.artifact_type == ArtifactType::VolumeHealthReport));
    let eligibility: String = connection
        .query_row(
            "SELECT payload_json FROM state_deltas WHERE state_id = 'volume-close-state:p1:1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&eligibility).unwrap()["eligibleForNextVolume"],
        true
    );
}

#[test]
fn b02_gate_certificate_same_id_with_different_digest_is_not_rewritten() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    CommitService::commit(&connection, &full_bundle(), NOW).unwrap();
    let original_digest: String = connection
        .query_row(
            "SELECT certificate_digest FROM gate_certificates WHERE certificate_id = 'gate:p1:1'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let mut conflict = full_bundle();
    conflict.command_id = "b02-certificate-conflict-command".into();
    conflict.idempotency_key = "b02-certificate-conflict-key".into();
    conflict.expected_project_revision = Some(1);
    for artifact in &mut conflict.artifacts {
        artifact.provenance.command_id = conflict.command_id.clone();
    }
    conflict.gate_certificates.as_mut().unwrap()[0].passed_at = "2026-08-26T00:00:01Z".into();
    let certificate = &mut conflict.gate_certificates.as_mut().unwrap()[0];
    let mut value = serde_json::to_value(&*certificate).unwrap();
    value["certificateDigest"] = Value::String(String::new());
    certificate.certificate_digest = sha256_canonical(&value).unwrap();
    assert!(matches!(
        CommitService::commit(&connection, &conflict, NOW),
        Err(CommitError::ContractInvalid(_))
    ));
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT revision FROM project_meta", [], |row| row.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT certificate_digest FROM gate_certificates WHERE certificate_id = 'gate:p1:1'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        original_digest
    );
}

#[test]
fn b02_volume_plan_failure_after_artifacts_rolls_back_every_write() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let mut bundle = volume_plan_bundle();
    bundle.state_delta.as_mut().unwrap()[0].state_type = String::new();
    assert!(CommitService::commit(&connection, &bundle, NOW).is_err());
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT revision FROM project_meta", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM artifacts WHERE artifact_id IN ('volume-plan:p1:1','volume-contract:p1:1','planning-ledger:p1:1','production-receipt:p1:volume:1')", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM canonical_heads WHERE artifact_type IN ('VolumePlan','VolumeContract','PlanningStateLedger')", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM dependency_edges", [], |row| row
                .get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM state_deltas", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM domain_receipts", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM commit_receipts", [], |row| row.get(0))
            .unwrap(),
        0
    );
}

#[test]
fn b02_volume_close_failure_after_artifacts_preserves_previous_volume_state() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let plan = volume_plan_bundle();
    let plan_hash = plan
        .artifacts
        .iter()
        .find(|artifact| artifact.artifact_id == "volume-plan:p1:1")
        .unwrap()
        .payload_hash
        .clone();
    let policy = gate_policy_artifact("p1", "b02-gate-policy-seed");
    CommitService::commit(&connection, &plan, NOW).unwrap();
    let mut close = volume_close_bundle(&plan_hash, &policy);
    close.state_delta.as_mut().unwrap()[0].state_type = String::new();
    assert!(CommitService::commit(&connection, &close, NOW).is_err());
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT revision FROM project_meta", [], |row| row.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM volumes WHERE volume_id = ?1",
                [VOLUME_ID],
                |row| row.get(0)
            )
            .unwrap(),
        "PLANNING"
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM artifacts WHERE artifact_id IN ('volume-exit:p1:1','volume-health:p1:1','production-receipt:p1:volume-close:1')", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM canonical_heads WHERE artifact_type IN ('VolumeExitState','VolumeHealthReport')", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM dependency_edges", [], |row| row
                .get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM state_deltas", [], |row| row.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM domain_receipts", [], |row| row.get(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM commit_receipts", [], |row| row.get(0))
            .unwrap(),
        1
    );
}

#[test]
fn b02_certificate_subject_rejects_reuse_stale_hash_and_wrong_subject_type() {
    let cases = ["reused-final", "stale-hash", "candidate-subject"];
    for case in cases {
        let temp = tempdir().unwrap();
        let manager = ProjectManager::new(AppPaths::new(temp.path()));
        let connection = manager.create("p1", "A", NOW).unwrap();
        seed_prerequisites(&connection);
        CommitService::commit(&connection, &full_bundle(), NOW).unwrap();
        let mut conflict = full_bundle();
        conflict.command_id = format!("b02-subject-{case}");
        conflict.idempotency_key = format!("b02-subject-{case}-key");
        conflict.expected_project_revision = Some(1);
        for artifact in &mut conflict.artifacts {
            artifact.provenance.command_id = conflict.command_id.clone();
        }
        let certificate = &mut conflict.gate_certificates.as_mut().unwrap()[0];
        match case {
            "reused-final" => {
                certificate.subject_ref = ArtifactRef {
                    artifact_id: "production-receipt:p1:chapter:1".into(),
                    version: 1,
                    payload_hash: conflict.artifacts[1].payload_hash.clone(),
                };
            }
            "stale-hash" => certificate.subject_ref.payload_hash = "e".repeat(64),
            "candidate-subject" => {
                conflict.artifacts[1].artifact_type = ArtifactType::ChapterCandidate;
                certificate.subject_ref = ArtifactRef {
                    artifact_id: conflict.artifacts[1].artifact_id.clone(),
                    version: conflict.artifacts[1].version,
                    payload_hash: conflict.artifacts[1].payload_hash.clone(),
                };
            }
            _ => unreachable!(),
        }
        refresh_certificate(certificate);
        assert!(
            CommitService::commit(&connection, &conflict, NOW).is_err(),
            "subject case {case}"
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT revision FROM project_meta", [], |row| row.get(0))
                .unwrap(),
            1,
            "revision {case}"
        );
        assert_eq!(
            connection
                .query_row::<i64, _, _>("SELECT COUNT(*) FROM gate_certificates", [], |row| row
                    .get(0))
                .unwrap(),
            1,
            "certificate {case}"
        );
    }
}

#[test]
fn b02_receipt_draft_rejects_wrong_hash_and_payload_mutation() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let mut wrong_hash = full_bundle();
    wrong_hash
        .receipt_draft
        .as_mut()
        .unwrap()
        .artifact_refs
        .as_mut()
        .unwrap()[0]
        .payload_hash = "f".repeat(64);
    assert!(matches!(
        CommitService::commit(&connection, &wrong_hash, NOW),
        Err(CommitError::ContractInvalid(_))
    ));
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM artifacts WHERE artifact_id = 'art:p1:chapter:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );

    CommitService::commit(&connection, &full_bundle(), NOW).unwrap();
    let mut mutated = full_bundle();
    mutated.command_id = "b02-receipt-payload-conflict".into();
    mutated.idempotency_key = "b02-receipt-payload-conflict-key".into();
    mutated.expected_project_revision = Some(1);
    for artifact in &mut mutated.artifacts {
        artifact.provenance.command_id = mutated.command_id.clone();
    }
    mutated
        .receipt_draft
        .as_mut()
        .unwrap()
        .payload
        .as_mut()
        .unwrap()
        .insert("scope".into(), Value::String("VOLUME".into()));
    assert!(matches!(
        CommitService::commit(&connection, &mutated, NOW),
        Err(CommitError::ContractInvalid(_))
    ));
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT revision FROM project_meta", [], |row| row.get(0))
            .unwrap(),
        1
    );
}

#[test]
fn fx1_obligation_delta_persists_source_event_id_and_preserves_it_on_update() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);

    let mut initial = full_bundle();
    initial.obligation_delta.as_mut().unwrap()[0]
        .payload_json
        .as_mut()
        .unwrap()
        .insert(
            "sourceEventId".into(),
            Value::String("event:fact:p1:born:1".into()),
        );
    CommitService::commit(&connection, &initial, NOW).unwrap();

    let stored: Option<String> = connection
        .query_row(
            "SELECT source_event_id FROM narrative_obligations WHERE obligation_id='obl:p1:1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored.as_deref(), Some("event:fact:p1:born:1"));

    let mut update = full_bundle();
    update.commit_id = "commit:b02:source-event-update".into();
    update.command_id = "b02-source-event-update".into();
    update.idempotency_key = "b02-source-event-update-key".into();
    update.expected_project_revision = Some(1);
    update.artifacts.clear();
    update.head_updates = None;
    update.canon_delta = None;
    update.foreshadow_delta = None;
    update.knowledge_delta = None;
    update.state_delta = None;
    update.dependency_edges = None;
    update.job_updates = None;
    update.decision_updates = None;
    update.reader_desire_delta = None;
    update.gate_certificates = None;
    update.receipt_draft = None;
    update.obligation_delta.as_mut().unwrap()[0]
        .payload_json
        .as_mut()
        .unwrap()
        .remove("sourceEventId");

    CommitService::commit(&connection, &update, NOW).unwrap();
    let stored: Option<String> = connection
        .query_row(
            "SELECT source_event_id FROM narrative_obligations WHERE obligation_id='obl:p1:1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored.as_deref(), Some("event:fact:p1:born:1"));
}

#[test]
fn fx1_obligation_delta_rejects_unknown_source_event_and_rolls_back() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let before_artifacts = connection
        .query_row::<i64, _, _>("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
        .unwrap();

    let mut bundle = full_bundle();
    bundle.obligation_delta.as_mut().unwrap()[0]
        .payload_json
        .as_mut()
        .unwrap()
        .insert(
            "sourceEventId".into(),
            Value::String("event:missing".into()),
        );
    let error = CommitService::commit(&connection, &bundle, NOW).unwrap_err();
    assert!(error
        .to_string()
        .contains("OBLIGATION_SOURCE_EVENT_NOT_FOUND"));
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT revision FROM project_meta", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
            .unwrap(),
        before_artifacts
    );
    for table in [
        "canon_events",
        "canon_facts",
        "narrative_obligations",
        "commit_receipts",
    ] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "unexpected rows in {table}");
    }
}
