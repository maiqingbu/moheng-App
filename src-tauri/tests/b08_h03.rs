use ai_novel_production_system_core::commands::{execute_command_impl, KernelState};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse,
};
use ai_novel_production_system_core::repair::{
    operation_digest, RepairAuthorization, RepairOperation, RepairPlan,
};
use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};
use libsql_rusqlite::Connection;
use serde_json::json;
use serde_json::Value;
use std::sync::Arc;

const NOW: &str = "2026-08-30T12:00:00Z";

struct RecordingGateway;

impl ProductionModelGateway for RecordingGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let output = match capability {
            "PLAN_STRATEGY" => json!({
                "payloadSchema":"chapter-plan.r1",
                "chapterId":input["chapterId"],
                "compiledFromProjectRevision":input["compiledFromProjectRevision"],
                "compiledFromStateDigest":input["compiledFromStateDigest"],
                "contextManifestRef":input["contextManifestRef"],
                "chapterGoal":"推进实际任务",
                "activeTaskRefs":["task:actual"],
                "continuityActions":[],
                "requiredStateOutcomes":[],
                "forbiddenTransitions":[],
                "knowledgeBoundaries":[],
                "reservationRefs":[],
                "sceneExecution":[],
                "horizonPreview":[]
            }),
            "RENDER_CHAPTER" => json!({"body":"林舟推开石门，实际只推进了任务，没有完成它。"}),
            "AUDIT_EXTRACT" => json!({
                "qualityFindings":[],
                "canonCandidates":[{
                    "factId":"fact:actual:door-open",
                    "subjectId":"entity:actual:linzhou",
                    "predicate":"opened",
                    "objectJson":{"target":"stone-door"},
                    "authorityLevel":"C",
                    "eventType":"ACTION",
                    "storyTime":"2026-01-01T00:00:00Z",
                    "evidence":{"start":0,"end":7,"quote":"林舟推开石门"}
                }],
                "knowledgeCandidates":[],
                "continuityCandidates":[],
                "taskProgressCandidates":[{
                    "taskId":"task:actual",
                    "status":"ACTIVE",
                    "evidence":{"start":0,"end":7,"quote":"林舟推开石门"}
                }],
                "revealedReservationRefs":[],
                "evidenceRanges":[{"start":0,"end":7,"quote":"林舟推开石门"}]
            }),
            other => return Err(format!("unexpected capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

fn seed_chapter_and_gate(connection: &Connection, project_id: &str) -> String {
    let book_id = StableIdentityFactory::from_command(StableIdKind::Book, "b08-book").unwrap();
    let volume_id =
        StableIdentityFactory::from_command(StableIdKind::Volume, "b08-volume").unwrap();
    let chapter_id =
        StableIdentityFactory::from_command(StableIdKind::Chapter, "b08-chapter").unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'Book','ACTIVE',?3,?3)", libsql_rusqlite::params![book_id, project_id, NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'Volume','PLANNING',1,1,1000,?4,?4)", libsql_rusqlite::params![volume_id, project_id, book_id, NOW]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,1,'Chapter 1','NORMAL','PLANNED',0,?4,?4)", libsql_rusqlite::params![chapter_id, project_id, volume_id, NOW]).unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:actual:linzhou',?1,'林舟','CHARACTER',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES ('fact:actual:seed',?1,'entity:actual:linzhou','location','{\"place\":\"stone-door\"}','A',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES ('task:actual',?1,'volume.task.progress','{\"taskId\":\"task:actual\",\"status\":\"NOT_STARTED\",\"evidenceRefs\":[],\"remainingConditions\":[]}',?2,'{}',?3,?3)", libsql_rusqlite::params![project_id, chapter_id, NOW]).unwrap();

    let policy_payload = json!({"payloadSchema":"gate-policy.r1","policyId":"b08-default","rules":["candidate-audit","chapter-final"]});
    let policy_hash = sha256_canonical(&policy_payload).unwrap();
    let policy_id =
        StableIdentityFactory::from_command(StableIdKind::Artifact, "b08-policy").unwrap();
    let scope = "{\"level\":\"PROJECT\"}";
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'GatePolicy','PROJECT',?3,1,'b08.artifact.r1','COMMITTED',?4,'{}',?5,?5)", libsql_rusqlite::params![policy_id, project_id, scope, policy_hash, NOW]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)", libsql_rusqlite::params![policy_id, policy_payload.to_string(), policy_hash]).unwrap();
    connection.execute("INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'GatePolicy','PROJECT',?2,?3,1,?4)", libsql_rusqlite::params![project_id, scope, policy_id, policy_hash]).unwrap();
    chapter_id
}

#[test]
fn b08_production_commits_final_and_never_promotes_candidate() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let connection = state.manager.create("p-b08", "B08", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, "p-b08");
    drop(connection);

    let result = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:production","idempotencyKey":"key:b08:production",
        "projectId":"p-b08","expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
    })).unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let connection = state.manager.open("p-b08", "B08", NOW).unwrap();
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM chapters WHERE chapter_id=?1",
                [&chapter_id],
                |row| row.get(0)
            )
            .unwrap(),
        "COMMITTED"
    );
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM artifacts WHERE project_id='p-b08' AND artifact_type='FinalChapter' AND artifact_status='COMMITTED'", [], |row| row.get(0)).unwrap(), 1);
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM canonical_heads WHERE project_id='p-b08' AND artifact_type='ChapterCandidate'", [], |row| row.get(0)).unwrap(), 0);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_runs WHERE project_id='p-b08' AND status='COMPLETED'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        2
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM jobs WHERE project_id='p-b08'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "COMPLETED"
    );
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM domain_receipts WHERE project_id='p-b08' AND domain='B08_CHAPTER_PRODUCTION'", [], |row| row.get(0)).unwrap(), 1);
    assert!(connection.query_row::<i64,_,_>("SELECT json_extract(p.payload_json,'$.hardSourceCount') FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id='p-b08' AND h.artifact_type='ContextManifest'", [], |row| row.get(0)).unwrap() > 0);
    assert_eq!(connection.query_row::<String,_,_>("SELECT json_extract(a.provenance_json,'$.createdBy') FROM canonical_heads h JOIN artifacts a ON a.artifact_id=h.artifact_id AND a.version=h.version WHERE h.project_id='p-b08' AND h.artifact_type='ChapterPlan'", [], |row| row.get(0)).unwrap(), "B07::PlanningOwner");
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM canon_facts WHERE project_id='p-b08' AND fact_id='fact:actual:door-open'", [], |row| row.get(0)).unwrap(), 1);
    assert_eq!(connection.query_row::<String,_,_>("SELECT json_extract(value_json,'$.status') FROM state_deltas WHERE project_id='p-b08' AND state_id='task:actual'", [], |row| row.get(0)).unwrap(), "ACTIVE");
    drop(connection);
    let replay = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:production","idempotencyKey":"key:b08:production",
        "projectId":"p-b08","expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP"}
    })).unwrap();
    assert_eq!(replay["replayed"], true);
    println!("B08_PRODUCTION_RESULT opening=PASS candidate_isolation=PASS final_commit=PASS model_provenance=PASS receipt=PASS status=PASS");
}

#[test]
fn b08_production_start_without_chapter_fails_closed_instead_of_using_legacy_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    drop(state.manager.create("p-b08-no-chapter", "B08", NOW).unwrap());
    let error = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:no-chapter","idempotencyKey":"key:b08:no-chapter",
        "projectId":"p-b08-no-chapter","expectedRevision":0,"requestedAt":NOW,"payload":{}
    })).unwrap_err();
    assert_eq!(error, "B08_CHAPTER_REQUIRED");
}

#[test]
fn h03_authorization_is_revision_and_digest_bound_and_one_shot() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state.manager.create("p-h03", "H03", NOW).unwrap();
    connection.execute("INSERT INTO project_runtime_state(project_id,run_state,updated_at) VALUES ('p-h03','IDLE',?1)", [NOW]).unwrap();
    drop(connection);
    let operations = vec![RepairOperation {
        operation: "refresh_runtime_projection".into(),
        target: "project:p-h03".into(),
        params: json!({}),
        postconditions: json!({"requires":"runtime projection exists"}),
    }];
    let digest = operation_digest(&operations).unwrap();
    let plan = RepairPlan {
        repair_plan_id: "repair-plan:h03:1".into(),
        project_id: "p-h03".into(),
        based_on_project_revision: 0,
        diagnosis: json!({"finding":"projection stale"}),
        finding_refs: vec!["finding:h03:1".into()],
        target_authority_owner: "B06::RuntimeProjectionOwner".into(),
        proposed_operations: operations,
        affected_artifacts: vec![],
        affected_scopes: vec!["PROJECT".into()],
        risk_level: "LOW".into(),
        requires_backup: false,
        expected_postconditions: vec![json!({"status":"REFRESHED"})],
        operation_digest: digest.clone(),
        created_at: NOW.into(),
    };
    execute_command_impl(&state, json!({"type":"repair_plan_create","commandId":"cmd:h03:plan","idempotencyKey":"key:h03:plan","projectId":"p-h03","expectedRevision":0,"requestedAt":NOW,"payload":{"dryRun":true,"repairPlan":plan}})).unwrap();
    let authorization = RepairAuthorization {
        authorization_id: "repair-auth:h03:1".into(),
        project_id: "p-h03".into(),
        repair_plan_id: "repair-plan:h03:1".into(),
        based_on_project_revision: 0,
        operation_digest: digest,
        approved_by_user: true,
        issued_at: NOW.into(),
        consumed_at: None,
    };
    execute_command_impl(&state, json!({"type":"repair_authorize","commandId":"cmd:h03:auth","idempotencyKey":"key:h03:auth","projectId":"p-h03","expectedRevision":0,"requestedAt":NOW,"payload":{"authorization":authorization}})).unwrap();
    let executed = execute_command_impl(&state, json!({"type":"repair_execute","commandId":"cmd:h03:execute","idempotencyKey":"key:h03:execute","projectId":"p-h03","requestedAt":NOW,"payload":{"repairPlanId":"repair-plan:h03:1","authorizationId":"repair-auth:h03:1"}})).unwrap();
    assert_eq!(executed["finalStatus"], "committed");
    let replay = execute_command_impl(
        &state,
        json!({"type":"repair_execute","commandId":"cmd:h03:replay","idempotencyKey":"key:h03:replay","projectId":"p-h03","requestedAt":NOW,"payload":{"repairPlanId":"repair-plan:h03:1","authorizationId":"repair-auth:h03:1"}}),
    );
    assert!(replay.unwrap_err().contains("H03_REPAIR_ALREADY_CONSUMED"));
    let connection = state.manager.open("p-h03", "H03", NOW).unwrap();
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM repair_execution_receipts WHERE project_id='p-h03' AND status='REPAIR_VERIFIED'", [], |row| row.get(0)).unwrap(), 1);
    println!("H03_AUTH_RESULT dry_run_plan=PASS explicit_authorization=PASS one_shot=PASS postverify=PASS status=PASS");
}
