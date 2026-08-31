//! B08 Production Center bounded read projection owner.

use libsql_rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum ProductionRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
}

pub struct ProductionRepository;

impl ProductionRepository {
    pub fn center_projection(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Value, ProductionRepositoryError> {
        let runtime: (String, i64, i64, i64) = connection
            .query_row(
                "SELECT COALESCE(run_state,'IDLE'), COALESCE(committed_chapters,0), COALESCE(total_cost_cents,0), COALESCE(candidate_chapters,0) FROM project_runtime_state WHERE project_id=?1",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap_or_else(|_| ("IDLE".into(), 0, 0, 0));
        let job: Option<(String, String, Option<String>, String)> = connection
            .query_row(
                "SELECT job_id, current_stage, chapter_id, status FROM jobs WHERE project_id=?1 ORDER BY updated_at DESC, job_id DESC LIMIT 1",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let current_stage = job
            .as_ref()
            .map(|(_, stage, _, _)| stage.clone())
            .unwrap_or_else(|| "IDLE".into());
        let active_detail = job
            .as_ref()
            .map(|(id, _, chapter, status)| {
                format!(
                    "{status} · {id} · {}",
                    chapter.as_deref().unwrap_or("未绑定章节")
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
        let stage_order = stages
            .iter()
            .position(|stage| *stage == current_stage)
            .unwrap_or(usize::MAX);
        let timeline = stages
            .iter()
            .enumerate()
            .map(|(order, stage)| {
                json!({
                    "id": format!("b08:{stage}"), "order": order + 1, "title": stage,
                    "description": "B08 Chapter Production Runtime", "status": if order < stage_order { "done" } else if order == stage_order { "active" } else { "pending" }
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
            .map(|(id, _, chapter, status)| {
                json!({"id":id,"title":"Chapter Production","description":format!("{} · {}", chapter.as_deref().unwrap_or(""), status),"progress":if status=="COMPLETED" {100} else {50}})
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
            .and_then(|(_, _, chapter, _)| chapter.clone())
            .or_else(|| next_chapter.as_ref().map(|row| row.0.clone()));
        let plan_row: Option<(String, i64, String, String)> = active_chapter_id
            .as_ref()
            .and_then(|chapter_id| {
                connection
                    .query_row(
                        "SELECT h.artifact_id,h.version,h.payload_hash,p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan' AND h.scope_level='CHAPTER' AND h.scope_key=?2",
                        libsql_rusqlite::params![project_id, json!({"level":"CHAPTER","chapterId":chapter_id}).to_string()],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )
                    .optional()
                    .ok()
                    .flatten()
            });
        let plan_projection = plan_row.as_ref().and_then(|(id, version, hash, raw)| {
            serde_json::from_str::<Value>(raw).ok().map(|payload| {
                json!({
                    "ref":{"artifactId":id,"version":version,"payloadHash":hash},
                    "chapterGoal":payload.get("chapterGoal"), "contextManifestRef":payload.get("contextManifestRef"),
                    "activeTaskRefs":payload.get("activeTaskRefs").cloned().unwrap_or_else(|| json!([])),
                    "continuityActions":payload.get("continuityActions").cloned().unwrap_or_else(|| json!([])),
                    "compiledFromProjectRevision":payload.get("compiledFromProjectRevision"),
                    "compiledFromStateDigest":payload.get("compiledFromStateDigest")
                })
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
        let model_runs = if let Some((job_id, _, _, _)) = &job {
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
            "SELECT rp.repair_plan_id,rp.based_on_project_revision,rp.diagnosis_json,rp.finding_refs_json,rp.target_authority_owner,rp.proposed_operations_json,rp.affected_artifacts_json,rp.affected_scopes_json,rp.risk_level,rp.requires_backup,rp.expected_postconditions_json,rp.operation_digest,rp.status,ra.authorization_id,ra.consumed_at,rr.receipt_ref,rr.status,rr.postconditions_json FROM repair_plans rp LEFT JOIN repair_authorizations ra ON ra.project_id=rp.project_id AND ra.repair_plan_id=rp.repair_plan_id LEFT JOIN repair_execution_receipts rr ON rr.project_id=rp.project_id AND rr.repair_plan_id=rp.repair_plan_id WHERE rp.project_id=?1 ORDER BY rp.created_at DESC,ra.issued_at DESC LIMIT 1",
            [project_id],
            |row| {
                let receipt_ref = row.get::<_, Option<String>>(15)?;
                let receipt_status = row.get::<_, Option<String>>(16)?;
                let receipt_postconditions = row.get::<_, Option<String>>(17)?;
                let execution_receipt = receipt_ref.map(|receipt_ref| {
                    json!({
                        "receiptRef":receipt_ref,
                        "status":receipt_status.unwrap_or_default(),
                        "postconditions":serde_json::from_str::<Value>(&receipt_postconditions.unwrap_or_else(|| "[]".into())).unwrap_or_else(|_| json!([]))
                    })
                });
                Ok(json!({"repairPlanId":row.get::<_,String>(0)?,"basedOnProjectRevision":row.get::<_,i64>(1)?,"diagnosis":serde_json::from_str::<Value>(&row.get::<_,String>(2)?).unwrap_or(Value::Null),"findingRefs":serde_json::from_str::<Value>(&row.get::<_,String>(3)?).unwrap_or_else(|_| json!([])),"targetAuthorityOwner":row.get::<_,String>(4)?,"proposedOperations":serde_json::from_str::<Value>(&row.get::<_,String>(5)?).unwrap_or_else(|_| json!([])),"affectedArtifacts":serde_json::from_str::<Value>(&row.get::<_,String>(6)?).unwrap_or_else(|_| json!([])),"affectedScopes":serde_json::from_str::<Value>(&row.get::<_,String>(7)?).unwrap_or_else(|_| json!([])),"riskLevel":row.get::<_,String>(8)?,"requiresBackup":row.get::<_,i64>(9)? != 0,"expectedPostconditions":serde_json::from_str::<Value>(&row.get::<_,String>(10)?).unwrap_or_else(|_| json!([])),"operationDigest":row.get::<_,String>(11)?,"status":row.get::<_,String>(12)?,"authorizationId":row.get::<_,Option<String>>(13)?,"consumedAt":row.get::<_,Option<String>>(14)?,"executionReceipt":execution_receipt}))
            },
        ).optional()?;
        let next_chapter_projection = next_chapter.map(|(id, number, title, volume_id)| json!({"id":id,"number":number,"title":title,"volumeId":volume_id,"executionMode":if plan_projection.is_some() {"LONG_FORM"} else {"OPENING_BOOTSTRAP"}}));
        Ok(json!({
            "timeline":timeline,"tasks":task,"artifacts":{"items":artifact_items,"nextCursor":null,"total":artifact_items.len()},
            "signing":{"status":"NOT_STARTED","verdict":"B08 production gate is authoritative"},"activeWork":{"label":current_stage,"detail":active_detail},
            "productionStage":current_stage,"nextChapter":next_chapter_projection,"chapterPlan":plan_projection,
            "triggeredContinuity":plan_projection.as_ref().and_then(|plan| plan.get("continuityActions")).cloned().unwrap_or_else(|| json!([])),
            "gate":{"status":if final_ref.is_some() {"PASS"} else if stale_status=="PLAN_STALE" {"BLOCKED"} else {"PENDING"},"reason":if stale_status=="PLAN_STALE" {"PLAN_STALE / RECOMPILE_REQUIRED"} else {""}},
            "stale":{"status":stale_status,"reason":if stale_status=="PLAN_STALE" {"Relevant authority changed"} else {""}},"modelRuns":model_runs,"candidateRef":candidate_ref,"finalChapterRef":final_ref,"reconciliation":reconciliation,"repair":repair,
            "costSummary":{"currentVolume":0,"projectTotal":runtime.2 as f64 / 100.0,"budgetRemaining":0},"committedChapters":runtime.1,"candidateChapters":runtime.3
        }))
    }
}
