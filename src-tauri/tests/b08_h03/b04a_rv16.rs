use super::*;

struct MisalignedEvidenceGateway;
impl ProductionModelGateway for MisalignedEvidenceGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            let mut misaligned = response.output["canonCandidates"][0].clone();
            misaligned["factId"] = json!("fact:actual:misaligned");
            misaligned["evidence"]["quote"] = json!("错误引用");
            response.output["canonCandidates"]
                .as_array_mut()
                .unwrap()
                .push(misaligned);
        }
        Ok(response)
    }
}
/// Owner decision (2026-10-18): a candidate whose quote is still not in the
/// body after the audit rerun is dropped and recorded; it never becomes
/// canon, and the chapter commits without it.
#[test]
fn b08_misaligned_exact_evidence_never_becomes_canon() {
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
    let result = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:b08:misaligned-evidence",
        "idempotencyKey":"key:b08:misaligned-evidence","projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
    })).unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let connection = state.manager.open(project_id, "B08 evidence", NOW).unwrap();
    assert!(connection.query_row::<String, _, _>(
        "SELECT json_extract(p.payload_json,'$.audit.normalization.droppedUnlocatable') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ReviewReport'",
        [project_id], |row| row.get(0)
    ).unwrap().contains("错误引用"));
    assert_eq!(connection.query_row::<i64, _, _>(
        "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1 AND fact_id='fact:actual:misaligned'",
        [project_id], |row| row.get(0)
    ).unwrap(), 0);
}
