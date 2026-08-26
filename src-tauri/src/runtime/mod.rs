use libsql_rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("runtime project not found")]
    ProjectNotFound,
    #[error("job not found")]
    JobNotFound,
    #[error("invalid job transition: {0} -> {1}")]
    InvalidTransition(String, String),
    #[error("job lease already held")]
    LeaseHeld,
    #[error("UNKNOWN_REMOTE_OUTCOME requires explicit decision")]
    UnknownRemoteOutcome,
    #[error("budget hard limit")]
    BudgetHardLimit,
    #[error("secret unavailable")]
    SecretUnavailable,
    #[error("transport: {0}")]
    Transport(#[from] TransportFailure),
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum TransportFailure {
    #[error("timeout")]
    Timeout,
    #[error("unavailable")]
    Unavailable,
}

#[derive(Debug, Clone)]
pub struct JobSpec {
    pub job_id: String,
    pub job_type: String,
    pub book_id: Option<String>,
    pub volume_id: Option<String>,
    pub chapter_id: Option<String>,
    pub stages: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RecoverySummary {
    pub recovered_jobs: i64,
}

pub struct DurableJobService;

impl DurableJobService {
    pub fn start_job(
        connection: &Connection,
        spec: &JobSpec,
        now: &str,
    ) -> Result<(), RuntimeError> {
        if !matches!(spec.job_type.as_str(), "BOOK" | "VOLUME" | "CHAPTER") {
            return Err(RuntimeError::InvalidTransition(
                "UNKNOWN".into(),
                "job_type".into(),
            ));
        }
        let project_id: String =
            connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
                row.get(0)
            })?;
        connection.execute(
            "INSERT INTO jobs(job_id, project_id, job_type, book_id, volume_id, chapter_id, status, current_stage, attempt_no, repair_round, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'QUEUED', ?7, 0, 0, ?8, ?8) ON CONFLICT(job_id) DO UPDATE SET updated_at=excluded.updated_at",
            libsql_rusqlite::params![spec.job_id, project_id, spec.job_type, spec.book_id, spec.volume_id, spec.chapter_id, spec.stages.first(), now],
        )?;
        for (index, stage) in spec.stages.iter().enumerate() {
            connection.execute(
                "INSERT INTO job_steps(job_step_id, project_id, job_id, step_no, step_type, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'QUEUED', ?6) ON CONFLICT(project_id, job_id, step_no) DO UPDATE SET step_type=excluded.step_type",
                libsql_rusqlite::params![format!("{}:step:{}", spec.job_id, index + 1), project_id, spec.job_id, index as i64 + 1, stage, now],
            )?;
        }
        Ok(())
    }

    pub fn transition(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        next: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        let current: String = connection
            .query_row(
                "SELECT status FROM jobs WHERE project_id = ?1 AND job_id = ?2",
                [project_id, job_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(RuntimeError::JobNotFound)?;
        if !transition_allowed(&current, next) {
            return Err(RuntimeError::InvalidTransition(current, next.to_string()));
        }
        connection.execute(
            "UPDATE jobs SET status = ?1, updated_at = ?2 WHERE project_id = ?3 AND job_id = ?4",
            libsql_rusqlite::params![next, now, project_id, job_id],
        )?;
        Ok(())
    }

    pub fn acquire_lease(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        holder: &str,
        expires_at: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        let existing: Option<(String, Option<String>)> = connection
            .query_row(
                "SELECT expires_at, released_at FROM job_leases WHERE project_id = ?1 AND job_id = ?2",
                [project_id, job_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((existing_expiry, released_at)) = existing {
            if released_at.is_none() && existing_expiry.as_str() > now {
                return Err(RuntimeError::LeaseHeld);
            }
        }
        connection.execute(
            "INSERT INTO job_leases(lease_id, project_id, job_id, holder, expires_at, acquired_at, released_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL) ON CONFLICT(project_id, job_id) DO UPDATE SET holder=excluded.holder, expires_at=excluded.expires_at, acquired_at=excluded.acquired_at, released_at=NULL",
            libsql_rusqlite::params![format!("lease:{}", job_id), project_id, job_id, holder, expires_at, now],
        )?;
        Ok(())
    }

    pub fn save_checkpoint(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        stage: &str,
        offset: i64,
        boundary: &Value,
        now: &str,
    ) -> Result<(), RuntimeError> {
        connection.execute(
            "INSERT INTO job_checkpoints(checkpoint_id, project_id, job_id, stage, offset, boundary_json, saved_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(project_id, job_id, stage) DO UPDATE SET offset=excluded.offset, boundary_json=excluded.boundary_json, saved_at=excluded.saved_at",
            libsql_rusqlite::params![format!("checkpoint:{}:{}", job_id, stage), project_id, job_id, stage, offset, serde_json::to_string(boundary)?, now],
        )?;
        Ok(())
    }

    pub fn pause(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        Self::transition(connection, project_id, job_id, "PAUSED", now)
    }

    pub fn resume(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        Self::transition(connection, project_id, job_id, "RUNNING", now)
    }

    pub fn cancel(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        Self::transition(connection, project_id, job_id, "CANCELLED", now)
    }

    pub fn latest_job_id(
        connection: &Connection,
        project_id: &str,
    ) -> Result<String, RuntimeError> {
        connection
            .query_row(
                "SELECT job_id FROM jobs WHERE project_id = ?1 ORDER BY updated_at DESC, job_id DESC LIMIT 1",
                [project_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(RuntimeError::JobNotFound)
    }

    pub fn set_runtime_state(
        connection: &Connection,
        project_id: &str,
        run_state: &str,
        block_reason: Option<&str>,
        now: &str,
    ) -> Result<(), RuntimeError> {
        connection.execute(
            "INSERT INTO project_runtime_state(project_id, run_state, block_reason, updated_at) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(project_id) DO UPDATE SET run_state=excluded.run_state, block_reason=excluded.block_reason, updated_at=excluded.updated_at",
            libsql_rusqlite::params![project_id, run_state, block_reason, now],
        )?;
        Ok(())
    }

    pub fn recover_after_restart(
        connection: &Connection,
        project_id: &str,
        now: &str,
    ) -> Result<RecoverySummary, RuntimeError> {
        let mut statement = connection.prepare(
            "SELECT job_id FROM jobs WHERE project_id = ?1 AND status IN ('RUNNING','WAITING_MODEL')",
        )?;
        let jobs = statement
            .query_map([project_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for job_id in &jobs {
            connection.execute(
                "UPDATE jobs SET status='PAUSED', error_code='RECOVERED_AFTER_RESTART', updated_at=?1 WHERE project_id=?2 AND job_id=?3",
                libsql_rusqlite::params![now, project_id, job_id],
            )?;
            connection.execute(
                "INSERT OR IGNORE INTO production_events(event_id, project_id, job_id, event_type, payload_json, created_at) VALUES (?1, ?2, ?3, 'JOB_RECOVERED_AFTER_RESTART', '{}', ?4)",
                libsql_rusqlite::params![format!("recovery:{}:{}", job_id, now), project_id, job_id, now],
            )?;
        }
        Ok(RecoverySummary {
            recovered_jobs: jobs.len() as i64,
        })
    }
}

fn transition_allowed(current: &str, next: &str) -> bool {
    matches!(
        (current, next),
        ("QUEUED", "RUNNING" | "PAUSED" | "CANCELLED")
            | (
                "RUNNING",
                "WAITING_MODEL" | "PAUSING" | "PAUSED" | "FAILED" | "CANCELLED" | "COMPLETED"
            )
            | (
                "WAITING_MODEL",
                "RUNNING" | "PAUSED" | "FAILED" | "CANCELLED"
            )
            | ("PAUSING", "PAUSED" | "CANCELLED")
            | ("PAUSED", "RUNNING" | "CANCELLED")
            | ("WAITING_DECISION", "RUNNING" | "PAUSED" | "CANCELLED")
            | ("WAITING_BUDGET", "RUNNING" | "PAUSED" | "CANCELLED")
    )
}

#[derive(Debug, Clone)]
pub struct ModelRunSpec {
    pub model_run_id: String,
    pub job_id: Option<String>,
    pub step_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub idempotency_key: String,
    pub capability: String,
    pub prompt_ref: Option<String>,
    pub context_manifest_ref: Option<String>,
    pub budget_id: Option<String>,
    pub predicted_cost_cents: i64,
    pub attempt_no: i64,
    pub repair_round: i64,
}

#[derive(Debug, Clone)]
pub struct ModelRunOutcome {
    pub status: String,
    pub parsed: Option<Value>,
    pub replayed: bool,
}

pub trait ModelTransport {
    fn send(&mut self, spec: &ModelRunSpec) -> Result<String, TransportFailure>;
}

pub struct FakeModelTransport {
    response: Result<String, TransportFailure>,
    calls: usize,
}

impl FakeModelTransport {
    pub fn success(value: Value) -> Self {
        Self {
            response: Ok(value.to_string()),
            calls: 0,
        }
    }

    pub fn failure(error: TransportFailure) -> Self {
        Self {
            response: Err(error),
            calls: 0,
        }
    }

    pub fn raw_success(raw: impl Into<String>) -> Self {
        Self {
            response: Ok(raw.into()),
            calls: 0,
        }
    }

    pub fn send_calls(&self) -> usize {
        self.calls
    }
}

impl ModelTransport for FakeModelTransport {
    fn send(&mut self, _spec: &ModelRunSpec) -> Result<String, TransportFailure> {
        self.calls += 1;
        self.response.clone()
    }
}

pub struct DurableModelRunner;

impl DurableModelRunner {
    pub fn execute<T: ModelTransport>(
        connection: &Connection,
        transport: &mut T,
        spec: &ModelRunSpec,
        now: &str,
    ) -> Result<ModelRunOutcome, RuntimeError> {
        if let Some(status) = existing_model_status(connection, spec)? {
            if status == "COMPLETED" {
                return Ok(ModelRunOutcome {
                    status,
                    parsed: None,
                    replayed: true,
                });
            }
            if status == "UNKNOWN_REMOTE_OUTCOME" {
                return Err(RuntimeError::UnknownRemoteOutcome);
            }
        }
        enforce_budget(connection, spec)?;
        persist_before_send(connection, spec, now)?;
        let raw = match transport.send(spec) {
            Ok(raw) => raw,
            Err(TransportFailure::Timeout) => {
                connection.execute(
                    "UPDATE model_runs SET status='UNKNOWN_REMOTE_OUTCOME', updated_at=?1 WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND model_run_id=?2",
                    libsql_rusqlite::params![now, spec.model_run_id],
                )?;
                return Err(RuntimeError::UnknownRemoteOutcome);
            }
            Err(error) => {
                connection.execute(
                    "UPDATE model_runs SET status='FAILED', updated_at=?1 WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND model_run_id=?2",
                    libsql_rusqlite::params![now, spec.model_run_id],
                )?;
                return Err(RuntimeError::Transport(error));
            }
        };
        persist_raw_response(connection, spec, &raw, now)?;
        finish_parse(connection, spec, &raw, now)
    }

    pub fn retry<T: ModelTransport>(
        connection: &Connection,
        transport: &mut T,
        spec: &ModelRunSpec,
        now: &str,
    ) -> Result<ModelRunOutcome, RuntimeError> {
        let status = existing_model_status(connection, spec)?;
        if status.as_deref() == Some("UNKNOWN_REMOTE_OUTCOME") {
            return Err(RuntimeError::UnknownRemoteOutcome);
        }
        let raw: Option<String> = connection
            .query_row(
                "SELECT raw_response FROM model_responses WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(raw) = raw {
            return finish_parse(connection, spec, &raw, now);
        }
        Self::execute(connection, transport, spec, now)
    }
}

fn existing_model_status(
    connection: &Connection,
    spec: &ModelRunSpec,
) -> Result<Option<String>, RuntimeError> {
    Ok(connection
        .query_row(
            "SELECT status FROM model_runs WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND (model_run_id=?1 OR idempotency_key=?2)",
            [&spec.model_run_id, &spec.idempotency_key],
            |row| row.get(0),
        )
        .optional()?)
}

fn enforce_budget(connection: &Connection, spec: &ModelRunSpec) -> Result<(), RuntimeError> {
    let Some(budget_id) = &spec.budget_id else {
        return Ok(());
    };
    let Some((hard_max, cost_budget)) = connection
        .query_row(
            "SELECT hard_max_model_runs, cost_budget_cents FROM chapter_production_budgets WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND budget_id=?1",
            [budget_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
    else {
        return Err(RuntimeError::BudgetHardLimit);
    };
    let over_limit: i64 = connection.query_row(
        "SELECT COUNT(*) FROM budget_reservations WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND budget_ref=?1 AND status='OVER_LIMIT'",
        [budget_id],
        |row| row.get(0),
    )?;
    if over_limit > 0 {
        record_budget_decision(connection, budget_id)?;
        return Err(RuntimeError::BudgetHardLimit);
    }
    let reserved: i64 = connection.query_row(
        "SELECT COUNT(*) FROM budget_reservations WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND budget_ref=?1 AND status='RESERVED'",
        [budget_id],
        |row| row.get(0),
    )?;
    if hard_max > 0 && reserved >= hard_max {
        record_budget_decision(connection, budget_id)?;
        return Err(RuntimeError::BudgetHardLimit);
    }
    if cost_budget > 0 && spec.predicted_cost_cents >= cost_budget {
        record_budget_decision(connection, budget_id)?;
        return Err(RuntimeError::BudgetHardLimit);
    }
    Ok(())
}

fn record_budget_decision(connection: &Connection, budget_id: &str) -> Result<(), RuntimeError> {
    let project_id: String =
        connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
            row.get(0)
        })?;
    connection.execute(
        "INSERT INTO decisions(decision_id, project_id, decision_type, outcome, reason, payload_json, created_at, updated_at) VALUES (?1, ?2, 'BUDGET_REPLAN', 'REPLAN_REQUIRED', 'hard budget reached', ?3, datetime('now'), datetime('now')) ON CONFLICT(decision_id) DO NOTHING",
        libsql_rusqlite::params![format!("decision:budget:{budget_id}"), project_id, serde_json::json!({"budgetId":budget_id}).to_string()],
    )?;
    Ok(())
}

fn persist_before_send(
    connection: &Connection,
    spec: &ModelRunSpec,
    now: &str,
) -> Result<(), RuntimeError> {
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        let project_id: String =
            connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
                row.get(0)
            })?;
        let step_id = spec.step_id.as_ref().filter(|step_id| {
            connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM job_steps WHERE project_id=?1 AND job_step_id=?2)",
                    [&project_id, step_id],
                    |row| row.get(0),
                )
                .unwrap_or(false)
        });
        connection.execute(
            "INSERT INTO model_runs(model_run_id, project_id, step_id, job_id, prompt_ref, context_manifest_ref, provider, model, idempotency_key, capability, attempt_no, repair_round, predicted_cost_cents, reserved_cost_cents, budget_reservation_id, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13, ?14, 'PREPARED', ?15, ?15)",
            libsql_rusqlite::params![spec.model_run_id, project_id, step_id, spec.job_id, spec.prompt_ref, spec.context_manifest_ref, spec.provider, spec.model, spec.idempotency_key, spec.capability, spec.attempt_no, spec.repair_round, spec.predicted_cost_cents, format!("reservation:{}", spec.model_run_id), now],
        )?;
        connection.execute(
            "INSERT INTO budget_reservations(reservation_id, project_id, model_run_id, budget_ref, reserved_cents, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'RESERVED', ?6)",
            libsql_rusqlite::params![format!("reservation:{}", spec.model_run_id), project_id, spec.model_run_id, spec.budget_id.as_deref().unwrap_or("UNSCOPED"), spec.predicted_cost_cents, now],
        )?;
        Ok::<_, RuntimeError>(())
    })();
    match result {
        Ok(()) => connection.execute_batch("COMMIT")?,
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    Ok(())
}

fn persist_raw_response(
    connection: &Connection,
    spec: &ModelRunSpec,
    raw: &str,
    now: &str,
) -> Result<(), RuntimeError> {
    let digest = hex::encode(Sha256::digest(raw.as_bytes()));
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = connection.execute(
        "INSERT INTO model_responses(response_id, project_id, model_run_id, raw_response, response_digest, received_at) VALUES (?1, (SELECT project_id FROM project_meta LIMIT 1), ?2, ?3, ?4, ?5)",
        libsql_rusqlite::params![format!("response:{}", spec.model_run_id), spec.model_run_id, raw, digest, now],
    );
    match result {
        Ok(_) => connection.execute_batch("COMMIT")?,
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            return Err(RuntimeError::Sqlite(error));
        }
    }
    Ok(())
}

fn finish_parse(
    connection: &Connection,
    spec: &ModelRunSpec,
    raw: &str,
    now: &str,
) -> Result<ModelRunOutcome, RuntimeError> {
    let parsed = match serde_json::from_str::<Value>(raw) {
        Ok(value) => value,
        Err(error) => {
            connection.execute(
                "INSERT INTO model_parse_results(parse_id, project_id, model_run_id, status, error_message, created_at) VALUES (?1, (SELECT project_id FROM project_meta LIMIT 1), ?2, 'SCHEMA_INVALID', ?3, ?4) ON CONFLICT(parse_id) DO UPDATE SET status='SCHEMA_INVALID', error_message=excluded.error_message, created_at=excluded.created_at",
                libsql_rusqlite::params![format!("parse:{}", spec.model_run_id), spec.model_run_id, error.to_string(), now],
            )?;
            connection.execute(
                "UPDATE model_runs SET status='FAILED', updated_at=?1 WHERE model_run_id=?2",
                libsql_rusqlite::params![now, spec.model_run_id],
            )?;
            return Err(RuntimeError::Json(error));
        }
    };
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        connection.execute(
            "INSERT INTO model_parse_results(parse_id, project_id, model_run_id, status, parsed_json, created_at) VALUES (?1, (SELECT project_id FROM project_meta LIMIT 1), ?2, 'OK', ?3, ?4) ON CONFLICT(parse_id) DO NOTHING",
            libsql_rusqlite::params![format!("parse:{}", spec.model_run_id), spec.model_run_id, parsed.to_string(), now],
        )?;
        connection.execute(
            "UPDATE model_runs SET status='COMPLETED', updated_at=?1 WHERE model_run_id=?2",
            libsql_rusqlite::params![now, spec.model_run_id],
        )?;
        connection.execute(
            "UPDATE budget_reservations SET status='SETTLED', settled_cents=reserved_cents, settled_at=?1 WHERE reservation_id=?2",
            libsql_rusqlite::params![now, format!("reservation:{}", spec.model_run_id)],
        )?;
        if let Some(step_id) = &spec.step_id {
            connection.execute(
                "UPDATE job_steps SET status='COMPLETED', finished_at=?1, model_run_id=?2 WHERE job_step_id=?3",
                libsql_rusqlite::params![now, spec.model_run_id, step_id],
            )?;
        }
        Ok::<_, RuntimeError>(())
    })();
    match result {
        Ok(()) => connection.execute_batch("COMMIT")?,
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    Ok(ModelRunOutcome {
        status: "COMPLETED".into(),
        parsed: Some(parsed),
        replayed: false,
    })
}

pub trait SecretStore {
    fn get(&self, provider: &str) -> Result<String, RuntimeError>;
}

#[derive(Clone, Default)]
pub struct MemorySecretStore {
    values: Arc<Mutex<HashMap<String, String>>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&self, provider: &str, secret: &str) -> Result<(), RuntimeError> {
        self.values
            .lock()
            .map_err(|_| RuntimeError::SecretUnavailable)?
            .insert(provider.into(), secret.into());
        Ok(())
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, provider: &str) -> Result<String, RuntimeError> {
        self.values
            .lock()
            .map_err(|_| RuntimeError::SecretUnavailable)?
            .get(provider)
            .cloned()
            .ok_or(RuntimeError::SecretUnavailable)
    }
}

pub struct OsKeychainSecretStore {
    service: String,
    account: String,
}

impl OsKeychainSecretStore {
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
        }
    }
}

#[cfg(target_os = "macos")]
impl SecretStore for OsKeychainSecretStore {
    fn get(&self, _provider: &str) -> Result<String, RuntimeError> {
        let output = std::process::Command::new("/usr/bin/security")
            .args([
                "find-generic-password",
                "-s",
                &self.service,
                "-a",
                &self.account,
                "-w",
            ])
            .output()
            .map_err(|_| RuntimeError::SecretUnavailable)?;
        if !output.status.success() {
            return Err(RuntimeError::SecretUnavailable);
        }
        String::from_utf8(output.stdout)
            .map(|secret| secret.trim_end().to_string())
            .map_err(|_| RuntimeError::SecretUnavailable)
    }
}

#[cfg(not(target_os = "macos"))]
impl SecretStore for OsKeychainSecretStore {
    fn get(&self, _provider: &str) -> Result<String, RuntimeError> {
        let _ = (&self.service, &self.account);
        Err(RuntimeError::SecretUnavailable)
    }
}
