use ai_novel_production_system_core::commands::{
    execute_book_plan_revise_with_observer, execute_command_impl, query_chapter_versions_impl,
    query_history_impl, query_production_center_impl, query_single_chapter_impl,
    query_story_directions_impl, KernelState,
};
use ai_novel_production_system_core::commit::CommitService;
use ai_novel_production_system_core::generated::types::{ArtifactRef, CommitBundle};
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

fn signing_fixture_body(chapter_no: usize, reality_variant: &str) -> String {
    let opening = if reality_variant == "A" {
        match chapter_no {
            1 => "林舟在雨夜翻开账本，发现账本缺页，页边的墨迹指向顾清。".to_string(),
            2 => "顾清把旧账册摊在桌上，承认缺失页码与昨夜的争执有关。".to_string(),
            3 => "林舟沿门缝找到被顾清夺走的账页，决定继续追查证据。".to_string(),
            _ => format!("第{chapter_no}章，林舟带着前三章留下的证据继续追查账本。"),
        }
    } else {
        match chapter_no {
            1 => "林舟在雨夜守住账本，发现封条上的暗记来自旧案。".to_string(),
            2 => "顾清拿出另一份记录，确认两人暂时结盟并共同核对。".to_string(),
            3 => "两人带着新的线索离开档案室，决定追问账本的去向。".to_string(),
            _ => format!("第{chapter_no}章，两人沿着新线索追查账册背后的责任。"),
        }
    };
    let mut body = opening;
    for index in 0..3500_u32 {
        let offset = (index
            .wrapping_mul(7919)
            .wrapping_add(index.wrapping_mul(index).wrapping_mul(13))
            .wrapping_add(chapter_no as u32 * 977))
            % 20_992;
        body.push(char::from_u32(0x4e00 + offset).expect("CJK fixture character"));
    }
    body
}

fn full_signing_items(input: &Value) -> Value {
    let chapters = input
        .pointer("/taskContext/chapters")
        .and_then(Value::as_array)
        .expect("signing chapters");
    let chapter = &chapters[0];
    let body = chapter["body"].as_str().expect("chapter body");
    let quote = body.chars().take(40).collect::<String>();
    let range_end = quote.chars().count();
    let mut ids = vec![
        "C0-2", "R2-1", "R2-2", "R2-3", "R2-4", "R2-5", "R2-6", "E3-1", "E3-2", "E3-3", "E3-4",
        "E3-5", "E3-6", "E3-7", "E3-8", "E3-9", "E3-11",
    ];
    ids.extend([
        "E3-10:FANQIE",
        "E3-10:QIDIAN",
        "E3-10:QIMAO",
        "E3-10:ZONGHENG",
        "E3-10:JJWXC",
    ]);
    let signing_items = ids.into_iter().map(|item_id| json!({
        "itemId":item_id,
        "result":"PASS",
        "reason":"确定性测试评估器返回有据结论。",
        "evidence":[{"chapterNo":chapter["chapterNo"],"rangeStart":0,"rangeEnd":range_end,"exactQuote":quote}]
    })).collect::<Vec<_>>();
    json!({
        "skillId":input["skillId"],
        "skillVersion":input["skillVersion"],
        "bundleDigest":input["bundleDigest"],
        "taskId":input["taskContext"]["taskId"],
        "status":"OK",
        "sourceRefs":[],
        "assumptions":[],
        "payload":{
            "quality_vector":{},
            "findings":[],
            "promise_coverage":{},
            "desire_state":{},
            "repetition":{},
            "signing_recommendation":{},
            "repair_scope":[],
            "signingEvaluationItems":signing_items
        },
        "findings":[],
        "contextRefs":[],
        "runtimeCheckRequests":[],
        "confidence":1.0,
        "nextSkillHints":[]
    })
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
                "body":signing_fixture_body(self.audit_calls.load(Ordering::SeqCst) + 1, self.reality_variant)
            }),
            "AUDIT_EXTRACT" => {
                let chapter_no = self.audit_calls.fetch_add(1, Ordering::SeqCst) + 1;
                let body = signing_fixture_body(chapter_no, self.reality_variant);
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
                        "storyTime":format!("2026-01-{:02}T00:00:00Z", chapter_no),
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
                        "acquiredStoryTime":"2026-01-02T00:00:00Z",
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
            "OPENING_SIGNING_EVALUATION" => full_signing_items(input),
            other => return Err(format!("unexpected B02 capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

struct NoSigningEvidenceGateway {
    inner: B02Gateway,
}

impl ProductionModelGateway for NoSigningEvidenceGateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        self.inner.resolve_profile()
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        if capability == "OPENING_SIGNING_EVALUATION" {
            let mut output = full_signing_items(input);
            output["payload"]["signingEvaluationItems"] = json!([]);
            return Ok(ProductionModelResponse::deterministic(output));
        }
        self.inner.invoke(capability, input)
    }
}

struct ShortSigningBodyGateway {
    inner: B02Gateway,
}

impl ProductionModelGateway for ShortSigningBodyGateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        self.inner.resolve_profile()
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        if capability == "RENDER_CHAPTER" {
            return Ok(ProductionModelResponse::deterministic(
                json!({"body":"林舟推开石门"}),
            ));
        }
        let mut response = self.inner.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" {
            let body = "林舟推开石门";
            let evidence = json!({
                "start":0,
                "end":body.chars().count(),
                "quote":body
            });
            for candidate in response.output["canonCandidates"]
                .as_array_mut()
                .into_iter()
                .flatten()
            {
                candidate["evidence"] = evidence.clone();
            }
            for candidate in response.output["knowledgeCandidates"]
                .as_array_mut()
                .into_iter()
                .flatten()
            {
                candidate["evidence"] = evidence.clone();
            }
            response.output["evidenceRanges"] = json!([evidence]);
        }
        Ok(response)
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
            long_body: signing_fixture_body(1, "A"),
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

fn state_without_signing_evidence() -> (KernelState, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("project directory");
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(NoSigningEvidenceGateway {
            inner: B02Gateway::new(true, "A"),
        }),
    );
    (state, temp)
}

fn state_with_custom_gateway(
    gateway: Arc<dyn ProductionModelGateway>,
) -> (KernelState, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("project directory");
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway);
    (state, temp)
}

fn revision(state: &KernelState, project_id: &str) -> i64 {
    state
        .manager
        .project_revision(project_id)
        .expect("revision")
}

fn install_opening_fixture_gate_rules(state: &KernelState, project_id: &str) {
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    let has_policy =
        NarrativeRepository::canonical_project_head_ref(&connection, project_id, "GatePolicy")
            .unwrap()
            .is_some();
    drop(connection);
    if !has_policy {
        command(
            state,
            project_id,
            "b02:test-gate-policy-default",
            "gate_policy_update",
            json!({"action":"ADOPT_DEFAULT"}),
        );
    }
    for (index, predicate) in [
        "found_evidence",
        "protected_oath",
        "appears_in_opening",
        "opening_event",
        "distrusts",
    ]
    .into_iter()
    .enumerate()
    {
        command(
            state,
            project_id,
            &format!("b02:test-gate-rule:{index}"),
            "gate_policy_update",
            json!({
                "action":"UPSERT_RULE",
                "rule":{"predicate":predicate,"cardinality":"MULTI","scope":{"kind":"FIXED","value":"PROJECT"}}
            }),
        );
    }
}

fn command(
    state: &KernelState,
    project_id: &str,
    command_id: &str,
    command_type: &str,
    payload: Value,
) -> Value {
    if command_type == "book_plan_revise" {
        let projection = query_production_center_impl(
            state,
            json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
        )
        .expect("pre-approval projection");
        if projection
            .pointer("/signing/status")
            .and_then(Value::as_str)
            == Some("PASS")
            && projection
                .pointer("/signing/confirmation/confirmed")
                .and_then(Value::as_bool)
                != Some(true)
        {
            let platform_id = projection
                .pointer("/signing/platformMatrix")
                .and_then(Value::as_array)
                .and_then(|items| {
                    items
                        .iter()
                        .find(|item| item["mainstream"] == true && item["status"] == "PASS")
                })
                .and_then(|item| item["platformId"].as_str())
                .expect("a passing mainstream platform")
                .to_owned();
            execute_command_impl(
                state,
                json!({
                    "type":"signing_confirm",
                    "commandId":format!("{command_id}:user-confirmation"),
                    "idempotencyKey":format!("{command_id}:user-confirmation:key"),
                    "projectId":project_id,
                    "expectedRevision":revision(state, project_id),
                    "requestedAt":NOW,
                    "payload":{"platformId":platform_id}
                }),
            )
            .expect("explicit passing platform confirmation");
        }
    }
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
    install_opening_fixture_gate_rules(state, &project_id);
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

fn current_b02_book_plan_payload(state: &KernelState, project_id: &str) -> Value {
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    let raw: String = connection
        .query_row(
            "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='BookPlan'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn install_legacy_signing_head(state: &KernelState, project_id: &str) -> ArtifactRef {
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    let current_ref = NarrativeRepository::canonical_project_head_ref(
        &connection,
        project_id,
        "SimulatedSigningReceipt",
    )
    .unwrap()
    .expect("current signing receipt");
    let current_payload =
        NarrativeRepository::artifact_payload_by_ref(&connection, project_id, &current_ref)
            .unwrap()
            .expect("current signing receipt payload");
    let legacy_payload = json!({
        "payloadSchema":"simulated-signing-receipt.legacy",
        "status":"PASS",
        "verdict":"旧规则评估通过",
        "openingChapterRefs":current_payload["openingChapterRefs"],
        "openingCanonRefs":current_payload["openingCanonRefs"],
        "signingEvidenceRefs":current_payload["signingEvidenceRefs"],
        "confirmation":current_payload["confirmation"]
    });
    let payload_hash = sha256_canonical(&legacy_payload).unwrap();
    let version = NarrativeRepository::next_artifact_version(
        &connection,
        project_id,
        &current_ref.artifact_id,
    )
    .unwrap();
    let artifact_id = current_ref.artifact_id.clone();
    let command_id = format!("test:legacy-signing-head:{project_id}");
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":format!("commit:{command_id}"),
        "projectId":project_id,
        "commandId":command_id,
        "idempotencyKey":format!("{command_id}:key"),
        "expectedProjectRevision":state.manager.project_revision(project_id).unwrap(),
        "contractVersion":"m0a.v1",
        "createdAt":NOW,
        "artifacts":[{
            "artifactId":artifact_id,
            "projectId":project_id,
            "artifactType":"SimulatedSigningReceipt",
            "scope":{"level":"PROJECT"},
            "version":version,
            "schemaVersion":"m0a.artifact.v1",
            "artifactStatus":"CANDIDATE",
            "payload":legacy_payload,
            "payloadHash":payload_hash,
            "dependencyRefs":[],
            "provenance":{"createdBy":"legacy-test-fixture","commandId":command_id},
            "createdAt":NOW
        }],
        "headUpdates":[{
            "artifactType":"SimulatedSigningReceipt",
            "scope":{"level":"PROJECT"},
            "artifactId":artifact_id,
            "version":version,
            "payloadHash":payload_hash
        }]
    }))
    .unwrap();
    CommitService::commit(&connection, &bundle, NOW).unwrap();
    ArtifactRef {
        artifact_id,
        version,
        payload_hash,
    }
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
                    let reserved_for_volume_no = if index == 1 {
                        reservation_volume_no - 1
                    } else {
                        reservation_volume_no + index - 1
                    };
                    reservation["reservedForVolumeNo"] = json!(reserved_for_volume_no);
                    if index == 1 {
                        reservation["notBeforeVolumeNo"] = json!(reserved_for_volume_no);
                    }
                }
                reservation
            })
            .collect::<Vec<_>>());
        let previous_volume_reservation = ten_volume_book["futureReservations"][1]["reservationId"]
            .as_str()
            .unwrap()
            .to_string();
        let volume_two_revealed = BTreeSet::from([previous_volume_reservation]);
        let (one_volume_reservations, one_excluded) = project_applicable_future_reservations(
            &one_volume_book,
            reservation_volume_no,
            &BTreeMap::new(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        )
        .unwrap();
        let (ten_volume_reservations, ten_excluded) = project_applicable_future_reservations(
            &ten_volume_book,
            reservation_volume_no,
            &BTreeMap::new(),
            &BTreeSet::new(),
            &volume_two_revealed,
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
            "F03_BOOKPLAN_HARD_SCALE currentVolume={reservation_volume_no} volumes=1 seeds=1 estimatedTokens={} serializedBytes={} tokenPercent={:.2} bytePercent={:.2} volumes=10 seeds=10 excludedByScopeOrReveal={ten_excluded} estimatedTokens={} serializedBytes={} tokenPercent={:.2} bytePercent={:.2}",
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

        let last_volume_no = ten_volume_book["futureReservations"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|reservation| reservation["reservedForVolumeNo"].as_i64())
            .max()
            .unwrap();
        assert_eq!(last_volume_no, 10, "F03 fixture must measure volume 10");
        let last_volume_revealed = ten_volume_book["futureReservations"]
            .as_array()
            .unwrap()
            .iter()
            .take(9)
            .filter_map(|reservation| reservation["reservationId"].as_str())
            .map(ToOwned::to_owned)
            .collect::<BTreeSet<_>>();
        let (last_volume_reservations, last_volume_excluded) =
            project_applicable_future_reservations(
                &ten_volume_book,
                last_volume_no,
                &BTreeMap::new(),
                &BTreeSet::new(),
                &last_volume_revealed,
            )
            .unwrap();
        assert_eq!(last_volume_reservations.len(), 1);
        assert_eq!(last_volume_excluded, 9);
        let last_volume_input = with_reservations(input.clone(), &last_volume_reservations);
        let last_volume_usage = measure_context_hard_budget(&last_volume_input).unwrap();
        assert!(
            last_volume_usage.estimated_tokens
                <= ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT,
            "F03 last volume exceeds HARD token limit: {last_volume_usage:?}"
        );
        assert!(
            last_volume_usage.serialized_bytes
                <= ai_novel_production_system_core::narrative::CONTEXT_HARD_SERIALIZED_BYTE_LIMIT,
            "F03 last volume exceeds HARD byte limit: {last_volume_usage:?}"
        );
        println!(
            "F03_BOOKPLAN_HARD_LAST_VOLUME currentVolume={last_volume_no} volumes=10 seeds=10 revealedPriorReservations={} includedReservations={} excludedReservations={} estimatedTokens={} serializedBytes={} tokenPercent={:.2} bytePercent={:.2}",
            last_volume_revealed.len(),
            last_volume_reservations.len(),
            last_volume_excluded,
            last_volume_usage.estimated_tokens,
            last_volume_usage.serialized_bytes,
            last_volume_usage.estimated_tokens as f64
                * 100.0
                / ai_novel_production_system_core::narrative::CONTEXT_HARD_TOKEN_LIMIT as f64,
            last_volume_usage.serialized_bytes as f64
                * 100.0
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
    assert_eq!(evaluation["result"]["coreStatus"], "PASS");
    assert_eq!(
        evaluation["result"]["evaluationRuleVersion"],
        "signing-eval.r1"
    );
    let signing_model_run_id = evaluation["result"]["modelRunId"]
        .as_str()
        .expect("durable signing ModelRun");
    let model_storage = state.manager.open(&project_id, "", NOW).unwrap();
    let model_run: (String, String) = model_storage
        .query_row(
            "SELECT capability,status FROM model_runs WHERE project_id=?1 AND model_run_id=?2",
            libsql_rusqlite::params![project_id, signing_model_run_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        model_run,
        ("OPENING_SIGNING_EVALUATION".into(), "COMPLETED".into())
    );
    let raw_response_len: i64 = model_storage
        .query_row(
            "SELECT length(raw_response) FROM model_responses WHERE model_run_id=?1",
            [signing_model_run_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(raw_response_len > 0);
    drop(model_storage);
    let matrix = evaluation["result"]["platformMatrix"].as_array().unwrap();
    for (platform_id, expected) in [
        ("FANQIE", "PASS"),
        ("QIDIAN", "FAIL"),
        ("QIMAO", "NOT_VERIFIED"),
        ("ZONGHENG", "NOT_VERIFIED"),
    ] {
        let platform = matrix
            .iter()
            .find(|item| item["platformId"] == platform_id)
            .unwrap();
        assert_eq!(platform["status"], expected);
    }
    assert_eq!(evaluation["result"]["confirmation"]["confirmed"], false);
    let core_items = evaluation["result"]["itemResults"].as_array().unwrap();
    let safety_item = core_items
        .iter()
        .find(|item| item["itemId"] == "C0-2")
        .unwrap();
    assert_eq!(safety_item["result"], "PASS");
    assert!(!safety_item["evidence"].as_array().unwrap().is_empty());
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
    assert_eq!(center["signing"]["confirmation"]["confirmed"], false);
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

    let before_unconfirmed_approval = revision(&state, &project_id);
    let unconfirmed_approval = execute_command_impl(
        &state,
        json!({
            "type":"book_plan_revise",
            "commandId":"b02:book-plan-revise:unconfirmed",
            "idempotencyKey":"b02:book-plan-revise:unconfirmed:key",
            "projectId":project_id,
            "expectedRevision":before_unconfirmed_approval,
            "requestedAt":NOW,
            "payload":{}
        }),
    );
    assert_eq!(
        unconfirmed_approval.unwrap_err(),
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );
    assert_eq!(revision(&state, &project_id), before_unconfirmed_approval);
    let unconfirmed_db = state.manager.open(&project_id, "", NOW).unwrap();
    assert_eq!(
        unconfirmed_db
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='BookPlan' AND json_extract(p.payload_json,'$.planningMaturity')='APPROVED'",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        unconfirmed_db
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='SimulatedSigningReceipt' AND artifact_status='COMMITTED'",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        1
    );
    drop(unconfirmed_db);

    let before_wrong_platform = revision(&state, &project_id);
    let wrong_platform = execute_command_impl(
        &state,
        json!({
            "type":"signing_confirm",
            "commandId":"b02:signing-confirm:wrong-platform",
            "idempotencyKey":"b02:signing-confirm:wrong-platform:key",
            "projectId":project_id,
            "expectedRevision":before_wrong_platform,
            "requestedAt":NOW,
            "payload":{"platformId":"QIDIAN"}
        }),
    );
    assert_eq!(
        wrong_platform.unwrap_err(),
        "SIGNING_CONFIRMATION_PLATFORM_NOT_PASS"
    );
    assert_eq!(revision(&state, &project_id), before_wrong_platform);
    let wrong_platform_db = state.manager.open(&project_id, "", NOW).unwrap();
    assert_eq!(
        wrong_platform_db
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='SimulatedSigningReceipt' AND artifact_status='COMMITTED'",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        1
    );
    drop(wrong_platform_db);

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
    let confirmed_center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("production center after user confirmation");
    assert_eq!(
        confirmed_center["signing"]["confirmation"]["confirmed"],
        true
    );
    assert_eq!(
        confirmed_center["signing"]["confirmation"]["platformId"],
        "FANQIE"
    );

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
    assert_eq!(
        after
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='SimulatedSigningReceipt' AND artifact_status='COMMITTED'",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        2,
        "unconfirmed evaluation history remains beside the confirmed receipt version"
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

    command(
        &state,
        &project_id,
        "b02:story-setup:change-contract",
        "story_setup_save",
        json!({"storyCore":{
            "protagonist":"林舟",
            "desire":"找回被移走的完整账册",
            "mechanism":"每次核验都会留下可追踪代价",
            "conflict":"旧秩序试图掩盖账册的真实来源",
            "readerPromise":"每章回收一条证据并留下新的选择",
            "forbiddenReveals":["前三章不揭开账册来源"]
        }}),
    );
    let after_contract_change = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("production center after contract change");
    assert_eq!(after_contract_change["signing"]["status"], "STALE");
    assert_eq!(
        after_contract_change["signing"]["receipt"]["id"], signing_receipt_id,
        "the prior signing receipt remains in history after its contract binding changes"
    );
    let history_db = state.manager.open(&project_id, "", NOW).unwrap();
    assert_eq!(
        ai_novel_production_system_core::signing::require_current_signing_for_long_form_if_present(
            &history_db,
            &project_id,
        )
        .unwrap_err(),
        "SIGNING_EVALUATION_STALE"
    );
    assert_eq!(
        history_db
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='SimulatedSigningReceipt' AND artifact_status='COMMITTED'",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        2
    );
    assert!(history_db
        .query_row::<i64, _, _>(
            "SELECT COUNT(*) FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='BookPlan' AND json_extract(p.payload_json,'$.planningMaturity')='APPROVED'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap()
        >= 1);
    println!("B02_RESULT opening_evaluation=PASS signing_receipt=PASS real_evidence=PASS approved_book_plan=PASS seed_preserved=PASS canon_unchanged=PASS status=PASS");
}

#[test]
fn b02_legacy_rule_head_is_stale_then_r1_reapproval_preserves_history() {
    let (state, _temp) = state(true);
    let project_id = create_opening_project(&state);
    commit_three_opening_chapters(&state, &project_id);
    let seed_payload = current_b02_book_plan_payload(&state, &project_id);

    let first_evaluation = command(
        &state,
        &project_id,
        "b02:legacy-receipt:first-evaluation",
        "opening_evaluate",
        json!({}),
    );
    assert_eq!(first_evaluation["result"]["status"], "PASS");
    let first_approval = command(
        &state,
        &project_id,
        "b02:legacy-receipt:first-approval",
        "book_plan_revise",
        json!({}),
    );
    assert_eq!(first_approval["result"]["planningMaturity"], "APPROVED");

    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let old_approved_ref =
        NarrativeRepository::canonical_project_head_ref(&connection, &project_id, "BookPlan")
            .unwrap()
            .expect("first approved BookPlan head");
    drop(connection);
    let legacy_receipt_ref = install_legacy_signing_head(&state, &project_id);

    let stale_center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("center with legacy signing head");
    assert_eq!(stale_center["signing"]["status"], "STALE");
    assert_eq!(
        stale_center["signing"]["verdict"],
        "签约规则已更新，请重新评估"
    );

    let before_revision = revision(&state, &project_id);
    let stale_approval = execute_command_impl(
        &state,
        json!({
            "type":"book_plan_revise",
            "commandId":"b02:legacy-receipt:stale-approval",
            "idempotencyKey":"b02:legacy-receipt:stale-approval:key",
            "projectId":project_id,
            "expectedRevision":before_revision,
            "requestedAt":NOW,
            "payload":{"plan":seed_payload}
        }),
    );
    assert_eq!(stale_approval.unwrap_err(), "SIGNING_EVALUATION_STALE");
    assert_eq!(revision(&state, &project_id), before_revision);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    assert_eq!(
        NarrativeRepository::canonical_project_head_ref(&connection, &project_id, "BookPlan")
            .unwrap(),
        Some(old_approved_ref.clone())
    );
    assert_eq!(
        NarrativeRepository::canonical_project_head_ref(
            &connection,
            &project_id,
            "SimulatedSigningReceipt"
        )
        .unwrap(),
        Some(legacy_receipt_ref.clone())
    );
    let old_approved =
        NarrativeRepository::artifact_payload_by_ref(&connection, &project_id, &old_approved_ref)
            .unwrap()
            .expect("old approved payload remains queryable");
    assert_eq!(old_approved["planningMaturity"], "APPROVED");
    let legacy_receipt =
        NarrativeRepository::artifact_payload_by_ref(&connection, &project_id, &legacy_receipt_ref)
            .unwrap()
            .expect("legacy receipt remains queryable");
    assert_eq!(legacy_receipt["status"], "PASS");
    assert_eq!(legacy_receipt.get("evaluationRuleVersion"), None);
    drop(connection);

    let reevaluation = command(
        &state,
        &project_id,
        "b02:legacy-receipt:r1-reevaluation",
        "opening_evaluate",
        json!({}),
    );
    assert_eq!(reevaluation["result"]["status"], "PASS");
    let reevaluation_center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("r1 reevaluation projection");
    assert_eq!(reevaluation_center["bookPlan"]["reapprovalRequired"], false);
    let platform_id = reevaluation_center["signing"]["platformMatrix"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entry| entry["mainstream"] == true && entry["status"] == "PASS")
        .and_then(|entry| entry["platformId"].as_str())
        .expect("passing mainstream platform");
    command(
        &state,
        &project_id,
        "b02:legacy-receipt:r1-confirmation",
        "signing_confirm",
        json!({"platformId":platform_id}),
    );
    let confirmed_center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("confirmed r1 receipt projection");
    assert_eq!(confirmed_center["bookPlan"]["reapprovalRequired"], true);
    let reapproval = command(
        &state,
        &project_id,
        "b02:legacy-receipt:r1-reapproval",
        "book_plan_revise",
        json!({}),
    );
    assert_eq!(reapproval["result"]["planningMaturity"], "APPROVED");

    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let new_approved_ref =
        NarrativeRepository::canonical_project_head_ref(&connection, &project_id, "BookPlan")
            .unwrap()
            .expect("new approved BookPlan head");
    assert_ne!(new_approved_ref, old_approved_ref);
    drop(connection);
    let final_center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("center after reapproval");
    assert_eq!(final_center["bookPlan"]["reapprovalRequired"], false);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    assert_eq!(
        NarrativeRepository::artifact_payload_by_ref(&connection, &project_id, &old_approved_ref)
            .unwrap()
            .unwrap()["planningMaturity"],
        "APPROVED"
    );
    assert_eq!(
        NarrativeRepository::artifact_payload_by_ref(&connection, &project_id, &legacy_receipt_ref)
            .unwrap()
            .unwrap()["status"],
        "PASS"
    );
    let current_signing_ref = NarrativeRepository::canonical_project_head_ref(
        &connection,
        &project_id,
        "SimulatedSigningReceipt",
    )
    .unwrap()
    .expect("new signing receipt head");
    let current_signing_payload = NarrativeRepository::artifact_payload_by_ref(
        &connection,
        &project_id,
        &current_signing_ref,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        current_signing_payload["evaluationRuleVersion"],
        "signing-eval.r1"
    );
    assert_eq!(current_signing_payload["confirmation"]["confirmed"], true);
}

#[test]
fn b3b_signing_missing_exact_item_evidence_cannot_pass() {
    let (state, _temp) = state_without_signing_evidence();
    let project_id = create_opening_project(&state);
    commit_three_opening_chapters(&state, &project_id);

    let evaluation = command(
        &state,
        &project_id,
        "b3b:signing:no-evidence",
        "opening_evaluate",
        json!({}),
    );

    assert_eq!(evaluation["result"]["status"], "NOT_VERIFIED");
    assert!(evaluation["result"]["platformMatrix"].as_array().is_some());
    let before_approval = revision(&state, &project_id);
    let approval = execute_command_impl(
        &state,
        json!({
            "type":"book_plan_revise",
            "commandId":"b3b:signing:no-evidence:approve",
            "idempotencyKey":"b3b:signing:no-evidence:approve:key",
            "projectId":project_id,
            "expectedRevision":before_approval,
            "requestedAt":NOW,
            "payload":{}
        }),
    );
    assert_eq!(
        approval.unwrap_err(),
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );
    assert_eq!(revision(&state, &project_id), before_approval);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='BookPlan' AND json_extract(p.payload_json,'$.planningMaturity')='APPROVED'",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn b3b_first_layer_failure_does_not_create_a_signing_model_run() {
    let (state, _temp) = state_with_custom_gateway(Arc::new(ShortSigningBodyGateway {
        inner: B02Gateway::new(true, "A"),
    }));
    let project_id = create_opening_project(&state);
    commit_three_opening_chapters(&state, &project_id);
    let before = state.manager.open(&project_id, "", NOW).unwrap();
    let before_runs: i64 = before
        .query_row(
            "SELECT COUNT(*) FROM model_runs WHERE project_id=?1 AND capability='OPENING_SIGNING_EVALUATION'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(before_runs, 0);
    drop(before);

    let evaluation = command(
        &state,
        &project_id,
        "b3b:signing:f1-block",
        "opening_evaluate",
        json!({}),
    );

    assert_eq!(evaluation["result"]["status"], "FAIL");
    assert_eq!(evaluation["result"]["modelRunId"], Value::Null);
    let after = state.manager.open(&project_id, "", NOW).unwrap();
    assert_eq!(
        after
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_runs WHERE project_id=?1 AND capability='OPENING_SIGNING_EVALUATION'",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        0
    );
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
fn b02_opening_evaluation_persists_not_verified_without_committed_canon() {
    let (state, _temp) = state(false);
    let project_id = create_opening_project(&state);
    commit_three_opening_chapters(&state, &project_id);

    let before = state.manager.open(&project_id, "", NOW).unwrap();
    assert_eq!(
        before
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM canon_evidence WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        0
    );
    drop(before);

    let evaluation = command(
        &state,
        &project_id,
        "b02:opening-evaluate:not-verified",
        "opening_evaluate",
        json!({}),
    );
    assert_eq!(evaluation["finalStatus"], "committed");
    assert_eq!(evaluation["result"]["status"], "NOT_VERIFIED");
    assert_eq!(
        evaluation["result"]["reasonCode"],
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );
    assert_eq!(evaluation["result"]["modelRunId"], Value::Null);
    assert!(evaluation["result"]["receiptRef"]["artifactId"].is_string());
    let after = state.manager.open(&project_id, "", NOW).unwrap();
    assert_eq!(
        after
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM model_runs WHERE project_id=?1 AND capability='OPENING_SIGNING_EVALUATION'",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap(),
        0
    );
    drop(after);

    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("production center after revise evaluation");
    assert_eq!(center["signing"]["status"], "NOT_VERIFIED");
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
    println!("B02_SIGNING_RESULT missing_committed_canon=NOT_VERIFIED zero_model_runs=PASS receipt_committed=PASS plan_blocked=PASS status=PASS");
}

#[test]
fn b02_signing_runtime_phases_are_backend_projected_with_one_active_step() {
    let (state, temp) = state(true);
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
    let signing_gateway = B02Gateway::new(true, "A");
    let skill_loader =
        ai_novel_production_system_core::creative_skills::SkillBundleLoader::development_package();
    let paths = AppPaths::new(temp.path());
    let result = execute_opening_evaluation_with_observer(
        &state.manager,
        &paths,
        &signing_gateway,
        &skill_loader,
        &request,
        &mut |phase| {
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
        },
    )
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
    command(
        &state,
        &project_id,
        "b02:signing-confirm:before-plan",
        "signing_confirm",
        json!({"platformId":"FANQIE"}),
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
