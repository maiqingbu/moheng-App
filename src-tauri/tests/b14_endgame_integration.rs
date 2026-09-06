use ai_novel_production_system_core::commands::{
    execute_command_impl, query_completion_quality_impl, query_completion_status_impl,
    query_endgame_advice_impl, KernelState,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use libsql_rusqlite::Connection;
use serde_json::json;
use std::path::PathBuf;

const NOW: &str = "2026-09-06T00:00:00Z";

fn test_state() -> (KernelState, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_path_buf();
    std::mem::forget(temp);
    let state = KernelState::new(AppPaths::new(root.clone()));
    drop(state.manager.create("proj-b14", "B14", NOW).unwrap());
    (state, root)
}

fn insert_committed_artifact(
    connection: &Connection,
    project_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    scope_level: &str,
    scope_key: &str,
    payload: &serde_json::Value,
) -> String {
    let hash = sha256_canonical(payload).unwrap();
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,authority_level,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'m0a.artifact.v1','COMMITTED','APPROVED_PLAN','C',?6,'{}',?7,?7)",
            libsql_rusqlite::params![artifact_id, project_id, artifact_type, scope_level, scope_key, hash, NOW],
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

fn seed_ready_project(state: &KernelState) {
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    let project_scope = r#"{"level":"PROJECT"}"#;
    let book_id = "book:b14";
    let volume_id = "volume:b14";
    handle
        .execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'B14 Book','COMPLETED',?3,?3)",
            libsql_rusqlite::params![book_id, "proj-b14", NOW],
        )
        .unwrap();
    handle
        .execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'Final Volume','CLOSED',1,1,1000,?4,?4)",
            libsql_rusqlite::params![volume_id, "proj-b14", book_id, NOW],
        )
        .unwrap();
    let book_plan =
        json!({"payloadSchema":"book-plan.r1","majorMysteryGoals":[],"futureReservations":[]});
    insert_committed_artifact(
        &handle,
        "proj-b14",
        "book-plan:b14",
        "BookPlan",
        "PROJECT",
        project_scope,
        &book_plan,
    );
    let volume_exit =
        json!({"payloadSchema":"volume-exit-state.r1","volumeId":volume_id,"status":"CLOSED"});
    let volume_scope = format!(r#"{{"level":"VOLUME","volumeId":"{volume_id}"}}"#);
    insert_committed_artifact(
        &handle,
        "proj-b14",
        "volume-exit:b14",
        "VolumeExitState",
        "VOLUME",
        &volume_scope,
        &volume_exit,
    );
}

#[test]
fn b14_contract_and_completion_use_atomic_commit_receipts() {
    let (state, _root) = test_state();
    seed_ready_project(&state);

    let initial = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    assert_eq!(initial["status"], "NEEDS_DECISION");
    assert_eq!(initial["projectRevision"], 0);

    let contract = execute_command_impl(
        &state,
        json!({
            "type":"endgame_contract_create",
            "projectId":"proj-b14",
            "commandId":"b14-contract-1",
            "idempotencyKey":"b14-contract-1:key",
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{"requestedEnding":{"summary":"主角完成选择并承担终局代价。","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW}}
        }),
    )
    .unwrap();
    assert_eq!(contract["finalStatus"], "committed");
    assert!(contract["receipt"]["receiptId"]
        .as_str()
        .unwrap()
        .starts_with("receipt:"));

    let replay = execute_command_impl(
        &state,
        json!({
            "type":"endgame_contract_create",
            "projectId":"proj-b14",
            "commandId":"b14-contract-1",
            "idempotencyKey":"b14-contract-1:key",
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{"requestedEnding":{"summary":"主角完成选择并承担终局代价。","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW}}
        }),
    )
    .unwrap();
    assert_eq!(
        replay["receipt"]["receiptId"],
        contract["receipt"]["receiptId"]
    );

    let ready = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    assert_eq!(ready["status"], "READY");
    assert_eq!(ready["projectRevision"], 1);

    let completion = execute_command_impl(
        &state,
        json!({
            "type":"complete_book",
            "projectId":"proj-b14",
            "commandId":"b14-complete-1",
            "idempotencyKey":"b14-complete-1:key",
            "expectedRevision":1,
            "requestedAt":NOW
        }),
    )
    .unwrap();
    assert_eq!(completion["finalStatus"], "committed");
    assert_eq!(completion["receipt"]["appliedDeltaCounts"]["artifacts"], 2);

    let after = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    assert_eq!(after["completed"], true);
    assert_eq!(after["projectRevision"], 2);
}

#[test]
fn b14_completion_rejects_second_command_after_book_exit_state() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    execute_command_impl(
        &state,
        json!({
            "type":"endgame_contract_create","projectId":"proj-b14","commandId":"b14-contract-2","idempotencyKey":"b14-contract-2:key","expectedRevision":0,"requestedAt":NOW,
            "payload":{"requestedEnding":{"summary":"正式结局","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW}}
        }),
    )
    .unwrap();
    execute_command_impl(
        &state,
        json!({
            "type":"complete_book","projectId":"proj-b14","commandId":"b14-complete-2","idempotencyKey":"b14-complete-2:key","expectedRevision":1,"requestedAt":NOW
        }),
    )
    .unwrap();
    let error = execute_command_impl(
        &state,
        json!({
            "type":"complete_book","projectId":"proj-b14","commandId":"b14-complete-3","idempotencyKey":"b14-complete-3:key","expectedRevision":2,"requestedAt":NOW
        }),
    )
    .unwrap_err();
    assert_eq!(error, "B14_ALREADY_COMPLETED");
}

#[test]
fn b14_relevant_change_requires_recheck_and_allows_new_digest_completion() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    execute_command_impl(
        &state,
        json!({
            "type":"endgame_contract_create","projectId":"proj-b14","commandId":"b14-contract-r1-v1","idempotencyKey":"b14-contract-r1-v1:key","expectedRevision":0,"requestedAt":NOW,
            "payload":{"requestedEnding":{"summary":"初版正式结局","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW}}
        }),
    )
    .unwrap();
    execute_command_impl(
        &state,
        json!({
            "type":"complete_book","projectId":"proj-b14","commandId":"b14-complete-r1-v1","idempotencyKey":"b14-complete-r1-v1:key","expectedRevision":1,"requestedAt":NOW
        }),
    )
    .unwrap();

    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    handle.execute(
        "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,lifecycle,created_at,updated_at) VALUES ('obligation:r1:resolved','proj-b14','BOOK','PAYOFF','A','book-plan:b14',1,'已完成的历史回报','RESOLVED',?1,?1)",
        [NOW],
    ).unwrap();
    drop(handle);

    let stale = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    assert_eq!(stale["status"], "NEEDS_RECHECK");
    assert_eq!(stale["completed"], false);

    execute_command_impl(
        &state,
        json!({
            "type":"endgame_contract_create","projectId":"proj-b14","commandId":"b14-contract-r1-v2","idempotencyKey":"b14-contract-r1-v2:key","expectedRevision":2,"requestedAt":NOW,
            "payload":{"requestedEnding":{"summary":"重新确认后的正式结局","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW}}
        }),
    )
    .unwrap();
    let ready = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    assert_eq!(ready["status"], "NEEDS_RECHECK");
    execute_command_impl(
        &state,
        json!({
            "type":"complete_book","projectId":"proj-b14","commandId":"b14-complete-r1-v2","idempotencyKey":"b14-complete-r1-v2:key","expectedRevision":3,"requestedAt":NOW
        }),
    )
    .unwrap();
    let current = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    assert_eq!(current["completed"], true);
}

#[test]
fn b14_unrelated_revision_does_not_make_endgame_contract_stale() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    execute_command_impl(
        &state,
        json!({
            "type":"endgame_contract_create","projectId":"proj-b14","commandId":"b14-contract-revision","idempotencyKey":"b14-contract-revision:key","expectedRevision":0,"requestedAt":NOW,
            "payload":{"requestedEnding":{"summary":"与版本号无关的正式结局","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW}}
        }),
    )
    .unwrap();
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    handle
        .execute(
            "UPDATE project_meta SET revision=revision+1 WHERE project_id='proj-b14'",
            [],
        )
        .unwrap();
    drop(handle);
    let result = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    assert_eq!(result["status"], "READY");
}

#[test]
fn b14_s44_s45_are_explicit_soft_only_actions_without_model_runs() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    let before = state.manager.open("proj-b14", "", NOW).unwrap();
    let before_runs: i64 = before
        .query_row(
            "SELECT COUNT(*) FROM model_runs WHERE project_id='proj-b14'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(before);

    let advice = query_endgame_advice_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let quality = query_completion_quality_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    assert_eq!(advice["softOnly"], true);
    assert_eq!(quality["softOnly"], true);
    assert_eq!(advice["runtimeCheckRequests"], json!([]));
    assert_eq!(quality["runtimeCheckRequests"], json!([]));

    let after = state.manager.open("proj-b14", "", NOW).unwrap();
    let after_runs: i64 = after
        .query_row(
            "SELECT COUNT(*) FROM model_runs WHERE project_id='proj-b14'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(after);
    assert_eq!(before_runs, after_runs);
}
