use ai_novel_production_system_core::commit::{CommitError, CommitService};
use ai_novel_production_system_core::generated::types::{ArtifactEnvelope, CommitBundle};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use serde_json::{json, Value};
use tempfile::tempdir;

const NOW: &str = "2026-08-26T00:00:00Z";
const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn final_artifact(project_id: &str, command_id: &str) -> ArtifactEnvelope {
    let payload = json!({"title":"第一章","content":"最终正文"});
    serde_json::from_value(json!({
        "artifactId": "art:p1:chapter:1",
        "projectId": project_id,
        "artifactType": "ChapterCandidate",
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
        "artifacts":[serde_json::to_value(final_artifact("p1", command_id)).unwrap()],
        "canonDelta":[{"entityId":"ent:p1:root:1","factId":"fact:p1:born:1","subjectId":"ent:p1:root:1","predicate":"cast","objectJson":{"name":"王五"},"authorityLevel":"C","eventType":"character_introduction","payloadJson":{"sourceArtifactId":"art:p1:chapter:1","sourceVersion":1,"chapterId":"chapter-1"}}],
        "obligationDelta":[{"obligationId":"obl:p1:1","scope":"VOLUME","obligationType":"payoff","authorityLevel":"B","lifecycle":"APPROVED","description":"pay off the planted mystery","payloadJson":{"sourceArtifactId":"art:p1:chapter:1","sourceVersion":1,"chapterId":"chapter-1"}}],
        "foreshadowDelta":[{"foreshadowId":"fs:p1:1","intent":"the same knife returns","status":"PLANTED","plantChapterId":"chap:p1:1","payloadJson":{"sourceAuthority":"C"}}],
        "knowledgeDelta":[{"knowledgeId":"kn:p1:1","knowerId":"ent:p1:root:1","factId":"fact:p1:born:1","epistemicState":"KNOWS","fromChapterId":"chap:p1:1","payloadJson":{"certainty":1}}],
        "stateDelta":[{"stateId":"st:p1:1","stateType":"mood","valueJson":{"valence":"grim"},"chapterId":"chapter-1","payloadJson":{"currentPhase":"OPENING"}}],
        "dependencyEdges":[{"edgeId":"edge:p1:1","sourceRef":"fact:p1:born:1","targetRef":"chap:p1:1","dependencyType":"CANON_FACT","scope":"CHAPTER","impactClass":"HARD","sourceHash":HASH,"payloadJson":{"createdBy":"b02-test"}}],
        "jobUpdates":[{"jobId":"job:p1:1","status":"COMPLETED","currentStage":"epilogue","attemptNo":1,"payloadJson":{"jobType":"CHAPTER"}}],
        "decisionUpdates":[{"decisionId":"dec:p1:1","decisionType":"cast","outcome":"approve","reason":"retains canon","sourceRef":"job:p1:1","payloadJson":{}}],
        "gateCertificates":[{"certificateId":"gate:p1:1","projectId":"p1","gatePolicyRef":{"artifactId":"art:p1:chapter:1","version":1,"payloadHash":HASH},"evidenceRefs":[],"sourceDigest":HASH,"passedAt":NOW,"certificateDigest":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}]
    })).unwrap();
    let artifact_hash = bundle.artifacts[0].payload_hash.clone();
    let certificates = bundle.gate_certificates.as_mut().unwrap();
    certificates[0].gate_policy_ref.payload_hash = artifact_hash;
    let mut certificate_value = serde_json::to_value(&certificates[0]).unwrap();
    certificate_value["certificateDigest"] = Value::String(String::new());
    certificates[0].certificate_digest = sha256_canonical(&certificate_value).unwrap();
    bundle
}

fn seed_prerequisites(connection: &libsql_rusqlite::Connection) {
    connection.execute("INSERT INTO books(book_id, project_id, book_no, title, status, created_at, updated_at) VALUES ('book:p1:1','p1',1,'Book','ACTIVE',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id, project_id, book_id, volume_no, title, status, chapter_start, chapter_end, target_words, created_at, updated_at) VALUES ('vol:p1:1','p1','book:p1:1',1,'Volume','PLANNING',1,1,1000,?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id, project_id, volume_id, chapter_no, title, importance_class, status, word_count, created_at, updated_at) VALUES ('chapter-1','p1','vol:p1:1',1,'Chapter','NORMAL','PLANNED',0,?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id, project_id, canonical_name, entity_type, created_at, updated_at) VALUES ('ent:p1:root:1','p1','王五','CHARACTER',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO jobs(job_id, project_id, job_type, chapter_id, status, created_at, updated_at) VALUES ('job:p1:1','p1','CHAPTER','chapter-1','RUNNING',?1,?1)", [NOW]).unwrap();
}

#[test]
fn b02_full_delta_bundle_is_not_rejected_as_unsupported() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let receipt = CommitService::commit(&connection, &full_bundle(), NOW).unwrap();
    assert_eq!(receipt.after_revision, 1);
    assert_eq!(receipt.applied_delta_counts["canon"], 3);
    for channel in [
        "obligation",
        "foreshadow",
        "knowledge",
        "state",
        "dependency",
        "job",
        "decision",
    ] {
        assert_eq!(
            receipt.applied_delta_counts[channel], 1,
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
            0,
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
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM commit_receipts", [], |row| row.get(0))
            .unwrap(),
        0
    );
}
