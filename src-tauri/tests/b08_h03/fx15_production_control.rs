use super::*;
use std::sync::mpsc;
use std::sync::Barrier;
use std::time::{Duration, Instant};

/// Blocks inside one model capability until released, so pause/cancel can be
/// sent while a chapter production really holds the project writer.
struct BlockingGateway {
    block_on: &'static str,
    entered: Mutex<Option<mpsc::Sender<()>>>,
    release: Mutex<Option<mpsc::Receiver<()>>>,
    calls: Arc<Mutex<Vec<String>>>,
}

struct BlockingState {
    state: KernelState,
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl ProductionModelGateway for BlockingGateway {
    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        self.calls.lock().unwrap().push(capability.to_string());
        if capability == self.block_on {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                entered.send(()).unwrap();
                if let Some(release) = self.release.lock().unwrap().take() {
                    release.recv().unwrap();
                }
            }
        }
        RecordingGateway.invoke(capability, input)
    }
}

fn blocking_state(temp: &tempfile::TempDir, block_on: &'static str) -> BlockingState {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(BlockingGateway {
            block_on,
            entered: Mutex::new(Some(entered_tx)),
            release: Mutex::new(Some(release_rx)),
            calls: calls.clone(),
        }),
    );
    BlockingState {
        state,
        entered: entered_rx,
        release: release_tx,
        calls,
    }
}

fn start_request(
    project_id: &str,
    (chapter_id, volume_id): (&str, &str),
    command: &str,
    retry_of: Option<&str>,
) -> Value {
    let mut payload = json!({"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM","entityIds":["entity:actual:linzhou"],"references":[],"skillsEnabled":false});
    if let Some(job_id) = retry_of {
        payload["retryOfJobId"] = json!(job_id);
    }
    json!({
        "type":"production_start","commandId":format!("cmd:fx15:{command}"),
        "idempotencyKey":format!("key:fx15:{command}"),"projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,"payload":payload
    })
}

fn control_request(project_id: &str, kind: &str, command: &str, job_id: &str) -> Value {
    json!({
        "type":kind,"commandId":format!("cmd:fx15:{command}"),
        "idempotencyKey":format!("key:fx15:{command}"),"projectId":project_id,
        "expectedRevision":0,"requestedAt":NOW,"payload":{"jobId":job_id}
    })
}

fn start_running_job(
    state: &KernelState,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    identity: &str,
    now: &str,
) -> (
    ai_novel_production_system_core::project_db::ProjectHandle,
    String,
) {
    use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};

    let job_id =
        StableIdentityFactory::from_command(StableIdKind::Job, &format!("cmd:fx15:{identity}"))
            .unwrap();
    let handle = state.manager.open(project_id, "", now).unwrap();
    let spec = ai_novel_production_system_core::repositories::runtime::JobSpec {
        job_id: job_id.clone(),
        job_type: "CHAPTER".into(),
        parent_job_id: None,
        book_id: None,
        volume_id: Some(volume_id.into()),
        chapter_id: Some(chapter_id.into()),
        stages: vec!["CONTEXT".into(), "COMMIT".into()],
    };
    ai_novel_production_system_core::repositories::runtime::DurableJobService::start_job(
        &handle, &spec, now,
    )
    .unwrap();
    ai_novel_production_system_core::repositories::runtime::DurableJobService::transition(
        &handle, project_id, &job_id, "RUNNING", now,
    )
    .unwrap();
    (handle, job_id)
}

fn job_status(state: &KernelState, project_id: &str, job_id: &str) -> String {
    state
        .manager
        .with_read_connection(project_id, NOW, |connection| {
            Ok(connection.query_row(
                "SELECT status FROM jobs WHERE project_id=?1 AND job_id=?2",
                libsql_rusqlite::params![project_id, job_id],
                |row| row.get::<_, String>(0),
            )?)
        })
        .unwrap()
}

fn final_chapter_count(state: &KernelState, project_id: &str) -> i64 {
    state
        .manager
        .with_read_connection(project_id, NOW, |connection| {
            Ok(connection.query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='FinalChapter'",
            [project_id],
            |row| row.get(0),
        )?)
        })
        .unwrap()
}

fn has_project_command_receipt(
    state: &KernelState,
    project_id: &str,
    idempotency_key: &str,
) -> bool {
    state
        .manager
        .find_project_command_receipt(project_id, idempotency_key)
        .unwrap()
        .is_some()
}

/// Runs production on a scoped thread, sends `control` once the gateway is
/// blocked, then releases it. Returns (control response, its latency, production response).
fn run_with_control(
    state: &KernelState,
    project_id: &str,
    chapter: (&str, &str),
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
    control: &str,
) -> (Value, Duration, Value) {
    thread::scope(|scope| {
        let production = scope.spawn(|| {
            execute_command_impl(state, start_request(project_id, chapter, "first", None))
        });
        if entered.recv_timeout(Duration::from_secs(60)).is_err() {
            panic!(
                "production never reached the blocked step: {:?}",
                production.join().unwrap()
            );
        }
        let started = Instant::now();
        let job_id =
            ai_novel_production_system_core::stable_id::StableIdentityFactory::from_command(
                ai_novel_production_system_core::stable_id::StableIdKind::Job,
                "cmd:fx15:first",
            )
            .unwrap();
        let response = execute_command_impl(
            state,
            control_request(project_id, control, "single", &job_id),
        );
        let latency = started.elapsed();
        // Always unblock production before asserting, so a failure cannot hang.
        release.send(()).unwrap();
        let produced = production.join().unwrap();
        (
            response.unwrap_or_else(|error| panic!("{control} failed: {error}")),
            latency,
            produced.unwrap_or_else(|error| panic!("production failed: {error}")),
        )
    })
}

#[test]
fn fx15_pause_during_production_takes_effect_at_the_next_step() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-fx15-pause";
    let blocking = blocking_state(&temp, "RENDER_CHAPTER");
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&blocking.state, project_id);
    let finals_before = final_chapter_count(&blocking.state, project_id);

    let (control, latency, produced) = run_with_control(
        &blocking.state,
        project_id,
        (&chapter_id, &volume_id),
        blocking.entered,
        blocking.release,
        "production_pause",
    );

    assert_eq!(control["actionStatus"], "PRODUCTION_CONTROL_REQUESTED");
    assert!(
        latency < Duration::from_secs(1),
        "pause must not wait for the writer: {latency:?}"
    );
    assert_eq!(produced["actionStatus"], "PRODUCTION_PAUSED");
    let job_id = produced["jobId"].as_str().unwrap();
    assert_eq!(job_status(&blocking.state, project_id, job_id), "PAUSED");
    assert!(has_project_command_receipt(
        &blocking.state,
        project_id,
        "key:fx15:single",
    ));
    assert_eq!(
        final_chapter_count(&blocking.state, project_id),
        finals_before,
        "nothing is committed"
    );
    let control_guard = blocking
        .state
        .manager
        .lock_production_control(project_id)
        .unwrap();
    assert!(
        blocking
            .state
            .manager
            .peek_production_control(&control_guard, project_id)
            .unwrap()
            .is_none(),
        "the control request is consumed"
    );
}

#[test]
fn fx15_cancel_during_production_stops_without_committing() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-fx15-cancel";
    let blocking = blocking_state(&temp, "RENDER_CHAPTER");
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&blocking.state, project_id);
    let finals_before = final_chapter_count(&blocking.state, project_id);

    let (control, _, produced) = run_with_control(
        &blocking.state,
        project_id,
        (&chapter_id, &volume_id),
        blocking.entered,
        blocking.release,
        "production_cancel",
    );

    assert_eq!(control["actionStatus"], "PRODUCTION_CONTROL_REQUESTED");
    assert_eq!(produced["actionStatus"], "PRODUCTION_CANCELLED");
    assert_eq!(
        job_status(
            &blocking.state,
            project_id,
            produced["jobId"].as_str().unwrap()
        ),
        "CANCELLED"
    );
    assert!(has_project_command_receipt(
        &blocking.state,
        project_id,
        "key:fx15:single",
    ));
    assert_eq!(
        final_chapter_count(&blocking.state, project_id),
        finals_before,
        "nothing is committed"
    );
}

#[test]
fn fx15_resume_after_pause_reuses_completed_render() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-fx15-resume";
    let blocking = blocking_state(&temp, "AUDIT_EXTRACT");
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&blocking.state, project_id);
    let finals_before = final_chapter_count(&blocking.state, project_id);
    let (_, _, paused) = run_with_control(
        &blocking.state,
        project_id,
        (&chapter_id, &volume_id),
        blocking.entered,
        blocking.release,
        "production_pause",
    );
    assert_eq!(paused["actionStatus"], "PRODUCTION_PAUSED");
    let paused_job = paused["jobId"].as_str().unwrap().to_owned();

    let resumed = execute_command_impl(
        &blocking.state,
        start_request(
            project_id,
            (&chapter_id, &volume_id),
            "resume",
            Some(&paused_job),
        ),
    )
    .unwrap();

    assert_eq!(resumed["finalStatus"], "committed");
    assert_eq!(
        final_chapter_count(&blocking.state, project_id),
        finals_before + 1
    );
    let renders = blocking
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|call| *call == "RENDER_CHAPTER")
        .count();
    assert_eq!(renders, 1, "the paused job's completed render is reused");
}

#[test]
fn fx15_pause_without_running_production_uses_the_normal_path() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-fx15-idle";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let _ = seed_b08_long_form_project(&state, project_id);
    let target_job =
        ai_novel_production_system_core::stable_id::StableIdentityFactory::from_command(
            ai_novel_production_system_core::stable_id::StableIdKind::Job,
            "cmd:fx15:idle-target",
        )
        .unwrap();
    let response = execute_command_impl(
        &state,
        control_request(project_id, "production_pause", "idle", &target_job),
    );
    if let Ok(value) = &response {
        assert_ne!(value["actionStatus"], "PRODUCTION_CONTROL_REQUESTED");
    }
    assert!(
        !temp
            .path()
            .join(format!(
                "projects/{project_id}/control/production-control.json"
            ))
            .exists(),
        "no control file when nothing is running"
    );
}

#[test]
fn fx15_duplicate_cancel_requests_share_one_pending_signal() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-fx15-control-race";
    let blocking = blocking_state(&temp, "RENDER_CHAPTER");
    let state = &blocking.state;
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(state, project_id);

    thread::scope(|scope| {
        let production = scope.spawn(|| {
            execute_command_impl(
                state,
                start_request(project_id, (&chapter_id, &volume_id), "race", None),
            )
        });
        blocking
            .entered
            .recv_timeout(Duration::from_secs(60))
            .unwrap();
        let barrier = Arc::new(Barrier::new(8));
        let mut requests = Vec::new();
        for _ in 0..8 {
            let barrier = barrier.clone();
            let job_id =
                ai_novel_production_system_core::stable_id::StableIdentityFactory::from_command(
                    ai_novel_production_system_core::stable_id::StableIdKind::Job,
                    "cmd:fx15:race",
                )
                .unwrap();
            let request = control_request(project_id, "production_cancel", "control-race", &job_id);
            requests.push(scope.spawn(move || {
                barrier.wait();
                execute_command_impl(state, request)
            }));
        }
        for request in requests {
            assert!(matches!(
                request.join().unwrap().unwrap()["actionStatus"].as_str(),
                Some("PRODUCTION_CONTROL_REQUESTED" | "PRODUCTION_CONTROL_REPLAYED")
            ));
        }
        blocking.release.send(()).unwrap();
        assert_eq!(
            production.join().unwrap().unwrap()["actionStatus"],
            "PRODUCTION_CANCELLED"
        );
    });
}

#[test]
fn fx15_distinct_control_cannot_replace_an_uncommitted_signal() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-fx15-control-pending";
    let blocking = blocking_state(&temp, "RENDER_CHAPTER");
    let state = &blocking.state;
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(state, project_id);
    let job_id = ai_novel_production_system_core::stable_id::StableIdentityFactory::from_command(
        ai_novel_production_system_core::stable_id::StableIdKind::Job,
        "cmd:fx15:pending-production",
    )
    .unwrap();

    thread::scope(|scope| {
        let production = scope.spawn(|| {
            execute_command_impl(
                state,
                start_request(
                    project_id,
                    (&chapter_id, &volume_id),
                    "pending-production",
                    None,
                ),
            )
        });
        blocking
            .entered
            .recv_timeout(Duration::from_secs(60))
            .unwrap();
        let pause = execute_command_impl(
            state,
            control_request(project_id, "production_pause", "pending-pause", &job_id),
        )
        .unwrap();
        assert_eq!(pause["actionStatus"], "PRODUCTION_CONTROL_REQUESTED");
        let cancel = execute_command_impl(
            state,
            control_request(project_id, "production_cancel", "pending-cancel", &job_id),
        )
        .unwrap();
        assert_eq!(cancel["actionStatus"], "PRODUCTION_CONTROL_PENDING");
        blocking.release.send(()).unwrap();
        assert_eq!(
            production.join().unwrap().unwrap()["actionStatus"],
            "PRODUCTION_PAUSED"
        );
    });

    assert_eq!(job_status(state, project_id, &job_id), "PAUSED");
    assert!(has_project_command_receipt(
        state,
        project_id,
        "key:fx15:pending-pause",
    ));
    assert!(!has_project_command_receipt(
        state,
        project_id,
        "key:fx15:pending-cancel",
    ));
}

#[test]
fn fx15_control_status_rolls_back_when_receipt_insert_fails() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-fx15-control-atomic";
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(&state, project_id);
    let (handle, job_id) = start_running_job(
        &state,
        project_id,
        &chapter_id,
        &volume_id,
        "atomic-job",
        NOW,
    );
    handle
        .execute_batch(
            "CREATE TRIGGER reject_fx15_control_receipt BEFORE INSERT ON project_command_receipts WHEN NEW.idempotency_key='key:fx15:atomic-cancel' BEGIN SELECT RAISE(ABORT, 'reject control receipt'); END;",
        )
        .unwrap();
    drop(handle);

    let error = execute_command_impl(
        &state,
        control_request(project_id, "production_cancel", "atomic-cancel", &job_id),
    )
    .unwrap_err();

    assert!(!error.is_empty());
    assert_eq!(job_status(&state, project_id, &job_id), "RUNNING");
    assert!(state
        .manager
        .with_read_connection(project_id, NOW, |connection| {
            Ok(connection.query_row(
                "SELECT released_at IS NULL FROM job_leases WHERE project_id=?1 AND job_id=?2",
                libsql_rusqlite::params![project_id, job_id],
                |row| row.get::<_, bool>(0),
            )?)
        })
        .unwrap());
    assert!(!has_project_command_receipt(
        &state,
        project_id,
        "key:fx15:atomic-cancel",
    ));
}

#[test]
fn fx15_commit_guard_rejects_a_control_that_arrives_after_commit_started() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-fx15-commit-race";
    let blocking = blocking_state(&temp, "AUDIT_EXTRACT");
    let state = &blocking.state;
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(state, project_id);
    let finals_before = final_chapter_count(state, project_id);
    let (commit_locked_tx, commit_locked_rx) = mpsc::channel();
    let (release_commit_tx, release_commit_rx) = mpsc::channel();
    state
        .manager
        .set_production_control_commit_test_hook(Some(Box::new(move || {
            commit_locked_tx.send(()).unwrap();
            release_commit_rx.recv().unwrap();
        })))
        .unwrap();

    thread::scope(|scope| {
        let production = scope.spawn(|| {
            execute_command_impl(
                state,
                start_request(project_id, (&chapter_id, &volume_id), "commit-race", None),
            )
        });
        blocking
            .entered
            .recv_timeout(Duration::from_secs(60))
            .unwrap();
        blocking.release.send(()).unwrap();
        commit_locked_rx
            .recv_timeout(Duration::from_secs(60))
            .unwrap();

        let (control_started_tx, control_started_rx) = mpsc::channel();
        let (control_result_tx, control_result_rx) = mpsc::channel();
        let control = scope.spawn(move || {
            control_started_tx.send(()).unwrap();
            control_result_tx
                .send(execute_command_impl(
                    state,
                    control_request(
                        project_id,
                        "production_cancel",
                        "late-cancel",
                        &ai_novel_production_system_core::stable_id::StableIdentityFactory::from_command(
                            ai_novel_production_system_core::stable_id::StableIdKind::Job,
                            "cmd:fx15:commit-race",
                        )
                        .unwrap(),
                    ),
                ))
                .unwrap();
        });
        control_started_rx.recv().unwrap();
        assert!(
            control_result_rx
                .recv_timeout(Duration::from_millis(150))
                .is_err(),
            "the control request must wait while the final commit owns the boundary"
        );

        release_commit_tx.send(()).unwrap();
        assert_eq!(
            production.join().unwrap().unwrap()["finalStatus"],
            "committed"
        );
        let control_receipt = control_result_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        control.join().unwrap();
        assert_eq!(
            control_receipt["actionStatus"],
            "PRODUCTION_CONTROL_NOT_ACCEPTED"
        );
        assert!(control_receipt["message"]
            .as_str()
            .unwrap()
            .contains("取消未生效"));
        assert_eq!(final_chapter_count(state, project_id), finals_before + 1);
        assert!(has_project_command_receipt(
            state,
            project_id,
            "key:fx15:late-cancel",
        ));
        let stored_cancel = state
            .manager
            .find_project_command_receipt(project_id, "key:fx15:late-cancel")
            .unwrap()
            .unwrap();
        assert!(!stored_cancel.control_result.as_ref().unwrap().applied);
        assert_eq!(
            stored_cancel.control_result.as_ref().unwrap().job_id,
            ai_novel_production_system_core::stable_id::StableIdentityFactory::from_command(
                ai_novel_production_system_core::stable_id::StableIdKind::Job,
                "cmd:fx15:commit-race",
            )
            .unwrap()
        );
        let replayed_cancel = execute_command_impl(
            state,
            control_request(
                project_id,
                "production_cancel",
                "late-cancel",
                &ai_novel_production_system_core::stable_id::StableIdentityFactory::from_command(
                    ai_novel_production_system_core::stable_id::StableIdKind::Job,
                    "cmd:fx15:commit-race",
                )
                .unwrap(),
            ),
        )
        .unwrap();
        assert_eq!(
            replayed_cancel["actionStatus"],
            "PRODUCTION_CONTROL_NOT_ACCEPTED"
        );
        assert_eq!(replayed_cancel["message"], control_receipt["message"]);
        assert!(replayed_cancel["message"]
            .as_str()
            .unwrap()
            .contains("取消未生效"));
    });
}

#[test]
fn fx15_control_lock_for_one_project_does_not_block_another() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let first = state
        .manager
        .lock_production_control("p-fx15-lock-first")
        .unwrap();
    let second = state
        .manager
        .lock_production_control("p-fx15-lock-second")
        .unwrap();
    drop(second);
    drop(first);
}

#[test]
fn fx15_replaying_a_control_command_after_restart_does_not_cancel_a_later_job() {
    let temp = tempfile::tempdir().unwrap();
    let project_id = "p-fx15-control-replay";
    let blocking = blocking_state(&temp, "RENDER_CHAPTER");
    let state = &blocking.state;
    let (chapter_id, volume_id, _, _) = seed_b08_long_form_project(state, project_id);
    let (handle, first_job_id) = start_running_job(
        state,
        project_id,
        &chapter_id,
        &volume_id,
        "first-job",
        "2026-10-13T12:00:00Z",
    );
    let request = control_request(
        project_id,
        "production_cancel",
        "replay-cancel",
        &first_job_id,
    );

    let first = execute_command_impl(state, request.clone()).unwrap();
    assert_eq!(first["actionStatus"], "PRODUCTION_CONTROL_REQUESTED");
    let sidecar = temp.path().join(format!(
        "projects/{project_id}/control/production-control.json"
    ));
    let pending_file: Value = serde_json::from_slice(&std::fs::read(&sidecar).unwrap()).unwrap();
    assert_eq!(pending_file["pending"]["jobId"], first_job_id);
    assert!(
        pending_file.get("requests").is_none(),
        "the sidecar is not a receipt ledger"
    );
    drop(handle);
    let restarted_state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(RecordingGateway),
    );
    let recovered = execute_command_impl(&restarted_state, request.clone()).unwrap();

    assert_eq!(recovered["finalStatus"], "committed");
    assert_eq!(recovered["actionStatus"], "PRODUCTION_CANCELLED");
    assert_eq!(recovered["controlResult"]["jobId"], first_job_id);
    assert_eq!(recovered["controlResult"]["applied"], true);
    assert_eq!(
        job_status(&restarted_state, project_id, &first_job_id),
        "CANCELLED"
    );
    assert!(has_project_command_receipt(
        &restarted_state,
        project_id,
        "key:fx15:replay-cancel",
    ));
    let recovered_file: Value = serde_json::from_slice(&std::fs::read(&sidecar).unwrap()).unwrap();
    assert!(recovered_file["pending"].is_null());
    assert!(recovered_file.get("requests").is_none());

    let (_later_handle, later_job_id) = start_running_job(
        &restarted_state,
        project_id,
        &chapter_id,
        &volume_id,
        "later-job",
        "2026-10-13T12:01:00Z",
    );
    let replay = execute_command_impl(&restarted_state, request).unwrap();
    assert_eq!(replay["finalStatus"], "committed");
    assert_eq!(replay["actionStatus"], recovered["actionStatus"]);
    assert_eq!(replay["message"], recovered["message"]);
    assert_eq!(replay["controlResult"]["jobId"], first_job_id);
    assert!(replay["receiptId"].as_str().is_some());
    assert_eq!(job_status(state, project_id, &later_job_id), "RUNNING");
}
