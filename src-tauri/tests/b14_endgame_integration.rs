use ai_novel_production_system_core::commands::{
    execute_command_impl, query_completion_quality_impl, query_completion_status_impl,
    query_endgame_advice_impl, KernelState,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
use libsql_rusqlite::Connection;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

const NOW: &str = "2026-09-06T00:00:00Z";

fn test_state() -> (KernelState, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_path_buf();
    std::mem::forget(temp);
    let state = KernelState::new(AppPaths::new(root.clone()));
    drop(state.manager.create("proj-b14", "B14", NOW).unwrap());
    (state, root)
}

struct EndgameSkillGateway;

impl ProductionModelGateway for EndgameSkillGateway {
    fn provider(&self) -> &str {
        "b14-test-provider"
    }
    fn model(&self, _capability: &str) -> String {
        "b14-test-model".into()
    }
    fn invoke(
        &self,
        capability: &str,
        _input: &serde_json::Value,
    ) -> Result<ProductionModelResponse, String> {
        Ok(ProductionModelResponse::deterministic(json!({
            "kind": if capability == "B14_S45_COMPLETION_QUALITY" { "QualityFinding" } else { "CreativeProposal" },
            "status":"OK",
            "summary":"测试 Skill Runtime 的软性建议。",
            "suggestions":["仅供作者参考，不改变正式完结结论。"],
            "runtimeCheckRequests":[]
        })))
    }
}

fn test_state_with_skill_gateway() -> (KernelState, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_path_buf();
    std::mem::forget(temp);
    let state = KernelState::with_production_gateway(
        AppPaths::new(root.clone()),
        Arc::new(EndgameSkillGateway),
    );
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

fn seed_ready_project_with_plan(state: &KernelState, book_plan: serde_json::Value) {
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

fn seed_ready_project(state: &KernelState) {
    seed_ready_project_with_plan(
        state,
        json!({"payloadSchema":"book-plan.r1","majorMysteryGoals":[],"futureReservations":[]}),
    );
}

fn insert_obligation(
    connection: &Connection,
    obligation_id: &str,
    authority_level: &str,
    lifecycle: &str,
) {
    connection
        .execute(
            "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,prerequisites_json,lifecycle,created_at,updated_at) VALUES (?1,'proj-b14','BOOK','PROMISE',?2,'book-plan:b14',1,'终局连续性责任',?3,?4,?5,?5)",
            libsql_rusqlite::params![
                obligation_id,
                authority_level,
                json!({"kind":"AFTER_CHAPTER","value":0}).to_string(),
                lifecycle,
                NOW
            ],
        )
        .unwrap();
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
fn b14_task_truth_uses_volume_plan_class_and_current_task_progress() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    insert_committed_artifact(
        &handle,
        "proj-b14",
        "volume-plan:b14",
        "VolumePlan",
        "VOLUME",
        r#"{"level":"VOLUME","volumeId":"volume:b14"}"#,
        &json!({
            "payloadSchema":"volume-plan.r1",
            "volumeId":"volume:b14",
            "tasks":[{"taskId":"task:b14:required","class":"REQUIRED"}]
        }),
    );
    handle
        .execute(
            "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES (?1,?2,'volume.task.progress',?3,NULL,'{}',?4,?4)",
            libsql_rusqlite::params![
                "task:b14:required",
                "proj-b14",
                json!({"taskId":"task:b14:required","status":"BLOCKED","evidenceRefs":[],"remainingConditions":[]}).to_string(),
                NOW
            ],
        )
        .unwrap();
    drop(handle);

    let result = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let task_check = result["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["kind"] == "PAYOFF")
        .unwrap();
    assert_eq!(task_check["status"], "BLOCKED");
    assert_eq!(task_check["blockerCount"], 1);
}

#[test]
fn b14_continuity_uses_formal_thread_projection_and_keeps_c_advisory() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    insert_obligation(&handle, "obligation:b14:a", "A", "PROGRESSING");
    let threads = NarrativeRepository::continuity_threads_complete(&handle, "proj-b14").unwrap();
    assert_eq!(threads[0].thread_id, "thread:obligation:b14:a");
    assert_eq!(threads[0].logical_status, "OPEN");
    drop(handle);

    let blocked = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let payoff = blocked["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["kind"] == "PAYOFF")
        .unwrap();
    assert_eq!(payoff["status"], "BLOCKED");

    let (advisory_state, _root) = test_state();
    seed_ready_project(&advisory_state);
    let handle = advisory_state.manager.open("proj-b14", "", NOW).unwrap();
    insert_obligation(&handle, "obligation:b14:c", "C", "PROGRESSING");
    drop(handle);
    let advisory =
        query_completion_status_impl(&advisory_state, json!({"projectId":"proj-b14"})).unwrap();
    let payoff = advisory["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["kind"] == "PAYOFF")
        .unwrap();
    assert_eq!(payoff["status"], "READY");
    assert_eq!(payoff["advisoryCount"], 1);
}

#[test]
fn b14_continuity_projection_includes_foreshadow_mystery_and_evidence_sources() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    let chapter_id = "chapter:b14:continuity";
    let artifact_id = "final:b14:continuity";
    insert_committed_artifact(
        &handle,
        "proj-b14",
        artifact_id,
        "FinalChapter",
        "CHAPTER",
        &format!(r#"{{"level":"CHAPTER","chapterId":"{chapter_id}"}}"#),
        &json!({"payloadSchema":"final-chapter.r1","chapterId":chapter_id,"body":"终章"}),
    );
    insert_obligation(&handle, "obligation:b14:evidence", "A", "RESOLVED");
    handle.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,status,word_count,final_artifact_id,final_version,created_at,updated_at) VALUES (?1,'proj-b14','volume:b14',1,'连续性章','COMMITTED',2,?2,1,?3,?3)", libsql_rusqlite::params![chapter_id,artifact_id,NOW]).unwrap();
    handle.execute("INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,source_authority,created_at,updated_at) VALUES ('foreshadow:b14:thread','proj-b14','回收身份线索','PLANTED','B06',?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO mystery_states(mystery_id,project_id,mystery_type,status,created_at,updated_at) VALUES ('mystery:b14:thread','proj-b14','身份谜团','OPEN',?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO canon_evidence(evidence_id,project_id,fact_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,created_at) VALUES ('evidence:b14:thread','proj-b14','obligation:b14:evidence','chapter:b14:continuity',?1,1,'CANON','终章证据',?2)", libsql_rusqlite::params![artifact_id,NOW]).unwrap();
    let threads = NarrativeRepository::continuity_threads_complete(&handle, "proj-b14").unwrap();
    assert!(threads
        .iter()
        .any(|thread| thread.thread_id == "thread:foreshadow:b14:thread"));
    assert!(threads
        .iter()
        .any(|thread| thread.thread_id == "thread:mystery:b14:thread"));
    assert!(threads
        .iter()
        .any(|thread| !thread.evidence_refs.is_empty()));
}

#[test]
fn b14_mystery_mapping_requires_current_responsibility_disposition() {
    let (state, _root) = test_state();
    let plan = json!({"payloadSchema":"book-plan.r1","majorMysteryGoals":[{"id":"mystery:b14:closure","description":"身份谜团"}],"futureReservations":[]});
    seed_ready_project_with_plan(&state, plan);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    insert_obligation(&handle, "obligation:b14:mystery", "C", "PROGRESSING");
    drop(handle);
    execute_command_impl(&state, json!({"type":"endgame_contract_create","projectId":"proj-b14","commandId":"b14-mystery-active","idempotencyKey":"b14-mystery-active:key","expectedRevision":0,"requestedAt":NOW,"payload":{"requestedEnding":{"summary":"正式结局","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW},"mysteryMappings":[{"sourceMysteryId":"mystery:b14:closure","mappedResponsibilityRefs":["obligation:obligation:b14:mystery"],"mappingConfirmation":"USER_CONFIRMED"}]}})).unwrap();
    let blocked = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let mystery = blocked["checks"].as_array().unwrap().iter().find(|check| check["kind"] == "MYSTERY").unwrap();
    assert_eq!(mystery["status"], "BLOCKED");
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    handle.execute("UPDATE narrative_obligations SET lifecycle='RESOLVED' WHERE project_id='proj-b14' AND obligation_id='obligation:b14:mystery'", []).unwrap();
    drop(handle);
    let resolved = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let mystery = resolved["checks"].as_array().unwrap().iter().find(|check| check["kind"] == "MYSTERY").unwrap();
    assert_eq!(mystery["status"], "READY");
}

#[test]
fn b14_unrelated_final_chapter_cannot_fulfill_reservation_or_payoff() {
    let (state, _root) = test_state();
    let plan = json!({"payloadSchema":"book-plan.r1","majorMysteryGoals":[],"futureReservations":[{"reservationId":"reservation:b14:future","description":"未来承诺","authorityLevel":"A"}]});
    seed_ready_project_with_plan(&state, plan);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    insert_committed_artifact(&handle,"proj-b14","final:b14:unrelated","FinalChapter","CHAPTER",r#"{"level":"CHAPTER","chapterId":"chapter:b14:unrelated"}"#,&json!({"payloadSchema":"final-chapter.r1","chapterId":"chapter:b14:unrelated","body":"无关章节"}));
    drop(handle);
    execute_command_impl(&state, json!({"type":"endgame_contract_create","projectId":"proj-b14","commandId":"b14-unrelated-mapping","idempotencyKey":"b14-unrelated-mapping:key","expectedRevision":0,"requestedAt":NOW,"payload":{"requestedEnding":{"summary":"正式结局","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW},"reservationMappings":[{"reservationId":"reservation:b14:future","fulfillmentEvidenceRefs":["final-chapter:final:b14:unrelated"],"mappingConfirmation":"USER_CONFIRMED"}],"hardPayoffMappings":[{"payoffId":"payoff:b14:unrelated","fulfillmentEvidenceRefs":["final-chapter:final:b14:unrelated"],"fulfilled":true}]}})).unwrap();
    let result = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let payoff = result["checks"].as_array().unwrap().iter().find(|check| check["kind"] == "PAYOFF").unwrap();
    assert_eq!(payoff["status"], "BLOCKED");
}

#[test]
fn b14_hard_payoff_mapping_is_a_completion_blocker_until_fulfilled() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    execute_command_impl(
        &state,
        json!({
            "type":"endgame_contract_create","projectId":"proj-b14","commandId":"b14-payoff-contract","idempotencyKey":"b14-payoff-contract:key","expectedRevision":0,"requestedAt":NOW,
            "payload":{
                "requestedEnding":{"summary":"完成最终回报","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW},
                "hardPayoffMappings":[{"payoffId":"payoff:b14:final","fulfillmentEvidenceRefs":[],"fulfilled":false}]
            }
        }),
    )
    .unwrap();
    let result = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let payoff = result["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["kind"] == "PAYOFF")
        .unwrap();
    assert_eq!(payoff["status"], "BLOCKED");
    assert_eq!(payoff["blockerCount"], 1);
}

#[test]
fn b14_hard_payoff_rejects_non_final_artifact_as_fulfillment_evidence() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    execute_command_impl(
        &state,
        json!({
            "type":"endgame_contract_create","projectId":"proj-b14","commandId":"b14-payoff-invalid-evidence","idempotencyKey":"b14-payoff-invalid-evidence:key","expectedRevision":0,"requestedAt":NOW,
            "payload":{
                "requestedEnding":{"summary":"完成最终回报","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW},
                "hardPayoffMappings":[{"payoffId":"payoff:b14:final","fulfillmentEvidenceRefs":["book-plan:b14"],"fulfilled":true}]
            }
        }),
    )
    .unwrap();
    let result = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let payoff = result["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["kind"] == "PAYOFF")
        .unwrap();
    assert_eq!(payoff["status"], "BLOCKED");
}

#[test]
fn b14_export_preflight_blocks_uncommitted_chapter_in_final_sequence() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    handle.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,arc_id,chapter_no,title,status,word_count,created_at,updated_at) VALUES ('chapter:b14:draft','proj-b14','volume:b14',NULL,1,'尚未定稿','PRODUCING',0,?1,?1)",
        [NOW],
    ).unwrap();
    drop(handle);
    let result = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let export = result["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["kind"] == "EXPORT_INTEGRITY")
        .unwrap();
    assert_eq!(export["status"], "BLOCKED");
    assert_eq!(export["blockerCount"], 1);
}

#[test]
fn b14_s44_s45_run_through_model_runtime_and_stay_soft_only() {
    let (state, _root) = test_state_with_skill_gateway();
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
    assert_eq!(after_runs, before_runs + 2);
    assert!(advice["provenance"]["modelRunId"].as_str().is_some());
    assert!(quality["provenance"]["modelRunId"].as_str().is_some());
    assert!(advice["provenance"]["loadedSkillRefs"]
        .as_array()
        .is_some_and(|refs| refs.iter().any(|item| item
            .as_str()
            .unwrap_or("")
            .contains("endgame-design/SKILL.md"))));
    assert!(quality["provenance"]["loadedSkillRefs"]
        .as_array()
        .is_some_and(|refs| refs.iter().any(|item| item
            .as_str()
            .unwrap_or("")
            .contains("completion-payoff-audit/SKILL.md"))));
    assert!(advice["provenance"]["guidanceDigest"].as_str().is_some());
    assert!(quality["provenance"]["skillTaskContextDigest"]
        .as_str()
        .is_some());
    assert!(advice["provenance"]["resourceDigests"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
    assert!(quality["provenance"]["resourceDigests"]
        .as_array()
        .is_some_and(|items| !items.is_empty()));
}

#[test]
fn b14_status_and_soft_queries_do_not_mutate_authority() {
    let (state, _root) = test_state_with_skill_gateway();
    seed_ready_project(&state);
    let count = |connection: &libsql_rusqlite::Connection, table: &str| -> i64 {
        connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE project_id='proj-b14'"),
                [],
                |row| row.get(0),
            )
            .unwrap()
    };
    let before = {
        let handle = state.manager.open("proj-b14", "", NOW).unwrap();
        let values = [
            "canon_entities",
            "canon_facts",
            "canon_knowledge",
            "narrative_obligations",
            "foreshadows",
            "state_deltas",
            "dependency_edges",
            "artifacts",
            "commit_receipts",
        ]
        .into_iter()
        .map(|table| (table, count(&handle, table)))
        .collect::<Vec<_>>();
        drop(handle);
        values
    };
    query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    query_endgame_advice_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    query_completion_quality_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let after = {
        let handle = state.manager.open("proj-b14", "", NOW).unwrap();
        let values = [
            "canon_entities",
            "canon_facts",
            "canon_knowledge",
            "narrative_obligations",
            "foreshadows",
            "state_deltas",
            "dependency_edges",
            "artifacts",
            "commit_receipts",
        ]
        .into_iter()
        .map(|table| (table, count(&handle, table)))
        .collect::<Vec<_>>();
        drop(handle);
        values
    };
    assert_eq!(before, after);
}
