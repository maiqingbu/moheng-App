use ai_novel_production_system_core::commands::{execute_command_impl, KernelState};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::narrative::{evaluate_plan_stale, PlanStaleStatus};
use ai_novel_production_system_core::production::UnavailableProductionGateway;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repair::{operation_digest_with_postconditions, RepairPlan};
use ai_novel_production_system_core::repositories::change_retcon::rebuild_global_dependency_summary;
use libsql_rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::sync::Arc;

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
    assert_eq!(bootstrap["result"]["summaryStatus"], "READY");
    let second = command(
        &state,
        "p-b12-r06-job",
        "dependency_summary_bootstrap",
        "cmd:b12:r06:job:bootstrap:again",
        0,
        json!({}),
    )
    .unwrap();
    assert_eq!(second["result"]["jobId"], bootstrap["result"]["jobId"]);
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
        json!({
            "changeRequestId":"change:b12:r01",
            "impactDigest":digest,
            "targetAuthority":{"artifactType":"BookPlan","artifactId":"plan:book:p-b12-r01"},
            "requestedChange":{"targetAuthority":{"artifactType":"BookPlan","artifactId":"plan:book:p-b12-r01"},"patch":{"storyPromise":"修订后的承诺"}}
        }),
    )
    .expect("apply typed change");
    assert_eq!(result["result"]["authorityType"], "BookPlan");
    state
        .manager
        .with_read_connection("p-b12-r01", NOW, |connection| {
            let (version, payload): (i64, String) = connection
                .query_row(
                    "SELECT h.version,p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='BookPlan'",
                    ["p-b12-r01"],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("read book plan head");
            assert_eq!(version, 2);
            assert_eq!(serde_json::from_str::<Value>(&payload).unwrap()["storyPromise"], "修订后的承诺");
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .expect("verify book plan head");
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
        json!({"changeRequestId":"change:b12:r03","impactDigest":digest,"requestedChange":{"target":"路线"}}),
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
    let handle = state
        .manager
        .open("p-b12-r04", "", NOW)
        .expect("open for mutation");
    handle
        .execute(
            "UPDATE retcon_plans SET downstream_impact_json=?1 WHERE project_id='p-b12-r04' AND retcon_plan_id='plan:r04:one'",
            [json!({"impactDigest":impact_digest,"changed":true}).to_string()],
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
