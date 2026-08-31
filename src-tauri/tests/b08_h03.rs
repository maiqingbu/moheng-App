use ai_novel_production_system_core::commands::query_context_manifest_impl;
use ai_novel_production_system_core::commands::{execute_command_impl, KernelState};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::production::{
    reconciliation_for_actual_state, ProductionModelGateway, ProductionModelResponse,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repair::{
    operation_digest_with_postconditions, OwnerExecution, RepairAuthorization, RepairOperation,
    RepairPlan, TypedRepairOwner,
};
use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};
use libsql_rusqlite::Connection;
use serde_json::json;
use serde_json::Value;
use std::path::PathBuf;
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
                "horizonPreview":[
                    {"slotNo":1,"objective":"推进当前行动","tentative":true},
                    {"slotNo":2,"objective":"观察行动后果","tentative":true},
                    {"slotNo":3,"objective":"准备下一冲突","tentative":true}
                ]
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
                    "evidence":{"start":0,"end":6,"quote":"林舟推开石门"}
                }],
                "knowledgeCandidates":[],
                "continuityCandidates":[],
                "taskProgressCandidates":[{
                    "taskId":"task:actual",
                    "status":"ACTIVE",
                    "evidence":{"start":0,"end":6,"quote":"林舟推开石门"}
                }],
                "revealedReservationRefs":[],
                "evidenceRanges":[{"start":0,"end":6,"quote":"林舟推开石门"}]
            }),
            other => return Err(format!("unexpected capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

struct MutatingGateway {
    database: PathBuf,
}

struct CommitFaultGateway;

impl ProductionModelGateway for CommitFaultGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        RecordingGateway.invoke(capability, input)
    }

    fn fault_injection(&self) -> Option<&str> {
        Some("dependency_source_hash")
    }
}

struct PromiseGateway;

impl ProductionModelGateway for PromiseGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "PLAN_STRATEGY" {
            response.output["continuityActions"] = json!([{
                "threadRef":"thread:promise:old","disposition":"ADVANCE"
            }]);
        }
        if capability == "AUDIT_EXTRACT" {
            response.output["continuityCandidates"] = json!([{
                "threadRef":"thread:promise:old","disposition":"ADVANCE",
                "evidence":{"start":0,"end":6,"quote":"林舟推开石门"}
            }]);
        }
        Ok(response)
    }
}

struct KnowledgeGateway {
    contradictory: bool,
}

struct AuditVariantGateway {
    variant: &'static str,
}

struct FailingPostverifyOwner;

impl TypedRepairOwner for FailingPostverifyOwner {
    fn execute(&self, _operation: &RepairOperation) -> Result<OwnerExecution, String> {
        Ok(OwnerExecution {
            owner: "TestTypedOwner".into(),
            owner_receipt_ref: Some("owner-receipt:test".into()),
        })
    }

    fn verify(&self, operation: &RepairOperation) -> Result<Value, String> {
        Ok(json!({"operation":operation.operation,"status":"FAIL","observed":"still stale"}))
    }
}

impl ProductionModelGateway for AuditVariantGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            match self.variant {
                "reservation" => {
                    response.output["revealedReservationRefs"] =
                        json!(["reservation:not-authorized"])
                }
                "blocked" => {
                    response.output["taskProgressCandidates"][0]["status"] = json!("BLOCKED")
                }
                _ => {}
            }
        }
        Ok(response)
    }
}

impl ProductionModelGateway for KnowledgeGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            response.output["canonCandidates"] = json!([{
                "factId":"fact:actual:flashback","subjectId":"entity:actual:linzhou",
                "predicate":"remembered","objectJson":{"memory":"stone-door"},"authorityLevel":"C",
                "eventType":"FLASHBACK","storyTime":"2000-01-01T00:00:00Z",
                "evidence":{"start":0,"end":6,"quote":"林舟推开石门"}
            }]);
            response.output["knowledgeCandidates"] = json!([{
                "knowledgeId":"knowledge:actual:flashback","knowerId":"entity:actual:linzhou",
                "factId":"fact:actual:flashback","epistemicState":"KNOWS",
                "visibilityScope":"CHARACTER:entity:actual:linzhou",
                "acquiredStoryTime":if self.contradictory {"1999-01-01T00:00:00Z"} else {"2001-01-01T00:00:00Z"},
                "evidence":{"start":0,"end":6,"quote":"林舟推开石门"}
            }]);
        }
        Ok(response)
    }
}

impl ProductionModelGateway for MutatingGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        if capability == "RENDER_CHAPTER" {
            let connection =
                ai_novel_production_system_core::project_db::connection::open_connection(
                    &self.database,
                    false,
                )
                .map_err(|error| error.to_string())?;
            connection.execute("UPDATE canon_facts SET object_json='{\"place\":\"changed-during-render\"}' WHERE fact_id='fact:actual:seed'", [])
                .map_err(|error| error.to_string())?;
            connection
                .execute(
                    "UPDATE project_meta SET revision=revision+1 WHERE project_id='p-b08-stale'",
                    [],
                )
                .map_err(|error| error.to_string())?;
        }
        RecordingGateway.invoke(capability, input)
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
    connection.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES ('task:actual',?1,'volume.task.progress','{\"taskId\":\"task:actual\",\"status\":\"ACTIVE\",\"evidenceRefs\":[],\"remainingConditions\":[]}',?2,'{}',?3,?3)", libsql_rusqlite::params![project_id, chapter_id, NOW]).unwrap();

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
        3
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
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM canonical_heads WHERE project_id='p-b08' AND artifact_type='VolumePlan'", [], |row| row.get(0)).unwrap(), 0);
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
fn b08_long_form_reuses_exact_current_plan_and_bound_context_in_same_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let connection = state.manager.create("p-b08-long", "B08 long", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, "p-b08-long");
    drop(connection);
    let context = query_context_manifest_impl(
        &state,
        json!({
            "projectId":"p-b08-long","chapterId":chapter_id,"executionMode":"LONG_FORM",
            "entityIds":["entity:actual:linzhou"],"references":[]
        }),
    )
    .unwrap();
    let context_id = "context:b08:long";
    let context_hash = sha256_canonical(&context).unwrap();
    let plan_id = "plan:chapterplan:long";
    let plan = json!({
        "payloadSchema":"chapter-plan.r1","chapterId":chapter_id,"compiledFromProjectRevision":0,
        "compiledFromStateDigest":context["compiledFromStateDigest"],
        "contextManifestRef":{"artifactId":context_id,"version":1,"payloadHash":context_hash},
        "chapterGoal":"复用当前正式计划","activeTaskRefs":["task:actual"],"continuityActions":[],
        "requiredStateOutcomes":[],"forbiddenTransitions":[],"knowledgeBoundaries":[],"reservationRefs":[],
        "sceneExecution":[],"horizonPreview":[
            {"slotNo":1,"objective":"继续行动","tentative":true},
            {"slotNo":2,"objective":"检查后果","tentative":true},
            {"slotNo":3,"objective":"准备冲突","tentative":true}
        ]
    });
    let plan_hash = sha256_canonical(&plan).unwrap();
    let scope = json!({"level":"CHAPTER","chapterId":chapter_id}).to_string();
    let connection = state.manager.open("p-b08-long", "B08 long", NOW).unwrap();
    for (artifact_id, artifact_type, payload, hash) in [
        (
            context_id,
            "ContextManifest",
            context.clone(),
            context_hash.clone(),
        ),
        (plan_id, "ChapterPlan", plan.clone(), plan_hash.clone()),
    ] {
        connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,'p-b08-long',?2,'CHAPTER',?3,1,'m0a.artifact.v1','COMMITTED',?4,?5,?6,?6)", libsql_rusqlite::params![artifact_id,artifact_type,scope,hash,json!({"createdBy":if artifact_type=="ChapterPlan" {"B07::PlanningOwner"} else {"B06::ContextCompilerOwner"}}).to_string(),NOW]).unwrap();
        connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)", libsql_rusqlite::params![artifact_id,payload.to_string(),hash]).unwrap();
        connection.execute("INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES ('p-b08-long',?1,'CHAPTER',?2,?3,1,?4)", libsql_rusqlite::params![artifact_type,scope,artifact_id,hash]).unwrap();
    }
    drop(connection);
    let result = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:long","idempotencyKey":"key:b08:long",
        "projectId":"p-b08-long","expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou"]}
    })).unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let connection = state.manager.open("p-b08-long", "B08 long", NOW).unwrap();
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_runs WHERE project_id='p-b08-long'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        2
    );
    assert_eq!(connection.query_row::<String,_,_>("SELECT json_extract(p.payload_json,'$.contextManifestRef.artifactId') FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id='p-b08-long' AND h.artifact_type='FinalChapter'", [], |row| row.get(0)).unwrap(), context_id);
}

#[test]
fn b08_production_start_without_chapter_fails_closed_instead_of_using_legacy_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    drop(
        state
            .manager
            .create("p-b08-no-chapter", "B08", NOW)
            .unwrap(),
    );
    let error = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:no-chapter","idempotencyKey":"key:b08:no-chapter",
        "projectId":"p-b08-no-chapter","expectedRevision":0,"requestedAt":NOW,"payload":{}
    })).unwrap_err();
    assert_eq!(error, "B08_CHAPTER_REQUIRED");
}

#[test]
fn b08_revalidates_relevant_state_after_render_and_blocks_stale_final_commit() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let database = paths.project_dir("p-b08-stale").unwrap().join("project.db");
    let state = KernelState::with_production_gateway(paths, Arc::new(MutatingGateway { database }));
    let connection = state
        .manager
        .create("p-b08-stale", "B08 stale", NOW)
        .unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, "p-b08-stale");
    drop(connection);
    let error = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:stale","idempotencyKey":"key:b08:stale",
        "projectId":"p-b08-stale","expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
    })).unwrap_err();
    assert_eq!(error, "PLAN_STALE:RECOMPILE_REQUIRED");
    let connection = state.manager.open("p-b08-stale", "B08 stale", NOW).unwrap();
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM artifacts WHERE project_id='p-b08-stale' AND artifact_type='FinalChapter'", [], |row| row.get(0)).unwrap(), 0);
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM canon_facts WHERE project_id='p-b08-stale' AND fact_id='fact:actual:door-open'", [], |row| row.get(0)).unwrap(), 0);
}

#[test]
fn b08_cross_volume_promise_keeps_old_evidence_and_adds_actual_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let state =
        KernelState::with_production_gateway(AppPaths::new(temp.path()), Arc::new(PromiseGateway));
    let connection = state
        .manager
        .create("p-b08-promise", "B08 promise", NOW)
        .unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, "p-b08-promise");
    let (policy_id, policy_version): (String,i64) = connection.query_row(
        "SELECT artifact_id,version FROM artifacts WHERE project_id='p-b08-promise' AND artifact_type='GatePolicy' LIMIT 1",
        [], |row| Ok((row.get(0)?,row.get(1)?))
    ).unwrap();
    connection.execute("INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,prerequisites_json,lifecycle,created_at,updated_at,source_event_id,resume_condition_json) VALUES ('promise:old','p-b08-promise','BOOK','PROMISE','A',?1,?2,'旧卷承诺','[]','PROGRESSING',?3,?3,NULL,'{\"kind\":\"ENTITY_PRESENT\",\"value\":\"entity:actual:linzhou\"}')", libsql_rusqlite::params![policy_id,policy_version,NOW]).unwrap();
    connection.execute("INSERT INTO obligation_evidence(evidence_id,project_id,obligation_id,evidence_type,artifact_id,version,created_at) VALUES ('obligation-evidence:old','p-b08-promise','promise:old','PROGRESS',?1,?2,?3)", libsql_rusqlite::params![policy_id,policy_version,NOW]).unwrap();
    drop(connection);
    let result = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:promise","idempotencyKey":"key:b08:promise",
        "projectId":"p-b08-promise","expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP",
            "entityIds":["entity:actual:linzhou"],"presentEntityIds":["entity:actual:linzhou"],
            "references":["promise:old"]}
    })).unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let connection = state
        .manager
        .open("p-b08-promise", "B08 promise", NOW)
        .unwrap();
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT lifecycle FROM narrative_obligations WHERE obligation_id='promise:old'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "PROGRESSING"
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM obligation_evidence WHERE obligation_id='promise:old'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        2
    );
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM obligation_evidence WHERE evidence_id='obligation-evidence:old'", [], |row| row.get(0)).unwrap(), 1);
}

#[test]
fn b08_flashback_knowledge_persists_provenance_and_future_contradiction_blocks() {
    for (suffix, contradictory, should_pass) in [("valid", false, true), ("future", true, false)] {
        let temp = tempfile::tempdir().unwrap();
        let project_id = format!("p-b08-knowledge-{suffix}");
        let state = KernelState::with_production_gateway(
            AppPaths::new(temp.path()),
            Arc::new(KnowledgeGateway { contradictory }),
        );
        let connection = state
            .manager
            .create(&project_id, "B08 knowledge", NOW)
            .unwrap();
        let chapter_id = seed_chapter_and_gate(&connection, &project_id);
        drop(connection);
        let result = execute_command_impl(
            &state,
            json!({
                "type":"production_start","commandId":format!("cmd:b08:knowledge:{suffix}"),
                "idempotencyKey":format!("key:b08:knowledge:{suffix}"),"projectId":project_id,
                "expectedRevision":0,"requestedAt":NOW,
                "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
            }),
        );
        if should_pass {
            assert_eq!(result.unwrap()["finalStatus"], "committed");
            let connection = state
                .manager
                .open(&project_id, "B08 knowledge", NOW)
                .unwrap();
            let (source, version): (Option<String>,Option<i64>) = connection.query_row(
                "SELECT source_artifact_id,source_version FROM canon_knowledge WHERE knowledge_id='knowledge:actual:flashback'",
                [], |row| Ok((row.get(0)?,row.get(1)?))
            ).unwrap();
            assert!(source.is_some());
            assert_eq!(version, Some(1));
        } else {
            assert_eq!(
                result.unwrap_err(),
                "B08_KNOWLEDGE_FUTURE_EVENT_CONTRADICTION"
            );
            let connection = state
                .manager
                .open(&project_id, "B08 knowledge", NOW)
                .unwrap();
            assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='FinalChapter'", [&project_id], |row| row.get(0)).unwrap(), 0);
        }
    }
}

#[test]
fn b08_reservation_gate_blocks_and_reconciliation_uses_actual_task_state() {
    for (suffix, variant, expected_error, expected_reconciliation) in [
        (
            "reservation",
            "reservation",
            Some("B08_RESERVATION_VIOLATION"),
            None,
        ),
        ("blocked", "blocked", None, Some("RECOMPILE_ROLLING_WINDOW")),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let project_id = format!("p-b08-audit-{suffix}");
        let state = KernelState::with_production_gateway(
            AppPaths::new(temp.path()),
            Arc::new(AuditVariantGateway { variant }),
        );
        let connection = state.manager.create(&project_id, "B08 audit", NOW).unwrap();
        let chapter_id = seed_chapter_and_gate(&connection, &project_id);
        drop(connection);
        let result = execute_command_impl(
            &state,
            json!({
                "type":"production_start","commandId":format!("cmd:b08:audit:{suffix}"),
                "idempotencyKey":format!("key:b08:audit:{suffix}"),"projectId":project_id,
                "expectedRevision":0,"requestedAt":NOW,
                "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
            }),
        );
        if let Some(error) = expected_error {
            assert_eq!(result.unwrap_err(), error);
        } else {
            assert_eq!(result.unwrap()["finalStatus"], "committed");
            let connection = state.manager.open(&project_id, "B08 audit", NOW).unwrap();
            let reconciliation: String = connection.query_row(
                "SELECT json_extract(p.payload_json,'$.reconciliation') FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ProductionReceipt'",
                [&project_id], |row| row.get(0)
            ).unwrap();
            assert_eq!(Some(reconciliation.as_str()), expected_reconciliation);
        }
    }
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
    let expected_postconditions = vec![json!({"status":"REFRESHED"})];
    let digest =
        operation_digest_with_postconditions(&operations, &expected_postconditions).unwrap();
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
        expected_postconditions,
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

fn create_h03_plan(
    state: &KernelState,
    project_id: &str,
    plan_id: &str,
    operation: RepairOperation,
    revision: i64,
) -> String {
    let operations = vec![operation];
    let expected_postconditions = vec![json!({"status":"PASS"})];
    let digest =
        operation_digest_with_postconditions(&operations, &expected_postconditions).unwrap();
    let plan = RepairPlan {
        repair_plan_id: plan_id.into(),
        project_id: project_id.into(),
        based_on_project_revision: revision,
        diagnosis: json!({"finding":"typed repair required"}),
        finding_refs: vec![format!("finding:{plan_id}")],
        target_authority_owner: "TypedRepairOwner".into(),
        proposed_operations: operations,
        affected_artifacts: vec![],
        affected_scopes: vec!["PROJECT".into()],
        risk_level: "LOW".into(),
        requires_backup: false,
        expected_postconditions,
        operation_digest: digest.clone(),
        created_at: NOW.into(),
    };
    execute_command_impl(state, json!({
        "type":"repair_plan_create","commandId":format!("cmd:{plan_id}:plan"),
        "idempotencyKey":format!("key:{plan_id}:plan"),"projectId":project_id,
        "expectedRevision":revision,"requestedAt":NOW,"payload":{"dryRun":true,"repairPlan":plan}
    })).unwrap();
    digest
}

fn authorize_h03(
    state: &KernelState,
    project_id: &str,
    plan_id: &str,
    authorization_id: &str,
    digest: &str,
    revision: i64,
) {
    execute_command_impl(
        state,
        json!({
            "type":"repair_authorize","commandId":format!("cmd:{authorization_id}"),
            "idempotencyKey":format!("key:{authorization_id}"),"projectId":project_id,
            "expectedRevision":revision,"requestedAt":NOW,"payload":{"authorization":{
                "authorizationId":authorization_id,"projectId":project_id,"repairPlanId":plan_id,
                "basedOnProjectRevision":revision,"operationDigest":digest,"approvedByUser":true,
                "issuedAt":NOW
            }}
        }),
    )
    .unwrap();
}

#[test]
fn h03_execute_without_authorization_and_stale_authorization_fail_before_owner() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state.manager.create("p-h03-negative", "H03", NOW).unwrap();
    connection.execute("INSERT INTO project_runtime_state(project_id,run_state,updated_at) VALUES ('p-h03-negative','IDLE',?1)", [NOW]).unwrap();
    drop(connection);
    let plan_id = "repair-plan:h03:no-auth";
    let digest = create_h03_plan(
        &state,
        "p-h03-negative",
        plan_id,
        RepairOperation {
            operation: "refresh_runtime_projection".into(),
            target: "project:p-h03-negative".into(),
            params: json!({}),
            postconditions: json!({"kind":"runtime_projection_current"}),
        },
        0,
    );
    let no_auth = execute_command_impl(&state, json!({
        "type":"repair_execute","commandId":"cmd:h03:no-auth","idempotencyKey":"key:h03:no-auth",
        "projectId":"p-h03-negative","requestedAt":NOW,
        "payload":{"repairPlanId":plan_id,"authorizationId":"repair-auth:missing"}
    })).unwrap_err();
    assert!(no_auth.contains("H03_AUTHORIZATION_REQUIRED"));

    authorize_h03(
        &state,
        "p-h03-negative",
        plan_id,
        "repair-auth:h03:stale",
        &digest,
        0,
    );
    let connection = state.manager.open("p-h03-negative", "H03", NOW).unwrap();
    connection
        .execute(
            "UPDATE project_meta SET revision=1 WHERE project_id='p-h03-negative'",
            [],
        )
        .unwrap();
    drop(connection);
    let stale = execute_command_impl(
        &state,
        json!({
            "type":"repair_execute","commandId":"cmd:h03:stale","idempotencyKey":"key:h03:stale",
            "projectId":"p-h03-negative","requestedAt":NOW,
            "payload":{"repairPlanId":plan_id,"authorizationId":"repair-auth:h03:stale"}
        }),
    )
    .unwrap_err();
    assert!(stale.contains("H03_STALE_AUTHORIZATION"));
}

#[test]
fn h03_owner_failure_consumes_authorization_and_persists_failed_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    drop(state.manager.create("p-h03-failure", "H03", NOW).unwrap());
    let plan_id = "repair-plan:h03:owner-failure";
    let digest = create_h03_plan(
        &state,
        "p-h03-failure",
        plan_id,
        RepairOperation {
            operation: "recompile_chapter_plan".into(),
            target: "chapter:missing".into(),
            params: json!({}),
            postconditions: json!({"kind":"chapter_plan_current"}),
        },
        0,
    );
    authorize_h03(
        &state,
        "p-h03-failure",
        plan_id,
        "repair-auth:h03:owner-failure",
        &digest,
        0,
    );
    let failed = execute_command_impl(&state, json!({
        "type":"repair_execute","commandId":"cmd:h03:owner-failure","idempotencyKey":"key:h03:owner-failure",
        "projectId":"p-h03-failure","requestedAt":NOW,
        "payload":{"repairPlanId":plan_id,"authorizationId":"repair-auth:h03:owner-failure"}
    })).unwrap();
    assert_eq!(failed["receipt"]["status"], "REPAIR_VERIFICATION_FAILED");
    let connection = state.manager.open("p-h03-failure", "H03", NOW).unwrap();
    assert!(connection.query_row::<Option<String>,_,_>("SELECT consumed_at FROM repair_authorizations WHERE authorization_id='repair-auth:h03:owner-failure'", [], |row| row.get(0)).unwrap().is_some());
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM repair_execution_receipts WHERE authorization_id='repair-auth:h03:owner-failure'", [], |row| row.get(0)).unwrap(), 1);
    drop(connection);
    let replay = execute_command_impl(&state, json!({
        "type":"repair_execute","commandId":"cmd:h03:owner-failure:replay","idempotencyKey":"key:h03:owner-failure:replay",
        "projectId":"p-h03-failure","requestedAt":NOW,
        "payload":{"repairPlanId":plan_id,"authorizationId":"repair-auth:h03:owner-failure"}
    })).unwrap_err();
    assert!(replay.contains("H03_REPAIR_ALREADY_CONSUMED"));
}

#[test]
fn h03_postverify_requery_failure_is_receipted_and_not_reported_as_repaired() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state
        .manager
        .create("p-h03-postverify", "H03 postverify", NOW)
        .unwrap();
    connection.execute("INSERT INTO project_runtime_state(project_id,run_state,updated_at) VALUES ('p-h03-postverify','IDLE',?1)", [NOW]).unwrap();
    drop(connection);
    let plan_id = "repair-plan:h03:postverify";
    let auth_id = "repair-auth:h03:postverify";
    let digest = create_h03_plan(
        &state,
        "p-h03-postverify",
        plan_id,
        RepairOperation {
            operation: "refresh_runtime_projection".into(),
            target: "p-h03-postverify".into(),
            params: json!({}),
            postconditions: json!({"kind":"runtime_projection_current"}),
        },
        0,
    );
    authorize_h03(&state, "p-h03-postverify", plan_id, auth_id, &digest, 0);
    let connection = state
        .manager
        .open("p-h03-postverify", "H03 postverify", NOW)
        .unwrap();
    let receipt = ai_novel_production_system_core::repair::execute(
        &connection,
        &FailingPostverifyOwner,
        "p-h03-postverify",
        plan_id,
        auth_id,
        NOW,
    )
    .unwrap();
    assert_eq!(receipt.status, "REPAIR_VERIFICATION_FAILED");
    assert_eq!(receipt.owner, "TestTypedOwner");
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM repair_execution_receipts WHERE authorization_id=?1",
                [auth_id],
                |row| row.get(0)
            )
            .unwrap(),
        "REPAIR_VERIFICATION_FAILED"
    );
}

#[test]
fn h03_stage_matrix_rejects_b09_route_and_keeps_change_request_proposal_only() {
    for (suffix, operation, expected) in [
        (
            "route",
            "replan_volume_route",
            "H03_CAPABILITY_UNAVAILABLE_BEFORE_B09",
        ),
        (
            "change",
            "execute_change_request",
            "H03_CHANGE_REQUEST_PROPOSAL_ONLY",
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let project_id = format!("p-h03-{suffix}");
        let state = KernelState::new(AppPaths::new(temp.path()));
        drop(state.manager.create(&project_id, "H03", NOW).unwrap());
        let plan_id = format!("repair-plan:h03:{suffix}");
        let auth_id = format!("repair-auth:h03:{suffix}");
        let digest = create_h03_plan(
            &state,
            &project_id,
            &plan_id,
            RepairOperation {
                operation: operation.into(),
                target: project_id.clone(),
                params: json!({}),
                postconditions: json!({}),
            },
            0,
        );
        authorize_h03(&state, &project_id, &plan_id, &auth_id, &digest, 0);
        let error = execute_command_impl(&state, json!({
            "type":"repair_execute","commandId":format!("cmd:h03:{suffix}:execute"),
            "idempotencyKey":format!("key:h03:{suffix}:execute"),"projectId":project_id,"requestedAt":NOW,
            "payload":{"repairPlanId":plan_id,"authorizationId":auth_id}
        })).unwrap_err();
        assert!(error.contains(expected));
    }
}

#[test]
fn h03_recompile_uses_b07_owner_and_b06_stale_requery() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state.manager.create("p-h03-recompile", "H03", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, "p-h03-recompile");
    drop(connection);
    let context = query_context_manifest_impl(&state, json!({
        "projectId":"p-h03-recompile","chapterId":chapter_id,"volumeId":StableIdentityFactory::from_command(StableIdKind::Volume, "b08-volume").unwrap(),
        "executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou"],"references":[]
    })).unwrap();
    let context_id = "context:h03:recompile";
    let context_hash = sha256_canonical(&context).unwrap();
    let connection = state.manager.open("p-h03-recompile", "H03", NOW).unwrap();
    let scope = json!({"level":"CHAPTER","chapterId":chapter_id}).to_string();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,'p-h03-recompile','ContextManifest','CHAPTER',?2,1,'m0a.artifact.v1','COMMITTED',?3,'{\"createdBy\":\"B06::ContextCompilerOwner\"}',?4,?4)", libsql_rusqlite::params![context_id, scope, context_hash, NOW]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)", libsql_rusqlite::params![context_id, context.to_string(), context_hash]).unwrap();
    drop(connection);
    let plan = json!({
        "payloadSchema":"chapter-plan.r1","chapterId":chapter_id,"compiledFromProjectRevision":1,
        "compiledFromStateDigest":context["compiledFromStateDigest"],
        "contextManifestRef":{"artifactId":context_id,"version":1,"payloadHash":context_hash},
        "chapterGoal":"由 B07 owner 重新编译","activeTaskRefs":["task:actual"],"continuityActions":[],
        "requiredStateOutcomes":[],"forbiddenTransitions":[],"knowledgeBoundaries":[],"reservationRefs":[],
        "sceneExecution":[],"horizonPreview":[
            {"slotNo":1,"objective":"继续当前行动","tentative":true},
            {"slotNo":2,"objective":"检查行动后果","tentative":true},
            {"slotNo":3,"objective":"准备下一冲突","tentative":true}
        ]
    });
    let plan_id = "repair-plan:h03:recompile";
    let auth_id = "repair-auth:h03:recompile";
    let digest = create_h03_plan(
        &state,
        "p-h03-recompile",
        plan_id,
        RepairOperation {
            operation: "recompile_chapter_plan".into(),
            target: chapter_id.clone(),
            params: json!({"chapterId":chapter_id,"plan":plan,"entityIds":["entity:actual:linzhou"]}),
            postconditions: json!({"kind":"chapter_plan_current"}),
        },
        0,
    );
    authorize_h03(&state, "p-h03-recompile", plan_id, auth_id, &digest, 0);
    let result = execute_command_impl(&state, json!({
        "type":"repair_execute","commandId":"cmd:h03:recompile:execute","idempotencyKey":"key:h03:recompile:execute",
        "projectId":"p-h03-recompile","requestedAt":NOW,
        "payload":{"repairPlanId":plan_id,"authorizationId":auth_id}
    })).unwrap();
    assert_eq!(result["receipt"]["status"], "REPAIR_VERIFIED");
    assert_eq!(result["receipt"]["owner"], "B07::PlanningOwner");
    assert!(result["receipt"]["ownerReceiptRef"]
        .as_str()
        .unwrap()
        .starts_with("receipt:"));
    assert_eq!(result["receipt"]["postconditions"][0]["stale"], "VALID");
}

#[test]
fn h03_support_schema_is_fresh_reopen_upgrade_and_copy_safe() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let state = KernelState::new(paths.clone());
    let connection = state
        .manager
        .create("p-h03-migration", "H03 migration", NOW)
        .unwrap();
    for table in [
        "repair_plans",
        "repair_authorizations",
        "repair_execution_receipts",
    ] {
        assert_eq!(
            connection
                .query_row::<i64, _, _>(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0)
                )
                .unwrap(),
            1
        );
    }
    connection
        .execute("DROP TABLE repair_execution_receipts", [])
        .unwrap();
    connection
        .execute("DROP TABLE repair_authorizations", [])
        .unwrap();
    connection.execute("DROP TABLE repair_plans", []).unwrap();
    drop(connection);
    let reopened = state
        .manager
        .open("p-h03-migration", "H03 migration", NOW)
        .unwrap();
    for table in [
        "repair_plans",
        "repair_authorizations",
        "repair_execution_receipts",
    ] {
        assert_eq!(
            reopened
                .query_row::<i64, _, _>(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0)
                )
                .unwrap(),
            1
        );
    }
    let operation = RepairOperation {
        operation: "refresh_runtime_projection".into(),
        target: "p-h03-migration".into(),
        params: json!({}),
        postconditions: json!({}),
    };
    let digest = operation_digest_with_postconditions(
        std::slice::from_ref(&operation),
        &[json!({"status":"PASS"})],
    )
    .unwrap();
    let plan = RepairPlan {
        repair_plan_id: "repair-plan:migration".into(),
        project_id: "p-h03-migration".into(),
        based_on_project_revision: 0,
        diagnosis: json!({}),
        finding_refs: vec![],
        target_authority_owner: "B06".into(),
        proposed_operations: vec![operation],
        affected_artifacts: vec![],
        affected_scopes: vec!["PROJECT".into()],
        risk_level: "LOW".into(),
        requires_backup: false,
        expected_postconditions: vec![json!({"status":"PASS"})],
        operation_digest: digest,
        created_at: NOW.into(),
    };
    ai_novel_production_system_core::repair::save_plan(&reopened, &plan).unwrap();
    drop(reopened);
    state
        .manager
        .duplicate("p-h03-migration", "p-h03-migration-copy", NOW)
        .unwrap();
    let copied = state
        .manager
        .open("p-h03-migration-copy", "copy", NOW)
        .unwrap();
    assert_eq!(
        copied
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM repair_plans", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        copied
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap(),
        0
    );
}

#[test]
fn h03_expected_postconditions_are_part_of_operation_digest() {
    let operation = RepairOperation {
        operation: "refresh_runtime_projection".into(),
        target: "project:h03".into(),
        params: json!({}),
        postconditions: json!({"kind":"runtime_projection_current"}),
    };
    let mut plan = RepairPlan {
        repair_plan_id: "repair-plan:digest".into(),
        project_id: "p-h03-digest".into(),
        based_on_project_revision: 0,
        diagnosis: json!({}),
        finding_refs: vec![],
        target_authority_owner: "B06".into(),
        proposed_operations: vec![operation],
        affected_artifacts: vec![],
        affected_scopes: vec!["PROJECT".into()],
        risk_level: "LOW".into(),
        requires_backup: false,
        expected_postconditions: vec![json!({"kind":"runtime_projection_current"})],
        operation_digest: String::new(),
        created_at: NOW.into(),
    };
    plan.operation_digest = operation_digest_with_postconditions(
        &plan.proposed_operations,
        &plan.expected_postconditions,
    )
    .unwrap();
    assert!(ai_novel_production_system_core::repair::validate_plan(&plan).is_ok());
    plan.expected_postconditions = vec![json!({"kind":"different_query"})];
    assert!(ai_novel_production_system_core::repair::validate_plan(&plan).is_err());
}

#[test]
fn b08_reconciliation_classifies_all_five_authoritative_outcomes() {
    assert_eq!(
        reconciliation_for_actual_state(false, false, false, false),
        "NO_ACTION"
    );
    assert_eq!(
        reconciliation_for_actual_state(true, false, false, false),
        "RECOMPILE_NEXT_CHAPTER"
    );
    assert_eq!(
        reconciliation_for_actual_state(false, true, false, false),
        "RECOMPILE_ROLLING_WINDOW"
    );
    assert_eq!(
        reconciliation_for_actual_state(false, false, true, false),
        "REPLAN_VOLUME_ROUTE"
    );
    assert_eq!(
        reconciliation_for_actual_state(false, false, false, true),
        "CHANGE_REQUEST"
    );
}

#[test]
fn b08_final_commit_fault_rolls_back_final_chapter_reality_and_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(CommitFaultGateway),
    );
    let connection = state
        .manager
        .create("p-b08-atomic", "B08 atomic", NOW)
        .unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, "p-b08-atomic");
    drop(connection);
    let error = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:atomic","idempotencyKey":"key:b08:atomic",
        "projectId":"p-b08-atomic","expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
    })).unwrap_err();
    assert!(error.contains("dependency sourceHash mismatch"));
    let connection = state
        .manager
        .open("p-b08-atomic", "B08 atomic", NOW)
        .unwrap();
    for artifact_type in ["FinalChapter", "ChapterExitState", "ProductionReceipt"] {
        assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM artifacts WHERE project_id='p-b08-atomic' AND artifact_type=?1", [artifact_type], |row| row.get(0)).unwrap(), 0);
    }
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM canon_facts WHERE project_id='p-b08-atomic' AND fact_id='fact:actual:door-open'", [], |row| row.get(0)).unwrap(), 0);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM canon_evidence WHERE project_id='p-b08-atomic'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );
}
