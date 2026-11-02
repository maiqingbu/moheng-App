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
    direction_inputs: Mutex<Vec<Value>>,
    /// Round 19: the first audit reports no change; set when it was asked
    /// again with feedback.
    blank_audit_once: AtomicBool,
    audit_feedback: Mutex<Vec<Value>>,
    /// P1: chapter 1 opens a thread, chapter 2 advances it and chapter 3
    /// resolves it, by the ref the audit is shown.
    threads: AtomicBool,
    /// The chapter-1 thread is a planted hint (a foreshadow).
    thread_foreshadows: AtomicBool,
    open_threads_seen: Mutex<Vec<Value>>,
}

impl BehaviorGateway {
    fn quote(chapter_no: usize) -> String {
        let opening = match chapter_no {
            1 => "林舟沿着雨水浸透的封条，辨认出一段旧案暗记。",
            2 => "顾清从渡口带回旧账联，发现边角新添了一枚签名。",
            3 => "两人在档案室核对页码后，决定追问账本缺页的去向。",
            _ => "调查者在档案室核对旧账，决定追问缺页背后的责任。",
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
        let body = chapter["body"].as_str().expect("signing chapter body");
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
                json!({"body":Self::quote(chapter_no)})
            }
            "AUDIT_EXTRACT" if self.blank_audit_once.swap(false, Ordering::SeqCst) => json!({
                "qualityFindings":[],"canonCandidates":[],"knowledgeCandidates":[],
                "continuityCandidates":[],"taskProgressCandidates":[],
                "revealedReservationRefs":[],"evidenceRanges":[],
                "auditCompleteness":"COMPLETE_NO_CHANGE","unresolvedHard":false
            }),
            "AUDIT_EXTRACT" => {
                if input.get("auditFeedback").is_some() {
                    self.audit_feedback
                        .lock()
                        .unwrap()
                        .push(input["auditFeedback"].clone());
                }
                let chapter_no = self.audit_calls.fetch_add(1, Ordering::SeqCst) + 1;
                let quote = Self::quote(chapter_no);
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
                let continuity_candidates = if !self.threads.load(Ordering::SeqCst) {
                    Vec::new()
                } else if chapter_no == 1 {
                    vec![json!({"disposition":"ADVANCE","description":"账本缺的那一页去了哪里","evidence":evidence,"fulfillmentObject":"缺页","promotionSignals":{"foreshadowRecoveryResponsibility":self.thread_foreshadows.load(Ordering::SeqCst)}})]
                } else {
                    self.open_threads_seen.lock().unwrap().push(input["openThreads"].clone());
                    input["openThreads"][0]["threadRef"]
                        .as_str()
                        .map(|thread| {
                            let disposition = if chapter_no == 2 { "ADVANCE" } else { "RESOLVE" };
                            vec![json!({"threadRef":thread,"disposition":disposition,"evidence":evidence})]
                        })
                        .unwrap_or_default()
                };
                json!({
                    "qualityFindings":[],
                    "canonCandidates":canon_candidates,
                    "knowledgeCandidates":knowledge_candidates,
                    "continuityCandidates":continuity_candidates,
                    "taskProgressCandidates":[],
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
                direction
            }
            "PLAN_VOLUME_DIRECTION" => {
                self.direction_inputs.lock().unwrap().push(input.clone());
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
                if self.table_directions {
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
        direction_inputs: Mutex::new(Vec::new()),
        blank_audit_once: AtomicBool::new(false),
        audit_feedback: Mutex::new(Vec::new()),
        threads: AtomicBool::new(false),
        thread_foreshadows: AtomicBool::new(false),
        open_threads_seen: Mutex::new(Vec::new()),
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

fn create_approved_book_plan(state: &KernelState) -> (String, Value) {
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
    let command_id = format!("rv03:legacy-book-plan:{project_id}");
    let artifact_id = format!("book-plan:rv03-legacy:{project_id}");
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
fn vd1a_a_direction_that_keeps_listing_chapters_leaves_the_template_and_a_reminder() {
    let (state, _temp, gateway) = state_with(true, true);
    let (project_id, approved) = create_approved_book_plan(&state);
    let inputs = gateway.direction_inputs.lock().unwrap().clone();
    assert_eq!(inputs.len(), 3, "one direction and two feedback rounds");
    assert!(inputs[2]["planFeedback"]["problems"][0]
        .as_str()
        .unwrap()
        .contains("不要写章表"));
    assert!(approved["mustAchieve"][0]["id"]
        .as_str()
        .unwrap()
        .starts_with("goal:opening-reality:"));
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let (reason, payload): (String, String) = connection
        .query_row(
            "SELECT reason, payload_json FROM decisions WHERE project_id=?1 AND decision_type='AUDIT_REMINDER' AND outcome='OPEN'",
            [&project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(
        reason.starts_with("全书方向没有生成，沿用了模板："),
        "{reason}"
    );
    assert!(payload.contains("规划提醒"), "{payload}");
}

/// VD-2: opens volume 2 after volume 1 (as the handoff does) and plans it.
fn open_volume_two(state: &KernelState, project_id: &str) -> (String, Value) {
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
    drop(connection);
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

#[test]
fn vd2_an_opened_volume_is_planned_from_the_one_before_and_its_route_follows() {
    let (state, _temp, gateway) = state_with(true, false);
    let (project_id, _) = create_approved_book_plan(&state);
    let (volume_id, planned) = open_volume_two(&state, &project_id);
    assert_eq!(planned, "PLANNED");
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
fn vd2_a_volume_direction_that_keeps_listing_chapters_leaves_a_reminder() {
    let (state, _temp, _gateway) = state_with(true, true);
    let (project_id, _) = create_approved_book_plan(&state);
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
            .query_row(&format!("SELECT COUNT(*) FROM {table} WHERE project_id=?1"), [&project_id], |row| {
                row.get(0)
            })
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
}

#[test]
fn p1_a_proposed_thread_later_chapters_take_up_is_advanced_then_resolved() {
    // P1 (real run round 20): 24 threads stayed PROPOSED, never shown to a
    // later chapter nor allowed to move outside its plan.
    let (state, _temp, gateway) = state();
    gateway.threads.store(true, Ordering::SeqCst);
    let (project_id, _) = create_approved_book_plan(&state);
    let seen = gateway.open_threads_seen.lock().unwrap().clone();
    assert_eq!(seen[0][0]["description"], "账本缺的那一页去了哪里", "{seen:?}");
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
    assert_eq!((status.as_str(), intent.as_str(), payoff.as_deref()), ("RECOVERED", "账本缺的那一页去了哪里", Some("缺页")));
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
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        mysteries,
        [("mystery:book:1".to_owned(), "APPROVED".to_owned(), "第 1 层：缺页记的是谁的债".to_owned())]
    );
    // A chapter may answer it without its plan naming it.
    use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
    let open = NarrativeRepository::open_candidate_threads(&connection, &project_id, 8).unwrap();
    assert!(open.iter().any(|thread| thread["threadRef"] == "thread:mystery:book:1"), "{open:?}");
    assert!(NarrativeRepository::is_open_candidate_thread(&connection, &project_id, "mystery:book:1").unwrap());
}
