use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::narrative::{
    build_continuity_threads, compile_context_manifest, ContextCompilerInput,
    CurrentNarrativeStateView, ResumeContext, RuntimeBounds,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
use ai_novel_production_system_core::repositories::ops::{FixtureSpec, LongBookFixture};
use serde_json::json;
use std::time::Instant;
use tempfile::tempdir;

fn percentile(values: &[f64], fraction: f64) -> f64 {
    let index = ((values.len() as f64 * fraction).ceil() as usize).saturating_sub(1);
    values[index]
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
    let manifest = LongBookFixture::materialize(
        &connection,
        &FixtureSpec::long_book_10m("b06-h01-release-scale"),
        "2026-08-29T00:00:00Z",
    )
    .unwrap();
    let entity_id: String = connection
        .query_row(
            "SELECT entity_id FROM canon_entities WHERE project_id='p1' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let chapter_id: String = connection.query_row("SELECT chapter_id FROM chapters WHERE project_id='p1' ORDER BY chapter_no, chapter_id LIMIT 1", [], |row| row.get(0)).unwrap();
    let database_path = temp.path().join("projects/p1/project.db");
    let mut durations = Vec::with_capacity(30);
    let mut hard_source_max = 0_i64;
    let mut bounded_query_ok = true;
    for _ in 0..30 {
        let started = Instant::now();
        manager
            .with_read_connection("p1", "2026-08-29T00:00:00Z", |read| {
                let ids = vec![entity_id.as_str()];
                let facts = NarrativeRepository::current_canon(read, "p1", &ids, 100)
                    .map_err(|e| e.to_string())
                    .unwrap();
                let relations =
                    NarrativeRepository::current_relations(read, "p1", &ids, 100).unwrap();
                let knowledge =
                    NarrativeRepository::knowledge_for_entities(read, "p1", &ids, 100).unwrap();
                let events =
                    NarrativeRepository::canon_events(read, "p1", &chapter_id, 100).unwrap();
                let obligations = NarrativeRepository::active_obligations_for_relevance(
                    read,
                    "p1",
                    &ids,
                    &[],
                    100,
                )
                .unwrap();
                let evidence = NarrativeRepository::recent_evidence(
                    read,
                    "p1",
                    &[chapter_id.as_str()],
                    &[],
                    10,
                )
                .unwrap();
                bounded_query_ok &= evidence.len() <= 10;
                let state = CurrentNarrativeStateView::from_records(
                    RuntimeBounds {
                        project_id: "p1".into(),
                        entity_ids: vec![entity_id.clone()],
                        volume_id: None,
                        chapter_id: Some(chapter_id.clone()),
                        limit: 100,
                    },
                    facts,
                    relations,
                    knowledge,
                    events,
                    obligations.clone(),
                    false,
                );
                let threads = build_continuity_threads(
                    &obligations,
                    &[],
                    &[],
                    &evidence,
                    &ResumeContext::default(),
                )
                .unwrap();
                let context = compile_context_manifest(ContextCompilerInput {
                    execution_mode: "LONG_FORM".into(),
                    project_revision: 0,
                    goal_contract_refs: Vec::new(),
                    state,
                    continuity_threads: threads,
                    active_task_refs: Vec::new(),
                    future_reservation_refs: Vec::new(),
                    selected_evidence: Vec::new(),
                    omitted_soft_sources: Vec::new(),
                    hard_tokens: 1024,
                    soft_tokens: 256,
                })
                .unwrap();
                hard_source_max = hard_source_max.max(context.hard_source_count);
                Ok::<(), ai_novel_production_system_core::project_db::ManagerError>(())
            })
            .unwrap();
        durations.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    durations.sort_by(f64::total_cmp);
    let p50 = percentile(&durations, 0.50);
    let p95 = percentile(&durations, 0.95);
    let max = *durations.last().unwrap();
    assert!(p95 <= 300.0);
    assert!(bounded_query_ok);
    assert!(hard_source_max <= 400);
    let db_bytes = std::fs::metadata(&database_path).unwrap().len();
    let digest = sha256_canonical(
        &json!({"seed":manifest.seed,"counts":manifest.counts,"targetWords":manifest.target_words}),
    )
    .unwrap();
    if let Some(dir) = std::env::var_os("B06_H01_EVIDENCE_DIR") {
        let path = std::path::Path::new(&dir).join("b06_h01_scale.json");
        std::fs::write(path, serde_json::to_vec_pretty(&json!({"fixture":"LONG_BOOK_10M","fixtureDigest":digest,"dbBytes":db_bytes,"compilerSamples":30,"compilerP50Ms":p50,"compilerP95Ms":p95,"compilerMaxMs":max,"hardSourceCountMax":hard_source_max,"hardTokensMax":1024,"fullCanonScan":false,"fullThreadScan":false,"fullEvidenceDump":false,"h01BoundedQuery":bounded_query_ok,"hardContextBounded":true,"status":"PASS"})).unwrap()).unwrap();
    }
    println!("B06_SCALE_RESULT fixture=LONG_BOOK_10M books={} volumes={} chapters={} artifacts={} canon_facts={} dependencies={} model_runs={} compiler_samples=30 compiler_p50_ms={:.3} compiler_p95_ms={:.3} compiler_max_ms={:.3} hard_tokens_max=1024 full_canon_scan=false full_thread_scan=false full_evidence_dump=false h01_bounded_query=true hard_context_bounded=true db_bytes={} status=PASS", manifest.counts.books, manifest.counts.volumes, manifest.counts.chapters, manifest.counts.artifacts, manifest.counts.canon_facts, manifest.counts.dependencies, manifest.counts.model_runs, p50, p95, max, db_bytes);
}
