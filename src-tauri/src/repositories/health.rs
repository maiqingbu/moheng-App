//! Repository boundary for the B13 long-form health projection.
//!
//! This module only reads the already frozen Authority and Projection tables.
//! It deliberately returns bounded, typed inputs; factor policy and user-facing
//! presentation live in `crate::health`.

use libsql_rusqlite::{Connection, OptionalExtension};

const ISSUE_ROW_BOUND: i64 = 51;
const TASK_ROW_BOUND: i64 = 201;
const ROUTE_ROW_BOUND: i64 = 101;
const TREND_ROW_BOUND: i64 = 20;

#[derive(Debug, thiserror::Error)]
pub enum HealthRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
}

#[derive(Debug, Clone)]
pub struct ObligationInput {
    pub obligation_id: String,
    pub authority_level: String,
    pub description: String,
    pub deadline: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ForeshadowInput {
    pub foreshadow_id: String,
    pub intent: String,
    pub status: String,
    pub recover_window_end_chapter: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ReaderDesireInput {
    pub desire_id: String,
    pub desire_type: String,
    pub status: String,
    pub expected_payoff_chapter: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct RouteInput {
    pub volume_id: String,
    pub plan_state: String,
    pub generated_revision: i64,
    pub remaining_chapter_capacity: Option<i64>,
    pub planning_pressure_json: String,
    pub task_status_json: String,
    pub route_plan_artifact_id: Option<String>,
    pub route_plan_version: Option<i64>,
    pub updated_at: String,
    pub route_payload_json: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskRealityInput {
    pub state_id: String,
    pub value_json: String,
    pub chapter_id: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ContextSample {
    pub capability: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: i64,
    pub recorded_at: String,
    pub model_run_id: String,
}

#[derive(Debug, Clone)]
pub struct CostSample {
    pub predicted_cost_cents: i64,
    pub actual_cost_cents: i64,
    pub recorded_at: String,
    pub model_run_id: String,
}

#[derive(Debug, Clone)]
pub struct QualitySample {
    pub sample_chapter_id: String,
    pub sample_chapter_no: i64,
    pub quality_vector_json: String,
    pub sampled_at: String,
}

#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    pub project_revision: i64,
    pub checked_at: String,
    pub current_chapter_no: i64,
    pub open_obligations: i64,
    pub overdue_obligations: i64,
    pub obligations: Vec<ObligationInput>,
    pub open_foreshadows: i64,
    pub overdue_foreshadows: i64,
    pub foreshadows: Vec<ForeshadowInput>,
    pub routes: Vec<RouteInput>,
    pub task_reality: Vec<TaskRealityInput>,
    pub open_reader_desires: i64,
    pub overdue_reader_desires: i64,
    pub reader_desires: Vec<ReaderDesireInput>,
    pub context_samples: Vec<ContextSample>,
    pub cost_samples: Vec<CostSample>,
    pub has_over_limit_reservation: bool,
    pub quality_samples: Vec<QualitySample>,
}

pub struct HealthRepository;

impl HealthRepository {
    pub fn load(
        connection: &Connection,
        project_id: &str,
    ) -> Result<HealthSnapshot, HealthRepositoryError> {
        let project_revision: i64 = connection.query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        let project_updated_at: String = connection.query_row(
            "SELECT updated_at FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        let current_chapter_no: i64 = connection.query_row(
            "SELECT COALESCE(MAX(chapter_no),0) FROM chapters WHERE project_id=?1 AND status='COMMITTED'",
            [project_id],
            |row| row.get(0),
        )?;
        let checked_at: String = connection
            .query_row(
                "SELECT MAX(source_time) FROM (
                   SELECT updated_at AS source_time FROM project_meta WHERE project_id=?1
                   UNION ALL SELECT MAX(updated_at) FROM chapters WHERE project_id=?1 AND status='COMMITTED'
                   UNION ALL SELECT MAX(updated_at) FROM narrative_obligations WHERE project_id=?1
                   UNION ALL SELECT MAX(updated_at) FROM foreshadows WHERE project_id=?1
                   UNION ALL SELECT MAX(updated_at) FROM reader_desires WHERE project_id=?1
                   UNION ALL SELECT MAX(updated_at) FROM planning_state_ledger_index WHERE project_id=?1
                   UNION ALL SELECT MAX(updated_at) FROM model_runs WHERE project_id=?1
                   UNION ALL SELECT MAX(recorded_at) FROM model_usage WHERE project_id=?1
                   UNION ALL SELECT MAX(created_at) FROM budget_reservations WHERE project_id=?1
                   UNION ALL SELECT MAX(sampled_at) FROM quality_trend_projection WHERE project_id=?1
                 )",
                [project_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten()
            .unwrap_or(project_updated_at);

        let (open_obligations, overdue_obligations): (i64, i64) = connection.query_row(
            "SELECT COUNT(*), COALESCE(SUM(CASE WHEN TRIM(COALESCE(must_resolve_before_chapter,'')) <> '' AND CAST(REPLACE(REPLACE(must_resolve_before_chapter,'chapter:',''),'CHAPTER:','') AS INTEGER) <= ?2 THEN 1 ELSE 0 END),0)
             FROM narrative_obligations
             WHERE project_id=?1 AND lifecycle IN ('PROPOSED','APPROVED','PLANTED','PROGRESSING')",
            libsql_rusqlite::params![project_id, current_chapter_no],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let obligations = {
            let mut statement = connection.prepare(
                "SELECT obligation_id, authority_level, description, must_resolve_before_chapter, updated_at
                 FROM narrative_obligations
                 WHERE project_id=?1 AND lifecycle IN ('PROPOSED','APPROVED','PLANTED','PROGRESSING')
                 ORDER BY CASE WHEN TRIM(COALESCE(must_resolve_before_chapter,'')) <> '' THEN CAST(REPLACE(REPLACE(must_resolve_before_chapter,'chapter:',''),'CHAPTER:','') AS INTEGER) ELSE 9223372036854775807 END, obligation_id
                 LIMIT ?2",
            )?;
            let rows = statement.query_map(
                libsql_rusqlite::params![project_id, ISSUE_ROW_BOUND],
                |row| {
                    Ok(ObligationInput {
                        obligation_id: row.get(0)?,
                        authority_level: row.get(1)?,
                        description: row.get(2)?,
                        deadline: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let (open_foreshadows, overdue_foreshadows): (i64, i64) = connection.query_row(
            "SELECT COUNT(*), COALESCE(SUM(CASE WHEN TRIM(COALESCE(recover_window_end_chapter,'')) <> '' AND CAST(REPLACE(REPLACE(recover_window_end_chapter,'chapter:',''),'CHAPTER:','') AS INTEGER) <= ?2 THEN 1 ELSE 0 END),0)
             FROM foreshadows
             WHERE project_id=?1 AND status IN ('PLANTED','REINFORCED','STALE')",
            libsql_rusqlite::params![project_id, current_chapter_no],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let foreshadows = {
            let mut statement = connection.prepare(
                "SELECT foreshadow_id, intent, status, recover_window_end_chapter, updated_at
                 FROM foreshadows
                 WHERE project_id=?1 AND status IN ('PLANTED','REINFORCED','STALE')
                 ORDER BY CASE WHEN TRIM(COALESCE(recover_window_end_chapter,'')) <> '' THEN CAST(REPLACE(REPLACE(recover_window_end_chapter,'chapter:',''),'CHAPTER:','') AS INTEGER) ELSE 9223372036854775807 END, foreshadow_id
                 LIMIT ?2",
            )?;
            let rows = statement.query_map(
                libsql_rusqlite::params![project_id, ISSUE_ROW_BOUND],
                |row| {
                    Ok(ForeshadowInput {
                        foreshadow_id: row.get(0)?,
                        intent: row.get(1)?,
                        status: row.get(2)?,
                        recover_window_end_chapter: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let routes = {
            let mut statement = connection.prepare(
                "SELECT l.volume_id, l.plan_state, l.generated_revision, l.remaining_chapter_capacity,
                        l.planning_pressure_json, l.task_status_json, l.route_plan_artifact_id,
                        l.route_plan_version, l.updated_at,
                        (SELECT p.payload_json FROM artifact_payloads p
                         JOIN artifacts a ON a.artifact_id=p.artifact_id AND a.version=p.version
                         WHERE a.project_id=?1 AND p.artifact_id=l.route_plan_artifact_id
                           AND p.version=l.route_plan_version LIMIT 1)
                 FROM planning_state_ledger_index l
                 WHERE l.project_id=?1
                 ORDER BY l.volume_id
                 LIMIT ?2",
            )?;
            let rows = statement.query_map(
                libsql_rusqlite::params![project_id, ROUTE_ROW_BOUND],
                |row| {
                    Ok(RouteInput {
                        volume_id: row.get(0)?,
                        plan_state: row.get(1)?,
                        generated_revision: row.get(2)?,
                        remaining_chapter_capacity: row.get(3)?,
                        planning_pressure_json: row.get(4)?,
                        task_status_json: row.get(5)?,
                        route_plan_artifact_id: row.get(6)?,
                        route_plan_version: row.get(7)?,
                        updated_at: row.get(8)?,
                        route_payload_json: row.get(9)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let task_reality = {
            let mut statement = connection.prepare(
                "SELECT state_id, value_json, chapter_id, updated_at
                 FROM state_deltas
                 WHERE project_id=?1 AND state_type='volume.task.progress'
                 ORDER BY updated_at DESC, state_id
                 LIMIT ?2",
            )?;
            let rows = statement.query_map(
                libsql_rusqlite::params![project_id, TASK_ROW_BOUND],
                |row| {
                    Ok(TaskRealityInput {
                        state_id: row.get(0)?,
                        value_json: row.get(1)?,
                        chapter_id: row.get(2)?,
                        updated_at: row.get(3)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let (open_reader_desires, overdue_reader_desires): (i64, i64) = connection.query_row(
            "SELECT COUNT(*), COALESCE(SUM(CASE WHEN TRIM(COALESCE(expected_payoff_chapter,'')) <> '' AND CAST(REPLACE(REPLACE(expected_payoff_chapter,'chapter:',''),'CHAPTER:','') AS INTEGER) <= ?2 THEN 1 ELSE 0 END),0)
             FROM reader_desires
             WHERE project_id=?1 AND status IN ('OPEN','ESCALATED','PARTIAL_PAYOFF','STALE')",
            libsql_rusqlite::params![project_id, current_chapter_no],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let reader_desires = {
            let mut statement = connection.prepare(
                "SELECT desire_id, desire_type, status, expected_payoff_chapter, updated_at
                 FROM reader_desires
                 WHERE project_id=?1 AND status IN ('OPEN','ESCALATED','PARTIAL_PAYOFF','STALE')
                 ORDER BY CASE WHEN TRIM(COALESCE(expected_payoff_chapter,'')) <> '' THEN CAST(REPLACE(REPLACE(expected_payoff_chapter,'chapter:',''),'CHAPTER:','') AS INTEGER) ELSE 9223372036854775807 END, desire_id
                 LIMIT ?2",
            )?;
            let rows = statement.query_map(
                libsql_rusqlite::params![project_id, ISSUE_ROW_BOUND],
                |row| {
                    Ok(ReaderDesireInput {
                        desire_id: row.get(0)?,
                        desire_type: row.get(1)?,
                        status: row.get(2)?,
                        expected_payoff_chapter: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let context_samples = Self::context_samples(connection, project_id)?;
        let cost_samples = Self::cost_samples(connection, project_id)?;
        let has_over_limit_reservation: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM budget_reservations WHERE project_id=?1 AND status='OVER_LIMIT')",
            [project_id],
            |row| row.get(0),
        )?;
        let quality_samples = {
            let mut statement = connection.prepare(
                "SELECT sample_chapter_id, sample_chapter_no, quality_vector_json, sampled_at
                 FROM quality_trend_projection
                 WHERE project_id=?1
                 ORDER BY sample_chapter_no DESC, sample_chapter_id DESC
                 LIMIT ?2",
            )?;
            let rows = statement.query_map(
                libsql_rusqlite::params![project_id, TREND_ROW_BOUND],
                |row| {
                    Ok(QualitySample {
                        sample_chapter_id: row.get(0)?,
                        sample_chapter_no: row.get(1)?,
                        quality_vector_json: row.get(2)?,
                        sampled_at: row.get(3)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        Ok(HealthSnapshot {
            project_revision,
            checked_at,
            current_chapter_no,
            open_obligations,
            overdue_obligations,
            obligations,
            open_foreshadows,
            overdue_foreshadows,
            foreshadows,
            routes,
            task_reality,
            open_reader_desires,
            overdue_reader_desires,
            reader_desires,
            context_samples,
            cost_samples,
            has_over_limit_reservation,
            quality_samples,
        })
    }

    fn context_samples(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Vec<ContextSample>, HealthRepositoryError> {
        let latest: Option<(String, String, String)> = connection
            .query_row(
                "SELECT capability, provider, model
                 FROM model_runs
                 WHERE project_id=?1 AND status='COMPLETED'
                 ORDER BY updated_at DESC, model_run_id DESC
                 LIMIT 1",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((capability, provider, model)) = latest else {
            return Ok(Vec::new());
        };
        let mut statement = connection.prepare(
            "SELECT r.capability, r.provider, r.model, u.input_tokens, u.recorded_at, r.model_run_id
             FROM model_runs r JOIN model_usage u ON u.project_id=r.project_id AND u.model_run_id=r.model_run_id
             WHERE r.project_id=?1 AND r.status='COMPLETED' AND r.capability=?2 AND r.provider=?3 AND r.model=?4
             ORDER BY r.updated_at DESC, r.model_run_id DESC
             LIMIT 16",
        )?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, capability, provider, model],
            |row| {
                Ok(ContextSample {
                    capability: row.get(0)?,
                    provider: row.get(1)?,
                    model: row.get(2)?,
                    input_tokens: row.get(3)?,
                    recorded_at: row.get(4)?,
                    model_run_id: row.get(5)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn cost_samples(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Vec<CostSample>, HealthRepositoryError> {
        let mut statement = connection.prepare(
            "SELECT r.predicted_cost_cents, u.cost_cents, u.recorded_at, r.model_run_id
             FROM model_runs r JOIN model_usage u ON u.project_id=r.project_id AND u.model_run_id=r.model_run_id
             WHERE r.project_id=?1 AND r.status='COMPLETED' AND r.predicted_cost_cents > 0
             ORDER BY r.updated_at DESC, r.model_run_id DESC
             LIMIT 8",
        )?;
        let rows = statement.query_map([project_id], |row| {
            Ok(CostSample {
                predicted_cost_cents: row.get(0)?,
                actual_cost_cents: row.get(1)?,
                recorded_at: row.get(2)?,
                model_run_id: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
