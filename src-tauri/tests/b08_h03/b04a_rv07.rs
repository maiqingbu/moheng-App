use super::*;

struct UnboundKnowledgeGateway;
struct UnknownKnowledgeMetadataGateway;
struct FutureNarrativeKnowledgeGateway;
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
fn b08_unbound_knowledge_candidate_makes_the_audit_incomplete() {
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
    let error = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:unbound-knowledge",
        "idempotencyKey":"key:b08:unbound-knowledge","projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
    })).unwrap_err();
    assert!(
        error.starts_with("B08_AUDIT_INCOMPLETE"),
        "unexpected error: {error}"
    );
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    assert_eq!(
        b08_committed_final_count_for_chapter(&connection, project_id, &chapter_id),
        0
    );
    assert_eq!(connection.query_row::<i64, _, _>(
        "SELECT COUNT(*) FROM canon_knowledge WHERE project_id=?1 AND knowledge_id='knowledge:actual:flashback'",
        [project_id], |row| row.get(0)
    ).unwrap(), 0);
}
#[test]
fn b08_unknown_knowledge_time_or_visibility_blocks_when_hard_scope_is_affected() {
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
    let error = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:unknown-knowledge-metadata",
        "idempotencyKey":"key:b08:unknown-knowledge-metadata","projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
    })).unwrap_err();
    assert_eq!(error, "B08_AUDIT_INCOMPLETE_KNOWLEDGE_TIME");
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    assert_eq!(
        b08_committed_final_count_for_chapter(&connection, project_id, &chapter_id),
        0
    );
}
#[test]
fn b08_knowledge_cannot_reference_an_event_from_a_later_narrative_chapter() {
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
    let error = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:future-narrative-knowledge",
        "idempotencyKey":"key:b08:future-narrative-knowledge","projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou"],"references":[]}
    })).unwrap_err();
    assert_eq!(error, "B08_KNOWLEDGE_FUTURE_EVENT_CONTRADICTION");
    let connection = state
        .manager
        .open(project_id, "B08 knowledge", NOW)
        .unwrap();
    assert_eq!(
        b08_committed_final_count_for_chapter(&connection, project_id, &chapter_id),
        0
    );
}
