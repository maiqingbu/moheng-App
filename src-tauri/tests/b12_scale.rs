use ai_novel_production_system_core::change_retcon;
use ai_novel_production_system_core::commands::{execute_command_impl, KernelState};
use ai_novel_production_system_core::production::UnavailableProductionGateway;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repositories::change_retcon::rebuild_global_dependency_summary;
use ai_novel_production_system_core::repositories::ops::SearchService;
use ai_novel_production_system_core::repositories::ops::{FixtureSpec, LongBookFixture};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

const NOW: &str = "2026-09-02T05:00:00Z";

#[test]
#[ignore = "B12 release-scale certification only"]
fn b12_three_million_dependency_preflight_has_bounded_cold_and_warm_paths() {
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
    // The scale fixture models an already-opened project whose ordinary B04
    // search projection is current. This is separate from the B12 dependency
    // summary: the cold B12 measurement below deliberately has no dependency
    // summary and must return the bounded bootstrap response.
    SearchService::reconcile(&connection, "p-b12-scale", NOW).unwrap();
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
    let cold_started = Instant::now();
    let cold_preflight = execute_command_impl(
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
    let cold_ms = cold_started.elapsed().as_secs_f64() * 1000.0;
    let cold_result = &cold_preflight["result"];
    assert_eq!(manifest.counts.dependencies, 3_000_000);
    assert_eq!(dependency_count, 3_000_000);
    assert_eq!(cold_result["status"], "SUMMARY_BOOTSTRAP_REQUIRED");
    assert_eq!(cold_result["summaryStatus"], "NOT_READY");
    assert_eq!(cold_result["fullGraphSynchronousScan"], false);
    assert!(
        cold_ms <= 2_000.0,
        "B12 cold preflight target exceeded: {cold_ms:.2}ms"
    );
    let summary_started = Instant::now();
    let summary = state
        .manager
        .open("p-b12-scale", "", NOW)
        .map(|handle| {
            let result = rebuild_global_dependency_summary(&handle, "p-b12-scale");
            drop(handle);
            result
        })
        .unwrap()
        .unwrap();
    let summary_ms = summary_started.elapsed().as_secs_f64() * 1000.0;
    let warm_started = Instant::now();
    let warm_preflight = execute_command_impl(
        &state,
        json!({
            "type":"impact_preflight",
            "commandId":"cmd:b12:scale:preflight:warm",
            "idempotencyKey":"key:b12:scale:preflight:warm",
            "projectId":"p-b12-scale",
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{"changeRequestId":"change:b12:scale"}
        }),
    )
    .unwrap();
    let warm_ms = warm_started.elapsed().as_secs_f64() * 1000.0;
    let warm_result = &warm_preflight["result"];
    assert_eq!(summary.total_edges, 3_000_000);
    assert_eq!(warm_result["status"], "PREFLIGHTED");
    assert_eq!(warm_result["summaryStatus"], "READY");
    assert_eq!(warm_result["fullGraphSynchronousScan"], false);
    assert_eq!(warm_result["totalEdges"], 3_000_000);
    assert_eq!(warm_result["expandedEdges"], 0);
    assert!(
        warm_ms <= 2_000.0,
        "B12 warm preflight target exceeded: {warm_ms:.2}ms"
    );
    println!(
        "B12_SCALE_CERTIFICATION_RESULT dependencies={} coldStatus={} coldPreflightMs={:.2} expanded={} summaryMs={:.2} warmPreflightMs={:.2} fullGraphSynchronousScanCold={} fullGraphSynchronousScanWarm={} status=PASS",
        dependency_count, cold_result["status"], cold_ms, warm_result["expandedEdges"], summary_ms, warm_ms, cold_result["fullGraphSynchronousScan"], warm_result["fullGraphSynchronousScan"]
    );
    let _ = change_retcon::OWNER_NAME;
}
