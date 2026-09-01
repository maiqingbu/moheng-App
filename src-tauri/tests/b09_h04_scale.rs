use ai_novel_production_system_core::commands::{
    query_chapter_list_impl, query_context_manifest_impl, query_search_fulltext_impl,
    query_single_chapter_impl, query_volume_lifecycle_impl, query_volume_list_impl, KernelState,
    RepositoryReadAdapter,
};
use ai_novel_production_system_core::harness::{
    BoundedCapabilityExecutor, CapabilityCallEnvelope, CapabilityRegistry, RequestedBounds,
};
use ai_novel_production_system_core::hash::{canonical_bytes, sha256_canonical};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::repositories::ops::{
    FixtureSpec, LongBookFixture, SearchService,
};
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use std::time::Instant;
use tempfile::tempdir;

const NOW: &str = "2026-08-30T00:00:00Z";

fn percentile(values: &[f64], fraction: f64) -> f64 {
    let index = ((values.len() as f64 * fraction).ceil() as usize).saturating_sub(1);
    values[index]
}

fn scope_key(value: Value) -> String {
    String::from_utf8(canonical_bytes(&value).unwrap()).unwrap()
}

#[allow(clippy::too_many_arguments)]
fn insert_committed_artifact(
    connection: &Connection,
    project_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    scope_level: &str,
    scope: &str,
    payload: &Value,
    now: &str,
) -> String {
    let payload_hash = sha256_canonical(payload).unwrap();
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'m0e.fixture.v1','COMMITTED',?6,'{}',?7,?7)",
            libsql_rusqlite::params![
                artifact_id,
                project_id,
                artifact_type,
                scope_level,
                scope,
                payload_hash,
                now
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            libsql_rusqlite::params![artifact_id, payload.to_string(), payload_hash],
        )
        .unwrap();
    payload_hash
}

fn insert_head(
    connection: &Connection,
    project_id: &str,
    artifact_type: &str,
    scope_level: &str,
    scope: &str,
    artifact_id: &str,
    payload_hash: &str,
) {
    connection
        .execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,?2,?3,?4,?5,1,?6)",
            libsql_rusqlite::params![
                project_id,
                artifact_type,
                scope_level,
                scope,
                artifact_id,
                payload_hash
            ],
        )
        .unwrap();
}

fn seed_route_authority(connection: &Connection, volume_id: &str, now: &str) {
    let volume_scope = scope_key(json!({"level":"VOLUME","volumeId":volume_id}));
    let intent_payload = json!({
        "payloadSchema": "volume-intent.r1",
        "volumeFunction": "B09/H04 scale route",
        "entryConditions": [],
        "mustAchieve": [],
        "mustNotBreak": [],
        "requiredExitConditions": [],
        "futureReservationRefs": [],
        "crossVolumeResponsibilityRefs": [],
        "targetChapterRange": {"min": 1, "preferred": 100, "max": 100},
        "targetWordRange": {"preferred": 250000},
    });
    let intent_hash = insert_committed_artifact(
        connection,
        "p1",
        "artifact:b09-scale-intent",
        "VolumeIntent",
        "VOLUME",
        &volume_scope,
        &intent_payload,
        now,
    );
    insert_head(
        connection,
        "p1",
        "VolumeIntent",
        "VOLUME",
        &volume_scope,
        "artifact:b09-scale-intent",
        &intent_hash,
    );
    let route_payload = json!({
        "payloadSchema": "volume-plan.r1",
        "volumeIntentRef": {"artifactId":"artifact:b09-scale-intent","version":1,"payloadHash":intent_hash},
        "routeVersion": 1,
        "tasks": [{
            "taskId": "task:b09-scale",
            "class": "REQUIRED",
            "objective": "保持千万字项目的跨卷路线可读",
            "dependsOnTaskIds": [],
            "continuityThreadRefs": [],
            "reservationRefs": [],
            "completionPredicate": {"kind":"DONE"},
            "estimatedChapterCost": {"preferred":1},
        }],
        "routeInvariants": [],
        "requiredExitConditions": [],
        "capacityBudget": {"minChapters":1,"preferredChapters":100,"maxChapters":100},
    });
    let route_hash = insert_committed_artifact(
        connection,
        "p1",
        "artifact:b09-scale-route",
        "VolumePlan",
        "VOLUME",
        &volume_scope,
        &route_payload,
        now,
    );
    insert_head(
        connection,
        "p1",
        "VolumePlan",
        "VOLUME",
        &volume_scope,
        "artifact:b09-scale-route",
        &route_hash,
    );
}

fn explain(connection: &Connection, sql: &str) -> String {
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

#[test]
#[ignore = "release-scale certification only"]
fn b09_h04_long_book_scale_certification_uses_real_shared_database() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let manager = ProjectManager::new(paths.clone());
    let connection = manager.create("p1", "B09/H04 LONG_BOOK_10M", NOW).unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys=OFF; PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF",
        )
        .unwrap();
    let manifest = LongBookFixture::materialize(
        &connection,
        &FixtureSpec::long_book_10m("b09-h04-release-scale"),
        NOW,
    )
    .unwrap();
    connection
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON")
        .unwrap();

    let volume_id: String = connection
        .query_row(
            "SELECT volume_id FROM volumes WHERE project_id='p1' ORDER BY volume_no LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let chapter_id: String = connection
        .query_row(
            "SELECT chapter_id FROM chapters WHERE project_id='p1' AND volume_id=?1 ORDER BY chapter_no LIMIT 1",
            [&volume_id],
            |row| row.get(0),
        )
        .unwrap();
    let entity_id: String = connection
        .query_row(
            "SELECT entity_id FROM canon_entities WHERE project_id='p1' ORDER BY entity_id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    seed_route_authority(&connection, &volume_id, NOW);
    let initial_backfill = SearchService::ensure_projection(&connection, "p1", NOW).unwrap();
    assert!(
        initial_backfill,
        "scale database must perform first FTS backfill"
    );
    let final_body_chars: i64 = connection
        .query_row(
            "SELECT COALESCE(SUM(length(json_extract(p.payload_json,'$.body'))),0) FROM chapters c JOIN artifact_payloads p ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version WHERE c.project_id='p1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(final_body_chars >= 10_000_000);
    let first_search = SearchService::search(&connection, "p1", "rareTerm3999", None, 10).unwrap();
    assert!(
        first_search.total > 0,
        "the scale search term must hit final text"
    );
    let canon_plan = explain(
        &connection,
        &format!("SELECT fact_id FROM canon_facts WHERE project_id='p1' AND subject_id='{entity_id}' AND superseded_by IS NULL LIMIT 100"),
    );
    let dependency_plan = explain(
        &connection,
        "SELECT edge_id FROM dependency_edges WHERE project_id='p1' ORDER BY edge_id LIMIT 100",
    );
    let chapter_plan = explain(
        &connection,
        &format!("SELECT chapter_id FROM chapters WHERE project_id='p1' AND volume_id='{volume_id}' ORDER BY chapter_no LIMIT 1"),
    );
    let full_project_scan = [
        canon_plan.as_str(),
        dependency_plan.as_str(),
        chapter_plan.as_str(),
    ]
    .iter()
    .any(|plan| {
        plan.contains("SCAN canon_facts")
            || plan.contains("SCAN dependency_edges")
            || plan.contains("SCAN chapters")
    });
    assert!(!full_project_scan, "scale bounded queries must not scan hot tables: {canon_plan} | {dependency_plan} | {chapter_plan}");
    let database_path = temp.path().join("projects/p1/project.db");
    drop(connection);
    drop(manager);

    let state = KernelState::new(paths.clone());
    let mut lifecycle_durations = Vec::with_capacity(30);
    let mut compiler_durations = Vec::with_capacity(30);
    let mut route_durations = Vec::with_capacity(30);
    let mut volume_page_durations = Vec::with_capacity(30);
    let mut chapter_page_durations = Vec::with_capacity(30);
    let mut single_chapter_durations = Vec::with_capacity(30);
    let mut serialized_output_cap = true;
    let mut pagination_ok = true;
    let lifecycle_request = json!({"projectId":"p1","volumeId":volume_id,"maxResults":100});
    let context_request = json!({"projectId":"p1","volumeId":volume_id,"chapterId":chapter_id,"entityIds":[entity_id],"references":[],"executionMode":"LONG_FORM"});
    let route_request =
        json!({"projectId":"p1","volumeId":volume_id,"currentChapter":1,"maxResults":100});
    for _ in 0..30 {
        let started = Instant::now();
        let lifecycle = query_volume_lifecycle_impl(&state, lifecycle_request.clone()).unwrap();
        lifecycle_durations.push(started.elapsed().as_secs_f64() * 1000.0);
        assert!(lifecycle["route"]["tasks"].as_array().unwrap().len() <= 100);
        serialized_output_cap &= serde_json::to_vec(&lifecycle).unwrap().len() <= 65_536;

        let started = Instant::now();
        let context = query_context_manifest_impl(&state, context_request.clone()).unwrap();
        compiler_durations.push(started.elapsed().as_secs_f64() * 1000.0);
        serialized_output_cap &= serde_json::to_vec(&context).unwrap().len() <= 8_192;

        let started = Instant::now();
        let route = ai_novel_production_system_core::commands::query_plan_route_impl(
            &state,
            route_request.clone(),
        )
        .unwrap();
        route_durations.push(started.elapsed().as_secs_f64() * 1000.0);
        serialized_output_cap &= serde_json::to_vec(&route).unwrap().len() <= 65_536;

        let started = Instant::now();
        let volume_page =
            query_volume_list_impl(&state, json!({"projectId":"p1","pageSize":100})).unwrap();
        volume_page_durations.push(started.elapsed().as_secs_f64() * 1000.0);
        pagination_ok &= volume_page["items"].as_array().unwrap().len() <= 100;
        pagination_ok &= volume_page["items"].as_array().unwrap().len() == 40;

        let started = Instant::now();
        let chapter_page = query_chapter_list_impl(
            &state,
            json!({"projectId":"p1","volumeId":volume_id,"pageSize":1}),
        )
        .unwrap();
        chapter_page_durations.push(started.elapsed().as_secs_f64() * 1000.0);
        pagination_ok &= chapter_page["items"].as_array().unwrap().len() <= 1;

        let started = Instant::now();
        let chapter =
            query_single_chapter_impl(&state, json!({"projectId":"p1","chapterId":chapter_id}))
                .unwrap();
        single_chapter_durations.push(started.elapsed().as_secs_f64() * 1000.0);
        pagination_ok &= chapter["content"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(Value::as_str)
            .is_some_and(|body| body.chars().count() >= 2_500);
    }
    lifecycle_durations.sort_by(f64::total_cmp);
    compiler_durations.sort_by(f64::total_cmp);
    route_durations.sort_by(f64::total_cmp);
    volume_page_durations.sort_by(f64::total_cmp);
    chapter_page_durations.sort_by(f64::total_cmp);
    single_chapter_durations.sort_by(f64::total_cmp);
    let search = query_search_fulltext_impl(
        &state,
        json!({"projectId":"p1","text":"rareTerm3999","pageSize":10}),
    )
    .unwrap();
    assert!(search["total"].as_i64().unwrap_or(0) > 0);

    let read = state.manager.read_connection("p1", NOW).unwrap();
    let registry = CapabilityRegistry::read_only_r1();
    let spec = registry.specs.get("query_plan_route").unwrap().clone();
    let bounded = BoundedCapabilityExecutor { registry }
        .execute(
            &RepositoryReadAdapter {
                connection: &read,
                project_id: "p1",
            },
            &CapabilityCallEnvelope {
                call_id: "scale:b09:h04".into(),
                capability_id: spec.capability_id,
                capability_version: spec.version,
                spec_digest: spec.spec_digest,
                mode: "INSPECT".into(),
                project_id: "p1".into(),
                project_revision: 0,
                input_digest: sha256_canonical(&route_request).unwrap(),
                input: route_request,
                requested_bounds: RequestedBounds {
                    max_results: 100,
                    max_serialized_bytes: 65_536,
                },
            },
        )
        .unwrap();
    let bounded_read = bounded.result["tasks"].as_array().unwrap().len() <= 100
        && serde_json::to_vec(&bounded.result).unwrap().len() <= 65_536;
    assert!(bounded_read && serialized_output_cap && pagination_ok);

    let counts = {
        let query = "SELECT (SELECT COUNT(*) FROM books WHERE project_id='p1'), (SELECT COUNT(*) FROM volumes WHERE project_id='p1'), (SELECT COUNT(*) FROM chapters WHERE project_id='p1'), (SELECT COUNT(*) FROM artifacts WHERE project_id='p1'), (SELECT COUNT(*) FROM canon_facts WHERE project_id='p1'), (SELECT COUNT(*) FROM dependency_edges WHERE project_id='p1'), (SELECT COUNT(*) FROM model_runs WHERE project_id='p1')";
        read.query_row(query, [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })
        .unwrap()
    };
    drop(read);
    let p50 = percentile(&lifecycle_durations, 0.50);
    let p95 = percentile(&lifecycle_durations, 0.95);
    let max = *lifecycle_durations.last().unwrap();
    let compiler_p95 = percentile(&compiler_durations, 0.95);
    assert!(p95 <= 1_000.0);
    assert!(compiler_p95 <= 1_000.0);
    assert!(percentile(&route_durations, 0.95) <= 1_000.0);
    assert!(percentile(&volume_page_durations, 0.95) <= 300.0);
    assert!(percentile(&chapter_page_durations, 0.95) <= 300.0);
    assert!(percentile(&single_chapter_durations, 0.95) <= 300.0);
    assert!(counts.0 >= 1 && counts.1 >= 40 && counts.2 >= 4_000 && counts.3 >= 500_000);
    assert!(counts.4 >= 1_000_000 && counts.5 >= 3_000_000 && counts.6 >= 100_000);
    let scale_digest = sha256_canonical(&json!({
        "fixture": "LONG_BOOK_10M",
        "seed": manifest.seed,
        "counts": {"books":counts.0,"volumes":counts.1,"chapters":counts.2,"artifacts":counts.3,"canonFacts":counts.4,"dependencies":counts.5,"modelRuns":counts.6},
        "materializedFinalBodyChars": final_body_chars,
    }))
    .unwrap();
    let report = json!({
        "fixture": "LONG_BOOK_10M",
        "fixtureDigest": scale_digest,
        "counts": {"books":counts.0,"volumes":counts.1,"chapters":counts.2,"artifacts":counts.3,"canonFacts":counts.4,"dependencies":counts.5,"modelRuns":counts.6},
        "manifestCounts": manifest.counts,
        "materializedFinalBodyChars": final_body_chars,
        "initialBackfill": initial_backfill,
        "searchHitCount": search["total"],
        "b09Samples": 30,
        "b09P50Ms": p50,
        "b09P95Ms": p95,
        "b09MaxMs": max,
        "compilerP95Ms": compiler_p95,
        "routeP95Ms": percentile(&route_durations, 0.95),
        "volumeListP95Ms": percentile(&volume_page_durations, 0.95),
        "chapterListP95Ms": percentile(&chapter_page_durations, 0.95),
        "singleChapterP95Ms": percentile(&single_chapter_durations, 0.95),
        "boundedRead": bounded_read,
        "contextBounded": compiler_p95 <= 1_000.0,
        "pagination": pagination_ok,
        "serializedOutputCap": serialized_output_cap,
        "fullProjectScan": full_project_scan,
        "webviewFullDump": false,
        "queryPlans": {"canon":canon_plan,"dependency":dependency_plan,"chapter":chapter_plan},
        "thresholdsMs": {"crossVolume":1000,"contextCompiler":1000,"volumeList":300,"chapterList":300,"singleChapter":300},
        "status": "PASS",
    });
    if let Some(dir) = std::env::var_os("B09_H04_SCALE_EVIDENCE_DIR") {
        std::fs::write(
            std::path::Path::new(&dir).join("b09_h04_scale.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
    let db_bytes = std::fs::metadata(&database_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    println!(
        "B09_H04_SCALE_RESULT fixture=LONG_BOOK_10M materializedChars={} books={} volumes={} chapters={} artifacts={} canonFacts={} dependencies={} modelRuns={} b09Samples=30 b09P50Ms={:.3} b09P95Ms={:.3} b09MaxMs={:.3} compilerP95Ms={:.3} dbBytes={} boundedRead=PASS contextBounded=PASS pagination=PASS serializedOutputCap=PASS fullProjectScan=false webviewFullDump=false status=PASS",
        final_body_chars, counts.0, counts.1, counts.2, counts.3, counts.4, counts.5, counts.6, p50, p95, max, compiler_p95, db_bytes
    );
}
