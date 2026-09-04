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

// The route projection is the formal B07/B09 stale input.  It deliberately
// carries both the compiled and current relevant-state digests; project
// revision is retained as metadata only and is never used as a stale verdict.
const ROUTE_ROWS_CTE: &str = r#"
WITH route_rows AS (
  SELECT
    l.volume_id,
    v.volume_no,
    v.title AS volume_title,
    l.plan_state,
    l.generated_revision,
    l.remaining_chapter_capacity,
    l.planning_pressure_json,
    l.task_status_json,
    l.route_plan_artifact_id,
    l.route_plan_version,
    l.updated_at,
    p.payload_json AS route_payload_json,
    l.state_digest AS ledger_state_digest,
    vp.state_digest AS current_projection_digest,
    CASE WHEN EXISTS (
      SELECT 1 FROM stale_queue sq
      WHERE sq.project_id=l.project_id AND sq.processed=0
        AND sq.target_ref IN (l.volume_id, l.route_plan_artifact_id)
    ) THEN 1 ELSE 0 END AS pending_stale,
    CASE WHEN v.volume_id IS NULL THEN 1 ELSE 0 END AS source_unavailable
  FROM planning_state_ledger_index l
  LEFT JOIN volumes v
    ON v.project_id=l.project_id AND v.volume_id=l.volume_id
  LEFT JOIN artifacts a
    ON a.project_id=l.project_id
   AND a.artifact_id=l.route_plan_artifact_id
   AND a.version=l.route_plan_version
  LEFT JOIN artifact_payloads p
    ON p.artifact_id=a.artifact_id AND p.version=a.version
  LEFT JOIN volume_planning_state_projection vp
    ON vp.project_id=l.project_id AND vp.volume_id=l.volume_id
  WHERE l.project_id=?1
), normalized_routes AS (
  SELECT route_rows.*,
    CASE WHEN json_valid(route_payload_json)
      THEN json_extract(route_payload_json, '$.compiledFromStateDigest')
      ELSE NULL END AS payload_compiled_digest,
    CASE WHEN json_valid(route_payload_json)
      THEN COALESCE(
        json_extract(route_payload_json, '$.currentStateDigest'),
        json_extract(route_payload_json, '$.relevantStateDigest')
      )
      ELSE NULL END AS payload_current_digest,
    CASE WHEN json_valid(planning_pressure_json)
      THEN json_extract(planning_pressure_json, '$.remainingChapterCapacity')
      ELSE NULL END AS pressure_capacity,
    CASE WHEN json_valid(planning_pressure_json)
      THEN json_extract(planning_pressure_json, '$.requiredTasksRemaining')
      ELSE NULL END AS pressure_required
  FROM route_rows
), route_status AS (
  SELECT normalized_routes.*,
    COALESCE(payload_compiled_digest, ledger_state_digest) AS compiled_state_digest,
    COALESCE(current_projection_digest, payload_current_digest, ledger_state_digest) AS current_state_digest,
    CASE WHEN plan_state IN ('STALE','PLAN_STALE')
         OR pending_stale=1
         OR (
           COALESCE(payload_compiled_digest, ledger_state_digest) IS NOT NULL
           AND COALESCE(current_projection_digest, payload_current_digest, ledger_state_digest) IS NOT NULL
           AND COALESCE(payload_compiled_digest, ledger_state_digest)
               <> COALESCE(current_projection_digest, payload_current_digest, ledger_state_digest)
         )
      THEN 1 ELSE 0 END AS formal_stale,
    COALESCE(remaining_chapter_capacity, pressure_capacity) AS effective_capacity,
    pressure_required AS required_tasks_remaining
    FROM normalized_routes
)
"#;

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
    pub due_chapter_no: Option<i64>,
    pub source_unavailable: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ForeshadowInput {
    pub foreshadow_id: String,
    pub intent: String,
    pub status: String,
    pub recover_window_end_chapter: Option<String>,
    pub end_chapter_no: Option<i64>,
    pub source_unavailable: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ReaderDesireInput {
    pub desire_id: String,
    pub desire_type: String,
    pub status: String,
    pub expected_payoff_chapter: Option<String>,
    pub payoff_chapter_no: Option<i64>,
    pub source_unavailable: bool,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct RouteInput {
    pub volume_id: String,
    pub volume_no: Option<i64>,
    pub volume_title: Option<String>,
    pub plan_state: String,
    pub generated_revision: i64,
    pub remaining_chapter_capacity: Option<i64>,
    pub planning_pressure_json: String,
    pub task_status_json: String,
    pub route_plan_artifact_id: Option<String>,
    pub route_plan_version: Option<i64>,
    pub updated_at: String,
    pub route_payload_json: Option<String>,
    pub compiled_state_digest: Option<String>,
    pub current_state_digest: Option<String>,
    pub formal_stale: bool,
    pub source_unavailable: bool,
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
    pub overdue_obligations_authority_ab: i64,
    pub overdue_obligations_authority_c: i64,
    pub unavailable_obligations: i64,
    pub obligations: Vec<ObligationInput>,
    pub open_foreshadows: i64,
    pub overdue_foreshadows: i64,
    pub stale_not_overdue_foreshadows: i64,
    pub unavailable_foreshadows: i64,
    pub foreshadows: Vec<ForeshadowInput>,
    pub routes: Vec<RouteInput>,
    pub route_scope_count: i64,
    pub route_stale_scope_count: i64,
    pub route_capacity_risk_scope_count: i64,
    pub route_capacity_attention_scope_count: i64,
    pub unavailable_routes: i64,
    pub task_reality: Vec<TaskRealityInput>,
    pub blocked_required_tasks: i64,
    pub blocked_optional_tasks: i64,
    pub open_reader_desires: i64,
    pub overdue_reader_desires: i64,
    pub unavailable_reader_desires: i64,
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

        let (
            open_obligations,
            overdue_obligations,
            overdue_obligations_authority_ab,
            overdue_obligations_authority_c,
            unavailable_obligations,
        ): (i64, i64, i64, i64, i64) = connection.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN due.chapter_no IS NOT NULL AND due.chapter_no <= ?2 THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN due.chapter_no IS NOT NULL AND due.chapter_no <= ?2 AND o.authority_level IN ('A','B') THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN due.chapter_no IS NOT NULL AND due.chapter_no <= ?2 AND o.authority_level='C' THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN TRIM(COALESCE(o.must_resolve_before_chapter,'')) <> '' AND due.chapter_id IS NULL THEN 1 ELSE 0 END),0)
             FROM narrative_obligations o
             LEFT JOIN chapters due
               ON due.project_id=o.project_id AND due.chapter_id=o.must_resolve_before_chapter
             WHERE o.project_id=?1 AND o.lifecycle IN ('PROPOSED','APPROVED','PLANTED','PROGRESSING')",
            libsql_rusqlite::params![project_id, current_chapter_no],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;
        let obligations = {
            let mut statement = connection.prepare(
                "SELECT o.obligation_id, o.authority_level, o.description,
                        o.must_resolve_before_chapter, o.updated_at,
                        due.chapter_no,
                        CASE WHEN TRIM(COALESCE(o.must_resolve_before_chapter,'')) <> ''
                                  AND due.chapter_id IS NULL THEN 1 ELSE 0 END
                 FROM narrative_obligations o
                 LEFT JOIN chapters due
                   ON due.project_id=o.project_id AND due.chapter_id=o.must_resolve_before_chapter
                 WHERE o.project_id=?1 AND o.lifecycle IN ('PROPOSED','APPROVED','PLANTED','PROGRESSING')
                 ORDER BY COALESCE(due.chapter_no, 9223372036854775807), o.obligation_id
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
                        due_chapter_no: row.get(5)?,
                        source_unavailable: row.get::<_, i64>(6)? != 0,
                        updated_at: row.get(4)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let (
            open_foreshadows,
            overdue_foreshadows,
            stale_not_overdue_foreshadows,
            unavailable_foreshadows,
        ): (i64, i64, i64, i64) = connection.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN due.chapter_no IS NOT NULL AND due.chapter_no <= ?2 THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN f.status='STALE' AND due.chapter_no IS NOT NULL AND due.chapter_no > ?2 THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN TRIM(COALESCE(f.recover_window_end_chapter,'')) <> '' AND due.chapter_id IS NULL THEN 1 ELSE 0 END),0)
             FROM foreshadows f
             LEFT JOIN chapters due
               ON due.project_id=f.project_id AND due.chapter_id=f.recover_window_end_chapter
             WHERE f.project_id=?1 AND f.status IN ('PLANTED','REINFORCED','STALE')",
            libsql_rusqlite::params![project_id, current_chapter_no],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        let foreshadows = {
            let mut statement = connection.prepare(
                "SELECT f.foreshadow_id, f.intent, f.status,
                        f.recover_window_end_chapter, f.updated_at,
                        due.chapter_no,
                        CASE WHEN TRIM(COALESCE(f.recover_window_end_chapter,'')) <> ''
                                  AND due.chapter_id IS NULL THEN 1 ELSE 0 END
                 FROM foreshadows f
                 LEFT JOIN chapters due
                   ON due.project_id=f.project_id AND due.chapter_id=f.recover_window_end_chapter
                 WHERE f.project_id=?1 AND f.status IN ('PLANTED','REINFORCED','STALE')
                 ORDER BY COALESCE(due.chapter_no, 9223372036854775807), f.foreshadow_id
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
                        end_chapter_no: row.get(5)?,
                        source_unavailable: row.get::<_, i64>(6)? != 0,
                        updated_at: row.get(4)?,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let routes = {
            let mut statement = connection.prepare(&format!(
                "{ROUTE_ROWS_CTE}
                 SELECT * FROM route_status
                 ORDER BY COALESCE(volume_no, 9223372036854775807), volume_id
                 LIMIT ?2",
            ))?;
            let rows = statement.query_map(
                libsql_rusqlite::params![project_id, ROUTE_ROW_BOUND],
                |row| {
                    Ok(RouteInput {
                        volume_id: row.get(0)?,
                        volume_no: row.get(1)?,
                        volume_title: row.get(2)?,
                        plan_state: row.get(3)?,
                        generated_revision: row.get(4)?,
                        remaining_chapter_capacity: row.get(5)?,
                        planning_pressure_json: row.get(6)?,
                        task_status_json: row.get(7)?,
                        route_plan_artifact_id: row.get(8)?,
                        route_plan_version: row.get(9)?,
                        updated_at: row.get(10)?,
                        route_payload_json: row.get(11)?,
                        compiled_state_digest: row.get(20)?,
                        current_state_digest: row.get(21)?,
                        formal_stale: row.get::<_, i64>(22)? != 0,
                        source_unavailable: row.get::<_, i64>(15)? != 0,
                    })
                },
            )?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let (
            route_scope_count,
            route_stale_scope_count,
            route_capacity_risk_scope_count,
            route_capacity_attention_scope_count,
            unavailable_routes,
        ): (i64, i64, i64, i64, i64) = connection.query_row(
            &format!(
                "{ROUTE_ROWS_CTE}
                 SELECT COUNT(*),
                        COALESCE(SUM(formal_stale),0),
                        COALESCE(SUM(CASE WHEN COALESCE(effective_capacity, -1)=0
                                               AND COALESCE(required_tasks_remaining, 0)>0
                                          THEN 1 ELSE 0 END),0),
                        COALESCE(SUM(CASE WHEN COALESCE(effective_capacity, -1)>0
                                               AND effective_capacity<=8
                                               AND COALESCE(required_tasks_remaining, 0)>0
                                          THEN 1 ELSE 0 END),0),
                        COALESCE(SUM(source_unavailable),0)
                 FROM route_status",
            ),
            [project_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )?;

        let (blocked_required_tasks, blocked_optional_tasks): (i64, i64) = connection.query_row(
            r#"
            WITH route_rows AS (
              SELECT l.project_id, l.volume_id, l.task_status_json, p.payload_json
              FROM planning_state_ledger_index l
              LEFT JOIN artifacts a
                ON a.project_id=l.project_id
               AND a.artifact_id=l.route_plan_artifact_id
               AND a.version=l.route_plan_version
              LEFT JOIN artifact_payloads p
                ON p.artifact_id=a.artifact_id AND p.version=a.version
              WHERE l.project_id=?1
            ), route_tasks AS (
              SELECT json_extract(task_entry.value, '$.class') AS task_class,
                     COALESCE(
                       CASE WHEN json_valid(reality.value_json)
                            THEN json_extract(reality.value_json, '$.status') END,
                       CASE WHEN status_entry.type='object' AND json_valid(status_entry.value)
                            THEN json_extract(status_entry.value, '$.status')
                            WHEN status_entry.type='text' THEN status_entry.value END,
                       'NOT_STARTED'
                     ) AS task_status
              FROM route_rows
              JOIN json_each(
                CASE WHEN json_valid(route_rows.payload_json)
                     THEN COALESCE(json_extract(route_rows.payload_json, '$.tasks'), '[]')
                     ELSE '[]' END
              ) task_entry
              LEFT JOIN json_each(
                CASE WHEN json_valid(route_rows.task_status_json)
                     THEN route_rows.task_status_json ELSE '{}' END
              ) status_entry
                ON status_entry.key=json_extract(task_entry.value, '$.taskId')
              LEFT JOIN state_deltas reality
                ON reality.project_id=route_rows.project_id
               AND reality.state_id=json_extract(task_entry.value, '$.taskId')
               AND reality.state_type='volume.task.progress'
            )
            SELECT
              COALESCE(SUM(CASE WHEN task_status='BLOCKED'
                                    AND task_class IN ('REQUIRED','CLIMAX','RESERVED')
                               THEN 1 ELSE 0 END),0),
              COALESCE(SUM(CASE WHEN task_status='BLOCKED'
                                    AND task_class IN ('OPTIONAL','FLEXIBLE')
                               THEN 1 ELSE 0 END),0)
            FROM route_tasks
            "#,
            [project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

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

        let (open_reader_desires, overdue_reader_desires, unavailable_reader_desires): (i64, i64, i64) = connection.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN due.chapter_no IS NOT NULL AND due.chapter_no <= ?2 THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN TRIM(COALESCE(d.expected_payoff_chapter,'')) <> '' AND due.chapter_id IS NULL THEN 1 ELSE 0 END),0)
             FROM reader_desires d
             LEFT JOIN chapters due
               ON due.project_id=d.project_id AND due.chapter_id=d.expected_payoff_chapter
             WHERE d.project_id=?1 AND d.status IN ('OPEN','ESCALATED','PARTIAL_PAYOFF','STALE')",
            libsql_rusqlite::params![project_id, current_chapter_no],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let reader_desires = {
            let mut statement = connection.prepare(
                "SELECT d.desire_id, d.desire_type, d.status,
                        d.expected_payoff_chapter, d.updated_at,
                        due.chapter_no,
                        CASE WHEN TRIM(COALESCE(d.expected_payoff_chapter,'')) <> ''
                                  AND due.chapter_id IS NULL THEN 1 ELSE 0 END
                 FROM reader_desires d
                 LEFT JOIN chapters due
                   ON due.project_id=d.project_id AND due.chapter_id=d.expected_payoff_chapter
                 WHERE d.project_id=?1 AND d.status IN ('OPEN','ESCALATED','PARTIAL_PAYOFF','STALE')
                 ORDER BY COALESCE(due.chapter_no, 9223372036854775807), d.desire_id
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
                        payoff_chapter_no: row.get(5)?,
                        source_unavailable: row.get::<_, i64>(6)? != 0,
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
            overdue_obligations_authority_ab,
            overdue_obligations_authority_c,
            unavailable_obligations,
            obligations,
            open_foreshadows,
            overdue_foreshadows,
            stale_not_overdue_foreshadows,
            unavailable_foreshadows,
            foreshadows,
            routes,
            route_scope_count,
            route_stale_scope_count,
            route_capacity_risk_scope_count,
            route_capacity_attention_scope_count,
            unavailable_routes,
            task_reality,
            blocked_required_tasks,
            blocked_optional_tasks,
            open_reader_desires,
            overdue_reader_desires,
            unavailable_reader_desires,
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
