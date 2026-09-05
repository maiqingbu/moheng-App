use ai_novel_production_system_core::commands::{execute_command_impl, query_book_health_impl, KernelState};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::production::UnavailableProductionGateway;
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use std::sync::Arc;

const NOW: &str = "2026-09-05T00:00:00Z";

fn state() -> KernelState {
    let temp = tempfile::tempdir().unwrap();
    let root = Box::leak(Box::new(temp)).path().to_path_buf();
    KernelState::with_production_gateway(
        AppPaths::new(root),
        Arc::new(UnavailableProductionGateway),
    )
}

fn create_project(state: &KernelState) -> String {
    execute_command_impl(
        state,
        json!({
            "type":"project_create_from_idea",
            "commandId":"b13-r2-formal-owner",
            "idempotencyKey":"b13-r2-formal-owner:key",
            "requestedAt":NOW,
            "payload":{"title":"B13 R2 正式规划链","idea":"正式规划链测试","platform":"起点中文网","mode":"DIRECTOR"}
        }),
    )
    .unwrap()["projectId"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn insert_formal_route(connection: &Connection, project_id: &str) {
    let book_id = "book:b13:r2";
    let volume_id = "volume:b13:r2";
    let route_id = "plan:b13:r2";
    let scope = json!({"level":"VOLUME","volumeId":volume_id}).to_string();
    let route = json!({
        "payloadSchema":"volume-plan.r1",
        "routeVersion":1,
        "tasks":[
            {"taskId":"task:b13:r2:required","class":"REQUIRED","objective":"正式必需任务"},
            {"taskId":"task:b13:r2:optional","class":"OPTIONAL","objective":"正式可选任务"}
        ],
        "requiredExitConditions":[],
        "capacityBudget":{"minChapters":1,"preferredChapters":3,"maxChapters":3}
    });
    let route_hash = sha256_canonical(&route).unwrap();
    connection
        .execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'R2 正式链','ACTIVE',?3,?3)",
            libsql_rusqlite::params![book_id, project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'正式规划卷','PRODUCING',1,3,3000,?4,?4)",
            libsql_rusqlite::params![volume_id, project_id, book_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'VolumePlan','VOLUME',?3,1,'b13.r2.fixture','COMMITTED',?4,'{}',?5,?5)",
            libsql_rusqlite::params![route_id, project_id, scope, route_hash, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            libsql_rusqlite::params![route_id, route.to_string(), route_hash],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'VolumePlan','VOLUME',?2,?3,1,?4)",
            libsql_rusqlite::params![project_id, scope, route_id, route_hash],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,?2,?3,1,'APPROVED',3,?4)",
            libsql_rusqlite::params![project_id, volume_id, route_id, NOW],
        )
        .unwrap();
    let required_state = json!({"taskId":"task:b13:r2:required","status":"BLOCKED"});
    connection
        .execute(
            "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES ('task:b13:r2:required',?1,'volume.task.progress',?2,'{}',?3,?3)",
            libsql_rusqlite::params![project_id, required_state.to_string(), NOW],
        )
        .unwrap();
    let projection = json!({
        "projectId":project_id,
        "volumeId":volume_id,
        "generatedRevision":0,
        "routeRef":{"artifactId":route_id,"version":1,"payloadHash":route_hash},
        "activeTaskIds":["task:b13:r2:required"],
        "blockedTaskIds":["task:b13:r2:required"],
        "requiredTaskIds":["task:b13:r2:required"],
        "remainingTaskIds":["task:b13:r2:required"],
        "requiredTasksRemaining":1,
        "blockedRequiredTasks":1,
        "remainingChapterCapacity":0,
        "planningPressure":{"remainingChapterCapacity":0,"requiredTasksRemaining":1},
        "taskReality":[{"taskId":"task:b13:r2:required","status":"BLOCKED"}],
        "stateDigest":"formal-state",
        "projectionDigest":"formal-projection"
    });
    connection
        .execute(
            "INSERT INTO volume_planning_state_projection(project_id,volume_id,projection_revision,state_digest,projection_digest,payload_json,updated_at) VALUES (?1,?2,0,?3,?4,?5,?6)",
            libsql_rusqlite::params![
                project_id,
                volume_id,
                "d".repeat(64),
                "e".repeat(64),
                projection.to_string(),
                NOW
            ],
        )
        .unwrap();
    // Compatibility fields are deliberately contradictory. They must not be
    // used as H03/H04 truth once formal B07/B09 sources exist.
    connection
        .execute(
            "INSERT INTO planning_state_ledger_index(project_id,volume_id,entry_state,plan_state,coverage_state,obligation_ids_json,foreshadow_ids_json,evidence_need_ids_json,repair_state,updated_at,route_plan_artifact_id,route_plan_version,task_status_json,active_task_ids_json,blocked_task_ids_json,remaining_chapter_capacity,planning_pressure_json,state_digest,generated_revision) VALUES (?1,?2,'OPEN','APPROVED','COMPLETE','[]','[]','[]','NONE',?3,?4,1,?5,'[]','[]',99,?6,?7,0)",
            libsql_rusqlite::params![
                project_id,
                volume_id,
                NOW,
                route_id,
                json!({"task:b13:r2:required":"SATISFIED"}).to_string(),
                json!({"remainingChapterCapacity":99,"requiredTasksRemaining":0}).to_string(),
                "d".repeat(64)
            ],
        )
        .unwrap();
}

#[test]
fn b13_r2_formal_owner_truth_ignores_poisoned_compatibility_ledger() {
    let state = state();
    let project_id = create_project(&state);
    let connection = state.manager.open(&project_id, "B13 R2", NOW).unwrap();
    insert_formal_route(&connection, &project_id);
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
            .cloned()
            .unwrap_or(Value::Null)
    };
    assert_eq!(factor("ROUTE_DRIFT")["severity"], "risk");
    assert_eq!(factor("TASK_BLOCKAGE")["severity"], "risk");
    assert_eq!(result["counters"]["blockedRequiredTasks"], 1);
    println!("B13_R2_FORMAL_OWNER_RESULT formal_projection=PASS formal_task_reality=PASS compatibility_poisoning_ignored=PASS status=PASS");
}
