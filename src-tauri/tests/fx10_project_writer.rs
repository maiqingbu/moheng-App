use ai_novel_production_system_core::commands::{
    execute_command_impl, query_dependency_summary_bootstrap_impl, KernelState,
};
use ai_novel_production_system_core::production::UnavailableProductionGateway;
use ai_novel_production_system_core::project_db::{
    paths::AppPaths, ManagerError, ProjectCommandSpec, ProjectManager, WRITER_WAIT_TIMEOUT,
};
use serde_json::{json, Value};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

const NOW: &str = "2026-10-13T00:00:00Z";

fn kernel(temp: &tempfile::TempDir) -> KernelState {
    KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(UnavailableProductionGateway),
    )
}

fn bootstrap_command(
    state: &KernelState,
    project_id: &str,
    command_id: &str,
) -> Result<Value, String> {
    execute_command_impl(
        state,
        json!({
            "type": "dependency_summary_bootstrap",
            "commandId": command_id,
            "idempotencyKey": format!("{command_id}:key"),
            "projectId": project_id,
            "expectedRevision": 0,
            "requestedAt": NOW,
            "payload": {},
        }),
    )
}

/// Holds the project's writer on another thread until `release` is signalled
/// (or the sender is dropped). Returns once the writer is actually held.
fn hold_writer_on_other_thread(
    manager: &ProjectManager,
    project_id: &str,
) -> (mpsc::Sender<()>, thread::JoinHandle<()>) {
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let manager = manager.clone();
    let project_id = project_id.to_string();
    let worker = thread::spawn(move || {
        let handle = manager.open(&project_id, "", NOW).unwrap();
        held_tx.send(()).unwrap();
        let _ = release_rx.recv();
        drop(handle);
    });
    held_rx.recv().unwrap();
    (release_tx, worker)
}

fn revision_and_job_rows(manager: &ProjectManager, project_id: &str) -> (i64, i64) {
    manager
        .with_read_connection(project_id, NOW, |connection| {
            Ok(connection.query_row(
                "SELECT (SELECT revision FROM project_meta WHERE project_id=?1), (SELECT COUNT(*) FROM jobs WHERE project_id=?1)",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?)
        })
        .unwrap()
}

#[test]
fn fx10_writer_waits_for_release_within_timeout() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    drop(manager.create("p-fx10-wait", "FX-10", NOW).unwrap());
    let holder = manager.clone();
    let (held_tx, held_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let handle = holder.open("p-fx10-wait", "", NOW).unwrap();
        held_tx.send(()).unwrap();
        thread::sleep(Duration::from_millis(300));
        drop(handle);
    });
    held_rx.recv().unwrap();

    let started = Instant::now();
    let handle = manager.open("p-fx10-wait", "", NOW).unwrap();
    let waited = started.elapsed();

    drop(handle);
    worker.join().unwrap();
    assert!(
        waited >= Duration::from_millis(250),
        "writer must wait for release; waited {waited:?}"
    );
    assert!(
        waited < WRITER_WAIT_TIMEOUT,
        "writer must not wait for the full timeout; waited {waited:?}"
    );
}

#[test]
fn fx10_writer_timeout_returns_project_busy_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let state = kernel(&temp);
    drop(
        state
            .manager
            .create("p-fx10-timeout", "FX-10", NOW)
            .unwrap(),
    );
    let before = revision_and_job_rows(&state.manager, "p-fx10-timeout");
    let (release, worker) = hold_writer_on_other_thread(&state.manager, "p-fx10-timeout");

    let started = Instant::now();
    let error = bootstrap_command(&state, "p-fx10-timeout", "cmd:fx10:timeout").unwrap_err();
    let waited = started.elapsed();

    assert!(error.contains("PROJECT_BUSY"), "unexpected error: {error}");
    assert!(
        !error.contains("project writer already open"),
        "raw writer text leaked: {error}"
    );
    assert!(
        waited >= WRITER_WAIT_TIMEOUT,
        "must wait for the bounded timeout; waited {waited:?}"
    );
    assert!(
        waited < WRITER_WAIT_TIMEOUT + Duration::from_secs(2),
        "waited {waited:?}"
    );
    assert_eq!(
        revision_and_job_rows(&state.manager, "p-fx10-timeout"),
        before
    );
    drop(release);
    worker.join().unwrap();
}

#[test]
fn fx10_same_thread_reentry_fails_fast() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let _held = manager.create("p-fx10-reentry", "FX-10", NOW).unwrap();

    let started = Instant::now();
    let result = manager.open("p-fx10-reentry", "", NOW);

    assert!(matches!(result, Err(ManagerError::AlreadyOpen)));
    assert!(
        started.elapsed() < Duration::from_millis(50),
        "re-entry waited {:?}",
        started.elapsed()
    );
}

#[test]
fn fx10_structural_ops_fail_fast_with_project_busy() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    drop(manager.create("p-fx10-structural", "FX-10", NOW).unwrap());
    let backup_path = temp.path().join("fx10-backup.db");
    let (release, worker) = hold_writer_on_other_thread(&manager, "p-fx10-structural");

    let started = Instant::now();
    let deleted = manager.delete_to_trash("p-fx10-structural", NOW);
    let duplicated = manager.duplicate("p-fx10-structural", "p-fx10-structural-copy", NOW);
    let restored = manager.restore_backup(
        ProjectCommandSpec {
            project_id: "p-fx10-structural",
            result_project_id: "p-fx10-structural",
            command_id: "cmd:fx10:restore",
            idempotency_key: "cmd:fx10:restore:key",
            command_type: "backup_restore",
            request_digest: &"c".repeat(64),
            expected_revision: Some(0),
            now: NOW,
            advance_revision: true,
        },
        &backup_path,
        &"d".repeat(64),
    );
    let elapsed = started.elapsed();

    assert!(
        matches!(deleted, Err(ManagerError::ProjectBusy)),
        "delete: {deleted:?}"
    );
    assert!(
        matches!(duplicated, Err(ManagerError::ProjectBusy)),
        "duplicate: {duplicated:?}"
    );
    assert!(
        matches!(restored, Err(ManagerError::ProjectBusy)),
        "restore: {:?}",
        restored.err()
    );
    assert!(
        elapsed < Duration::from_millis(500),
        "structural ops must not queue; took {elapsed:?}"
    );
    assert!(temp
        .path()
        .join("projects/p-fx10-structural/project.db")
        .is_file());
    drop(release);
    worker.join().unwrap();
}

#[test]
fn fx10_background_completion_race_stress() {
    let temp = tempfile::tempdir().unwrap();
    for index in 0..200 {
        // A fresh kernel per iteration keeps the shared read pool (16 live
        // connections) from filling with other projects' idle readers.
        let state = kernel(&temp);
        let project_id = format!("p-fx10-race-{index:03}");
        drop(
            state
                .manager
                .create(&project_id, "FX-10 race", NOW)
                .unwrap(),
        );
        let queued =
            bootstrap_command(&state, &project_id, &format!("cmd:fx10:race:{index}:first"))
                .unwrap();
        assert_eq!(queued["result"]["status"], "QUEUED");
        // Deliberately no writer-release probe: the next write is issued the
        // moment the background job reports completion.
        loop {
            let status =
                query_dependency_summary_bootstrap_impl(&state, json!({"projectId":project_id}))
                    .unwrap();
            if status["summary"]["summaryStatus"] == "READY"
                && status["job"]["status"] == "COMPLETED"
            {
                break;
            }
            thread::yield_now();
        }
        let again = bootstrap_command(&state, &project_id, &format!("cmd:fx10:race:{index}:again"))
            .unwrap_or_else(|error| panic!("iteration {index}: {error}"));
        assert_eq!(again["result"]["status"], "READY", "iteration {index}");
    }
}
