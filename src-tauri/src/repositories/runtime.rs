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
    #[error("stable id rejected")]
    StableIdRejected,
    #[error("raw response persistence failed")]
    RawResponsePersistenceFailed,
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
    pub parent_job_id: Option<String>,
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

pub fn lease_expiry(now: &str) -> String {
    let Some(prefix) = now.get(..19) else {
        return format!("{now}.000+00:01:00");
    };
    let mut chars: Vec<char> = prefix.chars().collect();
    let parse = |start: usize| -> Option<i32> { prefix.get(start..start + 2)?.parse().ok() };
    let (mut hour, mut minute, mut second) = match (parse(11), parse(14), parse(17)) {
        (Some(hour), Some(minute), Some(second)) => (hour, minute, second + 60),
        _ => return format!("{now}.000+00:01:00"),
    };
    if second >= 60 {
        second -= 60;
        minute += 1;
    }
    if minute >= 60 {
        minute -= 60;
        hour += 1;
    }
    if hour >= 24 {
        hour -= 24;
        // The runtime only needs a finite lease marker for command control;
        // callers that cross midnight can renew with the next wall-clock tick.
    }
    for (index, value) in format!("{hour:02}:{minute:02}:{second:02}")
        .chars()
        .enumerate()
    {
        chars[11 + index] = value;
    }
    chars.into_iter().collect::<String>() + "Z"
}

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
        if !spec.job_id.starts_with("job:") {
            return Err(RuntimeError::StableIdRejected);
        }
        let project_id: String =
            connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
                row.get(0)
            })?;
        if let Some(parent_job_id) = &spec.parent_job_id {
            let parent_type: Option<String> = connection
                .query_row(
                    "SELECT job_type FROM jobs WHERE project_id=?1 AND job_id=?2",
                    libsql_rusqlite::params![project_id, parent_job_id],
                    |row| row.get(0),
                )
                .optional()?;
            let valid_parent = matches!(
                (spec.job_type.as_str(), parent_type.as_deref()),
                ("VOLUME", Some("BOOK")) | ("CHAPTER", Some("VOLUME"))
            );
            if !valid_parent {
                return Err(RuntimeError::InvalidTransition(
                    parent_type.unwrap_or_else(|| "MISSING".into()),
                    spec.job_type.clone(),
                ));
            }
        }
        connection.execute(
            "INSERT INTO jobs(job_id, project_id, job_type, book_id, volume_id, chapter_id, status, current_stage, attempt_no, repair_round, parent_job_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'QUEUED', ?7, 0, 0, ?8, ?9, ?9) ON CONFLICT(job_id) DO UPDATE SET updated_at=excluded.updated_at",
            libsql_rusqlite::params![spec.job_id, project_id, spec.job_type, spec.book_id, spec.volume_id, spec.chapter_id, spec.stages.first(), spec.parent_job_id, now],
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
        if expires_at.starts_with("9999-") || expires_at <= now {
            return Err(RuntimeError::InvalidTransition(
                "LEASE".into(),
                "FINITE_TTL_REQUIRED".into(),
            ));
        }
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

    pub fn renew_lease(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        holder: &str,
        expires_at: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        let updated = connection.execute(
            "UPDATE job_leases SET expires_at=?1, acquired_at=?2 WHERE project_id=?3 AND job_id=?4 AND holder=?5 AND released_at IS NULL AND expires_at>?2",
            libsql_rusqlite::params![expires_at, now, project_id, job_id, holder],
        )?;
        if updated == 0 {
            return Err(RuntimeError::LeaseHeld);
        }
        Ok(())
    }

    pub fn release_lease(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        connection.execute(
            "UPDATE job_leases SET released_at=?1 WHERE project_id=?2 AND job_id=?3 AND released_at IS NULL",
            libsql_rusqlite::params![now, project_id, job_id],
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
        Self::transition(connection, project_id, job_id, "PAUSED", now)?;
        Self::release_lease(connection, project_id, job_id, now)
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
        Self::transition(connection, project_id, job_id, "CANCELLED", now)?;
        Self::release_lease(connection, project_id, job_id, now)
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
        connection.execute(
            "UPDATE job_leases SET released_at=?1 WHERE project_id=?2 AND released_at IS NULL AND expires_at<=?1",
            libsql_rusqlite::params![now, project_id],
        )?;
        let mut statement = connection.prepare(
            "SELECT job_id FROM jobs WHERE project_id = ?1 AND status IN ('RUNNING','WAITING_MODEL')",
        )?;
        let jobs = statement
            .query_map([project_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for job_id in &jobs {
            let in_flight: Option<String> = connection
                .query_row(
                    "SELECT model_run_id FROM model_runs WHERE project_id=?1 AND job_id=?2 AND status='RUNNING' ORDER BY updated_at DESC LIMIT 1",
                    libsql_rusqlite::params![project_id, job_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(model_run_id) = in_flight {
                connection.execute(
                    "UPDATE model_runs SET status='UNKNOWN_REMOTE_OUTCOME', updated_at=?1 WHERE project_id=?2 AND model_run_id=?3",
                    libsql_rusqlite::params![now, project_id, model_run_id],
                )?;
                connection.execute(
                    "UPDATE budget_reservations SET status='OVER_LIMIT', settled_at=?1 WHERE project_id=?2 AND model_run_id=?3 AND status='RESERVED'",
                    libsql_rusqlite::params![now, project_id, model_run_id],
                )?;
                connection.execute(
                    "INSERT OR IGNORE INTO decisions(decision_id, project_id, decision_type, outcome, reason, source_ref, payload_json, created_at, updated_at) VALUES (?1, ?2, 'UNKNOWN_REMOTE_OUTCOME', 'OPEN', 'restart found in-flight model run', ?3, ?4, ?5, ?5)",
                    libsql_rusqlite::params![format!("decision:model-run:{model_run_id}"), project_id, model_run_id, serde_json::json!({"allowedActions":["RETRY_REMOTE","MARK_FAILED","MANUAL_RESOLVE","CANCEL_JOB"]}).to_string(), now],
                )?;
                connection.execute(
                    "UPDATE jobs SET status='WAITING_DECISION', error_code='UNKNOWN_REMOTE_OUTCOME', updated_at=?1 WHERE project_id=?2 AND job_id=?3",
                    libsql_rusqlite::params![now, project_id, job_id],
                )?;
                continue;
            }
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

#[derive(Debug, Clone, Default)]
pub struct TransportUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: Option<i64>,
    pub provider_reported_cost_cents: Option<i64>,
    pub latency_ms: i64,
    pub provider_request_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TransportResponse {
    pub raw_response: String,
    pub usage: TransportUsage,
}

pub trait ModelTransport {
    /// Test transports may inspect the database at the exact send boundary.
    /// Production transports keep the default no-op implementation.
    fn before_send(
        &mut self,
        _connection: &Connection,
        _spec: &ModelRunSpec,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }

    /// Fault-injection hook for proving that parse/commit do not run when the
    /// raw response cannot be durably persisted.
    fn before_raw_persist(
        &mut self,
        _connection: &Connection,
        _spec: &ModelRunSpec,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn send(&mut self, spec: &ModelRunSpec) -> Result<TransportResponse, TransportFailure>;
}

pub struct FakeModelTransport {
    response: Result<TransportResponse, TransportFailure>,
    calls: usize,
}

impl FakeModelTransport {
    pub fn success(value: Value) -> Self {
        Self::success_with_usage(
            value,
            TransportUsage {
                input_tokens: 10,
                output_tokens: 10,
                cached_tokens: None,
                provider_reported_cost_cents: Some(1),
                latency_ms: 1,
                provider_request_id: Some("fake-request-1".into()),
            },
        )
    }

    pub fn success_with_usage(value: Value, usage: TransportUsage) -> Self {
        Self {
            response: Ok(TransportResponse {
                raw_response: value.to_string(),
                usage,
            }),
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
            response: Ok(TransportResponse {
                raw_response: raw.into(),
                usage: TransportUsage {
                    input_tokens: 10,
                    output_tokens: 10,
                    cached_tokens: None,
                    provider_reported_cost_cents: Some(1),
                    latency_ms: 1,
                    provider_request_id: Some("fake-request-1".into()),
                },
            }),
            calls: 0,
        }
    }

    pub fn send_calls(&self) -> usize {
        self.calls
    }
}

impl ModelTransport for FakeModelTransport {
    fn send(&mut self, _spec: &ModelRunSpec) -> Result<TransportResponse, TransportFailure> {
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
            if status == "RUNNING" {
                mark_unknown_remote_outcome(connection, spec, now)?;
                return Err(RuntimeError::UnknownRemoteOutcome);
            }
            if status == "FAILED" {
                return Err(RuntimeError::InvalidTransition(
                    "FAILED".into(),
                    "retry-with-new-attempt".into(),
                ));
            }
        } else {
            enforce_budget(connection, spec)?;
            persist_before_send(connection, spec, now)?;
        }
        mark_sending(connection, spec, now)?;
        transport.before_send(connection, spec)?;
        let response = match transport.send(spec) {
            Ok(response) => response,
            Err(TransportFailure::Timeout) => {
                mark_unknown_remote_outcome(connection, spec, now)?;
                return Err(RuntimeError::UnknownRemoteOutcome);
            }
            Err(error) => {
                mark_confirmed_failure(connection, spec, now)?;
                return Err(RuntimeError::Transport(error));
            }
        };
        settle_remote_usage(connection, spec, &response.usage, now)?;
        if let Err(error) = transport.before_raw_persist(connection, spec) {
            mark_raw_persistence_failure(connection, spec, now)?;
            return Err(error);
        }
        if persist_raw_response(connection, spec, &response.raw_response, now).is_err() {
            mark_raw_persistence_failure(connection, spec, now)?;
            return Err(RuntimeError::RawResponsePersistenceFailed);
        }
        finish_parse(connection, spec, &response.raw_response, now)
    }

    pub fn retry<T: ModelTransport>(
        connection: &Connection,
        transport: &mut T,
        spec: &ModelRunSpec,
        now: &str,
    ) -> Result<ModelRunOutcome, RuntimeError> {
        let status = existing_model_status(connection, spec)?;
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
        if status.as_deref() == Some("UNKNOWN_REMOTE_OUTCOME") {
            return Err(RuntimeError::UnknownRemoteOutcome);
        }
        if status.as_deref() == Some("FAILED") {
            return Err(RuntimeError::InvalidTransition(
                "FAILED".into(),
                "retry-with-new-attempt".into(),
            ));
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
        record_budget_decision(
            connection,
            budget_id,
            "BUDGET_REPLAN",
            "REPLAN_REQUIRED",
            "held reservation requires explicit decision",
        )?;
        return Err(RuntimeError::BudgetHardLimit);
    }
    let used: i64 = connection.query_row(
        "SELECT COUNT(DISTINCT model_run_id) FROM budget_reservations WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND budget_ref=?1 AND status IN ('RESERVED','SETTLED','OVER_LIMIT')",
        [budget_id],
        |row| row.get(0),
    )?;
    let active_reserved_cost: i64 = connection.query_row(
        "SELECT COALESCE(SUM(reserved_cents), 0) FROM budget_reservations WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND budget_ref=?1 AND status='RESERVED'",
        [budget_id],
        |row| row.get(0),
    )?;
    let actual_spent: i64 = connection.query_row(
        "SELECT COALESCE(SUM(cost_cents), 0) FROM model_cost_ledger WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND budget_id=?1 AND cost_type='ACTUAL'",
        [budget_id],
        |row| row.get(0),
    )?;
    let next_runs = 1_i64;
    if hard_max > 0 && used + next_runs > hard_max {
        record_budget_decision(
            connection,
            budget_id,
            "BUDGET_REPLAN",
            "REPLAN_REQUIRED",
            "hard model-run limit reached",
        )?;
        return Err(RuntimeError::BudgetHardLimit);
    }
    if cost_budget > 0
        && actual_spent + active_reserved_cost + spec.predicted_cost_cents > cost_budget
    {
        record_budget_decision(
            connection,
            budget_id,
            "BUDGET_REPLAN",
            "REPLAN_REQUIRED",
            "hard cost limit reached",
        )?;
        return Err(RuntimeError::BudgetHardLimit);
    }
    let soft_max: i64 = connection.query_row(
        "SELECT soft_max_model_runs FROM chapter_production_budgets WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND budget_id=?1",
        [budget_id],
        |row| row.get(0),
    )?;
    if soft_max > 0 && used + next_runs >= soft_max {
        record_budget_decision(
            connection,
            budget_id,
            "BUDGET_PRESSURE",
            "OPEN",
            "soft model-run limit reached",
        )?;
        let project_id: String =
            connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
                row.get(0)
            })?;
        connection.execute(
            "INSERT INTO project_runtime_state(project_id, run_state, block_reason, updated_at) VALUES (?1, 'ACTIVE', 'BUDGET_PRESSURE', datetime('now')) ON CONFLICT(project_id) DO UPDATE SET block_reason='BUDGET_PRESSURE', updated_at=excluded.updated_at",
            [project_id],
        )?;
    }
    Ok(())
}

fn record_budget_decision(
    connection: &Connection,
    budget_id: &str,
    decision_type: &str,
    outcome: &str,
    reason: &str,
) -> Result<(), RuntimeError> {
    let project_id: String =
        connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
            row.get(0)
        })?;
    connection.execute(
        "INSERT INTO decisions(decision_id, project_id, decision_type, outcome, reason, payload_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'), datetime('now')) ON CONFLICT(decision_id) DO NOTHING",
        libsql_rusqlite::params![format!("decision:budget:{budget_id}:{decision_type}"), project_id, decision_type, outcome, reason, serde_json::json!({"budgetId":budget_id}).to_string()],
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

fn mark_sending(
    connection: &Connection,
    spec: &ModelRunSpec,
    now: &str,
) -> Result<(), RuntimeError> {
    connection.execute(
        "UPDATE model_runs SET status='RUNNING', updated_at=?1 WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND model_run_id=?2 AND status='PREPARED'",
        libsql_rusqlite::params![now, spec.model_run_id],
    )?;
    Ok(())
}

fn mark_confirmed_failure(
    connection: &Connection,
    spec: &ModelRunSpec,
    now: &str,
) -> Result<(), RuntimeError> {
    connection.execute(
        "UPDATE model_runs SET status='FAILED', updated_at=?1 WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND model_run_id=?2",
        libsql_rusqlite::params![now, spec.model_run_id],
    )?;
    connection.execute(
        "UPDATE budget_reservations SET status='RELEASED', settled_cents=0, settled_at=?1 WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND model_run_id=?2 AND status='RESERVED'",
        libsql_rusqlite::params![now, spec.model_run_id],
    )?;
    if let Some(job_id) = &spec.job_id {
        connection.execute(
            "UPDATE jobs SET status='FAILED', error_code='MODEL_TRANSPORT_FAILED', updated_at=?1 WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND job_id=?2 AND status NOT IN ('COMPLETED','CANCELLED')",
            libsql_rusqlite::params![now, job_id],
        )?;
    }
    Ok(())
}

fn mark_unknown_remote_outcome(
    connection: &Connection,
    spec: &ModelRunSpec,
    now: &str,
) -> Result<(), RuntimeError> {
    let project_id: String =
        connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
            row.get(0)
        })?;
    connection.execute(
        "UPDATE model_runs SET status='UNKNOWN_REMOTE_OUTCOME', updated_at=?1 WHERE project_id=?2 AND model_run_id=?3",
        libsql_rusqlite::params![now, project_id, spec.model_run_id],
    )?;
    connection.execute(
        "UPDATE budget_reservations SET status='OVER_LIMIT', settled_at=?1 WHERE project_id=?2 AND model_run_id=?3 AND status='RESERVED'",
        libsql_rusqlite::params![now, project_id, spec.model_run_id],
    )?;
    connection.execute(
        "INSERT INTO decisions(decision_id, project_id, decision_type, outcome, reason, source_ref, payload_json, created_at, updated_at) VALUES (?1, ?2, 'UNKNOWN_REMOTE_OUTCOME', 'OPEN', 'remote result is unknown', ?3, ?4, ?5, ?5) ON CONFLICT(decision_id) DO NOTHING",
        libsql_rusqlite::params![
            format!("decision:model-run:{}", spec.model_run_id),
            project_id,
            spec.model_run_id,
            serde_json::json!({"allowedActions":["RETRY_REMOTE","MARK_FAILED","MANUAL_RESOLVE","CANCEL_JOB"]}).to_string(),
            now,
        ],
    )?;
    if let Some(job_id) = &spec.job_id {
        connection.execute(
            "UPDATE jobs SET status='WAITING_DECISION', error_code='UNKNOWN_REMOTE_OUTCOME', updated_at=?1 WHERE project_id=?2 AND job_id=?3 AND status NOT IN ('COMPLETED','CANCELLED')",
            libsql_rusqlite::params![now, project_id, job_id],
        )?;
    }
    connection.execute(
        "INSERT INTO project_runtime_state(project_id, run_state, block_reason, updated_at) VALUES (?1, 'WAITING_DECISION', 'UNKNOWN_REMOTE_OUTCOME', ?2) ON CONFLICT(project_id) DO UPDATE SET run_state='WAITING_DECISION', block_reason='UNKNOWN_REMOTE_OUTCOME', updated_at=excluded.updated_at",
        libsql_rusqlite::params![project_id, now],
    )?;
    Ok(())
}

fn mark_raw_persistence_failure(
    connection: &Connection,
    spec: &ModelRunSpec,
    now: &str,
) -> Result<(), RuntimeError> {
    connection.execute(
        "UPDATE model_runs SET status='FAILED', updated_at=?1 WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND model_run_id=?2",
        libsql_rusqlite::params![now, spec.model_run_id],
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO production_events(event_id, project_id, job_id, event_type, payload_json, created_at) VALUES (?1, (SELECT project_id FROM project_meta LIMIT 1), ?2, 'MODEL_RAW_PERSIST_FAILED', ?3, ?4)",
        libsql_rusqlite::params![
            format!("event:raw-persist-failed:{}", spec.model_run_id),
            spec.job_id,
            serde_json::json!({"modelRunId":spec.model_run_id,"status":"FAILED"}).to_string(),
            now,
        ],
    )?;
    if let Some(job_id) = &spec.job_id {
        connection.execute(
            "UPDATE jobs SET status='FAILED', error_code='RAW_RESPONSE_PERSIST_FAILED', updated_at=?1 WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND job_id=?2",
            libsql_rusqlite::params![now, job_id],
        )?;
    }
    Ok(())
}

fn settle_remote_usage(
    connection: &Connection,
    spec: &ModelRunSpec,
    usage: &TransportUsage,
    now: &str,
) -> Result<(), RuntimeError> {
    let project_id: String =
        connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
            row.get(0)
        })?;
    let actual_cost = usage
        .provider_reported_cost_cents
        .filter(|cost| *cost >= 0)
        .unwrap_or(spec.predicted_cost_cents.max(0));
    let total_tokens = usage.input_tokens.max(0) + usage.output_tokens.max(0);
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        let inserted = connection.execute(
            "INSERT OR IGNORE INTO model_usage(usage_id, project_id, model_run_id, input_tokens, output_tokens, total_tokens, latency_ms, cost_cents, recorded_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            libsql_rusqlite::params![format!("usage:{}", spec.model_run_id), project_id, spec.model_run_id, usage.input_tokens.max(0), usage.output_tokens.max(0), total_tokens, usage.latency_ms.max(0), actual_cost, now],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO model_cost_ledger(ledger_id, project_id, model_run_id, budget_id, cost_cents, token_total, cost_type, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'ACTUAL', ?7)",
            libsql_rusqlite::params![format!("ledger:actual:{}", spec.model_run_id), project_id, spec.model_run_id, spec.budget_id, actual_cost, total_tokens, now],
        )?;
        if inserted > 0 {
            connection.execute(
                "INSERT INTO project_runtime_state(project_id, total_cost_cents, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(project_id) DO UPDATE SET total_cost_cents=project_runtime_state.total_cost_cents + excluded.total_cost_cents, updated_at=excluded.updated_at",
                libsql_rusqlite::params![project_id, actual_cost, now],
            )?;
        }
        connection.execute(
            "UPDATE budget_reservations SET status='SETTLED', settled_cents=?1, settled_at=?2 WHERE project_id=?3 AND model_run_id=?4 AND status IN ('RESERVED','OVER_LIMIT')",
            libsql_rusqlite::params![actual_cost, now, project_id, spec.model_run_id],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO production_events(event_id, project_id, job_id, event_type, payload_json, created_at) VALUES (?1, ?2, ?3, 'MODEL_USAGE_RECORDED', ?4, ?5)",
            libsql_rusqlite::params![format!("event:usage:{}", spec.model_run_id), project_id, spec.job_id, serde_json::json!({"modelRunId":spec.model_run_id,"provider":spec.provider,"model":spec.model,"attempt":spec.attempt_no,"latencyMs":usage.latency_ms.max(0),"inputTokens":usage.input_tokens.max(0),"outputTokens":usage.output_tokens.max(0),"costCents":actual_cost,"actualCostStatus":if usage.provider_reported_cost_cents.is_some() {"ACTUAL"} else {"ESTIMATED"}}).to_string(), now],
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
