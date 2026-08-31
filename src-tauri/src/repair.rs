//! H03 authorized repair orchestration.
//!
//! SQL persistence belongs to `repositories::support`; this module owns only
//! contracts, authorization semantics, typed owner dispatch and post-verify.

use crate::hash::sha256_canonical;
use crate::repositories::support;
use libsql_rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepairOperation {
    pub operation: String,
    pub target: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub postconditions: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepairPlan {
    pub repair_plan_id: String,
    pub project_id: String,
    pub based_on_project_revision: i64,
    pub diagnosis: Value,
    pub finding_refs: Vec<String>,
    pub target_authority_owner: String,
    pub proposed_operations: Vec<RepairOperation>,
    pub affected_artifacts: Vec<String>,
    pub affected_scopes: Vec<String>,
    pub risk_level: String,
    pub requires_backup: bool,
    pub expected_postconditions: Vec<Value>,
    pub operation_digest: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepairAuthorization {
    pub authorization_id: String,
    pub project_id: String,
    pub repair_plan_id: String,
    pub based_on_project_revision: i64,
    pub operation_digest: String,
    pub approved_by_user: bool,
    pub issued_at: String,
    pub consumed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionReceipt {
    pub receipt_ref: String,
    pub repair_plan_id: String,
    pub authorization_id: String,
    pub project_id: String,
    pub before_revision: i64,
    pub after_revision: i64,
    pub operation_digest: String,
    pub owner: String,
    pub owner_receipt_ref: Option<String>,
    pub executed_at: String,
    pub status: String,
    pub postconditions: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerExecution {
    pub owner: String,
    pub owner_receipt_ref: Option<String>,
}

pub trait TypedRepairOwner {
    fn execute(&self, operation: &RepairOperation) -> Result<OwnerExecution, String>;
    fn verify(&self, operation: &RepairOperation) -> Result<Value, String>;
}

#[derive(Debug, thiserror::Error)]
pub enum RepairError {
    #[error("H03_INVALID:{0}")]
    Invalid(String),
    #[error("H03_STALE_AUTHORIZATION")]
    StaleAuthorization,
    #[error("H03_AUTHORIZATION_REQUIRED")]
    AuthorizationRequired,
    #[error("H03_REPAIR_ALREADY_CONSUMED")]
    AlreadyConsumed,
    #[error("H03_CAPABILITY_UNAVAILABLE_BEFORE_B09")]
    CapabilityUnavailable,
    #[error("H03_CHANGE_REQUEST_PROPOSAL_ONLY")]
    ProposalOnly,
    #[error("repository: {0}")]
    Repository(#[from] support::SupportRepositoryError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn operation_digest(operations: &[RepairOperation]) -> Result<String, RepairError> {
    operation_digest_with_postconditions(operations, &[])
}

pub fn operation_digest_with_postconditions(
    operations: &[RepairOperation],
    expected_postconditions: &[Value],
) -> Result<String, RepairError> {
    sha256_canonical(&json!({
        "operations": operations,
        "expectedPostconditions": expected_postconditions,
    }))
    .map_err(|error| RepairError::Invalid(error.to_string()))
}

pub fn validate_plan(plan: &RepairPlan) -> Result<(), RepairError> {
    if plan.repair_plan_id.trim().is_empty() || plan.project_id.trim().is_empty() {
        return Err(RepairError::Invalid("plan identity".into()));
    }
    if plan.based_on_project_revision < 0 || plan.proposed_operations.is_empty() {
        return Err(RepairError::Invalid("plan revision or operations".into()));
    }
    if operation_digest_with_postconditions(
        &plan.proposed_operations,
        &plan.expected_postconditions,
    )? != plan.operation_digest
    {
        return Err(RepairError::Invalid("operationDigest mismatch".into()));
    }
    for operation in &plan.proposed_operations {
        if !matches!(
            operation.operation.as_str(),
            "rebuild_projection"
                | "rebuild_fts"
                | "refresh_runtime_projection"
                | "recompile_chapter_plan"
                | "backup_restore_maintenance"
                | "replan_volume_route"
                | "execute_change_request"
        ) {
            return Err(RepairError::Invalid(format!(
                "unknown operation {}",
                operation.operation
            )));
        }
    }
    if plan.expected_postconditions.len() != plan.proposed_operations.len() {
        return Err(RepairError::Invalid(
            "every operation requires exactly one expectedPostcondition".into(),
        ));
    }
    for expected in &plan.expected_postconditions {
        let typed = expected
            .as_object()
            .and_then(|value| value.get("kind"))
            .and_then(Value::as_str)
            .is_some_and(|kind| !kind.trim().is_empty())
            && expected.get("expected").is_some();
        if !typed {
            return Err(RepairError::Invalid(
                "expectedPostcondition requires typed kind and expected".into(),
            ));
        }
    }
    Ok(())
}

pub fn save_plan(connection: &Connection, plan: &RepairPlan) -> Result<(), RepairError> {
    validate_plan(plan)?;
    support::save_repair_plan(connection, plan)?;
    Ok(())
}

pub fn issue_authorization(
    connection: &Connection,
    authorization: &RepairAuthorization,
) -> Result<(), RepairError> {
    if !authorization.approved_by_user {
        return Err(RepairError::AuthorizationRequired);
    }
    if authorization.consumed_at.is_some() {
        return Err(RepairError::AlreadyConsumed);
    }
    let plan = support::load_repair_plan(
        connection,
        &authorization.project_id,
        &authorization.repair_plan_id,
    )?
    .ok_or_else(|| RepairError::Invalid("repair plan not found".into()))?;
    let revision = crate::repositories::narrative::NarrativeRepository::project_revision(
        connection,
        &authorization.project_id,
    )
    .map_err(|error| RepairError::Invalid(error.to_string()))?;
    if plan.operation_digest != authorization.operation_digest
        || plan.based_on_project_revision != authorization.based_on_project_revision
        || revision != authorization.based_on_project_revision
    {
        return Err(RepairError::StaleAuthorization);
    }
    support::save_repair_authorization(connection, authorization)?;
    Ok(())
}

fn stage_gate(operation: &RepairOperation) -> Result<(), RepairError> {
    match operation.operation.as_str() {
        "replan_volume_route" => Err(RepairError::CapabilityUnavailable),
        "execute_change_request" => Err(RepairError::ProposalOnly),
        _ => Ok(()),
    }
}

pub fn execute(
    connection: &Connection,
    owner: &dyn TypedRepairOwner,
    project_id: &str,
    repair_plan_id: &str,
    authorization_id: &str,
    now: &str,
) -> Result<ExecutionReceipt, RepairError> {
    let plan = support::load_repair_plan(connection, project_id, repair_plan_id)?
        .ok_or_else(|| RepairError::Invalid("repair plan not found".into()))?;
    validate_plan(&plan)?;
    let authorization =
        support::load_repair_authorization(connection, project_id, authorization_id)?
            .ok_or(RepairError::AuthorizationRequired)?;
    if authorization.repair_plan_id != repair_plan_id
        || authorization.operation_digest != plan.operation_digest
        || authorization.based_on_project_revision != plan.based_on_project_revision
    {
        return Err(RepairError::StaleAuthorization);
    }
    if authorization.consumed_at.is_some() {
        return Err(RepairError::AlreadyConsumed);
    }
    for operation in &plan.proposed_operations {
        stage_gate(operation)?;
    }

    let claimed = support::claim_repair_authorization(
        connection,
        project_id,
        authorization_id,
        plan.based_on_project_revision,
        &plan.operation_digest,
        now,
    )?;
    if !claimed {
        let refreshed =
            support::load_repair_authorization(connection, project_id, authorization_id)?;
        return Err(
            if refreshed.is_some_and(|item| item.consumed_at.is_some()) {
                RepairError::AlreadyConsumed
            } else {
                RepairError::StaleAuthorization
            },
        );
    }

    let mut owners = Vec::new();
    let mut owner_receipts = Vec::new();
    let mut postconditions = Vec::new();
    for operation in &plan.proposed_operations {
        match owner.execute(operation) {
            Ok(execution) => {
                owners.push(execution.owner);
                if let Some(receipt) = execution.owner_receipt_ref {
                    owner_receipts.push(receipt);
                }
            }
            Err(error) => postconditions.push(json!({
                "operation":operation.operation,
                "status":"OWNER_EXECUTION_FAILED",
                "error":error
            })),
        }
    }
    if postconditions.is_empty() {
        for (operation, expected) in plan
            .proposed_operations
            .iter()
            .zip(plan.expected_postconditions.iter())
        {
            match owner.verify(operation) {
                Ok(mut value) => {
                    let matches = value.get("kind") == expected.get("kind")
                        && value.get("observed") == expected.get("expected")
                        && value.get("status").and_then(Value::as_str) == Some("PASS");
                    value["expectedPostcondition"] = expected.clone();
                    value["typedRequery"] = Value::Bool(true);
                    if !matches {
                        value["status"] = Value::String("FAIL".into());
                    }
                    postconditions.push(value)
                }
                Err(error) => postconditions.push(json!({
                    "operation":operation.operation,
                    "status":"REPAIR_VERIFICATION_INCOMPLETE",
                    "error":error
                })),
            }
        }
    }
    let status = if postconditions
        .iter()
        .all(|item| item.get("status").and_then(Value::as_str) == Some("PASS"))
    {
        "REPAIR_VERIFIED"
    } else if postconditions.iter().any(|item| {
        item.get("status").and_then(Value::as_str) == Some("REPAIR_VERIFICATION_INCOMPLETE")
    }) {
        "REPAIR_VERIFICATION_INCOMPLETE"
    } else {
        "REPAIR_VERIFICATION_FAILED"
    };
    let after_revision = crate::repositories::narrative::NarrativeRepository::project_revision(
        connection, project_id,
    )
    .map_err(|error| RepairError::Invalid(error.to_string()))?;
    let receipt = ExecutionReceipt {
        receipt_ref: format!("repair-receipt:{authorization_id}"),
        repair_plan_id: repair_plan_id.into(),
        authorization_id: authorization_id.into(),
        project_id: project_id.into(),
        before_revision: plan.based_on_project_revision,
        after_revision,
        operation_digest: plan.operation_digest,
        owner: owners.join(","),
        owner_receipt_ref: (!owner_receipts.is_empty()).then(|| owner_receipts.join(",")),
        executed_at: now.into(),
        status: status.into(),
        postconditions,
    };
    support::save_repair_execution_receipt(connection, &receipt)?;
    Ok(receipt)
}
