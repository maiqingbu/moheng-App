use ai_novel_production_system_core::commands::{
    execute_book_plan_revise_with_observer, execute_command_impl, query_chapter_versions_impl,
    query_history_impl, query_production_center_impl, query_single_chapter_impl,
    query_story_directions_impl, KernelState,
};
use ai_novel_production_system_core::narrative::{
    measure_context_hard_budget, ContextCompilerInput,
};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::signing::execute_opening_evaluation_with_observer;
use serde_json::{json, Value};
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
}

impl LongEvidenceGateway {
    fn new() -> Self {
        Self {
            inner: B02Gateway::new(true, "A"),
            long_body: "雨夜里的工作室留下了必须追查的现实痕迹。".repeat(60),
            render_calls: AtomicUsize::new(0),
            audit_calls: AtomicUsize::new(0),
            planning_contexts: Mutex::new(Vec::new()),
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
    for chapter_no in 1..=3 {
        let center = query_production_center_impl(
            state,
            json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
        )
        .expect("production center");
        assert_eq!(center["nextChapter"]["number"], chapter_no);
        command(
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
    }
}

#[test]
fn f03_formal_context_projection_carries_three_openings_through_long_form_and_reopen() {
    let temp = tempfile::tempdir().expect("F03 app-data directory");
    let gateway = Arc::new(LongEvidenceGateway::new());
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let project_id = create_opening_project(&state);
    commit_three_opening_chapters(&state, &project_id);

    let opening_contexts = gateway.planning_contexts.lock().unwrap();
    assert_eq!(opening_contexts.len(), 3);
    assert_eq!(opening_contexts[0]["selectedEvidence"], json!([]));
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
            usage.estimated_tokens <= 1024,
            "opening {}: {usage:?}",
            index + 1
        );
        println!(
            "F03_OPENING_CONTEXT chapter={} estimatedTokens={} serializedBytes={}",
            index + 1,
            usage.estimated_tokens,
            usage.serialized_bytes
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
    command(
        &state,
        &project_id,
        "f03:chapter-4-plan",
        "planning_compile_chapter_plan",
        json!({
            "chapterId":chapter_four,
            "chapterGoalHint":"根据前三章正式事实追查账本缺页的去向。"
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
        assert!(usage.estimated_tokens <= 1024);
        println!(
            "F03_LONG_FORM_CONTEXT estimatedTokens={} serializedBytes={}",
            usage.estimated_tokens, usage.serialized_bytes
        );
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
    assert!(history["entries"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| { item["id"] == commit_receipt_id && item["receipt"] == commit_receipt_id }));
    println!(
        "F03_CHAIN opening=3 openingEvaluation=PASS longForm=1 finalArtifact={} version={} payloadHash={} productionReceipt={} reopen=PASS",
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
    let relationship_text = approved_payload["relationshipMacroDirections"].to_string();
    let mystery_text = approved_payload["majorMysteryGoals"].to_string();
    let achievement_text = approved_payload["mustAchieve"].to_string();
    assert!(relationship_text.contains("互不信任"));
    assert!(mystery_text.contains("账本被顾清夺走"));
    assert!(achievement_text.contains("账本缺页"));
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
fn b02_approved_book_plan_changes_when_opening_relationship_and_event_change() {
    let variant_a = approved_payload_for_variant("A");
    let variant_b = approved_payload_for_variant("B");
    assert_ne!(
        variant_a["relationshipMacroDirections"],
        variant_b["relationshipMacroDirections"]
    );
    assert_ne!(
        variant_a["majorMysteryGoals"],
        variant_b["majorMysteryGoals"]
    );
    assert!(variant_a["relationshipMacroDirections"]
        .to_string()
        .contains("互不信任"));
    assert!(variant_b["relationshipMacroDirections"]
        .to_string()
        .contains("结盟"));
    assert!(variant_a["majorMysteryGoals"]
        .to_string()
        .contains("账本被顾清夺走"));
    assert!(variant_b["majorMysteryGoals"]
        .to_string()
        .contains("账本由林舟保管"));
    println!("B02_SEMANTIC_RESULT relation=PASS knowledge=PASS event=PASS variant_difference=PASS status=PASS");
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
