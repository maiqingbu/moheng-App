//! B08 Production Center bounded read projection owner.

use libsql_rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum ProductionRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("serialization: {0}")]
    Serialization(String),
    #[error("narrative: {0}")]
    Narrative(String),
}

pub struct ProductionRepository;

type ProductionJobRow = (String, String, Option<String>, String, Option<String>);

fn configured_budget_cents(
    connection: &Connection,
    project_id: &str,
    setting_key: &str,
) -> Result<Option<i64>, ProductionRepositoryError> {
    let raw: Option<String> = connection
        .query_row(
            "SELECT setting_value FROM project_settings WHERE project_id=?1 AND setting_key=?2",
            libsql_rusqlite::params![project_id, setting_key],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&raw)
        .map_err(|error| ProductionRepositoryError::Serialization(error.to_string()))?;
    if value.is_null() {
        return Ok(None);
    }
    let amount = value
        .as_f64()
        .filter(|amount| amount.is_finite() && *amount >= 0.0)
        .ok_or_else(|| ProductionRepositoryError::Serialization("invalid budget setting".into()))?;
    Ok(Some((amount * 100.0).ceil() as i64))
}

fn production_cost_summary(
    connection: &Connection,
    project_id: &str,
    chapter_id: Option<&str>,
    project_total_cents: i64,
) -> Result<Value, ProductionRepositoryError> {
    let chapter_budget = configured_budget_cents(connection, project_id, "chapterBudget")?;
    let monthly_budget = configured_budget_cents(connection, project_id, "monthlyBudget")?;
    let chapter_budget_id = if let Some(chapter_id) = chapter_id {
        connection
            .query_row(
                "SELECT budget_id FROM chapter_production_budgets WHERE project_id=?1 AND chapter_id=?2",
                libsql_rusqlite::params![project_id, chapter_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
    } else {
        None
    };
    let chapter_used = if let Some(budget_id) = chapter_budget_id.as_deref() {
        let actual: i64 = connection.query_row(
            "SELECT COALESCE(SUM(cost_cents),0) FROM model_cost_ledger WHERE project_id=?1 AND budget_id=?2 AND cost_type='ACTUAL'",
            libsql_rusqlite::params![project_id,budget_id],
            |row| row.get(0),
        )?;
        let unknown: i64 = connection.query_row(
            "SELECT COALESCE(SUM(reservation.settled_cents),0) FROM budget_reservations reservation LEFT JOIN model_usage usage ON usage.project_id=reservation.project_id AND usage.model_run_id=reservation.model_run_id WHERE reservation.project_id=?1 AND reservation.budget_ref=?2 AND reservation.status='SETTLED' AND usage.model_run_id IS NULL",
            libsql_rusqlite::params![project_id,budget_id],
            |row| row.get(0),
        )?;
        let reserved: i64 = connection.query_row(
            "SELECT COALESCE(SUM(reserved_cents),0) FROM budget_reservations WHERE project_id=?1 AND budget_ref=?2 AND status IN ('RESERVED','OVER_LIMIT')",
            libsql_rusqlite::params![project_id,budget_id],
            |row| row.get(0),
        )?;
        actual + unknown + reserved
    } else {
        0
    };
    let month_actual: i64 = connection.query_row(
        "SELECT COALESCE(SUM(cost_cents),0) FROM model_cost_ledger WHERE project_id=?1 AND cost_type='ACTUAL' AND strftime('%Y-%m',datetime(created_at,'localtime'))=strftime('%Y-%m','now','localtime')",
        [project_id],
        |row| row.get(0),
    )?;
    let month_unknown: i64 = connection.query_row(
        "SELECT COALESCE(SUM(reservation.settled_cents),0) FROM budget_reservations reservation LEFT JOIN model_usage usage ON usage.project_id=reservation.project_id AND usage.model_run_id=reservation.model_run_id WHERE reservation.project_id=?1 AND reservation.status='SETTLED' AND usage.model_run_id IS NULL AND strftime('%Y-%m',datetime(reservation.created_at,'localtime'))=strftime('%Y-%m','now','localtime')",
        [project_id],
        |row| row.get(0),
    )?;
    let month_reserved: i64 = connection.query_row(
        "SELECT COALESCE(SUM(reserved_cents),0) FROM budget_reservations WHERE project_id=?1 AND status IN ('RESERVED','OVER_LIMIT') AND strftime('%Y-%m',datetime(created_at,'localtime'))=strftime('%Y-%m','now','localtime')",
        [project_id],
        |row| row.get(0),
    )?;
    let month_used = month_actual + month_unknown + month_reserved;
    let chapter_remaining = chapter_budget.map(|limit| (limit - chapter_used).max(0));
    let monthly_remaining = monthly_budget.map(|limit| (limit - month_used).max(0));
    Ok(json!({
        "currentVolume":0,
        "projectTotal":project_total_cents as f64 / 100.0,
        "budgetRemaining":monthly_remaining.map(|value| value as f64 / 100.0),
        "chapterUsed":chapter_used as f64 / 100.0,
        "chapterBudget":chapter_budget.map(|value| value as f64 / 100.0),
        "chapterRemaining":chapter_remaining.map(|value| value as f64 / 100.0),
        "monthUsed":month_used as f64 / 100.0,
        "monthlyBudget":monthly_budget.map(|value| value as f64 / 100.0),
        "monthRemaining":monthly_remaining.map(|value| value as f64 / 100.0)
    }))
}

fn b02_phase_steps(phases: &[&str], current: Option<&str>, status: &str) -> Vec<Value> {
    let current_index = current.and_then(|phase| phases.iter().position(|item| *item == phase));
    phases
        .iter()
        .enumerate()
        .map(|(index, phase)| {
            let step_status = if status == "COMPLETED" {
                "done"
            } else if let Some(current_index) = current_index {
                if index < current_index {
                    "done"
                } else if index == current_index {
                    match status {
                        "FAILED" => "failed",
                        "RUNNING" | "WAITING_MODEL" => "active",
                        "PAUSED" | "WAITING_DECISION" | "WAITING_BUDGET" => "waiting",
                        _ => "pending",
                    }
                } else {
                    "pending"
                }
            } else {
                "pending"
            };
            json!({"id":phase,"label":phase,"status":step_status})
        })
        .collect()
}

fn b02_phase_label(phase: &str) -> &'static str {
    match phase {
        "READ_OPENING_CANON" => "读取三章事实",
        "SUMMARIZE_OPENING" => "汇总开篇状态",
        "EVALUATE_SIGNING" => "校验开篇评估",
        "COMMIT_RECEIPT" => "提交正式签约回执",
        "GENERATE_BOOK_PLAN" => "生成正式总纲",
        "VALIDATE_CONSTRAINTS" => "校验约束",
        "COMMIT_BOOK_PLAN" => "提交正式版本",
        "COMPLETED" => "已完成",
        _ => "处理中",
    }
}

fn b02_phase_steps_with_labels(phases: &[&str], current: Option<&str>, status: &str) -> Vec<Value> {
    b02_phase_steps(phases, current, status)
        .into_iter()
        .map(|mut step| {
            if let Some(id) = step.get("id").and_then(Value::as_str) {
                step["label"] = json!(b02_phase_label(id));
            }
            step
        })
        .collect()
}

/// Model runs a chapter gets per allowance; also the size of a manual reset.
/// Owner decision (2026-10-19): 16, since each chapter is now also reviewed
/// and rewritten.
pub const CHAPTER_RUN_ALLOWANCE: i64 = 16;

impl ProductionRepository {
    /// Model runs the chapter has used against its limit: the larger of the
    /// commit gate's count (model runs of the chapter's jobs) and the runtime
    /// budget's count (reservations), so a limit above it satisfies both.
    pub fn chapter_quota(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
    ) -> Result<Option<Value>, ProductionRepositoryError> {
        let Some((budget_id, limit)) = connection
            .query_row(
                "SELECT budget_id, hard_max_model_runs FROM chapter_production_budgets WHERE project_id=?1 AND chapter_id=?2",
                [project_id, chapter_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let used = crate::repositories::runtime::chapter_model_runs_used(
            connection, project_id, chapter_id, &budget_id,
        )?;
        Ok(Some(
            json!({"used":used,"limit":limit,"allowance":CHAPTER_RUN_ALLOWANCE}),
        ))
    }

    /// Owner decision (2026-10-17): the user may give a chapter a fresh
    /// allowance by hand. History is kept; the limit moves to what is already
    /// used plus one allowance, and jobs held for budget become retryable.
    pub fn reset_chapter_quota(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
        now: &str,
    ) -> Result<Value, ProductionRepositoryError> {
        let quota = Self::chapter_quota(connection, project_id, chapter_id)?.ok_or_else(|| {
            ProductionRepositoryError::Narrative("CHAPTER_BUDGET_NOT_FOUND".into())
        })?;
        let used = quota["used"].as_i64().unwrap_or(0);
        let limit = used + CHAPTER_RUN_ALLOWANCE;
        connection.execute(
            "UPDATE chapter_production_budgets SET hard_max_model_runs=?3 WHERE project_id=?1 AND chapter_id=?2",
            libsql_rusqlite::params![project_id, chapter_id, limit],
        )?;
        connection.execute(
            "UPDATE jobs SET status='FAILED', updated_at=?3 WHERE project_id=?1 AND chapter_id=?2 AND status='WAITING_BUDGET'",
            [project_id, chapter_id, now],
        )?;
        connection.execute(
            "UPDATE project_runtime_state SET run_state='IDLE', block_reason=NULL, updated_at=?2 WHERE project_id=?1 AND run_state='WAITING_BUDGET'",
            [project_id, now],
        )?;
        connection.execute(
            "INSERT INTO decisions(decision_id, project_id, decision_type, outcome, reason, payload_json, created_at, updated_at) VALUES (?1, ?2, 'CHAPTER_QUOTA_RESET', 'APPLIED', '用户手动重置本章额度', ?3, ?4, ?4) ON CONFLICT(decision_id) DO NOTHING",
            libsql_rusqlite::params![
                format!("decision:quota-reset:{chapter_id}:{used}"),
                project_id,
                json!({"chapterId":chapter_id,"usedBefore":used,"limitBefore":quota["limit"],"limitAfter":limit}).to_string(),
                now
            ],
        )?;
        Ok(json!({"used":used,"limit":limit,"allowance":CHAPTER_RUN_ALLOWANCE}))
    }

    pub fn center_projection(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Value, ProductionRepositoryError> {
        let runtime: (String, i64, i64, i64, String, String) = connection
            .query_row(
                "SELECT COALESCE(run_state,'IDLE'), COALESCE(committed_chapters,0), COALESCE(total_cost_cents,0), COALESCE(candidate_chapters,0), COALESCE(signing_status,'NOT_STARTED'), COALESCE(current_phase,'SETUP') FROM project_runtime_state WHERE project_id=?1",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )
            .unwrap_or_else(|_| ("IDLE".into(), 0, 0, 0, "NOT_STARTED".into(), "SETUP".into()));
        let actual_committed: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM chapters WHERE project_id=?1 AND status='COMMITTED'",
                [project_id],
                |row| row.get(0),
            )
            .unwrap_or(runtime.1);
        let runtime = (
            runtime.0,
            actual_committed,
            runtime.2,
            runtime.3,
            runtime.4,
            runtime.5,
        );
        let gate_policy_ref =
            crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
                connection,
                project_id,
                "GatePolicy",
            )
            .map_err(|error| ProductionRepositoryError::Narrative(error.to_string()))?;
        let gate_policy = if let Some(reference) = gate_policy_ref {
            let payload =
                crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                    connection, project_id, &reference,
                )
                .map_err(|error| ProductionRepositoryError::Narrative(error.to_string()))?
                .ok_or_else(|| {
                    ProductionRepositoryError::Narrative("B08_GATE_POLICY_REQUIRED".into())
                })?;
            json!({
                "version":reference.version,
                "policyId":payload.get("policyId").cloned().unwrap_or(Value::Null),
                "needsUpgrade":payload.get("policyId").and_then(Value::as_str)==Some("b11-opening"),
                "resourceRef":payload.get("resourceRef").cloned().unwrap_or(Value::Null),
                "rules":payload.pointer("/canonSemantics/rules").cloned().unwrap_or_else(|| json!([]))
            })
        } else {
            json!({"version":0,"policyId":Value::Null,"needsUpgrade":true,"resourceRef":Value::Null,"rules":[]})
        };
        let opening_progress =
            crate::repositories::project_entry::opening_progress(connection, project_id)
                .map_err(|error| ProductionRepositoryError::Serialization(error.to_string()))?;
        let opening_progress = serde_json::to_value(opening_progress)
            .map_err(|error| ProductionRepositoryError::Serialization(error.to_string()))?;
        let job: Option<ProductionJobRow> = connection
            .query_row(
                "SELECT job_id, COALESCE(current_stage,'IDLE'), chapter_id, status, error_code FROM jobs WHERE project_id=?1 ORDER BY CASE WHEN job_type='CHAPTER' THEN 0 ELSE 1 END, updated_at DESC, job_id DESC LIMIT 1",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        let b02_operation: Option<(String, String, String, String, Option<String>)> = connection
            .query_row(
                "SELECT job_id,book_id,current_stage,status,error_code FROM jobs WHERE project_id=?1 AND book_id IN (?2,?3) ORDER BY updated_at DESC,job_id DESC LIMIT 1",
                libsql_rusqlite::params![
                    project_id,
                    crate::repositories::runtime::B02_OPENING_EVALUATION_KIND,
                    crate::repositories::runtime::B02_BOOK_PLAN_KIND
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        let current_stage = job
            .as_ref()
            .map(|(_, stage, _, _, _)| stage.clone())
            .unwrap_or_else(|| "IDLE".into());
        let active_detail = job
            .as_ref()
            .map(|(id, _, chapter, status, error_code)| {
                format!(
                    "{status} · {id} · {}{}",
                    chapter.as_deref().unwrap_or("未绑定章节"),
                    error_code
                        .as_deref()
                        .map(|value| format!(" · {value}"))
                        .unwrap_or_default()
                )
            })
            .unwrap_or_else(|| "暂无生产任务".into());
        let stages = [
            "CONTEXT",
            "PLAN",
            "RENDER",
            "CANDIDATE",
            "AUDIT",
            "GATE",
            "COMMIT",
            "RECONCILE",
            "COMPLETED",
        ];
        let stage_order = stages.iter().position(|stage| *stage == current_stage);
        let timeline_status = |order: usize| {
            let Some((_, _, _, status, _)) = job.as_ref() else {
                return "pending";
            };
            if status == "COMPLETED" {
                return "done";
            }
            let Some(current_order) = stage_order else {
                return "pending";
            };
            if order < current_order {
                return "done";
            }
            if order > current_order {
                return "pending";
            }
            match status.as_str() {
                "FAILED" => "failed",
                "BLOCKED" | "CANCELLED" => "blocked",
                "PAUSED" | "WAITING_DECISION" | "WAITING_BUDGET" => "waiting",
                "RUNNING" | "WAITING_MODEL" => "active",
                _ => "pending",
            }
        };
        let timeline = stages
            .iter()
            .enumerate()
            .map(|(order, stage)| {
                json!({
                    "id": format!("b08:{stage}"), "order": order + 1, "title": stage,
                    "description": "B08 Chapter Production Runtime", "status": timeline_status(order)
                })
            })
            .collect::<Vec<_>>();
        let mut artifacts = connection.prepare(
            "SELECT artifact_id, artifact_type, artifact_status, payload_hash, version FROM artifacts WHERE project_id=?1 ORDER BY created_at DESC, artifact_id DESC LIMIT 20",
        )?;
        let artifact_items = artifacts
            .query_map([project_id], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?, "title": row.get::<_, String>(1)?,
                    "summary": format!("v{} · {} · {}", row.get::<_, i64>(4)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?),
                    "status": if row.get::<_, String>(2)? == "COMMITTED" { "official" } else { "candidate" }
                }))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let task = job
            .as_ref()
            .map(|(id, _, chapter, status, _)| {
                json!({"id":id,"title":"Chapter Production","description":format!("{} · {}", chapter.as_deref().unwrap_or(""), status),"status":if status=="FAILED" {"failed"} else if status=="COMPLETED" {"done"} else if matches!(status.as_str(), "RUNNING" | "WAITING_MODEL") {"active"} else {"waiting"},"progress":if status=="COMPLETED" {json!(1.0)} else {Value::Null}})
            })
            .into_iter()
            .collect::<Vec<_>>();
        let next_chapter: Option<(String, i64, String, String)> = connection
            .query_row(
                "SELECT c.chapter_id,c.chapter_no,c.title,c.volume_id FROM chapters c WHERE c.project_id=?1 AND c.status!='COMMITTED' ORDER BY c.chapter_no,c.chapter_id LIMIT 1",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let active_chapter_id = job
            .as_ref()
            .and_then(|(_, _, chapter, _, _)| chapter.clone())
            .or_else(|| next_chapter.as_ref().map(|row| row.0.clone()));
        let plan_row: Option<(String, i64, String, Value)> = if let Some(chapter_id) =
            active_chapter_id.as_ref()
        {
            connection
                .query_row(
                    "SELECT h.artifact_id,h.version,h.payload_hash,p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan' AND h.scope_level='CHAPTER' AND h.scope_key=?2",
                    libsql_rusqlite::params![project_id, json!({"level":"CHAPTER","chapterId":chapter_id}).to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get::<_, String>(3)?)),
                )
                .optional()?
                .map(|(artifact_id, version, payload_hash, raw)| {
                    crate::repositories::narrative::parse_formal_payload("ChapterPlan", &raw)
                    .map(|payload| (artifact_id, version, payload_hash, payload))
                    .map_err(|error| ProductionRepositoryError::Narrative(error.to_string()))
                })
                .transpose()?
        } else {
            None
        };
        let plan_projection = plan_row.as_ref().map(|(id, version, hash, payload)| {
            json!({
                "ref":{"artifactId":id,"version":version,"payloadHash":hash},
                "chapterGoal":payload.get("chapterGoal"), "contextManifestRef":payload.get("contextManifestRef"),
                "activeTaskRefs":payload.get("activeTaskRefs").cloned().unwrap_or_else(|| json!([])),
                "continuityActions":payload.get("continuityActions").cloned().unwrap_or_else(|| json!([])),
                "compiledFromProjectRevision":payload.get("compiledFromProjectRevision"),
                "compiledFromStateDigest":payload.get("compiledFromStateDigest")
            })
        });
        let current_revision: i64 = connection.query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        let stale_status = plan_projection
            .as_ref()
            .map(|plan| {
                if plan
                    .get("compiledFromProjectRevision")
                    .and_then(Value::as_i64)
                    .is_some_and(|revision| revision > current_revision)
                {
                    "PLAN_STALE"
                } else {
                    "VALID_OR_REQUIRES_RELEVANT_RECHECK"
                }
            })
            .unwrap_or("NO_PLAN");
        let scoped_ref = |artifact_type: &str, canonical: bool| -> Option<Value> {
            let chapter_id = active_chapter_id.as_ref()?;
            let sql = if canonical {
                "SELECT json_object('artifactId',h.artifact_id,'version',h.version,'payloadHash',h.payload_hash) FROM canonical_heads h WHERE h.project_id=?1 AND h.artifact_type=?2 AND h.scope_level='CHAPTER' AND h.scope_key=?3"
            } else {
                "SELECT json_object('artifactId',a.artifact_id,'version',a.version,'payloadHash',a.payload_hash) FROM artifacts a WHERE a.project_id=?1 AND a.artifact_type=?2 AND a.scope_level='CHAPTER' AND a.scope_key=?3 ORDER BY a.created_at DESC,a.version DESC LIMIT 1"
            };
            connection
                .query_row(
                    sql,
                    libsql_rusqlite::params![
                        project_id,
                        artifact_type,
                        json!({"level":"CHAPTER","chapterId":chapter_id}).to_string()
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .ok()
                .flatten()
                .and_then(|raw| serde_json::from_str(&raw).ok())
        };
        let candidate_ref = scoped_ref("ChapterCandidate", false);
        let final_ref = scoped_ref("FinalChapter", true);
        let quality_ref = scoped_ref("NarrativeQualityReport", false);
        let taste_profile_ref = scoped_ref("NarrativeTasteProfile", false);
        let payload_for_ref = |reference: &Value| -> Option<Value> {
            let artifact_id = reference.get("artifactId").and_then(Value::as_str)?;
            let version = reference.get("version").and_then(Value::as_i64)?;
            connection
                .query_row(
                    "SELECT payload_json FROM artifact_payloads WHERE artifact_id=?1 AND version=?2",
                    libsql_rusqlite::params![artifact_id, version],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .ok()
                .flatten()
                .and_then(|raw| serde_json::from_str(&raw).ok())
        };
        let creative_skills = candidate_ref
            .as_ref()
            .and_then(payload_for_ref)
            .and_then(|payload| payload.get("creativeEnrichment").cloned())
            .filter(|value| !value.is_null())
            .unwrap_or(Value::Null);
        let quality = quality_ref
            .as_ref()
            .and_then(payload_for_ref)
            .map(|mut payload| {
                payload["ref"] = quality_ref.clone().unwrap_or(Value::Null);
                payload
            })
            .unwrap_or(Value::Null);
        let repair_limit = creative_skills
            .pointer("/context/budget/repairRuns")
            .cloned()
            .unwrap_or_else(|| json!(0));
        let repair_scope = quality
            .get("repairableDimensions")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let repair_projection = quality
            .get("repair")
            .cloned()
            .or_else(|| quality.get("repairEvidence").cloned())
            .unwrap_or_else(|| {
                json!({"status":"NO_REPAIR","run":0,"limit":repair_limit,"transportCalls":0,"scope":repair_scope,"escalation":Value::Null})
            });
        let mut quality = quality;
        if let Some(object) = quality.as_object_mut() {
            object.entry("repair").or_insert(repair_projection);
        }
        let taste_profile = taste_profile_ref
            .as_ref()
            .and_then(payload_for_ref)
            .map(|mut payload| {
                payload["ref"] = taste_profile_ref.clone().unwrap_or(Value::Null);
                payload
            })
            .unwrap_or(Value::Null);
        let model_runs = if let Some((job_id, _, _, _, _)) = &job {
            let mut statement = connection.prepare(
                "SELECT model_run_id,capability,provider,model,status FROM model_runs WHERE project_id=?1 AND job_id=?2 AND capability IN ('PLAN_STRATEGY','RENDER_CHAPTER','AUDIT_EXTRACT') ORDER BY created_at,model_run_id",
            )?;
            let rows = statement
                .query_map(libsql_rusqlite::params![project_id, job_id], |row| {
                    Ok(json!({"modelRunId":row.get::<_,String>(0)?,"capability":row.get::<_,String>(1)?,"provider":row.get::<_,String>(2)?,"model":row.get::<_,String>(3)?,"status":row.get::<_,String>(4)?}))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        } else {
            Vec::new()
        };
        let latest_receipt: Option<String> = active_chapter_id.as_ref().and_then(|chapter_id| connection.query_row(
            "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ProductionReceipt' AND h.scope_level='CHAPTER' AND h.scope_key=?2",
            libsql_rusqlite::params![project_id, json!({"level":"CHAPTER","chapterId":chapter_id}).to_string()], |row| row.get(0),
        ).optional().ok().flatten());
        let reconciliation = latest_receipt
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|value| value.get("reconciliation").cloned())
            .unwrap_or(Value::Null);
        let repair: Option<Value> = connection.query_row(
            "SELECT rp.repair_plan_id,rp.based_on_project_revision,rp.diagnosis_json,rp.finding_refs_json,rp.target_authority_owner,rp.proposed_operations_json,rp.affected_artifacts_json,rp.affected_scopes_json,rp.risk_level,rp.requires_backup,rp.expected_postconditions_json,rp.operation_digest,rp.status,ra.authorization_id,ra.consumed_at,rr.receipt_ref,rr.status,rr.postconditions_json,rr.project_id,rr.repair_plan_id,rr.authorization_id,rr.before_revision,rr.after_revision,rr.operation_digest,rr.owner,rr.owner_receipt_ref,rr.executed_at FROM repair_plans rp LEFT JOIN repair_authorizations ra ON ra.project_id=rp.project_id AND ra.repair_plan_id=rp.repair_plan_id LEFT JOIN repair_execution_receipts rr ON rr.project_id=rp.project_id AND rr.repair_plan_id=rp.repair_plan_id WHERE rp.project_id=?1 ORDER BY rp.created_at DESC,ra.issued_at DESC LIMIT 1",
            [project_id],
            |row| {
                let receipt_ref = row.get::<_, Option<String>>(15)?;
                let receipt_status = row.get::<_, Option<String>>(16)?;
                let receipt_postconditions = row.get::<_, Option<String>>(17)?;
                let receipt_project_id = row.get::<_, Option<String>>(18)?;
                let receipt_plan_id = row.get::<_, Option<String>>(19)?;
                let receipt_authorization_id = row.get::<_, Option<String>>(20)?;
                let receipt_before_revision = row.get::<_, Option<i64>>(21)?;
                let receipt_after_revision = row.get::<_, Option<i64>>(22)?;
                let receipt_operation_digest = row.get::<_, Option<String>>(23)?;
                let receipt_owner = row.get::<_, Option<String>>(24)?;
                let owner_receipt_ref = row.get::<_, Option<String>>(25)?;
                let receipt_executed_at = row.get::<_, Option<String>>(26)?;
                let execution_receipt = receipt_ref.map(|receipt_ref| {
                    json!({
                        "receiptRef":receipt_ref,
                        "projectId":receipt_project_id,
                        "repairPlanId":receipt_plan_id,
                        "authorizationId":receipt_authorization_id,
                        "beforeRevision":receipt_before_revision,
                        "afterRevision":receipt_after_revision,
                        "operationDigest":receipt_operation_digest,
                        "owner":receipt_owner,
                        "ownerReceiptRef":owner_receipt_ref,
                        "executedAt":receipt_executed_at,
                        "status":receipt_status.unwrap_or_default(),
                        "postconditions":serde_json::from_str::<Value>(&receipt_postconditions.unwrap_or_else(|| "[]".into())).unwrap_or_else(|_| json!([]))
                    })
                });
                Ok(json!({"repairPlanId":row.get::<_,String>(0)?,"basedOnProjectRevision":row.get::<_,i64>(1)?,"diagnosis":serde_json::from_str::<Value>(&row.get::<_,String>(2)?).unwrap_or(Value::Null),"findingRefs":serde_json::from_str::<Value>(&row.get::<_,String>(3)?).unwrap_or_else(|_| json!([])),"targetAuthorityOwner":row.get::<_,String>(4)?,"proposedOperations":serde_json::from_str::<Value>(&row.get::<_,String>(5)?).unwrap_or_else(|_| json!([])),"affectedArtifacts":serde_json::from_str::<Value>(&row.get::<_,String>(6)?).unwrap_or_else(|_| json!([])),"affectedScopes":serde_json::from_str::<Value>(&row.get::<_,String>(7)?).unwrap_or_else(|_| json!([])),"riskLevel":row.get::<_,String>(8)?,"requiresBackup":row.get::<_,i64>(9)? != 0,"expectedPostconditions":serde_json::from_str::<Value>(&row.get::<_,String>(10)?).unwrap_or_else(|_| json!([])),"operationDigest":row.get::<_,String>(11)?,"status":row.get::<_,String>(12)?,"authorizationId":row.get::<_,Option<String>>(13)?,"consumedAt":row.get::<_,Option<String>>(14)?,"executionReceipt":execution_receipt}))
            },
        ).optional()?;
        let signing_ref =
            crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
                connection,
                project_id,
                "SimulatedSigningReceipt",
            )
            .map_err(|error| ProductionRepositoryError::Narrative(error.to_string()))?;
        let signing_payload = signing_ref.as_ref().and_then(|reference| {
            crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                connection, project_id, reference,
            )
            .ok()
            .flatten()
        });
        let signing_operation = b02_operation.as_ref().filter(|(_, kind, _, _, _)| {
            kind == crate::repositories::runtime::B02_OPENING_EVALUATION_KIND
        });
        let signing_status = signing_operation
            .filter(|(_, _, _, status, _)| status == "RUNNING" || status == "WAITING_MODEL")
            .map(|_| "IN_REVIEW")
            .or_else(|| {
                signing_payload
                    .as_ref()
                    .and_then(|payload| payload.get("status").and_then(Value::as_str))
                    .or(Some(runtime.4.as_str()))
            })
            .unwrap_or("NOT_STARTED");
        let legacy_revise = signing_status == "REVISE";
        let signing_status = if legacy_revise {
            "FAIL"
        } else {
            signing_status
        };
        let signing_evaluation = if let Some((_, _, phase, status, _)) = signing_operation {
            json!({
                "status": if status == "FAILED" { "FAILED" } else if status == "COMPLETED" { "COMPLETED" } else { "IN_REVIEW" },
                "phase": phase,
                "steps": b02_phase_steps_with_labels(crate::repositories::runtime::B02_SIGNING_PHASES, Some(phase.as_str()), status),
                "summary": "后端正在按阶段读取并核验黄金三章。"
            })
        } else {
            signing_payload
            .as_ref()
            .and_then(|payload| payload.get("evaluation").cloned())
            .unwrap_or_else(|| {
                json!({
                    "status": if signing_status == "IN_REVIEW" { "IN_REVIEW" } else { "NOT_STARTED" },
                    "phase": "IDLE",
                    "steps": [],
                    "summary": "等待三章正式提交后开始开篇评估。"
                })
            })
        };
        let signing_receipt = signing_ref.as_ref().map(|reference| {
            let payload = signing_payload.as_ref();
            json!({
                "id":reference.artifact_id,
                "signedAt":payload.and_then(|value| value.get("evaluatedAt")).cloned().unwrap_or(Value::Null),
                "chapterHashes":payload.and_then(|value| value.get("chapterEvidence")).and_then(Value::as_array).map(|items| items.iter().filter_map(|item| item.pointer("/finalChapterRef/payloadHash").cloned()).collect::<Vec<_>>()).unwrap_or_default(),
                "reviewReportId":payload.and_then(|value| value.get("reviewReportRef")).cloned().unwrap_or(Value::Null),
                "platformPolicyVersion":payload.and_then(|value| value.get("evaluationRuleVersion")).cloned().unwrap_or(Value::Null),
                "ruleSetVersion":payload.and_then(|value| value.get("ruleSetVersion")).cloned().unwrap_or(Value::Null),
                "ruleSetDigest":payload.and_then(|value| value.get("ruleSetDigest")).cloned().unwrap_or(Value::Null),
                "openingContractVersion":payload.and_then(|value| value.pointer("/binding/openingContractRef/version")).cloned().unwrap_or(Value::Null),
                "confirmedPlatform":payload.and_then(|value| value.pointer("/confirmation/platformId")).cloned().unwrap_or(Value::Null),
                "receiptHash":payload.and_then(|value| value.get("receiptHash")).cloned().unwrap_or_else(|| json!(reference.payload_hash))
            })
        });
        let mut signing = json!({
            "status":signing_status,
            "verdict":if legacy_revise {json!("历史签约评估未通过（旧版 REVISE）。")} else {signing_payload.as_ref().and_then(|payload| payload.get("verdict")).cloned().unwrap_or_else(|| json!("等待三章正式提交后开始开篇评估。"))},
            "summary":signing_payload.as_ref().and_then(|payload| payload.get("summary")).cloned().unwrap_or_else(|| json!({})),
            "coreStatus":signing_payload.as_ref().and_then(|payload| payload.get("coreStatus")).cloned().unwrap_or(Value::Null),
            "itemResults":signing_payload.as_ref().and_then(|payload| payload.get("itemResults")).cloned().unwrap_or_else(|| json!([])),
            "platformMatrix":signing_payload.as_ref().and_then(|payload| payload.get("platformMatrix")).cloned().unwrap_or_else(|| json!([])),
            "confirmation":signing_payload.as_ref().and_then(|payload| payload.get("confirmation")).cloned().unwrap_or(Value::Null),
            "declarationPrompt":signing_payload.as_ref().and_then(|payload| payload.get("declarationPrompt")).cloned().unwrap_or(Value::Null),
            "evaluation":signing_evaluation,
            "openingChapterRefs":signing_payload.as_ref().and_then(|payload| payload.get("openingChapterRefs")).cloned().unwrap_or_else(|| json!([])),
            "openingCanonRefs":signing_payload.as_ref().and_then(|payload| payload.get("openingCanonRefs")).cloned().unwrap_or_else(|| json!([])),
            "signingEvidenceRefs":signing_payload.as_ref().and_then(|payload| payload.get("signingEvidenceRefs")).cloned().unwrap_or_else(|| json!([])),
            "receipt":signing_receipt,
            "receiptRef":signing_ref.as_ref().map(|reference| json!({"artifactId":reference.artifact_id,"version":reference.version,"payloadHash":reference.payload_hash}))
        });
        let book_plan_ref =
            crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
                connection, project_id, "BookPlan",
            )
            .map_err(|error| ProductionRepositoryError::Narrative(error.to_string()))?;
        let book_plan_payload = book_plan_ref.as_ref().and_then(|reference| {
            crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                connection, project_id, reference,
            )
            .ok()
            .flatten()
        });
        let mut seed_count = 0_i64;
        let mut book_plans = connection.prepare(
            "SELECT p.payload_json FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='BookPlan'",
        )?;
        for row in book_plans.query_map([project_id], |row| row.get::<_, String>(0))? {
            let payload =
                crate::repositories::narrative::parse_formal_payload("BookPlan", &row?)
                    .map_err(|error| ProductionRepositoryError::Narrative(error.to_string()))?;
            if payload.get("planningMaturity").and_then(Value::as_str) == Some("SEED") {
                seed_count += 1;
            }
        }
        let book_plan_status = book_plan_payload
            .as_ref()
            .and_then(|payload| payload.get("planningMaturity").and_then(Value::as_str))
            .map(|maturity| {
                if maturity == "APPROVED" {
                    "APPROVED"
                } else {
                    "PENDING"
                }
            })
            .unwrap_or("PENDING");
        if book_plan_status == "APPROVED" && signing_ref.is_none() {
            signing["verdict"] = json!("需要先完成签约评估。");
            signing["evaluation"]["summary"] = json!("长篇规划与生产前需要先完成签约评估。");
        }
        let signing_is_current_and_confirmed = signing_ref
            .as_ref()
            .zip(signing_payload.as_ref())
            .is_some_and(|(reference, payload)| {
                payload.get("evaluationRuleVersion").and_then(Value::as_str)
                    == Some(crate::signing::SIGNING_RULESET_VERSION)
                    && payload.get("status").and_then(Value::as_str) == Some("PASS")
                    && payload
                        .pointer("/confirmation/confirmed")
                        .and_then(Value::as_bool)
                        == Some(true)
                    && crate::signing::signing_evaluation_is_current_in_project(
                        connection, project_id, reference,
                    )
                    .unwrap_or(false)
            });
        let book_plan_uses_current_signing = if let (Some(book_plan), Some(signing)) =
            (book_plan_ref.as_ref(), signing_ref.as_ref())
        {
            connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM artifact_dependencies WHERE source_artifact_id=?1 AND source_version=?2 AND target_artifact_id=?3 AND target_version=?4 AND target_payload_hash=?5)",
                    libsql_rusqlite::params![
                        book_plan.artifact_id,
                        book_plan.version,
                        signing.artifact_id,
                        signing.version,
                        signing.payload_hash
                    ],
                    |row| row.get::<_, bool>(0),
                )?
        } else {
            false
        };
        let book_plan_reapproval_required = book_plan_status == "APPROVED"
            && signing_is_current_and_confirmed
            && !book_plan_uses_current_signing;
        let book_plan_operation = b02_operation
            .as_ref()
            .filter(|(_, kind, _, _, _)| kind == crate::repositories::runtime::B02_BOOK_PLAN_KIND);
        let book_plan_steps = if let Some((_, _, phase, status, _)) = book_plan_operation {
            b02_phase_steps_with_labels(
                crate::repositories::runtime::B02_BOOK_PLAN_PHASES,
                Some(phase.as_str()),
                status,
            )
        } else if book_plan_status == "APPROVED" {
            b02_phase_steps_with_labels(
                crate::repositories::runtime::B02_BOOK_PLAN_PHASES,
                Some("COMPLETED"),
                "COMPLETED",
            )
        } else {
            Vec::new()
        };
        let book_plan = json!({
            "status":book_plan_status,
            "ref":book_plan_ref.as_ref().map(|reference| json!({"artifactId":reference.artifact_id,"version":reference.version,"payloadHash":reference.payload_hash})),
            "seedPreserved":seed_count > 0,
            "reapprovalRequired":book_plan_reapproval_required,
            "steps":book_plan_steps
        });
        let next_chapter_has_volume_plan = if let Some((_, _, _, volume_id)) = next_chapter.as_ref()
        {
            crate::repositories::narrative::NarrativeRepository::volume_plan_route(
                connection, project_id, volume_id,
            )
            .map_err(|error| ProductionRepositoryError::Narrative(error.to_string()))?
            .is_some()
        } else {
            false
        };
        let next_chapter_projection = if let Some((id, number, title, volume_id)) = next_chapter {
            let is_opening =
                crate::repositories::project_entry::is_opening_chapter(connection, project_id, &id)
                    .map_err(|error| ProductionRepositoryError::Narrative(error.to_string()))?;
            let execution_mode = if !next_chapter_has_volume_plan && is_opening {
                "OPENING_BOOTSTRAP"
            } else if plan_projection.is_some() {
                "LONG_FORM"
            } else {
                "OPENING_BOOTSTRAP"
            };
            let quota = Self::chapter_quota(connection, project_id, &id)?;
            Some(
                json!({"id":id,"number":number,"title":title,"volumeId":volume_id,"executionMode":execution_mode,"quota":quota}),
            )
        } else {
            None
        };
        let job_status = job.as_ref().map(|(_, _, _, status, _)| status.clone());
        let job_error = job
            .as_ref()
            .and_then(|(_, _, _, _, error_code)| error_code.clone());
        let is_active = matches!(job_status.as_deref(), Some("RUNNING" | "WAITING_MODEL"));
        let can_retry =
            matches!(job_status.as_deref(), Some("FAILED" | "PAUSED")) && final_ref.is_none();
        let gate_status = if job_status.as_deref() == Some("FAILED") {
            "BLOCKED"
        } else if final_ref.is_some() {
            "PASS"
        } else if stale_status == "PLAN_STALE" {
            "BLOCKED"
        } else {
            "PENDING"
        };
        let cost_chapter_id = job
            .as_ref()
            .and_then(|(_, _, chapter_id, _, _)| chapter_id.as_deref())
            .or_else(|| {
                next_chapter_projection
                    .as_ref()
                    .and_then(|chapter| chapter.get("id").and_then(Value::as_str))
            });
        let cost_summary =
            production_cost_summary(connection, project_id, cost_chapter_id, runtime.2)?;
        let new_responsibilities = Self::latest_new_responsibilities(connection, project_id)?;
        Ok(json!({
            "timeline":timeline,"tasks":task,"artifacts":{"items":artifact_items,"nextCursor":null,"total":artifact_items.len()},
            "signing":signing,"bookPlan":book_plan,"gatePolicy":gate_policy,"projectPhase":runtime.5,"activeWork":{"label":if job_status.as_deref()==Some("FAILED") { "FAILED" } else { &current_stage },"detail":active_detail},
            "runtimeOperation":b02_operation.as_ref().map(|(job_id,kind,phase,status,error_code)| json!({"jobId":job_id,"kind":kind,"phase":phase,"status":status,"errorCode":error_code,"active":matches!(status.as_str(),"RUNNING"|"WAITING_MODEL")})),
            "productionStage":current_stage,"jobStatus":job_status,"jobError":job_error,"isActive":is_active,"canRetry":can_retry,"lastStage":current_stage,"nextChapter":next_chapter_projection,"chapterPlan":plan_projection,
            "triggeredContinuity":plan_projection.as_ref().and_then(|plan| plan.get("continuityActions")).cloned().unwrap_or_else(|| json!([])),
            "newResponsibilities":new_responsibilities,
            "gate":{"status":gate_status,"reason":if job_status.as_deref()==Some("FAILED") {job_error.clone().unwrap_or_else(|| "MODEL_TRANSPORT_FAILED".into())} else if stale_status=="PLAN_STALE" {"PLAN_STALE / RECOMPILE_REQUIRED".into()} else {"".into()}},
            "stale":{"status":stale_status,"reason":if stale_status=="PLAN_STALE" {"Relevant authority changed"} else {""}},"modelRuns":model_runs,"candidateRef":candidate_ref,"finalChapterRef":final_ref,"reconciliation":reconciliation,"repair":repair,
            "creativeSkills":creative_skills,"quality":quality,"tasteProfile":taste_profile,
            "readerDesire":quality.get("softAssessment").cloned().unwrap_or(Value::Null),
            "openingProgress":opening_progress,
            "costSummary":cost_summary,"committedChapters":runtime.1,"candidateChapters":runtime.3
        }))
    }

    fn latest_new_responsibilities(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Value, ProductionRepositoryError> {
        let raw: Option<String> = connection
            .query_row(
                "SELECT p.payload_json FROM canonical_heads h JOIN artifacts a ON a.project_id=h.project_id AND a.artifact_id=h.artifact_id AND a.version=h.version JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version LEFT JOIN chapters c ON c.project_id=h.project_id AND c.chapter_id=json_extract(p.payload_json,'$.chapterId') WHERE h.project_id=?1 AND h.artifact_type='ProductionReceipt' ORDER BY COALESCE(c.chapter_no,0) DESC,a.created_at DESC,a.artifact_id DESC LIMIT 1",
                [project_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(raw) = raw else {
            return Ok(json!([]));
        };
        let payload: Value = serde_json::from_str(&raw)
            .map_err(|error| ProductionRepositoryError::Serialization(error.to_string()))?;
        let items = payload
            .get("threadPromotionDecisions")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|item| item.get("newResponsibility").and_then(Value::as_bool) == Some(true))
            .map(|item| {
                let thread_ref = item
                    .get("threadRef")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let obligation_id = thread_ref.strip_prefix("thread:").unwrap_or(thread_ref);
                let current = crate::repositories::narrative::NarrativeRepository::responsibility_adjustment_record(
                    connection,
                    project_id,
                    obligation_id,
                )
                .map_err(|error| ProductionRepositoryError::Narrative(error.to_string()))?;
                Ok(json!({
                    "threadRef":thread_ref,
                    "authorityLevel":current.as_ref().map(|record| record.authority_level.as_str()).or_else(|| item.get("authorityLevel").and_then(Value::as_str)).unwrap_or("C"),
                    "lifecycle":current.as_ref().map(|record| record.lifecycle.as_str()).unwrap_or("PROPOSED"),
                    "description":current.as_ref().map(|record| record.description.as_str()).or_else(|| item.get("description").and_then(Value::as_str)).unwrap_or_default(),
                    "fulfillmentObject":current.as_ref().and_then(|record| record.fulfillment_object.clone()).map(Value::String).or_else(|| item.get("fulfillmentObject").cloned()).unwrap_or(Value::Null),
                    "resumeCondition":current.as_ref().map(|record| record.resume_condition.clone()).or_else(|| item.get("resumeCondition").cloned()).unwrap_or(Value::Null),
                    "evidence":current.as_ref().and_then(|record| record.evidence_refs.iter().find(|evidence| evidence.quote.is_some()).map(|evidence| json!({"start":evidence.range_start,"end":evidence.range_end,"quote":evidence.quote}))).or_else(|| item.get("evidence").cloned()).unwrap_or(Value::Null),
                    "ruleVersion":item.get("ruleVersion").cloned().unwrap_or(Value::Null)
                }))
            })
            .collect::<Result<Vec<_>, ProductionRepositoryError>>()?;
        Ok(json!(items))
    }
}
