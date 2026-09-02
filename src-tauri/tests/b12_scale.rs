use ai_novel_production_system_core::change_retcon;
use ai_novel_production_system_core::commands::{execute_command_impl, KernelState};
use ai_novel_production_system_core::production::UnavailableProductionGateway;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repositories::change_retcon::rebuild_global_dependency_summary;
use ai_novel_production_system_core::repositories::ops::{FixtureSpec, LongBookFixture};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

const NOW: &str = "2026-09-02T05:00:00Z";

#[test]
#[ignore = "B12 release-scale certification only"]
fn b12_three_million_dependency_preflight_is_bounded_without_full_graph_expansion() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let state =
        KernelState::with_production_gateway(paths.clone(), Arc::new(UnavailableProductionGateway));
    let connection = state
        .manager
        .create("p-b12-scale", "B12 scale", NOW)
        .unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys=OFF; PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF",
        )
        .unwrap();
    let manifest = LongBookFixture::materialize(
        &connection,
        &FixtureSpec {
            seed: "b12-release-scale".into(),
            books: 1,
            volumes: 1,
            chapters: 1,
            artifacts: 1,
            canon_facts: 0,
            dependencies: 3_000_000,
            model_runs: 0,
            target_words: 10_000,
        },
        NOW,
    )
    .unwrap();
    let summary_started = Instant::now();
    let summary = rebuild_global_dependency_summary(&connection, "p-b12-scale").unwrap();
    let summary_ms = summary_started.elapsed().as_secs_f64() * 1000.0;
    connection
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON")
        .unwrap();
    let dependency_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM dependency_edges WHERE project_id='p-b12-scale'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);

    execute_command_impl(
        &state,
        json!({
            "type":"change_request_create",
            "commandId":"cmd:b12:scale:create",
            "idempotencyKey":"key:b12:scale:create",
            "projectId":"p-b12-scale",
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{"changeRequestId":"change:b12:scale","changeType":"CHANGE","scope":"GLOBAL","summary":"规模认证影响预览","requestedChange":{"target":"全局规划"}}
        }),
    )
    .unwrap();
    let started = Instant::now();
    let preflight = execute_command_impl(
        &state,
        json!({
            "type":"impact_preflight",
            "commandId":"cmd:b12:scale:preflight",
            "idempotencyKey":"key:b12:scale:preflight",
            "projectId":"p-b12-scale",
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{"changeRequestId":"change:b12:scale"}
        }),
    )
    .unwrap();
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    let result = &preflight["result"];
    assert_eq!(manifest.counts.dependencies, 3_000_000);
    assert_eq!(dependency_count, 3_000_000);
    assert_eq!(summary.total_edges, 3_000_000);
    assert_eq!(result["scopeClass"], "GLOBAL");
    assert_eq!(result["totalEdges"], 3_000_000);
    assert_eq!(result["expandedEdges"], 0);
    assert_eq!(result["preflightOnly"], true);
    assert!(
        elapsed_ms <= 2_000.0,
        "B12 preflight target exceeded: {elapsed_ms:.2}ms"
    );
    println!(
        "B12_SCALE_CERTIFICATION_RESULT dependencies={} expanded={} summaryMs={:.2} preflightMs={:.2} status=PASS",
        dependency_count, result["expandedEdges"], summary_ms, elapsed_ms
    );
    let _ = change_retcon::OWNER_NAME;
}
