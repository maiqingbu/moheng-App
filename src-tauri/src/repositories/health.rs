//! Repository boundary for the B13 long-form health projection.
//!
//! This module only reads the already frozen Authority and Projection tables.
//! It deliberately returns bounded, typed inputs; factor policy and user-facing
//! presentation live in `crate::health`.

use crate::narrative::{evaluate_plan_stale, PlanStaleStatus};
use crate::repositories::change_retcon;
use crate::repositories::narrative::NarrativeRepository;
use libsql_rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

const ISSUE_ROW_BOUND: i64 = 51;
const TASK_ROW_BOUND: i64 = 201;
const ROUTE_ROW_BOUND: i64 = 101;
const TREND_ROW_BOUND: i64 = 20;

#[derive(Debug, thiserror::Error)]
pub enum HealthRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("authority: {0}")]
    Authority(String),
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

fn parse_value(raw: Option<&str>) -> Option<Value> {
    raw.and_then(|value| serde_json::from_str(value).ok())
}

fn task_status(value: &Value) -> String {
    value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("NOT_STARTED")
        .to_string()
}

fn route_task_ids(route: Option<&Value>) -> Vec<String> {
    route
        .and_then(|value| value.get("tasks"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|task| task.get("taskId").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

fn route_required_tasks(route: &RouteInput) -> Option<i64> {
    parse_value(Some(&route.planning_pressure_json))
        .and_then(|value| value.get("requiredTasksRemaining").and_then(Value::as_i64))
}

fn route_compiled_state_digest(
    connection: &Connection,
    project_id: &str,
    volume_id: &str,
    artifact_id: Option<&str>,
    version: Option<i64>,
    payload: Option<&Value>,
) -> Result<Option<String>, HealthRepositoryError> {
    if let Some(digest) = payload
        .and_then(|value| value.get("compiledFromStateDigest"))
        .and_then(Value::as_str)
    {
        return Ok(Some(digest.to_string()));
    }
    let Some(artifact_id) = artifact_id else {
        return Ok(None);
    };
    let Some(version) = version else {
        return Ok(None);
    };
    let input_refs: Option<String> = connection.query_row(
        "SELECT input_refs_json FROM volume_route_replans WHERE project_id=?1 AND volume_id=?2 AND new_artifact_id=?3 AND new_version=?4 ORDER BY created_at DESC, replan_id DESC LIMIT 1",
        libsql_rusqlite::params![project_id, volume_id, artifact_id, version],
        |row| row.get(0),
    ).optional()?;
    Ok(input_refs.and_then(|raw| {
        parse_value(Some(&raw)).and_then(|value| {
            value
                .pointer("/actualReality/stateDigest")
                .or_else(|| value.pointer("/compiledInput/actualReality/stateDigest"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
    }))
}

/// Read-only adapter for the formal B07/B09 planning chain.
///
/// `planning_state_ledger_index` is intentionally absent here. It is a
/// compatibility projection owned by earlier batches, not the source of H03
/// route truth or H04 task reality. Current route authority is resolved from
/// the B07 canonical head, with `volume_plan_index` and the B09 planning
/// projection supplying the formal persisted projection inputs.
fn formal_route_rows(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<RouteInput>, HealthRepositoryError> {
    let mut statement = connection.prepare(
        "SELECT v.volume_id, v.volume_no, v.title, v.updated_at,
                i.plan_state, i.plan_artifact_id, i.plan_version, i.updated_at,
                vp.projection_revision, vp.state_digest, vp.payload_json, vp.updated_at
         FROM volumes v
         LEFT JOIN volume_plan_index i
           ON i.project_id=v.project_id AND i.volume_id=v.volume_id
         LEFT JOIN volume_planning_state_projection vp
           ON vp.project_id=v.project_id AND vp.volume_id=v.volume_id
         WHERE v.project_id=?1
           AND (i.volume_id IS NOT NULL OR EXISTS (
             SELECT 1 FROM canonical_heads h
             WHERE h.project_id=v.project_id
               AND h.artifact_type='VolumePlan'
               AND h.scope_key IN (v.volume_id, json_object('level','VOLUME','volumeId',v.volume_id))
           ))
         ORDER BY v.volume_no, v.volume_id",
    )?;
    let rows = statement.query_map([project_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<i64>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<String>>(10)?,
            row.get::<_, Option<String>>(11)?,
        ))
    })?;
    let raw_rows = rows.collect::<Result<Vec<_>, _>>()?;
    let mut routes = Vec::with_capacity(raw_rows.len());

    for (
        volume_id,
        volume_no,
        volume_title,
        volume_updated_at,
        index_plan_state,
        index_artifact_id,
        index_version,
        index_updated_at,
        projection_revision,
        projection_state_digest,
        projection_raw,
        projection_updated_at,
    ) in raw_rows
    {
        let canonical_ref =
            NarrativeRepository::volume_plan_route(connection, project_id, &volume_id)
                .map_err(|error| HealthRepositoryError::Authority(error.to_string()))?;
        let route_artifact_id = canonical_ref
            .as_ref()
            .map(|reference| reference.artifact_id.clone())
            .or(index_artifact_id);
        let route_version = canonical_ref
            .as_ref()
            .map(|reference| reference.version)
            .or(index_version);
        let expected_hash = canonical_ref
            .as_ref()
            .map(|reference| reference.payload_hash.as_str());
        let artifact = match (route_artifact_id.as_deref(), route_version) {
            (Some(artifact_id), Some(version)) => connection
                .query_row(
                    "SELECT a.artifact_type, a.artifact_status, a.payload_hash, p.payload_json
                     FROM artifacts a
                     LEFT JOIN artifact_payloads p
                       ON p.artifact_id=a.artifact_id AND p.version=a.version
                     WHERE a.project_id=?1 AND a.artifact_id=?2 AND a.version=?3",
                    libsql_rusqlite::params![project_id, artifact_id, version],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                        ))
                    },
                )
                .optional()?,
            _ => None,
        };
        let source_unavailable = match artifact.as_ref() {
            Some((artifact_type, status, artifact_hash, payload)) => {
                artifact_type != "VolumePlan"
                    || status != "COMMITTED"
                    || expected_hash.is_some_and(|hash| hash != artifact_hash)
                    || payload.is_none()
                    || parse_value(payload.as_deref()).is_none()
            }
            None => true,
        };
        let route_payload_json = artifact
            .as_ref()
            .and_then(|(_, _, _, payload)| payload.clone());
        let route_payload = parse_value(route_payload_json.as_deref());
        let task_ids = route_task_ids(route_payload.as_ref());
        let task_id_refs = task_ids.iter().map(String::as_str).collect::<Vec<_>>();
        let task_states =
            NarrativeRepository::task_states_by_ids(connection, project_id, &task_id_refs, 500)
                .map_err(|error| HealthRepositoryError::Authority(error.to_string()))?;
        let mut task_statuses = Map::new();
        for state in task_states {
            task_statuses.insert(state.state_id, json!(task_status(&state.value)));
        }
        let projection = parse_value(projection_raw.as_deref());
        let planning_pressure_json = projection
            .as_ref()
            .and_then(|value| value.get("planningPressure"))
            .map(Value::to_string)
            .unwrap_or_else(|| "{}".to_string());
        let remaining_chapter_capacity = projection
            .as_ref()
            .and_then(|value| value.get("remainingChapterCapacity"))
            .and_then(Value::as_i64);
        let generated_revision = projection
            .as_ref()
            .and_then(|value| value.get("generatedRevision"))
            .and_then(Value::as_i64)
            .or(projection_revision)
            .unwrap_or(0);
        let compiled_state_digest = route_compiled_state_digest(
            connection,
            project_id,
            &volume_id,
            route_artifact_id.as_deref(),
            route_version,
            route_payload.as_ref(),
        )?;
        let current_state_digest = projection_state_digest.or_else(|| {
            projection
                .as_ref()
                .and_then(|value| value.get("stateDigest"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        });
        let pending_stale = if let Some(artifact_id) = route_artifact_id.as_deref() {
            change_retcon::has_pending_stale_for_refs(
                connection,
                project_id,
                &[artifact_id, volume_id.as_str()],
            )
            .map_err(|error| HealthRepositoryError::Authority(error.to_string()))?
        } else {
            false
        };
        let digest_stale = match (
            compiled_state_digest.as_deref(),
            current_state_digest.as_deref(),
        ) {
            (Some(compiled), Some(current)) => matches!(
                evaluate_plan_stale(generated_revision, compiled, generated_revision, current),
                PlanStaleStatus::PlanStale
            ),
            _ => false,
        };
        let formal_stale = pending_stale
            || digest_stale
            || index_plan_state
                .as_deref()
                .is_some_and(|state| matches!(state, "STALE" | "PLAN_STALE"));
        let updated_at = projection_updated_at
            .or(index_updated_at)
            .unwrap_or(volume_updated_at);
        routes.push(RouteInput {
            volume_id,
            volume_no: Some(volume_no),
            volume_title: Some(volume_title),
            plan_state: index_plan_state.unwrap_or_else(|| "APPROVED".to_string()),
            generated_revision,
            remaining_chapter_capacity,
            planning_pressure_json,
            task_status_json: Value::Object(task_statuses).to_string(),
            route_plan_artifact_id: route_artifact_id,
            route_plan_version: route_version,
            updated_at,
            route_payload_json,
            compiled_state_digest,
            current_state_digest,
            formal_stale,
            source_unavailable,
        });
    }
    Ok(routes)
}

fn formal_blocked_task_counts(routes: &[RouteInput]) -> (i64, i64) {
    let mut seen = BTreeSet::new();
    let mut required = 0;
    let mut optional = 0;
    for route in routes {
        let Some(Value::Array(tasks)) =
            route_payload(route).and_then(|payload| payload.get("tasks").cloned())
        else {
            continue;
        };
        let statuses = parse_value(Some(&route.task_status_json));
        for task in tasks {
            let Some(task_id) = task.get("taskId").and_then(Value::as_str) else {
                continue;
            };
            if !seen.insert(task_id.to_string()) {
                continue;
            }
            let status = statuses
                .as_ref()
                .and_then(|value| value.get(task_id))
                .and_then(|value| value.as_str())
                .unwrap_or("NOT_STARTED");
            if status != "BLOCKED" {
                continue;
            }
            match task.get("class").and_then(Value::as_str) {
                Some("REQUIRED" | "CLIMAX" | "RESERVED") => required += 1,
                Some("OPTIONAL" | "FLEXIBLE") => optional += 1,
                _ => {}
            }
        }
    }
    (required, optional)
}

fn route_payload(route: &RouteInput) -> Option<Value> {
    parse_value(route.route_payload_json.as_deref())
}

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
                   UNION ALL SELECT MAX(updated_at) FROM volume_plan_index WHERE project_id=?1
                   UNION ALL SELECT MAX(updated_at) FROM volume_planning_state_projection WHERE project_id=?1
                   UNION ALL SELECT MAX(updated_at) FROM state_deltas WHERE project_id=?1 AND state_type='volume.task.progress'
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

        let all_routes = formal_route_rows(connection, project_id)?;
        let routes = all_routes
            .iter()
            .take(ROUTE_ROW_BOUND as usize)
            .cloned()
            .collect::<Vec<_>>();

        let (
            route_scope_count,
            route_stale_scope_count,
            route_capacity_risk_scope_count,
            route_capacity_attention_scope_count,
            unavailable_routes,
        ): (i64, i64, i64, i64, i64) = all_routes.iter().fold(
            (0, 0, 0, 0, 0),
            |(scope_count, stale_count, capacity_risk, capacity_attention, unavailable), route| {
                let capacity = route.remaining_chapter_capacity;
                let required = route_required_tasks(route).unwrap_or(0);
                (
                    scope_count + 1,
                    stale_count + i64::from(route.formal_stale),
                    capacity_risk + i64::from(capacity == Some(0) && required > 0),
                    capacity_attention
                        + i64::from(
                            capacity.is_some_and(|value| value > 0 && value <= 8) && required > 0,
                        ),
                    unavailable + i64::from(route.source_unavailable),
                )
            },
        );

        let (blocked_required_tasks, blocked_optional_tasks) =
            formal_blocked_task_counts(&all_routes);

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
