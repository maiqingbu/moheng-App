//! B10 Creative Skill and Narrative Quality policy boundary.
//!
//! This module owns only bounded creative advice and soft quality assessment.
//! It never opens a database, executes SQL, mutates authority, enforces a
//! reservation, changes a route, or commits a bundle. B08 remains the single
//! production composition root and B06/B07/B09 remain the authority owners.

use crate::generated::types::ArtifactRef;
use crate::hash::sha256_canonical;
use crate::skill_deployment::{self, VerifiedSkillDeployment};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const CREATIVE_SKILLS_VERSION: &str = "1.2.0";
pub const CREATIVE_SKILLS_ARCHIVE_SHA256: &str = skill_deployment::EXPECTED_PACKAGE_ARCHIVE_SHA256;
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

pub fn runtime_check_kind_allowed(kind: &str) -> bool {
    RUNTIME_CHECK_KINDS.contains(&kind)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillMetadata {
    pub id: String,
    pub slug: String,
    pub name_zh: String,
    pub version: String,
    pub category: String,
    pub role: String,
    pub runtime_stage: String,
    pub context_policy: String,
    pub mutation_policy: String,
    pub coverage: Vec<String>,
    pub bundle_digest: String,
    pub method_resource: Option<String>,
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

fn string_array(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(item)) => vec![item.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

fn required_registry_string(value: &Value, field: &str) -> Result<String, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| "SKILL_REGISTRY_SCHEMA_INVALID".to_string())
}

fn required_registry_bool(value: &Value, field: &str) -> Result<bool, String> {
    value
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| "SKILL_REGISTRY_SCHEMA_INVALID".to_string())
}

fn parse_skill_registry(bytes: &[u8]) -> Result<SkillRegistry, String> {
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| "SKILL_REGISTRY_INVALID".to_string())?;
    let skills = root
        .get("skills")
        .and_then(Value::as_array)
        .ok_or_else(|| "SKILL_REGISTRY_SKILLS_MISSING".to_string())?
        .iter()
        .map(|value| {
            let coverage = match value.get("coverage") {
                Some(Value::String(_)) | Some(Value::Array(_)) => {
                    let parsed = string_array(value.get("coverage"));
                    if parsed.is_empty()
                        || value
                            .get("coverage")
                            .and_then(Value::as_array)
                            .is_some_and(|items| items.len() != parsed.len())
                    {
                        return Err("SKILL_REGISTRY_SCHEMA_INVALID".to_string());
                    }
                    parsed
                }
                _ => return Err("SKILL_REGISTRY_SCHEMA_INVALID".into()),
            };
            let method_resource = match value.get("methodResource") {
                Some(Value::String(path)) if skill_deployment::is_safe_relative_path(path) => {
                    Some(path.clone())
                }
                Some(Value::Null) | None => None,
                _ => return Err("SKILL_REGISTRY_SCHEMA_INVALID".into()),
            };
            let skill = SkillMetadata {
                id: required_registry_string(value, "id")?,
                slug: required_registry_string(value, "slug")?,
                name_zh: required_registry_string(value, "name_zh")?,
                version: required_registry_string(value, "version")?,
                category: required_registry_string(value, "category")?,
                role: required_registry_string(value, "role")?,
                runtime_stage: required_registry_string(value, "runtimeStage")?,
                context_policy: required_registry_string(value, "contextPolicy")?,
                mutation_policy: required_registry_string(value, "mutationPolicy")?,
                coverage,
                bundle_digest: required_registry_string(value, "bundleDigest")?,
                method_resource,
                authority_access: required_registry_string(value, "authorityAccess")?,
                hard_gate_authority: required_registry_bool(value, "hardGateAuthority")?,
                control_decision_authority: required_registry_bool(
                    value,
                    "controlDecisionAuthority",
                )?,
            };
            if skill.id.trim().is_empty()
                || skill.slug.trim().is_empty()
                || skill.version != CREATIVE_SKILLS_VERSION
                || skill.authority_access != "NONE_DIRECT"
                || skill.hard_gate_authority
                || skill.control_decision_authority
                || skill
                    .bundle_digest
                    .strip_prefix("sha256:")
                    .is_none_or(|digest| !skill_deployment::is_sha256(digest))
            {
                return Err("SKILL_REGISTRY_ENTRY_INVALID".to_string());
            }
            Ok(skill)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let package = required_registry_string(&root, "package")?;
    let version = required_registry_string(&root, "version")?;
    let selection_policy = required_registry_string(&root, "selectionPolicy")?;
    required_registry_string(&root, "methodPolicy")?;
    required_registry_string(&root, "runtimeBoundary")?;
    let skill_count = root
        .get("skillCount")
        .and_then(Value::as_u64)
        .ok_or_else(|| "SKILL_REGISTRY_SCHEMA_INVALID".to_string())? as usize;
    if version != CREATIVE_SKILLS_VERSION || skill_count != 45 || skills.len() != 45 {
        return Err("SKILL_REGISTRY_VERSION_OR_COUNT_INVALID".into());
    }
    let ids = skills
        .iter()
        .map(|skill| skill.id.as_str())
        .collect::<BTreeSet<_>>();
    let slugs = skills
        .iter()
        .map(|skill| skill.slug.as_str())
        .collect::<BTreeSet<_>>();
    if ids.len() != skills.len() || slugs.len() != skills.len() {
        return Err("SKILL_REGISTRY_DUPLICATE_OR_UNKNOWN_ENTRY".into());
    }
    Ok(SkillRegistry {
        package,
        version,
        skill_count,
        selection_policy,
        skills,
    })
}

/// Development/test convenience. Release call paths load the registry through
/// the application resource resolver held by SkillBundleLoader.
#[cfg(any(test, debug_assertions))]
pub fn load_registry() -> Result<SkillRegistry, String> {
    SkillBundleLoader::development_package().load_registry()
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

fn select_from_registry(
    registry: SkillRegistry,
    request: SkillSelectionRequest,
) -> Result<SkillSelection, String> {
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

#[cfg(any(test, debug_assertions))]
pub fn select_creative_skills(request: SkillSelectionRequest) -> Result<SkillSelection, String> {
    SkillBundleLoader::development_package().select(request)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoadedSkillText {
    pub reference: String,
    pub digest: String,
    #[serde(skip_serializing)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoadedResource {
    pub reference: String,
    pub digest: String,
    #[serde(skip_serializing)]
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkillLoadingEvidence {
    pub selected_full_text_loads: usize,
    pub unselected_full_text_loads: usize,
    pub selected_resource_loads: usize,
    pub unselected_resource_loads: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedSkillFileDigest {
    pub skill_id: String,
    pub reference: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillResourceIdentity {
    pub package_version: String,
    pub source_archive_sha256: String,
    pub deployed_manifest_digest: String,
    pub registry_sha256: String,
    pub selected_file_digests: Vec<SelectedSkillFileDigest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoadedSelectedSkills {
    pub skill_texts: Vec<LoadedSkillText>,
    pub resources: Vec<LoadedResource>,
    pub loading: SkillLoadingEvidence,
    pub identity: SkillResourceIdentity,
}

/// Uses the same strict loader for development and installed resources. Only
/// the application composition root chooses which trusted roots to inject.
#[derive(Clone)]
pub struct SkillBundleLoader {
    package_root: PathBuf,
    manifest_path: PathBuf,
    expected_manifest_digest: String,
    // The bundle and registry are immutable per app process; selected files are
    // still read and hashed on every use, while process restart invalidates both caches.
    deployment: Arc<OnceLock<Result<VerifiedSkillDeployment, String>>>,
    registry: Arc<OnceLock<Result<SkillRegistry, String>>>,
}

impl SkillBundleLoader {
    pub fn from_application_package_root(package_root: PathBuf) -> Self {
        Self {
            manifest_path: package_root.join(skill_deployment::DEPLOYMENT_MANIFEST_NAME),
            package_root,
            expected_manifest_digest: env!("CREATIVE_SKILLS_DEPLOYMENT_MANIFEST_SHA256").into(),
            deployment: Arc::new(OnceLock::new()),
            registry: Arc::new(OnceLock::new()),
        }
    }

    pub fn diagnostic_report(&self) -> Result<Value, String> {
        let selection = self.select(SkillSelectionRequest {
            primary: "S14".into(),
            support: vec!["S10".into()],
            budget_reason: None,
            context_tokens: 1200,
            model_calls: 1,
        })?;
        let loaded = self.load_selected(&selection, &["resources/operating_method.md".into()])?;
        Ok(json!({
            "schema":"pre01-skill-resource-diagnostic-r1",
            "identity":loaded.identity,
            "loading":loaded.loading,
            "selectedSkillIds":selection.selected_ids,
            "networkCalls":0,
            "authorityWrites":0,
            "receiptCreated":false
        }))
    }

    #[cfg(any(test, debug_assertions))]
    pub fn development_package() -> Self {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let package_root = manifest_dir.join("../creative-skills/v1.2.0");
        Self {
            manifest_path: manifest_dir.join("resources/creative-skills-v1.2.0.deployment.json"),
            package_root,
            expected_manifest_digest: env!("CREATIVE_SKILLS_DEPLOYMENT_MANIFEST_SHA256").into(),
            deployment: Arc::new(OnceLock::new()),
            registry: Arc::new(OnceLock::new()),
        }
    }

    #[cfg(test)]
    fn for_test(
        package_root: PathBuf,
        manifest_path: PathBuf,
        expected_manifest_digest: String,
    ) -> Self {
        Self {
            package_root,
            manifest_path,
            expected_manifest_digest,
            deployment: Arc::new(OnceLock::new()),
            registry: Arc::new(OnceLock::new()),
        }
    }

    fn verified_deployment(&self) -> Result<VerifiedSkillDeployment, String> {
        self.deployment
            .get_or_init(|| {
                skill_deployment::load_runtime_manifest(
                    &self.manifest_path,
                    &self.expected_manifest_digest,
                )
            })
            .clone()
    }

    fn read_verified_file(
        &self,
        deployment: &VerifiedSkillDeployment,
        relative: &str,
    ) -> Result<Vec<u8>, String> {
        skill_deployment::read_verified_file(&self.package_root, deployment, relative)
    }

    pub fn load_registry(&self) -> Result<SkillRegistry, String> {
        self.registry
            .get_or_init(|| {
                let deployment = self.verified_deployment()?;
                let bytes = self.read_verified_file(&deployment, "registry.json")?;
                parse_skill_registry(&bytes)
            })
            .clone()
    }

    pub fn select(&self, request: SkillSelectionRequest) -> Result<SkillSelection, String> {
        select_from_registry(self.load_registry()?, request)
    }

    fn validate_selected_skill(
        &self,
        skill: &SkillMetadata,
        registry: &SkillRegistry,
    ) -> Result<(), String> {
        let registered = registry
            .skills
            .iter()
            .find(|registered| registered.id == skill.id)
            .ok_or_else(|| "SKILL_NOT_IN_DEPLOYED_REGISTRY".to_string())?;
        if registered.slug != skill.slug || registered.version != skill.version {
            return Err("SKILL_SELECTION_DEPLOYMENT_MISMATCH".into());
        }
        Ok(())
    }

    fn load_text(
        &self,
        deployment: &VerifiedSkillDeployment,
        skill_id: &str,
        relative: &str,
    ) -> Result<(String, SelectedSkillFileDigest), String> {
        let bytes = self.read_verified_file(deployment, relative)?;
        let text = String::from_utf8(bytes.clone())
            .map_err(|_| format!("SKILL_PACKAGE_UTF8_INVALID:{relative}"))?;
        let digest = hex::encode(Sha256::digest(&bytes));
        let reference = format!("creative-skills/v1.2.0/{relative}");
        Ok((
            text,
            SelectedSkillFileDigest {
                skill_id: skill_id.into(),
                reference,
                size_bytes: bytes.len() as u64,
                sha256: digest,
            },
        ))
    }

    pub fn load_selected(
        &self,
        selection: &SkillSelection,
        required_resources: &[String],
    ) -> Result<LoadedSelectedSkills, String> {
        let deployment = self.verified_deployment()?;
        let registry = self.load_registry()?;
        let mut loaded_skill_texts = Vec::with_capacity(selection.selected.len());
        let mut loaded_resources = Vec::new();
        let mut selected_file_digests = Vec::new();
        for skill in &selection.selected {
            self.validate_selected_skill(skill, &registry)?;
            let relative = format!("skills/{}/SKILL.md", skill.slug);
            let (text, file_digest) = self.load_text(&deployment, &skill.id, &relative)?;
            loaded_skill_texts.push(LoadedSkillText {
                reference: file_digest.reference.clone(),
                digest: file_digest.sha256.clone(),
                text,
            });
            selected_file_digests.push(file_digest);
            for resource in required_resources {
                if !resource.starts_with("resources/")
                    || !skill_deployment::is_safe_relative_path(resource)
                {
                    return Err("SKILL_RESOURCE_PATH_DENIED".into());
                }
                let resource_relative = format!("skills/{}/{resource}", skill.slug);
                let (text, file_digest) =
                    self.load_text(&deployment, &skill.id, &resource_relative)?;
                loaded_resources.push(LoadedResource {
                    reference: file_digest.reference.clone(),
                    digest: file_digest.sha256.clone(),
                    text,
                });
                selected_file_digests.push(file_digest);
            }
        }
        let selected_resource_loads = loaded_resources.len();
        Ok(LoadedSelectedSkills {
            skill_texts: loaded_skill_texts,
            resources: loaded_resources,
            loading: SkillLoadingEvidence {
                selected_full_text_loads: selection.selected.len(),
                unselected_full_text_loads: 0,
                selected_resource_loads,
                unselected_resource_loads: 0,
            },
            identity: SkillResourceIdentity {
                package_version: deployment.package_version.clone(),
                source_archive_sha256: deployment.source_archive_sha256.clone(),
                deployed_manifest_digest: deployment.manifest_digest.clone(),
                registry_sha256: deployment
                    .file("registry.json")
                    .ok_or_else(|| "SKILL_REGISTRY_NOT_IN_DEPLOYMENT_MANIFEST".to_string())?
                    .sha256
                    .clone(),
                selected_file_digests,
            },
        })
    }

    pub fn load_selected_resource(
        &self,
        selection: &SkillSelection,
        skill_id: &str,
        resource: &str,
    ) -> Result<LoadedResource, String> {
        let deployment = self.verified_deployment()?;
        let registry = self.load_registry()?;
        let skill = selection
            .selected
            .iter()
            .find(|skill| skill.id == skill_id)
            .ok_or_else(|| "SKILL_NOT_SELECTED".to_string())?;
        self.validate_selected_skill(skill, &registry)?;
        if !resource.starts_with("resources/") || !skill_deployment::is_safe_relative_path(resource)
        {
            return Err("SKILL_RESOURCE_PATH_DENIED".into());
        }
        let relative = format!("skills/{}/{resource}", skill.slug);
        let (text, file_digest) = self.load_text(&deployment, skill_id, &relative)?;
        Ok(LoadedResource {
            reference: file_digest.reference,
            digest: file_digest.sha256,
            text,
        })
    }
}

fn current_rfc3339_utc() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let days = seconds / 86_400;
    let day_seconds = seconds % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        day_seconds / 3_600,
        (day_seconds % 3_600) / 60,
        day_seconds % 60
    )
}

// Howard Hinnant's proleptic Gregorian conversion, kept local to avoid a new
// time dependency for one provenance timestamp.
fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    (year + if month <= 2 { 1 } else { 0 }, month, day)
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

/// Builds the bounded endgame context used by B14 S44/S45. It reuses the
/// frozen B10 context builder and only narrows the stage/profile; it never
/// reads manuscript bodies or opens a second skill runtime.
pub fn build_endgame_skill_task_context(
    project_id: &str,
    task_id: &str,
    project_revision: i64,
    context_manifest_ref: &str,
    endgame_payload: &Value,
) -> Result<SkillTaskContext, String> {
    let summary = endgame_payload
        .get("requestedEnding")
        .and_then(|value| value.get("summary"))
        .and_then(Value::as_str)
        .unwrap_or("完成全书终局检查");
    let mut context = build_skill_task_context(
        project_id,
        task_id,
        project_revision,
        context_manifest_ref,
        &serde_json::json!({
            "artifactId": "endgame-context",
            "chapterGoal": summary,
            "activeTaskRefs": []
        }),
    )?;
    context.runtime_stage = "ENDGAME_COMPLETION".into();
    context.bounded_refs.push(BoundedRef {
        kind: "EndgameConstraintSnapshot".into(),
        reference: "snapshot:b14:endgame-constraints".into(),
    });
    context.quality_inputs = serde_json::json!({
        "source": "B14_ENDGAME_BOUNDED",
        "requestedEnding": endgame_payload.get("requestedEnding"),
        "hardResponsibilitySummary": endgame_payload.get("hardResponsibilities"),
        "readerPromise": endgame_payload.get("readerPromise"),
        "characterArcSummary": endgame_payload.get("characterArcSummary"),
        "finalVolumeSummary": endgame_payload.get("finalVolumeSummary")
    });
    context.method_profile = serde_json::json!({
        "profile": "ENDGAME_COMPLETION",
        "version": "b14.r3",
        "softOnly": true
    });
    let mut digest_input = serde_json::to_value(&context).map_err(|error| error.to_string())?;
    digest_input
        .as_object_mut()
        .expect("SkillTaskContext serializes as object")
        .remove("contextDigest");
    context.context_digest = sha256_canonical(&digest_input).map_err(|error| error.to_string())?;
    Ok(context)
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_archive_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployed_manifest_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_file_digests: Vec<SelectedSkillFileDigest>,
    pub role: String,
    pub loaded_skill_text_ref: String,
    pub loaded_skill_text_digest: String,
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

fn method_signals(text: &str) -> Vec<&'static str> {
    [
        ("ending_mode", "ending_mode"),
        ("open_question", "open_question"),
        ("next_click_reason", "next_click_reason"),
        ("hook_type", "hook_type"),
        ("information_gap", "information_gap"),
        ("rhythm_plan", "rhythm_plan"),
        ("intensity_curve", "intensity_curve"),
        ("exposition_budget", "exposition_budget"),
        ("repair_plan", "repair_plan"),
        ("recheck_dimensions", "recheck_dimensions"),
    ]
    .into_iter()
    .filter_map(|(needle, signal)| text.contains(needle).then_some(signal))
    .collect()
}

pub fn build_method_guidance(
    skill_texts: &[LoadedSkillText],
    resources: &[LoadedResource],
) -> String {
    skill_texts
        .iter()
        .map(|loaded| {
            let signals = method_signals(&loaded.text).join(",");
            let resource_signals = resources
                .iter()
                .filter(|resource| {
                    resource
                        .reference
                        .split("/skills/")
                        .nth(1)
                        .and_then(|tail| tail.split('/').next())
                        .is_some_and(|slug| loaded.reference.contains(slug))
                })
                .flat_map(|resource| method_signals(&resource.text))
                .collect::<Vec<_>>();
            format!(
                "methodRef={} methodDigest={} signals={} resourceSignals={}",
                loaded.reference,
                loaded.digest,
                if signals.is_empty() { "none" } else { &signals },
                if resource_signals.is_empty() {
                    "none".into()
                } else {
                    resource_signals.join(",")
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(clippy::too_many_arguments)]
fn create_invocation_provenance_at(
    invocation_id: &str,
    skill: &SkillMetadata,
    context: &SkillTaskContext,
    loaded_skill_text: &LoadedSkillText,
    resources: &[LoadedResource],
    asset_identity: Option<&SkillResourceIdentity>,
    project_context_refs: Vec<String>,
    output_digest: String,
    created_at: &str,
) -> Result<InvocationProvenance, String> {
    if invocation_id.trim().is_empty()
        || created_at.trim().is_empty()
        || created_at.starts_with("1970-01-01")
    {
        return Err("SKILL_INVOCATION_PROVENANCE_INVALID".into());
    }
    Ok(InvocationProvenance {
        invocation_id: invocation_id.into(),
        skill_id: skill.id.clone(),
        skill_version: CREATIVE_SKILLS_VERSION.into(),
        bundle_digest: skill.bundle_digest.clone(),
        source_archive_sha256: asset_identity
            .map(|identity| identity.source_archive_sha256.clone()),
        deployed_manifest_digest: asset_identity
            .map(|identity| identity.deployed_manifest_digest.clone()),
        selected_file_digests: asset_identity
            .map(|identity| {
                identity
                    .selected_file_digests
                    .iter()
                    .filter(|file| file.skill_id == skill.id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        role: skill.role.clone(),
        loaded_skill_text_ref: loaded_skill_text.reference.clone(),
        loaded_skill_text_digest: loaded_skill_text.digest.clone(),
        resource_refs: resources
            .iter()
            .map(|resource| resource.reference.clone())
            .collect(),
        resource_digests: resources
            .iter()
            .map(|resource| json!({"ref":resource.reference,"digest":resource.digest}))
            .collect(),
        context_ref: context.context_manifest_ref.clone(),
        context_digest: context.context_digest.clone(),
        context_manifest_ref: context.context_manifest_ref.clone(),
        source_refs: vec![loaded_skill_text.reference.clone()],
        method_refs: vec![loaded_skill_text.reference.clone()],
        project_context_refs,
        runtime_check_requests: Vec::new(),
        project_id: context.project_id.clone(),
        project_revision: context.project_revision,
        model: "deterministic-skill-runtime".into(),
        provider: "deterministic".into(),
        model_run_refs: Vec::new(),
        tokens: BTreeMap::from([("input".into(), 0), ("output".into(), 0)]),
        cost_cents: 0,
        output_digest,
        created_at: created_at.into(),
    })
}

pub fn create_invocation_provenance(
    invocation_id: &str,
    skill: &SkillMetadata,
    context: &SkillTaskContext,
    method_refs: Vec<String>,
    project_context_refs: Vec<String>,
    output_digest: String,
) -> Result<InvocationProvenance, String> {
    let loaded_skill_text = LoadedSkillText {
        reference: method_refs
            .first()
            .cloned()
            .unwrap_or_else(|| "method:unbound".into()),
        digest: skill.bundle_digest.trim_start_matches("sha256:").into(),
        text: String::new(),
    };
    let mut provenance = create_invocation_provenance_at(
        invocation_id,
        skill,
        context,
        &loaded_skill_text,
        &[],
        None,
        project_context_refs,
        output_digest,
        &current_rfc3339_utc(),
    )?;
    provenance.method_refs = method_refs.clone();
    provenance.source_refs = method_refs;
    Ok(provenance)
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
    pub signal: String,
    pub evidence_ranges: Vec<QualityRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualityRange {
    pub start: usize,
    pub end: usize,
    pub quote: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QualityFinding {
    pub finding_id: String,
    pub dimension: String,
    pub severity: String,
    pub evidence: Vec<String>,
    pub repairable: bool,
    pub reason: String,
    pub evidence_ranges: Vec<QualityRange>,
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
    #[serde(default)]
    pub evaluator_kind: String,
    pub model_provenance: QualityModelProvenance,
    pub invocation_refs: Vec<String>,
    pub rechecked_dimensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair: Option<Value>,
    pub digest: String,
    pub soft_only: bool,
}

fn count_any(body: &str, needles: &[&str]) -> usize {
    needles
        .iter()
        .map(|needle| body.matches(needle).count())
        .sum()
}

fn first_quality_ranges(body: &str, needles: &[&str]) -> Vec<QualityRange> {
    needles
        .iter()
        .find_map(|needle| {
            let byte_start = body.find(needle)?;
            let byte_end = byte_start + needle.len();
            Some(QualityRange {
                start: body[..byte_start].chars().count(),
                end: body[..byte_end].chars().count(),
                quote: body[byte_start..byte_end].into(),
            })
        })
        .into_iter()
        .collect()
}

fn sentence_units(body: &str) -> Vec<String> {
    body.split(['。', '！', '？', '!', '?'])
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn duplicate_sentence_count(sentences: &[String]) -> usize {
    let mut counts = BTreeMap::new();
    for sentence in sentences {
        *counts.entry(sentence).or_insert(0_usize) += 1;
    }
    counts.values().map(|count| count.saturating_sub(1)).sum()
}

fn taste_text(profile: &NarrativeTasteProfile, field: &str) -> String {
    profile
        .profile
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or("默认")
        .into()
}

fn dimension(body: &str, score: i64, signal: String, needles: &[&str]) -> QualityDimension {
    let score = score.clamp(0, 100);
    let ranges = first_quality_ranges(body, needles);
    let evidence = if ranges.is_empty() {
        vec![format!("signal:{signal}")]
    } else {
        ranges
            .iter()
            .map(|range| format!("{}:{}-{}", range.quote, range.start, range.end))
            .collect()
    };
    QualityDimension {
        score,
        severity: if score < 45 { "WARN" } else { "INFO" }.into(),
        evidence,
        signal,
        evidence_ranges: ranges,
    }
}

pub fn evaluate_narrative_quality(
    candidate_ref: &ArtifactRef,
    project_id: &str,
    revision: i64,
    body: &str,
    taste_profile: &NarrativeTasteProfile,
    invocation_refs: Vec<String>,
) -> Result<NarrativeQualityReport, String> {
    let sentences = sentence_units(body);
    let sentence_count = sentences.len().max(1) as i64;
    let initiative = count_any(
        body,
        &[
            "决定", "选择", "推开", "拔刀", "冲向", "走向", "拒绝", "发誓", "主动", "decide",
            "choose",
        ],
    );
    let passive = count_any(
        body,
        &["被", "只能", "等待", "任由", "陷入", "被迫", "passively"],
    );
    let desire = count_any(
        body,
        &[
            "想", "要", "必须", "希望", "渴望", "目标", "为了", "want", "must",
        ],
    );
    let unresolved = count_any(
        body,
        &[
            "？", "?", "却", "突然", "门后", "谁", "为何", "秘密", "传来", "然而", "但",
        ],
    );
    let closure = count_any(body, &["结束", "完成", "关闭", "安静地结束", "终于平静"]);
    let action = count_any(
        body,
        &[
            "推", "走", "跑", "拔", "抓", "打", "看", "打开", "冲", "决定",
        ],
    );
    let dialogue_count = body.matches('“').count() + body.matches('"').count() / 2;
    let subtext = count_any(
        body,
        &[
            "沉默",
            "没有说",
            "却笑",
            "眼神",
            "停顿",
            "其实",
            "不敢",
            "沉默",
        ],
    );
    let exposition = count_any(
        body,
        &[
            "所谓",
            "也就是说",
            "历史",
            "制度",
            "地理",
            "定义",
            "资料",
            "因为",
            "据说",
        ],
    );
    let repeated_sentences = duplicate_sentence_count(&sentences);
    let voice_profile = taste_text(taste_profile, "voice");
    let pacing_profile = taste_text(taste_profile, "pacing");
    let sentence_variation = sentences
        .windows(2)
        .filter(|window| window[0].chars().count() != window[1].chars().count())
        .count();
    let emotional = count_any(
        body,
        &[
            "害怕", "愤怒", "痛", "笑", "哭", "失去", "终于", "代价", "心",
        ],
    );
    let mut dimensions: BTreeMap<String, QualityDimension> = BTreeMap::new();
    dimensions.insert(
        "agency".into(),
        dimension(
            body,
            45 + (initiative as i64 * 12).min(36) - (passive as i64 * 15).min(45),
            format!("initiative={initiative};passive={passive}"),
            &["决定", "选择", "推开", "被", "只能"],
        ),
    );
    dimensions.insert(
        "desire".into(),
        dimension(
            body,
            35 + (desire as i64 * 15).min(60),
            format!("desireMarkers={desire}"),
            &["想", "要", "必须", "希望", "目标", "为了"],
        ),
    );
    dimensions.insert(
        "hook".into(),
        dimension(
            body,
            42 + (unresolved as i64 * 12).min(48) - (closure as i64 * 18).min(36),
            format!("unresolved={unresolved};closure={closure}"),
            &["？", "?", "却", "突然", "门后", "传来", "结束"],
        ),
    );
    dimensions.insert(
        "pacing".into(),
        dimension(
            body,
            35 + (action as i64 * 8).min(35) + (sentence_variation as i64 * 5).min(20)
                + if pacing_profile.contains('紧') { 5 } else { 0 },
            format!("sentences={sentence_count};actions={action};variation={sentence_variation};profile={pacing_profile}"),
            &["推", "走", "跑", "突然", "。"],
        ),
    );
    dimensions.insert(
        "voice".into(),
        dimension(
            body,
            48 + (body.matches('，').count() as i64).min(12)
                + if voice_profile.contains("克制") {
                    12
                } else if voice_profile.contains("华丽") {
                    -8
                } else {
                    0
                },
            format!(
                "punctuation={};tasteVoice={voice_profile}",
                body.matches('，').count()
            ),
            &["，", "。", "！", "？"],
        ),
    );
    dimensions.insert(
        "dialogue".into(),
        dimension(
            body,
            20 + (dialogue_count as i64 * 22).min(70),
            format!("dialoguePairs={dialogue_count}"),
            &["“", "\"", "说", "回答"],
        ),
    );
    dimensions.insert(
        "subtext".into(),
        dimension(
            body,
            35 + (subtext as i64 * 15).min(60),
            format!("subtextMarkers={subtext}"),
            &["沉默", "没有说", "眼神", "其实", "不敢"],
        ),
    );
    dimensions.insert(
        "repetition".into(),
        dimension(
            body,
            100 - (repeated_sentences as i64 * 25).min(85),
            format!("sentences={sentence_count};duplicateSentences={repeated_sentences}"),
            &["门后传来脚步声", "重复"],
        ),
    );
    dimensions.insert(
        "exposition".into(),
        dimension(
            body,
            75 - (exposition as i64 * 12).min(55)
                - if dialogue_count == 0 && body.chars().count() > 24 {
                    12
                } else {
                    0
                }
                + if dialogue_count > 0 { 8 } else { 0 },
            format!("expositionMarkers={exposition};dialoguePairs={dialogue_count}"),
            &["所谓", "历史", "制度", "地理", "也就是说", "资料"],
        ),
    );
    dimensions.insert(
        "emotionalPayoff".into(),
        dimension(
            body,
            35 + (emotional as i64 * 10).min(35)
                + (initiative as i64 * 8).min(24)
                + if closure > 0 { 8 } else { 0 },
            format!("emotionMarkers={emotional};causalActions={initiative};closure={closure}"),
            &["终于", "代价", "决定", "笑", "哭", "失去"],
        ),
    );
    let findings = dimensions
        .iter()
        .filter(|(_, item)| item.score < 45)
        .map(|(dimension_name, item)| QualityFinding {
            finding_id: format!("quality:{}:{dimension_name}", candidate_ref.artifact_id),
            dimension: dimension_name.clone(),
            severity: item.severity.clone(),
            evidence: item.evidence.clone(),
            repairable: matches!(
                dimension_name.as_str(),
                "agency" | "hook" | "dialogue" | "pacing" | "repetition" | "exposition"
            ),
            reason: item.signal.clone(),
            evidence_ranges: item.evidence_ranges.clone(),
        })
        .collect::<Vec<_>>();
    let repairable_dimensions = findings
        .iter()
        .filter(|finding| finding.repairable)
        .map(|finding| finding.dimension.clone())
        .collect::<Vec<_>>();
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
        "repairableDimensions":repairable_dimensions.clone(),
        "runtimeChecks":[],
        "softAssessment":{"readerDesire":"SOFT","qualityScore":quality_score},
        "evaluatorKind":"HEURISTIC_KEYWORD",
        "modelProvenance":{"evaluator":"HEURISTIC_KEYWORD","evaluatorCalls":0,"provider":"none","model":"none"},
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
        repairable_dimensions,
        runtime_checks: Vec::new(),
        soft_assessment: json!({"readerDesire":"SOFT","qualityScore":quality_score}),
        evaluator_kind: "HEURISTIC_KEYWORD".into(),
        model_provenance: QualityModelProvenance {
            evaluator: "HEURISTIC_KEYWORD".into(),
            evaluator_calls: 0,
            provider: "none".into(),
            model: "none".into(),
        },
        invocation_refs,
        rechecked_dimensions: Vec::new(),
        repair: None,
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

pub fn rebind_quality_candidate(
    report: &mut NarrativeQualityReport,
    candidate_ref: ArtifactRef,
) -> Result<(), String> {
    report.candidate_ref = candidate_ref;
    let mut digest_input = serde_json::to_value(&report).map_err(|error| error.to_string())?;
    digest_input["digest"] = Value::String(String::new());
    report.digest = sha256_canonical(&digest_input).map_err(|error| error.to_string())?;
    Ok(())
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
    let deterministic = request.model.starts_with("deterministic-");
    if request.context_tokens <= 0 || (!deterministic && request.model_calls <= 0) {
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
        transport_calls: if deterministic { 0 } else { 1 },
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
    pub source_archive_sha256: String,
    pub deployed_manifest_digest: String,
    pub selected_file_digests: Vec<SelectedSkillFileDigest>,
    pub selection: SkillSelection,
    pub context: SkillTaskContext,
    pub proposal: NormalizedSkillOutput,
    pub taste_profile: NarrativeTasteProfile,
    pub loaded_skill_texts: Vec<LoadedSkillText>,
    pub loaded_resources: Vec<LoadedResource>,
    pub loading: SkillLoadingEvidence,
    pub method_guidance: String,
    pub invocations: Vec<InvocationProvenance>,
}

/// B08 calls this pure composition step immediately before rendering. It adds
/// bounded creative guidance to the existing render input; it does not create
/// a ChapterPlan or perform any write.
#[cfg(any(test, debug_assertions))]
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
    enrich_chapter_with_policy(
        project_id,
        chapter_id,
        revision,
        context_manifest_ref,
        plan_payload,
        execution_mode,
        primary,
        support,
        Vec::new(),
        &current_rfc3339_utc(),
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(any(test, debug_assertions))]
pub fn enrich_chapter_with_policy(
    project_id: &str,
    chapter_id: &str,
    revision: i64,
    context_manifest_ref: &str,
    plan_payload: &Value,
    execution_mode: &str,
    primary: &str,
    support: Vec<String>,
    required_resources: Vec<String>,
    now: &str,
) -> Result<ChapterSkillEnrichment, String> {
    let loader = SkillBundleLoader::development_package();
    enrich_chapter_with_loader(
        project_id,
        chapter_id,
        revision,
        context_manifest_ref,
        plan_payload,
        execution_mode,
        primary,
        support,
        required_resources,
        now,
        &loader,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn enrich_chapter_with_loader(
    project_id: &str,
    chapter_id: &str,
    revision: i64,
    context_manifest_ref: &str,
    plan_payload: &Value,
    execution_mode: &str,
    primary: &str,
    support: Vec<String>,
    required_resources: Vec<String>,
    now: &str,
    loader: &SkillBundleLoader,
) -> Result<ChapterSkillEnrichment, String> {
    let selection = loader.select(SkillSelectionRequest {
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
    let loaded = loader.load_selected(&selection, &required_resources)?;
    let method_guidance = build_method_guidance(&loaded.skill_texts, &loaded.resources);
    let proposal = normalize_skill_output(json!({
        "kind":"CreativeProposal",
        "status":"OK",
        "summary":format!("{}：已加载并消费选中方法（{}）\n{}", selection.primary.name_zh, execution_mode, method_guidance),
        "suggestions":["保留 ChapterPlan 的目标与边界；只对候选正文提供局部建议"],
        "runtimeCheckRequests":[]
    }))?;
    let output_digest =
        sha256_canonical(&serde_json::to_value(&proposal).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let invocations = selection
        .selected
        .iter()
        .enumerate()
        .map(|(index, skill)| {
            let loaded_text = loaded
                .skill_texts
                .iter()
                .find(|loaded| {
                    loaded
                        .reference
                        .ends_with(&format!("skills/{}/SKILL.md", skill.slug))
                })
                .ok_or_else(|| "SKILL_LOADED_TEXT_MISSING".to_string())?;
            let resources = loaded
                .resources
                .iter()
                .filter(|resource| {
                    resource
                        .reference
                        .contains(&format!("skills/{}/", skill.slug))
                })
                .cloned()
                .collect::<Vec<_>>();
            create_invocation_provenance_at(
                &format!("inv:{chapter_id}:{}", index + 1),
                skill,
                &context,
                loaded_text,
                &resources,
                Some(&loaded.identity),
                vec![context_manifest_ref.into(), format!("plan:{chapter_id}")],
                output_digest.clone(),
                now,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ChapterSkillEnrichment {
        package_version: CREATIVE_SKILLS_VERSION.into(),
        package_digest: loaded.identity.deployed_manifest_digest.clone(),
        source_archive_sha256: loaded.identity.source_archive_sha256.clone(),
        deployed_manifest_digest: loaded.identity.deployed_manifest_digest.clone(),
        selected_file_digests: loaded.identity.selected_file_digests.clone(),
        selection,
        context,
        proposal,
        taste_profile: default_taste_profile(revision),
        loaded_skill_texts: loaded.skill_texts,
        loaded_resources: loaded.resources,
        loading: loaded.loading,
        method_guidance,
        invocations,
    })
}

#[cfg(test)]
mod pre01_skill_resource_tests {
    use super::*;

    fn copy_directory(source: &std::path::Path, destination: &std::path::Path) {
        fs::create_dir_all(destination).unwrap();
        for entry in fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            let file_type = entry.file_type().unwrap();
            if file_type.is_dir() {
                copy_directory(&source_path, &destination_path);
            } else {
                assert!(
                    file_type.is_file(),
                    "test fixture must not contain symlinks"
                );
                fs::copy(source_path, destination_path).unwrap();
            }
        }
    }

    fn copy_skill_package_for_test() -> (tempfile::TempDir, PathBuf, String) {
        let temp = tempfile::tempdir().unwrap();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let package_source = manifest_dir.join("../creative-skills/v1.2.0");
        let package_root = temp.path().join("creative-skills/v1.2.0");
        copy_directory(&package_source, &package_root);
        let manifest_source = manifest_dir.join("resources/creative-skills-v1.2.0.deployment.json");
        let manifest_path = temp
            .path()
            .join("creative-skills/v1.2.0/deployment-manifest.json");
        fs::copy(&manifest_source, &manifest_path).unwrap();
        let expected_manifest_digest =
            hex::encode(Sha256::digest(fs::read(&manifest_path).unwrap()));
        (temp, manifest_path, expected_manifest_digest)
    }

    fn selection(loader: &SkillBundleLoader) -> SkillSelection {
        loader
            .select(SkillSelectionRequest {
                primary: "S14".into(),
                support: vec!["S10".into()],
                budget_reason: None,
                context_tokens: 1200,
                model_calls: 1,
            })
            .unwrap()
    }

    #[test]
    fn loader_reports_actual_deployment_and_selected_file_digests() {
        let loader = SkillBundleLoader::development_package();
        let selected = selection(&loader);
        let loaded = loader
            .load_selected(&selected, &["resources/operating_method.md".into()])
            .unwrap();

        assert_eq!(loaded.identity.package_version, "1.2.0");
        assert_eq!(
            loaded.identity.source_archive_sha256,
            CREATIVE_SKILLS_ARCHIVE_SHA256
        );
        assert_eq!(loaded.identity.registry_sha256.len(), 64);
        assert_eq!(loaded.identity.selected_file_digests.len(), 4);
        assert_eq!(loaded.loading.selected_full_text_loads, 2);
        assert_eq!(loaded.loading.unselected_full_text_loads, 0);
        assert_eq!(loaded.loading.unselected_resource_loads, 0);
        assert!(loaded.identity.deployed_manifest_digest.len() == 64);
    }

    #[test]
    fn loader_rejects_a_changed_selected_method_file() {
        let (root, manifest_path, expected_manifest_digest) = copy_skill_package_for_test();
        let selected_file = root
            .path()
            .join("creative-skills/v1.2.0")
            .join("skills/chapter-ending-next-click-design/SKILL.md");
        fs::write(&selected_file, "tampered method").unwrap();
        let loader = SkillBundleLoader::for_test(
            root.path().join("creative-skills/v1.2.0"),
            manifest_path,
            expected_manifest_digest,
        );
        let selected = selection(&loader);

        assert!(loader
            .load_selected(&selected, &[])
            .unwrap_err()
            .starts_with("SKILL_RESOURCE_DIGEST_MISMATCH"));
    }

    #[test]
    fn loader_rejects_a_missing_selected_method_file() {
        let (root, manifest_path, expected_manifest_digest) = copy_skill_package_for_test();
        let selected_file = root
            .path()
            .join("creative-skills/v1.2.0/skills/chapter-ending-next-click-design/SKILL.md");
        fs::remove_file(selected_file).unwrap();
        let loader = SkillBundleLoader::for_test(
            root.path().join("creative-skills/v1.2.0"),
            manifest_path,
            expected_manifest_digest,
        );
        let selected = selection(&loader);

        assert!(loader
            .load_selected(&selected, &[])
            .unwrap_err()
            .starts_with("SKILL_PACKAGE_FILE_NOT_FOUND"));
    }

    #[test]
    fn loader_rejects_a_manifest_that_differs_from_the_build_binding() {
        let (root, manifest_path, expected_manifest_digest) = copy_skill_package_for_test();
        let mut manifest = fs::read(&manifest_path).unwrap();
        manifest.push(b' ');
        fs::write(&manifest_path, manifest).unwrap();
        let loader = SkillBundleLoader::for_test(
            root.path().join("creative-skills/v1.2.0"),
            manifest_path,
            expected_manifest_digest,
        );

        assert!(loader
            .load_registry()
            .unwrap_err()
            .starts_with("SKILL_DEPLOYMENT_MANIFEST_DIGEST_MISMATCH"));
    }

    #[test]
    fn loader_rejects_a_missing_deployment_manifest() {
        let (root, manifest_path, expected_manifest_digest) = copy_skill_package_for_test();
        fs::remove_file(&manifest_path).unwrap();
        let loader = SkillBundleLoader::for_test(
            root.path().join("creative-skills/v1.2.0"),
            manifest_path,
            expected_manifest_digest,
        );

        assert_eq!(
            loader.load_registry().unwrap_err(),
            "SKILL_DEPLOYMENT_MANIFEST_NOT_FOUND"
        );
    }

    #[test]
    fn registry_rejects_duplicate_ids_and_unknown_selected_skills() {
        let (root, manifest_path, _) = copy_skill_package_for_test();
        let registry_path = root.path().join("creative-skills/v1.2.0/registry.json");
        let mut registry: Value =
            serde_json::from_slice(&fs::read(&registry_path).unwrap()).unwrap();
        let skills = registry["skills"].as_array_mut().unwrap();
        skills[1]["id"] = skills[0]["id"].clone();
        assert_eq!(
            parse_skill_registry(&serde_json::to_vec(&registry).unwrap()).unwrap_err(),
            "SKILL_REGISTRY_DUPLICATE_OR_UNKNOWN_ENTRY"
        );

        let loader = SkillBundleLoader::for_test(
            root.path().join("creative-skills/v1.2.0"),
            manifest_path.clone(),
            {
                let bytes = fs::read(&manifest_path).unwrap();
                hex::encode(Sha256::digest(bytes))
            },
        );
        assert_eq!(
            loader
                .select(SkillSelectionRequest {
                    primary: "UNKNOWN".into(),
                    support: Vec::new(),
                    budget_reason: None,
                    context_tokens: 100,
                    model_calls: 1,
                })
                .unwrap_err(),
            "SKILL_PRIMARY_NOT_FOUND"
        );
    }

    #[test]
    fn registry_rejects_wrong_version_and_malformed_bundle_digest() {
        let package_root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../creative-skills/v1.2.0");
        let mut registry: Value =
            serde_json::from_slice(&fs::read(package_root.join("registry.json")).unwrap()).unwrap();
        registry["version"] = Value::String("1.1.0".into());
        assert_eq!(
            parse_skill_registry(&serde_json::to_vec(&registry).unwrap()).unwrap_err(),
            "SKILL_REGISTRY_VERSION_OR_COUNT_INVALID"
        );

        registry["version"] = Value::String(CREATIVE_SKILLS_VERSION.into());
        registry["skills"][0]["bundleDigest"] = Value::String("not-a-digest".into());
        assert_eq!(
            parse_skill_registry(&serde_json::to_vec(&registry).unwrap()).unwrap_err(),
            "SKILL_REGISTRY_ENTRY_INVALID"
        );
    }

    #[cfg(unix)]
    #[test]
    fn loader_rejects_a_selected_resource_symlink_that_escapes_the_package() {
        use std::os::unix::fs::symlink;

        let (root, manifest_path, expected_manifest_digest) = copy_skill_package_for_test();
        let package_root = root.path().join("creative-skills/v1.2.0");
        let outside = root.path().join("outside-skill.md");
        fs::write(&outside, "outside package root").unwrap();
        let selected_method = package_root.join("skills/chapter-ending-next-click-design/SKILL.md");
        fs::remove_file(&selected_method).unwrap();
        symlink(&outside, &selected_method).unwrap();
        let loader =
            SkillBundleLoader::for_test(package_root, manifest_path, expected_manifest_digest);
        let selected = selection(&loader);

        assert_eq!(
            loader.load_selected(&selected, &[]).unwrap_err(),
            "SKILL_PACKAGE_PATH_DENIED"
        );
    }

    #[test]
    fn loader_rejects_resource_paths_that_escape_the_package_root() {
        let loader = SkillBundleLoader::development_package();
        let selected = selection(&loader);

        assert_eq!(
            loader
                .load_selected_resource(&selected, "S14", "resources/../../registry.json")
                .unwrap_err(),
            "SKILL_RESOURCE_PATH_DENIED"
        );
    }
}
