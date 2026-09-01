//! B10 Creative Skill and Narrative Quality policy boundary.
//!
//! This module owns only bounded creative advice and soft quality assessment.
//! It never opens a database, executes SQL, mutates authority, enforces a
//! reservation, changes a route, or commits a bundle. B08 remains the single
//! production composition root and B06/B07/B09 remain the authority owners.

use crate::generated::types::ArtifactRef;
use crate::hash::sha256_canonical;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const CREATIVE_SKILLS_VERSION: &str = "1.2.0";
pub const CREATIVE_SKILLS_ARCHIVE_SHA256: &str =
    "a90ab6e12d82d57b32f06d5a3780d0153d6f372e1efd47df1d70ef1aebbdef8f";
const CREATIVE_SKILLS_REGISTRY: &str = include_str!("../../creative-skills/v1.2.0/registry.json");
const MAX_CONTEXT_BYTES: usize = 64_000;
const MAX_REFS: usize = 64;
const MAX_CAPSULES: usize = 16;
const MAX_CAPSULE_SUMMARY: usize = 2_000;

const FORBIDDEN_CAPABILITIES: &[&str] = &[
    "DB_QUERY",
    "SQL",
    "REPOSITORY_READ",
    "AUTHORITY_WRITE",
    "COMMIT",
    "TASK_REALITY_WRITE",
    "THREAD_WRITE",
    "RESERVATION_ENFORCEMENT",
    "ROUTE_REPLAN",
    "CHANGE_EXECUTION",
    "H03_P3",
    "HARD_GATE",
];

const OUTPUT_KINDS: &[&str] = &[
    "CreativeProposal",
    "Candidate",
    "CandidatePatch",
    "QualityFinding",
    "RepairSuggestion",
    "EscalationAdvice",
];

const RUNTIME_CHECK_KINDS: &[&str] = &[
    "CANON_CONFLICT",
    "KNOWLEDGE_BOUNDARY",
    "THREAD_STATE",
    "TASK_REALITY",
    "RESERVATION",
    "INTEGRITY",
    "PLAN_STALE",
    "EVIDENCE_GAP",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillMetadata {
    pub id: String,
    pub slug: String,
    pub name_zh: String,
    pub category: String,
    pub role: String,
    pub runtime_stage: String,
    pub context_policy: String,
    pub mutation_policy: String,
    pub coverage: Vec<String>,
    pub bundle_digest: String,
    pub authority_access: String,
    pub hard_gate_authority: bool,
    pub control_decision_authority: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillRegistry {
    pub package: String,
    pub version: String,
    pub skill_count: usize,
    pub selection_policy: String,
    pub skills: Vec<SkillMetadata>,
}

fn string_field(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Loads package metadata only. Skill text and resources are intentionally not
/// embedded or read here; the frontend loader fetches only selected files.
pub fn load_registry() -> Result<SkillRegistry, String> {
    let root: Value = serde_json::from_str(CREATIVE_SKILLS_REGISTRY)
        .map_err(|error| format!("SKILL_REGISTRY_INVALID:{error}"))?;
    let skills = root
        .get("skills")
        .and_then(Value::as_array)
        .ok_or_else(|| "SKILL_REGISTRY_SKILLS_MISSING".to_string())?
        .iter()
        .map(|value| SkillMetadata {
            id: string_field(value, "id"),
            slug: string_field(value, "slug"),
            name_zh: string_field(value, "name_zh"),
            category: string_field(value, "category"),
            role: string_field(value, "role"),
            runtime_stage: string_field(value, "runtimeStage"),
            context_policy: string_field(value, "contextPolicy"),
            mutation_policy: string_field(value, "mutationPolicy"),
            coverage: string_array(value.get("coverage")),
            bundle_digest: string_field(value, "bundleDigest"),
            authority_access: string_field(value, "authorityAccess"),
            hard_gate_authority: value
                .get("hardGateAuthority")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            control_decision_authority: value
                .get("controlDecisionAuthority")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect::<Vec<_>>();
    let version = string_field(&root, "version");
    let skill_count = root
        .get("skillCount")
        .and_then(Value::as_u64)
        .unwrap_or(skills.len() as u64) as usize;
    if version != CREATIVE_SKILLS_VERSION || skill_count != 45 || skills.len() != 45 {
        return Err("SKILL_REGISTRY_VERSION_OR_COUNT_INVALID".into());
    }
    Ok(SkillRegistry {
        package: string_field(&root, "package"),
        version,
        skill_count,
        selection_policy: string_field(&root, "selectionPolicy"),
        skills,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillSelectionRequest {
    pub primary: String,
    pub support: Vec<String>,
    pub budget_reason: Option<String>,
    pub context_tokens: i64,
    pub model_calls: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillSelection {
    pub primary: SkillMetadata,
    pub support: Vec<SkillMetadata>,
    pub selected: Vec<SkillMetadata>,
    pub selected_ids: Vec<String>,
    pub budget_reason: Option<String>,
    pub context_tokens: i64,
    pub model_calls: i64,
}

pub fn select_creative_skills(request: SkillSelectionRequest) -> Result<SkillSelection, String> {
    let registry = load_registry()?;
    let by_id = registry
        .skills
        .iter()
        .map(|skill| (skill.id.as_str(), skill))
        .collect::<BTreeMap<_, _>>();
    let ids = std::iter::once(request.primary.as_str())
        .chain(request.support.iter().map(String::as_str))
        .collect::<Vec<_>>();
    if !by_id.contains_key(request.primary.as_str()) {
        return Err("SKILL_PRIMARY_NOT_FOUND".into());
    }
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != ids.len() {
        return Err("SKILL_SELECTION_DUPLICATE".into());
    }
    if ids.len() > 8 {
        return Err("SKILL_SELECTION_UNBOUNDED".into());
    }
    if ids.len() > 4
        && (request
            .budget_reason
            .as_deref()
            .is_none_or(|reason| reason.trim().is_empty())
            || request.context_tokens <= 0
            || request.model_calls <= 0)
    {
        return Err("SKILL_SELECTION_BUDGET_REQUIRED".into());
    }
    let selected = ids
        .iter()
        .map(|id| {
            by_id
                .get(id)
                .map(|skill| (*skill).clone())
                .ok_or_else(|| format!("SKILL_NOT_FOUND:{id}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SkillSelection {
        primary: selected[0].clone(),
        support: selected[1..].to_vec(),
        selected_ids: selected.iter().map(|skill| skill.id.clone()).collect(),
        selected,
        budget_reason: request.budget_reason,
        context_tokens: request.context_tokens,
        model_calls: request.model_calls,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BoundedRef {
    pub kind: String,
    pub reference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContextCapsule {
    pub kind: String,
    pub reference: String,
    pub summary: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillBudget {
    pub context_tokens: i64,
    pub model_calls: i64,
    pub repair_runs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillTaskContext {
    pub task_id: String,
    pub project_id: String,
    pub project_revision: i64,
    pub runtime_stage: String,
    pub context_manifest_ref: String,
    pub bounded_refs: Vec<BoundedRef>,
    pub capsules: Vec<ContextCapsule>,
    pub quality_inputs: Value,
    pub method_profile: Value,
    pub budget: SkillBudget,
    pub allowed_outputs: Vec<String>,
    pub forbidden_capabilities: Vec<String>,
    pub context_digest: String,
}

impl SkillTaskContext {
    pub fn assert_fresh(&self, current_revision: i64) -> Result<&'static str, String> {
        if current_revision != self.project_revision {
            Err("STALE/RECOMPILE".into())
        } else {
            Ok("FRESH")
        }
    }
}

fn required_string(value: &str, code: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        Err(code.into())
    } else {
        Ok(value.into())
    }
}

/// Builds a bounded context from already-authorized inputs. It intentionally
/// accepts a ChapterPlan payload, not a database connection or a project dump.
pub fn build_skill_task_context(
    project_id: &str,
    task_id: &str,
    project_revision: i64,
    context_manifest_ref: &str,
    plan_payload: &Value,
) -> Result<SkillTaskContext, String> {
    if project_revision < 0 {
        return Err("SKILL_CONTEXT_REVISION_INVALID".into());
    }
    let task_id = required_string(task_id, "SKILL_CONTEXT_TASK_INVALID")?;
    let project_id = required_string(project_id, "SKILL_CONTEXT_PROJECT_INVALID")?;
    let context_manifest_ref =
        required_string(context_manifest_ref, "SKILL_CONTEXT_MANIFEST_INVALID")?;
    let active_task_refs = plan_payload
        .get("activeTaskRefs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .take(MAX_REFS)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let bounded_refs = vec![
        BoundedRef {
            kind: "ContextManifest".into(),
            reference: context_manifest_ref.clone(),
        },
        BoundedRef {
            kind: "ChapterPlan".into(),
            reference: plan_payload
                .get("artifactId")
                .and_then(Value::as_str)
                .unwrap_or("bound:chapter-plan")
                .into(),
        },
    ];
    if bounded_refs.len() > MAX_REFS {
        return Err("SKILL_CONTEXT_REF_LIMIT".into());
    }
    let chapter_goal = plan_payload
        .get("chapterGoal")
        .and_then(Value::as_str)
        .unwrap_or("未提供章节目标")
        .chars()
        .take(MAX_CAPSULE_SUMMARY)
        .collect::<String>();
    let capsules = vec![ContextCapsule {
        kind: "chapter_goal".into(),
        reference: "plan:chapterGoal".into(),
        summary: chapter_goal,
        digest: sha256_canonical(&json!({
            "chapterGoal": plan_payload.get("chapterGoal")
        }))
        .map_err(|error| error.to_string())?,
    }];
    if capsules.len() > MAX_CAPSULES {
        return Err("SKILL_CONTEXT_CAPSULE_LIMIT".into());
    }
    let base = json!({
        "taskId": task_id,
        "projectId": project_id,
        "projectRevision": project_revision,
        "runtimeStage": "CHAPTER_PRODUCTION",
        "contextManifestRef": context_manifest_ref,
        "boundedRefs": bounded_refs,
        "capsules": capsules,
        "activeTaskRefs": active_task_refs,
        "qualityInputs": {"source": "B08_BOUND_INPUTS"},
        "methodProfile": {"profile": "NORMAL", "version": "b10.profile.v1"},
        "budget": {"contextTokens": 1200, "modelCalls": 1, "repairRuns": 2},
        "allowedOutputs": OUTPUT_KINDS,
        "forbiddenCapabilities": FORBIDDEN_CAPABILITIES,
    });
    if serde_json::to_vec(&base)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_CONTEXT_BYTES
    {
        return Err("SKILL_CONTEXT_BYTES_LIMIT".into());
    }
    Ok(SkillTaskContext {
        task_id,
        project_id,
        project_revision,
        runtime_stage: "CHAPTER_PRODUCTION".into(),
        context_manifest_ref,
        bounded_refs,
        capsules,
        quality_inputs: json!({"source": "B08_BOUND_INPUTS"}),
        method_profile: json!({"profile": "NORMAL", "version": "b10.profile.v1"}),
        budget: SkillBudget {
            context_tokens: 1200,
            model_calls: 1,
            repair_runs: 2,
        },
        allowed_outputs: OUTPUT_KINDS.iter().map(|value| (*value).into()).collect(),
        forbidden_capabilities: FORBIDDEN_CAPABILITIES
            .iter()
            .map(|value| (*value).into())
            .collect(),
        context_digest: sha256_canonical(&base).map_err(|error| error.to_string())?,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCheckRequest {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedSkillOutput {
    pub kind: String,
    pub status: String,
    pub summary: String,
    pub suggestions: Vec<String>,
    pub runtime_check_requests: Vec<RuntimeCheckRequest>,
    pub formal_block: bool,
    pub authority_write: bool,
    pub control_decision: bool,
}

pub fn normalize_skill_output(raw: Value) -> Result<NormalizedSkillOutput, String> {
    let kind = raw
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "SKILL_OUTPUT_KIND_INVALID".to_string())?;
    if !OUTPUT_KINDS.contains(&kind) {
        return Err("SKILL_OUTPUT_KIND_INVALID".into());
    }
    let requested_status = raw.get("status").and_then(Value::as_str).unwrap_or("OK");
    let status = if requested_status == "BLOCK" {
        "ADVISORY_ESCALATION"
    } else {
        requested_status
    };
    if !["OK", "NEEDS_INPUT", "NOT_APPLICABLE", "ADVISORY_ESCALATION"].contains(&status) {
        return Err("SKILL_OUTPUT_STATUS_INVALID".into());
    }
    let runtime_check_requests = raw
        .get("runtimeCheckRequests")
        .and_then(Value::as_array)
        .unwrap_or(&Vec::new())
        .iter()
        .take(8)
        .map(|item| {
            let item_kind = item
                .get("kind")
                .and_then(Value::as_str)
                .ok_or_else(|| "SKILL_RUNTIME_CHECK_KIND_INVALID".to_string())?;
            if !RUNTIME_CHECK_KINDS.contains(&item_kind) {
                return Err("SKILL_RUNTIME_CHECK_KIND_INVALID".into());
            }
            Ok(RuntimeCheckRequest {
                kind: item_kind.into(),
                detail: item
                    .get("detail")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                refs: item
                    .get("refs")
                    .and_then(Value::as_array)
                    .map(|refs| {
                        refs.iter()
                            .filter_map(Value::as_str)
                            .take(MAX_REFS)
                            .map(ToOwned::to_owned)
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if raw
        .get("runtimeCheckRequests")
        .and_then(Value::as_array)
        .is_some_and(|requests| requests.len() > 8)
    {
        return Err("SKILL_RUNTIME_CHECK_LIMIT".into());
    }
    Ok(NormalizedSkillOutput {
        kind: kind.into(),
        status: status.into(),
        summary: raw
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .take(4_000)
            .collect(),
        suggestions: raw
            .get("suggestions")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .take(16)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        runtime_check_requests,
        formal_block: false,
        authority_write: false,
        control_decision: false,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InvocationProvenance {
    pub invocation_id: String,
    pub skill_id: String,
    pub skill_version: String,
    pub bundle_digest: String,
    pub role: String,
    pub resource_refs: Vec<String>,
    pub resource_digests: Vec<Value>,
    pub context_ref: String,
    pub context_digest: String,
    pub context_manifest_ref: String,
    pub method_refs: Vec<String>,
    pub project_context_refs: Vec<String>,
    pub source_refs: Vec<String>,
    pub runtime_check_requests: Vec<RuntimeCheckRequest>,
    pub project_id: String,
    pub project_revision: i64,
    pub model: String,
    pub provider: String,
    pub model_run_refs: Vec<String>,
    pub tokens: BTreeMap<String, i64>,
    pub cost_cents: i64,
    pub output_digest: String,
    pub created_at: String,
}

pub fn create_invocation_provenance(
    invocation_id: &str,
    skill: &SkillMetadata,
    context: &SkillTaskContext,
    method_refs: Vec<String>,
    project_context_refs: Vec<String>,
    output_digest: String,
) -> Result<InvocationProvenance, String> {
    if invocation_id.trim().is_empty() {
        return Err("SKILL_INVOCATION_ID_INVALID".into());
    }
    Ok(InvocationProvenance {
        invocation_id: invocation_id.into(),
        skill_id: skill.id.clone(),
        skill_version: CREATIVE_SKILLS_VERSION.into(),
        bundle_digest: skill.bundle_digest.clone(),
        role: skill.role.clone(),
        resource_refs: Vec::new(),
        resource_digests: Vec::new(),
        context_ref: context.context_manifest_ref.clone(),
        context_digest: context.context_digest.clone(),
        context_manifest_ref: context.context_manifest_ref.clone(),
        source_refs: method_refs.clone(),
        method_refs,
        project_context_refs,
        runtime_check_requests: Vec::new(),
        project_id: context.project_id.clone(),
        project_revision: context.project_revision,
        model: "local-recording".into(),
        provider: "deterministic".into(),
        model_run_refs: Vec::new(),
        tokens: BTreeMap::from([("input".into(), 0), ("output".into(), 0)]),
        cost_cents: 0,
        output_digest,
        created_at: "1970-01-01T00:00:00Z".into(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NarrativeTasteProfile {
    pub version: String,
    pub digest: String,
    pub profile: Value,
    pub provenance: Value,
    pub soft_only: bool,
}

pub fn default_taste_profile(revision: i64) -> NarrativeTasteProfile {
    let profile = json!({"voice":"用户设定优先","pacing":"章节目标驱动","readerDesire":"SOFT"});
    let provenance = json!({"source":"B10_DEFAULT","projectRevision":revision});
    let digest = sha256_canonical(&json!({
        "version":"taste.v1",
        "profile":profile,
        "provenance":provenance
    }))
    .unwrap_or_else(|_| "0".repeat(64));
    NarrativeTasteProfile {
        version: "taste.v1".into(),
        digest,
        profile,
        provenance,
        soft_only: true,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualityDimension {
    pub score: i64,
    pub severity: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualityFinding {
    pub finding_id: String,
    pub dimension: String,
    pub severity: String,
    pub evidence: Vec<String>,
    pub repairable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualityModelProvenance {
    pub evaluator: String,
    pub evaluator_calls: i64,
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NarrativeQualityReport {
    pub candidate_ref: ArtifactRef,
    pub project_id: String,
    pub revision: i64,
    pub taste_profile_ref: String,
    pub taste_profile_version: String,
    pub dimensions: BTreeMap<String, QualityDimension>,
    pub findings: Vec<QualityFinding>,
    pub repairable_dimensions: Vec<String>,
    pub runtime_checks: Vec<RuntimeCheckRequest>,
    pub soft_assessment: Value,
    pub model_provenance: QualityModelProvenance,
    pub invocation_refs: Vec<String>,
    pub rechecked_dimensions: Vec<String>,
    pub digest: String,
    pub soft_only: bool,
}

pub fn evaluate_narrative_quality(
    candidate_ref: &ArtifactRef,
    project_id: &str,
    revision: i64,
    body: &str,
    taste_profile: &NarrativeTasteProfile,
    invocation_refs: Vec<String>,
) -> Result<NarrativeQualityReport, String> {
    let length_score = (body.chars().count() as i64 * 2).clamp(20, 100);
    let severity = if body.trim().chars().count() < 20 {
        "WARN"
    } else {
        "INFO"
    };
    let names = [
        "agency",
        "desire",
        "hook",
        "pacing",
        "voice",
        "dialogue",
        "subtext",
        "repetition",
        "exposition",
        "emotionalPayoff",
    ];
    let evidence = if body.trim().is_empty() {
        Vec::new()
    } else {
        vec![format!(
            "body:{}",
            body.chars().take(24).collect::<String>()
        )]
    };
    let dimensions = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            (
                (*name).into(),
                QualityDimension {
                    score: (length_score + index as i64 % 3 - 1).clamp(0, 100),
                    severity: severity.into(),
                    evidence: evidence.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let findings = if severity == "WARN" {
        vec![QualityFinding {
            finding_id: format!("quality:{}", candidate_ref.artifact_id),
            dimension: "exposition".into(),
            severity: "WARN".into(),
            evidence: Vec::new(),
            repairable: true,
        }]
    } else {
        Vec::new()
    };
    let quality_score =
        dimensions.values().map(|item| item.score).sum::<i64>() / dimensions.len().max(1) as i64;
    let base = json!({
        "candidateRef":candidate_ref,
        "projectId":project_id,
        "revision":revision,
        "tasteProfileRef":taste_profile.digest,
        "tasteProfileVersion":taste_profile.version,
        "dimensions":dimensions,
        "findings":findings,
        "repairableDimensions":if severity == "WARN" {json!(["exposition"])} else {json!([])},
        "runtimeChecks":[],
        "softAssessment":{"readerDesire":"SOFT","qualityScore":quality_score},
        "modelProvenance":{"evaluator":"B10::MergedNarrativeEvaluator","evaluatorCalls":1,"provider":"deterministic","model":"quality-rules-v1"},
        "invocationRefs":invocation_refs,
        "recheckedDimensions":[],
        "softOnly":true
    });
    Ok(NarrativeQualityReport {
        candidate_ref: candidate_ref.clone(),
        project_id: project_id.into(),
        revision,
        taste_profile_ref: taste_profile.digest.clone(),
        taste_profile_version: taste_profile.version.clone(),
        dimensions,
        findings,
        repairable_dimensions: if severity == "WARN" {
            vec!["exposition".into()]
        } else {
            Vec::new()
        },
        runtime_checks: Vec::new(),
        soft_assessment: json!({"readerDesire":"SOFT","qualityScore":quality_score}),
        model_provenance: QualityModelProvenance {
            evaluator: "B10::MergedNarrativeEvaluator".into(),
            evaluator_calls: 1,
            provider: "deterministic".into(),
            model: "quality-rules-v1".into(),
        },
        invocation_refs,
        rechecked_dimensions: Vec::new(),
        digest: sha256_canonical(&base).map_err(|error| error.to_string())?,
        soft_only: true,
    })
}

/// Re-evaluates only dimensions affected by a bounded CandidatePatch. The
/// result remains a soft report and must still pass the existing B08 HARD gate.
pub fn recheck_affected_dimensions(
    prior: &NarrativeQualityReport,
    repaired_body: &str,
    taste_profile: &NarrativeTasteProfile,
    affected_dimensions: Vec<String>,
) -> Result<NarrativeQualityReport, String> {
    let mut affected = affected_dimensions;
    affected.sort();
    affected.dedup();
    if affected
        .iter()
        .any(|dimension| !prior.dimensions.contains_key(dimension))
    {
        return Err("SKILL_AFFECTED_DIMENSION_INVALID".into());
    }
    let fresh = evaluate_narrative_quality(
        &prior.candidate_ref,
        &prior.project_id,
        prior.revision,
        repaired_body,
        taste_profile,
        prior.invocation_refs.clone(),
    )?;
    let mut result = prior.clone();
    for dimension in &affected {
        if let Some(value) = fresh.dimensions.get(dimension) {
            result.dimensions.insert(dimension.clone(), value.clone());
        }
    }
    result.rechecked_dimensions = affected.clone();
    result.findings = result
        .findings
        .into_iter()
        .filter(|finding| !affected.contains(&finding.dimension))
        .chain(
            fresh
                .findings
                .into_iter()
                .filter(|finding| affected.contains(&finding.dimension)),
        )
        .collect();
    result.repairable_dimensions = result
        .findings
        .iter()
        .filter(|finding| finding.repairable)
        .map(|finding| finding.dimension.clone())
        .collect();
    let mut digest_input = serde_json::to_value(&result).map_err(|error| error.to_string())?;
    digest_input["digest"] = Value::String(String::new());
    result.digest = sha256_canonical(&digest_input).map_err(|error| error.to_string())?;
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HardQualityDecision {
    pub decision: String,
    pub hard_reason: Option<String>,
    pub quality_action: String,
}

pub fn enforce_hard_quality_separation(
    report: &NarrativeQualityReport,
    hard_passed: bool,
    hard_reason: Option<String>,
) -> HardQualityDecision {
    if !hard_passed {
        HardQualityDecision {
            decision: "BLOCKED".into(),
            hard_reason: hard_reason.or_else(|| Some("HARD_GATE_FAILED".into())),
            quality_action: "NO_REPAIR".into(),
        }
    } else {
        HardQualityDecision {
            decision: "PASS".into(),
            hard_reason: None,
            quality_action: if report.findings.is_empty() {
                "NO_REPAIR".into()
            } else {
                "REPAIR_OR_ESCALATE".into()
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepairCandidateRequest {
    pub candidate_ref: String,
    pub candidate_status: String,
    pub body: String,
    pub finding_refs: Vec<String>,
    pub affected_dimensions: Vec<String>,
    pub start: usize,
    pub end: usize,
    pub replacement: String,
    pub skill_id: String,
    pub skill_version: String,
    pub model: String,
    pub repair_run: i64,
    pub repair_run_limit: i64,
    pub context_tokens: i64,
    pub model_calls: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepairCandidateResult {
    pub kind: String,
    pub status: String,
    pub body: String,
    pub candidate_ref: String,
    pub candidate_digest: String,
    pub finding_refs: Vec<String>,
    pub affected_dimensions: Vec<String>,
    pub repair_run: i64,
    pub repair_run_limit: i64,
    pub transport_calls: i64,
    pub reason: Option<String>,
    pub model: String,
    pub skill_id: String,
    pub skill_version: String,
}

pub fn repair_candidate(request: RepairCandidateRequest) -> Result<RepairCandidateResult, String> {
    if request.candidate_status != "CANDIDATE" {
        return Err("SKILL_CANDIDATE_ONLY".into());
    }
    if request.repair_run < 1 || request.repair_run_limit < 1 {
        return Err("SKILL_REPAIR_LIMIT_INVALID".into());
    }
    if request.repair_run >= request.repair_run_limit {
        return Ok(RepairCandidateResult {
            kind: "RepairSuggestion".into(),
            status: "ADVISORY_ESCALATION".into(),
            body: request.body,
            candidate_ref: request.candidate_ref,
            candidate_digest: String::new(),
            finding_refs: request.finding_refs,
            affected_dimensions: request.affected_dimensions,
            repair_run: request.repair_run,
            repair_run_limit: request.repair_run_limit,
            transport_calls: 0,
            reason: Some("REPAIR_RUN_LIMIT".into()),
            model: request.model,
            skill_id: request.skill_id,
            skill_version: request.skill_version,
        });
    }
    if request.context_tokens <= 0 || request.model_calls <= 0 {
        return Err("SKILL_REPAIR_BUDGET_INVALID".into());
    }
    let characters = request.body.chars().collect::<Vec<_>>();
    if request.start > request.end || request.end > characters.len() {
        return Err("SKILL_PATCH_RANGE_INVALID".into());
    }
    let body = characters[..request.start]
        .iter()
        .chain(request.replacement.chars().collect::<Vec<_>>().iter())
        .chain(characters[request.end..].iter())
        .collect::<String>();
    let digest = sha256_canonical(&json!({"body":body})).map_err(|error| error.to_string())?;
    Ok(RepairCandidateResult {
        kind: "CandidatePatch".into(),
        status: "CANDIDATE".into(),
        body,
        candidate_ref: request.candidate_ref,
        candidate_digest: digest,
        finding_refs: request.finding_refs,
        affected_dimensions: request.affected_dimensions,
        repair_run: request.repair_run,
        repair_run_limit: request.repair_run_limit,
        transport_calls: 1,
        reason: None,
        model: request.model,
        skill_id: request.skill_id,
        skill_version: request.skill_version,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChapterSkillEnrichment {
    pub package_version: String,
    pub package_digest: String,
    pub selection: SkillSelection,
    pub context: SkillTaskContext,
    pub proposal: NormalizedSkillOutput,
    pub taste_profile: NarrativeTasteProfile,
    pub invocations: Vec<InvocationProvenance>,
}

/// B08 calls this pure composition step immediately before rendering. It adds
/// bounded creative guidance to the existing render input; it does not create
/// a ChapterPlan or perform any write.
pub fn enrich_chapter(
    project_id: &str,
    chapter_id: &str,
    revision: i64,
    context_manifest_ref: &str,
    plan_payload: &Value,
    execution_mode: &str,
) -> Result<ChapterSkillEnrichment, String> {
    let (primary, support) = if execution_mode == "OPENING_BOOTSTRAP" {
        ("S14", vec!["S10".into()])
    } else {
        ("S21", vec!["S23".into()])
    };
    let selection = select_creative_skills(SkillSelectionRequest {
        primary: primary.into(),
        support,
        budget_reason: None,
        context_tokens: 1200,
        model_calls: 1,
    })?;
    let context = build_skill_task_context(
        project_id,
        &format!("task:{chapter_id}:creative"),
        revision,
        context_manifest_ref,
        plan_payload,
    )?;
    let proposal = normalize_skill_output(json!({
        "kind":"CreativeProposal",
        "status":"OK",
        "summary":format!("{}：仅提供有界创作建议", selection.primary.name_zh),
        "suggestions":["保留 ChapterPlan 的目标与边界；只对候选正文提供局部建议"]
    }))?;
    let output_digest =
        sha256_canonical(&serde_json::to_value(&proposal).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let invocations = selection
        .selected
        .iter()
        .enumerate()
        .map(|(index, skill)| {
            create_invocation_provenance(
                &format!("inv:{chapter_id}:{}", index + 1),
                skill,
                &context,
                vec![format!("method:creative-skill:{}", skill.id)],
                vec![context_manifest_ref.into(), format!("plan:{chapter_id}")],
                output_digest.clone(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ChapterSkillEnrichment {
        package_version: CREATIVE_SKILLS_VERSION.into(),
        package_digest: CREATIVE_SKILLS_ARCHIVE_SHA256.into(),
        selection,
        context,
        proposal,
        taste_profile: default_taste_profile(revision),
        invocations,
    })
}
