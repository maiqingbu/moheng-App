use crate::generated::types::{ArtifactRef, CommitBundle, GateCertificate};
use crate::hash::sha256_canonical;
use crate::repositories::artifacts::{enum_string, scope_key};
use libsql_rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};

use super::deltas::certificate_source_digest;
use super::CommitError;

fn invalid(message: impl Into<String>) -> CommitError {
    CommitError::ContractInvalid(message.into())
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

#[derive(Debug, Clone)]
struct ResolvedArtifact {
    artifact_type: String,
    scope_level: String,
    scope_key: String,
    status: String,
}

type FactDigestRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    String,
    Option<String>,
);

fn bundle_artifact(
    bundle: &CommitBundle,
    id: &str,
    version: i64,
    hash: &str,
) -> Result<Option<ResolvedArtifact>, CommitError> {
    let Some(artifact) = bundle.artifacts.iter().find(|artifact| {
        artifact.artifact_id == id && artifact.version == version && artifact.payload_hash == hash
    }) else {
        return Ok(None);
    };
    Ok(Some(ResolvedArtifact {
        artifact_type: enum_string(&artifact.artifact_type)?,
        scope_level: enum_string(&artifact.scope.level)?,
        scope_key: scope_key(&artifact.scope)?,
        status: enum_string(&artifact.artifact_status)?,
    }))
}

fn resolve_artifact(
    connection: &Connection,
    bundle: &CommitBundle,
    artifact: &ArtifactRef,
) -> Result<ResolvedArtifact, CommitError> {
    if let Some(found) = bundle_artifact(
        bundle,
        &artifact.artifact_id,
        artifact.version,
        &artifact.payload_hash,
    )? {
        return Ok(found);
    }
    connection
        .query_row(
            "SELECT artifact_type, scope_level, scope_key, artifact_status FROM artifacts WHERE project_id = ?1 AND artifact_id = ?2 AND version = ?3 AND payload_hash = ?4",
            libsql_rusqlite::params![bundle.project_id, artifact.artifact_id, artifact.version, artifact.payload_hash],
            |row| {
                Ok(ResolvedArtifact {
                    artifact_type: row.get(0)?,
                    scope_level: row.get(1)?,
                    scope_key: row.get(2)?,
                    status: row.get(3)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| invalid("artifact reference not found or hash mismatch"))
}

fn reference_artifact(
    connection: &Connection,
    bundle: &CommitBundle,
    reference: &str,
) -> Result<Option<String>, CommitError> {
    if let Some(artifact) = bundle
        .artifacts
        .iter()
        .find(|artifact| artifact.artifact_id == reference)
    {
        return Ok(Some(artifact.payload_hash.clone()));
    }
    Ok(connection
        .query_row(
            "SELECT payload_hash FROM artifacts WHERE project_id = ?1 AND artifact_id = ?2 ORDER BY version DESC LIMIT 1",
            libsql_rusqlite::params![bundle.project_id, reference],
            |row| row.get(0),
        )
        .optional()?)
}

fn current_fact_digest(
    connection: &Connection,
    project_id: &str,
    fact_id: &str,
) -> Result<Option<String>, CommitError> {
    let row: Option<FactDigestRow> = connection
        .query_row(
            "SELECT project_id, subject_id, predicate, object_json, valid_from_chapter_id, valid_to_chapter_id, source_artifact_id, source_version, authority_level, superseded_by FROM canon_facts WHERE project_id = ?1 AND fact_id = ?2",
            [project_id, fact_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?)),
        )
        .optional()?;
    row.map(
        |(
            project_id,
            subject_id,
            predicate,
            object_json,
            valid_from,
            valid_to,
            source_artifact_id,
            source_version,
            authority_level,
            superseded_by,
        )| {
            let object_json: Value = serde_json::from_str(&object_json)?;
            Ok(sha256_canonical(&json!({
                "projectId": project_id,
                "factId": fact_id,
                "subjectId": subject_id,
                "predicate": predicate,
                "objectJson": object_json,
                "validFromChapterId": valid_from,
                "validToChapterId": valid_to,
                "sourceArtifactId": source_artifact_id,
                "sourceVersion": source_version,
                "authorityLevel": authority_level,
                "supersededBy": superseded_by
            }))?)
        },
    )
    .transpose()
}

fn dependency_source_digest(
    connection: &Connection,
    bundle: &CommitBundle,
    source_ref: &str,
) -> Result<String, CommitError> {
    if let Some(hash) = reference_artifact(connection, bundle, source_ref)? {
        return Ok(hash);
    }
    if let Some(fact_digest) = current_fact_digest(connection, &bundle.project_id, source_ref)? {
        return Ok(fact_digest);
    }
    Err(invalid(format!(
        "dependency sourceRef not found: {source_ref}"
    )))
}

fn chapter_exists(
    connection: &Connection,
    project_id: &str,
    chapter_id: &str,
) -> Result<bool, CommitError> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM chapters WHERE project_id = ?1 AND chapter_id = ?2)",
        [project_id, chapter_id],
        |row| row.get(0),
    )?)
}

fn chapter_number(
    connection: &Connection,
    project_id: &str,
    chapter_id: &str,
) -> Result<Option<i64>, CommitError> {
    Ok(connection
        .query_row(
            "SELECT chapter_no FROM chapters WHERE project_id = ?1 AND chapter_id = ?2",
            [project_id, chapter_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn pending_fact(bundle: &CommitBundle, fact_id: &str) -> bool {
    bundle
        .canon_delta
        .as_ref()
        .is_some_and(|deltas| deltas.iter().any(|delta| delta.fact_id == fact_id))
}

fn validate_knowledge(connection: &Connection, bundle: &CommitBundle) -> Result<(), CommitError> {
    for knowledge in bundle.knowledge_delta.iter().flatten() {
        let fact_exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM canon_facts WHERE project_id = ?1 AND fact_id = ?2)",
            [&bundle.project_id, &knowledge.fact_id],
            |row| row.get(0),
        )?;
        if !fact_exists && !pending_fact(bundle, &knowledge.fact_id) {
            return Err(invalid(format!(
                "knowledge factId not found: {}",
                knowledge.fact_id
            )));
        }
        let knower_exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM canon_entities WHERE project_id = ?1 AND entity_id = ?2)",
            [&bundle.project_id, &knowledge.knower_id],
            |row| row.get(0),
        )?;
        let pending_knower = bundle.canon_delta.as_ref().is_some_and(|deltas| {
            deltas
                .iter()
                .any(|delta| delta.entity_id.as_deref() == Some(knowledge.knower_id.as_str()))
        });
        if !knower_exists && !pending_knower {
            return Err(invalid(format!(
                "knowledge knowerId not found: {}",
                knowledge.knower_id
            )));
        }
        for chapter_id in [
            knowledge.from_chapter_id.as_ref(),
            knowledge.until_chapter_id.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if !chapter_exists(connection, &bundle.project_id, chapter_id)? {
                return Err(invalid(format!(
                    "knowledge chapter reference not found: {chapter_id}"
                )));
            }
        }
        if let (Some(from), Some(until)) = (&knowledge.from_chapter_id, &knowledge.until_chapter_id)
        {
            if chapter_number(connection, &bundle.project_id, from)?
                > chapter_number(connection, &bundle.project_id, until)?
            {
                return Err(invalid("knowledge visibility interval is inverted"));
            }
        }
        if let Some(visibility) = &knowledge.visibility_scope {
            let valid = visibility == "PUBLIC"
                || visibility == &format!("CHARACTER:{}", knowledge.knower_id)
                || visibility
                    .strip_prefix("CHAPTER:")
                    .is_some_and(|chapter_id| {
                        chapter_exists(connection, &bundle.project_id, chapter_id).unwrap_or(false)
                    });
            if !valid {
                return Err(invalid(format!(
                    "unsupported knowledge visibility scope: {visibility}"
                )));
            }
        }
    }
    Ok(())
}

fn transaction_scope(bundle: &CommitBundle) -> Option<(String, String)> {
    bundle.artifacts.iter().find_map(|artifact| {
        let artifact_type = enum_string(&artifact.artifact_type).ok()?;
        if matches!(artifact_type.as_str(), "GatePolicy" | "ProductionReceipt") {
            return None;
        }
        Some((
            enum_string(&artifact.scope.level).ok()?,
            scope_key(&artifact.scope).ok()?,
        ))
    })
}

fn validate_heads(connection: &Connection, bundle: &CommitBundle) -> Result<(), CommitError> {
    let target_scope = transaction_scope(bundle);
    for head in bundle.head_updates.iter().flatten() {
        let artifact = ArtifactRef {
            artifact_id: head.artifact_id.clone(),
            version: head.version,
            payload_hash: head.payload_hash.clone(),
        };
        let resolved = resolve_artifact(connection, bundle, &artifact)?;
        let expected_type = enum_string(&head.artifact_type)?;
        let expected_level = enum_string(&head.scope.level)?;
        let expected_key = scope_key(&head.scope)?;
        if resolved.artifact_type != expected_type
            || resolved.scope_level != expected_level
            || resolved.scope_key != expected_key
        {
            return Err(invalid(
                "head artifact metadata does not match the referenced artifact",
            ));
        }
        if resolved.artifact_type == "ChapterCandidate" {
            return Err(CommitError::Artifact(
                crate::repositories::artifacts::ArtifactError::CandidateHead,
            ));
        }
        if !matches!(
            resolved.status.as_str(),
            "CANDIDATE" | "VALIDATED" | "COMMITTED"
        ) {
            return Err(invalid(
                "head artifact is not eligible for canonical promotion",
            ));
        }
        if resolved.artifact_type == "FinalChapter"
            && bundle
                .gate_certificates
                .as_ref()
                .is_none_or(|certificates| certificates.is_empty())
        {
            return Err(invalid("FinalChapter head requires a GateCertificate"));
        }
        if let Some((level, key)) = &target_scope {
            if expected_type != "GatePolicy"
                && expected_type != "ProductionReceipt"
                && (level != &expected_level || key != &expected_key)
            {
                return Err(invalid(
                    "head scope crosses the CommitBundle transaction scope",
                ));
            }
        }
    }
    Ok(())
}

fn evidence_type_allowed(artifact_type: &str) -> bool {
    matches!(
        artifact_type,
        "ReviewReport" | "NarrativeQualityReport" | "IntegrityReport" | "ProductionReceipt"
    )
}

fn existing_certificate_matches(
    connection: &Connection,
    bundle: &CommitBundle,
    certificate: &GateCertificate,
) -> Result<bool, CommitError> {
    let existing: Option<(String, i64, String, String, String, String, String)> = connection
        .query_row(
            "SELECT gate_policy_artifact_id, gate_policy_version, gate_policy_payload_hash, evidence_refs_json, source_digest, passed_at, certificate_digest FROM gate_certificates WHERE project_id = ?1 AND certificate_id = ?2",
            [&bundle.project_id, &certificate.certificate_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)),
        )
        .optional()?;
    let Some(existing) = existing else {
        return Ok(false);
    };
    let evidence_refs = serde_json::to_string(&certificate.evidence_refs)?;
    if existing
        == (
            certificate.gate_policy_ref.artifact_id.clone(),
            certificate.gate_policy_ref.version,
            certificate.gate_policy_ref.payload_hash.clone(),
            evidence_refs,
            certificate.source_digest.clone(),
            certificate.passed_at.clone(),
            certificate.certificate_digest.clone(),
        )
    {
        Ok(true)
    } else {
        Err(invalid(
            "GateCertificate id already exists with a different digest",
        ))
    }
}

fn validate_gate_certificates(
    connection: &Connection,
    bundle: &CommitBundle,
) -> Result<(), CommitError> {
    let target_scope = transaction_scope(bundle);
    for certificate in bundle.gate_certificates.iter().flatten() {
        if certificate.project_id != bundle.project_id {
            return Err(invalid("gate certificate projectId mismatch"));
        }
        if !valid_sha(&certificate.source_digest) || !valid_sha(&certificate.certificate_digest) {
            return Err(invalid("gate certificate digest format invalid"));
        }
        if certificate.source_digest != certificate_source_digest(certificate)? {
            return Err(invalid("gate certificate sourceDigest mismatch"));
        }
        if certificate.certificate_digest != super::deltas::certificate_digest(certificate)? {
            return Err(CommitError::GateCertificateDigestMismatch);
        }
        let policy = resolve_artifact(connection, bundle, &certificate.gate_policy_ref)?;
        if policy.artifact_type != "GatePolicy" {
            return Err(invalid(
                "gatePolicyRef must reference a GatePolicy artifact",
            ));
        }
        if !matches!(
            policy.status.as_str(),
            "CANDIDATE" | "VALIDATED" | "COMMITTED"
        ) {
            return Err(invalid("gate policy artifact is not active"));
        }
        if let Some((level, key)) = &target_scope {
            if policy.scope_level != "PROJECT"
                && (&policy.scope_level != level || &policy.scope_key != key)
            {
                return Err(invalid(
                    "gate policy scope does not match the transaction scope",
                ));
            }
        }
        for evidence in &certificate.evidence_refs {
            let resolved = resolve_artifact(connection, bundle, evidence)?;
            if !evidence_type_allowed(&resolved.artifact_type) {
                return Err(invalid("gate evidence artifact type is not allowed"));
            }
            if let Some((level, key)) = &target_scope {
                if &resolved.scope_level != level || &resolved.scope_key != key {
                    return Err(invalid(
                        "gate evidence scope does not match the transaction scope",
                    ));
                }
            }
        }
        let _ = existing_certificate_matches(connection, bundle, certificate)?;
    }
    Ok(())
}

fn validate_budget(connection: &Connection, bundle: &CommitBundle) -> Result<(), CommitError> {
    let mut chapter_ids = Vec::new();
    for artifact in &bundle.artifacts {
        if enum_string(&artifact.scope.level)? == "CHAPTER" {
            let chapter_id = artifact
                .scope
                .chapter_id
                .as_ref()
                .ok_or_else(|| invalid("chapter artifact scope is missing chapterId"))?;
            chapter_ids.push(chapter_id.clone());
        }
    }
    chapter_ids.sort();
    chapter_ids.dedup();
    for chapter_id in chapter_ids {
        let Some((budget_id, hard_max_runs, cost_budget)) = connection
            .query_row(
                "SELECT budget_id, hard_max_model_runs, cost_budget_cents FROM chapter_production_budgets WHERE project_id = ?1 AND chapter_id = ?2",
                [&bundle.project_id, &chapter_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?
        else {
            continue;
        };
        let over_limit: i64 = connection.query_row(
            "SELECT COUNT(*) FROM budget_reservations WHERE project_id = ?1 AND budget_ref = ?2 AND status = 'OVER_LIMIT'",
            [&bundle.project_id, &budget_id],
            |row| row.get(0),
        )?;
        if over_limit > 0 {
            return Err(CommitError::BudgetHardLimit);
        }
        let run_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM model_runs AS model_run LEFT JOIN jobs ON jobs.job_id = model_run.job_id WHERE model_run.project_id = ?1 AND jobs.chapter_id = ?2",
            [&bundle.project_id, &chapter_id],
            |row| row.get(0),
        )?;
        let reserved_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM budget_reservations WHERE project_id = ?1 AND budget_ref = ?2 AND status = 'RESERVED'",
            [&bundle.project_id, &budget_id],
            |row| row.get(0),
        )?;
        if hard_max_runs > 0 && run_count + reserved_count >= hard_max_runs {
            return Err(CommitError::BudgetHardLimit);
        }
        let actual_cost: i64 = connection.query_row(
            "SELECT COALESCE(SUM(cost_cents), 0) FROM model_cost_ledger WHERE project_id = ?1 AND budget_id = ?2 AND cost_type IN ('ACTUAL', 'OVERAGE')",
            [&bundle.project_id, &budget_id],
            |row| row.get(0),
        )?;
        let reserved_cost: i64 = connection.query_row(
            "SELECT COALESCE(SUM(reserved_cents), 0) FROM budget_reservations WHERE project_id = ?1 AND budget_ref = ?2 AND status = 'RESERVED'",
            [&bundle.project_id, &budget_id],
            |row| row.get(0),
        )?;
        if cost_budget > 0 && actual_cost + reserved_cost >= cost_budget {
            return Err(CommitError::BudgetHardLimit);
        }
    }
    Ok(())
}

fn validate_receipt_draft(
    connection: &Connection,
    bundle: &CommitBundle,
) -> Result<(), CommitError> {
    let Some(draft) = &bundle.receipt_draft else {
        return Ok(());
    };
    if draft.domain.trim().is_empty() {
        return Err(invalid("receipt draft domain cannot be empty"));
    }
    for artifact in draft.artifact_refs.iter().flatten() {
        let _ = resolve_artifact(connection, bundle, artifact)?;
    }
    Ok(())
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
        if dependency_source_digest(connection, bundle, &edge.source_ref)? != edge.source_hash {
            return Err(invalid(format!(
                "dependency sourceHash mismatch: {}",
                edge.source_ref
            )));
        }
    }
    validate_knowledge(connection, bundle)?;
    validate_heads(connection, bundle)?;
    validate_gate_certificates(connection, bundle)?;
    validate_budget(connection, bundle)?;
    validate_receipt_draft(connection, bundle)?;
    Ok(())
}
