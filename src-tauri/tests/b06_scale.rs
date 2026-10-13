use ai_novel_production_system_core::commands::{
    query_context_manifest_impl, query_volume_progress_impl, support_session_impl, KernelState,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
use ai_novel_production_system_core::repositories::ops::{
    deep_integrity_check, FixtureSpec, LongBookFixture, PerformanceHarness, SearchService,
};
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Instant;
use tempfile::tempdir;

fn percentile(values: &[f64], fraction: f64) -> f64 {
    let index = ((values.len() as f64 * fraction).ceil() as usize).saturating_sub(1);
    values[index]
}

fn explain_details(connection: &Connection, sql: &str) -> String {
    let mut statement = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap();
    statement
        .query_map([], |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join(" | ")
}

fn insert_artifact(
    connection: &Connection,
    artifact_id: &str,
    artifact_type: &str,
    scope_level: &str,
    scope_key: &str,
    payload: &Value,
    now: &str,
) -> String {
    let serialized = serde_json::to_string(payload).unwrap();
    let payload_hash = hex::encode(Sha256::digest(serialized.as_bytes()));
    connection.execute("INSERT INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, semantic_lifecycle, authority_level, payload_hash, provenance_json, created_at, committed_at) VALUES (?1, 'p1', ?2, ?3, ?4, 1, 'm0e.fixture.v1', 'COMMITTED', 'CANON', 'B', ?5, '{}', ?6, ?6)", libsql_rusqlite::params![artifact_id, artifact_type, scope_level, scope_key, payload_hash, now]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id, version, payload_json, payload_hash) VALUES (?1, 1, ?2, ?3)", libsql_rusqlite::params![artifact_id, serialized, payload_hash]).unwrap();
    payload_hash
}

fn insert_head(
    connection: &Connection,
    artifact_type: &str,
    scope_level: &str,
    scope_key: &str,
    artifact_id: &str,
    payload_hash: &str,
) {
    connection.execute("INSERT INTO canonical_heads(project_id, artifact_type, scope_level, scope_key, artifact_id, version, payload_hash) VALUES ('p1', ?1, ?2, ?3, ?4, 1, ?5)", libsql_rusqlite::params![artifact_type, scope_level, scope_key, artifact_id, payload_hash]).unwrap();
}

fn seed_support_topology(connection: &Connection, volume_id: &str, chapter_id: &str, now: &str) {
    let entity_id: String = connection
        .query_row(
            "SELECT entity_id FROM canon_entities WHERE project_id='p1' ORDER BY entity_id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let second_entity = "entity:b06-scale-second";
    connection.execute("INSERT INTO canon_entities(entity_id, project_id, canonical_name, entity_type, first_chapter_id, created_at, updated_at) VALUES (?1, 'p1', 'Scale Second Entity', 'PERSON', ?2, ?3, ?3)", libsql_rusqlite::params![second_entity, chapter_id, now]).unwrap();
    let evidence_artifact = "artifact:b06-scale-evidence";
    let _evidence_hash = insert_artifact(
        connection,
        evidence_artifact,
        "FixtureEvidence",
        "CHAPTER",
        chapter_id,
        &json!({"kind":"evidence","body":"scale evidence"}),
        now,
    );
    let fact_id = "fact:b06-scale-identity";
    connection.execute("INSERT INTO canon_facts(fact_id, project_id, subject_id, predicate, object_json, source_artifact_id, source_version, source_chapter_id, authority_level, created_at, updated_at) VALUES (?1, 'p1', ?2, 'identity', '{\"value\":\"scale\"}', ?3, 1, ?4, 'B', ?5, ?5)", libsql_rusqlite::params![fact_id, entity_id, evidence_artifact, chapter_id, now]).unwrap();
    let event_id = "event:b06-scale-promise";
    connection.execute("INSERT INTO canon_events(event_id, project_id, chapter_id, event_type, subject_id, object_json, story_time, payload_json, created_at, event_ordinal, story_time_end, source_artifact_id, source_version) VALUES (?1, 'p1', ?2, 'REVEAL', ?3, '{\"factId\":\"fact:b06-scale-identity\"}', '2030-01-01T00:00:00Z', '{}', ?4, 1, '2030-01-01T00:00:00Z', ?5, 1)", libsql_rusqlite::params![event_id, chapter_id, entity_id, now, evidence_artifact]).unwrap();
    connection.execute("INSERT INTO canon_knowledge(knowledge_id, project_id, knower_id, fact_id, epistemic_state, visibility_scope, from_chapter_id, created_at, updated_at) VALUES ('knowledge:b06-scale', 'p1', ?1, ?2, 'KNOWS', 'CHAPTER', ?3, ?4, ?4)", libsql_rusqlite::params![entity_id, fact_id, chapter_id, now]).unwrap();
    connection.execute("INSERT INTO canon_evidence(evidence_id, project_id, fact_id, event_id, final_chapter_id, source_artifact_id, source_version, evidence_type, quote, range_start, range_end, created_at) VALUES ('evidence:b06-scale', 'p1', ?1, ?2, ?3, ?4, 1, 'EVENT', 'scale evidence', 1, 14, ?5)", libsql_rusqlite::params![fact_id, event_id, chapter_id, evidence_artifact, now]).unwrap();
    let obligation_id = "obligation:b06-scale";
    let condition =
        json!({"kind":"NEXT_MEETING","value":{"entityA":entity_id,"entityB":second_entity}})
            .to_string();
    connection.execute("INSERT INTO narrative_obligations(obligation_id, project_id, scope, obligation_type, authority_level, source_artifact_id, source_version, description, lifecycle, last_touched_chapter_id, created_at, updated_at, source_event_id, resume_condition_json) VALUES (?1, 'p1', 'VOLUME', 'PROMISE', 'B', ?2, 1, 'scale promise', 'APPROVED', ?3, ?4, ?4, ?5, ?6)", libsql_rusqlite::params![obligation_id, evidence_artifact, chapter_id, now, event_id, condition]).unwrap();
    connection.execute("INSERT INTO dependency_edges(edge_id, project_id, source_ref, target_ref, dependency_type, scope, impact_class, source_hash, entity_id, created_by, created_at) VALUES ('edge:b06-scale-obligation', 'p1', ?1, ?2, 'OBLIGATION_INPUT', 'VOLUME', 'EVIDENCE', ?3, ?4, 'B06_SCALE', ?5)", libsql_rusqlite::params![obligation_id, event_id, "0".repeat(64), entity_id, now]).unwrap();
    connection.execute("INSERT INTO state_deltas(state_id, project_id, state_type, value_json, chapter_id, payload_json, created_at, updated_at) VALUES ('task:b06-scale', 'p1', 'volume.task.progress', '{\"status\":\"ACTIVE\"}', ?1, '{}', ?2, ?2)", libsql_rusqlite::params![chapter_id, now]).unwrap();
    let book_hash = insert_artifact(
        connection,
        "artifact:b06-scale-book-plan",
        "BookPlan",
        "PROJECT",
        "p1",
        &json!({"payloadSchema":"book-plan.r1","planningMaturity":"APPROVED","storyPromise":"推进实际任务","protagonistMacroDirection":"主动查证","endingBoundary":"基于证据解决当前问题","mustAchieve":[],"mustNotBreak":[],"relationshipMacroDirections":[],"majorMysteryGoals":[],"futureReservations":[{"reservationId":"reservation:b06-scale-book","description":"scale reservation","prerequisites":[],"mayForeshadow":true,"mayReveal":false,"authorityLevel":"B"}],"volumeIntentSeeds":[],"openingHypotheses":[],"globalLengthBudget":{"preferredWords":10000000}}),
        now,
    );
    insert_head(
        connection,
        "BookPlan",
        "PROJECT",
        "p1",
        "artifact:b06-scale-book-plan",
        &book_hash,
    );
    let intent_hash = insert_artifact(
        connection,
        "artifact:b06-scale-volume-intent",
        "VolumeIntent",
        "VOLUME",
        volume_id,
        &json!({"payloadSchema":"volume-intent.r1","volumeFunction":"scale volume","entryConditions":[],"mustAchieve":[],"mustNotBreak":[],"requiredExitConditions":[],"futureReservationRefs":[],"crossVolumeResponsibilityRefs":[],"targetChapterRange":{"min":1,"preferred":30,"max":30},"targetWordRange":{"preferred":75000}}),
        now,
    );
    insert_head(
        connection,
        "VolumeIntent",
        "VOLUME",
        volume_id,
        "artifact:b06-scale-volume-intent",
        &intent_hash,
    );
    let plan_hash = insert_artifact(
        connection,
        "artifact:b06-scale-volume-plan",
        "VolumePlan",
        "VOLUME",
        volume_id,
        &json!({"payloadSchema":"volume-plan.r1","volumeIntentRef":{"artifactId":"artifact:b06-scale-volume-intent","version":1,"payloadHash":intent_hash},"routeVersion":1,"tasks":[{"taskId":"task:b06-scale","class":"REQUIRED","objective":"scale task","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"DONE"},"estimatedChapterCost":{"preferred":1}}],"routeInvariants":[],"requiredExitConditions":[],"capacityBudget":{"minChapters":1,"preferredChapters":30,"maxChapters":30}}),
        now,
    );
    insert_head(
        connection,
        "VolumePlan",
        "VOLUME",
        volume_id,
        "artifact:b06-scale-volume-plan",
        &plan_hash,
    );
    let story_hash = insert_artifact(
        connection,
        "artifact:b06-scale-story",
        "StoryConstitution",
        "PROJECT",
        "p1",
        &json!({"payloadSchema":"story-constitution.r1","storyCore":{"protagonist":"主角","desire":"查明事实","mechanism":"每项判断依赖可核验事实","conflict":"证据与隐瞒冲突","readerPromise":"每章推进真实任务","worldRules":[],"forbiddenReveals":[]}}),
        now,
    );
    insert_head(
        connection,
        "StoryConstitution",
        "PROJECT",
        "p1",
        "artifact:b06-scale-story",
        &story_hash,
    );
    let main_hash = insert_artifact(
        connection,
        "artifact:b06-scale-main",
        "MainContract",
        "PROJECT",
        "p1",
        &json!({"payloadSchema":"main-contract.r1","storyConstitutionRef":{"artifactId":"artifact:b06-scale-story","version":1,"payloadHash":story_hash},"scope":"BOOK","hardRules":["依据正式证据推进"],"softPreferences":[]}),
        now,
    );
    insert_head(
        connection,
        "MainContract",
        "PROJECT",
        "p1",
        "artifact:b06-scale-main",
        &main_hash,
    );
    let _: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM canon_evidence WHERE project_id='p1' AND source_artifact_id=?1",
            [evidence_artifact],
            |row| row.get(0),
        )
        .unwrap();
}

#[test]
#[ignore = "release-scale certification only"]
fn b06_h01_long_book_context_compiler_and_bounded_query_certification() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let manager = ProjectManager::new(paths.clone());
    let connection = manager
        .create("p1", "B06/H01 LONG_BOOK_10M", "2026-08-29T00:00:00Z")
        .unwrap();
    // The fixture is an offline build step. Keep the certified runtime profile
    // FULL, but avoid one fsync per fixture chunk while creating the temporary
    // database; all reads, FTS backfill, integrity and benchmarks run after FULL
    // is restored.
    connection
        .execute_batch(
            "PRAGMA foreign_keys=OFF; PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF",
        )
        .unwrap();
    let manifest = LongBookFixture::materialize(
        &connection,
        &FixtureSpec::long_book_10m("b06-h01-release-scale"),
        "2026-08-29T00:00:00Z",
    )
    .unwrap();
    connection
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON")
        .unwrap();
    let mut foreign_key_check = connection.prepare("PRAGMA foreign_key_check").unwrap();
    assert!(foreign_key_check
        .query_map([], |_| Ok(()))
        .unwrap()
        .next()
        .is_none());
    drop(foreign_key_check);
    let entity_id: String = connection
        .query_row(
            "SELECT entity_id FROM canon_entities WHERE project_id='p1' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let chapter_id: String = connection.query_row("SELECT chapter_id FROM chapters WHERE project_id='p1' ORDER BY chapter_no, chapter_id LIMIT 1", [], |row| row.get(0)).unwrap();
    let volume_id: String = connection
        .query_row(
            "SELECT volume_id FROM chapters WHERE chapter_id=?1",
            [&chapter_id],
            |row| row.get(0),
        )
        .unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    seed_support_topology(&connection, &volume_id, &chapter_id, "2026-08-29T00:00:00Z");
    connection.execute_batch("COMMIT").unwrap();
    let final_body_chars: i64 = connection.query_row("SELECT COALESCE(SUM(length(json_extract(p.payload_json, '$.body'))),0) FROM chapters c JOIN artifact_payloads p ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version WHERE c.project_id='p1'", [], |row| row.get(0)).unwrap();
    assert!(final_body_chars >= manifest.target_words as i64);
    let initial_backfill =
        SearchService::ensure_projection(&connection, "p1", "2026-08-29T00:00:00Z").unwrap();
    assert!(
        initial_backfill,
        "B03 to B04 must perform a real first backfill"
    );
    let search = SearchService::search(&connection, "p1", "rareTerm3999", None, 10).unwrap();
    assert!(
        search.total >= 1,
        "scale FTS query must hit materialized final text"
    );
    let canon_plan = explain_details(&connection, &format!("SELECT fact_id FROM canon_facts WHERE project_id='p1' AND subject_id='{entity_id}' AND superseded_by IS NULL LIMIT 100"));
    let evidence_plan = explain_details(&connection, "SELECT evidence_id FROM canon_evidence WHERE project_id='p1' AND final_chapter_id=(SELECT chapter_id FROM chapters WHERE project_id='p1' LIMIT 1) LIMIT 100");
    let obligation_plan = explain_details(&connection, "SELECT obligation_id FROM narrative_obligations WHERE project_id='p1' AND lifecycle IN ('APPROVED','PLANTED','PROGRESSING') LIMIT 100");
    let query_plan_measured =
        !canon_plan.is_empty() && !evidence_plan.is_empty() && !obligation_plan.is_empty();
    let full_canon_scan = canon_plan.contains("SCAN canon_facts");
    let full_evidence_dump = evidence_plan.contains("SCAN canon_evidence");
    let full_thread_scan = obligation_plan.contains("SCAN narrative_obligations");
    assert!(query_plan_measured);
    assert!(
        !full_canon_scan && !full_evidence_dump && !full_thread_scan,
        "bounded scale reads must use indexed plans"
    );
    let database_path = temp.path().join("projects/p1/project.db");
    let mut durations = Vec::with_capacity(30);
    let mut hard_serialized_bytes_max = 0_usize;
    let mut hard_tokens_max = 0_i64;
    let mut bounded_query_ok = true;
    let mut hard_budget_blocked_samples = 0_usize;
    let mut observed_budget_blocker = None::<String>;
    for _ in 0..30 {
        let started = Instant::now();
        manager.with_read_connection("p1", "2026-08-29T00:00:00Z", |read| {
            let ids = vec![entity_id.as_str()];
            let facts = NarrativeRepository::current_canon(read, "p1", &ids, 100).unwrap();
            let relations = NarrativeRepository::current_relations(read, "p1", &ids, 100).unwrap();
            let knowledge = NarrativeRepository::knowledge_for_entities(read, "p1", &ids, 100).unwrap();
            let events = NarrativeRepository::canon_events(read, "p1", &chapter_id, 100).unwrap();
            let obligations = NarrativeRepository::active_obligations_for_relevance(read, "p1", &ids, &[], 100).unwrap();
            let evidence = NarrativeRepository::recent_evidence(read, "p1", &[chapter_id.as_str()], &[], 10).unwrap();
            bounded_query_ok &= facts.len() <= 100 && relations.len() <= 100 && knowledge.len() <= 100 && events.len() <= 100 && obligations.len() <= 100 && evidence.len() <= 10;
            let state = ai_novel_production_system_core::narrative::CurrentNarrativeStateView::from_records(ai_novel_production_system_core::narrative::RuntimeBounds { project_id:"p1".into(), entity_ids:vec![entity_id.clone()], volume_id:Some(volume_id.clone()), chapter_id:Some(chapter_id.clone()), limit:100 }, facts, relations, knowledge, events, obligations.clone(), false);
            let threads = ai_novel_production_system_core::narrative::build_continuity_threads(&obligations, &[], &[], &evidence, &Default::default()).unwrap();
            let input = ai_novel_production_system_core::narrative::ContextCompilerInput { execution_mode:"LONG_FORM".into(), project_revision:0, goal_contract_refs:Vec::new(), goal_contract_fields:Vec::new(), state, continuity_threads:threads, plan_task_refs:Vec::new(), plan_task_contexts:Vec::new(), task_states:Vec::new(), active_task_refs:Vec::new(), future_reservation_refs:Vec::new(), selected_evidence:Vec::new(), omitted_soft_sources:Vec::new(), hard_tokens:1024, soft_tokens:256 };
            let usage = ai_novel_production_system_core::narrative::measure_context_hard_budget(&input).unwrap();
            hard_serialized_bytes_max = hard_serialized_bytes_max.max(usage.serialized_bytes);
            hard_tokens_max = hard_tokens_max.max(usage.estimated_tokens);
            let blocker = ai_novel_production_system_core::narrative::compile_context_manifest(input).unwrap_err();
            assert!(blocker.starts_with("CONTEXT_HARD_BUDGET_EXCEEDED"));
            if let Some(previous) = &observed_budget_blocker {
                assert_eq!(previous, &blocker, "the same 10M context must block deterministically");
            } else {
                observed_budget_blocker = Some(blocker);
            }
            hard_budget_blocked_samples += 1;
            Ok::<(), ai_novel_production_system_core::project_db::ManagerError>(())
        }).unwrap();
        durations.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    durations.sort_by(f64::total_cmp);
    let p50 = percentile(&durations, 0.50);
    let p95 = percentile(&durations, 0.95);
    let max = *durations.last().unwrap();
    // FX-11: the hard budget blocks on whichever limit is exceeded (tokens or
    // serialized bytes); requiring the byte limit specifically mis-attributed
    // a token-limit block as "not enforced".
    let hard_budget_exceeded = hard_tokens_max
        > ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT
        || hard_serialized_bytes_max
            > ai_novel_production_system_core::narrative::CONTEXT_HARD_SERIALIZED_BYTE_LIMIT;
    let hard_budget_enforced = hard_budget_blocked_samples == 30
        && hard_budget_exceeded
        && observed_budget_blocker.is_some();
    println!("B06_H01_SCALE_DIAGNOSTIC p50_ms={p50:.3} p95_ms={p95:.3} max_ms={max:.3} bounded_query_ok={bounded_query_ok} hard_budget_enforced={hard_budget_enforced} hard_serialized_bytes_max={hard_serialized_bytes_max} limit={} hard_tokens_max={hard_tokens_max} token_limit={} hard_budget_blocked_samples={hard_budget_blocked_samples}", ai_novel_production_system_core::narrative::CONTEXT_HARD_SERIALIZED_BYTE_LIMIT, ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT);
    assert!(p95 <= 300.0 && bounded_query_ok && hard_budget_enforced);
    drop(connection);
    #[cfg(debug_assertions)]
    let kernel = KernelState::new(paths.clone());
    #[cfg(not(debug_assertions))]
    let kernel = KernelState::for_scale_bench(paths.clone());
    let context_error = query_context_manifest_impl(&kernel, json!({"projectId":"p1","volumeId":volume_id,"chapterId":chapter_id,"entityIds":[entity_id],"references":["fact:b06-scale-identity","event:b06-scale-promise","artifact:b06-scale-evidence"],"includeSoftSources":true})).unwrap_err();
    let context_blocked_by_hard_gate = context_error.contains("CONTEXT_HARD_BUDGET_EXCEEDED")
        || context_error.contains("CONTEXT_HARD_INCOMPLETE")
        || context_error.contains("CONTEXT_SOURCE_UNAVAILABLE");
    assert!(
        context_blocked_by_hard_gate,
        "10M formal Context must fail closed with a typed HARD reason: {context_error}"
    );
    let context_manifest_count: i64 = kernel.manager.with_read_connection("p1", "2026-08-29T00:00:00Z", |read| {
        read.query_row("SELECT COUNT(*) FROM artifacts WHERE project_id='p1' AND artifact_type='ContextManifest'", [], |row| row.get(0))
            .map_err(|error| ai_novel_production_system_core::project_db::ManagerError::ProjectCommand(error.to_string()))
    }).unwrap();
    assert_eq!(context_manifest_count, 0);
    let progress = query_volume_progress_impl(
        &kernel,
        json!({"projectId":"p1","volumeId":volume_id,"currentChapter":1}),
    )
    .unwrap();
    assert_eq!(
        progress["routePlanRef"]["artifactId"],
        "artifact:b06-scale-volume-plan"
    );
    let support = support_session_impl(&kernel, json!({"projectId":"p1","question":"人物知道身份吗","mode":"DIAGNOSE","scope":"VOLUME","volumeId":volume_id,"chapterId":chapter_id,"entityIds":[entity_id],"references":["fact:b06-scale-identity","event:b06-scale-promise","artifact:b06-scale-evidence"]})).unwrap();
    assert_eq!(support["finding"]["supportStatus"], "SUPPORTED");
    assert_eq!(
        support["evidenceLedger"].as_array().unwrap().len(),
        support["calls"].as_array().unwrap().len()
    );
    assert!(support["evidenceLedger"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["sourceRefs"]
            .as_array()
            .is_some_and(|refs| !refs.is_empty())));
    let integrity_writer = kernel
        .manager
        .open("p1", "", "2026-08-29T00:00:00Z")
        .unwrap();
    let integrity = deep_integrity_check(&integrity_writer, "p1", "2026-08-29T00:00:00Z").unwrap();
    drop(integrity_writer);
    assert!(
        integrity.ok,
        "scale integrity failed: {:?}",
        integrity.failures
    );
    let standard_writer = kernel
        .manager
        .open("p1", "", "2026-08-29T00:00:00Z")
        .unwrap();
    let standard = PerformanceHarness::measure_standard(&standard_writer, "p1", 5).unwrap();
    drop(standard_writer);
    PerformanceHarness::assert_thresholds(&standard).unwrap();
    let search_report = kernel
        .manager
        .with_read_connection("p1", "2026-08-29T00:00:00Z", |read| {
            PerformanceHarness::measure_search(read, "p1", "rareTerm3999", 5).map_err(|error| {
                ai_novel_production_system_core::project_db::ManagerError::ProjectCommand(
                    error.to_string(),
                )
            })
        })
        .unwrap();
    assert!(search_report.p95_ms <= 1000.0);
    let db_bytes = std::fs::metadata(&database_path).unwrap().len();
    let topology = json!({"promiseRows":1,"knowledgeRows":1,"eventRows":1,"evidenceRows":1,"taskRows":1,"volumePlanRows":1});
    let digest = sha256_canonical(&json!({"seed":manifest.seed,"counts":manifest.counts,"targetWords":manifest.target_words,"topology":topology})).unwrap();
    let report = json!({"fixture":"LONG_BOOK_10M","fixtureDigest":digest,"dbBytes":db_bytes,"compilerSamples":30,"compilerP50Ms":p50,"compilerP95Ms":p95,"compilerMaxMs":max,"hardSerializedBytesMax":hard_serialized_bytes_max,"hardBudgetBlockedSamples":hard_budget_blocked_samples,"hardBudgetBlocker":observed_budget_blocker,"formalContextBlocker":context_error,"formalContextManifestCount":context_manifest_count,"fullCanonScan":full_canon_scan,"fullThreadScan":full_thread_scan,"fullEvidenceDump":full_evidence_dump,"queryPlanMeasured":query_plan_measured,"h01BoundedQuery":bounded_query_ok,"hardBudgetEnforced":hard_budget_enforced,"formalContextBlockedByHardGate":context_blocked_by_hard_gate,"materializedFinalBodyChars":final_body_chars,"initialBackfill":initial_backfill,"searchHitCount":search.total,"topology":topology,"standardPerformance":standard,"searchPerformance":search_report,"integrity":integrity,"status":"PASS"});
    if let Some(dir) = std::env::var_os("B06_H01_EVIDENCE_DIR") {
        std::fs::write(
            std::path::Path::new(&dir).join("b06_h01_scale.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
    println!("B06_SCALE_RESULT fixture=LONG_BOOK_10M books={} volumes={} chapters={} artifacts={} canon_facts={} dependencies={} model_runs={} compiler_samples=30 compiler_p50_ms={:.3} compiler_p95_ms={:.3} compiler_max_ms={:.3} hard_serialized_bytes_max={} hard_budget_blocked_samples={} formal_context_manifest_count={} full_canon_scan={} full_thread_scan={} full_evidence_dump={} query_plan_measured={} h01_bounded_query={} hard_budget_enforced={} promise_rows=1 knowledge_rows=1 event_rows=1 evidence_rows=1 task_rows=1 volume_plan_rows=1 db_bytes={} status=PASS", manifest.counts.books, manifest.counts.volumes, manifest.counts.chapters, manifest.counts.artifacts, manifest.counts.canon_facts, manifest.counts.dependencies, manifest.counts.model_runs, p50, p95, max, hard_serialized_bytes_max, hard_budget_blocked_samples, context_manifest_count, full_canon_scan, full_thread_scan, full_evidence_dump, query_plan_measured, bounded_query_ok, hard_budget_enforced, db_bytes);
}
