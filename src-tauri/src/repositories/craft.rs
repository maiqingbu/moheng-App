//! CRAFT v3 local-only storage: the user's style sample (migration 0017).
//! The sample is deliberately outside artifacts and CommitBundles.

use libsql_rusqlite::{Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq)]
pub struct StyleSample {
    pub digest: String,
    pub body: String,
    pub saved_at: String,
}

pub fn save_style_sample(
    connection: &Connection,
    project_id: &str,
    digest: &str,
    body: &str,
    now: &str,
) -> Result<(), libsql_rusqlite::Error> {
    connection.execute(
        "INSERT INTO style_samples(project_id, sample_digest, body, saved_at) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(project_id) DO UPDATE SET sample_digest=excluded.sample_digest, body=excluded.body, saved_at=excluded.saved_at",
        libsql_rusqlite::params![project_id, digest, body, now],
    )?;
    Ok(())
}

/// The craft decisions of the committed chapters before `chapter_id`, in
/// chapter order: `(chapter_no, craft)` from each final chapter's r2 plan.
/// Chapters planned as r1 carry no craft and are skipped.
pub fn prior_plan_crafts(
    connection: &Connection,
    project_id: &str,
    chapter_id: &str,
) -> Result<Vec<(i64, serde_json::Value)>, libsql_rusqlite::Error> {
    let mut statement = connection.prepare(
        "SELECT c.chapter_no, json_extract(p.payload_json,'$.craft') FROM chapters c \
         JOIN artifact_payloads f ON f.artifact_id=c.final_artifact_id AND f.version=c.final_version \
         JOIN artifact_payloads p ON p.artifact_id=json_extract(f.payload_json,'$.chapterPlanRef.artifactId') AND p.version=json_extract(f.payload_json,'$.chapterPlanRef.version') \
         WHERE c.project_id=?1 AND c.status='COMMITTED' AND json_extract(p.payload_json,'$.craft') IS NOT NULL \
         AND c.chapter_no < (SELECT chapter_no FROM chapters WHERE project_id=?1 AND chapter_id=?2) \
         ORDER BY c.chapter_no",
    )?;
    let rows = statement
        .query_map(libsql_rusqlite::params![project_id, chapter_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(chapter, craft)| {
            serde_json::from_str(&craft)
                .ok()
                .map(|craft| (chapter, craft))
        })
        .collect())
}

/// The continuity details recorded in the current production receipt of each
/// committed chapter before `chapter_id`, in chapter order. Each receipt is
/// read through its chapter's canonical head, so retried runs are skipped.
pub fn prior_continuity_details(
    connection: &Connection,
    project_id: &str,
    chapter_id: &str,
) -> Result<Vec<(i64, serde_json::Value)>, String> {
    let mut statement = connection
        .prepare(
            "SELECT chapter_id, chapter_no FROM chapters WHERE project_id=?1 AND status='COMMITTED' \
             AND chapter_no < (SELECT chapter_no FROM chapters WHERE project_id=?1 AND chapter_id=?2) \
             ORDER BY chapter_no",
        )
        .map_err(|error| error.to_string())?;
    let chapters = statement
        .query_map(libsql_rusqlite::params![project_id, chapter_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .and_then(Iterator::collect::<Result<Vec<_>, _>>)
        .map_err(|error| error.to_string())?;
    let mut details = Vec::new();
    for (earlier, number) in chapters {
        let receipt = crate::repositories::project_entry::canonical_chapter_head(
            connection,
            project_id,
            "ProductionReceipt",
            &earlier,
        )
        .map_err(|error| error.to_string())?;
        if let Some(found) = receipt.map(|row| row.payload["continuityDetails"].clone()) {
            if found.is_array() {
                details.push((number, found));
            }
        }
    }
    Ok(details)
}

pub fn style_sample(
    connection: &Connection,
    project_id: &str,
) -> Result<Option<StyleSample>, libsql_rusqlite::Error> {
    connection
        .query_row(
            "SELECT sample_digest, body, saved_at FROM style_samples WHERE project_id=?1",
            [project_id],
            |row| {
                Ok(StyleSample {
                    digest: row.get(0)?,
                    body: row.get(1)?,
                    saved_at: row.get(2)?,
                })
            },
        )
        .optional()
}
