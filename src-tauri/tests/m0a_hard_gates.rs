use ai_novel_production_system_core::catalog::CatalogRepository;
use ai_novel_production_system_core::commit::{CommitError, CommitService};
use ai_novel_production_system_core::generated::types::{ArtifactEnvelope, CommitBundle};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::repositories::artifacts::{ArtifactError, ArtifactRepository};
use serde_json::{json, Value};
use tempfile::tempdir;

fn artifact(project_id: &str, artifact_id: &str, version: i64) -> ArtifactEnvelope {
    let payload = json!({"text": "a\r\nb"});
    serde_json::from_value(json!({
        "artifactId": artifact_id,
        "projectId": project_id,
        "artifactType": "ChapterCandidate",
        "scope": {"level": "CHAPTER", "chapterId": "chapter-1"},
        "version": version,
        "schemaVersion": "m0a.artifact.v1",
        "artifactStatus": "CANDIDATE",
        "payload": payload,
        "payloadHash": sha256_canonical(&payload).unwrap(),
        "dependencyRefs": [],
        "provenance": {"createdBy": "test", "commandId": "command-1"},
        "createdAt": "2026-08-24T00:00:00Z"
    }))
    .unwrap()
}

fn bundle(project_id: &str, artifact: ArtifactEnvelope) -> CommitBundle {
    serde_json::from_value(json!({
        "commitId": "commit-1",
        "projectId": project_id,
        "commandId": "command-1",
        "idempotencyKey": "key-1",
        "expectedProjectRevision": 0,
        "contractVersion": "m0a.v1",
        "createdAt": "2026-08-24T00:00:00Z",
        "artifacts": [artifact]
    }))
    .unwrap()
}

#[test]
fn project_manager_enforces_single_writer_and_wal() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let first = manager
        .create("project-a", "A", "2026-08-24T00:00:00Z")
        .unwrap();

    let journal_mode: String = first
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode.to_lowercase(), "wal");
    assert!(manager
        .open("project-a", "A", "2026-08-24T00:00:01Z")
        .is_err());
}

#[test]
fn catalog_tracks_create_rename_archive_and_last_opened() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let manager = ProjectManager::new(paths.clone());
    drop(
        manager
            .create("project-a", "A", "2026-08-24T00:00:00Z")
            .unwrap(),
    );
    manager
        .rename("project-a", "Renamed", "2026-08-24T00:00:01Z")
        .unwrap();
    manager
        .set_lifecycle("project-a", "ARCHIVED", "2026-08-24T00:00:02Z")
        .unwrap();

    let catalog = CatalogRepository::open(&paths, "2026-08-24T00:00:03Z").unwrap();
    let row: (String, String, Option<String>) = catalog
        .query_row(
            "SELECT title, lifecycle, last_opened_at FROM projects WHERE project_id = 'project-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(row.0, "Renamed");
    assert_eq!(row.1, "ARCHIVED");
    assert!(row.2.is_some());
}

#[test]
fn artifact_versions_are_monotonic_and_paged_metadata_excludes_payload() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("project-a", "A", "2026-08-24T00:00:00Z")
        .unwrap();

    assert!(matches!(
        ArtifactRepository::insert_candidate(
            &connection,
            "project-a",
            &artifact("project-a", "artifact-1", 2)
        ),
        Err(ArtifactError::VersionConflict)
    ));
    ArtifactRepository::insert_candidate(
        &connection,
        "project-a",
        &artifact("project-a", "artifact-1", 1),
    )
    .unwrap();
    let page = ArtifactRepository::list_metadata(&connection, "project-a", 10, None).unwrap();
    assert_eq!(page.len(), 1);
    assert!(!page[0].contains_key("payload"));
}

#[test]
fn commit_rejects_reserved_deltas_instead_of_silently_dropping_them() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("project-a", "A", "2026-08-24T00:00:00Z")
        .unwrap();
    let mut value =
        serde_json::to_value(bundle("project-a", artifact("project-a", "artifact-1", 1))).unwrap();
    value["canonDelta"] = json!([{"factId": "fact-1", "entityId": "ent-1"}]);
    let bundle: CommitBundle = serde_json::from_value(value).unwrap();

    assert!(matches!(
        CommitService::commit(&connection, &bundle, "2026-08-24T00:00:01Z"),
        Err(CommitError::UnsupportedDelta)
    ));
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn rust_hashes_every_shared_golden_vector() {
    let vectors: Vec<Value> = serde_json::from_str(include_str!(
        "../../contracts/fixtures/golden/jcs-vectors.json"
    ))
    .unwrap();
    assert!(vectors.len() >= 20);
    for vector in vectors {
        let expected = vector["canonicalSha256"].as_str().unwrap();
        assert_eq!(sha256_canonical(&vector["value"]).unwrap(), expected);
    }
}

#[test]
fn migration_reopen_creates_backup_before_reapplying_schema() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("project-a", "A", "2026-08-24T00:00:00Z")
        .unwrap();
    connection
        .execute("DELETE FROM migration_history WHERE version = 2", [])
        .unwrap();
    drop(connection);

    drop(
        manager
            .open("project-a", "A", "2026-08-24T00:00:01Z")
            .unwrap(),
    );
    let backups = temp.path().join("projects/project-a/backups");
    assert!(backups.is_dir());
    assert!(std::fs::read_dir(backups).unwrap().next().is_some());
}

#[test]
fn committed_receipt_survives_close_and_reopen() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("project-a", "A", "2026-08-24T00:00:00Z")
        .unwrap();
    let committed = bundle("project-a", artifact("project-a", "artifact-1", 1));
    let receipt = CommitService::commit(&connection, &committed, "2026-08-24T00:00:01Z").unwrap();
    drop(connection);

    let reopened = manager
        .open("project-a", "A", "2026-08-24T00:00:02Z")
        .unwrap();
    let replay = CommitService::find_receipt(&reopened, "project-a", "key-1")
        .unwrap()
        .unwrap();
    assert_eq!(replay.receipt_id, receipt.receipt_id);
}

#[test]
fn candidate_gc_is_policy_bounded_and_writes_receipt() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("project-a", "A", "2026-08-24T00:00:00Z")
        .unwrap();
    ArtifactRepository::insert_candidate(
        &connection,
        "project-a",
        &artifact("project-a", "candidate-1", 1),
    )
    .unwrap();

    let receipt = ArtifactRepository::collect_expired_candidates(
        &connection,
        "project-a",
        "2026-08-25T00:00:00Z",
        "policy-digest",
        "2026-08-26T00:00:00Z",
    )
    .unwrap();
    assert_eq!(receipt.deleted_candidate_count, 1);
    let remaining: i64 = connection
        .query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(remaining, 0);
    let receipts: i64 = connection
        .query_row("SELECT COUNT(*) FROM gc_receipts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(receipts, 1);
}
