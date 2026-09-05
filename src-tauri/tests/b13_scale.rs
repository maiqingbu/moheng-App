use ai_novel_production_system_core::health::query_book_health;
use ai_novel_production_system_core::project_db::connection::open_connection;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use tempfile::tempdir;

const NOW: &str = "2026-09-04T03:00:00Z";

fn explain(connection: &Connection, sql: &str) -> String {
    let mut statement = connection.prepare(sql).unwrap();
    let rows = statement
        .query_map(["scale"], |row| row.get::<_, String>(3))
        .unwrap();
    rows.collect::<Result<Vec<_>, _>>().unwrap().join(" | ")
}

fn factor<'a>(projection: &'a Value, code: &str) -> &'a Value {
    projection["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["factorCode"] == code)
        .unwrap()
}

#[test]
#[ignore = "B13 scale certification is deliberately opt-in"]
fn b13_scale_health_query_materializes_the_full_r1_fixture_and_keeps_truth_global() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("scale", "B13 scale", NOW).unwrap();
    connection
        .execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:scale','scale',1,'长篇规模测试','ACTIVE',?1,?1)",
            [NOW],
        )
        .unwrap();

    for volume_no in 1..=102_i64 {
        let volume_id = if volume_no == 1 {
            "volume:scale".to_string()
        } else {
            format!("volume:scale:{volume_no:03}")
        };
        connection
            .execute(
                "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,'scale','book:scale',?2,?3,'PRODUCING',1,1,100000,?4,?4)",
                libsql_rusqlite::params![
                    volume_id,
                    volume_no,
                    format!("规模卷 {volume_no}"),
                    NOW
                ],
            )
            .unwrap();
    }

    connection.execute_batch("BEGIN").unwrap();
    for chapter_no in 1..=4000_i64 {
        connection
            .execute(
                "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,'scale','volume:scale',?2,?3,'NORMAL','COMMITTED',2500,?4,?4)",
                libsql_rusqlite::params![
                    format!("chapter:scale:{chapter_no}"),
                    chapter_no,
                    format!("第 {chapter_no} 章"),
                    NOW
                ],
            )
            .unwrap();
    }
    connection.execute_batch("COMMIT").unwrap();

    let source_hash = "a".repeat(64);
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,authority_level,payload_hash,provenance_json,created_at,committed_at) VALUES ('artifact:scale:source','scale','BookPlan','BOOK','book:scale',1,'m0a.artifact.v1','COMMITTED','APPROVED_PLAN','B',?1,'{}',?2,?2)",
            libsql_rusqlite::params![source_hash, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:scale:source',1,'{}',?1)",
            [source_hash],
        )
        .unwrap();

    for index in 0..100_i64 {
        connection
            .execute(
                "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,lifecycle,created_at,updated_at) VALUES (?1,'scale','BOOK','LONG_FORM_FIXTURE','C','artifact:scale:source',1,?2,'chapter:scale:1','PROPOSED',?3,?3)",
                libsql_rusqlite::params![
                    format!("obligation:scale:{index:03}"),
                    format!("规模义务 {index}"),
                    NOW
                ],
            )
            .unwrap();
    }

    for index in 0..52_i64 {
        connection
            .execute(
                "INSERT INTO reader_desires(desire_id,project_id,desire_type,status,expected_payoff_chapter,created_at,updated_at) VALUES (?1,'scale','LONG_FORM_FIXTURE','OPEN','chapter:scale:1',?2,?2)",
                libsql_rusqlite::params![format!("desire:scale:{index:03}"), NOW],
            )
            .unwrap();
    }

    let tasks = (0..202_i64)
        .map(|index| {
            json!({
                "taskId": format!("task:scale:{index:03}"),
                "class": if index == 201 { "REQUIRED" } else { "OPTIONAL" },
                "objective": format!("规模任务 {index}"),
            })
        })
        .collect::<Vec<_>>();
    let route_payload = json!({
        "payloadSchema": "volume-plan.r1",
        "routeVersion": 1,
        "tasks": tasks,
    });
    let route_hash = "b".repeat(64);
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,authority_level,payload_hash,provenance_json,created_at,committed_at) VALUES ('artifact:scale:route','scale','VolumePlan','VOLUME','volume:scale',1,'m0a.artifact.v1','COMMITTED','APPROVED_PLAN','B',?1,'{}',?2,?2)",
            libsql_rusqlite::params![route_hash, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:scale:route',1,?1,?2)",
            libsql_rusqlite::params![route_payload.to_string(), route_hash],
        )
        .unwrap();

    for volume_no in 1..=102_i64 {
        let volume_id = if volume_no == 1 {
            "volume:scale".to_string()
        } else {
            format!("volume:scale:{volume_no:03}")
        };
        let route_artifact_id = (volume_no == 1).then_some("artifact:scale:route");
        let task_status = if volume_no == 1 {
            json!({"task:scale:201":"BLOCKED"}).to_string()
        } else {
            "{}".to_string()
        };
        connection
            .execute(
                "INSERT INTO planning_state_ledger_index(project_id,volume_id,entry_state,plan_state,coverage_state,obligation_ids_json,foreshadow_ids_json,evidence_need_ids_json,repair_state,updated_at,route_plan_artifact_id,route_plan_version,task_status_json,active_task_ids_json,blocked_task_ids_json,remaining_chapter_capacity,planning_pressure_json,state_digest,generated_revision) VALUES ('scale',?1,'OPEN',?2,'COMPLETE','[]','[]','[]','NONE',?3,?4,?5,?6,'[]','[]',10,?7,?8,1)",
                libsql_rusqlite::params![
                    volume_id,
                    if volume_no == 102 { "PLAN_STALE" } else { "APPROVED" },
                    NOW,
                    route_artifact_id,
                    route_artifact_id.map(|_| 1_i64),
                    task_status,
                    json!({"remainingChapterCapacity":10,"requiredTasksRemaining":if volume_no == 1 {1} else {0}}).to_string(),
                    "c".repeat(64),
                ],
            )
            .unwrap();
    }

    for index in 0..16_i64 {
        let run_id = format!("model-run:scale:{index:03}");
        connection
            .execute(
                "INSERT INTO model_runs(model_run_id,project_id,provider,model,idempotency_key,capability,predicted_cost_cents,status,created_at,updated_at) VALUES (?1,'scale','fixture','fixture',?2,'CHAPTER_CONTEXT',100,'COMPLETED',?3,?3)",
                libsql_rusqlite::params![run_id, format!("scale-idempotency:{index}"), NOW],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO model_usage(usage_id,project_id,model_run_id,input_tokens,output_tokens,total_tokens,latency_ms,cost_cents,recorded_at) VALUES (?1,'scale',?2,?3,100,?3,1,100,?4)",
                libsql_rusqlite::params![
                    format!("usage:scale:{index:03}"),
                    run_id,
                    1000 + index,
                    NOW
                ],
            )
            .unwrap();
    }

    for chapter_no in 1..=20_i64 {
        connection
            .execute(
                "INSERT INTO quality_trend_projection(project_id,sample_chapter_id,quality_vector_json,sample_chapter_no,sampled_at) VALUES ('scale',?1,?2,?3,?4)",
                libsql_rusqlite::params![
                    format!("chapter:scale:{chapter_no}"),
                    json!({"qualityScore":80}).to_string(),
                    chapter_no,
                    NOW
                ],
            )
            .unwrap();
    }

    let total_words: i64 = connection
        .query_row(
            "SELECT COALESCE(SUM(word_count),0) FROM chapters WHERE project_id='scale' AND status='COMMITTED'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(total_words >= 10_000_000);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM chapters WHERE project_id='scale'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        4000
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM narrative_obligations WHERE project_id='scale'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        100
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM planning_state_ledger_index WHERE project_id='scale'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        102
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM reader_desires WHERE project_id='scale'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        52
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_runs WHERE project_id='scale'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        16
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM quality_trend_projection WHERE project_id='scale'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        20
    );

    let small = query_book_health(&connection, "scale", None, 1).unwrap();
    let large = query_book_health(&connection, "scale", None, 50).unwrap();
    for field in ["score", "level", "coverage", "counters"] {
        assert_eq!(
            small[field], large[field],
            "global truth changed with page size for {field}"
        );
    }
    assert_eq!(
        factor(&small, "OBLIGATION_OVERDUE")["severity"],
        "attention"
    );
    assert_eq!(factor(&small, "ROUTE_DRIFT")["severity"], "risk");
    assert_eq!(factor(&small, "TASK_BLOCKAGE")["severity"], "risk");
    assert_eq!(factor(&small, "READER_DESIRE")["severity"], "attention");
    assert_eq!(small["counters"]["overdueObligations"], 100);
    assert_eq!(small["counters"]["blockedRequiredTasks"], 1);
    assert_eq!(small["counters"]["overdueReaderDesires"], 52);
    assert!(small["issues"]["items"].as_array().unwrap().len() <= 1);
    assert!(large["issues"]["items"].as_array().unwrap().len() <= 50);
    assert!(small["provenance"]["fullManuscriptScan"] == false);

    for (name, sql) in [
        (
            "H01",
            "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM narrative_obligations o LEFT JOIN chapters c ON c.project_id=o.project_id AND c.chapter_id=o.must_resolve_before_chapter WHERE o.project_id=?1",
        ),
        (
            "H02",
            "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM foreshadows f LEFT JOIN chapters c ON c.project_id=f.project_id AND c.chapter_id=f.recover_window_end_chapter WHERE f.project_id=?1",
        ),
        (
            "H03",
            "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM planning_state_ledger_index WHERE project_id=?1",
        ),
        (
            "H04",
            "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM planning_state_ledger_index l LEFT JOIN artifact_payloads p ON p.artifact_id=l.route_plan_artifact_id AND p.version=l.route_plan_version WHERE l.project_id=?1",
        ),
        (
            "H07",
            "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM reader_desires d LEFT JOIN chapters c ON c.project_id=d.project_id AND c.chapter_id=d.expected_payoff_chapter WHERE d.project_id=?1",
        ),
        (
            "ISSUES",
            "EXPLAIN QUERY PLAN SELECT obligation_id FROM narrative_obligations WHERE project_id=?1 ORDER BY obligation_id LIMIT 51",
        ),
    ] {
        let plan = explain(&connection, sql);
        assert!(!plan.is_empty(), "missing query plan for {name}");
        println!("B13_R1_EXPLAIN_{name}={plan}");
    }
    println!(
        "B13_R1_SCALE_RESULT words={total_words} chapters=4000 obligations=100 routeScopes=102 tasks=202 readerDesires=52 contextRuns=16 costRuns=16 qualitySamples=20 pageIndependentTruth=PASS noFullChapterBody=PASS status=PASS"
    );
    drop(connection);

    let reopened = open_connection(&temp.path().join("projects/scale/project.db"), true).unwrap();
    let _: Value = query_book_health(&reopened, "scale", None, 30).unwrap();
}
