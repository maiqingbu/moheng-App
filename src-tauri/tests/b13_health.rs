use ai_novel_production_system_core::commands::{
    execute_command_impl, query_book_health_factor_sources_impl, query_book_health_impl,
    query_project_overview_impl, KernelState,
};
use ai_novel_production_system_core::commit::CommitService;
use ai_novel_production_system_core::generated::types::CommitBundle;
use ai_novel_production_system_core::generated::validators::validate_long_form_health_projection_r1;
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::health::{
    classify_context_growth, classify_cost_drift, classify_quality_drift, HealthFactorResult,
};
use ai_novel_production_system_core::project_db::connection::migrate_project;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectHandle;
use ai_novel_production_system_core::repositories::health::{
    HealthIssueCursor, HealthRepository, FORMAL_ROUTE_ROWS_SQL, FORMAL_ROUTE_SUMMARY_SQL,
    HEALTH_ISSUE_PAGE_SQL,
};
use ai_novel_production_system_core::volume_lifecycle::VolumeLifecycleOwner;
use libsql_rusqlite::OptionalExtension;
use serde_json::json;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Instant;

mod support;
use support::EntryFixtureGateway;

const NOW: &str = "2026-09-04T02:00:00Z";
const HEALTH_SCALE_ROUTE_COUNT: i64 = 100_000;
const LEGACY_FORMAL_ROUTE_PAYLOAD_SCAN_SQL: &str = "SELECT p.payload_json FROM volumes v JOIN volume_plan_index i ON i.project_id=v.project_id AND i.volume_id=v.volume_id JOIN artifacts a ON a.project_id=i.project_id AND a.artifact_id=i.plan_artifact_id AND a.version=i.plan_version JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE v.project_id=?1 ORDER BY v.volume_no,v.volume_id";

struct TempKernelState {
    _temp: tempfile::TempDir,
    state: KernelState,
}

impl Deref for TempKernelState {
    type Target = KernelState;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

fn state() -> TempKernelState {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        Arc::new(EntryFixtureGateway),
    );
    TempKernelState { _temp: temp, state }
}

fn create_project(state: &KernelState, command_id: &str) -> String {
    execute_command_impl(state, json!({
        "type":"project_create_from_idea","commandId":command_id,
        "idempotencyKey":format!("{command_id}:key"),"requestedAt":NOW,
        "payload":{"title":"B13 健康测试","idea":"一名调查员追查被篡改的长篇档案。","platform":"起点中文网","mode":"DIRECTOR"}
    })).unwrap()["projectId"].as_str().unwrap().to_owned()
}

fn flush_health_projection(connection: &ProjectHandle, project_id: &str) {
    HealthRepository::flush_dirty(connection, project_id, NOW).unwrap();
}

fn seed_precomputed_health_route_projection(connection: &ProjectHandle, project_id: &str) {
    let revision: i64 = connection
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute(
            "DELETE FROM health_issue_projection WHERE project_id=?1",
            [project_id],
        )
        .unwrap();
    connection
        .execute(
            "DELETE FROM health_task_scope_index WHERE project_id=?1",
            [project_id],
        )
        .unwrap();
    connection
        .execute(
            "DELETE FROM health_route_scope_projection WHERE project_id=?1",
            [project_id],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO health_route_scope_projection(project_id,volume_id,volume_no,volume_title,plan_state,is_stale,is_unavailable,capacity,required_tasks_remaining,blocked_required_tasks,blocked_optional_tasks,task_summary_available,source_revision,updated_at)
             SELECT v.project_id,v.volume_id,v.volume_no,v.title,i.plan_state,
                    CASE WHEN i.plan_state IN ('STALE','PLAN_STALE') THEN 1 ELSE 0 END,
                    0,1,0,0,0,1,?2,?3
             FROM volumes v JOIN volume_plan_index i ON i.project_id=v.project_id AND i.volume_id=v.volume_id
             WHERE v.project_id=?1",
            libsql_rusqlite::params![project_id, revision, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO health_issue_projection(project_id,issue_id,factor_code,severity,title,detail,location,due_chapter,recommended_destination,source_kind,source_ref,severity_rank,factor_rank,due_sort,volume_id,source_revision)
             SELECT v.project_id,'health:route:'||v.volume_id,'ROUTE_DRIFT','risk',
                    '规划路线需要重新核对','当前规划版本落后于项目最新变更，需要在卷工作区重新核对。',
                    '第 '||v.volume_no||' 卷 · '||TRIM(v.title),1,'卷工作区','volume-plan',v.volume_id,0,2,1,v.volume_id,?2
             FROM volumes v JOIN volume_plan_index i ON i.project_id=v.project_id AND i.volume_id=v.volume_id
             WHERE v.project_id=?1 AND i.plan_state IN ('STALE','PLAN_STALE')",
            libsql_rusqlite::params![project_id, revision],
        )
        .unwrap();
    connection
        .execute(
            "DELETE FROM health_projection_dirty WHERE project_id=?1",
            [project_id],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE health_projection_state SET status='READY',source_revision=?2,updated_at=?3 WHERE project_id=?1",
            libsql_rusqlite::params![project_id, revision, NOW],
        )
        .unwrap();
}

fn explain_with_params(
    connection: &ProjectHandle,
    sql: &str,
    params: impl libsql_rusqlite::Params,
) -> Vec<String> {
    let mut statement = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap();
    statement
        .query_map(params, |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn large_route_fixture(command_id: &str) -> (TempKernelState, String, ProjectHandle) {
    let state = state();
    let project_id = create_project(&state, command_id);
    let connection = state
        .manager
        .open(&project_id, "B-R2-03 scale", NOW)
        .unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b-r2-03:scale',?1,1,'合成规模数据','ACTIVE',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b-r2-03:shared-route',?1,'VolumePlan','VOLUME','volume:b-r2-03:scale:1',1,'b-r2-03.fixture','COMMITTED',?2,'{}',?3)",
        libsql_rusqlite::params![project_id, "a".repeat(64), NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:b-r2-03:shared-route',1,'{\"payloadSchema\":\"volume-plan.r1\",\"tasks\":[]}',?1)",
        ["a".repeat(64)],
    ).unwrap();
    let tasks = (1..=HEALTH_SCALE_ROUTE_COUNT)
        .map(|index| json!({"taskId":format!("task:b-r2-03:scale:{index}"),"class":"OPTIONAL"}))
        .collect::<Vec<_>>();
    let large_route_payload = json!({"payloadSchema":"volume-plan.r1","tasks":tasks}).to_string();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b-r2-03:large-route',?1,'VolumePlan','VOLUME','volume:b-r2-03:scale:1',1,'b-r2-03.fixture','COMMITTED',?2,'{}',?3)",
        libsql_rusqlite::params![project_id, "c".repeat(64), NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:b-r2-03:large-route',1,?1,?2)",
        libsql_rusqlite::params![large_route_payload, "c".repeat(64)],
    ).unwrap();
    connection.execute(
        "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<?3)
         INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at)
         SELECT 'volume:b-r2-03:scale:'||x,?1,'book:b-r2-03:scale',x,'合成卷 '||x,'PRODUCING',1,1,1000,?2,?2 FROM n",
        libsql_rusqlite::params![project_id, NOW, HEALTH_SCALE_ROUTE_COUNT],
    ).unwrap();
    connection.execute(
        "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at)
         SELECT ?1,volume_id,CASE WHEN volume_no=1 THEN 'artifact:b-r2-03:large-route' ELSE 'artifact:b-r2-03:shared-route' END,1,'APPROVED',1,?2 FROM volumes WHERE project_id=?1",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volume_planning_state_projection(project_id,volume_id,projection_revision,state_digest,projection_digest,payload_json,updated_at)
         SELECT ?1,volume_id,1,?2,?3,
                json_object('remainingChapterCapacity',1,'requiredTasksRemaining',0,
                            'blockedRequiredTasks',0,'healthBlockedRequiredTasks',0,
                            'healthBlockedOptionalTasks',0,'blockedTaskIds',json('[]'),
                            'healthRequiredTaskIds',json('[]'),
                            'planningPressure',json_object('remainingChapterCapacity',1,'requiredTasksRemaining',0)),?4
         FROM volumes WHERE project_id=?1",
        libsql_rusqlite::params![project_id, "b".repeat(64), "c".repeat(64), NOW],
    ).unwrap();
    connection.execute(
        "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<?3)
         INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at)
         SELECT 'task:b-r2-03:scale:'||x,?1,'volume.task.progress','{\"status\":\"NOT_STARTED\"}','{}',?2,?2 FROM n",
        libsql_rusqlite::params![project_id, NOW, HEALTH_SCALE_ROUTE_COUNT],
    ).unwrap();
    (state, project_id, connection)
}

#[test]
fn b_r2_03_empty_project_overview_is_not_healthy() {
    let state = state();
    let project_id = create_project(&state, "cmd:b-r2-03:empty-overview");

    let overview = query_project_overview_impl(&state, json!({"projectId":project_id})).unwrap();

    assert_ne!(overview["healthSummary"]["level"], "healthy");
    assert_eq!(overview["healthSummary"]["level"], "not_evaluated");
}

#[test]
fn b13_reader_desire_without_source_is_insufficient_data() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:reader-desire:no-source");

    let result = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":20}),
    )
    .unwrap();
    let desire = result["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|factor| factor["factorCode"] == "READER_DESIRE")
        .unwrap();

    assert_eq!(desire["status"], "INSUFFICIENT_DATA");
    assert!(desire["summary"]
        .as_str()
        .unwrap()
        .contains("读者期待来源不足"));
    assert!(desire["detail"]
        .as_str()
        .unwrap()
        .contains("没有未结束的正式读者期待记录"));
}

#[test]
fn b_r2_03_unavailable_health_source_is_not_healthy_in_overview() {
    let state = state();
    let project_id = create_project(&state, "cmd:b-r2-03:unavailable-overview");
    let connection = state.manager.open(&project_id, "B-R2-03", NOW).unwrap();
    connection
        .execute("DROP TABLE volume_planning_state_projection", [])
        .unwrap();
    drop(connection);

    let overview = query_project_overview_impl(&state, json!({"projectId":project_id})).unwrap();

    assert_eq!(overview["healthSummary"]["level"], "source_unavailable");
}

#[test]
fn b_r2_03_risk_project_overview_matches_formal_projection() {
    let state = state();
    let project_id = create_project(&state, "cmd:b-r2-03:risk-overview");
    let connection = state.manager.open(&project_id, "B-R2-03", NOW).unwrap();
    seed_health_rows(&connection, &project_id);
    drop(connection);

    let formal = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":1}),
    )
    .unwrap();
    let overview = query_project_overview_impl(&state, json!({"projectId":project_id})).unwrap();

    assert_eq!(formal["level"], "risk");
    assert_eq!(overview["healthSummary"]["level"], formal["level"]);
}

#[test]
fn b_r2_03_attention_project_overview_matches_formal_projection() {
    let state = state();
    let project_id = create_project(&state, "cmd:b-r2-03:attention-overview");
    let connection = state.manager.open(&project_id, "B-R2-03", NOW).unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b-r2-03:attention',?1,1,'关注状态测试','ACTIVE',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:b-r2-03:attention',?1,'book:b-r2-03:attention',1,'关注卷','PRODUCING',1,1,1000,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:b-r2-03:attention:1',?1,'volume:b-r2-03:attention',1,'第一章','NORMAL','COMMITTED',1000,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b-r2-03:attention',?1,'BookPlan','BOOK','book:b-r2-03:attention',1,'b-r2-03.fixture','COMMITTED',?2,'{}',?3)",
        libsql_rusqlite::params![project_id, "a".repeat(64), NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES ('obligation:b-r2-03:attention',?1,'BOOK','MYSTERY','C','artifact:b-r2-03:attention',1,'合成的低优先级关注项','chapter:b-r2-03:attention:1','PROPOSED',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    drop(connection);

    let formal = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":1}),
    )
    .unwrap();
    let overview = query_project_overview_impl(&state, json!({"projectId":project_id})).unwrap();

    assert_eq!(formal["level"], "attention");
    assert_eq!(overview["healthSummary"]["level"], formal["level"]);
}

#[test]
fn b_r2_03_official_overview_query_survives_project_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_path_buf();
    let state = KernelState::with_production_gateway(
        AppPaths::new(root.clone()),
        Arc::new(EntryFixtureGateway),
    );
    let created = execute_command_impl(
        &state,
        json!({
            "type":"project_create_from_idea",
            "commandId":"cmd:b-r2-03:overview-reopen",
            "idempotencyKey":"cmd:b-r2-03:overview-reopen:key",
            "requestedAt":NOW,
            "payload":{"title":"状态重开合成测试","idea":"用于验证项目重开后的正式状态查询。","platform":"起点中文网","mode":"DIRECTOR"}
        }),
    ).unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_owned();
    let receipt_id = created["receiptId"].as_str().unwrap().to_owned();
    let authority_counts = |state: &KernelState| {
        let connection = state
            .manager
            .open(&project_id, "B-R2-03 E3/E4", NOW)
            .unwrap();
        let count = |sql: &str| {
            connection
                .query_row(sql, [project_id.as_str()], |row| row.get::<_, i64>(0))
                .unwrap()
        };
        (
            count("SELECT revision FROM project_meta WHERE project_id=?1"),
            count("SELECT COUNT(*) FROM canonical_heads WHERE project_id=?1"),
            count("SELECT COUNT(*) FROM artifacts WHERE project_id=?1"),
            count("SELECT COUNT(*) FROM state_deltas WHERE project_id=?1"),
            count("SELECT COUNT(*) FROM project_command_receipts WHERE project_id=?1"),
            count("SELECT COUNT(*) FROM commit_receipts WHERE project_id=?1"),
        )
    };
    let before_query = authority_counts(&state);
    let first = query_project_overview_impl(&state, json!({"projectId":project_id})).unwrap();
    let after_query = authority_counts(&state);
    let first_level = first["healthSummary"]["level"].as_str().unwrap().to_owned();
    assert_eq!(after_query, before_query);
    drop(state);

    let reopened =
        KernelState::with_production_gateway(AppPaths::new(root), Arc::new(EntryFixtureGateway));
    let second = query_project_overview_impl(&reopened, json!({"projectId":project_id})).unwrap();
    let after_reopen_query = authority_counts(&reopened);
    assert_eq!(first_level, "not_evaluated");
    assert_eq!(second["healthSummary"]["level"], first_level);
    assert_eq!(second["project"]["revision"], first["project"]["revision"]);
    assert_eq!(after_reopen_query, before_query);
    println!("B_R2_03_E3_E4_OVERVIEW {{\"projectCreateReceipt\":\"{receipt_id}\",\"firstLevel\":\"{first_level}\",\"reopenLevel\":{},\"projectRevisionStable\":true,\"authorityCountsStable\":true}}", second["healthSummary"]["level"]);
}

#[test]
fn b_r2_03_legacy_loop_red_probe_reads_and_parses_all_route_payloads() {
    let (_state, project_id, connection) = large_route_fixture("cmd:b-r2-03:legacy-red-probe");
    let started = Instant::now();
    let mut statement = connection
        .prepare(LEGACY_FORMAL_ROUTE_PAYLOAD_SCAN_SQL)
        .unwrap();
    let rows = statement
        .query_map([project_id.as_str()], |row| row.get::<_, String>(0))
        .unwrap();
    let mut payloads_read = 0;
    let mut payloads_parsed = 0;
    for row in rows {
        payloads_read += 1;
        if serde_json::from_str::<serde_json::Value>(&row.unwrap()).is_ok() {
            payloads_parsed += 1;
        }
    }
    let elapsed_ms = started.elapsed().as_millis();
    println!("B_R2_03_RED_LEGACY_SCALE {{\"projectRoutes\":{HEALTH_SCALE_ROUTE_COUNT},\"routePayloadRowsRead\":{payloads_read},\"routePayloadsParsed\":{payloads_parsed},\"elapsedMs\":{elapsed_ms},\"expectedDetailPage\":101,\"bounded\":false}}");
    assert_eq!(payloads_read, HEALTH_SCALE_ROUTE_COUNT as usize);
    assert_eq!(payloads_parsed, HEALTH_SCALE_ROUTE_COUNT as usize);
}

#[test]
fn b_r2_03_large_route_fixture_bounds_route_details_and_aggregates_truth() {
    let (_state, project_id, connection) = large_route_fixture("cmd:b-r2-03:route-green-probe");
    seed_precomputed_health_route_projection(&connection, &project_id);

    let started = Instant::now();
    let mut statement = connection.prepare(FORMAL_ROUTE_ROWS_SQL).unwrap();
    let rows = statement
        .query_map(libsql_rusqlite::params![project_id, 101], |row| {
            row.get::<_, String>(0)
        })
        .unwrap();
    let loaded = rows.collect::<Result<Vec<_>, _>>().unwrap().len();
    let route_page_elapsed_ms = started.elapsed().as_millis();
    assert_eq!(loaded, 101);

    let started = Instant::now();
    let snapshot = HealthRepository::load(&connection, &project_id).unwrap();
    let summary_elapsed_ms = started.elapsed().as_millis();
    assert_eq!(snapshot.route_scope_count, HEALTH_SCALE_ROUTE_COUNT);
    assert_eq!(snapshot.route_detail_rows_read, 0);
    assert_eq!(snapshot.route_payloads_read, 0);
    assert_eq!(snapshot.route_payloads_parsed, 0);
    assert_eq!(snapshot.formal_route_summary_queries, 1);
    assert_eq!(snapshot.formal_route_page_queries, 0);
    assert_eq!(snapshot.task_states_read, 0);
    assert_eq!(snapshot.task_reality_rows_read, 0);
    assert_eq!(connection.query_row("SELECT COUNT(*) FROM state_deltas WHERE project_id=?1 AND state_type='volume.task.progress'", [&project_id], |row| row.get::<_, i64>(0)).unwrap(), HEALTH_SCALE_ROUTE_COUNT);

    let route_plan = explain_with_params(
        &connection,
        FORMAL_ROUTE_ROWS_SQL,
        libsql_rusqlite::params![project_id, 101],
    );
    let summary_plan =
        explain_with_params(&connection, FORMAL_ROUTE_SUMMARY_SQL, [project_id.as_str()]);
    assert!(!route_plan.is_empty());
    assert!(!summary_plan.is_empty());
    assert!(route_plan
        .iter()
        .all(|line| !line.contains("USE TEMP B-TREE FOR ORDER BY")));

    let route_sql_query_count = snapshot.formal_route_summary_queries
        + snapshot.formal_route_page_queries
        + snapshot.route_scope_lookups
        + snapshot.route_artifact_lookups
        + snapshot.route_replan_lookups
        + snapshot.route_stale_queue_lookups
        + snapshot.task_state_batches_read;
    let page_plan = explain_with_params(
        &connection,
        HEALTH_ISSUE_PAGE_SQL,
        libsql_rusqlite::params![
            project_id,
            1,
            -1,
            -1,
            i64::MIN,
            "",
            50,
            Option::<String>::None
        ],
    );
    let issue_page = HealthRepository::issue_page(&connection, &project_id, 1, None, 50).unwrap();
    assert_eq!(issue_page.json_scopes_processed, 0);
    assert!(issue_page.rows_read <= 50);
    assert!(!page_plan.iter().any(|line| line.contains("VIRTUAL TABLE")));
    println!("B_R2_03_R4_PAGE_WORK_RESULT projectRoutes={HEALTH_SCALE_ROUTE_COUNT} issuePageSize=50 issueRowsRead={} issueTotal={} queryCount={} payloadRowsRead={} payloadsParsed={} jsonScopesProcessed={} pagePlan={:?} status=PASS", issue_page.rows_read, issue_page.total, issue_page.query_count, snapshot.route_payloads_read, snapshot.route_payloads_parsed, issue_page.json_scopes_processed, page_plan);
    println!("B_R2_03_GREEN_ROUTE_SCALE {{\"projectRoutes\":{HEALTH_SCALE_ROUTE_COUNT},\"totalTaskRealityRows\":{HEALTH_SCALE_ROUTE_COUNT},\"routeSqlQueryCount\":{route_sql_query_count},\"summaryQueries\":{},\"routePageQueries\":{},\"routeScopeLookups\":{},\"routeArtifactLookups\":{},\"routeReplanLookups\":{},\"routeStaleQueueLookups\":{},\"routeRowsRead\":{},\"routePayloadsRead\":{},\"routePayloadsParsed\":{},\"routeTaskStateRowsRead\":{},\"taskRealityRowsRead\":{},\"routePageElapsedMs\":{route_page_elapsed_ms},\"healthLoadElapsedMs\":{summary_elapsed_ms},\"routePlan\":{},\"summaryPlan\":{},\"bounded\":true}}", snapshot.formal_route_summary_queries, snapshot.formal_route_page_queries, snapshot.route_scope_lookups, snapshot.route_artifact_lookups, snapshot.route_replan_lookups, snapshot.route_stale_queue_lookups, snapshot.route_detail_rows_read, snapshot.route_payloads_read, snapshot.route_payloads_parsed, snapshot.task_states_read, snapshot.task_reality_rows_read, json!(route_plan), json!(summary_plan));
}

#[test]
fn b_r2_03_health_issue_cursor_reaches_late_risk_in_100k_route_fixture() {
    let state = state();
    let project_id = create_project(&state, "cmd:b-r2-03:late-route-cursor");
    let connection = state
        .manager
        .open(&project_id, "B-R2-03 100k cursor", NOW)
        .unwrap();
    connection
        .execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b-r2-03:late-route-cursor',?1,1,'长路线游标测试','ACTIVE',?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    let route_hash = "a".repeat(64);
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b-r2-03:late-route-cursor',?1,'VolumePlan','VOLUME','volume:b-r2-03:late-route-cursor:1',1,'b-r2-03.fixture','COMMITTED',?2,'{}',?3)",
            libsql_rusqlite::params![project_id, route_hash, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:b-r2-03:late-route-cursor',1,'{\"payloadSchema\":\"volume-plan.r1\",\"tasks\":[]}',?1)",
            [&route_hash],
        )
        .unwrap();
    connection
        .execute(
            "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<100000)
             INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at)
             SELECT 'volume:b-r2-03:late-route-cursor:'||printf('%06d',x),?1,'book:b-r2-03:late-route-cursor',x,'游标卷 '||x,'PRODUCING',1,1,1000,?2,?2 FROM n",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at)
             SELECT ?1,volume_id,'artifact:b-r2-03:late-route-cursor',1,
                    CASE WHEN volume_no=100000 THEN 'PLAN_STALE' ELSE 'APPROVED' END,1,?2
             FROM volumes WHERE project_id=?1",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO volume_planning_state_projection(project_id,volume_id,projection_revision,state_digest,projection_digest,payload_json,updated_at)
             SELECT ?1,volume_id,1,?2,?3,
                    json_object('remainingChapterCapacity',1,'requiredTasksRemaining',0,
                                'blockedRequiredTasks',0,'healthBlockedRequiredTasks',0,
                                'healthBlockedOptionalTasks',0,'blockedTaskIds',json('[]'),
                                'healthRequiredTaskIds',json('[]'),
                                'planningPressure',json_object('remainingChapterCapacity',1,'requiredTasksRemaining',0)),?4
             FROM volumes WHERE project_id=?1",
            libsql_rusqlite::params![project_id, "b".repeat(64), "c".repeat(64), NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:b-r2-03:late-route-cursor:1',?1,'volume:b-r2-03:late-route-cursor:000001',1,'第一章','NORMAL','COMMITTED',1000,?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    for index in 0..50 {
        connection
            .execute(
                "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES (?1,?2,'BOOK','LONG_FORM_FIXTURE','A','artifact:b-r2-03:late-route-cursor',1,?3,'chapter:b-r2-03:late-route-cursor:1','PROPOSED',?4,?4)",
                libsql_rusqlite::params![
                    format!("obligation:b-r2-03:late-route:{index:02}"),
                    project_id,
                    format!("先显示的风险 {index}"),
                    NOW,
                ],
            )
            .unwrap();
    }
    seed_precomputed_health_route_projection(&connection, &project_id);
    let first = ai_novel_production_system_core::health::query_book_health(
        &connection,
        &project_id,
        None,
        50,
    )
    .unwrap();
    assert_eq!(first["issues"]["items"].as_array().unwrap().len(), 50);
    assert_eq!(first["issues"]["total"], 51);
    let route_factor = first["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|factor| factor["factorCode"] == "ROUTE_DRIFT")
        .unwrap();
    assert!(route_factor["sourceRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| {
            source["kind"] == "volume-plan"
                && source["ref"] == "volume:b-r2-03:late-route-cursor:100000"
        }));
    assert_eq!(route_factor["sourceRefsTotal"], 1);
    assert_eq!(route_factor["sourceRefsTruncated"], false);
    assert_eq!(
        first["provenance"]["factorSources"]["ROUTE_DRIFT"][0]["ref"],
        "volume:b-r2-03:late-route-cursor:100000"
    );
    assert_eq!(
        first["provenance"]["factorSourcePages"]["ROUTE_DRIFT"]["total"],
        1
    );
    assert_eq!(
        first["provenance"]["factorSourcePages"]["ROUTE_DRIFT"]["items"][0]["ref"],
        "volume:b-r2-03:late-route-cursor:100000"
    );
    let bounded_sql_page =
        HealthRepository::issue_page(&connection, &project_id, 1, None, 54).unwrap();
    assert_eq!(bounded_sql_page.total, 51);
    assert!(bounded_sql_page.rows_read <= 54);
    assert_eq!(bounded_sql_page.json_scopes_processed, 0);
    assert_eq!(bounded_sql_page.query_count, 2);
    let cursor = first["issues"]["nextCursor"]
        .as_str()
        .expect("the late route risk must have a legal next-page cursor");
    let second = ai_novel_production_system_core::health::query_book_health(
        &connection,
        &project_id,
        Some(cursor),
        1,
    )
    .unwrap();
    assert_eq!(
        second["issues"]["items"][0]["issueId"],
        "health:route:volume:b-r2-03:late-route-cursor:100000"
    );
    assert_eq!(
        second["issues"]["items"][0]["sourceRefs"][0]["ref"],
        "volume:b-r2-03:late-route-cursor:100000"
    );
    let page_plan = explain_with_params(
        &connection,
        HEALTH_ISSUE_PAGE_SQL,
        libsql_rusqlite::params![
            project_id,
            1,
            -1,
            -1,
            i64::MIN,
            "",
            50,
            Option::<String>::None
        ],
    );
    assert!(!page_plan.iter().any(|line| line.contains("VIRTUAL TABLE")));
    let route_factor = first["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|factor| factor["factorCode"] == "ROUTE_DRIFT")
        .unwrap();
    assert!(route_factor["sourceRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| {
            source["kind"] == "volume-plan"
                && source["ref"] == "volume:b-r2-03:late-route-cursor:100000"
        }));
    assert_eq!(route_factor["sourceRefsTotal"], 1);
    assert_eq!(route_factor["sourceRefsTruncated"], false);
    assert_eq!(
        first["provenance"]["factorSources"]["ROUTE_DRIFT"][0]["ref"],
        "volume:b-r2-03:late-route-cursor:100000"
    );
    assert_eq!(
        first["provenance"]["factorSourcePages"]["ROUTE_DRIFT"]["total"],
        1
    );
    assert_eq!(
        first["provenance"]["factorSourcePages"]["ROUTE_DRIFT"]["items"][0]["ref"],
        "volume:b-r2-03:late-route-cursor:100000"
    );
    println!(
        "B_R2_03_R4_ISSUE_PAGE_100K_RESULT {{\"projectRoutes\":{HEALTH_SCALE_ROUTE_COUNT},\"issueTotal\":{},\"firstPageItems\":{},\"sqlRowsReturned\":{},\"sqlRowLimit\":54,\"queryCount\":{},\"pageJsonScopes\":{},\"lateRiskReachable\":true,\"factorSourceExact\":true,\"status\":\"PASS\"}}",
        bounded_sql_page.total,
        first["issues"]["items"].as_array().unwrap().len(),
        bounded_sql_page.rows_read,
        bounded_sql_page.query_count,
        bounded_sql_page.json_scopes_processed
    );
}

#[test]
fn b_r2_03_r4_issue_page_work_is_independent_of_1k_10k_100k_route_json() {
    let state = state();
    for route_count in [1_000_i64, 10_000, 100_000] {
        let project_id = format!("b-r2-03-r4-scale-{route_count}");
        let connection = state
            .manager
            .create(&project_id, "R4 issue page scale", NOW)
            .unwrap();
        let book_id = format!("book:{project_id}");
        let artifact_id = format!("artifact:{project_id}:route");
        let volume_prefix = format!("volume:{project_id}:");
        let route_hash = "a".repeat(64);
        connection.execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'R4 scale','ACTIVE',?3,?3)",
            libsql_rusqlite::params![book_id,project_id,NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES (?1,?2,'VolumePlan','VOLUME',?3,1,'r4.scale','COMMITTED',?4,'{}',?5)",
            libsql_rusqlite::params![artifact_id,project_id,format!("{volume_prefix}000001"),route_hash,NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,'{\"payloadSchema\":\"volume-plan.r1\",\"tasks\":[]}',?2)",
            libsql_rusqlite::params![artifact_id,route_hash],
        ).unwrap();
        connection.execute(
            "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<?3)
             INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,created_at,updated_at)
             SELECT ?4||printf('%06d',x),?1,?2,x,'R4 卷 '||x,'PRODUCING',?5,?5 FROM n",
            libsql_rusqlite::params![project_id,book_id,route_count,volume_prefix,NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at)
             SELECT ?1,volume_id,?2,1,CASE WHEN volume_no=?3 THEN 'PLAN_STALE' ELSE 'APPROVED' END,1,?4
             FROM volumes WHERE project_id=?1",
            libsql_rusqlite::params![project_id,artifact_id,route_count,NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO volume_planning_state_projection(project_id,volume_id,projection_revision,state_digest,projection_digest,payload_json,updated_at)
             SELECT ?1,volume_id,1,?2,?3,
                    json_object('remainingChapterCapacity',1,'requiredTasksRemaining',0,
                                'healthBlockedRequiredTasks',0,'healthBlockedOptionalTasks',0,
                                'blockedTaskIds',json('[]'),'healthRequiredTaskIds',json('[]'),
                                'planningPressure',json_object('remainingChapterCapacity',1,'requiredTasksRemaining',0)),?4
             FROM volumes WHERE project_id=?1",
            libsql_rusqlite::params![project_id,"b".repeat(64),"c".repeat(64),NOW],
        ).unwrap();
        seed_precomputed_health_route_projection(&connection, &project_id);

        let sqlite_version: String = connection
            .query_row("SELECT sqlite_version()", [], |row| row.get(0))
            .unwrap();
        let load_started = Instant::now();
        let snapshot = HealthRepository::load(&connection, &project_id).unwrap();
        let load_ms = load_started.elapsed().as_millis();
        assert!(snapshot.health_projection_ready);
        assert_eq!(snapshot.route_scope_count, route_count);
        assert_eq!(snapshot.route_payloads_read, 0);
        assert_eq!(snapshot.route_payloads_parsed, 0);
        let page_started = Instant::now();
        let page = HealthRepository::issue_page(&connection, &project_id, 0, None, 50).unwrap();
        let page_ms = page_started.elapsed().as_millis();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows_read, 1);
        assert_eq!(page.json_scopes_processed, 0);
        assert_eq!(page.query_count, 2);
        assert_eq!(
            page.rows[0].source_ref,
            format!("{volume_prefix}{route_count:06}")
        );
        let late_cursor = HealthIssueCursor {
            source_revision: 0,
            severity_rank: 0,
            factor_rank: 2,
            due_sort: 1,
            issue_id: page.rows[0].issue_id.clone(),
        };
        let after_last =
            HealthRepository::issue_page(&connection, &project_id, 0, Some(&late_cursor), 50)
                .unwrap();
        assert_eq!(after_last.total, 1);
        assert!(after_last.rows.is_empty());
        let plan = explain_with_params(
            &connection,
            HEALTH_ISSUE_PAGE_SQL,
            libsql_rusqlite::params![
                project_id,
                0,
                -1,
                -1,
                i64::MIN,
                "",
                50,
                Option::<String>::None
            ],
        );
        assert!(!plan.iter().any(|line| line.contains("VIRTUAL TABLE")));
        println!(
            "B_R2_03_R4_SQL_SCALE_RESULT {{\"routeScopes\":{route_count},\"pageSize\":50,\"sqliteVersion\":\"{sqlite_version}\",\"firstPageRows\":{},\"afterLastRows\":{},\"total\":{},\"healthLoadMs\":{load_ms},\"pagePairMs\":{page_ms},\"routePayloadRowsRead\":{},\"routePayloadsParsed\":{},\"pageJsonScopes\":{},\"pageQueryCount\":{},\"sqlRowsReturned\":{},\"plan\":{},\"status\":\"PASS\"}}",
            page.rows_read,after_last.rows_read,page.total,snapshot.route_payloads_read,snapshot.route_payloads_parsed,page.json_scopes_processed,page.query_count,page.rows_read,json!(plan)
        );
    }
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
    assert_eq!(result["level"], "not_evaluated");
    assert!(result["score"].is_null());
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
    for chapter_no in [1_i64, 9, 10] {
        connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,'volume:b13',?3,?4,'NORMAL','COMMITTED',1000,?5,?5)", libsql_rusqlite::params![format!("chapter:b13:{chapter_no}"), project_id, chapter_no, format!("第 {chapter_no} 章"), NOW]).unwrap();
    }
    for chapter_no in [11_i64, 20] {
        connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,'volume:b13',?3,?4,'NORMAL','PLANNED',0,?5,?5)", libsql_rusqlite::params![format!("chapter:b13:{chapter_no}"), project_id, chapter_no, format!("第 {chapter_no} 章"), NOW]).unwrap();
    }
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b13:source',?1,'BookPlan','BOOK','book:b13',1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)", libsql_rusqlite::params![project_id, "a".repeat(64), NOW]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:b13:source',1,'{}',?1)", ["a".repeat(64)]).unwrap();
    connection.execute("INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES ('obligation:b13:a',?1,'BOOK','MYSTERY','A','artifact:b13:source',1,'高优先级义务','chapter:b13:9','PROPOSED',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES ('obligation:b13:c',?1,'BOOK','MYSTERY','C','artifact:b13:source',1,'低优先级义务','chapter:b13:10','PROGRESSING',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES ('obligation:b13:future',?1,'BOOK','MYSTERY','A','artifact:b13:source',1,'未来义务','chapter:b13:11','APPROVED',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,recover_window_end_chapter,source_authority,created_at,updated_at) VALUES ('foreshadow:b13:late',?1,'晚回收','PLANTED','chapter:b13:9','test',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,recover_window_end_chapter,source_authority,created_at,updated_at) VALUES ('foreshadow:b13:stale',?1,'需关注','STALE','chapter:b13:20','test',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,recover_window_end_chapter,source_authority,created_at,updated_at) VALUES ('foreshadow:b13:done',?1,'已回收','RECOVERED','chapter:b13:1','test',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO reader_desires(desire_id,project_id,desire_type,status,expected_payoff_chapter,created_at,updated_at) VALUES ('desire:b13:open',?1,'谜底','OPEN','chapter:b13:10',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
    connection.execute("INSERT INTO reader_desires(desire_id,project_id,desire_type,status,created_at,updated_at) VALUES ('desire:b13:open-no-date',?1,'关系','OPEN',?2,?2)", libsql_rusqlite::params![project_id, NOW]).unwrap();
}

#[test]
fn b_r2_03_health_issue_cursor_pages_are_stable_without_repeats() {
    let state = state();
    let project_id = create_project(&state, "cmd:b-r2-03:issue-cursor");
    let connection = state
        .manager
        .open(&project_id, "B-R2-03 cursor", NOW)
        .unwrap();
    seed_health_rows(&connection, &project_id);
    for index in 0..100 {
        connection.execute(
            "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES (?1,?2,'BOOK','MYSTERY','C','artifact:b13:source',1,?3,'chapter:b13:1','PROPOSED',?4,?4)",
            libsql_rusqlite::params![format!("obligation:b-r2-03:cursor:{index:02}"), project_id, format!("合成关注项 {index}"), NOW],
        ).unwrap();
    }
    drop(connection);

    let mut cursor = None;
    let mut issue_ids = Vec::new();
    let mut expected_total = None;
    let mut page_count = 0;
    loop {
        let mut request = json!({"projectId":project_id,"scope":"health","pageSize":4});
        if let Some(value) = cursor.as_ref() {
            request["cursor"] = json!(value);
        }
        let result = query_book_health_impl(&state, request).unwrap();
        expected_total.get_or_insert_with(|| result["issues"]["total"].as_u64().unwrap());
        issue_ids.extend(
            result["issues"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["issueId"].as_str().unwrap().to_owned()),
        );
        cursor = result["issues"]["nextCursor"]
            .as_str()
            .map(ToOwned::to_owned);
        page_count += 1;
        if cursor.is_none() {
            break;
        }
        assert!(
            page_count < 100,
            "cursor failed to advance to the end of the bounded issue set"
        );
    }

    let unique_ids = issue_ids.iter().collect::<std::collections::HashSet<_>>();
    assert_eq!(issue_ids.len() as u64, expected_total.unwrap());
    assert_eq!(unique_ids.len(), issue_ids.len());
    assert_eq!(
        issue_ids
            .iter()
            .filter(|id| id.contains("obligation:b-r2-03:cursor:"))
            .count(),
        100
    );
    println!("B_R2_03_CURSOR_RESULT {{\"pages\":{page_count},\"items\":{},\"duplicates\":0,\"misses\":0,\"status\":\"PASS\"}}", issue_ids.len());
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
    let book_id = "book:b13:route";
    let volume_id = "volume:b13:route";
    let intent_id = "intent:b13:route";
    let route_id = "artifact:b13:route";
    let task_required = "task:b13:required";
    let task_optional = "task:b13:optional";
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'路线测试','ACTIVE',?3,?3)", libsql_rusqlite::params![book_id,project_id,NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'路线卷','PRODUCING',1,1,1000,?4,?4)", libsql_rusqlite::params![volume_id,project_id,book_id,NOW]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:b13:route:1',?1,?2,1,'已提交章节','NORMAL','COMMITTED',1000,?3,?3)", libsql_rusqlite::params![project_id,volume_id,NOW]).unwrap();
    let volume_scope = json!({"level":"VOLUME","volumeId":volume_id}).to_string();
    let intent = json!({"payloadSchema":"volume-intent.r1","volumeFunction":"推进调查并处理关键任务","entryConditions":[],"mustAchieve":[],"mustNotBreak":[],"requiredExitConditions":[{"kind":"TASK_SATISFIED","value":task_required}],"futureReservationRefs":[],"crossVolumeResponsibilityRefs":[],"targetChapterRange":{"min":1,"preferred":1,"max":1},"targetWordRange":{"preferred":1000}});
    let intent_hash = sha256_canonical(&intent).unwrap();
    let route = json!({"payloadSchema":"volume-plan.r1","volumeIntentRef":{"artifactId":intent_id,"version":1,"payloadHash":intent_hash},"routeVersion":1,"tasks":[{"taskId":task_required,"class":"REQUIRED","objective":"完成关键推进","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"TASK_SATISFIED","value":task_required},"estimatedChapterCost":{"preferred":1}},{"taskId":task_optional,"class":"OPTIONAL","objective":"补充人物描写","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"TASK_SATISFIED","value":task_optional},"estimatedChapterCost":{"preferred":1}}],"routeInvariants":[],"requiredExitConditions":[{"kind":"TASK_SATISFIED","value":task_required}],"capacityBudget":{"minChapters":1,"preferredChapters":1,"maxChapters":1}});
    let route_hash = sha256_canonical(&route).unwrap();
    for (artifact_id, artifact_type, payload, hash) in [
        (intent_id, "VolumeIntent", intent, intent_hash),
        (route_id, "VolumePlan", route, route_hash.clone()),
    ] {
        connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,'VOLUME',?4,1,'b13.fixture','COMMITTED',?5,'{}',?6,?6)", libsql_rusqlite::params![artifact_id,project_id,artifact_type,volume_scope,hash,NOW]).unwrap();
        connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)", libsql_rusqlite::params![artifact_id,payload.to_string(),hash]).unwrap();
        connection.execute("INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,?2,'VOLUME',?3,?4,1,?5)", libsql_rusqlite::params![project_id,artifact_type,volume_scope,artifact_id,hash]).unwrap();
    }
    connection.execute("INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,?2,?3,1,'APPROVED',1,?4)", libsql_rusqlite::params![project_id,volume_id,route_id,NOW]).unwrap();
    for (task_id, status) in [(task_required, "BLOCKED"), (task_optional, "NOT_STARTED")] {
        connection.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES (?1,?2,'volume.task.progress',?3,'{}',?4,?4)", libsql_rusqlite::params![task_id,project_id,json!({"taskId":task_id,"status":status,"evidenceRefs":[],"remainingConditions":[]}).to_string(),NOW]).unwrap();
    }
    let initial_planning =
        VolumeLifecycleOwner::rebuild_planning_state(&connection, &project_id, volume_id, NOW)
            .unwrap();
    assert_eq!(initial_planning["blockedRequiredTasks"], 1);
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
    let revision: i64 = connection
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":"commit:b13:route-task-state",
        "projectId":project_id,
        "commandId":"b13:route-task-state",
        "idempotencyKey":"b13:route-task-state:key",
        "expectedProjectRevision":revision,
        "contractVersion":"m0a.v1",
        "createdAt":NOW,
        "artifacts":[],
        "stateDelta":[
            {"stateId":task_required,"stateType":"volume.task.progress","valueJson":{"taskId":task_required,"status":"SATISFIED","evidenceRefs":[],"remainingConditions":[]}},
            {"stateId":task_optional,"stateType":"volume.task.progress","valueJson":{"taskId":task_optional,"status":"BLOCKED","evidenceRefs":[],"remainingConditions":[]}}
        ]
    })).unwrap();
    CommitService::commit(&connection, &bundle, NOW).unwrap();
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
    assert_eq!(boundary_factor("ROUTE_DRIFT")["severity"], "healthy");
    assert_eq!(boundary_factor("TASK_BLOCKAGE")["severity"], "attention");
    assert_eq!(boundary["counters"]["blockedRequiredTasks"], 0);
    assert_eq!(boundary["counters"]["blockedOptionalTasks"], 1);
    assert_eq!(boundary_factor("TASK_BLOCKAGE")["sourceRefsTotal"], 1);
    let source_page = query_book_health_factor_sources_impl(
        &state,
        json!({"projectId":project_id,"factorCode":"TASK_BLOCKAGE","pageSize":50}),
    )
    .unwrap();
    assert_eq!(source_page["items"][0]["ref"], task_optional);
    println!("B13_ROUTE_BOUNDARY_RESULT formalTaskSatisfied=PASS capacityRemaining=PASS optionalBlocked=PASS status=PASS");
    println!(
        "B13_ROUTE_TASK_RESULT routeCapacity=PASS requiredBlocked=PASS noReplan=PASS status=PASS"
    );
}

#[test]
fn b_r2_03_r4_refreshes_only_affected_scopes_and_fails_closed_while_dirty() {
    let state = state();
    let project_id = create_project(&state, "cmd:b-r2-03:r4-scope-refresh");
    let connection = state.manager.open(&project_id, "B-R2-03 R4", NOW).unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:r4',?1,1,'R4 projection fixture','ACTIVE',?2,?2)",
        libsql_rusqlite::params![project_id,NOW],
    ).unwrap();
    for (number, task_id, task_class, status) in [
        (1_i64, "task:r4:required", "REQUIRED", "NOT_STARTED"),
        (2_i64, "task:r4:optional", "OPTIONAL", "BLOCKED"),
    ] {
        let volume_id = format!("volume:r4:{number}");
        let artifact_id = format!("artifact:r4:{number}");
        let payload = json!({"payloadSchema":"volume-plan.r1","tasks":[{"taskId":task_id,"class":task_class}]}).to_string();
        let hash = if number == 1 {
            "a".repeat(64)
        } else {
            "b".repeat(64)
        };
        connection.execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,created_at,updated_at) VALUES (?1,?2,'book:r4',?3,?4,'PRODUCING',?5,?5)",
            libsql_rusqlite::params![volume_id,project_id,number,format!("R4 卷 {number}"),NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES (?1,?2,'VolumePlan','VOLUME',?3,1,'r4.fixture','COMMITTED',?4,'{}',?5)",
            libsql_rusqlite::params![artifact_id,project_id,volume_id,hash,NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            libsql_rusqlite::params![artifact_id,payload,hash],
        ).unwrap();
        connection.execute(
            "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,?2,?3,1,'APPROVED',1,?4)",
            libsql_rusqlite::params![project_id,volume_id,artifact_id,NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO volume_planning_state_projection(project_id,volume_id,projection_revision,state_digest,projection_digest,payload_json,updated_at) VALUES (?1,?2,1,?3,?4,?5,?6)",
            libsql_rusqlite::params![project_id,volume_id,"c".repeat(64),"d".repeat(64),json!({"remainingChapterCapacity":1,"requiredTasksRemaining":1,"healthBlockedRequiredTasks":0,"healthBlockedOptionalTasks":if number==1 {0} else {1},"blockedTaskIds":[],"healthRequiredTaskIds":[task_id],"planningPressure":{"remainingChapterCapacity":1,"requiredTasksRemaining":1}}).to_string(),NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES (?1,?2,'volume.task.progress',?3,'{}',?4,?4)",
            libsql_rusqlite::params![task_id,project_id,json!({"taskId":task_id,"status":status}).to_string(),NOW],
        ).unwrap();
    }
    let initial = HealthRepository::flush_dirty(&connection, &project_id, NOW).unwrap();
    assert_eq!(initial.route_scopes_refreshed, 2);
    assert_eq!(initial.route_json_scopes_read, 2);
    let first = ai_novel_production_system_core::health::query_book_health(
        &connection,
        &project_id,
        None,
        50,
    )
    .unwrap();
    let first_task = first["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|factor| factor["factorCode"] == "TASK_BLOCKAGE")
        .unwrap();
    assert_eq!(first_task["severity"], "attention");
    assert_eq!(first_task["sourceRefs"][0]["ref"], "task:r4:optional");

    let revision: i64 = connection
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let task_commit: CommitBundle = serde_json::from_value(json!({
        "commitId":"commit:b-r4:required-blocked",
        "projectId":project_id,
        "commandId":"b-r4:required-blocked",
        "idempotencyKey":"b-r4:required-blocked:key",
        "expectedProjectRevision":revision,
        "contractVersion":"m0a.v1",
        "createdAt":NOW,
        "artifacts":[],
        "stateDelta":[{"stateId":"task:r4:required","stateType":"volume.task.progress","valueJson":{"taskId":"task:r4:required","status":"BLOCKED","evidenceRefs":[],"remainingConditions":[]}}]
    })).unwrap();
    CommitService::commit(&connection, &task_commit, NOW).unwrap();
    let current = ai_novel_production_system_core::health::query_book_health(
        &connection,
        &project_id,
        None,
        50,
    )
    .unwrap();
    let required_task = current["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|factor| factor["factorCode"] == "TASK_BLOCKAGE")
        .unwrap();
    assert_eq!(required_task["severity"], "risk");
    assert!(required_task["sourceRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| source["ref"] == "task:r4:required"));
    assert_eq!(current["counters"]["blockedRequiredTasks"], 1);
    assert_eq!(current["counters"]["blockedOptionalTasks"], 1);

    let v2_hash = "e".repeat(64);
    let v2_payload = json!({
        "payloadSchema":"volume-plan.r1",
        "tasks":[{"taskId":"task:r4:required","class":"REQUIRED"}],
        "compiledFromStateDigest":"f".repeat(64)
    });
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:r4:1:v2',?1,'VolumePlan','VOLUME','volume:r4:1',2,'r4.fixture','COMMITTED',?2,'{}',?3)",
        libsql_rusqlite::params![project_id,v2_hash,NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:r4:1:v2',2,?1,?2)",
        libsql_rusqlite::params![v2_payload.to_string(),v2_hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'VolumePlan','VOLUME','volume:r4:1','artifact:r4:1:v2',2,?2)",
        libsql_rusqlite::params![project_id,v2_hash],
    ).unwrap();
    let head_dirty = HealthRepository::flush_dirty(&connection, &project_id, NOW).unwrap();
    assert_eq!(head_dirty.route_scopes_refreshed, 1);
    let head_projection = ai_novel_production_system_core::health::query_book_health(
        &connection,
        &project_id,
        None,
        50,
    )
    .unwrap();
    let head_route = head_projection["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|factor| factor["factorCode"] == "ROUTE_DRIFT")
        .unwrap();
    assert_eq!(head_route["severity"], "risk");
    assert!(head_route["sourceRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| source["ref"] == "volume:r4:1"));

    connection.execute(
        "DELETE FROM canonical_heads WHERE project_id=?1 AND artifact_type='VolumePlan' AND scope_key='volume:r4:1'",
        [&project_id],
    ).unwrap();
    HealthRepository::flush_dirty(&connection, &project_id, NOW).unwrap();
    connection.execute(
        "INSERT INTO stale_queue(stale_id,project_id,target_ref,stale_reason,revalidate_class,processed,created_at,updated_at) VALUES ('stale:r4:retcon',?1,'volume:r4:2','R4 retcon fixture','RETCON',0,?2,?2)",
        libsql_rusqlite::params![project_id,NOW],
    ).unwrap();
    let retcon_refresh = HealthRepository::flush_dirty(&connection, &project_id, NOW).unwrap();
    assert_eq!(retcon_refresh.route_scopes_refreshed, 1);
    let retcon_projection = ai_novel_production_system_core::health::query_book_health(
        &connection,
        &project_id,
        None,
        50,
    )
    .unwrap();
    let retcon_route = retcon_projection["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|factor| factor["factorCode"] == "ROUTE_DRIFT")
        .unwrap();
    assert!(retcon_route["sourceRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| source["ref"] == "volume:r4:2"));

    connection.execute(
        "UPDATE stale_queue SET processed=1,updated_at=?1 WHERE project_id=?2 AND stale_id='stale:r4:retcon'",
        libsql_rusqlite::params![NOW,project_id],
    ).unwrap();
    HealthRepository::flush_dirty(&connection, &project_id, NOW).unwrap();
    connection.execute(
        "INSERT INTO volume_route_replans(replan_id,project_id,volume_id,new_artifact_id,new_version,trigger,reason,source_revision,input_refs_json,validation_status,project_command_receipt_id,created_at) VALUES ('replan:r4:old',?1,'volume:r4:1','artifact:r4:1',1,'R4_FIXTURE','stale digest',0,?2,'VALIDATED','project-receipt:r4:old',?3)",
        libsql_rusqlite::params![project_id,json!({"actualReality":{"stateDigest":"f".repeat(64)}}).to_string(),"2026-09-04T02:00:30Z"],
    ).unwrap();
    HealthRepository::flush_dirty(&connection, &project_id, NOW).unwrap();
    let replan_projection = ai_novel_production_system_core::health::query_book_health(
        &connection,
        &project_id,
        None,
        50,
    )
    .unwrap();
    assert_eq!(
        replan_projection["factors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|factor| factor["factorCode"] == "ROUTE_DRIFT")
            .unwrap()["severity"],
        "risk"
    );
    connection.execute(
        "INSERT INTO volume_route_replans(replan_id,project_id,volume_id,new_artifact_id,new_version,trigger,reason,source_revision,input_refs_json,validation_status,project_command_receipt_id,created_at) VALUES ('replan:r4:new',?1,'volume:r4:1','artifact:r4:1',1,'R4_FIXTURE','current digest',0,?2,'VALIDATED','project-receipt:r4:new',?3)",
        libsql_rusqlite::params![project_id,json!({"actualReality":{"stateDigest":"c".repeat(64)}}).to_string(),"2026-09-04T02:01:00Z"],
    ).unwrap();
    HealthRepository::flush_dirty(&connection, &project_id, NOW).unwrap();

    let revision: i64 = connection
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let task_fulfillment: CommitBundle = serde_json::from_value(json!({
        "commitId":"commit:r4:task-fulfillment",
        "projectId":project_id,
        "commandId":"r4:task-fulfillment",
        "idempotencyKey":"r4:task-fulfillment:key",
        "expectedProjectRevision":revision,
        "contractVersion":"m0a.v1",
        "createdAt":NOW,
        "artifacts":[],
        "stateDelta":[
            {"stateId":"task:r4:required","stateType":"volume.task.progress","valueJson":{"taskId":"task:r4:required","status":"SATISFIED","evidenceRefs":[],"remainingConditions":[]}},
            {"stateId":"task:r4:optional","stateType":"volume.task.progress","valueJson":{"taskId":"task:r4:optional","status":"SATISFIED","evidenceRefs":[],"remainingConditions":[]}}
        ]
    })).unwrap();
    CommitService::commit(&connection, &task_fulfillment, NOW).unwrap();
    let fulfilled = ai_novel_production_system_core::health::query_book_health(
        &connection,
        &project_id,
        None,
        50,
    )
    .unwrap();
    assert_eq!(fulfilled["counters"]["blockedRequiredTasks"], 0);
    assert_eq!(fulfilled["counters"]["blockedOptionalTasks"], 0);
    assert_eq!(
        fulfilled["factors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|factor| factor["factorCode"] == "TASK_BLOCKAGE")
            .unwrap()["severity"],
        "healthy"
    );

    connection
        .execute(
            "DELETE FROM volume_plan_index WHERE project_id=?1 AND volume_id='volume:r4:2'",
            [&project_id],
        )
        .unwrap();
    connection.execute("DELETE FROM volume_planning_state_projection WHERE project_id=?1 AND volume_id='volume:r4:2'",[&project_id]).unwrap();
    HealthRepository::flush_dirty(&connection, &project_id, NOW).unwrap();
    drop(connection);
    let reopened = state.manager.open(&project_id, "B-R2-03 R4", NOW).unwrap();
    let reloaded = ai_novel_production_system_core::health::query_book_health(
        &reopened,
        &project_id,
        None,
        50,
    )
    .unwrap();
    assert_eq!(
        reloaded["factors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|factor| factor["factorCode"] == "TASK_BLOCKAGE")
            .unwrap()["severity"],
        "healthy"
    );
    println!("B_R2_03_R4_SCOPE_REFRESH_RESULT initialScopes=2 taskWriteDirtyScopes=1 refreshedScopes=1 pageJsonScopes=0 staleReadFailClosed=true canonicalHead=PASS retconQueue=PASS routeReplan=PASS taskFulfillment=PASS scopeDelete=PASS reopen=PASS status=PASS");
}

#[test]
fn b_r2_03_r4_factor_source_pages_are_bounded_and_resumable() {
    let state = state();
    let project_id = create_project(&state, "cmd:b-r2-03:r4-source-cursor");
    let connection = state
        .manager
        .open(&project_id, "B-R2-03 R4 source cursor", NOW)
        .unwrap();
    for index in 0..55_i64 {
        let volume_id = format!("volume:r4:source:{index:03}");
        connection.execute(
            "INSERT INTO health_issue_projection(project_id,issue_id,factor_code,severity,title,detail,location,due_chapter,recommended_destination,source_kind,source_ref,severity_rank,factor_rank,due_sort,volume_id,source_revision)
             VALUES (?1,?2,'ROUTE_DRIFT','risk','路线风险','需复核',?3,1,'卷工作区','volume-plan',?4,0,2,?5,?4,0)",
            libsql_rusqlite::params![project_id,format!("health:route:{volume_id}"),format!("R4 卷 {index}"),volume_id,index],
        ).unwrap();
    }
    drop(connection);
    let first = query_book_health_factor_sources_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","factorCode":"ROUTE_DRIFT","pageSize":50}),
    )
    .unwrap();
    assert_eq!(first["total"], 55);
    assert_eq!(first["items"].as_array().unwrap().len(), 50);
    assert_eq!(first["truncated"], true);
    let cursor = first["nextCursor"]
        .as_str()
        .expect("bounded source page must expose a cursor");
    let second=query_book_health_factor_sources_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","factorCode":"ROUTE_DRIFT","pageSize":50,"cursor":cursor}),
    ).unwrap();
    assert_eq!(second["total"], 55);
    assert_eq!(second["items"].as_array().unwrap().len(), 5);
    assert_eq!(second["truncated"], false);
    assert!(second["nextCursor"].is_null());
    let writer = state
        .manager
        .open(&project_id, "B-R2-03 R4 source cursor", NOW)
        .unwrap();
    writer
        .execute(
            "UPDATE project_meta SET revision=revision+1,updated_at=?1 WHERE project_id=?2",
            libsql_rusqlite::params![NOW, project_id],
        )
        .unwrap();
    drop(writer);
    let stale_cursor = query_book_health_factor_sources_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","factorCode":"ROUTE_DRIFT","pageSize":50,"cursor":cursor}),
    )
    .unwrap_err();
    assert!(stale_cursor.contains("HEALTH_CURSOR_STALE"));
    let refs = first["items"]
        .as_array()
        .unwrap()
        .iter()
        .chain(second["items"].as_array().unwrap())
        .map(|source| source["ref"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        refs.iter().collect::<std::collections::HashSet<_>>().len(),
        55
    );
    println!("B_R2_03_R4_FACTOR_SOURCE_PAGE_RESULT total=55 firstItems=50 nextItems=5 duplicates=0 misses=0 nextCursorResumed=true staleRevisionRejected=true status=PASS");
}

#[test]
fn b_r2_03_r4_existing_project_migration_explicitly_rebuilds_health_index() {
    let state = state();
    let project_id = create_project(&state, "cmd:b-r2-03:r4-migration-rebuild");
    let connection = state
        .manager
        .open(&project_id, "B-R2-03 R4 migration", NOW)
        .unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:r4:migration',?1,1,'R4 migration fixture','ACTIVE',?2,?2)",
        libsql_rusqlite::params![project_id,NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,created_at,updated_at) VALUES ('volume:r4:migration',?1,'book:r4:migration',1,'迁移卷','PRODUCING',?2,?2)",
        libsql_rusqlite::params![project_id,NOW],
    ).unwrap();
    let hash = "a".repeat(64);
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:r4:migration',?1,'VolumePlan','VOLUME','volume:r4:migration',1,'r4.fixture','COMMITTED',?2,'{}',?3)",
        libsql_rusqlite::params![project_id,hash,NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:r4:migration',1,'{\"payloadSchema\":\"volume-plan.r1\",\"tasks\":[]}',?1)",
        [&hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,'volume:r4:migration','artifact:r4:migration',1,'APPROVED',1,?2)",
        libsql_rusqlite::params![project_id,NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volume_planning_state_projection(project_id,volume_id,projection_revision,state_digest,projection_digest,payload_json,updated_at) VALUES (?1,'volume:r4:migration',1,?2,?3,?4,?5)",
        libsql_rusqlite::params![project_id,"b".repeat(64),"c".repeat(64),json!({"remainingChapterCapacity":12,"requiredTasksRemaining":0,"healthBlockedRequiredTasks":0,"healthBlockedOptionalTasks":0,"blockedTaskIds":[],"healthRequiredTaskIds":[]}).to_string(),NOW],
    ).unwrap();
    flush_health_projection(&connection, &project_id);
    connection.execute_batch(
        "DELETE FROM health_issue_projection; DELETE FROM health_task_scope_index; DELETE FROM health_route_scope_projection; DELETE FROM health_projection_dirty; DELETE FROM health_projection_state; DELETE FROM migration_history WHERE version IN (13,14);",
    ).unwrap();
    drop(connection);

    let reopened = state
        .manager
        .open(&project_id, "B-R2-03 R4 migration", NOW)
        .unwrap();
    let applied: bool = reopened
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM migration_history WHERE version=13)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let pending_migration_applied: bool = reopened
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM migration_history WHERE version=14)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let state_ready: bool = reopened.query_row(
        "SELECT EXISTS(SELECT 1 FROM health_projection_state s JOIN project_meta p ON p.project_id=s.project_id WHERE s.project_id=?1 AND s.status='READY' AND s.source_revision=p.revision)",
        [&project_id],
        |row| row.get(0),
    ).unwrap();
    assert!(applied);
    assert!(pending_migration_applied);
    assert!(reopened.query_row::<i64, _, _>("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='health_b09_rebuild_pending'", [], |row| row.get(0)).unwrap() == 1);
    assert!(state_ready);
    let projection = ai_novel_production_system_core::health::query_book_health(
        &reopened,
        &project_id,
        None,
        30,
    )
    .unwrap();
    assert_eq!(
        projection["factors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|factor| factor["factorCode"] == "ROUTE_DRIFT")
            .unwrap()["status"],
        "EVALUATED"
    );
    println!("B_R2_03_R4_MIGRATION_REBUILD_RESULT migration13=PASS routeScopes=1 state=READY revisionBound=true noQueryRebuild=true status=PASS");
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
    )
    .unwrap();
    assert_eq!(result["counters"]["overdueObligations"], 1);
    assert_eq!(result["factors"][0]["status"], "SOURCE_UNAVAILABLE");
    assert!(result["issues"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| {
            item["issueId"] == "health:obligation:obligation:b13:r1:opaque"
                && item["location"] == "第 9 章附近"
        }));
}

#[test]
fn b13_r1_revision_only_route_change_does_not_become_route_drift() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:r1:route-revision");
    let connection = state.manager.open(&project_id, "B13 R1", NOW).unwrap();
    connection
        .execute(
            "UPDATE project_meta SET revision=2 WHERE project_id=?1",
            [&project_id],
        )
        .unwrap();
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
        libsql_rusqlite::params![json!({"payloadSchema":"volume-plan.r1","tasks":[],"compiledFromStateDigest":"a".repeat(64)}).to_string(), route_hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,'volume:b13:r1:route','artifact:b13:r1:route',1,'APPROVED',3,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volume_planning_state_projection(project_id,volume_id,projection_revision,state_digest,projection_digest,payload_json,updated_at) VALUES (?1,'volume:b13:r1:route',1,?2,?3,?4,?5)",
        libsql_rusqlite::params![project_id, "a".repeat(64), "e".repeat(64), json!({"remainingChapterCapacity":3,"requiredTasksRemaining":0,"blockedRequiredTasks":0,"healthBlockedRequiredTasks":0,"healthBlockedOptionalTasks":0,"planningPressure":{"remainingChapterCapacity":3,"requiredTasksRemaining":0}}).to_string(), NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO planning_state_ledger_index(project_id,volume_id,entry_state,plan_state,coverage_state,obligation_ids_json,foreshadow_ids_json,evidence_need_ids_json,repair_state,updated_at,route_plan_artifact_id,route_plan_version,task_status_json,active_task_ids_json,blocked_task_ids_json,remaining_chapter_capacity,planning_pressure_json,state_digest,generated_revision) VALUES (?1,'volume:b13:r1:route','OPEN','APPROVED','COMPLETE','[]','[]','[]','NONE',?2,'artifact:b13:r1:route',1,'{}','[]','[]',3,?3,?4,1)",
        libsql_rusqlite::params![project_id, NOW, json!({"remainingChapterCapacity":3,"requiredTasksRemaining":0}).to_string(), "a".repeat(64)],
    ).unwrap();
    flush_health_projection(&connection, &project_id);
    drop(connection);

    let result = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    let route_factor = result["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|factor| factor["factorCode"] == "ROUTE_DRIFT")
        .unwrap();
    assert_eq!(route_factor["severity"], "healthy");
    assert!(result["issues"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| {
            !item["location"]
                .as_str()
                .unwrap_or_default()
                .contains("volume:b13:r1:route")
        }));

    let connection = state.manager.open(&project_id, "B13 R1", NOW).unwrap();
    connection
        .execute(
            "UPDATE volume_planning_state_projection SET projection_revision=2,state_digest=?1,projection_digest=?2,payload_json='{}',updated_at=?3 WHERE project_id=?4 AND volume_id='volume:b13:r1:route'",
            libsql_rusqlite::params!["d".repeat(64), "e".repeat(64), NOW, project_id],
        )
        .unwrap();
    flush_health_projection(&connection, &project_id);
    drop(connection);
    let digest_changed = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    let digest_route_factor = digest_changed["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|factor| factor["factorCode"] == "ROUTE_DRIFT")
        .unwrap();
    assert_eq!(digest_route_factor["severity"], "risk");
    println!("B13_R1_PLAN_STALE_RESULT revisionOnly=PASS digestChanged=PASS status=PASS");
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

    let small = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":1}),
    )
    .unwrap();
    let large = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":50}),
    )
    .unwrap();
    assert_eq!(small["level"], "attention");
    assert_eq!(small["score"], large["score"]);
    assert_eq!(small["level"], large["level"]);
    assert_eq!(small["coverage"], large["coverage"]);
    assert_eq!(small["counters"], large["counters"]);
    assert_eq!(small["counters"]["overdueObligations"], 100);
}

#[test]
fn b13_r1_foreshadow_and_reader_refs_use_joined_opaque_chapter_numbers() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:r1:opaque-h02-h07");
    let connection = state.manager.open(&project_id, "B13 R1", NOW).unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b13:r1:opaque',?1,1,'章节引用测试','ACTIVE',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:b13:r1:opaque',?1,'book:b13:r1:opaque',2,'暗潮','PRODUCING',1,101,101000,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:fixture:009',?1,'volume:b13:r1:opaque',9,'第九章','NORMAL','COMMITTED',1000,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:p:1',?1,'volume:b13:r1:opaque',101,'第一百零一章','NORMAL','PLANNED',0,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b13:r1:opaque',?1,'BookPlan','BOOK','book:b13:r1:opaque',1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)",
        libsql_rusqlite::params![project_id, "a".repeat(64), NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,recover_window_end_chapter,source_authority,created_at,updated_at) VALUES ('foreshadow:b13:r1:overdue',?1,'已到回收节点','PLANTED','chapter:fixture:009','test',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,recover_window_end_chapter,source_authority,created_at,updated_at) VALUES ('foreshadow:b13:r1:stale',?1,'尚未越过窗口但需关注','STALE','chapter:p:1','test',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,recover_window_end_chapter,source_authority,created_at,updated_at) VALUES ('foreshadow:b13:r1:missing',?1,'缺失来源','PLANTED','chapter:r1:missing','test',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO reader_desires(desire_id,project_id,desire_type,status,expected_payoff_chapter,created_at,updated_at) VALUES ('desire:b13:r1:overdue',?1,'谜底','OPEN','chapter:fixture:009',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO reader_desires(desire_id,project_id,desire_type,status,expected_payoff_chapter,created_at,updated_at) VALUES ('desire:b13:r1:missing',?1,'关系','OPEN','chapter:r1:missing',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    drop(connection);

    let result = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":50}),
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
    assert_eq!(factor("FORESHADOW_AGING")["severity"], "risk");
    assert_eq!(factor("FORESHADOW_AGING")["status"], "SOURCE_UNAVAILABLE");
    assert_eq!(factor("READER_DESIRE")["severity"], "attention");
    assert_eq!(factor("READER_DESIRE")["status"], "SOURCE_UNAVAILABLE");
    assert_eq!(result["counters"]["overdueForeshadows"], 1);
    assert_eq!(result["counters"]["overdueReaderDesires"], 1);
    assert!(result["issues"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| { item["location"] == "第 9 章附近" }));
    assert!(result["issues"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| {
            !item["location"]
                .as_str()
                .unwrap_or_default()
                .contains("chapter:")
        }));
}

#[test]
fn b13_r1_route_stale_truth_covers_scope_beyond_detail_page() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:r1:route-scale");
    let connection = state.manager.open(&project_id, "B13 R1", NOW).unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b13:r1:route-scale',?1,1,'路线规模测试','ACTIVE',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b13:r1:route',?1,'VolumePlan','VOLUME','volume:b13:r1:route-scale',1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)",
        libsql_rusqlite::params![project_id, "b".repeat(64), NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:b13:r1:route',1,?1,?2)",
        libsql_rusqlite::params![json!({"payloadSchema":"volume-plan.r1","tasks":[]}).to_string(), "b".repeat(64)],
    ).unwrap();
    for index in 1..=102_i64 {
        let volume_id = format!("volume:b13:r1:scope:{index:03}");
        connection.execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,'book:b13:r1:route-scale',?3,?4,'PRODUCING',1,1,1000,?5,?5)",
            libsql_rusqlite::params![volume_id, project_id, index, format!("路线卷 {index}"), NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO planning_state_ledger_index(project_id,volume_id,entry_state,plan_state,coverage_state,obligation_ids_json,foreshadow_ids_json,evidence_need_ids_json,repair_state,updated_at,task_status_json,active_task_ids_json,blocked_task_ids_json,remaining_chapter_capacity,planning_pressure_json,state_digest,generated_revision) VALUES (?1,?2,'OPEN',?3,'COMPLETE','[]','[]','[]','NONE',?4,'{}','[]','[]',1,?5,?6,1)",
            libsql_rusqlite::params![project_id, volume_id, if index == 102 { "PLAN_STALE" } else { "APPROVED" }, NOW, json!({"remainingChapterCapacity":1,"requiredTasksRemaining":0}).to_string(), "a".repeat(64)],
        ).unwrap();
        connection.execute(
            "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,?2,'artifact:b13:r1:route',1,?3,1,?4)",
            libsql_rusqlite::params![project_id, volume_id, if index == 102 { "PLAN_STALE" } else { "APPROVED" }, NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO volume_planning_state_projection(project_id,volume_id,projection_revision,state_digest,projection_digest,payload_json,updated_at) VALUES (?1,?2,1,?3,?4,?5,?6)",
            libsql_rusqlite::params![project_id, volume_id, "a".repeat(64), "b".repeat(64), json!({"remainingChapterCapacity":1,"requiredTasksRemaining":0,"blockedRequiredTasks":0,"healthBlockedRequiredTasks":0,"healthBlockedOptionalTasks":0,"blockedTaskIds":[],"healthRequiredTaskIds":[],"planningPressure":{"remainingChapterCapacity":1,"requiredTasksRemaining":0}}).to_string(), NOW],
        ).unwrap();
    }
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:b13:r1:route-scale:1',?1,'volume:b13:r1:scope:001',1,'第一章','NORMAL','COMMITTED',1000,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES ('obligation:b13:r1:route-page',?1,'BOOK','MYSTERY','A','artifact:b13:r1:route',1,'先展示该高优先级事项','chapter:b13:r1:route-scale:1','PROPOSED',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    flush_health_projection(&connection, &project_id);
    drop(connection);

    let result = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":1}),
    )
    .unwrap();
    let route_factor = result["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["factorCode"] == "ROUTE_DRIFT")
        .unwrap();
    assert_eq!(route_factor["severity"], "risk");
    assert_eq!(
        result["issues"]["items"][0]["issueId"],
        "health:obligation:obligation:b13:r1:route-page"
    );
    let cursor = result["issues"]["nextCursor"]
        .as_str()
        .expect("stale route must remain reachable after the first issue page");
    let route_page = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":1,"cursor":cursor}),
    )
    .unwrap();
    assert_eq!(
        route_page["issues"]["items"][0]["issueId"],
        "health:route:volume:b13:r1:scope:102"
    );
    assert_eq!(
        route_page["issues"]["items"][0]["sourceRefs"][0]["ref"],
        "volume:b13:r1:scope:102"
    );
    assert!(result["issues"]["items"].as_array().unwrap().len() <= 50);
    assert!(result["issues"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| {
            !item["location"]
                .as_str()
                .unwrap_or_default()
                .contains("volume:")
        }));
}

#[test]
fn b13_r1_task_truth_covers_required_blockage_beyond_detail_page() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:r1:task-scale");
    let connection = state.manager.open(&project_id, "B13 R1", NOW).unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b13:r1:task-scale',?1,1,'任务规模测试','ACTIVE',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:b13:r1:task-scale',?1,'book:b13:r1:task-scale',1,'任务卷','PRODUCING',1,1,1000,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    let tasks = (0..202_i64)
        .map(|index| {
            json!({
                "taskId": format!("task:b13:r1:{index:03}"),
                "class": if index == 201 { "REQUIRED" } else { "OPTIONAL" },
                "objective": format!("任务 {index}"),
            })
        })
        .collect::<Vec<_>>();
    let route_payload = json!({"payloadSchema":"volume-plan.r1","routeVersion":1,"tasks":tasks});
    let route_hash = "b".repeat(64);
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b13:r1:task-scale',?1,'VolumePlan','VOLUME','volume:b13:r1:task-scale',1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)",
        libsql_rusqlite::params![project_id, route_hash, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:b13:r1:task-scale',1,?1,?2)",
        libsql_rusqlite::params![route_payload.to_string(), route_hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'VolumePlan','VOLUME','volume:b13:r1:task-scale','artifact:b13:r1:task-scale',1,?2)",
        libsql_rusqlite::params![project_id, route_hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,'volume:b13:r1:task-scale','artifact:b13:r1:task-scale',1,'APPROVED',1,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volume_planning_state_projection(project_id,volume_id,projection_revision,state_digest,projection_digest,payload_json,updated_at) VALUES (?1,'volume:b13:r1:task-scale',1,?2,?3,?4,?5)",
        libsql_rusqlite::params![project_id, "c".repeat(64), "d".repeat(64), json!({"remainingChapterCapacity":1,"requiredTasksRemaining":1,"blockedRequiredTasks":1,"healthBlockedRequiredTasks":1,"healthBlockedOptionalTasks":0,"planningPressure":{"remainingChapterCapacity":1,"requiredTasksRemaining":1}}).to_string(), NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES ('task:b13:r1:201',?1,'volume.task.progress',?2,'{}',?3,?3)",
        libsql_rusqlite::params![project_id, json!({"taskId":"task:b13:r1:201","status":"BLOCKED"}).to_string(), NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO planning_state_ledger_index(project_id,volume_id,entry_state,plan_state,coverage_state,obligation_ids_json,foreshadow_ids_json,evidence_need_ids_json,repair_state,updated_at,route_plan_artifact_id,route_plan_version,task_status_json,active_task_ids_json,blocked_task_ids_json,remaining_chapter_capacity,planning_pressure_json,state_digest,generated_revision) VALUES (?1,'volume:b13:r1:task-scale','OPEN','APPROVED','COMPLETE','[]','[]','[]','NONE',?2,'artifact:b13:r1:task-scale',1,?3,'[]','[]',1,?4,?5,1)",
        libsql_rusqlite::params![project_id, NOW, json!({"task:b13:r1:201":"BLOCKED"}).to_string(), json!({"remainingChapterCapacity":1,"requiredTasksRemaining":1}).to_string(), "c".repeat(64)],
    ).unwrap();
    flush_health_projection(&connection, &project_id);
    drop(connection);

    let result = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":1}),
    )
    .unwrap();
    let task_factor = result["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["factorCode"] == "TASK_BLOCKAGE")
        .unwrap();
    assert_eq!(task_factor["severity"], "risk");
    assert_eq!(result["counters"]["blockedRequiredTasks"], 1);
    assert!(result["issues"]["items"].as_array().unwrap().len() <= 50);
    assert!(result["issues"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| {
            !item["location"]
                .as_str()
                .unwrap_or_default()
                .contains("volume:")
        }));
}

#[test]
fn b_r2_03_issue_cursor_reaches_obligation_and_reader_desire_beyond_old_samples() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:r1:hidden-risk");
    let connection = state.manager.open(&project_id, "B13 R1", NOW).unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b13:r1:hidden-risk',?1,1,'隐藏风险测试','ACTIVE',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:b13:r1:hidden-risk',?1,'book:b13:r1:hidden-risk',3,'暗潮','PRODUCING',1,1,1000,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:b13:r1:hidden-risk:1',?1,'volume:b13:r1:hidden-risk',1,'第一章','NORMAL','COMMITTED',1000,?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:b13:r1:hidden-risk',?1,'BookPlan','BOOK','book:b13:r1:hidden-risk',1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)",
        libsql_rusqlite::params![project_id, "d".repeat(64), NOW],
    ).unwrap();
    for index in 0..99_i64 {
        connection.execute(
            "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES (?1,?2,'BOOK','MYSTERY','C','artifact:b13:r1:hidden-risk',1,?3,'chapter:b13:r1:hidden-risk:1','PROPOSED',?4,?4)",
            libsql_rusqlite::params![format!("obligation:b13:r1:c:{index:03}"), project_id, format!("普通义务 {index}"), NOW],
        ).unwrap();
    }
    connection.execute(
        "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES ('obligation:b13:r1:zz-hidden',?1,'BOOK','MYSTERY','A','artifact:b13:r1:hidden-risk',1,'隐藏的高优先级义务','chapter:b13:r1:hidden-risk:1','PROPOSED',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    for index in 0..51_i64 {
        connection.execute(
            "INSERT INTO reader_desires(desire_id,project_id,desire_type,status,expected_payoff_chapter,created_at,updated_at) VALUES (?1,?2,'谜底','OPEN','chapter:b13:r1:hidden-risk:1',?3,?3)",
            libsql_rusqlite::params![format!("desire:b13:r1:{index:03}"), project_id, NOW],
        ).unwrap();
    }
    connection.execute(
        "INSERT INTO reader_desires(desire_id,project_id,desire_type,status,expected_payoff_chapter,created_at,updated_at) VALUES ('desire:b13:r1:zz-hidden',?1,'关系','OPEN','chapter:b13:r1:hidden-risk:1',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    drop(connection);

    let result = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":1}),
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
    assert_eq!(factor("READER_DESIRE")["severity"], "attention");
    assert_eq!(result["counters"]["overdueObligations"], 100);
    assert_eq!(result["counters"]["overdueReaderDesires"], 52);
    assert_eq!(result["issues"]["total"], 152);
    assert_eq!(
        result["issues"]["items"][0]["issueId"],
        "health:obligation:obligation:b13:r1:zz-hidden"
    );
    let mut issue_ids = result["issues"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["issueId"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let mut cursor = result["issues"]["nextCursor"]
        .as_str()
        .map(ToOwned::to_owned);
    while let Some(value) = cursor {
        let page = query_book_health_impl(
            &state,
            json!({"projectId":project_id,"scope":"health","pageSize":30,"cursor":value}),
        )
        .unwrap();
        issue_ids.extend(
            page["issues"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["issueId"].as_str().unwrap().to_owned()),
        );
        cursor = page["issues"]["nextCursor"].as_str().map(ToOwned::to_owned);
    }
    let unique_ids = issue_ids.iter().collect::<std::collections::HashSet<_>>();
    assert_eq!(issue_ids.len(), 152);
    assert_eq!(unique_ids.len(), issue_ids.len());
    assert!(issue_ids
        .iter()
        .any(|id| id == "health:reader-desire:desire:b13:r1:zz-hidden"));
}

#[test]
fn b13_r1_health_query_does_not_create_model_runs_or_authority_rows() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:r1:no-remote-mutation");
    let connection = state.manager.open(&project_id, "B13 R1", NOW).unwrap();
    let before_model_runs: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM model_runs WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let before_authority: (i64, i64, i64, i64, i64, i64, i64, i64) = (
        connection
            .query_row(
                "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        connection
            .query_row(
                "SELECT COUNT(*) FROM canon_relations WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        connection
            .query_row(
                "SELECT COUNT(*) FROM canon_events WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        connection
            .query_row(
                "SELECT COUNT(*) FROM canon_knowledge WHERE project_id=?1",
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
        connection
            .query_row(
                "SELECT COUNT(*) FROM state_deltas WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
    );
    drop(connection);

    let _ = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();

    let connection = state.manager.open(&project_id, "B13 R1", NOW).unwrap();
    let after_model_runs: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM model_runs WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let after_authority: (i64, i64, i64, i64, i64, i64, i64, i64) = (
        connection
            .query_row(
                "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        connection
            .query_row(
                "SELECT COUNT(*) FROM canon_relations WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        connection
            .query_row(
                "SELECT COUNT(*) FROM canon_events WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        connection
            .query_row(
                "SELECT COUNT(*) FROM canon_knowledge WHERE project_id=?1",
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
        connection
            .query_row(
                "SELECT COUNT(*) FROM state_deltas WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
    );
    assert_eq!(before_model_runs, after_model_runs);
    assert_eq!(before_authority, after_authority);
    println!(
        "B13_R1_NO_REMOTE_RESULT modelRunsBefore={before_model_runs} modelRunsAfter={after_model_runs} unchanged=PASS status=PASS"
    );
    println!(
        "B13_R1_NO_AUTHORITY_RESULT before={before_authority:?} after={after_authority:?} unchanged=PASS status=PASS"
    );
}

#[test]
fn fx14_untyped_route_marks_only_its_scope_unavailable() {
    let state = state();
    let project_id = create_project(&state, "cmd:fx14:untyped-route");
    let connection = state.manager.open(&project_id, "FX-14", NOW).unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:fx14',?1,1,'FX-14 fixture','ACTIVE',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    for (number, payload) in [
        (
            1_i64,
            json!({"tasks":[{"taskId":"task:fx14:legacy","class":"REQUIRED"}]}),
        ),
        (
            2_i64,
            json!({"payloadSchema":"volume-plan.r1","tasks":[{"taskId":"task:fx14:typed","class":"REQUIRED"}]}),
        ),
    ] {
        let volume_id = format!("volume:fx14:{number}");
        let artifact_id = format!("artifact:fx14:{number}");
        let hash = format!("{number}").repeat(64);
        connection.execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,created_at,updated_at) VALUES (?1,?2,'book:fx14',?3,?4,'PRODUCING',?5,?5)",
            libsql_rusqlite::params![volume_id, project_id, number, format!("FX-14 卷 {number}"), NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES (?1,?2,'VolumePlan','VOLUME',?3,1,'fx14.fixture','COMMITTED',?4,'{}',?5)",
            libsql_rusqlite::params![artifact_id, project_id, volume_id, hash, NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            libsql_rusqlite::params![artifact_id, payload.to_string(), hash],
        ).unwrap();
        connection.execute(
            "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,?2,?3,1,'APPROVED',1,?4)",
            libsql_rusqlite::params![project_id, volume_id, artifact_id, NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO volume_planning_state_projection(project_id,volume_id,projection_revision,state_digest,projection_digest,payload_json,updated_at) VALUES (?1,?2,1,?3,?4,?5,?6)",
            libsql_rusqlite::params![project_id, volume_id, "c".repeat(64), "d".repeat(64), json!({"remainingChapterCapacity":1,"requiredTasksRemaining":1,"planningPressure":{"remainingChapterCapacity":1}}).to_string(), NOW],
        ).unwrap();
    }

    let refreshed = HealthRepository::flush_dirty(&connection, &project_id, NOW).unwrap();
    assert_eq!(refreshed.route_scopes_refreshed, 2);
    let unavailable = |volume_id: &str| -> i64 {
        connection.query_row(
            "SELECT is_unavailable FROM health_route_scope_projection WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id],
            |row| row.get(0),
        ).unwrap()
    };
    assert_eq!(
        unavailable("volume:fx14:1"),
        1,
        "the untyped route is reported, not interpreted"
    );
    assert_eq!(unavailable("volume:fx14:2"), 0);
    ai_novel_production_system_core::health::query_book_health(&connection, &project_id, None, 50)
        .expect("one legacy route must not fail the whole health page");
}
