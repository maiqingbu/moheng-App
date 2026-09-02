//! Persistence primitives for the B12 Change/Retcon owner.
//!
//! This module contains SQL only.  Lifecycle decisions, authorization and the
//! CommitBundle assembly remain in `crate::change_retcon`, so there is one
//! formal Change/Retcon owner and one dependency authority.

#![allow(clippy::too_many_arguments)]

use libsql_rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum ChangeRetconRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct ChangeRequestRecord {
    pub change_request_id: String,
    pub change_type: String,
    pub scope: String,
    pub summary: String,
    pub status: String,
    pub impact_seed: Value,
    pub decision_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImpactSetRecord {
    pub impact_set_id: String,
    pub change_request_id: String,
    pub scope_class: String,
    pub estimate: Value,
    pub preflight_state: String,
    pub processed_pages: Value,
    pub total_edges: i64,
    pub expanded_edges: i64,
    pub checkpoint_offset: i64,
    pub status: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImpactEdge {
    pub edge_id: String,
    pub source_ref: String,
    pub target_ref: String,
    pub dependency_type: String,
    pub scope: String,
    pub impact_class: String,
    pub source_hash: String,
}

#[derive(Debug, Clone)]
pub struct RetconPlanRecord {
    pub retcon_plan_id: String,
    pub target_fact: Value,
    pub reason: String,
    pub fix_type: String,
    pub downstream_impact: Value,
    pub status: String,
    pub decision_ref: Option<String>,
}

pub fn insert_change_request(
    connection: &Connection,
    project_id: &str,
    change_request_id: &str,
    change_type: &str,
    scope: &str,
    summary: &str,
    seed: &Value,
    now: &str,
) -> Result<(), ChangeRetconRepositoryError> {
    connection.execute(
        "INSERT INTO change_requests(change_request_id,project_id,change_type,scope,summary,status,impact_seed_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'DRAFT',?6,?7,?7)",
        libsql_rusqlite::params![
            change_request_id,
            project_id,
            change_type,
            scope,
            summary,
            serde_json::to_string(seed)?,
            now
        ],
    )?;
    Ok(())
}

pub fn load_change_request(
    connection: &Connection,
    project_id: &str,
    change_request_id: &str,
) -> Result<Option<ChangeRequestRecord>, ChangeRetconRepositoryError> {
    connection
        .query_row(
            "SELECT change_request_id,change_type,scope,summary,status,impact_seed_json,decision_ref FROM change_requests WHERE project_id=?1 AND change_request_id=?2",
            libsql_rusqlite::params![project_id, change_request_id],
            |row| {
                let seed: String = row.get(5)?;
                Ok(ChangeRequestRecord {
                    change_request_id: row.get(0)?,
                    change_type: row.get(1)?,
                    scope: row.get(2)?,
                    summary: row.get(3)?,
                    status: row.get(4)?,
                    impact_seed: serde_json::from_str(&seed).unwrap_or_else(|_| json!({})),
                    decision_ref: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn update_change_request(
    connection: &Connection,
    project_id: &str,
    change_request_id: &str,
    status: &str,
    seed: &Value,
    decision_ref: Option<&str>,
    now: &str,
) -> Result<(), ChangeRetconRepositoryError> {
    connection.execute(
        "UPDATE change_requests SET status=?1,impact_seed_json=?2,decision_ref=COALESCE(?3,decision_ref),updated_at=?4 WHERE project_id=?5 AND change_request_id=?6",
        libsql_rusqlite::params![
            status,
            serde_json::to_string(seed)?,
            decision_ref,
            now,
            project_id,
            change_request_id
        ],
    )?;
    Ok(())
}

pub fn load_impact_set(
    connection: &Connection,
    project_id: &str,
    change_request_id: &str,
) -> Result<Option<ImpactSetRecord>, ChangeRetconRepositoryError> {
    connection
        .query_row(
            "SELECT impact_set_id,change_request_id,scope_class,impact_estimate_json,preflight_state,processed_pages_json,total_edges,expanded_edges,checkpoint_offset,status FROM impact_sets WHERE project_id=?1 AND change_request_id=?2 ORDER BY created_at DESC,impact_set_id DESC LIMIT 1",
            libsql_rusqlite::params![project_id, change_request_id],
            |row| {
                let estimate: String = row.get(3)?;
                let pages: String = row.get(5)?;
                Ok(ImpactSetRecord {
                    impact_set_id: row.get(0)?,
                    change_request_id: row.get(1)?,
                    scope_class: row.get(2)?,
                    estimate: serde_json::from_str(&estimate).unwrap_or_else(|_| json!({})),
                    preflight_state: row.get(4)?,
                    processed_pages: serde_json::from_str(&pages).unwrap_or_else(|_| json!([])),
                    total_edges: row.get(6)?,
                    expanded_edges: row.get(7)?,
                    checkpoint_offset: row.get(8)?,
                    status: row.get(9)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn insert_impact_set(
    connection: &Connection,
    project_id: &str,
    impact_set_id: &str,
    change_request_id: &str,
    scope_class: &str,
    estimate: &Value,
    total_edges: i64,
    status: &str,
    now: &str,
) -> Result<(), ChangeRetconRepositoryError> {
    connection.execute(
        "INSERT INTO impact_sets(impact_set_id,project_id,change_request_id,scope_class,impact_estimate_json,preflight_state,processed_pages_json,total_edges,expanded_edges,checkpoint_offset,status,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'READY','[]',?6,0,0,?7,?8,?8)",
        libsql_rusqlite::params![
            impact_set_id,
            project_id,
            change_request_id,
            scope_class,
            serde_json::to_string(estimate)?,
            total_edges,
            status,
            now
        ],
    )?;
    Ok(())
}

pub fn update_impact_set(
    connection: &Connection,
    project_id: &str,
    impact_set_id: &str,
    pages: &Value,
    expanded_edges: i64,
    checkpoint_offset: i64,
    status: &str,
    preflight_state: &str,
    now: &str,
) -> Result<(), ChangeRetconRepositoryError> {
    connection.execute(
        "UPDATE impact_sets SET processed_pages_json=?1,expanded_edges=?2,checkpoint_offset=?3,status=?4,preflight_state=?5,updated_at=?6 WHERE project_id=?7 AND impact_set_id=?8",
        libsql_rusqlite::params![
            serde_json::to_string(pages)?,
            expanded_edges,
            checkpoint_offset,
            status,
            preflight_state,
            now,
            project_id,
            impact_set_id
        ],
    )?;
    Ok(())
}

pub fn load_decision(
    connection: &Connection,
    project_id: &str,
    decision_id: &str,
) -> Result<Option<(String, Value)>, ChangeRetconRepositoryError> {
    connection
        .query_row(
            "SELECT outcome,payload_json FROM decisions WHERE project_id=?1 AND decision_id=?2",
            libsql_rusqlite::params![project_id, decision_id],
            |row| {
                let payload: String = row.get(1)?;
                Ok((
                    row.get(0)?,
                    serde_json::from_str(&payload).unwrap_or_else(|_| json!({})),
                ))
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn state_exists(
    connection: &Connection,
    project_id: &str,
    state_id: &str,
) -> Result<bool, ChangeRetconRepositoryError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM state_deltas WHERE project_id=?1 AND state_id=?2)",
            libsql_rusqlite::params![project_id, state_id],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

pub fn artifact_reference_exists(
    connection: &Connection,
    project_id: &str,
    artifact_id: &str,
    version: i64,
) -> Result<bool, ChangeRetconRepositoryError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM artifacts WHERE project_id=?1 AND artifact_id=?2 AND version=?3)",
            libsql_rusqlite::params![project_id, artifact_id, version],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

pub fn mark_impact_stale(
    connection: &Connection,
    project_id: &str,
    impact_set_id: &str,
    change_request_id: &str,
    seed: &Value,
    now: &str,
) -> Result<(), ChangeRetconRepositoryError> {
    update_impact_set(
        connection,
        project_id,
        impact_set_id,
        &json!([]),
        0,
        0,
        "FAILED",
        "STALE",
        now,
    )?;
    let mut lifecycle_seed = seed.clone();
    lifecycle_seed["lifecycle"] = json!("STALE");
    lifecycle_seed["impactState"] = json!("STALE");
    update_change_request(
        connection,
        project_id,
        change_request_id,
        "IMPACT_READY",
        &lifecycle_seed,
        None,
        now,
    )
}

pub fn count_dependency_edges(
    connection: &Connection,
    project_id: &str,
    source_refs: &Value,
    global: bool,
) -> Result<i64, ChangeRetconRepositoryError> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM dependency_edges WHERE project_id=?1 AND (?3=1 OR source_ref IN (SELECT value FROM json_each(?2)))",
        libsql_rusqlite::params![project_id, serde_json::to_string(source_refs)?, i64::from(global)],
        |row| row.get(0),
    )?;
    Ok(count)
}

pub fn dependency_class_counts(
    connection: &Connection,
    project_id: &str,
    source_refs: &Value,
    global: bool,
) -> Result<Value, ChangeRetconRepositoryError> {
    let mut statement = connection.prepare(
        "SELECT impact_class,COUNT(*) FROM dependency_edges WHERE project_id=?1 AND (?3=1 OR source_ref IN (SELECT value FROM json_each(?2))) GROUP BY impact_class ORDER BY impact_class",
    )?;
    let rows = statement.query_map(
        libsql_rusqlite::params![
            project_id,
            serde_json::to_string(source_refs)?,
            i64::from(global)
        ],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )?;
    let mut value = serde_json::Map::new();
    for row in rows {
        let (class, count) = row?;
        value.insert(class, json!(count));
    }
    Ok(Value::Object(value))
}

/// Produces a bounded SQLite fingerprint of the selected dependency source set.
/// The aggregate is evaluated inside SQLite and returns one summary row; the
/// graph is never materialized in Rust or sent to the WebView.  The boundary
/// includes source/target/hash extrema and byte totals, which is sufficient to
/// detect the source mutations that can invalidate a saved ImpactSet while
/// keeping the 3m preflight on the indexed database path.
pub fn dependency_fingerprint(
    connection: &Connection,
    project_id: &str,
    source_refs: &Value,
    global: bool,
) -> Result<String, ChangeRetconRepositoryError> {
    let source_refs = serde_json::to_string(source_refs)?;
    let summary: (i64, String, String, String, String, String, String, i64) = connection.query_row(
        "SELECT COUNT(*),COALESCE(MIN(edge_id),''),COALESCE(MAX(edge_id),''),COALESCE(MIN(source_ref),''),COALESCE(MAX(source_ref),''),COALESCE(MIN(target_ref),''),COALESCE(MAX(source_hash),''),COALESCE(SUM(length(edge_id)+length(source_ref)+length(target_ref)+length(source_hash)),0) FROM dependency_edges WHERE project_id=?1 AND (?3=1 OR source_ref IN (SELECT value FROM json_each(?2)))",
        libsql_rusqlite::params![project_id, source_refs, i64::from(global)],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
    )?;
    crate::hash::sha256_canonical(&json!({
        "count":summary.0,
        "minEdgeId":summary.1,
        "maxEdgeId":summary.2,
        "minSourceRef":summary.3,
        "maxSourceRef":summary.4,
        "minTargetRef":summary.5,
        "maxSourceHash":summary.6,
        "totalBytes":summary.7
    }))
    .map_err(Into::into)
}

#[derive(Debug, Clone)]
pub struct DependencySnapshot {
    pub total_edges: i64,
    pub class_counts: Value,
    pub fingerprint: String,
}

const DEPENDENCY_SUMMARY_KEY: &str = "b12.dependencySummary.global";

/// Computes every deterministic preflight input in one fixed aggregate.  In
/// particular, this avoids a separate GROUP BY sort and three independent
/// passes over a million-row index.
fn dependency_snapshot_uncached(
    connection: &Connection,
    project_id: &str,
    source_refs: &Value,
    global: bool,
) -> Result<DependencySnapshot, ChangeRetconRepositoryError> {
    let source_refs = serde_json::to_string(source_refs)?;
    let row: (
        i64,
        i64,
        i64,
        i64,
        i64,
        String,
        String,
        String,
        String,
        String,
    ) = connection.query_row(
        "SELECT COUNT(*),COALESCE(SUM(CASE WHEN impact_class='HARD' THEN 1 ELSE 0 END),0),COALESCE(SUM(CASE WHEN impact_class='REVIEW_REQUIRED' THEN 1 ELSE 0 END),0),COALESCE(SUM(CASE WHEN impact_class='SOFT' THEN 1 ELSE 0 END),0),COALESCE(SUM(CASE WHEN impact_class='EVIDENCE' THEN 1 ELSE 0 END),0),COALESCE(MIN(edge_id),''),COALESCE(MAX(edge_id),''),COALESCE(MIN(source_ref),''),COALESCE(MAX(target_ref),''),COALESCE(MIN(source_hash),'') || '|' || COALESCE(MAX(source_hash),'') FROM dependency_edges WHERE project_id=?1 AND (?3=1 OR source_ref IN (SELECT value FROM json_each(?2)))",
        libsql_rusqlite::params![project_id, source_refs, i64::from(global)],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
            ))
        },
    )?;
    let class_counts = json!({
        "HARD":row.1,
        "REVIEW_REQUIRED":row.2,
        "SOFT":row.3,
        "EVIDENCE":row.4
    });
    let fingerprint = crate::hash::sha256_canonical(&json!({
        "count":row.0,
        "minEdgeId":row.5,
        "maxEdgeId":row.6,
        "minSourceRef":row.7,
        "maxTargetRef":row.8,
        "sourceHashBounds":row.9
    }))?;
    Ok(DependencySnapshot {
        total_edges: row.0,
        class_counts,
        fingerprint,
    })
}

pub fn dependency_snapshot(
    connection: &Connection,
    project_id: &str,
    source_refs: &Value,
    global: bool,
) -> Result<DependencySnapshot, ChangeRetconRepositoryError> {
    if global {
        if let Some(value) = connection
            .query_row(
                "SELECT setting_value FROM project_settings WHERE project_id=?1 AND setting_key=?2",
                libsql_rusqlite::params![project_id, DEPENDENCY_SUMMARY_KEY],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            if let Ok(summary) = serde_json::from_str::<Value>(&value) {
                if let (Some(total_edges), Some(fingerprint)) = (
                    summary.get("totalEdges").and_then(Value::as_i64),
                    summary.get("fingerprint").and_then(Value::as_str),
                ) {
                    let epoch = summary
                        .get("mutationEpoch")
                        .and_then(Value::as_i64)
                        .unwrap_or(0);
                    let fingerprint = crate::hash::sha256_canonical(&json!({
                        "base":fingerprint,
                        "mutationEpoch":epoch
                    }))?;
                    return Ok(DependencySnapshot {
                        total_edges,
                        class_counts: summary
                            .get("classCounts")
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                        fingerprint,
                    });
                }
            }
        }
    }
    dependency_snapshot_uncached(connection, project_id, source_refs, global)
}

pub fn rebuild_global_dependency_summary(
    connection: &Connection,
    project_id: &str,
) -> Result<DependencySnapshot, ChangeRetconRepositoryError> {
    let snapshot = dependency_snapshot_uncached(connection, project_id, &json!([]), true)?;
    let value = json!({
        "totalEdges":snapshot.total_edges,
        "classCounts":snapshot.class_counts,
        "fingerprint":snapshot.fingerprint,
        "mutationEpoch":0
    });
    connection.execute(
        "INSERT INTO project_settings(project_id,setting_key,setting_value,updated_at) VALUES (?1,?2,?3,COALESCE((SELECT updated_at FROM project_settings WHERE project_id=?1 AND setting_key=?2),'1970-01-01T00:00:00Z')) ON CONFLICT(project_id,setting_key) DO UPDATE SET setting_value=excluded.setting_value",
        libsql_rusqlite::params![project_id, DEPENDENCY_SUMMARY_KEY, value.to_string()],
    )?;
    Ok(snapshot)
}

pub fn record_dependency_delta(
    connection: &Connection,
    project_id: &str,
    edge_id: &str,
    source_ref: &str,
    target_ref: &str,
    impact_class: &str,
    source_hash: &str,
    existed: bool,
) -> Result<(), ChangeRetconRepositoryError> {
    let current: Option<String> = connection
        .query_row(
            "SELECT setting_value FROM project_settings WHERE project_id=?1 AND setting_key=?2",
            libsql_rusqlite::params![project_id, DEPENDENCY_SUMMARY_KEY],
            |row| row.get(0),
        )
        .optional()?;
    let Some(current) = current else {
        return Ok(());
    };
    let mut summary: Value = serde_json::from_str(&current)?;
    if !existed {
        let total = summary
            .get("totalEdges")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            + 1;
        summary["totalEdges"] = json!(total);
        let mut class_counts = summary
            .get("classCounts")
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        let count = class_counts
            .get(impact_class)
            .and_then(Value::as_i64)
            .unwrap_or(0)
            + 1;
        class_counts[impact_class] = json!(count);
        summary["classCounts"] = class_counts;
    }
    let prior = summary
        .get("fingerprint")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    summary["fingerprint"] = json!(crate::hash::sha256_canonical(&json!({
        "prior":prior,
        "edgeId":edge_id,
        "sourceRef":source_ref,
        "targetRef":target_ref,
        "impactClass":impact_class,
        "sourceHash":source_hash
    }))?);
    connection.execute(
        "UPDATE project_settings SET setting_value=?1 WHERE project_id=?2 AND setting_key=?3",
        libsql_rusqlite::params![summary.to_string(), project_id, DEPENDENCY_SUMMARY_KEY],
    )?;
    Ok(())
}

pub fn dependency_page(
    connection: &Connection,
    project_id: &str,
    source_refs: &Value,
    global: bool,
    offset: i64,
    limit: i64,
) -> Result<Vec<ImpactEdge>, ChangeRetconRepositoryError> {
    let mut statement = connection.prepare(
        "SELECT edge_id,source_ref,target_ref,dependency_type,scope,impact_class,source_hash FROM dependency_edges WHERE project_id=?1 AND (?3=1 OR source_ref IN (SELECT value FROM json_each(?2))) ORDER BY edge_id LIMIT ?4 OFFSET ?5",
    )?;
    let rows = statement.query_map(
        libsql_rusqlite::params![
            project_id,
            serde_json::to_string(source_refs)?,
            i64::from(global),
            limit,
            offset
        ],
        |row| {
            Ok(ImpactEdge {
                edge_id: row.get(0)?,
                source_ref: row.get(1)?,
                target_ref: row.get(2)?,
                dependency_type: row.get(3)?,
                scope: row.get(4)?,
                impact_class: row.get(5)?,
                source_hash: row.get(6)?,
            })
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn insert_decision(
    connection: &Connection,
    project_id: &str,
    decision_id: &str,
    decision_type: &str,
    outcome: &str,
    reason: &str,
    source_ref: &str,
    payload: &Value,
    now: &str,
) -> Result<(), ChangeRetconRepositoryError> {
    connection.execute(
        "INSERT INTO decisions(decision_id,project_id,decision_type,outcome,reason,source_ref,payload_json,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8) ON CONFLICT(decision_id) DO UPDATE SET decision_type=excluded.decision_type,outcome=excluded.outcome,reason=excluded.reason,source_ref=excluded.source_ref,payload_json=excluded.payload_json,updated_at=excluded.updated_at",
        libsql_rusqlite::params![
            decision_id,
            project_id,
            decision_type,
            outcome,
            reason,
            source_ref,
            serde_json::to_string(payload)?,
            now
        ],
    )?;
    Ok(())
}

pub fn insert_retcon_plan(
    connection: &Connection,
    project_id: &str,
    retcon_plan_id: &str,
    target_fact: &Value,
    reason: &str,
    fix_type: &str,
    downstream_impact: &Value,
    now: &str,
) -> Result<(), ChangeRetconRepositoryError> {
    connection.execute(
        "INSERT INTO retcon_plans(retcon_plan_id,project_id,target_fact_json,reason,fix_type,downstream_impact_json,requires_resigning,estimated_cost_cents,status,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'PENDING_APPROVAL',?9,?9)",
        libsql_rusqlite::params![
            retcon_plan_id,
            project_id,
            serde_json::to_string(target_fact)?,
            reason,
            fix_type,
            serde_json::to_string(downstream_impact)?,
            i64::from(
                downstream_impact
                    .get("signingImpact")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
            downstream_impact
                .get("estimatedCostCents")
                .and_then(Value::as_i64)
                .unwrap_or(0),
            now
        ],
    )?;
    Ok(())
}

pub fn load_retcon_plan(
    connection: &Connection,
    project_id: &str,
    retcon_plan_id: &str,
) -> Result<Option<RetconPlanRecord>, ChangeRetconRepositoryError> {
    connection
        .query_row(
            "SELECT retcon_plan_id,target_fact_json,reason,fix_type,downstream_impact_json,status,decision_ref FROM retcon_plans WHERE project_id=?1 AND retcon_plan_id=?2",
            libsql_rusqlite::params![project_id, retcon_plan_id],
            |row| {
                let target: String = row.get(1)?;
                let downstream: String = row.get(4)?;
                Ok(RetconPlanRecord {
                    retcon_plan_id: row.get(0)?,
                    target_fact: serde_json::from_str(&target).unwrap_or_else(|_| json!({})),
                    reason: row.get(2)?,
                    fix_type: row.get(3)?,
                    downstream_impact: serde_json::from_str(&downstream).unwrap_or_else(|_| json!({})),
                    status: row.get(5)?,
                    decision_ref: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn update_retcon_plan(
    connection: &Connection,
    project_id: &str,
    retcon_plan_id: &str,
    status: &str,
    decision_ref: Option<&str>,
    now: &str,
) -> Result<(), ChangeRetconRepositoryError> {
    connection.execute(
        "UPDATE retcon_plans SET status=?1,decision_ref=COALESCE(?2,decision_ref),updated_at=?3 WHERE project_id=?4 AND retcon_plan_id=?5",
        libsql_rusqlite::params![status, decision_ref, now, project_id, retcon_plan_id],
    )?;
    Ok(())
}

pub fn current_fact(
    connection: &Connection,
    project_id: &str,
    fact_id: &str,
) -> Result<Option<Value>, ChangeRetconRepositoryError> {
    connection
        .query_row(
            "SELECT fact_id,subject_id,predicate,object_json,valid_from_chapter_id,valid_to_chapter_id,story_time_from,story_time_to,source_artifact_id,source_version,authority_level,superseded_by,source_chapter_id,evidence_range_json FROM canon_facts WHERE project_id=?1 AND fact_id=?2",
            libsql_rusqlite::params![project_id, fact_id],
            |row| {
                let object: String = row.get(3)?;
                Ok(json!({
                    "factId":row.get::<_,String>(0)?,
                    "subjectId":row.get::<_,String>(1)?,
                    "predicate":row.get::<_,String>(2)?,
                    "objectJson":serde_json::from_str::<Value>(&object).unwrap_or(Value::Null),
                    "validFromChapterId":row.get::<_,Option<String>>(4)?,
                    "validToChapterId":row.get::<_,Option<String>>(5)?,
                    "storyTimeFrom":row.get::<_,Option<String>>(6)?,
                    "storyTimeTo":row.get::<_,Option<String>>(7)?,
                    "sourceArtifactId":row.get::<_,Option<String>>(8)?,
                    "sourceVersion":row.get::<_,Option<i64>>(9)?,
                    "authorityLevel":row.get::<_,String>(10)?,
                    "supersededBy":row.get::<_,Option<String>>(11)?,
                    "sourceChapterId":row.get::<_,Option<String>>(12)?,
                    "evidenceRangeJson":row.get::<_,Option<String>>(13)?
                }))
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn activate_stale_queue(
    connection: &Connection,
    project_id: &str,
    change_request_id: &str,
    impact_set_id: &str,
    now: &str,
) -> Result<i64, ChangeRetconRepositoryError> {
    let changed = connection.execute(
        "INSERT OR IGNORE INTO stale_queue(stale_id,project_id,change_request_id,target_ref,stale_reason,revalidate_class,processed,created_at,updated_at) SELECT 'stale:'||?2||':'||edge.edge_id,?1,?2,edge.target_ref,'B12_CHANGE_IMPACT',CASE edge.impact_class WHEN 'HARD' THEN 'RETCON' WHEN 'REVIEW_REQUIRED' THEN 'REWRITE' ELSE 'REVALIDATE_ONLY' END,0,?4,?4 FROM dependency_edges edge WHERE edge.project_id=?1 AND (edge.source_ref IN (SELECT value FROM json_each(json_extract((SELECT impact_seed_json FROM change_requests WHERE project_id=?1 AND change_request_id=?2),'$.sourceRefs'))) OR edge.target_ref IN (SELECT value FROM json_each(json_extract((SELECT impact_seed_json FROM change_requests WHERE project_id=?1 AND change_request_id=?2),'$.targetRefs'))) OR EXISTS (SELECT 1 FROM impact_sets impact WHERE impact.project_id=?1 AND impact.impact_set_id=?3 AND impact.scope_class='GLOBAL'))",
        libsql_rusqlite::params![project_id, change_request_id, impact_set_id, now],
    )?;
    Ok(changed as i64)
}

pub fn mark_applied(
    connection: &Connection,
    project_id: &str,
    change_request_id: &str,
    impact_set_id: &str,
    retcon_plan_id: Option<&str>,
    seed: &Value,
    receipt_id: &str,
    now: &str,
) -> Result<i64, ChangeRetconRepositoryError> {
    let mut updated_seed = seed.clone();
    updated_seed["commitReceiptId"] = json!(receipt_id);
    updated_seed["postverifyStatus"] = json!("VERIFIED");
    update_change_request(
        connection,
        project_id,
        change_request_id,
        "APPLIED",
        &updated_seed,
        None,
        now,
    )?;
    let (pages, expanded_edges, checkpoint_offset): (String, i64, i64) = connection.query_row(
        "SELECT processed_pages_json,expanded_edges,checkpoint_offset FROM impact_sets WHERE project_id=?1 AND impact_set_id=?2",
        libsql_rusqlite::params![project_id, impact_set_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    update_impact_set(
        connection,
        project_id,
        impact_set_id,
        &serde_json::from_str(&pages).unwrap_or_else(|_| json!([])),
        expanded_edges,
        checkpoint_offset,
        "COMPLETED",
        "COMMITTED",
        now,
    )?;
    if let Some(plan_id) = retcon_plan_id {
        update_retcon_plan(connection, project_id, plan_id, "APPLIED", None, now)?;
    }
    activate_stale_queue(
        connection,
        project_id,
        change_request_id,
        impact_set_id,
        now,
    )
}

pub fn change_request_detail(
    connection: &Connection,
    project_id: &str,
    change_request_id: &str,
    page_size: i64,
    cursor: i64,
) -> Result<Option<Value>, ChangeRetconRepositoryError> {
    let change = match load_change_request(connection, project_id, change_request_id)? {
        Some(change) => change,
        None => return Ok(None),
    };
    let impact = load_impact_set(connection, project_id, change_request_id)?;
    let (impact_value, impact_digest) = if let Some(impact) = impact {
        let query_global = impact
            .estimate
            .get("queryGlobal")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let refs = change
            .impact_seed
            .get("sourceRefs")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let digest = impact
            .estimate
            .get("impactDigest")
            .cloned()
            .unwrap_or(Value::Null);
        let items = dependency_page(
            connection,
            project_id,
            &refs,
            query_global,
            cursor.max(0),
            page_size.clamp(1, MAX_DETAIL_PAGE),
        )?;
        (
            json!({
                "impactSetId":impact.impact_set_id,
                "scopeClass":impact.scope_class,
                "status":impact.status,
                "totalEdges":impact.total_edges,
                "expandedEdges":impact.expanded_edges,
                "checkpointOffset":impact.checkpoint_offset,
                "items":items,
                "impactDigest":digest,
                "bounded":true,
                "paged":true
            }),
            digest,
        )
    } else {
        (
            json!({"status":"NOT_STARTED","items":[],"bounded":true,"paged":true}),
            Value::Null,
        )
    };
    Ok(Some(json!({
        "changeRequestId":change.change_request_id,
        "changeType":change.change_type,
        "scope":change.scope,
        "summary":change.summary,
        "status":change.status,
        "basedOnRevision":change.impact_seed.get("basedOnRevision").cloned().unwrap_or(Value::Null),
        "impactDigest":impact_digest,
        "impact":impact_value,
        "decisionId":change.decision_ref,
        "retconPlanId":change.impact_seed.get("retconPlanId").cloned().unwrap_or(Value::Null),
        "postverifyStatus":change.impact_seed.get("postverifyStatus").cloned().unwrap_or(Value::Null)
    })))
}

const MAX_DETAIL_PAGE: i64 = 100;
