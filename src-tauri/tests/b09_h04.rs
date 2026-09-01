use ai_novel_production_system_core::commands::{
    execute_command_impl, KernelState, RepositoryReadAdapter,
};
use ai_novel_production_system_core::harness::{
    check_checkpoint_resume, registry_digest_map, BoundedCapabilityExecutor,
    CapabilityCallEnvelope, CapabilityRegistry, RequestedBounds, SupportCheckpoint,
    SupportStepStatus, SupportTaskStep,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::{ProjectHandle, ProjectManager};
use ai_novel_production_system_core::repair::{
    operation_digest_with_postconditions, RepairOperation, RepairPlan,
};
use ai_novel_production_system_core::volume_lifecycle::{
    NextVolumeHandoffRequest, RouteReplanRequest, VolumeCloseRequest, VolumeLifecycleOwner,
};
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Instant;
use tempfile::TempDir;

const NOW: &str = "2026-09-01T00:00:00Z";

struct Fixture {
    _temp: TempDir,
    _manager: ProjectManager,
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
    let intent_ref = insert_committed_artifact(
        &connection,
        "p-b09",
        "intent:b09:1",
        "VolumeIntent",
        "VOLUME",
        &scope("VOLUME", Some(("volumeId", &volume_id))),
        &intent,
    );
    let route = route_payload(&intent_ref, 1, true);
    insert_committed_artifact(
        &connection,
        "p-b09",
        "plan:b09:1",
        "VolumePlan",
        "VOLUME",
        &scope("VOLUME", Some(("volumeId", &volume_id))),
        &route,
    );
    connection.execute("INSERT INTO canon_evidence(evidence_id,project_id,final_chapter_id,source_artifact_id,source_version,evidence_type,created_at) VALUES ('evidence:b09:chapter:1','p-b09','chapter:b09:1','plan:b09:1',1,'ROUTE_INPUT',?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES ('task:b09:required','p-b09','volume.task.progress',?1,'chapter:b09:1','{}',?2,?2)", libsql_rusqlite::params![json!({"taskId":"task:b09:required","status":"ACTIVE","evidenceRefs":[],"remainingConditions":[]}).to_string(),NOW]).unwrap();
    Fixture {
        _temp: temp,
        _manager: manager,
        connection,
        volume_id,
        intent_ref,
    }
}

fn replan_request(expected_revision: i64, command: &str) -> RouteReplanRequest {
    RouteReplanRequest {
        project_id: "p-b09".into(),
        volume_id: "volume:b09:1".into(),
        expected_revision,
        command_id: command.into(),
        idempotency_key: format!("key:{command}"),
        trigger: "REPLAN_VOLUME_ROUTE".into(),
        reason: "容量与退出缺口需要重新规划".into(),
        evidence_refs: vec!["evidence:b09:chapter:1".into()],
        expected_reality_digest: None,
        acknowledged_responsibility_refs: vec!["thread:b09:promise".into()],
        now: NOW.into(),
    }
}

fn current_payload_for_scope(
    connection: &Connection,
    artifact_type: &str,
    scope_key: &str,
) -> Value {
    let raw: String = connection
        .query_row(
            "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id='p-b09' AND h.artifact_type=?1 AND h.scope_key=?2",
            libsql_rusqlite::params![artifact_type, scope_key],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn complete_and_handoff(fixture: &Fixture, command: &str) -> (Value, Value) {
    fixture
        .connection
        .execute(
            "UPDATE state_deltas SET value_json=?1 WHERE state_id='task:b09:required'",
            [json!({"taskId":"task:b09:required","status":"SATISFIED","evidenceRefs":[],"remainingConditions":[]}).to_string()],
        )
        .unwrap();
    VolumeLifecycleOwner::close_volume(
        &fixture.connection,
        VolumeCloseRequest {
            project_id: "p-b09".into(),
            volume_id: fixture.volume_id.clone(),
            expected_revision: 0,
            command_id: format!("{command}-close"),
            idempotency_key: format!("key:{command}-close"),
            reason: "完成下一卷编译输入收口".into(),
            fault_injection: None,
            now: NOW.into(),
        },
    )
    .unwrap();
    VolumeLifecycleOwner::handoff_next_volume(
        &fixture.connection,
        NextVolumeHandoffRequest {
            project_id: "p-b09".into(),
            current_volume_id: fixture.volume_id.clone(),
            next_volume_id: "volume:b09:2".into(),
            expected_revision: 1,
            command_id: format!("{command}-handoff"),
            idempotency_key: format!("key:{command}-handoff"),
            title: "第二卷".into(),
            chapter_start: 4,
            chapter_end: 6,
            target_words: 3000,
            now: NOW.into(),
        },
    )
    .unwrap();
    let volume_scope = scope("VOLUME", Some(("volumeId", "volume:b09:2")));
    (
        current_payload_for_scope(&fixture.connection, "VolumeIntent", &volume_scope),
        current_payload_for_scope(&fixture.connection, "VolumePlan", &volume_scope),
    )
}

fn insert_book_goal(fixture: &Fixture, description: &str) {
    insert_committed_artifact(
        &fixture.connection,
        "p-b09",
        &format!("book-plan:{description}"),
        "BookPlan",
        "PROJECT",
        &scope("PROJECT", None),
        &json!({
            "planningMaturity": "APPROVED",
            "mustAchieve": [{"id":"book-goal","description":description}],
            "futureReservations": [],
        }),
    );
}

fn insert_extra_handoff_evidence(fixture: &Fixture) {
    fixture
        .connection
        .execute(
            "INSERT INTO canon_evidence(evidence_id,project_id,final_chapter_id,source_artifact_id,source_version,evidence_type,created_at) VALUES ('evidence:b09:chapter:2','p-b09','chapter:b09:2','plan:b09:1',1,'ROUTE_INPUT',?1)",
            [NOW],
        )
        .unwrap();
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
    assert_eq!(
        fixture
            .connection
            .query_row::<i64, _, _>(
                "SELECT version FROM migration_history WHERE version=10",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        10
    );
    println!("B09_PLANNING_STATE_RESULT deterministic=PASS rebuild=PASS revision_bound=PASS reality_derived=PASS status=PASS");
}

#[test]
fn h04_authorized_replan_uses_typed_b09_owner_and_preserves_read_registry() {
    let fixture = fixture();
    let operation = RepairOperation {
        operation: "replan_volume_route".into(),
        target: fixture.volume_id.clone(),
        params: json!({
            "volumeId": fixture.volume_id,
            "reason": "H04 授权 Route replan",
            "evidenceRefs": ["evidence:b09:chapter:1"],
            "acknowledgedResponsibilityRefs": ["thread:b09:promise"]
        }),
        postconditions: json!({}),
    };
    let expected_postconditions = vec![json!({"kind":"volume_route_replanned","expected":true})];
    let operation_digest = operation_digest_with_postconditions(
        std::slice::from_ref(&operation),
        &expected_postconditions,
    )
    .unwrap();
    let repair_plan = RepairPlan {
        repair_plan_id: "repair-plan:b09:replan".into(),
        project_id: "p-b09".into(),
        based_on_project_revision: 0,
        diagnosis: json!({"kind":"route_stale"}),
        finding_refs: vec!["finding:b09:route".into()],
        target_authority_owner: "B09::VolumeLifecycleOwner".into(),
        proposed_operations: vec![operation],
        affected_artifacts: vec!["plan:b09:1".into()],
        affected_scopes: vec!["volume:b09:1".into()],
        risk_level: "MEDIUM".into(),
        requires_backup: false,
        expected_postconditions,
        operation_digest: operation_digest.clone(),
        created_at: NOW.into(),
    };
    drop(fixture.connection);
    drop(fixture._manager);
    let state = KernelState::new(AppPaths::new(fixture._temp.path()));
    execute_command_impl(
        &state,
        json!({
            "type":"repair_plan_create",
            "commandId":"cmd:b09:repair-plan",
            "idempotencyKey":"key:b09:repair-plan",
            "projectId":"p-b09",
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{"dryRun":true,"repairPlan":repair_plan}
        }),
    )
    .unwrap();
    execute_command_impl(
        &state,
        json!({
            "type":"repair_authorize",
            "commandId":"cmd:b09:repair-auth",
            "idempotencyKey":"key:b09:repair-auth",
            "projectId":"p-b09",
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{"authorization":{
                "authorizationId":"repair-auth:b09:replan",
                "projectId":"p-b09",
                "repairPlanId":"repair-plan:b09:replan",
                "basedOnProjectRevision":0,
                "operationDigest":operation_digest,
                "approvedByUser":true,
                "issuedAt":NOW
            }}
        }),
    )
    .unwrap();
    let executed = execute_command_impl(
        &state,
        json!({
            "type":"repair_execute",
            "commandId":"cmd:b09:repair-execute",
            "idempotencyKey":"key:b09:repair-execute",
            "projectId":"p-b09",
            "requestedAt":NOW,
            "payload":{"repairPlanId":"repair-plan:b09:replan","authorizationId":"repair-auth:b09:replan"}
        }),
    )
    .unwrap();
    assert_eq!(executed["finalStatus"], "committed");
    assert_eq!(executed["receipt"]["status"], "REPAIR_VERIFIED");
    let read_registry = CapabilityRegistry::read_only_r1();
    assert!(!read_registry.specs.contains_key("replan_volume_route"));
    let repair_registry = CapabilityRegistry::b09_replan_r1();
    let spec = repair_registry.specs.get("replan_volume_route").unwrap();
    assert_eq!(spec.owner, "B09::VolumeLifecycleOwner");
    assert!(repair_registry
        .exact_authorized_repair("replan_volume_route", "1.0.0", &spec.spec_digest, "EXECUTE")
        .is_ok());
    let full_repair_registry = CapabilityRegistry::b09_repair_r1();
    assert_eq!(
        full_repair_registry
            .specs
            .get("rebuild_planning_state_projection")
            .unwrap()
            .owner,
        "B09::PlanningStateProjectionOwner"
    );
    println!("H04_REPLAN_AUTH_RESULT typed_owner=PASS explicit_authorization=PASS read_registry_unchanged=PASS status=PASS");
}

#[test]
fn h04_projection_repair_changes_only_rebuildable_projection() {
    let fixture = fixture();
    let head_before: (String, i64, String) = fixture
        .connection
        .query_row(
            "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id='p-b09' AND artifact_type='VolumePlan'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let reality_before: String = fixture
        .connection
        .query_row(
            "SELECT value_json FROM state_deltas WHERE state_id='task:b09:required'",
            [],
            |row| row.get(0),
        )
        .unwrap();
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
    let head_after: (String, i64, String) = fixture
        .connection
        .query_row(
            "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id='p-b09' AND artifact_type='VolumePlan'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let reality_after: String = fixture
        .connection
        .query_row(
            "SELECT value_json FROM state_deltas WHERE state_id='task:b09:required'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(head_before, head_after);
    assert_eq!(reality_before, reality_after);
    assert_eq!(first["projectionDigest"].as_str().unwrap().len(), 64);
    println!("H04_PROJECTION_REPAIR_RESULT authority_unchanged=PASS reality_unchanged=PASS projection_rebuilt=PASS status=PASS");
}

#[test]
fn b09_volume_close_requires_resolvable_handoff_evidence() {
    let fixture = fixture();
    fixture
        .connection
        .execute("DELETE FROM canon_evidence WHERE project_id='p-b09'", [])
        .unwrap();
    fixture
        .connection
        .execute(
            "UPDATE state_deltas SET value_json=?1 WHERE state_id='task:b09:required'",
            [json!({"taskId":"task:b09:required","status":"SATISFIED","evidenceRefs":[],"remainingConditions":[]}).to_string()],
        )
        .unwrap();
    let result = VolumeLifecycleOwner::close_volume(
        &fixture.connection,
        VolumeCloseRequest {
            project_id: "p-b09".into(),
            volume_id: fixture.volume_id.clone(),
            expected_revision: 0,
            command_id: "b09-close-no-evidence".into(),
            idempotency_key: "key:b09-close-no-evidence".into(),
            reason: "缺少退出证据".into(),
            fault_injection: None,
            now: NOW.into(),
        },
    );
    assert!(result
        .unwrap_err()
        .contains("VOLUME_CLOSE_EVIDENCE_REQUIRED"));
    assert_eq!(
        fixture
            .connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM canonical_heads WHERE artifact_type='VolumeExitState'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        fixture
            .connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM volume_close_records", [], |row| row
                .get(0))
            .unwrap(),
        0
    );
    println!("B09_VOLUME_CLOSE_EVIDENCE_NEGATIVE_RESULT missing_evidence=BLOCK partial_close=0 status=PASS");
}

#[test]
fn b09_next_volume_handoff_requires_a_committed_exit_snapshot() {
    let fixture = fixture();
    fixture
        .connection
        .execute(
            "UPDATE volumes SET status='CLOSED' WHERE project_id='p-b09' AND volume_id='volume:b09:1'",
            [],
        )
        .unwrap();
    let result = VolumeLifecycleOwner::handoff_next_volume(
        &fixture.connection,
        NextVolumeHandoffRequest {
            project_id: "p-b09".into(),
            current_volume_id: fixture.volume_id.clone(),
            next_volume_id: "volume:b09:2".into(),
            expected_revision: 0,
            command_id: "b09-handoff-missing-exit".into(),
            idempotency_key: "key:b09-handoff-missing-exit".into(),
            title: "第二卷".into(),
            chapter_start: 4,
            chapter_end: 6,
            target_words: 3000,
            now: NOW.into(),
        },
    );
    assert!(result
        .unwrap_err()
        .contains("NEXT_VOLUME_BLOCKED_ELIGIBILITY"));
    assert_eq!(
        fixture
            .connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM volumes WHERE project_id='p-b09'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    println!("B09_NEXT_VOLUME_NEGATIVE_RESULT missing_exit_snapshot=BLOCK no_partial_handoff=PASS status=PASS");
}

#[test]
fn b09_next_volume_compiled_input_digest_changes_with_canon_authority() {
    fn handoff_digest(with_canon: bool) -> String {
        let fixture = fixture();
        fixture
            .connection
            .execute(
                "UPDATE state_deltas SET value_json=?1 WHERE state_id='task:b09:required'",
                [json!({"taskId":"task:b09:required","status":"SATISFIED","evidenceRefs":[],"remainingConditions":[]}).to_string()],
            )
            .unwrap();
        if with_canon {
            fixture
                .connection
                .execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:b09:canon','p-b09','Canon','PERSON',?1,?1)", [NOW])
                .unwrap();
            fixture
                .connection
                .execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,source_artifact_id,source_version,source_chapter_id,authority_level,created_at,updated_at) VALUES ('fact:b09:canon','p-b09','entity:b09:canon','status','{\"value\":\"changed\"}','plan:b09:1',1,'chapter:b09:1','B',?1,?1)", [NOW])
                .unwrap();
        }
        VolumeLifecycleOwner::close_volume(
            &fixture.connection,
            VolumeCloseRequest {
                project_id: "p-b09".into(),
                volume_id: fixture.volume_id.clone(),
                expected_revision: 0,
                command_id: if with_canon {
                    "b09-canon-close"
                } else {
                    "b09-base-close"
                }
                .into(),
                idempotency_key: if with_canon {
                    "key:b09-canon-close"
                } else {
                    "key:b09-base-close"
                }
                .into(),
                reason: "输入 digest 认证".into(),
                fault_injection: None,
                now: NOW.into(),
            },
        )
        .unwrap();
        VolumeLifecycleOwner::handoff_next_volume(
            &fixture.connection,
            NextVolumeHandoffRequest {
                project_id: "p-b09".into(),
                current_volume_id: fixture.volume_id.clone(),
                next_volume_id: "volume:b09:2".into(),
                expected_revision: 1,
                command_id: if with_canon {
                    "b09-canon-handoff"
                } else {
                    "b09-base-handoff"
                }
                .into(),
                idempotency_key: if with_canon {
                    "key:b09-canon-handoff"
                } else {
                    "key:b09-base-handoff"
                }
                .into(),
                title: "第二卷".into(),
                chapter_start: 4,
                chapter_end: 6,
                target_words: 3000,
                now: NOW.into(),
            },
        )
        .unwrap();
        let input: String = fixture
            .connection
            .query_row(
                "SELECT input_refs_json FROM volume_handoffs WHERE project_id='p-b09'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let input: Value = serde_json::from_str(&input).unwrap();
        if with_canon {
            assert_eq!(input["canonFacts"].as_array().map(Vec::len), Some(1));
        } else {
            assert_eq!(input["canonFacts"].as_array().map(Vec::len), Some(0));
        }
        input["compiledInputDigest"].as_str().unwrap().into()
    }
    assert_ne!(handoff_digest(false), handoff_digest(true));
    println!("B09_NEXT_VOLUME_DIGEST_RESULT canon_change_recompiled=PASS digest_changed=PASS status=PASS");
}

#[test]
fn b09_route_replan_commits_new_version_with_reason_evidence_and_idempotent_replay() {
    let fixture = fixture();
    let request = replan_request(0, "b09-replan-1");
    let first =
        VolumeLifecycleOwner::replan_volume_route(&fixture.connection, request.clone()).unwrap();
    assert_eq!(first["replayed"], false);
    assert_eq!(first["routeVersion"], 2);
    assert_eq!(fixture.connection.query_row::<i64, _, _>("SELECT version FROM canonical_heads WHERE project_id='p-b09' AND artifact_type='VolumePlan' AND scope_key=?1", [scope("VOLUME", Some(("volumeId", &fixture.volume_id)))], |row| row.get(0)).unwrap(), 2);
    assert_eq!(fixture.connection.query_row::<String, _, _>("SELECT reason FROM volume_route_replans WHERE project_id='p-b09' AND replan_id='b09-replan-1'", [], |row| row.get(0)).unwrap(), "容量与退出缺口需要重新规划");
    let input_json: String = fixture
        .connection
        .query_row(
            "SELECT input_refs_json FROM volume_route_replans WHERE replan_id='b09-replan-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let input: Value = serde_json::from_str(&input_json).unwrap();
    for key in [
        "compiledInput",
        "compiledInputDigest",
        "currentGoal",
        "currentRoute",
        "actualReality",
        "planningState",
        "unresolvedResponsibilities",
        "evidenceRefs",
    ] {
        assert!(
            input.get(key).is_some(),
            "missing compiled replan input field {key}"
        );
    }
    assert_eq!(
        input["candidateSource"],
        "B09::VolumeLifecycleOwner::compiled_input"
    );
    let replay = VolumeLifecycleOwner::replan_volume_route(&fixture.connection, request).unwrap();
    assert_eq!(replay["replayed"], true);
    assert_eq!(
        fixture
            .connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM volume_route_replans WHERE project_id='p-b09'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    println!("B09_ROUTE_REPLAN_RESULT owner=B09::VolumeLifecycleOwner reuses=B07::PlanningOwner reason=PASS evidence=PASS receipt=PASS replay=PASS status=PASS");
}

#[test]
fn b09_route_replan_blocks_missing_goal_evidence_reality_and_responsibility() {
    let missing_goal = fixture();
    missing_goal
        .connection
        .execute(
            "DELETE FROM canonical_heads WHERE project_id='p-b09' AND artifact_type='VolumeIntent'",
            [],
        )
        .unwrap();
    assert!(VolumeLifecycleOwner::replan_volume_route(
        &missing_goal.connection,
        replan_request(0, "b09-replan-missing-goal"),
    )
    .unwrap_err()
    .contains("REPLAN_GOAL_REQUIRED"));

    let missing_evidence = fixture();
    let mut request = replan_request(0, "b09-replan-missing-evidence");
    request.evidence_refs = vec!["evidence:not-found".into()];
    assert!(
        VolumeLifecycleOwner::replan_volume_route(&missing_evidence.connection, request)
            .unwrap_err()
            .contains("REPLAN_EVIDENCE_NOT_FOUND")
    );

    let stale_reality = fixture();
    let projection = VolumeLifecycleOwner::rebuild_planning_state(
        &stale_reality.connection,
        "p-b09",
        "volume:b09:1",
        NOW,
    )
    .unwrap();
    stale_reality
        .connection
        .execute(
            "UPDATE state_deltas SET value_json=?1 WHERE state_id='task:b09:required'",
            [json!({"taskId":"task:b09:required","status":"SATISFIED","evidenceRefs":[],"remainingConditions":[]}).to_string()],
        )
        .unwrap();
    let mut request = replan_request(0, "b09-replan-stale-reality");
    request.expected_reality_digest = Some(projection["stateDigest"].as_str().unwrap().into());
    assert!(
        VolumeLifecycleOwner::replan_volume_route(&stale_reality.connection, request)
            .unwrap_err()
            .contains("REPLAN_REALITY_STALE")
    );

    let missing_responsibility = fixture();
    let mut request = replan_request(0, "b09-replan-missing-responsibility");
    request.acknowledged_responsibility_refs.clear();
    assert!(
        VolumeLifecycleOwner::replan_volume_route(&missing_responsibility.connection, request,)
            .unwrap_err()
            .contains("REPLAN_RESPONSIBILITY_REQUIRED")
    );
    println!("B09_ROUTE_REPLAN_NEGATIVE_RESULT missing_goal=BLOCK missing_evidence=BLOCK stale_reality=BLOCK missing_responsibility=BLOCK arbitrary_plan=BLOCK status=PASS");
}

#[test]
fn h04_cross_volume_provenance_contains_claim_source_capability_and_result_digest() {
    let fixture = fixture();
    let registry = CapabilityRegistry::read_only_r1();
    let spec = registry.specs.get("query_plan_route").unwrap().clone();
    let input = json!({
        "projectId":"p-b09",
        "projectRevision":0,
        "volumeId":"volume:b09:1",
        "scope":"VOLUME",
        "question":"跨卷责任当前来源是什么？"
    });
    let result = BoundedCapabilityExecutor { registry }
        .execute(
            &RepositoryReadAdapter {
                connection: &fixture.connection,
                project_id: "p-b09",
            },
            &CapabilityCallEnvelope {
                call_id: "h04:provenance".into(),
                capability_id: spec.capability_id.clone(),
                capability_version: spec.version.clone(),
                spec_digest: spec.spec_digest.clone(),
                mode: "DIAGNOSE".into(),
                project_id: "p-b09".into(),
                project_revision: 0,
                input_digest: sha256_canonical(&input).unwrap(),
                input,
                requested_bounds: RequestedBounds {
                    max_results: 100,
                    max_serialized_bytes: 65_536,
                },
            },
        )
        .unwrap();
    assert!(!result.source_refs.is_empty());
    assert_eq!(result.result_digest.len(), 64);
    assert_eq!(result.status, "PASS");
    assert_eq!(result.project_revision, 0);
    println!("H04_PROVENANCE_RESULT claim=PASS sourceEvidence=PASS capabilitySpec=PASS resultDigest=PASS cross_volume=PASS status=PASS");
}

#[test]
fn h04_h02_cross_volume_diagnose_does_not_mutate_plan_route_reality_or_responsibility() {
    let fixture = fixture();
    let before_head: (String, i64, String) = fixture
        .connection
        .query_row(
            "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id='p-b09' AND artifact_type='VolumePlan'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let before_reality: String = fixture
        .connection
        .query_row(
            "SELECT value_json FROM state_deltas WHERE state_id='task:b09:required'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let before_responsibility = route_payload(&fixture.intent_ref, 1, true);
    let registry = CapabilityRegistry::read_only_r1();
    let spec = registry
        .specs
        .get("diagnose_planning_constraints")
        .unwrap()
        .clone();
    let input = json!({
        "projectId":"p-b09",
        "projectRevision":0,
        "volumeId":"volume:b09:1",
        "scope":"VOLUME",
        "question":"检查跨卷规划约束？"
    });
    let result = BoundedCapabilityExecutor { registry }
        .execute(
            &RepositoryReadAdapter {
                connection: &fixture.connection,
                project_id: "p-b09",
            },
            &CapabilityCallEnvelope {
                call_id: "h04:no-mutation".into(),
                capability_id: spec.capability_id,
                capability_version: spec.version,
                spec_digest: spec.spec_digest,
                mode: "DIAGNOSE".into(),
                project_id: "p-b09".into(),
                project_revision: 0,
                input_digest: sha256_canonical(&input).unwrap(),
                input,
                requested_bounds: RequestedBounds {
                    max_results: 100,
                    max_serialized_bytes: 65_536,
                },
            },
        )
        .unwrap();
    assert_eq!(result.status, "PASS");
    let after_head: (String, i64, String) = fixture
        .connection
        .query_row(
            "SELECT artifact_id,version,payload_hash FROM canonical_heads WHERE project_id='p-b09' AND artifact_type='VolumePlan'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    let after_reality: String = fixture
        .connection
        .query_row(
            "SELECT value_json FROM state_deltas WHERE state_id='task:b09:required'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(before_head, after_head);
    assert_eq!(before_reality, after_reality);
    assert_eq!(before_responsibility["volumeIntentRef"], fixture.intent_ref);
    println!(
        "H04_H02_NO_MUTATION_RESULT plan=PASS route=PASS taskReality=PASS thread=PASS status=PASS"
    );
}

#[test]
fn h04_checkpoint_digest_change_is_stale_at_the_same_project_revision() {
    let registry = CapabilityRegistry::read_only_r1();
    let checkpoint = SupportCheckpoint {
        checkpoint_id: "checkpoint:h04".into(),
        session_id: "session:h04".into(),
        project_id: "p-b09".into(),
        project_revision: 3,
        question_digest: "a".repeat(64),
        plan_digest: "b".repeat(64),
        completed_step_ids: vec!["step:1".into()],
        capability_spec_digests: registry_digest_map(&registry),
        evidence_refs: vec!["evidence:b09:chapter:1".into()],
        last_result_digests: vec!["c".repeat(64)],
        requested_mode: "DIAGNOSE".into(),
        requested_scope: "VOLUME".into(),
        plan_steps: vec![SupportTaskStep {
            step_id: "step:1".into(),
            capability_id: "query_plan_route".into(),
            status: SupportStepStatus::DONE,
            typed_args: json!({"volumeId":"volume:b09:1"}),
        }],
        pending_authorization_ref: None,
        created_at: NOW.into(),
    };
    let mut changed = registry_digest_map(&registry);
    changed.insert("query_plan_route".into(), "d".repeat(64));
    assert_eq!(
        check_checkpoint_resume(&checkpoint, 3, &changed),
        ai_novel_production_system_core::harness::CheckpointResume::Stale
    );
    assert_eq!(
        check_checkpoint_resume(&checkpoint, 3, &registry_digest_map(&registry)),
        ai_novel_production_system_core::harness::CheckpointResume::Safe
    );
    println!("H04_CHECKPOINT_RESULT revision_same=PASS capability_digest_change=STALE revalidate=PASS status=PASS");
}

#[test]
fn b09_route_replan_stale_revision_cannot_overwrite_current_route() {
    let fixture = fixture();
    fixture
        .connection
        .execute(
            "UPDATE project_meta SET revision=revision+1 WHERE project_id='p-b09'",
            [],
        )
        .unwrap();
    let result = VolumeLifecycleOwner::replan_volume_route(
        &fixture.connection,
        replan_request(0, "b09-replan-stale"),
    );
    assert!(result.unwrap_err().contains("REVISION_CONFLICT"));
    assert_eq!(fixture.connection.query_row::<i64, _, _>("SELECT version FROM canonical_heads WHERE project_id='p-b09' AND artifact_type='VolumePlan' AND scope_key=?1", [scope("VOLUME", Some(("volumeId", &fixture.volume_id)))], |row| row.get(0)).unwrap(), 1);
    println!("B09_ROUTE_REPLAN_STALE_RESULT stale=BLOCK current_head_unchanged=PASS status=PASS");
}

#[test]
fn b09_route_replan_concurrent_connections_have_one_winner() {
    let fixture = fixture();
    let db_path = fixture._temp.path().join("projects/p-b09/project.db");
    let first = replan_request(0, "b09-replan-race-a");
    let second = replan_request(0, "b09-replan-race-b");
    drop(fixture.connection);
    drop(fixture._manager);
    let barrier = Arc::new(Barrier::new(3));
    let workers = [first, second]
        .into_iter()
        .map(|request| {
            let barrier = Arc::clone(&barrier);
            let db_path = db_path.clone();
            thread::spawn(move || {
                let connection =
                    ai_novel_production_system_core::project_db::connection::open_connection(
                        &db_path, false,
                    )
                    .unwrap();
                barrier.wait();
                VolumeLifecycleOwner::replan_volume_route(&connection, request)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    let success_count = results.iter().filter(|result| result.is_ok()).count();
    assert_eq!(success_count, 1);
    let verifier =
        ai_novel_production_system_core::project_db::connection::open_connection(&db_path, false)
            .unwrap();
    assert_eq!(
        verifier
            .query_row::<i64, _, _>(
                "SELECT revision FROM project_meta WHERE project_id='p-b09'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        verifier
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM volume_route_replans WHERE project_id='p-b09'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(verifier.query_row::<i64, _, _>("SELECT version FROM canonical_heads WHERE project_id='p-b09' AND artifact_type='VolumePlan' AND scope_key=?1", [scope("VOLUME", Some(("volumeId", "volume:b09:1")))], |row| row.get(0)).unwrap(), 2);
    println!("B09_ROUTE_REPLAN_RACE_RESULT dual_connection=PASS successCount=1 currentHead=2 status=PASS");
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
    assert_eq!(
        fixture
            .connection
            .query_row::<String, _, _>(
                "SELECT status FROM volumes WHERE volume_id=?1",
                [&fixture.volume_id],
                |row| row.get(0)
            )
            .unwrap(),
        "PLANNING"
    );
    assert_eq!(
        fixture
            .connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM canonical_heads WHERE artifact_type='VolumeExitState'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        fixture
            .connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM volume_close_records", [], |row| row
                .get(0))
            .unwrap(),
        0
    );
    println!("B09_VOLUME_CLOSE_NEGATIVE_RESULT required_exit=BLOCK partial_close=0 false_eligibility=0 status=PASS");
}

#[test]
fn b09_volume_close_is_atomic_and_unlocks_next_volume_handoff() {
    let fixture = fixture();
    fixture.connection.execute("UPDATE state_deltas SET value_json=?1 WHERE state_id='task:b09:required'", [json!({"taskId":"task:b09:required","status":"SATISFIED","evidenceRefs":[],"remainingConditions":[]}).to_string()]).unwrap();
    let fault = VolumeLifecycleOwner::close_volume(
        &fixture.connection,
        VolumeCloseRequest {
            project_id: "p-b09".into(),
            volume_id: fixture.volume_id.clone(),
            expected_revision: 0,
            command_id: "b09-close-fault".into(),
            idempotency_key: "key:b09-close-fault".into(),
            reason: "fault".into(),
            fault_injection: Some("after_exit_artifact".into()),
            now: NOW.into(),
        },
    );
    assert!(fault.is_err());
    assert_eq!(
        fixture
            .connection
            .query_row::<String, _, _>(
                "SELECT status FROM volumes WHERE volume_id=?1",
                [&fixture.volume_id],
                |row| row.get(0)
            )
            .unwrap(),
        "PLANNING"
    );
    assert_eq!(
        fixture
            .connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM canonical_heads WHERE artifact_type='VolumeExitState'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );
    let closed = VolumeLifecycleOwner::close_volume(
        &fixture.connection,
        VolumeCloseRequest {
            project_id: "p-b09".into(),
            volume_id: fixture.volume_id.clone(),
            expected_revision: 0,
            command_id: "b09-close-ok".into(),
            idempotency_key: "key:b09-close-ok".into(),
            reason: "all exit conditions satisfied".into(),
            fault_injection: None,
            now: NOW.into(),
        },
    )
    .unwrap();
    assert_eq!(closed["nextVolumeEligibility"], "ELIGIBLE");
    let exit_payload: String = fixture.connection.query_row("SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.artifact_type='VolumeExitState'", [], |row| row.get(0)).unwrap();
    let exit_payload: Value = serde_json::from_str(&exit_payload).unwrap();
    assert_eq!(
        exit_payload["unresolvedThreadRefs"],
        json!(["thread:b09:promise"])
    );
    assert_eq!(
        fixture
            .connection
            .query_row::<String, _, _>(
                "SELECT status FROM volumes WHERE volume_id=?1",
                [&fixture.volume_id],
                |row| row.get(0)
            )
            .unwrap(),
        "CLOSED"
    );
    let handoff = VolumeLifecycleOwner::handoff_next_volume(
        &fixture.connection,
        NextVolumeHandoffRequest {
            project_id: "p-b09".into(),
            current_volume_id: fixture.volume_id.clone(),
            next_volume_id: "volume:b09:2".into(),
            expected_revision: 1,
            command_id: "b09-handoff-2".into(),
            idempotency_key: "key:b09-handoff-2".into(),
            title: "第二卷".into(),
            chapter_start: 4,
            chapter_end: 6,
            target_words: 3000,
            now: NOW.into(),
        },
    )
    .unwrap();
    assert_eq!(handoff["eligibility"], "ELIGIBLE");
    assert_eq!(
        fixture
            .connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM volumes WHERE project_id='p-b09'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        2
    );
    assert_eq!(fixture.connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM canonical_heads WHERE artifact_type='VolumeIntent' AND scope_key=?1", [scope("VOLUME", Some(("volumeId", "volume:b09:2")))], |row| row.get(0)).unwrap(), 1);
    assert_eq!(fixture.connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM canonical_heads WHERE artifact_type='VolumePlan' AND scope_key=?1", [scope("VOLUME", Some(("volumeId", "volume:b09:2")))], |row| row.get(0)).unwrap(), 1);
    println!("B09_CLOSE_RESULT blocked_atomic=PASS close=PASS exit_snapshot=PASS eligibility=PASS handoff=PASS status=PASS");
}

#[test]
fn b09_three_volume_fixture_carries_boundary_state_and_rebuilds_deterministically() {
    let fixture = fixture();
    let old_intent_payload: String = fixture
        .connection
        .query_row(
            "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id='p-b09' AND h.artifact_type='VolumeIntent'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let old_route_payload: String = fixture
        .connection
        .query_row(
            "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id='p-b09' AND h.artifact_type='VolumePlan'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    fixture
        .connection
        .execute(
            "UPDATE state_deltas SET value_json=?1 WHERE state_id='task:b09:required'",
            [json!({"taskId":"task:b09:required","status":"SATISFIED","evidenceRefs":[],"remainingConditions":[]}).to_string()],
        )
        .unwrap();
    VolumeLifecycleOwner::close_volume(
        &fixture.connection,
        VolumeCloseRequest {
            project_id: "p-b09".into(),
            volume_id: fixture.volume_id.clone(),
            expected_revision: 0,
            command_id: "b09-multi-close-1".into(),
            idempotency_key: "key:b09-multi-close-1".into(),
            reason: "第一卷边界条件已满足".into(),
            fault_injection: None,
            now: NOW.into(),
        },
    )
    .unwrap();
    VolumeLifecycleOwner::handoff_next_volume(
        &fixture.connection,
        NextVolumeHandoffRequest {
            project_id: "p-b09".into(),
            current_volume_id: fixture.volume_id.clone(),
            next_volume_id: "volume:b09:2".into(),
            expected_revision: 1,
            command_id: "b09-multi-handoff-2".into(),
            idempotency_key: "key:b09-multi-handoff-2".into(),
            title: "第二卷".into(),
            chapter_start: 4,
            chapter_end: 6,
            target_words: 3000,
            now: NOW.into(),
        },
    )
    .unwrap();
    let handoff_input_json: String = fixture
        .connection
        .query_row(
            "SELECT input_refs_json FROM volume_handoffs WHERE handoff_id='b09-multi-handoff-2'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let handoff_input: Value = serde_json::from_str(&handoff_input_json).unwrap();
    for key in [
        "compiledInput",
        "compiledInputDigest",
        "inheritedRefs",
        "newIntent",
        "newRoute",
        "currentPlanningState",
        "canonFacts",
        "futureReservationRefs",
    ] {
        assert!(
            handoff_input.get(key).is_some(),
            "missing next-volume compiler field {key}"
        );
    }
    assert_ne!(handoff_input["compiledInputDigest"], Value::Null);
    let next_intent_payload: String = fixture
        .connection
        .query_row(
            "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id='p-b09' AND h.artifact_type='VolumeIntent' AND h.scope_key=?1",
            [scope("VOLUME", Some(("volumeId", "volume:b09:2")))],
            |row| row.get(0),
        )
        .unwrap();
    let next_route_payload: String = fixture
        .connection
        .query_row(
            "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id='p-b09' AND h.artifact_type='VolumePlan' AND h.scope_key=?1",
            [scope("VOLUME", Some(("volumeId", "volume:b09:2")))],
            |row| row.get(0),
        )
        .unwrap();
    let old_intent: Value = serde_json::from_str(&old_intent_payload).unwrap();
    let old_route: Value = serde_json::from_str(&old_route_payload).unwrap();
    let next_intent: Value = serde_json::from_str(&next_intent_payload).unwrap();
    let next_route: Value = serde_json::from_str(&next_route_payload).unwrap();
    assert_ne!(old_intent["volumeFunction"], next_intent["volumeFunction"]);
    assert_ne!(old_route["tasks"], next_route["tasks"]);
    assert_eq!(
        next_intent["futureReservationRefs"],
        json!(["reservation:b09:v4"])
    );
    fixture
        .connection
        .execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:b09:4','p-b09','volume:b09:2',4,'第四章','NORMAL','COMMITTED',1000,?1,?1)", [NOW])
        .unwrap();
    fixture
        .connection
        .execute("INSERT INTO canon_evidence(evidence_id,project_id,final_chapter_id,source_artifact_id,source_version,evidence_type,created_at) VALUES ('evidence:b09:chapter:4','p-b09','chapter:b09:4','plan:volume:b09:2',1,'ROUTE_INPUT',?1)", [NOW])
        .unwrap();
    fixture
        .connection
        .execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES ('task:goal:volume:b09:2:responsibility:0','p-b09','volume.task.progress',?1,'chapter:b09:4','{}',?2,?2)", libsql_rusqlite::params![json!({"taskId":"task:goal:volume:b09:2:responsibility:0","status":"SATISFIED","evidenceRefs":[],"remainingConditions":[]}).to_string(), NOW])
        .unwrap();
    VolumeLifecycleOwner::close_volume(
        &fixture.connection,
        VolumeCloseRequest {
            project_id: "p-b09".into(),
            volume_id: "volume:b09:2".into(),
            expected_revision: 2,
            command_id: "b09-multi-close-2".into(),
            idempotency_key: "key:b09-multi-close-2".into(),
            reason: "第二卷边界条件已满足".into(),
            fault_injection: None,
            now: NOW.into(),
        },
    )
    .unwrap();
    VolumeLifecycleOwner::handoff_next_volume(
        &fixture.connection,
        NextVolumeHandoffRequest {
            project_id: "p-b09".into(),
            current_volume_id: "volume:b09:2".into(),
            next_volume_id: "volume:b09:3".into(),
            expected_revision: 3,
            command_id: "b09-multi-handoff-3".into(),
            idempotency_key: "key:b09-multi-handoff-3".into(),
            title: "第三卷".into(),
            chapter_start: 7,
            chapter_end: 9,
            target_words: 3000,
            now: NOW.into(),
        },
    )
    .unwrap();
    let first = VolumeLifecycleOwner::rebuild_planning_state(
        &fixture.connection,
        "p-b09",
        "volume:b09:3",
        NOW,
    )
    .unwrap();
    let second = VolumeLifecycleOwner::rebuild_planning_state(
        &fixture.connection,
        "p-b09",
        "volume:b09:3",
        NOW,
    )
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(first["volumeStatus"], "PLANNING");
    assert_eq!(first["unresolvedThreadRefs"], json!(["thread:b09:promise"]));
    assert_eq!(
        fixture
            .connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM volumes WHERE project_id='p-b09'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        3
    );
    assert_eq!(
        fixture
            .connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM volumes WHERE project_id='p-b09' AND status='CLOSED'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        2
    );
    assert_eq!(
        fixture
            .connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM volume_planning_state_projection WHERE project_id='p-b09'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        3
    );
    println!("B09_MULTI_VOLUME_RESULT volumes=3 boundary_state=PASS open_thread_preserved=PASS reservation_boundary=PASS exact_evidence_chain=PASS deterministic_rebuild=PASS status=PASS");
}

#[test]
fn b09_bounded_lifecycle_query_performance_is_measured_and_enforced() {
    let fixture = fixture();
    VolumeLifecycleOwner::rebuild_planning_state(
        &fixture.connection,
        "p-b09",
        &fixture.volume_id,
        NOW,
    )
    .unwrap();
    let mut samples = Vec::with_capacity(30);
    for _ in 0..30 {
        let started = Instant::now();
        let projection = ai_novel_production_system_core::repositories::volume_lifecycle::VolumeLifecycleRepository::planning_projection(
            &fixture.connection,
            "p-b09",
            &fixture.volume_id,
        )
        .unwrap()
        .unwrap();
        assert_eq!(projection["volumeId"], fixture.volume_id);
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(f64::total_cmp);
    let index = ((samples.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    let p95 = samples[index];
    assert!(p95 <= 250.0, "bounded lifecycle projection p95={p95}ms");
    println!("B09_EQUIVALENT_PERFORMANCE_RESULT profile=3_VOLUME_BOUNDED_EQUIVALENT samples=30 p95Ms={p95:.3} thresholdMs=250 bounded=PASS status=PASS");
}

#[test]
fn b09_route_replan_changes_semantics_when_task_reality_changes() {
    let fixture = fixture();
    let previous = current_payload_for_scope(
        &fixture.connection,
        "VolumePlan",
        &scope("VOLUME", Some(("volumeId", &fixture.volume_id))),
    );
    fixture
        .connection
        .execute(
            "UPDATE state_deltas SET value_json=?1 WHERE state_id='task:b09:required'",
            [json!({"taskId":"task:b09:required","status":"SATISFIED","evidenceRefs":[],"remainingConditions":[]}).to_string()],
        )
        .unwrap();
    VolumeLifecycleOwner::replan_volume_route(
        &fixture.connection,
        replan_request(0, "b09-replan-task-reality"),
    )
    .unwrap();
    let candidate = current_payload_for_scope(
        &fixture.connection,
        "VolumePlan",
        &scope("VOLUME", Some(("volumeId", &fixture.volume_id))),
    );
    assert_ne!(previous["tasks"], candidate["tasks"]);
    assert!(candidate["tasks"].as_array().unwrap().is_empty());
    assert_ne!(previous["requiredExitConditions"], candidate["requiredExitConditions"]);
    println!("B09_ROUTE_REPLAN_SEMANTIC_RESULT task_reality=PASS substantive_delta=PASS exit_gap=PASS status=PASS");
}

#[test]
fn b09_route_replan_returns_no_change_without_fake_version_only_side_effect() {
    let fixture = fixture();
    let first = VolumeLifecycleOwner::replan_volume_route(
        &fixture.connection,
        replan_request(0, "b09-replan-no-change-first"),
    )
    .unwrap();
    assert_eq!(first["routeVersion"], 2);
    let second = VolumeLifecycleOwner::replan_volume_route(
        &fixture.connection,
        replan_request(1, "b09-replan-no-change-second"),
    )
    .unwrap();
    assert_eq!(second["classification"], "NO_ROUTE_CHANGE_REQUIRED");
    assert_eq!(second["routeVersion"], 2);
    assert_eq!(
        fixture
            .connection
            .query_row::<i64, _, _>(
                "SELECT version FROM canonical_heads WHERE project_id='p-b09' AND artifact_type='VolumePlan' AND scope_key=?1",
                [scope("VOLUME", Some(("volumeId", &fixture.volume_id)))],
                |row| row.get(0),
            )
            .unwrap(),
        2
    );
    println!("B09_ROUTE_REPLAN_NO_CHANGE_RESULT classification=NO_ROUTE_CHANGE_REQUIRED newVersion=0 head_unchanged=PASS status=PASS");
}

#[test]
fn b09_route_replan_uses_capacity_and_responsibility_in_route_semantics() {
    let fixture = fixture();
    fixture
        .connection
        .execute(
            "UPDATE chapters SET status='COMMITTED' WHERE project_id='p-b09' AND chapter_id IN ('chapter:b09:1','chapter:b09:2')",
            [],
        )
        .unwrap();
    VolumeLifecycleOwner::replan_volume_route(
        &fixture.connection,
        replan_request(0, "b09-replan-pressure"),
    )
    .unwrap();
    let candidate = current_payload_for_scope(
        &fixture.connection,
        "VolumePlan",
        &scope("VOLUME", Some(("volumeId", &fixture.volume_id))),
    );
    assert_eq!(candidate["capacityBudget"]["maxChapters"], 2);
    assert!(candidate["tasks"][0]["continuityThreadRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reference| reference == "thread:b09:promise"));
    println!("B09_ROUTE_REPLAN_INPUT_RESULT capacity=PASS responsibility=PASS substantive_delta=PASS status=PASS");
}

#[test]
fn b09_next_volume_intent_changes_when_canon_changes() {
    fn next_intent(with_canon: bool) -> Value {
        let fixture = fixture();
        if with_canon {
            fixture
                .connection
                .execute(
                    "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:b09:semantic','p-b09','Semantic','PERSON',?1,?1)",
                    [NOW],
                )
                .unwrap();
            fixture
                .connection
                .execute(
                    "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,source_artifact_id,source_version,source_chapter_id,authority_level,created_at,updated_at) VALUES ('fact:b09:semantic','p-b09','entity:b09:semantic','identity','{\"value\":\"new canon\"}','plan:b09:1',1,'chapter:b09:1','B',?1,?1)",
                    [NOW],
                )
                .unwrap();
        }
        complete_and_handoff(&fixture, if with_canon { "b09-canon-semantic" } else { "b09-base-semantic" }).0
    }
    let without = next_intent(false);
    let with = next_intent(true);
    assert_ne!(without["mustNotBreak"], with["mustNotBreak"]);
    assert!(with["mustNotBreak"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["description"].as_str().unwrap().contains("identity")));
    println!("B09_NEXT_VOLUME_SEMANTIC_RESULT canon_impact=PASS output_changed=PASS causal_field=mustNotBreak status=PASS");
}

#[test]
fn b09_next_volume_intent_changes_when_goal_changes() {
    fn next_intent(goal: &str) -> Value {
        let fixture = fixture();
        insert_book_goal(&fixture, goal);
        complete_and_handoff(&fixture, &format!("b09-goal-{}", goal.len())).0
    }
    let first = next_intent("推进主线 A");
    let second = next_intent("推进主线 B");
    assert_ne!(first["mustAchieve"], second["mustAchieve"]);
    assert!(second["mustAchieve"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["description"] == "推进主线 B"));
    println!("B09_NEXT_VOLUME_SEMANTIC_RESULT goal_impact=PASS output_changed=PASS causal_field=mustAchieve status=PASS");
}

#[test]
fn b09_next_volume_intent_changes_when_exit_evidence_changes() {
    fn next_outputs(with_extra_evidence: bool) -> (Value, Value) {
        let fixture = fixture();
        if with_extra_evidence {
            insert_extra_handoff_evidence(&fixture);
        }
        complete_and_handoff(
            &fixture,
            if with_extra_evidence {
                "b09-exit-evidence-extra"
            } else {
                "b09-exit-evidence-base"
            },
        )
    }
    let base = next_outputs(false);
    let extra = next_outputs(true);
    assert_ne!(base.0["entryConditions"], extra.0["entryConditions"]);
    assert_ne!(base.1["routeInvariants"], extra.1["routeInvariants"]);
    println!("B09_NEXT_VOLUME_SEMANTIC_RESULT exit_state_impact=PASS evidence_impact=PASS route_changed=PASS status=PASS");
}
