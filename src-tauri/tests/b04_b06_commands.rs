//! Deterministic command-boundary coverage for the B04/B06 chapter workflow.
//!
//! The ignored real-provider test remains the acceptance harness.  These tests
//! only verify that the formal commands enforce their authority boundary and
//! that the planning command has one model boundary after the deterministic
//! ContextManifest compiler.

use ai_novel_production_system_core::commands::{execute_command_impl, KernelState};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const NOW: &str = "2026-09-15T00:00:00Z";

struct PlanningGateway {
    calls: AtomicUsize,
}

impl ProductionModelGateway for PlanningGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        assert_eq!(capability, "PLAN_STRATEGY");
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ProductionModelResponse::deterministic(json!({
            "payloadSchema":"chapter-plan.r1",
            "chapterId":input["chapterId"],
            "compiledFromProjectRevision":input["compiledFromProjectRevision"],
            "compiledFromStateDigest":input["compiledFromStateDigest"],
            "contextManifestRef":input["contextManifestRef"],
            "chapterGoal":"基于最新事实推进调查",
            "activeTaskRefs":[],
            "continuityActions":[],
            "requiredStateOutcomes":[],
            "forbiddenTransitions":[],
            "knowledgeBoundaries":[],
            "reservationRefs":[],
            "sceneExecution":[],
            "horizonPreview":[
                {"slotNo":1,"objective":"核对新证据","tentative":true},
                {"slotNo":2,"objective":"观察后果","tentative":true},
                {"slotNo":3,"objective":"准备下一冲突","tentative":true}
            ]
        })))
    }
}

fn insert_artifact(
    connection: &Connection,
    project_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    scope_level: &str,
    scope_key: &str,
    payload: &Value,
) -> String {
    let hash = sha256_canonical(payload).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'m0a.artifact.v1','COMMITTED',?6,'{}',?7,?7)",
        libsql_rusqlite::params![artifact_id, project_id, artifact_type, scope_level, scope_key, hash, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
        libsql_rusqlite::params![artifact_id, payload.to_string(), hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,?2,?3,?4,?5,1,?6)",
        libsql_rusqlite::params![project_id, artifact_type, scope_level, scope_key, artifact_id, hash],
    ).unwrap();
    hash
}

fn seed_authority(connection: &Connection, project_id: &str) -> (String, String) {
    let book_id = StableIdentityFactory::from_command(StableIdKind::Book, "b04-b06-book").unwrap();
    let volume_id =
        StableIdentityFactory::from_command(StableIdKind::Volume, "b04-b06-volume").unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'Book','ACTIVE',?3,?3)",
        libsql_rusqlite::params![book_id, project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'Volume','READY',1,3,1000,?4,?4)",
        libsql_rusqlite::params![volume_id, project_id, book_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:b04:1',?1,?2,1,'Chapter 1','NORMAL','COMMITTED',100,?3,?3)",
        libsql_rusqlite::params![project_id, volume_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:b04:2',?1,?2,2,'Chapter 2','NORMAL','PLANNED',0,?3,?3)",
        libsql_rusqlite::params![project_id, volume_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO chapter_production_budgets(budget_id,project_id,chapter_id,importance_class,hard_max_model_runs,cost_budget_cents,frozen_at) VALUES ('budget:chapter:b04:2',?1,'chapter:b04:2','NORMAL',12,500,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:b04:lead',?1,'主角','CHARACTER',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,source_chapter_id,valid_from_chapter_id,created_at,updated_at) VALUES ('fact:b04:reality',?1,'entity:b04:lead','DISCOVERED','{\"value\":true}','A','chapter:b04:1','chapter:b04:1',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();

    let project_scope = "{\"level\":\"PROJECT\"}";
    insert_artifact(
        connection,
        project_id,
        "authority:story",
        "StoryConstitution",
        "PROJECT",
        project_scope,
        &json!({"payloadSchema":"story-constitution.r1"}),
    );
    insert_artifact(
        connection,
        project_id,
        "authority:main",
        "MainContract",
        "PROJECT",
        project_scope,
        &json!({"payloadSchema":"main-contract.r1"}),
    );
    insert_artifact(
        connection,
        project_id,
        "authority:book",
        "BookPlan",
        "PROJECT",
        project_scope,
        &json!({"payloadSchema":"book-plan.r1","planningMaturity":"APPROVED"}),
    );
    let book_ref = json!({"artifactId":"authority:book","version":1,"payloadHash":sha256_canonical(&json!({"payloadSchema":"book-plan.r1","planningMaturity":"APPROVED"})).unwrap()});
    let volume_scope = json!({"level":"VOLUME","volumeId":volume_id}).to_string();
    let intent = json!({
        "payloadSchema":"volume-intent.r1","volumeFunction":"推进事实调查",
        "entryConditions":[{"kind":"BOOK_PLAN_APPROVED","value":book_ref}],
        "mustAchieve":[{"id":"goal:b04","description":"完成一项可验证调查"}],
        "mustNotBreak":[{"id":"rule:b04","description":"不得违背当前事实"}],
        "requiredExitConditions":[{"kind":"TASK_SATISFIED","value":"task:actual"}],
        "futureReservationRefs":[],"crossVolumeResponsibilityRefs":[],
        "targetChapterRange":{"min":1,"preferred":1,"max":1},"targetWordRange":{"preferred":1000},
        "sourceBookPlanRef":book_ref,"compiledFromProjectRevision":0,
        "sourceCanonRefs":["fact:b04:reality"],"sourceEvidenceRefs":["fact:b04:reality"],"semanticDigest":"c".repeat(64)
    });
    let intent_hash = insert_artifact(
        connection,
        project_id,
        "authority:intent",
        "VolumeIntent",
        "VOLUME",
        &volume_scope,
        &intent,
    );
    let intent_ref = json!({"artifactId":"authority:intent","version":1,"payloadHash":intent_hash});
    let route = json!({
        "payloadSchema":"volume-plan.r1","volumeIntentRef":intent_ref,"routeVersion":1,
        "tasks":[{"taskId":"task:actual","class":"REQUIRED","objective":"完成一项可验证调查","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"TASK_EVIDENCE_RECORDED","value":{"taskId":"task:actual","evidenceType":"FINAL_CHAPTER","requiresNewEvidence":true}},"estimatedChapterCost":{"preferred":1}}],
        "routeInvariants":["不得违背当前事实"],"requiredExitConditions":[{"kind":"TASK_SATISFIED","value":"task:actual"}],
        "capacityBudget":{"minChapters":1,"preferredChapters":1,"maxChapters":1},"semanticDigest":"d".repeat(64)
    });
    insert_artifact(
        connection,
        project_id,
        "authority:route",
        "VolumePlan",
        "VOLUME",
        &volume_scope,
        &route,
    );
    (volume_id, "chapter:b04:2".to_string())
}

#[test]
fn b04_b06_prepare_command_is_revision_checked_and_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p-b04-b06", "B04/B06", NOW).unwrap();
    let (volume_id, _) = seed_authority(&connection, "p-b04-b06");
    drop(connection);
    let state = KernelState::new(AppPaths::new(temp.path()));
    let request = json!({"type":"planning_prepare_next_chapter","commandId":"prepare-next","idempotencyKey":"prepare-next-key","projectId":"p-b04-b06","expectedRevision":0,"requestedAt":NOW,"payload":{"volumeId":volume_id}});
    let first = execute_command_impl(&state, request.clone()).unwrap();
    assert_eq!(first["finalStatus"], "committed");
    assert_eq!(first["chapter"]["chapterNo"], 3);
    let replay = execute_command_impl(&state, request).unwrap();
    assert_eq!(replay["finalStatus"], "committed");
    assert_eq!(replay["receiptId"], first["receiptId"]);
}

#[test]
fn b04_b06_compile_command_uses_one_model_after_bounded_context_and_commits_plan() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("p-b04-b06-plan", "B04/B06 plan", NOW)
        .unwrap();
    let (volume_id, chapter_id) = seed_authority(&connection, "p-b04-b06-plan");
    drop(connection);
    let gateway = Arc::new(PlanningGateway {
        calls: AtomicUsize::new(0),
    });
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let prepared = execute_command_impl(&state, json!({"type":"planning_prepare_next_chapter","commandId":"prepare-plan","idempotencyKey":"prepare-plan-key","projectId":"p-b04-b06-plan","expectedRevision":0,"requestedAt":NOW,"payload":{"volumeId":volume_id}})).unwrap();
    let chapter_id = prepared["chapter"]["chapterId"]
        .as_str()
        .unwrap_or(&chapter_id);
    let compiled = execute_command_impl(&state, json!({"type":"planning_compile_chapter_plan","commandId":"compile-plan","idempotencyKey":"compile-plan-key","projectId":"p-b04-b06-plan","expectedRevision":1,"requestedAt":NOW,"payload":{"chapterId":chapter_id}})).unwrap();
    assert_eq!(compiled["finalStatus"], "committed");
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        compiled["contextManifestRef"]["artifactId"]
            .as_str()
            .unwrap()
            .contains("compile-plan:context"),
        true
    );
    let connection = state
        .manager
        .open("p-b04-b06-plan", "B04/B06 plan", NOW)
        .unwrap();
    let source_count: i64 = connection.query_row("SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='ContextManifest'", ["p-b04-b06-plan"], |row| row.get(0)).unwrap();
    assert_eq!(source_count, 1);
}
