//! B3-B deterministic and model-assisted opening signing evaluation.
//!
//! Evaluation inputs are committed Golden Three reality. DurableModelRunner
//! records the S35 evaluation before sending and persists the raw provider
//! response before it is parsed. The signing receipt and user confirmation
//! are committed through the existing CommitService and artifact type.

use crate::catalog::CatalogRepository;
use crate::commit::CommitService;
use crate::creative_skills::{build_method_guidance, SkillBundleLoader, SkillSelectionRequest};
use crate::generated::types::{ArtifactEnvelope, ArtifactRef, CommitBundle, CommitReceipt};
use crate::hash::sha256_canonical;
use crate::production::{execute_production_model, ProductionModelGateway};
use crate::project_db::connection::open_connection;
use crate::project_db::paths::AppPaths;
use crate::project_db::ProjectManager;
use crate::repositories::narrative::{
    NarrativeRepository, OpeningActualState, OpeningEvaluationSnapshot,
};
use crate::repositories::runtime::{
    DurableJobService, DurableModelRunner, ModelRunSpec, B02_OPENING_EVALUATION_KIND,
    B02_SIGNING_PHASES,
};
use crate::stable_id::{StableIdKind, StableIdentityFactory};
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const SIGNING_EVALUATION_CAPABILITY: &str = "OPENING_SIGNING_EVALUATION";
pub const SIGNING_RULESET_VERSION: &str = "signing-eval.r1";

const CORE_RULES: &str = include_str!("../../contracts/resources/signing-eval.r1/core.json");
const MODEL_OUTPUT_SCHEMA: &str =
    include_str!("../../contracts/resources/signing-eval.r1/model-output.schema.json");
const FANQIE_PROFILE: &str =
    include_str!("../../contracts/resources/signing-eval.r1/platforms/FANQIE.json");
const QIDIAN_PROFILE: &str =
    include_str!("../../contracts/resources/signing-eval.r1/platforms/QIDIAN.json");
const QIMAO_PROFILE: &str =
    include_str!("../../contracts/resources/signing-eval.r1/platforms/QIMAO.json");
const ZONGHENG_PROFILE: &str =
    include_str!("../../contracts/resources/signing-eval.r1/platforms/ZONGHENG.json");
const JJWXC_PROFILE: &str =
    include_str!("../../contracts/resources/signing-eval.r1/platforms/JJWXC.json");

const EVALUATION_STEPS: &[(&str, &str)] = &[
    ("READ_OPENING_CANON", "读取三章事实"),
    ("SUMMARIZE_OPENING", "汇总开篇状态"),
    ("EVALUATE_SIGNING", "按规则评估签约"),
    ("COMMIT_RECEIPT", "提交正式签约回执"),
];

const SOURCE_KINDS: &[(&str, &str)] = &[
    ("storyConstitutionRef", "StoryConstitution"),
    ("mainContractRef", "MainContract"),
    ("openingContractRef", "OpeningContract"),
    ("bookPlanSeedRef", "BookPlan"),
];

const CORE_MODEL_ITEMS: &[&str] = &[
    "C0-2", "R2-1", "R2-2", "R2-3", "R2-4", "R2-5", "R2-6", "E3-1", "E3-2", "E3-3", "E3-4", "E3-5",
    "E3-6", "E3-7", "E3-8", "E3-9", "E3-11",
];

const PLATFORM_IDS: &[&str] = &["FANQIE", "QIDIAN", "QIMAO", "ZONGHENG", "JJWXC"];
const MAINSTREAM_PLATFORM_IDS: &[&str] = &["FANQIE", "QIDIAN", "QIMAO", "ZONGHENG"];

#[derive(Debug, Clone)]
struct ChapterInput {
    chapter_id: String,
    chapter_no: i64,
    title: String,
    final_ref: ArtifactRef,
    body: String,
    audit_unresolved_hard: Option<bool>,
}

#[derive(Debug, Clone)]
struct OriginalityCorpus {
    bodies: Vec<(String, String)>,
    digest: String,
    complete: bool,
}

struct SigningEvaluationContext<'a> {
    manager: &'a ProjectManager,
    paths: &'a AppPaths,
    connection: &'a Connection,
    project_id: &'a str,
    snapshot: &'a OpeningEvaluationSnapshot,
    actual_state: &'a OpeningActualState,
    sources: &'a [(String, ArtifactRef)],
    chapters: &'a [ChapterInput],
    now: &'a str,
}

struct ReceiptDraftContext<'a> {
    domain: &'a str,
    project_id: &'a str,
    command_id: &'a str,
    idempotency_key: &'a str,
    expected_revision: Option<i64>,
    now: &'a str,
    status: &'a str,
    opening_chapter_refs: &'a [String],
    opening_canon_refs: &'a [String],
    signing_evidence_refs: &'a [String],
}

fn required(request: &Value, path: &str) -> Result<String, String> {
    request
        .pointer(path)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("IPC_INVALID:{path}"))
}

fn ref_value(reference: &ArtifactRef) -> Value {
    json!({
        "artifactId": reference.artifact_id,
        "version": reference.version,
        "payloadHash": reference.payload_hash
    })
}

fn ref_from_value(value: &Value) -> Option<ArtifactRef> {
    Some(ArtifactRef {
        artifact_id: value.get("artifactId")?.as_str()?.to_owned(),
        version: value.get("version")?.as_i64()?,
        payload_hash: value.get("payloadHash")?.as_str()?.to_owned(),
    })
}

fn is_formal_legacy_signing_receipt(payload: &Value) -> bool {
    payload
        .get("sourceRefs")
        .and_then(Value::as_object)
        .is_some()
        && payload
            .get("chapterEvidence")
            .and_then(Value::as_array)
            .is_some()
}

fn dedupe_refs(refs: impl IntoIterator<Item = ArtifactRef>) -> Vec<ArtifactRef> {
    let mut seen = BTreeSet::new();
    refs.into_iter()
        .filter(|reference| {
            seen.insert((
                reference.artifact_id.clone(),
                reference.version,
                reference.payload_hash.clone(),
            ))
        })
        .collect()
}

fn json_resource(raw: &str, name: &str) -> Result<Value, String> {
    serde_json::from_str(raw).map_err(|error| format!("SIGNING_RESOURCE_INVALID:{name}:{error}"))
}

fn core_rules() -> Result<Value, String> {
    json_resource(CORE_RULES, "core")
}

fn model_output_schema() -> Result<Value, String> {
    json_resource(MODEL_OUTPUT_SCHEMA, "model-output-schema")
}

fn platform_profiles() -> Result<Vec<Value>, String> {
    [
        ("FANQIE", FANQIE_PROFILE),
        ("QIDIAN", QIDIAN_PROFILE),
        ("QIMAO", QIMAO_PROFILE),
        ("ZONGHENG", ZONGHENG_PROFILE),
        ("JJWXC", JJWXC_PROFILE),
    ]
    .into_iter()
    .map(|(id, raw)| json_resource(raw, id))
    .collect()
}

fn profile_bindings(profiles: &[Value]) -> Result<Value, String> {
    let mut bindings = serde_json::Map::new();
    for profile in profiles {
        let platform_id = profile
            .get("platformId")
            .and_then(Value::as_str)
            .ok_or_else(|| "SIGNING_PROFILE_ID_MISSING".to_string())?;
        bindings.insert(
            platform_id.to_string(),
            json!({
                "version":profile.get("version").cloned().unwrap_or(Value::Null),
                "digest":sha256_canonical(profile).map_err(|error| error.to_string())?
            }),
        );
    }
    Ok(Value::Object(bindings))
}

fn rule_set_digest(core: &Value, profiles: &[Value]) -> Result<String, String> {
    sha256_canonical(&json!({
        "version":SIGNING_RULESET_VERSION,
        "core":core,
        "profiles":profile_bindings(profiles)?,
        "modelOutputSchema":model_output_schema()?
    }))
    .map_err(|error| error.to_string())
}

fn evaluation_steps(status: &str) -> Value {
    Value::Array(
        EVALUATION_STEPS
            .iter()
            .map(|(id, label)| json!({"id":id,"label":label,"status":status}))
            .collect(),
    )
}

fn notify_phase(
    connection: &Connection,
    project_id: &str,
    job_id: &str,
    phase: &str,
    now: &str,
    observer: &mut dyn FnMut(&str),
) -> Result<(), String> {
    DurableJobService::update_b02_phase(connection, project_id, job_id, phase, now)
        .map_err(|error| error.to_string())?;
    observer(phase);
    Ok(())
}

fn status_result(item_id: &str, status: &str, reason: impl Into<String>, details: Value) -> Value {
    json!({"itemId":item_id,"result":status,"reason":reason.into(),"details":details})
}

fn empty_model_item(item_id: &str, reason: &str) -> Value {
    json!({"itemId":item_id,"result":"NOT_VERIFIED","reason":reason,"evidence":[]})
}

fn model_item_ids() -> Vec<String> {
    let mut ids = CORE_MODEL_ITEMS
        .iter()
        .map(|item| (*item).to_string())
        .collect::<Vec<_>>();
    ids.extend(
        PLATFORM_IDS
            .iter()
            .map(|platform| format!("E3-10:{platform}")),
    );
    ids
}

fn model_item_result(
    value: Option<&Value>,
    item_id: &str,
    chapters: &[ChapterInput],
    forced_reason: Option<&str>,
) -> Value {
    if let Some(reason) = forced_reason {
        return empty_model_item(item_id, reason);
    }
    let Some(value) = value else {
        return empty_model_item(item_id, "模型未返回该必评项。");
    };
    let result = value.get("result").and_then(Value::as_str).unwrap_or("");
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let Some(evidence) = value.get("evidence").and_then(Value::as_array) else {
        return empty_model_item(item_id, "评估项缺少可核验引用。");
    };
    if !matches!(result, "PASS" | "FAIL" | "NOT_VERIFIED")
        || reason.is_empty()
        || evidence.is_empty()
    {
        return empty_model_item(item_id, "评估项结果、原因或逐字引用不完整。");
    }
    let mut checked_evidence = Vec::with_capacity(evidence.len());
    for evidence_item in evidence {
        let chapter_no = evidence_item
            .get("chapterNo")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let range_start = evidence_item
            .get("rangeStart")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX) as usize;
        let range_end = evidence_item
            .get("rangeEnd")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX) as usize;
        let exact_quote = evidence_item
            .get("exactQuote")
            .and_then(Value::as_str)
            .unwrap_or("");
        let Some(chapter) = chapters
            .iter()
            .find(|chapter| chapter.chapter_no == chapter_no)
        else {
            return empty_model_item(item_id, "引用章节不属于本次黄金三章。");
        };
        if range_start >= range_end
            || crate::production::exact_quote_span(
                &chapter.body,
                range_start,
                range_end,
                exact_quote,
            )
            .is_err()
        {
            return empty_model_item(item_id, "引用原文或起止位置与已提交正文不完全一致。");
        }
        checked_evidence.push(json!({
            "chapterNo":chapter_no,
            "chapterId":chapter.chapter_id,
            "finalChapterRef":ref_value(&chapter.final_ref),
            "rangeStart":range_start,
            "rangeEnd":range_end,
            "exactQuote":exact_quote
        }));
    }
    json!({"itemId":item_id,"result":result,"reason":reason,"evidence":checked_evidence})
}

fn valid_s35_signing_envelope(value: &Value, expected_input: &Value) -> bool {
    value.get("skillId").and_then(Value::as_str) == Some("S35")
        && value.get("skillVersion") == expected_input.get("skillVersion")
        && value.get("bundleDigest") == expected_input.get("bundleDigest")
        && value.get("taskId") == expected_input.pointer("/taskContext/taskId")
        && value.get("status").and_then(Value::as_str) == Some("OK")
        && value.get("sourceRefs").and_then(Value::as_array).is_some()
        && value.get("assumptions").and_then(Value::as_array).is_some()
        && value.get("findings").and_then(Value::as_array).is_some()
        && value.get("contextRefs").and_then(Value::as_array).is_some()
        && value
            .get("runtimeCheckRequests")
            .and_then(Value::as_array)
            .is_some()
        && value.get("confidence").and_then(Value::as_f64).is_some()
        && value
            .get("nextSkillHints")
            .and_then(Value::as_array)
            .is_some()
        && [
            "quality_vector",
            "findings",
            "promise_coverage",
            "desire_state",
            "repetition",
            "signing_recommendation",
            "repair_scope",
            "signingEvaluationItems",
        ]
        .iter()
        .all(|field| value.pointer(&format!("/payload/{field}")).is_some())
}

fn model_results(
    raw: Option<&Value>,
    chapters: &[ChapterInput],
    forced_reason: Option<&str>,
    expected_input: Option<&Value>,
) -> Vec<Value> {
    let envelope_valid = raw
        .zip(expected_input)
        .is_some_and(|(value, input)| valid_s35_signing_envelope(value, input));
    let envelope_failure = forced_reason.or_else(|| {
        (!envelope_valid).then_some("S35 输出信封结构或 Skill 身份与当前加载资源不一致。")
    });
    let items = raw
        .filter(|_| envelope_valid)
        .and_then(|value| value.pointer("/payload/signingEvaluationItems"))
        .and_then(Value::as_array);
    let mut by_id = BTreeMap::<String, &Value>::new();
    let mut duplicates = BTreeSet::new();
    let mut unknown = false;
    if let Some(items) = items {
        let allowed = model_item_ids().into_iter().collect::<BTreeSet<_>>();
        for item in items {
            let Some(item_id) = item.get("itemId").and_then(Value::as_str) else {
                unknown = true;
                continue;
            };
            if !allowed.contains(item_id) {
                unknown = true;
                continue;
            }
            if by_id.insert(item_id.to_string(), item).is_some() {
                duplicates.insert(item_id.to_string());
            }
        }
    }
    model_item_ids()
        .iter()
        .map(|item_id| {
            let forced = envelope_failure
                .or_else(|| unknown.then_some("模型输出包含未知评估项，未按其结果判定。"))
                .or_else(|| {
                    duplicates
                        .contains(item_id)
                        .then_some("模型重复返回该评估项。")
                });
            model_item_result(by_id.get(item_id).copied(), item_id, chapters, forced)
        })
        .collect()
}

fn chapter_input(
    connection: &Connection,
    project_id: &str,
    chapter: &crate::repositories::narrative::OpeningEvaluationChapter,
) -> Result<Option<ChapterInput>, String> {
    let Some(final_ref) = chapter.final_ref.clone() else {
        return Ok(None);
    };
    let Some(payload) =
        NarrativeRepository::artifact_payload_by_ref(connection, project_id, &final_ref)
            .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let Some(body) = payload.get("body").and_then(Value::as_str) else {
        return Ok(None);
    };
    let title = NarrativeRepository::chapter_title(connection, project_id, &chapter.chapter_id)
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    let audit_status = if let Some(audit_ref) = payload.get("auditRef").and_then(ref_from_value) {
        NarrativeRepository::artifact_payload_by_ref(connection, project_id, &audit_ref)
            .map_err(|error| error.to_string())?
            .and_then(|audit| {
                audit
                    .get("unresolvedHard")
                    .or_else(|| audit.pointer("/audit/unresolvedHard"))
                    .and_then(Value::as_bool)
            })
    } else {
        None
    };
    Ok(Some(ChapterInput {
        chapter_id: chapter.chapter_id.clone(),
        chapter_no: chapter.chapter_no,
        title,
        final_ref,
        body: body.to_owned(),
        audit_unresolved_hard: audit_status,
    }))
}

fn read_originality_corpus(
    manager: &ProjectManager,
    paths: &AppPaths,
    current_connection: &Connection,
    project_id: &str,
    now: &str,
) -> OriginalityCorpus {
    let mut bodies = Vec::new();
    let mut project_final_heads = Vec::new();
    let mut complete = true;
    let mut found_current = false;
    match open_connection(&paths.catalog_db(), true)
        .map_err(|error| error.to_string())
        .and_then(|catalog| {
            CatalogRepository::list_projects(&catalog).map_err(|error| error.to_string())
        }) {
        Ok(projects) => {
            for project in projects {
                if project.project_id == project_id {
                    found_current = true;
                    match NarrativeRepository::canonical_final_head_refs(
                        current_connection,
                        project_id,
                        10_000,
                    ) {
                        Ok((refs, truncated)) => {
                            complete &= !truncated;
                            project_final_heads
                                .push(json!({"projectId":project_id,"finalChapterRefs":refs}));
                            match NarrativeRepository::canonical_final_bodies(
                                current_connection,
                                project_id,
                                10_000,
                            ) {
                                Ok((items, truncated)) => {
                                    complete &= !truncated;
                                    bodies.extend(items);
                                }
                                Err(_) => complete = false,
                            }
                        }
                        Err(_) => complete = false,
                    }
                    continue;
                }
                match manager.with_read_connection(&project.project_id, now, |connection| {
                    let (refs, truncated) = NarrativeRepository::canonical_final_head_refs(
                        connection,
                        &project.project_id,
                        10_000,
                    )
                    .map_err(|error| {
                        crate::project_db::ManagerError::ProjectCommand(error.to_string())
                    })?;
                    let bodies = NarrativeRepository::canonical_final_bodies(
                        connection,
                        &project.project_id,
                        10_000,
                    )
                    .map_err(|error| {
                        crate::project_db::ManagerError::ProjectCommand(error.to_string())
                    })?;
                    Ok((refs, truncated, bodies))
                }) {
                    Ok((refs, ref_truncated, (items, body_truncated))) => {
                        complete &= !ref_truncated && !body_truncated;
                        project_final_heads
                            .push(json!({"projectId":project.project_id,"finalChapterRefs":refs}));
                        bodies.extend(items);
                    }
                    Err(_) => complete = false,
                }
            }
        }
        Err(_) => complete = false,
    }
    complete &= found_current;
    project_final_heads
        .sort_by(|left, right| left["projectId"].as_str().cmp(&right["projectId"].as_str()));
    bodies.sort_by(|left, right| left.0.cmp(&right.0));
    let digest = sha256_canonical(&json!({
        "projectId":project_id,
        "canonicalFinalChapterHeads":project_final_heads,
        "complete":complete
    }))
    .unwrap_or_default();
    OriginalityCorpus {
        bodies,
        digest,
        complete,
    }
}

fn has_duplicate_long_span(chapters: &[ChapterInput], corpus: &OriginalityCorpus) -> bool {
    const MIN_SPAN: usize = 100;
    let current_ids = chapters
        .iter()
        .map(|chapter| chapter.final_ref.artifact_id.as_str())
        .collect::<BTreeSet<_>>();
    let external = corpus
        .bodies
        .iter()
        .filter(|(artifact_id, _)| !current_ids.contains(artifact_id.as_str()))
        .map(|(_, body)| body.chars().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut seen = BTreeSet::<String>::new();
    for body in &external {
        for window in body.windows(MIN_SPAN) {
            seen.insert(window.iter().collect());
        }
    }
    for chapter in chapters {
        let chars = chapter.body.chars().collect::<Vec<_>>();
        for window in chars.windows(MIN_SPAN) {
            if seen.contains(&window.iter().collect::<String>()) {
                return true;
            }
            seen.insert(window.iter().collect());
        }
    }
    false
}

fn punctuation_or_layout_findings(body: &str) -> Vec<String> {
    let chars = body.chars().collect::<Vec<_>>();
    let mut findings = Vec::new();
    if chars.windows(2).any(|pair| {
        pair[0] == pair[1]
            && matches!(
                pair[0],
                '。' | '，' | '！' | '？' | '；' | '：' | '.' | ',' | '!' | '?'
            )
    }) {
        findings.push("连续重复标点".to_string());
    }
    for (full, half) in [
        ('，', ','),
        ('。', '.'),
        ('！', '!'),
        ('？', '?'),
        ('：', ':'),
        ('；', ';'),
    ] {
        if body.contains(full) && body.contains(half) {
            findings.push(format!("全半角标点混用:{full}{half}"));
        }
    }
    if body.contains("\n\n\n") || body.contains("\r\n\r\n\r\n") {
        findings.push("异常连续空行".to_string());
    }
    if body.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("# ")
            || line.starts_with("## ")
            || line.starts_with("```")
            || line.starts_with("**")
            || line.starts_with("- ")
            || line.starts_with("* ")
    }) {
        findings.push("残留Markdown格式".to_string());
    }
    findings
}

fn outline_findings(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let numbered = trimmed
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .count();
            if numbered > 0
                && trimmed
                    .chars()
                    .nth(numbered)
                    .is_some_and(|character| matches!(character, '.' | '、' | ')' | '）'))
            {
                return Some(trimmed.to_string());
            }
            ["第一幕", "第二幕", "第三幕", "场景一", "场景二", "场景三"]
                .iter()
                .find(|marker| trimmed.starts_with(**marker))
                .map(|marker| marker.to_string())
        })
        .take(20)
        .collect()
}

fn max_recap_overlap(current: &str, previous: &str) -> usize {
    let current = current.chars().collect::<Vec<_>>();
    let previous = previous.chars().collect::<Vec<_>>();
    let upper = 180.min(current.len());
    (30..=upper)
        .rev()
        .find(|length| {
            let prefix = current[..*length].iter().collect::<String>();
            previous
                .windows(*length)
                .any(|window| window == &current[..*length])
                || previous.iter().collect::<String>().contains(&prefix)
        })
        .unwrap_or(0)
}

fn repeated_phrase_count(chapters: &[ChapterInput]) -> usize {
    let mut counts = BTreeMap::<String, usize>::new();
    for chapter in chapters {
        let chars = chapter.body.chars().collect::<Vec<_>>();
        for window in chars.windows(12) {
            *counts.entry(window.iter().collect()).or_default() += 1;
        }
    }
    counts.values().filter(|count| **count >= 3).count()
}

fn deterministic_results(chapters: &[ChapterInput], corpus: &OriginalityCorpus) -> Vec<Value> {
    let complete_chapters = chapters.len() == 3
        && chapters
            .iter()
            .enumerate()
            .all(|(index, chapter)| chapter.chapter_no == index as i64 + 1);
    let length_metrics = chapters
        .iter()
        .map(|chapter| json!({"chapterNo":chapter.chapter_no,"titlePresent":!chapter.title.trim().is_empty(),"characters":chapter.body.chars().count()}))
        .collect::<Vec<_>>();
    let length_fail = !complete_chapters
        || chapters.iter().any(|chapter| {
            let count = chapter.body.chars().count();
            chapter.title.trim().is_empty() || !(2000..=5000).contains(&count)
        });
    let formatting = chapters
        .iter()
        .flat_map(|chapter| punctuation_or_layout_findings(&chapter.body))
        .collect::<Vec<_>>();
    let outlines = chapters
        .iter()
        .flat_map(|chapter| outline_findings(&chapter.body))
        .collect::<Vec<_>>();
    let recap = chapters
        .windows(2)
        .map(|pair| max_recap_overlap(&pair[1].body, &pair[0].body))
        .max()
        .unwrap_or_default();
    let repeated_phrases = repeated_phrase_count(chapters);
    let originality_duplicate = has_duplicate_long_span(chapters, corpus);
    vec![
        status_result(
            "C0-3",
            if !corpus.complete {
                "NOT_VERIFIED"
            } else if originality_duplicate {
                "FAIL"
            } else {
                "PASS"
            },
            if !corpus.complete {
                "原创性语料读取不完整，不能证明没有大段重复。"
            } else if originality_duplicate {
                "与当前作品或其他作品的已提交正文存在至少 100 字连续重复。"
            } else {
                "与可读取的已提交正文未发现至少 100 字连续重复。"
            },
            json!({"corpusDigest":corpus.digest,"complete":corpus.complete,"minimumDuplicateSpanChars":100}),
        ),
        status_result(
            "F1-1",
            if length_fail { "FAIL" } else { "PASS" },
            if length_fail {
                "章节标题或字数未达到首版规则范围。"
            } else {
                "三章均有标题且处于首版字数范围。"
            },
            json!({"chapters":length_metrics,"minimumChars":2000,"maximumChars":5000,"hardFailBelowChars":800,"thresholdStatus":"calibrating"}),
        ),
        status_result(
            "F1-2",
            if formatting.is_empty() {
                "PASS"
            } else {
                "FAIL"
            },
            if formatting.is_empty() {
                "未发现重复标点、混用标点、异常空行或 Markdown 残留。"
            } else {
                "发现排版或标点问题。"
            },
            json!({"findings":formatting,"maximumFindings":0,"thresholdStatus":"calibrating"}),
        ),
        status_result(
            "F1-3",
            if outlines.is_empty() { "PASS" } else { "FAIL" },
            if outlines.is_empty() {
                "未发现提纲式小标题或编号列表。"
            } else {
                "正文中出现提纲式结构标记。"
            },
            json!({"findings":outlines,"maximumFindings":0,"thresholdStatus":"calibrating"}),
        ),
        status_result(
            "F1-4",
            if recap <= 180 { "PASS" } else { "FAIL" },
            if recap <= 180 {
                "章首与前章未发现超过阈值的连续复述。"
            } else {
                "章首与前章存在超过阈值的连续复述。"
            },
            json!({"maximumOverlappingChars":180,"observedMaximum":recap,"thresholdStatus":"calibrating"}),
        ),
        status_result(
            "F1-5",
            if repeated_phrases == 0 {
                "PASS"
            } else {
                "FAIL"
            },
            if repeated_phrases == 0 {
                "未发现达到首版频次的重复短语。"
            } else {
                "发现跨章或章内高频重复短语。"
            },
            json!({"phraseChars":12,"repeatedPhraseOccurrences":3,"observedRepeatedPhrases":repeated_phrases,"thresholdStatus":"calibrating"}),
        ),
    ]
}

fn result_for(items: &[Value], item_id: &str) -> &'static str {
    match items
        .iter()
        .find(|item| item.get("itemId").and_then(Value::as_str) == Some(item_id))
        .and_then(|item| item.get("result").and_then(Value::as_str))
    {
        Some("PASS") => "PASS",
        Some("FAIL") => "FAIL",
        _ => "NOT_VERIFIED",
    }
}

fn combine_statuses<'a>(statuses: impl IntoIterator<Item = &'a str>) -> &'static str {
    let mut has_unknown = false;
    for status in statuses {
        if status == "FAIL" {
            return "FAIL";
        }
        has_unknown |= status != "PASS";
    }
    if has_unknown {
        "NOT_VERIFIED"
    } else {
        "PASS"
    }
}

fn core_status(items: &[Value]) -> &'static str {
    let f1_failed = ["F1-1", "F1-2", "F1-3", "F1-4", "F1-5"]
        .iter()
        .any(|item_id| result_for(items, item_id) == "FAIL");
    let c03 = result_for(items, "C0-3");
    let c02 = result_for(items, "C0-2");
    let r2_failed = ["R2-1", "R2-2", "R2-3", "R2-4", "R2-5", "R2-6"]
        .iter()
        .any(|item_id| result_for(items, item_id) == "FAIL");
    let critical_e3_failed = ["E3-1", "E3-2", "E3-3", "E3-4", "E3-5", "E3-6", "E3-11"]
        .iter()
        .any(|item_id| result_for(items, item_id) == "FAIL");
    if f1_failed || c02 == "FAIL" || c03 == "FAIL" || r2_failed || critical_e3_failed {
        return "FAIL";
    }
    let has_unverified_required_item = ["C0-3", "F1-1", "F1-2", "F1-3", "F1-4", "F1-5"]
        .into_iter()
        .chain(CORE_MODEL_ITEMS.iter().copied())
        .any(|item_id| result_for(items, item_id) == "NOT_VERIFIED");
    if has_unverified_required_item {
        "NOT_VERIFIED"
    } else {
        "PASS"
    }
}

fn profile_compatibility(profile: &Value) -> &'static str {
    match profile.get("compatibility").and_then(Value::as_str) {
        Some("PASS_IF_CORE_PASS") => "PASS",
        Some("FAIL") => "FAIL",
        Some("NOT_VERIFIED") => "NOT_VERIFIED",
        _ => "NOT_VERIFIED",
    }
}

fn platform_matrix(
    items: &[Value],
    profiles: &[Value],
    core_status: &str,
    word_count: usize,
) -> Value {
    Value::Array(
        profiles
            .iter()
            .map(|profile| {
                let platform_id = profile.get("platformId").and_then(Value::as_str).unwrap_or("");
                let mainstream = profile.get("mainstream").and_then(Value::as_bool).unwrap_or(false);
                let c01 = profile_compatibility(profile);
                let c02 = result_for(items, "C0-2");
                let e310 = result_for(items, &format!("E3-10:{platform_id}"));
                let combined = combine_statuses([core_status, c01, c02, e310]);
                let minimum_words = profile
                    .get("rules")
                    .and_then(Value::as_array)
                    .and_then(|rules| rules.iter().find(|rule| rule.get("id").and_then(Value::as_str) == Some("submission-words")))
                    .and_then(|rule| rule.get("minimumWords").and_then(Value::as_u64))
                    .map(|value| value as usize);
                json!({
                    "platformId":platform_id,
                    "displayName":profile.get("displayName"),
                    "mainstream":mainstream,
                    "status":combined,
                    "items":[
                        {"itemId":"C0-1","result":c01,"reason":if c01=="PASS" {"当前创作方式与平台规则兼容性允许评估。"} else if c01=="FAIL" {"平台规则不允许正文由模型生成。"} else {"没有足够的平台规则证据确认兼容性。"}},
                        {"itemId":"C0-2","result":c02,"reason":"采用通用内容禁区逐字引用核验结果。"},
                        {"itemId":format!("E3-10:{platform_id}"),"result":e310,"reason":"逐项评估读者定位与平台档案的匹配度。"}
                    ],
                    "submissionWords":{"minimum":minimum_words,"current":word_count,"distance":minimum_words.map(|minimum| minimum.saturating_sub(word_count))}
                })
            })
            .collect(),
    )
}

fn overall_status(core: &str, matrix: &Value) -> &'static str {
    if core == "FAIL" {
        return "FAIL";
    }
    if core != "PASS" {
        return "NOT_VERIFIED";
    }
    let entries = matrix.as_array().map(Vec::as_slice).unwrap_or_default();
    if entries.iter().any(|entry| {
        entry.get("mainstream").and_then(Value::as_bool) == Some(true)
            && entry.get("status").and_then(Value::as_str) == Some("PASS")
    }) {
        return "PASS";
    }
    if entries.iter().any(|entry| {
        entry.get("mainstream").and_then(Value::as_bool) == Some(true)
            && entry.get("status").and_then(Value::as_str) == Some("NOT_VERIFIED")
    }) {
        "NOT_VERIFIED"
    } else {
        "FAIL"
    }
}

fn source_refs(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<(String, ArtifactRef)>, String> {
    SOURCE_KINDS
        .iter()
        .map(|(kind, artifact_type)| {
            let reference = NarrativeRepository::canonical_project_head_ref(
                connection,
                project_id,
                artifact_type,
            )
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("B02_OPENING_AUTHORITY_REQUIRED:{artifact_type}"))?;
            Ok(((*kind).to_string(), reference))
        })
        .collect()
}

fn source_payload(
    connection: &Connection,
    project_id: &str,
    sources: &[(String, ArtifactRef)],
    kind: &str,
) -> Result<Value, String> {
    let reference = sources
        .iter()
        .find(|(source_kind, _)| source_kind == kind)
        .map(|(_, reference)| reference)
        .ok_or_else(|| format!("SIGNING_SOURCE_MISSING:{kind}"))?;
    NarrativeRepository::artifact_payload_by_ref(connection, project_id, reference)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("SIGNING_SOURCE_PAYLOAD_MISSING:{kind}"))
}

fn opening_contract_complete(payload: &Value) -> bool {
    payload.get("openingReady").and_then(Value::as_bool) == Some(true)
        && payload
            .get("firstChapterGoal")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
        && payload
            .get("firstThreeChapterPromise")
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
        && payload
            .get("forbiddenReveals")
            .and_then(Value::as_array)
            .is_some()
}

fn story_seed_complete(payload: &Value) -> bool {
    [
        "protagonist",
        "desire",
        "mechanism",
        "conflict",
        "readerPromise",
    ]
    .iter()
    .all(|field| {
        payload
            .pointer(&format!("/storyCore/{field}"))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    })
}

fn opening_canon_is_readable(reference: &str, actual_state: &OpeningActualState) -> bool {
    actual_state
        .facts
        .iter()
        .any(|fact| fact.fact_id == reference)
        || actual_state
            .relations
            .iter()
            .any(|relation| relation.relation_id == reference)
        || actual_state
            .events
            .iter()
            .any(|event| event.event_id == reference)
}

fn opening_canon_complete(
    snapshot: &OpeningEvaluationSnapshot,
    actual_state: &OpeningActualState,
) -> bool {
    snapshot.chapters.len() == 3
        && snapshot
            .chapters
            .iter()
            .enumerate()
            .all(|(index, chapter)| {
                chapter.chapter_no == index as i64 + 1
                    && chapter.final_ref.is_some()
                    && !chapter.evidence.is_empty()
                    && !chapter.canon_refs.is_empty()
                    && chapter
                        .canon_refs
                        .iter()
                        .all(|reference| opening_canon_is_readable(reference, actual_state))
            })
}

fn source_binding(
    snapshot: &OpeningEvaluationSnapshot,
    actual_state: &OpeningActualState,
    sources: &[(String, ArtifactRef)],
) -> Result<(Value, String, Vec<Value>), String> {
    let core = core_rules()?;
    let profiles = platform_profiles()?;
    let bindings = profile_bindings(&profiles)?;
    let rule_digest = rule_set_digest(&core, &profiles)?;
    let constitution_ref = sources
        .iter()
        .find(|(kind, _)| kind == "storyConstitutionRef")
        .map(|(_, reference)| ref_value(reference))
        .ok_or_else(|| "SIGNING_SOURCE_MISSING:StoryConstitution".to_string())?;
    let main_contract_ref = sources
        .iter()
        .find(|(kind, _)| kind == "mainContractRef")
        .map(|(_, reference)| ref_value(reference))
        .ok_or_else(|| "SIGNING_SOURCE_MISSING:MainContract".to_string())?;
    let opening_contract_ref = sources
        .iter()
        .find(|(kind, _)| kind == "openingContractRef")
        .map(|(_, reference)| ref_value(reference))
        .ok_or_else(|| "SIGNING_SOURCE_MISSING:OpeningContract".to_string())?;
    let binding = json!({
        "openingChapterRefs":snapshot.chapters.iter().map(|chapter| chapter.chapter_id.clone()).collect::<Vec<_>>(),
        "finalChapterRefs":snapshot.chapters.iter().filter_map(|chapter| chapter.final_ref.as_ref().map(ref_value)).collect::<Vec<_>>(),
        "storyConstitutionRef":constitution_ref,
        "mainContractRef":main_contract_ref,
        "openingContractRef":opening_contract_ref,
        "openingActualStateDigest":actual_state.semantic_digest,
        "ruleSetVersion":SIGNING_RULESET_VERSION,
        "ruleSetDigest":rule_digest,
        "platformProfiles":bindings
    });
    let digest = sha256_canonical(&binding).map_err(|error| error.to_string())?;
    Ok((binding, digest, profiles))
}

fn current_binding(
    context: &SigningEvaluationContext<'_>,
) -> Result<(Value, String, OriginalityCorpus, Value, Vec<Value>), String> {
    let SigningEvaluationContext {
        manager,
        paths,
        connection,
        project_id,
        snapshot,
        actual_state,
        sources,
        chapters,
        now,
    } = context;
    let corpus = read_originality_corpus(manager, paths, connection, project_id, now);
    let (binding, digest, profiles) = source_binding(snapshot, actual_state, sources)?;
    let opening_contract = source_payload(connection, project_id, sources, "openingContractRef")?;
    let story_constitution =
        source_payload(connection, project_id, sources, "storyConstitutionRef")?;
    let opening_ready = opening_contract_complete(&opening_contract);
    let seed_ready = story_seed_complete(&story_constitution);
    let opening_canon_ready = opening_canon_complete(snapshot, actual_state);
    let chapters_ready = chapters.len() == 3
        && chapters.iter().enumerate().all(|(index, chapter)| {
            chapter.chapter_no == index as i64 + 1 && chapter.audit_unresolved_hard.is_some()
        });
    let readiness = json!({
        "ready":opening_ready && seed_ready && chapters_ready && opening_canon_ready,
        "openingContractComplete":opening_ready,
        "storySeedComplete":seed_ready,
        "openingCanonCommitted":opening_canon_ready,
        "auditStates":chapters.iter().map(|chapter| json!({"chapterNo":chapter.chapter_no,"unresolvedHard":chapter.audit_unresolved_hard})).collect::<Vec<_>>(),
        "chaptersComplete":chapters_ready
    });
    Ok((binding, digest, corpus, readiness, profiles))
}

fn deterministic_model_gate(
    deterministic: &[Value],
    chapters: &[ChapterInput],
    readiness: &Value,
) -> Option<&'static str> {
    if !readiness
        .get("openingCanonCommitted")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Some("BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE：开篇三章缺少可读取的已提交 Canon。未调用模型评估。");
    }
    if ["F1-1", "F1-2", "F1-3", "F1-4", "F1-5"]
        .iter()
        .any(|item_id| result_for(deterministic, item_id) == "FAIL")
    {
        return Some("第1层格式与结构检查失败，按规则不调用模型。");
    }
    if !readiness
        .get("openingContractComplete")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || !readiness
            .get("chaptersComplete")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return Some("开篇合同或审计来源不完整，不能调用模型评估。");
    }
    if !readiness
        .get("storySeedComplete")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Some("Story Seed 必填字段不完整，不能确认签约结论。");
    }
    if chapters
        .iter()
        .any(|chapter| chapter.audit_unresolved_hard == Some(true))
    {
        return Some("B08 审计存在未决硬问题；R2-3 按硬规则失败，不调用模型。");
    }
    None
}

fn signing_model_input(
    context: &SigningEvaluationContext<'_>,
    core: &Value,
    profiles: &[Value],
    output_schema: &Value,
    loader: &SkillBundleLoader,
) -> Result<(Value, String, String), String> {
    let project_id = context.project_id;
    let snapshot = context.snapshot;
    let chapters = context.chapters;
    let sources = context.sources;
    let connection = context.connection;
    let selection = loader.select(SkillSelectionRequest {
        primary: "S35".into(),
        support: Vec::new(),
        budget_reason: Some("B3-B 黄金三章签约评估，一次模型调用".into()),
        context_tokens: 20_000,
        model_calls: 1,
    })?;
    let required_resources = vec![
        "resources/operating_method.md".to_string(),
        "resources/eval_rubric.md".to_string(),
    ];
    let loaded = loader.load_selected(&selection, &required_resources)?;
    let method_guidance = build_method_guidance(&loaded.skill_texts, &loaded.resources);
    let constitution = source_payload(connection, project_id, sources, "storyConstitutionRef")?;
    let main_contract = source_payload(connection, project_id, sources, "mainContractRef")?;
    let opening_contract = source_payload(connection, project_id, sources, "openingContractRef")?;
    let book_seed = source_payload(connection, project_id, sources, "bookPlanSeedRef")?;
    let skill_task_context = json!({
        "taskId":format!("task:signing:{project_id}"),
        "projectId":project_id,
        "runtimeStage":"SIGNING_REVIEW",
        "boundedRefs":sources.iter().map(|(kind,reference)| json!({"kind":kind,"reference":ref_value(reference)})).collect::<Vec<_>>(),
        "chapters":chapters.iter().map(|chapter| json!({"chapterNo":chapter.chapter_no,"chapterId":chapter.chapter_id,"finalChapterRef":ref_value(&chapter.final_ref),"title":chapter.title,"body":chapter.body})).collect::<Vec<_>>(),
        "storySeed":book_seed,
        "storyConstitution":constitution,
        "mainContract":main_contract,
        "openingContract":opening_contract,
        "openingCanonRefs":snapshot.opening_canon_refs,
        "signingEvidenceRefs":snapshot.signing_evidence_refs,
        "sourceEvidence":snapshot.chapters.iter().map(|chapter| json!({"chapterNo":chapter.chapter_no,"evidence":chapter.evidence.iter().map(|evidence| json!({"evidenceId":evidence.evidence_id,"quote":evidence.quote,"rangeStart":evidence.range_start,"rangeEnd":evidence.range_end})).collect::<Vec<_>>()})).collect::<Vec<_>>()
    });
    let skill_input = json!({
        "skillId":selection.primary.id,
        "skillVersion":selection.primary.version,
        "bundleDigest":selection.primary.bundle_digest,
        "loadedSkillTexts":loaded.skill_texts.iter().map(|skill| json!({"ref":skill.reference,"digest":skill.digest,"text":skill.text})).collect::<Vec<_>>(),
        "resources":loaded.resources.iter().map(|resource| json!({"ref":resource.reference,"digest":resource.digest,"text":resource.text})).collect::<Vec<_>>(),
        "skillResourceIdentity":loaded.identity,
        "methodGuidance":method_guidance,
        "taskContext":skill_task_context,
        "ruleSetVersion":SIGNING_RULESET_VERSION,
        "commonCoreRules":core,
        "platformProfiles":profiles,
        "outputSchema":output_schema,
        "prompt":"执行 S35 黄金三章 SigningReview。确定性规则已由 Runtime 完成；逐项评估 C0-2、R2-1 至 R2-6、E3-1 至 E3-9、E3-11，以及五个平台各自的 E3-10。按 outputSchema 返回完整 S35 SkillResultEnvelope，并将评估项放入 payload.signingEvaluationItems。每个项目都必须提供正文中的 exactQuote、chapterNo、rangeStart、rangeEnd；引用必须逐字复制，不得改写、纠正或推测位置。不得评估或生成规避 AI 检测的内容。不得把 C0-4 计入分数，只提示投稿前如实声明。"
    });
    let context_digest =
        sha256_canonical(&skill_task_context).map_err(|error| error.to_string())?;
    let skill_digest = loaded.identity.deployed_manifest_digest.clone();
    Ok((skill_input, context_digest, skill_digest))
}

fn source_refs_value(sources: &[(String, ArtifactRef)]) -> Value {
    Value::Object(
        sources
            .iter()
            .map(|(kind, reference)| (kind.clone(), ref_value(reference)))
            .collect(),
    )
}

fn result_payload(payload: &Value, receipt_ref: &ArtifactRef) -> Value {
    json!({
        "status":payload.get("status").cloned().unwrap_or_else(|| json!("NOT_VERIFIED")),
        "reasonCode":payload.get("reasonCode").cloned().unwrap_or(Value::Null),
        "verdict":payload.get("verdict").cloned().unwrap_or(Value::Null),
        "summary":payload.get("summary").cloned().unwrap_or(Value::Null),
        "receiptRef":ref_value(receipt_ref),
        "openingChapterRefs":payload.get("openingChapterRefs").cloned().unwrap_or_else(|| json!([])),
        "openingCanonRefs":payload.get("openingCanonRefs").cloned().unwrap_or_else(|| json!([])),
        "signingEvidenceRefs":payload.get("signingEvidenceRefs").cloned().unwrap_or_else(|| json!([])),
        "evaluationRuleVersion":payload.get("evaluationRuleVersion").cloned().unwrap_or(Value::Null),
        "ruleSetDigest":payload.get("ruleSetDigest").cloned().unwrap_or(Value::Null),
        "modelRunId":payload.get("modelRunId").cloned().unwrap_or(Value::Null),
        "coreStatus":payload.get("coreStatus").cloned().unwrap_or(Value::Null),
        "itemResults":payload.get("itemResults").cloned().unwrap_or_else(|| json!([])),
        "platformMatrix":payload.get("platformMatrix").cloned().unwrap_or_else(|| json!([])),
        "confirmation":payload.get("confirmation").cloned().unwrap_or(Value::Null),
        "declarationPrompt":payload.get("declarationPrompt").cloned().unwrap_or(Value::Null),
        "sourceDigest":payload.get("sourceDigest").cloned().unwrap_or(Value::Null),
        "summaryCounts":payload.get("summary").cloned().unwrap_or(Value::Null)
    })
}

fn command_response(
    command_id: &str,
    project_id: &str,
    receipt: &CommitReceipt,
    payload: &Value,
    receipt_ref: &ArtifactRef,
) -> Result<Value, String> {
    let result = result_payload(payload, receipt_ref);
    let status = result["status"].as_str().unwrap_or("NOT_VERIFIED");
    let confirmed = result["confirmation"]["confirmed"].as_bool() == Some(true);
    let message = if status == "PASS" && confirmed {
        "签约评估通过，你已确认目标平台；可以生成正式总纲。"
    } else if status == "PASS" {
        "签约评估通过，等待你从通过的平台中选择并确认。"
    } else {
        "签约评估未通过或无法验证；请查看逐项结果与原文引用。"
    };
    Ok(json!({
        "commandId":command_id,
        "projectId":project_id,
        "finalStatus":"committed",
        "message":message,
        "receiptId":receipt.receipt_id,
        "beforeRevision":receipt.before_revision,
        "afterRevision":receipt.after_revision,
        "result":result,
        "receipt":serde_json::to_value(receipt).map_err(|error| error.to_string())?
    }))
}

fn build_receipt_artifact(
    project_id: &str,
    artifact_id: &str,
    version: i64,
    now: &str,
    command_id: &str,
    payload: Value,
    dependencies: Vec<ArtifactRef>,
) -> Result<(ArtifactEnvelope, ArtifactRef), String> {
    let payload_hash = sha256_canonical(&payload).map_err(|error| error.to_string())?;
    let receipt_ref = ArtifactRef {
        artifact_id: artifact_id.to_string(),
        version,
        payload_hash: payload_hash.clone(),
    };
    let artifact = serde_json::from_value(json!({
        "artifactId":artifact_id,
        "projectId":project_id,
        "artifactType":"SimulatedSigningReceipt",
        "scope":{"level":"PROJECT"},
        "version":version,
        "schemaVersion":"m0a.artifact.v1",
        "artifactStatus":"CANDIDATE",
        "payload":payload,
        "payloadHash":payload_hash,
        "dependencyRefs":dedupe_refs(dependencies),
        "provenance":{"createdBy":"B02::OpeningEvaluationOwner","commandId":command_id},
        "createdAt":now
    }))
    .map_err(|error| format!("B03B_SIGNING_ARTIFACT_INVALID:{error}"))?;
    Ok((artifact, receipt_ref))
}

fn commit_receipt_bundle(
    draft: &ReceiptDraftContext<'_>,
    artifact: ArtifactEnvelope,
    receipt_ref: &ArtifactRef,
) -> Result<CommitBundle, String> {
    serde_json::from_value(json!({
        "commitId":format!("commit:{}",draft.command_id),
        "projectId":draft.project_id,
        "commandId":draft.command_id,
        "idempotencyKey":draft.idempotency_key,
        "expectedProjectRevision":draft.expected_revision,
        "contractVersion":"m0a.v1",
        "createdAt":draft.now,
        "artifacts":[artifact],
        "headUpdates":[{"artifactType":"SimulatedSigningReceipt","scope":{"level":"PROJECT"},"artifactId":receipt_ref.artifact_id,"version":receipt_ref.version,"payloadHash":receipt_ref.payload_hash}],
        "receiptDraft":{"domain":draft.domain,"artifactRefs":[receipt_ref.clone()],"payload":{"status":draft.status,"openingChapterRefs":draft.opening_chapter_refs,"openingCanonRefs":draft.opening_canon_refs,"signingEvidenceRefs":draft.signing_evidence_refs}}
    }))
    .map_err(|error| format!("B03B_SIGNING_BUNDLE_INVALID:{error}"))
}

fn no_prior_unknown_model_run(connection: &Connection, project_id: &str) -> Result<(), String> {
    if DurableModelRunner::has_unknown_remote_run(
        connection,
        project_id,
        SIGNING_EVALUATION_CAPABILITY,
    )
    .map_err(|error| error.to_string())?
    {
        Err("UNKNOWN_REMOTE_OUTCOME_REQUIRES_DECISION".into())
    } else {
        Ok(())
    }
}

/// Executes the single formal B03-B evaluation command.
pub fn execute_opening_evaluation(
    manager: &ProjectManager,
    paths: &AppPaths,
    gateway: &dyn ProductionModelGateway,
    skill_loader: &SkillBundleLoader,
    request: &Value,
) -> Result<Value, String> {
    execute_opening_evaluation_with_observer(
        manager,
        paths,
        gateway,
        skill_loader,
        request,
        &mut |_| {},
    )
}

pub fn execute_opening_evaluation_with_observer(
    manager: &ProjectManager,
    paths: &AppPaths,
    gateway: &dyn ProductionModelGateway,
    skill_loader: &SkillBundleLoader,
    request: &Value,
    observer: &mut dyn FnMut(&str),
) -> Result<Value, String> {
    let project_id = required(request, "/projectId")?;
    let command_id = required(request, "/commandId")?;
    let idempotency_key = required(request, "/idempotencyKey")?;
    let now = required(request, "/requestedAt")?;
    let expected_revision = request.get("expectedRevision").and_then(Value::as_i64);
    let handle = manager
        .open(&project_id, "", &now)
        .map_err(|error| error.to_string())?;

    if let Some(previous) = CommitService::find_receipt(&handle, &project_id, &idempotency_key)
        .map_err(|error| error.to_string())?
    {
        let receipt_ref = previous
            .committed_artifact_refs
            .first()
            .cloned()
            .ok_or_else(|| "B03B_SIGNING_RECEIPT_REF_MISSING".to_string())?;
        let payload =
            NarrativeRepository::artifact_payload_by_ref(&handle, &project_id, &receipt_ref)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "B03B_SIGNING_RECEIPT_PAYLOAD_MISSING".to_string())?;
        return command_response(&command_id, &project_id, &previous, &payload, &receipt_ref);
    }

    no_prior_unknown_model_run(&handle, &project_id)?;
    let snapshot = NarrativeRepository::opening_evaluation_snapshot(&handle, &project_id)
        .map_err(|error| error.to_string())?;
    if snapshot.chapters.len() != 3
        || snapshot
            .chapters
            .iter()
            .enumerate()
            .any(|(index, chapter)| chapter.chapter_no != index as i64 + 1)
    {
        return Err("B02_OPENING_THREE_CHAPTERS_REQUIRED".into());
    }
    let sources = source_refs(&handle, &project_id)?;
    let actual_state = NarrativeRepository::opening_actual_state(&handle, &project_id, &snapshot)
        .map_err(|error| error.to_string())?;
    let chapters = snapshot
        .chapters
        .iter()
        .map(|chapter| chapter_input(&handle, &project_id, chapter))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "B02_OPENING_FINAL_SOURCE_UNAVAILABLE".to_string())?;
    let signing_context = SigningEvaluationContext {
        manager,
        paths,
        connection: &handle,
        project_id: &project_id,
        snapshot: &snapshot,
        actual_state: &actual_state,
        sources: &sources,
        chapters: &chapters,
        now: &now,
    };
    let (binding, binding_digest, corpus, readiness, profiles) = current_binding(&signing_context)?;
    let core = core_rules()?;
    let schema = model_output_schema()?;
    let deterministic = deterministic_results(&chapters, &corpus);
    let missing_committed_canon = !readiness
        .get("openingCanonCommitted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let forced_reason = deterministic_model_gate(&deterministic, &chapters, &readiness);
    let job_id = StableIdentityFactory::from_command(
        StableIdKind::Job,
        &format!("{command_id}:opening-evaluation"),
    )?;
    let worker_id = "b03b-opening-evaluation-worker";
    DurableJobService::start_b02_phase_job(
        &handle,
        &project_id,
        &job_id,
        B02_OPENING_EVALUATION_KIND,
        B02_SIGNING_PHASES,
        worker_id,
        &now,
    )
    .map_err(|error| error.to_string())?;
    DurableJobService::set_signing_projection(
        &handle,
        &project_id,
        "SIGNING",
        "IN_REVIEW",
        "ACTIVE",
        None,
        &now,
    )
    .map_err(|error| error.to_string())?;
    notify_phase(
        &handle,
        &project_id,
        &job_id,
        "READ_OPENING_CANON",
        &now,
        observer,
    )?;
    notify_phase(
        &handle,
        &project_id,
        &job_id,
        "SUMMARIZE_OPENING",
        &now,
        observer,
    )?;
    notify_phase(
        &handle,
        &project_id,
        &job_id,
        "EVALUATE_SIGNING",
        &now,
        observer,
    )?;

    let model_evaluation = (|| -> Result<(Vec<Value>, Option<String>), String> {
        if let Some(reason) = forced_reason {
            return Ok((model_results(None, &chapters, Some(reason), None), None));
        }
        let profile = gateway.resolve_profile()?;
        let first_chapter_id = chapters
            .first()
            .map(|chapter| chapter.chapter_id.as_str())
            .ok_or_else(|| "B02_OPENING_THREE_CHAPTERS_REQUIRED".to_string())?;
        let budget_id =
            DurableModelRunner::chapter_budget_id(&handle, &project_id, first_chapter_id)
                .map_err(|error| error.to_string())?;
        let run_id = StableIdentityFactory::from_command(
            StableIdKind::ModelRun,
            &format!("{command_id}:signing-evaluation"),
        )?;
        let spec = ModelRunSpec {
            model_run_id: run_id.clone(),
            job_id: Some(job_id.clone()),
            step_id: None,
            provider: profile.provider,
            model: gateway.model(SIGNING_EVALUATION_CAPABILITY),
            idempotency_key: format!("{idempotency_key}:signing-evaluation"),
            capability: SIGNING_EVALUATION_CAPABILITY.into(),
            prompt_ref: Some("creative-skills/v1.2.0/skills/opening-evaluator/SKILL.md".into()),
            context_manifest_ref: Some(binding_digest.clone()),
            budget_id: Some(budget_id),
            predicted_cost_cents: gateway
                .predicted_cost_cents(SIGNING_EVALUATION_CAPABILITY)
                .max(1),
            attempt_no: 1,
            repair_round: 0,
        };
        let (input, context_digest, deployed_skill_digest) =
            signing_model_input(&signing_context, &core, &profiles, &schema, skill_loader)?;
        let mut input = input;
        input["bindingDigest"] = json!(binding_digest);
        input["skillContextDigest"] = json!(context_digest);
        input["skillDeploymentDigest"] = json!(deployed_skill_digest);
        let expected_s35 = json!({
            "skillVersion":input["skillVersion"],
            "bundleDigest":input["bundleDigest"],
            "taskContext":{"taskId":input.pointer("/taskContext/taskId")}
        });
        let outcome = execute_production_model(
            &handle,
            gateway,
            &spec,
            SIGNING_EVALUATION_CAPABILITY,
            input,
            &now,
        )?;
        Ok((
            model_results(
                outcome.parsed.as_ref(),
                &chapters,
                None,
                Some(&expected_s35),
            ),
            Some(run_id),
        ))
    })();
    let (model_items, model_run_id) = match model_evaluation {
        Ok(result) => result,
        Err(error) => {
            if !error.contains("UNKNOWN_REMOTE_OUTCOME") {
                let _ = DurableJobService::fail_b02_phase_job(
                    &handle,
                    &project_id,
                    &job_id,
                    "B03B_SIGNING_EVALUATION_FAILED",
                    &now,
                );
            }
            return Err(error);
        }
    };

    let mut items = deterministic;
    items.extend(model_items);
    if chapters
        .iter()
        .any(|chapter| chapter.audit_unresolved_hard == Some(true))
    {
        if let Some(item) = items
            .iter_mut()
            .find(|item| item.get("itemId").and_then(Value::as_str) == Some("R2-3"))
        {
            *item = json!({"itemId":"R2-3","result":"FAIL","reason":"B08 审计存在未决硬问题。","evidence":[]});
        }
    }
    let core_verdict = core_status(&items);
    let word_count = chapters
        .iter()
        .map(|chapter| chapter.body.chars().count())
        .sum::<usize>();
    let matrix = platform_matrix(&items, &profiles, core_verdict, word_count);
    let overall = if missing_committed_canon {
        "NOT_VERIFIED"
    } else {
        overall_status(core_verdict, &matrix)
    };
    let reason_code = missing_committed_canon.then_some("BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE");
    let verdict = match overall {
        "PASS" => "通用核心通过，且至少一个主流平台适配结果通过；等待用户确认目标平台。",
        "FAIL" => "存在确定失败项或关键签约项失败，当前评估不通过。",
        _ => "至少一个必评项无法验证；当前评估不能通过。",
    };
    let rule_digest = binding["ruleSetDigest"].as_str().unwrap_or_default();
    let status_summary = json!({
        "chapters":chapters.len(),
        "canonRefs":snapshot.opening_canon_refs.len(),
        "evidenceRefs":snapshot.signing_evidence_refs.len(),
        "characters":word_count,
        "coreStatus":core_verdict,
        "mainstreamPassCount":matrix.as_array().map(|entries| entries.iter().filter(|entry| entry["mainstream"]==true && entry["status"]=="PASS").count()).unwrap_or_default()
    });
    let mut payload = json!({
        "payloadSchema":"simulated-signing-receipt.r1",
        "status":overall,
        "reasonCode":reason_code,
        "verdict":verdict,
        "evaluatedAt":now,
        "evaluationRuleVersion":SIGNING_RULESET_VERSION,
        "ruleSetVersion":SIGNING_RULESET_VERSION,
        "ruleSetDigest":rule_digest,
        "platformProfiles":binding["platformProfiles"],
        "binding":binding,
        "sourceDigest":binding_digest,
        "openingChapterRefs":snapshot.chapters.iter().map(|chapter| chapter.chapter_id.clone()).collect::<Vec<_>>(),
        "openingCanonRefs":snapshot.opening_canon_refs,
        "signingEvidenceRefs":snapshot.signing_evidence_refs,
        "chapterEvidence":chapters.iter().map(|chapter| json!({"chapterId":chapter.chapter_id,"chapterNo":chapter.chapter_no,"finalChapterRef":ref_value(&chapter.final_ref)})).collect::<Vec<_>>(),
        "sourceRefs":source_refs_value(&sources),
        "modelRunId":model_run_id,
        "skillId":"S35",
        "coreStatus":core_verdict,
        "itemResults":items,
        "platformMatrix":matrix,
        "confirmation":{"confirmed":false,"confirmedBy":Value::Null,"confirmedAt":Value::Null,"platformId":Value::Null},
        "declarationPrompt":core.pointer("/decisions/declarationPrompt").cloned().unwrap_or(Value::Null),
        "evaluation":{"status":"COMPLETED","phase":"COMPLETED","steps":evaluation_steps("done"),"summary":if missing_committed_canon {"评估尝试已完成；开篇三章缺少可读取的已提交 Canon，未调用模型。"} else {"确定性检查与 S35 逐项评估已完成；评估结果尚需用户选择通过的平台并确认。"}},
        "summary":status_summary,
        "readiness":readiness,
        "receiptHash":""
    });
    payload["receiptHash"] = json!(sha256_canonical(&payload).map_err(|error| error.to_string())?);

    notify_phase(
        &handle,
        &project_id,
        &job_id,
        "COMMIT_RECEIPT",
        &now,
        observer,
    )?;
    let artifact_id = format!("signing-receipt:{project_id}");
    let version = NarrativeRepository::next_artifact_version(&handle, &project_id, &artifact_id)
        .map_err(|error| error.to_string())?;
    let dependencies = dedupe_refs(
        sources
            .iter()
            .map(|(_, reference)| reference.clone())
            .chain(chapters.iter().map(|chapter| chapter.final_ref.clone())),
    );
    let (artifact, receipt_ref) = build_receipt_artifact(
        &project_id,
        &artifact_id,
        version,
        &now,
        &command_id,
        payload.clone(),
        dependencies,
    )?;
    let opening_chapter_refs = snapshot
        .chapters
        .iter()
        .map(|chapter| chapter.chapter_id.clone())
        .collect::<Vec<_>>();
    let receipt_draft = ReceiptDraftContext {
        domain: "B03B_SIGNING_EVALUATION",
        project_id: &project_id,
        command_id: &command_id,
        idempotency_key: &idempotency_key,
        expected_revision,
        now: &now,
        status: overall,
        opening_chapter_refs: &opening_chapter_refs,
        opening_canon_refs: &snapshot.opening_canon_refs,
        signing_evidence_refs: &snapshot.signing_evidence_refs,
    };
    let bundle = commit_receipt_bundle(&receipt_draft, artifact, &receipt_ref)?;
    let runtime_status = if overall == "PASS" { "IDLE" } else { "BLOCKED" };
    let block_reason = (overall != "PASS").then_some("SIGNING_REQUIRED");
    let receipt =
        match CommitService::commit_with_post_apply(&handle, &bundle, &now, |connection, _| {
            DurableJobService::complete_b02_phase_job(
                connection,
                &project_id,
                &job_id,
                worker_id,
                &now,
            )
            .map_err(|error| {
                crate::commit::CommitError::ContractInvalid(format!("B03B_RUNTIME_JOB:{error}"))
            })?;
            DurableJobService::set_signing_projection(
                connection,
                &project_id,
                "SIGNING",
                overall,
                runtime_status,
                block_reason,
                &now,
            )
            .map_err(|error| {
                crate::commit::CommitError::ContractInvalid(format!(
                    "B03B_RUNTIME_PROJECTION:{error}"
                ))
            })
        }) {
            Ok(receipt) => receipt,
            Err(error) => {
                let _ = DurableJobService::fail_b02_phase_job(
                    &handle,
                    &project_id,
                    &job_id,
                    "B02_COMMIT_FAILED",
                    &now,
                );
                return Err(error.to_string());
            }
        };
    observer("COMPLETED");
    command_response(&command_id, &project_id, &receipt, &payload, &receipt_ref)
}

fn load_current_binding(
    connection: &Connection,
    project_id: &str,
) -> Result<(Value, String), String> {
    let snapshot = NarrativeRepository::opening_evaluation_snapshot(connection, project_id)
        .map_err(|error| error.to_string())?;
    if snapshot.chapters.len() != 3 {
        return Err("B02_OPENING_THREE_CHAPTERS_REQUIRED".into());
    }
    let sources = source_refs(connection, project_id)?;
    let actual_state = NarrativeRepository::opening_actual_state(connection, project_id, &snapshot)
        .map_err(|error| error.to_string())?;
    let (binding, digest, _) = source_binding(&snapshot, &actual_state, &sources)?;
    Ok((binding, digest))
}

/// Returns true only when the latest evaluated PASS is explicitly confirmed
/// for a passing mainstream platform and every bound source is still current.
pub fn signing_receipt_is_current_and_confirmed_in_project(
    connection: &Connection,
    project_id: &str,
    receipt_ref: &ArtifactRef,
) -> Result<bool, String> {
    let Some(payload) =
        NarrativeRepository::artifact_payload_by_ref(connection, project_id, receipt_ref)
            .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };
    if payload.get("status").and_then(Value::as_str) != Some("PASS")
        || payload
            .pointer("/confirmation/confirmed")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Ok(false);
    }
    let selected_platform = payload
        .pointer("/confirmation/platformId")
        .and_then(Value::as_str);
    let Some(selected_platform) = selected_platform else {
        return Ok(false);
    };
    if !MAINSTREAM_PLATFORM_IDS.contains(&selected_platform) {
        return Ok(false);
    }
    let selected_passes = payload
        .get("platformMatrix")
        .and_then(Value::as_array)
        .is_some_and(|matrix| {
            matrix.iter().any(|entry| {
                entry.get("platformId").and_then(Value::as_str) == Some(selected_platform)
                    && entry.get("status").and_then(Value::as_str) == Some("PASS")
                    && entry.get("mainstream").and_then(Value::as_bool) == Some(true)
            })
        });
    if !selected_passes
        || payload.get("evaluationRuleVersion").and_then(Value::as_str)
            != Some(SIGNING_RULESET_VERSION)
    {
        return Ok(false);
    }
    signing_evaluation_is_current_in_project(connection, project_id, receipt_ref)
}

pub fn signing_evaluation_is_current_in_project(
    connection: &Connection,
    project_id: &str,
    receipt_ref: &ArtifactRef,
) -> Result<bool, String> {
    let Some(payload) =
        NarrativeRepository::artifact_payload_by_ref(connection, project_id, receipt_ref)
            .map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };
    if payload.get("status").and_then(Value::as_str) != Some("PASS") {
        return Ok(false);
    }
    if payload.get("evaluationRuleVersion").and_then(Value::as_str) != Some(SIGNING_RULESET_VERSION)
    {
        return Ok(!is_formal_legacy_signing_receipt(&payload));
    }
    let (current_binding, current_digest) = match load_current_binding(connection, project_id) {
        Ok(binding) => binding,
        Err(_) => return Ok(false),
    };
    Ok(
        payload.get("sourceDigest").and_then(Value::as_str) == Some(current_digest.as_str())
            && payload.get("binding") == Some(&current_binding)
            && payload.get("ruleSetDigest") == current_binding.get("ruleSetDigest"),
    )
}

/// Applies freshness and confirmation to newly issued signing-eval.r1
/// receipts before a LONG_FORM plan is consumed. Pre-B3-B approvals remain
/// historical inputs and are not rewritten by this batch.
pub fn require_current_signing_for_long_form_if_present(
    connection: &Connection,
    project_id: &str,
) -> Result<(), String> {
    let Some(receipt_ref) = NarrativeRepository::canonical_project_head_ref(
        connection,
        project_id,
        "SimulatedSigningReceipt",
    )
    .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let Some(payload) =
        NarrativeRepository::artifact_payload_by_ref(connection, project_id, &receipt_ref)
            .map_err(|error| error.to_string())?
    else {
        return Err("SIGNING_EVALUATION_REQUIRED".into());
    };
    if payload.get("evaluationRuleVersion").and_then(Value::as_str) != Some(SIGNING_RULESET_VERSION)
    {
        return Ok(());
    }
    if payload.get("status").and_then(Value::as_str) != Some("PASS") {
        return Err("SIGNING_EVALUATION_REQUIRED".into());
    }
    if payload
        .pointer("/confirmation/confirmed")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return Err("SIGNING_CONFIRMATION_REQUIRED".into());
    }
    if !signing_receipt_is_current_and_confirmed_in_project(connection, project_id, &receipt_ref)? {
        return Err("SIGNING_EVALUATION_STALE".into());
    }
    Ok(())
}

/// Records a separate, explicit user platform confirmation by advancing the
/// existing SimulatedSigningReceipt artifact version through CommitService.
pub fn confirm_signing_platform(
    manager: &ProjectManager,
    request: &Value,
) -> Result<Value, String> {
    let project_id = required(request, "/projectId")?;
    let command_id = required(request, "/commandId")?;
    let idempotency_key = required(request, "/idempotencyKey")?;
    let now = required(request, "/requestedAt")?;
    let expected_revision = request
        .get("expectedRevision")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            "IPC_INVALID:expectedRevision required for signing confirmation".to_string()
        })?;
    let platform_id = required(request, "/payload/platformId")?;
    if !MAINSTREAM_PLATFORM_IDS.contains(&platform_id.as_str()) {
        return Err("SIGNING_CONFIRMATION_PLATFORM_INVALID".into());
    }
    let handle = manager
        .open(&project_id, "", &now)
        .map_err(|error| error.to_string())?;
    if let Some(previous) = CommitService::find_receipt(&handle, &project_id, &idempotency_key)
        .map_err(|error| error.to_string())?
    {
        let receipt_ref = previous
            .committed_artifact_refs
            .first()
            .cloned()
            .ok_or_else(|| "B03B_SIGNING_RECEIPT_REF_MISSING".to_string())?;
        let payload =
            NarrativeRepository::artifact_payload_by_ref(&handle, &project_id, &receipt_ref)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "B03B_SIGNING_RECEIPT_PAYLOAD_MISSING".to_string())?;
        if payload
            .pointer("/confirmation/platformId")
            .and_then(Value::as_str)
            != Some(platform_id.as_str())
        {
            return Err("IDEMPOTENCY_CONFLICT".into());
        }
        return command_response(&command_id, &project_id, &previous, &payload, &receipt_ref);
    }
    let receipt_ref = NarrativeRepository::canonical_project_head_ref(
        &handle,
        &project_id,
        "SimulatedSigningReceipt",
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "SIGNING_EVALUATION_REQUIRED".to_string())?;
    let Some(mut payload) =
        NarrativeRepository::artifact_payload_by_ref(&handle, &project_id, &receipt_ref)
            .map_err(|error| error.to_string())?
    else {
        return Err("SIGNING_EVALUATION_REQUIRED".into());
    };
    if payload.get("status").and_then(Value::as_str) != Some("PASS") {
        return Err("SIGNING_CONFIRMATION_REQUIRES_PASS".into());
    }
    if payload
        .pointer("/confirmation/confirmed")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Err("SIGNING_ALREADY_CONFIRMED".into());
    }
    let selected_passes = payload
        .get("platformMatrix")
        .and_then(Value::as_array)
        .is_some_and(|matrix| {
            matrix.iter().any(|entry| {
                entry.get("platformId").and_then(Value::as_str) == Some(platform_id.as_str())
                    && entry.get("status").and_then(Value::as_str) == Some("PASS")
                    && entry.get("mainstream").and_then(Value::as_bool) == Some(true)
            })
        });
    if !selected_passes {
        return Err("SIGNING_CONFIRMATION_PLATFORM_NOT_PASS".into());
    }
    if !signing_evaluation_is_current_in_project(
        &handle,
        &project_id,
        &ArtifactRef {
            artifact_id: receipt_ref.artifact_id.clone(),
            version: receipt_ref.version,
            payload_hash: receipt_ref.payload_hash.clone(),
        },
    )
    .map_err(|error| error.to_string())?
    {
        return Err("SIGNING_EVALUATION_STALE".into());
    }
    payload["confirmation"] = json!({
        "confirmed":true,
        "confirmedBy":"LOCAL_USER",
        "confirmedAt":now,
        "platformId":platform_id
    });
    payload["receiptHash"] = json!("");
    payload["receiptHash"] = json!(sha256_canonical(&payload).map_err(|error| error.to_string())?);
    let next_version =
        NarrativeRepository::next_artifact_version(&handle, &project_id, &receipt_ref.artifact_id)
            .map_err(|error| error.to_string())?;
    let source_refs = payload
        .get("sourceRefs")
        .and_then(Value::as_object)
        .map(|sources| {
            sources
                .values()
                .filter_map(ref_from_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let chapter_refs = payload
        .get("chapterEvidence")
        .and_then(Value::as_array)
        .map(|chapters| {
            chapters
                .iter()
                .filter_map(|chapter| chapter.get("finalChapterRef").and_then(ref_from_value))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let dependencies = dedupe_refs(source_refs.into_iter().chain(chapter_refs));
    let (artifact, new_ref) = build_receipt_artifact(
        &project_id,
        &receipt_ref.artifact_id,
        next_version,
        &now,
        &command_id,
        payload.clone(),
        dependencies,
    )?;
    let chapter_ids = payload
        .get("openingChapterRefs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let canon_refs = payload
        .get("openingCanonRefs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let evidence_refs = payload
        .get("signingEvidenceRefs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let receipt_draft = ReceiptDraftContext {
        domain: "B03B_SIGNING_CONFIRMATION",
        project_id: &project_id,
        command_id: &command_id,
        idempotency_key: &idempotency_key,
        expected_revision: Some(expected_revision),
        now: &now,
        status: "PASS",
        opening_chapter_refs: &chapter_ids,
        opening_canon_refs: &canon_refs,
        signing_evidence_refs: &evidence_refs,
    };
    let bundle = commit_receipt_bundle(&receipt_draft, artifact, &new_ref)?;
    let receipt = CommitService::commit_with_post_apply(&handle, &bundle, &now, |connection, _| {
        DurableJobService::set_signing_projection(
            connection,
            &project_id,
            "SIGNING",
            "PASS",
            "IDLE",
            None,
            &now,
        )
        .map_err(|error| {
            crate::commit::CommitError::ContractInvalid(format!(
                "B03B_CONFIRMATION_PROJECTION:{error}"
            ))
        })
    })
    .map_err(|error| error.to_string())?;
    command_response(&command_id, &project_id, &receipt, &payload, &new_ref)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_chapter(chapter_no: i64, body: &str) -> ChapterInput {
        ChapterInput {
            chapter_id: format!("chapter:{chapter_no}"),
            chapter_no,
            title: format!("第{chapter_no}章"),
            final_ref: ArtifactRef {
                artifact_id: format!("final:{chapter_no}"),
                version: 1,
                payload_hash: "a".repeat(64),
            },
            body: body.to_owned(),
            audit_unresolved_hard: Some(false),
        }
    }

    fn all_pass_model_items(chapters: &[ChapterInput]) -> Vec<Value> {
        let chapter = &chapters[0];
        model_item_ids()
            .iter()
            .map(|item_id| {
                json!({
                    "itemId":item_id,
                    "result":"PASS",
                    "reason":"fixture pass",
                    "evidence":[{"chapterNo":chapter.chapter_no,"rangeStart":0,"rangeEnd":3,"exactQuote":chapter.body.chars().take(3).collect::<String>()}]
                })
            })
            .collect()
    }

    #[test]
    fn signing_quote_mismatch_is_not_verified_without_rebinding() {
        let chapter = fixture_chapter(1, "林舟找到一页旧账");
        let result = model_item_result(
            Some(&json!({
                "itemId":"R2-5",
                "result":"PASS",
                "reason":"fixture",
                "evidence":[{"chapterNo":1,"rangeStart":0,"rangeEnd":2,"exactQuote":"林舟错"}]
            })),
            "R2-5",
            &[chapter],
            None,
        );
        assert_eq!(result["result"], "NOT_VERIFIED");
        assert!(result["evidence"].as_array().unwrap().is_empty());
    }

    #[test]
    fn first_layer_failure_blocks_model_evaluation() {
        let chapters = (1..=3)
            .map(|chapter_no| fixture_chapter(chapter_no, "短正文"))
            .collect::<Vec<_>>();
        let corpus = OriginalityCorpus {
            bodies: Vec::new(),
            digest: "b".repeat(64),
            complete: true,
        };
        let deterministic = deterministic_results(&chapters, &corpus);
        assert!(deterministic_model_gate(
            &deterministic,
            &chapters,
            &json!({"openingContractComplete":true,"chaptersComplete":true})
        )
        .is_some_and(|reason| reason.contains("不调用模型")));
    }

    #[test]
    fn originality_check_fails_on_a_long_duplicate_and_stays_unverified_when_incomplete() {
        let repeated = "林舟在旧账册里找到一段可以逐字核对的共同记录。".repeat(12);
        let chapters = vec![fixture_chapter(1, &repeated)];
        let complete_corpus = OriginalityCorpus {
            bodies: vec![("old-final:1".into(), repeated.clone())],
            digest: "e".repeat(64),
            complete: true,
        };
        assert!(has_duplicate_long_span(&chapters, &complete_corpus));

        let incomplete_corpus = OriginalityCorpus {
            bodies: Vec::new(),
            digest: "f".repeat(64),
            complete: false,
        };
        let results = deterministic_results(&chapters, &incomplete_corpus);
        assert_eq!(result_for(&results, "C0-3"), "NOT_VERIFIED");
    }

    #[test]
    fn critical_e311_failure_fails_the_common_core() {
        let body = "林舟找到证据并决定继续调查。".repeat(2);
        let chapters = (1..=3)
            .map(|chapter_no| fixture_chapter(chapter_no, &body))
            .collect::<Vec<_>>();
        let mut items = vec![
            status_result("C0-3", "PASS", "fixture", Value::Null),
            status_result("F1-1", "PASS", "fixture", Value::Null),
            status_result("F1-2", "PASS", "fixture", Value::Null),
            status_result("F1-3", "PASS", "fixture", Value::Null),
            status_result("F1-4", "PASS", "fixture", Value::Null),
            status_result("F1-5", "PASS", "fixture", Value::Null),
        ];
        items.extend(all_pass_model_items(&chapters));
        if let Some(item) = items.iter_mut().find(|item| item["itemId"] == "E3-11") {
            item["result"] = json!("FAIL");
        }
        assert_eq!(core_status(&items), "FAIL");
    }

    #[test]
    fn prohibited_content_failure_fails_the_common_core() {
        let body = "林舟找到证据并决定继续调查。".repeat(2);
        let chapters = (1..=3)
            .map(|chapter_no| fixture_chapter(chapter_no, &body))
            .collect::<Vec<_>>();
        let mut items = vec![
            status_result("C0-3", "PASS", "fixture", Value::Null),
            status_result("F1-1", "PASS", "fixture", Value::Null),
            status_result("F1-2", "PASS", "fixture", Value::Null),
            status_result("F1-3", "PASS", "fixture", Value::Null),
            status_result("F1-4", "PASS", "fixture", Value::Null),
            status_result("F1-5", "PASS", "fixture", Value::Null),
        ];
        items.extend(all_pass_model_items(&chapters));
        if let Some(item) = items.iter_mut().find(|item| item["itemId"] == "C0-2") {
            item["result"] = json!("FAIL");
        }
        assert_eq!(core_status(&items), "FAIL");
    }

    #[test]
    fn core_pass_without_a_passing_mainstream_platform_is_not_a_signing_pass() {
        let body = "林舟找到证据并决定继续调查。".repeat(2);
        let chapters = (1..=3)
            .map(|chapter_no| fixture_chapter(chapter_no, &body))
            .collect::<Vec<_>>();
        let mut items = vec![
            status_result("C0-3", "PASS", "fixture", Value::Null),
            status_result("F1-1", "PASS", "fixture", Value::Null),
            status_result("F1-2", "PASS", "fixture", Value::Null),
            status_result("F1-3", "PASS", "fixture", Value::Null),
            status_result("F1-4", "PASS", "fixture", Value::Null),
            status_result("F1-5", "PASS", "fixture", Value::Null),
        ];
        items.extend(all_pass_model_items(&chapters));
        let mut profiles = platform_profiles().unwrap();
        profiles[0]["compatibility"] = json!("NOT_VERIFIED");
        let core = core_status(&items);
        assert_eq!(core, "PASS");
        let matrix = platform_matrix(&items, &profiles, core, 6_000);
        assert_eq!(overall_status(core, &matrix), "NOT_VERIFIED");
    }

    #[test]
    fn platform_profile_change_changes_the_bound_rule_digest() {
        let core = core_rules().unwrap();
        let profiles = platform_profiles().unwrap();
        let original = rule_set_digest(&core, &profiles).unwrap();
        let mut changed = profiles;
        changed[0]["version"] = json!("fanqie.r2");
        assert_ne!(original, rule_set_digest(&core, &changed).unwrap());
    }

    #[test]
    fn chapter_two_final_ref_change_changes_the_signing_binding_digest() {
        let make_ref = |artifact_id: &str| ArtifactRef {
            artifact_id: artifact_id.into(),
            version: 1,
            payload_hash: "c".repeat(64),
        };
        let make_snapshot = |chapter_two_ref: ArtifactRef| OpeningEvaluationSnapshot {
            chapters: vec![
                crate::repositories::narrative::OpeningEvaluationChapter {
                    chapter_id: "chapter:1".into(),
                    chapter_no: 1,
                    final_ref: Some(make_ref("final:1")),
                    evidence: Vec::new(),
                    canon_refs: Vec::new(),
                },
                crate::repositories::narrative::OpeningEvaluationChapter {
                    chapter_id: "chapter:2".into(),
                    chapter_no: 2,
                    final_ref: Some(chapter_two_ref),
                    evidence: Vec::new(),
                    canon_refs: Vec::new(),
                },
                crate::repositories::narrative::OpeningEvaluationChapter {
                    chapter_id: "chapter:3".into(),
                    chapter_no: 3,
                    final_ref: Some(make_ref("final:3")),
                    evidence: Vec::new(),
                    canon_refs: Vec::new(),
                },
            ],
            opening_canon_refs: Vec::new(),
            signing_evidence_refs: Vec::new(),
        };
        let sources = [
            ("storyConstitutionRef".into(), make_ref("constitution:1")),
            ("mainContractRef".into(), make_ref("main:1")),
            ("openingContractRef".into(), make_ref("opening:1")),
            ("bookPlanSeedRef".into(), make_ref("seed:1")),
        ];
        let actual = OpeningActualState {
            semantic_digest: "d".repeat(64),
            ..Default::default()
        };
        let original = source_binding(&make_snapshot(make_ref("final:2")), &actual, &sources)
            .unwrap()
            .1;
        let changed = source_binding(
            &make_snapshot(make_ref("final:2:changed")),
            &actual,
            &sources,
        )
        .unwrap()
        .1;
        assert_ne!(original, changed);
    }
}
