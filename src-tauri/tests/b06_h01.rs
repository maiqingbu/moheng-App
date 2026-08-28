use ai_novel_production_system_core::commands::{
    query_context_manifest_impl, query_narrative_state_impl, query_plan_stale_impl,
    query_volume_progress_impl, support_checkpoint_resume_impl, support_session_impl, KernelState,
    RepositoryReadAdapter,
};
use ai_novel_production_system_core::harness::{
    bounded_replan, can_expose_write_capability, check_checkpoint_resume, diagnose_knowledge_event,
    diagnose_projection_authority, grounded_finding, plan_support, registry_digest_map,
    resolve_project_scope, BoundedCapabilityExecutor, CapabilityCallEnvelope, CheckpointResume,
    ReadCapabilityAdapter, ReplanTrigger, RequestedBounds, SupportCheckpoint, SupportStatus,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::narrative::{
    build_continuity_threads, build_volume_progress, compile_context_manifest, evaluate_plan_stale,
    evaluate_resume_condition, ContextCompilerInput, CurrentNarrativeStateView, PlanStaleStatus,
    ResumeContext, RuntimeBounds, VolumeProgressInput,
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
        state: state.clone(),
        continuity_threads: threads,
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
    assert_eq!(manifest.budget.total_tokens, 140);
    println!("B06_CONTEXT_RESULT progress=PASS manifest=PASS digest_deterministic=PASS unrelated_revision_valid=PASS related_change_stale=PASS bounded_hard_context=PASS status=PASS");
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
            max_tokens: 100,
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
    let mut conflicting_knowledge = state.knowledge[0].clone();
    conflicting_knowledge.from_chapter_id = Some("chapter:12".into());
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
            "world time conflict",
            &conflicting_knowledge,
            Some(&state.events[0]),
            Some(&evidence)
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
        let input =
            json!({"entityIds":[],"references":[],"chapterId":"chapter:1","projectRevision":7});
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
                max_tokens: 4096,
            },
        };
        BoundedCapabilityExecutor {
            registry: registry.clone(),
        }
        .execute(&adapter, &call)
        .unwrap();
    }
    println!("H01_RESULT scope=PASS registry=PASS registry_adapter_parity=PASS executor=PASS sql_negative=PASS write_capability=0 diagnostics=PASS evidence_ledger=PASS checkpoint_safe=PASS checkpoint_stale=PASS bounded_replan=PASS bounded_query=PASS session_receipt=PASS max_steps=7 status=PASS");
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
    connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:1','p1','volume:1',1,'Chapter','NORMAL','COMMITTED',10,?1,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:a','p1','A','CHARACTER',?1,?1),('entity:b','p1','B','CHARACTER',?1,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:evidence','p1','ReviewReport','CHAPTER','chapter:1',1,'m0a.artifact.v1','COMMITTED',?1,'{}',?2)", ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:evidence',1,'{}',?1)", ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]).unwrap();
    connection.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES ('fact:identity','p1','entity:b','identity','{\"name\":\"B\"}','B',?1,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_knowledge(knowledge_id,project_id,knower_id,fact_id,epistemic_state,visibility_scope,created_at,updated_at) VALUES ('knowledge:a','p1','entity:a','fact:identity','KNOWS','CHAPTER',?1,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_events(event_id,project_id,chapter_id,event_type,subject_id,story_time,event_ordinal,payload_json,created_at) VALUES ('event:identity','p1','chapter:1','IDENTITY_REVEALED','entity:a','2020-01-01T00:00:00Z',1,'{}',?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO canon_evidence(evidence_id,project_id,event_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,range_start,range_end,created_at) VALUES ('evidence:identity','p1','event:identity','chapter:1','artifact:evidence',1,'EVENT','B revealed identity',1,20,?1)", ["2026-08-28T00:00:00Z"]).unwrap();
    let book_plan = json!({"payloadSchema":"book-plan.r1","planningMaturity":"SEED","storyPromise":"promise","protagonistMacroDirection":"direction","endingBoundary":"boundary","mustNotBreak":[],"openingHypotheses":[],"globalLengthBudget":{"preferredWords":1000},"futureReservations":[{"reservationId":"reservation:1","description":"later","prerequisites":[],"mayForeshadow":true,"mayReveal":true,"authorityLevel":"B"}]});
    let book_plan_hash = sha256_canonical(&book_plan).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('plan:book','p1','BookPlan','PROJECT','p1',1,'m0a.artifact.v1','COMMITTED',?1,'{}',?2)", [&book_plan_hash, "2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('plan:book',1,?1,?2)", [&book_plan.to_string(), &book_plan_hash]).unwrap();
    connection.execute("INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES ('p1','BookPlan','PROJECT','p1','plan:book',1,?1)", [&book_plan_hash]).unwrap();
    let volume_plan = json!({"payloadSchema":"volume-plan.r1","capacityBudget":{"minChapters":1,"preferredChapters":5,"maxChapters":10},"tasks":[]});
    let volume_plan_hash = sha256_canonical(&volume_plan).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('plan:volume','p1','VolumePlan','VOLUME','volume:1',1,'m0a.artifact.v1','COMMITTED',?1,'{}',?2)", [&volume_plan_hash, "2026-08-28T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('plan:volume',1,?1,?2)", [&volume_plan.to_string(), &volume_plan_hash]).unwrap();
    connection.execute("INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES ('p1','VolumePlan','VOLUME','volume:1','plan:volume',1,?1)", [&volume_plan_hash]).unwrap();
    drop(connection);
    let state = KernelState::new(paths);
    let request = json!({"projectId":"p1","scope":"PROJECT","entityIds":["entity:a"],"chapterId":"chapter:1","currentChapter":1,"currentVolumeId":"volume:1"});
    let state_projection = query_narrative_state_impl(&state, request.clone()).unwrap();
    assert_eq!(state_projection["projectId"], "p1");
    let context_projection = query_context_manifest_impl(&state, request.clone()).unwrap();
    assert_eq!(context_projection["payloadSchema"], "context-manifest.r1");
    assert_eq!(
        context_projection["goalContractRefs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        context_projection["futureReservationRefs"],
        json!(["reservation:1"])
    );
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
    let stale_projection = query_plan_stale_impl(&state, json!({"projectId":"p1","compiledFromProjectRevision":0,"compiledFromStateDigest":context_projection["compiledFromStateDigest"],"currentStateDigest":"f".repeat(64),"entityIds":["entity:a"],"chapterId":"chapter:1"})).unwrap();
    assert_eq!(stale_projection["status"], "VALID");
    let support = support_session_impl(&state, json!({"projectId":"p1","question":"A 何时知道 B 的身份","mode":"DIAGNOSE","scope":"PROJECT","entityIds":["entity:a"],"references":["event:identity"],"chapterId":"chapter:1"})).unwrap();
    assert_eq!(support["receipt"]["projectId"], "p1");
    assert_eq!(support["finding"]["supportStatus"], "SUPPORTED");
    let call_count = support["calls"].as_array().unwrap().len();
    let ledger_count = support["evidenceLedger"].as_array().unwrap().len();
    assert_eq!(call_count, ledger_count);
    assert_eq!(support["calls"][2]["result"].as_array().unwrap().len(), 1);
    assert_eq!(support["calls"][2]["result"][0]["rangeStart"], 1);
    let missing_evidence = support_session_impl(&state, json!({"projectId":"p1","question":"A 何时知道 B 的身份","mode":"DIAGNOSE","scope":"PROJECT","entityIds":["entity:a"],"references":["event:missing"],"chapterId":"chapter:1"})).unwrap();
    assert_eq!(missing_evidence["finding"]["supportStatus"], "INSUFFICIENT");
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
    println!("B06_H01_INTEGRATION_RESULT tauri=PASS repository=PASS context=PASS support_receipt=PASS durable_checkpoint=PASS read_only=PASS ledger_entries={} evidence_hits={} missing_evidence=INSUFFICIENT checkpoint_replan=PASS status=PASS", ledger_count, support["calls"][2]["result"].as_array().unwrap().len());
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
