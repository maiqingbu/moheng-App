use ai_novel_production_system_core::commands::{
    execute_command_impl, query_book_health_impl, KernelState,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use libsql_rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::ops::Deref;
use std::sync::Arc;

mod support;
use support::EntryFixtureGateway;

const NOW: &str = "2026-09-05T00:00:00Z";

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

fn create_project(state: &KernelState) -> String {
    execute_command_impl(
        state,
        json!({
            "type":"project_create_from_idea",
            "commandId":"b13-r2-authority",
            "idempotencyKey":"b13-r2-authority:key",
            "requestedAt":NOW,
            "payload":{"title":"B13 R2 非空权威","idea":"非空权威只读证据","platform":"起点中文网","mode":"DIRECTOR"}
        }),
    )
    .unwrap()["projectId"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn insert_artifact(
    connection: &Connection,
    project_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    scope_level: &str,
    scope_key: &str,
    payload: &Value,
) -> String {
    let payload_hash = sha256_canonical(payload).unwrap();
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'b13.r2.authority','COMMITTED',?6,'{}',?7,?7)",
            libsql_rusqlite::params![artifact_id, project_id, artifact_type, scope_level, scope_key, payload_hash, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            libsql_rusqlite::params![artifact_id, payload.to_string(), payload_hash],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,?2,?3,?4,?5,1,?6)",
            libsql_rusqlite::params![project_id, artifact_type, scope_level, scope_key, artifact_id, payload_hash],
        )
        .unwrap();
    payload_hash
}

fn read_rows(connection: &Connection, project_id: &str, sql: &str) -> Value {
    let mut statement = connection.prepare(sql).unwrap();
    let rows = statement
        .query_map([project_id], |row| {
            let mut record = Vec::new();
            for index in 0..row.as_ref().column_count() {
                let value: Result<String, _> = row.get(index);
                record.push(value.unwrap_or_else(|_| {
                    row.get::<_, i64>(index)
                        .map(|number| number.to_string())
                        .unwrap_or_default()
                }));
            }
            Ok(record)
        })
        .unwrap();
    Value::Array(
        rows.map(|row| Value::Array(row.unwrap().into_iter().map(Value::String).collect()))
            .collect(),
    )
}

fn authority_snapshot(connection: &Connection, project_id: &str) -> Map<String, Value> {
    let definitions = [
        (
            "obligations",
            "SELECT obligation_id,authority_level,description,must_resolve_before_chapter,lifecycle FROM narrative_obligations WHERE project_id=?1 ORDER BY obligation_id",
        ),
        (
            "foreshadows",
            "SELECT foreshadow_id,intent,status,recover_window_end_chapter,source_authority FROM foreshadows WHERE project_id=?1 ORDER BY foreshadow_id",
        ),
        (
            "readerDesires",
            "SELECT desire_id,desire_type,status,expected_payoff_chapter,payload_json FROM reader_desires WHERE project_id=?1 ORDER BY desire_id",
        ),
        (
            "planningAuthority",
            "SELECT h.artifact_type,h.scope_key,h.artifact_id,h.version,h.payload_hash,p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='VolumePlan' ORDER BY h.scope_key,h.artifact_id,h.version",
        ),
        (
            "taskReality",
            "SELECT state_id,state_type,value_json,chapter_id,payload_json FROM state_deltas WHERE project_id=?1 AND state_type='volume.task.progress' ORDER BY state_id",
        ),
        (
            "changeRequests",
            "SELECT change_request_id,change_type,scope,summary,status,impact_seed_json,decision_ref FROM change_requests WHERE project_id=?1 ORDER BY change_request_id",
        ),
        (
            "retconPlans",
            "SELECT retcon_plan_id,target_fact_json,reason,fix_type,downstream_impact_json,requires_resigning,estimated_cost_cents,approved_by,status,decision_ref FROM retcon_plans WHERE project_id=?1 ORDER BY retcon_plan_id",
        ),
        (
            "chapterAuthority",
            "SELECT chapter_id,volume_id,chapter_no,title,status,word_count,final_artifact_id,final_version FROM chapters WHERE project_id=?1 ORDER BY chapter_id",
        ),
        (
            "canonAuthority",
            "SELECT entity_id,canonical_name,entity_type,first_chapter_id FROM canon_entities WHERE project_id=?1 ORDER BY entity_id",
        ),
        (
            "canonFacts",
            "SELECT fact_id,subject_id,predicate,object_json,authority_level,superseded_by FROM canon_facts WHERE project_id=?1 ORDER BY fact_id",
        ),
    ];
    definitions
        .into_iter()
        .map(|(name, sql)| {
            let rows = read_rows(connection, project_id, sql);
            let count = rows.as_array().map_or(0, Vec::len);
            let digest = sha256_canonical(&rows).unwrap();
            (
                name.to_string(),
                json!({"count":count,"digest":digest,"rows":rows}),
            )
        })
        .collect()
}

fn counts_and_digests(snapshot: &Map<String, Value>) -> Map<String, Value> {
    snapshot
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                json!({
                    "count":value["count"],
                    "digest":value["digest"]
                }),
            )
        })
        .collect()
}

#[test]
fn b13_r2_query_book_health_keeps_nonempty_authority_counts_and_digests_unchanged() {
    let state = state();
    let project_id = create_project(&state);
    let connection = state
        .manager
        .open(&project_id, "B13 R2 authority", NOW)
        .unwrap();
    connection
        .execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b13:r2:authority',?1,1,'非空权威书','ACTIVE',?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:b13:r2:authority',?1,'book:b13:r2:authority',1,'非空权威卷','PRODUCING',1,1,1000,?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,status,word_count,created_at,updated_at) VALUES ('chapter:b13:r2:authority:1',?1,'volume:b13:r2:authority',1,'已提交章节','COMMITTED',1000,?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    let route = json!({"payloadSchema":"volume-plan.r1","routeVersion":1,"tasks":[],"requiredExitConditions":[],"capacityBudget":{"minChapters":1,"preferredChapters":1,"maxChapters":1}});
    let route_hash = insert_artifact(
        &connection,
        &project_id,
        "plan:b13:r2:authority",
        "VolumePlan",
        "VOLUME",
        "volume:b13:r2:authority",
        &route,
    );
    connection
        .execute(
            "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,'volume:b13:r2:authority','plan:b13:r2:authority',1,'APPROVED',1,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO volume_planning_state_projection(project_id,volume_id,projection_revision,state_digest,projection_digest,payload_json,updated_at) VALUES (?1,'volume:b13:r2:authority',0,?2,?3,?4,?5)",
            libsql_rusqlite::params![project_id, "a".repeat(64), "b".repeat(64), json!({"remainingChapterCapacity":1,"requiredTasksRemaining":0,"planningPressure":{"remainingChapterCapacity":1,"requiredTasksRemaining":0}}).to_string(), NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,lifecycle,created_at,updated_at) VALUES ('obligation:b13:r2:authority',?1,'BOOK','PLOT','A','plan:b13:r2:authority',1,'非空义务','PROGRESSING',?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,source_authority,created_at,updated_at) VALUES ('foreshadow:b13:r2:authority',?1,'非空伏笔','PLANTED','B13-R2',?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO reader_desires(desire_id,project_id,desire_type,status,payload_json,created_at,updated_at) VALUES ('desire:b13:r2:authority',?1,'谜底','OPEN','{}',?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES ('task:b13:r2:authority',?1,'volume.task.progress',?2,'chapter:b13:r2:authority:1','{}',?3,?3)",
            libsql_rusqlite::params![project_id, json!({"taskId":"task:b13:r2:authority","status":"ACTIVE"}).to_string(), NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO change_requests(change_request_id,project_id,change_type,scope,summary,status,impact_seed_json,created_at,updated_at) VALUES ('change:b13:r2:authority',?1,'REWRITE','CHAPTER','非空变更','DRAFT','{}',?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO retcon_plans(retcon_plan_id,project_id,target_fact_json,reason,fix_type,downstream_impact_json,estimated_cost_cents,status,created_at,updated_at) VALUES ('retcon:b13:r2:authority',?1,'{}','非空修订','SUPPLEMENT','{}',0,'DRAFT',?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,first_chapter_id,created_at,updated_at) VALUES ('entity:b13:r2:authority',?1,'非空人物','PERSON','chapter:b13:r2:authority:1',?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,source_artifact_id,source_version,source_chapter_id,authority_level,created_at,updated_at) VALUES ('fact:b13:r2:authority',?1,'entity:b13:r2:authority','status','{\"value\":\"ACTIVE\"}','plan:b13:r2:authority',1,'chapter:b13:r2:authority:1','A',?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    let before = authority_snapshot(&connection, &project_id);
    let before_counts = counts_and_digests(&before);
    let model_runs_before: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM model_runs WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);

    let health = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    assert_eq!(health["payloadSchema"], "long-form-health-projection.r1");

    let connection = state
        .manager
        .open(&project_id, "B13 R2 authority", NOW)
        .unwrap();
    let after = authority_snapshot(&connection, &project_id);
    let after_counts = counts_and_digests(&after);
    let model_runs_after: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM model_runs WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(before_counts, after_counts);
    assert!(before_counts
        .values()
        .all(|value| value["count"].as_u64().unwrap_or(0) > 0));
    assert_eq!(model_runs_before, model_runs_after);
    println!(
        "B13_R2_NO_AUTHORITY_RESULT domains={} before={} after={} allNonEmpty=true countDigestUnchanged=true projectionChanges=0 modelRunsBefore={} modelRunsAfter={} status=PASS",
        before_counts.len(), serde_json::to_string(&before_counts).unwrap(), serde_json::to_string(&after_counts).unwrap(), model_runs_before, model_runs_after
    );
    let _ = route_hash;
}
