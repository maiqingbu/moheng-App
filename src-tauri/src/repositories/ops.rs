use crate::commit::CommitService;
use libsql_rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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

impl SearchService {
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
            let mut count = 0_i64;
            let mut chapters = connection.prepare(
                "SELECT chapter_id, volume_id, title FROM chapters WHERE project_id=?1 ORDER BY chapter_no, chapter_id",
            )?;
            let rows = chapters.query_map([project_id], |row| {
                Ok(SearchDocument {
                    project_id: project_id.to_string(),
                    doc_id: format!("chapter:{}", row.get::<_, String>(0)?),
                    chapter_id: Some(row.get(0)?),
                    volume_id: Some(row.get(1)?),
                    title: row.get(2)?,
                    body: row.get(2)?,
                    kind: "CHAPTER".into(),
                })
            })?;
            for document in rows {
                upsert_search_document(connection, &document?, now)?;
                count += 1;
            }
            let mut artifacts = connection.prepare(
                "SELECT artifacts.artifact_id, artifacts.version, artifacts.scope_key, artifact_payloads.payload_json FROM artifacts LEFT JOIN artifact_payloads ON artifact_payloads.artifact_id=artifacts.artifact_id AND artifact_payloads.version=artifacts.version WHERE artifacts.project_id=?1 ORDER BY artifacts.artifact_id, artifacts.version",
            )?;
            let rows = artifacts.query_map([project_id], |row| {
                let artifact_id: String = row.get(0)?;
                let version: i64 = row.get(1)?;
                let scope_key: String = row.get(2)?;
                let body: Option<String> = row.get(3)?;
                Ok(SearchDocument {
                    project_id: project_id.to_string(),
                    doc_id: format!("artifact:{artifact_id}:{version}"),
                    chapter_id: Some(scope_key),
                    volume_id: None,
                    title: artifact_id,
                    body: body.unwrap_or_default(),
                    kind: "EVIDENCE".into(),
                })
            })?;
            for document in rows {
                upsert_search_document(connection, &document?, now)?;
                count += 1;
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
        checkpoint_policy: "PASSIVE_WITH_MILESTONE_CHECKPOINT".into(),
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
    let mut dependencies = connection
        .prepare("SELECT source_ref, source_hash FROM dependency_edges WHERE project_id=?1")?;
    let dependency_rows = dependencies.query_map([project_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for dependency in dependency_rows {
        let (source_ref, source_hash) = dependency?;
        let artifact_hash: Option<String> = connection
            .query_row(
                "SELECT payload_hash FROM artifacts WHERE project_id=?1 AND artifact_id=?2 ORDER BY version DESC LIMIT 1",
                libsql_rusqlite::params![project_id, source_ref],
                |row| row.get(0),
            )
            .optional()?;
        if artifact_hash.is_some_and(|hash| hash != source_hash) {
            failures.push("DEPENDENCY_SOURCE_HASH_MISMATCH".into());
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
    if mode == "INCREMENTAL" {
        connection.execute_batch("PRAGMA incremental_vacuum")?;
    } else if mode == "FULL" {
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
    };
    connection.execute(
        "INSERT INTO vacuum_reports(report_id, project_id, freelist_before, freelist_after, page_count, mode, maintenance_required, created_at, report_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        libsql_rusqlite::params![report.report_id, report.project_id, report.freelist_before, report.freelist_after, report.page_count, report.mode, report.maintenance_required, now, serde_json::to_string(&report)?],
    )?;
    Ok(report)
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
            for index in 0..spec.books {
                let book_id = fixture_id("bk", &spec.seed, index);
                connection.execute(
                    "INSERT OR IGNORE INTO books(book_id, project_id, book_no, title, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'PLANNING', ?5, ?5)",
                    libsql_rusqlite::params![book_id, project_id, index + 1, format!("B04 Book {index}"), now],
                )?;
            }
            for index in 0..spec.volumes {
                let volume_id = fixture_id("vol", &spec.seed, index);
                let book_index = index % spec.books;
                let book_id = fixture_id("bk", &spec.seed, book_index);
                connection.execute(
                    "INSERT OR IGNORE INTO volumes(volume_id, project_id, book_id, volume_no, title, status, chapter_start, chapter_end, target_words, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 'PLANNING', 0, 0, ?6, ?7, ?7)",
                    libsql_rusqlite::params![volume_id, project_id, book_id, index / spec.books + 1, format!("B04 Volume {index}"), spec.target_words / spec.volumes.max(1), now],
                )?;
            }
            for index in 0..spec.chapters {
                let chapter_id = fixture_id("chap", &spec.seed, index);
                let volume_index = index % spec.volumes.max(1);
                let volume_id = fixture_id("vol", &spec.seed, volume_index);
                connection.execute(
                    "INSERT OR IGNORE INTO chapters(chapter_id, project_id, volume_id, chapter_no, title, importance_class, status, word_count, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 'NORMAL', 'PLANNED', ?6, ?7, ?7)",
                    libsql_rusqlite::params![chapter_id, project_id, volume_id, index + 1, format!("B04 Chapter {index}"), spec.target_words / spec.chapters.max(1), now],
                )?;
            }
            for index in 0..spec.artifacts {
                let artifact_id = fixture_id("art", &spec.seed, index);
                let payload =
                    serde_json::json!({"seed":spec.seed,"artifactIndex":index}).to_string();
                let payload_hash = hex::encode(Sha256::digest(payload.as_bytes()));
                connection.execute(
                    "INSERT OR IGNORE INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, payload_hash, provenance_json, created_at, committed_at) VALUES (?1, ?2, 'FixtureEvidence', 'PROJECT', ?3, 1, 'm0e.fixture.v1', 'CANDIDATE', ?4, '{}', ?5, NULL)",
                    libsql_rusqlite::params![artifact_id, project_id, project_id, payload_hash, now],
                )?;
                connection.execute(
                    "INSERT OR IGNORE INTO artifact_payloads(artifact_id, version, payload_json, payload_hash) VALUES (?1, 1, ?2, ?3)",
                    libsql_rusqlite::params![artifact_id, payload, payload_hash],
                )?;
            }
            if spec.canon_facts > 0 {
                let entity_id = fixture_id("ent", &spec.seed, 0);
                connection.execute(
                    "INSERT OR IGNORE INTO canon_entities(entity_id, project_id, canonical_name, entity_type, created_at, updated_at) VALUES (?1, ?2, 'B04 Fixture Entity', 'FIXTURE', ?3, ?3)",
                    libsql_rusqlite::params![entity_id, project_id, now],
                )?;
                for index in 0..spec.canon_facts {
                    let fact_id = fixture_id("fact", &spec.seed, index);
                    connection.execute(
                        "INSERT OR IGNORE INTO canon_facts(fact_id, project_id, subject_id, predicate, object_json, authority_level, created_at, updated_at) VALUES (?1, ?2, ?3, 'fixtureValue', ?4, 'C', ?5, ?5)",
                        libsql_rusqlite::params![fact_id, project_id, entity_id, serde_json::json!({"seed":spec.seed,"factIndex":index}).to_string(), now],
                    )?;
                }
            }
            for index in 0..spec.dependencies {
                let edge_id = fixture_id("edge", &spec.seed, index);
                let source_ref = if spec.artifacts > 0 {
                    fixture_id("art", &spec.seed, index % spec.artifacts)
                } else {
                    project_id.clone()
                };
                connection.execute(
                    "INSERT OR IGNORE INTO dependency_edges(edge_id, project_id, source_ref, target_ref, dependency_type, scope, impact_class, source_hash, created_by, created_at) VALUES (?1, ?2, ?3, ?4, 'EVIDENCE', 'PROJECT', 'EVIDENCE', ?5, 'B04_FIXTURE', ?6)",
                    libsql_rusqlite::params![edge_id, project_id, source_ref, project_id, hex::encode(Sha256::digest(format!("{}:{index}", spec.seed).as_bytes())), now],
                )?;
            }
            for index in 0..spec.model_runs {
                let model_run_id = fixture_id("mr", &spec.seed, index);
                connection.execute(
                    "INSERT OR IGNORE INTO model_runs(model_run_id, project_id, provider, model, idempotency_key, capability, predicted_cost_cents, reserved_cost_cents, status, created_at, updated_at) VALUES (?1, ?2, 'fixture', 'fixture', ?3, 'fixture', 0, 0, 'COMPLETED', ?4, ?4)",
                    libsql_rusqlite::params![model_run_id, project_id, format!("fixture:{}:{index}", spec.seed), now],
                )?;
            }
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
}
