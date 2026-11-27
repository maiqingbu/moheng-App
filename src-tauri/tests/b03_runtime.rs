use ai_novel_production_system_core::commands::{
    execute_command_impl, query_project_overview_impl, KernelState,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::{ManagerError, ProjectHandle, ProjectManager};
use ai_novel_production_system_core::runtime::{
    lease_expiry, DurableJobService, DurableModelRunner, FakeModelTransport, JobSpec,
    MemorySecretStore, ModelRunSpec, ModelTransport, ModelTransportDiagnostic, RuntimeError,
    SecretStore, TransportFailure, TransportResponse, TransportUsage,
};
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use std::sync::{Arc, Barrier};
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
        cost_snapshot: None,
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
    DurableJobService::transition_for_worker(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "RUNNING",
        "worker-1",
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
    assert_eq!(
        connection
            .query_row::<(String, Option<String>), _, _>(
                "SELECT run_state, block_reason FROM project_runtime_state WHERE project_id='p1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?))
            )
            .unwrap(),
        (
            "WAITING_DECISION".into(),
            Some("UNKNOWN_REMOTE_OUTCOME".into())
        )
    );
}

struct DiagnosticUnknownTransport {
    diagnostic: Option<ModelTransportDiagnostic>,
    sends: usize,
}

impl ModelTransport for DiagnosticUnknownTransport {
    fn diagnostics_enabled(&self) -> bool {
        true
    }

    fn diagnostic_timeout_ms(&self) -> Option<u64> {
        Some(30_000)
    }

    fn take_diagnostic(&mut self) -> Option<ModelTransportDiagnostic> {
        self.diagnostic.take()
    }

    fn send(
        &mut self,
        _spec: &ModelRunSpec,
        _credential: &str,
    ) -> Result<TransportResponse, TransportFailure> {
        self.sends += 1;
        Err(TransportFailure::UnknownRemoteOutcome)
    }
}

#[test]
fn b03_transport_diagnostic_is_durable_without_changing_unknown_budget_decision() {
    let (_temp, manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    seed_budget(&connection, "budget:transport-diagnostic", 3, 3, 5);

    let diagnostic = ModelTransportDiagnostic {
        provider_send_started_at_unix_ms: Some(1_791_997_121_100),
        elapsed_ms: Some(1_250),
        timeout_ms: Some(30_000),
        client_timeout_ms: Some(30_000),
        phase: "response_body_read".into(),
        provider_error_code: Some("UNKNOWN_REMOTE_OUTCOME".into()),
        http_status: Some(200),
        response_read: "failed".into(),
        response_bytes: None,
        provider_request_id: Some("acs-fixture-request-123".into()),
    };
    let mut transport = DiagnosticUnknownTransport {
        diagnostic: Some(diagnostic),
        sends: 0,
    };
    let spec = budgeted_model_spec(
        "mr:dddddddddddddddddddddddd",
        "budget:transport-diagnostic",
        1,
    );

    assert!(matches!(
        DurableModelRunner::execute(&connection, &mut transport, &spec, NOW),
        Err(RuntimeError::UnknownRemoteOutcome)
    ));
    assert_eq!(transport.sends, 1);

    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM model_runs WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        "UNKNOWN_REMOTE_OUTCOME"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT outcome FROM decisions WHERE source_ref=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        "OPEN"
    );
    let reservation: (String, i64, i64) = connection
        .query_row(
            "SELECT status,reserved_cents,settled_cents FROM budget_reservations WHERE model_run_id=?1",
            [&spec.model_run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(reservation, ("OVER_LIMIT".into(), 1, 0));

    let events: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM production_events WHERE event_type='MODEL_REQUEST_STARTED' AND json_extract(payload_json,'$.modelRunId')=?1",
            [&spec.model_run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(events, 1);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM production_events WHERE event_type='MODEL_TRANSPORT_RESULT' AND json_extract(payload_json,'$.modelRunId')=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        1
    );
    let payload: String = connection
        .query_row(
            "SELECT payload_json FROM production_events WHERE event_type='MODEL_TRANSPORT_RESULT' AND json_extract(payload_json,'$.modelRunId')=?1",
            [&spec.model_run_id],
            |row| row.get(0),
        )
        .unwrap();
    let payload: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["diagnostic"]["httpStatus"], 200);
    assert_eq!(payload["diagnostic"]["responseRead"], "failed");
    assert_eq!(payload["diagnostic"]["rawResponsePersisted"], false);
    assert_eq!(
        payload["diagnostic"]["transportFailure"],
        "unknown remote outcome"
    );
    assert_eq!(
        payload["diagnostic"]["providerRequestId"],
        "acs-fixture-request-123"
    );
    assert!(payload.get("prompt").is_none());
    assert!(payload.get("rawResponse").is_none());

    let mut duplicate_transport = DiagnosticUnknownTransport {
        diagnostic: Some(ModelTransportDiagnostic {
            provider_send_started_at_unix_ms: Some(1_791_997_121_100),
            elapsed_ms: Some(1_250),
            timeout_ms: Some(30_000),
            client_timeout_ms: Some(30_000),
            phase: "response_body_read".into(),
            provider_error_code: Some("UNKNOWN_REMOTE_OUTCOME".into()),
            http_status: Some(200),
            response_read: "failed".into(),
            response_bytes: None,
            provider_request_id: Some("acs-fixture-request-123".into()),
        }),
        sends: 0,
    };
    assert!(matches!(
        DurableModelRunner::execute(&connection, &mut duplicate_transport, &spec, NOW),
        Err(RuntimeError::UnknownRemoteOutcome)
    ));
    assert_eq!(duplicate_transport.sends, 0);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM production_events WHERE event_type='MODEL_TRANSPORT_RESULT' AND json_extract(payload_json,'$.modelRunId')=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        1
    );

    drop(connection);
    let persisted_events = manager
        .with_read_connection("p1", NOW, |connection| {
            connection
                .query_row::<i64, _, _>(
                    "SELECT COUNT(*) FROM production_events WHERE json_extract(payload_json,'$.modelRunId')=?1",
                    [&spec.model_run_id],
                    |row| row.get(0),
                )
                .map_err(ManagerError::from)
        })
        .unwrap();
    assert_eq!(persisted_events, 2);
}

#[test]
fn b03_transport_diagnostic_insert_failure_does_not_change_unknown_result_or_budget() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    seed_budget(&connection, "budget:diagnostic-insert-failure", 3, 3, 5);
    connection
        .execute_batch(
            "CREATE TRIGGER reject_transport_diagnostic_events BEFORE INSERT ON production_events
             WHEN NEW.event_type IN ('MODEL_REQUEST_STARTED', 'MODEL_TRANSPORT_RESULT')
             BEGIN SELECT RAISE(ABORT, 'diagnostic insert failure'); END;",
        )
        .unwrap();

    let mut transport = DiagnosticUnknownTransport {
        diagnostic: Some(ModelTransportDiagnostic {
            provider_send_started_at_unix_ms: Some(1_791_997_121_100),
            elapsed_ms: Some(1_250),
            timeout_ms: Some(30_000),
            client_timeout_ms: Some(30_000),
            phase: "response_body_read".into(),
            provider_error_code: Some("UNKNOWN_REMOTE_OUTCOME".into()),
            http_status: Some(200),
            response_read: "failed".into(),
            response_bytes: None,
            provider_request_id: None,
        }),
        sends: 0,
    };
    let spec = budgeted_model_spec(
        "mr:eeeeeeeeeeeeeeeeeeeeeeee",
        "budget:diagnostic-insert-failure",
        1,
    );

    assert!(matches!(
        DurableModelRunner::execute(&connection, &mut transport, &spec, NOW),
        Err(RuntimeError::UnknownRemoteOutcome)
    ));
    assert_eq!(transport.sends, 1);
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM jobs WHERE job_id=?1",
                [spec.job_id.as_deref().unwrap()],
                |row| row.get(0),
            )
            .unwrap(),
        "WAITING_DECISION"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT error_code FROM jobs WHERE job_id=?1",
                [spec.job_id.as_deref().unwrap()],
                |row| row.get(0),
            )
            .unwrap(),
        "UNKNOWN_REMOTE_OUTCOME"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM model_runs WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        "UNKNOWN_REMOTE_OUTCOME"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT outcome FROM decisions WHERE source_ref=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        "OPEN"
    );
    let reservation: (String, i64, i64) = connection
        .query_row(
            "SELECT status,reserved_cents,settled_cents FROM budget_reservations WHERE model_run_id=?1",
            [&spec.model_run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(reservation, ("OVER_LIMIT".into(), 1, 0));
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM production_events WHERE event_type IN ('MODEL_REQUEST_STARTED', 'MODEL_TRANSPORT_RESULT') AND json_extract(payload_json,'$.modelRunId')=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        0
    );
}

struct DiagnosticRawPersistFailureTransport {
    diagnostic: Option<ModelTransportDiagnostic>,
}

impl ModelTransport for DiagnosticRawPersistFailureTransport {
    fn diagnostics_enabled(&self) -> bool {
        true
    }

    fn diagnostic_timeout_ms(&self) -> Option<u64> {
        Some(1_000)
    }

    fn take_diagnostic(&mut self) -> Option<ModelTransportDiagnostic> {
        self.diagnostic.take()
    }

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
            raw_response: "private-response-body-canary".into(),
            usage: TransportUsage::default(),
        })
    }
}

#[test]
fn b03_transport_diagnostic_marks_raw_persist_failure_without_changing_error_code() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut transport = DiagnosticRawPersistFailureTransport {
        diagnostic: Some(ModelTransportDiagnostic {
            provider_send_started_at_unix_ms: Some(1_791_997_121_100),
            elapsed_ms: Some(18),
            timeout_ms: Some(1_000),
            client_timeout_ms: Some(1_000),
            phase: "response_received".into(),
            provider_error_code: None,
            http_status: Some(200),
            response_read: "complete".into(),
            response_bytes: Some(30),
            provider_request_id: Some("fixture-request-raw-persist".into()),
        }),
    };
    let spec = model_spec();

    assert!(matches!(
        DurableModelRunner::execute(&connection, &mut transport, &spec, NOW),
        Err(RuntimeError::RawResponsePersistenceFailed)
    ));
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT error_code FROM jobs WHERE job_id=?1",
                [spec.job_id.as_deref().unwrap()],
                |row| row.get(0),
            )
            .unwrap(),
        "RAW_RESPONSE_PERSIST_FAILED"
    );
    let payload: String = connection
        .query_row(
            "SELECT payload_json FROM production_events WHERE event_type='MODEL_TRANSPORT_RESULT' AND json_extract(payload_json,'$.modelRunId')=?1",
            [&spec.model_run_id],
            |row| row.get(0),
        )
        .unwrap();
    let payload: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["diagnostic"]["httpStatus"], 200);
    assert_eq!(payload["diagnostic"]["rawResponsePersisted"], false);
    assert_eq!(
        payload["diagnostic"]["persistenceFailureCode"],
        "RAW_RESPONSE_PERSIST_FAILED"
    );
    assert!(!payload.to_string().contains("private-response-body-canary"));
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_responses WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        0
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
    let mut next_spec = model_spec();
    next_spec.model_run_id = "mr:121212121212121212121212".into();
    next_spec.idempotency_key = "model-key:explicit-retry".into();
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
    let (temp, _manager, connection) = project();
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
    let db_path = temp.path().join("projects/p1/project.db");
    drop(connection);
    let db_bytes = std::fs::read(&db_path).unwrap();
    assert!(!String::from_utf8_lossy(&db_bytes).contains("sk-secret-b03"));
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
    let mut third = second.clone();
    third.model_run_id = "mr:202020202020202020202020".into();
    third.idempotency_key = "model-key:unknown-third".into();
    third.attempt_no = 2;
    let mut third_transport = FakeModelTransport::success(json!({"draft":"after-resolve"}));
    DurableModelRunner::execute(&connection, &mut third_transport, &third, NOW).unwrap();
    assert_eq!(third_transport.send_calls(), 1);
    assert_eq!(connection.query_row::<String, _, _>("SELECT status FROM budget_reservations WHERE model_run_id='mr:131313131313131313131313'", [], |row| row.get(0)).unwrap(), "SETTLED");
}

#[test]
fn b03_explicit_unknown_retry_rejects_missing_original_budget_binding() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    seed_budget(&connection, "budget:unknown-required", 3, 0, 100);
    let first = budgeted_model_spec("mr:181818181818181818181818", "budget:unknown-required", 5);
    let mut timeout_transport = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(DurableModelRunner::execute(&connection, &mut timeout_transport, &first, NOW).is_err());
    let mut second = model_spec();
    second.model_run_id = "mr:191919191919191919191919".into();
    second.idempotency_key = "model-key:missing-budget".into();
    second.attempt_no = 1;
    let mut retry_transport = FakeModelTransport::success(json!({"draft":"must-not-send"}));
    assert!(matches!(
        DurableModelRunner::resolve_unknown(
            &connection,
            &first.model_run_id,
            "RETRY_REMOTE",
            &mut retry_transport,
            &second,
            NOW,
        ),
        Err(RuntimeError::BudgetBindingMismatch)
    ));
    assert_eq!(retry_transport.send_calls(), 0);
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
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT block_reason FROM project_runtime_state WHERE project_id='p1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "BUDGET_BLOCKED"
    );
}

#[test]
fn b03_unknown_remote_reservation_counts_against_project_monthly_budget() {
    let (_temp, _manager, connection) = project();
    connection
        .execute(
            "INSERT INTO project_settings(project_id,setting_key,setting_value,updated_at) VALUES ('p1','monthlyBudget','1.0',?1)",
            [NOW],
        )
        .unwrap();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableJobService::transition(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "RUNNING",
        NOW,
    )
    .unwrap();
    let snapshot = ai_novel_production_system_core::model_cost::ModelCostSnapshot {
        price_table_version: 1,
        provider_id: "fake".into(),
        model_id: "fake-model".into(),
        price_status: "PRICED".into(),
        input_price_per_million_tokens: Some(1.0),
        output_price_per_million_tokens: Some(1.0),
        cached_input_price_per_million_tokens: None,
        estimated_input_tokens: 50,
        estimated_output_tokens: 50,
        predicted_cost_cents: 60,
    };
    let mut first_spec = model_spec();
    first_spec.model_run_id = "mr:121212121212121212121212".into();
    first_spec.idempotency_key = "model-key:monthly:unknown:1".into();
    first_spec.budget_id = Some(DurableModelRunner::project_budget_id("p1"));
    first_spec.predicted_cost_cents = snapshot.predicted_cost_cents;
    first_spec.cost_snapshot = Some(snapshot.clone());
    let mut first_transport = FakeModelTransport::failure(TransportFailure::UnknownRemoteOutcome);
    assert!(matches!(
        DurableModelRunner::execute(&connection, &mut first_transport, &first_spec, NOW),
        Err(RuntimeError::UnknownRemoteOutcome)
    ));
    assert_eq!(first_transport.send_calls(), 1);

    let mut second_spec = first_spec.clone();
    second_spec.model_run_id = "mr:131313131313131313131313".into();
    second_spec.idempotency_key = "model-key:monthly:unknown:2".into();
    second_spec.predicted_cost_cents = 50;
    second_spec
        .cost_snapshot
        .as_mut()
        .unwrap()
        .predicted_cost_cents = 50;
    let mut second_transport = FakeModelTransport::success(json!({"draft":"must not send"}));
    assert!(matches!(
        DurableModelRunner::execute(&connection, &mut second_transport, &second_spec, NOW),
        Err(RuntimeError::BudgetHardLimit)
    ));
    assert_eq!(second_transport.send_calls(), 0);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_runs WHERE project_id='p1'",
                [],
                |row| row.get(0),
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
    for _ in 0..3 {
        DurableModelRunner::retry(&connection, &mut retry_transport, &model_spec(), NOW).unwrap();
    }
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
    DurableJobService::transition_for_worker(
        &connection,
        "p1",
        "job:222222222222222222222222",
        "RUNNING",
        "old-worker",
        "2026-08-27T00:00:01Z",
    )
    .unwrap();
    DurableJobService::recover_after_restart(&connection, "p1", "2026-08-27T00:02:00Z").unwrap();
    DurableJobService::resume(
        &connection,
        "p1",
        "job:222222222222222222222222",
        "runtime-worker",
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
        "runtime-worker"
    );
}

#[test]
fn b03_resume_kernel_invariant_acquires_lease_without_command_wrapper() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableJobService::resume(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "runtime-worker",
        NOW,
    )
    .unwrap();
    assert_eq!(connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM job_leases WHERE job_id='job:111111111111111111111111' AND released_at IS NULL", [], |row| row.get(0)).unwrap(), 1);
}

#[test]
fn b03_unknown_chain_resolves_each_old_reservation_at_decision_time() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    seed_budget(&connection, "budget:unknown-chain", 6, 0, 100);
    let run1 = budgeted_model_spec("mr:303030303030303030303030", "budget:unknown-chain", 5);
    let mut timeout1 = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(DurableModelRunner::execute(&connection, &mut timeout1, &run1, NOW).is_err());
    let run2 = budgeted_model_spec("mr:313131313131313131313131", "budget:unknown-chain", 5);
    let mut timeout2 = FakeModelTransport::failure(TransportFailure::Timeout);
    DurableModelRunner::resolve_unknown(
        &connection,
        &run1.model_run_id,
        "RETRY_REMOTE",
        &mut timeout2,
        &run2,
        NOW,
    )
    .unwrap_err();
    assert_eq!(connection.query_row::<String, _, _>("SELECT status FROM budget_reservations WHERE model_run_id='mr:303030303030303030303030'", [], |row| row.get(0)).unwrap(), "SETTLED");
    let run3 = budgeted_model_spec("mr:323232323232323232323232", "budget:unknown-chain", 5);
    let mut success3 = FakeModelTransport::success(json!({"draft":"chain-success"}));
    DurableModelRunner::resolve_unknown(
        &connection,
        &run2.model_run_id,
        "RETRY_REMOTE",
        &mut success3,
        &run3,
        NOW,
    )
    .unwrap();
    assert_eq!(success3.send_calls(), 1);
    assert_eq!(connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM budget_reservations WHERE budget_ref='budget:unknown-chain' AND status='OVER_LIMIT'", [], |row| row.get(0)).unwrap(), 0);
    assert_eq!(connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM decisions WHERE decision_type='UNKNOWN_REMOTE_OUTCOME' AND outcome IN ('OPEN','PENDING')", [], |row| row.get(0)).unwrap(), 0);
    let run4 = budgeted_model_spec("mr:333333333333333333333333", "budget:unknown-chain", 5);
    let mut success4 = FakeModelTransport::success(json!({"draft":"after-chain"}));
    DurableModelRunner::execute(&connection, &mut success4, &run4, NOW).unwrap();
    assert_eq!(success4.send_calls(), 1);
}

#[test]
fn b03_mark_failed_and_cancel_resolve_unknown_reservations_without_orphans() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    seed_budget(&connection, "budget:unknown-actions", 6, 0, 100);
    let first = budgeted_model_spec("mr:343434343434343434343434", "budget:unknown-actions", 5);
    let mut timeout = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(DurableModelRunner::execute(&connection, &mut timeout, &first, NOW).is_err());
    let second = budgeted_model_spec("mr:353535353535353535353535", "budget:unknown-actions", 5);
    let mut retry_timeout = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(DurableModelRunner::resolve_unknown(
        &connection,
        &first.model_run_id,
        "RETRY_REMOTE",
        &mut retry_timeout,
        &second,
        NOW
    )
    .is_err());
    DurableModelRunner::resolve_unknown::<FakeModelTransport>(
        &connection,
        &second.model_run_id,
        "MARK_FAILED",
        &mut FakeModelTransport::success(json!({})),
        &second,
        NOW,
    )
    .unwrap();
    assert_eq!(connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM budget_reservations WHERE budget_ref='budget:unknown-actions' AND status='OVER_LIMIT'", [], |row| row.get(0)).unwrap(), 0);
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT outcome FROM decisions WHERE source_ref='mr:353535353535353535353535'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "MARK_FAILED"
    );
}

#[test]
fn b03_cancel_job_clears_stale_projection_and_preserves_audit_after_restart() {
    let (_temp, manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let first = budgeted_model_spec("mr:363636363636363636363636", "budget:unknown-cancel", 5);
    seed_budget(&connection, "budget:unknown-cancel", 3, 0, 100);
    let mut timeout = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(DurableModelRunner::execute(&connection, &mut timeout, &first, NOW).is_err());
    connection
        .execute(
            "UPDATE jobs SET error_code='UNKNOWN_REMOTE_OUTCOME requires explicit decision' WHERE project_id='p1' AND job_id='job:111111111111111111111111'",
            [],
        )
        .unwrap();
    DurableModelRunner::resolve_unknown::<FakeModelTransport>(
        &connection,
        &first.model_run_id,
        "CANCEL_JOB",
        &mut FakeModelTransport::success(json!({})),
        &first,
        NOW,
    )
    .unwrap();
    assert_eq!(connection.query_row::<String, _, _>("SELECT status FROM budget_reservations WHERE model_run_id='mr:363636363636363636363636'", [], |row| row.get(0)).unwrap(), "SETTLED");
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM jobs WHERE job_id='job:111111111111111111111111'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "CANCELLED"
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM budget_reservations WHERE status='OVER_LIMIT'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<(String, Option<String>), _, _>(
                "SELECT run_state, block_reason FROM project_runtime_state WHERE project_id='p1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?))
            )
            .unwrap(),
        ("IDLE".into(), None)
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT error_code FROM jobs WHERE job_id='job:111111111111111111111111'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "UNKNOWN_REMOTE_OUTCOME requires explicit decision"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT reason FROM decisions WHERE source_ref='mr:363636363636363636363636'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "remote result is unknown"
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM production_events WHERE event_type='UNKNOWN_COST_SETTLED_ESTIMATE' AND json_extract(payload_json, '$.modelRunId')='mr:363636363636363636363636'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    drop(connection);

    let reopened = manager.open("p1", "A", NOW).unwrap();
    assert_eq!(
        reopened
            .query_row::<(String, Option<String>), _, _>(
                "SELECT run_state, block_reason FROM project_runtime_state WHERE project_id='p1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?))
            )
            .unwrap(),
        ("IDLE".into(), None)
    );
    DurableJobService::recover_after_restart(&reopened, "p1", "2026-08-27T00:03:00Z").unwrap();
    assert_eq!(
        reopened
            .query_row::<(String, Option<String>), _, _>(
                "SELECT run_state, block_reason FROM project_runtime_state WHERE project_id='p1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?))
            )
            .unwrap(),
        ("IDLE".into(), None)
    );
    assert_eq!(
        DurableJobService::reconcile_runtime_projection(&reopened, "p1", "2026-08-27T00:03:01Z")
            .unwrap(),
        "IDLE"
    );
    assert_eq!(
        reopened
            .query_row::<(String, Option<String>), _, _>(
                "SELECT run_state, block_reason FROM project_runtime_state WHERE project_id='p1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?))
            )
            .unwrap(),
        ("IDLE".into(), None)
    );
    assert_eq!(
        reopened
            .query_row::<String, _, _>(
                "SELECT reason FROM decisions WHERE source_ref='mr:363636363636363636363636'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "remote result is unknown"
    );
    assert_eq!(
        reopened
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM production_events WHERE event_type='UNKNOWN_COST_SETTLED_ESTIMATE' AND json_extract(payload_json, '$.modelRunId')='mr:363636363636363636363636'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn b03_cancelled_projection_keeps_unrelated_error_reason_visible() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableJobService::transition(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "CANCELLED",
        NOW,
    )
    .unwrap();
    connection
        .execute(
            "UPDATE jobs SET error_code='PERSISTENCE_FAILURE' WHERE project_id='p1' AND job_id='job:111111111111111111111111'",
            [],
        )
        .unwrap();

    DurableJobService::reconcile_runtime_projection(&connection, "p1", NOW).unwrap();

    assert_eq!(
        connection
            .query_row::<(String, Option<String>), _, _>(
                "SELECT run_state, block_reason FROM project_runtime_state WHERE project_id='p1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?))
            )
            .unwrap(),
        ("IDLE".into(), Some("PERSISTENCE_FAILURE".into()))
    );
}

#[test]
fn b03_concurrent_workers_have_one_lease_owner() {
    let (temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableJobService::transition(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "PAUSED",
        NOW,
    )
    .unwrap();
    let database = temp.path().join("projects/p1/project.db");
    drop(connection);
    let barrier = Arc::new(Barrier::new(2));
    let worker_a_barrier = Arc::clone(&barrier);
    let worker_a_database = database.clone();
    let worker_a = std::thread::spawn(move || {
        let connection = Connection::open(worker_a_database).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")
            .unwrap();
        worker_a_barrier.wait();
        DurableJobService::resume(
            &connection,
            "p1",
            "job:111111111111111111111111",
            "worker-a",
            NOW,
        )
    });
    let worker_b_barrier = Arc::clone(&barrier);
    let worker_b_database = database.clone();
    let worker_b = std::thread::spawn(move || {
        let connection = Connection::open(worker_b_database).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")
            .unwrap();
        worker_b_barrier.wait();
        DurableJobService::resume(
            &connection,
            "p1",
            "job:111111111111111111111111",
            "worker-b",
            NOW,
        )
    });
    let result_a = worker_a.join().unwrap();
    let result_b = worker_b.join().unwrap();
    let verifier = Connection::open(database).unwrap();
    let active_count: i64 = verifier
        .query_row(
            "SELECT COUNT(*) FROM job_leases WHERE project_id='p1' AND job_id='job:111111111111111111111111' AND released_at IS NULL AND expires_at>?1",
            [NOW],
            |row| row.get(0),
        )
        .unwrap();
    let holder: String = verifier
        .query_row(
            "SELECT holder FROM job_leases WHERE project_id='p1' AND job_id='job:111111111111111111111111' AND released_at IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let successes = [result_a.is_ok(), result_b.is_ok()]
        .into_iter()
        .filter(|success| *success)
        .count();
    assert_eq!(successes, 1);
    assert_eq!(active_count, 1);
    assert!(holder == "worker-a" || holder == "worker-b");
    println!(
        "B03_CONCURRENT_LEASE_RESULT successes={} active={} holder={}",
        successes, active_count, holder
    );
}

#[test]
fn b03_manual_resolve_is_not_advertised_or_accepted() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut timeout = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(DurableModelRunner::execute(&connection, &mut timeout, &model_spec(), NOW).is_err());
    let payload: String = connection
        .query_row(
            "SELECT payload_json FROM decisions WHERE decision_type='UNKNOWN_REMOTE_OUTCOME'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!payload.contains("MANUAL_RESOLVE"));
    assert!(matches!(
        DurableModelRunner::resolve_unknown::<FakeModelTransport>(
            &connection,
            "mr:aaaaaaaaaaaaaaaaaaaaaaaa",
            "MANUAL_RESOLVE",
            &mut FakeModelTransport::success(json!({})),
            &model_spec(),
            NOW,
        ),
        Err(RuntimeError::UnknownRemoteOutcome)
    ));
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
}

#[test]
fn b03_resume_enforces_lease_holder_ownership_and_expired_reclaim() {
    let (_temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableJobService::transition(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "PAUSED",
        NOW,
    )
    .unwrap();
    DurableJobService::acquire_lease(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "worker-a",
        "2026-08-27T00:05:00Z",
        NOW,
    )
    .unwrap();
    assert!(matches!(
        DurableJobService::resume(
            &connection,
            "p1",
            "job:111111111111111111111111",
            "worker-b",
            "2026-08-27T00:01:00Z"
        ),
        Err(RuntimeError::LeaseHeld)
    ));
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM jobs WHERE job_id='job:111111111111111111111111'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "PAUSED"
    );
    DurableJobService::resume(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "worker-a",
        "2026-08-27T00:01:00Z",
    )
    .unwrap();
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT holder FROM job_leases WHERE job_id='job:111111111111111111111111'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "worker-a"
    );
    DurableJobService::pause(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "2026-08-27T00:01:00Z",
    )
    .unwrap();
    DurableJobService::acquire_lease(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "worker-a",
        "2026-08-27T00:02:00Z",
        "2026-08-27T00:01:00Z",
    )
    .unwrap();
    DurableJobService::resume(
        &connection,
        "p1",
        "job:111111111111111111111111",
        "worker-b",
        "2026-08-27T00:03:00Z",
    )
    .unwrap();
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT holder FROM job_leases WHERE job_id='job:111111111111111111111111'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        "worker-b"
    );
}

#[test]
fn b03_lease_expiry_advances_date_across_midnight_month_and_year() {
    assert_eq!(lease_expiry("2026-08-27T23:59:30Z"), "2026-08-28T00:00:30Z");
    assert_eq!(lease_expiry("2026-01-31T23:59:30Z"), "2026-02-01T00:00:30Z");
    assert_eq!(lease_expiry("2026-12-31T23:59:30Z"), "2027-01-01T00:00:30Z");
}

#[test]
fn b03_raw_recovery_closes_and_reopens_project_db_without_transport_or_cost_growth() {
    let (temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut transport = FakeModelTransport::raw_success("not-json");
    assert!(DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).is_err());
    let before = connection.query_row::<(i64, i64), _, _>("SELECT (SELECT COUNT(*) FROM model_cost_ledger WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'), (SELECT total_cost_cents FROM project_runtime_state WHERE project_id='p1')", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    drop(connection);
    let reopened_manager = ProjectManager::new(AppPaths::new(temp.path()));
    let reopened = reopened_manager.open("p1", "A", NOW).unwrap();
    let mut retry_transport = FakeModelTransport::failure(TransportFailure::Timeout);
    assert!(
        DurableModelRunner::retry(&reopened, &mut retry_transport, &model_spec(), NOW).is_err()
    );
    let after = reopened.query_row::<(i64, i64), _, _>("SELECT (SELECT COUNT(*) FROM model_cost_ledger WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'), (SELECT total_cost_cents FROM project_runtime_state WHERE project_id='p1')", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(retry_transport.send_calls(), 0);
    assert_eq!(before, after);
}

#[test]
fn b03_completed_recovery_closes_and_reopens_project_db_without_duplicate_settlement() {
    let (temp, _manager, connection) = project();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    let mut transport = FakeModelTransport::success(json!({"draft":"completed-reopen"}));
    DurableModelRunner::execute(&connection, &mut transport, &model_spec(), NOW).unwrap();
    let before = connection.query_row::<(i64, i64, i64), _, _>("SELECT (SELECT COUNT(*) FROM model_usage WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'), (SELECT COUNT(*) FROM model_cost_ledger WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa' AND cost_type='ACTUAL'), (SELECT settled_cents FROM budget_reservations WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa')", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    drop(connection);
    let reopened_manager = ProjectManager::new(AppPaths::new(temp.path()));
    let reopened = reopened_manager.open("p1", "A", NOW).unwrap();
    DurableJobService::recover_after_restart(&reopened, "p1", "2026-08-27T00:03:00Z").unwrap();
    let mut retry_transport = FakeModelTransport::failure(TransportFailure::Timeout);
    DurableModelRunner::execute(&reopened, &mut retry_transport, &model_spec(), NOW).unwrap();
    let after = reopened.query_row::<(i64, i64, i64), _, _>("SELECT (SELECT COUNT(*) FROM model_usage WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa'), (SELECT COUNT(*) FROM model_cost_ledger WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa' AND cost_type='ACTUAL'), (SELECT settled_cents FROM budget_reservations WHERE model_run_id='mr:aaaaaaaaaaaaaaaaaaaaaaaa')", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!(retry_transport.send_calls(), 0);
    assert_eq!(before, after);
}

#[test]
fn b03_secret_store_keeps_secret_out_of_project_database() {
    let (temp, _manager, connection) = project();
    let store = MemorySecretStore::new();
    store.put("fake", "sk-secret-b03").unwrap();
    assert_eq!(store.get("fake").unwrap(), "sk-secret-b03");
    let db_path = temp.path().join("projects/p1/project.db");
    drop(connection);
    let db_bytes = std::fs::read(db_path).unwrap();
    assert!(!String::from_utf8_lossy(&db_bytes).contains("sk-secret-b03"));
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
                "requestedAt": NOW,
                "payload":{"jobId":job_spec().job_id}
            }),
        )
        .unwrap()
    };
    let connection = state.manager.open("p1", "A", NOW).unwrap();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableJobService::transition(&connection, "p1", &job_spec().job_id, "RUNNING", NOW).unwrap();
    DurableJobService::set_runtime_state(&connection, "p1", "ACTIVE", None, NOW).unwrap();
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
        command("b03-pause", "production_pause", 0)["finalStatus"],
        "committed"
    );
    assert_eq!(
        command("b03-resume", "production_resume", 1)["finalStatus"],
        "committed"
    );
    assert_eq!(
        command("b03-cancel", "production_cancel", 2)["finalStatus"],
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

#[test]
fn b03_control_receipt_replay_keeps_original_result_after_job_changes() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    drop(state.manager.create("p1", "A", NOW).unwrap());
    let connection = state.manager.open("p1", "A", NOW).unwrap();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableJobService::transition(&connection, "p1", &job_spec().job_id, "RUNNING", NOW).unwrap();
    DurableJobService::set_runtime_state(&connection, "p1", "ACTIVE", None, NOW).unwrap();
    drop(connection);

    let command = |command_id: &str, command_type: &str, revision: i64| {
        execute_command_impl(
            &state,
            json!({
                "type":command_type,
                "commandId":command_id,
                "idempotencyKey":format!("{command_id}-key"),
                "projectId":"p1",
                "expectedRevision":revision,
                "requestedAt":NOW,
                "payload":{"jobId":job_spec().job_id}
            }),
        )
        .unwrap()
    };

    let pause = command("b03-stable-pause", "production_pause", 0);
    assert_eq!(pause["actionStatus"], "PRODUCTION_PAUSED");
    let pause_receipt = state
        .manager
        .find_project_command_receipt("p1", "b03-stable-pause-key")
        .unwrap()
        .unwrap();
    let pause_receipt_json = serde_json::to_value(pause_receipt).unwrap();
    assert_eq!(
        pause_receipt_json["controlResult"]["jobId"],
        job_spec().job_id
    );
    assert_eq!(pause_receipt_json["controlResult"]["applied"], true);
    assert_eq!(
        pause_receipt_json["controlResult"]["actionStatus"],
        "PRODUCTION_PAUSED"
    );
    command("b03-stable-resume", "production_resume", 1);
    command("b03-stable-cancel", "production_cancel", 2);
    let pause_replay = command("b03-stable-pause", "production_pause", 0);

    assert_eq!(pause_replay["receiptId"], pause["receiptId"]);
    assert_eq!(pause_replay["actionStatus"], pause["actionStatus"]);
    assert_eq!(pause_replay["message"], pause["message"]);
}

#[test]
fn b03_old_project_command_receipt_without_control_result_keeps_its_digest() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "A", NOW).unwrap();
    let request_digest = "a".repeat(64);
    let spec = ai_novel_production_system_core::project_db::ProjectCommandSpec {
        project_id: "p1",
        result_project_id: "p1",
        command_id: "cmd:v1-receipt",
        idempotency_key: "key:v1-receipt",
        command_type: "project_rename",
        request_digest: &request_digest,
        expected_revision: None,
        now: NOW,
        advance_revision: false,
    };
    let outcome = ai_novel_production_system_core::commit::CommitService::commit_project_command(
        &connection,
        spec,
        |_| Ok(()),
    )
    .unwrap();
    drop(connection);

    let receipt = match outcome {
        ai_novel_production_system_core::commit::ProjectCommandOutcome::Committed {
            receipt,
            ..
        } => receipt,
        ai_novel_production_system_core::commit::ProjectCommandOutcome::Replayed(_) => {
            panic!("first invocation must commit")
        }
    };
    let serialized = serde_json::to_value(&receipt).unwrap();
    assert!(serialized.get("controlResult").is_none());
    let replay = manager.commit_project_command(spec, |_| Ok(())).unwrap();
    let replay_receipt = match replay {
        ai_novel_production_system_core::commit::ProjectCommandOutcome::Replayed(receipt) => {
            receipt
        }
        ai_novel_production_system_core::commit::ProjectCommandOutcome::Committed { .. } => {
            panic!("the old receipt must be replayed")
        }
    };
    assert!(serde_json::to_value(&replay_receipt)
        .unwrap()
        .get("controlResult")
        .is_none());
    assert_eq!(replay_receipt.receipt_digest, receipt.receipt_digest);

    let connection = manager.open("p1", "A", NOW).unwrap();
    let receipt_json: String = connection
        .query_row(
            "SELECT receipt_json FROM project_command_receipts WHERE project_id='p1' AND idempotency_key='key:v1-receipt'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let mut legacy_json: Value = serde_json::from_str(&receipt_json).unwrap();
    legacy_json["controlResult"] = Value::Null;
    connection
        .execute(
            "UPDATE project_command_receipts SET receipt_json=?1 WHERE project_id='p1' AND idempotency_key='key:v1-receipt'",
            [legacy_json.to_string()],
        )
        .unwrap();
    drop(connection);
    assert!(matches!(
        manager.find_project_command_receipt("p1", "key:v1-receipt"),
        Err(ManagerError::ProjectCommandReceiptDigestMismatch)
    ));
}

#[test]
fn b03_tampering_with_control_result_fails_receipt_digest_verification() {
    let temp = tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    drop(state.manager.create("p1", "A", NOW).unwrap());
    let connection = state.manager.open("p1", "A", NOW).unwrap();
    DurableJobService::start_job(&connection, &job_spec(), NOW).unwrap();
    DurableJobService::transition(&connection, "p1", &job_spec().job_id, "RUNNING", NOW).unwrap();
    DurableJobService::set_runtime_state(&connection, "p1", "ACTIVE", None, NOW).unwrap();
    drop(connection);

    execute_command_impl(
        &state,
        json!({
            "type":"production_pause",
            "commandId":"b03-tamper-pause",
            "idempotencyKey":"b03-tamper-pause-key",
            "projectId":"p1",
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{"jobId":job_spec().job_id}
        }),
    )
    .unwrap();
    let connection = state.manager.open("p1", "A", NOW).unwrap();
    let receipt_json: String = connection
        .query_row(
            "SELECT receipt_json FROM project_command_receipts WHERE project_id='p1' AND idempotency_key='b03-tamper-pause-key'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let mut receipt: Value = serde_json::from_str(&receipt_json).unwrap();
    receipt["controlResult"]["applied"] = json!(false);
    receipt["controlResult"]["actionStatus"] = json!("PRODUCTION_CONTROL_NOT_ACCEPTED");
    receipt["controlResult"]["message"] = json!("原控制结果被篡改");
    connection
        .execute(
            "UPDATE project_command_receipts SET receipt_json=?1 WHERE project_id='p1' AND idempotency_key='b03-tamper-pause-key'",
            [receipt.to_string()],
        )
        .unwrap();
    drop(connection);

    assert!(matches!(
        state
            .manager
            .find_project_command_receipt("p1", "b03-tamper-pause-key"),
        Err(ManagerError::ProjectCommandReceiptDigestMismatch)
    ));
}
