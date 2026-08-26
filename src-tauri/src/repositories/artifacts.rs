use crate::generated::types::{ArtifactEnvelope, ArtifactRef, ArtifactStatus};
use crate::generated::validators::validate_artifact_envelope;
use crate::hash::{canonical_bytes, sha256_canonical};
use libsql_rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Map, Value};

#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    #[error("project mismatch")]
    ProjectMismatch,
    #[error("hash mismatch")]
    HashMismatch,
    #[error("artifact version must be monotonic")]
    VersionConflict,
    #[error("candidate cannot become canonical head")]
    CandidateHead,
    #[error("artifact not found")]
    NotFound,
    #[error("artifact metadata mismatch")]
    MetadataMismatch,
    #[error("schema invalid: {0}")]
    Schema(String),
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GcReceipt {
    pub receipt_id: String,
    pub project_id: String,
    pub policy_digest: String,
    pub deleted_candidate_count: i64,
    pub created_at: String,
    pub receipt_digest: String,
}

pub struct ArtifactRepository;
impl ArtifactRepository {
    pub fn insert_candidate(
        connection: &Connection,
        project_id: &str,
        artifact: &ArtifactEnvelope,
    ) -> Result<(), ArtifactError> {
        let value = serde_json::to_value(artifact)?;
        validate_artifact_envelope(&value).map_err(ArtifactError::Schema)?;
        if artifact.project_id != project_id {
            return Err(ArtifactError::ProjectMismatch);
        }
        if sha256_canonical(&artifact.payload)? != artifact.payload_hash {
            return Err(ArtifactError::HashMismatch);
        }
        let latest: Option<i64> = connection
            .query_row(
                "SELECT MAX(version) FROM artifacts WHERE artifact_id = ?1",
                [&artifact.artifact_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        if artifact.version != latest.map_or(1, |version| version + 1) {
            return Err(ArtifactError::VersionConflict);
        }

        connection.execute_batch("SAVEPOINT artifact_insert")?;
        let result = (|| {
            let payload = serde_json::to_string(&artifact.payload)?;
            let provenance = serde_json::to_string(&artifact.provenance)?;
            let artifact_type = enum_string(&artifact.artifact_type)?;
            let scope_level = enum_string(&artifact.scope.level)?;
            let semantic_lifecycle = artifact
                .semantic_lifecycle
                .as_ref()
                .map(enum_string)
                .transpose()?;
            let authority_level = artifact
                .authority_level
                .as_ref()
                .map(enum_string)
                .transpose()?;
            let scope_key = scope_key(&artifact.scope)?;
            connection.execute("INSERT INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, semantic_lifecycle, authority_level, payload_hash, provenance_json, created_at, committed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)", libsql_rusqlite::params![artifact.artifact_id, artifact.project_id, artifact_type, scope_level, scope_key, artifact.version, artifact.schema_version, status_name(&artifact.artifact_status), semantic_lifecycle, authority_level, artifact.payload_hash, provenance, artifact.created_at, artifact.committed_at])?;
            connection.execute("INSERT INTO artifact_payloads(artifact_id, version, payload_json, payload_hash) VALUES (?1, ?2, ?3, ?4)", libsql_rusqlite::params![artifact.artifact_id, artifact.version, payload, artifact.payload_hash])?;
            for dependency in &artifact.dependency_refs {
                let target_hash: String = connection
                    .query_row(
                        "SELECT payload_hash FROM artifacts WHERE project_id = ?1 AND artifact_id = ?2 AND version = ?3",
                        libsql_rusqlite::params![project_id, dependency.artifact_id, dependency.version],
                        |row| row.get(0),
                    )
                    .optional()?
                    .ok_or(ArtifactError::NotFound)?;
                if target_hash != dependency.payload_hash {
                    return Err(ArtifactError::HashMismatch);
                }
                connection.execute("INSERT INTO artifact_dependencies(source_artifact_id, source_version, target_artifact_id, target_version, target_payload_hash) VALUES (?1, ?2, ?3, ?4, ?5)", libsql_rusqlite::params![artifact.artifact_id, artifact.version, dependency.artifact_id, dependency.version, dependency.payload_hash])?;
            }
            Ok::<_, ArtifactError>(())
        })();
        match result {
            Ok(()) => {
                connection.execute_batch("RELEASE artifact_insert")?;
                Ok(())
            }
            Err(error) => {
                let _ = connection
                    .execute_batch("ROLLBACK TO artifact_insert; RELEASE artifact_insert");
                Err(error)
            }
        }
    }

    pub fn promote_head(
        connection: &Connection,
        project_id: &str,
        artifact: &ArtifactRef,
        artifact_type: &str,
        scope_level: &str,
        scope_key_value: &str,
    ) -> Result<(), ArtifactError> {
        let row: Option<(String, String, String, String, String)> = connection
            .query_row(
                "SELECT artifact_status, artifact_type, scope_level, scope_key, payload_hash FROM artifacts WHERE project_id = ?1 AND artifact_id = ?2 AND version = ?3",
                libsql_rusqlite::params![project_id, artifact.artifact_id, artifact.version],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        let (status, actual_type, actual_scope_level, actual_scope_key, actual_hash) =
            row.ok_or(ArtifactError::NotFound)?;
        if actual_hash != artifact.payload_hash {
            return Err(ArtifactError::HashMismatch);
        }
        if actual_type != artifact_type
            || actual_scope_level != scope_level
            || actual_scope_key != scope_key_value
        {
            return Err(ArtifactError::MetadataMismatch);
        }
        if status != "COMMITTED" {
            return Err(ArtifactError::CandidateHead);
        }
        if actual_type == "ChapterCandidate" {
            return Err(ArtifactError::CandidateHead);
        }
        let current: Option<(String, i64)> = connection
            .query_row(
                "SELECT artifact_id, version FROM canonical_heads WHERE project_id = ?1 AND artifact_type = ?2 AND scope_level = ?3 AND scope_key = ?4",
                libsql_rusqlite::params![project_id, artifact_type, scope_level, scope_key_value],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if current.as_ref().is_some_and(|(id, version)| {
            id == &artifact.artifact_id && *version >= artifact.version
        }) {
            return Err(ArtifactError::VersionConflict);
        }
        connection.execute("INSERT INTO canonical_heads(project_id, artifact_type, scope_level, scope_key, artifact_id, version, payload_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(project_id, artifact_type, scope_level, scope_key) DO UPDATE SET artifact_id=excluded.artifact_id, version=excluded.version, payload_hash=excluded.payload_hash", libsql_rusqlite::params![project_id, artifact_type, scope_level, scope_key_value, artifact.artifact_id, artifact.version, artifact.payload_hash])?;
        Ok(())
    }

    pub fn list_metadata(
        connection: &Connection,
        project_id: &str,
        limit: i64,
        after_rowid: Option<i64>,
    ) -> Result<Vec<Map<String, Value>>, ArtifactError> {
        let mut statement = connection.prepare(
            "SELECT rowid, artifact_id, artifact_type, scope_level, version, artifact_status, payload_hash, created_at FROM artifacts WHERE project_id = ?1 AND rowid > ?2 ORDER BY rowid LIMIT ?3",
        )?;
        let rows = statement.query_map(
            libsql_rusqlite::params![project_id, after_rowid.unwrap_or(0), limit.clamp(1, 500)],
            |row| {
                Ok(json!({
                    "cursor": row.get::<_, i64>(0)?,
                    "artifactId": row.get::<_, String>(1)?,
                    "artifactType": row.get::<_, String>(2)?,
                    "scopeLevel": row.get::<_, String>(3)?,
                    "version": row.get::<_, i64>(4)?,
                    "artifactStatus": row.get::<_, String>(5)?,
                    "payloadHash": row.get::<_, String>(6)?,
                    "createdAt": row.get::<_, String>(7)?,
                }))
            },
        )?;
        rows.map(|row| {
            row.map_err(ArtifactError::Sqlite)?
                .as_object()
                .cloned()
                .ok_or_else(|| ArtifactError::Schema("metadata projection".into()))
        })
        .collect()
    }

    pub fn collect_expired_candidates(
        connection: &Connection,
        project_id: &str,
        cutoff: &str,
        policy_digest: &str,
        now: &str,
    ) -> Result<GcReceipt, ArtifactError> {
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            let mut statement = connection.prepare(
                "SELECT artifact_id, version FROM artifacts candidate
                 WHERE project_id = ?1
                   AND artifact_status IN ('CANDIDATE', 'VALIDATED', 'REJECTED')
                   AND created_at < ?2
                   AND NOT EXISTS (
                     SELECT 1 FROM artifact_dependencies dependency
                     WHERE dependency.target_artifact_id = candidate.artifact_id
                       AND dependency.target_version = candidate.version
                   )",
            )?;
            let rows = statement.query_map([project_id, cutoff], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            let candidates: Vec<(String, i64)> = rows.collect::<Result<_, _>>()?;
            drop(statement);
            for (artifact_id, version) in &candidates {
                connection.execute(
                    "DELETE FROM artifact_dependencies WHERE source_artifact_id = ?1 AND source_version = ?2",
                    libsql_rusqlite::params![artifact_id, version],
                )?;
                connection.execute(
                    "DELETE FROM artifact_payloads WHERE artifact_id = ?1 AND version = ?2",
                    libsql_rusqlite::params![artifact_id, version],
                )?;
                connection.execute(
                    "DELETE FROM artifacts WHERE project_id = ?1 AND artifact_id = ?2 AND version = ?3",
                    libsql_rusqlite::params![project_id, artifact_id, version],
                )?;
            }
            let mut receipt = GcReceipt {
                receipt_id: format!("gc:{project_id}:{now}"),
                project_id: project_id.to_string(),
                policy_digest: policy_digest.to_string(),
                deleted_candidate_count: candidates.len() as i64,
                created_at: now.to_string(),
                receipt_digest: String::new(),
            };
            receipt.receipt_digest = sha256_canonical(&serde_json::to_value(&receipt)?)?;
            connection.execute(
                "INSERT INTO gc_receipts(receipt_id, project_id, policy_digest, deleted_candidate_count, created_at, receipt_digest) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                libsql_rusqlite::params![
                    receipt.receipt_id,
                    receipt.project_id,
                    receipt.policy_digest,
                    receipt.deleted_candidate_count,
                    receipt.created_at,
                    receipt.receipt_digest
                ],
            )?;
            Ok::<_, ArtifactError>(receipt)
        })();
        match result {
            Ok(receipt) => {
                connection.execute_batch("COMMIT")?;
                Ok(receipt)
            }
            Err(error) => {
                let _ = connection.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }
}

fn status_name(status: &ArtifactStatus) -> &'static str {
    match status {
        ArtifactStatus::Candidate => "CANDIDATE",
        ArtifactStatus::Validated => "VALIDATED",
        ArtifactStatus::Committed => "COMMITTED",
        ArtifactStatus::Stale => "STALE",
        ArtifactStatus::Rejected => "REJECTED",
        ArtifactStatus::Superseded => "SUPERSEDED",
    }
}

pub(crate) fn enum_string<T: Serialize>(value: &T) -> Result<String, ArtifactError> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| ArtifactError::Schema("expected string enum".into()))
}

pub(crate) fn scope_key(
    scope: &crate::generated::types::ArtifactScope,
) -> Result<String, ArtifactError> {
    String::from_utf8(canonical_bytes(&serde_json::to_value(scope)?)?)
        .map_err(|error| ArtifactError::Schema(error.to_string()))
}
