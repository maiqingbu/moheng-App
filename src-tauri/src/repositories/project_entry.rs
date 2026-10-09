//! B11 Project Entry read/write support.
//!
//! This repository owns only the persistence queries needed by the project
//! entry projection. Authority remains in the existing artifacts, canonical
//! heads, decisions and CommitReceipt tables; market material is never copied
//! into Canon tables.

use crate::generated::types::ArtifactRef;
use crate::stable_id::{StableIdKind, StableIdentityFactory};
use libsql_rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

pub const OPENING_CHAPTER_COUNT: i64 = 3;

#[derive(Debug, thiserror::Error)]
pub enum ProjectEntryRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("stable id: {0}")]
    StableId(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryArtifactRow {
    pub artifact_id: String,
    pub artifact_type: String,
    pub version: i64,
    pub payload_hash: String,
    pub artifact_status: String,
    pub payload: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpeningTopology {
    pub book_id: String,
    pub volume_id: String,
    pub chapter_id: String,
    pub budget_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpeningProgress {
    pub completed: i64,
    pub total: i64,
    pub current_chapter: Option<i64>,
    pub previous_committed: bool,
    pub next_prepared: bool,
}

fn page_request(cursor: Option<&str>, limit: i64) -> (i64, i64) {
    let offset = cursor
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);
    (offset, limit.clamp(1, 100))
}

fn row_from_sql(
    row: &libsql_rusqlite::Row<'_>,
) -> Result<EntryArtifactRow, libsql_rusqlite::Error> {
    let payload_json: String = row.get(5)?;
    Ok(EntryArtifactRow {
        artifact_id: row.get(0)?,
        artifact_type: row.get(1)?,
        version: row.get(2)?,
        payload_hash: row.get(3)?,
        artifact_status: row.get(4)?,
        payload: serde_json::from_str(&payload_json).unwrap_or(Value::Null),
        created_at: row.get(6)?,
    })
}

pub fn list_artifacts(
    connection: &Connection,
    project_id: &str,
    artifact_type: &str,
    cursor: Option<&str>,
    limit: i64,
) -> Result<(Vec<EntryArtifactRow>, i64, Option<String>), ProjectEntryRepositoryError> {
    let (offset, bound) = page_request(cursor, limit);
    let total: i64 = connection.query_row(
        "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type=?2 AND artifact_status='COMMITTED'",
        libsql_rusqlite::params![project_id, artifact_type],
        |row| row.get(0),
    )?;
    let mut statement = connection.prepare(
        "SELECT a.artifact_id,a.artifact_type,a.version,a.payload_hash,a.artifact_status,p.payload_json,a.created_at FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type=?2 AND a.artifact_status='COMMITTED' ORDER BY a.created_at DESC,a.artifact_id DESC LIMIT ?3 OFFSET ?4",
    )?;
    let rows = statement
        .query_map(
            libsql_rusqlite::params![project_id, artifact_type, bound + 1, offset],
            row_from_sql,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let has_more = rows.len() as i64 > bound;
    let mut rows = rows;
    rows.truncate(bound as usize);
    let next_cursor = has_more.then(|| (offset + bound).to_string());
    Ok((rows, total, next_cursor))
}

pub fn latest_generation(
    connection: &Connection,
    project_id: &str,
    artifact_type: &str,
) -> Result<Option<i64>, ProjectEntryRepositoryError> {
    let (rows, _, _) = list_artifacts(connection, project_id, artifact_type, None, 100)?;
    Ok(rows
        .iter()
        .filter_map(|row| row.payload.get("generation").and_then(Value::as_i64))
        .max())
}

pub fn artifact_by_id(
    connection: &Connection,
    project_id: &str,
    artifact_id: &str,
) -> Result<Option<EntryArtifactRow>, ProjectEntryRepositoryError> {
    connection
        .query_row(
            "SELECT a.artifact_id,a.artifact_type,a.version,a.payload_hash,a.artifact_status,p.payload_json,a.created_at FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_id=?2 ORDER BY a.version DESC LIMIT 1",
            libsql_rusqlite::params![project_id, artifact_id],
            row_from_sql,
        )
        .optional()
        .map_err(Into::into)
}

pub fn artifact_ref_by_id(
    connection: &Connection,
    project_id: &str,
    artifact_id: &str,
) -> Result<Option<ArtifactRef>, ProjectEntryRepositoryError> {
    Ok(
        artifact_by_id(connection, project_id, artifact_id)?.map(|row| ArtifactRef {
            artifact_id: row.artifact_id,
            version: row.version,
            payload_hash: row.payload_hash,
        }),
    )
}

pub fn artifact_ref(
    connection: &Connection,
    project_id: &str,
    artifact_id: &str,
    version: i64,
) -> Result<Option<ArtifactRef>, ProjectEntryRepositoryError> {
    connection
        .query_row(
            "SELECT artifact_id,version,payload_hash FROM artifacts WHERE project_id=?1 AND artifact_id=?2 AND version=?3",
            libsql_rusqlite::params![project_id, artifact_id, version],
            |row| {
                Ok(ArtifactRef {
                    artifact_id: row.get(0)?,
                    version: row.get(1)?,
                    payload_hash: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub fn canonical_head(
    connection: &Connection,
    project_id: &str,
    artifact_type: &str,
) -> Result<Option<EntryArtifactRow>, ProjectEntryRepositoryError> {
    connection
        .query_row(
            "SELECT a.artifact_id,a.artifact_type,a.version,a.payload_hash,a.artifact_status,p.payload_json,a.created_at FROM canonical_heads h JOIN artifacts a ON a.project_id=h.project_id AND a.artifact_id=h.artifact_id AND a.version=h.version JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE h.project_id=?1 AND h.artifact_type=?2 AND h.scope_level='PROJECT' AND h.scope_key=?3",
            libsql_rusqlite::params![project_id, artifact_type, "{\"level\":\"PROJECT\"}"],
            row_from_sql,
        )
        .optional()
        .map_err(Into::into)
}

pub fn canonical_chapter_head(
    connection: &Connection,
    project_id: &str,
    artifact_type: &str,
    chapter_id: &str,
) -> Result<Option<EntryArtifactRow>, ProjectEntryRepositoryError> {
    let scope = serde_json::json!({"level":"CHAPTER","chapterId":chapter_id}).to_string();
    connection
        .query_row(
            "SELECT a.artifact_id,a.artifact_type,a.version,a.payload_hash,a.artifact_status,p.payload_json,a.created_at FROM canonical_heads h JOIN artifacts a ON a.project_id=h.project_id AND a.artifact_id=h.artifact_id AND a.version=h.version JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE h.project_id=?1 AND h.artifact_type=?2 AND h.scope_level='CHAPTER' AND h.scope_key=?3",
            libsql_rusqlite::params![project_id, artifact_type, scope],
            row_from_sql,
        )
        .optional()
        .map_err(Into::into)
}

pub fn canonical_head_ref(
    connection: &Connection,
    project_id: &str,
    artifact_type: &str,
) -> Result<Option<ArtifactRef>, ProjectEntryRepositoryError> {
    Ok(
        canonical_head(connection, project_id, artifact_type)?.map(|row| ArtifactRef {
            artifact_id: row.artifact_id,
            version: row.version,
            payload_hash: row.payload_hash,
        }),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn ensure_opening_topology(
    connection: &Connection,
    project_id: &str,
    book_id: &str,
    volume_id: &str,
    chapter_id: &str,
    budget_id: &str,
    title: &str,
    now: &str,
) -> Result<OpeningTopology, ProjectEntryRepositoryError> {
    connection.execute(
        "INSERT OR IGNORE INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,?3,'ACTIVE',?4,?4)",
        libsql_rusqlite::params![book_id, project_id, title, now],
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'第一卷 · 开篇', 'READY',1,36,120000,?4,?4)",
        libsql_rusqlite::params![volume_id, project_id, book_id, now],
    )?;
    for chapter_no in 1..=OPENING_CHAPTER_COUNT {
        let chapter_id = if chapter_no == 1 {
            chapter_id.to_owned()
        } else {
            StableIdentityFactory::from_command(
                StableIdKind::Chapter,
                &format!("{project_id}:chapter:{chapter_no}"),
            )
            .map_err(ProjectEntryRepositoryError::StableId)?
        };
        let budget_id = if chapter_no == 1 {
            budget_id.to_owned()
        } else {
            format!("budget:{project_id}:opening:{chapter_no}")
        };
        let chapter_title = format!("第{chapter_no}章 · 开篇");
        connection.execute(
            "INSERT OR IGNORE INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'IMPORTANT','PLANNED',0,?6,?6)",
            libsql_rusqlite::params![chapter_id, project_id, volume_id, chapter_no, chapter_title, now],
        )?;
        connection.execute(
            "UPDATE chapters SET title=?1, importance_class='IMPORTANT' WHERE project_id=?2 AND chapter_id=?3 AND chapter_no=?4",
            libsql_rusqlite::params![chapter_title, project_id, chapter_id, chapter_no],
        )?;
        connection.execute(
            "INSERT OR IGNORE INTO chapter_production_budgets(budget_id,project_id,chapter_id,importance_class,hard_max_model_runs,cost_budget_cents,frozen_at) VALUES (?1,?2,?3,'IMPORTANT',12,0,?4)",
            libsql_rusqlite::params![budget_id, project_id, chapter_id, now],
        )?;
    }
    Ok(OpeningTopology {
        book_id: book_id.into(),
        volume_id: volume_id.into(),
        chapter_id: chapter_id.into(),
        budget_id: budget_id.into(),
    })
}

pub fn opening_progress(
    connection: &Connection,
    project_id: &str,
) -> Result<OpeningProgress, ProjectEntryRepositoryError> {
    let completed: i64 = connection.query_row(
        "SELECT COUNT(*) FROM chapters WHERE project_id=?1 AND chapter_no BETWEEN 1 AND ?2 AND volume_id IN (SELECT volume_id FROM volumes WHERE project_id=?1 AND volume_no=1) AND status='COMMITTED'",
        libsql_rusqlite::params![project_id, OPENING_CHAPTER_COUNT],
        |row| row.get(0),
    )?;
    let current_chapter: Option<i64> = connection
        .query_row(
            "SELECT chapter_no FROM chapters WHERE project_id=?1 AND chapter_no BETWEEN 1 AND ?2 AND volume_id IN (SELECT volume_id FROM volumes WHERE project_id=?1 AND volume_no=1) AND status!='COMMITTED' ORDER BY chapter_no LIMIT 1",
            libsql_rusqlite::params![project_id, OPENING_CHAPTER_COUNT],
            |row| row.get(0),
        )
        .optional()?;
    let previous_committed = if let Some(chapter_no) = current_chapter {
        if chapter_no > 1 {
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM chapters WHERE project_id=?1 AND chapter_no=?2 AND volume_id IN (SELECT volume_id FROM volumes WHERE project_id=?1 AND volume_no=1) AND status='COMMITTED')",
                libsql_rusqlite::params![project_id, chapter_no - 1],
                |row| row.get(0),
            )?
        } else {
            false
        }
    } else {
        completed >= OPENING_CHAPTER_COUNT
    };
    let next_prepared = match current_chapter {
        Some(chapter_no) if chapter_no < OPENING_CHAPTER_COUNT => connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM chapters WHERE project_id=?1 AND chapter_no=?2 AND volume_id IN (SELECT volume_id FROM volumes WHERE project_id=?1 AND volume_no=1) AND status!='REJECTED')",
            libsql_rusqlite::params![project_id, chapter_no + 1],
            |row| row.get(0),
        )?,
        _ => false,
    };
    Ok(OpeningProgress {
        completed,
        total: OPENING_CHAPTER_COUNT,
        current_chapter,
        previous_committed,
        next_prepared,
    })
}

pub fn next_opening_chapter_id(
    connection: &Connection,
    project_id: &str,
) -> Result<Option<String>, ProjectEntryRepositoryError> {
    connection
        .query_row(
            "SELECT chapter_id FROM chapters WHERE project_id=?1 AND chapter_no BETWEEN 1 AND ?2 AND volume_id IN (SELECT volume_id FROM volumes WHERE project_id=?1 AND volume_no=1) AND status!='COMMITTED' ORDER BY chapter_no LIMIT 1",
            libsql_rusqlite::params![project_id, OPENING_CHAPTER_COUNT],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

pub fn is_opening_chapter(
    connection: &Connection,
    project_id: &str,
    chapter_id: &str,
) -> Result<bool, ProjectEntryRepositoryError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM chapters WHERE project_id=?1 AND chapter_id=?2 AND chapter_no BETWEEN 1 AND ?3 AND volume_id IN (SELECT volume_id FROM volumes WHERE project_id=?1 AND volume_no=1))",
            libsql_rusqlite::params![project_id, chapter_id, OPENING_CHAPTER_COUNT],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

pub fn opening_topology(
    connection: &Connection,
    project_id: &str,
) -> Result<Option<OpeningTopology>, ProjectEntryRepositoryError> {
    connection
        .query_row(
            "SELECT b.book_id,v.volume_id,c.chapter_id,cpb.budget_id FROM books b JOIN volumes v ON v.project_id=b.project_id AND v.book_id=b.book_id AND v.volume_no=1 JOIN chapters c ON c.project_id=v.project_id AND c.volume_id=v.volume_id AND c.chapter_no=1 JOIN chapter_production_budgets cpb ON cpb.project_id=c.project_id AND cpb.chapter_id=c.chapter_id WHERE b.project_id=?1 AND b.book_no=1",
            [project_id],
            |row| Ok(OpeningTopology { book_id: row.get(0)?, volume_id: row.get(1)?, chapter_id: row.get(2)?, budget_id: row.get(3)? }),
        )
        .optional()
        .map_err(Into::into)
}

pub fn latest_artifact_by_type(
    connection: &Connection,
    project_id: &str,
    artifact_type: &str,
) -> Result<Option<EntryArtifactRow>, ProjectEntryRepositoryError> {
    let (mut rows, _, _) = list_artifacts(connection, project_id, artifact_type, None, 1)?;
    Ok(rows.pop())
}

pub fn count_artifacts(
    connection: &Connection,
    project_id: &str,
    artifact_type: &str,
) -> Result<i64, ProjectEntryRepositoryError> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type=?2",
            libsql_rusqlite::params![project_id, artifact_type],
            |row| row.get(0),
        )
        .map_err(Into::into)
}
