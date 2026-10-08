use super::*;

#[test]
fn b08_task_satisfied_requires_satisfied_dependencies_and_completion_predicate() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-task-completion";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(TaskSatisfiedGateway),
    );
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state.manager.open(project_id, "B08 task", NOW).unwrap();
    connection.execute(
        "UPDATE state_deltas SET value_json=json_set(value_json,'$.status','ACTIVE') WHERE project_id=?1 AND state_id='task:prerequisite'",
        [project_id],
    ).unwrap();
    drop(connection);
    let error = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":"cmd:b08:task-completion",
            "idempotencyKey":"key:b08:task-completion","projectId":project_id,
            "expectedRevision":0,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM",
                "entityIds":["entity:actual:linzhou"],"references":[]}
        }),
    )
    .unwrap_err();
    assert_eq!(error, "B08_TASK_COMPLETION_PREDICATE_UNSATISFIED");
    let connection = state.manager.open(project_id, "B08 task", NOW).unwrap();
    assert_eq!(connection.query_row::<i64, _, _>(
        "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='FinalChapter' AND artifact_status='COMMITTED'",
        [project_id], |row| row.get(0)
    ).unwrap(), 0);
}
