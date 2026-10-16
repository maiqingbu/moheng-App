use super::*;

/// The first AUDIT_EXTRACT reply is cut off mid-JSON (as the provider did on
/// 2026-10-17); every later reply is complete.
#[derive(Default)]
struct TruncatedOnceAuditGateway {
    audits: AtomicUsize,
}

impl ProductionModelGateway for TruncatedOnceAuditGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        if capability == "AUDIT_EXTRACT" && self.audits.fetch_add(1, Ordering::SeqCst) == 0 {
            return Ok(ProductionModelResponse {
                output: Value::Null,
                raw_response: Some(
                    json!({"choices":[{"message":{"content":"{\"qualityFindings\":[],\"canonCandidates\":[{\"factId\":\"fact:cut"},"finish_reason":"stop"}]})
                        .to_string(),
                ),
                usage: ProductionModelResponse::deterministic(Value::Null).usage,
            });
        }
        RecordingGateway.invoke(capability, input)
    }
}

#[test]
fn fx19_reply_cut_off_mid_json_is_resent_once_and_the_chapter_commits() {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(TruncatedOnceAuditGateway::default());
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let project_id = "p-fx19-truncated-once";
    let connection = state.manager.create(project_id, "FX19", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:fx19","idempotencyKey":"key:fx19",
        "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","skillsEnabled":false}
    }))
    .unwrap();
    assert_eq!(gateway.audits.load(Ordering::SeqCst), 2);
    let connection = state.manager.open(project_id, "FX19", NOW).unwrap();
    let statuses = connection
        .prepare("SELECT status FROM model_runs WHERE project_id=?1 AND capability='AUDIT_EXTRACT' ORDER BY attempt_no")
        .unwrap()
        .query_map([project_id], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(statuses.len(), 2);
    assert_ne!(statuses[0], "COMPLETED");
    assert_eq!(statuses[1], "COMPLETED");
}
