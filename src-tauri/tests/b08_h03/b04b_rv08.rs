use super::*;
use ai_novel_production_system_core::commit::CommitService;
use ai_novel_production_system_core::generated::types::{
    ArtifactEnvelope, ArtifactRef, CommitBundle,
};
use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
use sha2::Digest;

fn gate_policy_update_request(
    project_id: &str,
    action: &str,
    payload: Value,
    expected_revision: i64,
    suffix: &str,
) -> Value {
    let mut request_payload = payload.as_object().cloned().unwrap_or_default();
    request_payload.insert("action".into(), json!(action));
    json!({
        "type":"gate_policy_update",
        "commandId":format!("cmd:rv08:{project_id}:{suffix}"),
        "idempotencyKey":format!("key:rv08:{project_id}:{suffix}"),
        "projectId":project_id,
        "expectedRevision":expected_revision,
        "requestedAt":NOW,
        "payload":request_payload
    })
}

fn long_form_production_request(
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    expected_revision: i64,
) -> Value {
    json!({
        "type":"production_start",
        "commandId":format!("cmd:rv08:production:{project_id}"),
        "idempotencyKey":format!("key:rv08:production:{project_id}"),
        "projectId":project_id,
        "expectedRevision":expected_revision,
        "requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou"],"references":[],"skillsEnabled":false}
    })
}

fn current_gate_policy(connection: &Connection, project_id: &str) -> (ArtifactRef, Value) {
    let reference =
        NarrativeRepository::canonical_project_head_ref(connection, project_id, "GatePolicy")
            .unwrap()
            .unwrap();
    let payload = NarrativeRepository::artifact_payload_by_ref(connection, project_id, &reference)
        .unwrap()
        .unwrap();
    (reference, payload)
}

struct SingleHorizonCountingGateway {
    capabilities: Arc<Mutex<Vec<String>>>,
}

struct CustomSinglePredicateConflictGateway {
    capabilities: Arc<Mutex<Vec<String>>>,
}

impl ProductionModelGateway for CustomSinglePredicateConflictGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        self.capabilities
            .lock()
            .unwrap()
            .push(capability.to_owned());
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "PLAN_STRATEGY" {
            let capacity = input
                .get("horizonPreviewCapacity")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .clamp(0, 8) as usize;
            let expected_length = if capacity >= 3 { 3 } else { capacity };
            response.output["horizonPreview"] = json!(response.output["horizonPreview"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .take(expected_length)
                .collect::<Vec<_>>());
        }
        if capability == "AUDIT_EXTRACT" {
            response.output["canonCandidates"] = json!([{
                "factId":"fact:actual:custom-status-conflict",
                "subjectId":"entity:actual:linzhou",
                "predicate":"custom_status",
                "objectJson":{"value":"false"},
                "authorityLevel":"C",
                "eventType":"ACTION",
                "evidence":{"start":0,"end":6,"quote":"林舟推开石门"}
            }]);
        }
        Ok(response)
    }
}

impl ProductionModelGateway for SingleHorizonCountingGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        self.capabilities
            .lock()
            .unwrap()
            .push(capability.to_owned());
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "PLAN_STRATEGY" {
            let capacity = input
                .get("horizonPreviewCapacity")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .clamp(0, 8) as usize;
            let expected_length = if capacity >= 3 { 3 } else { capacity };
            if let Some(horizon) = response
                .output
                .get_mut("horizonPreview")
                .and_then(Value::as_array_mut)
            {
                horizon.truncate(expected_length);
            }
        }
        Ok(response)
    }
}

fn project_write_snapshot(connection: &Connection, project_id: &str) -> Value {
    json!({
        "revision":NarrativeRepository::project_revision(connection, project_id).unwrap(),
        "artifacts":connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM artifacts WHERE project_id=?1",[project_id],|row|row.get(0)).unwrap(),
        "heads":connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM canonical_heads WHERE project_id=?1",[project_id],|row|row.get(0)).unwrap(),
        "decisions":connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM decisions WHERE project_id=?1",[project_id],|row|row.get(0)).unwrap(),
        "receipts":connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM commit_receipts WHERE project_id=?1",[project_id],|row|row.get(0)).unwrap(),
        "domainReceipts":connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM domain_receipts WHERE project_id=?1",[project_id],|row|row.get(0)).unwrap()
    })
}

fn commit_legacy_b11_policy(connection: &Connection, project_id: &str) -> ArtifactRef {
    let (reference, mut payload) = current_gate_policy(connection, project_id);
    payload["policyId"] = json!("b11-opening");
    let hash = sha256_canonical(&payload).unwrap();
    let version = reference.version + 1;
    let command_id = "cmd:rv08:seed-legacy-b11";
    let artifact: ArtifactEnvelope = serde_json::from_value(json!({
        "artifactId":reference.artifact_id,
        "projectId":project_id,
        "artifactType":"GatePolicy",
        "scope":{"level":"PROJECT"},
        "version":version,
        "schemaVersion":"m0a.artifact.v1",
        "artifactStatus":"CANDIDATE",
        "semanticLifecycle":"MATERIALIZED",
        "payload":payload,
        "payloadHash":hash,
        "dependencyRefs":[],
        "provenance":{"createdBy":"RV08::LegacyFixture","commandId":command_id},
        "createdAt":NOW
    }))
    .unwrap();
    let revision = NarrativeRepository::project_revision(connection, project_id).unwrap();
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":format!("commit:{command_id}"),
        "projectId":project_id,
        "commandId":command_id,
        "idempotencyKey":"key:rv08:seed-legacy-b11",
        "expectedProjectRevision":revision,
        "contractVersion":"m0a.v1",
        "createdAt":NOW,
        "artifacts":[artifact],
        "headUpdates":[{"artifactType":"GatePolicy","scope":{"level":"PROJECT"},"artifactId":reference.artifact_id,"version":version,"payloadHash":hash}],
        "receiptDraft":{"domain":"RV08_LEGACY_FIXTURE","payload":{}}
    })).unwrap();
    CommitService::commit(connection, &bundle, NOW).unwrap();
    ArtifactRef {
        artifact_id: reference.artifact_id,
        version,
        payload_hash: hash,
    }
}

#[test]
fn b08_rv08_new_project_commits_version_one_of_the_checksummed_default_policy() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let result = execute_command_impl(
        &state,
        json!({
            "type":"project_create_from_idea",
            "commandId":"cmd:rv08:new-project-default-policy",
            "idempotencyKey":"key:rv08:new-project-default-policy",
            "requestedAt":NOW,
            "payload":{"title":"RV-08 new project"}
        }),
    )
    .unwrap();
    let project_id = result["projectId"].as_str().unwrap();
    let connection = state
        .manager
        .open(project_id, "RV-08 default policy", NOW)
        .unwrap();
    let (reference, payload) = current_gate_policy(&connection, project_id);
    assert_eq!(payload["policyId"], "gate-policy-default.r2");
    let policy_bytes =
        std::fs::read("../contracts/resources/gate-policy-default.r2/policy.json").unwrap();
    let manifest: Value = serde_json::from_slice(
        &std::fs::read("../contracts/resources/gate-policy-default.r2/resource-manifest.json")
            .unwrap(),
    )
    .unwrap();
    let raw_digest = hex::encode(sha2::Sha256::digest(&policy_bytes));
    assert_eq!(reference.version, 1);
    assert_eq!(payload["resourceRef"]["version"], "gate-policy-default.r2");
    assert_eq!(
        payload["resourceRef"]["sha256"],
        manifest["files"]["policy.json"]
    );
    assert_eq!(
        raw_digest,
        manifest["files"]["policy.json"].as_str().unwrap()
    );
    assert_eq!(
        payload["canonSemantics"]["rules"].as_array().unwrap().len(),
        15
    );
    let policy =
        ai_novel_production_system_core::canon_semantics::CanonSemanticPolicy::from_gate_policy(
            &payload,
        )
        .unwrap();
    let current =
        ai_novel_production_system_core::repositories::narrative::SemanticCanonFactRecord {
            fact_id: "fact:current".into(),
            object_json: json!({"value":true}),
            valid_from_chapter_id: None,
            valid_to_chapter_id: None,
            story_time_from: None,
            story_time_to: None,
            valid_from_chapter_no: Some(1),
            valid_to_chapter_no: Some(4),
        };
    let candidate = json!({"value":false});
    assert_eq!(
        policy.evaluate(
            "alive",
            &current,
            ai_novel_production_system_core::canon_semantics::CandidateSemanticInput {
                object_json: &candidate,
                story_time: None,
                valid_from_chapter_no: Some(2),
                valid_to_chapter_no: Some(2),
            },
        ),
        ai_novel_production_system_core::canon_semantics::SemanticOutcome::Conflict(
            "CONFLICT_SINGLE"
        )
    );
    let current_item =
        ai_novel_production_system_core::repositories::narrative::SemanticCanonFactRecord {
            object_json: json!({"item":"sword"}),
            ..current.clone()
        };
    let map = json!({"item":"map"});
    assert_eq!(
        policy.evaluate(
            "owns_item",
            &current_item,
            ai_novel_production_system_core::canon_semantics::CandidateSemanticInput {
                object_json: &map,
                story_time: None,
                valid_from_chapter_no: Some(2),
                valid_to_chapter_no: Some(2),
            },
        ),
        ai_novel_production_system_core::canon_semantics::SemanticOutcome::NoConflict(
            "NO_CONFLICT_MULTI"
        )
    );
    let loadout_policy =
        ai_novel_production_system_core::canon_semantics::CanonSemanticPolicy::from_gate_policy(
            &json!({
                "canonSemantics":{
                    "policyVersion":"rv08-test-loadout.r1",
                    "rules":[{"predicate":"loadout","cardinality":"MULTI","scope":{"kind":"FIXED","value":"PROJECT"},"mutualExclusions":[{"left":{"kind":"A"},"right":{"kind":"B"}}]}]
                }
            }),
        )
        .unwrap();
    let loadout_a = json!({"kind":"A"});
    let loadout_b = json!({"kind":"B"});
    let current_loadout =
        ai_novel_production_system_core::repositories::narrative::SemanticCanonFactRecord {
            object_json: loadout_a,
            ..current
        };
    assert_eq!(
        loadout_policy.evaluate(
            "loadout",
            &current_loadout,
            ai_novel_production_system_core::canon_semantics::CandidateSemanticInput {
                object_json: &loadout_b,
                story_time: None,
                valid_from_chapter_no: Some(2),
                valid_to_chapter_no: Some(2),
            },
        ),
        ai_novel_production_system_core::canon_semantics::SemanticOutcome::Conflict(
            "CONFLICT_MUTUAL_EXCLUSION"
        )
    );
}

#[test]
fn b08_rv34_existing_r1_policy_is_preserved_until_explicit_r2_adoption() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let result = execute_command_impl(
        &state,
        json!({
            "type":"project_create_from_idea",
            "commandId":"cmd:rv34:r1-project",
            "idempotencyKey":"key:rv34:r1-project",
            "requestedAt":NOW,
            "payload":{"title":"RV-34 historical r1 policy"}
        }),
    )
    .unwrap();
    let project_id = result["projectId"].as_str().unwrap();
    let connection = state
        .manager
        .open(project_id, "RV-34 historical r1", NOW)
        .unwrap();
    let (old_ref, old_payload) = current_gate_policy(&connection, project_id);
    assert_eq!(old_payload["policyId"], "gate-policy-default.r1");
    drop(connection);

    query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let connection = state
        .manager
        .open(project_id, "RV-34 unchanged historical r1", NOW)
        .unwrap();
    assert_eq!(current_gate_policy(&connection, project_id).0, old_ref);
    drop(connection);

    let expected_revision = {
        let connection = state.manager.open(project_id, "", NOW).unwrap();
        NarrativeRepository::project_revision(&connection, project_id).unwrap()
    };
    execute_command_impl(
        &state,
        gate_policy_update_request(
            project_id,
            "ADOPT_DEFAULT",
            json!({}),
            expected_revision,
            "adopt-r2",
        ),
    )
    .expect("an existing r1 project can explicitly adopt r2");

    let connection = state
        .manager
        .open(project_id, "RV-34 adopted r2", NOW)
        .unwrap();
    let (current_ref, current_payload) = current_gate_policy(&connection, project_id);
    assert_eq!(current_payload["policyId"], "gate-policy-default.r2");
    assert_eq!(current_payload["previousPolicyRef"]["artifactId"], old_ref.artifact_id);
    assert_eq!(
        NarrativeRepository::artifact_payload_by_ref(&connection, project_id, &old_ref)
            .unwrap()
            .unwrap()["policyId"],
        "gate-policy-default.r1"
    );
    assert_eq!(current_ref.version, old_ref.version + 1);
}

#[test]
fn b08_rv08_user_rule_updates_are_versioned_and_malformed_rules_write_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-rv08-rule-versions";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let connection = state
        .manager
        .create(project_id, "RV-08 rule versions", NOW)
        .unwrap();
    seed_chapter_and_gate(&connection, project_id);
    let (old_ref, _) = current_gate_policy(&connection, project_id);
    drop(connection);

    let added = execute_command_impl(
        &state,
        gate_policy_update_request(
            project_id,
            "UPSERT_RULE",
            json!({"rule":{"predicate":"custom_status","cardinality":"SINGLE","scope":{"kind":"FIXED","value":"PROJECT"},"timeBasis":"VALIDITY_CHAPTER"}}),
            0,
            "add-rule",
        ),
    );
    assert!(
        matches!(added.as_ref(), Ok(result) if result["finalStatus"] == "committed"),
        "adding a rule should create a policy version; result={added:?}"
    );
    let connection = state
        .manager
        .open(project_id, "RV-08 rule versions", NOW)
        .unwrap();
    let (added_ref, added_payload) = current_gate_policy(&connection, project_id);
    assert_eq!(added_ref.version, old_ref.version + 1);
    assert_eq!(
        NarrativeRepository::artifact_payload_by_ref(&connection, project_id, &old_ref)
            .unwrap()
            .unwrap()["canonSemantics"]["rules"]
            .as_array()
            .unwrap()
            .clone(),
        current_gate_policy(&connection, project_id).1["canonSemantics"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|rule| rule["predicate"] != "custom_status")
            .cloned()
            .collect::<Vec<_>>()
    );
    assert!(added_payload["canonSemantics"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| rule["predicate"] == "custom_status"));
    drop(connection);

    let changed = execute_command_impl(&state, gate_policy_update_request(
        project_id,
        "UPSERT_RULE",
        json!({"rule":{"predicate":"custom_status","cardinality":"MULTI","scope":{"kind":"FIXED","value":"PROJECT"}}}),
        1,
        "modify-rule",
    )).unwrap();
    assert_eq!(changed["finalStatus"], "committed");
    let disabled = execute_command_impl(
        &state,
        gate_policy_update_request(
            project_id,
            "DISABLE_RULE",
            json!({"predicate":"custom_status"}),
            2,
            "disable-rule",
        ),
    )
    .unwrap();
    assert_eq!(disabled["finalStatus"], "committed");

    let connection = state
        .manager
        .open(project_id, "RV-08 malformed rule", NOW)
        .unwrap();
    let before = project_write_snapshot(&connection, project_id);
    let revision = before["revision"].as_i64().unwrap();
    drop(connection);
    let malformed = execute_command_impl(
        &state,
        gate_policy_update_request(
            project_id,
            "UPSERT_RULE",
            json!({"rule":{"predicate":"broken","cardinality":"UNKNOWN","scope":{"kind":"FIXED","value":"PROJECT"}}}),
            revision,
            "malformed-rule",
        ),
    );
    let connection = state
        .manager
        .open(project_id, "RV-08 malformed rule", NOW)
        .unwrap();
    assert!(
        matches!(malformed.as_ref(), Err(error) if error == "GATE_POLICY_RULE_INVALID"),
        "invalid rules must fail explicitly; result={malformed:?}"
    );
    assert_eq!(project_write_snapshot(&connection, project_id), before);
}

#[test]
fn b08_rv08_legacy_b11_policy_blocks_long_form_until_explicit_default_adoption() {
    let temp = tempfile::tempdir().unwrap();
    let capabilities = Arc::new(Mutex::new(Vec::new()));
    let gateway = SingleHorizonCountingGateway {
        capabilities: capabilities.clone(),
    };
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), Arc::new(gateway));
    let project_id = "p-b08-rv08-legacy-policy";
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state
        .manager
        .open(project_id, "RV-08 legacy policy", NOW)
        .unwrap();
    let legacy_ref = commit_legacy_b11_policy(&connection, project_id);
    let before_blocked_entries = project_write_snapshot(&connection, project_id);
    drop(connection);

    let planning = execute_command_impl(
        &state,
        json!({
            "type":"planning_compile_chapter_plan","commandId":"cmd:rv08:legacy-planning","idempotencyKey":"key:rv08:legacy-planning",
            "projectId":project_id,"expectedRevision":1,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"entityIds":["entity:actual:linzhou"],"references":[]}
        }),
    );
    assert!(
        matches!(planning.as_ref(), Err(error) if error == "GATE_POLICY_UPGRADE_REQUIRED"),
        "legacy policy should block planning; result={planning:?}"
    );
    assert!(capabilities.lock().unwrap().is_empty());
    let production = execute_command_impl(
        &state,
        long_form_production_request(project_id, &chapter_id, &volume_id, 1),
    );
    assert!(
        matches!(production.as_ref(), Err(error) if error == "GATE_POLICY_UPGRADE_REQUIRED"),
        "legacy policy should block production; result={production:?}"
    );
    assert!(
        capabilities.lock().unwrap().is_empty(),
        "neither blocked entry may call a model"
    );
    let connection = state
        .manager
        .open(project_id, "RV-08 blocked entries", NOW)
        .unwrap();
    assert_eq!(
        project_write_snapshot(&connection, project_id),
        before_blocked_entries
    );
    drop(connection);

    let adopted = execute_command_impl(
        &state,
        gate_policy_update_request(project_id, "ADOPT_DEFAULT", json!({}), 1, "adopt-default"),
    )
    .unwrap();
    assert_eq!(adopted["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "RV-08 adopted policy", NOW)
        .unwrap();
    let (current_ref, current_payload) = current_gate_policy(&connection, project_id);
    assert_eq!(current_ref.version, 3);
    assert_eq!(current_payload["policyId"], "gate-policy-default.r2");
    assert_eq!(
        NarrativeRepository::artifact_payload_by_ref(&connection, project_id, &legacy_ref)
            .unwrap()
            .unwrap()["policyId"],
        "b11-opening"
    );
    let revision = NarrativeRepository::project_revision(&connection, project_id).unwrap();
    drop(connection);

    let recompiled = execute_command_impl(
        &state,
        json!({
            "type":"planning_compile_chapter_plan","commandId":"cmd:rv08:legacy-planning-after-adoption","idempotencyKey":"key:rv08:legacy-planning-after-adoption",
            "projectId":project_id,"expectedRevision":revision,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"entityIds":["entity:actual:linzhou"],"references":[]}
        }),
    );
    assert!(
        matches!(recompiled.as_ref(), Ok(result) if result["finalStatus"] == "committed"),
        "explicit adoption should allow a fresh long-form plan; result={recompiled:?}"
    );
    assert!(capabilities
        .lock()
        .unwrap()
        .contains(&"PLAN_STRATEGY".to_owned()));
}

#[test]
fn b08_rv08_user_added_single_predicate_blocks_conflicting_actual_canon() {
    let temp = tempfile::tempdir().unwrap();
    let capabilities = Arc::new(Mutex::new(Vec::new()));
    let gateway = CustomSinglePredicateConflictGateway {
        capabilities: capabilities.clone(),
    };
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), Arc::new(gateway));
    let project_id = "p-b08-rv08-custom-single-conflict";
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state
        .manager
        .open(project_id, "RV-08 custom predicate", NOW)
        .unwrap();
    connection.execute(
        "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,valid_from_chapter_id,valid_to_chapter_id,authority_level,created_at,updated_at) VALUES ('fact:actual:custom-status-seed',?1,'entity:actual:linzhou','custom_status','{\"value\":\"true\"}',?2,?2,'A',?3,?3)",
        libsql_rusqlite::params![project_id, chapter_id, NOW],
    ).unwrap();
    drop(connection);

    let updated = execute_command_impl(
        &state,
        gate_policy_update_request(
            project_id,
            "UPSERT_RULE",
            json!({"rule":{"predicate":"custom_status","cardinality":"SINGLE","scope":{"kind":"FIXED","value":"PROJECT"},"timeBasis":"VALIDITY_CHAPTER"}}),
            0,
            "add-custom-single",
        ),
    ).unwrap();
    assert_eq!(updated["finalStatus"], "committed");
    let connection = state
        .manager
        .open(project_id, "RV-08 custom predicate", NOW)
        .unwrap();
    let (policy_ref, policy_payload) = current_gate_policy(&connection, project_id);
    assert_eq!(policy_ref.version, 2);
    assert!(policy_payload["canonSemantics"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| rule["predicate"] == "custom_status" && rule["cardinality"] == "SINGLE"));
    let revision = NarrativeRepository::project_revision(&connection, project_id).unwrap();
    drop(connection);

    let planning = execute_command_impl(
        &state,
        json!({
            "type":"planning_compile_chapter_plan",
            "commandId":"cmd:rv08:custom-single-plan",
            "idempotencyKey":"key:rv08:custom-single-plan",
            "projectId":project_id,
            "expectedRevision":revision,
            "requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"entityIds":["entity:actual:linzhou"],"references":[]}
        }),
    )
    .unwrap();
    assert_eq!(planning["finalStatus"], "committed");
    let production_revision = state.manager.project_revision(project_id).unwrap();
    let production = execute_command_impl(
        &state,
        json!({
            "type":"production_start",
            "commandId":"cmd:rv08:custom-single-production",
            "idempotencyKey":"key:rv08:custom-single-production",
            "projectId":project_id,
            "expectedRevision":production_revision,
            "requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou"],"references":[],"skillsEnabled":false}
        }),
    );
    assert!(
        matches!(production.as_ref(), Err(error) if error == "B08_CANON_CONFLICT_CHANGE_REQUEST_REQUIRED"),
        "the user-added SINGLE predicate must block a conflicting actual fact; result={production:?}"
    );
    assert_eq!(
        capabilities.lock().unwrap().as_slice(),
        ["PLAN_STRATEGY", "RENDER_CHAPTER", "AUDIT_EXTRACT"]
    );
    let connection = state
        .manager
        .open(project_id, "RV-08 conflict assertion", NOW)
        .unwrap();
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='FinalChapter'",[project_id],|row|row.get(0)).unwrap(),0);
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM domain_receipts WHERE project_id=?1 AND domain='B08_CHAPTER_PRODUCTION'",[project_id],|row|row.get(0)).unwrap(),0);
}

#[test]
fn b08_rv08_policy_rule_that_conflicts_with_existing_canon_is_rejected_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-rv08-policy-conflict";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let (chapter_id, _, _, _) = seed_b08_long_form_project(&state, project_id);
    let connection = state
        .manager
        .open(project_id, "RV-08 rule conflict", NOW)
        .unwrap();
    connection.execute(
        "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,valid_from_chapter_id,valid_to_chapter_id,authority_level,created_at,updated_at) VALUES ('fact:actual:custom-status-a',?1,'entity:actual:linzhou','custom_status','{\"value\":\"true\"}',?2,?2,'A',?3,?3),('fact:actual:custom-status-b',?1,'entity:actual:linzhou','custom_status','{\"value\":\"false\"}',?2,?2,'A',?3,?3)",
        libsql_rusqlite::params![project_id, chapter_id, NOW],
    ).unwrap();
    let before = project_write_snapshot(&connection, project_id);
    drop(connection);

    let update = execute_command_impl(
        &state,
        gate_policy_update_request(
            project_id,
            "UPSERT_RULE",
            json!({"rule":{"predicate":"custom_status","cardinality":"SINGLE","scope":{"kind":"FIXED","value":"PROJECT"},"timeBasis":"VALIDITY_CHAPTER"}}),
            0,
            "reject-new-conflict",
        ),
    );
    let connection = state
        .manager
        .open(project_id, "RV-08 rule conflict assertion", NOW)
        .unwrap();
    assert!(
        matches!(update.as_ref(), Err(error) if error == "GATE_POLICY_CANON_CONFLICT"),
        "a new rule must not make existing facts conflict silently; result={update:?}"
    );
    assert_eq!(project_write_snapshot(&connection, project_id), before);
}

#[test]
fn b08_rv08_gate_policy_version_change_stales_the_existing_chapter_plan_before_model_calls() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-b08-rv08-policy-stales-plan";
    let capabilities = Arc::new(Mutex::new(Vec::new()));
    let gateway = CountingCapabilitiesGateway {
        capabilities: capabilities.clone(),
    };
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), Arc::new(gateway));
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);

    let updated = execute_command_impl(
        &state,
        gate_policy_update_request(
            project_id,
            "UPSERT_RULE",
            json!({"rule":{"predicate":"new_multi_fact","cardinality":"MULTI","scope":{"kind":"FIXED","value":"PROJECT"}}}),
            0,
            "policy-stales-plan",
        ),
    ).unwrap();
    assert_eq!(updated["finalStatus"], "committed");
    let production = execute_command_impl(
        &state,
        long_form_production_request(project_id, &chapter_id, &volume_id, 1),
    );
    assert!(
        matches!(production.as_ref(), Err(error) if error.starts_with("PLAN_STALE")),
        "a GatePolicy version change must stale the plan; result={production:?}"
    );
    assert!(capabilities.lock().unwrap().is_empty());
    let connection = state
        .manager
        .open(project_id, "RV-08 stale plan assertion", NOW)
        .unwrap();
    assert_eq!(connection.query_row::<i64,_,_>("SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='FinalChapter'",[project_id],|row|row.get(0)).unwrap(),0);
}
