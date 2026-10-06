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

pub struct RepairVerificationContext<'a> {
    pub project_id: &'a str,
    pub before_revision: i64,
    pub after_revision: i64,
    pub expected_postcondition: &'a Value,
    pub owner_receipt_ref: Option<&'a str>,
}

pub trait TypedRepairOwner {
    fn execute(&self, operation: &RepairOperation) -> Result<OwnerExecution, String>;
    fn verify(&self, operation: &RepairOperation) -> Result<Value, String>;

    fn verify_with_context(
        &self,
        operation: &RepairOperation,
        _context: &RepairVerificationContext<'_>,
    ) -> Result<Value, String> {
        self.verify(operation)
    }
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
                | "rebuild_planning_state_projection"
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

pub fn generate_plan_from_findings(
    connection: &Connection,
    project_id: &str,
    finding_refs: &[String],
    requested_operation_kinds: &[String],
    now: &str,
) -> Result<RepairPlan, String> {
    if finding_refs.is_empty() || requested_operation_kinds.len() != 1 {
        return Err("H03_INVALID:findingRefs and one requestedOperationKind are required".into());
    }
    let question = match requested_operation_kinds[0].as_str() {
        "rebuild_projection" => "修复当前页面数据展示。",
        "rebuild_fts" => "修复全文索引。",
        "recompile_chapter_plan" => "修复章节规划。",
        "replan_volume_route" => "修复当前卷路线。",
        _ => return Err("H03_INVALID:requestedOperationKinds".into()),
    };
    let diagnosis =
        crate::project_entry::assistant_repair_plan(connection, project_id, question, now)?;
    if diagnosis.get("action").and_then(Value::as_str) != Some("EXECUTABLE") {
        return Err("H03_OPERATION_NOT_EXECUTABLE".into());
    }
    let generated_refs = diagnosis
        .get("findingRefs")
        .and_then(Value::as_array)
        .ok_or_else(|| "H03_DIAGNOSIS_FINDINGS_MISSING".to_string())?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| "H03_DIAGNOSIS_FINDING_INVALID".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let generated_kinds = diagnosis
        .get("proposedOperations")
        .and_then(Value::as_array)
        .ok_or_else(|| "H03_DIAGNOSIS_OPERATIONS_MISSING".to_string())?
        .iter()
        .map(|operation| {
            operation
                .get("operation")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .ok_or_else(|| "H03_DIAGNOSIS_OPERATION_INVALID".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut requested_refs = finding_refs.to_vec();
    requested_refs.sort();
    requested_refs.dedup();
    let mut current_refs = generated_refs.clone();
    current_refs.sort();
    current_refs.dedup();
    if requested_refs != current_refs || generated_kinds != requested_operation_kinds {
        return Err("H03_FINDING_OR_OPERATION_MISMATCH".into());
    }
    let plan: RepairPlan = serde_json::from_value(diagnosis)
        .map_err(|error| format!("H03_GENERATED_PLAN_INVALID:{error}"))?;
    validate_plan(&plan).map_err(|error| error.to_string())?;
    Ok(plan)
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
        // B09 promotes route replan from the H03 placeholder to the typed
        // lifecycle owner. Authorization is still required by `execute`;
        // this gate only rejects operations that belong to a later batch.
        "replan_volume_route" | "rebuild_planning_state_projection" => Ok(()),
        "execute_change_request" => {
            let b12_bound = operation
                .params
                .get("changeRequestId")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
                && operation
                    .params
                    .get("impactDigest")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value.len() == 64);
            if b12_bound {
                Ok(())
            } else {
                Err(RepairError::ProposalOnly)
            }
        }
        _ => Ok(()),
    }
}

fn verify_operations(
    owner: &dyn TypedRepairOwner,
    project_id: &str,
    plan: &RepairPlan,
    before_revision: i64,
    after_revision: i64,
    owner_receipt_refs: &[Option<String>],
) -> Vec<Value> {
    plan.proposed_operations
        .iter()
        .zip(plan.expected_postconditions.iter())
        .enumerate()
        .map(|(index, (operation, expected))| {
            let owner_receipt_ref = owner_receipt_refs.get(index).and_then(Option::as_deref);
            let context = RepairVerificationContext {
                project_id,
                before_revision,
                after_revision,
                expected_postcondition: expected,
                owner_receipt_ref,
            };
            match owner.verify_with_context(operation, &context) {
                Ok(mut value) => {
                    let status = value.get("status").and_then(Value::as_str);
                    let incomplete = matches!(
                        status,
                        Some(
                            "INCOMPLETE"
                                | "QUERY_FAILED"
                                | "UNKNOWN"
                                | "REPAIR_VERIFICATION_INCOMPLETE"
                        )
                    );
                    let matches = value.get("kind") == expected.get("kind")
                        && value.get("observed") == expected.get("expected")
                        && status == Some("PASS");
                    value["expectedPostcondition"] = expected.clone();
                    value["typedRequery"] = Value::Bool(true);
                    value["ownerReceiptRef"] = owner_receipt_ref
                        .map(|receipt| json!(receipt))
                        .unwrap_or(Value::Null);
                    if incomplete {
                        value["status"] = json!("REPAIR_VERIFICATION_INCOMPLETE");
                    } else if !matches {
                        value["status"] = json!("FAIL");
                    }
                    value
                }
                Err(error) => json!({
                    "operation":operation.operation,
                    "status":"REPAIR_VERIFICATION_INCOMPLETE",
                    "error":error,
                    "expectedPostcondition":expected,
                    "typedRequery":true,
                    "ownerReceiptRef":owner_receipt_ref
                }),
            }
        })
        .collect()
}

fn aggregate_receipt_status(postconditions: &[Value]) -> &'static str {
    if postconditions
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
    }
}

fn reverify_existing_receipt(
    connection: &Connection,
    owner: &dyn TypedRepairOwner,
    project_id: &str,
    plan: &RepairPlan,
    receipt: ExecutionReceipt,
) -> Result<ExecutionReceipt, RepairError> {
    let mut owner_receipt_refs = plan
        .proposed_operations
        .iter()
        .enumerate()
        .map(|(index, _)| {
            receipt
                .postconditions
                .get(index)
                .and_then(|item| item.get("ownerReceiptRef"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect::<Vec<_>>();
    if owner_receipt_refs.len() == 1 && owner_receipt_refs[0].is_none() {
        owner_receipt_refs[0] = receipt.owner_receipt_ref.clone();
    }
    let postconditions = verify_operations(
        owner,
        project_id,
        plan,
        receipt.before_revision,
        receipt.after_revision,
        &owner_receipt_refs,
    );
    let updated = ExecutionReceipt {
        status: aggregate_receipt_status(&postconditions).into(),
        postconditions,
        ..receipt
    };
    match support::update_repair_execution_receipt(connection, &updated) {
        Ok(()) => Ok(updated),
        Err(support::SupportRepositoryError::RepairReceiptBindingChanged) => {
            let latest = support::load_repair_execution_receipt(
                connection,
                project_id,
                &updated.authorization_id,
            )?
            .ok_or(RepairError::AlreadyConsumed)?;
            if latest.status == "REPAIR_VERIFIED"
                && latest.receipt_ref == updated.receipt_ref
                && latest.repair_plan_id == updated.repair_plan_id
                && latest.authorization_id == updated.authorization_id
                && latest.project_id == updated.project_id
                && latest.before_revision == updated.before_revision
                && latest.after_revision == updated.after_revision
                && latest.operation_digest == updated.operation_digest
                && latest.owner_receipt_ref == updated.owner_receipt_ref
            {
                Ok(latest)
            } else {
                Err(RepairError::AlreadyConsumed)
            }
        }
        Err(error) => Err(error.into()),
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
    for operation in &plan.proposed_operations {
        stage_gate(operation)?;
    }
    if authorization.consumed_at.is_some() {
        let Some(receipt) =
            support::load_repair_execution_receipt(connection, project_id, authorization_id)?
        else {
            return Err(RepairError::AlreadyConsumed);
        };
        if receipt.repair_plan_id != repair_plan_id
            || receipt.authorization_id != authorization_id
            || receipt.project_id != project_id
            || receipt.operation_digest != plan.operation_digest
            || receipt.before_revision != plan.based_on_project_revision
        {
            return Err(RepairError::StaleAuthorization);
        }
        if receipt.status == "REPAIR_VERIFICATION_INCOMPLETE" {
            return reverify_existing_receipt(connection, owner, project_id, &plan, receipt);
        }
        return Err(RepairError::AlreadyConsumed);
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
    let mut owner_receipts_by_operation = Vec::with_capacity(plan.proposed_operations.len());
    let mut postconditions = Vec::new();
    for operation in &plan.proposed_operations {
        match owner.execute(operation) {
            Ok(execution) => {
                owners.push(execution.owner);
                if let Some(receipt) = execution.owner_receipt_ref.as_ref() {
                    owner_receipts.push(receipt.clone());
                }
                owner_receipts_by_operation.push(execution.owner_receipt_ref);
            }
            Err(error) => {
                owner_receipts_by_operation.push(None);
                postconditions.push(json!({
                    "operation":operation.operation,
                    "status":"OWNER_EXECUTION_FAILED",
                    "error":error
                }));
            }
        }
    }
    let mut after_revision = crate::repositories::narrative::NarrativeRepository::project_revision(
        connection, project_id,
    )
    .map_err(|error| RepairError::Invalid(error.to_string()))?;
    if postconditions.is_empty() {
        let revision_at_verify = after_revision;
        postconditions = verify_operations(
            owner,
            project_id,
            &plan,
            plan.based_on_project_revision,
            revision_at_verify,
            &owner_receipts_by_operation,
        );
        after_revision = crate::repositories::narrative::NarrativeRepository::project_revision(
            connection, project_id,
        )
        .map_err(|error| RepairError::Invalid(error.to_string()))?;
        if after_revision != revision_at_verify {
            postconditions.push(json!({
                "status":"REPAIR_VERIFICATION_INCOMPLETE",
                "reason":"PROJECT_REVISION_CHANGED_DURING_POSTVERIFY",
                "verifiedAtRevision":revision_at_verify,
                "observedRevision":after_revision
            }));
        }
    }
    let status = aggregate_receipt_status(&postconditions);
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
