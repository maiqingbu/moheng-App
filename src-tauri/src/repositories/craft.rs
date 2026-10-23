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
