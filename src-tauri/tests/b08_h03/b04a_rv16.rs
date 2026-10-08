use super::*;

struct MisalignedEvidenceGateway;
impl ProductionModelGateway for MisalignedEvidenceGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            response.output["canonCandidates"][0]["evidence"]["quote"] = json!("错误引用");
        }
        Ok(response)
    }
}
#[test]
fn b08_misaligned_exact_evidence_blocks_the_final_commit() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-misaligned-evidence";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(MisalignedEvidenceGateway),
    );
    let connection = state
        .manager
        .create(project_id, "B08 evidence", NOW)
        .unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    let error = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:misaligned-evidence",
        "idempotencyKey":"key:b08:misaligned-evidence","projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
    })).unwrap_err();
    assert!(
        error.starts_with("B08_AUDIT_INCOMPLETE"),
        "unexpected error: {error}"
    );
    let connection = state.manager.open(project_id, "B08 evidence", NOW).unwrap();
    assert_eq!(connection.query_row::<i64, _, _>(
        "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='FinalChapter' AND artifact_status='COMMITTED'",
        [project_id], |row| row.get(0)
    ).unwrap(), 0);
    assert_eq!(connection.query_row::<i64, _, _>(
        "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1 AND fact_id='fact:actual:door-open'",
        [project_id], |row| row.get(0)
    ).unwrap(), 0);
}
