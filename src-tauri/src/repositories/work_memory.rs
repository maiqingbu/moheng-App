//! WM-1 phase 1: the work's memory table (WORK_MEMORY_DESIGN §7), derived
//! from each committed chapter's production receipt.

use crate::work_memory::Memory;
use libsql_rusqlite::Connection;
use serde_json::Value;

/// The details of the chapters committed before `chapter_id`. It first
/// brings the memory up to date (see `sync`), so it writes to `connection`.
pub fn earlier_memories(
    connection: &Connection,
    project_id: &str,
    chapter_id: &str,
) -> Result<Vec<Memory>, String> {
    sync(connection, project_id).map_err(|error| format!("WM1_MEMORY_SYNC_FAILED:{error}"))?;
    let mut statement = connection
        .prepare(
            "SELECT chapter_no, subject, kind, content FROM work_memory WHERE project_id=?1 \
             AND chapter_no < (SELECT chapter_no FROM chapters WHERE project_id=?1 AND chapter_id=?2) \
             ORDER BY chapter_no, ordinal",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(libsql_rusqlite::params![project_id, chapter_id], |row| {
            Ok(Memory {
                chapter: row.get(0)?,
                subject: row.get(1)?,
                kind: row.get(2)?,
                detail: row.get(3)?,
            })
        })
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(|error| error.to_string())?;
    Ok(rows)
}

/// Takes the continuity details of each committed chapter whose production
/// receipt the memory has not seen (new, or changed since) into the memory,
/// and lets go of chapters no longer committed. The memory is derived from
/// the receipts and can always be rebuilt from them. Nothing is written
/// when nothing changed.
fn sync(connection: &Connection, project_id: &str) -> Result<(), String> {
    let sql = |error: libsql_rusqlite::Error| error.to_string();
    let mut statement = connection
        .prepare(
            "SELECT c.chapter_id, c.chapter_no, h.payload_hash \
             FROM chapters c \
             JOIN canonical_heads h ON h.project_id=c.project_id AND h.artifact_type='ProductionReceipt' \
               AND h.scope_level='CHAPTER' AND json_extract(h.scope_key,'$.chapterId')=c.chapter_id \
             LEFT JOIN work_memory_sources s ON s.project_id=c.project_id AND s.chapter_id=c.chapter_id \
             WHERE c.project_id=?1 AND c.status='COMMITTED' \
               AND (s.receipt_hash IS NULL OR s.receipt_hash<>h.payload_hash)",
        )
        .map_err(sql)?;
    let changed = statement
        .query_map([project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(sql)?;
    let gone: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM work_memory_sources s WHERE s.project_id=?1 AND NOT EXISTS \
             (SELECT 1 FROM chapters c WHERE c.project_id=s.project_id AND c.chapter_id=s.chapter_id AND c.status='COMMITTED'))",
            [project_id],
            |row| row.get(0),
        )
        .map_err(sql)?;
    if changed.is_empty() && !gone {
        return Ok(());
    }
    connection
        .execute_batch("SAVEPOINT work_memory_sync")
        .map_err(sql)?;
    let result = (|| {
        for table in ["work_memory", "work_memory_sources"] {
            connection
                .execute(
                    &format!(
                        "DELETE FROM {table} WHERE project_id=?1 AND NOT EXISTS (SELECT 1 FROM chapters c \
                         WHERE c.project_id={table}.project_id AND c.chapter_id={table}.chapter_id AND c.status='COMMITTED')"
                    ),
                    [project_id],
                )
                .map_err(sql)?;
        }
        let mut insert = connection
            .prepare_cached(
                "INSERT INTO work_memory(project_id, chapter_id, ordinal, chapter_no, subject, kind, content) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(sql)?;
        for (chapter_id, chapter_no, receipt_hash) in &changed {
            let receipt = crate::repositories::project_entry::canonical_chapter_head(
                connection,
                project_id,
                "ProductionReceipt",
                chapter_id,
            )
            .map_err(|error| error.to_string())?;
            let details = receipt
                .map(|row| row.payload["continuityDetails"].clone())
                .unwrap_or(Value::Null);
            connection
                .execute(
                    "DELETE FROM work_memory WHERE project_id=?1 AND chapter_id=?2",
                    libsql_rusqlite::params![project_id, chapter_id],
                )
                .map_err(sql)?;
            for (ordinal, detail) in details.as_array().into_iter().flatten().enumerate() {
                let field = |name: &str| detail[name].as_str().unwrap_or("").trim().to_string();
                let (subject, kind, content) = (field("subject"), field("kind"), field("detail"));
                if subject.is_empty() || content.is_empty() {
                    continue;
                }
                insert
                    .execute(libsql_rusqlite::params![
                        project_id,
                        chapter_id,
                        ordinal as i64,
                        chapter_no,
                        subject,
                        kind,
                        content
                    ])
                    .map_err(sql)?;
            }
            connection
                .execute(
                    "INSERT INTO work_memory_sources(project_id, chapter_id, receipt_hash) VALUES (?1, ?2, ?3) \
                     ON CONFLICT(project_id, chapter_id) DO UPDATE SET receipt_hash=excluded.receipt_hash",
                    libsql_rusqlite::params![project_id, chapter_id, receipt_hash],
                )
                .map_err(sql)?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => connection
            .execute_batch("RELEASE work_memory_sync")
            .map_err(sql),
        Err(error) => {
            let _ =
                connection.execute_batch("ROLLBACK TO work_memory_sync; RELEASE work_memory_sync");
            Err(error)
        }
    }
}
