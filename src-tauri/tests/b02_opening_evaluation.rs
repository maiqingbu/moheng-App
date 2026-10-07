use ai_novel_production_system_core::commands::{
    execute_book_plan_revise_with_observer, execute_command_impl, query_chapter_versions_impl,
    query_history_impl, query_production_center_impl, query_single_chapter_impl,
    query_story_directions_impl, KernelState,
};
use ai_novel_production_system_core::commit::CommitService;
use ai_novel_production_system_core::generated::types::ArtifactRef;
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::narrative::{
    measure_context_hard_budget, project_applicable_future_reservations, ContextCompilerInput,
    GoalContractField,
};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
use ai_novel_production_system_core::signing::execute_opening_evaluation_with_observer;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const NOW: &str = "2026-09-13T12:00:00Z";

struct B02Gateway {
    audit_calls: AtomicUsize,
    emit_evidence: bool,
    reality_variant: &'static str,
}

impl B02Gateway {
    fn new(emit_evidence: bool, reality_variant: &'static str) -> Self {
        Self {
            audit_calls: AtomicUsize::new(0),
            emit_evidence,
            reality_variant,
        }
    }
}

impl ProductionModelGateway for B02Gateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        Ok(ResolvedProviderProfile {
            provider: "b02-deterministic-provider".into(),
            model: "b02-deterministic-model".into(),
            endpoint: "fixture://b02".into(),
            timeout_ms: 1_000,
        })
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let output = match capability {
            "GENERATE_STORY_DIRECTIONS" => json!({
                "pitches": (0..3)
                    .map(|index| json!({
                        "name":format!("B02 方向 {}", index + 1),
                        "promise":"每章回收一个真实线索并留下新的选择",
                        "protagonistDesire":"查明失踪账本的真实去向",
                        "mechanism":"每次追查都会留下可验证代价",
                        "worldConflict":"旧秩序试图掩盖账本中的秘密",
                        "longArc":"从被动躲藏走向主动追查",
                        "openingHook":"雨夜的账本少了一页",
                        "volumeGoal":"找到账本缺失页的去向",
                        "commercialEdge":"悬疑线索与成长选择并行",
                        "originality":format!("B02 原创支点 {index}"),
                        "difficulty":"中等",
                        "failureRisk":"线索回收过早"
                    }))
                    .collect::<Vec<_>>()
            }),
            "PLAN_STRATEGY" => json!({
                "payloadSchema":"chapter-plan.r1",
                "chapterId":input["chapterId"],
                "compiledFromProjectRevision":input["compiledFromProjectRevision"],
                "compiledFromStateDigest":input["compiledFromStateDigest"],
                "contextManifestRef":input["contextManifestRef"],
                "chapterGoal":"让主角完成一次主动追查并留下证据",
                "activeTaskRefs":[],
                "continuityActions":[],
                "requiredStateOutcomes":[],
                "forbiddenTransitions":[],
                "knowledgeBoundaries":[],
                "reservationRefs":[],
                "sceneExecution":[],
                "horizonPreview":[
                    {"slotNo":1,"objective":"追查账本线索","tentative":true},
                    {"slotNo":2,"objective":"承担追查代价","tentative":true},
                    {"slotNo":3,"objective":"留下新的问题","tentative":true}
                ]
            }),
            "RENDER_CHAPTER" => json!({
                "body": if self.reality_variant == "A" {
                    format!("林舟在雨夜翻开账本，发现第{}章留下的真实证据。", self.audit_calls.load(Ordering::SeqCst) + 1)
                } else {
                    format!("林舟在雨夜守住账本，发现第{}章留下的真实盟约。", self.audit_calls.load(Ordering::SeqCst) + 1)
                }
            }),
            "AUDIT_EXTRACT" => {
                let chapter_no = self.audit_calls.fetch_add(1, Ordering::SeqCst) + 1;
                let body = if self.reality_variant == "A" {
                    format!("林舟在雨夜翻开账本，发现第{}章留下的真实证据。", chapter_no)
                } else {
                    format!("林舟在雨夜守住账本，发现第{}章留下的真实盟约。", chapter_no)
                };
                let canon_candidates = if self.emit_evidence {
                    let mut candidates = vec![json!({
                        "factId":format!("fact:b02:opening:{}", chapter_no),
                        "subjectId":"entity:b02:linzhou",
                        "entityId":"entity:b02:linzhou",
                        "canonicalName":"林舟",
                        "entityType":"CHARACTER",
                        "predicate":if self.reality_variant == "A" { "found_evidence" } else { "protected_oath" },
                        "objectJson":if self.reality_variant == "A" { json!({"state":"账本缺页"}) } else { json!({"state":"账本仍在手中"}) },
                        "authorityLevel":"C",
                        "evidence":{"start":0,"end":body.chars().count(),"quote":body}
                    })];
                    if chapter_no == 1 {
                        candidates.push(json!({
                            "factId":"fact:b02:guqing",
                            "subjectId":"entity:b02:guqing",
                            "entityId":"entity:b02:guqing",
                            "canonicalName":"顾清",
                            "entityType":"CHARACTER",
                            "predicate":"appears_in_opening",
                            "objectJson":{"role":"账本见证人"},
                            "authorityLevel":"C",
                            "evidence":{"start":0,"end":body.chars().count(),"quote":body}
                        }));
                        candidates.push(json!({
                            "factId":"fact:b02:relation-anchor",
                            "subjectId":"entity:b02:linzhou",
                            "entityId":"entity:b02:linzhou",
                            "relationId":"relation:b02:linzhou-guqing",
                            "objectId":"entity:b02:guqing",
                            "predicate":if self.reality_variant == "A" { "distrusts" } else { "allied_with" },
                            "objectJson":{"objectId":"entity:b02:guqing","state":if self.reality_variant == "A" { "互不信任" } else { "结盟" }},
                            "authorityLevel":"C",
                            "evidence":{"start":0,"end":body.chars().count(),"quote":body}
                        }));
                    }
                    if chapter_no == 3 {
                        candidates.push(json!({
                            "factId":"fact:b02:chapter-three-event",
                            "subjectId":"entity:b02:linzhou",
                            "entityId":"entity:b02:linzhou",
                            "predicate":"opening_event",
                            "objectJson":if self.reality_variant == "A" { json!({"event":"账本被顾清夺走"}) } else { json!({"event":"账本由林舟保管"}) },
                            "authorityLevel":"C",
                            "eventType":if self.reality_variant == "A" { "LEDGER_STOLEN" } else { "LEDGER_PROTECTED" },
                            "storyTime":format!("2026-01-{:02}T00:00:00Z", chapter_no),
                            "evidence":{"start":0,"end":body.chars().count(),"quote":body}
                        }));
                    }
                    candidates
                } else {
                    Vec::new()
                };
                let mut knowledge_candidates = Vec::new();
                if self.emit_evidence && chapter_no == 2 {
                    knowledge_candidates.push(json!({
                        "knowledgeId":"knowledge:b02:linzhou-ledger",
                        "knowerId":"entity:b02:linzhou",
                        "factId":"fact:b02:opening:1",
                        "epistemicState":"KNOWS",
                        "visibilityScope":"CHARACTER:entity:b02:linzhou",
                        "evidence":{"start":0,"end":body.chars().count(),"quote":body}
                    }));
                }
                json!({
                    "qualityFindings":[],
                    "canonCandidates":canon_candidates,
                    "knowledgeCandidates":knowledge_candidates,
                    "continuityCandidates":[],"taskProgressCandidates":[],"revealedReservationRefs":[],"evidenceRanges":[],
                    "auditCompleteness":if canon_candidates.is_empty() && knowledge_candidates.is_empty() { "COMPLETE_NO_CHANGE" } else { "COMPLETE_WITH_DELTA" },
                    "unresolvedHard":false
                })
            }
            other => return Err(format!("unexpected B02 capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

struct LongEvidenceGateway {
    inner: B02Gateway,
    long_body: String,
    render_calls: AtomicUsize,
    audit_calls: AtomicUsize,
    planning_contexts: Mutex<Vec<Value>>,
    render_contexts: Mutex<Vec<Value>>,
    audit_contexts: Mutex<Vec<Value>>,
}

impl LongEvidenceGateway {
    fn new() -> Self {
        Self {
            inner: B02Gateway::new(true, "A"),
            long_body: "雨夜里的工作室留下了必须追查的现实痕迹。".repeat(60),
            render_calls: AtomicUsize::new(0),
            audit_calls: AtomicUsize::new(0),
            planning_contexts: Mutex::new(Vec::new()),
            render_contexts: Mutex::new(Vec::new()),
            audit_contexts: Mutex::new(Vec::new()),
        }
    }
}

impl ProductionModelGateway for LongEvidenceGateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        self.inner.resolve_profile()
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        if capability == "PLAN_STRATEGY" {
            self.planning_contexts
                .lock()
                .map_err(|error| error.to_string())?
                .push(input["authorityContext"].clone());
        }
        if capability == "RENDER_CHAPTER" {
            self.render_contexts
                .lock()
                .map_err(|error| error.to_string())?
                .push(input["authorityContext"].clone());
        }
        if capability == "AUDIT_EXTRACT" {
            self.audit_contexts
                .lock()
                .map_err(|error| error.to_string())?
                .push(input["authorityContext"].clone());
        }
        let mut response = self.inner.invoke(capability, input)?;
        match capability {
            "RENDER_CHAPTER" if self.render_calls.fetch_add(1, Ordering::SeqCst) == 0 => {
                response.output["body"] = json!(self.long_body);
            }
            "AUDIT_EXTRACT" if self.audit_calls.fetch_add(1, Ordering::SeqCst) == 0 => {
                let evidence = json!({
                    "start":0,
                    "end":self.long_body.chars().count(),
                    "quote":self.long_body
                });
                for candidate in response.output["canonCandidates"]
                    .as_array_mut()
                    .into_iter()
                    .flatten()
                {
                    candidate["evidence"] = evidence.clone();
                }
                response.output["evidenceRanges"] = json!([evidence]);
            }
            _ => {}
        }
        Ok(response)
    }
}

fn state(emit_evidence: bool) -> (KernelState, tempfile::TempDir) {
    state_with_variant(emit_evidence, "A")
}

fn state_with_variant(
    emit_evidence: bool,
    reality_variant: &'static str,
) -> (KernelState, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("project directory");
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(B02Gateway::new(emit_evidence, reality_variant)),
    );
    (state, temp)
}

fn revision(state: &KernelState, project_id: &str) -> i64 {
    state
        .manager
        .project_revision(project_id)
        .expect("revision")
}

fn command(
    state: &KernelState,
    project_id: &str,
    command_id: &str,
    command_type: &str,
    payload: Value,
) -> Value {
    execute_command_impl(
        state,
        json!({
            "type":command_type,
            "commandId":command_id,
            "idempotencyKey":format!("{command_id}:key"),
            "projectId":project_id,
            "expectedRevision":revision(state, project_id),
            "requestedAt":NOW,
            "payload":payload
        }),
    )
    .unwrap_or_else(|error| panic!("{command_type} failed: {error}"))
}

fn raw_command_request(
    state: &KernelState,
    project_id: &str,
    command_id: &str,
    command_type: &str,
    payload: Value,
) -> Value {
    json!({
        "type":command_type,
        "commandId":command_id,
        "idempotencyKey":format!("{command_id}:key"),
        "projectId":project_id,
        "expectedRevision":revision(state, project_id),
        "requestedAt":NOW,
        "payload":payload
    })
}

fn create_opening_project(state: &KernelState) -> String {
    let created = execute_command_impl(
        state,
        json!({
            "type":"project_create_from_idea",
            "commandId":"b02:create",
            "idempotencyKey":"b02:create:key",
            "requestedAt":NOW,
            "payload":{
                "title":"开篇评估真实链",
                "idea":"一个账房学徒发现失踪账本会改写城中所有人的债务。",
                "platform":"起点中文网",
                "mode":"DIRECTOR",
                "tags":["悬疑","成长"],
                "forbidden":"前三章不揭开账本来源"
            }
        }),
    )
    .expect("create project");
    let project_id = created["projectId"]
        .as_str()
        .expect("project id")
        .to_owned();
    command(
        state,
        &project_id,
        "b02:directions",
        "story_directions_generate",
        json!({}),
    );
    let directions = query_story_directions_impl(
        state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":10}),
    )
    .expect("directions");
    let pitch_id = directions["pitches"][0]["id"].as_str().expect("pitch id");
    command(
        state,
        &project_id,
        "b02:select",
        "pitch_select",
        json!({"pitchIds":[pitch_id]}),
    );
    command(
        state,
        &project_id,
        "b02:setup",
        "story_setup_save",
        json!({"storyCore":{
            "protagonist":"林舟",
            "desire":"查明失踪账本的真实去向",
            "mechanism":"每次追查都会留下可验证代价",
            "conflict":"旧秩序试图掩盖账本秘密",
            "readerPromise":"每章回收一个真实线索并留下新的选择",
            "forbiddenReveals":["前三章不揭开账本来源"]
        }}),
    );
    command(
        state,
        &project_id,
        "b02:opening-ready",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        state,
        &project_id,
        "b02:start-creating",
        "opening_start_creating",
        json!({}),
    );
    project_id
}

fn commit_three_opening_chapters(state: &KernelState, project_id: &str) {
    let _ = commit_three_opening_chapters_with_outputs(state, project_id);
}

fn commit_three_opening_chapters_with_outputs(
    state: &KernelState,
    project_id: &str,
) -> Vec<(String, Value)> {
    let mut productions = Vec::with_capacity(3);
    for chapter_no in 1..=3 {
        let center = query_production_center_impl(
            state,
            json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
        )
        .expect("production center");
        assert_eq!(center["nextChapter"]["number"], chapter_no);
        let chapter_id = center["nextChapter"]["id"]
            .as_str()
            .expect("next chapter id")
            .to_owned();
        let production = command(
            state,
            project_id,
            &format!("b02:production:{chapter_no}"),
            "production_start",
            json!({
                "chapterId":center["nextChapter"]["id"],
                "volumeId":center["nextChapter"]["volumeId"],
                "executionMode":"OPENING_BOOTSTRAP",
                "entityIds":[],
                "references":[],
                "skillsEnabled":false
            }),
        );
        productions.push((chapter_id, production));
    }
    productions
}

#[derive(Clone, Copy)]
struct ReopenedReceiptBinding<'a> {
    project_id: &'a str,
    chapter_id: &'a str,
    job_id: &'a str,
    commit_receipt_id: &'a str,
    receipt_ref: &'a Value,
    canonical_receipt_ref: &'a Value,
    receipt_payload: &'a Value,
    final_ref: &'a Value,
    final_payload: &'a Value,
    audit_ref: &'a Value,
    audit_payload: &'a Value,
    commit_receipt: &'a Value,
    history_entry: &'a Value,
}

fn validate_reopened_receipt_binding(evidence: &ReopenedReceiptBinding<'_>) -> Result<(), String> {
    if evidence.receipt_ref != evidence.canonical_receipt_ref {
        return Err("ProductionReceipt header differs from reopened canonical head".into());
    }
    for (label, payload) in [
        ("ProductionReceipt", evidence.receipt_payload),
        ("FinalChapter", evidence.final_payload),
        ("Audit", evidence.audit_payload),
    ] {
        let payload_ref = match label {
            "ProductionReceipt" => evidence.receipt_ref,
            "FinalChapter" => evidence.final_ref,
            _ => evidence.audit_ref,
        };
        let actual_hash = sha256_canonical(payload).map_err(|error| error.to_string())?;
        if payload_ref["payloadHash"].as_str() != Some(actual_hash.as_str()) {
            return Err(format!(
                "{label} payload hash differs from its exact reference"
            ));
        }
    }
    if evidence.receipt_payload["projectId"].as_str() != Some(evidence.project_id)
        || evidence.receipt_payload["chapterId"].as_str() != Some(evidence.chapter_id)
        || evidence.receipt_payload["jobId"].as_str() != Some(evidence.job_id)
    {
        return Err("ProductionReceipt project/chapter/job binding differs".into());
    }
    if evidence.receipt_payload["finalChapterRef"] != *evidence.final_ref {
        return Err("ProductionReceipt finalChapterRef differs from committed FinalChapter".into());
    }
    if evidence.receipt_payload["auditRef"] != *evidence.audit_ref
        || evidence.final_payload["auditRef"] != *evidence.audit_ref
    {
        return Err(
            "ProductionReceipt and FinalChapter do not share the exact Audit reference".into(),
        );
    }
    if evidence.final_payload["chapterId"].as_str() != Some(evidence.chapter_id)
        || evidence.final_payload["body"]
            .as_str()
            .is_none_or(str::is_empty)
    {
        return Err("FinalChapter payload is not the expected chapter or has no body".into());
    }
    if evidence.audit_payload["candidateRef"] != evidence.final_payload["candidateRef"] {
        return Err("Audit payload is not bound to the FinalChapter candidate".into());
    }
    if evidence.commit_receipt["receiptId"].as_str() != Some(evidence.commit_receipt_id)
        || evidence.commit_receipt["projectId"].as_str() != Some(evidence.project_id)
    {
        return Err("formal commit receipt identity or project differs".into());
    }
    for reference in [evidence.receipt_ref, evidence.final_ref, evidence.audit_ref] {
        if !evidence.commit_receipt["committedArtifactRefs"]
            .as_array()
            .is_some_and(|references| references.iter().any(|item| item == reference))
        {
            return Err("formal commit receipt does not include the referenced artifact".into());
        }
    }
    for (artifact_type, reference) in [
        ("ProductionReceipt", evidence.receipt_ref),
        ("FinalChapter", evidence.final_ref),
    ] {
        let matched = evidence.commit_receipt["headChanges"]
            .as_array()
            .is_some_and(|changes| {
                changes.iter().any(|change| {
                    change["artifactType"].as_str() == Some(artifact_type)
                        && change["artifactId"] == reference["artifactId"]
                        && change["version"] == reference["version"]
                        && change["payloadHash"] == reference["payloadHash"]
                })
            });
        if !matched {
            return Err(format!(
                "formal commit receipt does not advance {artifact_type}"
            ));
        }
    }
    if evidence.history_entry["id"].as_str() != Some(evidence.commit_receipt_id)
        || evidence.history_entry["receipt"].as_str() != Some(evidence.commit_receipt_id)
    {
        return Err("reopened History entry differs from the formal commit receipt".into());
    }
    Ok(())
}

fn artifact_payload_after_reopen(
    state: &KernelState,
    project_id: &str,
    reference: &Value,
) -> Option<Value> {
    let reference: ArtifactRef = serde_json::from_value(reference.clone())
        .expect("artifact reference uses the versioned ArtifactRef contract");
    state
        .manager
        .with_read_connection(project_id, NOW, |connection| {
            Ok(
                NarrativeRepository::artifact_payload_by_ref(connection, project_id, &reference)
                    .expect("scoped artifact repository read"),
            )
        })
        .expect("reopened project read connection")
}

fn assert_reopened_production_receipt_binding(
    state: &KernelState,
    project_id: &str,
    chapter_id: &str,
    production: &Value,
    history: &Value,
) {
    let commit_receipt_id = production["receipt"]["receiptId"]
        .as_str()
        .expect("formal commit receipt ID");
    let receipt_ref = production["receipt"]["headChanges"]
        .as_array()
        .and_then(|changes| {
            changes
                .iter()
                .find(|change| change["artifactType"] == "ProductionReceipt")
        })
        .map(|change| {
            json!({
                "artifactId":change["artifactId"],
                "version":change["version"],
                "payloadHash":change["payloadHash"]
            })
        })
        .expect("formal ProductionReceipt head change");
    let canonical_scope = json!({"level":"CHAPTER","chapterId":chapter_id}).to_string();
    let canonical_receipt_ref = state
        .manager
        .with_read_connection(project_id, NOW, |connection| {
            Ok(NarrativeRepository::canonical_head_ref(
                connection,
                project_id,
                "ProductionReceipt",
                &canonical_scope,
            )
            .expect("ProductionReceipt canonical head read")
            .map(|reference| serde_json::to_value(reference).expect("serialize ArtifactRef")))
        })
        .expect("reopened ProductionReceipt head")
        .expect("ProductionReceipt remains canonical after reopen");
    let final_ref = production["finalChapterRef"].clone();
    let final_payload = artifact_payload_after_reopen(state, project_id, &final_ref)
        .expect("exact FinalChapter ref resolves after reopen");
    let queried_chapter = query_single_chapter_impl(
        state,
        json!({"projectId":project_id,"chapterId":chapter_id}),
    )
    .expect("registered chapter query after reopen");
    assert_eq!(queried_chapter["status"], "official");
    assert_eq!(queried_chapter["version"], "v1");
    assert!(queried_chapter["content"][0]
        .as_str()
        .is_some_and(|body| body == final_payload["body"].as_str().unwrap_or_default()));
    let versions = query_chapter_versions_impl(
        state,
        json!({"projectId":project_id,"chapterId":chapter_id,"pageSize":20}),
    )
    .expect("registered chapter-version query after reopen");
    assert!(versions["items"].as_array().unwrap().iter().any(|item| {
        item["id"] == format!("{}:v1", final_ref["artifactId"].as_str().unwrap())
            && item["current"] == true
    }));
    let audit_ref = final_payload["auditRef"].clone();
    let audit_payload = artifact_payload_after_reopen(state, project_id, &audit_ref)
        .expect("exact Audit ref resolves after reopen");
    let receipt_payload = artifact_payload_after_reopen(state, project_id, &receipt_ref)
        .expect("exact ProductionReceipt ref resolves after reopen");
    let commit_idempotency_key = production["receipt"]["idempotencyKey"]
        .as_str()
        .expect("formal commit receipt idempotency key");
    let commit_receipt = state
        .manager
        .with_read_connection(project_id, NOW, |connection| {
            let receipt =
                CommitService::find_receipt(connection, project_id, commit_idempotency_key)
                    .expect("reopened formal commit receipt and digest")
                    .expect("formal commit receipt remains persisted");
            Ok(serde_json::to_value(receipt).expect("serialize formal CommitReceipt"))
        })
        .expect("reopened formal commit receipt query");
    assert_eq!(commit_receipt, production["receipt"]);
    let history_entry = history["entries"]["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["id"] == commit_receipt_id))
        .expect("registered History query returns this commit receipt")
        .clone();
    let evidence = ReopenedReceiptBinding {
        project_id,
        chapter_id,
        job_id: production["jobId"]
            .as_str()
            .expect("formal production job ID"),
        commit_receipt_id,
        receipt_ref: &receipt_ref,
        canonical_receipt_ref: &canonical_receipt_ref,
        receipt_payload: &receipt_payload,
        final_ref: &final_ref,
        final_payload: &final_payload,
        audit_ref: &audit_ref,
        audit_payload: &audit_payload,
        commit_receipt: &commit_receipt,
        history_entry: &history_entry,
    };
    validate_reopened_receipt_binding(&evidence)
        .unwrap_or_else(|error| panic!("reopened receipt binding failed: {error}"));

    let rejects_payload = |payload: &Value| {
        validate_reopened_receipt_binding(&ReopenedReceiptBinding {
            receipt_payload: payload,
            ..evidence
        })
        .is_err()
    };
    for (path, replacement) in [
        ("/finalChapterRef/artifactId", json!("artifact:wrong-final")),
        ("/auditRef/artifactId", json!("artifact:wrong-audit")),
        ("/auditRef/version", json!(999)),
        ("/auditRef/payloadHash", json!("0".repeat(64))),
        ("/projectId", json!("project:wrong")),
        ("/chapterId", json!("chapter:wrong")),
    ] {
        let mut mutant = receipt_payload.clone();
        *mutant
            .pointer_mut(path)
            .expect("mutation target exists in ProductionReceipt") = replacement;
        assert!(
            rejects_payload(&mutant),
            "mutation must be rejected: {path}"
        );
        if path.starts_with("/auditRef/") || path.starts_with("/finalChapterRef/") {
            let reference_path = if path.starts_with("/auditRef/") {
                "/auditRef"
            } else {
                "/finalChapterRef"
            };
            assert!(artifact_payload_after_reopen(
                state,
                project_id,
                mutant.pointer(reference_path).expect("mutated reference")
            )
            .is_none());
        }
    }
    let wrong_history = json!({"id":"receipt:wrong","receipt":"receipt:wrong"});
    assert!(validate_reopened_receipt_binding(&ReopenedReceiptBinding {
        history_entry: &wrong_history,
        ..evidence
    })
    .is_err());
    let mut wrong_commit_receipt = commit_receipt.clone();
    wrong_commit_receipt["receiptId"] = json!("receipt:wrong-commit");
    assert!(validate_reopened_receipt_binding(&ReopenedReceiptBinding {
        commit_receipt: &wrong_commit_receipt,
        ..evidence
    })
    .is_err());
    for (field, replacement) in [
        ("artifactId", json!("artifact:wrong-receipt")),
        ("version", json!(999)),
        ("payloadHash", json!("0".repeat(64))),
    ] {
        let mut wrong_receipt_header = receipt_ref.clone();
        wrong_receipt_header[field] = replacement;
        assert!(validate_reopened_receipt_binding(&ReopenedReceiptBinding {
            receipt_ref: &wrong_receipt_header,
            ..evidence
        })
        .is_err());
        assert!(artifact_payload_after_reopen(state, project_id, &wrong_receipt_header).is_none());
    }
    println!(
        "F03_RECEIPT_BINDING chapter={} commitReceipt={} final={} audit={} productionReceipt={} reopen=PASS mutants=11_REJECTED",
        chapter_id,
        commit_receipt_id,
        final_ref["artifactId"],
        audit_ref["artifactId"],
        receipt_ref["artifactId"]
    );
}

#[test]
fn f03_formal_context_projection_carries_three_openings_through_long_form_and_reopen() {
    let temp = tempfile::tempdir().expect("F03 app-data directory");
    let gateway = Arc::new(LongEvidenceGateway::new());
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let project_id = create_opening_project(&state);
    let opening_productions = commit_three_opening_chapters_with_outputs(&state, &project_id);

    let opening_contexts = gateway.planning_contexts.lock().unwrap();
    assert_eq!(opening_contexts.len(), 3);
    assert_eq!(opening_contexts[0]["selectedEvidence"], json!([]));
    let opening_two_input: ContextCompilerInput =
        serde_json::from_value(opening_contexts[1].clone()).unwrap();
    let deduplicated_forbidden_reveal = opening_two_input
        .goal_contract_fields
        .iter()
        .find(|field| field.field_path == "storyCore.forbiddenReveals[0]")
        .expect("shared StoryConstitution and OpeningContract rule remains HARD once");
    assert!(deduplicated_forbidden_reveal
        .additional_field_paths
        .iter()
        .any(|path| path == "openingContract.forbiddenReveals[0]"));
    assert!(deduplicated_forbidden_reveal
        .additional_source_refs
        .iter()
        .any(|reference| reference.artifact_id.starts_with("opening-contract:")));
    assert_eq!(
        opening_contexts[1]["state"]["facts"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let opening_two_fact_ids = opening_contexts[1]["state"]["facts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|fact| fact["factId"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(opening_two_fact_ids.contains("fact:b02:opening:1"));
    assert!(opening_two_fact_ids.contains("fact:b02:guqing"));
    assert!(opening_two_fact_ids.contains("fact:b02:relation-anchor"));
    assert_eq!(
        opening_contexts[1]["selectedEvidence"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        opening_contexts[1]["selectedEvidence"][0]["quote"],
        Value::Null
    );
    assert_eq!(
        opening_contexts[1]["selectedEvidence"][0]["rangeEnd"],
        gateway.long_body.chars().count()
    );
    for (index, context) in opening_contexts.iter().enumerate() {
        let input: ContextCompilerInput = serde_json::from_value(context.clone()).unwrap();
        let usage = measure_context_hard_budget(&input).unwrap();
        assert!(
            usage.estimated_tokens
                <= ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT,
            "opening {} exceeds token budget: {usage:?}",
            index + 1
        );
        assert!(
            usage.serialized_bytes
                <= ai_novel_production_system_core::narrative::CONTEXT_HARD_SERIALIZED_BYTE_LIMIT,
            "opening {} exceeds byte budget: {usage:?}",
            index + 1
        );
        println!(
            "F03_OPENING_CONTEXT chapter={} estimatedTokens={} serializedBytes={} tokenPercent={:.2} bytePercent={:.2}",
            index + 1,
            usage.estimated_tokens,
            usage.serialized_bytes,
            usage.estimated_tokens as f64 * 100.0
                / ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT as f64,
            usage.serialized_bytes as f64 * 100.0
                / ai_novel_production_system_core::narrative::CONTEXT_HARD_SERIALIZED_BYTE_LIMIT as f64
        );
    }
    drop(opening_contexts);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let (stored_evidence, evidence_truncated) =
        ai_novel_production_system_core::repositories::narrative::NarrativeRepository::
            evidence_for_refs_bounded(
                &connection,
                &project_id,
                &[
                    "fact:b02:opening:1",
                    "fact:b02:guqing",
                    "fact:b02:relation-anchor",
                ],
                100,
            )
            .unwrap();
    assert!(!evidence_truncated);
    assert_eq!(stored_evidence.len(), 3);
    assert!(stored_evidence
        .iter()
        .all(|item| item.quote.as_deref() == Some(gateway.long_body.as_str())));
    drop(connection);

    let evaluation = command(
        &state,
        &project_id,
        "f03:opening-evaluate",
        "opening_evaluate",
        json!({}),
    );
    assert_eq!(evaluation["result"]["status"], "PASS");
    let approval = command(
        &state,
        &project_id,
        "f03:book-plan-revise",
        "book_plan_revise",
        json!({}),
    );
    assert_eq!(approval["result"]["planningMaturity"], "APPROVED");

    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let volume_id: String = connection
        .query_row(
            "SELECT volume_id FROM chapters WHERE project_id=?1 AND chapter_no=1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);
    command(
        &state,
        &project_id,
        "f03:volume-intent",
        "planning_create_volume_intent",
        json!({"volumeId":volume_id}),
    );
    command(
        &state,
        &project_id,
        "f03:volume-plan",
        "planning_create_volume_plan",
        json!({"volumeId":volume_id}),
    );
    let prepared = command(
        &state,
        &project_id,
        "f03:chapter-4-prepare",
        "planning_prepare_next_chapter",
        json!({"volumeId":volume_id}),
    );
    let chapter_four = prepared["chapter"]["chapterId"]
        .as_str()
        .unwrap()
        .to_owned();
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let route_ref = NarrativeRepository::volume_plan_route(&connection, &project_id, &volume_id)
        .unwrap()
        .unwrap();
    let route = NarrativeRepository::artifact_payload_by_ref(&connection, &project_id, &route_ref)
        .unwrap()
        .unwrap();
    let intent =
        NarrativeRepository::artifact_payload(&connection, &project_id, "VolumeIntent", &volume_id)
            .unwrap()
            .unwrap();
    let scope_selection = json!({
        "taskRefs":route["tasks"].as_array().unwrap().iter()
            .filter(|task| task["class"] == "REQUIRED")
            .filter_map(|task| task["taskId"].as_str())
            .take(1)
            .collect::<Vec<_>>(),
        "responsibilityRefs":intent["crossVolumeResponsibilityRefs"],
        "reservationRefs":intent["futureReservationRefs"],
        "entityRefs":["entity:b02:linzhou","entity:b02:guqing"]
    });
    drop(connection);
    command(
        &state,
        &project_id,
        "f03:chapter-4-plan",
        "planning_compile_chapter_plan",
        json!({
            "chapterId":chapter_four,
            "chapterGoalHint":"根据前三章正式事实追查账本缺页的去向。",
            "scopeSelection":scope_selection
        }),
    );
    {
        let contexts = gateway.planning_contexts.lock().unwrap();
        assert_eq!(contexts.len(), 4);
        assert_eq!(contexts[3]["executionMode"], "LONG_FORM");
        assert!(contexts[3]["selectedEvidence"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["quote"].is_null()));
        let input: ContextCompilerInput = serde_json::from_value(contexts[3].clone()).unwrap();
        let usage = measure_context_hard_budget(&input).unwrap();
        let mut typical_chapter_sizes = vec![(4, usage.estimated_tokens, usage.serialized_bytes)];
        let opening_reality_hard = usage
            .source_sizes
            .iter()
            .filter(|source| {
                source.kind == "CONTRACT"
                    && source
                        .field_path
                        .as_deref()
                        .is_some_and(|path| path.contains("opening-reality"))
            })
            .collect::<Vec<_>>();
        assert_eq!(opening_reality_hard.len(), 2);
        for source in &opening_reality_hard {
            assert_eq!(
                input
                    .goal_contract_fields
                    .iter()
                    .find(|field| field.field_path
                        == source.field_path.as_deref().unwrap_or_default())
                    .unwrap()
                    .value
                    .as_object()
                    .map(|value| value.keys().map(String::as_str).collect::<BTreeSet<_>>()),
                Some(BTreeSet::from(["id", "refs"]))
            );
        }
        let opening_after_tokens = opening_reality_hard
            .iter()
            .map(|source| source.estimated_tokens)
            .sum::<i64>();
        let opening_after_bytes = opening_reality_hard
            .iter()
            .map(|source| source.serialized_bytes)
            .sum::<usize>();
        println!(
            "F03_OPENING_REALITY_DEDUP beforeTokens=4435 beforeBytes=13304 afterTokens={opening_after_tokens} afterBytes={opening_after_bytes} beforeFixture=F03-chapter-4-prior-capture"
        );
        println!(
            "F03_LONG_FORM_CONTEXT chapter=4 estimatedTokens={} serializedBytes={} tokenPercent={:.2} bytePercent={:.2}",
            usage.estimated_tokens,
            usage.serialized_bytes,
            usage.estimated_tokens as f64 * 100.0
                / ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT as f64,
            usage.serialized_bytes as f64 * 100.0
                / ai_novel_production_system_core::narrative::CONTEXT_HARD_SERIALIZED_BYTE_LIMIT as f64
        );
        for source in &usage.source_sizes {
            println!(
                "F03_AFTER_HARD_SOURCE chapter=4 kind={} sourceRef={} fieldPath={} estimatedTokens={} serializedBytes={}",
                source.kind,
                source.source_ref,
                source.field_path.as_deref().unwrap_or(""),
                source.estimated_tokens,
                source.serialized_bytes
            );
        }
        let mut contract_sizes = BTreeMap::<String, (i64, usize)>::new();
        for source in usage
            .source_sizes
            .iter()
            .filter(|source| source.kind == "CONTRACT")
        {
            let totals = contract_sizes.entry(source.source_ref.clone()).or_default();
            totals.0 += source.estimated_tokens;
            totals.1 += source.serialized_bytes;
        }
        for (source_ref, (tokens, bytes)) in contract_sizes {
            println!(
                "F03_LONG_FORM_CONTRACT_SOURCE chapter=4 sourceRef={source_ref} estimatedTokens={tokens} serializedBytes={bytes}"
            );
        }
        let mut largest_fields = usage
            .source_sizes
            .iter()
            .filter(|source| source.kind == "CONTRACT")
            .collect::<Vec<_>>();
        largest_fields.sort_by(|left, right| {
            right
                .estimated_tokens
                .cmp(&left.estimated_tokens)
                .then_with(|| left.field_path.cmp(&right.field_path))
        });
        for source in largest_fields.into_iter().take(10) {
            println!(
                "F03_LONG_FORM_CONTRACT_FIELD chapter=4 sourceRef={} fieldPath={} estimatedTokens={} serializedBytes={}",
                source.source_ref,
                source.field_path.as_deref().unwrap_or(""),
                source.estimated_tokens,
                source.serialized_bytes
            );
        }
        for required_path in [
            "storyCore.mechanism",
            "storyCore.forbiddenReveals",
            "hardRules",
            "volumeIntent.mustAchieve[",
            "volumeIntent.mustNotBreak[",
            "volumeIntent.requiredExitConditions[",
        ] {
            assert!(input
                .goal_contract_fields
                .iter()
                .any(|field| field.field_path.starts_with(required_path)));
        }
        for excluded_path in [
            "bookPlan.mustAchieve",
            "bookPlan.mustNotBreak",
            "bookPlan.volumeIntentSeeds",
            "bookPlan.majorMysteryGoals",
            "openingContract.firstChapterGoal",
        ] {
            assert!(input
                .goal_contract_fields
                .iter()
                .all(|field| !field.field_path.starts_with(excluded_path)));
        }
        assert!(input.omitted_soft_sources.iter().any(|source| {
            source
                .source_ref
                .ref_
                .contains("volumeIntent.mustNotBreak[")
                && source.source_ref.ref_.ends_with(".description")
                && source.reason.contains("Canon 的语义副本")
        }));
        assert!(input.omitted_soft_sources.iter().any(|source| {
            source
                .source_ref
                .ref_
                .contains("BookPlan.futureReservations[")
                && source.source_ref.ref_.ends_with(".description")
                && source.reason.contains("SOFT 历史摘录")
        }));
        assert!(input.goal_contract_fields.iter().all(|field| {
            field.source_ref.payload_hash.len() == 64
                && field
                    .additional_source_refs
                    .iter()
                    .all(|reference| reference.payload_hash.len() == 64)
        }));
        for chapter in [10, 30] {
            let mut later_chapter = input.clone();
            later_chapter.state.bounds.chapter_id = Some(format!("chapter:f03:{chapter}"));
            let later_usage = measure_context_hard_budget(&later_chapter).unwrap();
            typical_chapter_sizes.push((
                chapter,
                later_usage.estimated_tokens,
                later_usage.serialized_bytes,
            ));
            println!(
                "F03_LONG_FORM_CONTEXT chapter={chapter} estimatedTokens={} serializedBytes={} tokenPercent={:.2} bytePercent={:.2}",
                later_usage.estimated_tokens,
                later_usage.serialized_bytes,
                later_usage.estimated_tokens as f64 * 100.0
                    / ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT as f64,
                later_usage.serialized_bytes as f64 * 100.0
                    / ai_novel_production_system_core::narrative::CONTEXT_HARD_SERIALIZED_BYTE_LIMIT as f64
            );
        }

        let handle = state.manager.open(&project_id, "", NOW).unwrap();
        let book_ref =
            NarrativeRepository::canonical_project_head_ref(&handle, &project_id, "BookPlan")
                .unwrap()
                .unwrap();
        let book = NarrativeRepository::artifact_payload_by_ref(&handle, &project_id, &book_ref)
            .unwrap()
            .unwrap();
        let current_volume_no =
            NarrativeRepository::volume_number(&handle, &project_id, &volume_id)
                .unwrap()
                .unwrap();
        drop(handle);

        let actual_seed = book["volumeIntentSeeds"].as_array().unwrap()[0].clone();
        let actual_reservation = book["futureReservations"].as_array().unwrap()[0].clone();
        let reservation_volume_no = actual_reservation["reservedForVolumeNo"].as_i64().unwrap();
        assert!(current_volume_no < reservation_volume_no);
        let mut one_volume_book = book.clone();
        one_volume_book["volumeIntentSeeds"] = json!([actual_seed.clone()]);
        one_volume_book["futureReservations"] = json!([actual_reservation.clone()]);

        let mut ten_volume_book = book.clone();
        ten_volume_book["volumeIntentSeeds"] = json!((0..10)
            .map(|index| {
                let mut seed = actual_seed.clone();
                seed["seedId"] = json!(format!("f03:volume-seed:{}", index + 1));
                seed
            })
            .collect::<Vec<_>>());
        assert_eq!(
            ten_volume_book["volumeIntentSeeds"]
                .as_array()
                .unwrap()
                .len(),
            10
        );
        ten_volume_book["futureReservations"] = json!((0..10)
            .map(|index| {
                let mut reservation = actual_reservation.clone();
                if index > 0 {
                    reservation["reservationId"] = json!(format!("f03:reservation:{}", index + 1));
                    reservation["reservedForVolumeNo"] = json!(reservation_volume_no + index);
                }
                reservation
            })
            .collect::<Vec<_>>());
        let (one_volume_reservations, one_excluded) = project_applicable_future_reservations(
            &one_volume_book,
            reservation_volume_no,
            &BTreeMap::new(),
        )
        .unwrap();
        let (ten_volume_reservations, ten_excluded) = project_applicable_future_reservations(
            &ten_volume_book,
            reservation_volume_no,
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(one_volume_reservations, ten_volume_reservations);
        assert_eq!(one_excluded, 0);
        assert_eq!(ten_excluded, 9);

        let with_reservations = |mut base: ContextCompilerInput, reservations: &[Value]| {
            base.goal_contract_fields
                .retain(|field| !field.field_path.starts_with("bookPlan.futureReservations["));
            base.future_reservation_refs.clear();
            for reservation in reservations {
                let reservation_id = reservation["reservationId"].as_str().unwrap().to_owned();
                base.goal_contract_fields.push(GoalContractField {
                    field_path: format!("bookPlan.futureReservations[{reservation_id}]"),
                    source_ref: book_ref.clone(),
                    additional_source_refs: Vec::new(),
                    additional_field_paths: Vec::new(),
                    value: reservation.clone(),
                });
                base.future_reservation_refs.push(reservation_id);
            }
            base
        };
        let one_volume_input = with_reservations(input.clone(), &one_volume_reservations);
        let ten_volume_input = with_reservations(input.clone(), &ten_volume_reservations);
        let one_volume_usage = measure_context_hard_budget(&one_volume_input).unwrap();
        let ten_volume_usage = measure_context_hard_budget(&ten_volume_input).unwrap();
        assert_eq!(
            one_volume_usage.estimated_tokens,
            ten_volume_usage.estimated_tokens
        );
        assert_eq!(
            one_volume_usage.serialized_bytes,
            ten_volume_usage.serialized_bytes
        );
        println!(
            "F03_BOOKPLAN_HARD_SCALE currentVolume={reservation_volume_no} volumes=1 seeds=1 estimatedTokens={} serializedBytes={} tokenPercent={:.2} bytePercent={:.2} volumes=10 seeds=10 excludedFutureReservations=9 estimatedTokens={} serializedBytes={} tokenPercent={:.2} bytePercent={:.2}",
            one_volume_usage.estimated_tokens,
            one_volume_usage.serialized_bytes,
            one_volume_usage.estimated_tokens as f64 * 100.0
                / ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT as f64,
            one_volume_usage.serialized_bytes as f64 * 100.0
                / ai_novel_production_system_core::narrative::CONTEXT_HARD_SERIALIZED_BYTE_LIMIT as f64,
            ten_volume_usage.estimated_tokens,
            ten_volume_usage.serialized_bytes,
            ten_volume_usage.estimated_tokens as f64 * 100.0
                / ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT as f64,
            ten_volume_usage.serialized_bytes as f64 * 100.0
                / ai_novel_production_system_core::narrative::CONTEXT_HARD_SERIALIZED_BYTE_LIMIT as f64
        );

        for field in &serde_json::from_value::<ContextCompilerInput>(contexts[3].clone())
            .unwrap()
            .goal_contract_fields
        {
            let book_sourced = field.source_ref.artifact_id == book_ref.artifact_id
                || field
                    .additional_source_refs
                    .iter()
                    .any(|source_ref| source_ref.artifact_id == book_ref.artifact_id);
            if book_sourced {
                assert!(field.field_path.starts_with("bookPlan.futureReservations["));
                assert!(field
                    .additional_field_paths
                    .iter()
                    .all(|path| path.starts_with("bookPlan.futureReservations[")));
            }
        }

        let mut multi_person = serde_json::to_value(&input).unwrap();
        let mut multi_person_entities = input.state.bounds.entity_ids.clone();
        multi_person_entities.extend((3..=6).map(|index| format!("entity:f03:multi:{index}")));
        assert_eq!(multi_person_entities.len(), 6);
        multi_person["state"]["bounds"]["entityIds"] = json!(multi_person_entities);

        let fact_template = serde_json::to_value(input.state.facts.first().unwrap()).unwrap();
        let mut facts = multi_person["state"]["facts"].as_array().unwrap().clone();
        let mut state_source_refs = multi_person["state"]["sourceRefs"]
            .as_array()
            .unwrap()
            .clone();
        for index in 3..=6 {
            let mut fact = fact_template.clone();
            fact["factId"] = json!(format!("fact:f03:multi:{index}"));
            fact["subjectId"] = json!(format!("entity:f03:multi:{index}"));
            state_source_refs.push(json!({
                "kind":"authority",
                "ref":format!("fact:f03:multi:{index}")
            }));
            facts.push(fact);
        }
        multi_person["state"]["facts"] = json!(facts);
        multi_person["state"]["sourceRefs"] = json!(state_source_refs);

        let route_tasks = route["tasks"].as_array().unwrap();
        assert!(
            route_tasks.len() >= 3,
            "F03 Route must supply three real tasks"
        );
        let selected_tasks = route_tasks.iter().take(3).collect::<Vec<_>>();
        let task_refs = selected_tasks
            .iter()
            .map(|task| task["taskId"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let mut plan_task_contexts = Vec::new();
        let mut task_states = Vec::new();
        for task in selected_tasks {
            let task_id = task["taskId"].as_str().unwrap();
            let objective = task["objective"].as_str().unwrap();
            let mut contract = json!({
                "taskId":task["taskId"],
                "class":task["class"],
                "dependsOnTaskIds":task["dependsOnTaskIds"],
                "continuityThreadRefs":task["continuityThreadRefs"],
                "reservationRefs":task["reservationRefs"],
                "completionPredicate":task["completionPredicate"]
            });
            if let Some(goal) = input.goal_contract_fields.iter().find(|field| {
                field.value.get("constraint").and_then(Value::as_str) == Some(objective)
            }) {
                contract["objectiveContractRef"] = json!({
                    "fieldPath":goal.field_path,
                    "additionalFieldPaths":goal.additional_field_paths,
                    "sourceRef":goal.source_ref,
                    "additionalSourceRefs":goal.additional_source_refs
                });
            } else {
                contract["objective"] = json!(objective);
            }
            plan_task_contexts.push(json!({"taskRef":task_id,"contract":contract}));
            task_states.push(json!({
                "stateId":task_id,
                "value":{"status":"ACTIVE"},
                "chapterId":chapter_four
            }));
        }
        multi_person["planTaskRefs"] = json!(task_refs);
        multi_person["activeTaskRefs"] = json!(task_refs);
        multi_person["planTaskContexts"] = json!(plan_task_contexts);
        multi_person["taskStates"] = json!(task_states);

        let selected_evidence = multi_person["selectedEvidence"].as_array().unwrap().clone();
        assert!(
            selected_evidence.len() >= 3,
            "F03 evidence fixture must supply three references"
        );
        let triggered_responsibilities = (0..3)
            .map(|index| {
                json!({
                    "threadId":format!("thread:f03:trigger:{}", index + 1),
                    "threadClass":"PROMISE",
                    "logicalStatus":"OPEN",
                    "sourceEventRef":input.state.events.first().map(|event| event.event_id.clone()),
                    "evidenceRefs":[selected_evidence[index].clone()],
                    "participantEntities":[
                        multi_person_entities[index * 2],
                        multi_person_entities[index * 2 + 1]
                    ],
                    "resumeCondition":{"kind":"AFTER_CHAPTER","value":4},
                    "authorityLevel":"B",
                    "currentDisposition":"APPROVED",
                    "sourceRefs":[{"kind":"obligation","ref":format!("obligation:f03:trigger:{}", index + 1)}]
                })
            })
            .collect::<Vec<_>>();
        multi_person["continuityThreads"] = json!(triggered_responsibilities);

        let multi_person_input: ContextCompilerInput =
            serde_json::from_value(multi_person).unwrap();
        assert_eq!(multi_person_input.state.bounds.entity_ids.len(), 6);
        assert_eq!(multi_person_input.plan_task_refs.len(), 3);
        assert_eq!(multi_person_input.continuity_threads.len(), 3);
        let multi_person_usage = measure_context_hard_budget(&multi_person_input).unwrap();
        let total_source_tokens = multi_person_usage
            .source_sizes
            .iter()
            .map(|source| source.estimated_tokens)
            .sum::<i64>();
        let total_source_bytes = multi_person_usage
            .source_sizes
            .iter()
            .map(|source| source.serialized_bytes)
            .sum::<usize>();
        for kind in ["CANON", "CONTRACT"] {
            let sources = multi_person_usage
                .source_sizes
                .iter()
                .filter(|source| source.kind == kind)
                .collect::<Vec<_>>();
            let tokens = sources
                .iter()
                .map(|source| source.estimated_tokens)
                .sum::<i64>();
            let bytes = sources
                .iter()
                .map(|source| source.serialized_bytes)
                .sum::<usize>();
            println!(
                "F03_MULTI_PERSON_SOURCE_SHARE kind={kind} estimatedTokens={tokens} tokenShare={:.2} serializedBytes={bytes} byteShare={:.2}",
                tokens as f64 * 100.0 / total_source_tokens.max(1) as f64,
                bytes as f64 * 100.0 / total_source_bytes.max(1) as f64
            );
        }
        println!(
            "F03_MULTI_PERSON_CONTEXT entityCount=6 taskCount=3 triggeredResponsibilityCount=3 estimatedTokens={} serializedBytes={} tokenPercent={:.2} bytePercent={:.2} appliedTokenLimit={}",
            multi_person_usage.estimated_tokens,
            multi_person_usage.serialized_bytes,
            multi_person_usage.estimated_tokens as f64 * 100.0
                / ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT as f64,
            multi_person_usage.serialized_bytes as f64 * 100.0
                / ai_novel_production_system_core::narrative::CONTEXT_HARD_SERIALIZED_BYTE_LIMIT as f64,
            ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT
        );
        for source in &multi_person_usage.source_sizes {
            println!(
                "F03_MULTI_PERSON_HARD_SOURCE kind={} sourceRef={} fieldPath={} estimatedTokens={} serializedBytes={}",
                source.kind,
                source.source_ref,
                source.field_path.as_deref().unwrap_or(""),
                source.estimated_tokens,
                source.serialized_bytes
            );
        }
        assert!(
            multi_person_usage.estimated_tokens
                <= ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT,
            "multi-person F03 fixture exceeds the selected token limit: {multi_person_usage:?}"
        );
        assert!(
            multi_person_usage.serialized_bytes
                <= ai_novel_production_system_core::narrative::CONTEXT_HARD_SERIALIZED_BYTE_LIMIT,
            "multi-person F03 fixture exceeds the selected byte limit: {multi_person_usage:?}"
        );
        for (fixture_name, fixture_usage) in [
            ("single-volume-book", &one_volume_usage),
            ("ten-volume-book", &ten_volume_usage),
        ] {
            assert!(
                fixture_usage.estimated_tokens
                    <= ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT,
                "{fixture_name} exceeds token budget: {fixture_usage:?}"
            );
            assert!(
                fixture_usage.serialized_bytes
                    <= ai_novel_production_system_core::narrative::CONTEXT_HARD_SERIALIZED_BYTE_LIMIT,
                "{fixture_name} exceeds byte budget: {fixture_usage:?}"
            );
        }

        for (chapter, tokens, bytes) in &typical_chapter_sizes {
            assert!(
                *tokens <= ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT,
                "F03 chapter {chapter} exceeds token budget: {tokens}"
            );
            assert!(
                *bytes <= ai_novel_production_system_core::narrative::CONTEXT_HARD_SERIALIZED_BYTE_LIMIT,
                "F03 chapter {chapter} exceeds byte budget: {bytes}"
            );
        }
        let token_min = typical_chapter_sizes
            .iter()
            .map(|(_, tokens, _)| *tokens)
            .min()
            .unwrap();
        let token_max = typical_chapter_sizes
            .iter()
            .map(|(_, tokens, _)| *tokens)
            .max()
            .unwrap();
        let byte_min = typical_chapter_sizes
            .iter()
            .map(|(_, _, bytes)| *bytes)
            .min()
            .unwrap();
        let byte_max = typical_chapter_sizes
            .iter()
            .map(|(_, _, bytes)| *bytes)
            .max()
            .unwrap();
        assert!((token_max - token_min) * 100 <= token_max * 5);
        assert!((byte_max - byte_min) * 100 <= byte_max * 5);
    }
    let mut production_request = raw_command_request(
        &state,
        &project_id,
        "f03:chapter-4-production",
        "production_start",
        json!({
            "chapterId":chapter_four,
            "volumeId":volume_id,
            "executionMode":"LONG_FORM",
            "skillsEnabled":false
        }),
    );
    production_request["requestedAt"] = json!("2026-09-13T12:01:00Z");
    let production = execute_command_impl(&state, production_request).unwrap();
    assert_eq!(production["finalStatus"], "committed");
    assert_eq!(production["stage"], "COMPLETED");
    let commit_receipt_id = production["receipt"]["receiptId"]
        .as_str()
        .expect("formal commit receipt ID")
        .to_owned();
    let production_receipt_ref = production["receipt"]["headChanges"]
        .as_array()
        .unwrap()
        .iter()
        .find(|change| change["artifactType"] == "ProductionReceipt")
        .expect("production receipt head change")
        .clone();
    drop(state);

    let reopened =
        KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let center = query_production_center_impl(&reopened, json!({"projectId":project_id}))
        .expect("formal production query after app-data reopen");
    let final_ref = center["finalChapterRef"].clone();
    assert_eq!(
        final_ref["artifactId"],
        production["finalChapterRef"]["artifactId"]
    );
    assert_eq!(
        final_ref["version"],
        production["finalChapterRef"]["version"]
    );
    assert_eq!(
        final_ref["payloadHash"],
        production["finalChapterRef"]["payloadHash"]
    );
    assert!(final_ref["payloadHash"]
        .as_str()
        .is_some_and(|hash| hash.len() == 64));
    let receipt = center["artifacts"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["id"] == production_receipt_ref["artifactId"])
        .expect("registered query exposes ProductionReceipt artifact");
    assert_eq!(receipt["title"], "ProductionReceipt");
    assert!(receipt["summary"]
        .as_str()
        .unwrap()
        .contains(production_receipt_ref["payloadHash"].as_str().unwrap()));
    let final_chapter = query_single_chapter_impl(
        &reopened,
        json!({"projectId":project_id,"chapterId":chapter_four}),
    )
    .unwrap();
    assert_eq!(final_chapter["status"], "official");
    assert_eq!(final_chapter["version"], "v1");
    assert!(final_chapter["content"][0]
        .as_str()
        .is_some_and(|body| body.contains("第4章")));
    let versions = query_chapter_versions_impl(
        &reopened,
        json!({"projectId":project_id,"chapterId":chapter_four,"pageSize":20}),
    )
    .unwrap();
    assert!(versions["items"].as_array().unwrap().iter().any(|item| {
        item["id"] == format!("{}:v1", final_ref["artifactId"].as_str().unwrap())
            && item["current"] == true
    }));
    let history = query_history_impl(
        &reopened,
        json!({"projectId":project_id,"kind":"VERSION","pageSize":100}),
    )
    .unwrap();
    let first_opening = &opening_productions[0];
    assert_reopened_production_receipt_binding(
        &reopened,
        &project_id,
        &first_opening.0,
        &first_opening.1,
        &history,
    );
    assert_reopened_production_receipt_binding(
        &reopened,
        &project_id,
        &chapter_four,
        &production,
        &history,
    );
    assert!(history["entries"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| { item["id"] == commit_receipt_id && item["receipt"] == commit_receipt_id }));
    println!(
        "F03_CHAIN opening=3 openingEvaluation=PASS longForm=1 finalArtifact={} version={} payloadHash={} productionReceipt={} reopen=PASS receiptBindings=2",
        final_ref["artifactId"],
        final_ref["version"],
        final_ref["payloadHash"],
        receipt["id"]
    );
}

#[test]
fn b02_real_opening_evaluation_and_book_plan_approval_are_authoritative() {
    let (state, _temp) = state(true);
    let project_id = create_opening_project(&state);
    commit_three_opening_chapters(&state, &project_id);

    let before = state.manager.open(&project_id, "", NOW).expect("open db");
    let canon_before: i64 = before
        .query_row(
            "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .expect("canon count");
    let seed_ref: (String, i64) = before
        .query_row(
            "SELECT h.artifact_id,h.version FROM canonical_heads h WHERE h.project_id=?1 AND h.artifact_type='BookPlan'",
            [&project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("seed head");
    assert_eq!(seed_ref.1, 1);
    drop(before);

    let evaluation = command(
        &state,
        &project_id,
        "b02:opening-evaluate",
        "opening_evaluate",
        json!({}),
    );
    assert_eq!(evaluation["finalStatus"], "committed");
    assert_eq!(evaluation["result"]["status"], "PASS");
    let signing_receipt_id = evaluation["result"]["receiptRef"]["artifactId"]
        .as_str()
        .expect("signing receipt ref");
    let replay = command(
        &state,
        &project_id,
        "b02:opening-evaluate",
        "opening_evaluate",
        json!({}),
    );
    assert_eq!(
        replay["result"]["receiptRef"], evaluation["result"]["receiptRef"],
        "idempotent evaluation must replay the original receipt ref"
    );

    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("production center after evaluation");
    assert_eq!(center["projectPhase"], "SIGNING");
    assert_eq!(center["signing"]["status"], "PASS");
    assert_eq!(center["signing"]["receipt"]["id"], signing_receipt_id);
    assert_eq!(
        center["signing"]["openingChapterRefs"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(
        center["signing"]["openingCanonRefs"]
            .as_array()
            .unwrap()
            .len()
            >= 3,
    );
    assert!(
        center["signing"]["signingEvidenceRefs"]
            .as_array()
            .unwrap()
            .len()
            >= 3,
    );
    assert_eq!(center["bookPlan"]["status"], "PENDING");

    let explicit_wrong_receipt = execute_command_impl(
        &state,
        json!({
            "type":"book_plan_revise",
            "commandId":"b02:book-plan-revise:wrong-receipt",
            "idempotencyKey":"b02:book-plan-revise:wrong-receipt:key",
            "projectId":project_id,
            "expectedRevision":revision(&state, &project_id),
            "requestedAt":NOW,
            "payload":{"openingCanonRefs":center["signing"]["openingCanonRefs"],"signingEvidenceRefs":["artifact:fake-signing"]}
        }),
    );
    assert_eq!(
        explicit_wrong_receipt.unwrap_err(),
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );

    let approved = command(
        &state,
        &project_id,
        "b02:book-plan-revise",
        "book_plan_revise",
        json!({}),
    );
    assert_eq!(approved["finalStatus"], "committed");
    assert_eq!(approved["result"]["planningMaturity"], "APPROVED");

    let after = state.manager.open(&project_id, "", NOW).expect("reopen db");
    let canon_after: i64 = after
        .query_row(
            "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .expect("canon count after plan");
    assert_eq!(
        canon_after, canon_before,
        "plan commit must not mutate Canon"
    );
    let seed_exists: i64 = after
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='BookPlan' AND artifact_id=?2 AND version=1",
            libsql_rusqlite::params![project_id, seed_ref.0],
            |row| row.get(0),
        )
        .expect("seed history");
    assert_eq!(seed_exists, 1, "BookPlan SEED history must remain");
    let approved_head: (String, i64) = after
        .query_row(
            "SELECT h.artifact_id,h.version FROM canonical_heads h WHERE h.project_id=?1 AND h.artifact_type='BookPlan'",
            [&project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("approved head");
    assert!(approved_head.1 > 1 || approved_head.0 != seed_ref.0);
    let approved_payload: Value = after
        .query_row(
            "SELECT payload_json FROM artifact_payloads WHERE artifact_id=?1 AND version=?2",
            libsql_rusqlite::params![approved_head.0, approved_head.1],
            |row| {
                let raw: String = row.get(0)?;
                Ok(serde_json::from_str(&raw).unwrap())
            },
        )
        .expect("approved payload");
    for field in [
        "mustAchieve",
        "mustNotBreak",
        "relationshipMacroDirections",
        "majorMysteryGoals",
        "futureReservations",
        "volumeIntentSeeds",
        "globalLengthBudget",
    ] {
        assert!(approved_payload.get(field).is_some(), "missing {field}");
    }
    assert_eq!(
        approved_payload["openingActualStateDigest"]
            .as_str()
            .map(str::len),
        Some(64)
    );
    assert!(
        approved_payload["openingCanonRefs"]
            .as_array()
            .unwrap()
            .len()
            >= 3
    );
    assert!(
        approved_payload["signingEvidenceRefs"]
            .as_array()
            .unwrap()
            .len()
            >= 3
    );
    for (field, key, prefix, expected_description) in [
        (
            "mustAchieve",
            "id",
            "goal:opening-reality:",
            "开篇已提交事实不得违背；以 Canon 为准。",
        ),
        (
            "mustNotBreak",
            "id",
            "rule:opening-reality:",
            "开篇已提交事实不得违背；以 Canon 为准。",
        ),
        (
            "relationshipMacroDirections",
            "id",
            "relationship:opening",
            "开篇关系约束以 Canon 为准；不得无证据跳变。",
        ),
        (
            "majorMysteryGoals",
            "id",
            "mystery:opening-reality:",
            "开篇事实与知识边界以 Canon 为准；不得提前揭示。",
        ),
        (
            "futureReservations",
            "reservationId",
            "reservation:opening-reality:",
            "保留开篇 Canon 已确立的后续责任；以 Canon 为准。",
        ),
        (
            "volumeIntentSeeds",
            "seedId",
            "volume-intent:opening-reality:",
            "下一卷承接开篇 Canon 所确立的责任；以 Canon 为准。",
        ),
    ] {
        let item = approved_payload[field]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item[key].as_str().is_some_and(|id| id.starts_with(prefix)))
            .unwrap_or_else(|| panic!("missing opening item in {field}"));
        assert_eq!(
            item["kind"], "CANON_REFS",
            "{field} must be a Canon reference"
        );
        assert_eq!(item["refs"], approved_payload["openingCanonRefs"]);
        assert_eq!(item["description"], expected_description);
        let description = item["description"].as_str().unwrap();
        for canon_excerpt in ["账本缺页", "互不信任", "账本被顾清夺走"] {
            assert!(
                !description.contains(canon_excerpt),
                "{field} description must not duplicate Canon text"
            );
        }
    }
    drop(after);

    let final_center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("final production center");
    assert_eq!(final_center["bookPlan"]["status"], "APPROVED");
    assert_eq!(final_center["bookPlan"]["seedPreserved"], true);
    assert_eq!(final_center["projectPhase"], "POST_SIGNING_PLANNING");
    println!("B02_RESULT opening_evaluation=PASS signing_receipt=PASS real_evidence=PASS approved_book_plan=PASS seed_preserved=PASS canon_unchanged=PASS status=PASS");
}

fn approved_payload_for_variant(variant: &'static str) -> Value {
    let (state, _temp) = state_with_variant(true, variant);
    let project_id = create_opening_project(&state);
    commit_three_opening_chapters(&state, &project_id);
    command(
        &state,
        &project_id,
        "b02:opening-evaluate:variant",
        "opening_evaluate",
        json!({}),
    );
    command(
        &state,
        &project_id,
        "b02:book-plan-revise:variant",
        "book_plan_revise",
        json!({}),
    );
    let connection = state
        .manager
        .open(&project_id, "", NOW)
        .expect("variant db");
    let (artifact_id, version): (String, i64) = connection
        .query_row(
            "SELECT artifact_id,version FROM canonical_heads WHERE project_id=?1 AND artifact_type='BookPlan'",
            [&project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("variant book plan head");
    connection
        .query_row(
            "SELECT payload_json FROM artifact_payloads WHERE artifact_id=?1 AND version=?2",
            libsql_rusqlite::params![artifact_id, version],
            |row| {
                let raw: String = row.get(0)?;
                Ok(serde_json::from_str(&raw).unwrap())
            },
        )
        .expect("variant payload")
}

#[test]
fn b02_approved_book_plan_references_opening_canon_without_copying_text() {
    let variant_a = approved_payload_for_variant("A");
    let variant_b = approved_payload_for_variant("B");
    assert_ne!(
        variant_a["openingActualStateDigest"],
        variant_b["openingActualStateDigest"]
    );
    for payload in [&variant_a, &variant_b] {
        for field in ["relationshipMacroDirections", "majorMysteryGoals"] {
            let serialized = payload[field].to_string();
            assert!(serialized.contains("CANON_REFS"));
            for canon_excerpt in ["互不信任", "结盟", "账本被顾清夺走", "账本由林舟保管"]
            {
                assert!(!serialized.contains(canon_excerpt));
            }
        }
    }
    println!("B02_SEMANTIC_RESULT relation=CANON_REF knowledge=CANON_REF event=CANON_REF no_text_copy=PASS state_digest=PASS status=PASS");
}

#[test]
fn b02_opening_evaluation_persists_revise_when_real_evidence_is_incomplete() {
    let (state, _temp) = state(false);
    let project_id = create_opening_project(&state);
    commit_three_opening_chapters(&state, &project_id);

    let evaluation = command(
        &state,
        &project_id,
        "b02:opening-evaluate:revise",
        "opening_evaluate",
        json!({}),
    );
    assert_eq!(evaluation["finalStatus"], "committed");
    assert_eq!(evaluation["result"]["status"], "REVISE");
    assert!(evaluation["result"]["receiptRef"]["artifactId"].is_string());

    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("production center after revise evaluation");
    assert_eq!(center["signing"]["status"], "REVISE");
    assert_eq!(center["signing"]["evaluation"]["status"], "COMPLETED");
    assert_eq!(center["bookPlan"]["status"], "PENDING");
    let finalize = execute_command_impl(
        &state,
        json!({
            "type":"book_plan_revise",
            "commandId":"b02:book-plan-revise:blocked",
            "idempotencyKey":"b02:book-plan-revise:blocked:key",
            "projectId":project_id,
            "expectedRevision":revision(&state, &project_id),
            "requestedAt":NOW,
            "payload":{}
        }),
    );
    assert_eq!(
        finalize.unwrap_err(),
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );
    println!("B02_REVISE_RESULT incomplete_evidence=PASS receipt_committed=PASS plan_blocked=PASS status=PASS");
}

#[test]
fn b02_signing_runtime_phases_are_backend_projected_with_one_active_step() {
    let (state, _temp) = state(true);
    let project_id = create_opening_project(&state);
    commit_three_opening_chapters(&state, &project_id);
    let request = raw_command_request(
        &state,
        &project_id,
        "b02:opening-evaluate:phases",
        "opening_evaluate",
        json!({}),
    );
    let mut observed = Vec::new();
    let result = execute_opening_evaluation_with_observer(&state.manager, &request, &mut |phase| {
        let projection = query_production_center_impl(
            &state,
            json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
        )
        .expect("phase projection");
        let active = projection["signing"]["evaluation"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|step| step["status"] == "active")
            .count();
        observed.push((phase.to_string(), active));
    })
    .expect("opening evaluation");
    assert_eq!(result["result"]["status"], "PASS");
    assert_eq!(
        observed
            .iter()
            .map(|(phase, _)| phase.as_str())
            .collect::<Vec<_>>(),
        vec![
            "READ_OPENING_CANON",
            "SUMMARIZE_OPENING",
            "EVALUATE_SIGNING",
            "COMMIT_RECEIPT",
            "COMPLETED"
        ]
    );
    assert!(observed[..4].iter().all(|(_, active)| *active == 1));
    assert_eq!(observed.last().map(|(_, active)| *active), Some(0));
    println!(
        "B02_SIGNING_PHASE_RESULT ordered=PASS backend_active=PASS terminal_zero=PASS status=PASS"
    );
}

#[test]
fn b02_book_plan_runtime_phases_are_backend_projected_with_one_active_step() {
    let (state, _temp) = state(true);
    let project_id = create_opening_project(&state);
    commit_three_opening_chapters(&state, &project_id);
    command(
        &state,
        &project_id,
        "b02:opening-evaluate:before-plan",
        "opening_evaluate",
        json!({}),
    );
    let request = raw_command_request(
        &state,
        &project_id,
        "b02:book-plan-revise:phases",
        "book_plan_revise",
        json!({}),
    );
    let mut observed = Vec::new();
    let result = execute_book_plan_revise_with_observer(&state, &request, &mut |phase| {
        let projection = query_production_center_impl(
            &state,
            json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
        )
        .expect("plan phase projection");
        let active = projection["bookPlan"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|step| step["status"] == "active")
            .count();
        observed.push((phase.to_string(), active));
    })
    .expect("book plan revision");
    assert_eq!(result["result"]["planningMaturity"], "APPROVED");
    assert_eq!(
        observed
            .iter()
            .map(|(phase, _)| phase.as_str())
            .collect::<Vec<_>>(),
        vec![
            "READ_OPENING_CANON",
            "SUMMARIZE_OPENING",
            "GENERATE_BOOK_PLAN",
            "VALIDATE_CONSTRAINTS",
            "COMMIT_BOOK_PLAN",
            "COMPLETED"
        ]
    );
    assert!(observed[..5].iter().all(|(_, active)| *active == 1));
    assert_eq!(observed.last().map(|(_, active)| *active), Some(0));
    println!(
        "B02_PLAN_PHASE_RESULT ordered=PASS backend_active=PASS terminal_zero=PASS status=PASS"
    );
}
