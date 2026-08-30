use ai_novel_production_system_core::commands::{
    query_plan_route_impl, KernelState, RepositoryReadAdapter,
};
use ai_novel_production_system_core::generated::types::ArtifactRef;
use ai_novel_production_system_core::harness::{
    BoundedCapabilityExecutor, CapabilityCallEnvelope, CapabilityRegistry, RequestedBounds,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};
use serde_json::json;
use std::time::Instant;

#[test]
#[ignore = "B07/H02 relevant scale runs once after non-scale release gates"]
fn b07_h02_populated_route_scale_certification() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state
        .manager
        .create("scale-b07", "B07 scale", "2026-08-30T00:00:00Z")
        .unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:scale','scale-b07',1,'Scale','ACTIVE',?1,?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    let volume_id =
        StableIdentityFactory::from_command(StableIdKind::Volume, "scale-volume").unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,'scale-b07','book:scale',1,'Scale','PLANNING',1,1000,1000000,?2,?2)", libsql_rusqlite::params![volume_id, "2026-08-30T00:00:00Z"]).unwrap();
    let book_plan = json!({"planningMaturity":"APPROVED","futureReservations":[]});
    let book_hash = sha256_canonical(&book_plan).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('plan:book:scale','scale-b07','BookPlan','PROJECT',?1,1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)", libsql_rusqlite::params!["{\"level\":\"PROJECT\"}", book_hash, "2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('plan:book:scale',1,?1,?2)", libsql_rusqlite::params![book_plan.to_string(), book_hash]).unwrap();
    connection.execute("INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES ('scale-b07','BookPlan','PROJECT',?1,'plan:book:scale',1,?2)", libsql_rusqlite::params!["{\"level\":\"PROJECT\"}", book_hash]).unwrap();
    drop(connection);
    let intent = json!({"payloadSchema":"volume-intent.r1","volumeFunction":"scale route","entryConditions":[],"mustAchieve":[],"mustNotBreak":[],"requiredExitConditions":[],"futureReservationRefs":[],"crossVolumeResponsibilityRefs":[],"targetChapterRange":{"min":1,"preferred":1000,"max":1000},"targetWordRange":{"preferred":1000000}});
    let intent_result = ai_novel_production_system_core::commands::execute_command_impl(&state, json!({"type":"planning_create_volume_intent","commandId":"scale:intent","idempotencyKey":"scale:intent","projectId":"scale-b07","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"volumeId":volume_id,"plan":intent}})).unwrap();
    let intent_ref: ArtifactRef =
        serde_json::from_value(intent_result["receipt"]["committedArtifactRefs"][0].clone())
            .unwrap();
    let tasks = (0..1000)
        .map(|index| json!({"taskId":format!("scale:task:{index}"),"class":"REQUIRED","objective":format!("advance {index}"),"dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"DONE"},"estimatedChapterCost":{"preferred":1}}))
        .collect::<Vec<_>>();
    let route = json!({"payloadSchema":"volume-plan.r1","volumeIntentRef":intent_ref,"routeVersion":1,"tasks":tasks,"routeInvariants":[],"requiredExitConditions":[],"capacityBudget":{"minChapters":1,"preferredChapters":1000,"maxChapters":1000}});
    ai_novel_production_system_core::commands::execute_command_impl(&state, json!({"type":"planning_create_volume_plan","commandId":"scale:route","idempotencyKey":"scale:route","projectId":"scale-b07","expectedRevision":1,"requestedAt":"2026-08-30T00:00:01Z","payload":{"volumeId":volume_id,"plan":route}})).unwrap();
    let mut timings = Vec::new();
    for _ in 0..20 {
        let started = Instant::now();
        let page = query_plan_route_impl(&state, json!({"projectId":"scale-b07","volumeId":volume_id,"currentChapter":0,"maxResults":100})).unwrap();
        timings.push(started.elapsed().as_millis());
        assert_eq!(page["tasks"].as_array().unwrap().len(), 100);
        assert_eq!(page["truncated"], true);
    }
    timings.sort_unstable();
    let p95 = timings[18];
    let connection = state
        .manager
        .open("scale-b07", "", "2026-08-30T00:00:02Z")
        .unwrap();
    let registry = CapabilityRegistry::read_only_r1();
    let spec = registry
        .specs
        .get("diagnose_planning_constraints")
        .unwrap()
        .clone();
    let input = json!({"projectId":"scale-b07","projectRevision":2,"volumeId":volume_id,"currentChapter":0,"question":"哪些 Required Task 还没完成？","scope":"VOLUME","entityIds":[],"references":[]});
    let result = BoundedCapabilityExecutor { registry }
        .execute(
            &RepositoryReadAdapter {
                connection: &connection,
                project_id: "scale-b07",
            },
            &CapabilityCallEnvelope {
                call_id: "scale:h02".into(),
                capability_id: spec.capability_id,
                capability_version: spec.version,
                spec_digest: spec.spec_digest,
                mode: "DIAGNOSE".into(),
                project_id: "scale-b07".into(),
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
    assert!(p95 < 2000, "route p95 too slow: {p95}ms");
    println!("B07_SCALE_RESULT route_tasks=1000 returned=100 truncated=true h02_diagnostic=PASS p95_ms={p95} status=PASS");
}
