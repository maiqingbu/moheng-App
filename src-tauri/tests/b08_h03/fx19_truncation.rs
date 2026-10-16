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

/// Two values of the single-valued `location` for a new subject in one
/// chapter; `second_quote` decides whether the evidence overlaps the first.
struct InChapterLocationGateway {
    second_quote: &'static str,
}

impl ProductionModelGateway for InChapterLocationGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            let candidate = |fact: &str, place: &str, quote: &str| {
                json!({"factId":fact,"subjectId":"entity:fx19:guard","entityId":"entity:fx19:guard",
                    "canonicalName":"守卫","entityType":"CHARACTER","predicate":"location",
                    "objectJson":{"place":place},"authorityLevel":"B",
                    "evidence":{"start":0,"end":1,"quote":quote}})
            };
            // Listed out of reading order on purpose.
            response.output["canonCandidates"] = json!([
                candidate("fact:fx19:hall", "大厅", self.second_quote),
                candidate("fact:fx19:door", "石门", "林舟推开石门"),
            ]);
            response.output["knowledgeCandidates"] = json!([]);
            response.output["auditCompleteness"] = json!("COMPLETE_WITH_DELTA");
        }
        Ok(response)
    }
}

fn produce_in_chapter(
    second_quote: &'static str,
    project_id: &str,
) -> (tempfile::TempDir, KernelState, Result<Value, String>) {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(InChapterLocationGateway { second_quote }),
    );
    let connection = state.manager.create(project_id, "FX19", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    let result = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":format!("cmd:{project_id}"),"idempotencyKey":format!("key:{project_id}"),
            "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","skillsEnabled":false}
        }),
    );
    (temp, state, result)
}

#[test]
fn fx19_two_values_in_one_chapter_replace_each_other_in_reading_order() {
    let project_id = "p-fx19-in-chapter";
    let (_temp, state, result) = produce_in_chapter("没有完成它", project_id);
    result.unwrap();
    let connection = state.manager.open(project_id, "FX19", NOW).unwrap();
    let superseded_by = |fact_id: &str| {
        connection
            .query_row::<Option<String>, _, _>(
                "SELECT superseded_by FROM canon_facts WHERE project_id=?1 AND fact_id=?2",
                [project_id, fact_id],
                |row| row.get(0),
            )
            .unwrap()
    };
    assert_eq!(
        superseded_by("fact:fx19:door").as_deref(),
        Some("fact:fx19:hall")
    );
    assert_eq!(superseded_by("fact:fx19:hall"), None);
}

#[test]
fn fx19_two_values_with_overlapping_evidence_are_still_a_conflict() {
    let (_temp, _state, result) = produce_in_chapter("推开石门，实际", "p-fx19-overlap");
    let error = result.unwrap_err();
    assert!(error.contains("B08_CANON_"), "{error}");
}
