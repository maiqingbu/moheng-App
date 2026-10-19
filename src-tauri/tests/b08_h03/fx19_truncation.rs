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

/// The first AUDIT_EXTRACT reply is not JSON at all (a garbled reply seen on
/// 2026-10-18); every later reply is complete.
#[derive(Default)]
struct GarbledOnceAuditGateway {
    audits: AtomicUsize,
}

impl ProductionModelGateway for GarbledOnceAuditGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        if capability == "AUDIT_EXTRACT" && self.audits.fetch_add(1, Ordering::SeqCst) == 0 {
            return Ok(ProductionModelResponse {
                output: Value::Null,
                raw_response: Some(
                    json!({"choices":[{"message":{"content":"p\na5873...h，uFP是（，"},"finish_reason":"stop"}]})
                        .to_string(),
                ),
                usage: ProductionModelResponse::deterministic(Value::Null).usage,
            });
        }
        RecordingGateway.invoke(capability, input)
    }
}

#[test]
fn craft_a_reply_that_is_not_json_is_resent_once_and_the_chapter_commits() {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(GarbledOnceAuditGateway::default());
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let project_id = "p-craft-garbled-once";
    let connection = state.manager.create(project_id, "FX19", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:craft:garbled","idempotencyKey":"key:craft:garbled",
        "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","skillsEnabled":false}
    }))
    .unwrap();
    assert_eq!(gateway.audits.load(Ordering::SeqCst), 2);
}

/// The first audit quotes a sentence that is not in the body (as a real
/// audit did on 2026-10-18 by quoting the plan); later audits are correct.
#[derive(Default)]
struct UnlocatableQuoteOnceGateway {
    audit_inputs: std::sync::Mutex<Vec<Value>>,
}

impl ProductionModelGateway for UnlocatableQuoteOnceGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            let mut inputs = self.audit_inputs.lock().unwrap();
            if inputs.is_empty() {
                response.output["canonCandidates"][0]["evidence"]["quote"] =
                    json!("林舟推门而入，计划里的句子");
            }
            inputs.push(input.clone());
        }
        Ok(response)
    }
}

#[test]
fn craft_audit_with_unlocatable_evidence_is_rerun_once_with_the_quotes() {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(UnlocatableQuoteOnceGateway::default());
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let project_id = "p-craft-audit-quotes";
    let connection = state.manager.create(project_id, "FX19", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    let result = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:craft:audit-quotes","idempotencyKey":"key:craft:audit-quotes",
        "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","skillsEnabled":false,"entityIds":["entity:actual:linzhou"]}
    }))
    .unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let inputs = gateway.audit_inputs.lock().unwrap();
    assert_eq!(inputs.len(), 2);
    assert!(inputs[0].get("auditFeedback").is_none());
    assert_eq!(
        inputs[1]["auditFeedback"]["rejectedFor"],
        "B08_AUDIT_INCOMPLETE_EVIDENCE_REBOUND"
    );
    assert_eq!(
        inputs[1]["auditFeedback"]["unlocatableQuotes"],
        json!(["林舟推门而入，计划里的句子"])
    );
}

/// Every audit quotes a sentence that is not in the body.
#[derive(Default)]
struct UnlocatableQuoteAlwaysGateway {
    audits: std::sync::atomic::AtomicUsize,
}

impl ProductionModelGateway for UnlocatableQuoteAlwaysGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            self.audits
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut extra = response.output["canonCandidates"][0].clone();
            extra["factId"] = json!("fact:plan-quote");
            extra["evidence"]["quote"] = json!("林舟推门而入，计划里的句子");
            response.output["canonCandidates"]
                .as_array_mut()
                .unwrap()
                .push(extra);
        }
        Ok(response)
    }
}

#[test]
fn craft_audit_still_unlocatable_after_the_rerun_drops_and_records_the_candidate() {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(UnlocatableQuoteAlwaysGateway::default());
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let project_id = "p-craft-audit-drop";
    let connection = state.manager.create(project_id, "FX19", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    let result = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:craft:audit-drop","idempotencyKey":"key:craft:audit-drop",
        "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","skillsEnabled":false,"entityIds":["entity:actual:linzhou"]}
    }))
    .unwrap();
    assert_eq!(result["finalStatus"], "committed");
    assert_eq!(gateway.audits.load(std::sync::atomic::Ordering::SeqCst), 2);
    let connection = state.manager.open(project_id, "FX19", NOW).unwrap();
    let dropped: String = connection
        .query_row(
            "SELECT json_extract(p.payload_json,'$.audit.normalization.droppedUnlocatable') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.artifact_type='ReviewReport'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(dropped.contains("fact:plan-quote"));
}

/// The audit reports the reveal of a reservation the project never had, as
/// a real audit did on 2026-10-19.
struct InventedReservationGateway;

impl ProductionModelGateway for InventedReservationGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            response.output["revealedReservationRefs"] = json!(["resv:invented:secret"]);
        }
        Ok(response)
    }
}

#[test]
fn craft_audit_reveal_of_an_unknown_reservation_is_dropped_and_recorded() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(InventedReservationGateway),
    );
    let project_id = "p-craft-invented-reservation";
    let connection = state.manager.create(project_id, "FX19", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    let result = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:craft:invented-resv","idempotencyKey":"key:craft:invented-resv",
        "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","skillsEnabled":false,"entityIds":["entity:actual:linzhou"]}
    }))
    .unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let connection = state.manager.open(project_id, "FX19", NOW).unwrap();
    let dropped: String = connection
        .query_row(
            "SELECT json_extract(p.payload_json,'$.audit.normalization.droppedInventedReservations') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.artifact_type='ReviewReport'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(dropped, "[\"resv:invented:secret\"]");
}

/// The audit names a new fact with the id of a fact already in Canon, as a
/// real audit did on 2026-10-19 by copying ids from the authority context.
struct ReusedFactIdGateway;

impl ProductionModelGateway for ReusedFactIdGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            response.output["canonCandidates"][0]["factId"] = json!("fact:actual:seed");
        }
        Ok(response)
    }
}

#[test]
fn craft_a_new_fact_reusing_an_existing_fact_id_gets_its_own_id() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(ReusedFactIdGateway),
    );
    let project_id = "p-craft-reused-fact-id";
    let connection = state.manager.create(project_id, "FX19", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    let result = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:craft:reused-id","idempotencyKey":"key:craft:reused-id",
        "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","skillsEnabled":false,"entityIds":["entity:actual:linzhou"]}
    }))
    .unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let connection = state.manager.open(project_id, "FX19", NOW).unwrap();
    // The committed fact keeps its row; the new fact is stored beside it.
    let seed: (String, String) = connection
        .query_row(
            "SELECT predicate, object_json FROM canon_facts WHERE fact_id='fact:actual:seed'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(seed, ("location".to_string(), "{\"place\":\"stone-door\"}".to_string()));
    let opened: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM canon_facts WHERE predicate='opened' AND fact_id<>'fact:actual:seed'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(opened, 1);
}

/// The first audit states a fact with a predicate the canon semantics policy
/// does not register, as a real audit did on 2026-10-19; later audits keep
/// to registered predicates.
#[derive(Default)]
struct UnregisteredPredicateOnceGateway {
    audit_inputs: std::sync::Mutex<Vec<Value>>,
}

impl ProductionModelGateway for UnregisteredPredicateOnceGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            let mut inputs = self.audit_inputs.lock().unwrap();
            if inputs.is_empty() {
                response.output["canonCandidates"][0]["predicate"] = json!("lost_memory");
            }
            inputs.push(input.clone());
        }
        Ok(response)
    }
}

#[test]
fn craft_audit_with_an_unregistered_predicate_is_rerun_once_with_the_registered_ones() {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(UnregisteredPredicateOnceGateway::default());
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let project_id = "p-craft-unregistered-predicate";
    let connection = state.manager.create(project_id, "FX19", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    let result = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:craft:predicate","idempotencyKey":"key:craft:predicate",
        "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","skillsEnabled":false,"entityIds":["entity:actual:linzhou"]}
    }))
    .unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let inputs = gateway.audit_inputs.lock().unwrap();
    assert_eq!(inputs.len(), 2);
    assert!(inputs[1]["auditFeedback"]["rejectedFor"]
        .as_str()
        .unwrap()
        .starts_with("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS"));
    assert!(inputs[1]["auditFeedback"]["registeredPredicates"]
        .as_array()
        .unwrap()
        .contains(&json!("opened")));
}
