use super::*;

/// A completion claim whose dependencies or predicate are unmet never
/// satisfies the task (2026-10-19): the claim is kept as progress and
/// recorded, and the chapter commits.
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
    let result = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":"cmd:b08:task-completion",
            "idempotencyKey":"key:b08:task-completion","projectId":project_id,
            "expectedRevision":0,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM",
                "entityIds":["entity:actual:linzhou"],"references":[]}
        }),
    )
    .unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let connection = state.manager.open(project_id, "B08 task", NOW).unwrap();
    let claimed: String = connection
        .query_row(
            "SELECT json_extract(p.payload_json,'$.audit.normalization.downgradedTaskClaims') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ReviewReport'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(claimed.contains("task:actual"));
    let satisfied: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM state_deltas WHERE project_id=?1 AND state_id='task:actual' AND json_extract(value_json,'$.status')='SATISFIED'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(satisfied, 0);
}
