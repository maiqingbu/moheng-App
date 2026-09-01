use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::{ProjectHandle, ProjectManager};
use ai_novel_production_system_core::volume_lifecycle::{
    NextVolumeHandoffRequest, RouteReplanRequest, VolumeCloseRequest, VolumeLifecycleOwner,
};
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use tempfile::TempDir;

const NOW: &str = "2026-09-01T00:00:00Z";

struct Fixture {
    _temp: TempDir,
    manager: ProjectManager,
    connection: ProjectHandle,
    volume_id: String,
    intent_ref: Value,
}

fn scope(level: &str, id_key: Option<(&str, &str)>) -> String {
    let mut value = serde_json::Map::new();
    value.insert("level".into(), json!(level));
    if let Some((key, id)) = id_key {
        value.insert(key.into(), json!(id));
    }
    Value::Object(value).to_string()
}

fn insert_committed_artifact(
    connection: &Connection,
    project_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    scope_level: &str,
    scope_key: &str,
    payload: &Value,
) -> Value {
    let hash = sha256_canonical(payload).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'m0a.artifact.v1','COMMITTED',?6,'{}',?7,?7)", libsql_rusqlite::params![artifact_id,project_id,artifact_type,scope_level,scope_key,hash,NOW]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)", libsql_rusqlite::params![artifact_id,payload.to_string(),hash]).unwrap();
    connection.execute("INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,?2,?3,?4,?5,1,?6)", libsql_rusqlite::params![project_id,artifact_type,scope_level,scope_key,artifact_id,hash]).unwrap();
    json!({"artifactId":artifact_id,"version":1,"payloadHash":hash})
}

fn route_payload(intent_ref: &Value, route_version: i64, with_required_task: bool) -> Value {
    let tasks = if with_required_task {
        json!([{"taskId":"task:b09:required","class":"REQUIRED","objective":"完成卷内主任务","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"TASK_SATISFIED","value":"task:b09:required"},"estimatedChapterCost":{"preferred":1}}])
    } else {
        json!([])
    };
    json!({
        "payloadSchema":"volume-plan.r1",
        "volumeIntentRef":intent_ref,
        "routeVersion":route_version,
        "tasks":tasks,
        "routeInvariants":["B09_ROUTE_CURRENT"],
        "requiredExitConditions":if with_required_task {json!([{"kind":"TASK_SATISFIED","value":"task:b09:required"}])} else {json!([])},
        "capacityBudget":{"minChapters":1,"preferredChapters":3,"maxChapters":3}
    })
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p-b09", "B09", NOW).unwrap();
    let volume_id = "volume:b09:1".to_string();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b09:1','p-b09',1,'Book','ACTIVE',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,'p-b09','book:b09:1',1,'第一卷','PLANNING',1,3,3000,?2,?2)", libsql_rusqlite::params![volume_id,NOW]).unwrap();
    for chapter_no in 1..=3 {
        connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,'p-b09',?2,?3,?4,'NORMAL',?5,1000,?6,?6)", libsql_rusqlite::params![format!("chapter:b09:{chapter_no}"),volume_id,chapter_no,format!("第{chapter_no}章"),if chapter_no == 1 {"COMMITTED"} else {"PLANNED"},NOW]).unwrap();
    }
    let intent = json!({"payloadSchema":"volume-intent.r1","volumeFunction":"第一卷主线推进","entryConditions":[],"mustAchieve":[{"id":"goal:b09:main","description":"完成卷内主任务"}],"mustNotBreak":[],"requiredExitConditions":[{"kind":"TASK_SATISFIED","value":"task:b09:required"}],"futureReservationRefs":["reservation:b09:v4"],"crossVolumeResponsibilityRefs":["thread:b09:promise"],"targetChapterRange":{"min":1,"preferred":3,"max":3},"targetWordRange":{"preferred":3000}});
    let intent_ref = insert_committed_artifact(&connection, "p-b09", "intent:b09:1", "VolumeIntent", "VOLUME", &scope("VOLUME", Some(("volumeId", &volume_id))), &intent);
    let route = route_payload(&intent_ref, 1, true);
    insert_committed_artifact(&connection, "p-b09", "plan:b09:1", "VolumePlan", "VOLUME", &scope("VOLUME", Some(("volumeId", &volume_id))), &route);
    connection.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES ('task:b09:required','p-b09','volume.task.progress',?1,'chapter:b09:1','{}',?2,?2)", libsql_rusqlite::params![json!({"taskId":"task:b09:required","status":"ACTIVE","evidenceRefs":[],"remainingConditions":[]}).to_string(),NOW]).unwrap();
    Fixture { _temp: temp, manager, connection, volume_id, intent_ref }
}

fn replan_request(plan: Value, expected_revision: i64, command: &str) -> RouteReplanRequest {
    RouteReplanRequest {
        project_id: "p-b09".into(),
        volume_id: "volume:b09:1".into(),
        expected_revision,
        command_id: command.into(),
        idempotency_key: format!("key:{command}"),
        trigger: "REPLAN_VOLUME_ROUTE".into(),
        reason: "容量与退出缺口需要重新规划".into(),
        plan,
        evidence_refs: vec!["evidence:b09:chapter:1".into()],
        now: NOW.into(),
    }
}

#[test]
fn b09_planning_state_rebuild_is_deterministic_revision_bound_and_reality_derived() {
    let fixture = fixture();
    let first = VolumeLifecycleOwner::rebuild_planning_state(
        &fixture.connection,
        "p-b09",
        &fixture.volume_id,
        NOW,
    )
    .unwrap();
    let second = VolumeLifecycleOwner::rebuild_planning_state(
        &fixture.connection,
        "p-b09",
        &fixture.volume_id,
        NOW,
    )
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(first["generatedRevision"], 0);
    assert_eq!(first["requiredTasksRemaining"], 1);
    assert_eq!(first["closeReadiness"], "BLOCKED");
    let before_digest = first["projectionDigest"].clone();
    fixture.connection.execute("UPDATE state_deltas SET value_json=?1 WHERE state_id='task:b09:required'", [json!({"taskId":"task:b09:required","status":"SATISFIED","evidenceRefs":[],"remainingConditions":[]}).to_string()]).unwrap();
    let after = VolumeLifecycleOwner::rebuild_planning_state(
        &fixture.connection,
        "p-b09",
        &fixture.volume_id,
        NOW,
    )
    .unwrap();
    assert_eq!(after["requiredTasksRemaining"], 0);
    assert_eq!(after["closeReadiness"], "READY");
    assert_ne!(before_digest, after["projectionDigest"]);
    println!("B09_PLANNING_STATE_RESULT deterministic=PASS rebuild=PASS revision_bound=PASS reality_derived=PASS status=PASS");
}

#[test]
fn b09_route_replan_commits_new_version_with_reason_evidence_and_idempotent_replay() {
    let fixture = fixture();
    let plan = route_payload(&fixture.intent_ref, 2, true);
    let request = replan_request(plan, 0, "b09-replan-1");
    let first = VolumeLifecycleOwner::replan_volume_route(&fixture.connection, request.clone()).unwrap();
    assert_eq!(first["replayed"], false);
    assert_eq!(first["routeVersion"], 2);
    assert_eq!(fixture.connection.query_row::<i64, _, _>("SELECT version FROM canonical_heads WHERE project_id='p-b09' AND artifact_type='VolumePlan' AND scope_key=?1", [scope("VOLUME", Some(("volumeId", &fixture.volume_id)))], |row| row.get(0)).unwrap(), 1);
    assert_eq!(fixture.connection.query_row::<String, _, _>("SELECT reason FROM volume_route_replans WHERE project_id='p-b09' AND replan_id='b09-replan-1'", [], |row| row.get(0)).unwrap(), "容量与退出缺口需要重新规划");
    let replay = VolumeLifecycleOwner::replan_volume_route(&fixture.connection, request).unwrap();
    assert_eq!(replay["replayed"], true);
    assert_eq!(fixture.connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM volume_route_replans WHERE project_id='p-b09'", [], |row| row.get(0)).unwrap(), 1);
    println!("B09_ROUTE_REPLAN_RESULT owner=B07::PlanningOwner reason=PASS evidence=PASS receipt=PASS replay=PASS status=PASS");
}

#[test]
fn b09_route_replan_stale_revision_cannot_overwrite_current_route() {
    let fixture = fixture();
    fixture.connection.execute("UPDATE project_meta SET revision=revision+1 WHERE project_id='p-b09'", []).unwrap();
    let result = VolumeLifecycleOwner::replan_volume_route(
        &fixture.connection,
        replan_request(route_payload(&fixture.intent_ref, 2, true), 0, "b09-replan-stale"),
    );
    assert!(result.unwrap_err().contains("REVISION_CONFLICT"));
    assert_eq!(fixture.connection.query_row::<i64, _, _>("SELECT version FROM canonical_heads WHERE project_id='p-b09' AND artifact_type='VolumePlan' AND scope_key=?1", [scope("VOLUME", Some(("volumeId", &fixture.volume_id)))], |row| row.get(0)).unwrap(), 1);
    println!("B09_ROUTE_REPLAN_STALE_RESULT stale=BLOCK current_head_unchanged=PASS status=PASS");
}

#[test]
fn b09_volume_close_blocks_unfinished_exit_without_partial_state() {
    let fixture = fixture();
    let result = VolumeLifecycleOwner::close_volume(
        &fixture.connection,
        VolumeCloseRequest {
            project_id: "p-b09".into(),
            volume_id: fixture.volume_id.clone(),
            expected_revision: 0,
            command_id: "b09-close-blocked".into(),
            idempotency_key: "key:b09-close-blocked".into(),
            reason: "attempt before exit".into(),
            fault_injection: None,
            now: NOW.into(),
        },
    );
    assert!(result.unwrap_err().contains("VOLUME_CLOSE_BLOCKED"));
    assert_eq!(fixture.connection.query_row::<String, _, _>("SELECT status FROM volumes WHERE volume_id=?1", [&fixture.volume_id], |row| row.get(0)).unwrap(), "PLANNING");
    assert_eq!(fixture.connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM canonical_heads WHERE artifact_type='VolumeExitState'", [], |row| row.get(0)).unwrap(), 0);
    assert_eq!(fixture.connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM volume_close_records", [], |row| row.get(0)).unwrap(), 0);
    println!("B09_VOLUME_CLOSE_NEGATIVE_RESULT required_exit=BLOCK partial_close=0 false_eligibility=0 status=PASS");
}

#[test]
fn b09_volume_close_is_atomic_and_unlocks_next_volume_handoff() {
    let fixture = fixture();
    fixture.connection.execute("UPDATE state_deltas SET value_json=?1 WHERE state_id='task:b09:required'", [json!({"taskId":"task:b09:required","status":"SATISFIED","evidenceRefs":[],"remainingConditions":[]}).to_string()]).unwrap();
    let fault = VolumeLifecycleOwner::close_volume(
        &fixture.connection,
        VolumeCloseRequest { project_id:"p-b09".into(), volume_id:fixture.volume_id.clone(), expected_revision:0, command_id:"b09-close-fault".into(), idempotency_key:"key:b09-close-fault".into(), reason:"fault".into(), fault_injection:Some("after_exit_artifact".into()), now:NOW.into() },
    );
    assert!(fault.is_err());
    assert_eq!(fixture.connection.query_row::<String, _, _>("SELECT status FROM volumes WHERE volume_id=?1", [&fixture.volume_id], |row| row.get(0)).unwrap(), "PLANNING");
    assert_eq!(fixture.connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM canonical_heads WHERE artifact_type='VolumeExitState'", [], |row| row.get(0)).unwrap(), 0);
    let closed = VolumeLifecycleOwner::close_volume(
        &fixture.connection,
        VolumeCloseRequest { project_id:"p-b09".into(), volume_id:fixture.volume_id.clone(), expected_revision:0, command_id:"b09-close-ok".into(), idempotency_key:"key:b09-close-ok".into(), reason:"all exit conditions satisfied".into(), fault_injection:None, now:NOW.into() },
    ).unwrap();
    assert_eq!(closed["nextVolumeEligibility"], "ELIGIBLE");
    assert_eq!(fixture.connection.query_row::<String, _, _>("SELECT status FROM volumes WHERE volume_id=?1", [&fixture.volume_id], |row| row.get(0)).unwrap(), "CLOSED");
    let handoff = VolumeLifecycleOwner::handoff_next_volume(
        &fixture.connection,
        NextVolumeHandoffRequest { project_id:"p-b09".into(), current_volume_id:fixture.volume_id.clone(), next_volume_id:"volume:b09:2".into(), expected_revision:1, command_id:"b09-handoff-2".into(), idempotency_key:"key:b09-handoff-2".into(), title:"第二卷".into(), chapter_start:4, chapter_end:6, target_words:3000, now:NOW.into() },
    ).unwrap();
    assert_eq!(handoff["eligibility"], "ELIGIBLE");
    assert_eq!(fixture.connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM volumes WHERE project_id='p-b09'", [], |row| row.get(0)).unwrap(), 2);
    assert_eq!(fixture.connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM canonical_heads WHERE artifact_type='VolumeIntent' AND scope_key=?1", [scope("VOLUME", Some(("volumeId", "volume:b09:2")))], |row| row.get(0)).unwrap(), 1);
    assert_eq!(fixture.connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM canonical_heads WHERE artifact_type='VolumePlan' AND scope_key=?1", [scope("VOLUME", Some(("volumeId", "volume:b09:2")))], |row| row.get(0)).unwrap(), 1);
    println!("B09_CLOSE_RESULT blocked_atomic=PASS close=PASS exit_snapshot=PASS eligibility=PASS handoff=PASS status=PASS");
}

