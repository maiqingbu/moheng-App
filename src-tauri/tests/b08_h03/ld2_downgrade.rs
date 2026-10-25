//! LD-1 step 2: an audit item the ledger cannot take is held back and
//! reported; the chapter still commits with everything else.

use super::*;

/// Every audit adds a second fact whose predicate the policy does not
/// register, even after the feedback rerun.
struct UnregisteredPredicateAlwaysGateway;

impl ProductionModelGateway for UnregisteredPredicateAlwaysGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            let mut extra = response.output["canonCandidates"][0].clone();
            extra["predicate"] = json!("lost_memory");
            extra["objectJson"] = json!({"memory":"stone-door"});
            response.output["canonCandidates"]
                .as_array_mut()
                .unwrap()
                .push(extra);
        }
        Ok(response)
    }
}

#[test]
fn ld2_a_fact_the_ledger_cannot_take_is_held_back_and_the_chapter_commits() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(UnregisteredPredicateAlwaysGateway),
    );
    let project_id = "p-ld2-held-back-fact";
    let connection = state.manager.create(project_id, "LD2", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    let result = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:ld2:held-back","idempotencyKey":"key:ld2:held-back",
        "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","skillsEnabled":false,"entityIds":["entity:actual:linzhou"]}
    }))
    .unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let connection = state.manager.open(project_id, "LD2", NOW).unwrap();
    let count = |sql: &str, id: String| -> i64 {
        connection
            .query_row(sql, libsql_rusqlite::params![project_id, id], |row| {
                row.get(0)
            })
            .unwrap()
    };
    let facts = "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1 AND fact_id=?2";
    assert_eq!(count(facts, new_audit_item_id("canon", &chapter_id, 1)), 1);
    assert_eq!(count(facts, new_audit_item_id("canon", &chapter_id, 2)), 0);
    let (outcome, payload): (String, String) = connection
        .query_row(
            "SELECT outcome, payload_json FROM decisions WHERE project_id=?1 AND decision_type='AUDIT_REMINDER'",
            [project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(outcome, "OPEN");
    let payload: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["category"], "审计提醒");
    assert_eq!(payload["chapterId"], chapter_id);
    assert_eq!(payload["heldBack"].as_array().unwrap().len(), 1);
}

#[test]
fn ld2_an_audit_reminder_is_closed_once_by_acknowledging_it() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(UnregisteredPredicateAlwaysGateway),
    );
    let project_id = "p-ld2-acknowledge";
    let connection = state.manager.create(project_id, "LD2", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:ld2:ack-produce","idempotencyKey":"key:ld2:ack-produce",
        "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","skillsEnabled":false,"entityIds":["entity:actual:linzhou"]}
    }))
    .unwrap();
    let connection = state.manager.open(project_id, "LD2", NOW).unwrap();
    let decision_id: String = connection
        .query_row(
            "SELECT decision_id FROM decisions WHERE project_id=?1 AND decision_type='AUDIT_REMINDER'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);
    let acknowledge = |key: &str| {
        execute_command_impl(
            &state,
            json!({
                "type":"decision_submit","commandId":format!("cmd:ld2:{key}"),"idempotencyKey":format!("key:ld2:{key}"),
                "projectId":project_id,"expectedRevision":state.manager.project_revision(project_id).unwrap(),"requestedAt":NOW,
                "payload":{"decisionId":decision_id,"choice":"ACKNOWLEDGE"}
            }),
        )
    };
    assert_eq!(acknowledge("ack-1").unwrap()["finalStatus"], "committed");
    let connection = state.manager.open(project_id, "LD2", NOW).unwrap();
    let outcome: String = connection
        .query_row(
            "SELECT outcome FROM decisions WHERE project_id=?1 AND decision_type='AUDIT_REMINDER'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(outcome, "ACKNOWLEDGED");
    drop(connection);
    assert!(acknowledge("ack-2")
        .unwrap_err()
        .contains("AUDIT_REMINDER_NOT_OPEN"));
}
