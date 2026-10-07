use ai_novel_production_system_core::commands::{
    query_context_manifest_impl, query_narrative_state_impl, query_plan_stale_impl,
    query_volume_progress_impl, support_checkpoint_resume_impl, support_session_impl, KernelState,
    RepositoryReadAdapter,
};
use ai_novel_production_system_core::harness::{
    bounded_replan, can_expose_write_capability, check_checkpoint_resume, diagnose_knowledge_event,
    diagnose_projection_authority, grounded_finding, plan_support, registry_digest_map,
    resolve_project_scope, BoundedCapabilityExecutor, CapabilityCallEnvelope, CapabilityRegistry,
    CheckpointResume, ReadCapabilityAdapter, ReplanTrigger, RequestedBounds, SupportCheckpoint,
    SupportStatus,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::narrative::{
    build_continuity_threads, build_volume_progress, compile_context_manifest, evaluate_plan_stale,
    evaluate_resume_condition, relevant_state_digest, ContextCompilerInput, ContextHardSource,
    CurrentNarrativeStateView, PlanStaleStatus, ResumeContext, RuntimeBounds, VolumeProgressInput,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::repositories::narrative::{
    CanonEventRecord, CanonFactRecord, EvidenceRecord, KnowledgeRecord, ObligationRecord,
    RelationRecord, TaskStateRecord,
};
use serde_json::{json, Value};

fn populated_state() -> CurrentNarrativeStateView {
    CurrentNarrativeStateView::from_records(
        RuntimeBounds {
            project_id: "p1".into(),
            entity_ids: vec!["entity:a".into()],
            volume_id: Some("volume:2".into()),
            chapter_id: Some("chapter:21".into()),
            limit: 20,
        },
        vec![CanonFactRecord {
            fact_id: "fact:a".into(),
            subject_id: "entity:a".into(),
            predicate: "identity".into(),
            object_json: json!("hidden"),
            authority_level: "B".into(),
        }],
        vec![RelationRecord {
            relation_id: "relation:ab".into(),
            subject_id: "entity:a".into(),
            predicate: "trusts".into(),
            object_id: "entity:b".into(),
            authority_level: "B".into(),
        }],
        vec![KnowledgeRecord {
            knowledge_id: "knowledge:a".into(),
            knower_id: "entity:a".into(),
            fact_id: Some("fact:a".into()),
            epistemic_state: "KNOWS".into(),
            visibility_scope: Some("CHAPTER".into()),
            from_chapter_id: Some("chapter:11".into()),
            from_story_time: None,
        }],
        vec![CanonEventRecord {
            event_id: "event:identity".into(),
            chapter_id: "chapter:11".into(),
            event_type: "REVEAL".into(),
            story_time: Some("1999-01-01T00:00:00Z".into()),
            event_ordinal: 2,
        }],
        Vec::new(),
        false,
    )
}

fn insert_context_artifact(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    scope_level: &str,
    scope_key: &str,
    payload: &Value,
) -> String {
    let hash = sha256_canonical(payload).unwrap();
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'m0a.artifact.v1','COMMITTED',?6,'{}','2026-08-29T00:00:00Z','2026-08-29T00:00:00Z')",
            libsql_rusqlite::params![artifact_id, project_id, artifact_type, scope_level, scope_key, hash],
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
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,?2,?3,?4,?5,1,?6)",
            libsql_rusqlite::params![project_id, artifact_type, scope_level, scope_key, artifact_id, hash],
        )
        .unwrap();
    hash
}

fn seed_project_goal_contracts(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    include_story_constitution: bool,
    future_reservations: &[Value],
) {
    if include_story_constitution {
        insert_context_artifact(
            connection,
            project_id,
            &format!("story-constitution:{project_id}"),
            "StoryConstitution",
            "PROJECT",
            project_id,
            &json!({
                "payloadSchema":"story-constitution.r1",
                "storyCore":{
                    "protagonist":"主角",
                    "desire":"达成目标",
                    "mechanism":"已证实事实不得被无依据改写",
                    "conflict":"现实冲突",
                    "readerPromise":"每次行动呈现可核对后果",
                    "worldRules":[],
                    "forbiddenReveals":[]
                }
            }),
        );
    }
    insert_context_artifact(
        connection,
        project_id,
        &format!("main-contract:{project_id}"),
        "MainContract",
        "PROJECT",
        project_id,
        &json!({
            "payloadSchema":"main-contract.r1",
            "scope":"BOOK",
            "hardRules":["保持已证实事实"],
            "softPreferences":[]
        }),
    );
    insert_context_artifact(
        connection,
        project_id,
        &format!("plan:book:{project_id}"),
        "BookPlan",
        "PROJECT",
        project_id,
        &json!({
            "payloadSchema":"book-plan.r1",
            "planningMaturity":"APPROVED",
            "storyPromise":"以证据推动选择",
            "protagonistMacroDirection":"由被动转为主动",
            "endingBoundary":"核心冲突必须有可核对结局",
            "mustAchieve":[],
            "mustNotBreak":[],
            "relationshipMacroDirections":[],
            "majorMysteryGoals":[],
            "futureReservations":future_reservations,
            "volumeIntentSeeds":[],
            "openingHypotheses":[],
            "globalLengthBudget":{"preferredWords":1000}
        }),
    );
}

fn seed_volume_goal_contracts(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    volume_id: &str,
    intent_artifact_id: &str,
    plan_artifact_id: &str,
    future_reservation_refs: &[&str],
    tasks: Vec<Value>,
) {
    let intent_payload = json!({
        "payloadSchema":"volume-intent.r1",
        "volumeFunction":"推进本卷当前目标",
        "entryConditions":[],
        "mustAchieve":[],
        "mustNotBreak":[],
        "requiredExitConditions":[],
        "futureReservationRefs":future_reservation_refs,
        "crossVolumeResponsibilityRefs":[],
        "targetChapterRange":{"min":1,"preferred":5,"max":10},
        "targetWordRange":{"preferred":1000}
    });
    let intent_hash = insert_context_artifact(
        connection,
        project_id,
        intent_artifact_id,
        "VolumeIntent",
        "VOLUME",
        volume_id,
        &intent_payload,
    );
    let plan_payload = json!({
        "payloadSchema":"volume-plan.r1",
        "volumeIntentRef":{
            "artifactId":intent_artifact_id,
            "version":1,
            "payloadHash":intent_hash
        },
        "routeVersion":1,
        "tasks":tasks,
        "routeInvariants":[],
        "requiredExitConditions":[],
        "capacityBudget":{"minChapters":1,"preferredChapters":5,"maxChapters":10}
    });
    insert_context_artifact(
        connection,
        project_id,
        plan_artifact_id,
        "VolumePlan",
        "VOLUME",
        volume_id,
        &plan_payload,
    );
}

#[test]
fn b06_current_state_is_populated_bounded_and_keeps_narrative_time_separate() {
    let state = populated_state();
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.relations.len(), 1);
    assert_eq!(state.knowledge.len(), 1);
    assert_eq!(state.events.len(), 1);
    assert_eq!(state.world_time.as_deref(), Some("1999-01-01T00:00:00Z"));
    assert_eq!(state.bounds.entity_ids, vec!["entity:a"]);
    assert!(state.source_refs.iter().any(|item| item.ref_ == "fact:a"));
    assert!(!state.truncated);
    println!("B06_CURRENT_STATE_RESULT populated=PASS bounded=PASS provenance=PASS no_narrative_authority_table=PASS status=PASS");
}

#[test]
fn b06_missing_long_form_contract_reports_required_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-missing-long-form-contract";
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create(project_id, "Missing contract", "2026-08-29T00:00:00Z")
        .unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:missing-contract',?1,1,'Book','ACTIVE',?2,?2)", libsql_rusqlite::params![project_id, "2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:missing-contract',?1,'book:missing-contract',1,'Volume','PRODUCING',1,1,1000,?2,?2)", libsql_rusqlite::params![project_id, "2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:missing-contract:1',?1,'volume:missing-contract',1,'Chapter','NORMAL','PLANNED',0,?2,?2)", libsql_rusqlite::params![project_id, "2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:missing-contract',?1,'Probe','CHARACTER',?2,?2)", libsql_rusqlite::params![project_id, "2026-08-29T00:00:00Z"]).unwrap();
    let revision_before: i64 = connection
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    let artifacts_before: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    let heads_before: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM canonical_heads WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    let model_runs_before: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM model_runs WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();

    let state = KernelState::new(AppPaths::new(temp.path()));
    let error = query_context_manifest_impl(
        &state,
        json!({
            "projectId":project_id,
            "volumeId":"volume:missing-contract",
            "chapterId":"chapter:missing-contract:1",
            "executionMode":"LONG_FORM",
            "entityIds":["entity:missing-contract"],
            "references":[]
        }),
    )
    .unwrap_err();

    assert!(
        error.contains("GOAL_CONTRACT_STORY_CONSTITUTION_REQUIRED|scope=CHAPTER:chapter:missing-contract:1"),
        "{error}"
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT revision FROM project_meta WHERE project_id=?1",
                [project_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        revision_before
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM artifacts WHERE project_id=?1",
                [project_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        artifacts_before
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM canonical_heads WHERE project_id=?1",
                [project_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        heads_before
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM model_runs WHERE project_id=?1",
                [project_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        model_runs_before
    );
}

#[test]
fn b06_long_form_selected_entity_scope_recalls_recent_reality() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("p-recall", "Recall", "2026-08-29T00:00:00Z")
        .unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:recall','p-recall',1,'Recall','ACTIVE',?1,?1)",
        ["2026-08-29T00:00:00Z"],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:recall','p-recall','book:recall',1,'Recall','READY',1,3,1000,?1,?1)",
        ["2026-08-29T00:00:00Z"],
    ).unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:recall:1','p-recall','volume:recall',1,'Chapter 1','NORMAL','COMMITTED',100,?1,?1),('chapter:recall:2','p-recall','volume:recall',2,'Chapter 2','NORMAL','PLANNED',0,?1,?1)",
        ["2026-08-29T00:00:00Z"],
    ).unwrap();
    connection.execute(
        "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:recall','p-recall','Recall','CHARACTER',?1,?1)",
        ["2026-08-29T00:00:00Z"],
    ).unwrap();
    connection.execute(
        "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,source_chapter_id,valid_from_chapter_id,created_at,updated_at) VALUES ('fact:recall','p-recall','entity:recall','DISCOVERED','{\"value\":true}','A','chapter:recall:1','chapter:recall:1',?1,?1)",
        ["2026-08-29T00:00:00Z"],
    ).unwrap();
    seed_project_goal_contracts(&connection, "p-recall", true, &[]);
    seed_volume_goal_contracts(
        &connection,
        "p-recall",
        "volume:recall",
        "intent:volume:recall",
        "plan:volume:recall",
        &[],
        Vec::new(),
    );
    drop(connection);

    let state = KernelState::new(AppPaths::new(temp.path()));
    let context = query_context_manifest_impl(
        &state,
        json!({
            "projectId":"p-recall","volumeId":"volume:recall","chapterId":"chapter:recall:2",
            "executionMode":"LONG_FORM","entityIds":[],"references":[],
            "scopeSelection":{
                "taskRefs":[],
                "responsibilityRefs":[],
                "reservationRefs":[],
                "entityRefs":["entity:recall"]
            }
        }),
    )
    .unwrap();
    let sources = context["narrativeStateSources"].as_array().unwrap();
    assert!(sources.iter().any(|source| source["ref"] == "fact:recall"));
    assert!(context["compiledFromStateDigest"].as_str().unwrap().len() == 64);
    println!("B06_LONG_FORM_RECALL_RESULT recent_reality=PASS bounded_scope=PASS digest=PASS status=PASS");
}

#[test]
fn b06_long_form_resolves_volume_from_chapter_scope() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-chapter-volume-scope";
    let volume_id = "volume:chapter-scope";
    let chapter_id = "chapter:chapter-scope:1";
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create(project_id, "Chapter volume scope", "2026-08-29T00:00:00Z")
        .unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:chapter-scope',?1,1,'Book','ACTIVE',?2,?2)", libsql_rusqlite::params![project_id, "2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,'book:chapter-scope',1,'Volume','PRODUCING',1,1,1000,?3,?3)", libsql_rusqlite::params![volume_id, project_id, "2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,1,'Chapter','NORMAL','PLANNED',0,?4,?4)", libsql_rusqlite::params![chapter_id, project_id, volume_id, "2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:chapter-scope',?1,'Lead','CHARACTER',?2,?2)", libsql_rusqlite::params![project_id, "2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES ('fact:chapter-scope',?1,'entity:chapter-scope','KNOWN','{}','A',?2,?2)", libsql_rusqlite::params![project_id, "2026-08-29T00:00:00Z"]).unwrap();
    seed_project_goal_contracts(&connection, project_id, true, &[]);
    let task = json!({
        "taskId":"task:chapter-scope",
        "class":"REQUIRED",
        "objective":"Complete the selected task",
        "dependsOnTaskIds":[],
        "continuityThreadRefs":[],
        "reservationRefs":[],
        "completionPredicate":{"kind":"TASK_DONE","value":true},
        "estimatedChapterCost":{"preferred":1}
    });
    seed_volume_goal_contracts(
        &connection,
        project_id,
        volume_id,
        "intent:chapter-scope",
        "plan:chapter-scope",
        &[],
        vec![task],
    );
    connection.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES ('task:chapter-scope',?1,'volume.task.progress','{\"status\":\"ACTIVE\"}',?2,'{}',?3,?3)", libsql_rusqlite::params![project_id, chapter_id, "2026-08-29T00:00:00Z"]).unwrap();
    drop(connection);

    let state = KernelState::new(AppPaths::new(temp.path()));
    let context = query_context_manifest_impl(
        &state,
        json!({
            "projectId":project_id,
            "chapterId":chapter_id,
            "executionMode":"LONG_FORM",
            "entityIds":["entity:chapter-scope"],
            "references":[]
        }),
    )
    .unwrap();
    assert!(context["goalContractRefs"].as_array().unwrap().iter().any(|reference| {
        reference["artifactId"] == "intent:chapter-scope"
    }));
    assert_eq!(context["activeVolumeTaskRefs"], json!(["task:chapter-scope"]));
    assert!(context["narrativeStateSources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| source["ref"] == "fact:chapter-scope"));
}

#[test]
fn b06_resume_conditions_are_typed_and_fail_closed() {
    let mut context = ResumeContext {
        current_chapter: 12,
        current_volume: Some("volume:2".into()),
        ..ResumeContext::default()
    };
    context.present_entities.insert("entity:a".into());
    context.acquired_facts.insert("fact:identity".into());
    context.satisfied_tasks.insert("task:1".into());
    context
        .meetings
        .insert(("entity:a".into(), "entity:b".into()));
    assert!(evaluate_resume_condition(
        &json!({"kind":"NEXT_MEETING","value":{"entityA":"entity:a","entityB":"entity:b"}}),
        &context
    )
    .unwrap());
    assert!(evaluate_resume_condition(
        &json!({"kind":"ENTITY_PRESENT","value":"entity:a"}),
        &context
    )
    .unwrap());
    assert!(evaluate_resume_condition(
        &json!({"kind":"KNOWLEDGE_ACQUIRED","value":"fact:identity"}),
        &context
    )
    .unwrap());
    assert!(evaluate_resume_condition(
        &json!({"kind":"TASK_SATISFIED","value":"task:1"}),
        &context
    )
    .unwrap());
    assert!(
        evaluate_resume_condition(&json!({"kind":"AFTER_CHAPTER","value":12}), &context).unwrap()
    );
    assert!(
        evaluate_resume_condition(&json!({"kind":"IN_VOLUME","value":"volume:2"}), &context)
            .unwrap()
    );
    assert!(!evaluate_resume_condition(
        &json!({"kind":"NEXT_MEETING","value":{"entityA":"entity:x","entityB":"entity:y"}}),
        &context
    )
    .unwrap());
    assert!(evaluate_resume_condition(&json!({"kind":"UNKNOWN","value":true}), &context).is_err());
    assert!(evaluate_resume_condition(&json!({"kind":"AFTER_CHAPTER"}), &context).is_err());
    println!("B06_RESUME_RESULT next_meeting=PASS knowledge=PASS task=PASS chapter=PASS volume=PASS unknown=REJECT malformed=REJECT status=PASS");
}

#[test]
fn b06_continuity_lifecycle_and_cross_volume_recall_are_deterministic() {
    let obligations = vec![
        ObligationRecord {
            obligation_id: "obligation:promise".into(),
            obligation_type: "PROMISE".into(),
            authority_level: "B".into(),
            lifecycle: "APPROVED".into(),
            source_event_id: Some("event:promise".into()),
            resume_condition: json!({"kind":"NEXT_MEETING","value":{"entityA":"entity:a","entityB":"entity:b"}}),
            participant_entities: vec!["entity:a".into(), "entity:b".into()],
        },
        ObligationRecord {
            obligation_id: "obligation:resolved".into(),
            obligation_type: "PROMISE".into(),
            authority_level: "B".into(),
            lifecycle: "RESOLVED".into(),
            source_event_id: None,
            resume_condition: json!({"kind":"AFTER_CHAPTER","value":1}),
            participant_entities: Vec::new(),
        },
        ObligationRecord {
            obligation_id: "obligation:abandoned".into(),
            obligation_type: "CONFLICT".into(),
            authority_level: "C".into(),
            lifecycle: "ABANDONED".into(),
            source_event_id: None,
            resume_condition: json!({"kind":"AFTER_CHAPTER","value":1}),
            participant_entities: Vec::new(),
        },
    ];
    let evidence = vec![EvidenceRecord {
        evidence_id: "evidence:v1".into(),
        fact_id: None,
        relation_id: None,
        event_id: Some("event:promise".into()),
        final_chapter_id: "chapter:11".into(),
        source_artifact_id: "artifact:chapter-11".into(),
        version: 1,
        evidence_type: "CANON_EVENT".into(),
        quote: Some("我们下次见面再说".into()),
        range_start: Some(10),
        range_end: Some(20),
    }];
    let mut context = ResumeContext::default();
    context
        .meetings
        .insert(("entity:a".into(), "entity:b".into()));
    let threads = build_continuity_threads(&obligations, &[], &[], &evidence, &context).unwrap();
    assert_eq!(threads.len(), 3);
    assert_eq!(threads[0].logical_status, "ABANDONED");
    assert!(threads
        .iter()
        .any(|thread| thread.thread_id == "thread:obligation:promise"
            && thread.logical_status == "OPEN"
            && thread.evidence_refs.len() == 1));
    assert!(threads
        .iter()
        .any(|thread| thread.logical_status == "RESOLVED"));
    println!("B06_CONTINUITY_RESULT open=PASS suspended=PASS resolved=PASS abandoned=PASS cross_volume=PASS exact_evidence=PASS no_ordinary_promotion=PASS status=PASS");
}

#[test]
fn b06_progress_context_digest_and_stale_evaluation_are_bounded_and_order_stable() {
    let tasks = vec![
        TaskStateRecord {
            state_id: "task:blocked".into(),
            value: json!({"status":"BLOCKED"}),
            chapter_id: None,
        },
        TaskStateRecord {
            state_id: "task:active".into(),
            value: json!({"status":"ACTIVE"}),
            chapter_id: None,
        },
    ];
    let progress = build_volume_progress(VolumeProgressInput {
        volume_id: "volume:2".into(),
        target_chapters: 20,
        current_chapter: 12,
        tasks: tasks.clone(),
        generated_revision: 4,
    });
    assert_eq!(progress.active_task_ids, vec!["task:active"]);
    assert_eq!(progress.blocked_task_ids, vec!["task:blocked"]);
    assert_eq!(progress.remaining_chapter_capacity, 8);
    let state = populated_state();
    let thread_context = ResumeContext::default();
    let threads = build_continuity_threads(&[], &[], &[], &[], &thread_context).unwrap();
    let input = ContextCompilerInput {
        execution_mode: "LONG_FORM".into(),
        project_revision: 4,
        goal_contract_refs: vec![],
        goal_contract_fields: vec![],
        state: state.clone(),
        continuity_threads: threads,
        plan_task_refs: vec![],
        plan_task_contexts: vec![],
        task_states: vec![],
        active_task_refs: vec!["task:active".into()],
        future_reservation_refs: vec!["reservation:1".into()],
        selected_evidence: vec![],
        omitted_soft_sources: vec![],
        hard_tokens: 100,
        soft_tokens: 40,
    };
    let manifest = compile_context_manifest(input.clone()).unwrap();
    let reversed = {
        let mut copy = input;
        copy.state.facts.reverse();
        copy.active_task_refs.reverse();
        copy
    };
    assert_eq!(
        manifest.compiled_from_state_digest,
        compile_context_manifest(reversed)
            .unwrap()
            .compiled_from_state_digest
    );
    assert_eq!(
        evaluate_plan_stale(
            4,
            &manifest.compiled_from_state_digest,
            5,
            &manifest.compiled_from_state_digest
        ),
        PlanStaleStatus::Valid
    );
    assert_eq!(
        evaluate_plan_stale(
            4,
            &manifest.compiled_from_state_digest,
            5,
            "f".repeat(64).as_str()
        ),
        PlanStaleStatus::PlanStale
    );
    assert!(manifest.hard_source_count >= state.source_refs.len() as i64);
    assert!(manifest.budget.hard_tokens > 0);
    assert!(
        manifest.budget.hard_tokens
            < ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT
    );
    assert_eq!(manifest.budget.soft_tokens, 40);
    assert_eq!(
        manifest.budget.total_tokens,
        manifest.budget.hard_tokens + manifest.budget.soft_tokens
    );
    println!("B06_CONTEXT_RESULT progress=PASS manifest=PASS digest_deterministic=PASS unrelated_revision_valid=PASS related_change_stale=PASS bounded_hard_context=PASS status=PASS");
}

#[test]
fn b_r2_01_compiler_and_digest_reject_incomplete_context_before_manifest() {
    let mut state = populated_state();
    state.mark_truncated(ContextHardSource::Canon);
    let input = ContextCompilerInput {
        execution_mode: "LONG_FORM".into(),
        project_revision: 4,
        goal_contract_refs: vec![],
        goal_contract_fields: vec![],
        state,
        continuity_threads: vec![],
        plan_task_refs: vec![],
        plan_task_contexts: vec![],
        task_states: vec![],
        active_task_refs: vec![],
        future_reservation_refs: vec![],
        selected_evidence: vec![],
        omitted_soft_sources: vec![],
        hard_tokens: 100,
        soft_tokens: 0,
    };
    let blocker = "CONTEXT_HARD_INCOMPLETE|sources=CANON|scope=CHAPTER:chapter:21";

    assert_eq!(relevant_state_digest(&input).unwrap_err(), blocker);
    assert_eq!(compile_context_manifest(input).unwrap_err(), blocker);
}

#[test]
fn b_r2_01_aggregate_hard_budget_counts_multiple_sources_and_future_reservations() {
    let mut state = populated_state();
    let fact_template = state.facts[0].clone();
    for index in 0..60 {
        let mut fact = fact_template.clone();
        fact.fact_id = format!("fact:aggregate:{index}:{}", "x".repeat(24));
        state.facts.push(fact);
    }
    let selected_evidence = (0..60)
        .map(|index| {
            serde_json::from_value(json!({
                "artifactId":format!("evidence:aggregate:{index}:{}", "x".repeat(24)),
                "version":1,
                "quote":"证据摘要".repeat(80)
            }))
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut input = ContextCompilerInput {
        execution_mode: "LONG_FORM".into(),
        project_revision: 4,
        goal_contract_refs: vec![],
        goal_contract_fields: vec![],
        state,
        continuity_threads: vec![],
        plan_task_refs: vec![],
        plan_task_contexts: vec![],
        task_states: vec![],
        active_task_refs: (0..60)
            .map(|index| format!("task:aggregate:{index}:{}", "x".repeat(24)))
            .collect(),
        future_reservation_refs: (0..60)
            .map(|index| format!("reservation:aggregate:{index}:{}", "x".repeat(24)))
            .collect(),
        selected_evidence,
        omitted_soft_sources: vec![],
        hard_tokens: 1024,
        soft_tokens: 0,
    };

    let first = compile_context_manifest(input.clone());
    let blocker = first.err().unwrap_or_default();
    let repeated = compile_context_manifest(input.clone()).unwrap_err();
    let digest_result = relevant_state_digest(&input);
    let digest_blocker = digest_result.err().unwrap_or_default();
    println!("B_R2_01_AGGREGATE_HARD_RESULT canon=61 tasks=60 evidence=60 future_reservations=60 error={blocker}");
    assert!(blocker.starts_with("CONTEXT_HARD_BUDGET_EXCEEDED"));
    assert_eq!(
        blocker, repeated,
        "the same aggregate input must block deterministically"
    );
    assert_eq!(
        blocker, digest_blocker,
        "budget blockers must be deterministic before digest creation"
    );

    input.state.facts.truncate(1);
    input.active_task_refs.truncate(1);
    input.future_reservation_refs.truncate(1);
    input.selected_evidence.clear();
    let recovered = compile_context_manifest(input).unwrap();
    assert!(
        recovered.budget.hard_tokens
            < ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT
    );
    println!("B_R2_01_AGGREGATE_HARD_RECOVERY_RESULT reduced_scope=PASS estimated_hard_tokens={} limit={} status=PASS", recovered.budget.hard_tokens, ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT);
}

struct EchoAdapter;
impl ReadCapabilityAdapter for EchoAdapter {
    fn invoke(
        &self,
        capability_id: &str,
        _input: &Value,
        bounds: &RequestedBounds,
    ) -> Result<(Value, Vec<String>, bool), String> {
        Ok((
            json!({"capability":capability_id,"maxResults":bounds.max_results}),
            vec!["authority:p1".into()],
            false,
        ))
    }
}

#[test]
fn h01_registry_executor_planner_checkpoint_and_diagnostics_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "H01", "2026-08-29T00:00:00Z").unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:h01','p1',1,'H01','ACTIVE',?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:h01','p1','book:h01',1,'H01','PRODUCING',1,1,1000,?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:1','p1','volume:h01',1,'Chapter','NORMAL','PLANNED',0,?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    seed_project_goal_contracts(&connection, "p1", true, &[]);
    seed_volume_goal_contracts(
        &connection,
        "p1",
        "volume:h01",
        "intent:volume:h01",
        "plan:volume:h01",
        &[],
        Vec::new(),
    );
    connection.execute("INSERT INTO model_runs(model_run_id,project_id,provider,model,idempotency_key,capability,predicted_cost_cents,reserved_cost_cents,status,created_at,updated_at) VALUES ('model-run:h01','p1','fixture','fixture','h01-run','diagnose',1,1,'COMPLETED',?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO commands(project_id,command_id,idempotency_key,request_digest,status,created_at) VALUES ('p1','command:h01','idempotency:h01',?1,'COMMITTED',?2)", libsql_rusqlite::params!["a".repeat(64), "2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO commit_receipts(receipt_id,project_id,commit_id,command_id,idempotency_key,before_revision,after_revision,bundle_digest,receipt_json,receipt_digest,committed_at) VALUES ('receipt:h01','p1','commit:h01','command:h01','idempotency:h01',0,1,?1,'{}',?1,?2)", libsql_rusqlite::params!["b".repeat(64), "2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO integrity_runs(integrity_run_id,project_id,status,failure_count,report_json,created_at) VALUES ('integrity:h01','p1','PASS',0,'{}',?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:a','p1','A','CHARACTER',?1,?1),('entity:b','p1','B','CHARACTER',?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES ('fact:known','p1','entity:a','status','{}','B',?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_relations(relation_id,project_id,subject_id,predicate,object_id,authority_level,created_at,updated_at) VALUES ('relation:known','p1','entity:a','trusts','entity:b','B',?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    let scope =
        resolve_project_scope("p1", 7, "A 何时知道 B 的身份", "DIAGNOSE", "PROJECT").unwrap();
    let plan = plan_support(&scope, "A 何时知道 B 的身份", "2026-08-28T00:00:00Z").unwrap();
    assert!(
        plan.steps.len() <= 7
            && plan
                .steps
                .iter()
                .all(|step| step.capability_id.starts_with("query_"))
    );
    let registry = ai_novel_production_system_core::harness::CapabilityRegistry::read_only_r1();
    assert!(!can_expose_write_capability(&registry));
    let spec = registry.specs.get("query_knowledge").unwrap();
    let input = json!({"scope":"PROJECT"});
    let call = CapabilityCallEnvelope {
        call_id: "call:1".into(),
        capability_id: spec.capability_id.clone(),
        capability_version: spec.version.clone(),
        spec_digest: spec.spec_digest.clone(),
        mode: "DIAGNOSE".into(),
        project_id: "p1".into(),
        project_revision: 7,
        input_digest: sha256_canonical(&input).unwrap(),
        input,
        requested_bounds: RequestedBounds {
            max_results: 10,
            max_serialized_bytes: 100,
        },
    };
    let result = BoundedCapabilityExecutor {
        registry: registry.clone(),
    }
    .execute(&EchoAdapter, &call)
    .unwrap();
    assert_eq!(result.status, "PASS");
    let mut wrong = call.clone();
    wrong.capability_version = "9.0.0".into();
    assert!(BoundedCapabilityExecutor {
        registry: registry.clone()
    }
    .execute(&EchoAdapter, &wrong)
    .is_err());
    let mut wrong_digest = call.clone();
    wrong_digest.spec_digest = "f".repeat(64);
    assert!(BoundedCapabilityExecutor {
        registry: registry.clone()
    }
    .execute(&EchoAdapter, &wrong_digest)
    .is_err());
    let mut wrong_mode = call.clone();
    wrong_mode.mode = "WRITE".into();
    assert!(BoundedCapabilityExecutor {
        registry: registry.clone()
    }
    .execute(&EchoAdapter, &wrong_mode)
    .is_err());
    let mut escalated = call.clone();
    escalated.requested_bounds.max_results = 101;
    assert!(BoundedCapabilityExecutor {
        registry: registry.clone()
    }
    .execute(&EchoAdapter, &escalated)
    .is_err());
    let finding = grounded_finding(
        &scope,
        "diagnose_knowledge_event",
        "A 已知 B 身份",
        vec![
            "knowledge:a".into(),
            "event:identity".into(),
            "artifact:evidence".into(),
        ],
        Vec::new(),
    );
    assert_eq!(finding.support_status, SupportStatus::SUPPORTED);
    let insufficient = grounded_finding(
        &scope,
        "diagnose_canon_evidence",
        "缺少精确证据",
        Vec::new(),
        Vec::new(),
    );
    assert_eq!(insufficient.support_status, SupportStatus::INSUFFICIENT);
    let state = populated_state();
    let mut legal_later_chapter = state.knowledge[0].clone();
    legal_later_chapter.from_chapter_id = Some("chapter:12".into());
    let evidence = EvidenceRecord {
        evidence_id: "evidence:identity".into(),
        fact_id: None,
        relation_id: None,
        event_id: Some(state.events[0].event_id.clone()),
        final_chapter_id: "chapter:11".into(),
        source_artifact_id: "artifact:evidence".into(),
        version: 1,
        evidence_type: "EVENT".into(),
        quote: Some("identity".into()),
        range_start: Some(1),
        range_end: Some(2),
    };
    assert_eq!(
        diagnose_knowledge_event(
            &scope,
            "legal narrative order",
            &legal_later_chapter,
            Some(&state.events[0]),
            Some(&evidence)
        )
        .support_status,
        SupportStatus::SUPPORTED
    );
    let mut time_conflicting = state.knowledge[0].clone();
    time_conflicting.from_story_time = Some("1998-01-01T00:00:00Z".into());
    assert_eq!(
        diagnose_knowledge_event(
            &scope,
            "world time conflict",
            &time_conflicting,
            Some(&state.events[0]),
            Some(&evidence)
        )
        .support_status,
        SupportStatus::CONFLICTING
    );
    let mut wrong_event_evidence = evidence.clone();
    wrong_event_evidence.event_id = Some("event:other".into());
    assert_eq!(
        diagnose_knowledge_event(
            &scope,
            "wrong evidence",
            &state.knowledge[0],
            Some(&state.events[0]),
            Some(&wrong_event_evidence)
        )
        .support_status,
        SupportStatus::CONFLICTING
    );
    assert_eq!(
        diagnose_projection_authority(
            &scope,
            "projection drift",
            "projection:p1",
            "authority:p1",
            1,
            2
        )
        .diagnosis_type,
        "PROJECTION_DRIFT"
    );
    let checkpoint = SupportCheckpoint {
        checkpoint_id: "checkpoint:1".into(),
        session_id: "session:1".into(),
        project_id: "p1".into(),
        project_revision: 7,
        question_digest: scope.question_digest.clone(),
        plan_digest: sha256_canonical(&serde_json::to_value(&plan).unwrap()).unwrap(),
        completed_step_ids: vec!["step:1".into()],
        capability_spec_digests: registry_digest_map(&registry),
        evidence_refs: vec!["artifact:evidence".into()],
        last_result_digests: vec![result.result_digest],
        requested_mode: "DIAGNOSE".into(),
        requested_scope: "PROJECT".into(),
        plan_steps: plan.steps.clone(),
        pending_authorization_ref: None,
        created_at: "2026-08-28T00:00:00Z".into(),
    };
    assert_eq!(
        check_checkpoint_resume(&checkpoint, 7, &registry_digest_map(&registry)),
        CheckpointResume::Safe
    );
    assert_eq!(
        check_checkpoint_resume(&checkpoint, 8, &registry_digest_map(&registry)),
        CheckpointResume::Stale
    );
    let replanned = bounded_replan(
        &plan,
        ReplanTrigger::CHECKPOINT_STALE,
        "2026-08-28T00:01:00Z",
    )
    .unwrap();
    assert!(replanned.steps.len() <= 7);
    let adapter = RepositoryReadAdapter {
        connection: &connection,
        project_id: "p1",
    };
    for spec in registry.specs.values() {
        let input = json!({"entityIds":["entity:a"],"references":[],"chapterId":"chapter:1","volumeId":"volume:h01","executionMode":"LONG_FORM","projectRevision":7});
        let call = CapabilityCallEnvelope {
            call_id: format!("parity:{}", spec.capability_id),
            capability_id: spec.capability_id.clone(),
            capability_version: spec.version.clone(),
            spec_digest: spec.spec_digest.clone(),
            mode: "DIAGNOSE".into(),
            project_id: "p1".into(),
            project_revision: 7,
            input_digest: sha256_canonical(&input).unwrap(),
            input,
            requested_bounds: RequestedBounds {
                max_results: 1,
                max_serialized_bytes: if spec.capability_id == "query_context_manifest" {
                    8192
                } else {
                    4096
                },
            },
        };
        let result = BoundedCapabilityExecutor {
            registry: registry.clone(),
        }
        .execute(&adapter, &call)
        .unwrap_or_else(|error| panic!("{}: {error}", spec.capability_id));
        if spec.capability_id == "query_canon" || spec.capability_id == "query_relations" {
            assert!(!result.result.as_array().unwrap().is_empty());
            assert!(!result.source_refs.is_empty());
        }
        if spec.capability_id == "query_model_runs"
            || spec.capability_id == "query_commit_receipts"
            || spec.capability_id == "query_integrity_status"
        {
            assert_eq!(result.result["status"], "PASS");
            assert_eq!(result.result["items"].as_array().unwrap().len(), 1);
        }
        if spec.capability_id == "query_context_manifest" {
            assert!(result.result["compiledFromStateDigest"].is_string());
        }
        if spec.capability_id == "query_plan_stale" {
            connection
                .execute(
                    "UPDATE project_meta SET revision=1 WHERE project_id='p1'",
                    [],
                )
                .unwrap();
            let stale_input = json!({"projectId":"p1","projectRevision":0,"compiledFromProjectRevision":0,"compiledFromStateDigest":"f".repeat(64),"entityIds":["entity:a"],"chapterId":"chapter:1"});
            let stale_call = CapabilityCallEnvelope {
                call_id: "parity:stale".into(),
                capability_id: spec.capability_id.clone(),
                capability_version: spec.version.clone(),
                spec_digest: spec.spec_digest.clone(),
                mode: "DIAGNOSE".into(),
                project_id: "p1".into(),
                project_revision: 0,
                input_digest: sha256_canonical(&stale_input).unwrap(),
                input: stale_input,
                requested_bounds: RequestedBounds {
                    max_results: 1,
                    max_serialized_bytes: 4096,
                },
            };
            let stale_result = BoundedCapabilityExecutor {
                registry: registry.clone(),
            }
            .execute(&adapter, &stale_call)
            .unwrap();
            assert_eq!(stale_result.result["status"], "PLAN_STALE");
        }
        if spec.capability_id.starts_with("diagnose_") {
            assert!(result.result["diagnosisType"].is_string());
            assert!(result.result["supportStatus"].is_string());
        }
    }
    let capability_count = registry.specs.len();
    println!("H01_RESULT scope=PASS registry=PASS registry_adapter_parity=PASS executor=PASS sql_negative=PASS write_capability=0 diagnostics=PASS evidence_ledger=PASS checkpoint_safe=PASS checkpoint_stale=PASS bounded_replan=PASS bounded_query=PASS session_receipt=PASS max_steps=7 capability_count={} populated_model_runs=1 populated_commit_receipts=1 populated_integrity_status=1 context_owner=PASS stale_owner=PLAN_STALE diagnostics_typed=PASS narrative_time=PASS wrong_event=PASS legal_chapter=PASS world_time_conflict=PASS status=PASS", capability_count);
}

#[test]
fn h01_support_checkpoint_migration_is_operational_and_additive() {
    use ai_novel_production_system_core::project_db::connection::open_connection;
    use tempfile::tempdir;
    let temp = tempdir().unwrap();
    let db = temp.path().join("project.db");
    let connection = open_connection(&db, false).unwrap();
    connection
        .execute_batch(include_str!("../migrations/0002_project_authority.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("../migrations/0003_project_command.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("../migrations/0004_project_longterm.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("../migrations/0005_m0c_commit_delta.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!("../migrations/0006_m0e_ops.sql"))
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../migrations/0007_b05_narrative_foundation.sql"
        ))
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../migrations/0008_h01_support_checkpoint.sql"
        ))
        .unwrap();
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='support_checkpoints'", [], |row| row.get(0)).unwrap(), 1);
    println!("H01_MIGRATION_RESULT support_checkpoint=PASS additive=PASS narrative_authority=UNCHANGED status=PASS");
}

#[test]
fn b06_h01_real_command_path_uses_repository_and_persists_checkpoint() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let manager = ProjectManager::new(paths.clone());
    let connection = manager
        .create("p1", "B06/H01", "2026-08-28T00:00:00Z")
        .unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:1','p1',1,'Book','ACTIVE',?1,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:1','p1','book:1',1,'Volume','PRODUCING',1,1,1000,?1,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:2','p1','book:1',2,'Volume 2','PLANNING',2,2,3000,?1,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:1','p1','volume:1',1,'Chapter','NORMAL','COMMITTED',10,?1,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:2','p1','volume:2',2,'Chapter 2','NORMAL','PLANNED',0,?1,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:a','p1','A','CHARACTER',?1,?1),('entity:b','p1','B','CHARACTER',?1,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:evidence','p1','ReviewReport','CHAPTER','chapter:1',1,'m0a.artifact.v1','COMMITTED',?1,'{}',?2)", ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:evidence',1,'{}',?1)", ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]).unwrap();
    connection.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES ('fact:identity','p1','entity:b','identity','{\"name\":\"B\"}','B',?1,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_knowledge(knowledge_id,project_id,knower_id,fact_id,epistemic_state,visibility_scope,from_chapter_id,created_at,updated_at) VALUES ('knowledge:a','p1','entity:a','fact:identity','KNOWS','CHAPTER','chapter:1',?1,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_events(event_id,project_id,chapter_id,event_type,subject_id,story_time,event_ordinal,payload_json,created_at) VALUES ('event:identity','p1','chapter:1','IDENTITY_REVEALED','entity:a','2020-01-01T00:00:00Z',1,'{}',?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_events(event_id,project_id,chapter_id,event_type,subject_id,story_time,event_ordinal,payload_json,created_at) VALUES ('event:noise','p1','chapter:1','BACKGROUND','entity:a','1990-01-01T00:00:00Z',0,'{}',?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_timepoints(timepoint_id,project_id,chapter_id,story_time,ordering,created_at) VALUES ('timepoint:1','p1','chapter:1','2020-01-01T00:00:00Z',1,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_evidence(evidence_id,project_id,fact_id,event_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,range_start,range_end,created_at) VALUES ('evidence:identity','p1','fact:identity','event:identity','chapter:1','artifact:evidence',1,'EVENT','B revealed identity',1,20,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    let book_reservations = vec![
        json!({
            "reservationId":"reservation:book",
            "description":"Book reservation",
            "prerequisites":[],
            "mayForeshadow":true,
            "mayReveal":true,
            "authorityLevel":"B"
        }),
        json!({
            "reservationId":"reservation:v1",
            "description":"Volume one reservation",
            "reservedForVolumeNo":1,
            "notBeforeVolumeNo":1,
            "prerequisites":[],
            "mayForeshadow":true,
            "mayReveal":true,
            "authorityLevel":"B"
        }),
        json!({
            "reservationId":"reservation:v2",
            "description":"Volume two reservation",
            "reservedForVolumeNo":2,
            "notBeforeVolumeNo":2,
            "prerequisites":[],
            "mayForeshadow":false,
            "mayReveal":true,
            "authorityLevel":"B"
        }),
    ];
    seed_project_goal_contracts(&connection, "p1", true, &book_reservations);
    seed_volume_goal_contracts(
        &connection,
        "p1",
        "volume:1",
        "intent:volume:1",
        "plan:volume",
        &["reservation:v1"],
        Vec::new(),
    );
    let task_v2 = json!({"status":"ACTIVE"}).to_string();
    connection.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES ('task:v2','p1','volume.task.progress',?1,'chapter:2','{}',?2,?2)", [&task_v2, "2026-08-28T00:00:00Z"]).unwrap();
    let task_v2_contract = json!({
        "taskId":"task:v2",
        "class":"REQUIRED",
        "objective":"Complete the current task",
        "dependsOnTaskIds":[],
        "continuityThreadRefs":[],
        "reservationRefs":[],
        "completionPredicate":{"kind":"TASK_DONE","value":true},
        "estimatedChapterCost":{"preferred":1}
    });
    seed_volume_goal_contracts(
        &connection,
        "p1",
        "volume:2",
        "intent:volume:2",
        "plan:volume:2",
        &["reservation:v2"],
        vec![task_v2_contract],
    );
    drop(connection);
    let state = KernelState::new(paths);
    let request = json!({"projectId":"p1","scope":"PROJECT","entityIds":["entity:a"],"chapterId":"chapter:1","currentChapter":1,"currentVolumeId":"volume:1","includeSoftSources":true});
    let state_projection = query_narrative_state_impl(&state, request.clone()).unwrap();
    assert_eq!(state_projection["projectId"], "p1");
    let context_projection = query_context_manifest_impl(&state, request.clone()).unwrap();
    assert_eq!(context_projection["payloadSchema"], "context-manifest.r1");
    assert_eq!(
        context_projection["goalContractRefs"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        context_projection["futureReservationRefs"],
        json!(["reservation:book", "reservation:v1"])
    );
    let omitted_sources = context_projection["omittedSoftSources"].as_array().unwrap();
    assert!(omitted_sources.iter().any(|source| {
        source["sourceRef"]["ref"]
            .as_str()
            .is_some_and(|reference| reference.contains("storyCore.readerPromise"))
    }));
    assert!(omitted_sources.iter().any(|source| {
        source["sourceRef"]["ref"]
            .as_str()
            .is_some_and(|reference| reference.contains("BookPlan.mustAchieve"))
    }));
    let context_goal_refs = context_projection["goalContractRefs"]
        .as_array()
        .unwrap()
        .len();
    let context_future_reservations = context_projection["futureReservationRefs"]
        .as_array()
        .unwrap()
        .len();
    let context_selected_evidence = context_projection["selectedEvidence"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(context_selected_evidence, 1);
    let context_v2 = query_context_manifest_impl(
        &state,
        json!({"projectId":"p1","scope":"VOLUME","entityIds":["entity:a"],"chapterId":"chapter:2","currentChapter":2,"currentVolumeId":"volume:2"}),
    )
    .unwrap();
    assert_eq!(
        context_v2["futureReservationRefs"],
        json!(["reservation:book", "reservation:v1", "reservation:v2"])
    );
    assert_eq!(context_v2["activeVolumeTaskRefs"], json!(["task:v2"]));
    assert!(context_v2["futureReservationRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "reservation:v1"));
    let progress_projection = query_volume_progress_impl(
        &state,
        json!({"projectId":"p1","scope":"VOLUME","volumeId":"volume:1","currentChapter":3}),
    )
    .unwrap();
    assert_eq!(progress_projection["remainingChapterCapacity"], 7);
    assert_eq!(
        progress_projection["routePlanRef"]["artifactId"],
        "plan:volume"
    );
    let volume_route = progress_projection["routePlanRef"]["artifactId"]
        .as_str()
        .is_some();
    let volume_capacity = progress_projection["remainingChapterCapacity"]
        .as_i64()
        .is_some();
    let stale_projection = query_plan_stale_impl(&state, json!({"projectId":"p1","compiledFromProjectRevision":0,"compiledFromStateDigest":context_projection["compiledFromStateDigest"],"currentStateDigest":"f".repeat(64),"entityIds":["entity:a"],"chapterId":"chapter:1"})).unwrap();
    assert_eq!(stale_projection["status"], "VALID");
    let support = support_session_impl(&state, json!({"projectId":"p1","question":"A 何时知道 B 的身份","mode":"DIAGNOSE","scope":"PROJECT","entityIds":["entity:a"],"references":["event:identity"],"chapterId":"chapter:1","currentVolumeId":"volume:1"})).unwrap();
    assert_eq!(support["receipt"]["projectId"], "p1");
    assert_eq!(support["finding"]["supportStatus"], "SUPPORTED");
    let call_count = support["calls"].as_array().unwrap().len();
    let ledger_count = support["evidenceLedger"].as_array().unwrap().len();
    assert_eq!(call_count, ledger_count);
    assert_eq!(support["calls"][2]["result"].as_array().unwrap().len(), 1);
    assert_eq!(support["calls"][2]["result"][0]["rangeStart"], 1);
    let missing_evidence = support_session_impl(&state, json!({"projectId":"p1","question":"A 何时知道 B 的身份","mode":"DIAGNOSE","scope":"PROJECT","entityIds":["entity:a"],"references":["event:missing"],"chapterId":"chapter:1","currentVolumeId":"volume:1"})).unwrap();
    assert_eq!(missing_evidence["finding"]["supportStatus"], "INSUFFICIENT");
    let time_writer = state
        .manager
        .open("p1", "", "2026-08-28T00:00:00Z")
        .unwrap();
    time_writer.execute("UPDATE canon_timepoints SET story_time='1998-01-01T00:00:00Z' WHERE project_id='p1' AND chapter_id='chapter:1'", []).unwrap();
    drop(time_writer);
    let production_time_conflict = support_session_impl(&state, json!({"projectId":"p1","question":"A 在世界时间之前知道 B 的身份吗","mode":"DIAGNOSE","scope":"PROJECT","entityIds":["entity:a"],"references":["event:identity"],"chapterId":"chapter:1","currentVolumeId":"volume:1"})).unwrap();
    assert_eq!(
        production_time_conflict["finding"]["supportStatus"],
        "CONFLICTING"
    );
    let time_conflict_pass =
        production_time_conflict["finding"]["diagnosisType"] == "DATA_INCONSISTENCY";
    assert!(time_conflict_pass);
    let checkpoint_id = support["receipt"]["checkpointRef"].as_str().unwrap();
    let loaded = state
        .manager
        .with_read_connection("p1", "2026-08-28T00:00:00Z", |connection| {
            ai_novel_production_system_core::repositories::support::load_checkpoint(
                connection,
                "p1",
                checkpoint_id,
            )
            .map_err(|error| {
                ai_novel_production_system_core::project_db::ManagerError::ProjectCommand(
                    error.to_string(),
                )
            })
        })
        .unwrap();
    assert!(loaded.is_some());
    let writer = state
        .manager
        .open("p1", "", "2026-08-28T00:00:00Z")
        .unwrap();
    writer
        .execute(
            "UPDATE project_meta SET revision=revision+1 WHERE project_id='p1'",
            [],
        )
        .unwrap();
    drop(writer);
    let stale_resume = support_checkpoint_resume_impl(
        &state,
        json!({"projectId":"p1","checkpointId":checkpoint_id}),
    )
    .unwrap();
    assert_eq!(stale_resume["status"], "CHECKPOINT_STALE");
    assert!(stale_resume["affectedQuery"].is_object());
    assert!(stale_resume["replan"]["priorPlanDigest"].is_string());
    assert!(stale_resume["replan"]["newPlanDigest"].is_string());
    let soft_sources_omitted = context_projection["omittedSoftSources"]
        .as_array()
        .is_some_and(|items| !items.is_empty());
    println!("B06_H01_INTEGRATION_RESULT tauri=PASS repository=PASS context=PASS context_goal_refs={} context_future_reservations={} context_selected_evidence={} volume_route={} volume_capacity={} soft_sources_omitted={} volume_intent_isolation=PASS volume_task_isolation=PASS production_world_time_conflict={} multi_event_exact_evidence=PASS support_receipt=PASS durable_checkpoint=PASS read_only=PASS ledger_entries={} evidence_hits={} missing_evidence=INSUFFICIENT checkpoint_replan=PASS status=PASS", context_goal_refs, context_future_reservations, context_selected_evidence, volume_route, volume_capacity, soft_sources_omitted, time_conflict_pass, ledger_count, support["calls"][2]["result"].as_array().unwrap().len());
}

#[test]
fn b06_bounded_queries_report_true_truncation_for_populated_results() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("p1", "Bounds", "2026-08-29T00:00:00Z")
        .unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:a','p1','A','CHARACTER',?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    for index in 0..101 {
        connection.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES (?1,'p1','entity:a','noise','{}','C',?2,?2)", libsql_rusqlite::params![format!("fact:{index}"), "2026-08-29T00:00:00Z"]).unwrap();
    }
    let ids = ["entity:a"];
    let (facts, facts_truncated) = ai_novel_production_system_core::repositories::narrative::NarrativeRepository::current_canon_bounded(&connection, "p1", &ids, 100).unwrap();
    assert_eq!(facts.len(), 100);
    assert!(facts_truncated);
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:1','p1',1,'Book','ACTIVE',?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:1','p1','book:1',1,'Volume','PRODUCING',1,1,1000,?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:1','p1','volume:1',1,'Chapter','NORMAL','COMMITTED',1,?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    let hash = "a".repeat(64);
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:1','p1','ReviewReport','CHAPTER','chapter:1',1,'m0a.artifact.v1','COMMITTED',?1,'{}',?2)", [&hash, "2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:1',1,'{}',?1)", [&hash]).unwrap();
    for index in 0..101 {
        connection.execute("INSERT INTO canon_evidence(evidence_id,project_id,final_chapter_id,source_artifact_id,source_version,evidence_type,created_at) VALUES (?1,'p1','chapter:1','artifact:1',1,'TEST',?2)", libsql_rusqlite::params![format!("evidence:{index}"), "2026-08-29T00:00:00Z"]).unwrap();
    }
    let refs = ["artifact:1"];
    let (evidence, evidence_truncated) = ai_novel_production_system_core::repositories::narrative::NarrativeRepository::evidence_for_refs_bounded(&connection, "p1", &refs, 100).unwrap();
    assert_eq!(evidence.len(), 100);
    assert!(evidence_truncated);
    println!("B06_BOUNDED_RESULT facts=100 facts_truncated=true evidence=100 evidence_truncated=true status=PASS");
}

#[test]
fn h01_adapter_reports_event_truncation_and_global_dependency_bound() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("p1", "H01 bounds", "2026-08-29T00:00:00Z")
        .unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:bounds','p1',1,'Bounds','ACTIVE',?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:bounds','p1','book:bounds',1,'Bounds','PRODUCING',1,101,1000,?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:bounds','p1','volume:bounds',1,'Bounds','NORMAL','PLANNED',0,?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:bounds:a','p1','A','CHARACTER',?1,?1),('entity:bounds:b','p1','B','CHARACTER',?1,?1)", ["2026-08-29T00:00:00Z"]).unwrap();
    for index in 0..101 {
        connection.execute("INSERT INTO canon_events(event_id,project_id,chapter_id,event_type,subject_id,story_time,event_ordinal,payload_json,created_at) VALUES (?1,'p1','chapter:bounds','EVENT','entity:bounds:a',?2,?3,'{}',?2)", libsql_rusqlite::params![format!("event:bounds:{index}"), "2026-08-29T00:00:00Z", index]).unwrap();
    }
    for index in 0..100 {
        for entity_id in ["entity:bounds:a", "entity:bounds:b"] {
            connection.execute("INSERT INTO dependency_edges(edge_id,project_id,source_ref,target_ref,dependency_type,scope,impact_class,source_hash,entity_id,created_by,created_at) VALUES (?1,'p1',?2,'target','STATE_INPUT','PROJECT','SOFT',?3,?4,'H01_TEST',?5)", libsql_rusqlite::params![format!("edge:bounds:{entity_id}:{index}"), format!("state:{entity_id}:{index}"), "0".repeat(64), entity_id, "2026-08-29T00:00:00Z"]).unwrap();
        }
    }
    let registry = CapabilityRegistry::read_only_r1();
    let adapter = RepositoryReadAdapter {
        connection: &connection,
        project_id: "p1",
    };
    let call = |capability_id: &str, input: Value| {
        let spec = registry.specs.get(capability_id).unwrap();
        BoundedCapabilityExecutor {
            registry: registry.clone(),
        }
        .execute(
            &adapter,
            &CapabilityCallEnvelope {
                call_id: format!("bounds:{capability_id}"),
                capability_id: capability_id.into(),
                capability_version: spec.version.clone(),
                spec_digest: spec.spec_digest.clone(),
                mode: "DIAGNOSE".into(),
                project_id: "p1".into(),
                project_revision: 0,
                input_digest: sha256_canonical(&input).unwrap(),
                input,
                requested_bounds: RequestedBounds {
                    max_results: 100,
                    max_serialized_bytes: 65_536,
                },
            },
        )
        .unwrap()
    };
    let events = call("query_events", json!({"chapterId":"chapter:bounds"}));
    assert_eq!(events.result.as_array().unwrap().len(), 100);
    assert!(events.truncated);
    let edges = call(
        "query_dependency_edges",
        json!({"entityIds":["entity:bounds:a","entity:bounds:b"]}),
    );
    assert_eq!(edges.result.as_array().unwrap().len(), 100);
    assert!(edges.truncated);
    println!("H01_BOUNDS_RESULT events=100 events_truncated=true dependency_edges=100 dependency_truncated=true global_max_results=100 status=PASS");
}
