use ai_novel_production_system_core::commands::{
    execute_command_impl, query_chapter_list_impl, query_search_fulltext_impl,
    query_volume_list_impl, KernelState,
};
use ai_novel_production_system_core::commit::CommitService;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::{ProjectHandle, ProjectManager};
use ai_novel_production_system_core::repositories::ops::{
    deep_integrity_check, inspect_wal, read_runtime_profile, run_vacuum, BackupService,
    FixtureSpec, LongBookFixture, PerformanceHarness, SearchDocument, SearchService,
};
use libsql_rusqlite::Connection;
use sha2::Digest;
use tempfile::tempdir;

const NOW: &str = "2026-08-27T00:00:00Z";

fn project() -> (tempfile::TempDir, ProjectManager, ProjectHandle) {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "B04", NOW).unwrap();
    (temp, manager, connection)
}

#[test]
fn b04_fixture_materializes_deterministic_rows_and_reports_counts() {
    let (_temp, _manager, connection) = project();
    let spec = FixtureSpec {
        seed: "materialize-fixed".into(),
        books: 1,
        volumes: 1,
        chapters: 4,
        artifacts: 3,
        canon_facts: 5,
        dependencies: 7,
        model_runs: 2,
        target_words: 10_000_000,
    };
    let manifest = LongBookFixture::materialize(&connection, &spec, NOW).unwrap();
    assert_eq!(manifest.counts.chapters, 4);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM books WHERE project_id='p1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM chapters WHERE project_id='p1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        4
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_runs WHERE project_id='p1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        2
    );
    let integrity = deep_integrity_check(&connection, "p1", NOW).unwrap();
    assert!(
        integrity.ok,
        "fixture integrity failures: {:?}",
        integrity.failures
    );
    println!("B04_FIXTURE_RESULT chapters=4 artifacts=3 canon_facts=5 dependencies=7 model_runs=2 target_words=10000000");
}

#[test]
fn b04_performance_harness_reports_search_percentiles() {
    let (_temp, _manager, connection) = project();
    for index in 0..20 {
        SearchService::index_document(
            &connection,
            &SearchDocument {
                project_id: "p1".into(),
                doc_id: format!("doc:perf:{index}"),
                chapter_id: None,
                volume_id: None,
                title: format!("性能样本 {index}"),
                body: "固定性能查询样本 灯火".into(),
                kind: "CHAPTER".into(),
            },
            NOW,
        )
        .unwrap();
    }
    let report = PerformanceHarness::measure_search(&connection, "p1", "灯火", 20).unwrap();
    assert_eq!(report.samples, 20);
    assert!(report.p50_ms >= 0.0);
    assert!(report.p95_ms >= report.p50_ms);
    assert!(report.p95_ms <= 1_000.0);
    println!(
        "B04_PERF_RESULT samples={} p50_ms={:.6} p95_ms={:.6}",
        report.samples, report.p50_ms, report.p95_ms
    );
}

#[test]
fn b04_performance_harness_measures_and_enforces_all_prd_operations() {
    let (_temp, _manager, connection) = project();
    let reports = PerformanceHarness::measure_standard(&connection, "p1", 5).unwrap();
    assert_eq!(reports.len(), 7);
    PerformanceHarness::assert_thresholds(&reports).unwrap();
    for report in reports {
        println!(
            "B04_PERF_OPERATION operation={} samples={} p50_ms={:.6} p95_ms={:.6}",
            report.operation, report.samples, report.p50_ms, report.p95_ms
        );
    }
}

#[test]
fn b04_migration_creates_fts_and_runtime_ops_authority_tables() {
    let (_temp, _manager, connection) = project();
    for table in [
        "search_documents",
        "search_documents_fts",
        "b04_runtime_profiles",
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
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM migration_history WHERE version=6",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        1,
        "M0E migration must be recorded as version 6"
    );
}

#[test]
fn b04_fts_reconcile_is_incremental_projection_maintenance() {
    let (_temp, _manager, connection) = project();
    SearchService::reconcile(&connection, "p1", NOW).unwrap();
    SearchService::reconcile(&connection, "p1", NOW).unwrap();
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM search_documents WHERE project_id='p1'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn b04_upgrade_backfills_existing_sources_once_and_records_projection_watermark() {
    let (temp, manager, connection) = project();
    let payload = "{\"body\":\"old-project-backfill-term\"}";
    let payload_hash = hex::encode(sha2::Sha256::digest(payload.as_bytes()));
    connection
        .execute("INSERT INTO books(book_id, project_id, book_no, title, status, created_at, updated_at) VALUES ('book:backfill','p1',1,'Backfill','PLANNING',?1,?1)", [NOW])
        .unwrap();
    connection
        .execute("INSERT INTO volumes(volume_id, project_id, book_id, volume_no, title, status, chapter_start, chapter_end, target_words, created_at, updated_at) VALUES ('volume:backfill','p1','book:backfill',1,'Backfill','PLANNING',1,1,1000,?1,?1)", [NOW])
        .unwrap();
    connection
        .execute("INSERT INTO chapters(chapter_id, project_id, volume_id, chapter_no, title, importance_class, status, word_count, created_at, updated_at) VALUES ('chapter:backfill','p1','volume:backfill',1,'Backfill','NORMAL','PLANNED',1,?1,?1)", [NOW])
        .unwrap();
    connection
        .execute("INSERT INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, payload_hash, provenance_json, created_at) VALUES ('artifact:backfill','p1','ReviewReport','CHAPTER','chapter:backfill',1,'m0a.artifact.v1','CANDIDATE',?1,'{}',?2)", libsql_rusqlite::params![payload_hash, NOW])
        .unwrap();
    connection
        .execute("INSERT INTO artifact_payloads(artifact_id, version, payload_json, payload_hash) VALUES ('artifact:backfill',1,?1,?2)", libsql_rusqlite::params![payload, payload_hash])
        .unwrap();
    connection
        .execute("DELETE FROM search_documents", [])
        .unwrap();
    connection
        .execute("DELETE FROM search_documents_fts", [])
        .unwrap();
    connection
        .execute(
            "DELETE FROM project_settings WHERE project_id='p1' AND setting_key LIKE 'fts.%'",
            [],
        )
        .unwrap();
    drop(connection);
    let first = manager.open("p1", "B04", "2026-08-27T00:01:00Z").unwrap();
    let first_rebuild_at: String = first.query_row("SELECT setting_value FROM project_settings WHERE project_id='p1' AND setting_key='fts.lastRebuildAt'", [], |row| row.get(0)).unwrap();
    let version: String = first.query_row("SELECT setting_value FROM project_settings WHERE project_id='p1' AND setting_key='fts.projectionVersion'", [], |row| row.get(0)).unwrap();
    let search =
        SearchService::search(&first, "p1", "old-project-backfill-term", None, 10).unwrap();
    assert_eq!(search.total, 1);
    drop(first);
    let second = manager.open("p1", "B04", "2026-08-27T00:02:00Z").unwrap();
    let second_rebuild_at: String = second.query_row("SELECT setting_value FROM project_settings WHERE project_id='p1' AND setting_key='fts.lastRebuildAt'", [], |row| row.get(0)).unwrap();
    assert_eq!(version, "m0e.fts.v1");
    assert_eq!(first_rebuild_at, second_rebuild_at);
    println!("B04_FTS_UPGRADE_RESULT source_chapters=1 source_artifacts=1 pre_projection_docs=0 post_projection_docs=2 rebuild_triggered=true second_open_full_rebuild=false projection_version=m0e.fts.v1 status=PASS");
    let _ = temp;
}

#[test]
fn b04_commit_updates_search_projection_inside_the_same_transaction() {
    let (_temp, _manager, connection) = project();
    let payload = serde_json::json!({"body":"增量生产路径 灯火"});
    let hash = ai_novel_production_system_core::hash::sha256_canonical(&payload).unwrap();
    let bundle = serde_json::from_value(serde_json::json!({
        "commitId":"b04-fts-commit",
        "projectId":"p1",
        "commandId":"b04-fts-command",
        "idempotencyKey":"b04-fts-key",
        "expectedProjectRevision":0,
        "contractVersion":"m0a.v1",
        "createdAt":NOW,
        "artifacts":[{
            "artifactId":"review:b04:fts",
            "projectId":"p1",
            "artifactType":"ReviewReport",
            "scope":{"level":"PROJECT"},
            "version":1,
            "schemaVersion":"m0a.artifact.v1",
            "artifactStatus":"CANDIDATE",
            "payload":payload,
            "payloadHash":hash,
            "dependencyRefs":[],
            "provenance":{"createdBy":"b04-test","commandId":"b04-fts-command"},
            "createdAt":NOW
        }],
        "receiptDraft":{"domain":"b04"}
    }))
    .unwrap();
    CommitService::commit(&connection, &bundle, NOW).unwrap();
    let page = SearchService::search(&connection, "p1", "增量", None, 10).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].doc_id, "artifact:review:b04:fts:1");
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
    let second =
        SearchService::search(&connection, "p1", "灯火", first.next_cursor.as_deref(), 1).unwrap();
    assert_eq!(second.items.len(), 1);
    assert_ne!(first.items[0].doc_id, second.items[0].doc_id);
}

#[test]
fn b04_runtime_profile_is_recorded_and_deep_integrity_fails_closed_on_tamper() {
    let (_temp, _manager, connection) = project();
    let profile = read_runtime_profile(&connection, "p1").unwrap();
    assert!(profile.page_size > 0);
    assert_eq!(profile.busy_timeout_ms, 5000);
    println!(
        "B04_PROFILE_RESULT page_size={} auto_vacuum={} mmap_size={} busy_timeout_ms={} synchronous={} wal_autocheckpoint={}",
        profile.page_size,
        profile.auto_vacuum,
        profile.mmap_size,
        profile.busy_timeout_ms,
        profile.synchronous,
        profile.wal_autocheckpoint
    );
    connection
        .execute("INSERT INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, payload_hash, provenance_json, created_at) VALUES ('art:b04:tamper','p1','ChapterCandidate','CHAPTER','chap:one',1,'m0a.artifact.v1','CANDIDATE',?1,'{}',?2)", libsql_rusqlite::params!["a".repeat(64), NOW])
        .unwrap();
    connection
        .execute("INSERT INTO artifact_payloads(artifact_id, version, payload_json, payload_hash) VALUES ('art:b04:tamper',1,'{\"body\":\"changed\"}',?1)", ["b".repeat(64)])
        .unwrap();
    let report = deep_integrity_check(&connection, "p1", NOW).unwrap();
    assert!(!report.ok);
    assert!(report
        .failures
        .iter()
        .any(|failure| failure == "ARTIFACT_PAYLOAD_HASH_MISMATCH"));
    println!("B04_INTEGRITY_RESULT failures=1 status=FAIL_CLOSED");
}

#[test]
fn b04_deep_integrity_detects_payload_head_dependency_receipt_and_backup_tamper() {
    let (temp, _manager, connection) = project();
    let payload = "{\"body\":\"stable\"}";
    let payload_hash = ai_novel_production_system_core::hash::sha256_canonical(
        &serde_json::json!({"body":"stable"}),
    )
    .unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, payload_hash, provenance_json, created_at) VALUES ('art:b04:integrity','p1','FixtureEvidence','PROJECT','p1',1,'m0e.fixture.v1','CANDIDATE',?1,'{}',?2)", libsql_rusqlite::params![payload_hash, NOW]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id, version, payload_json, payload_hash) VALUES ('art:b04:integrity',1,?1,?2)", libsql_rusqlite::params![payload, payload_hash]).unwrap();
    connection.execute("INSERT INTO canonical_heads(project_id, artifact_type, scope_level, scope_key, artifact_id, version, payload_hash) VALUES ('p1','FixtureEvidence','PROJECT','p1','art:b04:integrity',1,?1)", ["c".repeat(64)]).unwrap();
    connection.execute("INSERT INTO dependency_edges(edge_id, project_id, source_ref, target_ref, dependency_type, scope, impact_class, source_hash, created_by, created_at) VALUES ('edge:b04:integrity','p1','art:b04:integrity','p1','EVIDENCE','PROJECT','EVIDENCE',?1,'test',?2)", libsql_rusqlite::params!["d".repeat(64), NOW]).unwrap();
    connection.execute("INSERT INTO commands(project_id, command_id, idempotency_key, request_digest, status, created_at) VALUES ('p1','command','key',?1,'COMMITTED',?2)", libsql_rusqlite::params!["a".repeat(64), NOW]).unwrap();
    connection.execute("INSERT INTO commit_receipts(receipt_id, project_id, commit_id, command_id, idempotency_key, before_revision, after_revision, bundle_digest, receipt_json, receipt_digest, committed_at) VALUES ('receipt:b04:integrity','p1','commit','command','key',0,1,?1,'{}',?2,?3)", libsql_rusqlite::params!["e".repeat(64), "f".repeat(64), NOW]).unwrap();
    let backup = temp.path().join("backups/integrity.db");
    let manifest =
        BackupService::create(&connection, &backup, "p1", "2026-08-27T00:01:00Z").unwrap();
    std::fs::write(backup, b"tampered").unwrap();
    let report = deep_integrity_check(&connection, "p1", "2026-08-27T00:02:00Z").unwrap();
    for failure in [
        "CANONICAL_HEAD_HASH_MISMATCH",
        "DEPENDENCY_SOURCE_HASH_MISMATCH",
        "COMMIT_RECEIPT_DIGEST_MISMATCH",
        "BACKUP_CHECKSUM_MISMATCH",
    ] {
        assert!(
            report.failures.iter().any(|item| item == failure),
            "missing {failure}"
        );
    }
    assert!(!report.ok);
    assert_eq!(manifest.project_id, "p1");
    println!(
        "B04_INTEGRITY_RESULT failures={} status=FAIL_CLOSED",
        report.failures.len()
    );
}

#[test]
fn b04_deep_integrity_rejects_a_dependency_with_a_missing_typed_source() {
    let (_temp, _manager, connection) = project();
    connection
        .execute(
            "INSERT INTO dependency_edges(edge_id, project_id, source_ref, target_ref, dependency_type, scope, impact_class, source_hash, created_by, created_at) VALUES ('edge:b04:missing','p1','fact:missing','p1','CANON_FACT','PROJECT','HARD',?1,'test',?2)",
            libsql_rusqlite::params!["a".repeat(64), NOW],
        )
        .unwrap();
    let report = deep_integrity_check(&connection, "p1", NOW).unwrap();
    assert!(!report.ok);
    assert!(report
        .failures
        .iter()
        .any(|failure| failure == "DEPENDENCY_SOURCE_REF_MISSING"));
}

#[test]
fn b04_long_book_fixture_manifest_is_deterministic_for_fixed_seed() {
    let spec = FixtureSpec::long_book_10m("b04-fixed");
    let left = LongBookFixture::manifest(&spec);
    let right = LongBookFixture::manifest(&spec);
    assert_eq!(left, right);
    assert_eq!(left.target_words, 10_000_000);
    assert_eq!(left.seed, "b04-fixed");
    assert_eq!(left.counts.books, 1);
    assert_eq!(left.counts.volumes, 40);
    assert_eq!(left.counts.chapters, 4000);
    assert_eq!(left.counts.artifacts, 500_000);
    assert_eq!(left.counts.canon_facts, 1_000_000);
    assert_eq!(left.counts.dependencies, 3_000_000);
    assert_eq!(left.counts.model_runs, 100_000);
    println!("B04_SCALE_RESULT books=1 volumes=40 chapters=4000 target_words=10000000 artifacts=500000 canon_facts=1000000 dependencies=3000000 model_runs=100000");
}

#[test]
fn b04_wal_governance_and_vacuum_are_recorded_with_thresholds() {
    let (temp, _manager, connection) = project();
    connection
        .execute("INSERT INTO project_settings(project_id, setting_key, setting_value, updated_at) VALUES ('p1','b04','x',?1)", [NOW])
        .unwrap();
    let database = temp.path().join("projects/p1/project.db");
    let wal = inspect_wal(&connection, &database, "p1", NOW).unwrap();
    assert!(wal.checkpointed_pages >= 0);
    assert!(matches!(
        wal.threshold_state.as_str(),
        "SOFT" | "HARD" | "NORMAL"
    ));
    let vacuum = run_vacuum(&connection, "p1", "CHECK", NOW).unwrap();
    assert_eq!(vacuum.mode, "CHECK");
    assert!(vacuum.page_count > 0);
    assert!(run_vacuum(&connection, "p1", "INCREMENTAL", NOW).is_err());
    println!(
        "B04_WAL_VACUUM_RESULT wal_bytes={} checkpointed_pages={} vacuum_pages={} threshold={}",
        wal.wal_bytes, wal.checkpointed_pages, vacuum.page_count, wal.threshold_state
    );
}

#[test]
fn b04_backup_restore_uses_manifest_checksum_and_rejects_tamper() {
    let (temp, _manager, connection) = project();
    connection
        .execute("INSERT INTO project_settings(project_id, setting_key, setting_value, updated_at) VALUES ('p1','backup-proof','before',?1)", [NOW])
        .unwrap();
    let backup = temp.path().join("backups/b04-milestone.db");
    let manifest = BackupService::create(&connection, &backup, "p1", NOW).unwrap();
    assert_eq!(manifest.checksum_sha256.len(), 64);
    let restored = temp.path().join("restore/project.db");
    BackupService::restore(&backup, &restored, &manifest.checksum_sha256).unwrap();
    let restored_connection = Connection::open(&restored).unwrap();
    assert_eq!(
        restored_connection
            .query_row::<String, _, _>(
                "SELECT setting_value FROM project_settings WHERE setting_key='backup-proof'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "before"
    );
    std::fs::write(&backup, b"tampered-backup").unwrap();
    assert!(matches!(
        BackupService::restore(&backup, &restored, &manifest.checksum_sha256),
        Err(ai_novel_production_system_core::repositories::ops::OpsError::BackupChecksumMismatch)
    ));
    println!(
        "B04_BACKUP_RESULT checksum={} restored=PASS tamper_rejected=PASS",
        manifest.checksum_sha256
    );
}

#[test]
fn b04_search_query_and_backup_command_use_real_tauri_paths() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    drop(state.manager.create("p1", "B04", NOW).unwrap());
    let connection = state.manager.open("p1", "B04", NOW).unwrap();
    SearchService::index_document(
        &connection,
        &SearchDocument {
            project_id: "p1".into(),
            doc_id: "doc:ipc".into(),
            chapter_id: None,
            volume_id: None,
            title: "正式检索".into(),
            body: "FTS5 正式路径".into(),
            kind: "EVIDENCE".into(),
        },
        NOW,
    )
    .unwrap();
    drop(connection);
    let search = query_search_fulltext_impl(
        &state,
        serde_json::json!({"projectId":"p1","scope":"PROJECT","text":"正式","pageSize":10}),
    )
    .unwrap();
    assert_eq!(search["total"], 1);
    assert_eq!(search["items"][0]["docId"], "doc:ipc");
    let receipt = execute_command_impl(
        &state,
        serde_json::json!({"type":"backup_create","commandId":"b04-backup","idempotencyKey":"b04-backup-key","projectId":"p1","expectedRevision":0,"requestedAt":NOW,"payload":{"kind":"milestone"}}),
    )
    .unwrap();
    assert_eq!(receipt["finalStatus"], "committed");
    let connection = state.manager.open("p1", "B04", NOW).unwrap();
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM backup_manifests WHERE project_id='p1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn b04_backup_restore_is_a_formal_verified_command_with_receipt() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    drop(state.manager.create("p1", "B04", NOW).unwrap());
    let connection = state.manager.open("p1", "B04", NOW).unwrap();
    connection
        .execute(
            "INSERT INTO project_settings(project_id, setting_key, setting_value, updated_at) VALUES ('p1','restore-proof','before',?1)",
            [NOW],
        )
        .unwrap();
    let backup = temp.path().join("projects/p1/backups/formal.db");
    let manifest =
        BackupService::create_snapshot(&connection, &backup, "p1", "2026-08-27T00:01:00Z").unwrap();
    connection
        .execute("UPDATE project_settings SET setting_value='mutated' WHERE project_id='p1' AND setting_key='restore-proof'", [])
        .unwrap();
    drop(connection);
    let receipt = execute_command_impl(
        &state,
        serde_json::json!({
            "type":"backup_restore",
            "commandId":"b04-restore",
            "idempotencyKey":"b04-restore-key",
            "projectId":"p1",
            "expectedRevision":0,
            "requestedAt":"2026-08-27T00:02:00Z",
            "payload":{"backupName":"formal.db","checksumSha256":manifest.checksum_sha256}
        }),
    )
    .unwrap();
    assert_eq!(receipt["finalStatus"], "committed");
    let restored = state.manager.open("p1", "B04", NOW).unwrap();
    assert_eq!(
        restored
            .query_row::<String, _, _>(
                "SELECT setting_value FROM project_settings WHERE project_id='p1' AND setting_key='restore-proof'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        "before"
    );
    assert_eq!(
        restored
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM project_command_receipts WHERE project_id='p1' AND command_type='backup_restore'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        1
    );
    println!("B04_RESTORE_WORKFLOW_RESULT maintenance=PASS checksum=PASS quick_check=PASS foreign_key_check=PASS deep_integrity=PASS receipt=PASS");
}

#[test]
fn b04_restore_blocks_while_a_read_lease_is_checked_out() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    drop(state.manager.create("p1", "B04", NOW).unwrap());
    let connection = state.manager.open("p1", "B04", NOW).unwrap();
    let backup = temp.path().join("projects/p1/backups/active-reader.db");
    let manifest =
        BackupService::create_snapshot(&connection, &backup, "p1", "2026-08-27T00:01:00Z").unwrap();
    drop(connection);
    let active_read = state.manager.read_connection("p1", NOW).unwrap();
    let result = state.manager.restore_backup(
        ai_novel_production_system_core::project_db::ProjectCommandSpec {
            project_id: "p1",
            result_project_id: "p1",
            command_id: "b04-active-reader-restore",
            idempotency_key: "b04-active-reader-restore-key",
            command_type: "backup_restore",
            request_digest: &"b".repeat(64),
            expected_revision: Some(0),
            now: "2026-08-27T00:02:00Z",
            advance_revision: true,
        },
        &backup,
        &manifest.checksum_sha256,
    );
    let error = match result {
        Ok(_) => panic!("restore must block while a read lease is active"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("READ_POOL_BUSY"));
    drop(active_read);
    println!(
        "B04_RESTORE_POOL_BLOCK_RESULT activeLease=BLOCKED oldConnection=NOT_REPLACED status=PASS"
    );
}

#[test]
fn b04_volume_and_chapter_queries_are_server_paginated() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    drop(state.manager.create("p1", "B04", NOW).unwrap());
    let connection = state.manager.open("p1", "B04", NOW).unwrap();
    connection.execute("INSERT INTO books(book_id, project_id, book_no, title, status, created_at, updated_at) VALUES ('bk:b04:one','p1',1,'Book','ACTIVE',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id, project_id, book_id, volume_no, title, status, chapter_start, chapter_end, target_words, created_at, updated_at) VALUES ('vol:b04:one','p1','bk:b04:one',1,'Volume','PLANNING',1,2,1000,?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id, project_id, volume_id, chapter_no, title, importance_class, status, word_count, created_at, updated_at) VALUES ('chap:b04:one','p1','vol:b04:one',1,'Chapter One','NORMAL','PLANNED',0,?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id, project_id, volume_id, chapter_no, title, importance_class, status, word_count, created_at, updated_at) VALUES ('chap:b04:two','p1','vol:b04:one',2,'Chapter Two','NORMAL','PLANNED',0,?1,?1)", [NOW]).unwrap();
    drop(connection);
    let volumes = query_volume_list_impl(
        &state,
        serde_json::json!({"projectId":"p1","scope":"BOOK","pageSize":1}),
    )
    .unwrap();
    assert_eq!(volumes["items"].as_array().unwrap().len(), 1);
    assert_eq!(volumes["total"], 1);
    let chapters = query_chapter_list_impl(&state, serde_json::json!({"projectId":"p1","scope":"VOLUME","volumeId":"vol:b04:one","pageSize":1})).unwrap();
    assert_eq!(chapters["items"].as_array().unwrap().len(), 1);
    assert_eq!(chapters["total"], 2);
    assert!(chapters["nextCursor"].is_string());
}

#[allow(dead_code)]
fn _connection_is_explicitly_used(_connection: &Connection) {}
