use ai_novel_production_system_core::commands::{
    execute_command_impl, query_dependency_summary_bootstrap_impl, query_retcon_context_impl,
    KernelState,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::narrative::{evaluate_plan_stale, PlanStaleStatus};
use ai_novel_production_system_core::production::UnavailableProductionGateway;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repair::{operation_digest_with_postconditions, RepairPlan};
use ai_novel_production_system_core::repositories::change_retcon::rebuild_global_dependency_summary;
use ai_novel_production_system_core::repositories::runtime::{
    lease_expiry, DurableJobService, DEPENDENCY_SUMMARY_BOOTSTRAP_STAGE, DEPENDENCY_SUMMARY_WORKER,
};
use libsql_rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Instant;

const NOW: &str = "2026-09-02T05:00:00Z";

#[test]
fn b12_same_revision_with_changed_authority_digest_is_stale() {
    assert_eq!(
        evaluate_plan_stale(7, &"a".repeat(64), 7, &"b".repeat(64)),
        PlanStaleStatus::PlanStale
    );
}

#[test]
fn b12_change_execute_cannot_replace_approved_patch() {
    let state = test_state("p-b12-r01-override");
    insert_book_plan(&state, "p-b12-r01-override", "plan:book:p-b12-r01-override");
    let digest = create_and_approve_change(
        &state,
        "p-b12-r01-override",
        "change:b12:r01:override",
        &[],
        json!({
            "targetAuthority":{"artifactType":"BookPlan","artifactId":"plan:book:p-b12-r01-override"},
            "patch":{"storyPromise":"A→B"}
        }),
    );
    let error = command(
        &state,
        "p-b12-r01-override",
        "change_request_apply",
        "cmd:b12:r01:override:apply",
        0,
        json!({
            "changeRequestId":"change:b12:r01:override",
            "impactDigest":digest,
            "requestedChange":{"targetAuthority":{"artifactType":"BookPlan","artifactId":"plan:book:p-b12-r01-override"},"patch":{"endingBoundary":"C"}}
        }),
    )
    .unwrap_err();
    assert_eq!(error, "B12_APPROVED_OPERATION_INPUT_FORBIDDEN");
}

#[test]
fn b12_change_execute_cannot_replace_approved_target() {
    let state = test_state("p-b12-r01-target");
    insert_book_plan(&state, "p-b12-r01-target", "plan:book:p-b12-r01-target");
    let digest = create_and_approve_change(
        &state,
        "p-b12-r01-target",
        "change:b12:r01:target",
        &[],
        json!({
            "targetAuthority":{"artifactType":"BookPlan","artifactId":"plan:book:p-b12-r01-target"},
            "patch":{"storyPromise":"已批准目标"}
        }),
    );
    let error = command(
        &state,
        "p-b12-r01-target",
        "change_request_apply",
        "cmd:b12:r01:target:apply",
        0,
        json!({
            "changeRequestId":"change:b12:r01:target",
            "impactDigest":digest,
            "targetAuthority":{"artifactType":"ChapterPlan","artifactId":"plan:chapter:other"}
        }),
    )
    .unwrap_err();
    assert_eq!(error, "B12_APPROVED_OPERATION_INPUT_FORBIDDEN");
}

#[test]
fn b12_change_target_head_change_is_decision_stale_before_apply() {
    let state = test_state("p-b12-r01-head");
    insert_book_plan(&state, "p-b12-r01-head", "plan:book:p-b12-r01-head");
    let digest = create_and_approve_change(
        &state,
        "p-b12-r01-head",
        "change:b12:r01:head",
        &[],
        json!({
            "targetAuthority":{"artifactType":"BookPlan","artifactId":"plan:book:p-b12-r01-head"},
            "patch":{"storyPromise":"批准版本"}
        }),
    );
    let handle = state.manager.open("p-b12-r01-head", "", NOW).unwrap();
    let current_hash: String = handle
        .query_row(
            "SELECT payload_hash FROM canonical_heads WHERE project_id=?1 AND artifact_type='BookPlan'",
            ["p-b12-r01-head"],
            |row| row.get(0),
        )
        .unwrap();
    handle
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,authority_level,payload_hash,provenance_json,created_at,committed_at) SELECT artifact_id,project_id,artifact_type,scope_level,scope_key,2,schema_version,'COMMITTED',semantic_lifecycle,authority_level,payload_hash,provenance_json,created_at,committed_at FROM artifacts WHERE project_id=?1 AND artifact_id='plan:book:p-b12-r01-head' AND version=1",
            ["p-b12-r01-head"],
        )
        .unwrap();
    handle
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) SELECT artifact_id,2,payload_json,payload_hash FROM artifact_payloads WHERE artifact_id='plan:book:p-b12-r01-head' AND version=1",
            [],
        )
        .unwrap();
    handle
        .execute(
            "UPDATE canonical_heads SET version=2 WHERE project_id=?1 AND artifact_type='BookPlan'",
            ["p-b12-r01-head"],
        )
        .unwrap();
    assert_eq!(current_hash.len(), 64);
    drop(handle);
    let error = command(
        &state,
        "p-b12-r01-head",
        "change_request_apply",
        "cmd:b12:r01:head:apply",
        0,
        json!({"changeRequestId":"change:b12:r01:head","impactDigest":digest}),
    )
    .unwrap_err();
    assert_eq!(error, "B12_DECISION_STALE");
}

#[test]
fn b12_unsupported_authority_target_fails_closed_without_apply() {
    let state = test_state("p-b12-r01-unsupported");
    command(
        &state,
        "p-b12-r01-unsupported",
        "change_request_create",
        "cmd:b12:r01:unsupported:create",
        0,
        json!({
            "changeRequestId":"change:b12:r01:unsupported",
            "changeType":"CHANGE",
            "scope":"PROJECT",
            "summary":"尝试绕过故事核心责任方",
            "requestedChange":{"targetAuthority":{"artifactType":"StoryConstitution","artifactId":"constitution:unsupported"},"patch":{"storyPromise":"不应直接写入"}}
        }),
    )
    .unwrap();
    let preflight = command(
        &state,
        "p-b12-r01-unsupported",
        "impact_preflight",
        "cmd:b12:r01:unsupported:preflight",
        0,
        json!({"changeRequestId":"change:b12:r01:unsupported"}),
    )
    .unwrap();
    let digest = preflight["result"]["impactDigest"].as_str().unwrap();
    let error = command(
        &state,
        "p-b12-r01-unsupported",
        "change_request_decide",
        "cmd:b12:r01:unsupported:decision",
        0,
        json!({"changeRequestId":"change:b12:r01:unsupported","decision":"APPROVE","impactDigest":digest,"scopeAcknowledged":true,"riskAcknowledged":true}),
    )
    .unwrap_err();
    assert_eq!(error, "B12_CHANGE_TARGET_UNSUPPORTED");
}

#[test]
fn b12_dependency_summary_bootstrap_formal_job_is_created_and_reusable() {
    let state = test_state("p-b12-r06-job");
    let handle = state.manager.open("p-b12-r06-job", "", NOW).unwrap();
    handle.execute_batch("BEGIN IMMEDIATE").unwrap();
    for index in 0..1_001 {
        handle
            .execute(
                "INSERT INTO dependency_edges(edge_id,project_id,source_ref,target_ref,dependency_type,scope,impact_class,source_hash,created_by,created_at) VALUES (?1,?2,'bootstrap:source',?3,'PLAN_INPUT','BOOK','HARD',?4,'B12_R06_TEST',?5)",
                params![
                    format!("edge:b12:r06:job:{index}"),
                    "p-b12-r06-job",
                    format!("bootstrap:target:{index}"),
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    NOW
                ],
            )
            .unwrap();
    }
    handle.execute_batch("COMMIT").unwrap();
    drop(handle);
    command(
        &state,
        "p-b12-r06-job",
        "change_request_create",
        "cmd:b12:r06:job:create",
        0,
        json!({"changeRequestId":"change:b12:r06:job","changeType":"CHANGE","scope":"GLOBAL","summary":"准备影响索引","requestedChange":{"target":"全局路线"}}),
    )
    .unwrap();
    let preflight = command(
        &state,
        "p-b12-r06-job",
        "impact_preflight",
        "cmd:b12:r06:job:preflight",
        0,
        json!({"changeRequestId":"change:b12:r06:job"}),
    )
    .unwrap();
    assert_eq!(preflight["result"]["status"], "SUMMARY_BOOTSTRAP_REQUIRED");
    assert!(preflight["result"]["bootstrapJobId"].as_str().is_some());
    let bootstrap = command(
        &state,
        "p-b12-r06-job",
        "dependency_summary_bootstrap",
        "cmd:b12:r06:job:bootstrap",
        0,
        json!({}),
    )
    .unwrap();
    assert_eq!(bootstrap["result"]["status"], "QUEUED");
    assert_eq!(bootstrap["result"]["workerDispatched"], true);
    let bootstrap_status = wait_for_summary_ready(&state, "p-b12-r06-job");
    assert_eq!(bootstrap_status["job"]["status"], "COMPLETED");
    let second = command(
        &state,
        "p-b12-r06-job",
        "dependency_summary_bootstrap",
        "cmd:b12:r06:job:bootstrap:again",
        0,
        json!({}),
    )
    .unwrap();
    assert_eq!(second["result"]["status"], "READY");
    assert_eq!(second["result"]["jobId"], bootstrap["result"]["jobId"]);
    let handle = state.manager.open("p-b12-r06-job", "", NOW).unwrap();
    handle
        .execute(
            "UPDATE jobs SET status='RUNNING' WHERE project_id=?1 AND current_stage=?2",
            params!["p-b12-r06-job", DEPENDENCY_SUMMARY_BOOTSTRAP_STAGE],
        )
        .unwrap();
    DurableJobService::acquire_lease(
        &handle,
        "p-b12-r06-job",
        bootstrap["result"]["jobId"].as_str().unwrap(),
        DEPENDENCY_SUMMARY_WORKER,
        &lease_expiry(NOW),
        NOW,
    )
    .unwrap();
    DurableJobService::recover_after_restart(&handle, "p-b12-r06-job", NOW).unwrap();
    drop(handle);
    let resumed = command(
        &state,
        "p-b12-r06-job",
        "dependency_summary_bootstrap",
        "cmd:b12:r06:job:restart",
        0,
        json!({}),
    )
    .unwrap();
    assert_eq!(resumed["result"]["status"], "QUEUED");
    let resumed_status = wait_for_summary_ready(&state, "p-b12-r06-job");
    assert_eq!(resumed_status["summary"]["summaryStatus"], "READY");
    state
        .manager
        .with_read_connection("p-b12-r06-job", NOW, |connection| {
            let (count, status): (i64, String) = connection
                .query_row(
                    "SELECT COUNT(*),status FROM jobs WHERE project_id=?1 AND current_stage='DEPENDENCY_SUMMARY_BOOTSTRAP'",
                    ["p-b12-r06-job"],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(count, 1);
            assert_eq!(status, "COMPLETED");
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();
    println!(
        "B12_SUMMARY_BOOTSTRAP_RESULT jobCreated=PASS restart=PASS concurrentJobs=1 status=PASS"
    );
}

#[test]
fn b12_dependency_summary_bootstrap_concurrent_requests_share_one_job() {
    let temp = tempfile::tempdir().unwrap();
    let root = Box::leak(Box::new(temp)).path().to_path_buf();
    let setup = KernelState::with_production_gateway(
        AppPaths::new(root.clone()),
        Arc::new(UnavailableProductionGateway),
    );
    setup
        .manager
        .create("p-b12-r06-concurrent", "B12 并发索引测试", NOW)
        .unwrap();
    let handle = setup.manager.open("p-b12-r06-concurrent", "", NOW).unwrap();
    handle.execute_batch("BEGIN IMMEDIATE").unwrap();
    for index in 0..1_001 {
        handle
            .execute(
                "INSERT INTO dependency_edges(edge_id,project_id,source_ref,target_ref,dependency_type,scope,impact_class,source_hash,created_by,created_at) VALUES (?1,?2,'concurrent:source',?3,'PLAN_INPUT','BOOK','HARD',?4,'B12_R06_TEST',?5)",
                params![
                    format!("edge:b12:r06:concurrent:{index}"),
                    "p-b12-r06-concurrent",
                    format!("concurrent:target:{index}"),
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    NOW
                ],
            )
            .unwrap();
    }
    handle.execute_batch("COMMIT").unwrap();
    drop(handle);
    command(
        &setup,
        "p-b12-r06-concurrent",
        "change_request_create",
        "cmd:b12:r06:concurrent:create",
        0,
        json!({"changeRequestId":"change:b12:r06:concurrent","changeType":"CHANGE","scope":"GLOBAL","summary":"并发准备索引","requestedChange":{"target":"全局路线"}}),
    )
    .unwrap();
    let preflight = command(
        &setup,
        "p-b12-r06-concurrent",
        "impact_preflight",
        "cmd:b12:r06:concurrent:preflight",
        0,
        json!({"changeRequestId":"change:b12:r06:concurrent"}),
    )
    .unwrap();
    assert_eq!(preflight["result"]["status"], "SUMMARY_BOOTSTRAP_REQUIRED");

    let first = KernelState::with_production_gateway(
        AppPaths::new(root.clone()),
        Arc::new(UnavailableProductionGateway),
    );
    let second = KernelState::with_production_gateway(
        AppPaths::new(root),
        Arc::new(UnavailableProductionGateway),
    );
    let barrier = Arc::new(Barrier::new(2));
    let first_barrier = barrier.clone();
    let second_barrier = barrier;
    let first_thread = thread::spawn(move || {
        first_barrier.wait();
        command(
            &first,
            "p-b12-r06-concurrent",
            "dependency_summary_bootstrap",
            "cmd:b12:r06:concurrent:worker-a",
            0,
            json!({}),
        )
    });
    let second_thread = thread::spawn(move || {
        second_barrier.wait();
        command(
            &second,
            "p-b12-r06-concurrent",
            "dependency_summary_bootstrap",
            "cmd:b12:r06:concurrent:worker-b",
            0,
            json!({}),
        )
    });
    assert!(first_thread.join().unwrap().is_ok());
    assert!(second_thread.join().unwrap().is_ok());

    let jobs = setup
        .manager
        .with_read_connection("p-b12-r06-concurrent", NOW, |connection| {
            let jobs: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM jobs WHERE project_id=?1 AND current_stage=?2",
                    params!["p-b12-r06-concurrent", DEPENDENCY_SUMMARY_BOOTSTRAP_STAGE],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(jobs, 1);
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(jobs)
        })
        .unwrap();
    println!("B12_SUMMARY_BOOTSTRAP_CONCURRENCY_RESULT concurrentJobs={jobs} status=PASS");
}

#[test]
fn b12_retcon_context_query_returns_current_fact_and_typed_evidence_options() {
    let state = test_state("p-b12-r05-context");
    let handle = state.manager.open("p-b12-r05-context", "", NOW).unwrap();
    handle
        .execute(
            "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:r05:context','p-b12-r05-context','父亲','CHARACTER',?1,?1)",
            [NOW],
        )
        .unwrap();
    handle
        .execute(
            "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES ('fact:r05:context','p-b12-r05-context','entity:r05:context','状态','{\"description\":\"已经死亡\"}','A',?1,?1)",
            [NOW],
        )
        .unwrap();
    handle
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES ('evidence:r05:context','p-b12-r05-context','ReviewReport','PROJECT','p-b12-r05-context',2,'m0a.artifact.v1','COMMITTED',?1,'{}',?2,?2)",
            params!["a".repeat(64), NOW],
        )
        .unwrap();
    let context = ai_novel_production_system_core::repositories::change_retcon::retcon_context(
        &handle,
        "p-b12-r05-context",
        Some("父亲"),
        20,
    )
    .unwrap();
    assert_eq!(context["facts"][0]["factId"], "fact:r05:context");
    assert_eq!(context["facts"][0]["current"], true);
    assert!(context["facts"][0]["label"]
        .as_str()
        .unwrap()
        .contains("已经死亡"));
    assert_eq!(context["evidence"][0]["version"], 2);
    assert_eq!(context["evidence"][0]["payloadHash"], "a".repeat(64));
}

#[test]
fn b12_assistant_retcon_uses_selected_fact_source_evidence() {
    let project_id = "p-b12-r05-source";
    let state = test_state(project_id);
    let handle = state.manager.open(project_id, "", NOW).expect("open source evidence fixture");
    handle
        .execute(
            "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:r05:source',?1,'父亲','CHARACTER',?2,?2)",
            params![project_id, NOW],
        )
        .expect("insert source fact entity");
    let source_chapter_id = insert_retcon_evidence_chapter(&handle, project_id, 18);
    let unrelated_hash = insert_committed_artifact(
        &handle,
        project_id,
        "ReviewReport",
        "artifact:r05:000-unrelated",
        "PROJECT",
        json!({"level":"PROJECT"}),
        json!({"kind":"unrelated review"}),
        false,
    );
    let source_hash = insert_committed_artifact(
        &handle,
        project_id,
        "FinalChapter",
        "artifact:r05:999-fact-source",
        "CHAPTER",
        json!({"level":"CHAPTER","chapterId":source_chapter_id.as_str()}),
        json!({"chapter":"父亲死亡的正式正文"}),
        false,
    );
    handle
        .execute(
            "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,source_artifact_id,source_version,source_chapter_id,evidence_range_json,authority_level,created_at,updated_at) VALUES ('fact:r05:source',?1,'entity:r05:source','状态','{\"description\":\"已经死亡\"}', 'artifact:r05:999-fact-source',1,?2,'{\"rangeStart\":12,\"rangeEnd\":27}','A',?3,?3)",
            params![project_id, source_chapter_id, NOW],
        )
        .expect("insert fact provenance");
    drop(handle);
    let context = ai_novel_production_system_core::repositories::change_retcon::retcon_context(
        &state.manager.open(project_id, "", NOW).expect("reopen source evidence fixture"),
        project_id,
        Some("父亲死亡"),
        20,
    )
    .expect("query grounded retcon context");
    assert_eq!(context["selectedFact"]["factId"], "fact:r05:source");
    assert_eq!(context["evidence"].as_array().map(Vec::len), Some(1));
    assert_eq!(context["evidence"][0]["artifactId"], "artifact:r05:999-fact-source");
    assert_eq!(context["evidence"][0]["payloadHash"], source_hash);
    assert_eq!(context["evidence"][0]["supportsFactId"], "fact:r05:source");
    assert_ne!(context["evidence"][0]["payloadHash"], unrelated_hash);
    println!("B12_ASSISTANT_RETCON_EVIDENCE_RESULT exactFactSource=PASS status=PASS");
}

#[test]
fn b12_assistant_retcon_missing_fact_evidence_fails_closed() {
    let project_id = "p-b12-r05-gap";
    let state = test_state(project_id);
    let handle = state.manager.open(project_id, "", NOW).expect("open evidence gap fixture");
    handle
        .execute(
            "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:r05:gap',?1,'父亲','CHARACTER',?2,?2)",
            params![project_id, NOW],
        )
        .expect("insert gap entity");
    handle
        .execute(
            "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES ('fact:r05:gap',?1,'entity:r05:gap','状态','{\"description\":\"已经死亡\"}','A',?2,?2)",
            params![project_id, NOW],
        )
        .expect("insert fact without provenance");
    drop(handle);
    let context = query_retcon_context_impl(
        &state,
        json!({"projectId":project_id,"query":"父亲死亡","pageSize":20}),
    )
    .expect("query evidence gap context");
    assert_eq!(context["selectedFact"]["factId"], "fact:r05:gap");
    assert_eq!(context["evidence"], json!([]));
    assert_eq!(context["grounding"]["evidenceStatus"], "EVIDENCE_GAP");
    let error = command(
        &state,
        project_id,
        "retcon_plan_create",
        "cmd:b12:r05:gap:plan",
        0,
        json!({"changeRequestId":"missing","retconPlanId":"plan:r05:gap","targetFactId":"fact:r05:gap","reason":"不能绕过来源依据","replacement":{"objectJson":{"description":"假死"}},"evidenceRefs":[]}),
    )
    .expect_err("missing exact fact evidence must fail closed before an executable RetconPlan");
    assert_eq!(error, "B12_RETCON_EVIDENCE_GAP");
}

#[test]
fn b12_retcon_rejects_unrelated_evidence_without_relationship() {
    let project_id = "p-b12-r05-unrelated";
    let state = test_state(project_id);
    let handle = state.manager.open(project_id, "", NOW).expect("open unrelated evidence fixture");
    handle
        .execute(
            "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:r05:a',?1,'父亲','CHARACTER',?2,?2),('entity:r05:b',?1,'师父','CHARACTER',?2,?2)",
            params![project_id, NOW],
        )
        .expect("insert retcon evidence entities");
    insert_committed_artifact(
        &handle,
        project_id,
        "FinalChapter",
        "artifact:r05:fact-a",
        "CHAPTER",
        json!({"level":"CHAPTER","chapterId":"chapter-r05-a"}),
        json!({"chapter":"父亲死亡"}),
        false,
    );
    insert_committed_artifact(
        &handle,
        project_id,
        "FinalChapter",
        "artifact:r05:fact-b",
        "CHAPTER",
        json!({"level":"CHAPTER","chapterId":"chapter-r05-b"}),
        json!({"chapter":"师父死亡"}),
        false,
    );
    handle
        .execute(
            "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,source_artifact_id,source_version,authority_level,created_at,updated_at) VALUES ('fact:r05:a',?1,'entity:r05:a','状态','{\"description\":\"已经死亡\"}','artifact:r05:fact-a',1,'A',?2,?2),('fact:r05:b',?1,'entity:r05:b','状态','{\"description\":\"已经死亡\"}','artifact:r05:fact-b',1,'A',?2,?2)",
            params![project_id, NOW],
        )
        .expect("insert facts with distinct provenance");
    drop(handle);
    command(
        &state,
        project_id,
        "change_request_create",
        "cmd:b12:r05:unrelated:create",
        0,
        json!({"changeRequestId":"change:b12:r05:unrelated","changeType":"RETCON","scope":"PROJECT","summary":"修订父亲事实","targetFactId":"fact:r05:a","sourceRefs":["fact:r05:a"],"requestedChange":{"replacement":{"objectJson":{"description":"假死"}}}}),
    )
    .expect("create retcon request");
    command(
        &state,
        project_id,
        "impact_preflight",
        "cmd:b12:r05:unrelated:preflight",
        0,
        json!({"changeRequestId":"change:b12:r05:unrelated"}),
    )
    .expect("preflight retcon request");
    let error = command(
        &state,
        project_id,
        "retcon_plan_create",
        "cmd:b12:r05:unrelated:plan",
        0,
        json!({"changeRequestId":"change:b12:r05:unrelated","retconPlanId":"plan:r05:unrelated","targetFactId":"fact:r05:a","reason":"不能用师父的依据修父亲事实","replacement":{"objectJson":{"description":"假死"}},"evidenceRefs":[{"artifactId":"artifact:r05:fact-b","version":1}]}),
    )
    .expect_err("unrelated evidence without an explicit relationship must be blocked");
    assert_eq!(error, "B12_RETCON_EVIDENCE_UNRELATED");
}

#[test]
fn b12_assistant_natural_language_father_retcon_resolves_current_fact() {
    let state = test_state("p-b12-r05-natural");
    let handle = state.manager.open("p-b12-r05-natural", "", NOW).unwrap();
    handle
        .execute(
            "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:r05:father','p-b12-r05-natural','父亲','CHARACTER',?1,?1)",
            [NOW],
        )
        .unwrap();
    handle
        .execute(
            "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES ('fact:r05:father','p-b12-r05-natural','entity:r05:father','状态','{\"description\":\"已经死亡\"}','A',?1,?1)",
            [NOW],
        )
        .unwrap();
    drop(handle);
    let context = query_retcon_context_impl(
        &state,
        json!({"projectId":"p-b12-r05-natural","query":"第一卷父亲已经写死了，我想改成假死。","pageSize":20}),
    )
    .unwrap();
    assert_eq!(context["grounding"]["status"], "UNIQUE");
    assert_eq!(context["grounding"]["candidateCount"], 1);
    assert_eq!(context["facts"][0]["factId"], "fact:r05:father");
    assert!(context["facts"][0]["matchScore"].as_i64().unwrap() > 0);
    println!("B12_ASSISTANT_GROUNDING_RESULT naturalLanguage=PASS exactFact=PASS status=PASS");
}

#[test]
fn b12_assistant_ambiguous_retcon_requires_user_selection() {
    let state = test_state("p-b12-r05-ambiguous");
    let handle = state.manager.open("p-b12-r05-ambiguous", "", NOW).unwrap();
    for (entity_id, name, fact_id) in [
        ("entity:r05:father", "父亲", "fact:r05:father"),
        ("entity:r05:master", "师父", "fact:r05:master"),
    ] {
        handle
            .execute(
                "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES (?1,'p-b12-r05-ambiguous',?2,'CHARACTER',?3,?3)",
                params![entity_id, name, NOW],
            )
            .unwrap();
        handle
            .execute(
                "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES (?1,'p-b12-r05-ambiguous',?2,'状态','{\"description\":\"已经死亡\"}','A',?3,?3)",
                params![fact_id, entity_id, NOW],
            )
            .unwrap();
    }
    drop(handle);
    let context = query_retcon_context_impl(
        &state,
        json!({"projectId":"p-b12-r05-ambiguous","query":"把那个死亡改成假死。","pageSize":20}),
    )
    .unwrap();
    assert_eq!(context["grounding"]["status"], "AMBIGUOUS");
    assert_eq!(context["grounding"]["candidateCount"], 2);
    assert_eq!(context["grounding"]["needsUserSelection"], true);
    assert_eq!(context["selectedFact"], Value::Null);
    println!("B12_ASSISTANT_GROUNDING_AMBIGUOUS_RESULT needsUserSelection=PASS status=PASS");
}

#[test]
fn b12_summary_bootstrap_command_returns_before_aggregation_complete() {
    let state = test_state("p-b12-r06-async");
    let handle = state.manager.open("p-b12-r06-async", "", NOW).unwrap();
    handle.execute_batch("BEGIN IMMEDIATE").unwrap();
    for index in 0..20_000 {
        handle
            .execute(
                "INSERT INTO dependency_edges(edge_id,project_id,source_ref,target_ref,dependency_type,scope,impact_class,source_hash,created_by,created_at) VALUES (?1,?2,'async:source',?3,'PLAN_INPUT','BOOK','HARD',?4,'B12_R06_TEST',?5)",
                params![
                    format!("edge:b12:r06:async:{index}"),
                    "p-b12-r06-async",
                    format!("async:target:{index}"),
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    NOW
                ],
            )
            .unwrap();
    }
    handle.execute_batch("COMMIT").unwrap();
    drop(handle);
    command(
        &state,
        "p-b12-r06-async",
        "change_request_create",
        "cmd:b12:r06:async:create",
        0,
        json!({"changeRequestId":"change:b12:r06:async","changeType":"CHANGE","scope":"GLOBAL","summary":"异步索引测试","requestedChange":{"target":"全局路线"}}),
    )
    .unwrap();
    command(
        &state,
        "p-b12-r06-async",
        "impact_preflight",
        "cmd:b12:r06:async:preflight",
        0,
        json!({"changeRequestId":"change:b12:r06:async"}),
    )
    .unwrap();
    let started = Instant::now();
    let bootstrap = command(
        &state,
        "p-b12-r06-async",
        "dependency_summary_bootstrap",
        "cmd:b12:r06:async:bootstrap",
        0,
        json!({}),
    )
    .unwrap();
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
    assert!(
        elapsed_ms < 500.0,
        "bootstrap command waited {elapsed_ms:.2}ms"
    );
    assert_eq!(bootstrap["result"]["status"], "QUEUED");
    assert_eq!(bootstrap["result"]["workerDispatched"], true);

    let mut worker_completed = false;
    for _ in 0..10_000 {
        let status =
            query_dependency_summary_bootstrap_impl(&state, json!({"projectId":"p-b12-r06-async"}))
                .unwrap();
        if status["summary"]["summaryStatus"] == "READY" && status["job"]["status"] == "COMPLETED" {
            worker_completed = true;
            break;
        }
        thread::yield_now();
    }
    assert!(worker_completed, "summary worker did not complete");
    println!(
        "B12_SUMMARY_BOOTSTRAP_ASYNC_RESULT commandReturnedBeforeWorker=PASS workerCompletesLater=PASS status=PASS"
    );
}

#[test]
fn b12_summary_running_job_not_reclaimed_after_lease_expiry() {
    let project_id = "p-b12-r06-expired-running";
    let state = test_state(project_id);
    let handle = state.manager.open(project_id, "", NOW).expect("open runtime fixture");
    let job_id = DurableJobService::ensure_dependency_summary_job(&handle, project_id, NOW)
        .expect("ensure summary job");
    assert!(DurableJobService::claim_dependency_summary_job(
        &handle,
        project_id,
        &job_id,
        "worker:r06:first",
        NOW,
    )
    .expect("first worker claim"));
    let after_expiry = "2026-09-02T05:01:01Z";
    assert!(
        !DurableJobService::claim_dependency_summary_job(
            &handle,
            project_id,
            &job_id,
            "worker:r06:second",
            after_expiry,
        )
        .expect("a normal bootstrap request must not reclaim live RUNNING work after TTL"),
    );
    let (status, holder, released_at): (String, String, Option<String>) = handle
        .query_row(
            "SELECT j.status,l.holder,l.released_at FROM jobs j JOIN job_leases l ON l.project_id=j.project_id AND l.job_id=j.job_id WHERE j.project_id=?1 AND j.job_id=?2",
            params![project_id, job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read live summary job lease");
    assert_eq!(status, "RUNNING");
    assert_eq!(holder, "worker:r06:first");
    assert_eq!(released_at, None);
    println!("B12_SUMMARY_LEASE_EXPIRY_RESULT duplicateWorkerCount=0 status=PASS");
}

#[test]
fn b12_summary_restart_recovery_allows_reclaim_after_pause() {
    let project_id = "p-b12-r06-recovery-reclaim";
    let state = test_state(project_id);
    let handle = state.manager.open(project_id, "", NOW).expect("open recovery fixture");
    let job_id = DurableJobService::ensure_dependency_summary_job(&handle, project_id, NOW)
        .expect("ensure summary job");
    assert!(DurableJobService::claim_dependency_summary_job(
        &handle,
        project_id,
        &job_id,
        "worker:r06:before-restart",
        NOW,
    )
    .expect("claim pre-restart worker"));
    let recovered_at = "2026-09-02T05:01:01Z";
    DurableJobService::recover_after_restart(&handle, project_id, recovered_at)
        .expect("recover live job after restart");
    assert!(DurableJobService::claim_dependency_summary_job(
        &handle,
        project_id,
        &job_id,
        "worker:r06:after-restart",
        recovered_at,
    )
    .expect("paused job may be reclaimed after explicit restart recovery"));
    let (status, holder): (String, String) = handle
        .query_row(
            "SELECT j.status,l.holder FROM jobs j JOIN job_leases l ON l.project_id=j.project_id AND l.job_id=j.job_id WHERE j.project_id=?1 AND j.job_id=?2",
            params![project_id, job_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("read reclaimed summary job");
    assert_eq!(status, "RUNNING");
    assert_eq!(holder, "worker:r06:after-restart");
}

#[test]
fn b12_summary_old_worker_cannot_release_new_holder_lease() {
    let project_id = "p-b12-r06-old-worker";
    let state = test_state(project_id);
    let handle = state.manager.open(project_id, "", NOW).expect("open old worker fixture");
    let job_id = DurableJobService::ensure_dependency_summary_job(&handle, project_id, NOW)
        .expect("ensure summary job");
    assert!(DurableJobService::claim_dependency_summary_job(
        &handle,
        project_id,
        &job_id,
        "worker:r06:old",
        NOW,
    )
    .expect("claim old worker"));
    let recovered_at = "2026-09-02T05:01:01Z";
    DurableJobService::recover_after_restart(&handle, project_id, recovered_at)
        .expect("recover old worker state");
    assert!(DurableJobService::claim_dependency_summary_job(
        &handle,
        project_id,
        &job_id,
        "worker:r06:new",
        recovered_at,
    )
    .expect("claim resumed worker"));
    DurableJobService::pause_dependency_summary_job(
        &handle,
        project_id,
        &job_id,
        "OLD_WORKER_FAILED",
        recovered_at,
    )
    .expect("simulated old worker completion path");
    let (status, holder, released_at): (String, String, Option<String>) = handle
        .query_row(
            "SELECT j.status,l.holder,l.released_at FROM jobs j JOIN job_leases l ON l.project_id=j.project_id AND l.job_id=j.job_id WHERE j.project_id=?1 AND j.job_id=?2",
            params![project_id, job_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read new worker lease after old worker return");
    assert_eq!(status, "RUNNING");
    assert_eq!(holder, "worker:r06:new");
    assert_eq!(released_at, None);
}

#[test]
fn b12_dependency_summary_epoch_invalidates_ready_summary_and_requeues_job() {
    let state = test_state("p-b12-r06-epoch");
    let handle = state.manager.open("p-b12-r06-epoch", "", NOW).unwrap();
    handle
        .execute(
            "INSERT INTO dependency_edges(edge_id,project_id,source_ref,target_ref,dependency_type,scope,impact_class,source_hash,created_by,created_at) VALUES ('edge:r06:epoch','p-b12-r06-epoch','epoch:source','epoch:target','PLAN_INPUT','BOOK','HARD',?1,'B12_R06_TEST',?2)",
            params!["a".repeat(64), NOW],
        )
        .unwrap();
    rebuild_global_dependency_summary(&handle, "p-b12-r06-epoch").unwrap();
    let ready =
        ai_novel_production_system_core::repositories::change_retcon::dependency_summary_status(
            &handle,
            "p-b12-r06-epoch",
        )
        .unwrap();
    assert_eq!(ready["summaryStatus"], "READY");
    handle
        .execute(
            "INSERT INTO dependency_edges(edge_id,project_id,source_ref,target_ref,dependency_type,scope,impact_class,source_hash,created_by,created_at) VALUES ('edge:r06:epoch:2','p-b12-r06-epoch','epoch:source','epoch:target:2','PLAN_INPUT','BOOK','HARD',?1,'B12_R06_TEST',?2)",
            params!["b".repeat(64), NOW],
        )
        .unwrap();
    let stale =
        ai_novel_production_system_core::repositories::change_retcon::dependency_summary_status(
            &handle,
            "p-b12-r06-epoch",
        )
        .unwrap();
    assert_eq!(stale["summaryStatus"], "NOT_READY");
    drop(handle);
    command(
        &state,
        "p-b12-r06-epoch",
        "change_request_create",
        "cmd:b12:r06:epoch:create",
        0,
        json!({"changeRequestId":"change:b12:r06:epoch","changeType":"CHANGE","scope":"GLOBAL","summary":"重新准备影响索引","requestedChange":{"target":"全局路线"}}),
    )
    .unwrap();
    let preflight = command(
        &state,
        "p-b12-r06-epoch",
        "impact_preflight",
        "cmd:b12:r06:epoch:preflight",
        0,
        json!({"changeRequestId":"change:b12:r06:epoch"}),
    )
    .unwrap();
    assert_eq!(preflight["result"]["status"], "SUMMARY_BOOTSTRAP_REQUIRED");
    assert!(preflight["result"]["bootstrapJobId"].as_str().is_some());
}

#[test]
fn b12_global_preflight_defers_frontier_materialization_until_expansion() {
    let state = test_state("p-b12-r06");
    let handle = state.manager.open("p-b12-r06", "", NOW).unwrap();
    handle.execute_batch("BEGIN IMMEDIATE").unwrap();
    for index in 0..1_001 {
        handle
            .execute(
                "INSERT INTO dependency_edges(edge_id,project_id,source_ref,target_ref,dependency_type,scope,impact_class,source_hash,created_by,created_at) VALUES (?1,?2,'scale:source',?3,'PLAN_INPUT','BOOK','HARD',?4,'B12_R06_TEST',?5)",
                params![
                    format!("edge:b12:r06:{index}"),
                    "p-b12-r06",
                    format!("target:{index}"),
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    NOW
                ],
            )
            .unwrap();
    }
    handle.execute_batch("COMMIT").unwrap();
    rebuild_global_dependency_summary(&handle, "p-b12-r06").unwrap();
    drop(handle);

    command(
        &state,
        "p-b12-r06",
        "change_request_create",
        "cmd:b12:r06:create",
        0,
        json!({"changeRequestId":"change:b12:r06","changeType":"CHANGE","scope":"GLOBAL","summary":"延迟全局影响队列","requestedChange":{"target":"全局路线"}}),
    )
    .unwrap();
    let preflight = command(
        &state,
        "p-b12-r06",
        "impact_preflight",
        "cmd:b12:r06:preflight",
        0,
        json!({"changeRequestId":"change:b12:r06"}),
    )
    .unwrap();
    let impact_set_id = preflight["result"]["impactSetId"].as_str().unwrap();
    state
        .manager
        .with_read_connection("p-b12-r06", NOW, |connection| {
            let frontier_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM impact_frontier WHERE project_id=?1 AND impact_set_id=?2",
                    params!["p-b12-r06", impact_set_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(frontier_count, 0);
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();
}

fn test_state(project_id: &str) -> KernelState {
    let temp = tempfile::tempdir().expect("temporary project root");
    let root = Box::leak(Box::new(temp)).path().to_path_buf();
    let state = KernelState::with_production_gateway(
        AppPaths::new(root),
        Arc::new(UnavailableProductionGateway),
    );
    state
        .manager
        .create(project_id, "B12 修订测试作品", NOW)
        .expect("create project");
    state
}

fn command(
    state: &KernelState,
    project_id: &str,
    command_type: &str,
    command_id: &str,
    expected_revision: i64,
    payload: Value,
) -> Result<Value, String> {
    execute_command_impl(
        state,
        json!({
            "type": command_type,
            "commandId": command_id,
            "idempotencyKey": format!("{command_id}:key"),
            "projectId": project_id,
            "expectedRevision": expected_revision,
            "requestedAt": NOW,
            "payload": payload,
        }),
    )
}

fn wait_for_summary_ready(state: &KernelState, project_id: &str) -> Value {
    for _ in 0..10_000 {
        let status =
            query_dependency_summary_bootstrap_impl(state, json!({"projectId":project_id}))
                .expect("query summary bootstrap status");
        if status["summary"]["summaryStatus"] == "READY" && status["job"]["status"] == "COMPLETED" {
            return status;
        }
        thread::yield_now();
    }
    panic!("summary worker did not reach READY");
}

fn insert_dependency(
    state: &KernelState,
    project_id: &str,
    edge_id: &str,
    source_ref: &str,
    target_ref: &str,
    scope: &str,
) {
    let handle = state
        .manager
        .open(project_id, "", NOW)
        .expect("open project");
    handle
        .execute(
            "INSERT INTO dependency_edges(edge_id,project_id,source_ref,target_ref,dependency_type,scope,impact_class,source_hash,created_by,created_at) VALUES (?1,?2,?3,?4,'PLAN_INPUT',?5,'REVIEW_REQUIRED',?6,'B12_REVISION_TEST',?7)",
            params![
                edge_id,
                project_id,
                source_ref,
                target_ref,
                scope,
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                NOW
            ],
        )
        .expect("insert dependency");
    drop(handle);
}

fn insert_book_plan(state: &KernelState, project_id: &str, artifact_id: &str) {
    let payload = json!({
        "payloadSchema":"book-plan.r1",
        "planningMaturity":"SEED",
        "storyPromise":"原始承诺",
        "protagonistMacroDirection":"主动查明真相",
        "endingBoundary":"完成核心选择",
        "mustNotBreak":[{"id":"rule:one","description":"代价必须可验证"}],
        "openingHypotheses":[],
        "globalLengthBudget":{"preferredWords":100000}
    });
    let hash = sha256_canonical(&payload).expect("book plan hash");
    let scope_key = json!({"level":"PROJECT"}).to_string();
    let handle = state
        .manager
        .open(project_id, "", NOW)
        .expect("open project");
    handle
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,authority_level,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'BookPlan', 'PROJECT',?3,1,'m0a.artifact.v1','COMMITTED','APPROVED_PLAN','A',?4,'{}',?5,?5)",
            params![artifact_id, project_id, scope_key, hash, NOW],
        )
        .expect("insert book plan metadata");
    handle
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            params![artifact_id, payload.to_string(), hash],
        )
        .expect("insert book plan payload");
    handle
        .execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'BookPlan','PROJECT',?2,?3,1,?4)",
            params![project_id, scope_key, artifact_id, hash],
        )
        .expect("insert book plan head");
    drop(handle);
}

fn insert_chapter_plan(state: &KernelState, project_id: &str, artifact_id: &str) {
    let chapter_id = format!("chapter-{project_id}-1");
    let payload = json!({
        "payloadSchema":"chapter-plan.r1",
        "chapterId":chapter_id,
        "compiledFromProjectRevision":0,
        "compiledFromStateDigest":"a".repeat(64),
        "contextManifestRef":{"artifactId":"context:b12:r01","version":1,"payloadHash":"b".repeat(64)},
        "chapterGoal":"推进父亲留下的线索",
        "activeTaskRefs":[],
        "continuityActions":[],
        "requiredStateOutcomes":[],
        "forbiddenTransitions":[],
        "knowledgeBoundaries":[],
        "reservationRefs":[],
        "sceneExecution":[],
        "horizonPreview":[
            {"slotNo":1,"objective":"发现线索","tentative":true},
            {"slotNo":2,"objective":"做出选择","tentative":true},
            {"slotNo":3,"objective":"承担代价","tentative":true}
        ]
    });
    let hash = sha256_canonical(&payload).expect("chapter plan hash");
    let scope_key = json!({"level":"CHAPTER","chapterId":chapter_id}).to_string();
    let handle = state
        .manager
        .open(project_id, "", NOW)
        .expect("open project");
    handle
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,authority_level,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'ChapterPlan','CHAPTER',?3,1,'m0a.artifact.v1','COMMITTED','APPROVED_PLAN','B',?4,'{}',?5,?5)",
            params![artifact_id, project_id, scope_key, hash, NOW],
        )
        .expect("insert chapter plan metadata");
    handle
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            params![artifact_id, payload.to_string(), hash],
        )
        .expect("insert chapter plan payload");
    handle
        .execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'ChapterPlan','CHAPTER',?2,?3,1,?4)",
            params![project_id, scope_key, artifact_id, hash],
        )
        .expect("insert chapter plan head");
    drop(handle);
}

struct SemanticChapterFixture {
    chapter_plan_id: String,
}

fn insert_committed_artifact(
    connection: &Connection,
    project_id: &str,
    artifact_type: &str,
    artifact_id: &str,
    scope_level: &str,
    scope: Value,
    payload: Value,
    make_head: bool,
) -> String {
    let scope_key = scope.to_string();
    let hash = sha256_canonical(&payload).expect("artifact payload hash");
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,authority_level,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'m0a.artifact.v1','COMMITTED','APPROVED_PLAN','B',?6,'{}',?7,?7)",
            params![artifact_id, project_id, artifact_type, scope_level, scope_key, hash, NOW],
        )
        .expect("insert artifact metadata");
    connection
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            params![artifact_id, payload.to_string(), hash],
        )
        .expect("insert artifact payload");
    if make_head {
        connection
            .execute(
                "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,?2,?3,?4,?5,1,?6)",
                params![project_id, artifact_type, scope_level, scope_key, artifact_id, hash],
            )
            .expect("insert authority head");
    }
    hash
}

fn insert_semantic_chapter_fixture(
    state: &KernelState,
    project_id: &str,
) -> SemanticChapterFixture {
    let chapter_plan_id = format!("plan:chapter:{project_id}:1");
    let chapter_id = format!("chapter-{project_id}-1");
    let volume_id = format!("volume-{project_id}-2");
    let book_id = format!("book:{project_id}");
    let handle = state
        .manager
        .open(project_id, "", NOW)
        .expect("open semantic chapter fixture");
    handle
        .execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'B12 语义测试书','ACTIVE',?3,?3)",
            params![book_id, project_id, NOW],
        )
        .expect("insert semantic fixture book");
    handle
        .execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,2,'第二卷','PLANNING',1,3,3000,?4,?4)",
            params![volume_id, project_id, book_id, NOW],
        )
        .expect("insert semantic fixture volume");
    handle
        .execute(
            "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,1,'第一章','NORMAL','PLANNED',0,?4,?4)",
            params![chapter_id, project_id, volume_id, NOW],
        )
        .expect("insert semantic fixture chapter");

    insert_committed_artifact(
        &handle,
        project_id,
        "BookPlan",
        &format!("plan:book:{project_id}"),
        "PROJECT",
        json!({"level":"PROJECT"}),
        json!({
            "payloadSchema":"book-plan.r1",
            "planningMaturity":"APPROVED",
            "futureReservations":[{
                "reservationId":"reservation:b12:future",
                "notBeforeVolumeNo":4,
                "mayForeshadow":true,
                "mayReveal":false
            }]
        }),
        true,
    );
    let context_id = format!("context:{project_id}:1");
    let context_hash = insert_committed_artifact(
        &handle,
        project_id,
        "ContextManifest",
        &context_id,
        "CHAPTER",
        json!({"level":"CHAPTER","chapterId":chapter_id}),
        json!({"payloadSchema":"context-manifest.r1"}),
        false,
    );
    let intent_id = format!("intent:{volume_id}");
    let intent_hash = insert_committed_artifact(
        &handle,
        project_id,
        "VolumeIntent",
        &intent_id,
        "VOLUME",
        json!({"level":"VOLUME","volumeId":volume_id}),
        json!({"payloadSchema":"volume-intent.r1","futureReservations":[]}),
        true,
    );
    let route_id = format!("plan:{volume_id}");
    insert_committed_artifact(
        &handle,
        project_id,
        "VolumePlan",
        &route_id,
        "VOLUME",
        json!({"level":"VOLUME","volumeId":volume_id}),
        json!({
            "payloadSchema":"volume-plan.r1",
            "volumeIntentRef":{"artifactId":intent_id,"version":1,"payloadHash":intent_hash},
            "routeVersion":1,
            "tasks":[{"taskId":"task:b12:route","class":"REQUIRED","dependsOnTaskIds":[],"objective":"推进当前路线"}],
            "routeInvariants":[],
            "requiredExitConditions":[],
            "capacityBudget":{"minChapters":1,"preferredChapters":3,"maxChapters":3}
        }),
        true,
    );
    handle
        .execute(
            "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES ('task:b12:route',?1,'volume.task.progress','{\"taskId\":\"task:b12:route\",\"status\":\"ACTIVE\",\"evidenceRefs\":[],\"remainingConditions\":[]}',?2,'{}',?3,?3)",
            params![project_id, chapter_id, NOW],
        )
        .expect("insert route task state");
    let chapter_plan = json!({
        "payloadSchema":"chapter-plan.r1",
        "chapterId":chapter_id,
        "compiledFromProjectRevision":0,
        "compiledFromStateDigest":"a".repeat(64),
        "contextManifestRef":{"artifactId":context_id,"version":1,"payloadHash":context_hash},
        "chapterGoal":"推进当前路线",
        "activeTaskRefs":["task:b12:route"],
        "continuityActions":[],
        "requiredStateOutcomes":[],
        "forbiddenTransitions":[],
        "knowledgeBoundaries":[],
        "reservationRefs":[],
        "sceneExecution":[],
        "horizonPreview":[
            {"slotNo":1,"objective":"推进路线","tentative":true},
            {"slotNo":2,"objective":"制造阻力","tentative":true},
            {"slotNo":3,"objective":"留下后果","tentative":true}
        ]
    });
    insert_committed_artifact(
        &handle,
        project_id,
        "ChapterPlan",
        &chapter_plan_id,
        "CHAPTER",
        json!({"level":"CHAPTER","chapterId":chapter_id}),
        chapter_plan,
        true,
    );
    drop(handle);
    SemanticChapterFixture {
        chapter_plan_id,
    }
}

fn insert_retcon_evidence_chapter(
    connection: &Connection,
    project_id: &str,
    chapter_no: i64,
) -> String {
    let book_id = format!("book-{project_id}-retcon");
    let volume_id = format!("volume-{project_id}-retcon");
    let chapter_id = format!("chapter-{project_id}-{chapter_no}");
    connection
        .execute(
            "INSERT OR IGNORE INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'Retcon 证据书','ACTIVE',?3,?3)",
            params![book_id, project_id, NOW],
        )
        .expect("insert retcon evidence book");
    connection
        .execute(
            "INSERT OR IGNORE INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'第一卷','PLANNING',1,99,3000,?4,?4)",
            params![volume_id, project_id, book_id, NOW],
        )
        .expect("insert retcon evidence volume");
    connection
        .execute(
            "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,?4,'证据章','NORMAL','COMMITTED',1000,?5,?5)",
            params![chapter_id, project_id, volume_id, chapter_no, NOW],
        )
        .expect("insert retcon evidence chapter");
    chapter_id
}

fn insert_volume_plan(state: &KernelState, project_id: &str, volume_id: &str) {
    let handle = state
        .manager
        .open(project_id, "", NOW)
        .expect("open project");
    handle
        .execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'B12 测试书','ACTIVE',?3,?3)",
            params![format!("book:{project_id}"), project_id, NOW],
        )
        .expect("insert book");
    handle
        .execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'第一卷','PLANNING',1,3,3000,?4,?4)",
            params![volume_id, project_id, format!("book:{project_id}"), NOW],
        )
        .expect("insert volume");
    let book_plan_id = format!("plan:book:{project_id}");
    let book_plan = json!({"payloadSchema":"book-plan.r1","planningMaturity":"APPROVED"});
    let book_plan_hash = sha256_canonical(&book_plan).expect("approved book plan hash");
    let book_scope = json!({"level":"PROJECT"}).to_string();
    handle
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,authority_level,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'BookPlan','PROJECT',?3,1,'m0a.artifact.v1','COMMITTED','APPROVED_PLAN','A',?4,'{}',?5,?5)",
            params![book_plan_id, project_id, book_scope, book_plan_hash, NOW],
        )
        .expect("insert approved book plan metadata");
    handle
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            params![book_plan_id, book_plan.to_string(), book_plan_hash],
        )
        .expect("insert approved book plan payload");
    handle
        .execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'BookPlan','PROJECT',?2,?3,1,?4)",
            params![project_id, book_scope, book_plan_id, book_plan_hash],
        )
        .expect("insert approved book plan head");
    let scope_key = json!({"level":"VOLUME","volumeId":volume_id}).to_string();
    let intent_id = format!("intent:{volume_id}");
    let intent = json!({
        "payloadSchema":"volume-intent.r1",
        "volumeFunction":"推进第一卷主线",
        "entryConditions":[],
        "mustAchieve":[{"id":"goal:b12:volume","description":"完成卷内主任务"}],
        "mustNotBreak":[],
        "requiredExitConditions":[],
        "futureReservationRefs":[],
        "crossVolumeResponsibilityRefs":[],
        "targetChapterRange":{"min":1,"preferred":3,"max":3},
        "targetWordRange":{"preferred":3000}
    });
    let intent_hash = sha256_canonical(&intent).expect("intent hash");
    handle
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,authority_level,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'VolumeIntent','VOLUME',?3,1,'m0a.artifact.v1','COMMITTED','APPROVED_PLAN','B',?4,'{}',?5,?5)",
            params![intent_id, project_id, scope_key, intent_hash, NOW],
        )
        .expect("insert intent metadata");
    handle
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            params![intent_id, intent.to_string(), intent_hash],
        )
        .expect("insert intent payload");
    handle
        .execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'VolumeIntent','VOLUME',?2,?3,1,?4)",
            params![project_id, scope_key, intent_id, intent_hash],
        )
        .expect("insert intent head");
    let intent_ref = json!({"artifactId":intent_id,"version":1,"payloadHash":intent_hash});
    let route = json!({
        "payloadSchema":"volume-plan.r1",
        "volumeIntentRef":intent_ref,
        "routeVersion":1,
        "tasks":[],
        "routeInvariants":["B09_ROUTE_CURRENT"],
        "requiredExitConditions":[],
        "capacityBudget":{"minChapters":1,"preferredChapters":3,"maxChapters":3}
    });
    let route_id = format!("plan:{volume_id}");
    let route_hash = sha256_canonical(&route).expect("route hash");
    handle
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,authority_level,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'VolumePlan','VOLUME',?3,1,'m0a.artifact.v1','COMMITTED','APPROVED_PLAN','B',?4,'{}',?5,?5)",
            params![route_id, project_id, scope_key, route_hash, NOW],
        )
        .expect("insert route metadata");
    handle
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            params![route_id, route.to_string(), route_hash],
        )
        .expect("insert route payload");
    handle
        .execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'VolumePlan','VOLUME',?2,?3,1,?4)",
            params![project_id, scope_key, route_id, route_hash],
        )
        .expect("insert route head");
    drop(handle);
}

fn create_and_approve_change(
    state: &KernelState,
    project_id: &str,
    change_id: &str,
    source_refs: &[&str],
    requested_change: Value,
) -> String {
    command(
        state,
        project_id,
        "change_request_create",
        &format!("cmd:{change_id}:create"),
        0,
        json!({
            "changeRequestId":change_id,
            "changeType":"CHANGE",
            "scope":"PROJECT",
            "summary":"B12 修订测试变更",
            "sourceRefs":source_refs,
            "requestedChange":requested_change
        }),
    )
    .expect("create change");
    let preflight = command(
        state,
        project_id,
        "impact_preflight",
        &format!("cmd:{change_id}:preflight"),
        0,
        json!({"changeRequestId":change_id}),
    )
    .expect("preflight");
    let digest = preflight["result"]["impactDigest"]
        .as_str()
        .expect("impact digest")
        .to_owned();
    for index in 0..8 {
        let page = command(
            state,
            project_id,
            "impact_expand",
            &format!("cmd:{change_id}:expand:{index}"),
            0,
            json!({"changeRequestId":change_id,"pageSize":100,"continueGlobal":true}),
        )
        .expect("expand change impact");
        if page["result"]["hasMore"] == false {
            break;
        }
    }
    command(
        state,
        project_id,
        "change_request_decide",
        &format!("cmd:{change_id}:decision"),
        0,
        json!({
            "changeRequestId":change_id,
            "decision":"APPROVE",
            "impactDigest":digest,
            "scopeAcknowledged":true,
            "riskAcknowledged":true
        }),
    )
    .expect("approve change");
    digest
}

#[test]
fn b12_change_applies_a_real_book_plan_authority_head() {
    let state = test_state("p-b12-r01");
    insert_book_plan(&state, "p-b12-r01", "plan:book:p-b12-r01");
    let digest = create_and_approve_change(
        &state,
        "p-b12-r01",
        "change:b12:r01",
        &["plan:book:p-b12-r01"],
        json!({
            "targetAuthority":{"artifactType":"BookPlan","artifactId":"plan:book:p-b12-r01"},
            "patch":{"storyPromise":"修订后的承诺"}
        }),
    );
    let result = command(
        &state,
        "p-b12-r01",
        "change_request_apply",
        "cmd:change:b12:r01:apply",
        0,
        json!({"changeRequestId":"change:b12:r01","impactDigest":digest}),
    )
    .expect("apply typed change");
    assert_eq!(result["result"]["authorityType"], "BookPlan");
    assert_eq!(result["result"]["authorityOwner"], "B07::PlanningOwner");
    state
        .manager
        .with_read_connection("p-b12-r01", NOW, |connection| {
            let (version, payload, provenance): (i64, String, String) = connection
                .query_row(
                    "SELECT h.version,p.payload_json,a.provenance_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version JOIN artifacts a ON a.artifact_id=h.artifact_id AND a.version=h.version WHERE h.project_id=?1 AND h.artifact_type='BookPlan'",
                    ["p-b12-r01"],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .expect("read book plan head");
            assert_eq!(version, 2);
            assert_eq!(serde_json::from_str::<Value>(&payload).unwrap()["storyPromise"], "修订后的承诺");
            assert!(provenance.contains("B07::PlanningOwner"));
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .expect("verify book plan head");
}

#[test]
fn b12_change_chapter_plan_invokes_b07_semantic_owner() {
    let state = test_state("p-b12-r01-chapter");
    let fixture = insert_semantic_chapter_fixture(&state, "p-b12-r01-chapter");
    let digest = create_and_approve_change(
        &state,
        "p-b12-r01-chapter",
        "change:b12:r01:chapter",
        &[fixture.chapter_plan_id.as_str()],
        json!({
            "targetAuthority":{"artifactType":"ChapterPlan","artifactId":fixture.chapter_plan_id},
            "patch":{"chapterGoal":"修订后的章节目标"}
        }),
    );
    let result = command(
        &state,
        "p-b12-r01-chapter",
        "change_request_apply",
        "cmd:change:b12:r01:chapter:apply",
        0,
        json!({"changeRequestId":"change:b12:r01:chapter","impactDigest":digest}),
    )
    .expect("apply chapter plan change");
    assert_eq!(result["result"]["authorityType"], "ChapterPlan");
    assert_eq!(result["result"]["authorityOwner"], "B07::PlanningOwner");
    assert_eq!(
        result["result"]["semanticOwnerReceipt"]["owner"],
        "B07::PlanningOwner"
    );
    assert_eq!(
        result["result"]["semanticOwnerReceipt"]["invocationCount"],
        1
    );
    assert_eq!(
        result["result"]["semanticOwnerReceipt"]["directB12HeadMutationCount"],
        0
    );
    assert_eq!(
        result["result"]["semanticOwnerReceipt"]["routeTaskRefsChecked"],
        1
    );
    assert_eq!(
        result["result"]["semanticOwnerReceipt"]["triggeredContinuityChecked"],
        0
    );
    assert_eq!(
        result["result"]["semanticOwnerReceipt"]["taskRealityChecked"],
        true
    );
    state
        .manager
        .with_read_connection("p-b12-r01-chapter", NOW, |connection| {
            let (version, payload): (i64, String) = connection
                .query_row(
                    "SELECT h.version,p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan'",
                    ["p-b12-r01-chapter"],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("read chapter plan head");
            assert_eq!(version, 2);
            assert_eq!(serde_json::from_str::<Value>(&payload).unwrap()["chapterGoal"], "修订后的章节目标");
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .expect("verify chapter plan owner mutation");
}

fn assert_chapter_change_rejected_without_head_mutation(
    state: &KernelState,
    project_id: &str,
) {
    state
        .manager
        .with_read_connection(project_id, NOW, |connection| {
            let (version, count): (i64, i64) = connection
                .query_row(
                    "SELECT h.version,(SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='ChapterPlan') FROM canonical_heads h WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan'",
                    [project_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("read unchanged ChapterPlan head");
            assert_eq!(version, 1);
            assert_eq!(count, 1);
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .expect("verify rejected ChapterPlan did not mutate authority");
}

#[test]
fn b12_change_chapter_plan_rejects_task_outside_real_route() {
    let project_id = "p-b12-r01-route";
    let state = test_state(project_id);
    let fixture = insert_semantic_chapter_fixture(&state, project_id);
    let change_id = "change:b12:r01:route";
    let digest = create_and_approve_change(
        &state,
        project_id,
        change_id,
        &[fixture.chapter_plan_id.as_str()],
        json!({
            "targetAuthority":{"artifactType":"ChapterPlan","artifactId":fixture.chapter_plan_id},
            "patch":{"activeTaskRefs":["task:b12:outside"]}
        }),
    );
    let error = command(
        &state,
        project_id,
        "change_request_apply",
        "cmd:b12:r01:route:apply",
        0,
        json!({"changeRequestId":change_id,"impactDigest":digest}),
    )
    .expect_err("task outside the formal B07 route must be rejected");
    assert_eq!(error, "TASK_SCOPE_VIOLATION");
    assert_chapter_change_rejected_without_head_mutation(&state, project_id);
}

#[test]
fn b12_change_chapter_plan_rejects_blocked_real_route_task() {
    let project_id = "p-b12-r01-blocked";
    let state = test_state(project_id);
    let fixture = insert_semantic_chapter_fixture(&state, project_id);
    let handle = state.manager.open(project_id, "", NOW).expect("open route state");
    handle
        .execute(
            "UPDATE state_deltas SET value_json='{\"taskId\":\"task:b12:route\",\"status\":\"BLOCKED\",\"evidenceRefs\":[],\"remainingConditions\":[\"blocked\"]}' WHERE project_id=?1 AND state_id='task:b12:route'",
            [project_id],
        )
        .expect("block route task");
    drop(handle);
    let change_id = "change:b12:r01:blocked";
    let digest = create_and_approve_change(
        &state,
        project_id,
        change_id,
        &[fixture.chapter_plan_id.as_str()],
        json!({
            "targetAuthority":{"artifactType":"ChapterPlan","artifactId":fixture.chapter_plan_id},
            "patch":{"chapterGoal":"不应绕过已阻塞任务"}
        }),
    );
    let error = command(
        &state,
        project_id,
        "change_request_apply",
        "cmd:b12:r01:blocked:apply",
        0,
        json!({"changeRequestId":change_id,"impactDigest":digest}),
    )
    .expect_err("blocked formal route task must be rejected");
    assert_eq!(error, "TASK_DEPENDENCY_UNMET");
    assert_chapter_change_rejected_without_head_mutation(&state, project_id);
}

#[test]
fn b12_change_chapter_plan_rejects_missing_triggered_continuity() {
    let project_id = "p-b12-r01-continuity";
    let state = test_state(project_id);
    let fixture = insert_semantic_chapter_fixture(&state, project_id);
    let handle = state
        .manager
        .open(project_id, "", NOW)
        .expect("open continuity fixture");
    handle
        .execute(
            "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,prerequisites_json,lifecycle,created_at,updated_at) VALUES ('obligation:b12:secret',?1,'VOLUME','PROMISE','A',?2,1,'必须处理的秘密','[]','APPROVED',?3,?3)",
            params![project_id, fixture.chapter_plan_id, NOW],
        )
        .expect("insert triggered continuity obligation");
    drop(handle);
    let change_id = "change:b12:r01:continuity";
    let digest = create_and_approve_change(
        &state,
        project_id,
        change_id,
        &[fixture.chapter_plan_id.as_str()],
        json!({
            "targetAuthority":{"artifactType":"ChapterPlan","artifactId":fixture.chapter_plan_id},
            "patch":{"chapterGoal":"漏掉强制连续性处理"}
        }),
    );
    let error = command(
        &state,
        project_id,
        "change_request_apply",
        "cmd:b12:r01:continuity:apply",
        0,
        json!({"changeRequestId":change_id,"impactDigest":digest}),
    )
    .expect_err("triggered A/B continuity must require a disposition");
    assert_eq!(error, "CONTINUITY_BRIDGE_REQUIRED");
    assert_chapter_change_rejected_without_head_mutation(&state, project_id);
}

#[test]
fn b12_change_chapter_plan_rejects_future_reservation_violation() {
    let project_id = "p-b12-r01-reservation";
    let state = test_state(project_id);
    let fixture = insert_semantic_chapter_fixture(&state, project_id);
    let change_id = "change:b12:r01:reservation";
    let digest = create_and_approve_change(
        &state,
        project_id,
        change_id,
        &[fixture.chapter_plan_id.as_str()],
        json!({
            "targetAuthority":{"artifactType":"ChapterPlan","artifactId":fixture.chapter_plan_id},
            "patch":{
                "reservationRefs":["reservation:b12:future"],
                "sceneExecution":[{"action":"REVEAL","reservationRef":"reservation:b12:future"}]
            }
        }),
    );
    let error = command(
        &state,
        project_id,
        "change_request_apply",
        "cmd:b12:r01:reservation:apply",
        0,
        json!({"changeRequestId":change_id,"impactDigest":digest}),
    )
    .expect_err("future reservation reveal must be rejected before B12 commits");
    assert_eq!(error, "FUTURE_RESERVATION_VIOLATION");
    assert_chapter_change_rejected_without_head_mutation(&state, project_id);
}

#[test]
fn b12_change_volume_plan_invokes_b09_semantic_owner() {
    let state = test_state("p-b12-r01-volume");
    insert_volume_plan(&state, "p-b12-r01-volume", "volume-p-b12-r01-volume-1");
    let digest = create_and_approve_change(
        &state,
        "p-b12-r01-volume",
        "change:b12:r01:volume",
        &["plan:volume-p-b12-r01-volume-1"],
        json!({
            "targetAuthority":{"artifactType":"VolumePlan","artifactId":"plan:volume-p-b12-r01-volume-1"},
            "patch":{"routeInvariants":["B09_ROUTE_REVISED"]}
        }),
    );
    let result = command(
        &state,
        "p-b12-r01-volume",
        "change_request_apply",
        "cmd:change:b12:r01:volume:apply",
        0,
        json!({"changeRequestId":"change:b12:r01:volume","impactDigest":digest}),
    )
    .expect("apply volume plan change");
    assert_eq!(result["result"]["authorityType"], "VolumePlan");
    assert_eq!(
        result["result"]["authorityOwner"],
        "B09::VolumeLifecycleOwner"
    );
    assert_eq!(
        result["result"]["semanticOwnerReceipt"]["owner"],
        "B09::VolumeLifecycleOwner"
    );
    assert_eq!(
        result["result"]["semanticOwnerReceipt"]["invocationCount"],
        1
    );
    assert_eq!(
        result["result"]["semanticOwnerReceipt"]["routeIntentReferenceValidated"],
        true
    );
}

#[test]
fn b12_change_volume_intent_invokes_b09_semantic_owner() {
    let state = test_state("p-b12-r01-intent");
    insert_volume_plan(&state, "p-b12-r01-intent", "volume-p-b12-r01-intent-1");
    let digest = create_and_approve_change(
        &state,
        "p-b12-r01-intent",
        "change:b12:r01:intent",
        &["intent:volume-p-b12-r01-intent-1"],
        json!({
            "targetAuthority":{"artifactType":"VolumeIntent","artifactId":"intent:volume-p-b12-r01-intent-1"},
            "patch":{"volumeFunction":"修订后的卷主线"}
        }),
    );
    let result = command(
        &state,
        "p-b12-r01-intent",
        "change_request_apply",
        "cmd:change:b12:r01:intent:apply",
        0,
        json!({"changeRequestId":"change:b12:r01:intent","impactDigest":digest}),
    )
    .expect("apply volume intent change");
    assert_eq!(result["result"]["authorityType"], "VolumeIntent");
    assert_eq!(
        result["result"]["authorityOwner"],
        "B09::VolumeLifecycleOwner"
    );
    assert_eq!(
        result["result"]["semanticOwnerReceipt"]["owner"],
        "B09::VolumeLifecycleOwner"
    );
    assert_eq!(
        result["result"]["semanticOwnerReceipt"]["topologyValidated"],
        true
    );
}

#[test]
fn b12_change_owner_reject_has_zero_head_mutation() {
    let state = test_state("p-b12-r01-reject");
    insert_chapter_plan(
        &state,
        "p-b12-r01-reject",
        "plan:chapter:p-b12-r01-reject:1",
    );
    let digest = create_and_approve_change(
        &state,
        "p-b12-r01-reject",
        "change:b12:r01:reject",
        &["plan:chapter:p-b12-r01-reject:1"],
        json!({
            "targetAuthority":{"artifactType":"ChapterPlan","artifactId":"plan:chapter:p-b12-r01-reject:1"},
            "patch":{"chapterId":"chapter:wrong"}
        }),
    );
    let error = command(
        &state,
        "p-b12-r01-reject",
        "change_request_apply",
        "cmd:change:b12:r01:reject:apply",
        0,
        json!({"changeRequestId":"change:b12:r01:reject","impactDigest":digest}),
    )
    .unwrap_err();
    assert_eq!(error, "CHAPTER_PLAN_CHAPTER_MISMATCH");
    state
        .manager
        .with_read_connection("p-b12-r01-reject", NOW, |connection| {
            let (version, count): (i64, i64) = connection
                .query_row(
                    "SELECT h.version,(SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='ChapterPlan') FROM canonical_heads h WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan'",
                    ["p-b12-r01-reject"],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("read rejected chapter plan");
            assert_eq!(version, 1);
            assert_eq!(count, 1);
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .expect("verify rejected owner leaves head unchanged");
}

#[test]
fn b12_change_b09_owner_reject_has_zero_head_mutation() {
    let state = test_state("p-b12-r01-reject-volume");
    insert_volume_plan(
        &state,
        "p-b12-r01-reject-volume",
        "volume-p-b12-r01-reject-volume-1",
    );
    let digest = create_and_approve_change(
        &state,
        "p-b12-r01-reject-volume",
        "change:b12:r01:reject-volume",
        &["plan:volume-p-b12-r01-reject-volume-1"],
        json!({
            "targetAuthority":{"artifactType":"VolumePlan","artifactId":"plan:volume-p-b12-r01-reject-volume-1"},
            "patch":{"volumeIntentRef":{"artifactId":"intent:wrong","version":1,"payloadHash":"a".repeat(64)}}
        }),
    );
    let error = command(
        &state,
        "p-b12-r01-reject-volume",
        "change_request_apply",
        "cmd:change:b12:r01:reject-volume:apply",
        0,
        json!({"changeRequestId":"change:b12:r01:reject-volume","impactDigest":digest}),
    )
    .unwrap_err();
    assert_eq!(error, "VOLUME_PLAN_INTENT_REF_MISMATCH");
    state
        .manager
        .with_read_connection("p-b12-r01-reject-volume", NOW, |connection| {
            let (version, count): (i64, i64) = connection
                .query_row(
                    "SELECT h.version,(SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='VolumePlan') FROM canonical_heads h WHERE h.project_id=?1 AND h.artifact_type='VolumePlan'",
                    ["p-b12-r01-reject-volume"],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("read rejected volume plan");
            assert_eq!(version, 1);
            assert_eq!(count, 1);
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .expect("verify rejected volume owner leaves head unchanged");
}

#[test]
fn b12_impact_traverses_multiple_hops_and_prunes_non_propagating_edges() {
    let state = test_state("p-b12-r02");
    insert_dependency(&state, "p-b12-r02", "edge:a-b", "A", "B", "HARD");
    insert_dependency(&state, "p-b12-r02", "edge:b-c", "B", "C", "HARD");
    insert_dependency(&state, "p-b12-r02", "edge:c-d", "C", "D", "HARD");
    insert_dependency(&state, "p-b12-r02", "edge:a-x", "A", "X", "NO_PROPAGATE");
    insert_dependency(&state, "p-b12-r02", "edge:x-y", "X", "Y", "HARD");
    command(
        &state,
        "p-b12-r02",
        "change_request_create",
        "cmd:b12:r02:create",
        0,
        json!({"changeRequestId":"change:b12:r02","changeType":"CHANGE","scope":"PROJECT","summary":"多跳影响","sourceRefs":["A"],"requestedChange":{"target":"路线"}}),
    )
    .expect("create change");
    command(
        &state,
        "p-b12-r02",
        "impact_preflight",
        "cmd:b12:r02:preflight",
        0,
        json!({"changeRequestId":"change:b12:r02"}),
    )
    .expect("preflight");
    let mut targets = Vec::new();
    for index in 0..5 {
        let page = command(
            &state,
            "p-b12-r02",
            "impact_expand",
            &format!("cmd:b12:r02:expand:{index}"),
            0,
            json!({"changeRequestId":"change:b12:r02","pageSize":1}),
        )
        .expect("expand impact");
        targets.extend(
            page["result"]["items"]
                .as_array()
                .expect("impact items")
                .iter()
                .filter_map(|item| item["targetRef"].as_str().map(ToOwned::to_owned)),
        );
        if page["result"]["hasMore"] == false {
            break;
        }
    }
    assert!(targets.contains(&"B".to_string()));
    assert!(targets.contains(&"C".to_string()));
    assert!(targets.contains(&"D".to_string()));
    assert!(!targets.contains(&"Y".to_string()));
}

#[test]
fn b12_impact_checkpoint_remains_bounded_after_many_pages() {
    let state = test_state("p-b12-r02-bounded");
    for index in 0..120 {
        insert_dependency(
            &state,
            "p-b12-r02-bounded",
            &format!("edge:bounded:{index}"),
            "A",
            &format!("target:{index}"),
            "HARD",
        );
    }
    command(
        &state,
        "p-b12-r02-bounded",
        "change_request_create",
        "cmd:b12:r02:bounded:create",
        0,
        json!({"changeRequestId":"change:b12:r02:bounded","changeType":"CHANGE","scope":"PROJECT","summary":"有界检查","sourceRefs":["A"],"requestedChange":{"target":"路线"}}),
    )
    .expect("create change");
    command(
        &state,
        "p-b12-r02-bounded",
        "impact_preflight",
        "cmd:b12:r02:bounded:preflight",
        0,
        json!({"changeRequestId":"change:b12:r02:bounded"}),
    )
    .expect("preflight");
    for index in 0..12 {
        command(
            &state,
            "p-b12-r02-bounded",
            "impact_expand",
            &format!("cmd:b12:r02:bounded:expand:{index}"),
            0,
            json!({"changeRequestId":"change:b12:r02:bounded","pageSize":10,"continueGlobal":true}),
        )
        .expect("expand bounded page");
    }
    state
        .manager
        .with_read_connection("p-b12-r02-bounded", NOW, |connection| {
            let (checkpoint_size, item_count, frontier_count): (i64, i64, i64) = connection
                .query_row(
                    "SELECT length(i.processed_pages_json),(SELECT COUNT(*) FROM impact_items),(SELECT COUNT(*) FROM impact_frontier)
                     FROM impact_sets i WHERE i.project_id=?1",
                    ["p-b12-r02-bounded"],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .expect("read bounded impact storage");
            assert!(checkpoint_size <= 512, "checkpoint grew to {checkpoint_size} bytes");
            assert_eq!(item_count, 120);
            assert!(frontier_count <= 121);
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .expect("verify bounded storage");
}

#[test]
fn b12_stale_queue_uses_actual_transitive_impact_targets() {
    let state = test_state("p-b12-r03");
    insert_dependency(&state, "p-b12-r03", "edge:a-b", "A", "B", "HARD");
    insert_dependency(
        &state,
        "p-b12-r03",
        "edge:b-plan",
        "B",
        "plan:chapter:p-b12-r03",
        "HARD",
    );
    let digest = create_and_approve_change(
        &state,
        "p-b12-r03",
        "change:b12:r03",
        &["A"],
        json!({"target":"路线"}),
    );
    command(
        &state,
        "p-b12-r03",
        "change_request_apply",
        "cmd:b12:r03:apply",
        0,
        json!({"changeRequestId":"change:b12:r03","impactDigest":digest}),
    )
    .expect("apply change");
    state
        .manager
        .with_read_connection("p-b12-r03", NOW, |connection| {
            let found: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM stale_queue WHERE project_id=?1 AND target_ref='plan:chapter:p-b12-r03' AND processed=0",
                    ["p-b12-r03"],
                    |row| row.get(0),
                )
                .expect("read stale queue");
            assert_eq!(found, 1);
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .expect("verify stale queue");
}

#[test]
fn b12_retcon_plan_change_invalidates_an_old_decision() {
    let state = test_state("p-b12-r04");
    let handle = state
        .manager
        .open("p-b12-r04", "", NOW)
        .expect("open project");
    handle
        .execute(
            "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:r04','p-b12-r04','主角','CHARACTER',?1,?1)",
            [NOW],
        )
        .expect("insert entity");
    handle
        .execute(
            "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES ('fact:r04','p-b12-r04','entity:r04','状态','{\"old\":true}','A',?1,?1)",
            [NOW],
        )
        .expect("insert fact");
    let evidence_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    handle
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES ('evidence:r04','p-b12-r04','ReviewReport','PROJECT','p-b12-r04',1,'m0a.artifact.v1','COMMITTED',?1,'{}',?2,?2)",
            params![evidence_hash, NOW],
        )
        .expect("insert evidence");
    drop(handle);
    command(
        &state,
        "p-b12-r04",
        "change_request_create",
        "cmd:r04:create",
        0,
        json!({"changeRequestId":"change:r04","changeType":"RETCON","scope":"PROJECT","summary":"修订事实","targetFactId":"fact:r04","sourceRefs":["fact:r04"],"requestedChange":{"objectJson":{"old":false}}}),
    )
    .expect("create retcon");
    let preflight = command(
        &state,
        "p-b12-r04",
        "impact_preflight",
        "cmd:r04:preflight",
        0,
        json!({"changeRequestId":"change:r04"}),
    )
    .expect("preflight retcon");
    let impact_digest = preflight["result"]["impactDigest"]
        .as_str()
        .unwrap()
        .to_owned();
    let plan = command(
        &state,
        "p-b12-r04",
        "retcon_plan_create",
        "cmd:r04:plan",
        0,
        json!({"changeRequestId":"change:r04","retconPlanId":"plan:r04:one","targetFactId":"fact:r04","reason":"新证据","replacement":{"objectJson":{"old":false}},"evidenceRefs":[{"artifactId":"evidence:r04","version":1}]}),
    )
    .expect("create retcon plan");
    let plan_digest = plan["result"]["planDigest"].as_str().unwrap().to_owned();
    command(
        &state,
        "p-b12-r04",
        "change_request_decide",
        "cmd:r04:decision",
        0,
        json!({"changeRequestId":"change:r04","retconPlanId":"plan:r04:one","planDigest":plan_digest,"decision":"APPROVE","impactDigest":impact_digest,"scopeAcknowledged":true,"riskAcknowledged":true}),
    )
    .expect("approve plan");
    state
        .manager
        .with_read_connection("p-b12-r04", NOW, |connection| {
            let payload: String = connection
                .query_row(
                    "SELECT payload_json FROM decisions WHERE project_id=?1 AND decision_id='decision:change:r04:1'",
                    ["p-b12-r04"],
                    |row| row.get(0),
                )
                .unwrap();
            let payload: Value = serde_json::from_str(&payload).unwrap();
            assert_eq!(payload["planDigest"], plan["result"]["planDigest"]);
            assert_eq!(payload["approvedScopeClass"], plan["result"]["approvedScopeClass"]);
            assert_eq!(payload["approvedRiskClass"], plan["result"]["riskClass"]);
            assert_eq!(payload["requiresBackup"], plan["result"]["requiresBackup"]);
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();
    let handle = state
        .manager
        .open("p-b12-r04", "", NOW)
        .expect("open for mutation");
    handle
        .execute(
            "UPDATE retcon_plans SET fix_type='SUPPLEMENT' WHERE project_id='p-b12-r04' AND retcon_plan_id='plan:r04:one'",
            [],
        )
        .expect("mutate plan payload");
    drop(handle);
    let blocked = command(
        &state,
        "p-b12-r04",
        "retcon_apply",
        "cmd:r04:apply",
        0,
        json!({"changeRequestId":"change:r04","retconPlanId":"plan:r04:one","impactDigest":impact_digest,"planDigest":plan_digest}),
    )
    .expect_err("changed plan must invalidate decision");
    assert_eq!(blocked, "B12_DECISION_STALE");
}

#[test]
fn b12_h03_retcon_execution_uses_the_same_owner_and_postverify() {
    let state = test_state("p-b12-r05");
    let handle = state
        .manager
        .open("p-b12-r05", "", NOW)
        .expect("open project");
    handle
        .execute(
            "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:r05','p-b12-r05','主角','CHARACTER',?1,?1)",
            [NOW],
        )
        .expect("insert entity");
    handle
        .execute(
            "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES ('fact:r05','p-b12-r05','entity:r05','状态','{\"old\":true}','A',?1,?1)",
            [NOW],
        )
        .expect("insert fact");
    handle
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES ('evidence:r05','p-b12-r05','ReviewReport','PROJECT','p-b12-r05',1,'m0a.artifact.v1','COMMITTED',?1,'{}',?2,?2)",
            params!["bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", NOW],
        )
        .expect("insert evidence");
    drop(handle);
    command(
        &state,
        "p-b12-r05",
        "change_request_create",
        "cmd:r05:create",
        0,
        json!({"changeRequestId":"change:r05","changeType":"RETCON","scope":"PROJECT","summary":"助手请求修订事实","targetFactId":"fact:r05","sourceRefs":["fact:r05"],"requestedChange":{"replacement":{"objectJson":{"old":false}}}}),
    )
    .expect("create retcon");
    let preflight = command(
        &state,
        "p-b12-r05",
        "impact_preflight",
        "cmd:r05:preflight",
        0,
        json!({"changeRequestId":"change:r05"}),
    )
    .expect("preflight retcon");
    let impact_digest = preflight["result"]["impactDigest"]
        .as_str()
        .unwrap()
        .to_owned();
    let plan = command(
        &state,
        "p-b12-r05",
        "retcon_plan_create",
        "cmd:r05:plan",
        0,
        json!({"changeRequestId":"change:r05","retconPlanId":"plan:r05","targetFactId":"fact:r05","reason":"助手提供新的核验依据","replacement":{"objectJson":{"old":false}},"evidenceRefs":[{"artifactId":"evidence:r05","version":1}]}),
    )
    .expect("create retcon plan");
    let plan_digest = plan["result"]["planDigest"].as_str().unwrap().to_owned();
    command(
        &state,
        "p-b12-r05",
        "change_request_decide",
        "cmd:r05:decision",
        0,
        json!({"changeRequestId":"change:r05","retconPlanId":"plan:r05","planDigest":plan_digest,"decision":"APPROVE","impactDigest":impact_digest,"scopeAcknowledged":true,"riskAcknowledged":true}),
    )
    .expect("approve retcon");

    let operation = ai_novel_production_system_core::repair::RepairOperation {
        operation: "execute_change_request".into(),
        target: "change:r05".into(),
        params: json!({"changeRequestId":"change:r05","impactDigest":impact_digest,"retconPlanId":"plan:r05","planDigest":plan_digest,"commandId":"h03:r05:apply","idempotencyKey":"h03:r05:apply:key"}),
        postconditions: json!({"kind":"b12_change_postverify","expected":true}),
    };
    let expected_postconditions = vec![operation.postconditions.clone()];
    let operation_digest = operation_digest_with_postconditions(
        std::slice::from_ref(&operation),
        &expected_postconditions,
    )
    .unwrap();
    let repair_plan = RepairPlan {
        repair_plan_id: "repair-plan:r05".into(),
        project_id: "p-b12-r05".into(),
        based_on_project_revision: 0,
        diagnosis: json!({"claim":"已核对事实修订方案"}),
        finding_refs: vec!["change:r05".into()],
        target_authority_owner: "B12::ChangeRetconOwner".into(),
        proposed_operations: vec![operation],
        affected_artifacts: vec!["fact:r05".into()],
        affected_scopes: vec!["当前作品".into()],
        risk_level: "HIGH".into(),
        requires_backup: false,
        expected_postconditions,
        operation_digest: operation_digest.clone(),
        created_at: NOW.into(),
    };
    execute_command_impl(
        &state,
        json!({"type":"repair_plan_create","commandId":"cmd:r05:repair-plan","idempotencyKey":"key:r05:repair-plan","projectId":"p-b12-r05","expectedRevision":0,"requestedAt":NOW,"payload":{"dryRun":true,"repairPlan":repair_plan}}),
    )
    .expect("save h03 plan");
    let authorization = ai_novel_production_system_core::repair::RepairAuthorization {
        authorization_id: "repair-auth:r05".into(),
        project_id: "p-b12-r05".into(),
        repair_plan_id: "repair-plan:r05".into(),
        based_on_project_revision: 0,
        operation_digest,
        approved_by_user: true,
        issued_at: NOW.into(),
        consumed_at: None,
    };
    execute_command_impl(
        &state,
        json!({"type":"repair_authorize","commandId":"cmd:r05:authorize","idempotencyKey":"key:r05:authorize","projectId":"p-b12-r05","expectedRevision":0,"requestedAt":NOW,"payload":{"authorization":authorization}}),
    )
    .expect("authorize h03 plan");
    let executed = execute_command_impl(
        &state,
        json!({"type":"repair_execute","commandId":"cmd:r05:execute","idempotencyKey":"key:r05:execute","projectId":"p-b12-r05","requestedAt":NOW,"payload":{"repairPlanId":"repair-plan:r05","authorizationId":"repair-auth:r05"}}),
    )
    .expect("execute h03 retcon");
    assert_eq!(executed["receipt"]["status"], "REPAIR_VERIFIED");
    assert_eq!(executed["receipt"]["owner"], "B12::ChangeRetconOwner");
    println!("B12_PRODUCT_UX_RETCON_RESULT command_boundary=PASS plan=PASS decision=PASS h03=PASS owner=B12::ChangeRetconOwner postverify=PASS status=PASS");
}

#[test]
fn b12_project_migration_records_additive_v11_and_trigger_ownership() {
    let state = test_state("p-b12-r07");
    state
        .manager
        .with_read_connection("p-b12-r07", NOW, |connection| {
            let version: Option<i64> = connection
                .query_row(
                    "SELECT version FROM migration_history WHERE version=11",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .expect("read migration history");
            assert_eq!(version, Some(11));
            let trigger_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name LIKE 'b12_dependency_summary_%'",
                    [],
                    |row| row.get(0),
                )
                .expect("read b12 trigger count");
            assert_eq!(trigger_count, 3);
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .expect("verify migration");
}
