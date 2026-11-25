use ai_novel_production_system_core::commands::{
    execute_command_impl, query_context_manifest_impl, query_plan_route_impl,
    query_production_center_impl, query_volume_list_impl, KernelState,
};
use ai_novel_production_system_core::commit::CommitService;
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const NOW: &str = "2026-09-13T12:00:00Z";

struct BehaviorGateway {
    audit_calls: AtomicUsize,
    planning_contexts: Mutex<Vec<Value>>,
    /// VD-1a: plans the book's direction after signing, with a chapter
    /// table in every reply when `table_directions` is set.
    plans_directions: bool,
    table_directions: bool,
    review_opening_replay: AtomicBool,
    report_stage_done: AtomicBool,
    verify_stage_done: AtomicBool,
    invalid_stage_evidence: AtomicBool,
    recover_stage_evidence: AtomicBool,
    split_stage: AtomicBool,
    enforce_length: AtomicBool,
    fail_capability_once: Mutex<Option<&'static str>>,
    table_volume_directions: AtomicBool,
    while_planning_volume: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    direction_inputs: Mutex<Vec<Value>>,
    /// Round 19: the first audit reports no change; set when it was asked
    /// again with feedback.
    blank_audit_once: AtomicBool,
    /// Chapter 1's first audit has one fact with an unregistered predicate,
    /// and the follow-up round comes back empty.
    blank_feedback_once: AtomicBool,
    audit_feedback: Mutex<Vec<Value>>,
    /// P1: chapter 1 opens a thread, chapter 2 advances it and chapter 3
    /// resolves it, by the ref the audit is shown.
    threads: AtomicBool,
    /// The chapter-1 thread is a planted hint (a foreshadow).
    thread_foreshadows: AtomicBool,
    select_main_event: AtomicBool,
    audit_first_event: Mutex<Option<&'static str>>,
    invalid_main_event: AtomicBool,
    audit_unbound_thread: AtomicBool,
    first_main_event_invalid: AtomicBool,
    open_threads_seen: Mutex<Vec<Value>>,
    /// Every model input, by capability.
    inputs: Mutex<Vec<(String, Value)>>,
}

impl BehaviorGateway {
    fn quote(chapter_no: usize) -> String {
        let opening = match chapter_no {
            1 => "林舟沿着雨水浸透的封条，辨认出一段旧案暗记。",
            2 => "顾清从渡口带回旧账联，发现边角新添了一枚签名。",
            3 => "两人在档案室核对页码后，决定追问账本缺页的去向。",
            _ => "林舟在档案室找到了线人周娘子。周娘子亲手交还了账本缺页。",
        };
        let mut body = opening.to_owned();
        for index in 0..2_400_u32 {
            let offset = index
                .wrapping_mul(7919)
                .wrapping_add(index.wrapping_mul(index).wrapping_mul(13))
                .wrapping_add(chapter_no as u32 * 977)
                % 20_992;
            body.push(char::from_u32(0x4e00 + offset).expect("CJK fixture character"));
        }
        body
    }

    fn signing_evaluation(input: &Value) -> Value {
        let chapter = &input["taskContext"]["chapters"][0];
        let body = chapter["sentences"][0]["text"]
            .as_str()
            .expect("first signing chapter sentence");
        let quote = body.chars().take(40).collect::<String>();
        let mut item_ids = vec![
            "C0-2", "R2-1", "R2-2", "R2-3", "R2-4", "R2-5", "R2-6", "E3-1", "E3-2", "E3-3", "E3-4",
            "E3-5", "E3-6", "E3-7", "E3-8", "E3-9", "E3-11",
        ];
        item_ids.extend([
            "E3-10:FANQIE",
            "E3-10:QIDIAN",
            "E3-10:QIMAO",
            "E3-10:ZONGHENG",
            "E3-10:JJWXC",
        ]);
        let items = item_ids
            .into_iter()
            .map(|item_id| {
                json!({
                    "itemId":item_id,
                    "result":"PASS",
                    "reason":"确定性测试评估器返回有据结论。",
                    "evidence":[{"chapterNo":chapter["chapterNo"],"rangeStart":0,"rangeEnd":quote.chars().count(),"exactQuote":quote}]
                })
            })
            .collect::<Vec<_>>();
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
                "signingEvaluationItems":items
            },
            "findings":[],
            "contextRefs":[],
            "runtimeCheckRequests":[],
            "confidence":1.0,
            "nextSkillHints":[]
        })
    }
}

impl ProductionModelGateway for BehaviorGateway {
    fn enforces_chapter_length(&self) -> bool {
        self.enforce_length.load(Ordering::SeqCst)
    }

    fn reviews_chapter_prose(&self) -> bool {
        false
    }

    fn checks_book_continuation(&self) -> bool {
        self.review_opening_replay.load(Ordering::SeqCst)
    }

    fn selects_main_event(&self) -> bool {
        self.select_main_event.load(Ordering::SeqCst)
    }
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        Ok(ResolvedProviderProfile {
            provider: "b2-red-behavior-provider".into(),
            model: "b2-red-behavior-model".into(),
            endpoint: "fixture://b2-red-behavior".into(),
            timeout_ms: 1_000,
        })
    }

    fn plans_book_direction(&self) -> bool {
        self.plans_directions
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        self.inputs
            .lock()
            .unwrap()
            .push((capability.to_owned(), input.clone()));
        {
            let mut failure = self.fail_capability_once.lock().unwrap();
            if *failure == Some(capability) {
                *failure = None;
                return Err("FIXTURE_AUDIT_FAILURE".into());
            }
        }
        if capability == "PLAN_STRATEGY" {
            self.planning_contexts
                .lock()
                .map_err(|error| error.to_string())?
                .push(input["authorityContext"].clone());
        }

        let output = match capability {
            "GENERATE_STORY_DIRECTIONS" => json!({
                "pitches": (0..3)
                    .map(|index| json!({
                        "name":format!("B2 行为方向 {}", index + 1),
                        "promise":"每章回收一个真实线索并留下新的选择",
                        "protagonistDesire":"查明失踪账本的真实去向",
                        "mechanism":"每次追查都会留下可验证代价",
                        "worldConflict":"旧秩序试图掩盖账本中的秘密",
                        "longArc":"从被动躲藏走向主动追查",
                        "openingHook":"雨夜的账本少了一页",
                        "volumeGoal":"找到账本缺失页的去向",
                        "commercialEdge":"悬疑线索与成长选择并行",
                        "originality":format!("B2 行为测试原创支点 {index}"),
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
                "chapterGoal":"根据正式 Canon 追查账本留下的线索",
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
            "RENDER_CHAPTER" => {
                let chapter_no = self.audit_calls.load(Ordering::SeqCst) + 1;
                let mut body = Self::quote(chapter_no);
                if chapter_no == 5 && self.recover_stage_evidence.load(Ordering::SeqCst) {
                    body = body.replacen(
                        "林舟在档案室找到了线人周娘子。周娘子亲手交还了账本缺页。",
                        "林舟顺着周娘子的指点，从柜底翻出了带朱印的封套。",
                        1,
                    );
                }
                if self.split_stage.load(Ordering::SeqCst) && chapter_no > 3 {
                    let original = "林舟在档案室找到了线人周娘子。周娘子亲手交还了账本缺页。";
                    let current = if chapter_no == 4 {
                        "林舟在档案室找到了线人周娘子。"
                    } else {
                        "周娘子亲手交还了账本缺页。"
                    };
                    body = body.replacen(original, current, 1);
                }
                json!({"body":body})
            }
            "SELECT_MAIN_EVENT"
                if self.invalid_main_event.load(Ordering::SeqCst)
                    || (self.first_main_event_invalid.load(Ordering::SeqCst)
                        && input.get("selectionFeedback").is_none()) =>
            {
                json!({"actionSentence":999,"outcomeSentence":999,"eventType":"DISCOVERY","outcomeQuote":"辨认出一段旧案暗记"})
            }
            "SELECT_MAIN_EVENT" => {
                json!({"firstSentence":0,"outcomeSentence":0,"eventType":"DISCOVERY"})
            }
            "VERIFY_STAGE_STEP" => {
                if self.recover_stage_evidence.load(Ordering::SeqCst) {
                    let quote = "林舟在档案室找到了线人周娘子。";
                    json!({"stageStepCompletion":{
                        "sourceChapter":4,"firstSentence":0,"outcomeSentence":0,"condition":1,
                        "actionQuote":quote,"outcomeQuote":quote
                    }})
                } else if self.invalid_stage_evidence.load(Ordering::SeqCst) {
                    json!({"stageStepCompletion":{"evidenceParts":[{
                        "firstSentence":0,"outcomeSentence":0,"condition":1,"actionQuote":"找到线人",
                        "outcomeQuote":"不合格的超长引文".repeat(100)
                    }]}})
                } else if self.split_stage.load(Ordering::SeqCst) {
                    let first_part = self.audit_calls.load(Ordering::SeqCst) == 4;
                    let condition = if first_part { 1 } else { 2 };
                    let proof = json!({"evidenceParts":[{
                        "firstSentence":0,"outcomeSentence":0,"condition":condition,"actionQuote":input["bodySentences"][0]["text"],
                        "outcomeQuote":input["bodySentences"][0]["text"]
                    }]});
                    if first_part {
                        json!({"stageStepCompletion":null,"stageStepProgress":proof})
                    } else {
                        json!({"stageStepCompletion":proof})
                    }
                } else if !self.verify_stage_done.load(Ordering::SeqCst) {
                    json!({"stageStepCompletion":null})
                } else if input.get("outputFeedback").is_none() {
                    json!({"firstSentence":0,"outcomeSentence":0,"eventType":"DISCOVERY"})
                } else {
                    json!({"stageStepCompletion":{"evidenceParts":[
                        {"firstSentence":0,"outcomeSentence":0,"condition":1,"actionQuote":input["bodySentences"][0]["text"],
                         "outcomeQuote":input["bodySentences"][0]["text"]},
                        {"firstSentence":1,"outcomeSentence":1,"condition":2,"actionQuote":input["bodySentences"][1]["text"],
                         "outcomeQuote":input["bodySentences"][1]["text"]}
                    ]}})
                }
            }
            "AUDIT_EXTRACT" if self.blank_audit_once.swap(false, Ordering::SeqCst) => json!({
                "qualityFindings":[],"canonCandidates":[],"knowledgeCandidates":[],
                "continuityCandidates":[],"taskProgressCandidates":[],
                "revealedReservationRefs":[],"evidenceRanges":[],
                "auditCompleteness":"COMPLETE_NO_CHANGE","unresolvedHard":false
            }),
            "AUDIT_EXTRACT"
                if input.get("auditFeedback").is_some()
                    && self.blank_feedback_once.swap(false, Ordering::SeqCst) =>
            {
                json!({
                    "qualityFindings":[],"canonCandidates":[],"knowledgeCandidates":[],
                    "continuityCandidates":[],"taskProgressCandidates":[],
                    "revealedReservationRefs":[],"evidenceRanges":[],
                    "auditCompleteness":"COMPLETE_NO_CHANGE","unresolvedHard":false
                })
            }
            "AUDIT_EXTRACT" => {
                if input.get("auditFeedback").is_some() {
                    self.audit_feedback
                        .lock()
                        .unwrap()
                        .push(input["auditFeedback"].clone());
                }
                let chapter_no = self.audit_calls.fetch_add(1, Ordering::SeqCst) + 1;
                let quote = input["body"]
                    .as_str()
                    .ok_or("FIXTURE_AUDIT_BODY_REQUIRED")?;
                let evidence = json!({
                    "start":0,
                    "end":quote.chars().count(),
                    "quote":quote
                });
                let mut canon_candidates = vec![json!({
                    "factId":format!("fact:b2:red:opening:{chapter_no}"),
                    "subjectId":"entity:b02:linzhou",
                    "entityId":"entity:b02:linzhou",
                    "canonicalName":"林舟",
                    "entityType":"CHARACTER",
                    "predicate":"found_evidence",
                    "objectJson":{"state":"账本缺页"},
                    "authorityLevel":"C",
                    "storyTime":format!("2026-01-{:02}T00:00:00Z", chapter_no),
                    "evidence":evidence
                })];
                if chapter_no == 1 {
                    if let Some(kind) = *self.audit_first_event.lock().unwrap() {
                        let through_period = quote.find('。').unwrap() + '。'.len_utf8();
                        let outcome = &quote[..through_period];
                        canon_candidates[0]["eventType"] = json!(kind);
                        canon_candidates[0]["evidence"] = json!({
                            "start":0,"end":outcome.chars().count(),"quote":outcome
                        });
                    }
                }
                if chapter_no == 1 {
                    canon_candidates.push(json!({
                        "factId":"fact:b2:red:guqing",
                        "subjectId":"entity:b02:guqing",
                        "entityId":"entity:b02:guqing",
                        "canonicalName":"顾清",
                        "entityType":"CHARACTER",
                        "predicate":"appears_in_opening",
                        "objectJson":{"role":"账本见证人"},
                        "authorityLevel":"C",
                        "evidence":evidence
                    }));
                    canon_candidates.push(json!({
                        "factId":"fact:b2:red:relation-anchor",
                        "subjectId":"entity:b02:linzhou",
                        "entityId":"entity:b02:linzhou",
                        "relationId":"relation:b02:red:linzhou-guqing",
                        "objectId":"entity:b02:guqing",
                        "predicate":"distrusts",
                        "objectJson":{"objectId":"entity:b02:guqing","state":"互不信任"},
                        "authorityLevel":"C",
                        "evidence":evidence
                    }));
                    // Real run round 23: a relation to someone named only
                    // by a title, who is no entity.
                    canon_candidates.push(json!({
                        "factId":"fact:b2:red:relation-unknown",
                        "subjectId":"entity:b02:linzhou",
                        "entityId":"entity:b02:linzhou",
                        "predicate":"relationship",
                        "objectJson":{"objectId":"师父","state":"敬畏"},
                        "authorityLevel":"C",
                        "evidence":evidence
                    }));
                }
                if chapter_no == 3 {
                    canon_candidates.push(json!({
                        "factId":"fact:b2:red:chapter-three-event",
                        "subjectId":"entity:b02:linzhou",
                        "entityId":"entity:b02:linzhou",
                        "predicate":"opening_event",
                        "objectJson":{"event":"账本被顾清夺走"},
                        "authorityLevel":"C",
                        "eventType":"LEDGER_STOLEN",
                        "storyTime":"2026-01-03T00:00:00Z",
                        "evidence":evidence
                    }));
                }
                let knowledge_candidates = if chapter_no == 2 {
                    vec![json!({
                        "knowledgeId":"knowledge:b2:red:linzhou-ledger",
                        "knowerId":"entity:b02:linzhou",
                        "factId":"fact:b2:red:opening:1",
                        "epistemicState":"KNOWS",
                        "visibilityScope":"CHARACTER:entity:b02:linzhou",
                        "acquiredStoryTime":"2026-01-02T00:00:00Z",
                        "evidence":evidence
                    })]
                } else {
                    Vec::new()
                };
                if chapter_no == 1 && self.blank_feedback_once.load(Ordering::SeqCst) {
                    canon_candidates.push(json!({
                        "factId":"fact:b2:red:unregistered",
                        "subjectId":"entity:b02:linzhou",
                        "entityId":"entity:b02:linzhou",
                        "predicate":"has_unregistered_mood",
                        "objectJson":{"mood":"烦躁"},
                        "authorityLevel":"C",
                        "evidence":evidence
                    }));
                }
                let continuity_candidates = if self.audit_unbound_thread.load(Ordering::SeqCst) {
                    vec![json!({"threadRef":null,"threadOperation":"TRANSITION",
                        "disposition":"ADVANCE","evidence":evidence})]
                } else if !self.threads.load(Ordering::SeqCst) {
                    Vec::new()
                } else if chapter_no == 1 {
                    vec![
                        json!({"disposition":"ADVANCE","description":"账本缺的那一页去了哪里","evidence":evidence,"fulfillmentObject":"缺页","promotionSignals":{"foreshadowRecoveryResponsibility":self.thread_foreshadows.load(Ordering::SeqCst)}}),
                    ]
                } else {
                    self.open_threads_seen
                        .lock()
                        .unwrap()
                        .push(input["openThreads"].clone());
                    input["openThreads"][0]["threadRef"]
                        .as_str()
                        .map(|thread| {
                            let disposition = if chapter_no == 2 { "ADVANCE" } else { "RESOLVE" };
                            vec![json!({"threadRef":thread,"disposition":disposition,"evidence":evidence})]
                        })
                        .unwrap_or_default()
                };
                let task_progress = input["chapterPlan"]["activeTaskRefs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|task| task.as_str().is_some_and(|id| id.contains(":step:")))
                    .filter(|_| self.report_stage_done.load(Ordering::SeqCst))
                    .map(|task| {
                        vec![json!({"taskId":task,"status":"SATISFIED","evidence":evidence})]
                    })
                    .unwrap_or_default();
                json!({
                    "qualityFindings":[],
                    "canonCandidates":canon_candidates,
                    "knowledgeCandidates":knowledge_candidates,
                    "continuityCandidates":continuity_candidates,
                    "taskProgressCandidates":task_progress,
                    "revealedReservationRefs":[],
                    "evidenceRanges":[evidence],
                    "auditCompleteness":"COMPLETE_WITH_DELTA",
                    "unresolvedHard":false
                })
            }
            "OPENING_SIGNING_EVALUATION" => Self::signing_evaluation(input),
            "PLAN_BOOK_DIRECTION" => {
                self.direction_inputs.lock().unwrap().push(input.clone());
                let mut direction = direction_reply(input);
                if self.table_directions {
                    direction["chapters"] = json!([{"no":4,"goal":"林舟去渡口"}]);
                }
                if self.review_opening_replay.load(Ordering::SeqCst)
                    && input.get("planFeedback").is_none()
                {
                    direction["volumeIntentSeeds"][0]["stages"][0]["steps"][0] =
                        json!("林舟辨认出雨水浸透的封条上的旧案暗记");
                }
                if self.verify_stage_done.load(Ordering::SeqCst) {
                    direction["volumeIntentSeeds"][0]["stages"][0]["steps"][0] =
                        json!("先找线人并且取得账本缺页");
                }
                direction
            }
            "CONFIRM_BOOK_CONTINUATION" => {
                if input.get("outputFeedback").is_none() {
                    json!({"issues":[]})
                } else {
                    let candidates = input["confirmCandidates"].as_array().unwrap();
                    json!({"confirmedCandidates":candidates.iter()
                        .filter(|candidate| candidate["step"] == 1 && candidate["plannedQuote"] != "先找线人")
                        .map(|candidate| json!({
                            "number":candidate["number"],
                            "plannedOutcomeQuote":candidate["plannedOutcome"],
                            "openingOutcomeQuote":candidate["openingQuote"],
                        })).collect::<Vec<_>>()})
                }
            }
            "CHECK_BOOK_CONTINUATION" => {
                let planned = &input["stages"][0]["steps"][0];
                if planned == "先找线人" {
                    json!({"issues":[{
                        "stage":1,"step":1,"plannedQuote":planned,"openingChapter":1,"openingSentence":0,
                        "openingQuote":"辨认出一段旧案暗记","reason":"两段都涉及调查"
                    }]})
                } else {
                    let opening_quote = if input.get("evidenceFeedback").is_some() {
                        "辨认出一段旧案暗记"
                    } else {
                        "凭空新增一份回执"
                    };
                    let mut issues = vec![json!({
                        "stage":1,"step":1,"plannedQuote":planned,"openingChapter":1,"openingSentence":99,
                        "openingQuote":opening_quote,"reason":"开篇已经看到并认出旧案暗记"
                    })];
                    let companion_step = if input.get("evidenceFeedback").is_none() {
                        2
                    } else {
                        3
                    };
                    issues.push(json!({
                        "stage":1,"step":companion_step,"plannedQuote":input["stages"][0]["steps"][companion_step - 1],
                        "openingChapter":1,"openingSentence":0,
                        "openingQuote":"辨认出一段旧案暗记","reason":"两段都涉及调查"
                    }));
                    json!({"issues":issues})
                }
            }
            "PLAN_VOLUME_DIRECTION" => {
                self.direction_inputs.lock().unwrap().push(input.clone());
                let concurrent_change = self.while_planning_volume.lock().unwrap().take();
                if let Some(change) = concurrent_change {
                    change();
                }
                let room = input["volume"]["chapters"].as_i64().unwrap();
                let stage = |goal: &str, chapters: i64| json!({"goal":goal,"pressure":"旧秩序的人抢先烧掉账页","cast":[],"turn":"账本的主人现身","estimatedChapters":chapters,"steps":["先找线人","再查底册","最后对质"]});
                let mut direction = json!({
                    "description":"林舟离开县城，去府城追查账本的来路",
                    "stages":[
                        stage("林舟进府城找到当年的账房", room / 3),
                        stage("林舟拿到账本的底册", room / 3),
                        stage("林舟当堂对质", room - 2 * (room / 3))
                    ],
                    "climax":"林舟在府衙当堂对质"
                });
                if self.table_volume_directions.load(Ordering::SeqCst) {
                    direction["chapters"] = json!([{"no":40}]);
                }
                direction
            }
            other => return Err(format!("unexpected behavior-test capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

/// A direction whose three stages fill the first volume after the opening.
fn direction_reply(input: &Value) -> Value {
    let room = input["firstVolume"]["chaptersAfterOpening"]
        .as_i64()
        .unwrap();
    let stage = |goal: &str, chapters: i64| json!({"goal":goal,"pressure":"旧秩序的人抢先烧掉账页","cast":[],"turn":"缺页的主人现身","estimatedChapters":chapters,"steps":["先找线人","再查底册","最后对质"]});
    json!({
        "protagonistMacroDirection":"林舟从躲债的学徒走到替全城重算旧账的人",
        "mustAchieve":[{"volumeNo":1,"description":"林舟找回失踪账本的缺页"}],
        "relationshipMacroDirections":[],
        "majorMysteryGoals":[{"layer":1,"answerInVolume":1,"description":"缺页记的是谁的债"}],
        "futureReservations":[{"description":"账本的来源","notBeforeVolumeNo":2}],
        "volumeIntentSeeds":[{"volumeNo":1,"description":"追回缺页，逼出改账的人","stages":[
            stage("林舟查清缺页落在谁手里", room / 3),
            stage("林舟拿到改账的证据", room / 3),
            stage("林舟当众对账", room - 2 * (room / 3))
        ],"climax":"林舟在城门口当众对账"}]
    })
}

fn state() -> (KernelState, tempfile::TempDir, Arc<BehaviorGateway>) {
    state_with(false, false)
}

fn state_with(
    plans_directions: bool,
    table_directions: bool,
) -> (KernelState, tempfile::TempDir, Arc<BehaviorGateway>) {
    let temp = tempfile::tempdir().expect("project directory");
    let gateway = Arc::new(BehaviorGateway {
        audit_calls: AtomicUsize::new(0),
        planning_contexts: Mutex::new(Vec::new()),
        plans_directions,
        table_directions,
        review_opening_replay: AtomicBool::new(false),
        report_stage_done: AtomicBool::new(false),
        verify_stage_done: AtomicBool::new(false),
        invalid_stage_evidence: AtomicBool::new(false),
        recover_stage_evidence: AtomicBool::new(false),
        split_stage: AtomicBool::new(false),
        enforce_length: AtomicBool::new(false),
        fail_capability_once: Mutex::new(None),
        table_volume_directions: AtomicBool::new(false),
        while_planning_volume: Mutex::new(None),
        direction_inputs: Mutex::new(Vec::new()),
        blank_audit_once: AtomicBool::new(false),
        blank_feedback_once: AtomicBool::new(false),
        audit_feedback: Mutex::new(Vec::new()),
        threads: AtomicBool::new(false),
        thread_foreshadows: AtomicBool::new(false),
        select_main_event: AtomicBool::new(false),
        audit_first_event: Mutex::new(None),
        invalid_main_event: AtomicBool::new(false),
        audit_unbound_thread: AtomicBool::new(false),
        first_main_event_invalid: AtomicBool::new(false),
        open_threads_seen: Mutex::new(Vec::new()),
        inputs: Mutex::new(Vec::new()),
    });
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    (state, temp, gateway)
}

fn revision(state: &KernelState, project_id: &str) -> i64 {
    state
        .manager
        .project_revision(project_id)
        .expect("project revision")
}

fn install_opening_fixture_gate_rules(state: &KernelState, project_id: &str, prefix: &str) {
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    let has_policy = ai_novel_production_system_core::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
        &connection,
        project_id,
        "GatePolicy",
    )
    .unwrap()
    .is_some();
    drop(connection);
    if !has_policy {
        command(
            state,
            project_id,
            &format!("{prefix}:test-gate-policy-default"),
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
            &format!("{prefix}:test-gate-rule:{index}"),
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

fn create_story_setup_seed(state: &KernelState, prefix: &str) -> String {
    let created = execute_command_impl(
        state,
        json!({
            "type":"project_create_from_idea",
            "commandId":format!("{prefix}:create"),
            "idempotencyKey":format!("{prefix}:create:key"),
            "requestedAt":NOW,
            "payload":{
                "title":"B2 开篇 Canon 行为测试",
                "idea":"账房学徒发现失踪账本会改写城中所有人的债务。",
                "platform":"起点中文网",
                "mode":"DIRECTOR",
                "tags":["悬疑","成长"],
                "forbidden":"前三章不揭开账本来源"
            }
        }),
    )
    .expect("create project");
    let project_id = created["projectId"].as_str().unwrap().to_owned();
    install_opening_fixture_gate_rules(state, &project_id, prefix);
    command(
        state,
        &project_id,
        &format!("{prefix}:directions"),
        "story_directions_generate",
        json!({}),
    );
    let directions = ai_novel_production_system_core::commands::query_story_directions_impl(
        state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":10}),
    )
    .expect("story directions");
    let pitch_id = directions["pitches"][0]["id"].as_str().unwrap();
    command(
        state,
        &project_id,
        &format!("{prefix}:select"),
        "pitch_select",
        json!({"pitchIds":[pitch_id]}),
    );
    command(
        state,
        &project_id,
        &format!("{prefix}:setup"),
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
    project_id
}

fn create_signed_opening(state: &KernelState) -> String {
    let project_id = create_story_setup_seed(state, "b2-red");
    let (_, _, mut legacy_seed) = current_book_plan_payload(state, &project_id);
    legacy_seed["createdAt"] = json!(NOW);
    commit_book_plan_fixture(state, &project_id, legacy_seed);
    command(
        state,
        &project_id,
        "b2-red:ready",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        state,
        &project_id,
        "b2-red:start",
        "opening_start_creating",
        json!({}),
    );

    for chapter_no in
        1..=ai_novel_production_system_core::repositories::project_entry::OPENING_CHAPTER_COUNT
    {
        let center = query_production_center_impl(
            state,
            json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
        )
        .expect("production center");
        assert_eq!(center["nextChapter"]["number"], chapter_no);
        command(
            state,
            &project_id,
            &format!("b2-red:production:{chapter_no}"),
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

    let evaluated = command(
        state,
        &project_id,
        "b2-red:evaluate",
        "opening_evaluate",
        json!({}),
    );
    assert_eq!(evaluated["result"]["status"], "PASS");
    let center = query_production_center_impl(
        state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("signing evaluation projection");
    let platform_id = center["signing"]["platformMatrix"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|entry| entry["mainstream"] == true && entry["status"] == "PASS")
        .and_then(|entry| entry["platformId"].as_str())
        .expect("a passing mainstream platform");
    command(
        state,
        &project_id,
        "b2-red:signing-confirm",
        "signing_confirm",
        json!({"platformId":platform_id}),
    );
    project_id
}

fn create_approved_book_plan(state: &KernelState) -> (String, Value) {
    let project_id = create_signed_opening(state);
    let approved = command(
        state,
        &project_id,
        "b2-red:approve",
        "book_plan_revise",
        json!({}),
    );
    assert_eq!(approved["result"]["planningMaturity"], "APPROVED");
    (project_id, approved["result"].clone())
}

fn current_book_plan_payload(state: &KernelState, project_id: &str) -> (String, i64, Value) {
    let connection = state
        .manager
        .open(project_id, "", NOW)
        .expect("open project db");
    let (artifact_id, version, raw): (String, i64, String) = connection
        .query_row(
            "SELECT a.artifact_id,a.version,p.payload_json FROM canonical_heads h JOIN artifacts a ON a.artifact_id=h.artifact_id AND a.version=h.version JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE h.project_id=?1 AND h.artifact_type='BookPlan'",
            [project_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("BookPlan head");
    let payload = serde_json::from_str(&raw).expect("BookPlan JSON");
    (artifact_id, version, payload)
}

fn commit_book_plan_fixture(state: &KernelState, project_id: &str, payload: Value) {
    let (current_artifact_id, current_version, _) = current_book_plan_payload(state, project_id);
    let connection = state
        .manager
        .open(project_id, "", NOW)
        .expect("open project db");
    let dependencies = {
        let mut statement = connection
            .prepare("SELECT target_artifact_id,target_version,target_payload_hash FROM artifact_dependencies WHERE source_artifact_id=?1 AND source_version=?2 ORDER BY target_artifact_id")
            .expect("prepare BookPlan dependencies");
        statement
            .query_map(
                libsql_rusqlite::params![current_artifact_id, current_version],
                |row| {
                    Ok(json!({
                        "artifactId":row.get::<_, String>(0)?,
                        "version":row.get::<_, i64>(1)?,
                        "payloadHash":row.get::<_, String>(2)?
                    }))
                },
            )
            .expect("query BookPlan dependencies")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect BookPlan dependencies")
    };
    let revision = state
        .manager
        .project_revision(project_id)
        .expect("project revision");
    let command_id = format!("rv03:legacy-book-plan:{project_id}:{revision}");
    let artifact_id = format!("book-plan:rv03-legacy:{project_id}:{revision}");
    let payload_hash = sha256_canonical(&payload).expect("legacy payload hash");
    let bundle: ai_novel_production_system_core::generated::types::CommitBundle =
        serde_json::from_value(json!({
            "commitId":format!("commit:{command_id}"),
            "projectId":project_id,
            "commandId":command_id,
            "idempotencyKey":format!("{command_id}:key"),
            "expectedProjectRevision":revision,
            "contractVersion":"m0a.v1",
            "createdAt":NOW,
            "artifacts":[{
                "artifactId":artifact_id,
                "projectId":project_id,
                "artifactType":"BookPlan",
                "scope":{"level":"PROJECT"},
                "version":1,
                "schemaVersion":"m0a.artifact.v1",
                "artifactStatus":"CANDIDATE",
                "payloadHash":payload_hash,
                "dependencyRefs":dependencies,
                "provenance":{"createdBy":"rv03-legacy-fixture","commandId":command_id},
                "payload":payload,
                "createdAt":NOW
            }],
            "headUpdates":[{
                "artifactType":"BookPlan",
                "scope":{"level":"PROJECT"},
                "artifactId":artifact_id,
                "version":1,
                "payloadHash":payload_hash
            }]
        }))
        .expect("legacy BookPlan CommitBundle");
    CommitService::commit(&connection, &bundle, NOW).expect("commit historical BookPlan fixture");
}

fn opening_context_manifest(state: &KernelState, project_id: &str) -> Result<Value, String> {
    let opening = ai_novel_production_system_core::commands::query_opening_ready_impl(
        state,
        json!({"request":{"projectId":project_id,"scope":"PROJECT","pageSize":10}}),
    )?;
    query_context_manifest_impl(
        state,
        json!({
            "projectId":project_id,
            "volumeId":opening["firstChapter"]["volumeId"],
            "chapterId":opening["firstChapter"]["chapterId"],
            "scope":"CHAPTER",
            "executionMode":"OPENING_BOOTSTRAP",
            "entityIds":[],
            "references":[]
        }),
    )
}

#[test]
fn rv03_legacy_book_plan_created_at_is_ignored_and_recorded_during_hard_compilation() {
    let (state, _temp, _gateway) = state();
    let project_id = create_story_setup_seed(&state, "rv03-legacy");
    let (_, _, mut legacy_seed) = current_book_plan_payload(&state, &project_id);
    legacy_seed["createdAt"] = json!(NOW);
    commit_book_plan_fixture(&state, &project_id, legacy_seed);

    command(
        &state,
        &project_id,
        "rv03-legacy:ready",
        "opening_ready",
        json!({"approved":true}),
    );
    let manifest = opening_context_manifest(&state, &project_id)
        .expect("legacy BookPlan should compile into opening HARD context");
    assert!(manifest["hardSourceCount"].as_i64().unwrap() > 0);
    assert!(manifest["omittedSoftSources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| {
            source["sourceRef"]["ref"]
                .as_str()
                .is_some_and(|reference| reference.contains("legacyFieldsIgnored"))
                && source["reason"] == "legacyFieldsIgnored:[\"createdAt\"]"
        }));
}

#[test]
fn rv03_book_plan_unknown_payload_field_is_still_rejected() {
    let (state, _temp, _gateway) = state();
    let project_id = create_story_setup_seed(&state, "rv03-unknown");
    let (_, _, mut seed) = current_book_plan_payload(&state, &project_id);
    seed["foo"] = json!("must remain invalid");
    commit_book_plan_fixture(&state, &project_id, seed);

    let command_id = "rv03-unknown:ready";
    let revision_before = revision(&state, &project_id);
    let error = execute_command_impl(
        &state,
        json!({
            "type":"opening_ready",
            "commandId":command_id,
            "idempotencyKey":format!("{command_id}:key"),
            "projectId":project_id,
            "expectedRevision":revision(&state, &project_id),
            "requestedAt":NOW,
            "payload":{"approved":true}
        }),
    )
    .expect_err("unknown BookPlan payload field must fail closed");
    assert!(
        error.starts_with("GOAL_CONTRACT_BOOK_PLAN_INVALID|"),
        "{error}"
    );
    assert!(error.contains("foo"), "{error}");
    assert_eq!(revision(&state, &project_id), revision_before);
}

#[test]
fn rv03_story_setup_seed_does_not_put_created_at_inside_payload() {
    let (state, _temp, _gateway) = state();
    let project_id = create_story_setup_seed(&state, "rv03-seed");
    let (_, _, seed) = current_book_plan_payload(&state, &project_id);

    assert!(seed.get("createdAt").is_none(), "{seed}");
    ai_novel_production_system_core::generated::validators::validate_book_plan_r1(&seed)
        .expect("new SEED should satisfy the frozen strict schema directly");
}

fn opening_item<'a>(book: &'a Value, field: &str, key: &str, prefix: &str) -> &'a Value {
    book[field]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item[key].as_str().is_some_and(|id| id.starts_with(prefix)))
        .unwrap_or_else(|| panic!("missing {prefix} item in {field}"))
}

#[test]
fn rv30_opening_contract_descriptions_reference_canon_without_copying_it() {
    let (state, _temp, _gateway) = state();
    let (_project_id, book) = create_approved_book_plan(&state);
    let quote_strings = (1
        ..=ai_novel_production_system_core::repositories::project_entry::OPENING_CHAPTER_COUNT
            as usize)
        .map(BehaviorGateway::quote)
        .collect::<Vec<_>>();
    let opening_items = [
        opening_item(&book, "mustAchieve", "id", "goal:opening-reality:"),
        opening_item(&book, "mustNotBreak", "id", "rule:opening-reality:"),
        opening_item(
            &book,
            "relationshipMacroDirections",
            "id",
            "relationship:opening",
        ),
        opening_item(&book, "majorMysteryGoals", "id", "mystery:opening-reality:"),
        opening_item(
            &book,
            "futureReservations",
            "reservationId",
            "reservation:opening-reality:",
        ),
        opening_item(
            &book,
            "volumeIntentSeeds",
            "seedId",
            "volume-intent:opening-reality:",
        ),
    ];

    for item in opening_items {
        let description = item["description"].as_str().unwrap();
        for canon_excerpt in ["账本缺页", "互不信任", "distrusts", "账本被顾清夺走"]
        {
            assert!(
                !description.contains(canon_excerpt),
                "opening-reality description copied Canon value {canon_excerpt:?}: {description}"
            );
        }
        for quote in &quote_strings {
            assert!(
                !description.contains(quote),
                "opening-reality description copied evidence quote: {description}"
            );
        }
    }

    let must_not_break = opening_items[1];
    assert!(must_not_break["kind"] == "CANON_REFS" || must_not_break["refs"].is_array());
}

#[test]
fn rv30_long_form_plan_input_keeps_canon_once_and_omits_evidence_quotes() {
    let (state, _temp, gateway) = state();
    let (project_id, _book) = create_approved_book_plan(&state);
    let volumes = query_volume_list_impl(&state, json!({"projectId":project_id,"pageSize":20}))
        .expect("volume list after approval");
    let volume_id = volumes["items"][0]["id"].as_str().expect("volume id");
    command(
        &state,
        &project_id,
        "b2-red:volume-intent",
        "planning_create_volume_intent",
        json!({"volumeId":volume_id}),
    );
    command(
        &state,
        &project_id,
        "b2-red:volume-plan",
        "planning_create_volume_plan",
        json!({"volumeId":volume_id}),
    );
    let prepared = command(
        &state,
        &project_id,
        "b2-red:chapter-four-prepare",
        "planning_prepare_next_chapter",
        json!({"volumeId":volume_id}),
    );
    let chapter_id = prepared["chapter"]["chapterId"].as_str().unwrap();
    let route = query_plan_route_impl(&state, json!({"projectId":project_id,"volumeId":volume_id}))
        .expect("volume route");
    let task_refs = route["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|task| task["class"] == "REQUIRED")
        .filter_map(|task| task["taskId"].as_str())
        .take(1)
        .collect::<Vec<_>>();
    command(
        &state,
        &project_id,
        "b2-red:chapter-four-plan",
        "planning_compile_chapter_plan",
        json!({
            "chapterId":chapter_id,
            "chapterGoalHint":"根据前三章正式事实追查账本缺页的去向。",
            "scopeSelection":{
                "taskRefs":task_refs,
                "responsibilityRefs":[],
                "reservationRefs":[],
                "entityRefs":["entity:b02:linzhou","entity:b02:guqing"]
            }
        }),
    );

    let contexts = gateway.planning_contexts.lock().unwrap();
    let hard_input = contexts
        .iter()
        .find(|context| context["executionMode"] == "LONG_FORM")
        .expect("LONG_FORM Plan input");
    let facts = hard_input["state"]["facts"].as_array().unwrap();
    assert!(facts
        .iter()
        .any(|fact| fact.to_string().contains("账本缺页")));
    let opening_fields = hard_input["goalContractFields"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|field| {
            field["fieldPath"]
                .as_str()
                .is_some_and(|path| path.contains("opening-reality"))
        })
        .collect::<Vec<_>>();
    for field in &opening_fields {
        let serialized = field["value"].to_string();
        assert!(serialized.contains("refs"));
        assert!(!serialized.contains("账本缺页"));
        assert!(!serialized.contains("互不信任"));
        assert!(!serialized.contains("distrusts"));
    }
    let serialized_hard_input = hard_input.to_string();
    for chapter_no in 1
        ..=ai_novel_production_system_core::repositories::project_entry::OPENING_CHAPTER_COUNT
            as usize
    {
        assert!(
            !serialized_hard_input.contains(&BehaviorGateway::quote(chapter_no)),
            "HARD Plan input contains raw evidence quote from opening chapter {chapter_no}"
        );
    }
}

#[test]
fn vd1a_a_direction_planned_after_signing_fills_the_approved_book_plan() {
    let (state, _temp, gateway) = state_with(true, false);
    let (project_id, approved) = create_approved_book_plan(&state);
    let inputs = gateway.direction_inputs.lock().unwrap().clone();
    assert_eq!(inputs.len(), 1);
    assert!(
        inputs[0]["firstVolume"]["chaptersAfterOpening"]
            .as_i64()
            .unwrap()
            > 0
    );
    assert!(!inputs[0]["signing"]["verdict"].as_str().unwrap().is_empty());
    let opening = inputs[0]["openingCommittedProse"].as_array().unwrap();
    assert_eq!(opening.len(), 3);
    for (index, chapter) in opening.iter().enumerate() {
        let number = index + 1;
        assert_eq!(chapter["chapterNo"], number);
        assert!(chapter["body"]
            .as_str()
            .unwrap()
            .contains(&BehaviorGateway::quote(number)));
    }
    assert_eq!(
        approved["protagonistMacroDirection"],
        "林舟从躲债的学徒走到替全城重算旧账的人"
    );
    assert_eq!(
        approved["mustAchieve"],
        json!([{"id":"goal:book:1","volumeNo":1,"description":"林舟找回失踪账本的缺页"}])
    );
    let seed = &approved["volumeIntentSeeds"][0];
    assert_eq!(seed["seedId"], "volume-1");
    assert_eq!(seed["stages"].as_array().unwrap().len(), 3);
    assert_eq!(seed["stages"][2]["goal"], "林舟当众对账");
    assert_eq!(approved["futureReservations"][0]["mayReveal"], false);
    // The opening's facts still bind the plan as a rule, not as a goal.
    assert!(approved["mustNotBreak"]
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| rule["description"] == "开篇已提交事实不得违背；以 Canon 为准。"));
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let runs: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM model_runs WHERE capability='PLAN_BOOK_DIRECTION'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(runs, 1);
    drop(connection);
    // VD-1b: the volume route follows the stages.
    let volumes = query_volume_list_impl(&state, json!({"projectId":project_id,"pageSize":20}))
        .expect("volume list after approval");
    let volume_id = volumes["items"][0]["id"].as_str().expect("volume id");
    for (step, command_type) in [
        ("volume-intent", "planning_create_volume_intent"),
        ("volume-plan", "planning_create_volume_plan"),
    ] {
        command(
            &state,
            &project_id,
            &format!("vd1a:{step}"),
            command_type,
            json!({"volumeId":volume_id}),
        );
    }
    let route = query_plan_route_impl(&state, json!({"projectId":project_id,"volumeId":volume_id}))
        .expect("volume route");
    let required = route["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|task| task["class"] == "REQUIRED")
        .map(|task| task["objective"].as_str().unwrap())
        .collect::<Vec<_>>();
    // VD-1h: one task per step, three steps a stage, then the book goal.
    assert_eq!(required.len(), 10, "{required:?}");
    // Real run round 25: steps are route tasks only; the volume's goals,
    // part of every chapter's hard context, are its stages and book goal.
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let intent_goals: String = connection
        .query_row(
            "SELECT json_extract(p.payload_json,'$.mustAchieve') FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='VolumeIntent'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);
    let intent_goals: Value = serde_json::from_str(&intent_goals).unwrap();
    assert_eq!(intent_goals.as_array().unwrap().len(), 4, "{intent_goals}");
    assert_eq!(required[..3], ["先找线人", "再查底册", "最后对质"]);
    assert_eq!(required[9], "林舟找回失踪账本的缺页");
    // VD-1c: chapter 4 is planned at the start of the first stage.
    let prepared = command(
        &state,
        &project_id,
        "vd1c:chapter-four-prepare",
        "planning_prepare_next_chapter",
        json!({"volumeId":volume_id}),
    );
    let chapter_id = prepared["chapter"]["chapterId"].as_str().unwrap();
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    assert_eq!(center["nextChapter"]["executionMode"], "LONG_FORM");
    assert_eq!(center["nextChapter"]["planReady"], false);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let direction = ai_novel_production_system_core::book_direction::for_chapter(
        &connection,
        &project_id,
        chapter_id,
        4,
    )
    .unwrap()
    .expect("a direction for chapter 4");
    // VD-1h: the planner sees the first step, not the stage's end.
    assert_eq!(direction["currentStage"]["goal"], "先找线人");
    assert_eq!(direction["currentStage"]["step"], 1);
    assert_eq!(direction["currentStage"]["stepCount"], 3);
    assert_eq!(direction["currentStage"]["chaptersSoFar"], 0);
    assert!(direction["currentStage"]["taskId"]
        .as_str()
        .unwrap()
        .starts_with("task:required:goal:"));
    assert_eq!(direction["climax"], "林舟在城门口当众对账");
}

#[test]
fn a_direction_that_replays_the_opening_is_replanned_before_approval() {
    let (state, _temp, gateway) = state_with(true, false);
    gateway.review_opening_replay.store(true, Ordering::SeqCst);
    let (_, approved) = create_approved_book_plan(&state);
    assert_eq!(
        approved["volumeIntentSeeds"][0]["stages"][0]["steps"][0]["goal"],
        "先找线人"
    );
    let calls = gateway.direction_inputs.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls[1]["planFeedback"]
        .to_string()
        .contains("辨认出一段旧案暗记"));
    assert!(gateway
        .inputs
        .lock()
        .unwrap()
        .iter()
        .any(|(capability, input)| {
            capability == "CHECK_BOOK_CONTINUATION"
                && input["evidenceFeedback"]["previousIssues"][0]["openingQuote"]
                    == "凭空新增一份回执"
                && input["opening"][0]["sentences"][0]["number"] == 0
        }));
    assert!(gateway
        .inputs
        .lock()
        .unwrap()
        .iter()
        .any(|(capability, input)| {
            capability == "CONFIRM_BOOK_CONTINUATION"
                && input["outputFeedback"]["rejectedFor"]
                    .as_str()
                    .is_some_and(|reason| reason.contains("P01_MODEL_OUTPUT_SPEC"))
                && input["confirmCandidates"]
                    .as_array()
                    .is_some_and(|candidates| {
                        candidates.len() == 2
                            && candidates.iter().any(|candidate| candidate["step"] == 1)
                            && candidates.iter().any(|candidate| candidate["step"] == 2)
                    })
        }));
}

#[test]
fn a_stage_step_audit_claim_without_verified_outcome_stays_active() {
    check_stage_step_completion(true, false, false);
}

#[test]
fn a_stage_step_claim_binds_the_successful_verification_run_after_shape_retry() {
    check_stage_step_completion(true, true, false);
}

#[test]
fn a_recovered_step_credits_the_source_chapter_without_rewriting_or_claiming_it_again() {
    use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
    let (state, _temp, gateway) = state_with(true, false);
    let (project_id, _) = create_approved_book_plan(&state);
    gateway.select_main_event.store(true, Ordering::SeqCst);
    let volumes =
        query_volume_list_impl(&state, json!({"projectId":project_id,"pageSize":20})).unwrap();
    let volume = volumes["items"][0]["id"].as_str().unwrap();
    let first = prepare_stage_chapter(&state, &project_id, volume);
    let source_id = first["chapter"]["chapterId"].as_str().unwrap();
    let produce = |command_id: &str, chapter: &str| {
        command(
            &state,
            &project_id,
            command_id,
            "production_start",
            json!({"chapterId":chapter,"volumeId":volume,"executionMode":"LONG_FORM"}),
        )
    };
    let source = produce("stage-recovery:four", source_id);
    gateway.recover_stage_evidence.store(true, Ordering::SeqCst);
    let next = command(
        &state,
        &project_id,
        "stage-recovery:prepare",
        "planning_prepare_next_chapter",
        json!({"volumeId":volume}),
    );
    let current_id = next["chapter"]["chapterId"].as_str().unwrap();
    command(
        &state,
        &project_id,
        "stage-recovery:plan",
        "planning_compile_chapter_plan",
        json!({"chapterId":current_id}),
    );
    produce("stage-recovery:five", current_id);

    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let completed =
        NarrativeRepository::satisfied_task_chapters(&connection, &project_id, 6).unwrap();
    let (task, source_number) = completed
        .iter()
        .find(|(task, _)| task.ends_with(":step:1:1:1"))
        .unwrap();
    assert_eq!(*source_number, 4);
    let progress =
        NarrativeRepository::task_states_by_ids_complete(&connection, &project_id, &[task])
            .unwrap()
            .pop()
            .unwrap();
    assert_eq!(
        progress.value["evidenceRefs"][0]["artifactId"],
        source["finalChapterRef"]["artifactId"]
    );
    let source_head =
        NarrativeRepository::committed_final_chapter_head(&connection, &project_id, source_id)
            .unwrap()
            .unwrap();
    assert_eq!(
        serde_json::to_value(source_head).unwrap(),
        source["finalChapterRef"]
    );
    let review: String = connection.query_row(
        "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads r USING(artifact_id,version)
         JOIN artifact_payloads p ON p.artifact_id=json_extract(r.payload_json,'$.auditRef.artifactId')
           AND p.version=json_extract(r.payload_json,'$.auditRef.version')
         WHERE h.project_id=?1 AND h.artifact_type='ProductionReceipt'
           AND json_extract(h.scope_key,'$.chapterId')=?2",
        [project_id.as_str(),current_id], |row| row.get(0),
    ).unwrap();
    let audit: Value = serde_json::from_str(&review).unwrap();
    assert_eq!(
        audit["audit"]["normalization"]["stageStepVerification"]["currentChapterEvidence"],
        false
    );
    assert!(audit["audit"]["taskProgressCandidates"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["taskId"] != *task));
    drop(connection);
    replace_split_step_source(&state, &project_id, source_id);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let reopened =
        NarrativeRepository::task_states_by_ids_complete(&connection, &project_id, &[task])
            .unwrap()
            .pop()
            .unwrap();
    assert_eq!(reopened.value["status"], "ACTIVE");
    assert_eq!(reopened.value["evidenceRefs"], json!([]));
}

#[test]
fn a_verified_stage_step_omitted_by_audit_advances_the_next_chapter() {
    check_stage_step_completion(false, true, false);
}

#[test]
fn round62_two_invalid_stage_evidence_replies_leave_the_step_open_without_discarding_the_chapter() {
    check_stage_step_completion(true, false, true);
}

#[test]
fn a_split_stage_step_keeps_partial_evidence_and_completes_from_two_official_chapters() {
    check_split_stage_step(true, false);
}

#[test]
fn a_replaced_final_chapter_cannot_supply_stage_step_evidence() {
    check_split_stage_step(false, false);
}

#[test]
fn replacing_a_closed_step_source_reopens_just_the_missing_outcome() {
    check_split_stage_step(true, true);
}

fn replace_split_step_source(state: &KernelState, project_id: &str, first_id: &str) {
    use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    let previous =
        NarrativeRepository::committed_final_chapter_head(&connection, project_id, first_id)
            .unwrap()
            .unwrap();
    let mut payload =
        NarrativeRepository::artifact_payload_by_ref(&connection, project_id, &previous)
            .unwrap()
            .unwrap();
    let corrected = payload["body"].as_str().unwrap().replacen(
        "林舟在档案室找到了线人周娘子。",
        "林舟没能找到线人周娘子。",
        1,
    );
    payload["bodyDigest"] = json!(sha256_canonical(&json!(corrected)).unwrap());
    payload["wordCount"] = json!(corrected.chars().count());
    payload["body"] = json!(corrected);
    payload["validatedEvidence"] = json!([]);
    let payload_hash = sha256_canonical(&payload).unwrap();
    let scope = json!({"level":"CHAPTER","chapterId":first_id});
    let version = previous.version + 1;
    let gate =
        NarrativeRepository::canonical_project_head_ref(&connection, project_id, "GatePolicy")
            .unwrap()
            .unwrap();
    let subject =
        json!({"artifactId":previous.artifact_id,"version":version,"payloadHash":payload_hash});
    let digest =
        sha256_canonical(&json!({"gatePolicyRef":gate,"subjectRef":subject,"evidenceRefs":[]}))
            .unwrap();
    let mut certificate = json!({"certificateId":"cert:split-step:replacement","projectId":project_id,
        "gatePolicyRef":gate,"subjectRef":subject,"evidenceRefs":[],"sourceDigest":digest,
        "passedAt":NOW,"certificateDigest":""});
    certificate["certificateDigest"] = json!(sha256_canonical(&certificate).unwrap());
    let bundle = serde_json::from_value(json!({
        "commitId":"commit:split-step:replacement","projectId":project_id,"commandId":"split-step:replacement",
        "idempotencyKey":"split-step:replacement:key","contractVersion":"m0a.v1","createdAt":NOW,
        "expectedProjectRevision":NarrativeRepository::project_revision(&connection, project_id).unwrap(),
        "artifacts":[{
            "artifactId":previous.artifact_id,"projectId":project_id,"artifactType":"FinalChapter","scope":scope,
            "version":version,"schemaVersion":"m0a.artifact.v1","artifactStatus":"CANDIDATE",
            "payload":payload,"payloadHash":payload_hash,"dependencyRefs":[previous],
            "provenance":{"createdBy":"test","commandId":"split-step:replacement"},"createdAt":NOW
        }],
        "gateCertificates":[certificate],
        "headUpdates":[{
            "artifactType":"FinalChapter","scope":scope,"artifactId":previous.artifact_id,
            "version":version,"payloadHash":payload_hash
        }]
    })).unwrap();
    CommitService::commit(&connection, &bundle, NOW).unwrap();
}

fn check_split_stage_step(keep_first_version: bool, replace_after_completion: bool) {
    use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
    let (state, _temp, gateway) = state_with(true, false);
    gateway.verify_stage_done.store(true, Ordering::SeqCst);
    gateway.split_stage.store(true, Ordering::SeqCst);
    let (project_id, _) = create_approved_book_plan(&state);
    gateway.select_main_event.store(true, Ordering::SeqCst);
    let volumes =
        query_volume_list_impl(&state, json!({"projectId":project_id,"pageSize":20})).unwrap();
    let volume_id = volumes["items"][0]["id"].as_str().unwrap();
    let prepared = prepare_stage_chapter(&state, &project_id, volume_id);
    let first_id = prepared["chapter"]["chapterId"].as_str().unwrap();
    let produce = |command_id: &str, chapter_id: &str| {
        command(
            &state,
            &project_id,
            command_id,
            "production_start",
            json!({"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM"}),
        )
    };
    let first = produce("split-step:first", first_id);
    if !keep_first_version {
        replace_split_step_source(&state, &project_id, first_id);
    }
    let next = command(
        &state,
        &project_id,
        "split-step:prepare-next",
        "planning_prepare_next_chapter",
        json!({"volumeId":volume_id}),
    );
    let second_id = next["chapter"]["chapterId"].as_str().unwrap();
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let direction = ai_novel_production_system_core::book_direction::for_chapter(
        &connection,
        &project_id,
        second_id,
        5,
    )
    .unwrap()
    .unwrap();
    let task_id = direction["currentStage"]["taskId"].as_str().unwrap();
    assert!(task_id.ends_with(":step:1:1:1"));
    let partial =
        NarrativeRepository::task_states_by_ids_complete(&connection, &project_id, &[task_id])
            .unwrap()
            .pop()
            .unwrap();
    assert_eq!(partial.value["status"], "ACTIVE");
    if keep_first_version {
        assert_eq!(
            partial.value["evidenceRefs"][0]["artifactId"],
            first["finalChapterRef"]["artifactId"]
        );
    } else {
        assert_eq!(partial.value["evidenceRefs"], json!([]));
    }
    let expected_progress = if keep_first_version {
        json!(["取得账本缺页"])
    } else {
        Value::Null
    };
    assert_eq!(
        direction["currentStage"]["progress"]["remainingObjectives"],
        expected_progress
    );
    if keep_first_version {
        assert_eq!(
            direction["currentStage"]["recentUnverifiedAttempts"][0]["chapterNo"],
            4
        );
    } else {
        assert!(direction["currentStage"]["recentUnverifiedAttempts"].is_null());
    }
    drop(connection);
    command(
        &state,
        &project_id,
        "split-step:plan-next",
        "planning_compile_chapter_plan",
        json!({"chapterId":second_id}),
    );
    let second = produce("split-step:second", second_id);
    if replace_after_completion {
        replace_split_step_source(&state, &project_id, first_id);
    }
    let still_complete = keep_first_version && !replace_after_completion;
    let next = command(
        &state,
        &project_id,
        "split-step:prepare-third",
        "planning_prepare_next_chapter",
        json!({"volumeId":volume_id}),
    );
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let completed =
        NarrativeRepository::task_states_by_ids_complete(&connection, &project_id, &[task_id])
            .unwrap()
            .pop()
            .unwrap();
    assert_eq!(
        completed.value["status"],
        if still_complete {
            "SATISFIED"
        } else {
            "ACTIVE"
        }
    );
    let evidence = completed.value["evidenceRefs"].as_array().unwrap();
    assert_eq!(evidence.len(), if still_complete { 2 } else { 1 });
    let expected_refs = if still_complete {
        vec![&first["finalChapterRef"], &second["finalChapterRef"]]
    } else {
        vec![&second["finalChapterRef"]]
    };
    for final_ref in expected_refs {
        assert!(evidence
            .iter()
            .any(|item| item["artifactId"] == final_ref["artifactId"]
                && item["version"] == final_ref["version"]));
    }
    let inherited_edge: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM dependency_edges WHERE project_id=?1 AND source_ref=?2
         AND target_ref=?3 AND source_hash=?4 AND dependency_type='STATE_INPUT' AND impact_class='HARD')",
        libsql_rusqlite::params![project_id, first["finalChapterRef"]["artifactId"].as_str(),
            second["finalChapterRef"]["artifactId"].as_str(), first["finalChapterRef"]["payloadHash"].as_str()],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(inherited_edge, keep_first_version);
    let direction = ai_novel_production_system_core::book_direction::for_chapter(
        &connection,
        &project_id,
        next["chapter"]["chapterId"].as_str().unwrap(),
        6,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        direction["currentStage"]["goal"],
        if still_complete {
            "再查底册"
        } else {
            "先找线人并且取得账本缺页"
        }
    );
    if replace_after_completion {
        assert_eq!(
            direction["currentStage"]["progress"]["remainingObjectives"],
            json!(["先找线人"])
        );
        drop(connection);
        let lifecycle = ai_novel_production_system_core::commands::query_volume_lifecycle_impl(
            &state,
            json!({"projectId":project_id,"volumeId":volume_id}),
        )
        .unwrap();
        assert!(lifecycle["planningState"]["remainingTaskIds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == task_id));
        let connection = state.manager.open(&project_id, "", NOW).unwrap();
        let projection = ai_novel_production_system_core::volume_lifecycle::VolumeLifecycleOwner::rebuild_planning_state(
            &connection, &project_id, volume_id, NOW,
        ).unwrap();
        assert!(projection["remainingTaskIds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == task_id));
        drop(connection);
        let third_id = next["chapter"]["chapterId"].as_str().unwrap();
        command(
            &state,
            &project_id,
            "split-step:replan-after-replacement",
            "planning_compile_chapter_plan",
            json!({"chapterId":third_id}),
        );
        let connection = state.manager.open(&project_id, "", NOW).unwrap();
        let plan = NarrativeRepository::artifact_payload(
            &connection,
            &project_id,
            "ChapterPlan",
            third_id,
        )
        .unwrap()
        .unwrap();
        let tasks = plan["activeTaskRefs"].as_array().unwrap();
        assert!(tasks.iter().any(|value| value == task_id));
        assert!(tasks
            .iter()
            .all(|value| !value.as_str().unwrap_or_default().ends_with(":step:1:1:2")));
    }
    let inputs = gateway.inputs.lock().unwrap();
    for capability in ["RENDER_CHAPTER", "VERIFY_STAGE_STEP"] {
        let (_, input) = inputs
            .iter()
            .find(|(name, input)| name == capability && input["chapterId"] == second_id)
            .unwrap();
        assert_eq!(
            input["stageStep"]["progress"]["remainingObjectives"],
            expected_progress
        );
    }
    let (_, plan) = inputs
        .iter()
        .find(|(name, input)| name == "PLAN_STRATEGY" && input["chapterId"] == second_id)
        .unwrap();
    assert_eq!(
        plan["volumeDirection"]["currentStage"]["progress"]["remainingObjectives"],
        expected_progress
    );
}

#[test]
fn audit_retry_reuses_the_sized_long_form_prose() {
    check_sized_prose_retry(false, "AUDIT_EXTRACT");
}

#[test]
fn audit_retry_writes_again_after_the_chapter_plan_changes() {
    check_sized_prose_retry(true, "AUDIT_EXTRACT");
}

#[test]
fn a_stage_verifier_failure_can_retry_without_publishing_or_rewriting() {
    check_sized_prose_retry(false, "VERIFY_STAGE_STEP");
}

fn check_sized_prose_retry(recompile_plan: bool, fail_capability: &'static str) {
    let (state, _temp, gateway) = state_with(true, false);
    let (project_id, _) = create_approved_book_plan(&state);
    let volumes =
        query_volume_list_impl(&state, json!({"projectId":project_id,"pageSize":20})).unwrap();
    let volume_id = volumes["items"][0]["id"].as_str().unwrap();
    let chapter = prepare_stage_chapter(&state, &project_id, volume_id);
    let chapter_id = chapter["chapter"]["chapterId"].as_str().unwrap();
    gateway.enforce_length.store(true, Ordering::SeqCst);
    gateway
        .select_main_event
        .store(fail_capability == "VERIFY_STAGE_STEP", Ordering::SeqCst);
    *gateway.fail_capability_once.lock().unwrap() = Some(fail_capability);
    let execute = |id: &str, retry_of_job_id: Value| {
        execute_command_impl(
            &state,
            json!({
                "type":"production_start","commandId":id,"idempotencyKey":format!("{id}:key"),
                "projectId":project_id,"expectedRevision":revision(&state, &project_id),"requestedAt":NOW,
                "payload":{"executionMode":"LONG_FORM","chapterId":chapter_id,"volumeId":volume_id,
                    "retryOfJobId":retry_of_job_id}
            }),
        )
    };
    let first_error = execute("step-review:produce", Value::Null).unwrap_err();
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let previous_job_id: String = connection.query_row(
        "SELECT job_id FROM jobs WHERE chapter_id=?1 AND status='FAILED' AND current_stage='AUDIT'",
        [chapter_id], |row| row.get(0),
    ).unwrap_or_else(|error| panic!("expected audit failure, got {first_error}; {error}"));
    let published: Option<String> = connection
        .query_row(
            "SELECT final_artifact_id FROM chapters WHERE project_id=?1 AND chapter_id=?2",
            libsql_rusqlite::params![project_id, chapter_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(published, None);
    drop(connection);
    let render_count = || {
        gateway
            .inputs
            .lock()
            .unwrap()
            .iter()
            .filter(|(capability, input)| {
                capability == "RENDER_CHAPTER" && input["chapterId"] == chapter_id
            })
            .count()
    };
    let first_render_count = render_count();
    assert!(first_render_count > 0);
    if recompile_plan {
        command(
            &state,
            &project_id,
            "step-review:replan",
            "planning_compile_chapter_plan",
            json!({"chapterId":chapter_id,"chapterGoalHint":"先问清线人为何失约",
                "chapterLengthRange":{"min":2350,"max":2650}}),
        );
    }
    let result = execute("step-review:retry", json!(previous_job_id)).unwrap();
    assert_eq!(result["finalStatus"], "committed");
    if recompile_plan {
        assert!(render_count() > first_render_count);
        return;
    }
    assert_eq!(render_count(), first_render_count);
    let calls = gateway.inputs.lock().unwrap();
    let audits = calls
        .iter()
        .filter(|(capability, input)| {
            capability == "AUDIT_EXTRACT" && input["chapterId"] == chapter_id
        })
        .map(|(_, input)| &input["body"])
        .collect::<Vec<_>>();
    assert_eq!(audits.len(), 2);
    assert_eq!(audits[0], audits[1]);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let published: String = connection
        .query_row(
            "SELECT json_extract(p.payload_json,'$.body') FROM chapters c JOIN artifact_payloads p
         ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version
         WHERE c.project_id=?1 AND c.chapter_id=?2 AND c.status='COMMITTED'",
            libsql_rusqlite::params![project_id, chapter_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(Some(published.as_str()), audits[0].as_str());
}

fn prepare_stage_chapter(state: &KernelState, project_id: &str, volume_id: &str) -> Value {
    for (id, command_type) in [
        ("step-review:intent", "planning_create_volume_intent"),
        ("step-review:route", "planning_create_volume_plan"),
    ] {
        command(
            state,
            project_id,
            id,
            command_type,
            json!({"volumeId":volume_id}),
        );
    }
    let prepared = command(
        state,
        project_id,
        "step-review:prepare",
        "planning_prepare_next_chapter",
        json!({"volumeId":volume_id}),
    );
    command(
        state,
        project_id,
        "step-review:plan",
        "planning_compile_chapter_plan",
        json!({"chapterId":prepared["chapter"]["chapterId"],"chapterGoalHint":"让线人说出账本的来路",
            "chapterLengthRange":{"min":2300,"max":2600}}),
    );
    prepared
}

fn check_stage_step_completion(audit_reports_done: bool, verified: bool, invalid_evidence: bool) {
    let (state, _temp, gateway) = state_with(true, false);
    gateway.verify_stage_done.store(verified, Ordering::SeqCst);
    gateway
        .invalid_stage_evidence
        .store(invalid_evidence, Ordering::SeqCst);
    let (project_id, _) = create_approved_book_plan(&state);
    gateway
        .report_stage_done
        .store(audit_reports_done, Ordering::SeqCst);
    gateway.select_main_event.store(true, Ordering::SeqCst);
    let volumes =
        query_volume_list_impl(&state, json!({"projectId":project_id,"pageSize":20})).unwrap();
    let volume_id = volumes["items"][0]["id"].as_str().unwrap();
    let prepared = prepare_stage_chapter(&state, &project_id, volume_id);
    let chapter_id = prepared["chapter"]["chapterId"].as_str().unwrap();
    command(
        &state,
        &project_id,
        "step-review:produce",
        "production_start",
        json!({
            "chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM",
            "entityIds":[],"references":[],"skillsEnabled":false
        }),
    );
    let calls = gateway.inputs.lock().unwrap();
    let objective = if verified {
        "先找线人并且取得账本缺页"
    } else {
        "先找线人"
    };
    let (_, verification) = calls
        .iter()
        .find(|(name, _)| name == "VERIFY_STAGE_STEP")
        .unwrap();
    assert_eq!(verification["stageStep"]["objective"], objective);
    let (_, render) = calls
        .iter()
        .find(|(name, input)| name == "RENDER_CHAPTER" && input["chapterId"] == chapter_id)
        .unwrap();
    assert_eq!(render["stageStep"], verification["stageStep"]);
    let (_, plan) = calls
        .iter()
        .find(|(name, input)| name == "PLAN_STRATEGY" && input["chapterId"] == chapter_id)
        .unwrap();
    let stage_snapshot = &plan["volumeDirection"]["currentStage"];
    assert_eq!(
        stage_snapshot["outcomeRules"],
        verification["stageStep"]["outcomeRules"]
    );
    assert_eq!(
        render["chapterPlan"]["stageStepRulesDigest"],
        stage_snapshot["ruleDigest"]
    );
    let rule_digest = stage_snapshot["ruleDigest"].as_str().unwrap().to_owned();
    let (_, audit) = calls
        .iter()
        .find(|(name, input)| name == "AUDIT_EXTRACT" && input["chapterId"] == chapter_id)
        .unwrap();
    assert_eq!(audit["stageStep"], verification["stageStep"]);
    assert!(calls
        .iter()
        .filter(|(name, _)| name == "SELECT_MAIN_EVENT")
        .all(|(_, input)| input.get("stageStep").is_none()));
    if invalid_evidence {
        assert_eq!(
            calls
                .iter()
                .filter(|(name, _)| name == "VERIFY_STAGE_STEP")
                .count(),
            2
        );
        assert!(calls.iter().any(|(name, _)| name == "SELECT_MAIN_EVENT"));
    }
    drop(calls);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let recorded_rules: (String, String) = connection.query_row(
        "SELECT json_extract(p.payload_json,'$.audit.normalization.stageStepVerification.rulesDigest'),
                json_extract(p.payload_json,'$.audit.normalization.stageStepVerification.status')
         FROM canonical_heads h JOIN artifact_payloads r USING(artifact_id,version)
         JOIN artifact_payloads p ON p.artifact_id=json_extract(r.payload_json,'$.auditRef.artifactId')
           AND p.version=json_extract(r.payload_json,'$.auditRef.version')
         WHERE h.project_id=?1 AND h.artifact_type='ProductionReceipt'
           AND json_extract(h.scope_key,'$.chapterId')=?2",
        libsql_rusqlite::params![project_id, chapter_id], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(recorded_rules.0, rule_digest);
    assert_eq!(
        recorded_rules.1,
        if verified { "SATISFIED" } else { "UNVERIFIED" }
    );
    let status: String = connection.query_row(
        "SELECT json_extract(value_json,'$.status') FROM state_deltas WHERE project_id=?1 AND chapter_id=?2 AND state_type='volume.task.progress' AND state_id LIKE '%:step:1:1:1'",
        libsql_rusqlite::params![project_id, chapter_id], |row| row.get(0),
    ).unwrap();
    assert_eq!(status, if verified { "SATISFIED" } else { "ACTIVE" });
    if verified {
        let evidence: (String, String) = connection.query_row(
            "SELECT r.capability,r.status FROM canonical_heads h JOIN artifact_payloads p USING(artifact_id,version) \
             JOIN artifact_payloads a ON a.artifact_id=json_extract(p.payload_json,'$.auditRef.artifactId') AND a.version=json_extract(p.payload_json,'$.auditRef.version') \
             JOIN model_runs r ON r.model_run_id=json_extract(a.payload_json,'$.audit.normalization.stageStepVerification.modelRunId') \
             WHERE h.project_id=?1 AND h.artifact_type='ProductionReceipt' AND json_extract(h.scope_key,'$.chapterId')=?2 \
             AND EXISTS (SELECT 1 FROM json_each(a.payload_json,'$.audit.taskProgressCandidates') task \
                WHERE json_extract(task.value,'$.taskId')=json_extract(a.payload_json,'$.audit.normalization.stageStepVerification.taskId') \
                AND json_extract(task.value,'$.status')='SATISFIED') \
             AND r.job_id=json_extract(p.payload_json,'$.jobId') AND EXISTS (SELECT 1 FROM json_each(p.payload_json,'$.modelRunRefs') WHERE value=r.model_run_id)",
            libsql_rusqlite::params![project_id,chapter_id], |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(evidence, ("VERIFY_STAGE_STEP".into(), "COMPLETED".into()));
        let references: i64 = connection.query_row(
            "SELECT json_array_length(value_json,'$.evidenceRefs') FROM state_deltas WHERE project_id=?1 AND chapter_id=?2 AND state_type='volume.task.progress' AND state_id LIKE '%:step:1:1:1'",
            libsql_rusqlite::params![project_id, chapter_id], |row| row.get(0),
        ).unwrap();
        assert_eq!(references, 2);
    }
    drop(connection);
    let next = command(
        &state,
        &project_id,
        "step-review:prepare-next",
        "planning_prepare_next_chapter",
        json!({"volumeId":volume_id}),
    );
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let next_direction = ai_novel_production_system_core::book_direction::for_chapter(
        &connection,
        &project_id,
        next["chapter"]["chapterId"].as_str().unwrap(),
        5,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        next_direction["currentStage"]["goal"],
        if verified {
            "再查底册"
        } else {
            "先找线人"
        }
    );
    if invalid_evidence {
        let attempts = next_direction["currentStage"]["recentUnverifiedAttempts"]
            .as_array()
            .unwrap();
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0]["chapterNo"], 4);
        let final_ref = ai_novel_production_system_core::repositories::narrative::NarrativeRepository
            ::committed_final_chapter_head(&connection, &project_id, chapter_id).unwrap().unwrap();
        assert_eq!(
            attempts[0]["finalRef"],
            serde_json::to_value(final_ref).unwrap()
        );
        assert!(next_direction["currentStage"]["progress"].is_null());
    }
}

#[test]
fn vd1a_invalid_direction_preserves_the_signed_seed_and_blocks_approval() {
    let (state, _temp, gateway) = state_with(true, true);
    let project_id = create_signed_opening(&state);
    let error = execute_command_impl(
        &state,
        json!({
            "type":"book_plan_revise","commandId":"b2-red:invalid-direction",
            "idempotencyKey":"b2-red:invalid-direction:key","projectId":project_id,
            "expectedRevision":state.manager.project_revision(&project_id).unwrap(),
            "requestedAt":NOW,"payload":{}
        }),
    )
    .unwrap_err();
    assert!(error.starts_with("B07_BOOK_DIRECTION_REQUIRED:"), "{error}");
    let inputs = gateway.direction_inputs.lock().unwrap();
    assert_eq!(inputs.len(), 3, "one direction and two feedback rounds");
    assert!(inputs[1]["planFeedback"]["previousDirection"]["chapters"].is_array());
    assert!(inputs[2]["planFeedback"]["problems"][0]
        .as_str()
        .unwrap()
        .contains("不要写章表"));
    let (_, _, seed) = current_book_plan_payload(&state, &project_id);
    assert_eq!(seed["planningMaturity"], "SEED");
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    assert_eq!(center["bookPlan"]["status"], "PENDING");
    assert_eq!(center["runtimeOperation"]["status"], "FAILED");
}

#[test]
fn a_new_confirmed_signing_requires_reapproving_the_book_plan_before_production() {
    let (state, _temp, gateway) = state_with(true, false);
    let (project_id, _) = create_approved_book_plan(&state);
    let volumes =
        query_volume_list_impl(&state, json!({"projectId":project_id,"pageSize":20})).unwrap();
    let volume_id = volumes["items"][0]["id"].as_str().unwrap();
    let prepared = prepare_stage_chapter(&state, &project_id, volume_id);
    let chapter_id = prepared["chapter"]["chapterId"].as_str().unwrap();
    let evaluation = command(
        &state,
        &project_id,
        "new-signing:evaluate",
        "opening_evaluate",
        json!({}),
    );
    let platform = evaluation["result"]["platformMatrix"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["mainstream"] == true && item["status"] == "PASS")
        .unwrap()["platformId"]
        .clone();
    command(
        &state,
        &project_id,
        "new-signing:confirm",
        "signing_confirm",
        json!({"platformId":platform}),
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    assert_eq!(center["bookPlan"]["reapprovalRequired"], true);
    let calls_before = gateway.inputs.lock().unwrap().len();
    for (kind, payload) in [
        (
            "production_start",
            json!({"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM"}),
        ),
        (
            "planning_compile_chapter_plan",
            json!({"chapterId":chapter_id}),
        ),
    ] {
        let error = execute_command_impl(&state, json!({
            "type":kind,"commandId":format!("new-signing:{kind}"),"idempotencyKey":format!("new-signing:{kind}:key"),
            "projectId":project_id,"expectedRevision":revision(&state, &project_id),"requestedAt":NOW,"payload":payload
        })).unwrap_err();
        assert_eq!(error, "BOOK_PLAN_SIGNING_REAPPROVAL_REQUIRED");
    }
    assert_eq!(gateway.inputs.lock().unwrap().len(), calls_before);
    command(
        &state,
        &project_id,
        "new-signing:approve",
        "book_plan_revise",
        json!({}),
    );
    command(
        &state,
        &project_id,
        "new-signing:intent",
        "planning_create_volume_intent",
        json!({"volumeId":volume_id}),
    );
    command(
        &state,
        &project_id,
        "new-signing:route",
        "planning_create_volume_plan",
        json!({"volumeId":volume_id}),
    );
    command(
        &state,
        &project_id,
        "new-signing:chapter-plan",
        "planning_compile_chapter_plan",
        json!({"chapterId":chapter_id}),
    );
    let final_result = command(
        &state,
        &project_id,
        "new-signing:produce",
        "production_start",
        json!({"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM"}),
    );
    assert_eq!(final_result["finalStatus"], "committed");
}

#[test]
fn a_previously_approved_template_cannot_start_the_first_volume() {
    let (state, _temp, _gateway) = state_with(true, false);
    let (project, mut approved) = create_approved_book_plan(&state);
    approved["volumeIntentSeeds"][0]["stages"] = json!([]);
    commit_book_plan_fixture(&state, &project, approved);
    let volumes =
        query_volume_list_impl(&state, json!({"projectId":project,"pageSize":20})).unwrap();
    let volume_id = volumes["items"][0]["id"].as_str().unwrap();
    let result = execute_command_impl(
        &state,
        json!({
            "type":"planning_create_volume_plan","commandId":"b2-red:legacy-direction",
            "idempotencyKey":"b2-red:legacy-direction:key","projectId":project,
            "expectedRevision":state.manager.project_revision(&project).unwrap(),
            "requestedAt":NOW,"payload":{"volumeId":volume_id}
        }),
    );
    assert_eq!(result.unwrap_err(), "B07_BOOK_DIRECTION_REQUIRED");
}

/// VD-2: opens volume 2 after volume 1 (as the handoff does) and plans it.
fn open_volume_two(state: &KernelState, project_id: &str) -> (String, Value) {
    let volume_id = insert_volume_two(state, project_id);
    let planned = ai_novel_production_system_core::commands::direct_volume(
        state,
        project_id,
        &volume_id,
        "vd2:direct",
        "vd2:direct:key",
        NOW,
    )
    .unwrap();
    (volume_id, planned)
}

fn insert_volume_two(state: &KernelState, project_id: &str) -> String {
    use ai_novel_production_system_core::repositories::volume_lifecycle::VolumeLifecycleRepository;
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    let (book_id, end): (String, i64) = connection
        .query_row(
            "SELECT book_id, chapter_end FROM volumes WHERE project_id=?1 AND volume_no=1",
            [project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let volume_id = "vol:2d2d2d2d2d2d2d2d2d2d2d2d".to_string();
    VolumeLifecycleRepository::insert_next_volume(
        &connection,
        project_id,
        &volume_id,
        &book_id,
        2,
        "第二卷",
        end + 1,
        end + 30,
        90_000,
        NOW,
    )
    .unwrap();
    volume_id
}

#[test]
fn vd2_does_not_commit_a_direction_after_its_signing_input_changes() {
    use ai_novel_production_system_core::project_db::connection::open_connection;
    use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
    let (state, _temp, gateway) = state_with(true, false);
    let (project_id, _) = create_approved_book_plan(&state);
    let volume_id = insert_volume_two(&state, &project_id);
    let (_, old_version, _) = current_book_plan_payload(&state, &project_id);
    let db = state
        .paths
        .project_dir(&project_id)
        .unwrap()
        .join("project.db");
    let edited_project = project_id.clone();
    *gateway.while_planning_volume.lock().unwrap() = Some(Box::new(move || {
        // A separate connection injects a formal revision at the model boundary.
        let connection = open_connection(&db, false).unwrap();
        let signing = NarrativeRepository::canonical_project_head_ref(
            &connection,
            &edited_project,
            "SimulatedSigningReceipt",
        )
        .unwrap()
        .unwrap();
        let payload =
            NarrativeRepository::artifact_payload_by_ref(&connection, &edited_project, &signing)
                .unwrap()
                .unwrap();
        let bundle = serde_json::from_value(json!({
            "commitId":"commit:vd2:external-signing",
            "projectId":edited_project,"commandId":"vd2:external-signing",
            "idempotencyKey":"vd2:external-signing:key","contractVersion":"m0a.v1","createdAt":NOW,
            "expectedProjectRevision":NarrativeRepository::project_revision(&connection, &edited_project).unwrap(),
            "artifacts":[{
                "artifactId":signing.artifact_id,"projectId":edited_project,"artifactType":"SimulatedSigningReceipt",
                "scope":{"level":"PROJECT"},"version":signing.version+1,"schemaVersion":"m0a.artifact.v1",
                "artifactStatus":"CANDIDATE","payload":payload,"payloadHash":signing.payload_hash,
                "dependencyRefs":[signing],"provenance":{"createdBy":"test","commandId":"vd2:external-signing"},
                "createdAt":NOW
            }],
            "headUpdates":[{
                "artifactType":"SimulatedSigningReceipt","scope":{"level":"PROJECT"},"artifactId":signing.artifact_id,
                "version":signing.version+1,"payloadHash":signing.payload_hash
            }]
        })).unwrap();
        CommitService::commit(&connection, &bundle, NOW).unwrap();
    }));
    let result = ai_novel_production_system_core::commands::direct_volume(
        &state,
        &project_id,
        &volume_id,
        "vd2:stale-direction",
        "vd2:stale-direction:key",
        NOW,
    );
    assert_eq!(result.unwrap_err(), "REVISION_CONFLICT");
    let (_, version, plan) = current_book_plan_payload(&state, &project_id);
    assert_eq!(version, old_version);
    assert!(plan["volumeIntentSeeds"]
        .as_array()
        .unwrap()
        .iter()
        .all(|seed| seed["seedId"] != "volume-2"
            || seed["stages"].as_array().is_none_or(Vec::is_empty)));
}

#[test]
fn vd2_an_opened_volume_is_planned_from_the_one_before_and_its_route_follows() {
    let (state, _temp, gateway) = state_with(true, false);
    let (project_id, _) = create_approved_book_plan(&state);
    let (volume_id, planned) = open_volume_two(&state, &project_id);
    assert_eq!(planned, "PLANNED");
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    assert_eq!(center["bookPlan"]["reapprovalRequired"], false);
    let inputs = gateway.direction_inputs.lock().unwrap().clone();
    let input = inputs.last().unwrap();
    assert_eq!(
        input["volume"],
        json!({"volumeNo":2,"chapters":30,"stageChapters":{"min":15,"max":30}})
    );
    assert_eq!(
        input["book"]["storyPromise"],
        approved_story_promise(&state, &project_id)
    );
    let (_, _, book_plan) = current_book_plan_payload(&state, &project_id);
    let seed = book_plan["volumeIntentSeeds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|seed| seed["seedId"] == "volume-2")
        .unwrap()
        .clone();
    assert_eq!(seed["description"], "林舟离开县城，去府城追查账本的来路");
    assert_eq!(seed["stages"][0]["id"], "stage:2:1");
    let route = query_plan_route_impl(&state, json!({"projectId":project_id,"volumeId":volume_id}))
        .expect("volume 2 route");
    let required = route["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|task| task["class"] == "REQUIRED")
        .map(|task| task["objective"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(required.len(), 9, "{required:?}");
    assert_eq!(required[..3], ["先找线人", "再查底册", "最后对质"]);
}

#[test]
fn vd2_uses_the_previous_volumes_completed_steps_and_official_prose() {
    let (state, _temp, gateway) = state_with(true, false);
    let (project_id, approved) = create_approved_book_plan(&state);
    let volumes =
        query_volume_list_impl(&state, json!({"projectId":project_id,"pageSize":20})).unwrap();
    let volume_id = volumes["items"][0]["id"].as_str().unwrap();
    command(
        &state,
        &project_id,
        "vd2:previous-intent",
        "planning_create_volume_intent",
        json!({"volumeId":volume_id}),
    );
    command(
        &state,
        &project_id,
        "vd2:previous-route",
        "planning_create_volume_plan",
        json!({"volumeId":volume_id}),
    );
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let stages = approved["volumeIntentSeeds"][0]["stages"]
        .as_array()
        .unwrap();
    // A persisted, completed first stage and an incomplete second stage.
    for (stage_number, step_number, chapter_no) in [(0, 0, 1), (0, 1, 2), (0, 2, 3), (1, 2, 3)] {
        let step = stages[stage_number]["steps"][step_number]["id"]
            .as_str()
            .unwrap();
        let task_id = format!("task:required:goal:{volume_id}:{step}");
        connection.execute(
            "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at)
             SELECT ?1,?2,'volume.task.progress',?3,chapter_id,'{}',?5,?5
             FROM chapters WHERE project_id=?2 AND chapter_no=?4 AND status='COMMITTED'",
            libsql_rusqlite::params![task_id,project_id,json!({"taskId":task_id,"status":"SATISFIED"}).to_string(),
                chapter_no,NOW],
        ).unwrap();
    }
    drop(connection);
    assert_eq!(open_volume_two(&state, &project_id).1, "PLANNED");
    let requests = gateway.direction_inputs.lock().unwrap();
    let previous = &requests.last().unwrap()["previousVolume"];
    assert_eq!(previous["stages"][0]["completedInChapter"], 3);
    assert_eq!(previous["stages"][1]["completedInChapter"], Value::Null);
    assert!(previous["recentCommittedProse"][0]["body"]
        .as_str()
        .unwrap()
        .contains("决定追问账本缺页的去向"));
}

#[test]
fn vd2_a_volume_direction_that_keeps_listing_chapters_leaves_a_reminder() {
    let (state, _temp, gateway) = state_with(true, false);
    let (project_id, _) = create_approved_book_plan(&state);
    gateway
        .table_volume_directions
        .store(true, Ordering::SeqCst);
    let (_, planned) = open_volume_two(&state, &project_id);
    assert_eq!(planned, "TEMPLATE");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let reason: String = connection
        .query_row(
            "SELECT reason FROM decisions WHERE project_id=?1 AND decision_type='AUDIT_REMINDER' AND reason LIKE '第 2 卷方向%'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        reason.starts_with("第 2 卷方向没有生成，沿用了模板："),
        "{reason}"
    );
}

fn approved_story_promise(state: &KernelState, project_id: &str) -> Value {
    current_book_plan_payload(state, project_id).2["storyPromise"].clone()
}

#[test]
fn an_opening_audit_that_records_nothing_is_asked_again_so_signing_can_read_the_chapter() {
    // Real run round 19: chapter 2's audit reported no change and signing
    // found no committed facts for it.
    let (state, _temp, gateway) = state_with(false, false);
    gateway.blank_audit_once.store(true, Ordering::SeqCst);
    let (_project_id, approved) = create_approved_book_plan(&state);
    assert_eq!(approved["planningMaturity"], "APPROVED");
    let feedback = gateway.audit_feedback.lock().unwrap().clone();
    assert_eq!(feedback.len(), 1, "{feedback:?}");
    assert_eq!(
        feedback[0]["rejectedFor"],
        "B08_AUDIT_OPENING_CANON_REQUIRED"
    );
}

#[test]
fn p1_an_opening_records_its_events_and_relations() {
    // P1 (real run round 20): ten chapters left canon_events and
    // canon_relations empty.
    let (state, _temp, _gateway) = state();
    let (project_id, _) = create_approved_book_plan(&state);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let count = |table: &str| -> i64 {
        connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE project_id=?1"),
                [&project_id],
                |row| row.get(0),
            )
            .unwrap()
    };
    assert!(count("canon_events") > 0, "an event-typed fact is an event");
    let (subject, predicate, object): (String, String, String) = connection
        .query_row(
            "SELECT subject_id, predicate, object_id FROM canon_relations WHERE project_id=?1",
            [&project_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("a fact about another person is a relation");
    assert_eq!(
        (subject.as_str(), predicate.as_str(), object.as_str()),
        ("entity:b02:linzhou", "distrusts", "entity:b02:guqing")
    );
    // A relation to someone who is no entity stays a fact only.
    assert_eq!(count("canon_relations"), 1);
}

#[test]
fn p1_a_proposed_thread_later_chapters_take_up_is_advanced_then_resolved() {
    // P1 (real run round 20): 24 threads stayed PROPOSED, never shown to a
    // later chapter nor allowed to move outside its plan.
    let (state, _temp, gateway) = state();
    gateway.threads.store(true, Ordering::SeqCst);
    let (project_id, _) = create_approved_book_plan(&state);
    let seen = gateway.open_threads_seen.lock().unwrap().clone();
    assert_eq!(
        seen[0][0]["description"], "账本缺的那一页去了哪里",
        "{seen:?}"
    );
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let lifecycle: String = connection
        .query_row(
            "SELECT lifecycle FROM narrative_obligations WHERE project_id=?1 AND description='账本缺的那一页去了哪里'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(lifecycle, "RESOLVED");
}

#[test]
fn p1_a_hint_a_chapter_plants_is_a_foreshadow_until_a_later_chapter_recovers_it() {
    // P1 (real run round 20): the foreshadow table stayed empty.
    let (state, _temp, gateway) = state();
    gateway.threads.store(true, Ordering::SeqCst);
    gateway.thread_foreshadows.store(true, Ordering::SeqCst);
    let (project_id, _) = create_approved_book_plan(&state);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let (status, intent, payoff): (String, String, Option<String>) = connection
        .query_row(
            "SELECT status,intent,secret_payload_ref FROM foreshadows WHERE project_id=?1",
            [&project_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (status.as_str(), intent.as_str(), payoff.as_deref()),
        ("RECOVERED", "账本缺的那一页去了哪里", Some("缺页"))
    );
}

#[test]
fn p1_the_approved_book_plan_records_each_mystery_layer() {
    // P1 (real run round 20): no mystery was ever recorded, so the climax
    // task's MYSTERY_RESOLVED condition had nothing to resolve.
    let (state, _temp, _gateway) = state_with(true, false);
    let (project_id, _) = create_approved_book_plan(&state);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let mut statement = connection
        .prepare("SELECT obligation_id,lifecycle,description FROM narrative_obligations WHERE project_id=?1 AND obligation_type='MYSTERY'")
        .unwrap();
    let mysteries = statement
        .query_map([&project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        mysteries,
        [(
            "mystery:book:1".to_owned(),
            "APPROVED".to_owned(),
            "第 1 层：缺页记的是谁的债".to_owned()
        )]
    );
    // A chapter may answer it without its plan naming it.
    use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
    let open = NarrativeRepository::open_candidate_threads(&connection, &project_id, 8).unwrap();
    assert!(
        open.iter()
            .any(|thread| thread["threadRef"] == "thread:mystery:book:1"),
        "{open:?}"
    );
    assert!(NarrativeRepository::is_open_candidate_thread(
        &connection,
        &project_id,
        "mystery:book:1"
    )
    .unwrap());
}

#[test]
fn p2_the_setup_and_direction_steps_are_given_their_skills() {
    // P2: story directions and the book and volume directions never called
    // a skill (the story bible: craft_v3_setup).
    let (state, _temp, gateway) = state_with(true, false);
    let (project_id, _) = create_approved_book_plan(&state);
    open_volume_two(&state, &project_id);
    let inputs = gateway.inputs.lock().unwrap().clone();
    for (capability, stage, resident) in [
        ("GENERATE_STORY_DIRECTIONS", "story_directions", "S09"),
        ("PLAN_BOOK_DIRECTION", "book_plan", "S02"),
        ("PLAN_VOLUME_DIRECTION", "volume_plan", "S17"),
    ] {
        let (_, input) = inputs
            .iter()
            .find(|(called, _)| called == capability)
            .unwrap_or_else(|| panic!("{capability} was not called"));
        let guidance = if input["craftGuidance"].is_null() {
            &input["input"]["craftGuidance"]
        } else {
            &input["craftGuidance"]
        };
        assert_eq!(guidance["stage"], stage, "{capability}");
        let skills = guidance["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|skill| skill["skillId"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(skills.contains(&resident), "{capability}: {skills:?}");
    }
}

#[test]
fn an_empty_follow_up_audit_does_not_wipe_the_facts_the_first_one_found() {
    // Real run round 24: chapters 1 and 2's follow-up audits came back
    // COMPLETE_NO_CHANGE and were committed, so signing found no facts.
    let (state, _temp, gateway) = state();
    gateway.blank_feedback_once.store(true, Ordering::SeqCst);
    let (project_id, approved) = create_approved_book_plan(&state);
    assert_eq!(approved["planningMaturity"], "APPROVED");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let chapter_one_facts: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM canon_facts f JOIN chapters c ON c.chapter_id=f.valid_from_chapter_id WHERE f.project_id=?1 AND c.chapter_no=1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        chapter_one_facts > 0,
        "chapter 1 keeps the facts its first audit found"
    );
    let unregistered: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1 AND predicate='has_unregistered_mood'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        unregistered, 0,
        "the fact the ledger cannot take is held back"
    );
}

#[test]
fn a_chapter_plan_sees_the_next_steps_not_the_whole_route() {
    // Real run round 25: 26 step tasks and every other route task went into
    // chapter 4's context, over the hard budget.
    let (state, _temp, gateway) = state_with(true, false);
    let (project_id, _) = create_approved_book_plan(&state);
    let volumes = query_volume_list_impl(&state, json!({"projectId":project_id,"pageSize":20}))
        .expect("volume list after approval");
    let volume_id = volumes["items"][0]["id"]
        .as_str()
        .expect("volume id")
        .to_owned();
    for (step, command_type) in [
        ("volume-intent", "planning_create_volume_intent"),
        ("volume-plan", "planning_create_volume_plan"),
    ] {
        command(
            &state,
            &project_id,
            &format!("ctx:{step}"),
            command_type,
            json!({"volumeId":volume_id}),
        );
    }
    let prepared = command(
        &state,
        &project_id,
        "ctx:chapter-four-prepare",
        "planning_prepare_next_chapter",
        json!({"volumeId":volume_id}),
    );
    let chapter_id = prepared["chapter"]["chapterId"]
        .as_str()
        .unwrap()
        .to_owned();
    command(
        &state,
        &project_id,
        "ctx:chapter-four-plan",
        "planning_compile_chapter_plan",
        json!({"chapterId":chapter_id,"chapterGoalHint":"追查账本缺页的去向。"}),
    );
    let contexts = gateway.planning_contexts.lock().unwrap();
    let context = contexts
        .iter()
        .rev()
        .find(|context| context["executionMode"] == "LONG_FORM")
        .expect("LONG_FORM Plan input");
    let required = context["planTaskContexts"]
        .as_array()
        .expect("plan task contexts")
        .iter()
        .filter(|task| task["contract"]["class"] == "REQUIRED")
        .filter_map(|task| task["taskRef"].as_str())
        .collect::<Vec<_>>();
    // Nine steps and the book goal are required; the chapter sees the next few.
    assert_eq!(required.len(), 4, "{required:?}");
    assert!(required[0].ends_with(":step:1:1:1"), "{required:?}");
}

#[test]
fn after_a_step_is_done_the_next_chapter_works_on_the_next_step() {
    // Real run round 26: once step 1 was satisfied it left the context, its
    // state with it, so step 2 never looked ready and chapters 5-10 worked
    // only on an optional task.
    let (state, _temp, _gateway) = state_with(true, false);
    let (project_id, _) = create_approved_book_plan(&state);
    let volumes = query_volume_list_impl(&state, json!({"projectId":project_id,"pageSize":20}))
        .expect("volume list after approval");
    let volume_id = volumes["items"][0]["id"]
        .as_str()
        .expect("volume id")
        .to_owned();
    for (step, command_type) in [
        ("volume-intent", "planning_create_volume_intent"),
        ("volume-plan", "planning_create_volume_plan"),
    ] {
        command(
            &state,
            &project_id,
            &format!("next:{step}"),
            command_type,
            json!({"volumeId":volume_id}),
        );
    }
    let route = query_plan_route_impl(&state, json!({"projectId":project_id,"volumeId":volume_id}))
        .expect("volume route");
    let steps = route["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|task| task["class"] == "REQUIRED")
        .filter_map(|task| task["taskId"].as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    {
        let connection = state.manager.open(&project_id, "", NOW).unwrap();
        let chapter_three: String = connection
            .query_row(
                "SELECT chapter_id FROM chapters WHERE project_id=?1 AND chapter_no=3",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES (?1,?2,'volume.task.progress',?3,?4,'{}',?5,?5)",
                libsql_rusqlite::params![
                    steps[0],
                    project_id,
                    json!({"taskId":steps[0],"status":"SATISFIED"}).to_string(),
                    chapter_three,
                    NOW
                ],
            )
            .unwrap();
    }
    let prepared = command(
        &state,
        &project_id,
        "next:chapter-four-prepare",
        "planning_prepare_next_chapter",
        json!({"volumeId":volume_id}),
    );
    let chapter_id = prepared["chapter"]["chapterId"]
        .as_str()
        .unwrap()
        .to_owned();
    command(
        &state,
        &project_id,
        "next:chapter-four-plan",
        "planning_compile_chapter_plan",
        json!({"chapterId":chapter_id,"chapterGoalHint":"追查账本缺页的去向。"}),
    );
    let (_, _, plan) = {
        let connection = state.manager.open(&project_id, "", NOW).unwrap();
        let raw: String = connection
            .query_row(
                "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan' AND h.scope_key LIKE '%'||?2||'%'",
                libsql_rusqlite::params![project_id, chapter_id],
                |row| row.get(0),
            )
            .unwrap();
        ((), (), serde_json::from_str::<Value>(&raw).unwrap())
    };
    let active = plan["activeTaskRefs"].as_array().unwrap();
    assert!(
        active.iter().any(|task| task == &json!(steps[1])),
        "{active:?}"
    );
}

#[test]
fn a_chapter_whose_audit_marks_no_event_does_not_fabricate_one() {
    // 第 34 轮：首条事实只是环境描写，不能因此记成主要事件。
    let (state, _temp, _gateway) = state();
    let (project_id, _) = create_approved_book_plan(&state);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let events: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM canon_events e JOIN chapters c ON c.chapter_id=e.chapter_id WHERE e.project_id=?1 AND c.chapter_no=1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    // Chapter 1's audit marks no event.
    assert_eq!(events, 0);
}

#[test]
fn a_chapter_whose_audit_marks_no_event_records_a_reminder() {
    let (state, _temp, _gateway) = state();
    let (project_id, _) = create_approved_book_plan(&state);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM decisions d, json_each(d.payload_json,'$.heldBack') h WHERE d.project_id=?1 AND d.decision_type='AUDIT_REMINDER' AND json_extract(h.value,'$.reason')='QC1_MAIN_EVENT_MISSING'",
        [&project_id], |row| row.get(0)
    ).unwrap();
    assert!(count > 0, "缺少有证据的事件必须留提醒");
}

#[test]
fn a_main_event_is_selected_from_final_prose_and_committed_with_its_model_run() {
    // 假如审计漏掉事件，当补充检查从正式正文中选中行动和结果时，
    // 那么事件、证据和模型运行均应由正式提交回执记录。
    let (state, _temp, gateway) = state();
    gateway.select_main_event.store(true, Ordering::SeqCst);
    let (project_id, _) = create_approved_book_plan(&state);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let (event_type, quote, fact_id, origin, event_id): (String, String, Option<String>, String, String) = connection.query_row(
        "SELECT e.event_type, ce.quote, ce.fact_id, json_extract(e.payload_json,'$.event.eventTypeSource'), e.event_id FROM canon_events e JOIN canon_evidence ce ON ce.event_id=e.event_id JOIN chapters c ON c.chapter_id=e.chapter_id WHERE e.project_id=?1 AND c.chapter_no=1",
        [&project_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,row.get(4)?))
    ).unwrap();
    assert_eq!(event_type, "DISCOVERY");
    assert_eq!(origin, "model_selection");
    assert!(quote.contains("辨认出一段旧案暗记"));
    assert!(fact_id.is_none(), "事件无需创建虚假的事实");
    let selected: String = connection.query_row(
        "SELECT json_extract(a.payload_json,'$.audit.normalization.mainEventSelection.eventId') FROM canonical_heads h JOIN artifact_payloads r ON r.artifact_id=h.artifact_id AND r.version=h.version JOIN artifact_payloads a ON a.artifact_id=json_extract(r.payload_json,'$.auditRef.artifactId') AND a.version=json_extract(r.payload_json,'$.auditRef.version') JOIN chapters c ON c.chapter_id=json_extract(h.scope_key,'$.chapterId') WHERE h.artifact_type='ProductionReceipt' AND c.project_id=?1 AND c.chapter_no=1",
        [&project_id], |row| row.get(0)
    ).unwrap();
    assert_eq!(selected, event_id);
    let goal_review: Value = connection.query_row(
        "SELECT COALESCE(json_extract(a.payload_json,'$.audit.normalization.chapterGoalReview'),'null') FROM canonical_heads h
         JOIN artifact_payloads r USING(artifact_id,version)
         JOIN artifact_payloads a ON a.artifact_id=json_extract(r.payload_json,'$.auditRef.artifactId')
           AND a.version=json_extract(r.payload_json,'$.auditRef.version')
         JOIN chapters c ON c.chapter_id=json_extract(h.scope_key,'$.chapterId')
         WHERE h.artifact_type='ProductionReceipt' AND c.project_id=?1 AND c.chapter_no=1",
        [&project_id], |row| row.get::<_, String>(0)
    ).map(|raw| serde_json::from_str(&raw).unwrap()).unwrap();
    assert_eq!(goal_review["status"], "UNVERIFIED");
    let linked: i64 = connection.query_row(
        "SELECT COUNT(*) FROM canonical_heads h JOIN artifact_payloads r ON r.artifact_id=h.artifact_id AND r.version=h.version JOIN chapters c ON c.chapter_id=json_extract(h.scope_key,'$.chapterId') JOIN json_each(r.payload_json,'$.modelRunRefs') j JOIN model_runs m ON m.model_run_id=j.value WHERE h.artifact_type='ProductionReceipt' AND c.project_id=?1 AND c.chapter_no=1 AND m.capability='SELECT_MAIN_EVENT' AND m.status='COMPLETED'",
        [&project_id], |row| row.get(0)
    ).unwrap();
    assert_eq!(linked, 1);
}

#[test]
fn an_unbound_thread_update_leaves_an_audit_reminder_and_keeps_the_committed_story_event() {
    let (state, _temp, gateway) = state();
    gateway.select_main_event.store(true, Ordering::SeqCst);
    gateway.audit_unbound_thread.store(true, Ordering::SeqCst);
    let (project_id, _) = create_approved_book_plan(&state);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let (event_count, reminder_count): (i64, i64) = connection
        .query_row(
            "SELECT
          (SELECT COUNT(*) FROM canon_events e JOIN chapters c ON c.chapter_id=e.chapter_id
           WHERE e.project_id=?1 AND c.chapter_no=1 AND c.status='COMMITTED'),
          (SELECT COUNT(*) FROM decisions d, json_each(d.payload_json,'$.heldBack') item
           WHERE d.project_id=?1 AND d.decision_type='AUDIT_REMINDER'
             AND json_extract(item.value,'$.reason')='B08_THREAD_AUTHORITY_UNBOUND')",
            [&project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(event_count, 1);
    assert!(reminder_count > 0);
}

#[test]
fn an_audit_event_label_is_checked_and_the_same_outcome_is_committed_once() {
    let (state, _temp, gateway) = state();
    gateway.select_main_event.store(true, Ordering::SeqCst);
    *gateway.audit_first_event.lock().unwrap() = Some("DISCOVERY");
    let (project_id, _) = create_approved_book_plan(&state);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let events: (i64, i64, i64) = connection.query_row(
        "SELECT COUNT(*), COALESCE(SUM(json_extract(e.payload_json,'$.event.eventTypeSource')='model_selection'),0),
            COALESCE(SUM(EXISTS(SELECT 1 FROM canon_evidence x WHERE x.project_id=e.project_id AND x.event_id=e.event_id AND x.fact_id IS NOT NULL)),0)
         FROM canon_events e JOIN chapters c ON c.project_id=e.project_id AND c.chapter_id=e.chapter_id
         WHERE e.project_id=?1 AND c.chapter_no=1",
        [&project_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
    ).unwrap();
    assert_eq!(events, (1, 1, 1));
}

#[test]
fn an_unsupported_audit_event_type_does_not_replace_the_evidenced_outcome() {
    let (state, _temp, gateway) = state();
    gateway.select_main_event.store(true, Ordering::SeqCst);
    *gateway.audit_first_event.lock().unwrap() = Some("GAIN");
    let (project_id, _) = create_approved_book_plan(&state);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let events: Vec<String> = connection.prepare(
        "SELECT e.event_type FROM canon_events e JOIN chapters c ON c.project_id=e.project_id AND c.chapter_id=e.chapter_id
         WHERE e.project_id=?1 AND c.chapter_no=1",
    ).unwrap().query_map([&project_id], |row| row.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
    assert_eq!(events, ["DISCOVERY"]);
}

#[test]
fn an_invalid_main_event_selection_preserves_facts_and_leaves_a_reminder() {
    let (state, _temp, gateway) = state();
    gateway.select_main_event.store(true, Ordering::SeqCst);
    gateway.invalid_main_event.store(true, Ordering::SeqCst);
    let (project_id, _) = create_approved_book_plan(&state);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let (facts, events): (i64, i64) = connection.query_row(
        "SELECT (SELECT COUNT(*) FROM canon_facts f JOIN chapters c ON c.chapter_id=f.source_chapter_id WHERE f.project_id=?1 AND c.chapter_no=1), (SELECT COUNT(*) FROM canon_events e JOIN chapters c ON c.chapter_id=e.chapter_id WHERE e.project_id=?1 AND c.chapter_no=1)",
        [&project_id], |row| Ok((row.get(0)?, row.get(1)?))
    ).unwrap();
    assert!(facts > 0);
    assert_eq!(events, 0);
    let reminders: i64 = connection.query_row(
        "SELECT COUNT(*) FROM decisions d, json_each(d.payload_json,'$.heldBack') h WHERE d.project_id=?1 AND d.decision_type='AUDIT_REMINDER' AND json_extract(h.value,'$.reason')='QC1_MAIN_EVENT_SELECTION_FAILED'",
        [&project_id], |row| row.get(0)
    ).unwrap();
    assert!(reminders > 0);
}

#[test]
fn a_second_event_selection_binds_the_valid_citation_after_the_first_is_unusable() {
    let (state, _temp, gateway) = state();
    gateway.select_main_event.store(true, Ordering::SeqCst);
    gateway
        .first_main_event_invalid
        .store(true, Ordering::SeqCst);
    let (project, _) = create_approved_book_plan(&state);
    let connection = state.manager.open(&project, "", NOW).unwrap();
    let attempt: i64 = connection.query_row(
        "SELECT m.attempt_no FROM canon_events e JOIN chapters c ON c.chapter_id=e.chapter_id JOIN model_runs m ON m.model_run_id=json_extract(e.payload_json,'$.event.modelRunId') WHERE c.project_id=?1 AND c.chapter_no=1",
        [&project], |row| row.get(0),
    ).unwrap();
    assert_eq!(attempt, 2);
    assert!(gateway
        .inputs
        .lock()
        .unwrap()
        .iter()
        .any(|(capability, input)| {
            capability == "SELECT_MAIN_EVENT"
                && input["selectionFeedback"]["previousSelection"]["outcomeSentence"] == 999
        }));
}

#[test]
fn the_owner_rewrites_and_abandons_a_foreshadow_and_a_finished_one_stays() {
    // Owner decision 2026-11-03: a foreshadow from the audit is changed by
    // the owner directly, like a C-level thread, with a receipt.
    let (state, _temp, _gateway) = state();
    let (project_id, _) = create_approved_book_plan(&state);
    {
        let connection = state.manager.open(&project_id, "", NOW).unwrap();
        connection
            .execute(
                "INSERT INTO foreshadows(foreshadow_id,project_id,intent,secret_payload_ref,status,source_authority,created_at,updated_at) VALUES ('thread-fs-1',?1,'他按了按。','缺页','PLANTED','B08_ACTUAL_NARRATIVE',?2,?2)",
                libsql_rusqlite::params![project_id, NOW],
            )
            .unwrap();
    }
    let foreshadow = |state: &KernelState| -> (String, Option<String>, String) {
        let connection = state.manager.open(&project_id, "", NOW).unwrap();
        connection
            .query_row(
                "SELECT intent,secret_payload_ref,status FROM foreshadows WHERE project_id=?1 AND foreshadow_id='thread-fs-1'",
                [&project_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    };
    // The production center lists the open foreshadows to change.
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    assert_eq!(
        center["openForeshadows"],
        json!([{"threadRef":"thread:thread-fs-1","intent":"他按了按。","payoff":"缺页","recoverWindowEndChapter":null,"status":"PLANTED"}])
    );
    let edited = command(
        &state,
        &project_id,
        "fs:edit",
        "thread_responsibility_adjust",
        json!({"threadRef":"thread:thread-fs-1","action":"EDIT","intent":"账本缺的那一页记着谁的债","fulfillmentObject":"缺页上的名字"}),
    );
    assert_eq!(edited["finalStatus"], "committed");
    assert_eq!(
        foreshadow(&state),
        (
            "账本缺的那一页记着谁的债".to_owned(),
            Some("缺页上的名字".to_owned()),
            "PLANTED".to_owned()
        )
    );
    command(
        &state,
        &project_id,
        "fs:abandon",
        "thread_responsibility_adjust",
        json!({"threadRef":"thread:thread-fs-1","action":"DELETE"}),
    );
    assert_eq!(foreshadow(&state).2, "ABANDONED");
    let error = execute_command_impl(
        &state,
        json!({
            "type":"thread_responsibility_adjust","commandId":"fs:again","idempotencyKey":"fs:again:key",
            "projectId":project_id,"expectedRevision":revision(&state, &project_id),"requestedAt":NOW,
            "payload":{"threadRef":"thread:thread-fs-1","action":"EDIT","intent":"再改一次"}
        }),
    )
    .unwrap_err();
    assert!(error.contains("B08_FORESHADOW_NOT_ACTIVE"), "{error}");
}
