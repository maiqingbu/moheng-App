use ai_novel_production_system_core::commands::{
    execute_command_impl, query_project_overview_impl, KernelState,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::{ProjectHandle, ProjectManager};
use ai_novel_production_system_core::runtime::{
    DurableJobService, DurableModelRunner, FakeModelTransport, JobSpec, MemorySecretStore,
    ModelRunSpec, ModelTransport, RuntimeError, SecretStore, TransportFailure, TransportResponse,
    TransportUsage,
};
use libsql_rusqlite::Connection;
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
        job_id: "job:111111111111111111111111".into(),
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
        model_run_id: "mr:aaaaaaaaaaaaaaaaaaaaaaaa".into(),
        job_id: Some("job:111111111111111111111111".into()),
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

struct PreSendAssertingTransport {
    calls: usize,
}

impl ModelTransport for PreSendAssertingTransport {
    fn before_send(
        &mut self,
        connection: &Connection,
        spec: &ModelRunSpec,
    ) -> Result<(), RuntimeError> {
        let status: String = connection
            .query_row(
                "SELECT status FROM model_runs WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .map_err(RuntimeError::Sqlite)?;
        assert_eq!(status, "RUNNING");
        let reservation_status: String = connection
            .query_row(
                "SELECT status FROM budget_reservations WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .map_err(RuntimeError::Sqlite)?;
        assert_eq!(reservation_status, "RESERVED");
        Ok(())
    }

    fn send(
        &mut self,
        _spec: &ModelRunSpec,
        _credential: &str,
    ) -> Result<TransportResponse, TransportFailure> {
        self.calls += 1;
        Ok(TransportResponse {
            raw_response: json!({"draft":"send-boundary"}).to_string(),
            usage: TransportUsage {
                input_tokens: 3,
                output_tokens: 4,
                cached_tokens: Some(1),
                provider_reported_cost_cents: Some(2),
                latency_ms: 7,
                provider_request_id: Some("pre-send-check".into()),
            },
        })
    }
}

struct RawPersistFailureTransport;

impl ModelTransport for RawPersistFailureTransport {
    fn before_raw_persist(
        &mut self,
        _connection: &Connection,
        _spec: &ModelRunSpec,
    ) -> Result<(), RuntimeError> {
        Err(RuntimeError::RawResponsePersistenceFailed)
    }

    fn send(
        &mut self,
        _spec: &ModelRunSpec,
        _credential: &str,
    ) -> Result<TransportResponse, TransportFailure> {
        Ok(TransportResponse {
            raw_response: json!({"draft":"not-committed"}).to_string(),
            usage: TransportUsage {
                input_tokens: 2,
                output_tokens: 3,
                cached_tokens: None,
                provider_reported_cost_cents: Some(4),
                latency_ms: 5,
                provider_request_id: None,
            },
        })
    }
}

struct CredentialAssertingTransport {
    expected: String,
    calls: usize,
}

impl ModelTransport for CredentialAssertingTransport {
    fn send(
        &mut self,
        _spec: &ModelRunSpec,
        credential: &str,
    ) -> Result<TransportResponse, TransportFailure> {
        assert_eq!(credential, self.expected);
        self.calls += 1;
        Ok(TransportResponse {
            raw_response: json!({"draft":"credential-asserted"}).to_string(),
            usage: TransportUsage {
                provider_reported_cost_cents: Some(1),
                ..TransportUsage::default()
            },
        })
    }
}

#[test]
fn b03_job_lifecycle_persists_steps_lease_checkpoint_and_restart_recovery() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableJobService::acquire_lease(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "worker-1",
        "2026-08-27T01:00:00Z",
        NOW,
    )
    .unwrap();
    DurableJobService::save_checkpoint(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "outline",
        12,
        &json!({"chapter":"chapter-1"}),
        NOW,
    )
    .unwrap();
    DurableJobService::transition(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "RUNNING",
        NOW,
    )
    .unwrap();
    let recovered = DurableJobService::recover_after_restart(&connection, "p1", NOW).unwrap();
    assert_eq!(recovered.recovered_jobs, 1);
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM jobs WHERE job_id = 'job:111111111111111111111111'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "PAUSED"
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM job_steps WHERE job_id = 'job:111111111111111111111111'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        3
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM job_leases WHERE job_id = 'job:111111111111111111111111'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT offset FROM job_checkpoints WHERE job_id = 'job:111111111111111111111111'",
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
    DurableJobService::transition(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "RUNNING",
        NOW,
    )
    .unwrap();
    let mut transport = FakeModelTransport::success(json!({"draft":"hello"}));
    DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).unwrap();
    assert_eq!(transport.send_calls(), 1);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_runs WHERE model_run_id = 'mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_responses WHERE model_run_id = 'mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_parse_results WHERE model_run_id = 'mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM model_runs WHERE model_run_id = 'mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
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
                "SELECT status FROM model_runs WHERE model_run_id = 'mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
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
                "SELECT COUNT(*) FROM model_responses WHERE model_run_id = 'mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM model_parse_results WHERE model_run_id = 'mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
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
                "SELECT COUNT(*) FROM model_responses WHERE model_run_id = 'mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
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
        &budgeted_model_spec("mr:bbbbbbbbbbbbbbbbbbbbbbbb", "budget:hard-run", 1),
        NOW,
    )
    .unwrap();
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM budget_reservations WHERE model_run_id='mr:bbbbbbbbbbbbbbbbbbbbbbbb'",
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
        &budgeted_model_spec("mr:cccccccccccccccccccccccc", "budget:hard-run", 1),
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
    connection.execute("INSERT INTO model_runs(model_run_id, project_id, job_id, provider, model, idempotency_key, capability, status, created_at, updated_at) VALUES ('mr:dddddddddddddddddddddddd','p1','job:111111111111111111111111','fake','fake-model','spent-key','chapter-draft','COMPLETED',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO model_usage(usage_id, project_id, model_run_id, input_tokens, output_tokens, total_tokens, latency_ms, cost_cents, recorded_at) VALUES ('usage:spent','p1','mr:dddddddddddddddddddddddd',10,10,20,1,90,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO model_cost_ledger(ledger_id, project_id, model_run_id, budget_id, cost_cents, token_total, cost_type, created_at) VALUES ('ledger:spent','p1','mr:dddddddddddddddddddddddd','budget:cost',90,20,'ACTUAL',?1)", [NOW]).unwrap();
    let mut transport = FakeModelTransport::success(json!({"draft":"must-not-send"}));
    assert!(DurableModelRunner::execute(
        &connection,
        &mut transport,
        &budgeted_model_spec("mr:eeeeeeeeeeeeeeeeeeeeeeee", "budget:cost", 20),
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
        &budgeted_model_spec("mr:ffffffffffffffffffffffff", "budget:soft", 1),
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
                "SELECT status FROM budget_reservations WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
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
                "SELECT status FROM budget_reservations WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "SETTLED"
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_usage WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
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
    assert_eq!(connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM model_cost_ledger WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa' AND cost_type='ACTUAL'", [], |row| row.get(0)).unwrap(), 1);
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
                "SELECT status FROM jobs WHERE job_id='job:111111111111111111111111'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "WAITING_DECISION"
    );
}

#[test]
fn b03_send_boundary_is_durable_before_transport() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut transport = PreSendAssertingTransport { calls: 0 };
    DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).unwrap();
    assert_eq!(transport.calls, 1);
}

#[test]
fn b03_prepared_recovery_reuses_model_run_and_reservation() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableModelRunner::prepare(&connection, &model_spec(), NOW).unwrap();
    let mut transport = FakeModelTransport::success(json!({"draft":"recovered"}));
    DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).unwrap();
    assert_eq!(transport.send_calls(), 1);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_runs WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM budget_reservations WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn b03_running_model_run_recovers_to_unknown_without_resend() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut transport = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).is_err());
    assert_eq!(transport.send_calls(), 1);
    let mut retry_transport = FakeModelTransport::success(json!({"draft":"must-not-send"}));
    assert!(
        DurableModelRunner::execute(&connection, &mut retry_transport, &model_spec(), NOW).is_err()
    );
    assert_eq!(retry_transport.send_calls(), 0);
}

#[test]
fn b03_raw_persistence_failure_stops_parse_and_commit_but_keeps_cost() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut transport = RawPersistFailureTransport;
    assert!(DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).is_err());
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_responses WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_parse_results WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_usage WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM model_runs WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "FAILED"
    );
}

#[test]
fn b03_lease_is_finite_reclaimable_and_released_on_pause_cancel() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableJobService::acquire_lease(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "worker-a",
        "2026-08-27T00:01:00Z",
        NOW,
    )
    .unwrap();
    assert!(matches!(
        DurableJobService::acquire_lease(
            &connection,
            "p1",
            "job:111111111111111111111111",
            "worker-b",
            "2026-08-27T00:03:00Z",
            NOW
        ),
        Err(RuntimeError::LeaseHeld)
    ));
    DurableJobService::acquire_lease(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "worker-b",
        "2026-08-27T00:03:00Z",
        "2026-08-27T00:02:00Z",
    )
    .unwrap();
    DurableJobService::pause(&connection, "p1", "job:111111111111111111111111", NOW).unwrap();
    assert_eq!(connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM job_leases WHERE job_id='job:111111111111111111111111' AND released_at IS NULL", [], |row| row.get(0)).unwrap(), 0);
}

#[test]
fn b03_job_hierarchy_uses_parent_job_id_and_rejects_wrong_parent() {
    let (_temp, _manager, connection) = project();
    let mut book = job_spec();
    book.job_id = "job:333333333333333333333333".into();
    book.job_type = "BOOK".into();
    book.parent_job_id = None;
    DurableJobService::start_job(&connection, &book, NOW).unwrap();
    let mut volume = job_spec();
    volume.job_id = "job:444444444444444444444444".into();
    volume.job_type = "VOLUME".into();
    volume.parent_job_id = Some(book.job_id.clone());
    DurableJobService::start_job(&connection, &volume, NOW).unwrap();
    let mut chapter = job_spec();
    chapter.parent_job_id = Some(volume.job_id.clone());
    DurableJobService::start_job(&connection, &chapter, NOW).unwrap();
    let mut invalid = job_spec();
    invalid.job_id = "job:555555555555555555555555".into();
    invalid.parent_job_id = Some(chapter.job_id.clone());
    assert!(DurableJobService::start_job(&connection, &invalid, NOW).is_err());
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT parent_job_id FROM jobs WHERE job_id='job:111111111111111111111111'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        volume.job_id
    );
}

#[test]
fn b03_runtime_rejects_arbitrary_durable_ids() {
    let (_temp, _manager, connection) = project();
    let mut invalid = job_spec();
    invalid.job_id = "my-job".into();
    assert!(matches!(
        DurableJobService::start_job(&connection, &invalid, NOW),
        Err(RuntimeError::StableIdRejected)
    ));
    let mut invalid_model = model_spec();
    invalid_model.model_run_id = "my-run".into();
    let mut transport = FakeModelTransport::success(json!({"draft":"must-not-send"}));
    assert!(matches!(
        DurableModelRunner::execute(&connection, &mut transport, &invalid_model, NOW),
        Err(RuntimeError::StableIdRejected)
    ));
    assert_eq!(transport.send_calls(), 0);
}

#[test]
fn b03_unknown_requires_explicit_retry_decision_for_new_attempt() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut timeout_transport = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(
        DurableModelRunner::execute(&connection, &mut timeout_transport, &model_spec(), NOW)
            .is_err()
    );
    let mut next_spec = budgeted_model_spec("mr:121212121212121212121212", "", 1);
    next_spec.budget_id = None;
    next_spec.attempt_no = 1;
    let mut retry_transport = FakeModelTransport::success(json!({"draft":"explicit"}));
    DurableModelRunner::resolve_unknown(
        &connection,
        "mr:aaaaaaaaaaaaaaaaaaaaaaaa",
        "RETRY_REMOTE",
        &mut retry_transport,
        &next_spec,
        NOW,
    )
    .unwrap();
    assert_eq!(retry_transport.send_calls(), 1);
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT outcome FROM decisions WHERE decision_type='UNKNOWN_REMOTE_OUTCOME'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "RETRY_REMOTE"
    );
}

#[test]
fn b03_restart_recovery_projects_unknown_decision_state() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableModelRunner::prepare(&connection, &model_spec(), NOW).unwrap();
    connection
        .execute(
            "UPDATE model_runs SET status='RUNNING' WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'",
            [],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE jobs SET status='RUNNING' WHERE job_id='job:111111111111111111111111'",
            [],
        )
        .unwrap();
    let recovered =
        DurableJobService::recover_after_restart(&connection, "p1", "2026-08-27T00:02:00Z")
            .unwrap();
    assert_eq!(recovered.recovered_jobs, 1);
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT run_state FROM project_runtime_state WHERE project_id='p1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "WAITING_DECISION"
    );
    assert_eq!(connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM decisions WHERE decision_type='UNKNOWN_REMOTE_OUTCOME' AND outcome='OPEN'", [], |row| row.get(0)).unwrap(), 1);
}

#[test]
fn b03_secret_store_is_consumed_at_transport_boundary_and_redacted() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let store = MemorySecretStore::new();
    let mut transport = ai_novel_production_system_core::runtime::SecretBackedTransport::new(
        FakeModelTransport::success(json!({"draft":"secret-boundary"})),
        store.clone(),
        "fake",
    );
    assert!(matches!(
        DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW),
        Err(RuntimeError::SecretUnavailable)
    ));
    store.put("fake", "sk-secret-b03").unwrap();
    let mut transport = ai_novel_production_system_core::runtime::SecretBackedTransport::new(
        CredentialAssertingTransport {
            expected: "sk-secret-b03".into(),
            calls: 0,
        },
        store,
        "fake",
    );
    assert!(DurableModelRunner::execute(
        &connection,
        &mut transport,
        &ModelRunSpec {
            model_run_id: "mr:171717171717171717171717".into(),
            idempotency_key: "model-key:secret".into(),
            ..model_spec()
        },
        NOW
    )
    .is_ok());
    assert_eq!(
        ai_novel_production_system_core::runtime::redact_secret(
            "Authorization: Bearer sk-secret-b03",
            "sk-secret-b03"
        ),
        "Authorization: Bearer [REDACTED]"
    );
}

#[test]
fn b03_production_start_rejects_caller_supplied_new_job_id() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    drop(state.manager.create("p1", "A", NOW).unwrap());
    let result = execute_command_impl(
        &state,
        json!({"type":"production_start","commandId":"b03-id","idempotencyKey":"b03-id-key","projectId":"p1","expectedRevision":0,"requestedAt":NOW,"payload":{"jobId":"job:caller-supplied"}}),
    );
    assert!(result.is_err());
}

#[test]
fn b03_explicit_unknown_retry_keeps_original_budget_binding() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    seed_budget(&connection, "budget:unknown-retry", 3, 0, 100);
    let first = budgeted_model_spec("mr:131313131313131313131313", "budget:unknown-retry", 5);
    let mut timeout_transport = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(DurableModelRunner::execute(&connection, &mut timeout_transport, &first, NOW).is_err());
    let second = budgeted_model_spec("mr:141414141414141414141414", "budget:unknown-retry", 5);
    let mut retry_transport = FakeModelTransport::success(json!({"draft":"same-budget"}));
    DurableModelRunner::resolve_unknown(
        &connection,
        &first.model_run_id,
        "RETRY_REMOTE",
        &mut retry_transport,
        &second,
        NOW,
    )
    .unwrap();
    assert_eq!(retry_transport.send_calls(), 1);
    assert_eq!(connection.query_row::<String, _, _>("SELECT budget_ref FROM budget_reservations WHERE model_run_id='mr:141414141414141414141414'", [], |row| row.get(0)).unwrap(), "budget:unknown-retry");
}

#[test]
fn b03_explicit_unknown_retry_is_budget_blocked_before_transport_when_hold_exceeds_hard() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    seed_budget(&connection, "budget:unknown-hard", 1, 0, 10);
    let first = budgeted_model_spec("mr:151515151515151515151515", "budget:unknown-hard", 5);
    let mut timeout_transport = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(DurableModelRunner::execute(&connection, &mut timeout_transport, &first, NOW).is_err());
    let second = budgeted_model_spec("mr:161616161616161616161616", "budget:unknown-hard", 5);
    let mut retry_transport = FakeModelTransport::success(json!({"draft":"must-not-send"}));
    assert!(DurableModelRunner::resolve_unknown(
        &connection,
        &first.model_run_id,
        "RETRY_REMOTE",
        &mut retry_transport,
        &second,
        NOW,
    )
    .is_err());
    assert_eq!(retry_transport.send_calls(), 0);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM decisions WHERE decision_type='BUDGET_BLOCKED'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn b03_actual_settlement_is_not_overwritten_by_predicted_cost() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut transport = FakeModelTransport::success_with_usage(
        json!({"draft":"actual-cost"}),
        TransportUsage {
            input_tokens: 8,
            output_tokens: 13,
            cached_tokens: None,
            provider_reported_cost_cents: Some(8),
            latency_ms: 21,
            provider_request_id: Some("actual-cost-request".into()),
        },
    );
    let mut spec = model_spec();
    spec.predicted_cost_cents = 2;
    DurableModelRunner::execute(&connection, &mut transport, &spec, NOW).unwrap();
    assert_eq!(connection.query_row::<i64, _, _>("SELECT settled_cents FROM budget_reservations WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'", [], |row| row.get(0)).unwrap(), 8);
    assert_eq!(connection.query_row::<i64, _, _>("SELECT cost_cents FROM model_usage WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'", [], |row| row.get(0)).unwrap(), 8);
    assert_eq!(connection.query_row::<i64, _, _>("SELECT cost_cents FROM model_cost_ledger WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa' AND cost_type='ACTUAL'", [], |row| row.get(0)).unwrap(), 8);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT total_cost_cents FROM project_runtime_state WHERE project_id='p1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        8
    );
}

#[test]
fn b03_parse_and_completed_recovery_do_not_duplicate_settlement() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut transport = FakeModelTransport::success(json!({"draft":"completed"}));
    DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).unwrap();
    let before = connection.query_row::<(i64, i64, i64), _, _>("SELECT (SELECT COUNT(*) FROM model_usage WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'), (SELECT COUNT(*) FROM model_cost_ledger WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa' AND cost_type='ACTUAL'), (SELECT total_cost_cents FROM project_runtime_state WHERE project_id='p1')", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    let mut retry_transport = FakeModelTransport::failure(TransportFailure::Timeout);
    DurableModelRunner::retry(&connection, &mut retry_transport, &model_spec(), NOW).unwrap();
    let after = connection.query_row::<(i64, i64, i64), _, _>("SELECT (SELECT COUNT(*) FROM model_usage WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'), (SELECT COUNT(*) FROM model_cost_ledger WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa' AND cost_type='ACTUAL'), (SELECT total_cost_cents FROM project_runtime_state WHERE project_id='p1')", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!(before, after);
    assert_eq!(retry_transport.send_calls(), 0);
}

#[test]
fn b03_full_stable_validator_rejects_prefixed_non_opaque_ids() {
    let (_temp, _manager, connection) = project();
    let mut invalid_job = job_spec();
    invalid_job.job_id = "job:hello".into();
    assert!(matches!(
        DurableJobService::start_job(&connection, &invalid_job, NOW),
        Err(RuntimeError::StableIdRejected)
    ));
    let mut invalid_model = model_spec();
    invalid_model.model_run_id = "mr:anything".into();
    let mut transport = FakeModelTransport::success(json!({"draft":"must-not-send"}));
    assert!(matches!(
        DurableModelRunner::execute(&connection, &mut transport, &invalid_model, NOW),
        Err(RuntimeError::StableIdRejected)
    ));
    assert_eq!(transport.send_calls(), 0);
}

#[test]
fn b03_lease_renew_cancel_restart_and_resume_are_observable() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableJobService::acquire_lease(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "worker-a",
        "2026-08-27T00:01:00Z",
        NOW,
    )
    .unwrap();
    DurableJobService::renew_lease(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "worker-a",
        "2026-08-27T00:03:00Z",
        "2026-08-27T00:00:30Z",
    )
    .unwrap();
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT expires_at FROM job_leases WHERE job_id='job:111111111111111111111111'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "2026-08-27T00:03:00Z"
    );
    DurableJobService::cancel(&connection, "p1", "job:111111111111111111111111", NOW).unwrap();
    assert_eq!(connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM job_leases WHERE job_id='job:111111111111111111111111' AND released_at IS NULL", [], |row| row.get(0)).unwrap(), 0);
    let mut restart_job = job_spec();
    restart_job.job_id = "job:222222222222222222222222".into();
    DurableJobService::start_job(&connection, &restart_job, "2026-08-27T00:00:01Z").unwrap();
    DurableJobService::acquire_lease(
        &connection,
        "p1",
        "job:222222222222222222222222",
        "old-worker",
        "2026-08-27T00:01:00Z",
        "2026-08-27T00:00:01Z",
    )
    .unwrap();
    DurableJobService::transition(
        &connection,
        "p1",
        "job:222222222222222222222222",
        "RUNNING",
        "2026-08-27T00:00:01Z",
    )
    .unwrap();
    DurableJobService::recover_after_restart(&connection, "p1", "2026-08-27T00:02:00Z").unwrap();
    DurableJobService::resume(
        &connection,
        "p1",
        "job:222222222222222222222222",
        "2026-08-27T00:02:00Z",
    )
    .unwrap();
    DurableJobService::acquire_lease(
        &connection,
        "p1",
        "job:222222222222222222222222",
        "new-worker",
        "2026-08-27T00:03:00Z",
        "2026-08-27T00:02:00Z",
    )
    .unwrap();
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT holder FROM job_leases WHERE job_id='job:222222222222222222222222'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "new-worker"
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
