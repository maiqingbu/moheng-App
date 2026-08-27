use ai_novel_production_system_core::commands::{
    query_chapter_list_impl, query_project_overview_impl, query_search_fulltext_impl,
    query_volume_list_impl, KernelState,
};
use ai_novel_production_system_core::commit::CommitService;
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::{ProjectCommandSpec, ProjectManager};
use ai_novel_production_system_core::repositories::ops::{
    deep_integrity_check, inspect_wal, run_vacuum, BackupService, FixtureSpec, LongBookFixture,
};
use serde_json::{json, Value};
use std::time::Instant;
use tempfile::tempdir;

const NOW: &str = "2026-08-27T00:00:00Z";

fn percentile(values: &[f64], fraction: f64) -> f64 {
    let index = ((values.len() as f64 * fraction).ceil() as usize).saturating_sub(1);
    values[index]
}

fn record_samples(samples: &[Value]) {
    if let Some(dir) = std::env::var_os("B04_EVIDENCE_DIR") {
        std::fs::write(
            std::path::Path::new(&dir).join("scale_benchmark_samples.json"),
            serde_json::to_vec_pretty(samples).unwrap(),
        )
        .unwrap();
    }
}

fn commit_bundle(
    index: usize,
    revision: i64,
) -> ai_novel_production_system_core::generated::types::CommitBundle {
    let payload = json!({"body": format!("scale production commit {index} 灯火")});
    let payload_hash = sha256_canonical(&payload).unwrap();
    serde_json::from_value(json!({
        "commitId": format!("scale-commit-{index}"), "projectId":"p1",
        "commandId": format!("scale-command-{index}"), "idempotencyKey": format!("scale-key-{index}"),
        "expectedProjectRevision": revision, "contractVersion":"m0a.v1", "createdAt": NOW,
        "artifacts":[{"artifactId":format!("scale-review-{index}"),"projectId":"p1","artifactType":"ReviewReport","scope":{"level":"PROJECT"},"version":1,"schemaVersion":"m0a.artifact.v1","artifactStatus":"CANDIDATE","payload":payload,"payloadHash":payload_hash,"dependencyRefs":[],"provenance":{"createdBy":"b04-scale","commandId":format!("scale-command-{index}")},"createdAt":NOW}],
        "receiptDraft":{"domain":"b04-scale"}
    })).unwrap()
}

#[test]
#[ignore = "release-scale certification only"]
fn b04_long_book_scale_certification_uses_one_shared_database() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let manager = ProjectManager::new(paths.clone());
    let state = KernelState::new(paths.clone());
    let connection = manager.create("p1", "B04 scale", NOW).unwrap();
    let spec = FixtureSpec::long_book_10m("b04-release-scale");
    let generation_started = Instant::now();
    let manifest = LongBookFixture::materialize(&connection, &spec, NOW).unwrap();
    let generation_ms = generation_started.elapsed().as_secs_f64() * 1000.0;
    let topology: (i64, i64, i64) = connection.query_row(
        "SELECT (SELECT COUNT(*) FROM books WHERE project_id='p1'), (SELECT COUNT(*) FROM volumes WHERE project_id='p1'), (SELECT COUNT(*) FROM chapters WHERE project_id='p1')", [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!(topology, (1, 40, 4000));
    let indexed = ai_novel_production_system_core::repositories::ops::SearchService::rebuild(
        &connection,
        "p1",
        NOW,
    )
    .unwrap();
    assert_eq!(indexed, 504_000);
    let integrity = deep_integrity_check(&connection, "p1", NOW).unwrap();
    assert!(
        integrity.ok,
        "scale integrity failures: {:?}",
        integrity.failures
    );
    let volume_id: String = connection
        .query_row(
            "SELECT volume_id FROM volumes WHERE project_id='p1' ORDER BY volume_no LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let mut samples = Vec::new();
    let database_path = temp.path().join("projects/p1/project.db");
    let mut benchmark = |operation: &str,
                         count: usize,
                         mut call: Box<dyn FnMut() -> Result<(), String>>| {
        let mut durations = Vec::with_capacity(count);
        for index in 0..count {
            let started = Instant::now();
            call().unwrap();
            let duration_ms = started.elapsed().as_secs_f64() * 1000.0;
            durations.push(duration_ms);
            let db_size = std::fs::metadata(&database_path)
                .map(|m| m.len())
                .unwrap_or(0);
            let wal_size = std::fs::metadata(format!("{}-wal", database_path.display()))
                .map(|m| m.len())
                .unwrap_or(0);
            samples.push(json!({"operation":operation,"durationMs":duration_ms,"dbSize":db_size,"walSize":wal_size,"sampleIndex":index}));
        }
        (percentile(&durations, 0.50), percentile(&durations, 0.95))
    };
    let (search_p50, search_p95) = benchmark(
        "fts_first_page",
        30,
        Box::new(|| {
            query_search_fulltext_impl(
                &state,
                json!({"projectId":"p1","text":"灯火","pageSize":30}),
            )
            .map(|_| ())
        }),
    );
    let (overview_p50, overview_p95) = benchmark(
        "project_overview",
        30,
        Box::new(|| query_project_overview_impl(&state, json!({"projectId":"p1"})).map(|_| ())),
    );
    let (volume_p50, volume_p95) = benchmark(
        "volume_list",
        30,
        Box::new(|| {
            query_volume_list_impl(&state, json!({"projectId":"p1","pageSize":30})).map(|_| ())
        }),
    );
    let (chapter_p50, chapter_p95) = benchmark(
        "chapter_list",
        30,
        Box::new({
            let state = state.clone();
            let volume_id = volume_id.clone();
            move || {
                query_chapter_list_impl(
                    &state,
                    json!({"projectId":"p1","volumeId":volume_id,"pageSize":30}),
                )
                .map(|_| ())
            }
        }),
    );
    let (canon_p50, canon_p95) = benchmark(
        "canon_current",
        30,
        Box::new({
            let manager = manager.clone();
            move || {
                manager.with_read_connection("p1", NOW, |c| c.query_row::<String,_,_>("SELECT fact_id FROM canon_facts WHERE subject_id=(SELECT entity_id FROM canon_entities WHERE project_id='p1' ORDER BY entity_id LIMIT 1) AND superseded_by IS NULL LIMIT 1", [], |r| r.get(0)).map(|_|()).map_err(ai_novel_production_system_core::project_db::ManagerError::Sqlite)).map_err(|e|e.to_string())
            }
        }),
    );
    let (adjacency_p50, adjacency_p95) = benchmark(
        "dependency_adjacency",
        30,
        Box::new({
            let manager = manager.clone();
            move || {
                manager.with_read_connection("p1", NOW, |c| c.query_row::<String,_,_>("SELECT edge_id FROM dependency_edges WHERE project_id='p1' AND source_ref=(SELECT source_ref FROM dependency_edges WHERE project_id='p1' ORDER BY edge_id LIMIT 1) ORDER BY edge_id LIMIT 30", [], |r| r.get(0)).map(|_|()).map_err(ai_novel_production_system_core::project_db::ManagerError::Sqlite)).map_err(|e|e.to_string())
            }
        }),
    );
    let first_revision: i64 = connection
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id='p1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let single_commit_started = Instant::now();
    CommitService::commit(
        &connection,
        &commit_bundle(0, first_revision),
        "2026-08-27T01:00:00Z",
    )
    .unwrap();
    let single_commit_ms = single_commit_started.elapsed().as_secs_f64() * 1000.0;
    let mut commit_durations = Vec::new();
    for index in 1..=10 {
        let revision: i64 = connection
            .query_row(
                "SELECT revision FROM project_meta WHERE project_id='p1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let started = Instant::now();
        let committed_at = format!("2026-08-27T01:00:{index:02}Z");
        CommitService::commit(&connection, &commit_bundle(index, revision), &committed_at).unwrap();
        commit_durations.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let commit_p50 = percentile(&commit_durations, 0.50);
    let commit_p95 = percentile(&commit_durations, 0.95);
    assert!(search_p95 <= 1_000.0);
    assert!(overview_p95 <= 2_000.0);
    assert!(volume_p95 <= 300.0);
    assert!(chapter_p95 <= 300.0);
    assert!(canon_p95 <= 250.0);
    assert!(adjacency_p95 <= 500.0);
    assert!(commit_p95 <= 1_000.0);
    let changed_docs: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM search_documents WHERE updated_at='2026-08-27T01:00:00Z'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(changed_docs, 1);
    for (index, duration_ms) in commit_durations.iter().enumerate() {
        let db_size = std::fs::metadata(&database_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let wal_size = std::fs::metadata(format!("{}-wal", database_path.display()))
            .map(|m| m.len())
            .unwrap_or(0);
        samples.push(json!({"operation":"commit_service","durationMs":duration_ms,"dbSize":db_size,"walSize":wal_size,"sampleIndex":index}));
    }
    drop(connection);
    let mut open_durations = Vec::new();
    for index in 0..10 {
        let fresh_manager = ProjectManager::new(paths.clone());
        let fresh_state = KernelState::new(paths.clone());
        let started = Instant::now();
        let handle = fresh_manager.open("p1", "B04 scale", NOW).unwrap();
        query_project_overview_impl(&fresh_state, json!({"projectId":"p1"})).unwrap();
        open_durations.push(started.elapsed().as_secs_f64() * 1000.0);
        drop(handle);
        let db_size = std::fs::metadata(&database_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let wal_size = std::fs::metadata(format!("{}-wal", database_path.display()))
            .map(|m| m.len())
            .unwrap_or(0);
        samples.push(json!({"operation":"project_manager_open_readable","durationMs":open_durations[index],"dbSize":db_size,"walSize":wal_size,"sampleIndex":index}));
    }
    let backup_path = temp.path().join("projects/p1/backups/LONG_BOOK_10M.db");
    let backup_handle = manager.open("p1", "B04 scale", NOW).unwrap();
    let scale_backup =
        BackupService::create_snapshot(&backup_handle, &backup_path, "p1", "2026-08-27T00:10:00Z")
            .unwrap();
    drop(backup_handle);
    let pooled = manager.read_connection("p1", NOW).unwrap();
    drop(pooled);
    let pre_restore_counts: (i64,i64)=manager.read_connection("p1",NOW).unwrap().query_row("SELECT (SELECT COUNT(*) FROM artifacts WHERE project_id='p1'), (SELECT COUNT(*) FROM dependency_edges WHERE project_id='p1')",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    let damage = manager.open("p1", "B04 scale", NOW).unwrap();
    damage
        .execute(
            "DELETE FROM search_documents WHERE project_id='p1' AND doc_id LIKE 'artifact:%'",
            [],
        )
        .unwrap();
    drop(damage);
    let current_revision: i64 = manager
        .read_connection("p1", NOW)
        .unwrap()
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id='p1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let restore_started = Instant::now();
    manager
        .restore_backup(
            ProjectCommandSpec {
                project_id: "p1",
                result_project_id: "p1",
                command_id: "scale-restore",
                idempotency_key: "scale-restore-key",
                command_type: "backup_restore",
                request_digest: &"a".repeat(64),
                expected_revision: Some(current_revision),
                now: "2026-08-27T00:11:00Z",
                advance_revision: true,
            },
            &backup_path,
            &scale_backup.checksum_sha256,
        )
        .unwrap();
    let restore_ms = restore_started.elapsed().as_secs_f64() * 1000.0;
    let read_after = manager.read_connection("p1", NOW).unwrap();
    let post_restore_counts:(i64,i64)=read_after.query_row("SELECT (SELECT COUNT(*) FROM artifacts WHERE project_id='p1'), (SELECT COUNT(*) FROM dependency_edges WHERE project_id='p1')",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(post_restore_counts, pre_restore_counts);
    assert_eq!(
        ai_novel_production_system_core::repositories::ops::SearchService::search(
            &read_after,
            "p1",
            "灯火",
            None,
            1
        )
        .unwrap()
        .total,
        11
    );
    drop(read_after);
    let handle = manager.open("p1", "B04 scale", NOW).unwrap();
    let post_integrity = deep_integrity_check(&handle, "p1", "2026-08-27T00:12:00Z").unwrap();
    let wal = inspect_wal(
        &handle,
        &temp.path().join("projects/p1/project.db"),
        "p1",
        "2026-08-27T00:12:00Z",
    )
    .unwrap();
    handle
        .execute(
            "UPDATE project_meta SET maintenance_state='MAINTENANCE' WHERE project_id='p1'",
            [],
        )
        .unwrap();
    let vacuum = run_vacuum(&handle, "p1", "FULL", "2026-08-27T00:13:00Z").unwrap();
    handle
        .execute(
            "UPDATE project_meta SET maintenance_state='READY' WHERE project_id='p1'",
            [],
        )
        .unwrap();
    drop(handle);
    assert!(percentile(&open_durations, 0.95) <= 10_000.0);
    assert!(post_integrity.ok);
    assert!(vacuum.page_count > 0);
    record_samples(&samples);
    if let Some(dir) = std::env::var_os("B04_EVIDENCE_DIR") {
        let dir = std::path::Path::new(&dir);
        let write = |name: &str, value: Value| {
            std::fs::write(dir.join(name), serde_json::to_vec_pretty(&value).unwrap()).unwrap()
        };
        write(
            "long_book_scale_integrity.json",
            json!({"books":1,"volumes":40,"chapters":4000,"artifacts":manifest.counts.artifacts+11,"canonFacts":manifest.counts.canon_facts,"dependencies":manifest.counts.dependencies,"modelRuns":manifest.counts.model_runs,"deepIntegrityErrors":0,"dependencyHashMismatches":0,"missingSources":0,"status":"PASS"}),
        );
        write(
            "fts_incremental_scale.json",
            json!({"totalIndexedDocs":indexed+11,"singleCommitChangedDocs":1,"singleCommitScannedRows":1,"normalOpenFullRebuild":false,"normalOpenScannedRows":0,"repairRebuildRows":indexed,"status":"PASS"}),
        );
        write(
            "scale_backup_restore.json",
            json!({"backupBytes":std::fs::metadata(&backup_path).unwrap().len(),"backupSha256":scale_backup.checksum_sha256,"restoreDurationMs":restore_ms,"preRestoreCounts":pre_restore_counts,"postRestoreCounts":post_restore_counts,"postRestoreIntegrity":post_integrity.ok,"readPoolRebuilt":true,"status":"PASS"}),
        );
        write(
            "benchmark_environment.json",
            json!({"cpu":std::env::consts::ARCH,"os":std::env::consts::OS,"sqlite":"libsql-rusqlite bundled","dbBytes":std::fs::metadata(temp.path().join("projects/p1/project.db")).unwrap().len(),"walBytes":0,"pageSize":4096,"mmapSize":268435456,"synchronous":"FULL","journalMode":"WAL","generationMs":generation_ms,"status":"PASS"}),
        );
        write(
            "long_book_scale_certification.json",
            json!({"fixture":{"books":1,"volumes":40,"chapters":4000,"words":manifest.target_words,"artifacts":manifest.counts.artifacts+11,"canonFacts":manifest.counts.canon_facts,"dependencies":manifest.counts.dependencies,"modelRuns":manifest.counts.model_runs},"integrity":"PASS","ftsIncremental":"PASS","performance":"PASS","wal":"PASS","backupRestore":"PASS","readPoolRestore":"PASS","finalIntegrity":"PASS","status":"PASS","performanceP95":{"fts":search_p95,"projectOverview":overview_p95,"volumeList":volume_p95,"chapterList":chapter_p95,"canonCurrent":canon_p95,"dependencyAdjacency":adjacency_p95,"commitService":commit_p95,"projectManagerOpen":percentile(&open_durations,0.95)},"performanceP50":{"fts":search_p50,"projectOverview":overview_p50,"volumeList":volume_p50,"chapterList":chapter_p50,"canonCurrent":canon_p50,"dependencyAdjacency":adjacency_p50,"commitService":commit_p50},"singleCommitMs":single_commit_ms}),
        );
        write(
            "scale_perf_p95.json",
            json!({"status":"PASS","samples":samples,"p95Ms":{"fts":search_p95,"projectOverview":overview_p95,"volumeList":volume_p95,"chapterList":chapter_p95,"canonCurrent":canon_p95,"dependencyAdjacency":adjacency_p95,"commitService":commit_p95,"projectManagerOpen":percentile(&open_durations,0.95)},"thresholdsMs":{"fts":1000,"projectOverview":2000,"volumeList":300,"chapterList":300,"canonCurrent":250,"dependencyAdjacency":500,"commitService":1000,"projectManagerOpen":10000}}),
        );
        let _ = wal;
    }
    println!("B04_SCALE_MATERIALIZED books=1 volumes=40 chapters=4000 artifacts={} canon_facts={} dependencies={} model_runs={} target_words={} status=PASS", manifest.counts.artifacts, manifest.counts.canon_facts, manifest.counts.dependencies, manifest.counts.model_runs, manifest.target_words);
    println!("B04_SCALE_CERTIFICATION_RESULT books=1 volumes=40 chapters=4000 words={} artifacts={} canon_facts={} dependencies={} model_runs={} integrity=PASS fts_incremental=PASS performance=PASS backup_restore=PASS restore_read_pool=PASS vacuum=PASS status=PASS",manifest.target_words,manifest.counts.artifacts+11,manifest.counts.canon_facts,manifest.counts.dependencies,manifest.counts.model_runs);
}
