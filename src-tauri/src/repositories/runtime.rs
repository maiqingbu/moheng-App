use crate::stable_id::{validate_stable_id, StableIdKind, StableIdentityFactory};
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
    #[error("retry job not found")]
    RetryJobNotFound,
    #[error("retry job invalid")]
    RetryJobInvalid,
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
    #[error("model retry must keep the original budget binding")]
    BudgetBindingMismatch,
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
    #[error("unknown remote outcome")]
    UnknownRemoteOutcome,
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

pub const DEPENDENCY_SUMMARY_BOOTSTRAP_STAGE: &str = "DEPENDENCY_SUMMARY_BOOTSTRAP";
pub const DEPENDENCY_SUMMARY_WORKER: &str = "b12-dependency-summary-worker";

#[derive(Debug, Clone, Default)]
pub struct RecoverySummary {
    pub recovered_jobs: i64,
}

pub struct DurableJobService;

pub fn pending_decision_count(
    connection: &Connection,
    project_id: &str,
) -> Result<i64, RuntimeError> {
    Ok(connection.query_row(
        "SELECT COUNT(*) FROM decisions WHERE project_id=?1 AND outcome IN ('OPEN','PENDING')",
        [project_id],
        |row| row.get(0),
    )?)
}

pub fn lease_expiry(now: &str) -> String {
    let Some(prefix) = now.get(..19) else {
        return now.to_string();
    };
    let parse = |start: usize, end: usize| -> Option<i32> { prefix.get(start..end)?.parse().ok() };
    let (mut year, mut month, mut day, mut hour, mut minute, mut second) = match (
        parse(0, 4),
        parse(5, 7),
        parse(8, 10),
        parse(11, 13),
        parse(14, 16),
        parse(17, 19),
    ) {
        (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) => {
            (year, month, day, hour, minute, second + 60)
        }
        _ => return now.to_string(),
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
        day += 1;
    }
    while day > days_in_month(year, month) {
        day -= days_in_month(year, month);
        month += 1;
        if month > 12 {
            month = 1;
            year += 1;
        }
    }
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => 31,
    }
}

impl DurableJobService {
    pub fn dependency_summary_job_id(project_id: &str) -> Result<String, RuntimeError> {
        StableIdentityFactory::from_command(
            StableIdKind::Job,
            &format!("b12:dependency-summary:{project_id}"),
        )
        .map_err(|_| RuntimeError::StableIdRejected)
    }

    pub fn ensure_dependency_summary_job(
        connection: &Connection,
        project_id: &str,
        now: &str,
    ) -> Result<String, RuntimeError> {
        let job_id = Self::dependency_summary_job_id(project_id)?;
        Self::start_job(
            connection,
            &JobSpec {
                job_id: job_id.clone(),
                job_type: "BOOK".into(),
                parent_job_id: None,
                book_id: None,
                volume_id: None,
                chapter_id: None,
                stages: vec![DEPENDENCY_SUMMARY_BOOTSTRAP_STAGE.into()],
            },
            now,
        )?;
        let job_status: Option<String> = connection
            .query_row(
                "SELECT status FROM jobs WHERE project_id=?1 AND job_id=?2",
                libsql_rusqlite::params![project_id, job_id],
                |row| row.get(0),
            )
            .optional()?;
        let summary_ready = connection
            .query_row(
                "SELECT COALESCE(json_extract(setting_value,'$.summaryStatus'),'NOT_READY')='READY' AND COALESCE(json_extract(setting_value,'$.mutationEpoch'),0)=COALESCE(json_extract(setting_value,'$.computedEpoch'),-1) FROM project_settings WHERE project_id=?1 AND setting_key='b12.dependencySummary.global'",
                [project_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(false);
        if job_status.as_deref() == Some("COMPLETED") && !summary_ready {
            connection.execute(
                "UPDATE jobs SET status='QUEUED',error_code=NULL,updated_at=?1 WHERE project_id=?2 AND job_id=?3",
                libsql_rusqlite::params![now, project_id, job_id],
            )?;
        }
        Ok(job_id)
    }

    /// Claims the single project-scoped bootstrap job.  The caller must hold
    /// the project command transaction, so the status transition and lease
    /// acquisition are serialized with competing UI/worker requests.
    pub fn claim_dependency_summary_job(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        worker_id: &str,
        now: &str,
    ) -> Result<bool, RuntimeError> {
        let current: Option<String> = connection
            .query_row(
                "SELECT status FROM jobs WHERE project_id=?1 AND job_id=?2 AND current_stage=?3",
                libsql_rusqlite::params![project_id, job_id, DEPENDENCY_SUMMARY_BOOTSTRAP_STAGE],
                |row| row.get(0),
            )
            .optional()?;
        let Some(current) = current else {
            return Err(RuntimeError::JobNotFound);
        };
        // A slow local worker may outlive the short lease TTL.  A normal UI
        // bootstrap request is not restart recovery, so it must never steal a
        // live RUNNING job merely because that TTL elapsed.
        if matches!(current.as_str(), "COMPLETED" | "RUNNING") {
            return Ok(false);
        }
        let active_holder: Option<(String, String)> = connection
            .query_row(
                "SELECT holder,expires_at FROM job_leases WHERE project_id=?1 AND job_id=?2 AND released_at IS NULL",
                libsql_rusqlite::params![project_id, job_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if active_holder
            .as_ref()
            .is_some_and(|(_, expires_at)| expires_at.as_str() > now)
        {
            return Ok(false);
        }
        if !matches!(current.as_str(), "QUEUED" | "PAUSED" | "FAILED") {
            return Ok(false);
        }
        let updated = connection.execute(
            "UPDATE jobs SET status='RUNNING',error_code=NULL,updated_at=?1 WHERE project_id=?2 AND job_id=?3 AND current_stage=?4 AND status IN ('QUEUED','PAUSED','FAILED')",
            libsql_rusqlite::params![now, project_id, job_id, DEPENDENCY_SUMMARY_BOOTSTRAP_STAGE],
        )?;
        if updated != 1 {
            return Ok(false);
        }
        Self::acquire_lease(
            connection,
            project_id,
            job_id,
            worker_id,
            &lease_expiry(now),
            now,
        )?;
        Ok(true)
    }

    pub fn complete_dependency_summary_job(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        worker_id: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        Self::transition_for_worker(connection, project_id, job_id, "COMPLETED", worker_id, now)?;
        Self::release_lease_for_holder(connection, project_id, job_id, worker_id, now)
    }

    pub fn pause_dependency_summary_job(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        worker_id: &str,
        error_code: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        ensure_active_lease_for_holder(connection, project_id, job_id, worker_id, now)?;
        let updated = connection.execute(
            "UPDATE jobs SET status='PAUSED',error_code=?1,updated_at=?2 WHERE project_id=?3 AND job_id=?4 AND status='RUNNING'",
            libsql_rusqlite::params![error_code, now, project_id, job_id],
        )?;
        if updated != 1 {
            return Err(RuntimeError::InvalidTransition(
                "NOT_RUNNING".into(),
                "PAUSED".into(),
            ));
        }
        Self::release_lease_for_holder(connection, project_id, job_id, worker_id, now)
    }

    pub fn dependency_summary_job_status(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Option<Value>, RuntimeError> {
        let job_id = Self::dependency_summary_job_id(project_id)?;
        let job: Option<(String, Option<String>, Option<String>, i64)> = connection
            .query_row(
                "SELECT status,current_stage,error_code,attempt_no FROM jobs WHERE project_id=?1 AND job_id=?2",
                libsql_rusqlite::params![project_id, job_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((status, stage, error_code, attempt_no)) = job else {
            return Ok(None);
        };
        let checkpoint: Option<(i64, String)> = connection
            .query_row(
                "SELECT offset,boundary_json FROM job_checkpoints WHERE project_id=?1 AND job_id=?2 AND stage=?3",
                libsql_rusqlite::params![project_id, job_id, DEPENDENCY_SUMMARY_BOOTSTRAP_STAGE],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(Some(serde_json::json!({
            "jobId":job_id,
            "status":status,
            "stage":stage,
            "errorCode":error_code,
            "attemptNo":attempt_no,
            "checkpointOffset":checkpoint.as_ref().map(|item| item.0).unwrap_or(0),
            "checkpoint":checkpoint.and_then(|item| serde_json::from_str::<Value>(&item.1).ok()).unwrap_or_else(|| serde_json::json!({}))
        })))
    }

    pub fn runtime_projection_updated_at(
        connection: &Connection,
        project_id: &str,
    ) -> Result<String, RuntimeError> {
        Ok(connection.query_row(
            "SELECT updated_at FROM project_runtime_state WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?)
    }
    pub fn update_stage(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        stage: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        connection.execute(
            "UPDATE jobs SET current_stage=?1, updated_at=?2 WHERE project_id=?3 AND job_id=?4",
            libsql_rusqlite::params![stage, now, project_id, job_id],
        )?;
        Ok(())
    }

    pub fn block_for_plan_stale(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        connection.execute(
            "UPDATE jobs SET status='BLOCKED',current_stage='RECONCILE',error_code='PLAN_STALE',updated_at=?1 WHERE project_id=?2 AND job_id=?3",
            libsql_rusqlite::params![now, project_id, job_id],
        )?;
        Self::release_lease(connection, project_id, job_id, now)?;
        Self::set_runtime_state(connection, project_id, "PAUSED", Some("PLAN_STALE"), now)?;
        Ok(())
    }
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
        validate_stable_id(&spec.job_id, StableIdKind::Job)
            .map_err(|_| RuntimeError::StableIdRejected)?;
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

    pub fn validate_retry_source(
        connection: &Connection,
        project_id: &str,
        previous_job_id: &str,
        chapter_id: &str,
    ) -> Result<(), RuntimeError> {
        let previous: Option<(String, Option<String>)> = connection
            .query_row(
                "SELECT status, chapter_id FROM jobs WHERE project_id=?1 AND job_id=?2",
                libsql_rusqlite::params![project_id, previous_job_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((status, previous_chapter_id)) = previous else {
            return Err(RuntimeError::RetryJobNotFound);
        };
        if status != "FAILED" || previous_chapter_id.as_deref() != Some(chapter_id) {
            return Err(RuntimeError::RetryJobInvalid);
        }
        Ok(())
    }

    pub fn record_retry_lineage(
        connection: &Connection,
        project_id: &str,
        new_job_id: &str,
        previous_job_id: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        connection.execute(
            "INSERT INTO production_events(event_id, project_id, job_id, event_type, payload_json, created_at) VALUES (?1, ?2, ?3, 'JOB_RETRY_CREATED', ?4, ?5)",
            libsql_rusqlite::params![
                format!("event:retry:{new_job_id}"),
                project_id,
                new_job_id,
                serde_json::json!({"retryOfJobId":previous_job_id,"lineage":"RETRY"}).to_string(),
                now,
            ],
        )?;
        Ok(())
    }

    pub fn fail_job(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        error_code: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        connection.execute(
            "UPDATE jobs SET status='FAILED', error_code=?1, updated_at=?2 WHERE project_id=?3 AND job_id=?4 AND status NOT IN ('COMPLETED','CANCELLED')",
            libsql_rusqlite::params![error_code, now, project_id, job_id],
        )?;
        Self::release_lease(connection, project_id, job_id, now)?;
        Self::reconcile_runtime_projection(connection, project_id, now)?;
        Ok(())
    }

    pub fn transition(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        next: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        Self::transition_for_worker(connection, project_id, job_id, next, "runtime-worker", now)
    }

    pub fn transition_for_worker(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        next: &str,
        worker_id: &str,
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
        if next == "RUNNING" {
            ensure_active_lease_for_holder(connection, project_id, job_id, worker_id, now)?;
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
        let updated = connection.execute(
            "INSERT INTO job_leases(lease_id, project_id, job_id, holder, expires_at, acquired_at, released_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL) ON CONFLICT(project_id, job_id) DO UPDATE SET holder=excluded.holder, expires_at=excluded.expires_at, acquired_at=excluded.acquired_at, released_at=NULL WHERE job_leases.released_at IS NOT NULL OR job_leases.expires_at <= excluded.acquired_at",
            libsql_rusqlite::params![format!("lease:{}", job_id), project_id, job_id, holder, expires_at, now],
        )?;
        if updated == 0 {
            return Err(RuntimeError::LeaseHeld);
        }
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

    pub fn release_lease_for_holder(
        connection: &Connection,
        project_id: &str,
        job_id: &str,
        holder: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        let updated = connection.execute(
            "UPDATE job_leases SET released_at=?1 WHERE project_id=?2 AND job_id=?3 AND holder=?4 AND released_at IS NULL",
            libsql_rusqlite::params![now, project_id, job_id, holder],
        )?;
        if updated != 1 {
            return Err(RuntimeError::LeaseHeld);
        }
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
        worker_id: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        Self::transition_for_worker(connection, project_id, job_id, "RUNNING", worker_id, now)
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

    /// Recompute the rebuildable project runtime projection from the latest
    /// relevant chapter Job. The Job is the authority; this helper exists so
    /// terminal transport failures cannot leave a stale ACTIVE projection.
    pub fn reconcile_runtime_projection(
        connection: &Connection,
        project_id: &str,
        now: &str,
    ) -> Result<String, RuntimeError> {
        let job: Option<(String, Option<String>)> = connection
            .query_row(
                "SELECT status, error_code FROM jobs WHERE project_id=?1 ORDER BY CASE WHEN job_type='CHAPTER' THEN 0 ELSE 1 END, updated_at DESC, job_id DESC LIMIT 1",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (run_state, block_reason) = match job {
            Some((status, error_code)) => match status.as_str() {
                "RUNNING" | "WAITING_MODEL" => ("ACTIVE", None),
                "WAITING_DECISION" => ("WAITING_DECISION", error_code),
                "PAUSED" => ("PAUSED", error_code),
                "BLOCKED" => ("BLOCKED", error_code),
                "FAILED" => ("FAILED", error_code),
                "COMPLETED" => ("COMPLETED", None),
                _ => ("IDLE", error_code),
            },
            None => ("IDLE", None),
        };
        Self::set_runtime_state(
            connection,
            project_id,
            run_state,
            block_reason.as_deref(),
            now,
        )?;
        Ok(run_state.into())
    }

    pub fn recover_after_restart(
        connection: &Connection,
        project_id: &str,
        now: &str,
    ) -> Result<RecoverySummary, RuntimeError> {
        connection.execute(
            "UPDATE job_leases SET released_at=?1 WHERE project_id=?2 AND released_at IS NULL",
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
                    libsql_rusqlite::params![format!("decision:model-run:{model_run_id}"), project_id, model_run_id, serde_json::json!({"allowedActions":["RETRY_REMOTE","MARK_FAILED","CANCEL_JOB"]}).to_string(), now],
                )?;
                connection.execute(
                    "UPDATE jobs SET status='WAITING_DECISION', error_code='UNKNOWN_REMOTE_OUTCOME', updated_at=?1 WHERE project_id=?2 AND job_id=?3",
                    libsql_rusqlite::params![now, project_id, job_id],
                )?;
                connection.execute(
                    "INSERT INTO project_runtime_state(project_id, run_state, block_reason, updated_at) VALUES (?1, 'WAITING_DECISION', 'UNKNOWN_REMOTE_OUTCOME', ?2) ON CONFLICT(project_id) DO UPDATE SET run_state='WAITING_DECISION', block_reason='UNKNOWN_REMOTE_OUTCOME', updated_at=excluded.updated_at",
                    libsql_rusqlite::params![project_id, now],
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

fn ensure_active_lease_for_holder(
    connection: &Connection,
    project_id: &str,
    job_id: &str,
    worker_id: &str,
    now: &str,
) -> Result<(), RuntimeError> {
    let active: Option<(String, String)> = connection
        .query_row(
            "SELECT lease_id, holder FROM job_leases WHERE project_id=?1 AND job_id=?2 AND released_at IS NULL AND expires_at>?3",
            libsql_rusqlite::params![project_id, job_id, now],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match active {
        Some((_, holder)) if holder != worker_id => return Err(RuntimeError::LeaseHeld),
        Some(_) => {}
        None => {
            DurableJobService::acquire_lease(
                connection,
                project_id,
                job_id,
                worker_id,
                &lease_expiry(now),
                now,
            )?;
        }
    }
    Ok(())
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

    /// Provider metadata may be authoritative only after the exact raw body
    /// has been persisted.  Legacy transports keep the historical behavior;
    /// the configured production transport opts into this post-persist path.
    fn defer_usage_until_raw_persist(&self) -> bool {
        false
    }

    fn usage_after_raw_persist(
        &mut self,
        _connection: &Connection,
        _spec: &ModelRunSpec,
        _raw_response: &str,
        usage: &TransportUsage,
    ) -> Result<TransportUsage, RuntimeError> {
        Ok(usage.clone())
    }

    fn credential(&self) -> &str {
        ""
    }

    fn send(
        &mut self,
        spec: &ModelRunSpec,
        credential: &str,
    ) -> Result<TransportResponse, TransportFailure>;
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
    fn send(
        &mut self,
        _spec: &ModelRunSpec,
        _credential: &str,
    ) -> Result<TransportResponse, TransportFailure> {
        self.calls += 1;
        self.response.clone()
    }
}

pub struct DurableModelRunner;

impl DurableModelRunner {
    pub fn persisted_raw_response(
        connection: &Connection,
        model_run_id: &str,
    ) -> Result<Option<String>, RuntimeError> {
        connection
            .query_row(
                "SELECT raw_response FROM model_responses WHERE model_run_id=?1",
                [model_run_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Resolve the chapter's existing Budget Authority binding.  Production
    /// model stages must use this lookup rather than creating an unscoped
    /// reservation or accepting a caller-provided budget id.
    pub fn chapter_budget_id(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
    ) -> Result<String, RuntimeError> {
        connection
            .query_row(
                "SELECT budget_id FROM chapter_production_budgets WHERE project_id=?1 AND chapter_id=?2",
                libsql_rusqlite::params![project_id, chapter_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(RuntimeError::BudgetHardLimit)
    }

    pub fn prepare(
        connection: &Connection,
        spec: &ModelRunSpec,
        now: &str,
    ) -> Result<(), RuntimeError> {
        validate_model_run_id(spec)?;
        if existing_model_status(connection, spec)?.is_none() {
            enforce_budget(connection, spec, false, now)?;
            persist_before_send(connection, spec, now)?;
        }
        Ok(())
    }

    pub fn execute<T: ModelTransport>(
        connection: &Connection,
        transport: &mut T,
        spec: &ModelRunSpec,
        now: &str,
    ) -> Result<ModelRunOutcome, RuntimeError> {
        Self::execute_with_budget_mode(connection, transport, spec, now, false)
    }

    /// Record a failure discovered while parsing a provider envelope's model
    /// payload.  The raw response has already been durably stored at this
    /// point, so replay/audit remains possible while the failed payload cannot
    /// be mistaken for an authority-ready result.
    pub fn mark_model_payload_parse_failure(
        connection: &Connection,
        spec: &ModelRunSpec,
        error_message: &str,
        now: &str,
    ) -> Result<(), RuntimeError> {
        connection.execute(
            "UPDATE model_parse_results SET status='SCHEMA_INVALID', parsed_json=NULL, error_message=?1, created_at=?2 WHERE model_run_id=?3",
            libsql_rusqlite::params![error_message, now, spec.model_run_id],
        )?;
        connection.execute(
            "UPDATE model_runs SET status='FAILED', updated_at=?1 WHERE model_run_id=?2",
            libsql_rusqlite::params![now, spec.model_run_id],
        )?;
        let job_error_code = error_message
            .strip_prefix("P01_")
            .and_then(|value| value.split(':').next())
            .unwrap_or("MODEL_PAYLOAD_INVALID");
        if let Some(job_id) = &spec.job_id {
            let project_id: String =
                connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
                    row.get(0)
                })?;
            connection.execute(
                "UPDATE jobs SET status='FAILED', error_code=?1, updated_at=?2 WHERE project_id=?3 AND job_id=?4 AND status NOT IN ('COMPLETED','CANCELLED')",
                libsql_rusqlite::params![job_error_code, now, project_id, job_id],
            )?;
            DurableJobService::release_lease(connection, &project_id, job_id, now)?;
            DurableJobService::reconcile_runtime_projection(connection, &project_id, now)?;
        }
        Ok(())
    }

    fn execute_with_budget_mode<T: ModelTransport>(
        connection: &Connection,
        transport: &mut T,
        spec: &ModelRunSpec,
        now: &str,
        allow_unknown_hold: bool,
    ) -> Result<ModelRunOutcome, RuntimeError> {
        validate_model_run_id(spec)?;
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
            enforce_budget(connection, spec, allow_unknown_hold, now)?;
            persist_before_send(connection, spec, now)?;
        }
        mark_sending(connection, spec, now)?;
        if let Err(error) = transport.before_send(connection, spec) {
            mark_confirmed_failure(connection, spec, now)?;
            return Err(error);
        }
        let credential = transport.credential().to_owned();
        let response = match transport.send(spec, &credential) {
            Ok(response) => response,
            Err(TransportFailure::Timeout | TransportFailure::UnknownRemoteOutcome) => {
                mark_unknown_remote_outcome(connection, spec, now)?;
                return Err(RuntimeError::UnknownRemoteOutcome);
            }
            Err(error) => {
                mark_confirmed_failure(connection, spec, now)?;
                return Err(RuntimeError::Transport(error));
            }
        };
        let defer_usage_until_raw_persist = transport.defer_usage_until_raw_persist();
        if !defer_usage_until_raw_persist {
            settle_remote_usage(connection, spec, &response.usage, now)?;
        }
        if let Err(error) = transport.before_raw_persist(connection, spec) {
            mark_raw_persistence_failure(connection, spec, now)?;
            return Err(error);
        }
        if persist_raw_response(connection, spec, &response.raw_response, now).is_err() {
            mark_raw_persistence_failure(connection, spec, now)?;
            return Err(RuntimeError::RawResponsePersistenceFailed);
        }
        if defer_usage_until_raw_persist {
            let usage = transport.usage_after_raw_persist(
                connection,
                spec,
                &response.raw_response,
                &response.usage,
            )?;
            settle_remote_usage(connection, spec, &usage, now)?;
        }
        finish_parse(connection, spec, &response.raw_response, now)
    }

    pub fn retry<T: ModelTransport>(
        connection: &Connection,
        transport: &mut T,
        spec: &ModelRunSpec,
        now: &str,
    ) -> Result<ModelRunOutcome, RuntimeError> {
        validate_model_run_id(spec)?;
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

    pub fn resolve_unknown<T: ModelTransport>(
        connection: &Connection,
        model_run_id: &str,
        action: &str,
        transport: &mut T,
        next_spec: &ModelRunSpec,
        now: &str,
    ) -> Result<ModelRunOutcome, RuntimeError> {
        let status: String = connection
            .query_row(
                "SELECT status FROM model_runs WHERE model_run_id=?1",
                [model_run_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(RuntimeError::JobNotFound)?;
        if status != "UNKNOWN_REMOTE_OUTCOME" {
            return Err(RuntimeError::InvalidTransition(
                status,
                "explicit-unknown-decision".into(),
            ));
        }
        let decision_id = format!("decision:model-run:{model_run_id}");
        match action {
            "RETRY_REMOTE" => {
                validate_model_run_id(next_spec)?;
                if next_spec.model_run_id == model_run_id {
                    return Err(RuntimeError::StableIdRejected);
                }
                let old_budget: String = connection.query_row(
                    "SELECT budget_ref FROM budget_reservations WHERE model_run_id=?1 ORDER BY created_at DESC LIMIT 1",
                    [model_run_id],
                    |row| row.get(0),
                )?;
                if next_spec.budget_id.as_deref().unwrap_or("UNSCOPED") != old_budget {
                    return Err(RuntimeError::BudgetBindingMismatch);
                }
                resolve_unknown_responsibility(connection, model_run_id, now)?;
                if let Some(job_id) = &next_spec.job_id {
                    let project_id: String = connection.query_row(
                        "SELECT project_id FROM project_meta LIMIT 1",
                        [],
                        |row| row.get(0),
                    )?;
                    DurableJobService::transition_for_worker(
                        connection,
                        &project_id,
                        job_id,
                        "RUNNING",
                        "runtime-worker",
                        now,
                    )?;
                    connection.execute(
                        "UPDATE jobs SET error_code=NULL WHERE project_id=?1 AND job_id=?2",
                        libsql_rusqlite::params![project_id, job_id],
                    )?;
                }
                let result =
                    Self::execute_with_budget_mode(connection, transport, next_spec, now, true);
                let decision_outcome = if matches!(&result, Err(RuntimeError::BudgetHardLimit)) {
                    "BUDGET_BLOCKED"
                } else {
                    "RETRY_REMOTE"
                };
                connection.execute(
                    "UPDATE decisions SET outcome=?1, updated_at=?2 WHERE decision_id=?3 AND outcome IN ('OPEN','PENDING')",
                    libsql_rusqlite::params![decision_outcome, now, decision_id],
                )?;
                result
            }
            "MARK_FAILED" | "CANCEL_JOB" => {
                let job_status = if action == "CANCEL_JOB" {
                    "CANCELLED"
                } else {
                    "FAILED"
                };
                resolve_unknown_responsibility(connection, model_run_id, now)?;
                connection.execute(
                    "UPDATE model_runs SET status='FAILED', updated_at=?1 WHERE model_run_id=?2",
                    libsql_rusqlite::params![now, model_run_id],
                )?;
                connection.execute(
                    "UPDATE decisions SET outcome=?1, updated_at=?2 WHERE decision_id=?3",
                    libsql_rusqlite::params![action, now, decision_id],
                )?;
                connection.execute(
                    "UPDATE jobs SET status=?1, updated_at=?2 WHERE job_id=(SELECT job_id FROM model_runs WHERE model_run_id=?3)",
                    libsql_rusqlite::params![job_status, now, model_run_id],
                )?;
                Ok(ModelRunOutcome {
                    status: "FAILED".into(),
                    parsed: None,
                    replayed: false,
                })
            }
            _ => Err(RuntimeError::UnknownRemoteOutcome),
        }
    }
}

fn resolve_unknown_responsibility(
    connection: &Connection,
    model_run_id: &str,
    now: &str,
) -> Result<(), RuntimeError> {
    let project_id: String =
        connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
            row.get(0)
        })?;
    let reserved_cents: Option<i64> = connection
        .query_row(
            "SELECT reserved_cents FROM budget_reservations WHERE project_id=?1 AND model_run_id=?2 AND status='OVER_LIMIT'",
            libsql_rusqlite::params![project_id, model_run_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(reserved_cents) = reserved_cents else {
        return Ok(());
    };
    connection.execute(
        "UPDATE budget_reservations SET status='SETTLED', settled_cents=reserved_cents, settled_at=?1 WHERE project_id=?2 AND model_run_id=?3 AND status='OVER_LIMIT'",
        libsql_rusqlite::params![now, project_id, model_run_id],
    )?;
    connection.execute(
        "INSERT INTO project_runtime_state(project_id, total_cost_cents, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(project_id) DO UPDATE SET total_cost_cents=project_runtime_state.total_cost_cents + excluded.total_cost_cents, updated_at=excluded.updated_at",
        libsql_rusqlite::params![project_id, reserved_cents, now],
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO production_events(event_id, project_id, event_type, payload_json, created_at) VALUES (?1, ?2, 'UNKNOWN_COST_SETTLED_ESTIMATE', ?3, ?4)",
        libsql_rusqlite::params![format!("event:unknown-settled:{model_run_id}"), project_id, serde_json::json!({"modelRunId":model_run_id,"settledCents":reserved_cents,"costStatus":"ESTIMATED"}).to_string(), now],
    )?;
    Ok(())
}

fn validate_model_run_id(spec: &ModelRunSpec) -> Result<(), RuntimeError> {
    validate_stable_id(&spec.model_run_id, StableIdKind::ModelRun)
        .map_err(|_| RuntimeError::StableIdRejected)
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

fn enforce_budget(
    connection: &Connection,
    spec: &ModelRunSpec,
    allow_unknown_hold: bool,
    now: &str,
) -> Result<(), RuntimeError> {
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
    if over_limit > 0 && !allow_unknown_hold {
        record_budget_decision(
            connection,
            budget_id,
            "BUDGET_REPLAN",
            "REPLAN_REQUIRED",
            "held reservation requires explicit decision",
        )?;
        mark_budget_blocked(connection, spec, now)?;
        return Err(RuntimeError::BudgetHardLimit);
    }
    let used: i64 = connection.query_row(
        "SELECT COUNT(DISTINCT model_run_id) FROM budget_reservations WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND budget_ref=?1 AND status IN ('RESERVED','SETTLED','OVER_LIMIT')",
        [budget_id],
        |row| row.get(0),
    )?;
    let active_reserved_cost: i64 = connection.query_row(
        "SELECT COALESCE(SUM(reserved_cents), 0) FROM budget_reservations WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND budget_ref=?1 AND status IN ('RESERVED','OVER_LIMIT')",
        [budget_id],
        |row| row.get(0),
    )?;
    let actual_spent: i64 = connection.query_row(
        "SELECT COALESCE(SUM(cost_cents), 0) FROM model_cost_ledger WHERE project_id=(SELECT project_id FROM project_meta LIMIT 1) AND budget_id=?1 AND cost_type='ACTUAL'",
        [budget_id],
        |row| row.get(0),
    )?;
    let estimated_unknown_spent: i64 = connection.query_row(
        "SELECT COALESCE(SUM(reservation.settled_cents), 0) FROM budget_reservations AS reservation LEFT JOIN model_usage AS usage ON usage.project_id=reservation.project_id AND usage.model_run_id=reservation.model_run_id WHERE reservation.project_id=(SELECT project_id FROM project_meta LIMIT 1) AND reservation.budget_ref=?1 AND reservation.status='SETTLED' AND usage.model_run_id IS NULL",
        [budget_id],
        |row| row.get(0),
    )?;
    let actual_spent = actual_spent + estimated_unknown_spent;
    let next_runs = 1_i64;
    if hard_max > 0 && used + next_runs > hard_max {
        record_budget_decision(
            connection,
            budget_id,
            if allow_unknown_hold {
                "BUDGET_BLOCKED"
            } else {
                "BUDGET_REPLAN"
            },
            "REPLAN_REQUIRED",
            "hard model-run limit reached",
        )?;
        mark_budget_blocked(connection, spec, now)?;
        return Err(RuntimeError::BudgetHardLimit);
    }
    if cost_budget > 0
        && actual_spent + active_reserved_cost + spec.predicted_cost_cents > cost_budget
    {
        record_budget_decision(
            connection,
            budget_id,
            if allow_unknown_hold {
                "BUDGET_BLOCKED"
            } else {
                "BUDGET_REPLAN"
            },
            "REPLAN_REQUIRED",
            "hard cost limit reached",
        )?;
        mark_budget_blocked(connection, spec, now)?;
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

fn mark_budget_blocked(
    connection: &Connection,
    spec: &ModelRunSpec,
    now: &str,
) -> Result<(), RuntimeError> {
    let project_id: String =
        connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
            row.get(0)
        })?;
    connection.execute(
        "INSERT INTO project_runtime_state(project_id, run_state, block_reason, updated_at) VALUES (?1, 'WAITING_BUDGET', 'BUDGET_BLOCKED', datetime('now')) ON CONFLICT(project_id) DO UPDATE SET run_state='WAITING_BUDGET', block_reason='BUDGET_BLOCKED', updated_at=excluded.updated_at",
        [&project_id],
    )?;
    if let Some(job_id) = &spec.job_id {
        connection.execute(
            "UPDATE jobs SET status='WAITING_BUDGET', error_code='BUDGET_BLOCKED', updated_at=datetime('now') WHERE project_id=?1 AND job_id=?2 AND status NOT IN ('COMPLETED','CANCELLED')",
            libsql_rusqlite::params![project_id, job_id],
        )?;
        DurableJobService::release_lease(connection, &project_id, job_id, now)?;
    }
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
    let project_id: String =
        connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
            row.get(0)
        })?;
    connection.execute(
        "UPDATE model_runs SET status='FAILED', updated_at=?1 WHERE project_id=?2 AND model_run_id=?3",
        libsql_rusqlite::params![now, project_id, spec.model_run_id],
    )?;
    connection.execute(
        "UPDATE budget_reservations SET status='RELEASED', settled_cents=0, settled_at=?1 WHERE project_id=?2 AND model_run_id=?3 AND status='RESERVED'",
        libsql_rusqlite::params![now, project_id, spec.model_run_id],
    )?;
    if let Some(job_id) = &spec.job_id {
        connection.execute(
            "UPDATE jobs SET status='FAILED', error_code='MODEL_TRANSPORT_FAILED', updated_at=?1 WHERE project_id=?2 AND job_id=?3 AND status NOT IN ('COMPLETED','CANCELLED')",
            libsql_rusqlite::params![now, project_id, job_id],
        )?;
        DurableJobService::release_lease(connection, &project_id, job_id, now)?;
    }
    DurableJobService::reconcile_runtime_projection(connection, &project_id, now)?;
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
            serde_json::json!({"allowedActions":["RETRY_REMOTE","MARK_FAILED","CANCEL_JOB"]}).to_string(),
            now,
        ],
    )?;
    if let Some(job_id) = &spec.job_id {
        connection.execute(
            "UPDATE jobs SET status='WAITING_DECISION', error_code='UNKNOWN_REMOTE_OUTCOME', updated_at=?1 WHERE project_id=?2 AND job_id=?3 AND status NOT IN ('COMPLETED','CANCELLED')",
            libsql_rusqlite::params![now, project_id, job_id],
        )?;
        DurableJobService::release_lease(connection, &project_id, job_id, now)?;
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
    let project_id: String =
        connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
            row.get(0)
        })?;
    connection.execute(
        "UPDATE model_runs SET status='FAILED', updated_at=?1 WHERE project_id=?2 AND model_run_id=?3",
        libsql_rusqlite::params![now, project_id, spec.model_run_id],
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO production_events(event_id, project_id, job_id, event_type, payload_json, created_at) VALUES (?1, ?2, ?3, 'MODEL_RAW_PERSIST_FAILED', ?4, ?5)",
        libsql_rusqlite::params![
            format!("event:raw-persist-failed:{}", spec.model_run_id),
            project_id,
            spec.job_id,
            serde_json::json!({"modelRunId":spec.model_run_id,"status":"FAILED"}).to_string(),
            now,
        ],
    )?;
    if let Some(job_id) = &spec.job_id {
        connection.execute(
        "UPDATE jobs SET status='FAILED', error_code='RAW_RESPONSE_PERSIST_FAILED', updated_at=?1 WHERE project_id=?2 AND job_id=?3",
        libsql_rusqlite::params![now, project_id, job_id],
    )?;
        DurableJobService::release_lease(connection, &project_id, job_id, now)?;
    }
    DurableJobService::reconcile_runtime_projection(connection, &project_id, now)?;
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
            libsql_rusqlite::params![format!("event:usage:{}", spec.model_run_id), project_id, spec.job_id, serde_json::json!({"modelRunId":spec.model_run_id,"provider":spec.provider,"model":spec.model,"attempt":spec.attempt_no,"latencyMs":usage.latency_ms.max(0),"inputTokens":usage.input_tokens.max(0),"outputTokens":usage.output_tokens.max(0),"cachedTokens":usage.cached_tokens,"costCents":actual_cost,"providerRequestId":usage.provider_request_id,"actualCostStatus":if usage.provider_reported_cost_cents.is_some() {"ACTUAL"} else {"ESTIMATED"}}).to_string(), now],
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
            let provider_error_code = if raw.trim().is_empty() {
                "PROVIDER_EMPTY_BODY"
            } else {
                "PROVIDER_MALFORMED_ENVELOPE"
            };
            if let Some(job_id) = &spec.job_id {
                let project_id: String = connection.query_row(
                    "SELECT project_id FROM project_meta LIMIT 1",
                    [],
                    |row| row.get(0),
                )?;
                connection.execute(
                    "UPDATE jobs SET status='FAILED', error_code=?1, updated_at=?2 WHERE project_id=?3 AND job_id=?4 AND status NOT IN ('COMPLETED','CANCELLED')",
                    libsql_rusqlite::params![provider_error_code, now, project_id, job_id],
                )?;
                DurableJobService::release_lease(connection, &project_id, job_id, now)?;
                DurableJobService::reconcile_runtime_projection(connection, &project_id, now)?;
            }
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

pub trait SecretStore: Send + Sync {
    fn get(&self, provider: &str) -> Result<String, RuntimeError>;
}

pub struct SecretBackedTransport<T, S> {
    inner: T,
    store: S,
    provider: String,
    credential: Option<String>,
}

impl<T, S> SecretBackedTransport<T, S> {
    pub fn new(inner: T, store: S, provider: impl Into<String>) -> Self {
        Self {
            inner,
            store,
            provider: provider.into(),
            credential: None,
        }
    }
}

impl<T: ModelTransport, S: SecretStore> ModelTransport for SecretBackedTransport<T, S> {
    fn before_send(
        &mut self,
        connection: &Connection,
        spec: &ModelRunSpec,
    ) -> Result<(), RuntimeError> {
        self.credential = Some(self.store.get(&self.provider)?);
        self.inner.before_send(connection, spec)
    }

    fn before_raw_persist(
        &mut self,
        connection: &Connection,
        spec: &ModelRunSpec,
    ) -> Result<(), RuntimeError> {
        self.inner.before_raw_persist(connection, spec)
    }

    fn defer_usage_until_raw_persist(&self) -> bool {
        self.inner.defer_usage_until_raw_persist()
    }

    fn usage_after_raw_persist(
        &mut self,
        connection: &Connection,
        spec: &ModelRunSpec,
        raw_response: &str,
        usage: &TransportUsage,
    ) -> Result<TransportUsage, RuntimeError> {
        self.inner
            .usage_after_raw_persist(connection, spec, raw_response, usage)
    }

    fn credential(&self) -> &str {
        self.credential.as_deref().unwrap_or("")
    }

    fn send(
        &mut self,
        spec: &ModelRunSpec,
        credential: &str,
    ) -> Result<TransportResponse, TransportFailure> {
        self.inner.send(spec, credential)
    }
}

pub fn redact_secret(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        text.to_string()
    } else {
        text.replace(secret, "[REDACTED]")
    }
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

    #[cfg(target_os = "macos")]
    pub fn set(&self, provider: &str, secret: &str) -> Result<(), RuntimeError> {
        if secret.trim().is_empty() {
            return Err(RuntimeError::SecretUnavailable);
        }
        let account = format!("{}:{}", self.account, provider);
        let status = std::process::Command::new("/usr/bin/security")
            .args([
                "add-generic-password",
                "-U",
                "-s",
                &self.service,
                "-a",
                &account,
                "-w",
                secret,
            ])
            .status()
            .map_err(|_| RuntimeError::SecretUnavailable)?;
        if status.success() {
            Ok(())
        } else {
            Err(RuntimeError::SecretUnavailable)
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn set(&self, _provider: &str, _secret: &str) -> Result<(), RuntimeError> {
        Err(RuntimeError::SecretUnavailable)
    }
}

#[cfg(target_os = "macos")]
impl SecretStore for OsKeychainSecretStore {
    fn get(&self, provider: &str) -> Result<String, RuntimeError> {
        let account = format!("{}:{}", self.account, provider);
        let output = std::process::Command::new("/usr/bin/security")
            .args([
                "find-generic-password",
                "-s",
                &self.service,
                "-a",
                &account,
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
