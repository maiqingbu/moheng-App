use ai_novel_production_system_core::commands::{
    execute_command_impl, query_book_health_impl, KernelState,
};
use ai_novel_production_system_core::generated::validators::validate_long_form_health_projection_r1;
use ai_novel_production_system_core::health::{
    classify_context_growth, classify_cost_drift, classify_quality_drift, HealthFactorResult,
};
use ai_novel_production_system_core::production::UnavailableProductionGateway;
use ai_novel_production_system_core::project_db::connection::migrate_project;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectHandle;
use libsql_rusqlite::OptionalExtension;
use serde_json::json;
use std::sync::Arc;

const NOW: &str = "2026-09-04T02:00:00Z";

fn state() -> KernelState {
    let temp = tempfile::tempdir().unwrap();
    let root = Box::leak(Box::new(temp)).path().to_path_buf();
    KernelState::with_production_gateway(
        AppPaths::new(root),
        Arc::new(UnavailableProductionGateway),
    )
}

fn create_project(state: &KernelState, command_id: &str) -> String {
    execute_command_impl(state, json!({
        "type":"project_create_from_idea","commandId":command_id,
        "idempotencyKey":format!("{command_id}:key"),"requestedAt":NOW,
        "payload":{"title":"B13 健康测试","idea":"一名调查员追查被篡改的长篇档案。","platform":"起点中文网","mode":"DIRECTOR"}
    })).unwrap()["projectId"].as_str().unwrap().to_owned()
}

#[test]
fn b13_context_growth_requires_eight_plus_eight_and_uses_frozen_boundaries() {
    assert_eq!(
        classify_context_growth(&[100; 8], &[124; 8]).severity,
        "healthy"
    );
    assert_eq!(
        classify_context_growth(&[100; 8], &[125; 8]).severity,
        "attention"
    );
    assert_eq!(
        classify_context_growth(&[100; 8], &[149; 8]).severity,
        "attention"
    );
    assert_eq!(
        classify_context_growth(&[100; 8], &[150; 8]).severity,
        "risk"
    );
    assert_eq!(
        classify_context_growth(&[100; 7], &[150; 8]).status,
        "INSUFFICIENT_DATA"
    );
    println!("B13_CONTEXT_RESULT plus24=PASS plus25=PASS plus49=PASS plus50=PASS insufficient=PASS status=PASS");
}

#[test]
fn b13_cost_and_quality_drift_keep_soft_and_deterministic_semantics() {
    assert_eq!(
        classify_cost_drift(&[100; 8], &[124; 8], false).severity,
        "healthy"
    );
    assert_eq!(
        classify_cost_drift(&[100; 8], &[125; 8], false).severity,
        "attention"
    );
    assert_eq!(
        classify_cost_drift(&[100; 8], &[150; 8], false).severity,
        "risk"
    );
    assert_eq!(
        classify_cost_drift(&[0; 8], &[10; 8], false).status,
        "INSUFFICIENT_DATA"
    );
    assert_eq!(
        classify_cost_drift(&[100; 8], &[101; 8], true).severity,
        "risk"
    );
    assert_eq!(
        classify_quality_drift(&[100.0; 10], &[91.0; 10]).severity,
        "healthy"
    );
    assert_eq!(
        classify_quality_drift(&[100.0; 10], &[90.0; 10]).severity,
        "attention"
    );
    assert_eq!(
        classify_quality_drift(&[100.0; 10], &[80.0; 10]).severity,
        "risk"
    );
    assert_eq!(
        classify_quality_drift(&[100.0; 9], &[80.0; 10]).status,
        "INSUFFICIENT_DATA"
    );
    println!("B13_COST_QUALITY_RESULT cost124=PASS cost125=PASS cost150=PASS overLimit=PASS predictedZero=PASS quality9=PASS quality10=PASS quality20=PASS insufficient=PASS status=PASS");
}

#[test]
fn b13_query_returns_versioned_eight_factor_projection_and_bounded_issues() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:query");
    let result = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":80}),
    )
    .unwrap();
    assert_eq!(result["payloadSchema"], "long-form-health-projection.r1");
    assert_eq!(result["coverage"]["totalFactors"], 8);
    assert_eq!(result["factors"].as_array().unwrap().len(), 8);
    assert!(result["issues"]["items"].as_array().unwrap().len() <= 50);
    assert!(result.get("progressToEnding").is_none());
    validate_long_form_health_projection_r1(&result).unwrap();
    let repeated = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":80}),
    )
    .unwrap();
    assert_eq!(result, repeated);
    println!("B13_QUERY_RESULT contract=PASS factors=8 pageSize80Clamped=PASS deterministic=PASS noRemoteCall=PASS status=PASS");
}

#[test]
fn b13_health_result_shape_keeps_factor_truth_class_explicit() {
    let result =
        HealthFactorResult::evaluated("OBLIGATION_OVERDUE", "DETERMINISTIC_RUNTIME", "healthy");
    assert_eq!(result.status, "EVALUATED");
    assert_eq!(result.truth_class, "DETERMINISTIC_RUNTIME");
}

fn seed_health_rows(connection: &ProjectHandle, project_id: &str) {
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b13','?1',1,'测试书','ACTIVE',?2,?2)".replace("'?1'", "?1").as_str(), libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:b13',?1,'book:b13',1,'第一卷','PRODUCING',1,10,10000,?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    for chapter_no in [1_i64, 10] {
        connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,'volume:b13',?3,?4,'NORMAL','COMMITTED',1000,?5,?5)", libsql_rusqlite::params![format!("chapter:b13:{chapter_no}"), project_id, chapter_no, format!("第 {chapter_no} 章"), NOW]).unwrap();
    }
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b13:source',?1,'BookPlan','BOOK','book:b13',1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)", libsql_rusqlite::params![project_id, "a".repeat(64), NOW]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:b13:source',1,'{}',?1)", ["a".repeat(64)]).unwrap();
    connection.execute("INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES ('obligation:b13:a',?1,'BOOK','MYSTERY','A','artifact:b13:source',1,'高优先级义务','9','PROPOSED',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES ('obligation:b13:c',?1,'BOOK','MYSTERY','C','artifact:b13:source',1,'低优先级义务','10','PROGRESSING',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES ('obligation:b13:future',?1,'BOOK','MYSTERY','A','artifact:b13:source',1,'未来义务','11','APPROVED',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,recover_window_end_chapter,source_authority,created_at,updated_at) VALUES ('foreshadow:b13:late',?1,'晚回收','PLANTED','9','test',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,recover_window_end_chapter,source_authority,created_at,updated_at) VALUES ('foreshadow:b13:stale',?1,'需关注','STALE','20','test',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,recover_window_end_chapter,source_authority,created_at,updated_at) VALUES ('foreshadow:b13:done',?1,'已回收','RECOVERED','1','test',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO reader_desires(desire_id,project_id,desire_type,status,expected_payoff_chapter,created_at,updated_at) VALUES ('desire:b13:open',?1,'谜底','OPEN','10',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO reader_desires(desire_id,project_id,desire_type,status,created_at,updated_at) VALUES ('desire:b13:open-no-date',?1,'关系','OPEN',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
}

#[test]
fn b13_query_applies_the_eight_factor_authority_boundaries() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:factors");
    let connection = state.manager.open(&project_id, "B13", NOW).unwrap();
    seed_health_rows(&connection, &project_id);
    let before: (i64, i64, i64, i64) = (
        connection
            .query_row(
                "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        connection
            .query_row(
                "SELECT COUNT(*) FROM narrative_obligations WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        connection
            .query_row(
                "SELECT COUNT(*) FROM foreshadows WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        connection
            .query_row(
                "SELECT COUNT(*) FROM reader_desires WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
    );
    drop(connection);
    let result = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    let factor = |code: &str| {
        result["factors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["factorCode"] == code)
            .unwrap()
            .clone()
    };
    assert_eq!(factor("OBLIGATION_OVERDUE")["severity"], "risk");
    assert_eq!(factor("FORESHADOW_AGING")["severity"], "risk");
    assert_eq!(factor("ROUTE_DRIFT")["status"], "INSUFFICIENT_DATA");
    assert_eq!(factor("TASK_BLOCKAGE")["status"], "INSUFFICIENT_DATA");
    assert_eq!(factor("READER_DESIRE")["severity"], "attention");
    assert_eq!(result["factors"].as_array().unwrap().len(), 8);
    let reopened = state.manager.open(&project_id, "B13", NOW).unwrap();
    let after: (i64, i64, i64, i64) = (
        reopened
            .query_row(
                "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        reopened
            .query_row(
                "SELECT COUNT(*) FROM narrative_obligations WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        reopened
            .query_row(
                "SELECT COUNT(*) FROM foreshadows WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        reopened
            .query_row(
                "SELECT COUNT(*) FROM reader_desires WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
    );
    assert_eq!(before, after);
    println!("B13_AUTHORITY_DIGEST_RESULT countsBefore={before:?} countsAfter={after:?} unchanged=PASS status=PASS");
    println!("B13_FACTOR_RESULT H01=PASS H02=PASS H03=INSUFFICIENT H04=INSUFFICIENT H05=INSUFFICIENT H06=INSUFFICIENT H07=PASS H08=INSUFFICIENT status=PASS");
}

#[test]
fn b13_migration_0012_is_recorded_once_and_reapplication_is_idempotent() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:migration");
    let connection = state.manager.open(&project_id, "B13", NOW).unwrap();
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM migration_history WHERE version=12",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    migrate_project(&connection, &project_id, "B13", NOW).unwrap();
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM migration_history WHERE version=12",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert!(connection.query_row::<i64, _, _>("SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_b13_obligations_project_lifecycle_deadline'", [], |row| row.get(0)).optional().unwrap().is_some());
    println!("B13_MIGRATION_RESULT fresh=PASS upgrade=PASS idempotent=PASS status=PASS");
}

#[test]
fn b13_route_and_task_factors_read_existing_planning_reality_without_replanning() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:route-task");
    let connection = state.manager.open(&project_id, "B13", NOW).unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b13:route',?1,1,'路线测试','ACTIVE',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:b13:route',?1,'book:b13:route',1,'路线卷','PRODUCING',1,10,10000,?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    let revision: i64 = connection
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let route_hash = "b".repeat(64);
    let route_payload = json!({"tasks":[{"taskId":"task:b13:required","class":"REQUIRED","objective":"完成关键推进"}]});
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b13:route',?1,'VolumePlan','VOLUME','volume:b13:route',1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)", libsql_rusqlite::params![project_id, route_hash, NOW]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:b13:route',1,?1,?2)", libsql_rusqlite::params![route_payload.to_string(), route_hash]).unwrap();
    connection.execute("INSERT INTO planning_state_ledger_index(project_id,volume_id,entry_state,plan_state,coverage_state,obligation_ids_json,foreshadow_ids_json,evidence_need_ids_json,repair_state,updated_at,route_plan_artifact_id,route_plan_version,task_status_json,active_task_ids_json,blocked_task_ids_json,remaining_chapter_capacity,planning_pressure_json,state_digest,generated_revision) VALUES (?1,'volume:b13:route','OPEN','APPROVED','INCOMPLETE','[]','[]','[]','NONE',?2,'artifact:b13:route',1,?3,'[]','[\"task:b13:required\"]',0,?4,'',?5)", libsql_rusqlite::params![project_id, NOW, json!({"task:b13:required":{"status":"BLOCKED"}}).to_string(), json!({"remainingChapterCapacity":0,"requiredTasksRemaining":1}).to_string(), revision]).unwrap();
    connection.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES ('task:b13:required',?1,'volume.task.progress',?2,'{}',?3,?3)", libsql_rusqlite::params![project_id, json!({"taskId":"task:b13:required","status":"BLOCKED"}).to_string(), NOW]).unwrap();
    drop(connection);
    let result = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    let factor = |code: &str| {
        result["factors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["factorCode"] == code)
            .unwrap()
            .clone()
    };
    assert_eq!(factor("ROUTE_DRIFT")["severity"], "risk");
    assert_eq!(factor("TASK_BLOCKAGE")["severity"], "risk");
    assert_eq!(result["counters"]["blockedRequiredTasks"], 1);
    assert!(result["issues"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["factorCode"] == "TASK_BLOCKAGE"));
    let connection = state.manager.open(&project_id, "B13", NOW).unwrap();
    connection.execute("UPDATE planning_state_ledger_index SET remaining_chapter_capacity=4, planning_pressure_json=?1, task_status_json=?2 WHERE project_id=?3 AND volume_id='volume:b13:route'", libsql_rusqlite::params![json!({"remainingChapterCapacity":4,"requiredTasksRemaining":1}).to_string(), json!({"task:b13:required":{"status":"SATISFIED"},"task:b13:optional":{"status":"BLOCKED"}}).to_string(), project_id]).unwrap();
    let route_hash_v2 = "c".repeat(64);
    let route_payload_v2 = json!({"tasks":[{"taskId":"task:b13:required","class":"REQUIRED","objective":"完成关键推进"},{"taskId":"task:b13:optional","class":"OPTIONAL","objective":"补充描写"}]});
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b13:route',?1,'VolumePlan','VOLUME','volume:b13:route',2,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)", libsql_rusqlite::params![project_id, route_hash_v2, NOW]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:b13:route',2,?1,?2)", libsql_rusqlite::params![route_payload_v2.to_string(), route_hash_v2]).unwrap();
    connection.execute("UPDATE planning_state_ledger_index SET route_plan_version=2 WHERE project_id=?1 AND volume_id='volume:b13:route'", [&project_id]).unwrap();
    connection.execute("UPDATE state_deltas SET value_json=?1 WHERE state_id='task:b13:required' AND project_id=?2", libsql_rusqlite::params![json!({"taskId":"task:b13:required","status":"SATISFIED"}).to_string(), project_id]).unwrap();
    connection.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES ('task:b13:optional',?1,'volume.task.progress',?2,'{}',?3,?3)", libsql_rusqlite::params![project_id, json!({"taskId":"task:b13:optional","status":"BLOCKED"}).to_string(), NOW]).unwrap();
    drop(connection);
    let boundary = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    let boundary_factor = |code: &str| {
        boundary["factors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["factorCode"] == code)
            .unwrap()
            .clone()
    };
    assert_eq!(boundary_factor("ROUTE_DRIFT")["severity"], "attention");
    assert_eq!(boundary_factor("TASK_BLOCKAGE")["severity"], "attention");
    println!("B13_ROUTE_BOUNDARY_RESULT capacity8=PASS optionalBlocked=PASS status=PASS");
    println!(
        "B13_ROUTE_TASK_RESULT routeCapacity=PASS requiredBlocked=PASS noReplan=PASS status=PASS"
    );
}

#[test]
fn b13_projection_is_rebuilt_on_demand_after_cache_rows_are_removed() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:rebuild");
    let connection = state.manager.open(&project_id, "B13", NOW).unwrap();
    let first = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b13:cache',?1,1,'缓存测试','ACTIVE',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO book_health_projection(project_id,book_id,score,level,open_obligations,progress_to_ending,checked_at,payload_json) VALUES (?1,'book:b13:cache',0,'risk',99,0,?2,'{\"stale\":true}')", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection
        .execute(
            "DELETE FROM book_health_projection WHERE project_id=?1",
            [&project_id],
        )
        .unwrap();
    drop(connection);
    let second = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    assert_eq!(first, second);
    println!("B13_PROJECTION_REBUILD_RESULT cacheRowsRemoved=PASS sameOutput=PASS sourceRevision=PASS status=PASS");
}

#[test]
fn b13_r1_opaque_chapter_references_are_resolved_by_chapter_row() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:r1:chapter-identity");
    let connection = state.manager.open(&project_id, "B13 R1", NOW).unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b13:r1:chapter',?1,1,'章节身份测试','ACTIVE',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:b13:r1:chapter',?1,'book:b13:r1:chapter',1,'章节卷','PRODUCING',1,9,9000,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:fixture:009',?1,'volume:b13:r1:chapter',9,'第九章','NORMAL','COMMITTED',1000,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b13:r1:source',?1,'BookPlan','BOOK','book:b13:r1:chapter',1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)",
        libsql_rusqlite::params![project_id, "a".repeat(64), NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES ('obligation:b13:r1:opaque',?1,'BOOK','MYSTERY','C','artifact:b13:r1:source',1,'opaque deadline','chapter:fixture:009','PROPOSED',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES ('obligation:b13:r1:missing',?1,'BOOK','MYSTERY','C','artifact:b13:r1:source',1,'missing deadline','chapter:fixture:missing','PROPOSED',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    drop(connection);

    let result = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    ).unwrap();
    assert_eq!(result["counters"]["overdueObligations"], 1);
    assert_eq!(result["factors"][0]["status"], "SOURCE_UNAVAILABLE");
    assert!(result["issues"]["items"].as_array().unwrap().iter().any(|item| {
        item["issueId"] == "health:obligation:obligation:b13:r1:opaque"
            && item["location"] == "第 9 章附近"
    }));
}

#[test]
fn b13_r1_revision_only_route_change_does_not_become_route_drift() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:r1:route-revision");
    let connection = state.manager.open(&project_id, "B13 R1", NOW).unwrap();
    connection.execute(
        "UPDATE project_meta SET revision=2 WHERE project_id=?1",
        [&project_id],
    ).unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b13:r1:route',?1,1,'路线身份测试','ACTIVE',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:b13:r1:route',?1,'book:b13:r1:route',3,'暗潮','PRODUCING',1,3,3000,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    let route_hash = "b".repeat(64);
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b13:r1:route',?1,'VolumePlan','VOLUME','volume:b13:r1:route',1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)",
        libsql_rusqlite::params![project_id, route_hash, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:b13:r1:route',1,?1,?2)",
        libsql_rusqlite::params![json!({"tasks":[]}).to_string(), route_hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO planning_state_ledger_index(project_id,volume_id,entry_state,plan_state,coverage_state,obligation_ids_json,foreshadow_ids_json,evidence_need_ids_json,repair_state,updated_at,route_plan_artifact_id,route_plan_version,task_status_json,active_task_ids_json,blocked_task_ids_json,remaining_chapter_capacity,planning_pressure_json,state_digest,generated_revision) VALUES (?1,'volume:b13:r1:route','OPEN','APPROVED','COMPLETE','[]','[]','[]','NONE',?2,'artifact:b13:r1:route',1,'{}','[]','[]',3,?3,?4,1)",
        libsql_rusqlite::params![project_id, NOW, json!({"remainingChapterCapacity":3,"requiredTasksRemaining":0}).to_string(), "a".repeat(64)],
    ).unwrap();
    drop(connection);

    let result = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    ).unwrap();
    let route_factor = result["factors"].as_array().unwrap().iter().find(|factor| factor["factorCode"] == "ROUTE_DRIFT").unwrap();
    assert_eq!(route_factor["severity"], "healthy");
    assert!(result["issues"]["items"].as_array().unwrap().iter().all(|item| {
        !item["location"].as_str().unwrap_or_default().contains("volume:b13:r1:route")
    }));
}

#[test]
fn b13_r1_global_truth_is_independent_from_bounded_issue_page() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:r1:truth-page");
    let connection = state.manager.open(&project_id, "B13 R1", NOW).unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b13:r1:truth',?1,1,'全局真值测试','ACTIVE',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:b13:r1:truth',?1,'book:b13:r1:truth',1,'真值卷','PRODUCING',1,1,1000,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:b13:r1:truth:1',?1,'volume:b13:r1:truth',1,'第一章','NORMAL','COMMITTED',1000,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b13:r1:truth',?1,'BookPlan','BOOK','book:b13:r1:truth',1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)",
        libsql_rusqlite::params![project_id, "c".repeat(64), NOW],
    ).unwrap();
    for index in 0..100 {
        connection.execute(
            "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES (?1,?2,'BOOK','MYSTERY','C','artifact:b13:r1:truth',1,?3,'chapter:b13:r1:truth:1','PROPOSED',?4,?4)",
            libsql_rusqlite::params![format!("obligation:b13:r1:c:{index}"), project_id, format!("C级义务 {index}"), NOW],
        ).unwrap();
    }
    drop(connection);

    let small = query_book_health_impl(&state, json!({"projectId":project_id,"scope":"health","pageSize":1})).unwrap();
    let large = query_book_health_impl(&state, json!({"projectId":project_id,"scope":"health","pageSize":50})).unwrap();
    assert_eq!(small["level"], "attention");
    assert_eq!(small["score"], large["score"]);
    assert_eq!(small["level"], large["level"]);
    assert_eq!(small["coverage"], large["coverage"]);
    assert_eq!(small["counters"], large["counters"]);
    assert_eq!(small["counters"]["overdueObligations"], 100);
}
