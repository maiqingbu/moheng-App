use crate::harness::SupportCheckpoint;
use crate::generated::types::DomainReceipt;
use crate::repair::{ExecutionReceipt, RepairAuthorization, RepairOperation, RepairPlan};
use libsql_rusqlite::{Connection, OptionalExtension};

#[derive(Debug, thiserror::Error)]
pub enum SupportRepositoryError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("repair execution receipt binding changed during re-verification")]
    RepairReceiptBindingChanged,
}

pub fn load_domain_receipt(
    connection: &Connection,
    project_id: &str,
    commit_receipt_id: &str,
    domain: &str,
) -> Result<Option<DomainReceipt>, SupportRepositoryError> {
    let receipt_json: Option<String> = connection
        .query_row(
            "SELECT receipt_json FROM domain_receipts WHERE project_id=?1 AND commit_receipt_id=?2 AND domain=?3",
            libsql_rusqlite::params![project_id, commit_receipt_id, domain],
            |row| row.get(0),
        )
        .optional()?;
    receipt_json
        .map(|value| serde_json::from_str(&value).map_err(Into::into))
        .transpose()
}

pub fn save_repair_plan(
    connection: &Connection,
    plan: &RepairPlan,
) -> Result<(), SupportRepositoryError> {
    if let Some(existing_digest) = connection
        .query_row(
            "SELECT operation_digest FROM repair_plans WHERE project_id=?1 AND repair_plan_id=?2",
            libsql_rusqlite::params![plan.project_id, plan.repair_plan_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        if existing_digest != plan.operation_digest {
            return Err(SupportRepositoryError::Json(serde_json::Error::io(
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "repair plan id already exists with different digest",
                ),
            )));
        }
        return Ok(());
    }
    connection.execute(
        "INSERT INTO repair_plans(repair_plan_id,project_id,based_on_project_revision,diagnosis_json,finding_refs_json,target_authority_owner,proposed_operations_json,affected_artifacts_json,affected_scopes_json,risk_level,requires_backup,expected_postconditions_json,operation_digest,status,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'PROPOSED',?14)",
        libsql_rusqlite::params![
            plan.repair_plan_id,
            plan.project_id,
            plan.based_on_project_revision,
            serde_json::to_string(&plan.diagnosis)?,
            serde_json::to_string(&plan.finding_refs)?,
            plan.target_authority_owner,
            serde_json::to_string(&plan.proposed_operations)?,
            serde_json::to_string(&plan.affected_artifacts)?,
            serde_json::to_string(&plan.affected_scopes)?,
            plan.risk_level,
            plan.requires_backup as i64,
            serde_json::to_string(&plan.expected_postconditions)?,
            plan.operation_digest,
            plan.created_at,
        ],
    )?;
    Ok(())
}

pub fn load_repair_plan(
    connection: &Connection,
    project_id: &str,
    repair_plan_id: &str,
) -> Result<Option<RepairPlan>, SupportRepositoryError> {
    let row = connection
        .query_row(
            "SELECT repair_plan_id,project_id,based_on_project_revision,diagnosis_json,finding_refs_json,target_authority_owner,proposed_operations_json,affected_artifacts_json,affected_scopes_json,risk_level,requires_backup,expected_postconditions_json,operation_digest,created_at FROM repair_plans WHERE project_id=?1 AND repair_plan_id=?2",
            libsql_rusqlite::params![project_id, repair_plan_id],
            |row| Ok((
                row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?,
                row.get::<_, String>(6)?, row.get::<_, String>(7)?, row.get::<_, String>(8)?,
                row.get::<_, String>(9)?, row.get::<_, i64>(10)?, row.get::<_, String>(11)?,
                row.get::<_, String>(12)?, row.get::<_, String>(13)?
            )),
        )
        .optional()?;
    row.map(|row| {
        Ok(RepairPlan {
            repair_plan_id: row.0,
            project_id: row.1,
            based_on_project_revision: row.2,
            diagnosis: serde_json::from_str(&row.3)?,
            finding_refs: serde_json::from_str(&row.4)?,
            target_authority_owner: row.5,
            proposed_operations: serde_json::from_str::<Vec<RepairOperation>>(&row.6)?,
            affected_artifacts: serde_json::from_str(&row.7)?,
            affected_scopes: serde_json::from_str(&row.8)?,
            risk_level: row.9,
            requires_backup: row.10 != 0,
            expected_postconditions: serde_json::from_str(&row.11)?,
            operation_digest: row.12,
            created_at: row.13,
        })
    })
    .transpose()
}

pub fn save_repair_authorization(
    connection: &Connection,
    authorization: &RepairAuthorization,
) -> Result<(), SupportRepositoryError> {
    connection.execute(
        "INSERT INTO repair_authorizations(authorization_id,project_id,repair_plan_id,based_on_project_revision,operation_digest,approved_by_user,issued_at,consumed_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        libsql_rusqlite::params![authorization.authorization_id, authorization.project_id, authorization.repair_plan_id, authorization.based_on_project_revision, authorization.operation_digest, authorization.approved_by_user as i64, authorization.issued_at, authorization.consumed_at],
    )?;
    connection.execute(
        "UPDATE repair_plans SET status='AUTHORIZED' WHERE project_id=?1 AND repair_plan_id=?2",
        libsql_rusqlite::params![authorization.project_id, authorization.repair_plan_id],
    )?;
    Ok(())
}

pub fn load_repair_authorization(
    connection: &Connection,
    project_id: &str,
    authorization_id: &str,
) -> Result<Option<RepairAuthorization>, SupportRepositoryError> {
    connection.query_row(
        "SELECT authorization_id,project_id,repair_plan_id,based_on_project_revision,operation_digest,approved_by_user,issued_at,consumed_at FROM repair_authorizations WHERE project_id=?1 AND authorization_id=?2",
        libsql_rusqlite::params![project_id, authorization_id],
        |row| Ok(RepairAuthorization {
            authorization_id: row.get(0)?, project_id: row.get(1)?, repair_plan_id: row.get(2)?,
            based_on_project_revision: row.get(3)?, operation_digest: row.get(4)?,
            approved_by_user: row.get::<_, i64>(5)? != 0, issued_at: row.get(6)?, consumed_at: row.get(7)?,
        }),
    ).optional().map_err(Into::into)
}

pub fn claim_repair_authorization(
    connection: &Connection,
    project_id: &str,
    authorization_id: &str,
    expected_revision: i64,
    expected_digest: &str,
    now: &str,
) -> Result<bool, SupportRepositoryError> {
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        let revision: i64 = connection.query_row(
            "SELECT revision FROM project_meta WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )?;
        if revision != expected_revision {
            return Ok(false);
        }
        let updated = connection.execute(
            "UPDATE repair_authorizations SET consumed_at=?1 WHERE project_id=?2 AND authorization_id=?3 AND based_on_project_revision=?4 AND operation_digest=?5 AND approved_by_user=1 AND consumed_at IS NULL",
            libsql_rusqlite::params![now, project_id, authorization_id, expected_revision, expected_digest],
        )?;
        Ok(updated == 1)
    })();
    match result {
        Ok(value) => {
            connection.execute_batch("COMMIT")?;
            Ok(value)
        }
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

pub fn save_repair_execution_receipt(
    connection: &Connection,
    receipt: &ExecutionReceipt,
) -> Result<(), SupportRepositoryError> {
    connection.execute(
        "INSERT INTO repair_execution_receipts(receipt_ref,repair_plan_id,authorization_id,project_id,before_revision,after_revision,operation_digest,owner,owner_receipt_ref,executed_at,status,postconditions_json) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        libsql_rusqlite::params![receipt.receipt_ref, receipt.repair_plan_id, receipt.authorization_id, receipt.project_id, receipt.before_revision, receipt.after_revision, receipt.operation_digest, receipt.owner, receipt.owner_receipt_ref, receipt.executed_at, receipt.status, serde_json::to_string(&receipt.postconditions)?],
    )?;
    connection.execute(
        "UPDATE repair_plans SET status=?1 WHERE project_id=?2 AND repair_plan_id=?3",
        libsql_rusqlite::params![receipt.status, receipt.project_id, receipt.repair_plan_id],
    )?;
    Ok(())
}

pub fn load_repair_execution_receipt(
    connection: &Connection,
    project_id: &str,
    authorization_id: &str,
) -> Result<Option<ExecutionReceipt>, SupportRepositoryError> {
    let row = connection
        .query_row(
            "SELECT receipt_ref,repair_plan_id,authorization_id,project_id,before_revision,after_revision,operation_digest,owner,owner_receipt_ref,executed_at,status,postconditions_json FROM repair_execution_receipts WHERE project_id=?1 AND authorization_id=?2",
            libsql_rusqlite::params![project_id, authorization_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                ))
            },
        )
        .optional()?;
    row.map(
        |(
            receipt_ref,
            repair_plan_id,
            authorization_id,
            project_id,
            before_revision,
            after_revision,
            operation_digest,
            owner,
            owner_receipt_ref,
            executed_at,
            status,
            postconditions_json,
        )| {
            Ok(ExecutionReceipt {
                receipt_ref,
                repair_plan_id,
                authorization_id,
                project_id,
                before_revision,
                after_revision,
                operation_digest,
                owner,
                owner_receipt_ref,
                executed_at,
                status,
                postconditions: serde_json::from_str(&postconditions_json)?,
            })
        },
    )
    .transpose()
}

pub fn update_repair_execution_receipt(
    connection: &Connection,
    receipt: &ExecutionReceipt,
) -> Result<(), SupportRepositoryError> {
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        let changed = connection.execute(
            "UPDATE repair_execution_receipts SET status=?1,postconditions_json=?2 WHERE receipt_ref=?3 AND repair_plan_id=?4 AND authorization_id=?5 AND project_id=?6 AND before_revision=?7 AND after_revision=?8 AND operation_digest=?9 AND owner=?10 AND owner_receipt_ref IS ?11 AND executed_at=?12 AND status='REPAIR_VERIFICATION_INCOMPLETE'",
            libsql_rusqlite::params![
                receipt.status,
                serde_json::to_string(&receipt.postconditions)?,
                receipt.receipt_ref,
                receipt.repair_plan_id,
                receipt.authorization_id,
                receipt.project_id,
                receipt.before_revision,
                receipt.after_revision,
                receipt.operation_digest,
                receipt.owner,
                receipt.owner_receipt_ref,
                receipt.executed_at,
            ],
        )?;
        if changed != 1 {
            return Err(SupportRepositoryError::RepairReceiptBindingChanged);
        }
        let plan_changed = connection.execute(
            "UPDATE repair_plans SET status=?1 WHERE project_id=?2 AND repair_plan_id=?3 AND operation_digest=?4",
            libsql_rusqlite::params![
                receipt.status,
                receipt.project_id,
                receipt.repair_plan_id,
                receipt.operation_digest
            ],
        )?;
        if plan_changed != 1 {
            return Err(SupportRepositoryError::RepairReceiptBindingChanged);
        }
        Ok(())
    })();
    match result {
        Ok(()) => connection.execute_batch("COMMIT")?,
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    Ok(())
}

pub fn save_checkpoint(
    connection: &Connection,
    checkpoint: &SupportCheckpoint,
) -> Result<(), SupportRepositoryError> {
    connection.execute(
        "INSERT INTO support_checkpoints(checkpoint_id,session_id,project_id,project_revision,question_digest,plan_digest,completed_step_ids_json,capability_spec_digests_json,evidence_refs_json,last_result_digests_json,requested_mode,requested_scope,plan_steps_json,pending_authorization_ref,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) ON CONFLICT(checkpoint_id) DO UPDATE SET completed_step_ids_json=excluded.completed_step_ids_json, capability_spec_digests_json=excluded.capability_spec_digests_json, evidence_refs_json=excluded.evidence_refs_json, last_result_digests_json=excluded.last_result_digests_json, requested_mode=excluded.requested_mode, requested_scope=excluded.requested_scope, plan_steps_json=excluded.plan_steps_json, pending_authorization_ref=excluded.pending_authorization_ref, created_at=excluded.created_at",
        libsql_rusqlite::params![
            checkpoint.checkpoint_id,
            checkpoint.session_id,
            checkpoint.project_id,
            checkpoint.project_revision,
            checkpoint.question_digest,
            checkpoint.plan_digest,
            serde_json::to_string(&checkpoint.completed_step_ids)?,
            serde_json::to_string(&checkpoint.capability_spec_digests)?,
            serde_json::to_string(&checkpoint.evidence_refs)?,
            serde_json::to_string(&checkpoint.last_result_digests)?,
            checkpoint.requested_mode,
            checkpoint.requested_scope,
            serde_json::to_string(&checkpoint.plan_steps)?,
            checkpoint.pending_authorization_ref,
            checkpoint.created_at,
        ],
    )?;
    Ok(())
}

pub fn load_checkpoint(
    connection: &Connection,
    project_id: &str,
    checkpoint_id: &str,
) -> Result<Option<SupportCheckpoint>, SupportRepositoryError> {
    let row = connection
        .query_row(
            "SELECT checkpoint_id,session_id,project_id,project_revision,question_digest,plan_digest,completed_step_ids_json,capability_spec_digests_json,evidence_refs_json,last_result_digests_json,requested_mode,requested_scope,plan_steps_json,pending_authorization_ref,created_at FROM support_checkpoints WHERE project_id=?1 AND checkpoint_id=?2",
            [project_id, checkpoint_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, String>(14)?,
                ))
            },
        )
        .optional()?;
    row.map(
        |(
            checkpoint_id,
            session_id,
            project_id,
            project_revision,
            question_digest,
            plan_digest,
            completed_step_ids,
            capability_spec_digests,
            evidence_refs,
            last_result_digests,
            requested_mode,
            requested_scope,
            plan_steps,
            pending_authorization_ref,
            created_at,
        )| {
            Ok(SupportCheckpoint {
                checkpoint_id,
                session_id,
                project_id,
                project_revision,
                question_digest,
                plan_digest,
                completed_step_ids: serde_json::from_str(&completed_step_ids)?,
                capability_spec_digests: serde_json::from_str(&capability_spec_digests)?,
                evidence_refs: serde_json::from_str(&evidence_refs)?,
                last_result_digests: serde_json::from_str(&last_result_digests)?,
                requested_mode,
                requested_scope,
                plan_steps: serde_json::from_str(&plan_steps)?,
                pending_authorization_ref,
                created_at,
            })
        },
    )
    .transpose()
}
