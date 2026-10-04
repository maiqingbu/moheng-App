use ai_novel_production_system_core::commands::{
    execute_command_impl, query_production_center_impl, KernelState,
};
use ai_novel_production_system_core::production::UnavailableProductionGateway;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repair::{
    operation_digest_with_postconditions, RepairAuthorization, RepairOperation, RepairPlan,
};
use libsql_rusqlite::params;
use serde_json::{json, Value};
use std::sync::{Arc, Barrier};
use std::thread;

const NOW: &str = "2026-09-02T04:00:00Z";

fn state(project_id: &str) -> KernelState {
    let temp = tempfile::tempdir().unwrap();
    let root = Box::leak(Box::new(temp)).path().to_path_buf();
    let state = KernelState::with_production_gateway(
        AppPaths::new(root),
        Arc::new(UnavailableProductionGateway),
    );
    state
        .manager
        .create(project_id, "B12 测试作品", NOW)
        .unwrap();
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

#[test]
fn b12_change_proposal_is_durable_without_mutating_canon_and_canon_change_requires_retcon() {
    let state = state("p-b12-proposal");
    let proposal = command(
        &state,
        "p-b12-proposal",
        "change_request_create",
        "cmd:b12:proposal",
        0,
        json!({
            "changeRequestId":"change:b12:proposal",
            "changeType":"CHANGE",
            "scope":"PROJECT",
            "summary":"调整后续卷的冲突节奏",
            "requestedChange":{"target":"后续卷","operation":"调整节奏"},
            "sourceRefs":[]
        }),
    )
    .unwrap();
    assert_eq!(proposal["finalStatus"], "committed");
    assert_eq!(proposal["result"]["status"], "DRAFT");

    let ordinary_canon = command(
        &state,
        "p-b12-proposal",
        "change_request_create",
        "cmd:b12:canon-guard",
        0,
        json!({
            "changeRequestId":"change:b12:canon-guard",
            "changeType":"CHANGE",
            "scope":"PROJECT",
            "summary":"直接改写已确认事实",
            "targetFactId":"fact:b12:existing",
            "requestedChange":{"object":{"value":"新值"}}
        }),
    )
    .unwrap_err();
    assert_eq!(ordinary_canon, "B12_RETCON_REQUIRED");

    state
        .manager
        .with_read_connection("p-b12-proposal", NOW, |connection| {
            let revision: i64 = connection
                .query_row(
                    "SELECT revision FROM project_meta WHERE project_id=?1",
                    ["p-b12-proposal"],
                    |row| row.get(0),
                )
                .unwrap();
            let canon_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1",
                    ["p-b12-proposal"],
                    |row| row.get(0),
                )
                .unwrap();
            let status: String = connection
                .query_row(
                    "SELECT status FROM change_requests WHERE change_request_id=?1",
                    ["change:b12:proposal"],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(revision, 0);
            assert_eq!(canon_count, 0);
            assert_eq!(status, "DRAFT");
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();
}

#[test]
fn b12_global_impact_requires_explicit_expansion_decision_and_supports_bounded_pages() {
    let state = state("p-b12-impact");
    let handle = state.manager.open("p-b12-impact", "", NOW).unwrap();
    for index in 0..5 {
        handle
            .execute(
                "INSERT INTO dependency_edges(edge_id,project_id,source_ref,target_ref,dependency_type,scope,impact_class,source_hash,created_by,created_at) VALUES (?1,?2,?3,?4,'HARD_CONTRACT','BOOK','HARD',?5,'B12_TEST',?6)",
                params![
                    format!("edge:b12:{index}"),
                    "p-b12-impact",
                    "artifact:b12:source",
                    format!("artifact:b12:target:{index}"),
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    NOW
                ],
            )
            .unwrap();
    }
    drop(handle);

    command(
        &state,
        "p-b12-impact",
        "change_request_create",
        "cmd:b12:impact:create",
        0,
        json!({
            "changeRequestId":"change:b12:impact",
            "changeType":"CHANGE",
            "scope":"GLOBAL",
            "summary":"重新规划跨卷主线",
            "requestedChange":{"target":"全书路线"}
        }),
    )
    .unwrap();
    let preflight = command(
        &state,
        "p-b12-impact",
        "impact_preflight",
        "cmd:b12:impact:preflight",
        0,
        json!({"changeRequestId":"change:b12:impact"}),
    )
    .unwrap();
    assert_eq!(preflight["result"]["scopeClass"], "GLOBAL");
    assert_eq!(preflight["result"]["expandedEdges"], 0);

    let blocked = command(
        &state,
        "p-b12-impact",
        "impact_expand",
        "cmd:b12:impact:blocked",
        0,
        json!({"changeRequestId":"change:b12:impact","pageSize":2}),
    )
    .unwrap_err();
    assert_eq!(blocked, "B12_GLOBAL_EXPANSION_CONFIRMATION_REQUIRED");

    let first_page = command(
        &state,
        "p-b12-impact",
        "impact_expand",
        "cmd:b12:impact:page-1",
        0,
        json!({"changeRequestId":"change:b12:impact","pageSize":2,"continueGlobal":true}),
    )
    .unwrap();
    assert_eq!(first_page["result"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(first_page["result"]["expandedEdges"], 2);
    assert!(first_page["result"]["impactDigest"].as_str().unwrap().len() == 64);
}

#[test]
fn b12_explicit_decision_then_apply_uses_commit_service_and_is_idempotent() {
    let state = state("p-b12-apply");
    command(
        &state,
        "p-b12-apply",
        "change_request_create",
        "cmd:b12:apply:create",
        0,
        json!({
            "changeRequestId":"change:b12:apply",
            "changeType":"CHANGE",
            "scope":"PROJECT",
            "summary":"确认新的后续策略",
            "requestedChange":{"target":"后续策略","operation":"替换"}
        }),
    )
    .unwrap();
    let preflight = command(
        &state,
        "p-b12-apply",
        "impact_preflight",
        "cmd:b12:apply:preflight",
        0,
        json!({"changeRequestId":"change:b12:apply"}),
    )
    .unwrap();
    let digest = preflight["result"]["impactDigest"]
        .as_str()
        .unwrap()
        .to_owned();
    command(
        &state,
        "p-b12-apply",
        "change_request_decide",
        "cmd:b12:apply:decision",
        0,
        json!({"changeRequestId":"change:b12:apply","decision":"APPROVE","impactDigest":digest,"scopeAcknowledged":true,"riskAcknowledged":true}),
    )
    .unwrap();
    let applied = command(
        &state,
        "p-b12-apply",
        "change_request_apply",
        "cmd:b12:apply:commit",
        0,
        json!({"changeRequestId":"change:b12:apply","impactDigest":digest}),
    )
    .unwrap();
    assert_eq!(applied["finalStatus"], "committed");
    assert_eq!(applied["result"]["status"], "APPLIED");
    assert!(applied["receiptId"].as_str().is_some());

    let replay = command(
        &state,
        "p-b12-apply",
        "change_request_apply",
        "cmd:b12:apply:commit",
        1,
        json!({"changeRequestId":"change:b12:apply","impactDigest":digest}),
    )
    .unwrap();
    assert_eq!(replay["receiptId"], applied["receiptId"]);
}

#[test]
fn b12_retcon_creates_versioned_canon_and_preserves_old_fact_history() {
    let state = state("p-b12-retcon");
    let handle = state.manager.open("p-b12-retcon", "", NOW).unwrap();
    handle
        .execute(
            "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:b12:hero','p-b12-retcon','主角','CHARACTER',?1,?1)",
            [NOW],
        )
        .unwrap();
    handle
        .execute(
            "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,source_artifact_id,source_version,authority_level,created_at,updated_at) VALUES ('fact:b12:father','p-b12-retcon','entity:b12:hero','父亲状态','{\"alive\":false}','evidence:b12:father',1,'A',?1,?1)",
            [NOW],
        )
        .unwrap();
    handle
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES ('evidence:b12:father','p-b12-retcon','ReviewReport','PROJECT','p-b12-retcon',1,'m0a.artifact.v1','COMMITTED',?1,'{}',?2,?2)",
            params![
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                NOW
            ],
        )
        .unwrap();
    drop(handle);

    command(
        &state,
        "p-b12-retcon",
        "change_request_create",
        "cmd:b12:retcon:create",
        0,
        json!({
            "changeRequestId":"change:b12:retcon",
            "changeType":"RETCON",
            "scope":"PROJECT",
            "summary":"修订主角父亲的历史事实",
            "targetFactId":"fact:b12:father",
            "targetRefs":["fact:b12:father"],
            "sourceRefs":["fact:b12:father"],
            "requestedChange":{"objectJson":{"alive":true}}
        }),
    )
    .unwrap();
    let preflight = command(
        &state,
        "p-b12-retcon",
        "impact_preflight",
        "cmd:b12:retcon:preflight",
        0,
        json!({"changeRequestId":"change:b12:retcon"}),
    )
    .unwrap();
    let digest = preflight["result"]["impactDigest"]
        .as_str()
        .unwrap()
        .to_owned();
    command(
        &state,
        "p-b12-retcon",
        "retcon_plan_create",
        "cmd:b12:retcon:plan",
        0,
        json!({
            "changeRequestId":"change:b12:retcon",
            "retconPlanId":"retcon-plan:b12:father",
            "targetFactId":"fact:b12:father",
            "reason":"用户提供新的历史证据",
            "fixType":"REWRITE",
            "replacement":{"objectJson":{"alive":true}},
            "evidenceRefs":[{"artifactId":"evidence:b12:father","version":1}]
        }),
    )
    .unwrap();
    command(
        &state,
        "p-b12-retcon",
        "change_request_decide",
        "cmd:b12:retcon:decision",
        0,
        json!({"changeRequestId":"change:b12:retcon","retconPlanId":"retcon-plan:b12:father","decision":"APPROVE","impactDigest":digest,"scopeAcknowledged":true,"riskAcknowledged":true}),
    )
    .unwrap();
    let applied = command(
        &state,
        "p-b12-retcon",
        "retcon_apply",
        "cmd:b12:retcon:apply",
        0,
        json!({"changeRequestId":"change:b12:retcon","retconPlanId":"retcon-plan:b12:father","impactDigest":digest}),
    )
    .unwrap();
    assert_eq!(applied["finalStatus"], "committed");
    assert_eq!(applied["postverify"]["status"], "VERIFIED");
    let replayed = command(
        &state,
        "p-b12-retcon",
        "retcon_apply",
        "cmd:b12:retcon:apply",
        0,
        json!({"changeRequestId":"change:b12:retcon","retconPlanId":"retcon-plan:b12:father","impactDigest":digest}),
    )
    .expect("same logical Retcon retry must replay its durable receipt");
    assert_eq!(replayed["finalStatus"], "committed");
    assert_eq!(replayed["result"]["status"], "REPLAYED");
    assert_eq!(replayed["receiptId"], applied["receiptId"]);

    state
        .manager
        .with_read_connection("p-b12-retcon", NOW, |connection| {
            let old_superseded: String = connection
                .query_row("SELECT superseded_by FROM canon_facts WHERE project_id=?1 AND fact_id='fact:b12:father'", ["p-b12-retcon"], |row| row.get(0))
                .unwrap();
            let new_count: i64 = connection
                .query_row("SELECT COUNT(*) FROM canon_facts WHERE project_id=?1 AND fact_id='fact:b12:father:retcon:retcon-plan:b12:father' AND superseded_by IS NULL", ["p-b12-retcon"], |row| row.get(0))
                .unwrap();
            assert_eq!(old_superseded, "fact:b12:father:retcon:retcon-plan:b12:father");
            assert_eq!(new_count, 1);
            let receipt_count: i64 = connection
                .query_row("SELECT COUNT(*) FROM commit_receipts WHERE project_id='p-b12-retcon' AND idempotency_key='cmd:b12:retcon:apply:key'", [], |row| row.get(0))
                .unwrap();
            let revision: i64 = connection
                .query_row("SELECT revision FROM project_meta WHERE project_id='p-b12-retcon'", [], |row| row.get(0))
                .unwrap();
            let stale_delta_count: i64 = connection
                .query_row("SELECT COUNT(*) FROM state_deltas WHERE project_id='p-b12-retcon' AND state_id='stale:change:b12:retcon'", [], |row| row.get(0))
                .unwrap();
            let postverify_status: String = connection
                .query_row("SELECT json_extract(impact_seed_json,'$.postverifyStatus') FROM change_requests WHERE project_id='p-b12-retcon' AND change_request_id='change:b12:retcon'", [], |row| row.get(0))
                .unwrap();
            assert_eq!(receipt_count, 1);
            assert_eq!(revision, 1);
            assert_eq!(stale_delta_count, 1);
            assert_eq!(postverify_status, "VERIFIED");
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();
    println!("B12_RETCON_REPLAY_RUNTIME_RESULT receipt=PASS revision=PASS canon=PASS stale=PASS status=PASS");
}

#[test]
fn b12_impact_cancel_and_resume_continue_from_checkpoint_without_duplicate_pages() {
    let state = state("p-b12-resume");
    let handle = state.manager.open("p-b12-resume", "", NOW).unwrap();
    for index in 0..3 {
        handle
            .execute(
                "INSERT INTO dependency_edges(edge_id,project_id,source_ref,target_ref,dependency_type,scope,impact_class,source_hash,created_by,created_at) VALUES (?1,?2,'artifact:b12:source',?3,'PLAN_INPUT','CHAPTER','REVIEW_REQUIRED',?4,'B12_TEST',?5)",
                params![
                    format!("edge:b12:resume:{index}"),
                    "p-b12-resume",
                    format!("artifact:b12:target:{index}"),
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    NOW
                ],
            )
            .unwrap();
    }
    drop(handle);
    command(
        &state,
        "p-b12-resume",
        "change_request_create",
        "cmd:b12:resume:create",
        0,
        json!({"changeRequestId":"change:b12:resume","changeType":"CHANGE","scope":"PROJECT","summary":"重新安排章节顺序","sourceRefs":["artifact:b12:source"],"requestedChange":{"target":"章节顺序"}}),
    )
    .unwrap();
    command(
        &state,
        "p-b12-resume",
        "impact_preflight",
        "cmd:b12:resume:preflight",
        0,
        json!({"changeRequestId":"change:b12:resume"}),
    )
    .unwrap();
    let first = command(
        &state,
        "p-b12-resume",
        "impact_expand",
        "cmd:b12:resume:page-1",
        0,
        json!({"changeRequestId":"change:b12:resume","pageSize":1}),
    )
    .unwrap();
    assert_eq!(first["result"]["expandedEdges"], 1);
    command(
        &state,
        "p-b12-resume",
        "impact_cancel",
        "cmd:b12:resume:cancel",
        0,
        json!({"changeRequestId":"change:b12:resume"}),
    )
    .unwrap();
    let resumed = command(
        &state,
        "p-b12-resume",
        "impact_resume",
        "cmd:b12:resume:page-2",
        0,
        json!({"changeRequestId":"change:b12:resume","pageSize":1}),
    )
    .unwrap();
    assert_eq!(resumed["result"]["expandedEdges"], 2);
    assert_ne!(
        first["result"]["items"][0]["edgeId"],
        resumed["result"]["items"][0]["edgeId"]
    );
}

#[test]
fn b12_h03_execute_change_request_uses_one_time_p3_and_the_same_typed_owner() {
    let state = state("p-b12-h03");
    command(
        &state,
        "p-b12-h03",
        "change_request_create",
        "cmd:b12:h03:create",
        0,
        json!({"changeRequestId":"change:b12:h03","changeType":"CHANGE","scope":"PROJECT","summary":"助手建议调整后续策略","requestedChange":{"source":"assistant"}}),
    )
    .unwrap();
    let preflight = command(
        &state,
        "p-b12-h03",
        "impact_preflight",
        "cmd:b12:h03:preflight",
        0,
        json!({"changeRequestId":"change:b12:h03"}),
    )
    .unwrap();
    let impact_digest = preflight["result"]["impactDigest"]
        .as_str()
        .unwrap()
        .to_owned();
    command(
        &state,
        "p-b12-h03",
        "change_request_decide",
        "cmd:b12:h03:decision",
        0,
        json!({"changeRequestId":"change:b12:h03","decision":"APPROVE","impactDigest":impact_digest,"scopeAcknowledged":true,"riskAcknowledged":true}),
    )
    .unwrap();

    let operation = RepairOperation {
        operation: "execute_change_request".into(),
        target: "change:b12:h03".into(),
        params: json!({"changeRequestId":"change:b12:h03","impactDigest":impact_digest,"commandId":"h03:b12:apply","idempotencyKey":"h03:b12:apply:key"}),
        postconditions: json!({"kind":"b12_change_postverify","expected":true}),
    };
    let expected_postconditions = vec![operation.postconditions.clone()];
    let operation_digest = operation_digest_with_postconditions(
        std::slice::from_ref(&operation),
        &expected_postconditions,
    )
    .unwrap();
    let plan = RepairPlan {
        repair_plan_id: "repair-plan:b12:h03".into(),
        project_id: "p-b12-h03".into(),
        based_on_project_revision: 0,
        diagnosis: json!({"claim":"已核对变更提案"}),
        finding_refs: vec!["change:b12:h03".into()],
        target_authority_owner: "B12::ChangeRetconOwner".into(),
        proposed_operations: vec![operation],
        affected_artifacts: vec!["change:b12:h03".into()],
        affected_scopes: vec!["当前作品".into()],
        risk_level: "MEDIUM".into(),
        requires_backup: false,
        expected_postconditions,
        operation_digest: operation_digest.clone(),
        created_at: NOW.into(),
    };
    execute_command_impl(&state, json!({
        "type":"repair_plan_create","commandId":"cmd:b12:h03:plan","idempotencyKey":"key:b12:h03:plan","projectId":"p-b12-h03","expectedRevision":0,"requestedAt":NOW,
        "payload":{"dryRun":true,"repairPlan":plan}
    })).unwrap();
    let authorization = RepairAuthorization {
        authorization_id: "repair-auth:b12:h03".into(),
        project_id: "p-b12-h03".into(),
        repair_plan_id: "repair-plan:b12:h03".into(),
        based_on_project_revision: 0,
        operation_digest,
        approved_by_user: true,
        issued_at: NOW.into(),
        consumed_at: None,
    };
    execute_command_impl(&state, json!({
        "type":"repair_authorize","commandId":"cmd:b12:h03:auth","idempotencyKey":"key:b12:h03:auth","projectId":"p-b12-h03","expectedRevision":0,"requestedAt":NOW,
        "payload":{"authorization":authorization}
    })).unwrap();
    let fault = state.manager.open("p-b12-h03", "", NOW).unwrap();
    fault
        .execute_batch(
            "CREATE TRIGGER force_b12_h03_incomplete_postverify
         AFTER UPDATE OF impact_seed_json ON change_requests
         WHEN NEW.project_id='p-b12-h03'
           AND NEW.status='APPLIED'
           AND json_extract(NEW.impact_seed_json,'$.postverifyStatus')='VERIFIED'
         BEGIN
           UPDATE change_requests
           SET impact_seed_json=json_set(NEW.impact_seed_json,'$.postverifyStatus','INCOMPLETE')
           WHERE project_id=NEW.project_id AND change_request_id=NEW.change_request_id;
         END;",
        )
        .unwrap();
    drop(fault);
    let executed = execute_command_impl(&state, json!({
        "type":"repair_execute","commandId":"cmd:b12:h03:execute","idempotencyKey":"key:b12:h03:execute","projectId":"p-b12-h03","requestedAt":NOW,
        "payload":{"repairPlanId":"repair-plan:b12:h03","authorizationId":"repair-auth:b12:h03"}
    })).unwrap();
    let first_h03_status = executed["receipt"]["status"].as_str().unwrap().to_owned();
    let red_probe = state.manager.open("p-b12-h03", "", NOW).unwrap();
    let red_persisted_postverify: String = red_probe.query_row(
        "SELECT json_extract(impact_seed_json,'$.postverifyStatus') FROM change_requests WHERE project_id='p-b12-h03' AND change_request_id='change:b12:h03'",
        [],
        |row| row.get(0),
    ).unwrap();
    drop(red_probe);
    println!("B_R2_02_NORMAL_INITIAL_RESULT h03_status={first_h03_status} final_status={} persisted_b12_status={red_persisted_postverify}", executed["finalStatus"]);
    assert_eq!(executed["finalStatus"], "blocked");
    assert!(executed["message"]
        .as_str()
        .unwrap()
        .contains("不会再次执行变更"));
    let incomplete_postcondition = &executed["receipt"]["postconditions"][0];
    assert_eq!(
        incomplete_postcondition["status"],
        "REPAIR_VERIFICATION_INCOMPLETE"
    );
    assert_eq!(incomplete_postcondition["projectId"], "p-b12-h03");
    assert_eq!(
        incomplete_postcondition["changeRequestId"],
        "change:b12:h03"
    );
    assert_eq!(
        incomplete_postcondition["ownerReceiptRef"],
        executed["receipt"]["ownerReceiptRef"]
    );
    assert_eq!(incomplete_postcondition["beforeRevision"], 0);
    assert_eq!(incomplete_postcondition["afterRevision"], 1);
    assert_eq!(incomplete_postcondition["postverifyStatus"], "INCOMPLETE");
    let faulted = state.manager.open("p-b12-h03", "", NOW).unwrap();
    let persisted_postverify: String = faulted.query_row(
        "SELECT json_extract(impact_seed_json,'$.postverifyStatus') FROM change_requests WHERE project_id='p-b12-h03' AND change_request_id='change:b12:h03'",
        [],
        |row| row.get(0),
    ).unwrap();
    let pending_projection =
        query_production_center_impl(&state, json!({"projectId":"p-b12-h03"})).unwrap();
    assert_eq!(
        pending_projection["repair"]["status"],
        "REPAIR_VERIFICATION_INCOMPLETE"
    );
    assert_eq!(
        pending_projection["repair"]["executionReceipt"]["status"],
        "REPAIR_VERIFICATION_INCOMPLETE"
    );
    assert_eq!(
        pending_projection["repair"]["executionReceipt"]["ownerReceiptRef"],
        executed["receipt"]["ownerReceiptRef"]
    );
    faulted
        .execute_batch("DROP TRIGGER force_b12_h03_incomplete_postverify;")
        .unwrap();
    drop(faulted);
    let recovered = execute_command_impl(
        &state,
        json!({
            "type":"repair_execute","commandId":"cmd:b12:h03:reverify","idempotencyKey":"key:b12:h03:reverify","projectId":"p-b12-h03","requestedAt":NOW,
            "payload":{"repairPlanId":"repair-plan:b12:h03","authorizationId":"repair-auth:b12:h03"}
        }),
    );
    let recovered_status = recovered
        .as_ref()
        .ok()
        .and_then(|value| value.pointer("/receipt/status"))
        .and_then(Value::as_str)
        .unwrap_or("REVERIFY_REJECTED");
    println!("B_R2_02_NORMAL_POSTVERIFY_RESULT first_h03_status={first_h03_status} persisted_b12_status={persisted_postverify} recovered_h03_status={recovered_status}");
    assert_eq!(persisted_postverify, "INCOMPLETE");
    assert_eq!(first_h03_status, "REPAIR_VERIFICATION_INCOMPLETE");
    assert_eq!(recovered_status, "REPAIR_VERIFIED");
    let recovered = recovered.unwrap();
    assert_eq!(recovered["finalStatus"], "committed");
    assert_eq!(
        recovered["receipt"]["receiptRef"],
        executed["receipt"]["receiptRef"]
    );
    assert_eq!(recovered["receipt"]["postconditions"][0]["status"], "PASS");
    assert_eq!(
        recovered["receipt"]["postconditions"][0]["postverifyStatus"],
        "VERIFIED"
    );
    let recovered_projection =
        query_production_center_impl(&state, json!({"projectId":"p-b12-h03"})).unwrap();
    assert_eq!(recovered_projection["repair"]["status"], "REPAIR_VERIFIED");
    assert_eq!(
        recovered_projection["repair"]["executionReceipt"]["status"],
        "REPAIR_VERIFIED"
    );
    assert_eq!(
        recovered_projection["repair"]["executionReceipt"]["ownerReceiptRef"],
        executed["receipt"]["ownerReceiptRef"]
    );
    let recovered_handle = state.manager.open("p-b12-h03", "", NOW).unwrap();
    let h03_receipt_count: i64 = recovered_handle
        .query_row(
            "SELECT COUNT(*) FROM repair_execution_receipts WHERE project_id='p-b12-h03' AND authorization_id='repair-auth:b12:h03'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let b12_receipt_count: i64 = recovered_handle
        .query_row(
            "SELECT COUNT(*) FROM commit_receipts WHERE project_id='p-b12-h03' AND idempotency_key='h03:b12:apply:key'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let consumed_authorization_count: i64 = recovered_handle
        .query_row(
            "SELECT COUNT(*) FROM repair_authorizations WHERE project_id='p-b12-h03' AND authorization_id='repair-auth:b12:h03' AND consumed_at IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let recovered_revision: i64 = recovered_handle
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id='p-b12-h03'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let stale_activation_count: i64 = recovered_handle
        .query_row(
            "SELECT COUNT(*) FROM state_deltas WHERE project_id='p-b12-h03' AND state_id='stale:change:b12:h03'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    println!("B_R2_02_NORMAL_RECOVERY_BINDING_RESULT h03_receipts={h03_receipt_count} b12_receipts={b12_receipt_count} consumed_authorizations={consumed_authorization_count} revision={recovered_revision} stale_activations={stale_activation_count}");
    assert_eq!(h03_receipt_count, 1);
    assert_eq!(b12_receipt_count, 1);
    assert_eq!(consumed_authorization_count, 1);
    assert_eq!(recovered_revision, 1);
    assert_eq!(stale_activation_count, 1);
    drop(recovered_handle);
    assert_eq!(executed["receipt"]["owner"], "B12::ChangeRetconOwner");
    println!("B12_H03_CHANGE_RECOVERY_RESULT postverify=VERIFIED h03_receipts=1 b12_receipts=1 revision=1 stale_activations=1 noDuplicate=PASS status=PASS");
}

#[test]
fn b12_impact_source_digest_change_marks_the_saved_impact_stale_before_expansion() {
    let state = state("p-b12-stale");
    let handle = state.manager.open("p-b12-stale", "", NOW).unwrap();
    handle
        .execute(
            "INSERT INTO dependency_edges(edge_id,project_id,source_ref,target_ref,dependency_type,scope,impact_class,source_hash,created_by,created_at) VALUES ('edge:b12:stale','p-b12-stale','artifact:b12:source','artifact:b12:target','PLAN_INPUT','CHAPTER','REVIEW_REQUIRED',?1,'B12_TEST',?2)",
            params![
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                NOW
            ],
        )
        .unwrap();
    drop(handle);
    command(
        &state,
        "p-b12-stale",
        "change_request_create",
        "cmd:b12:stale:create",
        0,
        json!({"changeRequestId":"change:b12:stale","changeType":"CHANGE","scope":"PROJECT","summary":"检查来源变更","sourceRefs":["artifact:b12:source"],"requestedChange":{"target":"章节"}}),
    )
    .unwrap();
    command(
        &state,
        "p-b12-stale",
        "impact_preflight",
        "cmd:b12:stale:preflight",
        0,
        json!({"changeRequestId":"change:b12:stale"}),
    )
    .unwrap();
    let handle = state.manager.open("p-b12-stale", "", NOW).unwrap();
    handle
        .execute(
            "UPDATE dependency_edges SET source_hash=?1 WHERE project_id='p-b12-stale' AND edge_id='edge:b12:stale'",
            ["cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"],
        )
        .unwrap();
    drop(handle);
    let error = command(
        &state,
        "p-b12-stale",
        "impact_expand",
        "cmd:b12:stale:expand",
        0,
        json!({"changeRequestId":"change:b12:stale","pageSize":1}),
    )
    .unwrap_err();
    assert_eq!(error, "B12_IMPACT_STALE");
}

#[test]
fn b12_two_workers_competing_for_the_same_approved_change_have_one_canonical_winner() {
    let temp = tempfile::tempdir().unwrap();
    let root = Box::leak(Box::new(temp)).path().to_path_buf();
    let first_state = KernelState::with_production_gateway(
        AppPaths::new(root.clone()),
        Arc::new(UnavailableProductionGateway),
    );
    first_state
        .manager
        .create("p-b12-concurrent", "B12 并发测试", NOW)
        .unwrap();
    command(
        &first_state,
        "p-b12-concurrent",
        "change_request_create",
        "cmd:b12:concurrent:create",
        0,
        json!({"changeRequestId":"change:b12:concurrent","changeType":"CHANGE","scope":"PROJECT","summary":"并发执行同一个变更","requestedChange":{"target":"同一方案"}}),
    )
    .unwrap();
    let preflight = command(
        &first_state,
        "p-b12-concurrent",
        "impact_preflight",
        "cmd:b12:concurrent:preflight",
        0,
        json!({"changeRequestId":"change:b12:concurrent"}),
    )
    .unwrap();
    let digest = preflight["result"]["impactDigest"]
        .as_str()
        .unwrap()
        .to_owned();
    command(
        &first_state,
        "p-b12-concurrent",
        "change_request_decide",
        "cmd:b12:concurrent:decision",
        0,
        json!({"changeRequestId":"change:b12:concurrent","decision":"APPROVE","impactDigest":digest,"scopeAcknowledged":true,"riskAcknowledged":true}),
    )
    .unwrap();

    let second_state = KernelState::with_production_gateway(
        AppPaths::new(root),
        Arc::new(UnavailableProductionGateway),
    );
    let verify_state = second_state.clone();
    let barrier = Arc::new(Barrier::new(2));
    let first_barrier = barrier.clone();
    let second_barrier = barrier;
    let first_digest = digest.clone();
    let second_digest = digest;
    let first = thread::spawn(move || {
        first_barrier.wait();
        command(
            &first_state,
            "p-b12-concurrent",
            "change_request_apply",
            "cmd:b12:concurrent:worker-a",
            0,
            json!({"changeRequestId":"change:b12:concurrent","impactDigest":first_digest}),
        )
    });
    let second = thread::spawn(move || {
        second_barrier.wait();
        command(
            &second_state,
            "p-b12-concurrent",
            "change_request_apply",
            "cmd:b12:concurrent:worker-b",
            0,
            json!({"changeRequestId":"change:b12:concurrent","impactDigest":second_digest}),
        )
    });
    let outcomes = [first.join().unwrap(), second.join().unwrap()];
    let successful = outcomes
        .iter()
        .filter(|result| {
            result
                .as_ref()
                .is_ok_and(|value| value["finalStatus"] == "committed")
        })
        .count();
    assert_eq!(successful, 1);
    let verified: i64 = verify_state
        .manager
        .with_read_connection("p-b12-concurrent", NOW, |connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM state_deltas WHERE project_id=?1 AND state_id='change-state:change:b12:concurrent'",
                    ["p-b12-concurrent"],
                    |row| row.get(0),
                )
                .map_err(ai_novel_production_system_core::project_db::ManagerError::Sqlite)
        })
        .unwrap();
    assert_eq!(verified, 1);
}
