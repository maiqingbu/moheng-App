use super::*;

struct KnowledgeTimeGateway {
    acquired_story_time: String,
    fact_story_time: String,
    use_existing_fact: bool,
}

impl ProductionModelGateway for KnowledgeTimeGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            let fact_id = if self.use_existing_fact {
                response.output["canonCandidates"] = json!([]);
                "fact:actual:seed".to_string()
            } else {
                let candidate = response.output["canonCandidates"]
                    .as_array_mut()
                    .and_then(|items| items.first_mut())
                    .ok_or_else(|| {
                        "recording gateway did not return a Canon candidate".to_string()
                    })?;
                candidate["storyTime"] = json!(self.fact_story_time);
                candidate["factId"]
                    .as_str()
                    .ok_or_else(|| "recording gateway Canon candidate has no factId".to_string())?
                    .to_string()
            };
            response.output["knowledgeCandidates"] = json!([{
                "knowledgeId":"knowledge:fx4:time",
                "knowerId":"entity:actual:linzhou",
                "factId":fact_id,
                "epistemicState":"KNOWS",
                "visibilityScope":"CHARACTER:entity:actual:linzhou",
                "acquiredStoryTime":self.acquired_story_time,
                "evidence":{"start":0,"end":6,"quote":"林舟推开石门"}
            }]);
            response.output["auditCompleteness"] = json!("COMPLETE_WITH_DELTA");
        }
        Ok(response)
    }
}

fn setup_opening_knowledge_project(
    project_id: &str,
    acquired_story_time: &str,
    fact_story_time: &str,
    use_existing_fact: bool,
) -> (tempfile::TempDir, KernelState, String) {
    let temp = tempfile::tempdir().expect("temporary project root");
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(KnowledgeTimeGateway {
            acquired_story_time: acquired_story_time.into(),
            fact_story_time: fact_story_time.into(),
            use_existing_fact,
        }),
    );
    let connection = state
        .manager
        .create(project_id, "FX-4 knowledge time", NOW)
        .expect("create project");
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    (temp, state, chapter_id)
}

fn start_opening_knowledge_audit(
    state: &KernelState,
    project_id: &str,
    chapter_id: &str,
    command_id: &str,
) -> Result<Value, String> {
    execute_command_impl(
        state,
        json!({
            "type":"production_start",
            "commandId":command_id,
            "idempotencyKey":format!("{command_id}:key"),
            "projectId":project_id,
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{
                "chapterId":chapter_id,
                "executionMode":"OPENING_BOOTSTRAP",
                "entityIds":["entity:actual:linzhou"]
            }
        }),
    )
}

fn assert_no_knowledge_production_write(state: &KernelState, project_id: &str, chapter_id: &str) {
    let connection = state
        .manager
        .open(project_id, "FX-4 knowledge time", NOW)
        .expect("reopen project after rejected audit");
    assert_eq!(
        b08_committed_final_count_for_chapter(&connection, project_id, chapter_id),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM canon_knowledge WHERE project_id=?1",
                [project_id],
                |row| row.get(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn fx4_knowledge_time_compares_parsed_instants() {
    let project_id = "p-fx4-equal-instants";
    let (_temp, state, chapter_id) = setup_opening_knowledge_project(
        project_id,
        "2030-01-01T08:00:00+08:00",
        "2030-01-01T00:00:00Z",
        false,
    );

    let result =
        start_opening_knowledge_audit(&state, project_id, &chapter_id, "cmd:fx4:equal-instants")
            .expect("equivalent timezone instants should pass the audit");

    assert_eq!(result["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "FX-4 knowledge time", NOW)
        .unwrap();
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM canon_knowledge WHERE project_id=?1 AND knowledge_id=?2",
                libsql_rusqlite::params![project_id, new_audit_item_id("know", &chapter_id, 1)],
                |row| row.get(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn fx4_knowledge_before_fact_blocks() {
    let project_id = "p-fx4-knowledge-before-fact";
    let (_temp, state, chapter_id) = setup_opening_knowledge_project(
        project_id,
        "2029-12-31T00:00:00Z",
        "2030-01-01T00:00:00Z",
        false,
    );

    let error = start_opening_knowledge_audit(
        &state,
        project_id,
        &chapter_id,
        "cmd:fx4:knowledge-before-fact",
    )
    .unwrap_err();

    assert_eq!(error, "B08_KNOWLEDGE_FUTURE_EVENT_CONTRADICTION");
    assert_no_knowledge_production_write(&state, project_id, &chapter_id);
}

#[test]
fn fx4_unparseable_story_time_fails_closed() {
    let project_id = "p-fx4-unparseable-candidate-time";
    let (_temp, state, chapter_id) =
        setup_opening_knowledge_project(project_id, "第三年春", "2030-01-01T00:00:00Z", false);

    let error = start_opening_knowledge_audit(
        &state,
        project_id,
        &chapter_id,
        "cmd:fx4:unparseable-candidate-time",
    )
    .unwrap_err();

    assert_eq!(error, "B08_AUDIT_INCOMPLETE_KNOWLEDGE_TIME");
    assert_no_knowledge_production_write(&state, project_id, &chapter_id);
}

#[test]
fn fx4_unparseable_historical_fact_time_fails_closed() {
    let project_id = "p-fx4-unparseable-historical-time";
    let (_temp, state, chapter_id) = setup_opening_knowledge_project(
        project_id,
        "2021-01-01T00:00:00Z",
        "unused-pending-fact-time",
        true,
    );
    let connection = state
        .manager
        .open(project_id, "FX-4 knowledge time", NOW)
        .unwrap();
    connection
        .execute(
            "UPDATE canon_facts SET story_time_from='第三年春' WHERE project_id=?1 AND fact_id='fact:actual:seed'",
            [project_id],
        )
        .unwrap();
    drop(connection);

    let error = start_opening_knowledge_audit(
        &state,
        project_id,
        &chapter_id,
        "cmd:fx4:unparseable-historical-time",
    )
    .unwrap_err();

    assert_eq!(error, "B08_AUDIT_INCOMPLETE_KNOWLEDGE_TIME");
    assert_no_knowledge_production_write(&state, project_id, &chapter_id);
}
