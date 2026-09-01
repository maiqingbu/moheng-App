use ai_novel_production_system_core::commands::{execute_command_impl, KernelState};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::production::UnavailableProductionGateway;
use libsql_rusqlite::params;
use serde_json::{json, Value};
use std::sync::Arc;

const NOW: &str = "2026-09-02T04:00:00Z";

fn state(project_id: &str) -> KernelState {
    let temp = tempfile::tempdir().unwrap();
    let root = Box::leak(Box::new(temp)).path().to_path_buf();
    let state = KernelState::with_production_gateway(
        AppPaths::new(root),
        Arc::new(UnavailableProductionGateway),
    );
    state.manager.create(project_id, "B12 测试作品", NOW).unwrap();
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
                .query_row("SELECT revision FROM project_meta WHERE project_id=?1", ["p-b12-proposal"], |row| row.get(0))
                .unwrap();
            let canon_count: i64 = connection
                .query_row("SELECT COUNT(*) FROM canon_facts WHERE project_id=?1", ["p-b12-proposal"], |row| row.get(0))
                .unwrap();
            let status: String = connection
                .query_row("SELECT status FROM change_requests WHERE change_request_id=?1", ["change:b12:proposal"], |row| row.get(0))
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
    let digest = preflight["result"]["impactDigest"].as_str().unwrap();
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
        json!({"changeRequestId":"change:b12:apply","impactDigest":digest,"requestedChange":{"target":"后续策略","operation":"替换"}}),
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
        json!({"changeRequestId":"change:b12:apply","impactDigest":digest,"requestedChange":{"target":"后续策略","operation":"替换"}}),
    )
    .unwrap();
    assert_eq!(replay["receiptId"], applied["receiptId"]);
}

