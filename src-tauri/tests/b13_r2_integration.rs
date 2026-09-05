use ai_novel_production_system_core::commands::{
    execute_command_impl, query_book_health_impl, KernelState,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::production::UnavailableProductionGateway;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};
use ai_novel_production_system_core::volume_lifecycle::VolumeLifecycleOwner;
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
            libsql_rusqlite::params![project_id, volume_id, route_id, route_hash],
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

fn insert_committed_artifact(
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
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'b13.r2.owner','COMMITTED',?6,'{}',?7,?7)",
            libsql_rusqlite::params![
                artifact_id,
                project_id,
                artifact_type,
                scope_level,
                scope_key,
                payload_hash,
                NOW
            ],
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
            libsql_rusqlite::params![
                project_id,
                artifact_type,
                scope_level,
                scope_key,
                artifact_id,
                payload_hash
            ],
        )
        .unwrap();
    payload_hash
}

#[test]
fn b13_r2_formal_owner_path_rebuilds_projection_without_compatibility_fields() {
    let state = state();
    let project_id = create_project(&state);
    let connection = state
        .manager
        .open(&project_id, "B13 R2 owner", NOW)
        .unwrap();
    let book_id = "book:b13:r2:owner";
    let volume_id =
        StableIdentityFactory::from_command(StableIdKind::Volume, "b13-r2-owner-volume").unwrap();
    connection
        .execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'Owner 链','ACTIVE',?3,?3)",
            libsql_rusqlite::params![book_id, project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'Owner 卷','PRODUCING',1,1,1000,?4,?4)",
            libsql_rusqlite::params![volume_id, project_id, book_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:b13:r2:owner:1',?1,?2,1,'第一章','NORMAL','COMMITTED',1000,?3,?3)",
            libsql_rusqlite::params![project_id, volume_id, NOW],
        )
        .unwrap();
    let book_scope = json!({"level":"PROJECT"}).to_string();
    let book_plan = json!({"planningMaturity":"APPROVED"});
    let _book_plan_hash = insert_committed_artifact(
        &connection,
        &project_id,
        "book-plan:b13:r2:owner",
        "BookPlan",
        "PROJECT",
        &book_scope,
        &book_plan,
    );
    drop(connection);

    let intent = json!({
        "payloadSchema":"volume-intent.r1",
        "volumeFunction":"Owner 正式规划",
        "entryConditions":[],
        "mustAchieve":[],
        "mustNotBreak":[],
        "requiredExitConditions":[],
        "futureReservationRefs":[],
        "crossVolumeResponsibilityRefs":[],
        "targetChapterRange":{"min":1,"preferred":1,"max":1},
        "targetWordRange":{"preferred":1000}
    });
    let intent_result = execute_command_impl(
        &state,
        json!({
            "type":"planning_create_volume_intent",
            "commandId":"b13-r2-owner-intent",
            "idempotencyKey":"b13-r2-owner-intent:key",
            "projectId":project_id,
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{"volumeId":volume_id,"plan":intent}
        }),
    )
    .unwrap();
    assert_eq!(intent_result["finalStatus"], "committed");
    let intent_ref = intent_result["receipt"]["committedArtifactRefs"][0].clone();
    let route = json!({
        "payloadSchema":"volume-plan.r1",
        "volumeIntentRef":intent_ref,
        "routeVersion":1,
        "tasks":[{"taskId":"task:b13:r2:owner:required","class":"REQUIRED","objective":"Owner 必需任务","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"TASK_SATISFIED","value":"task:b13:r2:owner:required"},"estimatedChapterCost":{"preferred":1}}],
        "routeInvariants":[],
        "requiredExitConditions":[],
        "capacityBudget":{"minChapters":1,"preferredChapters":1,"maxChapters":1}
    });
    let route_result = execute_command_impl(
        &state,
        json!({
            "type":"planning_create_volume_plan",
            "commandId":"b13-r2-owner-route",
            "idempotencyKey":"b13-r2-owner-route:key",
            "projectId":project_id,
            "expectedRevision":1,
            "requestedAt":NOW,
            "payload":{"volumeId":volume_id,"plan":route}
        }),
    )
    .unwrap();
    assert_eq!(route_result["finalStatus"], "committed");

    let connection = state
        .manager
        .open(&project_id, "B13 R2 owner", NOW)
        .unwrap();
    connection
        .execute(
            "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES ('task:b13:r2:owner:required',?1,'volume.task.progress',?2,'{}',?3,?3)",
            libsql_rusqlite::params![
                project_id,
                json!({"taskId":"task:b13:r2:owner:required","status":"BLOCKED"}).to_string(),
                NOW
            ],
        )
        .unwrap();
    let projection =
        VolumeLifecycleOwner::rebuild_planning_state(&connection, &project_id, &volume_id, NOW)
            .unwrap();
    assert_eq!(projection["remainingChapterCapacity"], 0);
    assert_eq!(projection["requiredTasksRemaining"], 1);
    // Keep the legacy compatibility projection at its schema defaults.
    connection
        .execute(
            "INSERT INTO planning_state_ledger_index(project_id,volume_id,entry_state,plan_state,coverage_state,obligation_ids_json,foreshadow_ids_json,evidence_need_ids_json,repair_state,updated_at) VALUES (?1,?2,'OPEN','APPROVED','INCOMPLETE','[]','[]','[]','NONE',?3)",
            libsql_rusqlite::params![project_id, volume_id, NOW],
        )
        .unwrap();
    let compatibility: (Option<String>, Option<i64>, String, Option<i64>) = connection
        .query_row(
            "SELECT route_plan_artifact_id,remaining_chapter_capacity,planning_pressure_json,generated_revision FROM planning_state_ledger_index WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(compatibility, (None, None, "{}".into(), Some(0)));
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
            .unwrap()
    };
    assert_eq!(factor("ROUTE_DRIFT")["severity"], "risk");
    assert_eq!(factor("TASK_BLOCKAGE")["severity"], "risk");
    assert_eq!(result["counters"]["blockedRequiredTasks"], 1);
    println!("B13_R2_FORMAL_OWNER_PATH_RESULT writer=B07::PlanningOwner projectionOwner=B09::PlanningStateProjectionOwner compatibilityDefaults=PASS health=PASS status=PASS");
}

#[test]
fn b13_r2_formal_owner_scale_covers_more_than_101_scopes_and_201_tasks() {
    let state = state();
    let project_id = create_project(&state);
    let connection = state
        .manager
        .open(&project_id, "B13 R2 scale", NOW)
        .unwrap();
    let book_id = "book:b13:r2:scale";
    connection
        .execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'R2 规模链','ACTIVE',?3,?3)",
            libsql_rusqlite::params![book_id, project_id, NOW],
        )
        .unwrap();

    let mut first_volume_id = String::new();
    for volume_no in 1..=102_i64 {
        let volume_id = format!("volume:b13:r2:scale:{volume_no:03}");
        if volume_no == 1 {
            first_volume_id = volume_id.clone();
        }
        connection
            .execute(
                "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'PRODUCING',1,1,1000,?6,?6)",
                libsql_rusqlite::params![
                    volume_id,
                    project_id,
                    book_id,
                    volume_no,
                    format!("R2 卷 {volume_no}"),
                    NOW
                ],
            )
            .unwrap();
        let intent_id = format!("intent:b13:r2:scale:{volume_no:03}");
        let intent = json!({
            "payloadSchema":"volume-intent.r1",
            "volumeFunction":format!("R2 规模卷 {volume_no}"),
            "entryConditions":[],"mustAchieve":[],"mustNotBreak":[],
            "requiredExitConditions":[],"futureReservationRefs":[],
            "crossVolumeResponsibilityRefs":[],
            "targetChapterRange":{"min":1,"preferred":1,"max":1},
            "targetWordRange":{"preferred":1000}
        });
        let intent_hash = insert_committed_artifact(
            &connection,
            &project_id,
            &intent_id,
            "VolumeIntent",
            "VOLUME",
            &volume_id,
            &intent,
        );
        let intent_ref = json!({"artifactId":intent_id,"version":1,"payloadHash":intent_hash});
        let tasks = if volume_no == 1 {
            (0..202_i64)
                .map(|index| {
                    json!({
                        "taskId":format!("task:b13:r2:scale:{index:03}"),
                        "class":if index == 201 {"REQUIRED"} else {"OPTIONAL"},
                        "objective":format!("R2 规模任务 {index}"),
                        "dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],
                        "completionPredicate":{"kind":"TASK_SATISFIED","value":format!("task:b13:r2:scale:{index:03}")},
                        "estimatedChapterCost":{"preferred":1}
                    })
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let route_id = format!("plan:b13:r2:scale:{volume_no:03}");
        let route = json!({
            "payloadSchema":"volume-plan.r1","volumeIntentRef":intent_ref,
            "routeVersion":1,"tasks":tasks,"routeInvariants":[],"requiredExitConditions":[],
            "capacityBudget":{"minChapters":1,"preferredChapters":1,"maxChapters":1}
        });
        let route_hash = insert_committed_artifact(
            &connection,
            &project_id,
            &route_id,
            "VolumePlan",
            "VOLUME",
            &volume_id,
            &route,
        );
        let _ = route_hash;
        connection
            .execute(
                "INSERT INTO planning_state_ledger_index(project_id,volume_id,entry_state,plan_state,coverage_state,obligation_ids_json,foreshadow_ids_json,evidence_need_ids_json,repair_state,updated_at) VALUES (?1,?2,'OPEN','APPROVED','INCOMPLETE','[]','[]','[]','NONE',?3)",
                libsql_rusqlite::params![project_id, volume_id, NOW],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES ('task:b13:r2:scale:201',?1,'volume.task.progress',?2,'{}',?3,?3)",
            libsql_rusqlite::params![
                project_id,
                json!({"taskId":"task:b13:r2:scale:201","status":"BLOCKED"}).to_string(),
                NOW
            ],
        )
        .unwrap();
    let projection = VolumeLifecycleOwner::rebuild_planning_state(
        &connection,
        &project_id,
        &first_volume_id,
        NOW,
    )
    .unwrap();
    assert_eq!(projection["requiredTasksRemaining"], 1);
    assert_eq!(projection["blockedRequiredTasks"], 1);
    for volume_no in 2..=102_i64 {
        let volume_id = format!("volume:b13:r2:scale:{volume_no:03}");
        let projection =
            VolumeLifecycleOwner::rebuild_planning_state(&connection, &project_id, &volume_id, NOW)
                .unwrap();
        assert_eq!(projection["requiredTasksRemaining"], 0);
    }
    drop(connection);

    let result = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":1}),
    )
    .unwrap();
    assert_eq!(result["counters"]["blockedRequiredTasks"], 1);
    assert_eq!(result["provenance"]["bounded"], true);
    assert_eq!(result["factors"].as_array().unwrap().len(), 8);
    println!("B13_R2_FORMAL_OWNER_SCALE_RESULT routeScopes=102 taskCount=202 lastRequiredBlocked=PASS pageIndependent=PASS status=PASS");
}

#[test]
fn b13_r2_formal_capacity_and_stale_boundaries_use_owner_sources() {
    let state = state();
    let project_id = create_project(&state);
    let connection = state
        .manager
        .open(&project_id, "B13 R2 boundaries", NOW)
        .unwrap();
    insert_formal_route(&connection, &project_id);
    drop(connection);

    let factor = |projection: &Value, code: &str| {
        projection["factors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["factorCode"] == code)
            .cloned()
            .unwrap()
    };
    let capacity_risk = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    assert_eq!(factor(&capacity_risk, "ROUTE_DRIFT")["severity"], "risk");

    let connection = state
        .manager
        .open(&project_id, "B13 R2 boundaries", NOW)
        .unwrap();
    connection
        .execute(
            "UPDATE volume_planning_state_projection SET state_digest=?1, payload_json=?2 WHERE project_id=?3 AND volume_id='volume:b13:r2'",
            libsql_rusqlite::params![
                "d".repeat(64),
                json!({"remainingChapterCapacity":8,"requiredTasksRemaining":1,"planningPressure":{"remainingChapterCapacity":8,"requiredTasksRemaining":1}}).to_string(),
                project_id
            ],
        )
        .unwrap();
    drop(connection);
    let capacity_attention = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    assert_eq!(
        factor(&capacity_attention, "ROUTE_DRIFT")["severity"],
        "attention"
    );

    let connection = state
        .manager
        .open(&project_id, "B13 R2 boundaries", NOW)
        .unwrap();
    connection
        .execute(
            "INSERT INTO stale_queue(stale_id,project_id,target_ref,stale_reason,revalidate_class,processed,created_at,updated_at) VALUES ('stale:b13:r2',?1,'plan:b13:r2','formal B06 stale test','REPLAN',0,?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    drop(connection);
    let stale = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    assert_eq!(factor(&stale, "ROUTE_DRIFT")["severity"], "risk");

    let connection = state
        .manager
        .open(&project_id, "B13 R2 boundaries", NOW)
        .unwrap();
    connection
        .execute(
            "DELETE FROM stale_queue WHERE project_id=?1 AND stale_id='stale:b13:r2'",
            [&project_id],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE project_meta SET revision=revision+1 WHERE project_id=?1",
            [&project_id],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE volume_planning_state_projection SET payload_json=?1 WHERE project_id=?2 AND volume_id='volume:b13:r2'",
            libsql_rusqlite::params![json!({"remainingChapterCapacity":10,"requiredTasksRemaining":0,"planningPressure":{"remainingChapterCapacity":10,"requiredTasksRemaining":0}}).to_string(), project_id],
        )
        .unwrap();
    drop(connection);
    let revision_only = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    assert_eq!(factor(&revision_only, "ROUTE_DRIFT")["severity"], "healthy");
    println!("B13_R2_FORMAL_BOUNDARY_RESULT capacity0=PASS capacity8=PASS staleQueue=PASS revisionOnlyDigestStable=PASS status=PASS");
}

#[test]
fn b13_r2_formal_task_classes_distinguish_optional_blocked_and_satisfied() {
    let state = state();
    let project_id = create_project(&state);
    let connection = state
        .manager
        .open(&project_id, "B13 R2 task classes", NOW)
        .unwrap();
    insert_formal_route(&connection, &project_id);
    connection
        .execute(
            "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES ('task:b13:r2:optional',?1,'volume.task.progress',?2,'{}',?3,?3)",
            libsql_rusqlite::params![
                project_id,
                json!({"taskId":"task:b13:r2:optional","status":"BLOCKED"}).to_string(),
                NOW
            ],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE state_deltas SET value_json=?1 WHERE project_id=?2 AND state_id='task:b13:r2:required'",
            libsql_rusqlite::params![json!({"taskId":"task:b13:r2:required","status":"SATISFIED"}).to_string(), project_id],
        )
        .unwrap();
    drop(connection);

    let result = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    let task_factor = result["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["factorCode"] == "TASK_BLOCKAGE")
        .unwrap();
    assert_eq!(task_factor["severity"], "attention");
    assert_eq!(result["counters"]["blockedRequiredTasks"], 0);
    assert_eq!(result["counters"]["blockedOptionalTasks"], 1);

    let connection = state
        .manager
        .open(&project_id, "B13 R2 task classes", NOW)
        .unwrap();
    connection
        .execute(
            "UPDATE state_deltas SET value_json=?1 WHERE project_id=?2 AND state_id='task:b13:r2:optional'",
            libsql_rusqlite::params![json!({"taskId":"task:b13:r2:optional","status":"SATISFIED"}).to_string(), project_id],
        )
        .unwrap();
    drop(connection);
    let satisfied = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    assert_eq!(satisfied["counters"]["blockedRequiredTasks"], 0);
    assert_eq!(satisfied["counters"]["blockedOptionalTasks"], 0);
    println!("B13_R2_TASK_CLASS_RESULT requiredBlocked=PASS optionalBlocked=PASS satisfiedNotBlocked=PASS status=PASS");
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
