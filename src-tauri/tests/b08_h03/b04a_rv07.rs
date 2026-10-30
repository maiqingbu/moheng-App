use super::*;

struct UnboundKnowledgeGateway;
struct UnknownKnowledgeMetadataGateway;
struct FutureNarrativeKnowledgeGateway;
struct FutureNarrativeUnknownTimeGateway;
struct ExplicitUnknownPrivateKnowledgeGateway;
struct LocationTransitionGateway;
impl ProductionModelGateway for LocationTransitionGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            response.output["canonCandidates"] = json!([{
                "factId":"fact:fx17:moved","subjectId":"entity:actual:linzhou",
                "predicate":"location","objectJson":{"place":"river"},"authorityLevel":"A",
                "evidence":{"start":0,"end":6,"quote":"林舟推开石门"}
            }]);
            response.output["knowledgeCandidates"] = json!([]);
            response.output["auditCompleteness"] = json!("COMPLETE_WITH_DELTA");
        }
        Ok(response)
    }
}
impl ProductionModelGateway for FutureNarrativeUnknownTimeGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = FutureNarrativeKnowledgeGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            response.output["knowledgeCandidates"][0]["acquiredStoryTime"] = json!("UNKNOWN");
        }
        Ok(response)
    }
}
impl ProductionModelGateway for ExplicitUnknownPrivateKnowledgeGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = KnowledgeGateway {
            contradictory: false,
        }
        .invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            response.output["canonCandidates"][0]
                .as_object_mut()
                .unwrap()
                .remove("storyTime");
            response.output["knowledgeCandidates"][0]["acquiredStoryTime"] = json!("UNKNOWN");
            response.output["knowledgeCandidates"][0]["visibilityScope"] = json!("PRIVATE");
        }
        Ok(response)
    }
}
impl ProductionModelGateway for UnboundKnowledgeGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = KnowledgeGateway {
            contradictory: false,
        }
        .invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            response.output["knowledgeCandidates"][0]["factId"] = json!("fact:unbound");
        }
        Ok(response)
    }
}
impl ProductionModelGateway for UnknownKnowledgeMetadataGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = KnowledgeGateway {
            contradictory: false,
        }
        .invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            response.output["knowledgeCandidates"][0]["acquiredStoryTime"] = json!("UNKNOWN");
            response.output["knowledgeCandidates"][0]["visibilityScope"] = json!("UNKNOWN");
        }
        Ok(response)
    }
}
impl ProductionModelGateway for FutureNarrativeKnowledgeGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            response.output["canonCandidates"] = json!([]);
            response.output["knowledgeCandidates"] = json!([{
                "knowledgeId":"knowledge:future-event",
                "knowerId":"entity:actual:linzhou",
                "factId":"fact:actual:seed",
                "epistemicState":"KNOWS",
                "visibilityScope":"CHARACTER:entity:actual:linzhou",
                "acquiredStoryTime":"2020-01-01T00:00:00Z",
                "evidence":{"start":0,"end":6,"quote":"林舟推开石门"}
            }]);
            response.output["auditCompleteness"] = json!("COMPLETE_WITH_DELTA");
        }
        Ok(response)
    }
}
#[test]
fn fx17_unbound_knowledge_candidate_is_dropped_and_recorded() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-unbound-knowledge";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(UnboundKnowledgeGateway),
    );
    let connection = state
        .manager
        .create(project_id, "B08 knowledge", NOW)
        .unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:unbound-knowledge",
        "idempotencyKey":"key:b08:unbound-knowledge","projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
    })).unwrap();
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    assert_eq!(
        b08_committed_final_count_for_chapter(&connection, project_id, &chapter_id),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM canon_knowledge WHERE project_id=?1 AND knowledge_id=?2",
                libsql_rusqlite::params![project_id, new_audit_item_id("know", &chapter_id, 1)],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );
    let receipt: Value = serde_json::from_str(&connection.query_row::<String, _, _>(
        "SELECT receipt_json FROM domain_receipts WHERE project_id=?1 AND domain='B08_CHAPTER_PRODUCTION'",
        [project_id], |row| row.get(0)
    ).unwrap()).unwrap();
    let dropped = receipt
        .pointer("/payload/droppedUnboundKnowledge")
        .unwrap_or_else(|| panic!("{receipt}"));
    assert_eq!(
        dropped[0]["knowledgeId"],
        new_audit_item_id("know", &chapter_id, 1)
    );
    assert_eq!(dropped[0]["factId"], "fact:unbound");
}
#[test]
fn b08_unknown_knowledge_time_or_visibility_is_held_back() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-unknown-knowledge-metadata";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(UnknownKnowledgeMetadataGateway),
    );
    let connection = state
        .manager
        .create(project_id, "B08 knowledge", NOW)
        .unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    let result = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:unknown-knowledge-metadata",
        "idempotencyKey":"key:b08:unknown-knowledge-metadata","projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
    })).unwrap();
    // LD-1 step 2: the chapter commits; the item is held back.
    assert_eq!(result["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    assert_held_back(
        &connection,
        project_id,
        "B08_AUDIT_INCOMPLETE_KNOWLEDGE_TIME",
    );
    assert_eq!(
        b08_committed_final_count_for_chapter(&connection, project_id, &chapter_id),
        1
    );
}
#[test]
fn b08_knowledge_of_an_event_from_a_later_narrative_chapter_is_held_back() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-future-narrative-knowledge";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(FutureNarrativeKnowledgeGateway),
    );
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    connection
        .execute(
            "UPDATE chapters SET chapter_no=5 WHERE project_id=?1 AND chapter_id=?2",
            libsql_rusqlite::params![project_id, chapter_id],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE volumes SET chapter_end=9 WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id],
        )
        .unwrap();
    let future_chapter_id =
        StableIdentityFactory::from_command(StableIdKind::Chapter, "b08:future-event").unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,9,'第九章','NORMAL','PLANNED',0,?4,?4)",
        libsql_rusqlite::params![future_chapter_id, project_id, volume_id, NOW],
    ).unwrap();
    connection.execute(
        "UPDATE canon_facts SET source_chapter_id=?1 WHERE project_id=?2 AND fact_id='fact:actual:seed'",
        libsql_rusqlite::params![future_chapter_id, project_id],
    ).unwrap();
    drop(connection);
    let result = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:future-narrative-knowledge",
        "idempotencyKey":"key:b08:future-narrative-knowledge","projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou"],"references":[]}
    })).unwrap();
    // LD-1 step 2: the chapter commits; the item is held back.
    assert_eq!(result["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    assert_held_back(
        &connection,
        project_id,
        "B08_KNOWLEDGE_FUTURE_EVENT_CONTRADICTION",
    );
    assert_eq!(
        b08_committed_final_count_for_chapter(&connection, project_id, &chapter_id),
        1
    );
}

#[test]
fn fx17_unknown_world_time_still_holds_back_knowledge_of_a_later_chapter_event() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-fx17-future-narrative-unknown-time";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(FutureNarrativeUnknownTimeGateway),
    );
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    connection
        .execute(
            "UPDATE chapters SET chapter_no=5 WHERE project_id=?1 AND chapter_id=?2",
            libsql_rusqlite::params![project_id, chapter_id],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE volumes SET chapter_end=9 WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id],
        )
        .unwrap();
    let future_chapter_id =
        StableIdentityFactory::from_command(StableIdKind::Chapter, "b08:future-event").unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,9,'第九章','NORMAL','PLANNED',0,?4,?4)",
        libsql_rusqlite::params![future_chapter_id, project_id, volume_id, NOW],
    ).unwrap();
    connection.execute(
        "UPDATE canon_facts SET source_chapter_id=?1 WHERE project_id=?2 AND fact_id='fact:actual:seed'",
        libsql_rusqlite::params![future_chapter_id, project_id],
    ).unwrap();
    drop(connection);
    let result = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:future-narrative-unknown-time",
        "idempotencyKey":"key:b08:future-narrative-unknown-time","projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou"],"references":[]}
    })).unwrap();
    // LD-1 step 2: the chapter commits; the item is held back.
    assert_eq!(result["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    assert_held_back(
        &connection,
        project_id,
        "B08_KNOWLEDGE_FUTURE_EVENT_CONTRADICTION",
    );
    assert_eq!(
        b08_committed_final_count_for_chapter(&connection, project_id, &chapter_id),
        1
    );
}
#[test]
fn fx17_explicit_unknown_time_and_private_visibility_commit_by_narrative_order() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-fx17-unknown-private-knowledge";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(ExplicitUnknownPrivateKnowledgeGateway),
    );
    let connection = state
        .manager
        .create(project_id, "B08 knowledge", NOW)
        .unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:fx17:unknown-private",
        "idempotencyKey":"key:fx17:unknown-private","projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
    })).unwrap();
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    assert_eq!(connection.query_row::<String, _, _>(
        "SELECT visibility_scope FROM canon_knowledge WHERE project_id=?1 AND knowledge_id=?2",
        libsql_rusqlite::params![project_id, new_audit_item_id("know", &chapter_id, 1)], |row| row.get(0)
    ).unwrap(), "CHARACTER:entity:actual:linzhou");
}

#[test]
fn fx17_later_chapter_location_change_supersedes_the_open_earlier_fact() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-fx17-location-transition";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(LocationTransitionGateway),
    );
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    connection
        .execute(
            "UPDATE chapters SET chapter_no=5 WHERE project_id=?1 AND chapter_id=?2",
            libsql_rusqlite::params![project_id, chapter_id],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE volumes SET chapter_end=9 WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id],
        )
        .unwrap();
    let earlier_chapter_id =
        StableIdentityFactory::from_command(StableIdKind::Chapter, "fx17:earlier").unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,2,'第二章','NORMAL','COMMITTED',0,?4,?4)",
        libsql_rusqlite::params![earlier_chapter_id, project_id, volume_id, NOW],
    ).unwrap();
    connection.execute(
        "UPDATE canon_facts SET valid_from_chapter_id=?1, source_chapter_id=?1, story_time_from=NULL, story_time_to=NULL WHERE project_id=?2 AND fact_id='fact:actual:seed'",
        libsql_rusqlite::params![earlier_chapter_id, project_id],
    ).unwrap();
    drop(connection);
    execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:fx17:location-transition",
        "idempotencyKey":"key:fx17:location-transition","projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou"],"references":[]}
    })).unwrap();
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    let superseded_by = connection.query_row::<Option<String>, _, _>(
        "SELECT superseded_by FROM canon_facts WHERE project_id=?1 AND fact_id='fact:actual:seed'",
        [project_id], |row| row.get(0)
    ).unwrap();
    assert_eq!(
        superseded_by.as_deref(),
        Some(new_audit_item_id("canon", &chapter_id, 1).as_str())
    );
    let (object, valid_from) = connection.query_row::<(String, String), _, _>(
        "SELECT object_json, valid_from_chapter_id FROM canon_facts WHERE project_id=?1 AND fact_id=?2 AND superseded_by IS NULL",
        libsql_rusqlite::params![project_id, new_audit_item_id("canon", &chapter_id, 1)], |row| Ok((row.get(0)?, row.get(1)?))
    ).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&object).unwrap(),
        json!({"place":"river"})
    );
    assert_eq!(valid_from, chapter_id);
}

/// An earlier chapter restated the same location, leaving two equal open
/// facts (seen on 2026-10-18); `duplicate_object` sets the second one.
fn location_change_over_two_open_facts(
    project_id: &str,
    duplicate_object: &str,
) -> (KernelState, tempfile::TempDir, Result<Value, String>) {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(LocationTransitionGateway),
    );
    // An opening chapter is planned at production start, so the extra fact
    // cannot make a precompiled plan stale.
    let connection = state
        .manager
        .create(project_id, "B08 knowledge", NOW)
        .unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    let volume_id: String = connection
        .query_row(
            "SELECT volume_id FROM chapters WHERE project_id=?1 AND chapter_id=?2",
            [project_id, chapter_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute(
            "UPDATE chapters SET chapter_no=3 WHERE project_id=?1 AND chapter_id=?2",
            libsql_rusqlite::params![project_id, chapter_id],
        )
        .unwrap();
    let earlier_chapter_id =
        StableIdentityFactory::from_command(StableIdKind::Chapter, "craft:earlier").unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,2,'第二章','NORMAL','COMMITTED',0,?4,?4)",
        libsql_rusqlite::params![earlier_chapter_id, project_id, volume_id, NOW],
    ).unwrap();
    connection.execute(
        "UPDATE canon_facts SET valid_from_chapter_id=?1, source_chapter_id=?1, story_time_from=NULL, story_time_to=NULL WHERE project_id=?2 AND fact_id='fact:actual:seed'",
        libsql_rusqlite::params![earlier_chapter_id, project_id],
    ).unwrap();
    connection.execute(
        "INSERT INTO canon_facts(fact_id, project_id, subject_id, predicate, object_json, valid_from_chapter_id, valid_to_chapter_id, story_time_from, story_time_to, source_artifact_id, source_version, source_chapter_id, evidence_range_json, authority_level, superseded_by, created_at, updated_at) SELECT 'fact:actual:seed-again', project_id, subject_id, predicate, ?2, valid_from_chapter_id, valid_to_chapter_id, story_time_from, story_time_to, source_artifact_id, source_version, source_chapter_id, evidence_range_json, authority_level, NULL, created_at, updated_at FROM canon_facts WHERE project_id=?1 AND fact_id='fact:actual:seed'",
        libsql_rusqlite::params![project_id, duplicate_object],
    ).unwrap();
    let seed_object: String = connection.query_row("SELECT object_json FROM canon_facts WHERE project_id=?1 AND fact_id='fact:actual:seed'", [project_id], |row| row.get(0)).unwrap();
    if duplicate_object.is_empty() {
        connection.execute("UPDATE canon_facts SET object_json=?2 WHERE project_id=?1 AND fact_id='fact:actual:seed-again'", libsql_rusqlite::params![project_id, seed_object]).unwrap();
    }
    drop(connection);
    let result = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":format!("cmd:{project_id}"),
            "idempotencyKey":format!("key:{project_id}"),"projectId":project_id,
            "expectedRevision":0,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
        }),
    );
    (state, temp, result)
}

#[test]
fn craft_location_change_supersedes_every_equal_open_fact() {
    let project_id = "p-craft-equal-open-facts";
    let (state, _temp, result) = location_change_over_two_open_facts(project_id, "");
    result.unwrap();
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    let chapter_id: String = connection
        .query_row(
            "SELECT chapter_id FROM chapters WHERE project_id=?1 AND chapter_no=3",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    let moved = new_audit_item_id("canon", &chapter_id, 1);
    for fact_id in ["fact:actual:seed", "fact:actual:seed-again"] {
        let superseded_by = connection
            .query_row::<Option<String>, _, _>(
                "SELECT superseded_by FROM canon_facts WHERE project_id=?1 AND fact_id=?2",
                [project_id, fact_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(superseded_by.as_deref(), Some(moved.as_str()), "{fact_id}");
    }
}

#[test]
fn craft_location_change_over_two_different_open_facts_is_held_back() {
    let project_id = "p-craft-different-open-facts";
    let (state, _temp, result) =
        location_change_over_two_open_facts(project_id, "{\"place\":\"cave\"}");
    // LD-1 step 2: the chapter commits; the unresolved location is held back.
    assert_eq!(result.unwrap()["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    assert_held_back(&connection, project_id, "B08_CANON_TIME_UNRESOLVED");
}
