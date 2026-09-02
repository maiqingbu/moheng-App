//! B11 formal UI read projections.
//!
//! These queries are deliberately bounded and read-only.  They are the
//! repository owner for the projections exposed by the Tauri FrontendPort;
//! commands only validate the request and open the project read connection.

use super::ops::{chapter_page, ProjectionPage};
use libsql_rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum UiProjectionError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("operations: {0}")]
    Operations(#[from] super::ops::OpsError),
    #[error("projection not found")]
    NotFound,
}

fn bound(limit: u64, default: u64) -> usize {
    (if limit == 0 { default } else { limit }).clamp(1, 100) as usize
}

fn offset(cursor: Option<&str>) -> i64 {
    cursor
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0)
}

fn page(mut items: Vec<Value>, total: i64, start: i64, limit: usize) -> ProjectionPage {
    let next_cursor = (items.len() > limit).then(|| (start + limit as i64).to_string());
    items.truncate(limit);
    ProjectionPage {
        items,
        next_cursor,
        total,
    }
}

pub fn volume_detail(
    connection: &Connection,
    project_id: &str,
    volume_id: &str,
    cursor: Option<&str>,
    limit: u64,
) -> Result<Value, UiProjectionError> {
    let row: Option<(i64, String, String, i64, i64, i64, String)> = connection
        .query_row(
            "SELECT volume_no,title,status,chapter_start,chapter_end,target_words,updated_at FROM volumes WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
        )
        .optional()?;
    let Some((number, title, status, chapter_start, chapter_end, target_words, updated_at)) = row
    else {
        return Err(UiProjectionError::NotFound);
    };
    let committed: i64 = connection.query_row(
        "SELECT COUNT(*) FROM chapters WHERE project_id=?1 AND volume_id=?2 AND status='COMMITTED'",
        libsql_rusqlite::params![project_id, volume_id],
        |row| row.get(0),
    )?;
    let words: i64 = connection.query_row(
        "SELECT COALESCE(SUM(word_count),0) FROM chapters WHERE project_id=?1 AND volume_id=?2",
        libsql_rusqlite::params![project_id, volume_id],
        |row| row.get(0),
    )?;
    let planned = (chapter_end - chapter_start + 1).max(0);
    let chapter_limit = bound(limit, 30);
    let chapters = chapter_page(connection, project_id, volume_id, cursor, chapter_limit)?;
    let volume_plan_version: Option<String> = connection
        .query_row(
            "SELECT 'v'||plan_version FROM volume_plan_index WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id],
            |row| row.get(0),
        )
        .optional()?;
    let ledger_version: Option<String> = connection
        .query_row(
            "SELECT updated_at FROM planning_state_ledger_index WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id],
            |row| row.get(0),
        )
        .optional()?;
    let open_obligations: i64 = connection.query_row(
        "SELECT COUNT(*) FROM narrative_obligations o WHERE o.project_id=?1 AND o.lifecycle IN ('PROPOSED','APPROVED','PLANTED','PROGRESSING') AND (EXISTS (SELECT 1 FROM chapters c WHERE c.project_id=o.project_id AND c.volume_id=?2 AND c.chapter_id=o.last_touched_chapter_id) OR EXISTS (SELECT 1 FROM artifacts a WHERE a.project_id=o.project_id AND a.artifact_id=o.source_artifact_id AND a.scope_level='VOLUME' AND a.scope_key=?2))",
        libsql_rusqlite::params![project_id, volume_id],
        |row| row.get(0),
    )?;
    let overdue_foreshadows: i64 = connection.query_row(
        "SELECT COUNT(*) FROM foreshadows f JOIN chapters c ON c.project_id=f.project_id AND c.chapter_id=f.plant_chapter_id WHERE f.project_id=?1 AND c.volume_id=?2 AND f.status NOT IN ('RECOVERED','ABANDONED') AND f.recover_window_end_chapter IS NOT NULL AND CAST(f.recover_window_end_chapter AS INTEGER) < c.chapter_no",
        libsql_rusqlite::params![project_id, volume_id],
        |row| row.get(0),
    )?;
    let ready = status == "CLOSED" && open_obligations == 0 && overdue_foreshadows == 0;
    Ok(json!({
        "volume": {
            "id":volume_id,"number":number,"title":title,"status":status,
            "chapterStart":chapter_start,"chapterEnd":chapter_end,"committedChapters":committed,
            "plannedChapters":planned,"wordCount":words,"targetWords":target_words,
            "progress":if planned == 0 { 0.0 } else { committed as f64 / planned as f64 },
            "goal":"","health":if open_obligations > 0 || overdue_foreshadows > 0 { "attention" } else { "healthy" },"updatedAt":updated_at
        },
        "planVersion":volume_plan_version.unwrap_or_else(|| "未生成".into()),
        "contractVersion":"VolumeContract",
        "stateLedgerVersion":ledger_version.unwrap_or_else(|| "未生成".into()),
        "arcs":[],"chapters":chapters,"openObligations":open_obligations,"overdueForeshadows":overdue_foreshadows,
        "exitEligibility":{"ready":ready,"blockers":if ready { Vec::<String>::new() } else { vec!["本卷退出条件尚未全部满足".to_string()] }},
        "nextVolumeEligibility":if ready { "已解锁下一卷规划" } else { "完成本卷退出条件后解锁" }
    }))
}

pub fn decisions(
    connection: &Connection,
    project_id: &str,
    status: Option<&str>,
    cursor: Option<&str>,
    limit: u64,
) -> Result<Value, UiProjectionError> {
    let start = offset(cursor);
    let limit = bound(limit, 50);
    let pending_only = status == Some("pending");
    let resolved_only = status == Some("resolved");
    let total: i64 = if pending_only {
        connection.query_row(
            "SELECT COUNT(*) FROM decisions WHERE project_id=?1 AND outcome IN ('OPEN','PENDING')",
            [project_id],
            |row| row.get(0),
        )?
    } else if resolved_only {
        connection.query_row("SELECT COUNT(*) FROM decisions WHERE project_id=?1 AND outcome NOT IN ('OPEN','PENDING')", [project_id], |row| row.get(0))?
    } else {
        connection.query_row(
            "SELECT COUNT(*) FROM decisions WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?
    };
    let mut sql = String::from("SELECT decision_id,decision_type,outcome,COALESCE(reason,''),COALESCE(payload_json,'{}'),created_at FROM decisions WHERE project_id=?1");
    if pending_only {
        sql.push_str(" AND outcome IN ('OPEN','PENDING')");
    }
    if resolved_only {
        sql.push_str(" AND outcome NOT IN ('OPEN','PENDING')");
    }
    sql.push_str(" ORDER BY created_at DESC, decision_id DESC LIMIT ?3 OFFSET ?4");
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        libsql_rusqlite::params![project_id, "", (limit + 1) as i64, start],
        |row| {
            let payload: String = row.get(4)?;
            let payload: Value = serde_json::from_str(&payload).unwrap_or_else(|_| json!({}));
            let outcome: String = row.get(2)?;
            let category = payload.get("category").and_then(Value::as_str).unwrap_or("项目事项");
            let priority = payload.get("priority").and_then(Value::as_str).unwrap_or("normal");
            Ok(json!({
                "id":row.get::<_,String>(0)?,"title":payload.get("title").and_then(Value::as_str).unwrap_or(category),
                "summary":row.get::<_,String>(3)?,"category":category,"priority":priority,
                "status":if outcome == "OPEN" || outcome == "PENDING" { "pending" } else { "resolved" },
                "createdAt":row.get::<_,String>(5)?,"recommendation":payload.get("recommendation"),"evidence":payload.get("evidence")
            }))
        },
    )?;
    let items = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(serde_json::to_value(page(items, total, start, limit))?)
}

pub fn chapter_versions(
    connection: &Connection,
    project_id: &str,
    chapter_id: &str,
    cursor: Option<&str>,
    limit: u64,
) -> Result<Value, UiProjectionError> {
    let start = offset(cursor);
    let limit = bound(limit, 30);
    let scope = serde_json::to_string(&json!({"level":"CHAPTER","chapterId":chapter_id}))?;
    let total: i64 = connection.query_row(
        "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND scope_level='CHAPTER' AND scope_key=?2",
        libsql_rusqlite::params![project_id, scope], |row| row.get(0),
    )?;
    let current: Option<(String, i64)> = connection.query_row(
        "SELECT artifact_id,version FROM canonical_heads WHERE project_id=?1 AND scope_level='CHAPTER' AND scope_key=?2 AND artifact_type='FinalChapter'",
        libsql_rusqlite::params![project_id, scope], |row| Ok((row.get(0)?,row.get(1)?)),
    ).optional()?;
    let mut statement = connection.prepare("SELECT a.artifact_id,a.artifact_type,a.version,a.created_at,COALESCE(p.payload_json,'{}') FROM artifacts a LEFT JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.scope_level='CHAPTER' AND a.scope_key=?2 ORDER BY a.version DESC,a.created_at DESC LIMIT ?3 OFFSET ?4")?;
    let rows = statement.query_map(libsql_rusqlite::params![project_id, scope, (limit + 1) as i64, start], |row| {
        let payload: String = row.get(4)?;
        let payload: Value = serde_json::from_str(&payload).unwrap_or_else(|_| json!({}));
        let id: String = row.get(0)?;
        let version: i64 = row.get(2)?;
        let artifact_type: String = row.get(1)?;
        let excerpt = payload.get("body").and_then(Value::as_str).unwrap_or("").chars().take(120).collect::<String>();
        Ok(json!({"id":format!("{id}:v{version}"),"chapterId":chapter_id,"label":format!("v{version} · {}",if artifact_type == "FinalChapter" { "正式版本" } else { "制作版本" }),"createdAt":row.get::<_,String>(3)?,"reason":artifact_type,"excerpt":excerpt,"current":current.as_ref().is_some_and(|(current_id,current_version)| current_id == &id && *current_version == version)}))
    })?;
    let items = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(serde_json::to_value(page(items, total, start, limit))?)
}

pub fn canon(
    connection: &Connection,
    project_id: &str,
    limit: u64,
) -> Result<Value, UiProjectionError> {
    let limit = bound(limit, 60) as i64;
    let characters = {
        let mut statement = connection.prepare("SELECT entity_id,canonical_name,entity_type,COALESCE(first_chapter_id,'') FROM canon_entities WHERE project_id=?1 ORDER BY entity_id LIMIT ?2")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id,limit], |row| Ok(json!({"id":row.get::<_,String>(0)?,"name":row.get::<_,String>(1)?,"role":row.get::<_,String>(2)?,"status":"正式","location":"未记录","note":"当前正式作品记录"})))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let relations = {
        let mut statement = connection.prepare("SELECT subject_id,object_id,predicate FROM canon_relations WHERE project_id=?1 AND superseded_by IS NULL ORDER BY relation_id LIMIT ?2")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id,limit], |row| Ok(json!({"from":row.get::<_,String>(0)?,"to":row.get::<_,String>(1)?,"label":row.get::<_,String>(2)?,"tone":"neutral"})))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let timeline = {
        let mut statement = connection.prepare("SELECT e.event_id,e.chapter_id,e.event_type,COALESCE(c.title,''),COALESCE(e.story_time,'') FROM canon_events e JOIN chapters c ON c.project_id=e.project_id AND c.chapter_id=e.chapter_id WHERE e.project_id=?1 ORDER BY c.chapter_no,e.event_ordinal,e.event_id LIMIT ?2")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id,limit], |row| Ok(json!({"chapter":row.get::<_,String>(1)?,"title":row.get::<_,String>(3)?,"detail":row.get::<_,String>(2)?,"status":"official"})))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let foreshadows = {
        let mut statement = connection.prepare("SELECT foreshadow_id,intent,status,COALESCE(plant_chapter_id,''),COALESCE(recover_window_end_chapter,'') FROM foreshadows WHERE project_id=?1 ORDER BY foreshadow_id LIMIT ?2")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id,limit], |row| Ok(json!({"id":row.get::<_,String>(0)?,"title":row.get::<_,String>(1)?,"planted":row.get::<_,String>(3)?,"planned":row.get::<_,String>(4)?,"status":if row.get::<_,String>(2)? == "RECOVERED" { "resolved" } else { "open" },"detail":"当前伏笔记录"})))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let knowledge = {
        let mut statement = connection.prepare("SELECT k.knower_id,COALESCE(k.fact_id,''),k.epistemic_state,COALESCE(k.visibility_scope,'') FROM canon_knowledge k WHERE k.project_id=?1 ORDER BY k.knowledge_id LIMIT ?2")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id,limit], |row| {
            let state: String = row.get(2)?;
            Ok(json!({"character":row.get::<_,String>(0)?,"topic":"相关事实","state":match state.as_str() { "KNOWS" => "knows", "BELIEVES_TRUE"|"BELIEVES_FALSE"|"SUSPECTS" => "believes", "CONCEALS" => "conceals", _ => "unknown" },"detail":row.get::<_,String>(3)?}))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let wrap = |items: Vec<Value>| {
        let total = items.len();
        json!({"items":items,"nextCursor":Value::Null,"total":total})
    };
    Ok(
        json!({"characters":wrap(characters),"relations":wrap(relations),"timeline":wrap(timeline),"foreshadows":wrap(foreshadows),"knowledge":wrap(knowledge)}),
    )
}

pub fn book_health(
    connection: &Connection,
    project_id: &str,
    cursor: Option<&str>,
    limit: u64,
) -> Result<Value, UiProjectionError> {
    let checked_at: String = connection.query_row(
        "SELECT updated_at FROM project_meta WHERE project_id=?1",
        [project_id],
        |row| row.get(0),
    )?;
    let current_words: i64 = connection.query_row("SELECT COALESCE(SUM(word_count),0) FROM chapters WHERE project_id=?1 AND status='COMMITTED'", [project_id], |row| row.get(0))?;
    let open_obligations: i64 = connection.query_row("SELECT COUNT(*) FROM narrative_obligations WHERE project_id=?1 AND lifecycle IN ('PROPOSED','APPROVED','PLANTED','PROGRESSING')", [project_id], |row| row.get(0))?;
    let open_foreshadows: i64 = connection.query_row("SELECT COUNT(*) FROM foreshadows WHERE project_id=?1 AND status NOT IN ('RECOVERED','ABANDONED')", [project_id], |row| row.get(0))?;
    let pending: i64 = connection.query_row(
        "SELECT COUNT(*) FROM decisions WHERE project_id=?1 AND outcome IN ('OPEN','PENDING')",
        [project_id],
        |row| row.get(0),
    )?;
    let score =
        (100 - (pending * 4 + open_obligations.min(10) * 2 + open_foreshadows.min(10) * 2)).max(0);
    let issues = if pending > 0 {
        vec![
            json!({"id":"pending-decisions","category":"主线","severity":"attention","title":"有待处理的项目决定","detail":"当前项目仍有待处理记录。","location":"当前作品","suggestedAction":"先处理待决定事项。"}),
        ]
    } else {
        Vec::new()
    };
    let issue_page = page(issues, pending.min(1), offset(cursor), bound(limit, 50));
    Ok(
        json!({"score":score,"level":if score < 70 { "risk" } else if score < 90 { "attention" } else { "healthy" },"checkedAt":checked_at,"progressToEnding":0,"targetLength":"未设置","currentWords":current_words,"issues":issue_page,"counters":{"openObligations":open_obligations,"overdueObligations":0,"openForeshadows":open_foreshadows,"overdueForeshadows":0,"absentCharacters":0,"repeatedPatterns":0}}),
    )
}

pub fn history(
    connection: &Connection,
    project_id: &str,
    kind: Option<&str>,
    cursor: Option<&str>,
    limit: u64,
) -> Result<Value, UiProjectionError> {
    let start = offset(cursor);
    let limit = bound(limit, 40);
    let mut entries = Vec::new();
    if kind.is_none() || kind == Some("VERSION") {
        let mut statement = connection.prepare("SELECT receipt_id,command_id,committed_at FROM commit_receipts WHERE project_id=?1 ORDER BY committed_at DESC,receipt_id DESC LIMIT ?2 OFFSET ?3")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id,(limit + 1) as i64,start], |row| Ok(json!({"id":row.get::<_,String>(0)?,"kind":"VERSION","title":"正式提交记录","status":"applied","createdAt":row.get::<_,String>(2)?,"summary":"一次正式项目提交已记录。","impact":"以提交回执为准。","scope":"当前作品","receipt":row.get::<_,String>(0)?})))?;
        entries.extend(rows.collect::<Result<Vec<_>, _>>()?);
    }
    if kind.is_none() || kind == Some("CHANGE_REQUEST") || kind == Some("RETCON") {
        let mut statement = connection.prepare("SELECT change_request_id,change_type,status,summary,created_at,updated_at,impact_seed_json FROM change_requests WHERE project_id=?1 ORDER BY created_at DESC,change_request_id DESC LIMIT ?2 OFFSET ?3")?;
        let rows = statement.query_map(libsql_rusqlite::params![project_id,(limit + 1) as i64,start], |row| {
            let change_type: String = row.get(1)?;
            let entry_kind = if change_type.to_uppercase().contains("RETCON") { "RETCON" } else { "CHANGE_REQUEST" };
            let seed: String = row.get(6)?;
            let seed: serde_json::Value = serde_json::from_str(&seed).unwrap_or_else(|_| json!({}));
            let scope = seed.pointer("/preflight/scopeClass").and_then(|value| value.as_str()).unwrap_or("LOCAL");
            let impact = match scope { "GLOBAL" => "预计影响全局内容。", "WIDE" => "预计影响较多关联内容。", _ => "预计影响当前相关内容。" };
            let status = match row.get::<_,String>(2)?.as_str() { "APPLIED" => "applied", "APPROVED" => "approved", "IMPACT_READY" | "PENDING_APPROVAL" => "impact_ready", "REJECTED" => "rejected", _ => "draft" };
            Ok(json!({"id":row.get::<_,String>(0)?,"kind":entry_kind,"title":if entry_kind == "RETCON" {"事实修订计划"} else {"作品修改请求"},"status":status,"createdAt":row.get::<_,String>(4)?,"summary":row.get::<_,String>(3)?,"impact":impact,"scope":match scope { "GLOBAL" => "全局", "WIDE" => "较广", _ => "局部" }}))
        })?;
        entries.extend(rows.collect::<Result<Vec<_>, _>>()?);
    }
    entries.sort_by(|a, b| b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    let versions = connection.query_row(
        "SELECT COUNT(*) FROM commit_receipts WHERE project_id=?1",
        [project_id],
        |row| row.get::<_, i64>(0),
    )?;
    let changes = connection.query_row("SELECT COUNT(*) FROM change_requests WHERE project_id=?1 AND change_type NOT LIKE '%RETCON%'", [project_id], |row| row.get::<_,i64>(0))?;
    let retcons = connection.query_row(
        "SELECT COUNT(*) FROM change_requests WHERE project_id=?1 AND change_type LIKE '%RETCON%'",
        [project_id],
        |row| row.get::<_, i64>(0),
    )?;
    let pending = connection.query_row("SELECT COUNT(*) FROM change_requests WHERE project_id=?1 AND status IN ('DRAFT','IMPACT_READY','PENDING_APPROVAL')", [project_id], |row| row.get::<_,i64>(0))?;
    Ok(
        json!({"entries":page(entries,versions + changes + retcons,start,limit),"stats":{"versions":versions,"changes":changes,"retcons":retcons,"pending":pending}}),
    )
}
