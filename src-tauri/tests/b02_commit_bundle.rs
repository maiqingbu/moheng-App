use ai_novel_production_system_core::commit::{CommitError, CommitService};
use ai_novel_production_system_core::generated::types::{ArtifactEnvelope, CommitBundle};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use serde_json::json;
use tempfile::tempdir;

const NOW: &str = "2026-08-26T00:00:00Z";
const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn final_artifact(project_id: &str, command_id: &str) -> ArtifactEnvelope {
    let payload = json!({"title":"第一章","content":"最终正文"});
    serde_json::from_value(json!({
        "artifactId": "art:p1:chapter:1",
        "projectId": project_id,
        "artifactType": "ChapterCandidate",
        "scope": {"level":"CHAPTER","chapterId":"chap:p1:1"},
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
    serde_json::from_value(json!({
        "commitId":"commit:b02:full",
        "projectId":"p1",
        "commandId":command_id,
        "idempotencyKey":"b02-full-delta-key",
        "expectedProjectRevision":0,
        "contractVersion":"m0a.v1",
        "createdAt":NOW,
        "artifacts":[serde_json::to_value(final_artifact("p1", command_id)).unwrap()],
        "canonDelta":[{"entityId":"ent:p1:root:1","factId":"fact:p1:born:1","subjectId":"ent:p1:root:1","predicate":"cast","objectJson":{"name":"王五"},"authorityLevel":"C","eventType":"character_introduction","payloadJson":{"sourceArtifactId":"art:p1:chapter:1","sourceVersion":1}}],
        "obligationDelta":[{"obligationId":"obl:p1:1","scope":"VOLUME","obligationType":"payoff","authorityLevel":"B","lifecycle":"APPROVED","description":"pay off the planted mystery","payloadJson":{"sourceArtifactId":"art:p1:chapter:1","sourceVersion":1}}],
        "foreshadowDelta":[{"foreshadowId":"fs:p1:1","intent":"the same knife returns","status":"PLANTED","plantChapterId":"chap:p1:1","payloadJson":{"sourceAuthority":"C"}}],
        "knowledgeDelta":[{"knowledgeId":"kn:p1:1","knowerId":"ent:p1:root:1","factId":"fact:p1:born:1","epistemicState":"KNOWS","fromChapterId":"chap:p1:1","payloadJson":{"certainty":1}}],
        "stateDelta":[{"stateId":"st:p1:1","stateType":"mood","valueJson":{"valence":"grim"},"chapterId":"chap:p1:1","payloadJson":{"currentPhase":"OPENING"}}],
        "dependencyEdges":[{"edgeId":"edge:p1:1","sourceRef":"fact:p1:born:1","targetRef":"chap:p1:1","dependencyType":"CANON_FACT","scope":"CHAPTER","impactClass":"HARD","sourceHash":HASH,"payloadJson":{"createdBy":"b02-test"}}],
        "jobUpdates":[{"jobId":"job:p1:1","status":"COMPLETED","currentStage":"epilogue","attemptNo":1,"payloadJson":{"jobType":"CHAPTER"}}],
        "decisionUpdates":[{"decisionId":"dec:p1:1","decisionType":"cast","outcome":"approve","reason":"retains canon","sourceRef":"job:p1:1","payloadJson":{}}],
        "gateCertificates":[{"certificateId":"gate:p1:1","projectId":"p1","gatePolicyRef":{"artifactId":"art:p1:chapter:1","version":1,"payloadHash":HASH},"evidenceRefs":[],"sourceDigest":HASH,"passedAt":NOW,"certificateDigest":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}]
    })).unwrap()
}

fn seed_prerequisites(connection: &libsql_rusqlite::Connection) {
    connection.execute("INSERT INTO canon_entities(entity_id, project_id, canonical_name, entity_type, created_at, updated_at) VALUES ('ent:p1:root:1','p1','王五','CHARACTER',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO jobs(job_id, project_id, job_type, chapter_id, status, created_at, updated_at) VALUES ('job:p1:1','p1','CHAPTER','chap:p1:1','RUNNING',?1,?1)", [NOW]).unwrap();
}

#[test]
fn b02_full_delta_bundle_is_not_rejected_as_unsupported() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    seed_prerequisites(&connection);
    let error = CommitService::commit(&connection, &full_bundle(), NOW).unwrap_err();
    assert!(
        !matches!(error, CommitError::UnsupportedDelta),
        "B02 must remove the reserved-delta rejection: {error:?}"
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
