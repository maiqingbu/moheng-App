use ai_novel_production_system_core::commands::{
    query_plan_route_impl, KernelState, RepositoryReadAdapter,
};
use ai_novel_production_system_core::harness::{
    BoundedCapabilityExecutor, CapabilityCallEnvelope, CapabilityRegistry, RequestedBounds,
};
use ai_novel_production_system_core::hash::{canonical_bytes, sha256_canonical};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repositories::ops::{FixtureSpec, LongBookFixture};
use libsql_rusqlite::Connection;
use serde_json::json;
use std::time::Instant;

fn scope_key(value: serde_json::Value) -> String {
    String::from_utf8(canonical_bytes(&value).unwrap()).unwrap()
}

#[allow(clippy::too_many_arguments)]
fn insert_committed_artifact(
    connection: &Connection,
    project_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    scope_level: &str,
    scope_key: &str,
    payload: &serde_json::Value,
    now: &str,
) -> String {
    let hash = sha256_canonical(payload).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'m0e.fixture.v1','COMMITTED',?6,'{}',?7,?7)", libsql_rusqlite::params![artifact_id, project_id, artifact_type, scope_level, scope_key, hash, now]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)", libsql_rusqlite::params![artifact_id, payload.to_string(), hash]).unwrap();
    hash
}

#[test]
#[ignore = "B07/H02 relevant scale runs once after non-scale release gates"]
fn b07_h02_long_book_relevant_scale_certification() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state
        .manager
        .create("p1", "B07 H02 LONG_BOOK_10M", "2026-08-30T00:00:00Z")
        .unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys=OFF; PRAGMA journal_mode=MEMORY; PRAGMA synchronous=OFF",
        )
        .unwrap();
    let manifest = LongBookFixture::materialize(
        &connection,
        &FixtureSpec::long_book_10m("b07-h02-relevant-scale"),
        "2026-08-30T00:00:00Z",
    )
    .unwrap();
    connection
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON")
        .unwrap();
    let volume_id: String = connection
        .query_row(
            "SELECT volume_id FROM volumes WHERE project_id='p1' AND volume_no=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let intent_scope = scope_key(json!({"level":"VOLUME","volumeId":volume_id}));
    let intent_payload = json!({"payloadSchema":"volume-intent.r1","volumeFunction":"scale route","entryConditions":[],"mustAchieve":[],"mustNotBreak":[],"requiredExitConditions":[],"futureReservationRefs":[],"crossVolumeResponsibilityRefs":[],"targetChapterRange":{"min":1,"preferred":100,"max":100},"targetWordRange":{"preferred":250000}});
    let plan_payload = json!({"payloadSchema":"volume-plan.r1","volumeIntentRef":{"artifactId":"artifact:b07-scale-intent","version":1,"payloadHash":"PLACEHOLDER"},"routeVersion":1,"tasks":(0..1000).map(|index| json!({"taskId":format!("scale:task:{index}"),"class":"REQUIRED","objective":format!("advance {index}"),"dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"DONE"},"estimatedChapterCost":{"preferred":1}})).collect::<Vec<_>>(),"routeInvariants":[],"requiredExitConditions":[],"capacityBudget":{"minChapters":1,"preferredChapters":100,"maxChapters":100}});
    let intent_hash = insert_committed_artifact(
        &connection,
        "p1",
        "artifact:b07-scale-intent",
        "VolumeIntent",
        "VOLUME",
        &intent_scope,
        &intent_payload,
        "2026-08-30T00:00:00Z",
    );
    let mut plan_payload = plan_payload;
    plan_payload["volumeIntentRef"]["payloadHash"] = json!(intent_hash);
    let plan_hash = insert_committed_artifact(
        &connection,
        "p1",
        "artifact:b07-scale-plan",
        "VolumePlan",
        "VOLUME",
        &intent_scope,
        &plan_payload,
        "2026-08-30T00:00:00Z",
    );
    connection.execute("INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES ('p1','VolumeIntent','VOLUME',?1,'artifact:b07-scale-intent',1,?2)", libsql_rusqlite::params![intent_scope, intent_hash]).unwrap();
    connection.execute("INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES ('p1','VolumePlan','VOLUME',?1,'artifact:b07-scale-plan',1,?2)", libsql_rusqlite::params![intent_scope, plan_hash]).unwrap();
    let book_plan = json!({"planningMaturity":"APPROVED","futureReservations":[]});
    let book_hash = insert_committed_artifact(
        &connection,
        "p1",
        "artifact:b07-scale-book",
        "BookPlan",
        "PROJECT",
        "p1",
        &book_plan,
        "2026-08-30T00:00:00Z",
    );
    connection.execute("INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES ('p1','BookPlan','PROJECT','p1','artifact:b07-scale-book',1,?1)", [&book_hash]).unwrap();
    drop(connection);
    let mut timings = Vec::new();
    for _ in 0..20 {
        let started = Instant::now();
        let page = query_plan_route_impl(
            &state,
            json!({"projectId":"p1","volumeId":volume_id,"currentChapter":0,"maxResults":100}),
        )
        .unwrap();
        timings.push(started.elapsed().as_millis());
        assert_eq!(page["tasks"].as_array().unwrap().len(), 100);
        assert_eq!(page["truncated"], true);
    }
    timings.sort_unstable();
    let p95 = timings[18];
    let connection = state
        .manager
        .open("p1", "", "2026-08-30T00:00:01Z")
        .unwrap();
    let registry = CapabilityRegistry::read_only_r1();
    let spec = registry
        .specs
        .get("diagnose_planning_constraints")
        .unwrap()
        .clone();
    let input = json!({"projectId":"p1","projectRevision":2,"volumeId":volume_id,"currentChapter":0,"question":"哪些 Required Task 还没完成？","scope":"VOLUME","entityIds":[],"references":[]});
    let result = BoundedCapabilityExecutor { registry }
        .execute(
            &RepositoryReadAdapter {
                connection: &connection,
                project_id: "p1",
            },
            &CapabilityCallEnvelope {
                call_id: "scale:h02".into(),
                capability_id: spec.capability_id,
                capability_version: spec.version,
                spec_digest: spec.spec_digest,
                mode: "DIAGNOSE".into(),
                project_id: "p1".into(),
                project_revision: 2,
                input_digest: sha256_canonical(&input).unwrap(),
                input,
                requested_bounds: RequestedBounds {
                    max_results: 100,
                    max_serialized_bytes: 65_536,
                },
            },
        )
        .unwrap();
    assert_eq!(result.result["supportStatus"], "SUPPORTED");
    assert_eq!(result.result["truncated"], true);
    assert!(
        manifest.counts.chapters >= 4000
            && manifest.counts.artifacts >= 500000
            && manifest.counts.canon_facts >= 1000000
            && manifest.counts.dependencies >= 3000000
    );
    assert!(p95 < 2000, "LONG_BOOK route p95 too slow: {p95}ms");
    println!("B07_SCALE_RESULT fixture=LONG_BOOK_10M chapters={} artifacts={} canon_facts={} dependencies={} route_tasks=1000 returned=100 truncated=true h02_diagnostic=PASS p95_ms={p95} status=PASS", manifest.counts.chapters, manifest.counts.artifacts, manifest.counts.canon_facts, manifest.counts.dependencies);
}
