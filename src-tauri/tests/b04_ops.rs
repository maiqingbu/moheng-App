use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::{ProjectHandle, ProjectManager};
use ai_novel_production_system_core::repositories::ops::{
    deep_integrity_check, read_runtime_profile, FixtureSpec, LongBookFixture, SearchDocument,
    SearchService,
};
use libsql_rusqlite::Connection;
use tempfile::tempdir;

const NOW: &str = "2026-08-27T00:00:00Z";

fn project() -> (tempfile::TempDir, ProjectManager, ProjectHandle) {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "B04", NOW).unwrap();
    (temp, manager, connection)
}

#[test]
fn b04_migration_creates_fts_and_runtime_ops_authority_tables() {
    let (_temp, _manager, connection) = project();
    for table in [
        "search_documents",
        "search_documents_fts",
        "sqlite_runtime_profiles",
        "wal_governance_reports",
        "vacuum_reports",
        "backup_manifests",
        "integrity_runs",
    ] {
        assert_eq!(
            connection
                .query_row::<i64, _, _>(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap(),
            1,
            "missing B04 table {table}"
        );
    }
}

#[test]
fn b04_search_uses_fts_and_server_side_cursor_pagination() {
    let (_temp, _manager, connection) = project();
    SearchService::index_document(
        &connection,
        &SearchDocument {
            project_id: "p1".into(),
            doc_id: "doc:one".into(),
            chapter_id: Some("chap:one".into()),
            volume_id: Some("vol:one".into()),
            title: "星门初启".into(),
            body: "星门在雨夜开启，主角第一次看见远方灯火。".into(),
            kind: "CHAPTER".into(),
        },
        NOW,
    )
    .unwrap();
    SearchService::index_document(
        &connection,
        &SearchDocument {
            project_id: "p1".into(),
            doc_id: "doc:two".into(),
            chapter_id: Some("chap:two".into()),
            volume_id: Some("vol:one".into()),
            title: "灯火回响".into(),
            body: "灯火照见旧城的秘密。".into(),
            kind: "EVIDENCE".into(),
        },
        NOW,
    )
    .unwrap();
    let first = SearchService::search(&connection, "p1", "灯火", None, 1).unwrap();
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.total, 2);
    assert!(first.next_cursor.is_some());
    let second = SearchService::search(
        &connection,
        "p1",
        "灯火",
        first.next_cursor.as_deref(),
        1,
    )
    .unwrap();
    assert_eq!(second.items.len(), 1);
    assert_ne!(first.items[0].doc_id, second.items[0].doc_id);
}

#[test]
fn b04_runtime_profile_is_recorded_and_deep_integrity_fails_closed_on_tamper() {
    let (_temp, _manager, connection) = project();
    let profile = read_runtime_profile(&connection, "p1").unwrap();
    assert!(profile.page_size > 0);
    assert_eq!(profile.busy_timeout_ms, 5000);
    connection
        .execute("INSERT INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, payload_hash, provenance_json, created_at) VALUES ('art:b04:tamper','p1','ChapterCandidate','CHAPTER','chap:one',1,'m0a.artifact.v1','CANDIDATE',?1,'{}',?2)", ["a".repeat(64), NOW])
        .unwrap();
    connection
        .execute("INSERT INTO artifact_payloads(artifact_id, version, payload_json, payload_hash) VALUES ('art:b04:tamper',1,'{\"body\":\"changed\"}',?1)", ["b".repeat(64)])
        .unwrap();
    let report = deep_integrity_check(&connection, "p1", NOW).unwrap();
    assert!(!report.ok);
    assert!(report.failures.iter().any(|failure| failure == "ARTIFACT_PAYLOAD_HASH_MISMATCH"));
}

#[test]
fn b04_long_book_fixture_manifest_is_deterministic_for_fixed_seed() {
    let spec = FixtureSpec {
        seed: "b04-fixed".into(),
        books: 1,
        volumes: 2,
        chapters: 8,
        artifacts: 16,
        canon_facts: 24,
        dependencies: 32,
        model_runs: 4,
        target_words: 10_000_000,
    };
    let left = LongBookFixture::manifest(&spec);
    let right = LongBookFixture::manifest(&spec);
    assert_eq!(left, right);
    assert_eq!(left.target_words, 10_000_000);
    assert_eq!(left.seed, "b04-fixed");
}

#[allow(dead_code)]
fn _connection_is_explicitly_used(_connection: &Connection) {}
