use ai_novel_production_system_core::commit::{CommitError, CommitService};
use ai_novel_production_system_core::generated::types::{
    ArtifactEnvelope, ArtifactStatus, CommitBundle, CommitBundleHeadUpdatesItem,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::{
    connection::open_connection, ManagerError, ProjectManager,
};
use ai_novel_production_system_core::repositories::artifacts::{ArtifactError, ArtifactRepository};
use serde_json::json;
use tempfile::tempdir;

fn artifact(project_id: &str, id: &str, status: ArtifactStatus) -> ArtifactEnvelope {
    let payload = json!({"title": "第一章", "content": "你好\n世界"});
    let status = serde_json::to_value(status).unwrap();
    serde_json::from_value(json!({
        "artifactId": id,
        "projectId": project_id,
        "artifactType": "ChapterCandidate",
        "scope": {"level": "CHAPTER", "chapterId": "chapter-1"},
        "version": 1,
        "schemaVersion": "m0a.artifact.v1",
        "artifactStatus": status,
        "payloadHash": sha256_canonical(&payload).unwrap(),
        "dependencyRefs": [],
        "provenance": {"createdBy": "test", "commandId": "placeholder"},
        "payload": payload,
        "createdAt": "2026-08-24T00:00:00Z"
    }))
    .unwrap()
}
fn bundle(project: &str, key: &str, mut artifact: ArtifactEnvelope) -> CommitBundle {
    let command_id = format!("command-{key}");
    artifact.provenance.command_id = command_id.clone();
    CommitBundle {
        commit_id: format!("commit-{key}"),
        project_id: project.into(),
        command_id,
        idempotency_key: key.into(),
        expected_project_revision: Some(0),
        contract_version: "m0a.v1".into(),
        created_at: "2026-08-24T00:00:00Z".into(),
        artifacts: vec![artifact],
        head_updates: None,
        canon_delta: None,
        obligation_delta: None,
        foreshadow_delta: None,
        knowledge_delta: None,
        state_delta: None,
        dependency_edges: None,
        job_updates: None,
        decision_updates: None,
        gate_certificates: None,
    }
}

#[test]
fn opening_missing_project_fails_closed_without_creating_database() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));

    assert!(matches!(
        manager.open("missing", "Missing", "2026-08-24T00:00:00Z"),
        Err(ManagerError::ProjectNotFound)
    ));
    assert!(!temp.path().join("projects/missing/project.db").exists());
}

#[test]
fn read_only_connection_does_not_create_missing_database() {
    let temp = tempdir().unwrap();
    let database = temp.path().join("nested/project.db");

    assert!(open_connection(&database, true).is_err());
    assert!(!database.exists());
}

#[test]
fn projects_are_isolated_and_migrations_are_repeatable() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let first = manager
        .create("project-a", "A", "2026-08-24T00:00:00Z")
        .unwrap();
    let second = manager
        .create("project-b", "B", "2026-08-24T00:00:00Z")
        .unwrap();
    assert_eq!(
        first
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM project_meta", [], |row| row.get(0))
            .unwrap(),
        1
    );
    drop(first);
    manager
        .open("project-a", "A", "2026-08-24T00:00:01Z")
        .unwrap();
    assert_eq!(
        second
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM project_meta", [], |row| row.get(0))
            .unwrap(),
        1
    );
}

#[test]
fn receipt_digest_is_verified_when_replayed() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("project-a", "A", "2026-08-24T00:00:00Z")
        .unwrap();
    let first_bundle = bundle(
        "project-a",
        "receipt-key",
        artifact("project-a", "artifact-1", ArtifactStatus::Candidate),
    );
    CommitService::commit(&connection, &first_bundle, "2026-08-24T00:00:01Z").unwrap();
    connection
        .execute(
            "UPDATE commit_receipts SET receipt_digest = ?1 WHERE idempotency_key = ?2",
            ["tampered", "receipt-key"],
        )
        .unwrap();

    assert!(matches!(
        CommitService::commit(&connection, &first_bundle, "2026-08-24T00:00:02Z"),
        Err(CommitError::ReceiptDigestMismatch)
    ));
}

#[test]
fn commit_is_idempotent_and_rejects_different_digest() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("project-a", "A", "2026-08-24T00:00:00Z")
        .unwrap();
    let first_bundle = bundle(
        "project-a",
        "stable-key",
        artifact("project-a", "artifact-1", ArtifactStatus::Candidate),
    );
    let receipt =
        CommitService::commit(&connection, &first_bundle, "2026-08-24T00:00:01Z").unwrap();
    assert_eq!(receipt.after_revision, 1);
    let replay = CommitService::commit(&connection, &first_bundle, "2026-08-24T00:00:02Z").unwrap();
    assert_eq!(replay.receipt_id, receipt.receipt_id);
    let mut conflict = first_bundle.clone();
    conflict.created_at = "2026-08-24T00:00:03Z".into();
    assert!(matches!(
        CommitService::commit(&connection, &conflict, "2026-08-24T00:00:03Z"),
        Err(CommitError::IdempotencyConflict)
    ));
}

#[test]
fn promote_head_rejects_hash_and_scope_mismatches() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("project-a", "A", "2026-08-24T00:00:00Z")
        .unwrap();
    let candidate = artifact("project-a", "artifact-1", ArtifactStatus::Candidate);
    ArtifactRepository::insert_candidate(&connection, "project-a", &candidate).unwrap();
    connection
        .execute(
            "UPDATE artifacts SET artifact_status = 'COMMITTED' WHERE artifact_id = ?1",
            ["artifact-1"],
        )
        .unwrap();

    let mut wrong_hash = candidate.payload_hash.clone();
    wrong_hash.replace_range(..1, "0");
    assert!(matches!(
        ArtifactRepository::promote_head(
            &connection,
            "project-a",
            &ai_novel_production_system_core::generated::types::ArtifactRef {
                artifact_id: "artifact-1".into(),
                version: 1,
                payload_hash: wrong_hash,
            },
            "ChapterCandidate",
            "CHAPTER",
            &serde_json::to_string(&candidate.scope).unwrap(),
        ),
        Err(ArtifactError::HashMismatch)
    ));

    assert!(matches!(
        ArtifactRepository::promote_head(
            &connection,
            "project-a",
            &ai_novel_production_system_core::generated::types::ArtifactRef {
                artifact_id: "artifact-1".into(),
                version: 1,
                payload_hash: candidate.payload_hash,
            },
            "WrongType",
            "CHAPTER",
            &serde_json::to_string(&candidate.scope).unwrap(),
        ),
        Err(ArtifactError::MetadataMismatch)
    ));
}

#[test]
fn failed_commit_rolls_back_all_rows_and_revision() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("project-a", "A", "2026-08-24T00:00:00Z")
        .unwrap();
    let mut failed = bundle(
        "project-a",
        "rollback-key",
        artifact("project-a", "artifact-1", ArtifactStatus::Candidate),
    );
    failed.head_updates = Some(vec![
        serde_json::from_value::<CommitBundleHeadUpdatesItem>(
            json!({"artifactId":"missing", "version":1, "payloadHash":"0000000000000000000000000000000000000000000000000000000000000000", "artifactType":"ChapterCandidate", "scope":{"level":"CHAPTER", "chapterId":"chapter-1"}}),
        )
        .unwrap(),
    ]);
    assert!(CommitService::commit(&connection, &failed, "2026-08-24T00:00:01Z").is_err());
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
