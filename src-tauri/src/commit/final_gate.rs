use crate::generated::types::CommitBundle;
use libsql_rusqlite::Connection;
use serde_json::Value;

use super::deltas::certificate_digest;
use super::CommitError;

fn invalid(message: impl Into<String>) -> CommitError {
    CommitError::ContractInvalid(message.into())
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

fn artifact_exists(
    bundle: &CommitBundle,
    connection: &Connection,
    id: &str,
    version: i64,
    hash: &str,
) -> Result<bool, CommitError> {
    if bundle.artifacts.iter().any(|artifact| {
        artifact.artifact_id == id && artifact.version == version && artifact.payload_hash == hash
    }) {
        return Ok(true);
    }
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM artifacts WHERE project_id = ?1 AND artifact_id = ?2 AND version = ?3 AND payload_hash = ?4)",
        libsql_rusqlite::params![bundle.project_id, id, version, hash],
        |row| row.get(0),
    )?)
}

pub(crate) fn validate(connection: &Connection, bundle: &CommitBundle) -> Result<(), CommitError> {
    if bundle.project_id.trim().is_empty()
        || bundle.command_id.trim().is_empty()
        || bundle.idempotency_key.trim().is_empty()
    {
        return Err(invalid(
            "project/command/idempotency identity cannot be empty",
        ));
    }
    for artifact in &bundle.artifacts {
        if artifact.project_id != bundle.project_id {
            return Err(invalid("artifact projectId mismatch"));
        }
    }
    for edge in bundle.dependency_edges.iter().flatten() {
        if !valid_sha(&edge.source_hash) {
            return Err(invalid(
                "dependency sourceHash must be a 64-character hex digest",
            ));
        }
    }
    for state in bundle.state_delta.iter().flatten() {
        if state
            .payload_json
            .as_ref()
            .is_some_and(|payload| payload.get("hardLimitExceeded") == Some(&Value::Bool(true)))
        {
            return Err(CommitError::BudgetHardLimit);
        }
    }
    for certificate in bundle.gate_certificates.iter().flatten() {
        if certificate.project_id != bundle.project_id {
            return Err(invalid("gate certificate projectId mismatch"));
        }
        if !valid_sha(&certificate.source_digest) || !valid_sha(&certificate.certificate_digest) {
            return Err(invalid("gate certificate digest format invalid"));
        }
        if certificate.certificate_digest != certificate_digest(certificate)? {
            return Err(CommitError::GateCertificateDigestMismatch);
        }
        if !artifact_exists(
            bundle,
            connection,
            &certificate.gate_policy_ref.artifact_id,
            certificate.gate_policy_ref.version,
            &certificate.gate_policy_ref.payload_hash,
        )? {
            return Err(invalid(
                "gate policy artifact reference not found or hash mismatch",
            ));
        }
        for evidence in &certificate.evidence_refs {
            if !valid_sha(&evidence.payload_hash)
                || !artifact_exists(
                    bundle,
                    connection,
                    &evidence.artifact_id,
                    evidence.version,
                    &evidence.payload_hash,
                )?
            {
                return Err(invalid(
                    "gate evidence artifact reference not found or hash mismatch",
                ));
            }
        }
    }
    Ok(())
}
