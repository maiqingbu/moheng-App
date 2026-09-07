use ai_novel_production_system_core::commands::{
    execute_command_impl, query_completion_quality_impl, query_completion_status_impl,
    query_endgame_advice_impl, query_endgame_status_impl, KernelState,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repositories::endgame::EndgameRepository;
use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
use libsql_rusqlite::Connection;
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const NOW: &str = "2026-09-06T00:00:00Z";

fn test_state() -> (KernelState, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_path_buf();
    std::mem::forget(temp);
    let state = KernelState::new(AppPaths::new(root.clone()));
    drop(state.manager.create("proj-b14", "B14", NOW).unwrap());
    (state, root)
}

struct EndgameSkillGateway {
    inputs: Arc<Mutex<Vec<serde_json::Value>>>,
}

struct ExplicitRuntimeGateway {
    requests: serde_json::Value,
}

impl ProductionModelGateway for ExplicitRuntimeGateway {
    fn provider(&self) -> &str {
        "b14-r5-test-provider"
    }
    fn model(&self, _capability: &str) -> String {
        "b14-r5-test-model".into()
    }
    fn invoke(
        &self,
        _capability: &str,
        _input: &serde_json::Value,
    ) -> Result<ProductionModelResponse, String> {
        Ok(ProductionModelResponse::deterministic(json!({
            "kind":"QualityFinding",
            "status":"OK",
            "summary":"R5 explicit runtime check",
            "suggestions":[],
            "runtimeCheckRequests":self.requests
        })))
    }
}

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
        input: &serde_json::Value,
    ) -> Result<ProductionModelResponse, String> {
        self.inputs.lock().unwrap().push(input.clone());
        Ok(ProductionModelResponse::deterministic(json!({
            "kind": if capability == "B14_S45_COMPLETION_QUALITY" { "QualityFinding" } else { "CreativeProposal" },
            "status":"OK",
            "summary":"测试 Skill Runtime 的软性建议。",
            "suggestions":["仅供作者参考，不改变正式完结结论。"],
            "runtimeCheckRequests":[{"kind":if capability == "B14_S45_COMPLETION_QUALITY" {"TASK_REALITY"} else {"THREAD_STATE"},"refs":[]}]
        })))
    }
}

fn test_state_with_skill_gateway() -> (KernelState, PathBuf, Arc<Mutex<Vec<serde_json::Value>>>) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_path_buf();
    std::mem::forget(temp);
    let inputs = Arc::new(Mutex::new(Vec::new()));
    let state = KernelState::with_production_gateway(
        AppPaths::new(root.clone()),
        Arc::new(EndgameSkillGateway {
            inputs: inputs.clone(),
        }),
    );
    drop(state.manager.create("proj-b14", "B14", NOW).unwrap());
    (state, root, inputs)
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

#[allow(clippy::too_many_arguments)]
fn insert_committed_artifact_with_command(
    connection: &Connection,
    project_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    scope_level: &str,
    scope_key: &str,
    payload: &serde_json::Value,
    command_id: &str,
) -> String {
    let hash = sha256_canonical(payload).unwrap();
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,authority_level,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'m0a.artifact.v1','COMMITTED','APPROVED_PLAN','C',?6,?7,?8,?8)",
            libsql_rusqlite::params![
                artifact_id,
                project_id,
                artifact_type,
                scope_level,
                scope_key,
                hash,
                json!({"commandId":command_id}).to_string(),
                NOW
            ],
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

fn r7_domain_snapshot(connection: &Connection) -> Vec<serde_json::Value> {
    let rows = [
        (
            "Canon",
            "canon_facts",
            "SELECT fact_id,subject_id,predicate,object_json FROM canon_facts WHERE project_id='proj-b14' ORDER BY fact_id",
        ),
        (
            "Knowledge",
            "canon_knowledge",
            "SELECT knowledge_id,knower_id,fact_id,epistemic_state FROM canon_knowledge WHERE project_id='proj-b14' ORDER BY knowledge_id",
        ),
        (
            "Narrative Obligation",
            "narrative_obligations",
            "SELECT obligation_id,authority_level,lifecycle FROM narrative_obligations WHERE project_id='proj-b14' ORDER BY obligation_id",
        ),
        (
            "Foreshadow",
            "foreshadows",
            "SELECT foreshadow_id,status FROM foreshadows WHERE project_id='proj-b14' ORDER BY foreshadow_id",
        ),
        (
            "ReaderDesire",
            "reader_desires",
            "SELECT desire_id,status FROM reader_desires WHERE project_id='proj-b14' ORDER BY desire_id",
        ),
        (
            "Planning",
            "volume_plan",
            "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id='proj-b14' AND artifact_type='VolumePlan' ORDER BY artifact_id",
        ),
        (
            "Task Reality",
            "state_deltas",
            "SELECT state_id,value_json FROM state_deltas WHERE project_id='proj-b14' AND state_type='volume.task.progress' ORDER BY state_id",
        ),
        (
            "ChangeRequest",
            "change_requests",
            "SELECT change_request_id,status,summary FROM change_requests WHERE project_id='proj-b14' ORDER BY change_request_id",
        ),
        (
            "RetconPlan",
            "retcon_plans",
            "SELECT retcon_plan_id,status,reason FROM retcon_plans WHERE project_id='proj-b14' ORDER BY retcon_plan_id",
        ),
        (
            "FinalChapter",
            "chapters",
            "SELECT chapter_id,final_artifact_id,final_version FROM chapters WHERE project_id='proj-b14' AND status='COMMITTED' ORDER BY chapter_no,chapter_id",
        ),
    ];
    let mut domains = rows
        .into_iter()
        .map(|(name, source, sql)| {
            let mut statement = connection.prepare(sql).unwrap();
            let values = statement
                .query_map([], |row| {
                    let mut value = Vec::new();
                    for index in 0..row.as_ref().column_count() {
                        value.push(row.get::<_, String>(index).unwrap_or_default());
                    }
                    Ok(value)
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            json!({
                "name": name,
                "source": source,
                "count": values.len(),
                "digest": sha256_canonical(&json!(values)).unwrap()
            })
        })
        .collect::<Vec<_>>();
    let continuity =
        NarrativeRepository::continuity_threads_complete(connection, "proj-b14").unwrap();
    domains.insert(
        5,
        json!({
            "name": "Continuity",
            "source": "NarrativeRepository::continuity_threads_complete",
            "count": continuity.len(),
            "digest": sha256_canonical(&serde_json::to_value(&continuity).unwrap()).unwrap()
        }),
    );
    domains
}

fn assert_r7_domains_unchanged(
    before: &[serde_json::Value],
    after: &[serde_json::Value],
    phase: &str,
) {
    assert_eq!(before.len(), 11, "R7 expected 11 domains before {phase}");
    assert_eq!(after.len(), 11, "R7 expected 11 domains after {phase}");
    for (before_domain, after_domain) in before.iter().zip(after) {
        assert_eq!(
            before_domain["count"], after_domain["count"],
            "R7 {phase} count changed for {}",
            before_domain["name"]
        );
        assert_eq!(
            before_domain["digest"], after_domain["digest"],
            "R7 {phase} digest changed for {}",
            before_domain["name"]
        );
    }
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
fn b14_r4_continuity_complete_reads_501_rows_and_exposes_hard_thread() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    insert_committed_artifact(
        &handle,
        "proj-b14",
        "final:b14:r4",
        "FinalChapter",
        "CHAPTER",
        r#"{"level":"CHAPTER","chapterId":"chapter:b14:r4"}"#,
        &json!({"payloadSchema":"final-chapter.r1","chapterId":"chapter:b14:r4","body":"R4 continuity source"}),
    );
    handle.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,status,word_count,final_artifact_id,final_version,created_at,updated_at) VALUES ('chapter:b14:r4','proj-b14','volume:b14',1,'R4 continuity source','COMMITTED',3,'final:b14:r4',1,?1,?1)",
        [NOW],
    ).unwrap();
    for index in 1..=501 {
        handle.execute(
            "INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,source_authority,created_at,updated_at) VALUES (?1,'proj-b14',?2,'PLANTED','B06',?3,?3)",
            libsql_rusqlite::params![format!("foreshadow:b14:r4:{index:03}"), format!("伏笔 {index}"), NOW],
        ).unwrap();
        handle.execute(
            "INSERT INTO mystery_states(mystery_id,project_id,mystery_type,status,created_at,updated_at) VALUES (?1,'proj-b14','R4谜团','OPEN',?2,?2)",
            libsql_rusqlite::params![format!("mystery:b14:r4:{index:03}"), NOW],
        ).unwrap();
        handle.execute(
            "INSERT INTO canon_evidence(evidence_id,project_id,fact_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,created_at) VALUES (?1,'proj-b14',?2,?3,'final:b14:r4',1,'CANON',?4,?5)",
            libsql_rusqlite::params![format!("evidence:b14:r4:{index:03}"), format!("fact:b14:r4:{index:03}"), "chapter:b14:r4", format!("证据 {index}"), NOW],
        ).unwrap();
    }
    insert_obligation(&handle, "obligation:b14:r4:hard", "A", "PROGRESSING");
    let foreshadows =
        NarrativeRepository::active_foreshadows_complete(&handle, "proj-b14").unwrap();
    let mysteries = NarrativeRepository::open_mysteries_complete(&handle, "proj-b14").unwrap();
    let evidence = NarrativeRepository::recent_evidence_complete(&handle, "proj-b14").unwrap();
    assert_eq!(foreshadows.len(), 501);
    assert_eq!(mysteries.len(), 501);
    assert_eq!(evidence.len(), 501);
    let threads = NarrativeRepository::continuity_threads_complete(&handle, "proj-b14").unwrap();
    assert!(threads
        .iter()
        .any(|thread| thread.thread_id == "thread:obligation:b14:r4:hard"
            && thread.logical_status == "OPEN"));
    assert!(threads
        .iter()
        .any(|thread| thread.thread_id == "thread:foreshadow:b14:r4:501"));
    println!("B14_R4_CONTINUITY|foreshadows=501|mysteries=501|evidence=501|hardBlocker=PASS|digestSource=complete");
    drop(handle);
    execute_command_impl(
        &state,
        json!({
            "type":"endgame_contract_create","projectId":"proj-b14","commandId":"b14-r4-continuity-contract","idempotencyKey":"b14-r4-continuity-contract:key","expectedRevision":0,"requestedAt":NOW,
            "payload":{"requestedEnding":{"summary":"R4 连续性测试结局","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW}}
        }),
    ).unwrap();
    let result = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    assert_eq!(result["status"], "BLOCKED");
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
    let mystery = blocked["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["kind"] == "MYSTERY")
        .unwrap();
    assert_eq!(mystery["status"], "BLOCKED");
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    handle.execute("UPDATE narrative_obligations SET lifecycle='RESOLVED' WHERE project_id='proj-b14' AND obligation_id='obligation:b14:mystery'", []).unwrap();
    drop(handle);
    let resolved = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let mystery = resolved["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["kind"] == "MYSTERY")
        .unwrap();
    assert_eq!(mystery["status"], "READY");
}

#[test]
fn b14_hard_responsibilities_are_instance_level_and_change_matchable() {
    let (state, _root) = test_state();
    seed_ready_project_with_plan(
        &state,
        json!({
            "payloadSchema":"book-plan.r1",
            "majorMysteryGoals":[{"id":"mystery:b14:instance"}],
            "futureReservations":[{"reservationId":"reservation:b14:instance","authorityLevel":"A"}]
        }),
    );
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    insert_obligation(&handle, "obligation:b14:instance", "A", "PROGRESSING");
    let responsibilities =
        EndgameRepository::instance_responsibilities(&handle, "proj-b14").unwrap();
    let items = responsibilities.as_array().unwrap();
    assert!(items
        .iter()
        .any(|item| item["responsibilityId"] == "obligation:obligation:b14:instance"));
    assert!(items
        .iter()
        .any(|item| item["responsibilityId"] == "mystery:mystery:b14:instance"));
    assert!(items
        .iter()
        .any(|item| item["responsibilityId"] == "reservation:reservation:b14:instance"));
    assert!(items
        .iter()
        .all(|item| item["sourceRefs"]
            .as_array()
            .is_some_and(|refs| refs.iter().all(|reference| reference
                .as_str()
                .is_some_and(|value| !value.contains("narrative_obligations")
                    && !value.contains("continuity_threads"))))));
    println!(
        "B14_R3_INSTANCE_REFS|{}",
        serde_json::to_string(&responsibilities).unwrap()
    );
}

#[test]
fn b14_unrelated_final_chapter_cannot_fulfill_reservation_or_payoff() {
    let (state, _root) = test_state();
    let plan = json!({"payloadSchema":"book-plan.r1","majorMysteryGoals":[],"futureReservations":[{"reservationId":"reservation:b14:future","description":"未来承诺","authorityLevel":"A"}]});
    seed_ready_project_with_plan(&state, plan);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    insert_committed_artifact(
        &handle,
        "proj-b14",
        "final:b14:unrelated",
        "FinalChapter",
        "CHAPTER",
        r#"{"level":"CHAPTER","chapterId":"chapter:b14:unrelated"}"#,
        &json!({"payloadSchema":"final-chapter.r1","chapterId":"chapter:b14:unrelated","body":"无关章节"}),
    );
    drop(handle);
    execute_command_impl(&state, json!({"type":"endgame_contract_create","projectId":"proj-b14","commandId":"b14-unrelated-mapping","idempotencyKey":"b14-unrelated-mapping:key","expectedRevision":0,"requestedAt":NOW,"payload":{"requestedEnding":{"summary":"正式结局","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW},"reservationMappings":[{"reservationId":"reservation:b14:future","fulfillmentEvidenceRefs":["final-chapter:final:b14:unrelated"],"mappingConfirmation":"USER_CONFIRMED"}],"hardPayoffMappings":[{"payoffId":"payoff:b14:unrelated","fulfillmentEvidenceRefs":["final-chapter:final:b14:unrelated"],"fulfilled":true}]}})).unwrap();
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
fn b14_r4_mapping_candidates_are_relation_scoped_before_ui() {
    let (state, _root) = test_state();
    seed_ready_project_with_plan(
        &state,
        json!({
            "payloadSchema":"book-plan.r1",
            "majorMysteryGoals":[],
            "futureReservations":[{"reservationId":"reservation:b14:r4","description":"R4未来约束","authorityLevel":"A"}]
        }),
    );
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    insert_committed_artifact(
        &handle,
        "proj-b14",
        "final:b14:r4:related",
        "FinalChapter",
        "CHAPTER",
        r#"{"level":"CHAPTER","chapterId":"chapter:b14:r4:related"}"#,
        &json!({"payloadSchema":"final-chapter.r1","chapterId":"chapter:b14:r4:related","reservationId":"reservation:b14:r4","evidenceSummary":"关联正式依据"}),
    );
    insert_committed_artifact(
        &handle,
        "proj-b14",
        "final:b14:r4:unrelated",
        "FinalChapter",
        "CHAPTER",
        r#"{"level":"CHAPTER","chapterId":"chapter:b14:r4:unrelated"}"#,
        &json!({"payloadSchema":"final-chapter.r1","chapterId":"chapter:b14:r4:unrelated","evidenceSummary":"无关章节"}),
    );
    handle.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,status,word_count,final_artifact_id,final_version,created_at,updated_at) VALUES ('chapter:b14:r4:related','proj-b14','volume:b14',1,'关联章','COMMITTED',3,'final:b14:r4:related',1,?1,?1),('chapter:b14:r4:unrelated','proj-b14','volume:b14',2,'无关章','COMMITTED',3,'final:b14:r4:unrelated',1,?1,?1)", [NOW]).unwrap();
    let workflow = EndgameRepository::mapping_workflow(&handle, "proj-b14").unwrap();
    let candidates = workflow["reservationCandidates"]["reservation:b14:r4"]
        .as_array()
        .unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0]["ref"], "final:b14:r4:related");
    assert!(!workflow["fulfillmentCandidates"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["ref"] == "final:b14:r4:unrelated"));
    println!("B14_R4_MAPPING|relatedCandidates=1|unrelatedOmitted=true|bounded=true");
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
    let (state, _root, inputs) = test_state_with_skill_gateway();
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
    assert_eq!(advice["runtimeCheckRequests"].as_array().unwrap().len(), 1);
    assert_eq!(quality["runtimeCheckRequests"].as_array().unwrap().len(), 1);
    assert_eq!(advice["runtimeCheckResults"].as_array().unwrap().len(), 1);
    assert_eq!(quality["runtimeCheckResults"].as_array().unwrap().len(), 1);
    assert_eq!(
        advice["runtimeCheckResults"][0]["source"],
        "B14::DeterministicRuntime"
    );
    assert_eq!(
        quality["runtimeCheckResults"][0]["source"],
        "B14::DeterministicRuntime"
    );

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
    let inputs = inputs.lock().unwrap();
    assert_eq!(inputs.len(), 2);
    for input in inputs.iter() {
        assert_eq!(
            input["skillTaskContext"]["runtimeStage"],
            "ENDGAME_COMPLETION"
        );
        assert_eq!(
            input["skillTaskContext"]["qualityInputs"]["source"],
            "B14_ENDGAME_BOUNDED"
        );
        assert!(input["methodGuidance"]
            .as_str()
            .is_some_and(|guidance| guidance.contains("methodRef=")));
    }
    println!(
        "B14_R3_SKILL_PROVENANCE|{}",
        serde_json::to_string(&json!({
            "S44": advice["provenance"],
            "S45": quality["provenance"],
            "boundedRuntimeStage": "ENDGAME_COMPLETION",
            "guidanceConsumed": true
        }))
        .unwrap()
    );
    println!("B14_R4_RUNTIME_CHECK_HANDOFF|S44=PASS|S45=PASS|deterministic=true|modelRunsDelta=2|authorityWrite=false");
}

#[test]
fn b14_r5_empty_runtime_checks_recheck_current_task_and_thread_scope() {
    let (state, _root, _inputs) = test_state_with_skill_gateway();
    seed_ready_project(&state);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    let volume_scope = r#"{"level":"VOLUME","volumeId":"volume:b14"}"#;
    insert_committed_artifact(
        &handle,
        "proj-b14",
        "volume-plan:b14:r5",
        "VolumePlan",
        "VOLUME",
        volume_scope,
        &json!({"payloadSchema":"volume-plan.r1","tasks":[{"taskId":"task:b14:r5:blocked","class":"REQUIRED"}]}),
    );
    handle.execute(
        "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES ('task:b14:r5:blocked','proj-b14','volume.task.progress',?1,'{}',?2,?2)",
        libsql_rusqlite::params![json!({"taskId":"task:b14:r5:blocked","status":"BLOCKED"}).to_string(), NOW],
    ).unwrap();
    insert_obligation(&handle, "obligation:b14:r5:thread", "A", "PROGRESSING");
    drop(handle);

    let blocked_task =
        query_completion_quality_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let task_result = &blocked_task["runtimeCheckResults"][0]["result"];
    assert_eq!(task_result["status"], "ISSUE_FOUND");
    assert!(task_result["findingCount"].as_i64().unwrap_or(0) >= 1);
    assert_eq!(
        task_result["resolvedScope"],
        "current_endgame_hard_task_reality"
    );

    let open_thread = query_endgame_advice_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let thread_result = &open_thread["runtimeCheckResults"][0]["result"];
    assert_eq!(thread_result["status"], "ISSUE_FOUND");
    assert!(thread_result["findingCount"].as_i64().unwrap_or(0) >= 1);
    assert_eq!(thread_result["resolvedScope"], "current_endgame_continuity");

    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    handle.execute("UPDATE state_deltas SET value_json=?1 WHERE project_id='proj-b14' AND state_id='task:b14:r5:blocked'", [json!({"taskId":"task:b14:r5:blocked","status":"SATISFIED"}).to_string()]).unwrap();
    handle.execute("UPDATE narrative_obligations SET lifecycle='RESOLVED' WHERE project_id='proj-b14' AND obligation_id='obligation:b14:r5:thread'", []).unwrap();
    drop(handle);

    let satisfied_task =
        query_completion_quality_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    assert_eq!(
        satisfied_task["runtimeCheckResults"][0]["result"]["status"],
        "NO_ISSUE"
    );
    let resolved_thread =
        query_endgame_advice_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    assert_eq!(
        resolved_thread["runtimeCheckResults"][0]["result"]["status"],
        "NO_ISSUE"
    );
    println!("B14_R5_RUNTIME_EMPTY_SCOPE|taskPositive=PASS|taskNegative=PASS|threadPositive=PASS|threadNegative=PASS");
}

#[test]
fn b14_r5_continuity_101_and_501_are_real_hard_blockers() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    for index in 1..=100 {
        insert_obligation(
            &handle,
            &format!("obligation:r5:{index:03}"),
            "C",
            "RESOLVED",
        );
    }
    insert_obligation(&handle, "obligation:r5:101", "A", "PROGRESSING");
    drop(handle);

    let first_threads = NarrativeRepository::continuity_threads_complete(
        &state.manager.open("proj-b14", "", NOW).unwrap(),
        "proj-b14",
    )
    .unwrap();
    let first_hard = first_threads
        .iter()
        .filter(|thread| {
            matches!(thread.authority_level.as_str(), "A" | "B")
                && matches!(thread.logical_status.as_str(), "OPEN" | "SUSPENDED")
        })
        .map(|thread| thread.thread_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(first_hard, vec!["thread:obligation:r5:101"]);

    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    for index in 102..=500 {
        insert_obligation(
            &handle,
            &format!("obligation:r5:{index:03}"),
            "C",
            "RESOLVED",
        );
    }
    insert_obligation(&handle, "obligation:r5:501", "A", "PROGRESSING");
    drop(handle);

    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    let final_threads =
        NarrativeRepository::continuity_threads_complete(&handle, "proj-b14").unwrap();
    let final_hard = final_threads
        .iter()
        .filter(|thread| {
            matches!(thread.authority_level.as_str(), "A" | "B")
                && matches!(thread.logical_status.as_str(), "OPEN" | "SUSPENDED")
        })
        .map(|thread| thread.thread_id.clone())
        .collect::<Vec<_>>();
    assert!(final_hard
        .iter()
        .any(|thread| thread == "thread:obligation:r5:101"));
    assert!(final_hard
        .iter()
        .any(|thread| thread == "thread:obligation:r5:501"));
    assert_eq!(final_threads.len(), 501);
    println!("B14_R5_CONTINUITY_REAL|sourceIndex101=thread:obligation:r5:101|sourceIndex501=thread:obligation:r5:501|hardBlocker101=true|hardBlocker501=true|completeRows={}", final_threads.len());
}

#[test]
fn b14_r5_no_authority_mutation_uses_non_empty_domain_fixtures() {
    let (state, _root, _inputs) = test_state_with_skill_gateway();
    seed_ready_project(&state);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    handle.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:r5','proj-b14','R5人物','PERSON',?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,source_artifact_id,source_version,authority_level,created_at,updated_at) VALUES ('fact:r5','proj-b14','entity:r5','状态','{}','book-plan:b14',1,'A',?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO canon_knowledge(knowledge_id,project_id,knower_id,fact_id,epistemic_state,visibility_scope,created_at,updated_at) VALUES ('knowledge:r5','proj-b14','entity:r5','fact:r5','KNOWS','CHAPTER',?1,?1)", [NOW]).unwrap();
    insert_obligation(&handle, "obligation:r5:authority", "C", "RESOLVED");
    handle.execute("INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,source_authority,created_at,updated_at) VALUES ('foreshadow:r5','proj-b14','R5伏笔','PLANTED','B06',?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO reader_desires(desire_id,project_id,desire_type,status,payload_json,created_at,updated_at) VALUES ('desire:r5','proj-b14','谜底','OPEN','{}',?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES ('task:r5:authority','proj-b14','volume.task.progress',?1,'{}',?2,?2)", libsql_rusqlite::params![json!({"taskId":"task:r5:authority","status":"SATISFIED"}).to_string(), NOW]).unwrap();
    handle.execute("INSERT INTO change_requests(change_request_id,project_id,change_type,scope,summary,status,impact_seed_json,created_at,updated_at) VALUES ('change:r5','proj-b14','REWRITE','CHAPTER','R5变更','DRAFT','{}',?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO retcon_plans(retcon_plan_id,project_id,target_fact_json,reason,fix_type,downstream_impact_json,estimated_cost_cents,status,created_at,updated_at) VALUES ('retcon:r5','proj-b14','{}','R5修订','SUPPLEMENT','{}',0,'DRAFT',?1,?1)", [NOW]).unwrap();
    insert_committed_artifact(
        &handle,
        "proj-b14",
        "volume-plan:r5",
        "VolumePlan",
        "VOLUME",
        r#"{"level":"VOLUME","volumeId":"volume:b14"}"#,
        &json!({"payloadSchema":"volume-plan.r1","tasks":[]}),
    );
    let final_hash = insert_committed_artifact(
        &handle,
        "proj-b14",
        "final:r5",
        "FinalChapter",
        "CHAPTER",
        r#"{"level":"CHAPTER","chapterId":"chapter:r5"}"#,
        &json!({"payloadSchema":"final-chapter.r1","chapterId":"chapter:r5","body":"R5正文"}),
    );
    handle.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,status,word_count,final_artifact_id,final_version,created_at,updated_at) VALUES ('chapter:r5','proj-b14','volume:b14',1,'R5章','COMMITTED',3,'final:r5',1,?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO canon_evidence(evidence_id,project_id,fact_id,final_chapter_id,source_artifact_id,source_version,evidence_type,created_at) VALUES ('evidence:r5','proj-b14','fact:r5','chapter:r5','final:r5',1,'CANON',?1)", [NOW]).unwrap();

    let snapshot = |connection: &Connection| {
        let rows = [
            ("Canon", "canon_facts", "SELECT fact_id,subject_id,predicate,object_json FROM canon_facts WHERE project_id='proj-b14' ORDER BY fact_id"),
            ("Knowledge", "canon_knowledge", "SELECT knowledge_id,knower_id,fact_id,epistemic_state FROM canon_knowledge WHERE project_id='proj-b14' ORDER BY knowledge_id"),
            ("Narrative Obligation", "narrative_obligations", "SELECT obligation_id,authority_level,lifecycle FROM narrative_obligations WHERE project_id='proj-b14' ORDER BY obligation_id"),
            ("Foreshadow", "foreshadows", "SELECT foreshadow_id,status FROM foreshadows WHERE project_id='proj-b14' ORDER BY foreshadow_id"),
            ("ReaderDesire", "reader_desires", "SELECT desire_id,status FROM reader_desires WHERE project_id='proj-b14' ORDER BY desire_id"),
            ("Planning", "volume_plan", "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id='proj-b14' AND artifact_type='VolumePlan' ORDER BY artifact_id"),
            ("Task Reality", "state_deltas", "SELECT state_id,value_json FROM state_deltas WHERE project_id='proj-b14' AND state_type='volume.task.progress' ORDER BY state_id"),
            ("ChangeRequest", "change_requests", "SELECT change_request_id,status,summary FROM change_requests WHERE project_id='proj-b14' ORDER BY change_request_id"),
            ("RetconPlan", "retcon_plans", "SELECT retcon_plan_id,status,reason FROM retcon_plans WHERE project_id='proj-b14' ORDER BY retcon_plan_id"),
            ("FinalChapter", "chapters", "SELECT chapter_id,final_artifact_id,final_version FROM chapters WHERE project_id='proj-b14' AND status='COMMITTED' ORDER BY chapter_no,chapter_id"),
        ];
        rows.into_iter().map(|(name, source, sql)| {
            let mut statement = connection.prepare(sql).unwrap();
            let values = statement.query_map([], |row| {
                let mut value = Vec::new();
                for index in 0..row.as_ref().column_count() { value.push(row.get::<_, String>(index).unwrap_or_default()); }
                Ok(value)
            }).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
            json!({"name":name,"source":source,"beforeCount":values.len(),"digest":sha256_canonical(&json!(values)).unwrap()})
        }).collect::<Vec<_>>()
    };
    let before = snapshot(&handle);
    assert!(before
        .iter()
        .all(|domain| domain["beforeCount"].as_i64().unwrap_or(0) > 0));
    let before_digests = EndgameRepository::domain_digests(&handle, "proj-b14").unwrap();
    drop(handle);
    query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    query_endgame_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    query_endgame_advice_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    query_completion_quality_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    let after = snapshot(&handle);
    let after_digests = EndgameRepository::domain_digests(&handle, "proj-b14").unwrap();
    for (before_domain, after_domain) in before.iter().zip(after.iter()) {
        assert_eq!(before_domain["beforeCount"], after_domain["beforeCount"]);
        assert_eq!(before_domain["digest"], after_domain["digest"]);
    }
    assert_eq!(before_digests, after_digests);
    let domains = before.into_iter().zip(after).map(|(before, after)| json!({"name":before["name"],"source":before["source"],"beforeCount":before["beforeCount"],"beforeDigest":before["digest"],"afterCount":after["beforeCount"],"afterDigest":after["digest"],"unchanged":true})).collect::<Vec<_>>();
    println!(
        "B14_R5_NO_AUTHORITY_REAL|domains={}",
        serde_json::to_string(&domains).unwrap()
    );
    assert!(!final_hash.is_empty());
}

#[test]
fn b14_r5_explicit_runtime_refs_are_scoped_and_cross_project_refs_are_unsupported() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(ExplicitRuntimeGateway {
            requests: json!([{"kind":"TASK_REALITY","refs":["task:r5:only"]}]),
        }),
    );
    drop(state.manager.create("proj-b14", "B14", NOW).unwrap());
    seed_ready_project(&state);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    insert_committed_artifact(
        &handle,
        "proj-b14",
        "volume-plan:r5:explicit",
        "VolumePlan",
        "VOLUME",
        r#"{"level":"VOLUME","volumeId":"volume:b14"}"#,
        &json!({"payloadSchema":"volume-plan.r1","tasks":[{"taskId":"task:r5:only","class":"REQUIRED"},{"taskId":"task:r5:other","class":"REQUIRED"}]}),
    );
    handle.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES ('task:r5:only','proj-b14','volume.task.progress',?1,'{}',?2,?2),('task:r5:other','proj-b14','volume.task.progress',?3,'{}',?2,?2)", libsql_rusqlite::params![json!({"taskId":"task:r5:only","status":"BLOCKED"}).to_string(), NOW, json!({"taskId":"task:r5:other","status":"BLOCKED"}).to_string()]).unwrap();
    drop(handle);
    let scoped = query_completion_quality_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    assert_eq!(
        scoped["runtimeCheckResults"][0]["result"]["status"],
        "ISSUE_FOUND"
    );
    assert_eq!(
        scoped["runtimeCheckResults"][0]["result"]["sourceRefs"],
        json!(["task:r5:only"])
    );

    let invalid_state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().join("invalid")),
        Arc::new(ExplicitRuntimeGateway {
            requests: json!([{"kind":"TASK_REALITY","refs":["task:other-project:blocked"]}]),
        }),
    );
    drop(
        invalid_state
            .manager
            .create("proj-b14", "B14", NOW)
            .unwrap(),
    );
    seed_ready_project(&invalid_state);
    let invalid =
        query_completion_quality_impl(&invalid_state, json!({"projectId":"proj-b14"})).unwrap();
    assert_eq!(
        invalid["runtimeCheckResults"][0]["result"]["status"],
        "UNSUPPORTED"
    );
    println!("B14_R5_RUNTIME_EXPLICIT|scopedTask=PASS|otherTaskExcluded=true|crossProjectUnsupported=PASS");
}

#[test]
fn b14_r6_paged_foreshadow_and_mystery_sources_create_real_hard_blockers() {
    let run_case = |source_class: &str, ordinal: usize| -> serde_json::Value {
        let (state, _root) = test_state();
        seed_ready_project(&state);
        let handle = state.manager.open("proj-b14", "", NOW).unwrap();
        if source_class == "Foreshadow" {
            for index in 1..=ordinal {
                handle.execute(
                    "INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,source_authority,created_at,updated_at) VALUES (?1,'proj-b14',?2,'PLANTED','B06',?3,?3)",
                    libsql_rusqlite::params![format!("foreshadow:r6:{index:03}"), format!("R6伏笔 {index}"), NOW],
                ).unwrap();
            }
        } else {
            for index in 1..=ordinal {
                handle.execute(
                    "INSERT INTO mystery_states(mystery_id,project_id,mystery_type,status,created_at,updated_at) VALUES (?1,'proj-b14','R6谜团','OPEN',?2,?2)",
                    libsql_rusqlite::params![format!("mystery:r6:{index:03}"), NOW],
                ).unwrap();
            }
        }
        let source_ref = format!("{}:r6:{ordinal:03}", source_class.to_lowercase());
        handle.execute(
            "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_event_id,source_artifact_id,source_version,description,prerequisites_json,lifecycle,created_at,updated_at) VALUES (?1,'proj-b14','BOOK','R6_PAGED_SOURCE','A',?2,'book-plan:b14',1,'分页来源产生的硬连续性责任',?3,'PROGRESSING',?4,?4)",
            libsql_rusqlite::params![format!("obligation:r6:{source_class}:{ordinal:03}"), source_ref, json!({"kind":"AFTER_CHAPTER","value":0}).to_string(), NOW],
        ).unwrap();
        drop(handle);
        execute_command_impl(&state, json!({
            "type":"endgame_contract_create","projectId":"proj-b14","commandId":format!("r6-contract-{source_class}-{ordinal}"),"idempotencyKey":format!("r6-contract-{source_class}-{ordinal}:key"),"expectedRevision":0,"requestedAt":NOW,
            "payload":{"requestedEnding":{"summary":"R6 分页来源验证","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW}}
        })).unwrap();
        let handle = state.manager.open("proj-b14", "", NOW).unwrap();
        let threads =
            NarrativeRepository::continuity_threads_complete(&handle, "proj-b14").unwrap();
        let thread = threads
            .iter()
            .find(|thread| {
                thread.thread_id == format!("thread:obligation:r6:{source_class}:{ordinal:03}")
            })
            .unwrap();
        let read_count = if source_class == "Foreshadow" {
            NarrativeRepository::active_foreshadows_complete(&handle, "proj-b14")
                .unwrap()
                .len()
        } else {
            NarrativeRepository::open_mysteries_complete(&handle, "proj-b14")
                .unwrap()
                .len()
        };
        let page_size_results = [1, 30, 50].into_iter().map(|page_size| {
            let result = query_completion_status_impl(&state, json!({"projectId":"proj-b14","pageSize":page_size})).unwrap();
            let payoff = result["checks"].as_array().unwrap().iter().find(|item| item["kind"] == "PAYOFF").unwrap();
            json!({"pageSize":page_size,"status":result["status"],"continuityBlockerCount":payoff["blockerCount"]})
        }).collect::<Vec<_>>();
        assert!(page_size_results
            .iter()
            .all(|item| item["status"] == "BLOCKED"));
        assert!(page_size_results
            .iter()
            .all(|item| item["continuityBlockerCount"].as_i64().unwrap_or(0) >= 1));
        assert_eq!(read_count, ordinal);
        assert_eq!(thread.authority_level, "A");
        assert!(matches!(
            thread.logical_status.as_str(),
            "OPEN" | "SUSPENDED"
        ));
        json!({"sourceClass":format!("{source_class}-derived"),"sourceOrdinal":ordinal,"sourceRef":thread.source_event_ref,"threadRef":thread.thread_id,"authorityLevel":thread.authority_level,"logicalStatus":thread.logical_status,"blockerDetected":true,"readCount":read_count,"blockerCount":page_size_results[0]["continuityBlockerCount"],"completionStatus":page_size_results[0]["status"],"pageSizeResults":page_size_results})
    };
    let case101 = run_case("Foreshadow", 101);
    let case501 = run_case("Mystery", 501);
    assert_eq!(case101["sourceOrdinal"], 101);
    assert_eq!(case501["sourceOrdinal"], 501);
    println!(
        "B14_R6_CONTINUITY_PAGED_BOUNDARY|{}",
        serde_json::to_string(&json!({"case101":case101,"case501":case501})).unwrap()
    );
}

#[test]
fn b14_r6_continuity_domain_is_non_empty_and_unchanged_after_read_and_completion() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    const R6_CONTINUITY_DOMAIN_SEED: bool = true;
    if R6_CONTINUITY_DOMAIN_SEED {
        insert_obligation(&handle, "obligation:r6:continuity-domain", "C", "RESOLVED");
    }
    drop(handle);
    execute_command_impl(&state, json!({
        "type":"endgame_contract_create","projectId":"proj-b14","commandId":"r6-continuity-domain-contract","idempotencyKey":"r6-continuity-domain-contract:key","expectedRevision":0,"requestedAt":NOW,
        "payload":{"requestedEnding":{"summary":"R6 Continuity domain","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW}}
    })).unwrap();
    let before = {
        let handle = state.manager.open("proj-b14", "", NOW).unwrap();
        let rows = NarrativeRepository::continuity_threads_complete(&handle, "proj-b14").unwrap();
        let digest = EndgameRepository::domain_digests(&handle, "proj-b14").unwrap()
            ["continuityDigest"]
            .clone();
        json!({"name":"Continuity","source":"NarrativeRepository::continuity_threads_complete","beforeCount":rows.len(),"beforeDigest":digest})
    };
    assert!(
        before["beforeCount"].as_i64().unwrap_or(0) > 0,
        "REQUIRED_AUTHORITY_DOMAIN_EMPTY"
    );
    query_endgame_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let completion = execute_command_impl(&state, json!({
        "type":"complete_book","projectId":"proj-b14","commandId":"r6-continuity-domain-complete","idempotencyKey":"r6-continuity-domain-complete:key","expectedRevision":1,"requestedAt":NOW,"payload":{}
    })).unwrap();
    assert_eq!(completion["finalStatus"], "committed");
    let after = {
        let handle = state.manager.open("proj-b14", "", NOW).unwrap();
        let rows = NarrativeRepository::continuity_threads_complete(&handle, "proj-b14").unwrap();
        let digest = EndgameRepository::domain_digests(&handle, "proj-b14").unwrap()
            ["continuityDigest"]
            .clone();
        json!({"afterCount":rows.len(),"afterDigest":digest})
    };
    assert_eq!(before["beforeCount"], after["afterCount"]);
    assert_eq!(before["beforeDigest"], after["afterDigest"]);
    println!(
        "B14_R6_CONTINUITY_DOMAIN|{}",
        serde_json::to_string(&json!({"before":before,"after":after,"unchanged":true})).unwrap()
    );
}

#[test]
fn b14_status_and_soft_queries_do_not_mutate_authority() {
    let (state, _root, _inputs) = test_state_with_skill_gateway();
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
    let before_digests = {
        let handle = state.manager.open("proj-b14", "", NOW).unwrap();
        let value = EndgameRepository::domain_digests(&handle, "proj-b14").unwrap();
        drop(handle);
        value
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
    let after_digests = {
        let handle = state.manager.open("proj-b14", "", NOW).unwrap();
        let value = EndgameRepository::domain_digests(&handle, "proj-b14").unwrap();
        drop(handle);
        value
    };
    assert_eq!(before, after);
    assert_eq!(before_digests, after_digests);
}

#[test]
fn b14_core_queries_and_completion_do_not_schedule_remote_models() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    execute_command_impl(
        &state,
        json!({
            "type":"endgame_contract_create","projectId":"proj-b14","commandId":"b14-no-remote-contract","idempotencyKey":"b14-no-remote-contract:key","expectedRevision":0,"requestedAt":NOW,
            "payload":{"requestedEnding":{"summary":"不调用远程模型的正式结局","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW}}
        }),
    )
    .unwrap();
    let before = {
        let handle = state.manager.open("proj-b14", "", NOW).unwrap();
        let model_runs: i64 = handle
            .query_row(
                "SELECT COUNT(*) FROM model_runs WHERE project_id='proj-b14'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let digests = EndgameRepository::domain_digests(&handle, "proj-b14").unwrap();
        drop(handle);
        (model_runs, digests)
    };
    query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    query_endgame_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    execute_command_impl(
        &state,
        json!({"type":"complete_book","projectId":"proj-b14","commandId":"b14-no-remote-complete","idempotencyKey":"b14-no-remote-complete:key","expectedRevision":1,"requestedAt":NOW}),
    )
    .unwrap();
    let after = {
        let handle = state.manager.open("proj-b14", "", NOW).unwrap();
        let model_runs: i64 = handle
            .query_row(
                "SELECT COUNT(*) FROM model_runs WHERE project_id='proj-b14'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let digests = EndgameRepository::domain_digests(&handle, "proj-b14").unwrap();
        drop(handle);
        (model_runs, digests)
    };
    assert_eq!(before.0, after.0);
    for key in [
        "canonDigest",
        "knowledgeDigest",
        "continuityDigest",
        "reservationDigest",
        "obligationDigest",
        "taskRealityDigest",
        "finalChapterInventoryDigest",
    ] {
        assert_eq!(
            before.1[key], after.1[key],
            "domain digest changed for {key}"
        );
    }
    println!("B14_R3_NO_REMOTE_CORE|modelRunsDelta=0|domainDigestsUnchanged=true");
}

#[test]
fn b14_domain_digests_use_independent_authoritative_sources() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    let before = {
        let handle = state.manager.open("proj-b14", "", NOW).unwrap();
        let value = EndgameRepository::domain_digests(&handle, "proj-b14").unwrap();
        drop(handle);
        value
    };
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    handle
        .execute(
            "INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,source_authority,created_at,updated_at) VALUES ('foreshadow:b14:digest','proj-b14','独立摘要','PLANTED','B06',?1,?1)",
            [NOW],
        )
        .unwrap();
    drop(handle);
    let after = {
        let handle = state.manager.open("proj-b14", "", NOW).unwrap();
        let value = EndgameRepository::domain_digests(&handle, "proj-b14").unwrap();
        drop(handle);
        value
    };
    assert_ne!(before["continuityDigest"], after["continuityDigest"]);
    assert_eq!(before["obligationDigest"], after["obligationDigest"]);
}

#[test]
fn b14_r7_paged_sources_enter_continuity_and_change_digest() {
    let run_case = |source_class: &str, ordinal: usize| -> serde_json::Value {
        let (state, _root) = test_state();
        seed_ready_project(&state);
        let handle = state.manager.open("proj-b14", "", NOW).unwrap();
        let source_id = format!("{}:r7:{ordinal:03}", source_class.to_lowercase());
        if source_class == "Foreshadow" {
            for index in 1..=ordinal {
                handle
                    .execute(
                        "INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,source_authority,created_at,updated_at) VALUES (?1,'proj-b14',?2,'PLANTED','B06',?3,?3)",
                        libsql_rusqlite::params![
                            format!("foreshadow:r7:{index:03}"),
                            format!("R7伏笔 {index}"),
                            NOW
                        ],
                    )
                    .unwrap();
            }
        } else {
            for index in 1..=ordinal {
                handle
                    .execute(
                        "INSERT INTO mystery_states(mystery_id,project_id,mystery_type,status,created_at,updated_at) VALUES (?1,'proj-b14','R7谜团','OPEN',?2,?2)",
                        libsql_rusqlite::params![format!("mystery:r7:{index:03}"), NOW],
                    )
                    .unwrap();
            }
        }
        drop(handle);

        let before = state.manager.open("proj-b14", "", NOW).unwrap();
        let complete_read_count = if source_class == "Foreshadow" {
            NarrativeRepository::active_foreshadows_complete(&before, "proj-b14")
                .unwrap()
                .len()
        } else {
            NarrativeRepository::open_mysteries_complete(&before, "proj-b14")
                .unwrap()
                .len()
        };
        let before_threads =
            NarrativeRepository::continuity_threads_complete(&before, "proj-b14").unwrap();
        let before_thread = before_threads
            .iter()
            .find(|thread| thread.thread_id == format!("thread:{source_id}"));
        assert!(
            before_thread.is_some(),
            "PAGED_SOURCE_{ordinal}_MISSING_FROM_CONTINUITY"
        );
        let before_thread = before_thread.unwrap();
        let source_ref = before_thread
            .source_refs
            .first()
            .map(|source| source.ref_.clone())
            .unwrap();
        let continuity_digest_with_source = EndgameRepository::domain_digests(&before, "proj-b14")
            .unwrap()["continuityDigest"]
            .clone();
        assert_eq!(complete_read_count, ordinal);
        assert_eq!(source_ref, source_id);
        assert_eq!(before_thread.authority_level, "C");
        assert_eq!(before_thread.logical_status, "OPEN");
        assert!(
            before_threads
                .iter()
                .any(|thread| thread.thread_id == format!("thread:{source_id}")),
            "PAGED_SOURCE_{ordinal}_MISSING_FROM_CONTINUITY"
        );
        drop(before);

        let after = state.manager.open("proj-b14", "", NOW).unwrap();
        if source_class == "Foreshadow" {
            after
                .execute(
                    "UPDATE foreshadows SET status='RECOVERED' WHERE project_id='proj-b14' AND foreshadow_id=?1",
                    [source_id.as_str()],
                )
                .unwrap();
        } else {
            after
                .execute(
                    "UPDATE mystery_states SET status='RESOLVED' WHERE project_id='proj-b14' AND mystery_id=?1",
                    [source_id.as_str()],
                )
                .unwrap();
        }
        let after_threads =
            NarrativeRepository::continuity_threads_complete(&after, "proj-b14").unwrap();
        let continuity_digest_without_source =
            EndgameRepository::domain_digests(&after, "proj-b14").unwrap()["continuityDigest"]
                .clone();
        let present_after_removal = after_threads
            .iter()
            .any(|thread| thread.thread_id == format!("thread:{source_id}"));
        assert!(!present_after_removal);
        assert_ne!(
            continuity_digest_with_source, continuity_digest_without_source,
            "PAGED_SOURCE_{ordinal}_DID_NOT_CHANGE_CONTINUITY_DIGEST"
        );
        json!({
            "sourceClass": source_class,
            "sourceOrdinal": ordinal,
            "sourceRef": source_ref,
            "threadRef": before_thread.thread_id,
            "threadAuthority": before_thread.authority_level,
            "threadStatus": before_thread.logical_status,
            "completeReadCount": complete_read_count,
            "projectionThreadCount": before_threads.len(),
            "presentInProjection": true,
            "continuityDigestWithSource": continuity_digest_with_source,
            "continuityDigestWithoutSource": continuity_digest_without_source,
            "digestChanges": true,
            "presentAfterSourceRemoval": present_after_removal,
            "status": "PASS"
        })
    };

    let case101 = run_case("Foreshadow", 101);
    let case501 = run_case("Mystery", 501);
    assert_eq!(case101["sourceOrdinal"], 101);
    assert_eq!(case501["sourceOrdinal"], 501);
    println!(
        "B14_R7_CONTINUITY_PAGED_SOURCE|{}",
        serde_json::to_string(&json!({"case101":case101,"case501":case501})).unwrap()
    );
}

#[test]
fn b14_r7_hard_continuity_responsibility_blocks_completion_independently() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    insert_obligation(&handle, "obligation:r7:hard", "A", "PROGRESSING");
    drop(handle);
    execute_command_impl(
        &state,
        json!({
            "type":"endgame_contract_create",
            "projectId":"proj-b14",
            "commandId":"r7-hard-contract",
            "idempotencyKey":"r7-hard-contract:key",
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{"requestedEnding":{"summary":"R7 HARD responsibility","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW}}
        }),
    )
    .unwrap();
    let status = query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    let thread = NarrativeRepository::continuity_threads_complete(&handle, "proj-b14")
        .unwrap()
        .into_iter()
        .find(|thread| thread.thread_id == "thread:obligation:r7:hard")
        .unwrap();
    assert_eq!(status["status"], "BLOCKED");
    assert_eq!(thread.authority_level, "A");
    assert!(matches!(
        thread.logical_status.as_str(),
        "OPEN" | "SUSPENDED"
    ));
    println!(
        "B14_R7_HARD_CONTINUITY|{}",
        serde_json::to_string(&json!({
            "hardResponsibilityRef":"obligation:r7:hard",
            "threadRef":thread.thread_id,
            "authority":thread.authority_level,
            "status":thread.logical_status,
            "completionBlocked":status["status"] == "BLOCKED"
        }))
        .unwrap()
    );
}

#[test]
fn b14_r7_one_fixture_keeps_all_11_domains_unchanged_across_queries_and_completion() {
    let (state, _root) = test_state();
    seed_ready_project(&state);
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    handle.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:r7','proj-b14','R7人物','PERSON',?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,source_artifact_id,source_version,authority_level,created_at,updated_at) VALUES ('fact:r7','proj-b14','entity:r7','状态','{}','book-plan:b14',1,'A',?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO canon_knowledge(knowledge_id,project_id,knower_id,fact_id,epistemic_state,visibility_scope,created_at,updated_at) VALUES ('knowledge:r7','proj-b14','entity:r7','fact:r7','KNOWS','CHAPTER',?1,?1)", [NOW]).unwrap();
    insert_obligation(&handle, "obligation:r7:resolved", "C", "RESOLVED");
    handle.execute("INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,source_authority,created_at,updated_at) VALUES ('foreshadow:r7:resolved','proj-b14','R7伏笔','RECOVERED','B06',?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO reader_desires(desire_id,project_id,desire_type,status,payload_json,created_at,updated_at) VALUES ('desire:r7','proj-b14','谜底','PAID','{}',?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES ('task:r7','proj-b14','volume.task.progress',?1,'{}',?2,?2)", libsql_rusqlite::params![json!({"taskId":"task:r7","status":"SATISFIED"}).to_string(), NOW]).unwrap();
    handle.execute("INSERT INTO change_requests(change_request_id,project_id,change_type,scope,summary,status,impact_seed_json,created_at,updated_at) VALUES ('change:r7','proj-b14','REWRITE','CHAPTER','R7变更','APPLIED','{}',?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO retcon_plans(retcon_plan_id,project_id,target_fact_json,reason,fix_type,downstream_impact_json,estimated_cost_cents,status,created_at,updated_at) VALUES ('retcon:r7','proj-b14','{}','R7修订','SUPPLEMENT','{}',0,'APPLIED',?1,?1)", [NOW]).unwrap();
    insert_committed_artifact(
        &handle,
        "proj-b14",
        "volume-plan:r7",
        "VolumePlan",
        "VOLUME",
        r#"{"level":"VOLUME","volumeId":"volume:b14"}"#,
        &json!({"payloadSchema":"volume-plan.r1","tasks":[]}),
    );
    insert_committed_artifact_with_command(
        &handle,
        "proj-b14",
        "final:r7",
        "FinalChapter",
        "CHAPTER",
        r#"{"level":"CHAPTER","chapterId":"chapter:r7"}"#,
        &json!({"payloadSchema":"final-chapter.r1","chapterId":"chapter:r7","body":"R7正文"}),
        "r7-final-command",
    );
    handle.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,status,word_count,final_artifact_id,final_version,created_at,updated_at) VALUES ('chapter:r7','proj-b14','volume:b14',1,'R7章','COMMITTED',3,'final:r7',1,?1,?1)", [NOW]).unwrap();
    handle.execute("INSERT INTO commands(project_id,command_id,idempotency_key,request_digest,status,created_at) VALUES ('proj-b14','r7-final-command','r7-final-command:key',?1,'COMMITTED',?2)", libsql_rusqlite::params!["a".repeat(64), NOW]).unwrap();
    handle.execute("INSERT INTO commit_receipts(receipt_id,project_id,commit_id,command_id,idempotency_key,before_revision,after_revision,bundle_digest,receipt_json,receipt_digest,committed_at) VALUES ('receipt:r7:final','proj-b14','commit:r7:final','r7-final-command','r7-final-command:key',0,1,?1,'{}',?2,?3)", libsql_rusqlite::params!["a".repeat(64), "b".repeat(64), NOW]).unwrap();
    handle.execute("INSERT INTO canon_evidence(evidence_id,project_id,fact_id,final_chapter_id,source_artifact_id,source_version,evidence_type,created_at) VALUES ('evidence:r7','proj-b14','fact:r7','chapter:r7','final:r7',1,'CANON',?1)", [NOW]).unwrap();
    drop(handle);

    execute_command_impl(
        &state,
        json!({
            "type":"endgame_contract_create",
            "projectId":"proj-b14",
            "commandId":"r7-no-authority-contract",
            "idempotencyKey":"r7-no-authority-contract:key",
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{"requestedEnding":{"summary":"R7 no-authority fixture","confirmationSource":"USER_CONFIRMED","confirmedAt":NOW}}
        }),
    )
    .unwrap();
    let handle = state.manager.open("proj-b14", "", NOW).unwrap();
    let before = r7_domain_snapshot(&handle);
    assert_eq!(before.len(), 11);
    for domain in &before {
        assert!(
            domain["count"].as_u64().unwrap_or(0) > 0,
            "R7 empty authority domain: {domain}"
        );
    }
    drop(handle);

    query_endgame_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let after_endgame_query = r7_domain_snapshot(&state.manager.open("proj-b14", "", NOW).unwrap());
    assert_r7_domains_unchanged(&before, &after_endgame_query, "endgame query");

    query_completion_status_impl(&state, json!({"projectId":"proj-b14"})).unwrap();
    let after_completion_query =
        r7_domain_snapshot(&state.manager.open("proj-b14", "", NOW).unwrap());
    assert_r7_domains_unchanged(&before, &after_completion_query, "completion query");
    let completion = execute_command_impl(
        &state,
        json!({
            "type":"complete_book",
            "projectId":"proj-b14",
            "commandId":"r7-no-authority-complete",
            "idempotencyKey":"r7-no-authority-complete:key",
            "expectedRevision":1,
            "requestedAt":NOW,
            "payload":{}
        }),
    )
    .unwrap();
    assert_eq!(completion["finalStatus"], "committed");
    let after_complete_book = r7_domain_snapshot(&state.manager.open("proj-b14", "", NOW).unwrap());
    assert_r7_domains_unchanged(&before, &after_complete_book, "complete book");
    println!(
        "B14_R7_NO_AUTHORITY_11_DOMAIN|{}",
        serde_json::to_string(&json!({
            "fixtureProject":"proj-b14",
            "domainCount":11,
            "oneFixture":true,
            "before":before,
            "afterEndgameQuery":after_endgame_query,
            "afterCompletionQuery":after_completion_query,
            "afterCompleteBook":after_complete_book,
            "completeBookSucceeded":true,
            "allowedMutationDomains":["B14 control artifacts","Endgame/Completion control state","BookExitState","BookCompletionReport","completion receipt / current completion refs"],
            "forbiddenMutationDomains":["Canon","Knowledge","Narrative Obligation","Foreshadow","ReaderDesire","Continuity","Planning","Task Reality","ChangeRequest","RetconPlan","FinalChapter"],
            "status":"PASS"
        }))
        .unwrap()
    );
}
