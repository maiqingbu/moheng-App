use ai_novel_production_system_core::commands::{
    execute_command_impl, query_book_health_factor_sources_impl, query_book_health_impl,
    KernelState,
};
use ai_novel_production_system_core::commit::CommitService;
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repositories::health::HealthRepository;
use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};
use ai_novel_production_system_core::volume_lifecycle::VolumeLifecycleOwner;
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
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
    insert_formal_route_with_task_status(connection, project_id, "BLOCKED");
}

fn insert_formal_route_with_task_status(
    connection: &Connection,
    project_id: &str,
    required_status: &str,
) {
    let book_id = "book:b13:r2";
    let volume_id = "volume:b13:r2";
    let route_id = "plan:b13:r2";
    let intent_id = "intent:b13:r2";
    let required_task_id = "task:b13:r2:required";
    let is_blocked = required_status == "BLOCKED";
    let is_satisfied = required_status == "SATISFIED";
    let scope = json!({"level":"VOLUME","volumeId":volume_id}).to_string();
    let intent = json!({
        "payloadSchema":"volume-intent.r1",
        "volumeFunction":"正式 R2 任务真值验证",
        "entryConditions":[],
        "mustAchieve":[],
        "mustNotBreak":[],
        "requiredExitConditions":[],
        "futureReservationRefs":[],
        "crossVolumeResponsibilityRefs":[],
        "targetChapterRange":{"min":1,"preferred":3,"max":3},
        "targetWordRange":{"preferred":3000}
    });
    let intent_hash = sha256_canonical(&intent).unwrap();
    let route = json!({
        "payloadSchema":"volume-plan.r1",
        "volumeIntentRef":{"artifactId":intent_id,"version":1,"payloadHash":intent_hash},
        "routeVersion":1,
        "tasks":[
            {"taskId":required_task_id,"class":"REQUIRED","objective":"正式必需任务","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"TASK_SATISFIED","value":required_task_id},"estimatedChapterCost":{"preferred":1}},
            {"taskId":"task:b13:r2:optional","class":"OPTIONAL","objective":"正式可选任务","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"TASK_SATISFIED","value":"task:b13:r2:optional"},"estimatedChapterCost":{"preferred":1}}
        ],
        "routeInvariants":[],
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
    for chapter_no in
        1..=ai_novel_production_system_core::repositories::project_entry::OPENING_CHAPTER_COUNT
    {
        connection.execute(
            "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'NORMAL','COMMITTED',1000,?6,?6)",
            libsql_rusqlite::params![format!("chapter:b13:r2:{chapter_no}"),project_id,volume_id,chapter_no,format!("R2 已提交章节 {chapter_no}"),NOW],
        ).unwrap();
    }
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'VolumeIntent','VOLUME',?3,1,'b13.r2.fixture','COMMITTED',?4,'{}',?5,?5)",
            libsql_rusqlite::params![intent_id, project_id, volume_id, intent_hash, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            libsql_rusqlite::params![intent_id, intent.to_string(), intent_hash],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'VolumeIntent','VOLUME',?2,?3,1,?4)",
            libsql_rusqlite::params![project_id, volume_id, intent_id, intent_hash],
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
    let required_state = json!({"taskId":required_task_id,"status":required_status});
    connection
        .execute(
            "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES ('task:b13:r2:required',?1,'volume.task.progress',?2,'{}',?3,?3)",
            libsql_rusqlite::params![project_id, required_state.to_string(), NOW],
        )
        .unwrap();
    let projection =
        VolumeLifecycleOwner::rebuild_planning_state(connection, project_id, volume_id, NOW)
            .unwrap();
    assert_eq!(projection["remainingChapterCapacity"], 0);
    assert_eq!(
        projection["requiredTasksRemaining"],
        if is_satisfied { 0 } else { 1 }
    );
    assert_eq!(projection["blockedRequiredTasks"], i64::from(is_blocked));
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

fn insert_second_route_scope_for_task(
    connection: &Connection,
    project_id: &str,
    volume_id: &str,
    volume_no: i64,
    task_id: &str,
    task_class: &str,
) {
    let book_id = "book:b13:r2";
    let intent_id = format!("intent:{volume_id}");
    let route_id = format!("plan:{volume_id}");
    let intent = json!({
        "payloadSchema":"volume-intent.r1",
        "volumeFunction":format!("跨卷任务 {volume_no}"),
        "entryConditions":[],"mustAchieve":[],"mustNotBreak":[],
        "requiredExitConditions":[],"futureReservationRefs":[],
        "crossVolumeResponsibilityRefs":[],
        "targetChapterRange":{"min":1,"preferred":3,"max":3},
        "targetWordRange":{"preferred":3000}
    });
    let intent_hash = sha256_canonical(&intent).unwrap();
    let route = json!({
        "payloadSchema":"volume-plan.r1",
        "volumeIntentRef":{"artifactId":intent_id,"version":1,"payloadHash":intent_hash},
        "routeVersion":1,
        "tasks":[{"taskId":task_id,"class":task_class,"objective":"跨 scope 任务","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"TASK_SATISFIED","value":task_id},"estimatedChapterCost":{"preferred":1}}],
        "routeInvariants":[],"requiredExitConditions":[],
        "capacityBudget":{"minChapters":1,"preferredChapters":3,"maxChapters":3}
    });
    let route_hash = sha256_canonical(&route).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'PRODUCING',?6,?7,3000,?8,?8)",
        libsql_rusqlite::params![volume_id,project_id,book_id,volume_no,format!("R2 scope {volume_no}"),(volume_no-1)*3+1,volume_no*3,NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'VolumeIntent','VOLUME',?3,1,'b13.r5.fixture','COMMITTED',?4,'{}',?5,?5)",
        libsql_rusqlite::params![intent_id,project_id,volume_id,intent_hash,NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
        libsql_rusqlite::params![intent_id,intent.to_string(),intent_hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'VolumeIntent','VOLUME',?2,?3,1,?4)",
        libsql_rusqlite::params![project_id,volume_id,intent_id,intent_hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'VolumePlan','VOLUME',?3,1,'b13.r5.fixture','COMMITTED',?4,'{}',?5,?5)",
        libsql_rusqlite::params![route_id,project_id,volume_id,route_hash,NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
        libsql_rusqlite::params![route_id,route.to_string(),route_hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'VolumePlan','VOLUME',?2,?3,1,?4)",
        libsql_rusqlite::params![project_id,volume_id,route_id,route_hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,?2,?3,1,'APPROVED',3,?4)",
        libsql_rusqlite::params![project_id,volume_id,route_id,NOW],
    ).unwrap();
    VolumeLifecycleOwner::rebuild_planning_state(connection, project_id, volume_id, NOW).unwrap();
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

fn commit_task_progress(
    state: &KernelState,
    project_id: &str,
    task_id: &str,
    status: &str,
    command_id: &str,
) -> i64 {
    commit_task_state_value(
        state,
        project_id,
        task_id,
        json!({"taskId":task_id,"status":status,"evidenceRefs":[],"remainingConditions":[]}),
        command_id,
    )
}

fn commit_task_state_value(
    state: &KernelState,
    project_id: &str,
    task_id: &str,
    value: Value,
    command_id: &str,
) -> i64 {
    let connection = state
        .manager
        .open(project_id, "B-R2-03 R5 task truth", NOW)
        .unwrap();
    let revision = connection
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap();
    let bundle: ai_novel_production_system_core::generated::types::CommitBundle =
        serde_json::from_value(json!({
            "commitId":format!("commit:{command_id}"),
            "projectId":project_id,
            "commandId":command_id,
            "idempotencyKey":format!("{command_id}:key"),
            "expectedProjectRevision":revision,
            "contractVersion":"m0a.v1",
            "createdAt":NOW,
            "artifacts":[],
            "stateDelta":[{
                "stateId":task_id,
                "stateType":"volume.task.progress",
                "valueJson":value,
                "payloadJson":{"source":"R5 formal state transition"}
            }]
        }))
        .unwrap();
    CommitService::commit(&connection, &bundle, NOW).unwrap();
    drop(connection);
    revision + 1
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
    let book_plan_hash = insert_committed_artifact(
        &connection,
        &project_id,
        "book-plan:b13:r2:owner",
        "BookPlan",
        "PROJECT",
        &book_scope,
        &book_plan,
    );
    insert_committed_artifact(
        &connection,
        &project_id,
        "story-constitution:b13:r2:owner",
        "StoryConstitution",
        "PROJECT",
        &book_scope,
        &json!({"storyCore":{"protagonist":"Owner"}}),
    );
    insert_committed_artifact(
        &connection,
        &project_id,
        "main-contract:b13:r2:owner",
        "MainContract",
        "PROJECT",
        &book_scope,
        &json!({"hardRules":["keep evidence"]}),
    );
    drop(connection);

    let intent = json!({
        "payloadSchema":"volume-intent.r1",
        "volumeFunction":"Owner 正式规划",
        "entryConditions":[],
        "mustAchieve":[{"id":"goal:b13:r2:owner","description":"完成 Owner 证据任务"}],
        "mustNotBreak":[{"id":"rule:b13:r2:owner","description":"不得改变当前事实"}],
        "requiredExitConditions":[{"kind":"GOAL_ACHIEVED","value":"goal:b13:r2:owner"}],
        "futureReservationRefs":[],
        "crossVolumeResponsibilityRefs":[],
        "targetChapterRange":{"min":1,"preferred":1,"max":1},
        "targetWordRange":{"preferred":1000},
        "sourceBookPlanRef":{"artifactId":"book-plan:b13:r2:owner","version":1,"payloadHash":book_plan_hash},
        "compiledFromProjectRevision":0,
        "sourceEvidenceRefs":["evidence:b13:r2:owner"],
        "semanticDigest":"0".repeat(64)
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
        "tasks":[{"taskId":"task:b13:r2:owner:required","class":"REQUIRED","objective":"Owner 必需任务","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"TASK_EVIDENCE_RECORDED","value":{"taskId":"task:b13:r2:owner:required","evidenceType":"FINAL_CHAPTER","requiresNewEvidence":true}},"estimatedChapterCost":{"preferred":1}}],
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
            "INSERT INTO stale_queue(stale_id,project_id,target_ref,stale_reason,revalidate_class,processed,created_at,updated_at) VALUES ('stale:b13:r2',?1,'plan:b13:r2','formal B06 stale test','REPLAN',0,?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    HealthRepository::flush_dirty(&connection, &project_id, NOW).unwrap();
    drop(connection);
    let stale = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    assert_eq!(factor(&stale, "ROUTE_DRIFT")["severity"], "risk");
    assert!(stale["issues"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["title"] == "规划路线需要重新核对"));

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
    HealthRepository::flush_dirty(&connection, &project_id, NOW).unwrap();
    drop(connection);
    let revision_only = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    assert_eq!(factor(&revision_only, "ROUTE_DRIFT")["severity"], "risk");
    assert!(!revision_only["issues"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["title"] == "规划路线需要重新核对"));
    println!("B13_R2_FORMAL_BOUNDARY_RESULT capacityExhausted=PASS staleQueue=PASS staleQueueRemoval=PASS status=PASS");
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
    drop(connection);

    commit_task_progress(
        &state,
        &project_id,
        "task:b13:r2:required",
        "SATISFIED",
        "b13-r2-formal-required-satisfied",
    );
    commit_task_progress(
        &state,
        &project_id,
        "task:b13:r2:optional",
        "BLOCKED",
        "b13-r2-formal-optional-blocked",
    );

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
    assert_eq!(task_factor["sourceRefsTotal"], 1);
    assert_eq!(task_factor["sourceRefs"][0]["ref"], "task:b13:r2:optional");

    commit_task_progress(
        &state,
        &project_id,
        "task:b13:r2:optional",
        "SATISFIED",
        "b13-r2-formal-optional-satisfied",
    );
    let satisfied = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":30}),
    )
    .unwrap();
    assert_eq!(satisfied["counters"]["blockedRequiredTasks"], 0);
    assert_eq!(satisfied["counters"]["blockedOptionalTasks"], 0);
    let satisfied_factor = satisfied["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["factorCode"] == "TASK_BLOCKAGE")
        .unwrap();
    assert_eq!(satisfied_factor["severity"], "healthy");
    assert_eq!(satisfied_factor["sourceRefsTotal"], 0);
    println!("B13_R2_TASK_CLASS_RESULT requiredBlocked=PASS optionalBlocked=PASS satisfiedNotBlocked=PASS status=PASS");
}

#[test]
fn b_r2_03_r5_formal_task_state_is_the_single_health_truth_source() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(EntryFixtureGateway),
    );
    let project_id = create_project(&state);
    let connection = state
        .manager
        .open(&project_id, "B-R2-03 R5 task truth", NOW)
        .unwrap();
    insert_formal_route_with_task_status(&connection, &project_id, "NOT_STARTED");
    drop(connection);

    let health_at = |state: &KernelState| {
        query_book_health_impl(
            state,
            json!({"projectId":project_id,"scope":"health","pageSize":50}),
        )
        .unwrap()
    };
    let task_factor = |health: &Value| {
        health["factors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|factor| factor["factorCode"] == "TASK_BLOCKAGE")
            .cloned()
            .unwrap()
    };
    let source_page = |state: &KernelState| {
        query_book_health_factor_sources_impl(
            state,
            json!({"projectId":project_id,"factorCode":"TASK_BLOCKAGE","pageSize":50}),
        )
        .unwrap()
    };

    let not_started = health_at(&state);
    assert_eq!(not_started["counters"]["blockedRequiredTasks"], 0);
    assert_eq!(task_factor(&not_started)["severity"], "healthy");
    assert_eq!(source_page(&state)["total"], 0);

    let revision = commit_task_progress(
        &state,
        &project_id,
        "task:b13:r2:required",
        "BLOCKED",
        "b-r2-03-r5-task-blocked",
    );
    let blocked = health_at(&state);
    assert_eq!(blocked["projectRevision"], revision);
    assert_eq!(blocked["counters"]["blockedRequiredTasks"], 1);
    assert_eq!(blocked["counters"]["blockedOptionalTasks"], 0);
    assert_eq!(task_factor(&blocked)["status"], "EVALUATED");
    assert_eq!(task_factor(&blocked)["severity"], "risk");
    assert!(blocked["issues"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["issueId"] == "health:task:task:b13:r2:required"));
    assert_eq!(
        source_page(&state)["items"][0]["ref"],
        "task:b13:r2:required"
    );
    assert_eq!(source_page(&state)["total"], 1);

    // Let the formal B09 owner persist its normal projection, then change only
    // the formal state delta. Health must prefer that state over B09's now-stale
    // blockedTaskIds and blocked counters.
    let connection = state
        .manager
        .open(&project_id, "B-R2-03 R5 task truth", NOW)
        .unwrap();
    let planning = VolumeLifecycleOwner::rebuild_planning_state(
        &connection,
        &project_id,
        "volume:b13:r2",
        NOW,
    )
    .unwrap();
    assert_eq!(planning["blockedTaskIds"][0], "task:b13:r2:required");
    drop(connection);

    let revision = commit_task_progress(
        &state,
        &project_id,
        "task:b13:r2:required",
        "SATISFIED",
        "b-r2-03-r5-task-satisfied",
    );
    let satisfied = health_at(&state);
    assert_eq!(satisfied["projectRevision"], revision);
    assert_eq!(satisfied["counters"]["blockedRequiredTasks"], 0);
    assert_eq!(task_factor(&satisfied)["severity"], "healthy");
    assert_eq!(source_page(&state)["total"], 0);
    let stale_planning_blocked: String = state
        .manager
        .with_read_connection(&project_id, NOW, |connection| {
            connection.query_row(
                "SELECT json_extract(payload_json,'$.blockedTaskIds[0]') FROM volume_planning_state_projection WHERE project_id=?1 AND volume_id='volume:b13:r2'",
                [project_id.as_str()],
                |row| row.get(0),
            ).map_err(Into::into)
        })
        .unwrap();
    assert_eq!(stale_planning_blocked, "task:b13:r2:required");

    let revision = commit_task_progress(
        &state,
        &project_id,
        "task:b13:r2:required",
        "INVALIDATED",
        "b-r2-03-r5-task-invalidated",
    );
    let invalidated = health_at(&state);
    assert_eq!(invalidated["projectRevision"], revision);
    assert_eq!(invalidated["counters"]["blockedRequiredTasks"], 0);
    assert_eq!(task_factor(&invalidated)["severity"], "healthy");
    assert_eq!(source_page(&state)["total"], 0);

    let revision = commit_task_progress(
        &state,
        &project_id,
        "task:b13:r2:optional",
        "BLOCKED",
        "b-r2-03-r5-optional-blocked",
    );
    let optional_blocked = health_at(&state);
    assert_eq!(optional_blocked["projectRevision"], revision);
    assert_eq!(optional_blocked["counters"]["blockedRequiredTasks"], 0);
    assert_eq!(optional_blocked["counters"]["blockedOptionalTasks"], 1);
    assert_eq!(task_factor(&optional_blocked)["severity"], "attention");
    assert_eq!(
        source_page(&state)["items"][0]["ref"],
        "task:b13:r2:optional"
    );

    let revision = commit_task_progress(
        &state,
        &project_id,
        "task:b13:r2:optional",
        "INVALIDATED",
        "b-r2-03-r5-optional-invalidated",
    );
    let optional_invalidated = health_at(&state);
    assert_eq!(optional_invalidated["projectRevision"], revision);
    assert_eq!(optional_invalidated["counters"]["blockedOptionalTasks"], 0);
    assert_eq!(task_factor(&optional_invalidated)["severity"], "healthy");
    assert_eq!(source_page(&state)["total"], 0);

    let database = AppPaths::new(temp.path())
        .project_dir(&project_id)
        .unwrap()
        .join("project.db");
    let connection =
        ai_novel_production_system_core::project_db::connection::open_connection(&database, false)
            .unwrap();
    // Fault-inject a malformed persisted Task Reality row; ordinary task
    // transitions above and the repair below use formal CommitBundle owners.
    connection
        .execute(
            "UPDATE state_deltas SET value_json=?1 WHERE project_id=?2 AND state_id='task:b13:r2:required' AND state_type='volume.task.progress'",
            libsql_rusqlite::params![json!({"taskId":"task:b13:r2:required","evidenceRefs":[],"remainingConditions":[]}).to_string(),project_id],
        )
        .unwrap();
    let b09_failure = VolumeLifecycleOwner::rebuild_planning_state(
        &connection,
        &project_id,
        "volume:b13:r2",
        NOW,
    )
    .unwrap_err();
    assert!(b09_failure.starts_with("PLANNING_TASK_STATE_STATUS_UNAVAILABLE"));
    let missing_status = health_at(&state);
    assert_eq!(task_factor(&missing_status)["status"], "SOURCE_UNAVAILABLE");
    assert_eq!(source_page(&state)["total"], 0);
    let dirty_count: i64 = state
        .manager
        .with_read_connection(&project_id, NOW, |connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM health_projection_dirty WHERE project_id=?1",
                    [project_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(Into::into)
        })
        .unwrap();
    assert_eq!(dirty_count, 1);

    let revision: i64 = connection
        .query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    let recovered_bundle: ai_novel_production_system_core::generated::types::CommitBundle =
        serde_json::from_value(json!({
            "commitId":"commit:b-r2-03-r5-task-status-recovered",
            "projectId":project_id,
            "commandId":"b-r2-03-r5-task-status-recovered",
            "idempotencyKey":"b-r2-03-r5-task-status-recovered:key",
            "expectedProjectRevision":revision,
            "contractVersion":"m0a.v1",
            "createdAt":NOW,
            "artifacts":[],
            "stateDelta":[{"stateId":"task:b13:r2:required","stateType":"volume.task.progress","valueJson":{"taskId":"task:b13:r2:required","status":"NOT_STARTED","evidenceRefs":[],"remainingConditions":[]}}]
        }))
        .unwrap();
    CommitService::commit(&connection, &recovered_bundle, NOW).unwrap();
    VolumeLifecycleOwner::rebuild_planning_state(&connection, &project_id, "volume:b13:r2", NOW)
        .unwrap();
    drop(connection);
    let recovered = health_at(&state);
    assert_eq!(task_factor(&recovered)["status"], "EVALUATED");
    assert_eq!(task_factor(&recovered)["severity"], "healthy");
    println!("B_R2_03_R5_TASK_TRUTH_RESULT formalCommitBundleOnly=true blocked=PASS satisfied=PASS invalidated=PASS optional=PASS stalePlanningProjectionIgnored=true noManualHealthWrites=true status=PASS");
}

#[test]
fn b_r2_03_r5_cross_scope_task_counts_are_deduplicated_and_conflicts_fail_closed() {
    for (suffix, second_class, expected_status) in [
        ("same", "REQUIRED", "EVALUATED"),
        ("conflict", "OPTIONAL", "SOURCE_UNAVAILABLE"),
    ] {
        let state = state();
        let project_id = create_project(&state);
        let connection = state
            .manager
            .open(&project_id, "B-R2-03 R5 cross-scope", NOW)
            .unwrap();
        insert_formal_route_with_task_status(&connection, &project_id, "NOT_STARTED");
        insert_second_route_scope_for_task(
            &connection,
            &project_id,
            &format!("volume:b13:r2:second:{suffix}"),
            2,
            "task:b13:r2:required",
            second_class,
        );
        drop(connection);

        commit_task_progress(
            &state,
            &project_id,
            "task:b13:r2:required",
            "BLOCKED",
            &format!("b-r2-03-r5-cross-scope-{suffix}"),
        );
        let health = query_book_health_impl(
            &state,
            json!({"projectId":project_id,"scope":"health","pageSize":50}),
        )
        .unwrap();
        let factor = health["factors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|factor| factor["factorCode"] == "TASK_BLOCKAGE")
            .unwrap();
        assert_eq!(factor["status"], expected_status);
        assert_eq!(health["counters"]["blockedRequiredTasks"], 1);
        assert_eq!(
            health["issues"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|issue| issue["issueId"] == "health:task:task:b13:r2:required")
                .count(),
            1
        );
        let sources = query_book_health_factor_sources_impl(
            &state,
            json!({"projectId":project_id,"factorCode":"TASK_BLOCKAGE","pageSize":50}),
        )
        .unwrap();
        assert_eq!(sources["total"], 1);
        assert_eq!(sources["items"].as_array().unwrap().len(), 1);
    }
    println!("B_R2_03_R5_CROSS_SCOPE_RESULT sameClassDedup=PASS classConflictFailClosed=PASS countersIssuesSourcesAligned=true status=PASS");
}

#[test]
fn b_r2_03_r5_route_capacity_attention_comes_from_b09_owner_rebuild() {
    let state = state();
    let project_id = create_project(&state);
    let connection = state
        .manager
        .open(&project_id, "B-R2-03 R5 capacity attention", NOW)
        .unwrap();
    let book_id = "book:b-r2-03:r5-capacity";
    let volume_id = "volume:b-r2-03:r5-capacity";
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'R5 容量 attention','ACTIVE',?3,?3)",
        libsql_rusqlite::params![book_id,project_id,NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'十章容量卷','PRODUCING',1,10,10000,?4,?4)",
        libsql_rusqlite::params![volume_id,project_id,book_id,NOW],
    ).unwrap();
    for chapter_no in 1..=2_i64 {
        connection.execute(
            "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'NORMAL','COMMITTED',1000,?6,?6)",
            libsql_rusqlite::params![format!("chapter:r5:capacity:{chapter_no}"),project_id,volume_id,chapter_no,format!("R5 章节 {chapter_no}"),NOW],
        ).unwrap();
    }
    let intent_id = "intent:r5:capacity";
    let intent = json!({
        "payloadSchema":"volume-intent.r1","volumeFunction":"保持十章容量中的关键任务安排",
        "entryConditions":[],"mustAchieve":[],"mustNotBreak":[],"requiredExitConditions":[],
        "futureReservationRefs":[],"crossVolumeResponsibilityRefs":[],
        "targetChapterRange":{"min":1,"preferred":10,"max":10},"targetWordRange":{"preferred":10000}
    });
    let intent_hash = insert_committed_artifact(
        &connection,
        &project_id,
        intent_id,
        "VolumeIntent",
        "VOLUME",
        volume_id,
        &intent,
    );
    let intent_ref = json!({"artifactId":intent_id,"version":1,"payloadHash":intent_hash});
    let route_id = "plan:r5:capacity";
    let route = json!({
        "payloadSchema":"volume-plan.r1","volumeIntentRef":intent_ref,"routeVersion":1,
        "tasks":[{"taskId":"task:r5:capacity:required","class":"REQUIRED","objective":"完成关键任务","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"TASK_SATISFIED","value":"task:r5:capacity:required"},"estimatedChapterCost":{"preferred":1}}],
        "routeInvariants":[],"requiredExitConditions":[],"capacityBudget":{"minChapters":1,"preferredChapters":10,"maxChapters":10}
    });
    insert_committed_artifact(
        &connection,
        &project_id,
        route_id,
        "VolumePlan",
        "VOLUME",
        volume_id,
        &route,
    );
    connection.execute(
        "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,?2,?3,1,'APPROVED',10,?4)",
        libsql_rusqlite::params![project_id,volume_id,route_id,NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,payload_json,created_at,updated_at) VALUES ('task:r5:capacity:required',?1,'volume.task.progress',?2,'{}',?3,?3)",
        libsql_rusqlite::params![project_id,json!({"taskId":"task:r5:capacity:required","status":"NOT_STARTED"}).to_string(),NOW],
    ).unwrap();
    let projection =
        VolumeLifecycleOwner::rebuild_planning_state(&connection, &project_id, volume_id, NOW)
            .unwrap();
    assert_eq!(projection["remainingChapterCapacity"], 8);
    assert_eq!(projection["requiredTasksRemaining"], 1);
    drop(connection);

    let health = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":50}),
    )
    .unwrap();
    let route_factor = health["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|factor| factor["factorCode"] == "ROUTE_DRIFT")
        .unwrap();
    assert_eq!(route_factor["status"], "EVALUATED");
    assert_eq!(route_factor["severity"], "attention");
    assert_eq!(health["counters"]["blockedRequiredTasks"], 0);
    assert_eq!(
        health["provenance"]["sourceRevision"],
        health["projectRevision"]
    );
    println!("B_R2_03_R5_CAPACITY_OWNER_RESULT remainingCapacity=8 requiredRemaining=1 routeAttention=PASS b09OwnerRebuild=PASS status=PASS");
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
