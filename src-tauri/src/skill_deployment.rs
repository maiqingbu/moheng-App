use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEPLOYMENT_MANIFEST_NAME: &str = "deployment-manifest.json";
pub const DEPLOYMENT_MANIFEST_SCHEMA: &str = "moheng.creative-skills.deployment.v1";
pub const CREATIVE_SKILLS_PACKAGE_VERSION: &str = "1.2.0";
pub const EXPECTED_PACKAGE_ARCHIVE_SHA256: &str =
    "a90ab6e12d82d57b32f06d5a3780d0153d6f372e1efd47df1d70ef1aebbdef8f";
pub const EXPECTED_PROVENANCE_ARCHIVE_SHA256: &str =
    "1aaf54cdc2bfc24fa10042a4a503549a3eaed162473bbecf7127be67f9667c03";

const HASH_ALGORITHM: &str = "SHA-256 over raw file bytes";
const MANIFEST_DIGEST_ALGORITHM: &str = "SHA-256 over raw UTF-8 manifest bytes";
const PATH_ENCODING: &str = "UTF-8 POSIX relative paths";

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillDeploymentFile {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SkillDeploymentManifest {
    pub schema: String,
    pub package_version: String,
    pub source_archive_sha256: String,
    pub provenance_source_archive_sha256: String,
    pub hash_algorithm: String,
    pub manifest_digest_algorithm: String,
    pub path_encoding: String,
    pub file_count: usize,
    pub files: Vec<SkillDeploymentFile>,
}

#[derive(Debug, Clone)]
pub struct VerifiedSkillDeployment {
    pub package_version: String,
    pub source_archive_sha256: String,
    pub provenance_source_archive_sha256: String,
    pub manifest_digest: String,
    files: BTreeMap<String, SkillDeploymentFile>,
}

impl VerifiedSkillDeployment {
    pub fn file(&self, relative: &str) -> Option<&SkillDeploymentFile> {
        self.files.get(relative)
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn file_paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn load_runtime_manifest(
    manifest_path: &Path,
    expected_manifest_digest: &str,
) -> Result<VerifiedSkillDeployment, String> {
    let raw =
        fs::read(manifest_path).map_err(|_| "SKILL_DEPLOYMENT_MANIFEST_NOT_FOUND".to_string())?;
    let actual_digest = sha256_hex(&raw);
    if actual_digest != expected_manifest_digest {
        return Err("SKILL_DEPLOYMENT_MANIFEST_DIGEST_MISMATCH".into());
    }
    parse_manifest(&raw, actual_digest)
}

pub fn verify_source_package(
    package_root: &Path,
    manifest_path: &Path,
) -> Result<VerifiedSkillDeployment, String> {
    let raw =
        fs::read(manifest_path).map_err(|_| "SKILL_DEPLOYMENT_MANIFEST_NOT_FOUND".to_string())?;
    let manifest_digest = sha256_hex(&raw);
    let verified = parse_manifest(&raw, manifest_digest)?;
    if verified.source_archive_sha256 != EXPECTED_PACKAGE_ARCHIVE_SHA256 {
        return Err("SKILL_DEPLOYMENT_SOURCE_ARCHIVE_MISMATCH".into());
    }
    if verified.provenance_source_archive_sha256 != EXPECTED_PROVENANCE_ARCHIVE_SHA256 {
        return Err("SKILL_DEPLOYMENT_SOURCE_PROVENANCE_MISMATCH".into());
    }
    verify_build_provenance(package_root, &verified)?;
    verify_package_files(package_root, &verified)?;
    verify_registry(package_root, &verified)?;
    Ok(verified)
}

pub fn read_verified_file(
    package_root: &Path,
    deployment: &VerifiedSkillDeployment,
    relative: &str,
) -> Result<Vec<u8>, String> {
    if !is_safe_relative_path(relative) {
        return Err("SKILL_PACKAGE_PATH_DENIED".into());
    }
    let expected = deployment
        .file(relative)
        .ok_or_else(|| "SKILL_PACKAGE_FILE_UNDECLARED".to_string())?;
    let root = package_root
        .canonicalize()
        .map_err(|_| "SKILL_PACKAGE_NOT_FOUND".to_string())?;
    let file_path = package_root
        .join(relative)
        .canonicalize()
        .map_err(|_| format!("SKILL_PACKAGE_FILE_NOT_FOUND:{relative}"))?;
    if !file_path.starts_with(&root) {
        return Err("SKILL_PACKAGE_PATH_DENIED".into());
    }
    let bytes =
        fs::read(&file_path).map_err(|_| format!("SKILL_PACKAGE_READ_FAILED:{relative}"))?;
    if bytes.len() as u64 != expected.size_bytes || sha256_hex(&bytes) != expected.sha256 {
        return Err(format!("SKILL_RESOURCE_DIGEST_MISMATCH:{relative}"));
    }
    Ok(bytes)
}

fn parse_manifest(raw: &[u8], manifest_digest: String) -> Result<VerifiedSkillDeployment, String> {
    let manifest: SkillDeploymentManifest =
        serde_json::from_slice(raw).map_err(|_| "SKILL_DEPLOYMENT_MANIFEST_INVALID".to_string())?;
    if manifest.schema != DEPLOYMENT_MANIFEST_SCHEMA
        || manifest.package_version != CREATIVE_SKILLS_PACKAGE_VERSION
        || manifest.hash_algorithm != HASH_ALGORITHM
        || manifest.manifest_digest_algorithm != MANIFEST_DIGEST_ALGORITHM
        || manifest.path_encoding != PATH_ENCODING
        || manifest.file_count != manifest.files.len()
        || !is_sha256(&manifest.source_archive_sha256)
        || !is_sha256(&manifest.provenance_source_archive_sha256)
    {
        return Err("SKILL_DEPLOYMENT_MANIFEST_INVALID".into());
    }
    let mut files = BTreeMap::new();
    let mut previous_path: Option<&str> = None;
    for file in &manifest.files {
        if !is_safe_relative_path(&file.path)
            || file.path == DEPLOYMENT_MANIFEST_NAME
            || !is_sha256(&file.sha256)
            || previous_path.is_some_and(|previous| previous.as_bytes() >= file.path.as_bytes())
        {
            return Err("SKILL_DEPLOYMENT_MANIFEST_INVALID".into());
        }
        previous_path = Some(&file.path);
        files.insert(file.path.clone(), file.clone());
    }
    if files.len() != manifest.file_count {
        return Err("SKILL_DEPLOYMENT_MANIFEST_INVALID".into());
    }
    Ok(VerifiedSkillDeployment {
        package_version: manifest.package_version,
        source_archive_sha256: manifest.source_archive_sha256,
        provenance_source_archive_sha256: manifest.provenance_source_archive_sha256,
        manifest_digest,
        files,
    })
}

pub fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn is_safe_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains(':')
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn verify_build_provenance(
    package_root: &Path,
    deployment: &VerifiedSkillDeployment,
) -> Result<(), String> {
    let bytes = read_verified_file(package_root, deployment, "BUILD_PROVENANCE.json")?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| "SKILL_BUILD_PROVENANCE_INVALID".to_string())?;
    if value.get("package_version").and_then(Value::as_str) != Some(CREATIVE_SKILLS_PACKAGE_VERSION)
        || value.get("source_archive_sha256").and_then(Value::as_str)
            != Some(deployment.provenance_source_archive_sha256.as_str())
    {
        return Err("SKILL_BUILD_PROVENANCE_MISMATCH".into());
    }
    Ok(())
}

fn verify_registry(
    package_root: &Path,
    deployment: &VerifiedSkillDeployment,
) -> Result<(), String> {
    let bytes = read_verified_file(package_root, deployment, "registry.json")?;
    let registry: Value =
        serde_json::from_slice(&bytes).map_err(|_| "SKILL_REGISTRY_INVALID".to_string())?;
    for field in [
        "package",
        "version",
        "selectionPolicy",
        "methodPolicy",
        "runtimeBoundary",
    ] {
        required_non_empty_string(&registry, field)?;
    }
    let skills = registry
        .get("skills")
        .and_then(Value::as_array)
        .ok_or_else(|| "SKILL_REGISTRY_SKILLS_MISSING".to_string())?;
    if registry.get("version").and_then(Value::as_str) != Some(CREATIVE_SKILLS_PACKAGE_VERSION)
        || registry.get("skillCount").and_then(Value::as_u64) != Some(45)
        || skills.len() != 45
    {
        return Err("SKILL_REGISTRY_VERSION_OR_COUNT_INVALID".into());
    }
    let mut ids = BTreeSet::new();
    let mut slugs = BTreeSet::new();
    for skill in skills {
        for field in [
            "id",
            "slug",
            "name_zh",
            "version",
            "category",
            "role",
            "runtimeStage",
            "contextPolicy",
            "mutationPolicy",
            "bundleDigest",
            "authorityAccess",
            "architectureAlignment",
            "architectureNote",
        ] {
            required_non_empty_string(skill, field)?;
        }
        let id = required_non_empty_string(skill, "id")?;
        let slug = required_non_empty_string(skill, "slug")?;
        let bundle_digest = required_non_empty_string(skill, "bundleDigest")?;
        let coverage = skill.get("coverage");
        let coverage_is_valid = coverage
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
            || coverage.and_then(Value::as_array).is_some_and(|values| {
                !values.is_empty() && values.iter().all(|value| value.as_str().is_some())
            });
        let method_resource_is_valid = match skill.get("methodResource") {
            Some(Value::String(path)) => is_safe_relative_path(path),
            Some(Value::Null) | None => true,
            _ => false,
        };
        let hard_gate_authority = skill
            .get("hardGateAuthority")
            .and_then(Value::as_bool)
            .ok_or_else(|| "SKILL_REGISTRY_SCHEMA_INVALID".to_string())?;
        let control_decision_authority = skill
            .get("controlDecisionAuthority")
            .and_then(Value::as_bool)
            .ok_or_else(|| "SKILL_REGISTRY_SCHEMA_INVALID".to_string())?;
        if skill.get("version").and_then(Value::as_str) != Some(CREATIVE_SKILLS_PACKAGE_VERSION)
            || !bundle_digest.strip_prefix("sha256:").is_some_and(is_sha256)
            || !coverage_is_valid
            || !method_resource_is_valid
            || skill.get("authorityAccess").and_then(Value::as_str) != Some("NONE_DIRECT")
            || hard_gate_authority
            || control_decision_authority
            || !ids.insert(id)
            || !slugs.insert(slug)
        {
            return Err("SKILL_REGISTRY_DUPLICATE_OR_UNKNOWN_ENTRY".into());
        }
        let method_path = format!("skills/{slug}/SKILL.md");
        if !deployment.files.contains_key(&method_path) {
            return Err("SKILL_REGISTRY_METHOD_RESOURCE_MISSING".into());
        }
    }
    Ok(())
}

fn required_non_empty_string<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| "SKILL_REGISTRY_SCHEMA_INVALID".to_string())
}

fn verify_package_files(
    package_root: &Path,
    deployment: &VerifiedSkillDeployment,
) -> Result<(), String> {
    let mut actual = Vec::new();
    collect_files(package_root, package_root, &mut actual)?;
    actual.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    if actual.len() != deployment.file_count() {
        return Err("SKILL_DEPLOYMENT_FILE_SET_MISMATCH".into());
    }
    for (relative, path) in actual {
        let expected = deployment
            .file(&relative)
            .ok_or_else(|| format!("SKILL_DEPLOYMENT_FILE_UNDECLARED:{relative}"))?;
        let metadata = fs::metadata(&path)
            .map_err(|_| format!("SKILL_DEPLOYMENT_FILE_READ_FAILED:{relative}"))?;
        let bytes =
            fs::read(&path).map_err(|_| format!("SKILL_DEPLOYMENT_FILE_READ_FAILED:{relative}"))?;
        if metadata.len() != expected.size_bytes || sha256_hex(&bytes) != expected.sha256 {
            return Err(format!("SKILL_DEPLOYMENT_FILE_DIGEST_MISMATCH:{relative}"));
        }
    }
    Ok(())
}

fn collect_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|_| "SKILL_PACKAGE_NOT_FOUND".to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "SKILL_PACKAGE_READ_FAILED".to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|_| "SKILL_PACKAGE_READ_FAILED".to_string())?;
        if file_type.is_symlink() {
            return Err("SKILL_PACKAGE_SYMLINK_NOT_ALLOWED".into());
        }
        if file_type.is_dir() {
            collect_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "SKILL_PACKAGE_PATH_DENIED".to_string())?
                .to_str()
                .ok_or_else(|| "SKILL_PACKAGE_PATH_NOT_UTF8".to_string())?
                .replace('\\', "/");
            if !is_safe_relative_path(&relative) {
                return Err("SKILL_PACKAGE_PATH_DENIED".into());
            }
            files.push((relative, path));
        } else {
            return Err("SKILL_PACKAGE_ENTRY_TYPE_NOT_ALLOWED".into());
        }
    }
    Ok(())
}
