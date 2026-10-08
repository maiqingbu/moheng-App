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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const NOW: &str = "2026-09-13T12:00:00Z";

struct BehaviorGateway {
    audit_calls: AtomicUsize,
    planning_contexts: Mutex<Vec<Value>>,
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
            let offset = (index
                .wrapping_mul(7919)
                .wrapping_add(index.wrapping_mul(index).wrapping_mul(13))
                .wrapping_add(chapter_no as u32 * 977)
                % 20_992) as u32;
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
            "AUDIT_EXTRACT" => {
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
                        "evidence":evidence
                    })]
                } else {
                    Vec::new()
                };
                json!({
                    "qualityFindings":[],
                    "canonCandidates":canon_candidates,
                    "knowledgeCandidates":knowledge_candidates,
                    "continuityCandidates":[],
                    "taskProgressCandidates":[],
                    "revealedReservationRefs":[],
                    "evidenceRanges":[evidence],
                    "auditCompleteness":"COMPLETE_WITH_DELTA",
                    "unresolvedHard":false
                })
            }
            "OPENING_SIGNING_EVALUATION" => Self::signing_evaluation(input),
            other => return Err(format!("unexpected behavior-test capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

fn state() -> (KernelState, tempfile::TempDir, Arc<BehaviorGateway>) {
    let temp = tempfile::tempdir().expect("project directory");
    let gateway = Arc::new(BehaviorGateway {
        audit_calls: AtomicUsize::new(0),
        planning_contexts: Mutex::new(Vec::new()),
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

    for chapter_no in 1..=3 {
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
    let quote_strings = (1..=3).map(BehaviorGateway::quote).collect::<Vec<_>>();
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
    for chapter_no in 1..=3 {
        assert!(
            !serialized_hard_input.contains(&BehaviorGateway::quote(chapter_no)),
            "HARD Plan input contains raw evidence quote from opening chapter {chapter_no}"
        );
    }
}
