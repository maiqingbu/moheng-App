use ai_novel_production_system_core::commands::{
    execute_command_impl, query_project_overview_impl, KernelState,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::{ProjectHandle, ProjectManager};
use ai_novel_production_system_core::runtime::{
    DurableJobService, DurableModelRunner, FakeModelTransport, JobSpec, MemorySecretStore,
    ModelRunSpec, SecretStore, TransportFailure,
};
use serde_json::json;
use tempfile::tempdir;

const NOW: &str = "2026-08-27T00:00:00Z";

fn project() -> (tempfile::TempDir, ProjectManager, ProjectHandle) {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    (temp, manager, connection)
}

fn job_spec() -> JobSpec {
    JobSpec {
        job_id: "job:b03:chapter:1".into(),
        job_type: "CHAPTER".into(),
        parent_job_id: None,
        chapter_id: Some("chapter-1".into()),
        volume_id: None,
        book_id: None,
        stages: vec!["outline".into(), "draft".into(), "review".into()],
    }
}

fn model_spec() -> ModelRunSpec {
    ModelRunSpec {
        model_run_id: "mr:b03:1".into(),
        job_id: Some("job:b03:chapter:1".into()),
        step_id: Some("step:b03:1".into()),
        provider: "fake".into(),
        model: "fake-model".into(),
        idempotency_key: "model-key:b03:1".into(),
        capability: "chapter-draft".into(),
        prompt_ref: Some("prompt:b03:1".into()),
        context_manifest_ref: Some("context:b03:1".into()),
        budget_id: None,
        predicted_cost_cents: 1,
        attempt_no: 0,
        repair_round: 0,
    }
}

fn budgeted_model_spec(run_id: &str, budget_id: &str, predicted_cost_cents: i64) -> ModelRunSpec {
    let mut spec = model_spec();
    spec.model_run_id = run_id.into();
    spec.idempotency_key = format!("model-key:{run_id}");
    spec.budget_id = Some(budget_id.into());
    spec.predicted_cost_cents = predicted_cost_cents;
    spec
}

fn seed_budget(
    connection: &ProjectHandle,
    budget_id: &str,
    hard_runs: i64,
    soft_runs: i64,
    cost: i64,
) {
    connection.execute("INSERT INTO books(book_id, project_id, book_no, title, status, created_at, updated_at) VALUES ('book:b03:budget','p1',1,'Book','ACTIVE',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id, project_id, book_id, volume_no, title, status, chapter_start, chapter_end, target_words, created_at, updated_at) VALUES ('vol:b03:budget','p1','book:b03:budget',1,'Volume','PLANNING',1,1,1000,?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id, project_id, volume_id, chapter_no, title, importance_class, status, word_count, created_at, updated_at) VALUES ('chapter-budget','p1','vol:b03:budget',1,'Chapter','NORMAL','PLANNED',0,?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO chapter_production_budgets(budget_id, project_id, chapter_id, importance_class, soft_max_model_runs, hard_max_model_runs, cost_budget_cents, frozen_at) VALUES (?1,'p1','chapter-budget','NORMAL',?2,?3,?4,?5)", libsql_rusqlite::params![budget_id, soft_runs, hard_runs, cost, NOW]).unwrap();
}

#[test]
fn b03_job_lifecycle_persists_steps_lease_checkpoint_and_restart_recovery() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableJobService::acquire_lease(
        &connection,
        "p1",
        "job:b03:chapter:1",
        "worker-1",
        "2026-08-27T01:00:00Z",
        NOW,
    )
    .unwrap();
    DurableJobService::save_checkpoint(
        &connection,
        "p1",
        "job:b03:chapter:1",
        "outline",
        12,
        &json!({"chapter":"chapter-1"}),
        NOW,
    )
    .unwrap();
    DurableJobService::transition(&connection, "p1", "job:b03:chapter:1", "RUNNING", NOW).unwrap();
    let recovered = DurableJobService::recover_after_restart(&connection, "p1", NOW).unwrap();
    assert_eq!(recovered.recovered_jobs, 1);
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM jobs WHERE job_id = 'job:b03:chapter:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "PAUSED"
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM job_steps WHERE job_id = 'job:b03:chapter:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        3
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM job_leases WHERE job_id = 'job:b03:chapter:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT offset FROM job_checkpoints WHERE job_id = 'job:b03:chapter:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        12
    );
}

#[test]
fn b03_model_run_and_budget_reservation_exist_before_transport_send() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableJobService::transition(&connection, "p1", "job:b03:chapter:1", "RUNNING", NOW).unwrap();
    let mut transport = FakeModelTransport::success(json!({"draft":"hello"}));
    DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).unwrap();
    assert_eq!(transport.send_calls(), 1);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_runs WHERE model_run_id = 'mr:b03:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_responses WHERE model_run_id = 'mr:b03:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_parse_results WHERE model_run_id = 'mr:b03:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM model_runs WHERE model_run_id = 'mr:b03:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "COMPLETED"
    );
}

#[test]
fn b03_timeout_is_unknown_remote_outcome_and_does_not_retry_transport() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut transport = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).is_err());
    assert_eq!(transport.send_calls(), 1);
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM model_runs WHERE model_run_id = 'mr:b03:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "UNKNOWN_REMOTE_OUTCOME"
    );
    assert_eq!(
        DurableModelRunner::retry(&connection, &mut transport, &model_spec(), NOW)
            .unwrap_err()
            .to_string(),
        "UNKNOWN_REMOTE_OUTCOME requires explicit decision"
    );
    assert_eq!(transport.send_calls(), 1);
}

#[test]
fn b03_parse_failure_keeps_raw_response_and_replays_without_transport() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut transport = FakeModelTransport::raw_success("not-json");
    assert!(DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).is_err());
    assert_eq!(transport.send_calls(), 1);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_responses WHERE model_run_id = 'mr:b03:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM model_parse_results WHERE model_run_id = 'mr:b03:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "SCHEMA_INVALID"
    );
    let mut retry_transport = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(
        DurableModelRunner::retry(&connection, &mut retry_transport, &model_spec(), NOW).is_err()
    );
    assert_eq!(retry_transport.send_calls(), 0);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_responses WHERE model_run_id = 'mr:b03:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn b03_authoritative_hard_budget_blocks_transport_and_records_decision() {
    let (_temp, _manager, connection) = project();
    connection.execute("INSERT INTO books(book_id, project_id, book_no, title, status, created_at, updated_at) VALUES ('book:b03:1','p1',1,'Book','ACTIVE',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id, project_id, book_id, volume_no, title, status, chapter_start, chapter_end, target_words, created_at, updated_at) VALUES ('vol:b03:1','p1','book:b03:1',1,'Volume','PLANNING',1,1,1000,?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id, project_id, volume_id, chapter_no, title, importance_class, status, word_count, created_at, updated_at) VALUES ('chapter-1','p1','vol:b03:1',1,'Chapter','NORMAL','PLANNED',0,?1,?1)", [NOW]).unwrap();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    connection.execute("INSERT INTO chapter_production_budgets(budget_id, project_id, chapter_id, importance_class, hard_max_model_runs, frozen_at) VALUES ('budget:b03:1','p1','chapter-1','NORMAL',1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO budget_reservations(reservation_id, project_id, budget_ref, reserved_cents, status, created_at) VALUES ('reservation:b03:1','p1','budget:b03:1',1,'OVER_LIMIT',?1)", [NOW]).unwrap();
    let mut spec = model_spec();
    spec.budget_id = Some("budget:b03:1".into());
    let mut transport = FakeModelTransport::success(json!({"draft":"should-not-send"}));
    assert!(DurableModelRunner::execute(&connection, &mut transport, &spec, NOW).is_err());
    assert_eq!(transport.send_calls(), 0);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM decisions WHERE decision_type = 'BUDGET_REPLAN'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn b03_hard_run_limit_counts_settled_runs_before_transport() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    seed_budget(&connection, "budget:hard-run", 1, 0, 0);
    let mut first_transport = FakeModelTransport::success(json!({"draft":"first"}));
    DurableModelRunner::execute(
        &connection,
        &mut first_transport,
        &budgeted_model_spec("mr:b03:first", "budget:hard-run", 1),
        NOW,
    )
    .unwrap();
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM budget_reservations WHERE model_run_id='mr:b03:first'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "SETTLED"
    );
    let mut second_transport = FakeModelTransport::success(json!({"draft":"must-not-send"}));
    assert!(DurableModelRunner::execute(
        &connection,
        &mut second_transport,
        &budgeted_model_spec("mr:b03:second", "budget:hard-run", 1),
        NOW,
    )
    .is_err());
    assert_eq!(second_transport.send_calls(), 0);
}

#[test]
fn b03_cost_limit_counts_actual_spent_reserved_and_next_prediction() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    seed_budget(&connection, "budget:cost", 0, 0, 100);
    connection.execute("INSERT INTO model_runs(model_run_id, project_id, job_id, provider, model, idempotency_key, capability, status, created_at, updated_at) VALUES ('mr:b03:spent','p1','job:b03:chapter:1','fake','fake-model','spent-key','chapter-draft','COMPLETED',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO model_usage(usage_id, project_id, model_run_id, input_tokens, output_tokens, total_tokens, latency_ms, cost_cents, recorded_at) VALUES ('usage:spent','p1','mr:b03:spent',10,10,20,1,90,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO model_cost_ledger(ledger_id, project_id, model_run_id, budget_id, cost_cents, token_total, cost_type, created_at) VALUES ('ledger:spent','p1','mr:b03:spent','budget:cost',90,20,'ACTUAL',?1)", [NOW]).unwrap();
    let mut transport = FakeModelTransport::success(json!({"draft":"must-not-send"}));
    assert!(DurableModelRunner::execute(
        &connection,
        &mut transport,
        &budgeted_model_spec("mr:b03:next", "budget:cost", 20),
        NOW,
    )
    .is_err());
    assert_eq!(transport.send_calls(), 0);
}

#[test]
fn b03_soft_limit_creates_visible_budget_pressure() {
    let (temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    seed_budget(&connection, "budget:soft", 3, 1, 0);
    let mut first_transport = FakeModelTransport::success(json!({"draft":"first"}));
    DurableModelRunner::execute(
        &connection,
        &mut first_transport,
        &budgeted_model_spec("mr:b03:soft-first", "budget:soft", 1),
        NOW,
    )
    .unwrap();
    assert_eq!(
        connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM decisions WHERE project_id='p1' AND decision_type='BUDGET_PRESSURE'", [], |row| row.get(0)).unwrap(),
        1
    );
    drop(connection);
    let state = KernelState::new(AppPaths::new(temp.path()));
    let overview = query_project_overview_impl(&state, json!({"projectId":"p1"})).unwrap();
    assert_eq!(overview["pendingDecisionCount"], 1);
}

#[test]
fn b03_definite_transport_failure_releases_reservation() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut transport = FakeModelTransport::failure(TransportFailure::Unavailable);
    assert!(DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).is_err());
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM budget_reservations WHERE model_run_id='mr:b03:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "RELEASED"
    );
}

#[test]
fn b03_parse_failure_settles_remote_cost_once_and_replay_does_not_send() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut transport = FakeModelTransport::raw_success("not-json");
    assert!(DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).is_err());
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM budget_reservations WHERE model_run_id='mr:b03:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "SETTLED"
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_usage WHERE model_run_id='mr:b03:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    let mut retry_transport = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(
        DurableModelRunner::retry(&connection, &mut retry_transport, &model_spec(), NOW).is_err()
    );
    assert_eq!(retry_transport.send_calls(), 0);
    assert_eq!(connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM model_cost_ledger WHERE model_run_id='mr:b03:1' AND cost_type='ACTUAL'", [], |row| row.get(0)).unwrap(), 1);
}

#[test]
fn b03_unknown_remote_outcome_creates_decision_and_waiting_projection() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut transport = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).is_err());
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT outcome FROM decisions WHERE decision_type='UNKNOWN_REMOTE_OUTCOME'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "OPEN"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM jobs WHERE job_id='job:b03:chapter:1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "WAITING_DECISION"
    );
}

#[test]
fn b03_secret_store_keeps_secret_out_of_project_database() {
    let (_temp, _manager, connection) = project();
    let store = MemorySecretStore::new();
    store.put("fake", "sk-secret-b03").unwrap();
    assert_eq!(store.get("fake").unwrap(), "sk-secret-b03");
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM sqlite_master WHERE sql LIKE '%sk-secret-b03%'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );
}

#[test]
fn b03_real_command_path_controls_durable_job_and_runtime_projection() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    drop(state.manager.create("p1", "A", NOW).unwrap());
    let command = |command_id: &str, command_type: &str, revision: i64| {
        execute_command_impl(
            &state,
            json!({
                "type": command_type,
                "commandId": command_id,
                "idempotencyKey": format!("{command_id}-key"),
                "projectId": "p1",
                "expectedRevision": revision,
                "requestedAt": NOW
            }),
        )
        .unwrap()
    };
    assert_eq!(
        command("b03-start", "production_start", 0)["finalStatus"],
        "committed"
    );
    let connection = state.manager.open("p1", "A", NOW).unwrap();
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT run_state FROM project_runtime_state WHERE project_id='p1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "ACTIVE"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM jobs WHERE project_id='p1' ORDER BY updated_at DESC LIMIT 1",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "RUNNING"
    );
    drop(connection);
    assert_eq!(
        command("b03-pause", "production_pause", 1)["finalStatus"],
        "committed"
    );
    assert_eq!(
        command("b03-resume", "production_resume", 2)["finalStatus"],
        "committed"
    );
    assert_eq!(
        command("b03-cancel", "production_cancel", 3)["finalStatus"],
        "committed"
    );
    let connection = state.manager.open("p1", "A", NOW).unwrap();
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT run_state FROM project_runtime_state WHERE project_id='p1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "FAILED"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM jobs WHERE project_id='p1' ORDER BY updated_at DESC LIMIT 1",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "CANCELLED"
    );
}
