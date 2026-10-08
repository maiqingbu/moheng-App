use super::*;
use ai_novel_production_system_core::commit::CommitService;
use ai_novel_production_system_core::generated::types::{ArtifactEnvelope, CommitBundle};
use ai_novel_production_system_core::project_db::connection::open_connection;
use ai_novel_production_system_core::project_db::ProjectCommandSpec;
use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy)]
enum MutationKind {
    UnrelatedProjectRename,
    RelevantBookPlanRevision,
}

struct CommitAtGateBoundaryGateway {
    database: Arc<Mutex<Option<PathBuf>>>,
    mutation: MutationKind,
    mutated: AtomicBool,
    capabilities: Arc<Mutex<Vec<String>>>,
}

type GateBoundaryRun = (
    tempfile::TempDir,
    Result<Value, String>,
    Arc<Mutex<Vec<String>>>,
    KernelState,
);

impl ProductionModelGateway for CommitAtGateBoundaryGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        self.capabilities
            .lock()
            .unwrap()
            .push(capability.to_owned());
        RecordingGateway.invoke(capability, input)
    }

    fn fault_injection(&self) -> Option<&str> {
        if self.mutated.swap(true, Ordering::SeqCst) {
            return None;
        }
        let database = self.database.lock().unwrap().as_ref().unwrap().clone();
        let connection = open_connection(&database, false).unwrap();
        let revision = ai_novel_production_system_core::repositories::narrative::NarrativeRepository::project_revision(
            &connection,
            "p-b08-rv15-boundary",
        )
        .unwrap();
        match self.mutation {
            MutationKind::UnrelatedProjectRename => {
                let digest = sha256_canonical(&json!({"title":"RV-15 unrelated rename"})).unwrap();
                CommitService::commit_project_command(
                    &connection,
                    ProjectCommandSpec {
                        project_id: "p-b08-rv15-boundary",
                        result_project_id: "p-b08-rv15-boundary",
                        command_id: "cmd:rv15:unrelated-rename",
                        idempotency_key: "key:rv15:unrelated-rename",
                        command_type: "project_rename",
                        request_digest: &digest,
                        expected_revision: Some(revision),
                        now: NOW,
                        advance_revision: true,
                    },
                    |connection| {
                        connection.execute(
                            "UPDATE project_meta SET title=?1 WHERE project_id=?2",
                            libsql_rusqlite::params![
                                "RV-15 unrelated rename",
                                "p-b08-rv15-boundary"
                            ],
                        )?;
                        Ok(())
                    },
                )
                .unwrap();
            }
            MutationKind::RelevantBookPlanRevision => {
                let book_plan_ref = NarrativeRepository::canonical_project_head_ref(
                    &connection,
                    "p-b08-rv15-boundary",
                    "BookPlan",
                )
                .unwrap()
                .unwrap();
                let mut payload = NarrativeRepository::artifact_payload_by_ref(
                    &connection,
                    "p-b08-rv15-boundary",
                    &book_plan_ref,
                )
                .unwrap()
                .unwrap();
                payload["storyPromise"] = json!("提交边界期间改变了本章相关合同");
                let payload_hash = sha256_canonical(&payload).unwrap();
                let version = book_plan_ref.version + 1;
                let artifact: ArtifactEnvelope = serde_json::from_value(json!({
                    "artifactId":book_plan_ref.artifact_id,
                    "projectId":"p-b08-rv15-boundary",
                    "artifactType":"BookPlan",
                    "scope":{"level":"PROJECT"},
                    "version":version,
                    "schemaVersion":"m0a.artifact.v1",
                    "artifactStatus":"CANDIDATE",
                    "semanticLifecycle":"MATERIALIZED",
                    "payload":payload,
                    "payloadHash":payload_hash,
                    "dependencyRefs":[],
                    "provenance":{"createdBy":"B07::PlanningOwner","commandId":"cmd:rv15:book-plan"},
                    "createdAt":NOW
                }))
                .unwrap();
                let bundle: CommitBundle = serde_json::from_value(json!({
                    "commitId":"commit:rv15:book-plan",
                    "projectId":"p-b08-rv15-boundary",
                    "commandId":"cmd:rv15:book-plan",
                    "idempotencyKey":"key:rv15:book-plan",
                    "expectedProjectRevision":revision,
                    "contractVersion":"m0a.v1",
                    "createdAt":NOW,
                    "artifacts":[artifact],
                    "headUpdates":[{"artifactType":"BookPlan","scope":{"level":"PROJECT"},"artifactId":book_plan_ref.artifact_id,"version":version,"payloadHash":payload_hash}],
                    "receiptDraft":{"domain":"RV15_TEST_BOOK_PLAN","payload":{}}
                }))
                .unwrap();
                CommitService::commit(&connection, &bundle, NOW).unwrap();
            }
        }
        None
    }
}

fn production_request(project_id: &str, chapter_id: &str, volume_id: &str) -> Value {
    json!({
        "type":"production_start",
        "commandId":format!("cmd:rv15:production:{project_id}"),
        "idempotencyKey":format!("key:rv15:production:{project_id}"),
        "projectId":project_id,
        "expectedRevision":0,
        "requestedAt":NOW,
        "payload":{
            "chapterId":chapter_id,
            "volumeId":volume_id,
            "executionMode":"LONG_FORM",
            "entityIds":["entity:actual:linzhou"],
            "references":[],
            "skillsEnabled":false
        }
    })
}

fn run_production_at_gate_boundary(mutation: MutationKind) -> GateBoundaryRun {
    let temp = tempfile::tempdir().unwrap();
    let database_slot = Arc::new(Mutex::new(None));
    let capabilities = Arc::new(Mutex::new(Vec::new()));
    let gateway = Arc::new(CommitAtGateBoundaryGateway {
        database: database_slot.clone(),
        mutation,
        mutated: AtomicBool::new(false),
        capabilities: capabilities.clone(),
    });
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway);
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, "p-b08-rv15-boundary");
    *database_slot.lock().unwrap() = Some(
        AppPaths::new(temp.path())
            .project_dir("p-b08-rv15-boundary")
            .unwrap()
            .join("project.db"),
    );
    let request = production_request("p-b08-rv15-boundary", &chapter_id, &volume_id);
    (
        temp,
        execute_command_impl(&state, request),
        capabilities,
        state,
    )
}

#[test]
fn b08_rv15_unrelated_revision_change_rebases_bundle_without_repeating_model_runs() {
    let (_temp, result, capabilities, state) =
        run_production_at_gate_boundary(MutationKind::UnrelatedProjectRename);
    let result = result.expect("an unrelated revision change should not block the chapter");
    assert_eq!(result["finalStatus"], "committed");
    assert_eq!(
        capabilities.lock().unwrap().as_slice(),
        ["RENDER_CHAPTER", "AUDIT_EXTRACT"]
    );
    assert_eq!(result["receipt"]["beforeRevision"], 1);
    let connection = state
        .manager
        .open("p-b08-rv15-boundary", "RV-15 assertion", NOW)
        .unwrap();
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_runs WHERE project_id='p-b08-rv15-boundary'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        2
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM artifacts WHERE project_id='p-b08-rv15-boundary' AND artifact_type='FinalChapter'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn b08_rv15_relevant_revision_change_is_plan_stale_without_final_write_or_model_retry() {
    let (_temp, result, capabilities, state) =
        run_production_at_gate_boundary(MutationKind::RelevantBookPlanRevision);
    assert!(
        matches!(result.as_ref(), Err(error) if error.starts_with("PLAN_STALE")),
        "a relevant authority change should stale the plan; result={result:?}"
    );
    assert_eq!(
        capabilities.lock().unwrap().as_slice(),
        ["RENDER_CHAPTER", "AUDIT_EXTRACT"]
    );
    let connection = state
        .manager
        .open("p-b08-rv15-boundary", "RV-15 assertion", NOW)
        .unwrap();
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_runs WHERE project_id='p-b08-rv15-boundary'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        2
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM artifacts WHERE project_id='p-b08-rv15-boundary' AND artifact_type='FinalChapter'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM domain_receipts WHERE project_id='p-b08-rv15-boundary' AND domain='B08_CHAPTER_PRODUCTION'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        0
    );
}
