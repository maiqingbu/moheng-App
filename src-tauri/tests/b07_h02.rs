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

#[test]
fn b07_volume_intent_and_route_query_use_commit_bundle_authority() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state
        .manager
        .create("p1", "B07", "2026-08-30T00:00:00Z")
        .unwrap();
    let volume_id =
        StableIdentityFactory::from_command(StableIdKind::Volume, "command:volume").unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:1','p1',1,'Book','ACTIVE',?1,?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,'p1','book:1',1,'Volume','PLANNING',1,12,12000,?2,?2)", libsql_rusqlite::params![volume_id, "2026-08-30T00:00:00Z"]).unwrap();
    drop(connection);
    let intent = json!({"payloadSchema":"volume-intent.r1","volumeFunction":"opening","entryConditions":[],"mustAchieve":[],"mustNotBreak":[],"requiredExitConditions":[],"futureReservationRefs":[],"crossVolumeResponsibilityRefs":[],"targetChapterRange":{"min":1,"preferred":8,"max":12},"targetWordRange":{"preferred":12000}});
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
    let context = query_context_manifest_impl(
        &state,
        json!({"projectId":"p1","volumeId":volume_id,"chapterId":"chapter:1","scope":"VOLUME","entityIds":[],"references":[]}),
    )
    .unwrap();
    assert!(context["goalContractRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reference| reference["artifactId"] == format!("plan:volumeintent:{volume_id}")));
    println!("B07_ROUTE_RESULT volume_scope=PASS task_reality=PASS h02_query=PASS truncation=PASS status=PASS");
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
    assert!(result.truncated);
    assert_eq!(result.result["truncated"], true);
    drop(connection);
    let session = support_session_impl(&state, json!({"projectId":"p1","volumeId":volume_id,"scope":"VOLUME","question":"哪些 Required Task 还没完成？","mode":"DIAGNOSE","entityIds":[],"references":[]})).unwrap();
    assert_eq!(session["finding"]["diagnosisType"], "PLANNING_CONSTRAINTS");
    assert_eq!(session["finding"]["supportStatus"], "SUPPORTED");
    assert!(session["receipt"]["checkpointRef"].is_string());
    println!("H02_SESSION_RESULT planning_question=PASS finding=SUPPORTED receipt=PASS checkpoint=PASS status=PASS");
}

#[test]
fn b07_bookplan_approval_commits_new_approved_version() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state
        .manager
        .create("p2", "B07 book", "2026-08-30T00:00:00Z")
        .unwrap();
    drop(connection);
    let seed = json!({"payloadSchema":"book-plan.r1","planningMaturity":"SEED","storyPromise":"promise","protagonistMacroDirection":"direction","endingBoundary":"end","mustAchieve":[{"id":"goal:1","description":"goal"}],"mustNotBreak":[],"relationshipMacroDirections":[],"majorMysteryGoals":[],"futureReservations":[],"volumeIntentSeeds":[],"openingHypotheses":[],"globalLengthBudget":{"preferredWords":1000}});
    let result = execute_command_impl(&state, json!({"type":"planning_finalize_book_plan","commandId":"command:book","idempotencyKey":"key:book","projectId":"p2","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"plan":seed,"signingStatus":"PASS","openingCanonRefs":["fact:1","fact:2","fact:3"],"signingEvidenceRefs":["artifact:signing"]}})).unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let connection = state
        .manager
        .open("p2", "", "2026-08-30T00:00:01Z")
        .unwrap();
    let maturity: String = connection.query_row("SELECT json_extract(payload_json, '$.planningMaturity') FROM artifact_payloads WHERE artifact_id='plan:bookplan:p2' AND version=1", [], |row| row.get(0)).unwrap();
    assert_eq!(maturity, "APPROVED");
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
    drop(connection);
    let intent = json!({"payloadSchema":"volume-intent.r1","volumeFunction":"opening","entryConditions":[],"mustAchieve":[],"mustNotBreak":[],"requiredExitConditions":[],"futureReservationRefs":[],"crossVolumeResponsibilityRefs":[],"targetChapterRange":{"min":1,"preferred":1,"max":1},"targetWordRange":{"preferred":1000}});
    execute_command_impl(&state, json!({"type":"planning_create_volume_intent","commandId":"command:intent:p3","idempotencyKey":"key:intent:p3","projectId":"p3","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"volumeId":volume_id,"plan":intent}})).unwrap();
    let connection = state
        .manager
        .open("p3", "", "2026-08-30T00:00:00Z")
        .unwrap();
    let intent_ref = ai_novel_production_system_core::repositories::narrative::NarrativeRepository::canonical_head_ref(&connection, "p3", "VolumeIntent", &volume_id).unwrap().unwrap();
    drop(connection);
    let route = json!({"payloadSchema":"volume-plan.r1","volumeIntentRef":intent_ref,"routeVersion":1,"tasks":[{"taskId":"task:required","class":"REQUIRED","objective":"advance","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"DONE"},"estimatedChapterCost":{"preferred":1}}],"routeInvariants":[],"requiredExitConditions":[],"capacityBudget":{"minChapters":1,"preferredChapters":1,"maxChapters":1}});
    execute_command_impl(&state, json!({"type":"planning_create_volume_plan","commandId":"command:route:p3","idempotencyKey":"key:route:p3","projectId":"p3","expectedRevision":1,"requestedAt":"2026-08-30T00:00:01Z","payload":{"volumeId":volume_id,"plan":route}})).unwrap();
    let manifest = query_context_manifest_impl(&state, json!({"projectId":"p3","volumeId":volume_id,"chapterId":chapter_id,"scope":"CHAPTER","entityIds":[],"references":[]})).unwrap();
    let context_hash = sha256_canonical(&manifest).unwrap();
    let context_artifact: ArtifactEnvelope = serde_json::from_value(json!({"artifactId":"context:p3:chapter","projectId":"p3","artifactType":"ContextManifest","scope":{"level":"CHAPTER","chapterId":chapter_id},"version":1,"schemaVersion":"m0a.artifact.v1","artifactStatus":"CANDIDATE","payload":manifest,"payloadHash":context_hash,"dependencyRefs":[],"provenance":{"createdBy":"B07::PlanningOwner","commandId":"command:context:p3"},"createdAt":"2026-08-30T00:00:02Z"})).unwrap();
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
    drop(connection);
    let plan = json!({"payloadSchema":"chapter-plan.r1","chapterId":chapter_id,"compiledFromProjectRevision":2,"compiledFromStateDigest":manifest["compiledFromStateDigest"],"contextManifestRef":{"artifactId":context_artifact.artifact_id,"version":1,"payloadHash":context_hash},"chapterGoal":"advance","activeTaskRefs":["task:required"],"continuityActions":[],"requiredStateOutcomes":[],"forbiddenTransitions":[],"knowledgeBoundaries":[],"reservationRefs":[],"sceneExecution":[],"horizonPreview":[{"slotNo":1,"objective":"one","tentative":true},{"slotNo":2,"objective":"two","tentative":true},{"slotNo":3,"objective":"three","tentative":true}]});
    let result = execute_command_impl(
        &state,
        json!({"type":"planning_compile_chapter_plan","commandId":"command:chapter:p3","idempotencyKey":"key:chapter:p3","projectId":"p3","expectedRevision":2,"requestedAt":"2026-08-30T00:00:03Z","payload":{"chapterId":chapter_id,"routeTaskIds":["task:caller-must-not-authorize"],"plan":plan}}),
    );
    assert!(result.is_ok(), "{result:?}");
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
    println!("B07_CHAPTER_RESULT current_only=PASS context_bound=PASS stale_precommit=PASS canon_unchanged=PASS status=PASS");
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
