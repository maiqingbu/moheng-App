//! Deterministic command-boundary coverage for the B04/B06 chapter workflow.
//!
//! The ignored real-provider test remains the acceptance harness.  These tests
//! only verify that the formal commands enforce their authority boundary and
//! that the planning command has one model boundary after the deterministic
//! ContextManifest compiler.

use ai_novel_production_system_core::commands::{
    execute_command_impl, query_context_manifest_impl, query_project_overview_impl, KernelState,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::repositories::health::HealthRepository;
use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
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
    assert!(compiled["contextManifestRef"]["artifactId"]
        .as_str()
        .unwrap()
        .contains("compile-plan:context"));
    let connection = state
        .manager
        .open("p-b04-b06-plan", "B04/B06 plan", NOW)
        .unwrap();
    let source_count: i64 = connection.query_row("SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='ContextManifest'", ["p-b04-b06-plan"], |row| row.get(0)).unwrap();
    assert_eq!(source_count, 1);
}

#[test]
fn b04_b06_overview_rebuilds_chapter_counts_from_authoritative_rows() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("p-b04-b06-overview", "B04/B06 overview", NOW)
        .unwrap();
    let (_volume_id, _chapter_id) = seed_authority(&connection, "p-b04-b06-overview");
    drop(connection);
    let state = KernelState::new(AppPaths::new(temp.path()));
    let overview =
        query_project_overview_impl(&state, json!({"projectId":"p-b04-b06-overview"})).unwrap();
    assert_eq!(overview["project"]["committedChapters"], 1);
    assert_eq!(overview["recentChapters"].as_array().unwrap().len(), 1);
}

fn assert_context_manifest_blocks_hard_overflow(
    source: &str,
    insert_overflow: impl FnOnce(&Connection, &str, &str),
) {
    let temp = tempfile::tempdir().unwrap();
    let project_id = format!("p-context-overflow-{source}");
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create(&project_id, "Context completeness", NOW)
        .unwrap();
    let (volume_id, chapter_id) = seed_authority(&connection, &project_id);
    insert_overflow(&connection, &project_id, &chapter_id);
    drop(connection);

    let state = KernelState::new(AppPaths::new(temp.path()));
    let entity_ids = if source == "CANON" {
        vec!["entity:b04:lead", "entity:b04:overflow"]
    } else {
        vec!["entity:b04:lead"]
    };
    let references = if source == "CONTINUITY" {
        (0..101)
            .map(|index| format!("obligation:overflow:{index}"))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let result = query_context_manifest_impl(
        &state,
        json!({
            "projectId":project_id,
            "volumeId":volume_id,
            "chapterId":chapter_id,
            "executionMode":"OPENING_BOOTSTRAP",
            "entityIds":entity_ids,
            "references":references
        }),
    );
    let error = match result {
        Err(error) => {
            assert!(error.contains("CONTEXT_HARD_INCOMPLETE"), "{error}");
            assert!(error.contains(source), "{error}");
            error
        }
        Ok(_) => panic!("incomplete {source} HARD context compiled a formal manifest"),
    };
    let repeated = query_context_manifest_impl(
        &state,
        json!({
            "projectId":project_id,
            "volumeId":volume_id,
            "chapterId":chapter_id,
            "executionMode":"OPENING_BOOTSTRAP",
            "entityIds":entity_ids,
            "references":references
        }),
    )
    .unwrap_err();
    assert_eq!(error, repeated, "the same blocker must be deterministic");

    let reopened_connection = state.manager.open(&project_id, "", NOW).unwrap();
    let (table, pattern) = match source {
        "CANON" => ("canon_facts", "fact:overflow:%"),
        "KNOWLEDGE" => ("canon_knowledge", "knowledge:overflow:%"),
        "TASK" => ("state_deltas", "task-state:overflow:%"),
        "EVIDENCE" => ("canon_evidence", "evidence:overflow:%"),
        "CONTINUITY" => ("narrative_obligations", "obligation:overflow:%"),
        _ => unreachable!(),
    };
    reopened_connection
        .execute(
            &format!(
                "DELETE FROM {table} WHERE project_id=?1 AND {} LIKE ?2",
                match source {
                    "CANON" => "fact_id",
                    "KNOWLEDGE" => "knowledge_id",
                    "TASK" => "state_id",
                    "EVIDENCE" => "evidence_id",
                    "CONTINUITY" => "obligation_id",
                    _ => unreachable!(),
                }
            ),
            libsql_rusqlite::params![project_id, pattern],
        )
        .unwrap();
    drop(reopened_connection);
    let recovered = query_context_manifest_impl(
        &state,
        json!({
            "projectId":project_id,
            "volumeId":volume_id,
            "chapterId":chapter_id,
            "executionMode":"OPENING_BOOTSTRAP",
            "entityIds":entity_ids,
            "references":references
        }),
    )
    .unwrap();
    assert_eq!(recovered["payloadSchema"], "context-manifest.r1");
    println!("B_R2_01_HARD_OVERFLOW_RESULT source={source} overflow_rows=101 blocker=PASS deterministic=PASS complete_after_scope_reduction=PASS status=PASS");
}

#[test]
fn b_r2_01_canon_overflow_blocks_formal_context_manifest() {
    assert_context_manifest_blocks_hard_overflow("CANON", |connection, project_id, _| {
        connection.execute(
            "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:b04:overflow',?1,'Overflow','CHARACTER',?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        ).unwrap();
        for index in 0..100 {
            connection.execute(
                "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES (?1,?2,'entity:b04:lead','HARD_CONTEXT',?3,'A',?4,?4)",
                libsql_rusqlite::params![format!("fact:overflow:{index}"), project_id, json!(index).to_string(), NOW],
            ).unwrap();
        }
        connection.execute(
            "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES ('fact:overflow:other',?1,'entity:b04:overflow','HARD_CONTEXT','true','A',?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        ).unwrap();
    });
}

#[test]
fn b_r2_01_knowledge_overflow_blocks_formal_context_manifest() {
    assert_context_manifest_blocks_hard_overflow("KNOWLEDGE", |connection, project_id, _| {
        for index in 0..101 {
            connection.execute(
                "INSERT INTO canon_knowledge(knowledge_id,project_id,knower_id,epistemic_state,created_at,updated_at) VALUES (?1,?2,'entity:b04:lead','KNOWS',?3,?3)",
                libsql_rusqlite::params![format!("knowledge:overflow:{index}"), project_id, NOW],
            ).unwrap();
        }
    });
}

#[test]
fn b_r2_01_task_overflow_blocks_formal_context_manifest() {
    assert_context_manifest_blocks_hard_overflow("TASK", |connection, project_id, chapter_id| {
        for index in 0..101 {
            connection.execute(
                "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES (?1,?2,'volume.task.progress','{\"status\":\"ACTIVE\"}',?3,'{}',?4,?4)",
                libsql_rusqlite::params![format!("task-state:overflow:{index}"), project_id, chapter_id, NOW],
            ).unwrap();
        }
    });
}

#[test]
fn b_r2_01_evidence_overflow_blocks_formal_context_manifest() {
    assert_context_manifest_blocks_hard_overflow(
        "EVIDENCE",
        |connection, project_id, chapter_id| {
            for index in 0..101 {
                connection.execute(
                "INSERT INTO canon_evidence(evidence_id,project_id,fact_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,created_at) VALUES (?1,?2,'fact:b04:reality',?3,'authority:book',1,'FINAL_CHAPTER','test evidence',?4)",
                libsql_rusqlite::params![format!("evidence:overflow:{index}"), project_id, chapter_id, NOW],
            ).unwrap();
            }
        },
    );
}

#[test]
fn b_r2_01_continuity_overflow_blocks_formal_context_manifest() {
    assert_context_manifest_blocks_hard_overflow("CONTINUITY", |connection, project_id, _| {
        for index in 0..101 {
            connection.execute(
                "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,lifecycle,resume_condition_json,created_at,updated_at) VALUES (?1,?2,'BOOK','CONTINUITY_TEST','B','authority:book',1,'required continuity','APPROVED','{\"kind\":\"AFTER_CHAPTER\",\"value\":1}',?3,?3)",
                libsql_rusqlite::params![format!("obligation:overflow:{index}"), project_id, NOW],
            ).unwrap();
        }
    });
}

#[test]
fn b_r2_01_unavailable_hard_source_returns_typed_blocker_with_scope() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-context-unavailable";
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create(project_id, "Context unavailable", NOW)
        .unwrap();
    let (volume_id, chapter_id) = seed_authority(&connection, project_id);
    connection
        .execute_batch("DROP TABLE canon_knowledge")
        .unwrap();
    drop(connection);

    let state = KernelState::new(AppPaths::new(temp.path()));
    let error = query_context_manifest_impl(
        &state,
        json!({"projectId":project_id,"volumeId":volume_id,"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:b04:lead"],"references":[]}),
    )
    .unwrap_err();
    assert!(error.contains("CONTEXT_SOURCE_UNAVAILABLE"), "{error}");
    assert!(error.contains("source=KNOWLEDGE"), "{error}");
    assert!(error.contains("scope=CHAPTER:chapter:b04:2"), "{error}");
    println!("B_R2_01_SOURCE_UNAVAILABLE_RESULT source=KNOWLEDGE scope=CHAPTER:chapter:b04:2 typed_blocker=PASS status=PASS");
}

#[test]
fn b_r2_01_derived_long_form_entity_scope_overflow_is_not_silently_cut() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-context-derived-scope-overflow";
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create(project_id, "Derived context scope", NOW)
        .unwrap();
    let (volume_id, chapter_id) = seed_authority(&connection, project_id);
    for index in 0..100 {
        let entity_id = format!("entity:derived:{index}");
        connection.execute(
            "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES (?1,?2,?1,'CHARACTER',?3,?3)",
            libsql_rusqlite::params![entity_id, project_id, NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,source_chapter_id,valid_from_chapter_id,created_at,updated_at) VALUES (?1,?2,?3,'HARD_CONTEXT','true','A','chapter:b04:1','chapter:b04:1',?4,?4)",
            libsql_rusqlite::params![format!("fact:derived:{index}"), project_id, entity_id, NOW],
        ).unwrap();
    }
    drop(connection);

    let state = KernelState::new(AppPaths::new(temp.path()));
    let error = query_context_manifest_impl(
        &state,
        json!({"projectId":project_id,"volumeId":volume_id,"chapterId":chapter_id,"executionMode":"LONG_FORM","entityIds":[],"references":[]}),
    )
    .unwrap_err();
    assert!(error.contains("CONTEXT_HARD_INCOMPLETE"), "{error}");
    assert!(error.contains("CANON"), "{error}");
    println!(
        "B_R2_01_DERIVED_SCOPE_RESULT source=CANON derived_entities=101 blocker=PASS status=PASS"
    );
}

#[test]
fn b_r2_01_soft_omission_keeps_reason_and_allows_complete_context() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-context-soft-omission";
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create(project_id, "Context soft omission", NOW)
        .unwrap();
    let (volume_id, chapter_id) = seed_authority(&connection, project_id);
    drop(connection);

    let state = KernelState::new(AppPaths::new(temp.path()));
    let manifest = query_context_manifest_impl(
        &state,
        json!({"projectId":project_id,"volumeId":volume_id,"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:b04:lead"],"references":[],"includeSoftSources":true}),
    )
    .unwrap();
    assert_eq!(
        manifest["omittedSoftSources"][0]["reason"],
        "soft cache is excluded from HARD digest"
    );
}

#[test]
fn b_r2_01_planning_blocks_before_model_and_persists_no_context_after_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-context-overflow-planning";
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create(project_id, "Context completeness", NOW)
        .unwrap();
    let (volume_id, _) = seed_authority(&connection, project_id);
    for index in 0..101 {
        connection.execute(
            "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,source_chapter_id,created_at,updated_at) VALUES (?1,?2,'entity:b04:lead','HARD_CONTEXT',?3,'A','chapter:b04:1',?4,?4)",
            libsql_rusqlite::params![format!("fact:planning-overflow:{index}"), project_id, json!(index).to_string(), NOW],
        ).unwrap();
    }
    drop(connection);

    let gateway = Arc::new(PlanningGateway {
        calls: AtomicUsize::new(0),
    });
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let prepared = execute_command_impl(
        &state,
        json!({"type":"planning_prepare_next_chapter","commandId":"prepare-overflow","idempotencyKey":"prepare-overflow-key","projectId":project_id,"expectedRevision":0,"requestedAt":NOW,"payload":{"volumeId":volume_id}}),
    )
    .unwrap();
    let chapter_id = prepared["chapter"]["chapterId"].as_str().unwrap();
    let connection_before = state.manager.open(project_id, "", NOW).unwrap();
    let context_count_before: i64 = connection_before
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='ContextManifest'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection_before);
    let compile_result = execute_command_impl(
        &state,
        json!({"type":"planning_compile_chapter_plan","commandId":"compile-overflow","idempotencyKey":"compile-overflow-key","projectId":project_id,"expectedRevision":1,"requestedAt":NOW,"payload":{"chapterId":chapter_id}}),
    );
    let reloaded = KernelState::new(AppPaths::new(temp.path()));
    let requery = query_context_manifest_impl(
        &reloaded,
        json!({"projectId":project_id,"volumeId":volume_id,"chapterId":chapter_id,"executionMode":"LONG_FORM","entityIds":["entity:b04:lead"],"references":[]}),
    );
    let connection = reloaded.manager.open(project_id, "", NOW).unwrap();
    let context_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='ContextManifest'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    let blocked = |result: Result<Value, String>| {
        result.is_err_and(|error| error.contains("CONTEXT_HARD_INCOMPLETE"))
    };
    assert_eq!(context_count_before, 0);
    assert!(blocked(compile_result), "planning command was not blocked");
    assert!(blocked(requery), "reopened authority query did not block");
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 0);
    assert_eq!(context_count, 0);
    println!("B_R2_01_E3_E4_RESULT mode=LONG_FORM planning_entry=PASS model_calls=0 context_manifest_before={context_count_before} context_manifest_after={context_count} reopened_query=BLOCKED status=PASS");
}

#[test]
fn b_r2_01_long_form_route_task_total_over_bound_blocks_before_state_reads_and_model() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-context-longform-task-overflow";
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create(project_id, "Long-form Task bound", NOW)
        .unwrap();
    let (volume_id, _) = seed_authority(&connection, project_id);
    let route_json: String = connection
        .query_row(
            "SELECT payload_json FROM artifact_payloads WHERE artifact_id='authority:route' AND version=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let mut route: Value = serde_json::from_str(&route_json).unwrap();
    let template = route["tasks"][0].clone();
    let mut tasks = vec![template.clone()];
    let task_ids = (0..101)
        .map(|index| {
            if index == 0 {
                "task:actual".to_string()
            } else {
                format!("task:longform-overflow:{index}")
            }
        })
        .collect::<Vec<_>>();
    for task_id in task_ids.iter().skip(1) {
        let mut task = template.clone();
        task["taskId"] = json!(task_id);
        task["class"] = json!("OPTIONAL");
        task["objective"] = json!(format!("处理 {task_id}"));
        task["completionPredicate"]["value"]["taskId"] = json!(task_id);
        tasks.push(task);
    }
    route["tasks"] = json!(tasks);
    let gateway = Arc::new(PlanningGateway {
        calls: AtomicUsize::new(0),
    });
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let route_commit = execute_command_impl(
        &state,
        json!({"type":"planning_create_volume_plan","commandId":"commit-task-overflow-route","idempotencyKey":"commit-task-overflow-route-key","projectId":project_id,"expectedRevision":0,"requestedAt":NOW,"payload":{"volumeId":volume_id,"plan":route}}),
    )
    .unwrap();
    assert_eq!(route_commit["finalStatus"], "committed");
    let prepared = execute_command_impl(
        &state,
        json!({"type":"planning_prepare_next_chapter","commandId":"prepare-task-overflow","idempotencyKey":"prepare-task-overflow-key","projectId":project_id,"expectedRevision":1,"requestedAt":NOW,"payload":{"volumeId":volume_id}}),
    )
    .unwrap();
    let chapter_id = prepared["chapter"]["chapterId"]
        .as_str()
        .unwrap()
        .to_string();
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    for task_id in &task_ids {
        connection.execute(
            "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES (?1,?2,'volume.task.progress','{\"status\":\"ACTIVE\"}',?3,'{}',?4,?4)",
            libsql_rusqlite::params![task_id, project_id, chapter_id, NOW],
        ).unwrap();
    }
    HealthRepository::flush_dirty(&connection, project_id, NOW).unwrap();
    connection.execute_batch(
        "ALTER TABLE state_deltas RENAME TO task_state_read_trap;
         CREATE VIEW state_deltas AS
         SELECT state_id, project_id, state_type,
                CASE WHEN state_type='volume.task.progress' THEN json_extract('invalid-json', '$.status') ELSE value_json END AS value_json,
                chapter_id, payload_json, created_at, updated_at
         FROM task_state_read_trap;",
    ).unwrap();
    drop(connection);

    let expected_revision = state.manager.project_revision(project_id).unwrap();
    let request = json!({"projectId":project_id,"volumeId":volume_id,"chapterId":chapter_id,"executionMode":"LONG_FORM","entityIds":["entity:b04:lead"],"references":[]});
    let context_result = query_context_manifest_impl(&state, request);
    let planning_result = execute_command_impl(
        &state,
        json!({"type":"planning_compile_chapter_plan","commandId":"compile-task-overflow","idempotencyKey":"compile-task-overflow-key","projectId":project_id,"expectedRevision":expected_revision,"requestedAt":NOW,"payload":{"chapterId":chapter_id}}),
    );
    let planning_result_summary = match &planning_result {
        Ok(value) => format!(
            "finalStatus={}",
            value["finalStatus"].as_str().unwrap_or("unknown")
        ),
        Err(error) => error.clone(),
    };
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    let context_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='ContextManifest'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    let context_blocked = context_result
        .err()
        .is_some_and(|error| error.contains("CONTEXT_HARD_INCOMPLETE") && error.contains("TASK"));
    let planning_blocked = planning_result
        .err()
        .is_some_and(|error| error.contains("CONTEXT_HARD_INCOMPLETE") && error.contains("TASK"));
    println!("B_R2_01_LONG_FORM_TASK_BOUND_RESULT route_tasks=101 task_state_read_trap=NOT_TRIGGERED context_blocked={context_blocked} planning_blocked={planning_blocked} planning_result={planning_result_summary} model_calls={} context_manifest_count={context_count}", gateway.calls.load(Ordering::SeqCst));
    assert!(
        context_blocked,
        "LONG_FORM Context query must block a route with 101 tasks"
    );
    assert!(
        planning_blocked,
        "LONG_FORM planning must propagate the typed Task blocker"
    );
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 0);
    assert_eq!(context_count, 0);
}

#[test]
fn b_r2_01_long_form_historical_evidence_quote_is_soft_and_remains_retrievable() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-context-aggregate-budget";
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create(project_id, "Aggregate HARD budget", NOW)
        .unwrap();
    let (volume_id, _) = seed_authority(&connection, project_id);
    let quote = "证据内容".repeat(3_000);
    connection.execute(
        "INSERT INTO canon_evidence(evidence_id,project_id,fact_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,range_start,range_end,created_at) VALUES ('evidence:budget:oversized',?1,'fact:b04:reality','chapter:b04:1','authority:book',1,'FINAL_CHAPTER',?2,0,?3,?4)",
        libsql_rusqlite::params![project_id, quote, quote.chars().count() as i64, NOW],
    ).unwrap();
    drop(connection);

    let gateway = Arc::new(PlanningGateway {
        calls: AtomicUsize::new(0),
    });
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let prepared = execute_command_impl(
        &state,
        json!({"type":"planning_prepare_next_chapter","commandId":"prepare-budget","idempotencyKey":"prepare-budget-key","projectId":project_id,"expectedRevision":0,"requestedAt":NOW,"payload":{"volumeId":volume_id}}),
    )
    .unwrap();
    let chapter_id = prepared["chapter"]["chapterId"].as_str().unwrap();
    let query_request = json!({"projectId":project_id,"volumeId":volume_id,"chapterId":chapter_id,"executionMode":"LONG_FORM","entityIds":["entity:b04:lead"],"references":[]});
    let query_result = query_context_manifest_impl(&state, query_request.clone());
    let context_manifest = query_result.expect("historical excerpt is excluded from HARD context");
    assert!(context_manifest["budget"]["hardTokens"].as_i64().unwrap() <= 1024);
    assert_eq!(
        context_manifest["selectedEvidence"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(context_manifest["selectedEvidence"][0]["quote"].is_null());
    assert_eq!(
        context_manifest["selectedEvidence"][0]["artifactId"],
        "authority:book"
    );
    assert_eq!(context_manifest["selectedEvidence"][0]["version"], 1);
    assert_eq!(context_manifest["selectedEvidence"][0]["rangeStart"], 0);
    assert_eq!(
        context_manifest["selectedEvidence"][0]["rangeEnd"],
        quote.chars().count()
    );
    assert_eq!(
        gateway.calls.load(Ordering::SeqCst),
        0,
        "the registered context query is local"
    );
    let (stored_evidence, stored_truncated) =
        ai_novel_production_system_core::repositories::narrative::NarrativeRepository::
            evidence_for_refs_bounded(&state.manager.open(project_id, "", NOW).unwrap(), project_id, &["fact:b04:reality"], 100)
                .unwrap();
    assert!(!stored_truncated);
    assert_eq!(stored_evidence.len(), 1);
    assert_eq!(stored_evidence[0].quote.as_deref(), Some(quote.as_str()));
    let planning_result = execute_command_impl(
        &state,
        json!({"type":"planning_compile_chapter_plan","commandId":"compile-budget","idempotencyKey":"compile-budget-key","projectId":project_id,"expectedRevision":1,"requestedAt":NOW,"payload":{"chapterId":chapter_id}}),
    );
    assert_eq!(planning_result.unwrap()["finalStatus"], "committed");
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    let context_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='ContextManifest'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    println!("B_R2_01_HISTORICAL_EVIDENCE_RESULT evidence_rows=1 quote_chars=12000 hard_reference_preserved=true quote_in_hard_input=false original_quote_retrievable=true planning_committed=true model_calls={} context_manifest_count={context_count}", gateway.calls.load(Ordering::SeqCst));
    assert_eq!(gateway.calls.load(Ordering::SeqCst), 1);
    assert_eq!(context_count, 1);

    connection
        .execute(
            "DELETE FROM canon_evidence WHERE project_id=?1 AND evidence_id='evidence:budget:oversized'",
            [project_id],
        )
        .unwrap();
    drop(connection);
    let recovered = query_context_manifest_impl(&state, query_request).unwrap();
    assert_eq!(recovered["payloadSchema"], "context-manifest.r1");
}

#[test]
fn b_r2_01_context_manifest_and_evidence_are_project_scoped() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let manager = ProjectManager::new(paths.clone());
    let projects = [
        ("p-context-isolation-a", "PROJECT_A_ONLY", "甲项目证据"),
        ("p-context-isolation-b", "PROJECT_B_ONLY", "乙项目证据"),
    ];
    let mut scopes = Vec::new();
    for (project_id, marker, quote) in projects {
        let connection = manager
            .create(project_id, "Context project isolation", NOW)
            .unwrap();
        let (volume_id, chapter_id) = seed_authority(&connection, project_id);
        connection
            .execute(
                "UPDATE canon_facts SET object_json=?1 WHERE project_id=?2 AND fact_id='fact:b04:reality'",
                libsql_rusqlite::params![json!({"projectMarker":marker}).to_string(), project_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO canon_evidence(evidence_id,project_id,fact_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,range_start,range_end,created_at) VALUES ('evidence:project-isolation',?1,'fact:b04:reality','chapter:b04:1','authority:book',1,'FINAL_CHAPTER',?2,0,?3,?4)",
                libsql_rusqlite::params![project_id, quote, quote.chars().count() as i64, NOW],
            )
            .unwrap();
        scopes.push((project_id, marker, quote, volume_id, chapter_id));
    }
    let state = KernelState::new(paths);
    let context = |project_id: &str, volume_id: &str, chapter_id: &str| {
        query_context_manifest_impl(
            &state,
            json!({
                "projectId":project_id,
                "volumeId":volume_id,
                "chapterId":chapter_id,
                "executionMode":"OPENING_BOOTSTRAP",
                "entityIds":["entity:b04:lead"],
                "references":["fact:b04:reality"],
                "includeSoftSources":true
            }),
        )
        .unwrap()
    };
    let first = context(scopes[0].0, &scopes[0].3, &scopes[0].4);
    let second = context(scopes[1].0, &scopes[1].3, &scopes[1].4);
    for projection in [&first, &second] {
        assert_eq!(projection["selectedEvidence"].as_array().unwrap().len(), 1);
        assert_eq!(
            projection["selectedEvidence"][0]["artifactId"],
            "authority:book"
        );
        assert!(projection["selectedEvidence"][0]["quote"].is_null());
    }
    assert_ne!(
        first["compiledFromStateDigest"],
        second["compiledFromStateDigest"]
    );
    for (project_id, _, quote, _, _) in scopes {
        let connection = manager
            .open(project_id, "Context project isolation query", NOW)
            .unwrap();
        let (evidence, truncated) = NarrativeRepository::evidence_for_refs_bounded(
            &connection,
            project_id,
            &["fact:b04:reality"],
            10,
        )
        .unwrap();
        assert!(!truncated);
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].quote.as_deref(), Some(quote));
    }
    println!("B_R2_01_CONTEXT_PROJECT_ISOLATION_RESULT sharedArtifactId=true localFacts=PASS localEvidence=PASS digestSeparated=true status=PASS");
}
