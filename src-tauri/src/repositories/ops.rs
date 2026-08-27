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
