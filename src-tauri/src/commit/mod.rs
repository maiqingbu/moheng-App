use crate::generated::schema_versions::CONTRACT_VERSION;
use crate::generated::types::ArtifactScopeLevel;
use crate::generated::types::{
    ArtifactRef, CommitBundle, CommitReceipt, DomainReceipt, HeadChange, ProjectCommandReceipt,
};
use crate::generated::validators::validate_commit_bundle;
use crate::hash::sha256_canonical;
use crate::repositories::artifacts::{enum_string, scope_key, ArtifactError, ArtifactRepository};
use libsql_rusqlite::{Connection, OptionalExtension};
use std::collections::BTreeMap;

mod deltas;
mod final_gate;

type StoredGateCertificateRow = (
    String,
    i64,
    String,
    String,
    i64,
    String,
    String,
    String,
    String,
    String,
);

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
    #[error("budget hard limit")]
    BudgetHardLimit,
    #[error("gate certificate digest mismatch")]
    GateCertificateDigestMismatch,
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

    fn apply(
        connection: &Connection,
        bundle: &CommitBundle,
        bundle_digest: String,
        before: i64,
        committed_at: &str,
    ) -> Result<CommitReceipt, CommitError> {
        final_gate::validate(connection, bundle)?;
        let artifacts_before: i64 = connection.query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id = ?1",
            [&bundle.project_id],
            |row| row.get(0),
        )?;
        let heads_before: i64 = connection.query_row(
            "SELECT COUNT(*) FROM canonical_heads WHERE project_id = ?1",
            [&bundle.project_id],
            |row| row.get(0),
        )?;
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
        let heads_after: i64 = connection.query_row(
            "SELECT COUNT(*) FROM canonical_heads WHERE project_id = ?1",
            [&bundle.project_id],
            |row| row.get(0),
        )?;
        let fallback_artifact = bundle
            .artifacts
            .first()
            .map(|artifact| (artifact.artifact_id.as_str(), artifact.version));
        let delta_counts = deltas::apply_all(
            connection,
            bundle.canon_delta.as_ref(),
            bundle.obligation_delta.as_ref(),
            bundle.foreshadow_delta.as_ref(),
            bundle.knowledge_delta.as_ref(),
            bundle.state_delta.as_ref(),
            bundle.dependency_edges.as_ref(),
            bundle.job_updates.as_ref(),
            bundle.decision_updates.as_ref(),
            bundle.reader_desire_delta.as_ref(),
            committed_at,
            fallback_artifact,
        )?;
        crate::repositories::ops::SearchService::reconcile_in_transaction(
            connection,
            &bundle.project_id,
            committed_at,
        )
        .map_err(|error| CommitError::ProjectCommandApply(error.to_string()))?;
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
        // head_updates are already HeadChange (shared contract type); the receipt's
        // headChanges is the same type, so a plain clone suffices (the bundle is
        // still borrowed later for the digest).
        let head_changes = read_head_changes(connection, bundle)?;
        let artifacts_after: i64 = connection.query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id = ?1",
            [&bundle.project_id],
            |row| row.get(0),
        )?;
        let mut applied_delta_counts = BTreeMap::new();
        applied_delta_counts.insert("artifacts".into(), artifacts_after - artifacts_before);
        applied_delta_counts.insert("heads".into(), heads_after - heads_before);
        applied_delta_counts.extend(delta_counts);
        let gate_certificates_before: i64 = connection.query_row(
            "SELECT COUNT(*) FROM gate_certificates WHERE project_id = ?1",
            [&bundle.project_id],
            |row| row.get(0),
        )?;
        if let Some(certificates) = &bundle.gate_certificates {
            for certificate in certificates {
                let evidence_refs = serde_json::to_string(&certificate.evidence_refs)?;
                let existing: Option<StoredGateCertificateRow> =
                    connection
                        .query_row(
                            "SELECT gate_policy_artifact_id, gate_policy_version, gate_policy_payload_hash, subject_artifact_id, subject_version, subject_payload_hash, evidence_refs_json, source_digest, passed_at, certificate_digest FROM gate_certificates WHERE project_id = ?1 AND certificate_id = ?2",
                            [&bundle.project_id, &certificate.certificate_id],
                            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?)),
                        )
                        .optional()?;
                if let Some(existing) = existing {
                    let expected = (
                        certificate.gate_policy_ref.artifact_id.clone(),
                        certificate.gate_policy_ref.version,
                        certificate.gate_policy_ref.payload_hash.clone(),
                        certificate.subject_ref.artifact_id.clone(),
                        certificate.subject_ref.version,
                        certificate.subject_ref.payload_hash.clone(),
                        evidence_refs,
                        certificate.source_digest.clone(),
                        certificate.passed_at.clone(),
                        certificate.certificate_digest.clone(),
                    );
                    if existing != expected {
                        return Err(CommitError::ContractInvalid(
                            "GateCertificate id already exists with a different digest".into(),
                        ));
                    }
                } else {
                    connection.execute(
                        "INSERT INTO gate_certificates(certificate_id, project_id, gate_policy_artifact_id, gate_policy_version, gate_policy_payload_hash, subject_artifact_id, subject_version, subject_payload_hash, evidence_refs_json, source_digest, passed_at, certificate_digest) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                        libsql_rusqlite::params![certificate.certificate_id, certificate.project_id, certificate.gate_policy_ref.artifact_id, certificate.gate_policy_ref.version, certificate.gate_policy_ref.payload_hash, certificate.subject_ref.artifact_id, certificate.subject_ref.version, certificate.subject_ref.payload_hash, evidence_refs, certificate.source_digest, certificate.passed_at, certificate.certificate_digest],
                    )?;
                }
            }
        }
        let gate_certificates_after: i64 = connection.query_row(
            "SELECT COUNT(*) FROM gate_certificates WHERE project_id = ?1",
            [&bundle.project_id],
            |row| row.get(0),
        )?;
        applied_delta_counts.insert(
            "gateCertificates".into(),
            gate_certificates_after - gate_certificates_before,
        );
        let gate_certificate_refs = bundle
            .gate_certificates
            .as_ref()
            .map(|certificates| {
                certificates
                    .iter()
                    .map(|certificate| certificate.certificate_id.clone())
                    .collect()
            })
            .unwrap_or_default();
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
            gate_certificate_refs,
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
        if let Some(draft) = &bundle.receipt_draft {
            let receipt_id = draft
                .receipt_id
                .clone()
                .unwrap_or_else(|| format!("domain-receipt:{}:{}", bundle.commit_id, draft.domain));
            let mut domain_receipt = DomainReceipt {
                receipt_id,
                project_id: bundle.project_id.clone(),
                domain: draft.domain.clone(),
                command_id: bundle.command_id.clone(),
                commit_receipt_id: receipt.receipt_id.clone(),
                artifact_refs: draft.artifact_refs.clone().unwrap_or_default(),
                payload: draft.payload.clone().unwrap_or_default(),
                created_at: committed_at.to_string(),
                receipt_digest: String::new(),
            };
            let mut unsigned = serde_json::to_value(&domain_receipt)?;
            unsigned["receiptDigest"] = serde_json::json!("");
            domain_receipt.receipt_digest = sha256_canonical(&unsigned)?;
            let receipt_json = serde_json::to_string(&domain_receipt)?;
            let existing: Option<String> = connection
                .query_row(
                    "SELECT receipt_json FROM domain_receipts WHERE project_id = ?1 AND receipt_id = ?2",
                    [&bundle.project_id, &domain_receipt.receipt_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(existing) = existing {
                if existing != receipt_json {
                    return Err(CommitError::ContractInvalid(
                        "DomainReceipt id already exists with a different payload".into(),
                    ));
                }
            } else {
                connection.execute(
                    "INSERT INTO domain_receipts(receipt_id, project_id, domain, command_id, commit_receipt_id, created_at, receipt_json, receipt_digest) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    libsql_rusqlite::params![domain_receipt.receipt_id, domain_receipt.project_id, domain_receipt.domain, domain_receipt.command_id, domain_receipt.commit_receipt_id, domain_receipt.created_at, receipt_json, domain_receipt.receipt_digest],
                )?;
            }
        }
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

fn read_head_changes(
    connection: &Connection,
    bundle: &CommitBundle,
) -> Result<Vec<HeadChange>, CommitError> {
    bundle
        .head_updates
        .as_ref()
        .map(|heads| {
            heads
                .iter()
                .map(|head| {
                    let artifact_type = enum_string(&head.artifact_type)?;
                    let scope_level = enum_string(&head.scope.level)?;
                    let scope_key = scope_key(&head.scope)?;
                    let (artifact_id, version, payload_hash): (String, i64, String) = connection
                        .query_row(
                            "SELECT artifact_id, version, payload_hash FROM canonical_heads WHERE project_id = ?1 AND artifact_type = ?2 AND scope_level = ?3 AND scope_key = ?4",
                            libsql_rusqlite::params![bundle.project_id, artifact_type, scope_level, scope_key],
                            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                        )?;
                    let mut actual = head.clone();
                    actual.artifact_id = artifact_id;
                    actual.version = version;
                    actual.payload_hash = payload_hash;
                    Ok(actual)
                })
                .collect()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

/// Runs the stable-id boundary check over every scope binding in an artifact.
/// Prefixed references are authoritative and must be well-formed opaque ids;
/// malformed prefixed references (`vol:1`-style) are rejected before persistence.
/// Bare legacy references are tolerated by `validate_scope_binding` (which owns
/// the strict-vs-legacy decision), so this simply delegates wholesale.
fn validate_artifact_scope_bindings(
    artifact: &crate::generated::types::ArtifactEnvelope,
) -> Result<(), CommitError> {
    let scope = &artifact.scope;
    let check = |scope_level: &ArtifactScopeLevel, id: &str| -> Result<(), CommitError> {
        crate::stable_id::validate_scope_binding(id).map_err(|_| {
            CommitError::ContractInvalid(format!(
                "scope {scope_level:?} id is not a well-formed opaque stable id: {id}"
            ))
        })?;
        Ok(())
    };
    for (scope_level, id) in [
        (&ArtifactScopeLevel::Book, scope.book_id.as_deref()),
        (&ArtifactScopeLevel::Volume, scope.volume_id.as_deref()),
        (&ArtifactScopeLevel::Arc, scope.arc_id.as_deref()),
        (&ArtifactScopeLevel::Chapter, scope.chapter_id.as_deref()),
        (&ArtifactScopeLevel::Entity, scope.entity_id.as_deref()),
    ] {
        if let Some(id) = id {
            check(scope_level, id)?;
        }
    }
    Ok(())
}
