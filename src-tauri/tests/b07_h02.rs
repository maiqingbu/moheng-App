use ai_novel_production_system_core::commands::RepositoryReadAdapter;
use ai_novel_production_system_core::commands::{
    execute_command_impl, query_context_manifest_impl, query_plan_route_impl, support_session_impl,
    KernelState,
};
use ai_novel_production_system_core::generated::types::{ArtifactEnvelope, ArtifactRef};
use ai_novel_production_system_core::harness::{
    check_checkpoint_resume, registry_digest_map, BoundedCapabilityExecutor,
    CapabilityCallEnvelope, CapabilityRegistry, RequestedBounds, SupportCheckpoint,
    SupportStepStatus, SupportTaskStep,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repositories::artifacts::ArtifactRepository;
use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};
use serde_json::json;
use std::collections::BTreeMap;

fn insert_project_authority(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    payload: &serde_json::Value,
) -> String {
    let scope = "{\"level\":\"PROJECT\"}";
    let hash = sha256_canonical(payload).unwrap();
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES (?1,?2,?3,'PROJECT',?4,1,'m0a.artifact.v1','COMMITTED',?5,'{}',?6)",
            libsql_rusqlite::params![artifact_id, project_id, artifact_type, scope, hash, "2026-08-30T00:00:00Z"],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            libsql_rusqlite::params![artifact_id, payload.to_string(), hash],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,?2,'PROJECT',?3,?4,1,?5)",
            libsql_rusqlite::params![project_id, artifact_type, scope, artifact_id, hash],
        )
        .unwrap();
    hash
}

fn persist_context_manifest_version(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    chapter_id: &str,
    artifact_id: &str,
    version: i64,
    payload: &serde_json::Value,
) -> ArtifactRef {
    let payload_hash = sha256_canonical(payload).unwrap();
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId":artifact_id,
        "projectId":project_id,
        "artifactType":"ContextManifest",
        "scope":{"level":"CHAPTER","chapterId":chapter_id},
        "version":version,
        "schemaVersion":"m0a.artifact.v1",
        "artifactStatus":"CANDIDATE",
        "payload":payload,
        "payloadHash":payload_hash,
        "dependencyRefs":[],
        "provenance":{"createdBy":"B07::PlanningOwner","commandId":format!("command:context:{project_id}:v{version}")},
        "createdAt":"2026-08-30T00:00:02Z"
    }))
    .unwrap();
    ArtifactRepository::insert_candidate(connection, project_id, &artifact).unwrap();
    connection
        .execute(
            "UPDATE artifacts SET artifact_status='COMMITTED' WHERE artifact_id=?1",
            [&artifact.artifact_id],
        )
        .unwrap();
    let scope_key = String::from_utf8(
        ai_novel_production_system_core::hash::canonical_bytes(
            &json!({"level":"CHAPTER","chapterId":chapter_id}),
        )
        .unwrap(),
    )
    .unwrap();
    let reference = ArtifactRef {
        artifact_id: artifact.artifact_id,
        version,
        payload_hash: payload_hash.clone(),
    };
    ArtifactRepository::promote_head(
        connection,
        project_id,
        &reference,
        "ContextManifest",
        "CHAPTER",
        &scope_key,
    )
    .unwrap();
    reference
}

#[test]
fn b07_volume_intent_and_route_query_use_commit_bundle_authority() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state
        .manager
        .create("p1", "B07", "2026-08-30T00:00:00Z")
        .unwrap();
    for index in 1..=3 {
        connection.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES (?1,'p1',?2,'CHARACTER',?3,?3)", libsql_rusqlite::params![format!("entity:book:{index}"), format!("Entity {index}"), "2026-08-30T00:00:00Z"]).unwrap();
        connection.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES (?1,'p1',?2,'opening','{}','A',?3,?3)", libsql_rusqlite::params![format!("fact:{index}"), format!("entity:book:{index}"), "2026-08-30T00:00:00Z"]).unwrap();
    }
    let signing_hash = "c".repeat(64);
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:signing','p1','SimulatedSigningReceipt','PROJECT',?1,1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)", libsql_rusqlite::params!["{\"level\":\"PROJECT\"}", signing_hash, "2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:signing',1,'{}',?1)", [&signing_hash]).unwrap();
    let approved_book_plan = json!({"payloadSchema":"book-plan.r1","planningMaturity":"APPROVED","storyPromise":"promise","protagonistMacroDirection":"direction","endingBoundary":"end","mustAchieve":[],"mustNotBreak":[],"relationshipMacroDirections":[],"majorMysteryGoals":[],"futureReservations":[{"reservationId":"secret:v5","description":"reserved reveal","reservedForVolumeNo":5,"notBeforeVolumeNo":5,"prerequisites":[],"mayForeshadow":true,"mayReveal":true,"authorityLevel":"A"}],"volumeIntentSeeds":[],"openingHypotheses":[],"globalLengthBudget":{"preferredWords":1000}});
    let approved_hash = sha256_canonical(&approved_book_plan).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('plan:bookplan:p1','p1','BookPlan','PROJECT',?1,1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)", libsql_rusqlite::params!["{\"level\":\"PROJECT\"}", approved_hash, "2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('plan:bookplan:p1',1,?1,?2)", libsql_rusqlite::params![approved_book_plan.to_string(), approved_hash]).unwrap();
    connection.execute("INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES ('p1','BookPlan','PROJECT',?1,'plan:bookplan:p1',1,?2)", libsql_rusqlite::params!["{\"level\":\"PROJECT\"}", approved_hash]).unwrap();
    insert_project_authority(
        &connection,
        "p1",
        "story-constitution:p1",
        "StoryConstitution",
        &json!({
            "storyCore":{
                "protagonist":"Entity 1",
                "mechanism":"每次追查都会留下可验证代价"
            }
        }),
    );
    insert_project_authority(
        &connection,
        "p1",
        "main-contract:p1",
        "MainContract",
        &json!({"hardRules":["keep evidence"]}),
    );
    let volume_id =
        StableIdentityFactory::from_command(StableIdKind::Volume, "command:volume").unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:1','p1',1,'Book','ACTIVE',?1,?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,'p1','book:1',1,'Volume','PLANNING',1,12,12000,?2,?2)", libsql_rusqlite::params![volume_id, "2026-08-30T00:00:00Z"]).unwrap();
    drop(connection);
    let intent = json!({"payloadSchema":"volume-intent.r1","volumeFunction":"opening","entryConditions":[],"mustAchieve":[{"id":"goal:p1","description":"完成真实证据核对"}],"mustNotBreak":[{"id":"rule:p1","description":"不得改变既有事实"}],"requiredExitConditions":[{"kind":"GOAL_ACHIEVED","value":"goal:p1"}],"futureReservationRefs":[],"crossVolumeResponsibilityRefs":[],"targetChapterRange":{"min":1,"preferred":8,"max":12},"targetWordRange":{"preferred":12000},"sourceBookPlanRef":{"artifactId":"plan:bookplan:p1","version":1,"payloadHash":approved_hash},"compiledFromProjectRevision":0,"sourceEvidenceRefs":["artifact:signing"],"semanticDigest":"0".repeat(64)});
    let mut malformed_intent = intent.clone();
    malformed_intent
        .as_object_mut()
        .unwrap()
        .remove("requiredExitConditions");
    let malformed = execute_command_impl(
        &state,
        json!({"type":"planning_create_volume_intent","commandId":"command:intent:malformed","idempotencyKey":"key:intent:malformed","projectId":"p1","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"volumeId":volume_id,"plan":malformed_intent}}),
    );
    assert!(malformed
        .unwrap_err()
        .starts_with("VOLUME_INTENT_CONTRACT:"));
    let committed = execute_command_impl(&state, json!({"type":"planning_create_volume_intent","commandId":"command:intent","idempotencyKey":"key:intent","projectId":"p1","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"volumeId":volume_id,"plan":intent}})).unwrap();
    assert_eq!(committed["finalStatus"], "committed");
    let tasks = (0..101).map(|index| json!({"taskId":format!("task:{index}"),"class":"REQUIRED","objective":format!("objective {index}"),"dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"DONE"},"estimatedChapterCost":{"preferred":1}})).collect::<Vec<_>>();
    let route = json!({"payloadSchema":"volume-plan.r1","volumeIntentRef":{"artifactId":format!("plan:volumeintent:{volume_id}"),"version":1,"payloadHash":committed["receipt"]["committedArtifactRefs"][0]["payloadHash"]},"routeVersion":1,"tasks":tasks,"routeInvariants":[],"requiredExitConditions":[],"capacityBudget":{"minChapters":1,"preferredChapters":8,"maxChapters":12}});
    let route_result = execute_command_impl(
        &state,
        json!({"type":"planning_create_volume_plan","commandId":"command:route","idempotencyKey":"key:route","projectId":"p1","expectedRevision":1,"requestedAt":"2026-08-30T00:00:01Z","payload":{"volumeId":volume_id,"plan":route}}),
    );
    assert!(route_result.is_ok());
    let query = query_plan_route_impl(
        &state,
        json!({"projectId":"p1","volumeId":volume_id,"currentChapter":0}),
    )
    .unwrap();
    assert_eq!(query["owner"], "B07::PlanningRoute");
    assert_eq!(query["routeVersion"], 1);
    assert_eq!(query["tasks"].as_array().unwrap().len(), 100);
    assert_eq!(query["truncated"], true);
    let context_error = query_context_manifest_impl(
        &state,
        json!({"projectId":"p1","volumeId":volume_id,"chapterId":"chapter:1","scope":"VOLUME","entityIds":[],"references":[]}),
    )
    .unwrap_err();
    assert!(
        context_error.contains("CONTEXT_HARD_INCOMPLETE|sources=TASK|scope=CHAPTER:chapter:1"),
        "expected TASK hard truncation, got: {context_error}"
    );
    println!("B07_INTENT_RESULT malformed_rejected=PASS valid_commit=PASS");
    println!("B07_ROUTE_RESULT volume_scope=PASS h02_query=PASS truncation=PASS context_over_bound=BLOCKED status=PASS");
    let connection = state
        .manager
        .open("p1", "", "2026-08-30T00:00:02Z")
        .unwrap();
    let registry = CapabilityRegistry::read_only_r1();
    let input = json!({"projectId":"p1","projectRevision":2,"volumeId":volume_id,"question":"哪些 Required Task 还没完成？","currentChapter":0});
    let spec = registry
        .specs
        .get("diagnose_planning_constraints")
        .unwrap()
        .clone();
    let result = BoundedCapabilityExecutor { registry }
        .execute(
            &RepositoryReadAdapter {
                connection: &connection,
                project_id: "p1",
            },
            &CapabilityCallEnvelope {
                call_id: "call:h02".into(),
                capability_id: spec.capability_id,
                capability_version: spec.version,
                spec_digest: spec.spec_digest,
                mode: "DIAGNOSE".into(),
                project_id: "p1".into(),
                project_revision: 2,
                input_digest: sha256_canonical(&input).unwrap(),
                input,
                requested_bounds: RequestedBounds {
                    max_results: 10,
                    max_serialized_bytes: 65_536,
                },
            },
        )
        .unwrap();
    assert_eq!(result.result["diagnosisType"], "PLANNING_CONSTRAINTS");
    assert_eq!(result.result["supportStatus"], "SUPPORTED");
    assert_eq!(
        result.result["explanation"]["scopeNote"],
        "仅展示边界内的部分 Route，不能视为全部约束"
    );
    assert!(result.result["provenance"]["volumePlanRef"]["artifactId"].is_string());
    assert!(result.result["provenance"]["taskReality"].is_array());
    assert!(result.result["futureReservations"].is_array());
    assert!(result.result["continuityThreads"].is_array());
    assert!(result.truncated);
    assert_eq!(result.result["truncated"], true);
    drop(connection);
    let session = support_session_impl(&state, json!({"projectId":"p1","volumeId":volume_id,"scope":"VOLUME","question":"哪些 Required Task 还没完成？","mode":"DIAGNOSE","entityIds":[],"references":[]})).unwrap();
    assert_eq!(session["finding"]["diagnosisType"], "PLANNING_CONSTRAINTS");
    assert_eq!(session["finding"]["supportStatus"], "SUPPORTED");
    assert!(session["receipt"]["checkpointRef"].is_string());
    println!("H02_SESSION_RESULT planning_question=PASS finding=SUPPORTED receipt=PASS checkpoint=PASS provenance=PASS status=PASS");
}

#[test]
fn b07_bookplan_approval_commits_new_approved_version() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state
        .manager
        .create("p2", "B07 book", "2026-08-30T00:00:00Z")
        .unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:p2','p2',1,'Book','ACTIVE',?1,?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:p2','p2','book:p2',1,'Volume','PLANNING',1,3,3000,?1,?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    for index in 1..=3 {
        connection.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES (?1,'p2',?2,'CHARACTER',?3,?3)", libsql_rusqlite::params![format!("entity:p2:{index}"), format!("Entity {index}"), "2026-08-30T00:00:00Z"]).unwrap();
        connection.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES (?1,'p2',?2,'opening','{}','A',?3,?3)", libsql_rusqlite::params![format!("fact:p2:{index}"), format!("entity:p2:{index}"), "2026-08-30T00:00:00Z"]).unwrap();
        connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,'p2','volume:p2',?2,?3,'NORMAL','PLANNED',0,?4,?4)", libsql_rusqlite::params![format!("chapter:p2:{index}"), index, format!("Chapter {index}"), "2026-08-30T00:00:00Z"]).unwrap();
    }
    connection.execute("INSERT INTO project_runtime_state(project_id,signing_status,updated_at) VALUES ('p2','PASS',?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    let signing_payload = json!({"status":"PASS","openingChapterRefs":["chapter:p2:1","chapter:p2:2","chapter:p2:3"],"openingCanonRefs":["fact:p2:1","fact:p2:2","fact:p2:3"]});
    let signing_hash = sha256_canonical(&signing_payload).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:p2:signing','p2','SimulatedSigningReceipt','PROJECT',?1,1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)", libsql_rusqlite::params!["{\"level\":\"PROJECT\"}", signing_hash, "2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:p2:signing',1,?1,?2)", libsql_rusqlite::params![signing_payload.to_string(), signing_hash]).unwrap();
    for index in 1..=3 {
        connection.execute("INSERT INTO canon_evidence(evidence_id,project_id,fact_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,range_start,range_end,created_at) VALUES (?1,'p2',?2,?3,'artifact:p2:signing',1,'OPENING','opening evidence',1,10,?4)", libsql_rusqlite::params![format!("evidence:p2:{index}"), format!("fact:p2:{index}"), format!("chapter:p2:{index}"), "2026-08-30T00:00:00Z"]).unwrap();
    }
    for index in 50..=52 {
        connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,'p2','volume:p2',?2,?3,'NORMAL','PLANNED',0,?4,?4)", libsql_rusqlite::params![format!("chapter:p2:{index}"), index, format!("Chapter {index}"), "2026-08-30T00:00:00Z"]).unwrap();
    }
    connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:p2:4','p2','volume:p2',4,'Chapter 4','NORMAL','PLANNED',0,?1,?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES ('fact:p2:later','p2','entity:p2:1','later','{}','A',?1,?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES ('fact:p2:no-evidence','p2','entity:p2:1','later','{}','A',?1,?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_evidence(evidence_id,project_id,fact_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,range_start,range_end,created_at) VALUES ('evidence:p2:later','p2','fact:p2:later','chapter:p2:4','artifact:p2:signing',1,'LATER','later evidence',1,10,?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    for (artifact_id, payload) in [
        (
            "artifact:p2:latechap",
            json!({"status":"PASS","openingChapterRefs":["chapter:p2:50","chapter:p2:51","chapter:p2:52"],"openingCanonRefs":["fact:p2:1","fact:p2:2","fact:p2:3"]}),
        ),
        (
            "artifact:p2:latecanon",
            json!({"status":"PASS","openingChapterRefs":["chapter:p2:1","chapter:p2:2","chapter:p2:3"],"openingCanonRefs":["fact:p2:later"]}),
        ),
        (
            "artifact:p2:noevidence",
            json!({"status":"PASS","openingChapterRefs":["chapter:p2:1","chapter:p2:2","chapter:p2:3"],"openingCanonRefs":["fact:p2:no-evidence"]}),
        ),
    ] {
        let payload_hash = sha256_canonical(&payload).unwrap();
        connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES (?1,'p2','SimulatedSigningReceipt','PROJECT',?2,1,'m0a.artifact.v1','COMMITTED',?3,'{}',?4)", libsql_rusqlite::params![artifact_id, "{\"level\":\"PROJECT\"}", payload_hash, "2026-08-30T00:00:00Z"]).unwrap();
        connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)", libsql_rusqlite::params![artifact_id, payload.to_string(), payload_hash]).unwrap();
    }
    drop(connection);
    let seed = json!({"payloadSchema":"book-plan.r1","planningMaturity":"SEED","storyPromise":"promise","protagonistMacroDirection":"direction","endingBoundary":"end","mustAchieve":[{"id":"goal:1","description":"goal"}],"mustNotBreak":[],"relationshipMacroDirections":[],"majorMysteryGoals":[],"futureReservations":[],"volumeIntentSeeds":[],"openingHypotheses":[],"globalLengthBudget":{"preferredWords":1000}});
    let rejected = execute_command_impl(
        &state,
        json!({"type":"planning_finalize_book_plan","commandId":"command:book:bad","idempotencyKey":"key:book:bad","projectId":"p2","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"plan":seed,"signingStatus":"PASS","openingCanonRefs":["fact:missing:1","fact:missing:2","fact:missing:3"],"signingEvidenceRefs":["artifact:missing"]}}),
    );
    assert_eq!(
        rejected.unwrap_err(),
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );
    let planned_opening = execute_command_impl(
        &state,
        json!({"type":"planning_finalize_book_plan","commandId":"command:book:g3:planned","idempotencyKey":"key:book:g3:planned","projectId":"p2","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"plan":seed,"signingStatus":"PASS","openingCanonRefs":["fact:p2:1","fact:p2:2","fact:p2:3"],"signingEvidenceRefs":["artifact:p2:signing"]}}),
    );
    assert_eq!(
        planned_opening.unwrap_err(),
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );
    let connection = state
        .manager
        .open("p2", "", "2026-08-30T00:00:00Z")
        .unwrap();
    connection.execute("UPDATE chapters SET status='COMMITTED' WHERE project_id='p2' AND chapter_no IN (1,2,3)", []).unwrap();
    drop(connection);
    let no_final_artifact = execute_command_impl(
        &state,
        json!({"type":"planning_finalize_book_plan","commandId":"command:book:g3:no-final","idempotencyKey":"key:book:g3:no-final","projectId":"p2","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"plan":seed,"signingStatus":"PASS","openingCanonRefs":["fact:p2:1","fact:p2:2","fact:p2:3"],"signingEvidenceRefs":["artifact:p2:signing"]}}),
    );
    assert_eq!(
        no_final_artifact.unwrap_err(),
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );
    let connection = state
        .manager
        .open("p2", "", "2026-08-30T00:00:00Z")
        .unwrap();
    for index in 1..=3 {
        let artifact_id = format!("artifact:p2:final:{index}");
        let payload =
            json!({"chapterId":format!("chapter:p2:{index}"),"body":format!("opening {index}")});
        let payload_hash = sha256_canonical(&payload).unwrap();
        connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES (?1,'p2','FinalChapter','CHAPTER',?2,1,'m0a.artifact.v1','CANDIDATE',?3,'{}',?4)", libsql_rusqlite::params![artifact_id, format!("{{\"level\":\"CHAPTER\",\"chapterId\":\"chapter:p2:{index}\"}}"), payload_hash, "2026-08-30T00:00:00Z"]).unwrap();
        connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)", libsql_rusqlite::params![artifact_id, payload.to_string(), payload_hash]).unwrap();
        connection
            .execute(
                "UPDATE chapters SET final_artifact_id=?1,final_version=1 WHERE chapter_id=?2",
                libsql_rusqlite::params![artifact_id, format!("chapter:p2:{index}")],
            )
            .unwrap();
    }
    drop(connection);
    let candidate_final = execute_command_impl(
        &state,
        json!({"type":"planning_finalize_book_plan","commandId":"command:book:g3:candidate","idempotencyKey":"key:book:g3:candidate","projectId":"p2","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"plan":seed,"signingStatus":"PASS","openingCanonRefs":["fact:p2:1","fact:p2:2","fact:p2:3"],"signingEvidenceRefs":["artifact:p2:signing"]}}),
    );
    assert_eq!(
        candidate_final.unwrap_err(),
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );
    let connection = state
        .manager
        .open("p2", "", "2026-08-30T00:00:00Z")
        .unwrap();
    connection.execute("UPDATE artifacts SET artifact_status='COMMITTED' WHERE project_id='p2' AND artifact_type='FinalChapter'", []).unwrap();
    drop(connection);
    for (suffix, receipt) in [
        ("latechap", "artifact:p2:latechap"),
        ("latecanon", "artifact:p2:latecanon"),
        ("noevidence", "artifact:p2:noevidence"),
    ] {
        let result = execute_command_impl(
            &state,
            json!({"type":"planning_finalize_book_plan","commandId":format!("command:book:g3:{suffix}"),"idempotencyKey":format!("key:book:g3:{suffix}"),"projectId":"p2","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"plan":seed,"signingStatus":"PASS","openingCanonRefs":if suffix == "latecanon" { vec!["fact:p2:later"] } else if suffix == "noevidence" { vec!["fact:p2:no-evidence"] } else { vec!["fact:p2:1","fact:p2:2","fact:p2:3"] },"signingEvidenceRefs":[receipt]}}),
        );
        assert_eq!(
            result.unwrap_err(),
            "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
        );
    }
    let connection = state
        .manager
        .open("p2", "", "2026-08-30T00:00:00Z")
        .unwrap();
    let fake_payload = json!({"review":"not a signing receipt"});
    let fake_hash = sha256_canonical(&fake_payload).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:p2:fake','p2','ReviewReport','PROJECT',?1,1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)", libsql_rusqlite::params!["{\"level\":\"PROJECT\"}", fake_hash, "2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:p2:fake',1,?1,?2)", libsql_rusqlite::params![fake_payload.to_string(), fake_hash]).unwrap();
    drop(connection);
    let fake_receipt = execute_command_impl(
        &state,
        json!({"type":"planning_finalize_book_plan","commandId":"command:book:fake","idempotencyKey":"key:book:fake","projectId":"p2","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"plan":seed,"signingStatus":"PASS","openingCanonRefs":["fact:p2:1","fact:p2:2","fact:p2:3"],"signingEvidenceRefs":["artifact:p2:fake"]}}),
    );
    assert_eq!(
        fake_receipt.unwrap_err(),
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );
    let connection = state
        .manager
        .open("p2", "", "2026-08-30T00:00:00Z")
        .unwrap();
    let revise_payload = json!({"status":"REVISE","openingChapterRefs":["chapter:p2:1","chapter:p2:2","chapter:p2:3"],"openingCanonRefs":["fact:p2:1","fact:p2:2","fact:p2:3"]});
    let revise_hash = sha256_canonical(&revise_payload).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:p2:revise','p2','SimulatedSigningReceipt','PROJECT',?1,1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)", libsql_rusqlite::params!["{\"level\":\"PROJECT\"}", revise_hash, "2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:p2:revise',1,?1,?2)", libsql_rusqlite::params![revise_payload.to_string(), revise_hash]).unwrap();
    drop(connection);
    let wrong_verdict = execute_command_impl(
        &state,
        json!({"type":"planning_finalize_book_plan","commandId":"command:book:revise","idempotencyKey":"key:book:revise","projectId":"p2","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"plan":seed,"signingStatus":"PASS","openingCanonRefs":["fact:p2:1","fact:p2:2","fact:p2:3"],"signingEvidenceRefs":["artifact:p2:revise"]}}),
    );
    assert_eq!(
        wrong_verdict.unwrap_err(),
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );
    let result = execute_command_impl(&state, json!({"type":"planning_finalize_book_plan","commandId":"command:book","idempotencyKey":"key:book","projectId":"p2","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"plan":seed,"signingStatus":"PASS","openingCanonRefs":["fact:p2:1","fact:p2:2","fact:p2:3"],"signingEvidenceRefs":["artifact:p2:signing"]}})).unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let connection = state
        .manager
        .open("p2", "", "2026-08-30T00:00:01Z")
        .unwrap();
    let maturity: String = connection.query_row("SELECT json_extract(payload_json, '$.planningMaturity') FROM artifact_payloads WHERE artifact_id='plan:bookplan:p2' AND version=1", [], |row| row.get(0)).unwrap();
    assert_eq!(maturity, "APPROVED");
    println!("B07_SEMANTIC_NEGATIVE_RESULT planned_opening=PASS missing_final_ref=PASS candidate_final=PASS late_chapters=PASS late_canon_evidence=PASS no_evidence=PASS actual_command_path=PASS status=PASS");
    println!("B07_SIGN_RESULT no_receipt=PASS wrong_verdict=PASS fake_artifact=PASS actual_pass=PASS opening_bound=PASS");
    println!("B07_BOOKPLAN_RESULT signing_pass=PASS seed_preserved=PASS approved=PASS static_chapters=0 status=PASS");
}

#[test]
fn b07_chapter_plan_reads_route_and_manifest_from_authority() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state
        .manager
        .create("p3", "B07 chapter", "2026-08-30T00:00:00Z")
        .unwrap();
    let volume_id =
        StableIdentityFactory::from_command(StableIdKind::Volume, "command:volume:p3").unwrap();
    let chapter_id =
        StableIdentityFactory::from_command(StableIdKind::Chapter, "command:chapter:p3").unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:3','p3',1,'Book','ACTIVE',?1,?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,'p3','book:3',1,'Volume','PLANNING',1,1,1000,?2,?2)", libsql_rusqlite::params![volume_id, "2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,'p3',?2,1,'Chapter','NORMAL','PLANNED',0,?3,?3)", libsql_rusqlite::params![chapter_id, volume_id, "2026-08-30T00:00:00Z"]).unwrap();
    let approved_book_plan = json!({"payloadSchema":"book-plan.r1","planningMaturity":"APPROVED","storyPromise":"promise","protagonistMacroDirection":"direction","endingBoundary":"end","mustAchieve":[],"mustNotBreak":[],"relationshipMacroDirections":[],"majorMysteryGoals":[],"futureReservations":[{"reservationId":"secret:v5","description":"reserved reveal","reservedForVolumeNo":5,"notBeforeVolumeNo":5,"prerequisites":[],"mayForeshadow":true,"mayReveal":true,"authorityLevel":"A"}],"volumeIntentSeeds":[],"openingHypotheses":[],"globalLengthBudget":{"preferredWords":1000}});
    let approved_hash = sha256_canonical(&approved_book_plan).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('plan:bookplan:p3','p3','BookPlan','PROJECT',?1,1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)", libsql_rusqlite::params!["{\"level\":\"PROJECT\"}", approved_hash, "2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('plan:bookplan:p3',1,?1,?2)", libsql_rusqlite::params![approved_book_plan.to_string(), approved_hash]).unwrap();
    connection.execute("INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES ('p3','BookPlan','PROJECT',?1,'plan:bookplan:p3',1,?2)", libsql_rusqlite::params!["{\"level\":\"PROJECT\"}", approved_hash]).unwrap();
    insert_project_authority(
        &connection,
        "p3",
        "story-constitution:p3",
        "StoryConstitution",
        &json!({"storyCore":{"protagonist":"Chapter"}}),
    );
    insert_project_authority(
        &connection,
        "p3",
        "main-contract:p3",
        "MainContract",
        &json!({"hardRules":["keep evidence"]}),
    );
    drop(connection);
    let intent = json!({"payloadSchema":"volume-intent.r1","volumeFunction":"opening","entryConditions":[],"mustAchieve":[{"id":"goal:p3","description":"完成章节证据核对"}],"mustNotBreak":[{"id":"rule:p3","description":"保持当前事实"}],"requiredExitConditions":[{"kind":"GOAL_ACHIEVED","value":"goal:p3"}],"futureReservationRefs":["secret:v5"],"crossVolumeResponsibilityRefs":[],"targetChapterRange":{"min":1,"preferred":1,"max":1},"targetWordRange":{"preferred":1000},"sourceBookPlanRef":{"artifactId":"plan:bookplan:p3","version":1,"payloadHash":approved_hash},"compiledFromProjectRevision":0,"sourceEvidenceRefs":["artifact:signing"],"semanticDigest":"0".repeat(64)});
    execute_command_impl(&state, json!({"type":"planning_create_volume_intent","commandId":"command:intent:p3","idempotencyKey":"key:intent:p3","projectId":"p3","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"volumeId":volume_id,"plan":intent}})).unwrap();
    let connection = state
        .manager
        .open("p3", "", "2026-08-30T00:00:00Z")
        .unwrap();
    let intent_ref = ai_novel_production_system_core::repositories::narrative::NarrativeRepository::canonical_head_ref(&connection, "p3", "VolumeIntent", &volume_id).unwrap().unwrap();
    drop(connection);
    let route = json!({"payloadSchema":"volume-plan.r1","volumeIntentRef":intent_ref,"routeVersion":1,"tasks":[{"taskId":"task:dependency","class":"REQUIRED","objective":"prepare","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"DONE"},"estimatedChapterCost":{"preferred":1}},{"taskId":"task:required","class":"REQUIRED","objective":"advance","dependsOnTaskIds":["task:dependency"],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"DONE"},"estimatedChapterCost":{"preferred":1}}],"routeInvariants":[],"requiredExitConditions":[],"capacityBudget":{"minChapters":1,"preferredChapters":1,"maxChapters":1}});
    execute_command_impl(&state, json!({"type":"planning_create_volume_plan","commandId":"command:route:p3","idempotencyKey":"key:route:p3","projectId":"p3","expectedRevision":1,"requestedAt":"2026-08-30T00:00:01Z","payload":{"volumeId":volume_id,"plan":route}})).unwrap();
    let thread_source_payload = json!({"source":"fixture"});
    let thread_source_hash = sha256_canonical(&thread_source_payload).unwrap();
    let connection = state
        .manager
        .open("p3", "", "2026-08-30T00:00:02Z")
        .unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:p3:thread-source','p3','VolumeContract','VOLUME',?1,1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)", libsql_rusqlite::params![serde_json::to_string(&json!({"level":"VOLUME","volumeId":volume_id})).unwrap(), thread_source_hash, "2026-08-30T00:00:02Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:p3:thread-source',1,?1,?2)", libsql_rusqlite::params![thread_source_payload.to_string(), thread_source_hash]).unwrap();
    connection.execute("INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,lifecycle,created_at,updated_at,resume_condition_json) VALUES ('obligation:p3','p3','VOLUME','PROMISE','A','artifact:p3:thread-source',1,'must continue','APPROVED',?1,?1,?2)", libsql_rusqlite::params!["2026-08-30T00:00:02Z", json!({"kind":"AFTER_CHAPTER","value":0}).to_string()]).unwrap();
    drop(connection);
    let scope_selection = json!({
        "taskRefs":["task:required"],
        "responsibilityRefs":["thread:obligation:p3"],
        "reservationRefs":["secret:v5"],
        "entityRefs":[]
    });
    let context_request = json!({"projectId":"p3","volumeId":volume_id,"chapterId":chapter_id,"scope":"CHAPTER","entityIds":[],"references":[],"scopeSelection":scope_selection});
    let manifest = query_context_manifest_impl(&state, context_request.clone()).unwrap();
    assert_eq!(manifest["futureReservationRefs"], json!(["secret:v5"]));
    assert!(manifest["omittedSoftSources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| source["reason"] == "undeclaredFields:[\"storyCore.mechanism\"]"));
    let context_hash = sha256_canonical(&manifest).unwrap();
    let context_artifact: ArtifactEnvelope = serde_json::from_value(json!({"artifactId":"context:p3:chapter","projectId":"p3","artifactType":"ContextManifest","scope":{"level":"CHAPTER","chapterId":chapter_id},"version":1,"schemaVersion":"m0a.artifact.v1","artifactStatus":"CANDIDATE","payload":manifest,"payloadHash":context_hash,"dependencyRefs":[],"provenance":{"createdBy":"B07::PlanningOwner","commandId":"command:context:p3"},"createdAt":"2026-08-30T00:00:02Z"})).unwrap();
    let model_run_id =
        StableIdentityFactory::from_command(StableIdKind::ModelRun, "command:model:p3").unwrap();
    let connection = state
        .manager
        .open("p3", "", "2026-08-30T00:00:02Z")
        .unwrap();
    ArtifactRepository::insert_candidate(&connection, "p3", &context_artifact).unwrap();
    connection
        .execute(
            "UPDATE artifacts SET artifact_status='COMMITTED' WHERE artifact_id=?1",
            [&context_artifact.artifact_id],
        )
        .unwrap();
    ArtifactRepository::promote_head(
        &connection,
        "p3",
        &ArtifactRef {
            artifact_id: context_artifact.artifact_id.clone(),
            version: 1,
            payload_hash: context_hash.clone(),
        },
        "ContextManifest",
        "CHAPTER",
        &String::from_utf8(
            ai_novel_production_system_core::hash::canonical_bytes(
                &json!({"level":"CHAPTER","chapterId":chapter_id}),
            )
            .unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    connection.execute("INSERT INTO model_runs(model_run_id,project_id,context_manifest_ref,provider,model,idempotency_key,capability,status,created_at,updated_at) VALUES (?1,'p3',?2,'fixture','fixture','planning:p3','PLAN_STRATEGY','COMPLETED',?3,?3)", libsql_rusqlite::params![model_run_id, context_artifact.artifact_id, "2026-08-30T00:00:02Z"]).unwrap();
    drop(connection);
    let plan = json!({"payloadSchema":"chapter-plan.r1","chapterId":chapter_id,"compiledFromProjectRevision":2,"compiledFromStateDigest":manifest["compiledFromStateDigest"],"contextManifestRef":{"artifactId":context_artifact.artifact_id,"version":1,"payloadHash":context_hash},"chapterGoal":"advance","activeTaskRefs":["task:required"],"continuityActions":[{"threadRef":"thread:obligation:p3","disposition":"ADVANCE"}],"requiredStateOutcomes":[],"forbiddenTransitions":[],"knowledgeBoundaries":[],"reservationRefs":["secret:v5"],"sceneExecution":[{"action":"FORESHADOW","reservationRef":"secret:v5"}],"horizonPreview":[{"slotNo":1,"objective":"one","tentative":true},{"slotNo":2,"objective":"two","tentative":true},{"slotNo":3,"objective":"three","tentative":true}]});
    let plan_digest = sha256_canonical(&plan).unwrap();
    let connection = state
        .manager
        .open("p3", "", "2026-08-30T00:00:03Z")
        .unwrap();
    connection.execute("INSERT INTO model_parse_results(parse_id,project_id,model_run_id,status,parsed_json,created_at) VALUES (?1,'p3',?2,'OK',?3,?4)", libsql_rusqlite::params![format!("parse:{model_run_id}"), model_run_id, json!({"outputDigest":plan_digest}).to_string(), "2026-08-30T00:00:03Z"]).unwrap();
    drop(connection);
    let dependency_blocked = execute_command_impl(
        &state,
        json!({"type":"planning_compile_chapter_plan","commandId":"command:chapter:dependency-blocked:p3","idempotencyKey":"key:chapter:dependency-blocked:p3","projectId":"p3","expectedRevision":2,"requestedAt":"2026-08-30T00:00:02Z","payload":{"chapterId":chapter_id,"modelRunId":model_run_id,"routeTaskIds":[],"scopeSelection":scope_selection,"plan":plan}}),
    );
    assert_eq!(dependency_blocked.unwrap_err(), "TASK_DEPENDENCY_UNMET");
    let connection = state
        .manager
        .open("p3", "", "2026-08-30T00:00:02Z")
        .unwrap();
    connection.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES ('task:dependency','p3','volume.task.progress','{\"status\":\"SATISFIED\"}',?1,'{}',?2,?2)", libsql_rusqlite::params![chapter_id, "2026-08-30T00:00:02Z"]).unwrap();
    drop(connection);
    let refreshed_manifest = query_context_manifest_impl(&state, context_request.clone()).unwrap();
    let refreshed_model_run_id = StableIdentityFactory::from_command(
        StableIdKind::ModelRun,
        "command:model:p3:dependency-satisfied",
    )
    .unwrap();
    let connection = state
        .manager
        .open("p3", "", "2026-08-30T00:00:03Z")
        .unwrap();
    let refreshed_context_ref = persist_context_manifest_version(
        &connection,
        "p3",
        &chapter_id,
        "context:p3:chapter:dependency-satisfied",
        1,
        &refreshed_manifest,
    );
    connection.execute("INSERT INTO model_runs(model_run_id,project_id,context_manifest_ref,provider,model,idempotency_key,capability,status,created_at,updated_at) VALUES (?1,'p3',?2,'fixture','fixture','planning:p3:dependency-satisfied','PLAN_STRATEGY','COMPLETED',?3,?3)", libsql_rusqlite::params![refreshed_model_run_id, refreshed_context_ref.artifact_id, "2026-08-30T00:00:03Z"]).unwrap();
    let mut refreshed_plan = plan.clone();
    refreshed_plan["compiledFromStateDigest"] =
        refreshed_manifest["compiledFromStateDigest"].clone();
    refreshed_plan["contextManifestRef"] = serde_json::to_value(&refreshed_context_ref).unwrap();
    let refreshed_plan_digest = sha256_canonical(&refreshed_plan).unwrap();
    connection.execute("INSERT INTO model_parse_results(parse_id,project_id,model_run_id,status,parsed_json,created_at) VALUES (?1,'p3',?2,'OK',?3,?4)", libsql_rusqlite::params![format!("parse:{refreshed_model_run_id}"), refreshed_model_run_id, json!({"outputDigest":refreshed_plan_digest}).to_string(), "2026-08-30T00:00:03Z"]).unwrap();
    drop(connection);
    let result = execute_command_impl(
        &state,
        json!({"type":"planning_compile_chapter_plan","commandId":"command:chapter:p3","idempotencyKey":"key:chapter:p3","projectId":"p3","expectedRevision":2,"requestedAt":"2026-08-30T00:00:03Z","payload":{"chapterId":chapter_id,"modelRunId":refreshed_model_run_id,"routeTaskIds":["task:caller-must-not-authorize"],"scopeSelection":scope_selection,"plan":refreshed_plan}}),
    );
    assert!(result.is_ok(), "{result:?}");
    let mut reveal_plan = refreshed_plan.clone();
    reveal_plan["sceneExecution"][0]["action"] = json!("REVEAL");
    let reveal_digest = sha256_canonical(&reveal_plan).unwrap();
    let connection = state
        .manager
        .open("p3", "", "2026-08-30T00:00:04Z")
        .unwrap();
    connection
        .execute(
            "UPDATE model_parse_results SET parsed_json=?1 WHERE model_run_id=?2",
            libsql_rusqlite::params![
                json!({"outputDigest":reveal_digest}).to_string(),
                refreshed_model_run_id
            ],
        )
        .unwrap();
    drop(connection);
    let before_future_reveal = {
        let connection = state
            .manager
            .open("p3", "", "2026-08-30T00:00:04Z")
            .unwrap();
        let heads: String = connection
            .query_row(
                "SELECT COALESCE(group_concat(head,'|'),'') FROM (SELECT artifact_type||':'||artifact_id||':'||version||':'||payload_hash AS head FROM canonical_heads WHERE project_id='p3' ORDER BY artifact_type,scope_key)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let revision: i64 = connection
            .query_row(
                "SELECT revision FROM project_meta WHERE project_id='p3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let chapters: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM artifacts WHERE project_id='p3' AND artifact_type='ChapterPlan'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let model_runs: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM model_runs WHERE project_id='p3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let canon_facts: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM canon_facts WHERE project_id='p3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let state_deltas: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM state_deltas WHERE project_id='p3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        (
            heads,
            revision,
            chapters,
            model_runs,
            canon_facts,
            state_deltas,
        )
    };
    let reveal = execute_command_impl(
        &state,
        json!({"type":"planning_compile_chapter_plan","commandId":"command:chapter:reveal:p3","idempotencyKey":"key:chapter:reveal:p3","projectId":"p3","expectedRevision":3,"requestedAt":"2026-08-30T00:00:04Z","payload":{"chapterId":chapter_id,"modelRunId":refreshed_model_run_id,"scopeSelection":scope_selection,"plan":reveal_plan}}),
    );
    assert_eq!(reveal.unwrap_err(), "FUTURE_RESERVATION_VIOLATION");
    let after_future_reveal = {
        let connection = state
            .manager
            .open("p3", "", "2026-08-30T00:00:04Z")
            .unwrap();
        let heads: String = connection
            .query_row(
                "SELECT COALESCE(group_concat(head,'|'),'') FROM (SELECT artifact_type||':'||artifact_id||':'||version||':'||payload_hash AS head FROM canonical_heads WHERE project_id='p3' ORDER BY artifact_type,scope_key)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let revision: i64 = connection
            .query_row(
                "SELECT revision FROM project_meta WHERE project_id='p3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let chapters: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM artifacts WHERE project_id='p3' AND artifact_type='ChapterPlan'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let model_runs: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM model_runs WHERE project_id='p3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let canon_facts: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM canon_facts WHERE project_id='p3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let state_deltas: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM state_deltas WHERE project_id='p3'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        (
            heads,
            revision,
            chapters,
            model_runs,
            canon_facts,
            state_deltas,
        )
    };
    assert_eq!(after_future_reveal, before_future_reveal);
    let connection = state
        .manager
        .open("p3", "", "2026-08-30T00:00:04Z")
        .unwrap();
    connection
        .execute(
            "UPDATE model_parse_results SET parsed_json=?1 WHERE model_run_id=?2",
            libsql_rusqlite::params![
                json!({"outputDigest":refreshed_plan_digest}).to_string(),
                refreshed_model_run_id
            ],
        )
        .unwrap();
    drop(connection);
    let connection = state
        .manager
        .open("p3", "", "2026-08-30T00:00:04Z")
        .unwrap();
    let canon_before: i64 = connection
        .query_row("SELECT COUNT(*) FROM canon_facts", [], |row| row.get(0))
        .unwrap();
    drop(connection);
    let connection = state
        .manager
        .open("p3", "", "2026-08-30T00:00:04Z")
        .unwrap();
    connection
        .execute(
            "UPDATE project_meta SET revision=4 WHERE project_id='p3'",
            [],
        )
        .unwrap();
    connection.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES ('task:required','p3','volume.task.progress','{\"status\":\"ACTIVE\"}',?1,'{}',?2,?2)", libsql_rusqlite::params![chapter_id, "2026-08-30T00:00:04Z"]).unwrap();
    let state_before_stale: i64 = connection
        .query_row("SELECT COUNT(*) FROM state_deltas", [], |row| row.get(0))
        .unwrap();
    drop(connection);
    let connection = state
        .manager
        .open("p3", "", "2026-08-30T00:00:04Z")
        .unwrap();
    let registry = CapabilityRegistry::read_only_r1();
    let spec = registry.specs.get("diagnose_planning_constraints").unwrap();
    let capability_id = spec.capability_id.clone();
    let capability_version = spec.version.clone();
    let spec_digest = spec.spec_digest.clone();
    let stale_input = json!({"projectId":"p3","projectRevision":4,"volumeId":volume_id,"currentChapter":1,"scope":"VOLUME","question":"为什么这个 ChapterPlan stale？","entityIds":[],"references":[]});
    let stale_result = BoundedCapabilityExecutor { registry }
        .execute(
            &RepositoryReadAdapter {
                connection: &connection,
                project_id: "p3",
            },
            &CapabilityCallEnvelope {
                call_id: "call:h02:stale".into(),
                capability_id,
                capability_version,
                spec_digest,
                mode: "DIAGNOSE".into(),
                project_id: "p3".into(),
                project_revision: 4,
                input_digest: sha256_canonical(&stale_input).unwrap(),
                input: stale_input,
                requested_bounds: RequestedBounds {
                    max_results: 10,
                    max_serialized_bytes: 65_536,
                },
            },
        )
        .unwrap();
    assert_eq!(
        stale_result.result["supportStatus"], "CONFLICTING",
        "{:#}",
        stale_result.result
    );
    assert_eq!(
        stale_result.result["provenance"]["stale"]["status"],
        "PLAN_STALE"
    );
    assert!(stale_result.result["provenance"]["chapterPlanRef"]["artifactId"].is_string());
    assert!(stale_result.result["provenance"]["futureReservations"].is_array());
    println!("H02_DIAGNOSTIC_RESULT stale_owner=PASS reservation_provenance=PASS thread_provenance=PASS status=PASS");
    drop(connection);
    let questions = [
        "为什么这一卷还没接近高潮？",
        "哪些 Required Task 还没完成？",
        "为什么这个 ChapterPlan stale？",
        "为什么这个秘密现在不能揭露？",
        "哪些跨卷承诺正在约束下一章？",
        "这一卷还有多少 Capacity？",
        "Planner 为什么选择当前 Task？",
    ];
    for question in questions {
        let session = support_session_impl(
            &state,
            json!({"projectId":"p3","volumeId":volume_id,"currentChapter":1,"scope":"VOLUME","question":question,"mode":"DIAGNOSE","entityIds":[],"references":[]}),
        )
        .unwrap();
        assert_eq!(session["plan"]["steps"].as_array().unwrap().len(), 2);
        assert_eq!(session["finding"]["diagnosisType"], "PLANNING_CONSTRAINTS");
        assert!(session["receipt"]["checkpointRef"].is_string());
        let diagnostic = &session["calls"][1]["result"];
        assert!(diagnostic["provenance"]["volumePlanRef"]["artifactId"].is_string());
        assert!(diagnostic["provenance"]["taskReality"].is_array());
        if question.contains("秘密") {
            assert!(diagnostic["provenance"]["futureReservations"]
                .as_array()
                .is_some_and(|items| !items.is_empty()));
        }
        if question.contains("承诺") {
            assert!(diagnostic["provenance"]["continuityThreads"]
                .as_array()
                .is_some_and(|items| !items.is_empty()));
        }
    }
    println!("H02_QUESTIONS_RESULT q1=PASS q2=PASS q3=PASS q4=PASS q5=PASS q6=PASS q7=PASS real_support=PASS receipt=PASS status=PASS");
    let stale = execute_command_impl(
        &state,
        json!({"type":"planning_compile_chapter_plan","commandId":"command:chapter:stale:p3","idempotencyKey":"key:chapter:stale:p3","projectId":"p3","expectedRevision":4,"requestedAt":"2026-08-30T00:00:05Z","payload":{"chapterId":chapter_id,"plan":plan}}),
    );
    assert_eq!(stale.unwrap_err(), "PLAN_STALE");
    let connection = state
        .manager
        .open("p3", "", "2026-08-30T00:00:05Z")
        .unwrap();
    let canon_after: i64 = connection
        .query_row("SELECT COUNT(*) FROM canon_facts", [], |row| row.get(0))
        .unwrap();
    let state_after: i64 = connection
        .query_row("SELECT COUNT(*) FROM state_deltas", [], |row| row.get(0))
        .unwrap();
    let chapter_plan_versions: i64 = connection
        .query_row("SELECT COUNT(*) FROM canonical_heads WHERE project_id='p3' AND artifact_type='ChapterPlan'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(canon_before, canon_after);
    assert_eq!(state_before_stale, state_after);
    assert_eq!(chapter_plan_versions, 1);
    println!("B07_CHAPTER_RESULT current_only=PASS context_bound=PASS reservation_gate=PASS task_dependency=PASS continuity_authority=PASS stale_precommit=PASS canon_unchanged=PASS model_output_bound=PASS status=PASS");
}

#[test]
fn h02_registry_digest_change_stales_old_checkpoint() {
    let registry = CapabilityRegistry::read_only_r1();
    let checkpoint = SupportCheckpoint {
        checkpoint_id: "checkpoint:h02".into(),
        session_id: "session:h02".into(),
        project_id: "p".into(),
        project_revision: 1,
        question_digest: "a".repeat(64),
        plan_digest: "b".repeat(64),
        completed_step_ids: vec![],
        capability_spec_digests: registry_digest_map(&registry),
        evidence_refs: vec![],
        last_result_digests: vec![],
        requested_mode: "DIAGNOSE".into(),
        requested_scope: "PROJECT".into(),
        plan_steps: vec![SupportTaskStep {
            step_id: "step:1".into(),
            capability_id: "query_plan_route".into(),
            status: SupportStepStatus::DONE,
            typed_args: json!({}),
        }],
        pending_authorization_ref: None,
        created_at: "2026-08-30T00:00:00Z".into(),
    };
    let mut changed = registry_digest_map(&registry);
    changed.insert("query_plan_route".into(), "f".repeat(64));
    assert_eq!(
        check_checkpoint_resume(&checkpoint, 1, &changed),
        ai_novel_production_system_core::harness::CheckpointResume::Stale
    );
    let mut same = BTreeMap::new();
    same.extend(registry_digest_map(&registry));
    assert_eq!(
        check_checkpoint_resume(&checkpoint, 1, &same),
        ai_novel_production_system_core::harness::CheckpointResume::Safe
    );
    println!("H02_CHECKPOINT_RESULT registry_digest_change=STALE unchanged=SAFE status=PASS");
}
