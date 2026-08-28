use crate::hash::sha256_canonical;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const READ_ONLY_STAGE: &str = "H01_READ_DIAGNOSE";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectScope {
    pub project_id: String,
    pub project_revision: i64,
    pub question_digest: String,
    pub requested_mode: String,
    pub requested_scope: String,
}

pub fn resolve_project_scope(
    project_id: &str,
    project_revision: i64,
    question: &str,
    mode: &str,
    scope: &str,
) -> Result<ProjectScope, String> {
    if project_id.trim().is_empty() {
        return Err("PROJECT_SCOPE_INVALID:projectId".into());
    }
    if project_revision < 0 {
        return Err("PROJECT_SCOPE_INVALID:projectRevision".into());
    }
    if mode != "INSPECT" && mode != "DIAGNOSE" {
        return Err("PROJECT_SCOPE_INVALID:mode".into());
    }
    if scope.trim().is_empty() {
        return Err("PROJECT_SCOPE_INVALID:scope".into());
    }
    Ok(ProjectScope {
        project_id: project_id.into(),
        project_revision,
        question_digest: sha256_canonical(&json!(question)).map_err(|e| e.to_string())?,
        requested_mode: mode.into(),
        requested_scope: scope.into(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySpec {
    pub capability_id: String,
    pub version: String,
    pub spec_digest: String,
    pub modes: Vec<String>,
    pub stage: String,
    pub owner: String,
    pub read_or_write: String,
    pub input_contract: String,
    pub output_contract: String,
    pub hard_bounds: BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityRegistry {
    pub specs: BTreeMap<String, CapabilitySpec>,
}

impl CapabilityRegistry {
    pub fn read_only_r1() -> Self {
        let ids = [
            "query_project_meta",
            "query_canon",
            "query_relations",
            "query_knowledge",
            "query_events",
            "query_evidence",
            "query_obligations",
            "query_foreshadows",
            "query_mysteries",
            "query_task_states",
            "query_dependency_edges",
            "query_model_runs",
            "query_commit_receipts",
            "query_integrity_status",
            "query_continuity",
            "query_volume_progress",
            "query_context_manifest",
            "query_plan_stale",
            "diagnose_canon_evidence",
            "diagnose_knowledge_event",
            "diagnose_thread_evidence",
            "diagnose_task_evidence",
            "diagnose_projection_authority",
        ];
        let mut specs = BTreeMap::new();
        for id in ids {
            let mut hard_bounds = BTreeMap::new();
            hard_bounds.insert("maxResults".into(), 100);
            hard_bounds.insert("maxTokens".into(), 4096);
            let digest = sha256_canonical(&json!({"capabilityId":id,"version":"1.0.0","modes":["INSPECT","DIAGNOSE"],"stage":READ_ONLY_STAGE,"owner":"Rust","readOrWrite":"READ","inputContract":"typed-json","outputContract":"bounded-result","hardBounds":hard_bounds})).unwrap();
            specs.insert(
                id.into(),
                CapabilitySpec {
                    capability_id: id.into(),
                    version: "1.0.0".into(),
                    spec_digest: digest,
                    modes: vec!["INSPECT".into(), "DIAGNOSE".into()],
                    stage: READ_ONLY_STAGE.into(),
                    owner: "Rust".into(),
                    read_or_write: "READ".into(),
                    input_contract: "typed-json".into(),
                    output_contract: "bounded-result".into(),
                    hard_bounds,
                },
            );
        }
        Self { specs }
    }

    pub fn digest(&self) -> Result<String, String> {
        sha256_canonical(&serde_json::to_value(&self.specs).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }
    pub fn exact(
        &self,
        id: &str,
        version: &str,
        digest: &str,
        mode: &str,
    ) -> Result<&CapabilitySpec, String> {
        let spec = self
            .specs
            .get(id)
            .ok_or_else(|| "CAPABILITY_UNKNOWN".to_string())?;
        if spec.version != version {
            return Err("CAPABILITY_VERSION_MISMATCH".into());
        }
        if spec.spec_digest != digest {
            return Err("CAPABILITY_SPEC_DIGEST_MISMATCH".into());
        }
        if !spec.modes.iter().any(|candidate| candidate == mode) {
            return Err("CAPABILITY_MODE_FORBIDDEN".into());
        }
        if spec.stage != READ_ONLY_STAGE || spec.read_or_write != "READ" {
            return Err("CAPABILITY_STAGE_FORBIDDEN".into());
        }
        Ok(spec)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestedBounds {
    pub max_results: usize,
    pub max_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityCallEnvelope {
    pub call_id: String,
    pub capability_id: String,
    pub capability_version: String,
    pub spec_digest: String,
    pub mode: String,
    pub project_id: String,
    pub project_revision: i64,
    pub input: Value,
    pub input_digest: String,
    pub requested_bounds: RequestedBounds,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityResultEnvelope {
    pub call_id: String,
    pub status: String,
    pub project_id: String,
    pub project_revision: i64,
    pub source_refs: Vec<String>,
    pub result: Value,
    pub result_digest: String,
    pub bounds_applied: RequestedBounds,
    pub truncated: bool,
    pub receipt_ref: String,
}

pub trait ReadCapabilityAdapter {
    fn invoke(
        &self,
        capability_id: &str,
        input: &Value,
        bounds: &RequestedBounds,
    ) -> Result<(Value, Vec<String>, bool), String>;
}

pub struct BoundedCapabilityExecutor {
    pub registry: CapabilityRegistry,
}

impl BoundedCapabilityExecutor {
    pub fn execute(
        &self,
        adapter: &dyn ReadCapabilityAdapter,
        call: &CapabilityCallEnvelope,
    ) -> Result<CapabilityResultEnvelope, String> {
        let spec = self.registry.exact(
            &call.capability_id,
            &call.capability_version,
            &call.spec_digest,
            &call.mode,
        )?;
        if call.project_id.trim().is_empty() || call.project_revision < 0 {
            return Err("PROJECT_SCOPE_INVALID".into());
        }
        let input_digest = sha256_canonical(&call.input).map_err(|e| e.to_string())?;
        if input_digest != call.input_digest {
            return Err("CAPABILITY_INPUT_DIGEST_MISMATCH".into());
        }
        if call.requested_bounds.max_results
            > *spec.hard_bounds.get("maxResults").unwrap_or(&0) as usize
            || call.requested_bounds.max_tokens
                > *spec.hard_bounds.get("maxTokens").unwrap_or(&0) as usize
        {
            return Err("CAPABILITY_BOUNDS_ESCALATION".into());
        }
        let (result, source_refs, truncated) =
            adapter.invoke(&call.capability_id, &call.input, &call.requested_bounds)?;
        let result_digest = sha256_canonical(&result).map_err(|e| e.to_string())?;
        Ok(CapabilityResultEnvelope {
            call_id: call.call_id.clone(),
            status: "PASS".into(),
            project_id: call.project_id.clone(),
            project_revision: call.project_revision,
            source_refs,
            result,
            result_digest,
            bounds_applied: call.requested_bounds.clone(),
            truncated,
            receipt_ref: format!("support-call:{}", call.call_id),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SupportStepStatus {
    PENDING,
    RUNNING,
    DONE,
    BLOCKED,
    SKIPPED,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SupportTaskStep {
    pub step_id: String,
    pub capability_id: String,
    pub status: SupportStepStatus,
    pub typed_args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SupportTaskPlan {
    pub plan_id: String,
    pub project_id: String,
    pub based_on_project_revision: i64,
    pub question_digest: String,
    pub steps: Vec<SupportTaskStep>,
    pub status: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replan_trigger: Option<ReplanTrigger>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_plan_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_plan_digest: Option<String>,
}

pub fn plan_support(
    scope: &ProjectScope,
    question: &str,
    created_at: &str,
) -> Result<SupportTaskPlan, String> {
    let lower = question.to_lowercase();
    let capabilities = if lower.contains("知道") || lower.contains("knowledge") {
        vec!["query_knowledge", "query_events", "query_evidence"]
    } else if lower.contains("承诺") || lower.contains("promise") {
        vec!["query_continuity", "query_events", "query_evidence"]
    } else {
        vec!["query_project_meta", "query_context_manifest"]
    };
    let steps = capabilities
        .into_iter()
        .enumerate()
        .map(|(index, capability_id)| SupportTaskStep {
            step_id: format!("step:{}", index + 1),
            capability_id: capability_id.into(),
            status: SupportStepStatus::PENDING,
            typed_args: json!({"scope":scope.requested_scope}),
        })
        .collect::<Vec<_>>();
    if steps.len() > 7 {
        return Err("SUPPORT_PLAN_BUDGET_EXCEEDED".into());
    }
    Ok(SupportTaskPlan {
        plan_id: format!("plan:{}", &scope.question_digest[..16]),
        project_id: scope.project_id.clone(),
        based_on_project_revision: scope.project_revision,
        question_digest: scope.question_digest.clone(),
        steps,
        status: "READY".into(),
        created_at: created_at.into(),
        replan_trigger: None,
        prior_plan_digest: None,
        new_plan_digest: None,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SupportStatus {
    SUPPORTED,
    CONFLICTING,
    INSUFFICIENT,
    INFERRED,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub finding_id: String,
    pub diagnosis_type: String,
    pub project_id: String,
    pub project_revision: i64,
    pub claim: String,
    pub support_status: SupportStatus,
    pub source_refs: Vec<String>,
    pub conflicting_refs: Vec<String>,
    pub impact_scopes: Vec<String>,
    pub recommended_next_action: String,
}

pub fn grounded_finding(
    scope: &ProjectScope,
    diagnosis_type: &str,
    claim: &str,
    source_refs: Vec<String>,
    conflicting_refs: Vec<String>,
) -> Finding {
    let support_status = if !conflicting_refs.is_empty() {
        SupportStatus::CONFLICTING
    } else if source_refs.is_empty() {
        SupportStatus::INSUFFICIENT
    } else {
        SupportStatus::SUPPORTED
    };
    Finding {
        finding_id: format!("finding:{}", &scope.question_digest[..16]),
        diagnosis_type: diagnosis_type.into(),
        project_id: scope.project_id.clone(),
        project_revision: scope.project_revision,
        claim: claim.into(),
        support_status,
        source_refs,
        conflicting_refs,
        impact_scopes: vec![scope.requested_scope.clone()],
        recommended_next_action: "仅供用户审阅；H01 不自动修复".into(),
    }
}

pub fn diagnose_canon_evidence(
    scope: &ProjectScope,
    claim: &str,
    evidence_refs: Vec<String>,
) -> Finding {
    grounded_finding(
        scope,
        "diagnose_canon_evidence",
        claim,
        evidence_refs,
        Vec::new(),
    )
}

pub fn diagnose_knowledge_event(
    scope: &ProjectScope,
    claim: &str,
    knowledge_ref: Option<&str>,
    event_ref: Option<&str>,
    evidence_ref: Option<&str>,
    world_time_conflict: bool,
) -> Finding {
    let source_refs = [knowledge_ref, event_ref, evidence_ref]
        .into_iter()
        .flatten()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let conflicting_refs = if world_time_conflict {
        source_refs.clone()
    } else {
        Vec::new()
    };
    grounded_finding(
        scope,
        "diagnose_knowledge_event",
        claim,
        source_refs,
        conflicting_refs,
    )
}

pub fn diagnose_thread_evidence(
    scope: &ProjectScope,
    claim: &str,
    thread_ref: Option<&str>,
    authority_ref: Option<&str>,
    evidence_ref: Option<&str>,
) -> Finding {
    grounded_finding(
        scope,
        "diagnose_thread_evidence",
        claim,
        [thread_ref, authority_ref, evidence_ref]
            .into_iter()
            .flatten()
            .map(ToOwned::to_owned)
            .collect(),
        Vec::new(),
    )
}

pub fn diagnose_task_evidence(
    scope: &ProjectScope,
    claim: &str,
    task_ref: Option<&str>,
    evidence_ref: Option<&str>,
) -> Finding {
    grounded_finding(
        scope,
        "diagnose_task_evidence",
        claim,
        [task_ref, evidence_ref]
            .into_iter()
            .flatten()
            .map(ToOwned::to_owned)
            .collect(),
        Vec::new(),
    )
}

pub fn diagnose_projection_authority(
    scope: &ProjectScope,
    claim: &str,
    projection_ref: &str,
    authority_ref: &str,
    drift: bool,
) -> Finding {
    grounded_finding(
        scope,
        "diagnose_projection_authority",
        claim,
        vec![projection_ref.into(), authority_ref.into()],
        if drift {
            vec![projection_ref.into(), authority_ref.into()]
        } else {
            Vec::new()
        },
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceLedgerEntry {
    pub call_id: String,
    pub capability_id: String,
    pub capability_version: String,
    pub spec_digest: String,
    pub project_id: String,
    pub project_revision: i64,
    pub input_digest: String,
    pub source_refs: Vec<String>,
    pub result_digest: String,
    pub truncated: bool,
    pub bounds_applied: RequestedBounds,
    pub started_at: String,
    pub finished_at: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SupportCheckpoint {
    pub checkpoint_id: String,
    pub session_id: String,
    pub project_id: String,
    pub project_revision: i64,
    pub question_digest: String,
    pub plan_digest: String,
    pub completed_step_ids: Vec<String>,
    pub capability_spec_digests: BTreeMap<String, String>,
    pub evidence_refs: Vec<String>,
    pub last_result_digests: Vec<String>,
    pub pending_authorization_ref: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointResume {
    Safe,
    Stale,
}

pub fn check_checkpoint_resume(
    checkpoint: &SupportCheckpoint,
    current_revision: i64,
    current_registry_digest: &BTreeMap<String, String>,
) -> CheckpointResume {
    if checkpoint.project_revision != current_revision {
        return CheckpointResume::Stale;
    }
    if checkpoint.capability_spec_digests != *current_registry_digest {
        return CheckpointResume::Stale;
    }
    CheckpointResume::Safe
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum ReplanTrigger {
    INSUFFICIENT_DATA,
    SCOPE_MISMATCH,
    CAPABILITY_FAILURE,
    PROJECT_REVISION_CHANGED,
    CHECKPOINT_STALE,
    EVIDENCE_CONFLICT,
    USER_STEER,
}

pub fn bounded_replan(
    plan: &SupportTaskPlan,
    trigger: ReplanTrigger,
    created_at: &str,
) -> Result<SupportTaskPlan, String> {
    if plan.steps.len() > 7 {
        return Err("SUPPORT_PLAN_BUDGET_EXCEEDED".into());
    }
    let prior_plan_digest =
        sha256_canonical(&serde_json::to_value(plan).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut next = plan.clone();
    next.plan_id = format!("{}:replan", plan.plan_id);
    next.created_at = created_at.into();
    next.status = format!("REPLANNED:{trigger:?}");
    next.replan_trigger = Some(trigger);
    next.prior_plan_digest = Some(prior_plan_digest);
    for step in &mut next.steps {
        if step.status == SupportStepStatus::RUNNING {
            step.status = SupportStepStatus::PENDING;
        }
    }
    next.new_plan_digest = Some(
        sha256_canonical(&serde_json::to_value(&next.steps).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?,
    );
    Ok(next)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SupportSessionReceipt {
    pub session_id: String,
    pub question_digest: String,
    pub project_id: String,
    pub project_revision: i64,
    pub plan_digest: String,
    pub capabilities_invoked: Vec<String>,
    pub source_refs: Vec<String>,
    pub finding_refs: Vec<String>,
    pub checkpoint_ref: Option<String>,
    pub final_support_status: SupportStatus,
    pub started_at: String,
    pub finished_at: String,
}

pub fn registry_digest_map(registry: &CapabilityRegistry) -> BTreeMap<String, String> {
    registry
        .specs
        .iter()
        .map(|(id, spec)| (id.clone(), spec.spec_digest.clone()))
        .collect()
}

pub fn can_expose_write_capability(registry: &CapabilityRegistry) -> bool {
    registry
        .specs
        .values()
        .any(|spec| spec.read_or_write != "READ" || spec.stage != READ_ONLY_STAGE)
}
