use crate::generated::schema_versions::CONTRACT_VERSION;
use crate::generated::types::ArtifactScopeLevel;
use crate::generated::types::{
    ArtifactRef, CommitBundle, CommitReceipt, HeadChange, ProjectCommandReceipt,
};
use crate::generated::validators::validate_commit_bundle;
use crate::hash::sha256_canonical;
use crate::repositories::artifacts::{enum_string, scope_key, ArtifactError, ArtifactRepository};
use libsql_rusqlite::{Connection, OptionalExtension};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub enum CommitError {
    #[error("project not found")]
    ProjectNotFound,
    #[error("REVISION_CONFLICT")]
    RevisionConflict,
    #[error("IDEMPOTENCY_CONFLICT")]
    IdempotencyConflict,
    #[error("candidate status required")]
    CandidateStatusRequired,
    #[error("unsupported reserved delta")]
    UnsupportedDelta,
    #[error("contract invalid: {0}")]
    ContractInvalid(String),
    #[error("commit rolled back")]
    RolledBack,
    #[error("receipt digest mismatch")]
    ReceiptDigestMismatch,
    #[error("PROJECT_COMMAND_RECEIPT_DIGEST_MISMATCH")]
    ProjectCommandReceiptDigestMismatch,
    #[error("project command apply failed: {0}")]
    ProjectCommandApply(String),
    #[error("artifact: {0}")]
    Artifact(#[from] ArtifactError),
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct CommitService;

pub enum ProjectCommandOutcome<T> {
    Replayed(ProjectCommandReceipt),
    Committed {
        value: T,
        receipt: ProjectCommandReceipt,
    },
}

#[derive(Clone, Copy)]
pub struct ProjectCommandSpec<'a> {
    pub project_id: &'a str,
    pub result_project_id: &'a str,
    pub command_id: &'a str,
    pub idempotency_key: &'a str,
    pub command_type: &'a str,
    pub request_digest: &'a str,
    pub expected_revision: Option<i64>,
    pub now: &'a str,
    pub advance_revision: bool,
}

impl CommitService {
    pub fn commit_project_command<T>(
        connection: &Connection,
        spec: ProjectCommandSpec<'_>,
        apply: impl FnOnce(&Connection) -> Result<T, CommitError>,
    ) -> Result<ProjectCommandOutcome<T>, CommitError> {
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            if let Some(previous) = Self::find_project_command_receipt(
                connection,
                spec.project_id,
                spec.idempotency_key,
            )? {
                if previous.request_digest == spec.request_digest {
                    return Ok(ProjectCommandOutcome::Replayed(previous));
                }
                return Err(CommitError::IdempotencyConflict);
            }

            let before: i64 = connection
                .query_row(
                    "SELECT revision FROM project_meta WHERE project_id = ?1 AND maintenance_state = 'READY'",
                    [spec.project_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or(CommitError::ProjectNotFound)?;
            if let Some(expected) = spec.expected_revision {
                if expected != before {
                    return Err(CommitError::RevisionConflict);
                }
            }

            let value = apply(connection)?;
            let after = if spec.advance_revision {
                before + 1
            } else {
                before
            };
            if spec.advance_revision {
                let updated = connection.execute(
                    "UPDATE project_meta SET revision = ?1, updated_at = ?2 WHERE project_id = ?3 AND revision = ?4",
                    libsql_rusqlite::params![after, spec.now, spec.project_id, before],
                )?;
                if updated != 1 {
                    return Err(CommitError::RevisionConflict);
                }
            }

            let mut receipt = ProjectCommandReceipt {
                receipt_id: format!("project-receipt:{}", spec.command_id),
                project_id: spec.project_id.to_string(),
                result_project_id: spec.result_project_id.to_string(),
                command_id: spec.command_id.to_string(),
                idempotency_key: spec.idempotency_key.to_string(),
                command_type: spec.command_type.to_string(),
                before_revision: before,
                after_revision: after,
                request_digest: spec.request_digest.to_string(),
                committed_at: spec.now.to_string(),
                receipt_digest: String::new(),
            };
            let mut digest_input = serde_json::to_value(&receipt)?;
            digest_input["receiptDigest"] = serde_json::json!("");
            receipt.receipt_digest = sha256_canonical(&digest_input)?;
            let receipt_json = serde_json::to_string(&receipt)?;
            connection.execute(
                "INSERT INTO project_command_receipts(receipt_id, project_id, result_project_id, command_id, idempotency_key, command_type, before_revision, after_revision, request_digest, receipt_json, receipt_digest, committed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                libsql_rusqlite::params![
                    receipt.receipt_id,
                    receipt.project_id,
                    receipt.result_project_id,
                    receipt.command_id,
                    receipt.idempotency_key,
                    receipt.command_type,
                    receipt.before_revision,
                    receipt.after_revision,
                    receipt.request_digest,
                    receipt_json,
                    receipt.receipt_digest,
                    receipt.committed_at,
                ],
            )?;
            Ok(ProjectCommandOutcome::Committed { value, receipt })
        })();

        match result {
            Ok(outcome) => {
                connection.execute_batch("COMMIT")?;
                Ok(outcome)
            }
            Err(error) => {
                let _ = connection.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub fn find_project_command_receipt(
        connection: &Connection,
        project_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<ProjectCommandReceipt>, CommitError> {
        let row: Option<(String, String)> = connection
            .query_row(
                "SELECT receipt_json, receipt_digest FROM project_command_receipts WHERE project_id = ?1 AND idempotency_key = ?2",
                [project_id, idempotency_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(receipt_json, stored_digest)| {
            let receipt: ProjectCommandReceipt = serde_json::from_str(&receipt_json)?;
            let mut digest_input = serde_json::to_value(&receipt)?;
            digest_input["receiptDigest"] = serde_json::json!("");
            let expected = sha256_canonical(&digest_input)?;
            if receipt.receipt_digest != expected || stored_digest != expected {
                return Err(CommitError::ProjectCommandReceiptDigestMismatch);
            }
            Ok(receipt)
        })
        .transpose()
    }

    pub fn commit(
        connection: &Connection,
        bundle: &CommitBundle,
        committed_at: &str,
    ) -> Result<CommitReceipt, CommitError> {
        let bundle_value = serde_json::to_value(bundle)?;
        validate_commit_bundle(&bundle_value).map_err(CommitError::ContractInvalid)?;
        if bundle.contract_version != CONTRACT_VERSION {
            return Err(CommitError::ContractInvalid(
                "unsupported contractVersion".into(),
            ));
        }
        if Self::has_reserved_delta(bundle) {
            return Err(CommitError::UnsupportedDelta);
        }
        if bundle
            .artifacts
            .iter()
            .any(|artifact| artifact.provenance.command_id != bundle.command_id)
        {
            return Err(CommitError::ContractInvalid(
                "artifact provenance commandId mismatch".into(),
            ));
        }
        let bundle_digest = sha256_canonical(&bundle_value)?;
        if let Some(previous) =
            Self::find_receipt(connection, &bundle.project_id, &bundle.idempotency_key)?
        {
            if previous.bundle_digest == bundle_digest {
                return Ok(previous);
            }
            return Err(CommitError::IdempotencyConflict);
        }
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| {
            let before: i64 = connection
                .query_row(
                    "SELECT revision FROM project_meta WHERE project_id = ?1 AND maintenance_state = 'READY'",
                    [&bundle.project_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or(CommitError::ProjectNotFound)?;
            if let Some(expected) = bundle.expected_project_revision {
                if expected != before {
                    return Err(CommitError::RevisionConflict);
                }
            }
            Self::apply(connection, bundle, bundle_digest, before, committed_at)
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

    fn has_reserved_delta(bundle: &CommitBundle) -> bool {
        bundle.canon_delta.is_some()
            || bundle.obligation_delta.is_some()
            || bundle.foreshadow_delta.is_some()
            || bundle.knowledge_delta.is_some()
            || bundle.state_delta.is_some()
            || bundle.dependency_edges.is_some()
            || bundle.job_updates.is_some()
            || bundle.decision_updates.is_some()
            || bundle.gate_certificates.is_some()
    }

    fn apply(
        connection: &Connection,
        bundle: &CommitBundle,
        bundle_digest: String,
        before: i64,
        committed_at: &str,
    ) -> Result<CommitReceipt, CommitError> {
        connection.execute("INSERT INTO commands(project_id, command_id, idempotency_key, request_digest, status, created_at) VALUES (?1, ?2, ?3, ?4, 'PREPARED', ?5)", libsql_rusqlite::params![bundle.project_id, bundle.command_id, bundle.idempotency_key, bundle_digest, bundle.created_at])?;
        for artifact in &bundle.artifacts {
            if !matches!(
                artifact.artifact_status,
                crate::generated::types::ArtifactStatus::Candidate
                    | crate::generated::types::ArtifactStatus::Validated
            ) {
                return Err(CommitError::CandidateStatusRequired);
            }
            // Production stable-id boundary (B01 修订): every scope binding that
            // carries a `kind:` prefix must be a well-formed opaque stable id. This
            // is where the unified Stable ID implementation enters the real creation
            // path — a malformed `vol:1`/`chap:3` display reference is rejected
            // before a row is persisted.
            validate_artifact_scope_bindings(artifact)?;
            ArtifactRepository::insert_candidate(connection, &bundle.project_id, artifact)?;
            connection.execute("UPDATE artifacts SET artifact_status = 'COMMITTED', committed_at = ?1 WHERE artifact_id = ?2 AND version = ?3", libsql_rusqlite::params![committed_at, artifact.artifact_id, artifact.version])?;
        }
        if let Some(heads) = &bundle.head_updates {
            for head in heads {
                let artifact_type = enum_string(&head.artifact_type)?;
                let scope_level = enum_string(&head.scope.level)?;
                let scope_key = scope_key(&head.scope)?;
                ArtifactRepository::promote_head(
                    connection,
                    &bundle.project_id,
                    &ArtifactRef {
                        artifact_id: head.artifact_id.clone(),
                        version: head.version,
                        payload_hash: head.payload_hash.clone(),
                    },
                    &artifact_type,
                    &scope_level,
                    &scope_key,
                )?;
            }
        }
        let after = before + 1;
        let updated = connection.execute(
            "UPDATE project_meta SET revision = ?1, updated_at = ?2 WHERE project_id = ?3 AND revision = ?4",
            libsql_rusqlite::params![after, committed_at, bundle.project_id, before],
        )?;
        if updated != 1 {
            return Err(CommitError::RevisionConflict);
        }
        let refs: Vec<ArtifactRef> = bundle
            .artifacts
            .iter()
            .map(|artifact| ArtifactRef {
                artifact_id: artifact.artifact_id.clone(),
                version: artifact.version,
                payload_hash: artifact.payload_hash.clone(),
            })
            .collect();
        let head_changes = bundle
            .head_updates
            .as_ref()
            .map(|heads| {
                heads
                    .iter()
                    .map(|head| HeadChange {
                        artifact_type: head.artifact_type.clone(),
                        scope: head.scope.clone(),
                        artifact_id: head.artifact_id.clone(),
                        version: head.version,
                        payload_hash: head.payload_hash.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut applied_delta_counts = BTreeMap::new();
        applied_delta_counts.insert("artifacts".into(), bundle.artifacts.len() as i64);
        applied_delta_counts.insert(
            "heads".into(),
            bundle.head_updates.as_ref().map_or(0, Vec::len) as i64,
        );
        let mut receipt = CommitReceipt {
            receipt_id: format!("receipt:{}", bundle.commit_id),
            commit_id: bundle.commit_id.clone(),
            project_id: bundle.project_id.clone(),
            command_id: bundle.command_id.clone(),
            idempotency_key: bundle.idempotency_key.clone(),
            before_revision: before,
            after_revision: after,
            bundle_digest,
            committed_artifact_refs: refs,
            head_changes,
            applied_delta_counts,
            gate_certificate_refs: Vec::new(),
            committed_at: committed_at.to_string(),
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = sha256_canonical(&serde_json::to_value(&receipt)?)?;
        let receipt_json = serde_json::to_string(&receipt)?;
        connection.execute("INSERT INTO commit_receipts(receipt_id, project_id, commit_id, command_id, idempotency_key, before_revision, after_revision, bundle_digest, receipt_json, receipt_digest, committed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)", libsql_rusqlite::params![receipt.receipt_id, receipt.project_id, receipt.commit_id, receipt.command_id, receipt.idempotency_key, receipt.before_revision, receipt.after_revision, receipt.bundle_digest, receipt_json, receipt.receipt_digest, receipt.committed_at])?;
        connection.execute(
            "UPDATE commands SET status = 'COMMITTED' WHERE project_id = ?1 AND command_id = ?2",
            [&bundle.project_id, &bundle.command_id],
        )?;
        Ok(receipt)
    }

    pub fn find_receipt(
        connection: &Connection,
        project_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<CommitReceipt>, CommitError> {
        let value: Option<(String, String)> = connection
            .query_row(
                "SELECT receipt_json, receipt_digest FROM commit_receipts WHERE project_id = ?1 AND idempotency_key = ?2",
                [project_id, idempotency_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        value
            .map(|(json, stored_digest)| {
                let receipt: CommitReceipt = serde_json::from_str(&json)?;
                let mut unsigned = receipt.clone();
                unsigned.receipt_digest.clear();
                let expected = sha256_canonical(&serde_json::to_value(&unsigned)?)?;
                if receipt.receipt_digest != expected || stored_digest != expected {
                    return Err(CommitError::ReceiptDigestMismatch);
                }
                Ok(receipt)
            })
            .transpose()
    }
}

/// Runs the stable-id boundary check over every scope binding in an artifact.
/// Prefixed references are authoritative and must be well-formed opaque ids;
/// malformed prefixed references (`vol:1`-style) are rejected before persistence.
fn validate_artifact_scope_bindings(
    artifact: &crate::generated::types::ArtifactEnvelope,
) -> Result<(), CommitError> {
    let scope = &artifact.scope;
    let check = |scope_level: &ArtifactScopeLevel, id: &str| -> Result<(), CommitError> {
        // Only a prefixed reference enters the strict opaque-id check; the
        // tolerated legacy bare form is left to the caller.
        if id.contains(':') {
            crate::stable_id::validate_scope_binding(id).map_err(|_| {
                CommitError::ContractInvalid(format!(
                    "scope {scope_level:?} id is not a well-formed opaque stable id: {id}"
                ))
            })?;
        }
        Ok(())
    };
    if let Some(id) = &scope.book_id {
        check(&ArtifactScopeLevel::Book, id)?;
    }
    if let Some(id) = &scope.volume_id {
        check(&ArtifactScopeLevel::Volume, id)?;
    }
    if let Some(id) = &scope.arc_id {
        check(&ArtifactScopeLevel::Arc, id)?;
    }
    if let Some(id) = &scope.chapter_id {
        check(&ArtifactScopeLevel::Chapter, id)?;
    }
    if let Some(id) = &scope.entity_id {
        check(&ArtifactScopeLevel::Entity, id)?;
    }
    let _ = scope;
    Ok(())
}
