use super::*;

fn seed_formal_contract_responsibility(
    state: &KernelState,
    project_id: &str,
    obligation_id: &str,
    authority_level: &str,
) -> i64 {
    let _ = seed_b08_long_form_project(state, project_id);
    let connection = state
        .manager
        .open(project_id, "RV33 formal source", NOW)
        .unwrap();
    let source_artifact_id = format!("main-contract:{project_id}");
    connection.execute(
        "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,prerequisites_json,lifecycle,resume_condition_json,created_at,updated_at) VALUES (?1,?2,'BOOK','CONTRACT_PROMISE',?3,?4,1,'作者合同中的正式责任','[]','PROGRESSING','{\"kind\":\"AFTER_CHAPTER\",\"value\":2}',?5,?5)",
        libsql_rusqlite::params![obligation_id, project_id, authority_level, source_artifact_id, NOW],
    ).unwrap();
    ai_novel_production_system_core::repositories::narrative::NarrativeRepository::project_revision(
        &connection,
        project_id,
    )
    .unwrap()
}

fn formal_source_snapshot(state: &KernelState, project_id: &str, obligation_id: &str) -> Value {
    let connection = state
        .manager
        .open(project_id, "RV33 formal source", NOW)
        .unwrap();
    let row = connection.query_row(
        "SELECT authority_level,lifecycle,source_artifact_id,source_version,decision_ref FROM narrative_obligations WHERE project_id=?1 AND obligation_id=?2",
        libsql_rusqlite::params![project_id, obligation_id],
        |row| Ok(json!({
            "authorityLevel":row.get::<_,String>(0)?,
            "lifecycle":row.get::<_,String>(1)?,
            "sourceArtifactId":row.get::<_,String>(2)?,
            "sourceVersion":row.get::<_,i64>(3)?,
            "decisionRef":row.get::<_,Option<String>>(4)?,
        })),
    ).unwrap();
    json!({
        "revision":ai_novel_production_system_core::repositories::narrative::NarrativeRepository::project_revision(&connection, project_id).unwrap(),
        "artifactCount":connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM artifacts WHERE project_id=?1", [project_id], |row| row.get(0)).unwrap(),
        "headCount":connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM canonical_heads WHERE project_id=?1", [project_id], |row| row.get(0)).unwrap(),
        "decisionCount":connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM decisions WHERE project_id=?1", [project_id], |row| row.get(0)).unwrap(),
        "receiptCount":connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM commit_receipts WHERE project_id=?1", [project_id], |row| row.get(0)).unwrap(),
        "obligation":row,
    })
}

fn responsibility_adjustment_request(
    project_id: &str,
    obligation_id: &str,
    action: &str,
    expected_revision: i64,
) -> Value {
    json!({
        "type":"thread_responsibility_adjust",
        "commandId":format!("cmd:rv33:{project_id}:{action}"),
        "idempotencyKey":format!("key:rv33:{project_id}:{action}"),
        "projectId":project_id,
        "expectedRevision":expected_revision,
        "requestedAt":NOW,
        "payload":{"threadRef":format!("thread:{obligation_id}"),"action":action}
    })
}

#[test]
fn fx8_ab_delete_creates_change_request() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-fx8-ab-delete-change-request";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(AuditResponsibilitiesGateway),
    );
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state
        .manager
        .open(project_id, "FX-8 responsibility", NOW)
        .unwrap();
    connection
        .execute(
            "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:actual:gugu',?1,'顾清','CHARACTER',?2,?2)",
            libsql_rusqlite::params![project_id, NOW],
        )
        .unwrap();
    let revision = ai_novel_production_system_core::repositories::narrative::NarrativeRepository::project_revision(&connection, project_id).unwrap();
    drop(connection);
    let production = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":"cmd:fx8:responsibility-production",
            "idempotencyKey":"key:fx8:responsibility-production","projectId":project_id,
            "expectedRevision":revision,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou","entity:actual:gugu"],"references":[]}
        }),
    )
    .unwrap();
    assert_eq!(production["finalStatus"], "committed");

    let connection = state
        .manager
        .open(project_id, "FX-8 responsibility", NOW)
        .unwrap();
    let (source_artifact_id, source_version): (String, i64) = connection
        .query_row(
            "SELECT artifact_id,version FROM artifacts WHERE project_id=?1 AND artifact_type='GatePolicy' ORDER BY version DESC LIMIT 1",
            [project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let manual_a_id = "obligation:fx8:manual-a";
    let c_id = "obligation:fx8:c-delete";
    for (obligation_id, level) in [(manual_a_id, "A"), (c_id, "C")] {
        connection
            .execute(
                "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,prerequisites_json,lifecycle,resume_condition_json,created_at,updated_at) VALUES (?1,?2,'BOOK','PROMISE',?3,?4,?5,'FX-8 responsibility fixture','[]','PROGRESSING','{\"kind\":\"AFTER_CHAPTER\",\"value\":2}',?6,?6)",
                libsql_rusqlite::params![obligation_id, project_id, level, source_artifact_id, source_version, NOW],
            )
            .unwrap();
        let decision_type = if level == "A" {
            "USER_THREAD_RESPONSIBILITY_ADJUSTMENT"
        } else {
            "THREAD_PROMOTION_RULE"
        };
        let outcome = if level == "A" { "UPGRADE" } else { "C" };
        let payload = if level == "A" {
            json!({"authorityLevel":"A","actor":"USER"})
        } else {
            json!({"ruleVersion":"thread-promotion.r1"})
        };
        connection
            .execute(
                "INSERT INTO decisions(decision_id,project_id,decision_type,outcome,reason,source_ref,payload_json,created_at,updated_at) VALUES (?1,?2,?3,?4,'FX-8 permission fixture',?5,?6,?7,?7)",
                libsql_rusqlite::params![
                    format!("decision:fx8:{level}"),
                    project_id,
                    decision_type,
                    outcome,
                    format!("thread:{obligation_id}"),
                    payload.to_string(),
                    NOW
                ],
            )
            .unwrap();
    }
    let delete_b_revision = ai_novel_production_system_core::repositories::narrative::NarrativeRepository::project_revision(&connection, project_id).unwrap();
    drop(connection);

    let mut delete_b = responsibility_adjustment_request(
        project_id,
        "promise:rv33-delete",
        "DELETE",
        delete_b_revision,
    );
    delete_b["commandId"] = json!("cmd:fx8:delete-b");
    delete_b["idempotencyKey"] = json!("key:fx8:delete-b");
    let delete_b_replay_request = delete_b.clone();
    let deleted_b = execute_command_impl(&state, delete_b).unwrap();
    assert_eq!(deleted_b["actionStatus"], "CHANGE_REQUEST_SUBMITTED");
    assert_eq!(
        deleted_b["message"],
        "B08_RESPONSIBILITY_CHANGE_REQUEST_CREATED"
    );
    let b_request_id = deleted_b["changeRequestId"].as_str().unwrap().to_owned();
    assert!(!b_request_id.is_empty());

    let connection = state
        .manager
        .open(project_id, "FX-8 responsibility", NOW)
        .unwrap();
    let b_lifecycle: String = connection
        .query_row(
            "SELECT lifecycle FROM narrative_obligations WHERE project_id=?1 AND obligation_id='promise:rv33-delete'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    let b_proposal: String = connection
        .query_row(
            "SELECT p.payload_json FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ChangeRequest' AND a.artifact_id=?2",
            libsql_rusqlite::params![project_id, b_request_id],
            |row| row.get(0),
        )
        .unwrap();
    let b_proposal: Value = serde_json::from_str(&b_proposal).unwrap();
    assert_eq!(b_lifecycle, "PROGRESSING");
    assert_eq!(b_proposal["status"], "PROPOSED");
    assert_eq!(
        b_proposal["requestedChange"]["kind"],
        "WITHDRAW_RESPONSIBILITY"
    );
    drop(connection);

    let replayed_b = execute_command_impl(&state, delete_b_replay_request).unwrap();
    assert_eq!(replayed_b["changeRequestId"], b_request_id);
    assert_eq!(replayed_b["replayed"], true);
    let connection = state
        .manager
        .open(project_id, "FX-8 responsibility", NOW)
        .unwrap();
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='ChangeRequest' AND artifact_id=?2",
                libsql_rusqlite::params![project_id, b_request_id],
                |row| row.get(0),
            )
            .unwrap(),
        1
    );
    let revision = ai_novel_production_system_core::repositories::narrative::NarrativeRepository::project_revision(&connection, project_id).unwrap();
    drop(connection);

    let mut delete_a =
        responsibility_adjustment_request(project_id, manual_a_id, "DELETE", revision);
    delete_a["commandId"] = json!("cmd:fx8:delete-manual-a");
    delete_a["idempotencyKey"] = json!("key:fx8:delete-manual-a");
    let deleted_a = execute_command_impl(&state, delete_a).unwrap();
    assert_eq!(deleted_a["actionStatus"], "CHANGE_REQUEST_SUBMITTED");
    let a_request_id = deleted_a["changeRequestId"].as_str().unwrap();
    let connection = state
        .manager
        .open(project_id, "FX-8 responsibility", NOW)
        .unwrap();
    let a_lifecycle: String = connection
        .query_row(
            "SELECT lifecycle FROM narrative_obligations WHERE project_id=?1 AND obligation_id=?2",
            libsql_rusqlite::params![project_id, manual_a_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(a_lifecycle, "PROGRESSING");
    let a_proposal_status: String = connection
        .query_row(
            "SELECT json_extract(p.payload_json,'$.status') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ChangeRequest' AND a.artifact_id=?2",
            libsql_rusqlite::params![project_id, a_request_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(a_proposal_status, "PROPOSED");
    let revision = ai_novel_production_system_core::repositories::narrative::NarrativeRepository::project_revision(&connection, project_id).unwrap();
    drop(connection);

    let mut delete_c = responsibility_adjustment_request(project_id, c_id, "DELETE", revision);
    delete_c["commandId"] = json!("cmd:fx8:delete-c");
    delete_c["idempotencyKey"] = json!("key:fx8:delete-c");
    let deleted_c = execute_command_impl(&state, delete_c).unwrap();
    assert_eq!(deleted_c["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "FX-8 responsibility", NOW)
        .unwrap();
    let c_lifecycle: String = connection
        .query_row(
            "SELECT lifecycle FROM narrative_obligations WHERE project_id=?1 AND obligation_id=?2",
            libsql_rusqlite::params![project_id, c_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(c_lifecycle, "ABANDONED");
}

#[test]
fn b08_rv33_contract_a_delete_requires_change_request_and_zero_writes() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-rv33-contract-a-delete";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let obligation_id = "obligation:formal:a";
    let revision = seed_formal_contract_responsibility(&state, project_id, obligation_id, "A");
    let before = formal_source_snapshot(&state, project_id, obligation_id);

    let result = execute_command_impl(
        &state,
        responsibility_adjustment_request(project_id, obligation_id, "DELETE", revision),
    );
    let after = formal_source_snapshot(&state, project_id, obligation_id);
    assert!(
        matches!(result.as_ref(), Err(error) if error == "B08_RESPONSIBILITY_CHANGE_REQUEST_REQUIRED"),
        "formal A responsibility must require ChangeRequest; result={result:?}"
    );
    assert_eq!(
        after, before,
        "rejected contract A deletion must perform zero writes"
    );
}

#[test]
fn b08_rv33_contract_b_downgrade_requires_change_request_and_zero_writes() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-rv33-contract-b-downgrade";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let obligation_id = "obligation:formal:b";
    let revision = seed_formal_contract_responsibility(&state, project_id, obligation_id, "B");
    let before = formal_source_snapshot(&state, project_id, obligation_id);

    let result = execute_command_impl(
        &state,
        responsibility_adjustment_request(project_id, obligation_id, "DOWNGRADE", revision),
    );
    let after = formal_source_snapshot(&state, project_id, obligation_id);
    assert!(
        matches!(result.as_ref(), Err(error) if error == "B08_RESPONSIBILITY_CHANGE_REQUEST_REQUIRED"),
        "formal B responsibility must require ChangeRequest; result={result:?}"
    );
    assert_eq!(
        after, before,
        "rejected contract B downgrade must perform zero writes"
    );
}

struct AuditResponsibilitiesGateway;

impl ProductionModelGateway for AuditResponsibilitiesGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        let body = "林舟推开石门。答应顾清归来时把玉符交给顾清。又答应顾清归来时把铜铃交给顾清。";
        if capability == "RENDER_CHAPTER" {
            response.output["body"] = json!(body);
        }
        if capability == "AUDIT_EXTRACT" {
            let candidates = [
                (
                    "thread:promise:rv33-downgrade",
                    "答应顾清归来时把玉符交给顾清",
                    "林舟承诺在顾清归来时交还玉符。",
                    "item:jade-token",
                ),
                (
                    "thread:promise:rv33-delete",
                    "又答应顾清归来时把铜铃交给顾清",
                    "林舟承诺在顾清归来时交还铜铃。",
                    "item:bronze-bell",
                ),
            ];
            response.output["continuityCandidates"] = Value::Array(
                candidates
                    .into_iter()
                    .map(|(thread_ref, quote, description, fulfillment_object)| {
                        let byte_start = body.find(quote).unwrap();
                        let start = body[..byte_start].chars().count();
                        let end = start + quote.chars().count();
                        json!({
                            "threadRef":thread_ref,
                            "disposition":"ADVANCE",
                            "description":description,
                            "fulfillmentObject":fulfillment_object,
                            "resumeCondition":{"kind":"ENTITY_PRESENT","value":"entity:actual:gugu"},
                            "promotionSignals":{"explicitPromise":true},
                            "evidence":{"start":start,"end":end,"quote":quote}
                        })
                    })
                    .collect(),
            );
        }
        Ok(response)
    }
}

#[test]
fn b08_rv33_audit_added_b_downgrade_and_delete_remain_available() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-rv33-audit-adjustments";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(AuditResponsibilitiesGateway),
    );
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state
        .manager
        .open(project_id, "RV33 audit source", NOW)
        .unwrap();
    connection.execute(
        "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:actual:gugu',?1,'顾清','CHARACTER',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    let revision: i64 = ai_novel_production_system_core::repositories::narrative::NarrativeRepository::project_revision(&connection, project_id).unwrap();
    drop(connection);
    let production = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":"cmd:rv33:audit-produce",
            "idempotencyKey":"key:rv33:audit-produce","projectId":project_id,
            "expectedRevision":revision,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM",
                "entityIds":["entity:actual:linzhou","entity:actual:gugu"],"references":[]}
        }),
    )
    .unwrap();
    assert_eq!(production["finalStatus"], "committed");

    let downgraded = execute_command_impl(
        &state,
        responsibility_adjustment_request(
            project_id,
            "promise:rv33-downgrade",
            "DOWNGRADE",
            production["receipt"]["afterRevision"].as_i64().unwrap(),
        ),
    )
    .unwrap();
    assert_eq!(downgraded["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "RV33 audit source", NOW)
        .unwrap();
    let (level_after_downgrade, lifecycle_after_downgrade): (String, String) = connection
        .query_row(
            "SELECT authority_level,lifecycle FROM narrative_obligations WHERE project_id=?1 AND obligation_id='promise:rv33-downgrade'",
            [project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (
            level_after_downgrade.as_str(),
            lifecycle_after_downgrade.as_str()
        ),
        ("C", "PROGRESSING")
    );
    drop(connection);

    let mut upgrade_request = responsibility_adjustment_request(
        project_id,
        "promise:rv33-downgrade",
        "UPGRADE",
        downgraded["receipt"]["afterRevision"].as_i64().unwrap(),
    );
    upgrade_request["payload"]["authorityLevel"] = json!("B");
    let upgraded = execute_command_impl(&state, upgrade_request).unwrap();
    assert_eq!(upgraded["finalStatus"], "committed");

    let deleted = execute_command_impl(
        &state,
        responsibility_adjustment_request(
            project_id,
            "promise:rv33-delete",
            "DELETE",
            upgraded["receipt"]["afterRevision"].as_i64().unwrap(),
        ),
    )
    .unwrap();
    assert_eq!(deleted["finalStatus"], "committed");
    let mut delete_after_upgrade_request = responsibility_adjustment_request(
        project_id,
        "promise:rv33-downgrade",
        "DELETE",
        deleted["receipt"]["afterRevision"].as_i64().unwrap(),
    );
    delete_after_upgrade_request["commandId"] = json!("cmd:rv33:manual-upgrade-delete");
    delete_after_upgrade_request["idempotencyKey"] = json!("key:rv33:manual-upgrade-delete");
    let deleted_after_upgrade = execute_command_impl(&state, delete_after_upgrade_request).unwrap();
    assert_eq!(deleted_after_upgrade["finalStatus"], "committed");

    let connection = state
        .manager
        .open(project_id, "RV33 audit source", NOW)
        .unwrap();
    let (upgraded_then_deleted_level, upgraded_then_deleted_lifecycle): (String, String) = connection.query_row(
        "SELECT authority_level,lifecycle FROM narrative_obligations WHERE project_id=?1 AND obligation_id='promise:rv33-downgrade'",
        [project_id], |row| Ok((row.get(0)?, row.get(1)?))
    ).unwrap();
    let (deleted_level, deleted_lifecycle): (String, String) = connection.query_row(
        "SELECT authority_level,lifecycle FROM narrative_obligations WHERE project_id=?1 AND obligation_id='promise:rv33-delete'",
        [project_id], |row| Ok((row.get(0)?, row.get(1)?))
    ).unwrap();
    assert_eq!(
        (
            upgraded_then_deleted_level.as_str(),
            upgraded_then_deleted_lifecycle.as_str()
        ),
        ("B", "PROGRESSING")
    );
    assert_eq!(
        (deleted_level.as_str(), deleted_lifecycle.as_str()),
        ("B", "PROGRESSING")
    );
    assert_eq!(connection.query_row::<i64,_,_>(
        "SELECT COUNT(*) FROM decisions WHERE project_id=?1 AND decision_type='THREAD_PROMOTION_RULE' AND json_extract(payload_json,'$.ruleVersion')='thread-promotion.r1'",
        [project_id], |row| row.get(0)
    ).unwrap(), 2);
}
