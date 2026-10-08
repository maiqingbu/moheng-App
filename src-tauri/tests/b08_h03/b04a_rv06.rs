use super::*;

struct NewResponsibilityGateway;
struct IncompleteResponsibilityGateway;
struct LifecycleActionGateway {
    disposition: &'static str,
    bridge: Option<&'static str>,
}
impl ProductionModelGateway for NewResponsibilityGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        let body = "林舟推开石门。答应在顾清归来时把玉符交给顾清。";
        if capability == "RENDER_CHAPTER" {
            response.output["body"] = json!(body);
        }
        if capability == "AUDIT_EXTRACT" {
            let quote = "答应在顾清归来时把玉符交给顾清";
            let start = body.find(quote).unwrap();
            let start = body[..start].chars().count();
            let end = start + quote.chars().count();
            response.output["continuityCandidates"] = json!([{
                "threadRef":"thread:promise:jade-to-gugu",
                "disposition":"ADVANCE",
                "description":"林舟承诺在顾清归来时把玉符交给顾清。",
                "fulfillmentObject":"item:jade-token",
                "resumeCondition":{"kind":"ENTITY_PRESENT","value":"entity:actual:gugu"},
                "promotionSignals":{"explicitPromise":true},
                "evidence":{"start":start,"end":end,"quote":quote}
            }]);
        }
        Ok(response)
    }
}
impl ProductionModelGateway for IncompleteResponsibilityGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            response.output["continuityCandidates"] = json!([{
                "threadRef":"thread:mystery:hint-only",
                "disposition":"ADVANCE",
                "promotionSignals":{"mysteryOrRevealResponsibility":true}
            }]);
        }
        Ok(response)
    }
}
impl ProductionModelGateway for LifecycleActionGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "PLAN_STRATEGY" {
            response.output["horizonPreview"] = json!([]);
            let mut action = json!({
                "threadRef":"thread:promise:existing",
                "disposition":self.disposition
            });
            if let Some(bridge) = self.bridge {
                action["bridge"] = json!(bridge);
            }
            response.output["continuityActions"] = json!([action]);
        }
        if capability == "AUDIT_EXTRACT" {
            response.output["continuityCandidates"] = json!([{
                "threadRef":"thread:promise:existing",
                "disposition":self.disposition,
                "evidence":{"start":0,"end":6,"quote":"林舟推开石门"}
            }]);
        }
        Ok(response)
    }
}
fn seed_b4a_existing_obligation(connection: &Connection, project_id: &str) {
    connection.execute(
        "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,prerequisites_json,lifecycle,created_at,updated_at,resume_condition_json) VALUES ('promise:existing',?1,'BOOK','PROMISE','B','plan:book:b08-long',1,'既有承诺','[]','PROGRESSING',?2,?2,'{\"kind\":\"ENTITY_PRESENT\",\"value\":\"entity:actual:linzhou\"}')",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
}
fn compile_b4a_lifecycle_plan(
    state: &KernelState,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
) -> i64 {
    let result = execute_command_impl(state, json!({
        "type":"planning_compile_chapter_plan","commandId":format!("cmd:b08:lifecycle-plan:{project_id}"),
        "idempotencyKey":format!("key:b08:lifecycle-plan:{project_id}"),"projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"volumeId":volume_id,
            "scopeSelection":{"taskRefs":["task:actual"],"responsibilityRefs":["promise:existing"],"reservationRefs":[],"entityRefs":["entity:actual:linzhou"]}}
    })).unwrap();
    assert_eq!(result["finalStatus"], "committed");
    result["receipt"]["afterRevision"].as_i64().unwrap()
}
#[test]
fn b08_new_explicit_promise_is_b_responsibility_in_next_chapter_context() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-new-responsibility";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(NewResponsibilityGateway),
    );
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state
        .manager
        .open(project_id, "B08 responsibility", NOW)
        .unwrap();
    connection.execute(
        "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES ('entity:actual:gugu',?1,'顾清','CHARACTER',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    let before_revision = connection
        .query_row::<i64, _, _>(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);

    let result = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":"cmd:b08:new-responsibility",
            "idempotencyKey":"key:b08:new-responsibility","projectId":project_id,
            "expectedRevision":before_revision,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM",
                "entityIds":["entity:actual:linzhou","entity:actual:gugu"],"references":[]}
        }),
    )
    .unwrap();
    assert_eq!(result["finalStatus"], "committed");

    let connection = state
        .manager
        .open(project_id, "B08 responsibility", NOW)
        .unwrap();
    let (authority, lifecycle, resume_condition): (String, String, String) = connection.query_row(
        "SELECT authority_level,lifecycle,resume_condition_json FROM narrative_obligations WHERE project_id=?1 AND obligation_id='promise:jade-to-gugu'",
        [project_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    ).unwrap();
    assert_eq!(authority, "B");
    assert_eq!(lifecycle, "PROGRESSING");
    assert_eq!(
        serde_json::from_str::<Value>(&resume_condition).unwrap(),
        json!({"kind":"ENTITY_PRESENT","value":"entity:actual:gugu"})
    );
    assert_eq!(connection.query_row::<i64, _, _>(
        "SELECT COUNT(*) FROM decisions WHERE project_id=?1 AND decision_type='THREAD_PROMOTION_RULE' AND json_extract(payload_json,'$.ruleVersion')='thread-promotion.r1'",
        [project_id], |row| row.get(0)
    ).unwrap(), 1);
    let center = query_production_center_impl(&state, json!({"projectId":project_id})).unwrap();
    assert_eq!(center["newResponsibilities"][0]["authorityLevel"], "B");
    assert!(center["newResponsibilities"][0]["evidence"]["quote"]
        .as_str()
        .unwrap()
        .contains("玉符"));
    assert_eq!(
        center["newResponsibilities"][0]["ruleVersion"],
        "thread-promotion.r1"
    );

    let next_chapter_id =
        StableIdentityFactory::from_command(StableIdKind::Chapter, "b08:new-responsibility-next")
            .unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,2,'第二章','NORMAL','PLANNED',0,?4,?4)",
        libsql_rusqlite::params![next_chapter_id, project_id, volume_id, NOW],
    ).unwrap();
    drop(connection);
    let manifest = query_context_manifest_impl(&state, json!({
        "projectId":project_id,"chapterId":next_chapter_id,"volumeId":volume_id,
        "executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou","entity:actual:gugu"],
        "references":["promise:jade-to-gugu"],
        "scopeSelection":{"taskRefs":["task:actual"],"responsibilityRefs":["promise:jade-to-gugu"],"reservationRefs":[],"entityRefs":["entity:actual:linzhou","entity:actual:gugu"]}
    })).unwrap();
    assert!(manifest.to_string().contains("promise:jade-to-gugu"));
    let continuity = ai_novel_production_system_core::commands::query_continuity_impl(
        &state,
        json!({
            "projectId":project_id,"references":["promise:jade-to-gugu"],
            "entityIds":["entity:actual:linzhou","entity:actual:gugu"]
        }),
    )
    .unwrap();
    let thread = continuity["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["threadId"] == "thread:promise:jade-to-gugu")
        .unwrap();
    assert_eq!(
        thread["resumeCondition"],
        json!({"kind":"ENTITY_PRESENT","value":"entity:actual:gugu"})
    );
    assert!(thread["evidenceRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|evidence| evidence["quote"]
            .as_str()
            .is_some_and(|quote| quote.contains("玉符"))));
    let adjustment = execute_command_impl(&state, json!({
        "type":"thread_responsibility_adjust","commandId":"cmd:b08:new-responsibility-downgrade",
        "idempotencyKey":"key:b08:new-responsibility-downgrade","projectId":project_id,
        "expectedRevision":before_revision + 1,"requestedAt":NOW,
        "payload":{"threadRef":"thread:promise:jade-to-gugu","action":"DOWNGRADE"}
    })).unwrap();
    assert_eq!(adjustment["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "B08 responsibility", NOW)
        .unwrap();
    let adjustment_payload: String = connection.query_row(
        "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='NarrativeObligationSet'",
        [project_id], |row| row.get(0)
    ).unwrap();
    let adjustment_payload: Value = serde_json::from_str(&adjustment_payload).unwrap();
    assert!(adjustment_payload["sourceEvidenceRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reference| reference["quote"]
            .as_str()
            .is_some_and(|quote| quote.contains("玉符"))));
    let current_level: String = connection.query_row(
        "SELECT authority_level FROM narrative_obligations WHERE project_id=?1 AND obligation_id='promise:jade-to-gugu'",
        [project_id], |row| row.get(0)
    ).unwrap();
    assert_eq!(current_level, "C");
    drop(connection);
    let downgraded_context = query_context_manifest_impl(&state, json!({
        "projectId":project_id,"chapterId":next_chapter_id,"volumeId":volume_id,
        "executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou","entity:actual:gugu"],
        "references":[],"scopeSelection":{"taskRefs":["task:actual"],"responsibilityRefs":[],"reservationRefs":[],"entityRefs":["entity:actual:linzhou","entity:actual:gugu"]}
    })).unwrap();
    assert!(!downgraded_context["continuityThreadRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "thread:promise:jade-to-gugu"));
    let upgrade = execute_command_impl(&state, json!({
        "type":"thread_responsibility_adjust","commandId":"cmd:b08:new-responsibility-upgrade",
        "idempotencyKey":"key:b08:new-responsibility-upgrade","projectId":project_id,
        "expectedRevision":before_revision + 2,"requestedAt":NOW,
        "payload":{"threadRef":"thread:promise:jade-to-gugu","action":"UPGRADE","authorityLevel":"A"}
    })).unwrap();
    assert_eq!(upgrade["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "B08 responsibility", NOW)
        .unwrap();
    assert_eq!(connection.query_row::<String, _, _>(
        "SELECT authority_level FROM narrative_obligations WHERE project_id=?1 AND obligation_id='promise:jade-to-gugu'",
        [project_id], |row| row.get(0)
    ).unwrap(), "A");
}
#[test]
fn b08_incomplete_new_responsibility_is_retained_as_c_without_blocking_the_chapter() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-incomplete-responsibility";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(IncompleteResponsibilityGateway),
    );
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let result = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":"cmd:b08:incomplete-responsibility",
            "idempotencyKey":"key:b08:incomplete-responsibility","projectId":project_id,
            "expectedRevision":0,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM",
                "entityIds":["entity:actual:linzhou"],"references":[]}
        }),
    )
    .unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "B08 responsibility", NOW)
        .unwrap();
    let authority: String = connection.query_row(
        "SELECT authority_level FROM narrative_obligations WHERE project_id=?1 AND obligation_id='mystery:hint-only'",
        [project_id], |row| row.get(0)
    ).unwrap();
    assert_eq!(authority, "C");
    let next_chapter_id = StableIdentityFactory::from_command(
        StableIdKind::Chapter,
        "b08:incomplete-responsibility-next",
    )
    .unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,2,'第二章','NORMAL','PLANNED',0,?4,?4)",
        libsql_rusqlite::params![next_chapter_id, project_id, volume_id, NOW],
    ).unwrap();
    drop(connection);
    let manifest = query_context_manifest_impl(&state, json!({
        "projectId":project_id,"chapterId":next_chapter_id,"volumeId":volume_id,
        "executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou"],"references":[],
        "scopeSelection":{"taskRefs":["task:actual"],"responsibilityRefs":[],"reservationRefs":[],"entityRefs":["entity:actual:linzhou"]}
    })).unwrap();
    assert!(!manifest["continuityThreadRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "thread:mystery:hint-only"));
    let upgrade = execute_command_impl(&state, json!({
        "type":"thread_responsibility_adjust","commandId":"cmd:b08:incomplete-responsibility-upgrade",
        "idempotencyKey":"key:b08:incomplete-responsibility-upgrade","projectId":project_id,
        "expectedRevision":1,"requestedAt":NOW,
        "payload":{"threadRef":"thread:mystery:hint-only","action":"UPGRADE","authorityLevel":"B",
            "fulfillmentObject":"entity:actual:linzhou",
            "resumeCondition":{"kind":"ENTITY_PRESENT","value":"entity:actual:linzhou"},
            "evidence":{"start":0,"end":6,"quote":"林舟推开石门"}}
    })).unwrap();
    assert_eq!(upgrade["finalStatus"], "committed");
    let recompiled_context = query_context_manifest_impl(&state, json!({
        "projectId":project_id,"chapterId":next_chapter_id,"volumeId":volume_id,
        "executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou"],
        "references":["mystery:hint-only"],
        "scopeSelection":{"taskRefs":["task:actual"],"responsibilityRefs":["mystery:hint-only"],"reservationRefs":[],"entityRefs":["entity:actual:linzhou"]}
    })).unwrap();
    assert!(recompiled_context["continuityThreadRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "thread:mystery:hint-only"));
}
#[test]
fn b08_defer_with_bridge_updates_existing_thread_and_records_the_bridge() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-defer-with-bridge";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(LifecycleActionGateway {
            disposition: "DEFER_WITH_BRIDGE",
            bridge: Some("顾清归来前保管玉符"),
        }),
    );
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state
        .manager
        .open(project_id, "B08 lifecycle", NOW)
        .unwrap();
    seed_b4a_existing_obligation(&connection, project_id);
    connection
        .execute(
            "DELETE FROM canonical_heads WHERE project_id=?1 AND artifact_type='ChapterPlan'",
            [project_id],
        )
        .unwrap();
    drop(connection);
    let production_revision =
        compile_b4a_lifecycle_plan(&state, project_id, &chapter_id, &volume_id);
    let result = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":"cmd:b08:defer-with-bridge",
            "idempotencyKey":"key:b08:defer-with-bridge","projectId":project_id,
            "expectedRevision":production_revision,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM",
                "entityIds":["entity:actual:linzhou"],"references":["promise:existing"]}
        }),
    )
    .unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "B08 lifecycle", NOW)
        .unwrap();
    assert_eq!(connection.query_row::<String, _, _>(
        "SELECT lifecycle FROM narrative_obligations WHERE project_id=?1 AND obligation_id='promise:existing'",
        [project_id], |row| row.get(0)
    ).unwrap(), "PROGRESSING");
    let decision_payload: String = connection.query_row(
        "SELECT payload_json FROM decisions WHERE project_id=?1 AND decision_type='THREAD_DEFER_WITH_BRIDGE'",
        [project_id], |row| row.get(0)
    ).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&decision_payload).unwrap()["bridge"],
        "顾清归来前保管玉符"
    );
}
#[test]
fn b08_defer_without_bridge_blocks_before_final_commit() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-defer-without-bridge";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(LifecycleActionGateway {
            disposition: "DEFER_WITH_BRIDGE",
            bridge: None,
        }),
    );
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state
        .manager
        .open(project_id, "B08 lifecycle", NOW)
        .unwrap();
    seed_b4a_existing_obligation(&connection, project_id);
    connection
        .execute(
            "DELETE FROM canonical_heads WHERE project_id=?1 AND artifact_type='ChapterPlan'",
            [project_id],
        )
        .unwrap();
    drop(connection);
    let error = execute_command_impl(&state, json!({
        "type":"planning_compile_chapter_plan","commandId":"cmd:b08:defer-without-bridge",
        "idempotencyKey":"key:b08:defer-without-bridge","projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"volumeId":volume_id,
            "scopeSelection":{"taskRefs":["task:actual"],"responsibilityRefs":["promise:existing"],"reservationRefs":[],"entityRefs":["entity:actual:linzhou"]}}
    })).unwrap_err();
    assert_eq!(error, "CONTINUITY_BRIDGE_REQUIRED");
    let connection = state
        .manager
        .open(project_id, "B08 lifecycle", NOW)
        .unwrap();
    assert_eq!(connection.query_row::<i64, _, _>(
        "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='FinalChapter' AND artifact_status='COMMITTED'",
        [project_id], |row| row.get(0)
    ).unwrap(), 0);
    assert_eq!(connection.query_row::<String, _, _>(
        "SELECT lifecycle FROM narrative_obligations WHERE project_id=?1 AND obligation_id='promise:existing'",
        [project_id], |row| row.get(0)
    ).unwrap(), "PROGRESSING");
}
#[test]
fn b08_no_longer_applicable_creates_a_change_request_proposal_without_resolving_the_thread() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-no-longer-applicable";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(LifecycleActionGateway {
            disposition: "NO_LONGER_APPLICABLE_CHANGE_REQUEST",
            bridge: None,
        }),
    );
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state
        .manager
        .open(project_id, "B08 lifecycle", NOW)
        .unwrap();
    seed_b4a_existing_obligation(&connection, project_id);
    connection
        .execute(
            "DELETE FROM canonical_heads WHERE project_id=?1 AND artifact_type='ChapterPlan'",
            [project_id],
        )
        .unwrap();
    drop(connection);
    let production_revision =
        compile_b4a_lifecycle_plan(&state, project_id, &chapter_id, &volume_id);
    let result = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":"cmd:b08:no-longer-applicable",
            "idempotencyKey":"key:b08:no-longer-applicable","projectId":project_id,
            "expectedRevision":production_revision,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM",
                "entityIds":["entity:actual:linzhou"],"references":["promise:existing"]}
        }),
    )
    .unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "B08 lifecycle", NOW)
        .unwrap();
    assert_eq!(connection.query_row::<String, _, _>(
        "SELECT lifecycle FROM narrative_obligations WHERE project_id=?1 AND obligation_id='promise:existing'",
        [project_id], |row| row.get(0)
    ).unwrap(), "PROGRESSING");
    assert_eq!(connection.query_row::<i64, _, _>(
        "SELECT COUNT(*) FROM canonical_heads WHERE project_id=?1 AND artifact_type='ChangeRequest'",
        [project_id], |row| row.get(0)
    ).unwrap(), 1);
    let status: String = connection.query_row(
        "SELECT json_extract(p.payload_json,'$.status') FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ChangeRequest'",
        [project_id], |row| row.get(0)
    ).unwrap();
    assert_eq!(status, "PROPOSED");
}
#[test]
fn b08_user_responsibility_adjustments_are_versioned_and_replayable() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-responsibility-adjustment";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state
        .manager
        .open(project_id, "B08 adjustment", NOW)
        .unwrap();
    seed_b4a_existing_obligation(&connection, project_id);
    connection.execute(
        "INSERT INTO decisions(decision_id,project_id,decision_type,outcome,reason,source_ref,payload_json,created_at,updated_at) VALUES ('decision:rv33:audited-existing-responsibility',?1,'THREAD_PROMOTION_RULE','B','B4-A 用户调整控制项应代表审计新增责任','thread:promise:existing','{\"ruleVersion\":\"thread-promotion.r1\"}',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    drop(connection);
    let downgrade = json!({
        "type":"thread_responsibility_adjust","commandId":"cmd:b08:adjust:1",
        "idempotencyKey":"key:b08:adjust:1","projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
        "payload":{"threadRef":"thread:promise:existing","action":"DOWNGRADE","authorityLevel":"C"}
    });
    let result = execute_command_impl(&state, downgrade.clone()).unwrap();
    assert_eq!(result["finalStatus"], "committed");
    assert_eq!(result["receipt"]["afterRevision"], 1);
    let replay = execute_command_impl(&state, downgrade).unwrap();
    assert_eq!(replay["replayed"], true);
    let connection = state
        .manager
        .open(project_id, "B08 adjustment", NOW)
        .unwrap();
    assert_eq!(connection.query_row::<String, _, _>(
        "SELECT authority_level FROM narrative_obligations WHERE project_id=?1 AND obligation_id='promise:existing'",
        [project_id], |row| row.get(0)
    ).unwrap(), "C");
    assert_eq!(connection.query_row::<i64, _, _>(
        "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='NarrativeObligationSet'",
        [project_id], |row| row.get(0)
    ).unwrap(), 1);
    let snapshot_v1: String = connection.query_row(
        "SELECT payload_json FROM artifact_payloads WHERE artifact_id=(SELECT artifact_id FROM canonical_heads WHERE project_id=?1 AND artifact_type='NarrativeObligationSet') AND version=1",
        [project_id], |row| row.get(0)
    ).unwrap();
    let snapshot_v1: Value = serde_json::from_str(&snapshot_v1).unwrap();
    assert_eq!(snapshot_v1["previousAuthorityLevel"], "B");
    assert_eq!(snapshot_v1["actor"], "USER");
    assert_eq!(snapshot_v1["changedAt"], NOW);
    drop(connection);

    let deletion = execute_command_impl(&state, json!({
        "type":"thread_responsibility_adjust","commandId":"cmd:b08:adjust:2",
        "idempotencyKey":"key:b08:adjust:2","projectId":project_id,"expectedRevision":1,"requestedAt":NOW,
        "payload":{"threadRef":"thread:promise:existing","action":"DELETE"}
    })).unwrap();
    assert_eq!(deletion["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "B08 adjustment", NOW)
        .unwrap();
    assert_eq!(connection.query_row::<String, _, _>(
        "SELECT authority_level FROM narrative_obligations WHERE project_id=?1 AND obligation_id='promise:existing'",
        [project_id], |row| row.get(0)
    ).unwrap(), "C");
    assert_eq!(connection.query_row::<String, _, _>(
        "SELECT lifecycle FROM narrative_obligations WHERE project_id=?1 AND obligation_id='promise:existing'",
        [project_id], |row| row.get(0)
    ).unwrap(), "ABANDONED");
    assert_eq!(connection.query_row::<i64, _, _>(
        "SELECT COUNT(*) FROM artifact_payloads p JOIN artifacts a ON a.artifact_id=p.artifact_id AND a.version=p.version WHERE a.project_id=?1 AND a.artifact_type='NarrativeObligationSet'",
        [project_id], |row| row.get(0)
    ).unwrap(), 2);
    let snapshot_v2: String = connection.query_row(
        "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=2 WHERE h.project_id=?1 AND h.artifact_type='NarrativeObligationSet'",
        [project_id], |row| row.get(0)
    ).unwrap();
    let snapshot_v2: Value = serde_json::from_str(&snapshot_v2).unwrap();
    assert_eq!(snapshot_v2["previousAdjustmentRef"]["version"], 1);
    let _ = (chapter_id, volume_id);
}
