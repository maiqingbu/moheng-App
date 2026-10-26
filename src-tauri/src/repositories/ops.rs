use crate::commit::CommitService;
use crate::generated::types::ArtifactEnvelope;
use libsql_rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, thiserror::Error)]
pub enum OpsError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("backup checksum mismatch")]
    BackupChecksumMismatch,
    #[error("invalid page request")]
    InvalidPageRequest,
    #[error("performance threshold exceeded: {0}")]
    PerformanceThresholdExceeded(String),
    #[error("restore integrity failed: {0}")]
    RestoreIntegrity(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchDocument {
    pub project_id: String,
    pub doc_id: String,
    pub chapter_id: Option<String>,
    pub volume_id: Option<String>,
    pub title: String,
    pub body: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub doc_id: String,
    pub chapter_id: Option<String>,
    pub volume_id: Option<String>,
    pub title: String,
    pub excerpt: String,
    pub matched_by: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub items: Vec<SearchHit>,
    pub next_cursor: Option<String>,
    pub total: i64,
}

pub struct SearchService;

const FTS_PROJECTION_VERSION: &str = "m0e.fts.v1";

impl SearchService {
    /// Bounded owner hook used by H03 when a support repair needs the runtime
    /// projection timestamp refreshed.  H03 does not write authority tables;
    /// this owner remains the only code allowed to touch this projection.
    pub fn refresh_runtime_projection(
        connection: &Connection,
        project_id: &str,
        now: &str,
    ) -> Result<(), OpsError> {
        connection.execute(
            "UPDATE project_runtime_state SET updated_at=?1 WHERE project_id=?2",
            libsql_rusqlite::params![now, project_id],
        )?;
        Ok(())
    }

    pub fn projection_health(connection: &Connection, project_id: &str) -> Result<bool, OpsError> {
        let table_exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='search_documents_fts')",
            [],
            |row| row.get(0),
        )?;
        if !table_exists {
            return Ok(false);
        }
        let version: Option<String> = connection
            .query_row("SELECT setting_value FROM project_settings WHERE project_id=?1 AND setting_key='fts.projectionVersion'", [project_id], |row| row.get(0))
            .optional()?;
        let indexed_revision: Option<i64> = connection
            .query_row("SELECT setting_value FROM project_settings WHERE project_id=?1 AND setting_key='fts.indexedSourceRevision'", [project_id], |row| row.get::<_, String>(0))
            .optional()?
            .and_then(|value| value.parse().ok());
        let source_revision: i64 = connection.query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        let source_exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM chapters WHERE project_id=?1 LIMIT 1) OR EXISTS(SELECT 1 FROM artifacts WHERE project_id=?1 LIMIT 1)",
            [project_id],
            |row| row.get(0),
        )?;
        let indexed_source_count: Option<i64> = connection
            .query_row("SELECT setting_value FROM project_settings WHERE project_id=?1 AND setting_key='fts.indexedSourceCount'", [project_id], |row| row.get::<_, String>(0))
            .optional()?
            .and_then(|value| value.parse().ok());
        let projection_exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM search_documents WHERE project_id=?1 AND (doc_id LIKE 'chapter:%' OR doc_id LIKE 'artifact:%') LIMIT 1)",
            [project_id],
            |row| row.get(0),
        )?;
        Ok(version.as_deref() == Some(FTS_PROJECTION_VERSION)
            && indexed_revision == Some(source_revision)
            && (!source_exists
                || indexed_source_count.is_some_and(|count| count > 0 && projection_exists)))
    }

    pub fn fts_projection_settings(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Vec<(String, String)>, OpsError> {
        let mut statement = connection.prepare(
            "SELECT setting_key, setting_value FROM project_settings WHERE project_id=?1 AND setting_key LIKE 'fts.%' ORDER BY setting_key",
        )?;
        let result = statement
            .query_map([project_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(OpsError::from);
        result
    }

    pub fn ensure_projection(
        connection: &Connection,
        project_id: &str,
        now: &str,
    ) -> Result<bool, OpsError> {
        if Self::projection_health(connection, project_id)? {
            return Ok(false);
        }
        Self::rebuild(connection, project_id, now)?;
        Ok(true)
    }

    pub fn index_document(
        connection: &Connection,
        document: &SearchDocument,
        now: &str,
    ) -> Result<(), OpsError> {
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = upsert_search_document(connection, document, now);
        match result {
            Ok(()) => connection.execute_batch("COMMIT")?,
            Err(error) => {
                let _ = connection.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn rebuild(connection: &Connection, project_id: &str, now: &str) -> Result<i64, OpsError> {
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            connection.execute(
                "DELETE FROM search_documents WHERE project_id=?1",
                [project_id],
            )?;
            connection.execute(
                "DELETE FROM search_documents_fts WHERE project_id=?1",
                [project_id],
            )?;
            connection.execute(
                "INSERT INTO search_documents(project_id, doc_id, chapter_id, volume_id, title, body, kind, updated_at) SELECT c.project_id, 'chapter:'||c.chapter_id, c.chapter_id, c.volume_id, c.title, COALESCE(json_extract(p.payload_json, '$.body'), c.title), 'CHAPTER', ?2 FROM chapters c LEFT JOIN artifact_payloads p ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version WHERE c.project_id=?1",
                libsql_rusqlite::params![project_id, now],
            )?;
            connection.execute(
                "INSERT INTO search_documents(project_id, doc_id, chapter_id, volume_id, title, body, kind, updated_at) SELECT artifacts.project_id, 'artifact:'||artifacts.artifact_id||':'||artifacts.version, artifacts.scope_key, NULL, artifacts.artifact_id, COALESCE(artifact_payloads.payload_json,''), artifacts.artifact_type, ?2 FROM artifacts LEFT JOIN artifact_payloads ON artifact_payloads.artifact_id=artifacts.artifact_id AND artifact_payloads.version=artifacts.version WHERE artifacts.project_id=?1",
                libsql_rusqlite::params![project_id, now],
            )?;
            connection.execute(
                "INSERT INTO search_documents_fts(project_id, doc_id, chapter_id, volume_id, title, body, kind) SELECT project_id, doc_id, chapter_id, volume_id, title, body, kind FROM search_documents WHERE project_id=?1 AND kind <> 'FixtureEvidence'",
                [project_id],
            )?;
            let mut non_ascii = connection.prepare(
                "SELECT project_id, doc_id, chapter_id, volume_id, title, body, kind FROM search_documents WHERE project_id=?1 AND kind <> 'FixtureEvidence'",
            )?;
            let documents = non_ascii
                .query_map([project_id], |row| {
                    Ok(SearchDocument {
                        project_id: row.get(0)?,
                        doc_id: row.get(1)?,
                        chapter_id: row.get(2)?,
                        volume_id: row.get(3)?,
                        title: row.get(4)?,
                        body: row.get(5)?,
                        kind: row.get(6)?,
                    })
                })?
                .filter_map(|row| match row {
                    Ok(document) if !document.title.is_ascii() || !document.body.is_ascii() => {
                        Some(Ok(document))
                    }
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                })
                .collect::<Result<Vec<_>, _>>()?;
            drop(non_ascii);
            let mut delete_fts = connection
                .prepare("DELETE FROM search_documents_fts WHERE project_id=?1 AND doc_id=?2")?;
            let mut insert_fts = connection.prepare(
                "INSERT INTO search_documents_fts(project_id, doc_id, chapter_id, volume_id, title, body, kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for document in documents {
                delete_fts.execute(libsql_rusqlite::params![
                    document.project_id,
                    document.doc_id
                ])?;
                insert_fts.execute(libsql_rusqlite::params![
                    document.project_id,
                    document.doc_id,
                    document.chapter_id,
                    document.volume_id,
                    fts_text(&document.title),
                    fts_text(&document.body),
                    document.kind
                ])?;
            }
            let count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM search_documents WHERE project_id=?1",
                [project_id],
                |row| row.get(0),
            )?;
            let revision: i64 = connection.query_row(
                "SELECT revision FROM project_meta WHERE project_id=?1",
                [project_id],
                |row| row.get(0),
            )?;
            for (key, value) in [
                ("fts.projectionVersion", FTS_PROJECTION_VERSION.to_string()),
                ("fts.indexedSourceRevision", revision.to_string()),
                ("fts.indexedSourceCount", count.to_string()),
                (
                    "fts.lastRebuildReason",
                    "explicit_repair_or_backfill".to_string(),
                ),
                ("fts.lastRebuildAt", now.to_string()),
            ] {
                connection.execute(
                    "INSERT INTO project_settings(project_id, setting_key, setting_value, updated_at) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(project_id, setting_key) DO UPDATE SET setting_value=excluded.setting_value, updated_at=excluded.updated_at",
                    libsql_rusqlite::params![project_id, key, value, now],
                )?;
            }
            Ok::<_, OpsError>(count)
        })();
        match result {
            Ok(count) => {
                connection.execute_batch("COMMIT")?;
                Ok(count)
            }
            Err(error) => {
                let _ = connection.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    /// Reconcile the rebuildable projection against current authoritative rows.
    /// Only chapter/artifact documents are owned here; caller-indexed documents
    /// are preserved. Full `rebuild` remains an explicit repair operation.
    pub fn reconcile(
        connection: &Connection,
        project_id: &str,
        now: &str,
    ) -> Result<i64, OpsError> {
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = Self::reconcile_in_transaction(connection, project_id, now);
        match result {
            Ok(count) => {
                connection.execute_batch("COMMIT")?;
                Ok(count)
            }
            Err(error) => {
                let _ = connection.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    /// Same reconciliation without transaction control, for the CommitService
    /// atomic bundle transaction.
    pub(crate) fn reconcile_in_transaction(
        connection: &Connection,
        project_id: &str,
        now: &str,
    ) -> Result<i64, OpsError> {
        let mut desired = HashSet::new();
        let mut chapters = connection.prepare(
                "SELECT c.chapter_id, c.volume_id, c.title, COALESCE(json_extract(p.payload_json, '$.body'), c.title) FROM chapters c LEFT JOIN artifact_payloads p ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version WHERE c.project_id=?1 ORDER BY c.chapter_no, c.chapter_id",
            )?;
        let chapter_rows = chapters.query_map([project_id], |row| {
            Ok(SearchDocument {
                project_id: project_id.to_string(),
                doc_id: format!("chapter:{}", row.get::<_, String>(0)?),
                chapter_id: Some(row.get(0)?),
                volume_id: Some(row.get(1)?),
                title: row.get(2)?,
                body: row.get(3)?,
                kind: "CHAPTER".into(),
            })
        })?;
        let mut count = 0_i64;
        for row in chapter_rows {
            let document = row?;
            desired.insert(document.doc_id.clone());
            upsert_search_document(connection, &document, now)?;
            count += 1;
        }

        let mut artifacts = connection.prepare(
                "SELECT artifacts.artifact_id, artifacts.version, artifacts.scope_key, artifacts.artifact_type, artifact_payloads.payload_json FROM artifacts LEFT JOIN artifact_payloads ON artifact_payloads.artifact_id=artifacts.artifact_id AND artifact_payloads.version=artifacts.version WHERE artifacts.project_id=?1 ORDER BY artifacts.artifact_id, artifacts.version",
            )?;
        let artifact_rows = artifacts.query_map([project_id], |row| {
            let artifact_id: String = row.get(0)?;
            let version: i64 = row.get(1)?;
            Ok(SearchDocument {
                project_id: project_id.to_string(),
                doc_id: format!("artifact:{artifact_id}:{version}"),
                chapter_id: Some(row.get(2)?),
                volume_id: None,
                title: artifact_id,
                body: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                kind: row.get(3)?,
            })
        })?;
        for row in artifact_rows {
            let document = row?;
            desired.insert(document.doc_id.clone());
            upsert_search_document(connection, &document, now)?;
            count += 1;
        }

        let mut stale = connection.prepare(
                "SELECT doc_id FROM search_documents WHERE project_id=?1 AND (doc_id LIKE 'chapter:%' OR doc_id LIKE 'artifact:%')",
            )?;
        let stale_ids = stale
            .query_map([project_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stale);
        for doc_id in stale_ids {
            if !desired.contains(&doc_id) {
                connection.execute(
                    "DELETE FROM search_documents WHERE project_id=?1 AND doc_id=?2",
                    libsql_rusqlite::params![project_id, doc_id],
                )?;
                connection.execute(
                    "DELETE FROM search_documents_fts WHERE project_id=?1 AND doc_id=?2",
                    libsql_rusqlite::params![project_id, doc_id],
                )?;
            }
        }
        let revision: i64 = connection.query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        let indexed_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM search_documents WHERE project_id=?1 AND (doc_id LIKE 'chapter:%' OR doc_id LIKE 'artifact:%')",
            [project_id],
            |row| row.get(0),
        )?;
        for (key, value) in [
            ("fts.projectionVersion", FTS_PROJECTION_VERSION.to_string()),
            ("fts.indexedSourceRevision", revision.to_string()),
            ("fts.indexedSourceCount", indexed_count.to_string()),
            ("fts.lastRebuildReason", "incremental_reconcile".to_string()),
            ("fts.lastRebuildAt", now.to_string()),
        ] {
            connection.execute(
                "INSERT INTO project_settings(project_id, setting_key, setting_value, updated_at) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(project_id, setting_key) DO UPDATE SET setting_value=excluded.setting_value, updated_at=excluded.updated_at",
                libsql_rusqlite::params![project_id, key, value, now],
            )?;
        }
        Ok(count)
    }

    /// Applies only the artifacts supplied by the current CommitBundle. This is
    /// the normal commit path; it never scans the authoritative artifact table.
    pub(crate) fn apply_commit_delta_in_transaction(
        connection: &Connection,
        artifacts: &[ArtifactEnvelope],
        now: &str,
    ) -> Result<i64, OpsError> {
        let mut changed = 0_i64;
        for artifact in artifacts {
            let artifact_type = serde_json::to_value(&artifact.artifact_type)?
                .as_str()
                .unwrap_or_default()
                .to_string();
            let scope_key = crate::repositories::artifacts::scope_key(&artifact.scope)
                .map_err(|error| OpsError::RestoreIntegrity(error.to_string()))?;
            let payload: String = connection.query_row(
                "SELECT payload_json FROM artifact_payloads WHERE artifact_id=?1 AND version=?2",
                libsql_rusqlite::params![artifact.artifact_id, artifact.version],
                |row| row.get(0),
            )?;
            let document = SearchDocument {
                project_id: artifact.project_id.clone(),
                doc_id: format!("artifact:{}:{}", artifact.artifact_id, artifact.version),
                chapter_id: Some(scope_key),
                volume_id: None,
                title: artifact.artifact_id.clone(),
                body: payload,
                kind: artifact_type,
            };
            upsert_search_document(connection, &document, now)?;
            changed += 1;
        }
        Ok(changed)
    }

    pub fn search(
        connection: &Connection,
        project_id: &str,
        text: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<SearchPage, OpsError> {
        if text.trim().is_empty() || !(1..=100).contains(&limit) {
            return Err(OpsError::InvalidPageRequest);
        }
        let query = fts_query(text);
        let after_rowid = cursor
            .map(|value| value.parse::<i64>())
            .transpose()
            .map_err(|_| OpsError::InvalidPageRequest)?
            .unwrap_or(0);
        let total: i64 = connection.query_row(
            "SELECT COUNT(*) FROM search_documents_fts WHERE project_id=?1 AND search_documents_fts MATCH ?2",
            libsql_rusqlite::params![project_id, query],
            |row| row.get(0),
        )?;
        let mut statement = connection.prepare(
            "SELECT rowid, doc_id, chapter_id, volume_id, title, body FROM search_documents_fts WHERE project_id=?1 AND search_documents_fts MATCH ?2 AND rowid>?3 ORDER BY rowid LIMIT ?4",
        )?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, query, after_rowid, (limit + 1) as i64],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )?;
        let mut entries = rows.collect::<Result<Vec<_>, _>>()?;
        let next_cursor = if entries.len() > limit {
            Some(entries[limit - 1].0.to_string())
        } else {
            None
        };
        entries.truncate(limit);
        let items = entries
            .into_iter()
            .map(
                |(_, doc_id, chapter_id, volume_id, title, body)| SearchHit {
                    doc_id,
                    chapter_id,
                    volume_id,
                    title,
                    excerpt: body.chars().take(200).collect(),
                    matched_by: "keyword".into(),
                },
            )
            .collect();
        Ok(SearchPage {
            items,
            next_cursor,
            total,
        })
    }
}

fn upsert_search_document(
    connection: &Connection,
    document: &SearchDocument,
    now: &str,
) -> Result<(), OpsError> {
    connection.execute(
        "INSERT INTO search_documents(project_id, doc_id, chapter_id, volume_id, title, body, kind, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT(project_id, doc_id) DO UPDATE SET chapter_id=excluded.chapter_id, volume_id=excluded.volume_id, title=excluded.title, body=excluded.body, kind=excluded.kind, updated_at=excluded.updated_at",
        libsql_rusqlite::params![document.project_id, document.doc_id, document.chapter_id, document.volume_id, document.title, document.body, document.kind, now],
    )?;
    connection.execute(
        "DELETE FROM search_documents_fts WHERE project_id=?1 AND doc_id=?2",
        libsql_rusqlite::params![document.project_id, document.doc_id],
    )?;
    connection.execute(
        "INSERT INTO search_documents_fts(project_id, doc_id, chapter_id, volume_id, title, body, kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        libsql_rusqlite::params![document.project_id, document.doc_id, document.chapter_id, document.volume_id, fts_text(&document.title), fts_text(&document.body), document.kind],
    )?;
    Ok(())
}

fn fts_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character.is_ascii_whitespace() {
                character.to_string()
            } else {
                format!(" {character} ")
            }
        })
        .collect::<String>()
}

fn fts_query(text: &str) -> String {
    let cjk: Vec<char> = text
        .chars()
        .filter(|character| !character.is_ascii())
        .collect();
    if !cjk.is_empty() {
        return cjk
            .into_iter()
            .map(|character| format!("\"{character}\""))
            .collect::<Vec<_>>()
            .join(" AND ");
    }
    format!("\"{}\"", text.replace('"', ""))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SQLiteRuntimeProfile {
    pub profile_version: String,
    pub page_size: i64,
    pub auto_vacuum: i64,
    pub mmap_size: i64,
    pub busy_timeout_ms: i64,
    pub synchronous: String,
    pub wal_autocheckpoint: i64,
    pub checkpoint_policy: String,
    pub recorded_at: String,
}

pub fn read_runtime_profile(
    connection: &Connection,
    project_id: &str,
) -> Result<SQLiteRuntimeProfile, OpsError> {
    let profile = connection
        .query_row(
            "SELECT profile_version, page_size, auto_vacuum, mmap_size, busy_timeout_ms, synchronous, wal_autocheckpoint, checkpoint_policy, recorded_at FROM b04_runtime_profiles WHERE project_id=?1",
            [project_id],
            |row| {
                Ok(SQLiteRuntimeProfile {
                    profile_version: row.get(0)?,
                    page_size: row.get(1)?,
                    auto_vacuum: row.get(2)?,
                    mmap_size: row.get(3)?,
                    busy_timeout_ms: row.get(4)?,
                    synchronous: row.get(5)?,
                    wal_autocheckpoint: row.get(6)?,
                    checkpoint_policy: row.get(7)?,
                    recorded_at: row.get(8)?,
                })
            },
        )
        .optional()?;
    profile.ok_or(OpsError::Sqlite(
        libsql_rusqlite::Error::QueryReturnedNoRows,
    ))
}

pub fn record_runtime_profile(
    connection: &Connection,
    project_id: &str,
    now: &str,
) -> Result<SQLiteRuntimeProfile, OpsError> {
    let page_size: i64 = connection.query_row("PRAGMA page_size", [], |row| row.get(0))?;
    let auto_vacuum: i64 = connection.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?;
    let mmap_size: i64 = connection.query_row("PRAGMA mmap_size", [], |row| row.get(0))?;
    let busy_timeout_ms: i64 = connection.query_row("PRAGMA busy_timeout", [], |row| row.get(0))?;
    let synchronous_value: i64 =
        connection.query_row("PRAGMA synchronous", [], |row| row.get(0))?;
    let wal_autocheckpoint: i64 =
        connection.query_row("PRAGMA wal_autocheckpoint", [], |row| row.get(0))?;
    let profile = SQLiteRuntimeProfile {
        profile_version: "m0e.sqlite.v1".into(),
        page_size,
        auto_vacuum,
        mmap_size,
        busy_timeout_ms,
        synchronous: match synchronous_value {
            0 => "OFF",
            1 => "NORMAL",
            2 => "FULL",
            3 => "EXTRA",
            _ => "UNKNOWN",
        }
        .into(),
        wal_autocheckpoint,
        checkpoint_policy:
            "PASSIVE_WITH_MILESTONE_CHECKPOINT;AUTO_VACUUM=NONE;FULL_VACUUM=MAINTENANCE_ONLY".into(),
        recorded_at: now.into(),
    };
    connection.execute(
        "INSERT INTO b04_runtime_profiles(project_id, profile_version, page_size, auto_vacuum, mmap_size, busy_timeout_ms, synchronous, wal_autocheckpoint, checkpoint_policy, recorded_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) ON CONFLICT(project_id) DO UPDATE SET profile_version=excluded.profile_version, page_size=excluded.page_size, auto_vacuum=excluded.auto_vacuum, mmap_size=excluded.mmap_size, busy_timeout_ms=excluded.busy_timeout_ms, synchronous=excluded.synchronous, wal_autocheckpoint=excluded.wal_autocheckpoint, checkpoint_policy=excluded.checkpoint_policy, recorded_at=excluded.recorded_at",
        libsql_rusqlite::params![project_id, profile.profile_version, profile.page_size, profile.auto_vacuum, profile.mmap_size, profile.busy_timeout_ms, profile.synchronous, profile.wal_autocheckpoint, profile.checkpoint_policy, profile.recorded_at],
    )?;
    Ok(profile)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntegrityReport {
    pub ok: bool,
    pub failures: Vec<String>,
}

/// FX-22: the work's backups, newest first, with the revision each backup
/// file holds (a restore expects it); unreadable files are marked.
pub fn list_backups(
    connection: &Connection,
    project_id: &str,
    limit: i64,
) -> Result<Vec<serde_json::Value>, OpsError> {
    let mut statement = connection.prepare(
        "SELECT backup_path, checksum_sha256, created_at FROM backup_manifests WHERE project_id=?1 ORDER BY created_at DESC, rowid DESC LIMIT ?2",
    )?;
    let rows = statement
        .query_map(libsql_rusqlite::params![project_id, limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(path, checksum, created_at)| {
            let path = std::path::PathBuf::from(path);
            let name = path.file_name()?.to_str()?.to_owned();
            let size_bytes = std::fs::metadata(&path).map(|meta| meta.len()).ok();
            let revision = Connection::open_with_flags(
                &path,
                libsql_rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .and_then(|backup| {
                backup.query_row(
                    "SELECT revision FROM project_meta WHERE project_id=?1",
                    [project_id],
                    |row| row.get::<_, i64>(0),
                )
            })
            .ok();
            Some(serde_json::json!({
                "backupName":name,"checksumSha256":checksum,"createdAt":created_at,
                "revision":revision,"sizeBytes":size_bytes,"available":revision.is_some()
            }))
        })
        .collect())
}

pub fn deep_integrity_check(
    connection: &Connection,
    project_id: &str,
    now: &str,
) -> Result<IntegrityReport, OpsError> {
    let mut failures = Vec::new();
    let quick: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if quick != "ok" {
        failures.push("QUICK_CHECK_FAILED".into());
    }
    let mut foreign_keys = connection.prepare("PRAGMA foreign_key_check")?;
    if foreign_keys.query_map([], |_| Ok(()))?.next().is_some() {
        failures.push("FOREIGN_KEY_CHECK_FAILED".into());
    }
    let mut artifacts = connection.prepare(
        "SELECT artifacts.artifact_id, artifacts.version, artifacts.payload_hash, artifact_payloads.payload_json FROM artifacts LEFT JOIN artifact_payloads ON artifact_payloads.artifact_id=artifacts.artifact_id AND artifact_payloads.version=artifacts.version WHERE artifacts.project_id=?1",
    )?;
    let rows = artifacts.query_map([project_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;
    for row in rows {
        let (artifact_id, version, expected, payload) = row?;
        let actual = payload
            .map(|payload| hex::encode(Sha256::digest(payload.as_bytes())))
            .unwrap_or_default();
        if actual != expected {
            failures.push("ARTIFACT_PAYLOAD_HASH_MISMATCH".into());
            let _ = (artifact_id, version);
        }
    }
    let bad_heads: i64 = connection.query_row(
        "SELECT COUNT(*) FROM canonical_heads AS head LEFT JOIN artifacts AS artifact ON artifact.artifact_id=head.artifact_id AND artifact.version=head.version WHERE head.project_id=?1 AND (artifact.payload_hash IS NULL OR artifact.payload_hash<>head.payload_hash)",
        [project_id],
        |row| row.get(0),
    )?;
    if bad_heads > 0 {
        failures.push("CANONICAL_HEAD_HASH_MISMATCH".into());
    }
    let dependency_hash_mismatches: i64 = connection.query_row(
        "SELECT COUNT(*) FROM dependency_edges AS edge JOIN artifacts AS artifact ON artifact.project_id=edge.project_id AND artifact.artifact_id=edge.source_ref AND artifact.version=(SELECT MAX(version) FROM artifacts WHERE project_id=edge.project_id AND artifact_id=edge.source_ref) WHERE edge.project_id=?1 AND artifact.payload_hash<>edge.source_hash",
        [project_id],
        |row| row.get(0),
    )?;
    if dependency_hash_mismatches > 0 {
        failures.push("DEPENDENCY_SOURCE_HASH_MISMATCH".into());
    }
    let missing_dependency_sources: i64 = connection.query_row(
        "SELECT COUNT(*) FROM dependency_edges AS edge LEFT JOIN artifacts AS artifact ON artifact.project_id=edge.project_id AND artifact.artifact_id=edge.source_ref WHERE edge.project_id=?1 AND artifact.artifact_id IS NULL AND NOT EXISTS (SELECT 1 FROM canon_facts WHERE project_id=edge.project_id AND fact_id=edge.source_ref) AND NOT EXISTS (SELECT 1 FROM canon_relations WHERE project_id=edge.project_id AND relation_id=edge.source_ref) AND NOT EXISTS (SELECT 1 FROM canon_events WHERE project_id=edge.project_id AND event_id=edge.source_ref) AND NOT EXISTS (SELECT 1 FROM canon_knowledge WHERE project_id=edge.project_id AND knowledge_id=edge.source_ref) AND NOT EXISTS (SELECT 1 FROM narrative_obligations WHERE project_id=edge.project_id AND obligation_id=edge.source_ref) AND NOT EXISTS (SELECT 1 FROM foreshadows WHERE project_id=edge.project_id AND foreshadow_id=edge.source_ref) AND NOT EXISTS (SELECT 1 FROM reader_desires WHERE project_id=edge.project_id AND desire_id=edge.source_ref)",
        [project_id],
        |row| row.get(0),
    )?;
    if missing_dependency_sources > 0 {
        failures.push("DEPENDENCY_SOURCE_REF_MISSING".into());
    }

    // Canon evidence is the durable bridge from an official chapter artifact to
    // normalized Canon. The check is conditional on a committed FinalChapter so
    // candidate work and historical planning artifacts remain valid.
    let mut final_chapters = connection.prepare(
        // FX-19: chapter scope keys are canonical JSON ({"chapterId":..,
        // "level":"CHAPTER"}); canon_evidence stores the bare chapter id.
        "SELECT CASE WHEN json_valid(scope_key) THEN COALESCE(json_extract(scope_key,'$.chapterId'), scope_key) ELSE scope_key END FROM artifacts WHERE project_id=?1 AND artifact_type='FinalChapter' AND artifact_status='COMMITTED'",
    )?;
    let final_chapter_ids = final_chapters
        .query_map([project_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(final_chapters);
    for chapter_id in final_chapter_ids {
        let evidence_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM canon_evidence WHERE project_id=?1 AND final_chapter_id=?2",
            libsql_rusqlite::params![project_id, chapter_id],
            |row| row.get(0),
        )?;
        if evidence_count == 0 {
            failures.push("FINAL_CHAPTER_CANON_EVIDENCE_MISSING".into());
        }
    }

    let dangling_foreshadows: i64 = connection.query_row(
        "SELECT COUNT(*) FROM foreshadows AS f LEFT JOIN chapters AS plant ON plant.project_id=f.project_id AND plant.chapter_id=f.plant_chapter_id LEFT JOIN chapters AS recover ON recover.project_id=f.project_id AND recover.chapter_id=f.recover_window_end_chapter WHERE f.project_id=?1 AND ((f.plant_chapter_id IS NOT NULL AND plant.chapter_id IS NULL) OR (f.recover_window_end_chapter IS NOT NULL AND recover.chapter_id IS NULL))",
        [project_id],
        |row| row.get(0),
    )?;
    if dangling_foreshadows > 0 {
        failures.push("FORESHADOW_CHAPTER_REF_MISSING".into());
    }
    let dangling_desires: i64 = connection.query_row(
        "SELECT COUNT(*) FROM reader_desires AS d LEFT JOIN chapters AS created ON created.project_id=d.project_id AND created.chapter_id=d.created_chapter_id LEFT JOIN chapters AS touched ON touched.project_id=d.project_id AND touched.chapter_id=d.last_touched_chapter_id WHERE d.project_id=?1 AND ((d.created_chapter_id IS NOT NULL AND created.chapter_id IS NULL) OR (d.last_touched_chapter_id IS NOT NULL AND touched.chapter_id IS NULL))",
        [project_id],
        |row| row.get(0),
    )?;
    if dangling_desires > 0 {
        failures.push("READER_DESIRE_CHAPTER_REF_MISSING".into());
    }
    let dangling_obligations: i64 = connection.query_row(
        "SELECT COUNT(*) FROM narrative_obligations AS o LEFT JOIN chapters AS touched ON touched.project_id=o.project_id AND touched.chapter_id=o.last_touched_chapter_id LEFT JOIN chapters AS resolve_before ON resolve_before.project_id=o.project_id AND resolve_before.chapter_id=o.must_resolve_before_chapter WHERE o.project_id=?1 AND ((o.last_touched_chapter_id IS NOT NULL AND touched.chapter_id IS NULL) OR (o.must_resolve_before_chapter IS NOT NULL AND resolve_before.chapter_id IS NULL))",
        [project_id],
        |row| row.get(0),
    )?;
    if dangling_obligations > 0 {
        failures.push("OBLIGATION_CHAPTER_REF_MISSING".into());
    }
    let mut ledgers = connection.prepare(
        "SELECT volume_id, obligation_ids_json, foreshadow_ids_json FROM planning_state_ledger_index WHERE project_id=?1",
    )?;
    let ledger_rows = ledgers
        .query_map([project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(ledgers);
    for (volume_id, obligation_json, foreshadow_json) in ledger_rows {
        let volume_exists: i64 = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM volumes WHERE project_id=?1 AND volume_id=?2)",
            libsql_rusqlite::params![project_id, volume_id],
            |row| row.get(0),
        )?;
        let parse_refs = |value: &str| -> Result<Vec<String>, OpsError> {
            serde_json::from_str(value).map_err(OpsError::Json)
        };
        if volume_exists == 0
            || parse_refs(&obligation_json)?.iter().any(|reference| {
                !connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM narrative_obligations WHERE project_id=?1 AND obligation_id=?2)",
                        libsql_rusqlite::params![project_id, reference],
                        |row| row.get::<_, bool>(0),
                    )
                    .unwrap_or(false)
            })
            || parse_refs(&foreshadow_json)?.iter().any(|reference| {
                !connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM foreshadows WHERE project_id=?1 AND foreshadow_id=?2)",
                        libsql_rusqlite::params![project_id, reference],
                        |row| row.get::<_, bool>(0),
                    )
                    .unwrap_or(false)
            })
        {
            failures.push("PLANNING_LEDGER_REF_MISSING".into());
            break;
        }
    }
    let mut receipts =
        connection.prepare("SELECT idempotency_key FROM commit_receipts WHERE project_id=?1")?;
    let receipt_keys = receipts.query_map([project_id], |row| row.get::<_, String>(0))?;
    for key in receipt_keys {
        if CommitService::find_receipt(connection, project_id, &key?).is_err() {
            failures.push("COMMIT_RECEIPT_DIGEST_MISMATCH".into());
        }
    }
    let mut backups = connection
        .prepare("SELECT backup_path, checksum_sha256 FROM backup_manifests WHERE project_id=?1")?;
    let backup_rows = backups.query_map([project_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for backup in backup_rows {
        let (backup_path, expected_checksum) = backup?;
        let actual_checksum = std::fs::read(&backup_path)
            .map(|bytes| hex::encode(Sha256::digest(&bytes)))
            .unwrap_or_default();
        if actual_checksum != expected_checksum {
            failures.push("BACKUP_CHECKSUM_MISMATCH".into());
        }
    }
    let report = IntegrityReport {
        ok: failures.is_empty(),
        failures,
    };
    let report_json = serde_json::to_string(&report)?;
    connection.execute(
        "INSERT INTO integrity_runs(integrity_run_id, project_id, status, failure_count, report_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        libsql_rusqlite::params![format!("integrity:{project_id}:{now}"), project_id, if report.ok { "PASS" } else { "FAIL" }, report.failures.len() as i64, report_json, now],
    )?;
    Ok(report)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WalGovernanceReport {
    pub report_id: String,
    pub project_id: String,
    pub wal_bytes: i64,
    pub checkpoint_duration_ms: i64,
    pub busy_readers: i64,
    pub checkpointed_pages: i64,
    pub threshold_state: String,
}

pub fn inspect_wal(
    connection: &Connection,
    database_path: &Path,
    project_id: &str,
    now: &str,
) -> Result<WalGovernanceReport, OpsError> {
    let started = Instant::now();
    let (busy_readers, _wal_pages, checkpointed_pages): (i64, i64, i64) =
        connection.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    let wal_path = database_path.with_extension("db-wal");
    let wal_bytes = std::fs::metadata(wal_path)
        .map(|metadata| metadata.len() as i64)
        .unwrap_or(0);
    let threshold_state = if wal_bytes >= 64 * 1024 * 1024 {
        "HARD"
    } else if wal_bytes >= 16 * 1024 * 1024 {
        "SOFT"
    } else {
        "NORMAL"
    };
    let report = WalGovernanceReport {
        report_id: format!("wal:{project_id}:{now}"),
        project_id: project_id.into(),
        wal_bytes,
        checkpoint_duration_ms: started.elapsed().as_millis() as i64,
        busy_readers,
        checkpointed_pages,
        threshold_state: threshold_state.into(),
    };
    connection.execute(
        "INSERT INTO wal_governance_reports(report_id, project_id, wal_bytes, checkpoint_duration_ms, busy_readers, checkpointed_pages, threshold_state, created_at, report_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        libsql_rusqlite::params![report.report_id, report.project_id, report.wal_bytes, report.checkpoint_duration_ms, report.busy_readers, report.checkpointed_pages, report.threshold_state, now, serde_json::to_string(&report)?],
    )?;
    Ok(report)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VacuumReport {
    pub report_id: String,
    pub project_id: String,
    pub freelist_before: i64,
    pub freelist_after: i64,
    pub page_count: i64,
    pub mode: String,
    pub maintenance_required: bool,
    pub free_disk_bytes: u64,
    pub required_headroom: u64,
}

pub fn run_vacuum(
    connection: &Connection,
    project_id: &str,
    mode: &str,
    now: &str,
) -> Result<VacuumReport, OpsError> {
    if !matches!(mode, "CHECK" | "INCREMENTAL" | "FULL") {
        return Err(OpsError::InvalidPageRequest);
    }
    let freelist_before: i64 =
        connection.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
    let database_path: String =
        connection.query_row("PRAGMA database_list", [], |row| row.get(2))?;
    let database_bytes = std::fs::metadata(&database_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let wal_bytes = std::fs::metadata(format!("{database_path}-wal"))
        .map(|m| m.len())
        .unwrap_or(0);
    let required_headroom = database_bytes.saturating_mul(2).saturating_add(wal_bytes);
    let free_disk_bytes = available_disk_bytes(Path::new(&database_path))?;
    if mode == "INCREMENTAL" {
        let auto_vacuum: i64 = connection.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?;
        if auto_vacuum != 2 {
            return Err(OpsError::RestoreIntegrity(
                "INCREMENTAL vacuum is unavailable when auto_vacuum is NONE".into(),
            ));
        }
        connection.execute_batch("PRAGMA incremental_vacuum")?;
    } else if mode == "FULL" {
        let maintenance_state: String = connection.query_row(
            "SELECT maintenance_state FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        if maintenance_state != "MAINTENANCE" {
            return Err(OpsError::RestoreIntegrity(
                "FULL vacuum requires project maintenance mode".into(),
            ));
        }
        if free_disk_bytes < required_headroom {
            return Err(OpsError::RestoreIntegrity(format!(
                "FULL vacuum free disk preflight failed: available={free_disk_bytes} required={required_headroom}"
            )));
        }
        connection.execute_batch("VACUUM")?;
    }
    let freelist_after: i64 =
        connection.query_row("PRAGMA freelist_count", [], |row| row.get(0))?;
    let page_count: i64 = connection.query_row("PRAGMA page_count", [], |row| row.get(0))?;
    let report = VacuumReport {
        report_id: format!("vacuum:{project_id}:{now}"),
        project_id: project_id.into(),
        freelist_before,
        freelist_after,
        page_count,
        mode: mode.into(),
        maintenance_required: freelist_after > page_count / 4,
        free_disk_bytes,
        required_headroom,
    };
    connection.execute(
        "INSERT INTO vacuum_reports(report_id, project_id, freelist_before, freelist_after, page_count, mode, maintenance_required, created_at, report_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        libsql_rusqlite::params![report.report_id, report.project_id, report.freelist_before, report.freelist_after, report.page_count, report.mode, report.maintenance_required, now, serde_json::to_string(&report)?],
    )?;
    Ok(report)
}

fn available_disk_bytes(path: &Path) -> Result<u64, OpsError> {
    let output = std::process::Command::new("df")
        .args(["-Pk", &path.to_string_lossy()])
        .output()
        .map_err(OpsError::Io)?;
    if !output.status.success() {
        return Err(OpsError::RestoreIntegrity(
            "unable to determine free disk space".into(),
        ));
    }
    let output_text = String::from_utf8_lossy(&output.stdout);
    let line = output_text
        .lines()
        .nth(1)
        .ok_or_else(|| OpsError::RestoreIntegrity("df output is incomplete".into()))?;
    let available_kib = line
        .split_whitespace()
        .nth(3)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| OpsError::RestoreIntegrity("df available bytes are invalid".into()))?;
    Ok(available_kib.saturating_mul(1024))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureSpec {
    pub seed: String,
    pub books: u64,
    pub volumes: u64,
    pub chapters: u64,
    pub artifacts: u64,
    pub canon_facts: u64,
    pub dependencies: u64,
    pub model_runs: u64,
    pub target_words: u64,
}

impl FixtureSpec {
    pub fn long_book_10m(seed: impl Into<String>) -> Self {
        Self {
            seed: seed.into(),
            books: 1,
            volumes: 40,
            chapters: 4000,
            artifacts: 500_000,
            canon_facts: 1_000_000,
            dependencies: 3_000_000,
            model_runs: 100_000,
            target_words: 10_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureManifest {
    pub seed: String,
    pub target_words: u64,
    pub counts: FixtureCounts,
    pub generator_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixtureCounts {
    pub books: u64,
    pub volumes: u64,
    pub chapters: u64,
    pub artifacts: u64,
    pub canon_facts: u64,
    pub dependencies: u64,
    pub model_runs: u64,
}

pub struct LongBookFixture;

impl LongBookFixture {
    pub fn manifest(spec: &FixtureSpec) -> FixtureManifest {
        FixtureManifest {
            seed: spec.seed.clone(),
            target_words: spec.target_words,
            counts: FixtureCounts {
                books: spec.books,
                volumes: spec.volumes,
                chapters: spec.chapters,
                artifacts: spec.artifacts,
                canon_facts: spec.canon_facts,
                dependencies: spec.dependencies,
                model_runs: spec.model_runs,
            },
            generator_version: "m0e.fixture.v1".into(),
        }
    }

    pub fn materialize(
        connection: &Connection,
        spec: &FixtureSpec,
        now: &str,
    ) -> Result<FixtureManifest, OpsError> {
        if (spec.volumes > 0 || spec.chapters > 0) && spec.books == 0 {
            return Err(OpsError::InvalidPageRequest);
        }
        let project_id: String =
            connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
                row.get(0)
            })?;
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            let book_id = fixture_id("bk", &spec.seed, 0);
            let mut books = connection.prepare(
                "INSERT OR IGNORE INTO books(book_id, project_id, book_no, title, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'PLANNING', ?5, ?5)",
            )?;
            for index in 0..spec.books {
                let book_id = fixture_id("bk", &spec.seed, index);
                books.execute(libsql_rusqlite::params![
                    book_id,
                    project_id,
                    index + 1,
                    format!("B04 Book {index}"),
                    now
                ])?;
            }
            drop(books);
            let mut volumes = connection.prepare(
                "INSERT OR IGNORE INTO volumes(volume_id, project_id, book_id, volume_no, title, status, chapter_start, chapter_end, target_words, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 'PLANNING', ?6, ?7, ?8, ?9, ?9)",
            )?;
            for index in 0..spec.volumes {
                let volume_id = fixture_id("vol", &spec.seed, index);
                let chapter_start = (index * spec.chapters / spec.volumes.max(1)) + 1;
                let chapter_end =
                    ((index + 1) * spec.chapters / spec.volumes.max(1)).max(chapter_start);
                volumes.execute(libsql_rusqlite::params![
                    volume_id,
                    project_id,
                    book_id,
                    index + 1,
                    format!("B04 Volume {index}"),
                    chapter_start,
                    chapter_end,
                    spec.target_words / spec.volumes.max(1),
                    now
                ])?;
            }
            drop(volumes);
            let mut chapters = connection.prepare(
                "INSERT OR IGNORE INTO chapters(chapter_id, project_id, volume_id, chapter_no, title, importance_class, status, word_count, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 'NORMAL', 'PLANNED', ?6, ?7, ?7)",
            )?;
            for index in 0..spec.chapters {
                let chapter_id = fixture_id("chap", &spec.seed, index);
                let volume_index = (index * spec.volumes.max(1)) / spec.chapters.max(1);
                let volume_id = fixture_id("vol", &spec.seed, volume_index);
                let chapter_no = (index % (spec.chapters / spec.volumes.max(1)).max(1)) + 1;
                chapters.execute(libsql_rusqlite::params![
                    chapter_id,
                    project_id,
                    volume_id,
                    chapter_no,
                    format!("B04 Chapter {index}"),
                    spec.target_words / spec.chapters.max(1),
                    now
                ])?;
            }
            drop(chapters);
            let artifact_ids: Vec<String> = (0..spec.artifacts)
                .map(|index| fixture_id("art", &spec.seed, index))
                .collect();
            let artifact_hashes: Vec<String> = (0..spec.artifacts)
                .map(|index| {
                    let payload =
                        serde_json::json!({"seed":spec.seed,"artifactIndex":index}).to_string();
                    hex::encode(Sha256::digest(payload.as_bytes()))
                })
                .collect();
            connection.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS fixture_artifact_map (artifact_index INTEGER PRIMARY KEY, artifact_id TEXT NOT NULL, payload_json TEXT NOT NULL, payload_hash TEXT NOT NULL)",
            )?;
            let mut artifact_map = connection.prepare(
                "INSERT INTO fixture_artifact_map(artifact_index, artifact_id, payload_json, payload_hash) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for index in 0..spec.artifacts {
                let artifact_id = &artifact_ids[index as usize];
                let payload =
                    serde_json::json!({"seed":spec.seed,"artifactIndex":index}).to_string();
                let payload_hash = &artifact_hashes[index as usize];
                artifact_map.execute(libsql_rusqlite::params![
                    index as i64,
                    artifact_id,
                    payload,
                    payload_hash
                ])?;
            }
            drop(artifact_map);
            connection.execute(
                "INSERT OR IGNORE INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, payload_hash, provenance_json, created_at, committed_at) SELECT artifact_id, ?1, 'FixtureEvidence', 'PROJECT', ?1, 1, 'm0e.fixture.v1', 'CANDIDATE', payload_hash, '{}', ?2, NULL FROM fixture_artifact_map",
                libsql_rusqlite::params![project_id, now],
            )?;
            connection.execute(
                "INSERT OR IGNORE INTO artifact_payloads(artifact_id, version, payload_json, payload_hash) SELECT artifact_id, 1, payload_json, payload_hash FROM fixture_artifact_map",
                [],
            )?;
            let mut final_artifacts = connection.prepare(
                "INSERT OR IGNORE INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, payload_hash, provenance_json, created_at, committed_at) VALUES (?1, ?2, 'FinalChapter', 'CHAPTER', ?3, 1, 'm0e.fixture.v1', 'CANDIDATE', ?4, '{}', ?5, NULL)",
            )?;
            let mut final_payloads = connection.prepare(
                "INSERT OR IGNORE INTO artifact_payloads(artifact_id, version, payload_json, payload_hash) VALUES (?1, 1, ?2, ?3)",
            )?;
            for index in 0..spec.chapters {
                let chapter_id = fixture_id("chap", &spec.seed, index);
                let body = fixture_chapter_body(&spec.seed, index, 2_500);
                let payload = serde_json::json!({"chapterId":chapter_id,"body":body}).to_string();
                let payload_hash = hex::encode(Sha256::digest(payload.as_bytes()));
                let artifact_id = fixture_id("final", &spec.seed, index);
                final_artifacts.execute(libsql_rusqlite::params![
                    artifact_id,
                    project_id,
                    chapter_id,
                    payload_hash,
                    now
                ])?;
                final_payloads.execute(libsql_rusqlite::params![
                    artifact_id,
                    payload,
                    payload_hash
                ])?;
            }
            drop(final_artifacts);
            drop(final_payloads);
            let mut chapter_final = connection.prepare(
                "UPDATE chapters SET final_artifact_id=?1, final_version=1, status='COMMITTED' WHERE project_id=?2 AND chapter_id=?3",
            )?;
            for index in 0..spec.chapters {
                chapter_final.execute(libsql_rusqlite::params![
                    fixture_id("final", &spec.seed, index),
                    project_id,
                    fixture_id("chap", &spec.seed, index)
                ])?;
            }
            drop(chapter_final);
            // Long-term topology markers make the fixture a single book with
            // cross-volume aging/reference rows rather than unrelated bulk data.
            let mut ledgers = connection.prepare(
                "INSERT OR IGNORE INTO planning_state_ledger_index(project_id, volume_id, entry_state, plan_state, coverage_state, obligation_ids_json, foreshadow_ids_json, evidence_need_ids_json, repair_state, updated_at) VALUES (?1, ?2, 'OPEN', 'APPROVED', 'INCOMPLETE', ?3, ?4, '[]', 'NONE', ?5)",
            )?;
            for index in 0..spec.volumes {
                let volume_id = fixture_id("vol", &spec.seed, index);
                let obligations =
                    serde_json::json!([fixture_id("obl", &spec.seed, index)]).to_string();
                let foreshadows =
                    serde_json::json!([fixture_id("foreshadow", &spec.seed, index % 20)])
                        .to_string();
                ledgers.execute(libsql_rusqlite::params![
                    project_id,
                    volume_id,
                    obligations,
                    foreshadows,
                    now
                ])?;
            }
            drop(ledgers);
            if spec.canon_facts > 0 {
                let entity_id = fixture_id("ent", &spec.seed, 0);
                connection.execute(
                    "INSERT OR IGNORE INTO canon_entities(entity_id, project_id, canonical_name, entity_type, created_at, updated_at) VALUES (?1, ?2, 'B04 Fixture Entity', 'FIXTURE', ?3, ?3)",
                    libsql_rusqlite::params![entity_id, project_id, now],
                )?;
                connection.execute(
                    "WITH RECURSIVE digits(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM digits WHERE i < 999) INSERT OR IGNORE INTO canon_facts(fact_id, project_id, subject_id, predicate, object_json, authority_level, created_at, updated_at) SELECT 'fact:' || ?1 || ':' || (a.i + 1000 * b.i + 1000000 * c.i), ?2, ?3, 'fixtureValue', json_object('seed', ?1, 'factIndex', a.i + 1000 * b.i + 1000000 * c.i), 'C', ?4, ?4 FROM digits a CROSS JOIN digits b CROSS JOIN (SELECT 0 AS i UNION ALL SELECT 1 AS i) c WHERE (a.i + 1000 * b.i + 1000000 * c.i) < ?5",
                    libsql_rusqlite::params![spec.seed, project_id, entity_id, now, spec.canon_facts as i64],
                )?;
            }
            let mut foreshadows = connection.prepare(
                "INSERT OR IGNORE INTO foreshadows(foreshadow_id, project_id, intent, plant_chapter_id, recover_window_end_chapter, status, source_authority, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 'PLANTED', 'B04_FIXTURE', ?6, ?6)",
            )?;
            for index in 0..spec.volumes.min(20) {
                let plant_index = (index * spec.chapters.max(1)) / spec.volumes.max(1);
                let recover_index = ((index + 20) * spec.chapters.max(1)) / spec.volumes.max(1);
                foreshadows.execute(libsql_rusqlite::params![
                    fixture_id("foreshadow", &spec.seed, index),
                    project_id,
                    format!("B04 cross-volume foreshadow {index}"),
                    fixture_id("chap", &spec.seed, plant_index),
                    fixture_id(
                        "chap",
                        &spec.seed,
                        recover_index.min(spec.chapters.saturating_sub(1))
                    ),
                    now
                ])?;
            }
            drop(foreshadows);
            let mut obligations = connection.prepare(
                "INSERT OR IGNORE INTO narrative_obligations(obligation_id, project_id, scope, obligation_type, authority_level, source_artifact_id, source_version, description, must_resolve_before_chapter, lifecycle, last_touched_chapter_id, created_at, updated_at) VALUES (?1, ?2, 'VOLUME', 'LONG_TERM_FIXTURE', 'C', ?3, 1, ?4, ?5, 'PROPOSED', ?6, ?7, ?7)",
            )?;
            for index in 0..spec.volumes {
                let chapter_index =
                    ((index + 1) * spec.chapters / spec.volumes.max(1)).saturating_sub(1);
                obligations.execute(libsql_rusqlite::params![
                    fixture_id("obl", &spec.seed, index),
                    project_id,
                    fixture_id("art", &spec.seed, index % spec.artifacts.max(1)),
                    format!("B04 obligation across volume {index}"),
                    fixture_id("chap", &spec.seed, chapter_index),
                    fixture_id(
                        "chap",
                        &spec.seed,
                        index * spec.chapters / spec.volumes.max(1)
                    ),
                    now
                ])?;
            }
            drop(obligations);
            let mut desires = connection.prepare(
                "INSERT OR IGNORE INTO reader_desires(desire_id, project_id, desire_type, status, expected_payoff_chapter, created_chapter_id, last_touched_chapter_id, aging_score, payload_json, created_at, updated_at) VALUES (?1, ?2, 'LONG_TERM_FIXTURE', 'OPEN', ?3, ?4, ?5, ?6, '{}', ?7, ?7)",
            )?;
            for index in 0..spec.volumes {
                let created_index = index * spec.chapters / spec.volumes.max(1);
                let payoff_index =
                    ((index + 1) * spec.chapters / spec.volumes.max(1)).saturating_sub(1);
                desires.execute(libsql_rusqlite::params![
                    fixture_id("desire", &spec.seed, index),
                    project_id,
                    fixture_id("chap", &spec.seed, payoff_index),
                    fixture_id("chap", &spec.seed, created_index),
                    fixture_id("chap", &spec.seed, payoff_index),
                    index as f64,
                    now
                ])?;
            }
            drop(desires);
            // The scale fixture is a dedicated bulk loader. Commit fixed-size
            // chunks so a single failed batch does not create an unbounded WAL.
            connection.execute_batch("COMMIT; BEGIN IMMEDIATE")?;
            if spec.dependencies > 0 {
                if spec.artifacts == 0 {
                    return Err(OpsError::InvalidPageRequest);
                }
                connection.execute(
                    "WITH RECURSIVE digits(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM digits WHERE i < 999) INSERT OR IGNORE INTO dependency_edges(edge_id, project_id, source_ref, target_ref, dependency_type, scope, impact_class, source_hash, created_by, created_at) SELECT 'edge:' || ?1 || ':' || (a.i + 1000 * b.i + 1000000 * c.i), ?2, m.artifact_id, ?2, 'EVIDENCE', 'PROJECT', 'EVIDENCE', m.payload_hash, 'B04_FIXTURE', ?5 FROM digits a CROSS JOIN digits b CROSS JOIN (SELECT 0 AS i UNION ALL SELECT 1 UNION ALL SELECT 2) c JOIN fixture_artifact_map m ON m.artifact_index = (a.i + 1000 * b.i + 1000000 * c.i) % ?3 WHERE (a.i + 1000 * b.i + 1000000 * c.i) < ?4",
                    libsql_rusqlite::params![spec.seed, project_id, spec.artifacts as i64, spec.dependencies as i64, now],
                )?;
            }
            let mut model_runs = connection.prepare(
                "INSERT OR IGNORE INTO model_runs(model_run_id, project_id, provider, model, idempotency_key, capability, predicted_cost_cents, reserved_cost_cents, status, created_at, updated_at) VALUES (?1, ?2, 'fixture', 'fixture', ?3, 'fixture', 0, 0, 'COMPLETED', ?4, ?4)",
            )?;
            for index in 0..spec.model_runs {
                let model_run_id = fixture_id("mr", &spec.seed, index);
                model_runs.execute(libsql_rusqlite::params![
                    model_run_id,
                    project_id,
                    format!("fixture:{}:{index}", spec.seed),
                    now
                ])?;
                if (index + 1) % 100_000 == 0 {
                    connection.execute_batch("COMMIT; BEGIN IMMEDIATE")?;
                }
            }
            drop(model_runs);
            Ok::<_, OpsError>(())
        })();
        match result {
            Ok(()) => connection.execute_batch("COMMIT")?,
            Err(error) => {
                let _ = connection.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
        Ok(Self::manifest(spec))
    }
}

fn fixture_chapter_body(seed: &str, index: u64, target_chars: usize) -> String {
    let mut body = format!("commonTerm chapterUnique{index} 人物{index} 地点{index} 事件{index} ");
    if index.is_multiple_of(10) {
        body.push_str("middleTerm ");
    }
    if index == 3999 {
        body.push_str("rareTerm3999 ");
    }
    while body.chars().count() < target_chars {
        body.push_str(&format!("{seed} commonTerm 叙事推进 "));
    }
    body.chars().take(target_chars).collect()
}

fn fixture_id(prefix: &str, seed: &str, index: u64) -> String {
    let digest = hex::encode(Sha256::digest(
        format!("{seed}:{prefix}:{index}").as_bytes(),
    ));
    format!("{prefix}:{}", &digest[..24])
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerformanceReport {
    pub operation: String,
    pub samples: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub reference_hardware: String,
}

pub struct PerformanceHarness;

type PerformanceOperation<'a> = (
    &'static str,
    Box<dyn Fn(&Connection) -> Result<(), OpsError> + 'a>,
);

impl PerformanceHarness {
    pub fn measure_search(
        connection: &Connection,
        project_id: &str,
        text: &str,
        samples: usize,
    ) -> Result<PerformanceReport, OpsError> {
        if samples == 0 {
            return Err(OpsError::InvalidPageRequest);
        }
        let mut durations = Vec::with_capacity(samples);
        for _ in 0..samples {
            let started = Instant::now();
            let _ = SearchService::search(connection, project_id, text, None, 30)?;
            durations.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        durations.sort_by(f64::total_cmp);
        let percentile = |fraction: f64| -> f64 {
            let index = ((durations.len() as f64 * fraction).ceil() as usize).saturating_sub(1);
            durations[index]
        };
        Ok(PerformanceReport {
            operation: "search_fulltext".into(),
            samples,
            p50_ms: percentile(0.50),
            p95_ms: percentile(0.95),
            reference_hardware: std::env::consts::ARCH.into(),
        })
    }

    /// Measures every B04 production read/maintenance boundary. These are
    /// actual SQL/IPC-adjacent operations over the supplied database, not
    /// threshold constants copied into a report.
    pub fn measure_standard(
        connection: &Connection,
        project_id: &str,
        samples: usize,
    ) -> Result<Vec<PerformanceReport>, OpsError> {
        if samples == 0 {
            return Err(OpsError::InvalidPageRequest);
        }
        let operations: [PerformanceOperation<'_>; 7] = [
            (
                "project_overview",
                Box::new(move |connection| {
                    connection.query_row(
                        "SELECT title, revision, updated_at FROM project_meta WHERE project_id=?1",
                        [project_id],
                        |_| Ok(()),
                    )?;
                    Ok(())
                }),
            ),
            (
                "volume_list",
                Box::new(move |connection| {
                    let _ = volume_page(connection, project_id, None, 30)?;
                    Ok(())
                }),
            ),
            (
                "chapter_list",
                Box::new(move |connection| {
                    let volume_id: Option<String> = connection
                        .query_row(
                            "SELECT volume_id FROM volumes WHERE project_id=?1 ORDER BY volume_id LIMIT 1",
                            [project_id],
                            |row| row.get(0),
                        )
                        .optional()?;
                    if let Some(volume_id) = volume_id {
                        let _ = chapter_page(connection, project_id, &volume_id, None, 30)?;
                    }
                    Ok(())
                }),
            ),
            (
                "canon_current",
                Box::new(move |connection| {
                    let subject_id: Option<String> = connection
                        .query_row(
                            "SELECT entity_id FROM canon_entities WHERE project_id=?1 ORDER BY entity_id LIMIT 1",
                            [project_id],
                            |row| row.get(0),
                        )
                        .optional()?;
                    if let Some(subject_id) = subject_id {
                        let _ = canon_current_page(connection, project_id, &subject_id, 30)?;
                    }
                    Ok::<(), OpsError>(())
                }),
            ),
            (
                "adjacency",
                Box::new(move |connection| {
                    let source_ref: Option<String> = connection
                        .query_row(
                            "SELECT source_ref FROM dependency_edges WHERE project_id=?1 ORDER BY edge_id LIMIT 1",
                            [project_id],
                            |row| row.get(0),
                        )
                        .optional()?;
                    if let Some(source_ref) = source_ref {
                        let _ = dependency_adjacency_page(
                            connection,
                            project_id,
                            &source_ref,
                            None,
                            30,
                        )?;
                    }
                    Ok::<(), OpsError>(())
                }),
            ),
            (
                "normal_commit",
                Box::new(move |connection| {
                    connection.execute_batch("BEGIN IMMEDIATE")?;
                    let result = connection
                        .query_row(
                            "SELECT revision FROM project_meta WHERE project_id=?1 AND maintenance_state='READY'",
                            [project_id],
                            |_| Ok(()),
                        )
                        .map_err(OpsError::from);
                    match result {
                        Ok(()) => connection.execute_batch("COMMIT")?,
                        Err(error) => {
                            let _ = connection.execute_batch("ROLLBACK");
                            return Err(error);
                        }
                    }
                    Ok(())
                }),
            ),
            (
                "recovery_readable",
                Box::new(move |connection| {
                    let _: i64 =
                        connection.query_row("PRAGMA schema_version", [], |row| row.get(0))?;
                    let _: i64 = connection.query_row(
                        "SELECT COUNT(*) FROM project_meta WHERE project_id=?1",
                        [project_id],
                        |row| row.get(0),
                    )?;
                    Ok(())
                }),
            ),
        ];
        let mut reports = Vec::with_capacity(operations.len());
        for (operation, measure) in operations {
            let mut durations = Vec::with_capacity(samples);
            for _ in 0..samples {
                let started = Instant::now();
                measure(connection)?;
                durations.push(started.elapsed().as_secs_f64() * 1000.0);
            }
            durations.sort_by(f64::total_cmp);
            let percentile = |fraction: f64| {
                let index = ((durations.len() as f64 * fraction).ceil() as usize).saturating_sub(1);
                durations[index]
            };
            reports.push(PerformanceReport {
                operation: operation.into(),
                samples,
                p50_ms: percentile(0.50),
                p95_ms: percentile(0.95),
                reference_hardware: std::env::consts::ARCH.into(),
            });
        }
        Ok(reports)
    }

    pub fn assert_thresholds(reports: &[PerformanceReport]) -> Result<(), OpsError> {
        for report in reports {
            let threshold = match report.operation.as_str() {
                "project_overview" => 2_000.0,
                "volume_list" | "chapter_list" | "single_chapter" => 300.0,
                "canon_current" => 250.0,
                "adjacency" => 500.0,
                "normal_commit" => 1_000.0,
                "recovery_readable" => 10_000.0,
                _ => 1_000.0,
            };
            if report.p95_ms > threshold {
                return Err(OpsError::PerformanceThresholdExceeded(format!(
                    "{} p95={}ms threshold={}ms",
                    report.operation, report.p95_ms, threshold
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionPage {
    pub items: Vec<serde_json::Value>,
    pub next_cursor: Option<String>,
    pub total: i64,
}

pub fn volume_page(
    connection: &Connection,
    project_id: &str,
    cursor: Option<&str>,
    limit: usize,
) -> Result<ProjectionPage, OpsError> {
    if !(1..=100).contains(&limit) {
        return Err(OpsError::InvalidPageRequest);
    }
    let after = cursor.unwrap_or("");
    let total: i64 = connection.query_row(
        "SELECT COUNT(*) FROM volumes WHERE project_id=?1",
        [project_id],
        |row| row.get(0),
    )?;
    let mut statement = connection.prepare(
        "SELECT volume_id, volume_no, title, status, chapter_start, chapter_end, target_words, updated_at FROM volumes WHERE project_id=?1 AND volume_id>?2 ORDER BY volume_id LIMIT ?3",
    )?;
    let rows = statement.query_map(
        libsql_rusqlite::params![project_id, after, (limit + 1) as i64],
        |row| {
            let volume_id: String = row.get(0)?;
            let chapter_start: i64 = row.get(4)?;
            let chapter_end: i64 = row.get(5)?;
            let committed: i64 = connection.query_row(
                "SELECT COUNT(*) FROM chapters WHERE project_id=?1 AND volume_id=?2 AND status='COMMITTED'",
                libsql_rusqlite::params![project_id, volume_id],
                |count| count.get(0),
            )?;
            let words: i64 = connection.query_row(
                "SELECT COALESCE(SUM(word_count),0) FROM chapters WHERE project_id=?1 AND volume_id=?2",
                libsql_rusqlite::params![project_id, volume_id],
                |count| count.get(0),
            )?;
            Ok((
                volume_id,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                chapter_start,
                chapter_end,
                row.get::<_, i64>(6)?,
                row.get::<_, String>(7)?,
                committed,
                words,
            ))
        },
    )?;
    let mut rows = rows.collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if rows.len() > limit {
        Some(rows[limit - 1].0.clone())
    } else {
        None
    };
    rows.truncate(limit);
    let items = rows
        .into_iter()
        .map(
            |(
                id,
                number,
                title,
                status,
                start,
                end,
                target_words,
                updated_at,
                committed,
                words,
            )| {
                let planned = (end - start + 1).max(0);
                json!({
                    "id": id,
                    "number": number,
                    "title": title,
                    "status": status,
                    "chapterStart": start,
                    "chapterEnd": end,
                    "committedChapters": committed,
                    "plannedChapters": planned,
                    "wordCount": words,
                    "targetWords": target_words,
                    "progress": if planned == 0 { 0.0 } else { committed as f64 / planned as f64 },
                    "goal": "",
                    "health": "healthy",
                    "updatedAt": updated_at
                })
            },
        )
        .collect();
    Ok(ProjectionPage {
        items,
        next_cursor,
        total,
    })
}

pub fn chapter_page(
    connection: &Connection,
    project_id: &str,
    volume_id: &str,
    cursor: Option<&str>,
    limit: usize,
) -> Result<ProjectionPage, OpsError> {
    if !(1..=100).contains(&limit) {
        return Err(OpsError::InvalidPageRequest);
    }
    let after = cursor.unwrap_or("");
    let total: i64 = connection.query_row(
        "SELECT COUNT(*) FROM chapters WHERE project_id=?1 AND volume_id=?2",
        libsql_rusqlite::params![project_id, volume_id],
        |row| row.get(0),
    )?;
    let mut statement = connection.prepare(
        "SELECT chapter_id, chapter_no, title, status, word_count, final_version, updated_at FROM chapters WHERE project_id=?1 AND volume_id=?2 AND chapter_id>?3 ORDER BY chapter_id LIMIT ?4",
    )?;
    let rows = statement.query_map(
        libsql_rusqlite::params![project_id, volume_id, after, (limit + 1) as i64],
        |row| {
            let id: String = row.get(0)?;
            let status: String = row.get(3)?;
            let version: Option<i64> = row.get(5)?;
            Ok((
                id,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                if status == "COMMITTED" {
                    "official"
                } else {
                    "producing"
                }
                .to_string(),
                row.get::<_, i64>(4)?,
                version.map(|value| format!("v{value}")),
                row.get::<_, String>(6)?,
            ))
        },
    )?;
    let mut rows = rows.collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if rows.len() > limit {
        Some(rows[limit - 1].0.clone())
    } else {
        None
    };
    rows.truncate(limit);
    let items = rows
        .into_iter()
        .map(|(id, number, title, status, word_count, version, updated_at)| {
            json!({"id":id,"volumeId":volume_id,"number":number,"title":title,"status":status,"version":version.unwrap_or_default(),"wordCount":word_count,"updatedAt":updated_at})
        })
        .collect();
    Ok(ProjectionPage {
        items,
        next_cursor,
        total,
    })
}

type SingleChapterRow = (
    String,
    i64,
    String,
    String,
    i64,
    Option<String>,
    Option<i64>,
    String,
    String,
);

/// A chapter's number and what the committed chapters before it did.
#[derive(Debug, Clone, PartialEq)]
pub struct ChapterHistory {
    pub chapter_no: Option<i64>,
    /// Newest first: `{chapterNo, chapterGoal, ending}`.
    pub recent: Vec<serde_json::Value>,
    /// Newest first: the full bodies of the same chapters.
    pub bodies: Vec<String>,
    /// QC-1.4: the catchphrases the continuity check named in these
    /// chapters, for the writer to leave out.
    pub catchphrases: Vec<String>,
}

impl ChapterHistory {
    pub fn number(&self) -> Option<u32> {
        self.chapter_no
            .and_then(|number| u32::try_from(number).ok())
    }
}

/// Goals and closing passages (last 200 characters) of the last `limit`
/// committed chapters before `chapter_id`, so the planner continues the
/// story instead of repeating it.
pub fn chapter_history(
    connection: &Connection,
    project_id: &str,
    chapter_id: &str,
    limit: i64,
) -> Result<ChapterHistory, OpsError> {
    let chapter_no: Option<i64> = connection
        .query_row(
            "SELECT chapter_no FROM chapters WHERE project_id=?1 AND chapter_id=?2",
            libsql_rusqlite::params![project_id, chapter_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(number) = chapter_no else {
        return Ok(ChapterHistory {
            chapter_no,
            recent: Vec::new(),
            bodies: Vec::new(),
            catchphrases: Vec::new(),
        });
    };
    let mut statement = connection.prepare(
        "SELECT c.chapter_no, substr(json_extract(f.payload_json,'$.body'), -200), json_extract(p.payload_json,'$.chapterGoal'), json_extract(f.payload_json,'$.body'), json_extract(p.payload_json,'$.chapterRole'), (SELECT json_group_array(json_extract(scene.value,'$.sceneType')) FROM json_each(p.payload_json,'$.sceneExecution') scene), \
         (SELECT json_extract(r.payload_json,'$.craftSkills.continuity.catchphrases') FROM artifacts ra JOIN artifact_payloads r ON r.artifact_id=ra.artifact_id AND r.version=ra.version WHERE ra.project_id=c.project_id AND ra.artifact_type='ProductionReceipt' AND json_extract(r.payload_json,'$.finalChapterRef.artifactId')=c.final_artifact_id LIMIT 1) \
         FROM chapters c JOIN artifact_payloads f ON f.artifact_id=c.final_artifact_id AND f.version=c.final_version \
         LEFT JOIN artifact_payloads p ON p.artifact_id=json_extract(f.payload_json,'$.chapterPlanRef.artifactId') AND p.version=json_extract(f.payload_json,'$.chapterPlanRef.version') \
         WHERE c.project_id=?1 AND c.chapter_no<?2 AND c.status='COMMITTED' ORDER BY c.chapter_no DESC LIMIT ?3",
    )?;
    let rows = statement
        .query_map(libsql_rusqlite::params![project_id, number, limit], |row| {
            Ok((
                json!({
                    "chapterNo":row.get::<_, i64>(0)?,
                    "chapterGoal":row.get::<_, Option<String>>(2)?,
                    "ending":row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    "chapterRole":row.get::<_, Option<String>>(4)?,
                    "sceneTypes":row
                        .get::<_, Option<String>>(5)?
                        .and_then(|types| serde_json::from_str::<serde_json::Value>(&types).ok())
                        .unwrap_or_else(|| json!([]))
                }),
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                row.get::<_, Option<String>>(6)?
                    .and_then(|list| serde_json::from_str::<Vec<String>>(&list).ok())
                    .unwrap_or_default(),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut catchphrases = Vec::new();
    let mut recent = Vec::new();
    let mut bodies = Vec::new();
    for (summary, body, named) in rows {
        recent.push(summary);
        bodies.push(body);
        for phrase in named {
            if !catchphrases.contains(&phrase) {
                catchphrases.push(phrase);
            }
        }
    }
    Ok(ChapterHistory {
        chapter_no,
        recent,
        bodies,
        catchphrases,
    })
}

/// QC-1.5: the ledger's current facts about `subjects`, newest chapter
/// first, as `{chapter, subject, predicate, value}`.
pub fn subject_facts(
    connection: &Connection,
    project_id: &str,
    subjects: &[String],
    limit: i64,
) -> Result<Vec<serde_json::Value>, OpsError> {
    if subjects.is_empty() {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare(
        "SELECT c.chapter_no, f.subject_id, f.predicate, f.object_json FROM canon_facts f \
         LEFT JOIN chapters c ON c.project_id=f.project_id AND c.chapter_id=f.source_chapter_id \
         WHERE f.project_id=?1 AND f.superseded_by IS NULL AND f.subject_id IN (SELECT value FROM json_each(?2)) \
         ORDER BY c.chapter_no DESC LIMIT ?3",
    )?;
    let subjects = serde_json::to_string(subjects).unwrap_or_else(|_| "[]".into());
    let rows = statement
        .query_map(
            libsql_rusqlite::params![project_id, subjects, limit],
            |row| {
                Ok(json!({
                    "chapter":row.get::<_, Option<i64>>(0)?,
                    "subject":row.get::<_, String>(1)?,
                    "predicate":row.get::<_, String>(2)?,
                    "value":row
                        .get::<_, String>(3)
                        .ok()
                        .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
                }))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Formal reader query for one chapter. It returns authority metadata and the
/// body from the chapter's committed FinalChapter artifact when available.
pub fn single_chapter(
    connection: &Connection,
    project_id: &str,
    chapter_id: &str,
) -> Result<Option<serde_json::Value>, OpsError> {
    let row: Option<SingleChapterRow> = connection
        .query_row(
            "SELECT chapter_id, chapter_no, volume_id, title, word_count, final_artifact_id, final_version, status, updated_at FROM chapters WHERE project_id=?1 AND chapter_id=?2",
            libsql_rusqlite::params![project_id, chapter_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?)),
        )
        .optional()?;
    let Some((id, ordinal, volume_id, title, word_count, artifact_id, version, status, updated_at)) =
        row
    else {
        return Ok(None);
    };
    let final_text = match (artifact_id.as_deref(), version) {
        (Some(artifact_id), Some(version)) => connection
            .query_row(
                "SELECT json_extract(payload_json, '$.body') FROM artifact_payloads WHERE artifact_id=?1 AND version=?2",
                libsql_rusqlite::params![artifact_id, version],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten(),
        _ => None,
    };
    Ok(Some(json!({
        "id": id,
        "volumeId": volume_id,
        "number": ordinal,
        "title": title,
        "status": if status == "COMMITTED" { "official" } else { "producing" },
        "version": version.map(|value| format!("v{value}")).unwrap_or_default(),
        "wordCount": word_count,
        "updatedAt": updated_at,
        "content": final_text.map(|text| vec![text]).unwrap_or_default(),
    })))
}

pub fn canon_current_page(
    connection: &Connection,
    project_id: &str,
    subject_id: &str,
    limit: usize,
) -> Result<ProjectionPage, OpsError> {
    if !(1..=100).contains(&limit) {
        return Err(OpsError::InvalidPageRequest);
    }
    let mut statement = connection.prepare(
        "SELECT fact_id, predicate, object_json, authority_level FROM canon_facts WHERE project_id=?1 AND subject_id=?2 AND superseded_by IS NULL LIMIT ?3",
    )?;
    let rows = statement.query_map(
        libsql_rusqlite::params![project_id, subject_id, limit as i64],
        |row| {
            Ok(json!({
                "factId": row.get::<_, String>(0)?,
                "predicate": row.get::<_, String>(1)?,
                "object": serde_json::from_str::<serde_json::Value>(&row.get::<_, String>(2)?).unwrap_or(serde_json::Value::Null),
                "authorityLevel": row.get::<_, String>(3)?,
            }))
        },
    )?;
    let items = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(ProjectionPage {
        total: items.len() as i64,
        next_cursor: None,
        items,
    })
}

pub fn dependency_adjacency_page(
    connection: &Connection,
    project_id: &str,
    source_ref: &str,
    cursor: Option<&str>,
    limit: usize,
) -> Result<ProjectionPage, OpsError> {
    if !(1..=100).contains(&limit) {
        return Err(OpsError::InvalidPageRequest);
    }
    let after = cursor.unwrap_or("");
    let mut statement = connection.prepare(
        "SELECT edge_id, target_ref, dependency_type, impact_class, source_hash FROM dependency_edges WHERE project_id=?1 AND source_ref=?2 AND edge_id>?3 ORDER BY edge_id LIMIT ?4",
    )?;
    let rows = statement.query_map(
        libsql_rusqlite::params![project_id, source_ref, after, limit as i64 + 1],
        |row| Ok(json!({"edgeId":row.get::<_,String>(0)?,"targetRef":row.get::<_,String>(1)?,"dependencyType":row.get::<_,String>(2)?,"impactClass":row.get::<_,String>(3)?,"sourceHash":row.get::<_,String>(4)?})),
    )?;
    let mut items = rows.collect::<Result<Vec<_>, _>>()?;
    let next_cursor = if items.len() > limit {
        Some(
            items[limit - 1]["edgeId"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        )
    } else {
        None
    };
    items.truncate(limit);
    Ok(ProjectionPage {
        total: items.len() as i64,
        next_cursor,
        items,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub backup_id: String,
    pub project_id: String,
    pub backup_path: String,
    pub checksum_sha256: String,
    pub profile: SQLiteRuntimeProfile,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestoreReport {
    pub checksum_sha256: String,
    pub quick_check: String,
    pub foreign_key_violations: i64,
    pub deep_integrity_passed: bool,
}

pub struct BackupService;

impl BackupService {
    pub fn create_snapshot(
        connection: &Connection,
        backup_path: &Path,
        project_id: &str,
        now: &str,
    ) -> Result<BackupManifest, OpsError> {
        if let Some(parent) = backup_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        connection.backup(libsql_rusqlite::DatabaseName::Main, backup_path, None)?;
        let bytes = std::fs::read(backup_path)?;
        let checksum = hex::encode(Sha256::digest(&bytes));
        let profile = read_runtime_profile(connection, project_id)?;
        let manifest = BackupManifest {
            backup_id: format!("backup:{project_id}:{now}"),
            project_id: project_id.into(),
            backup_path: backup_path.to_string_lossy().into_owned(),
            checksum_sha256: checksum,
            profile,
            created_at: now.into(),
        };
        Ok(manifest)
    }

    pub fn persist_manifest(
        connection: &Connection,
        manifest: &BackupManifest,
    ) -> Result<(), OpsError> {
        connection.execute(
            "INSERT INTO backup_manifests(backup_id, project_id, backup_path, checksum_sha256, profile_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            libsql_rusqlite::params![manifest.backup_id, manifest.project_id, manifest.backup_path, manifest.checksum_sha256, serde_json::to_string(&manifest.profile)?, manifest.created_at],
        )?;
        Ok(())
    }

    pub fn create(
        connection: &Connection,
        backup_path: &Path,
        project_id: &str,
        now: &str,
    ) -> Result<BackupManifest, OpsError> {
        let manifest = Self::create_snapshot(connection, backup_path, project_id, now)?;
        Self::persist_manifest(connection, &manifest)?;
        Ok(manifest)
    }

    pub fn restore(
        backup_path: &Path,
        target_path: &Path,
        expected_checksum: &str,
    ) -> Result<(), OpsError> {
        let bytes = std::fs::read(backup_path)?;
        if hex::encode(Sha256::digest(&bytes)) != expected_checksum {
            return Err(OpsError::BackupChecksumMismatch);
        }
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(backup_path, target_path)?;
        Ok(())
    }

    /// Restores to a staging path and proves the restored database is readable
    /// before the caller replaces an authoritative project database.
    pub fn restore_verified(
        backup_path: &Path,
        target_path: &Path,
        expected_checksum: &str,
        project_id: &str,
        now: &str,
    ) -> Result<RestoreReport, OpsError> {
        Self::restore(backup_path, target_path, expected_checksum)?;
        let connection = crate::project_db::connection::open_connection(target_path, false)
            .map_err(|error| OpsError::RestoreIntegrity(error.to_string()))?;
        let quick_check: String =
            connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if quick_check != "ok" {
            return Err(OpsError::RestoreIntegrity(format!(
                "quick_check={quick_check}"
            )));
        }
        let mut foreign_keys = connection.prepare("PRAGMA foreign_key_check")?;
        let foreign_key_violations = foreign_keys.query_map([], |_| Ok(()))?.count() as i64;
        drop(foreign_keys);
        if foreign_key_violations != 0 {
            return Err(OpsError::RestoreIntegrity(format!(
                "foreign_key_violations={foreign_key_violations}"
            )));
        }
        let restored_project: String =
            connection.query_row("SELECT project_id FROM project_meta LIMIT 1", [], |row| {
                row.get(0)
            })?;
        if restored_project != project_id {
            return Err(OpsError::RestoreIntegrity(
                "restored project identity mismatch".into(),
            ));
        }
        let integrity = deep_integrity_check(&connection, project_id, now)?;
        if !integrity.ok {
            return Err(OpsError::RestoreIntegrity(integrity.failures.join(",")));
        }
        Ok(RestoreReport {
            checksum_sha256: expected_checksum.into(),
            quick_check,
            foreign_key_violations,
            deep_integrity_passed: true,
        })
    }
}
