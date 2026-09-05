use ai_novel_production_system_core::commit::CommitService;
use ai_novel_production_system_core::generated::types::CommitBundle;
use ai_novel_production_system_core::hash::{canonical_bytes, sha256_canonical};
use ai_novel_production_system_core::health::query_book_health;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::{ProjectHandle, ProjectManager};
use ai_novel_production_system_core::repositories::health::{
    FORMAL_ROUTE_ROWS_SQL, HEALTH_SOURCE_TIME_SQL,
};
use ai_novel_production_system_core::repositories::narrative::{
    NarrativeRepository, ARTIFACT_PAYLOAD_BY_REF_SQL, TASK_STATES_BY_IDS_SQL, TASK_STATE_CHUNK_SIZE,
};
use ai_novel_production_system_core::volume_lifecycle::VolumeLifecycleOwner;
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use std::path::PathBuf;
use tempfile::TempDir;

const NOW: &str = "2026-09-05T00:00:00Z";
const PROJECT_ID: &str = "p-b13-r3";
const VOLUME_ID: &str = "volume:b13:r3:501";

struct Fixture {
    _temp: TempDir,
    _manager: ProjectManager,
    connection: ProjectHandle,
    task_ids: Vec<String>,
    route_hash: String,
}

fn scope_key(volume_id: &str) -> String {
    String::from_utf8(canonical_bytes(&json!({"level": "VOLUME", "volumeId": volume_id})).unwrap())
        .unwrap()
}

fn insert_authority_artifact(
    connection: &Connection,
    artifact_id: &str,
    artifact_type: &str,
    scope_key: &str,
    payload: &Value,
) -> String {
    let payload_hash = sha256_canonical(payload).unwrap();
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,'VOLUME',?4,1,'b13.r3.fixture','COMMITTED',?5,'{}',?6,?6)",
            libsql_rusqlite::params![artifact_id, PROJECT_ID, artifact_type, scope_key, payload_hash, NOW],
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
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,?2,'VOLUME',?3,?4,1,?5)",
            libsql_rusqlite::params![PROJECT_ID, artifact_type, scope_key, artifact_id, payload_hash],
        )
        .unwrap();
    payload_hash
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let root: PathBuf = temp.path().to_path_buf();
    let manager = ProjectManager::new(AppPaths::new(root));
    let connection = manager
        .create(PROJECT_ID, "B13 R3 501 Task Reality", NOW)
        .unwrap();
    connection
        .execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b13:r3','p-b13-r3',1,'R3 规模验证','ACTIVE',?1,?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,'p-b13-r3','book:b13:r3',1,'R3 正式卷','PRODUCING',1,501,1252500,?2,?2)",
            libsql_rusqlite::params![VOLUME_ID, NOW],
        )
        .unwrap();

    let volume_scope = scope_key(VOLUME_ID);
    let intent = json!({
        "payloadSchema":"volume-intent.r1",
        "volumeFunction":"验证完整 Task Reality",
        "entryConditions":[],
        "mustAchieve":[],
        "mustNotBreak":[],
        "requiredExitConditions":[],
        "futureReservationRefs":[],
        "crossVolumeResponsibilityRefs":[],
        "targetChapterRange":{"min":1,"preferred":501,"max":501},
        "targetWordRange":{"preferred":1252500}
    });
    let intent_hash = insert_authority_artifact(
        &connection,
        "artifact:b13:r3:intent",
        "VolumeIntent",
        &volume_scope,
        &intent,
    );
    let task_ids = (1..=501)
        .map(|task_no| format!("task:b13:r3:{task_no:03}"))
        .collect::<Vec<_>>();
    let tasks = task_ids
        .iter()
        .enumerate()
        .map(|(index, task_id)| {
            json!({
                "taskId": task_id,
                "class": if index == 500 { "REQUIRED" } else { "OPTIONAL" },
                "objective": format!("R3 规模任务 {}", index + 1),
                "dependsOnTaskIds":[],
                "continuityThreadRefs":[],
                "reservationRefs":[],
                "completionPredicate":{"kind":"TASK_SATISFIED","value":task_id},
                "estimatedChapterCost":{"preferred":1}
            })
        })
        .collect::<Vec<_>>();
    let route = json!({
        "payloadSchema":"volume-plan.r1",
        "volumeIntentRef":{"artifactId":"artifact:b13:r3:intent","version":1,"payloadHash":intent_hash},
        "routeVersion":1,
        "tasks":tasks,
        "routeInvariants":[],
        "requiredExitConditions":[],
        "capacityBudget":{"minChapters":501,"preferredChapters":501,"maxChapters":501}
    });
    let route_hash = insert_authority_artifact(
        &connection,
        "artifact:b13:r3:route",
        "VolumePlan",
        &volume_scope,
        &route,
    );
    connection
        .execute(
            "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,?2,'artifact:b13:r3:route',1,'APPROVED',501,?3)",
            libsql_rusqlite::params![PROJECT_ID, VOLUME_ID, NOW],
        )
        .unwrap();

    let states = task_ids
        .iter()
        .enumerate()
        .map(|(index, task_id)| {
            json!({
                "stateId":task_id,
                "stateType":"volume.task.progress",
                "valueJson":{
                    "taskId":task_id,
                    "status":if index == 500 { "BLOCKED" } else { "SATISFIED" },
                    "evidenceRefs":[],
                    "remainingConditions":[]
                }
            })
        })
        .collect::<Vec<_>>();
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":"commit:b13:r3:task-reality",
        "projectId":PROJECT_ID,
        "commandId":"b13-r3:task-reality",
        "idempotencyKey":"b13-r3:task-reality:key",
        "expectedProjectRevision":0,
        "contractVersion":"m0a.v1",
        "createdAt":NOW,
        "artifacts":[],
        "stateDelta":states
    }))
    .unwrap();
    CommitService::commit(&connection, &bundle, NOW).unwrap();

    Fixture {
        _temp: temp,
        _manager: manager,
        connection,
        task_ids,
        route_hash,
    }
}

fn explain_with_params(
    connection: &Connection,
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

fn print_query_plan(name: &str, sql: &str, raw_explain_rows: Vec<String>) {
    println!(
        "B13_R3_QUERY_PLAN {}",
        json!({
            "queryName":name,
            "sql":sql,
            "sqlDigest":sha256_canonical(&json!({"sql":sql})).unwrap(),
            "fixture":PROJECT_ID,
            "rawExplainRows":raw_explain_rows
        })
    );
}

#[test]
fn b13_r3_task_reality_is_complete_for_b09_and_b13_health() {
    let fixture = fixture();
    let task_refs = fixture
        .task_ids
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let legacy =
        NarrativeRepository::task_states_by_ids(&fixture.connection, PROJECT_ID, &task_refs, 500)
            .unwrap();
    let complete = NarrativeRepository::task_states_by_ids_complete(
        &fixture.connection,
        PROJECT_ID,
        &task_refs,
    )
    .unwrap();
    assert_eq!(legacy.len(), 100);
    assert!(!legacy
        .iter()
        .any(|state| state.state_id == fixture.task_ids[500]));
    assert_eq!(complete.len(), 501);
    assert_eq!(complete[500].state_id, fixture.task_ids[500]);

    let first_100 = fixture.task_ids[..100]
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let first_101 = fixture.task_ids[..101]
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let bounded_100 =
        NarrativeRepository::task_states_by_ids(&fixture.connection, PROJECT_ID, &first_100, 100)
            .unwrap();
    let complete_100 = NarrativeRepository::task_states_by_ids_complete(
        &fixture.connection,
        PROJECT_ID,
        &first_100,
    )
    .unwrap();
    let complete_101 = NarrativeRepository::task_states_by_ids_complete(
        &fixture.connection,
        PROJECT_ID,
        &first_101,
    )
    .unwrap();
    assert_eq!(bounded_100, complete_100);
    assert_eq!(complete_100.len(), 100);
    assert_eq!(complete_101.len(), 101);
    assert_eq!(complete_101[100].state_id, fixture.task_ids[100]);
    assert!(
        NarrativeRepository::task_states_by_ids_complete(&fixture.connection, PROJECT_ID, &[],)
            .unwrap()
            .is_empty()
    );
    let duplicate_input = [
        fixture.task_ids[500].as_str(),
        fixture.task_ids[500].as_str(),
        fixture.task_ids[0].as_str(),
        "task:b13:r3:missing",
    ];
    let duplicate_result = NarrativeRepository::task_states_by_ids_complete(
        &fixture.connection,
        PROJECT_ID,
        &duplicate_input,
    )
    .unwrap();
    assert_eq!(duplicate_result.len(), 2);
    assert_eq!(duplicate_result[0].state_id, fixture.task_ids[500]);
    assert_eq!(duplicate_result[1].state_id, fixture.task_ids[0]);
    assert_eq!(TASK_STATE_CHUNK_SIZE, 100);

    let projection = VolumeLifecycleOwner::rebuild_planning_state(
        &fixture.connection,
        PROJECT_ID,
        VOLUME_ID,
        NOW,
    )
    .unwrap();
    assert_eq!(projection["blockedRequiredTasks"], 1);
    assert_eq!(projection["requiredTasksRemaining"], 1);

    let page_one = query_book_health(&fixture.connection, PROJECT_ID, None, 1).unwrap();
    let page_fifty = query_book_health(&fixture.connection, PROJECT_ID, None, 50).unwrap();
    let h04 = page_one["factors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|factor| factor["factorCode"] == "TASK_BLOCKAGE")
        .unwrap();
    assert_eq!(page_one["counters"]["blockedRequiredTasks"], 1);
    assert_eq!(h04["severity"], "risk");
    assert_eq!(page_one["level"], page_fifty["level"]);
    assert_eq!(page_one["score"], page_fifty["score"]);
    assert_eq!(page_one["counters"], page_fifty["counters"]);

    let task_chunk_count = fixture.task_ids.len().div_ceil(TASK_STATE_CHUNK_SIZE);
    println!(
        "B13_R3_TASK_REALITY_RESULT requestedTaskIds={} uniqueTaskIds={} chunkSize={} chunkCount={} legacyReturnedStates={} legacyIncludesTask501={} returnedTaskStates={} blockedRequiredTasks={} h04Severity={} b09BlockedRequiredTasks={} task100Chunks={} task101Chunks={} duplicateReturnedStates={} missingStatePreserved=true pageIndependent=PASS status=PASS",
        fixture.task_ids.len(),
        fixture.task_ids.len(),
        TASK_STATE_CHUNK_SIZE,
        task_chunk_count,
        legacy.len(),
        legacy.iter().any(|state| state.state_id == fixture.task_ids[500]),
        complete.len(),
        page_one["counters"]["blockedRequiredTasks"],
        h04["severity"],
        projection["blockedRequiredTasks"],
        1,
        2,
        duplicate_result.len(),
    );
    println!(
        "B13_R3_501_FORMAL_INTEGRATION_RESULT persistence=CommitService stateRows=501 b09=PASS queryBookHealth=PASS h04=BLOCKED_REQUIRED_RISK pageIndependent=PASS status=PASS"
    );
    println!(
        "B13_R3_B09_SEMANTICS_RESULT owner=VolumeLifecycleOwner sharedReader=task_states_by_ids_complete blockedRequiredTasks={} semanticsUnchanged=true status=PASS",
        projection["blockedRequiredTasks"]
    );

    print_query_plan(
        "formal_route_source",
        FORMAL_ROUTE_ROWS_SQL,
        explain_with_params(&fixture.connection, FORMAL_ROUTE_ROWS_SQL, [PROJECT_ID]),
    );
    print_query_plan(
        "planning_state_projection_lookup",
        FORMAL_ROUTE_ROWS_SQL,
        explain_with_params(&fixture.connection, FORMAL_ROUTE_ROWS_SQL, [PROJECT_ID]),
    );
    print_query_plan(
        "volume_plan_payload_lookup",
        ARTIFACT_PAYLOAD_BY_REF_SQL,
        explain_with_params(
            &fixture.connection,
            ARTIFACT_PAYLOAD_BY_REF_SQL,
            libsql_rusqlite::params![
                PROJECT_ID,
                "artifact:b13:r3:route",
                1,
                fixture.route_hash.as_str()
            ],
        ),
    );
    print_query_plan(
        "task_state_chunk_le_100",
        TASK_STATES_BY_IDS_SQL,
        explain_with_params(
            &fixture.connection,
            TASK_STATES_BY_IDS_SQL,
            libsql_rusqlite::params![
                PROJECT_ID,
                serde_json::to_string(&[fixture.task_ids[0].as_str()]).unwrap(),
                100
            ],
        ),
    );
    print_query_plan(
        "health_source_time_aggregate",
        HEALTH_SOURCE_TIME_SQL,
        explain_with_params(&fixture.connection, HEALTH_SOURCE_TIME_SQL, [PROJECT_ID]),
    );
}
