use crate::hash::sha256_canonical;
use crate::repositories::narrative::{CanonEventRecord, EvidenceRecord, KnowledgeRecord};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const READ_ONLY_STAGE: &str = "H01_READ_DIAGNOSE";
pub const H02_READ_ONLY_STAGE: &str = "H02_PLANNING_DIAGNOSE";
pub const H03_REPAIR_STAGE: &str = "H03_AUTHORIZED_REPAIR";

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
            "query_plan_route",
            "diagnose_planning_constraints",
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
            hard_bounds.insert("maxSerializedBytes".into(), 65_536);
            let stage = if matches!(id, "query_plan_route" | "diagnose_planning_constraints") {
                H02_READ_ONLY_STAGE
            } else {
                READ_ONLY_STAGE
            };
            let digest = sha256_canonical(&json!({"capabilityId":id,"version":"1.0.0","modes":["INSPECT","DIAGNOSE"],"stage":stage,"owner":"Rust","readOrWrite":"READ","inputContract":"typed-json","outputContract":"bounded-result","hardBounds":hard_bounds})).unwrap();
            specs.insert(
                id.into(),
                CapabilitySpec {
                    capability_id: id.into(),
                    version: "1.0.0".into(),
                    spec_digest: digest,
                    modes: vec!["INSPECT".into(), "DIAGNOSE".into()],
                    stage: stage.into(),
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

    pub fn b09_replan_r1() -> Self {
        let capability_id = "replan_volume_route";
        let version = "1.0.0";
        let modes = vec!["EXECUTE".into()];
        let mut hard_bounds = BTreeMap::new();
        hard_bounds.insert("maxResults".into(), 100);
        hard_bounds.insert("maxSerializedBytes".into(), 65_536);
        let spec_digest = sha256_canonical(&json!({
            "capabilityId": capability_id,
            "version": version,
            "modes": modes,
            "stage": H03_REPAIR_STAGE,
            "owner": "B09::VolumeLifecycleOwner",
            "readOrWrite": "WRITE",
            "inputContract": "route-replan-r1",
            "outputContract": "project-command-receipt",
            "hardBounds": hard_bounds,
        }))
        .unwrap();
        let mut specs = BTreeMap::new();
        specs.insert(
            capability_id.into(),
            CapabilitySpec {
                capability_id: capability_id.into(),
                version: version.into(),
                spec_digest,
                modes,
                stage: H03_REPAIR_STAGE.into(),
                owner: "B09::VolumeLifecycleOwner".into(),
                read_or_write: "WRITE".into(),
                input_contract: "route-replan-r1".into(),
                output_contract: "project-command-receipt".into(),
                hard_bounds,
            },
        );
        Self { specs }
    }

    pub fn b09_repair_r1() -> Self {
        let mut registry = Self::b09_replan_r1();
        let capability_id = "rebuild_planning_state_projection";
        let version = "1.0.0";
        let modes = vec!["EXECUTE".into()];
        let mut hard_bounds = BTreeMap::new();
        hard_bounds.insert("maxResults".into(), 100);
        hard_bounds.insert("maxSerializedBytes".into(), 65_536);
        let spec_digest = sha256_canonical(&json!({
            "capabilityId": capability_id,
            "version": version,
            "modes": modes,
            "stage": H03_REPAIR_STAGE,
            "owner": "B09::PlanningStateProjectionOwner",
            "readOrWrite": "WRITE",
            "inputContract": "planning-state-projection-r1",
            "outputContract": "project-command-receipt",
            "hardBounds": hard_bounds,
        }))
        .unwrap();
        registry.specs.insert(
            capability_id.into(),
            CapabilitySpec {
                capability_id: capability_id.into(),
                version: version.into(),
                spec_digest,
                modes,
                stage: H03_REPAIR_STAGE.into(),
                owner: "B09::PlanningStateProjectionOwner".into(),
                read_or_write: "WRITE".into(),
                input_contract: "planning-state-projection-r1".into(),
                output_contract: "project-command-receipt".into(),
                hard_bounds,
            },
        );
        registry
    }

    pub fn exact_authorized_repair(
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
        if spec.stage != H03_REPAIR_STAGE || spec.read_or_write != "WRITE" {
            return Err("CAPABILITY_STAGE_FORBIDDEN".into());
        }
        Ok(spec)
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
        if !matches!(spec.stage.as_str(), READ_ONLY_STAGE | H02_READ_ONLY_STAGE)
            || spec.read_or_write != "READ"
        {
            return Err("CAPABILITY_STAGE_FORBIDDEN".into());
        }
        Ok(spec)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestedBounds {
    pub max_results: usize,
    pub max_serialized_bytes: usize,
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
            || call.requested_bounds.max_serialized_bytes
                > *spec.hard_bounds.get("maxSerializedBytes").unwrap_or(&0) as usize
        {
            return Err("CAPABILITY_BOUNDS_ESCALATION".into());
        }
        let (result, source_refs, truncated) =
            adapter.invoke(&call.capability_id, &call.input, &call.requested_bounds)?;
        if serde_json::to_vec(&result)
            .map_err(|e| e.to_string())?
            .len()
            > call.requested_bounds.max_serialized_bytes
        {
            return Err("CAPABILITY_RESULT_EXCEEDS_MAX_SERIALIZED_BYTES".into());
        }
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
    let capabilities = if [
        "高潮",
        "required task",
        "未完成",
        "stale",
        "揭露",
        "秘密",
        "capacity",
        "容量",
        "planner",
        "承诺",
        "任务",
        "规划",
    ]
    .iter()
    .any(|term| lower.contains(term))
    {
        vec!["query_plan_route", "diagnose_planning_constraints"]
    } else if lower.contains("知道") || lower.contains("knowledge") {
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
    knowledge: &KnowledgeRecord,
    establishing_event: Option<&CanonEventRecord>,
    evidence: Option<&EvidenceRecord>,
) -> Finding {
    let mut source_refs = vec![knowledge.knowledge_id.clone()];
    if let Some(event) = establishing_event {
        source_refs.push(event.event_id.clone());
    }
    if let Some(item) = evidence {
        source_refs.push(item.evidence_id.clone());
    }
    let evidence_matches_event = evidence.is_some_and(|item| {
        establishing_event
            .is_some_and(|event| item.event_id.as_deref() == Some(event.event_id.as_str()))
    });
    let evidence_matches_fact = evidence.is_none_or(|item| {
        item.fact_id
            .as_deref()
            .is_none_or(|fact| knowledge.fact_id.as_deref() == Some(fact))
    });
    let time_conflict = knowledge
        .from_story_time
        .as_ref()
        .zip(establishing_event.and_then(|event| event.story_time.as_ref()))
        .is_some_and(|(knowledge_time, event_time)| knowledge_time < event_time);
    let missing_chain = establishing_event.is_none() || evidence.is_none();
    let conflicting_chain =
        !missing_chain && (!evidence_matches_event || !evidence_matches_fact || time_conflict);
    grounded_finding(
        scope,
        if conflicting_chain {
            "DATA_INCONSISTENCY"
        } else if missing_chain {
            "EVIDENCE_GAP"
        } else {
            "diagnose_knowledge_event"
        },
        claim,
        if missing_chain || conflicting_chain {
            Vec::new()
        } else {
            source_refs.clone()
        },
        if conflicting_chain {
            source_refs
        } else {
            Vec::new()
        },
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
    projection_revision: i64,
    authority_revision: i64,
) -> Finding {
    let refs = vec![projection_ref.into(), authority_ref.into()];
    grounded_finding(
        scope,
        if projection_revision == authority_revision {
            "NO_ISSUE"
        } else {
            "PROJECTION_DRIFT"
        },
        claim,
        refs.clone(),
        if projection_revision == authority_revision {
            Vec::new()
        } else {
            refs
        },
    )
}

pub struct DiagnosticEngine;

impl DiagnosticEngine {
    pub fn canon_evidence(
        scope: &ProjectScope,
        claim: &str,
        evidence_refs: Vec<String>,
    ) -> Finding {
        diagnose_canon_evidence(scope, claim, evidence_refs)
    }

    pub fn knowledge_event(
        scope: &ProjectScope,
        claim: &str,
        knowledge: &KnowledgeRecord,
        event: Option<&CanonEventRecord>,
        evidence: Option<&EvidenceRecord>,
    ) -> Finding {
        diagnose_knowledge_event(scope, claim, knowledge, event, evidence)
    }

    pub fn projection_authority(
        scope: &ProjectScope,
        claim: &str,
        projection_ref: &str,
        authority_ref: &str,
        projection_revision: i64,
        authority_revision: i64,
    ) -> Finding {
        diagnose_projection_authority(
            scope,
            claim,
            projection_ref,
            authority_ref,
            projection_revision,
            authority_revision,
        )
    }

    pub fn thread_evidence(
        scope: &ProjectScope,
        claim: &str,
        thread_ref: Option<&str>,
        authority_ref: Option<&str>,
        evidence_ref: Option<&str>,
    ) -> Finding {
        diagnose_thread_evidence(scope, claim, thread_ref, authority_ref, evidence_ref)
    }

    pub fn task_evidence(
        scope: &ProjectScope,
        claim: &str,
        task_ref: Option<&str>,
        evidence_ref: Option<&str>,
    ) -> Finding {
        diagnose_task_evidence(scope, claim, task_ref, evidence_ref)
    }
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
    pub requested_mode: String,
    pub requested_scope: String,
    pub plan_steps: Vec<SupportTaskStep>,
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
        .any(|spec| spec.read_or_write != "READ")
}
