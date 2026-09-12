use ai_novel_production_system_core::commands::{
    execute_command_impl, query_opening_ready_impl, query_production_center_impl,
    query_story_directions_impl, KernelState,
};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const NOW: &str = "2026-09-13T09:00:00Z";

struct B01Gateway {
    audit_calls: AtomicUsize,
}

impl B01Gateway {
    fn new() -> Self {
        Self {
            audit_calls: AtomicUsize::new(0),
        }
    }
}

impl ProductionModelGateway for B01Gateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        Ok(ResolvedProviderProfile {
            provider: "b01-fixture-provider".into(),
            model: "b01-fixture-model".into(),
            endpoint: "fixture://b01".into(),
            timeout_ms: 1_000,
        })
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let output = match capability {
            "GENERATE_STORY_DIRECTIONS" => {
                let source = input
                    .pointer("/input/sourceText")
                    .and_then(Value::as_str)
                    .unwrap_or("用户故事想法");
                let pitches = (0..3)
                    .map(|index| {
                        json!({
                            "name": format!("B01 方向 {}", index + 1),
                            "promise": format!("围绕 {source} 的三章开篇承诺 {index}"),
                            "protagonistDesire": "主角主动追索异常的源头",
                            "mechanism": "每次选择都要承担可验证的代价",
                            "worldConflict": "旧秩序阻止主角查明真相",
                            "longArc": "从被动承受走向主动选择",
                            "openingHook": "石门后的声音只回答了一半",
                            "volumeGoal": "完成第一次局部回报",
                            "commercialEdge": "悬疑推进与成长回报并行",
                            "originality": format!("B01 原创支点 {index}"),
                            "difficulty": "中等",
                            "failureRisk": "线索过早揭示"
                        })
                    })
                    .collect::<Vec<_>>();
                json!({"pitches": pitches})
            }
            "PLAN_STRATEGY" => json!({
                "payloadSchema":"chapter-plan.r1",
                "chapterId":input["chapterId"],
                "compiledFromProjectRevision":input["compiledFromProjectRevision"],
                "compiledFromStateDigest":input["compiledFromStateDigest"],
                "contextManifestRef":input["contextManifestRef"],
                "chapterGoal":"推进当前开篇行动并留下可验证后果",
                "activeTaskRefs":[],
                "continuityActions":[],
                "requiredStateOutcomes":[],
                "forbiddenTransitions":[],
                "knowledgeBoundaries":[],
                "reservationRefs":[],
                "sceneExecution":[],
                "horizonPreview":[
                    {"slotNo":1,"objective":"推进当前行动","tentative":true},
                    {"slotNo":2,"objective":"观察行动后果","tentative":true},
                    {"slotNo":3,"objective":"准备下一冲突","tentative":true}
                ]
            }),
            "RENDER_CHAPTER" => {
                let chapter_no = self.audit_calls.load(Ordering::SeqCst) + 1;
                json!({"body": format!("林舟推开石门，留下第{}章的真实后果。", chapter_no)})
            }
            "AUDIT_EXTRACT" => {
                let call = self.audit_calls.fetch_add(1, Ordering::SeqCst);
                if call == 0 {
                    json!({
                        "qualityFindings":[],
                        "canonCandidates":[{
                            "factId":"fact:b01:door-open",
                            "subjectId":"entity:b01:linzhou",
                            "entityId":"entity:b01:linzhou",
                            "canonicalName":"林舟",
                            "entityType":"CHARACTER",
                            "predicate":"opened",
                            "objectJson":{"target":"stone-door"},
                            "authorityLevel":"C",
                            "eventType":"ACTION",
                            "storyTime":"2026-01-01T00:00:00Z",
                            "evidence":{"start":0,"end":6,"quote":"林舟推开石门"}
                        }],
                        "knowledgeCandidates":[],
                        "continuityCandidates":[],
                        "taskProgressCandidates":[],
                        "revealedReservationRefs":[],
                        "evidenceRanges":[{"start":0,"end":6,"quote":"林舟推开石门"}]
                    })
                } else {
                    json!({
                        "qualityFindings":[],"canonCandidates":[],"knowledgeCandidates":[],
                        "continuityCandidates":[],"taskProgressCandidates":[],
                        "revealedReservationRefs":[],"evidenceRanges":[]
                    })
                }
            }
            other => return Err(format!("unexpected B01 capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

fn state() -> (KernelState, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("temp project directory");
    let root = temp.path().to_path_buf();
    let state =
        KernelState::with_production_gateway(AppPaths::new(root), Arc::new(B01Gateway::new()));
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
            "type": command_type,
            "commandId": command_id,
            "idempotencyKey": format!("{command_id}:key"),
            "projectId": project_id,
            "expectedRevision": revision(state, project_id),
            "requestedAt": NOW,
            "payload": payload
        }),
    )
    .unwrap_or_else(|error| panic!("{command_type} failed: {error}"))
}

fn create_project_through_opening_ready(state: &KernelState) -> String {
    let created = execute_command_impl(
        state,
        json!({
            "type":"project_create_from_idea",
            "commandId":"b01:create",
            "idempotencyKey":"b01:create:key",
            "requestedAt":NOW,
            "payload":{
                "title":"开篇三章验收",
                "idea":"一个炼丹师发现废丹会让妖兽进化。",
                "platform":"起点中文网",
                "mode":"DIRECTOR",
                "tags":["悬疑","成长"],
                "forbidden":"前三章不揭开机制来源"
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
        "b01:directions",
        "story_directions_generate",
        json!({}),
    );
    let directions = query_story_directions_impl(
        state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":10}),
    )
    .expect("query directions");
    let pitch_id = directions["pitches"][0]["id"]
        .as_str()
        .expect("pitch id")
        .to_owned();
    command(
        state,
        &project_id,
        "b01:select",
        "pitch_select",
        json!({"pitchIds":[pitch_id]}),
    );
    command(
        state,
        &project_id,
        "b01:setup",
        "story_setup_save",
        json!({
            "storyCore":{
                "protagonist":"林舟",
                "desire":"查明废丹进化的代价",
                "mechanism":"每次选择都会留下可验证代价",
                "conflict":"宗门要掩盖旧案",
                "readerPromise":"每章都有可回收的局部线索",
                "forbiddenReveals":["前三章不揭开机制来源"]
            }
        }),
    );
    command(
        state,
        &project_id,
        "b01:opening-ready",
        "opening_ready",
        json!({"approved":true}),
    );
    project_id
}

#[test]
fn b01_real_opening_lifecycle_creates_and_commits_three_chapters_without_volume_plan() {
    let (state, _temp) = state();
    let project_id = create_project_through_opening_ready(&state);

    let opening = query_opening_ready_impl(
        &state,
        json!({"request":{"projectId":project_id,"scope":"PROJECT","pageSize":10}}),
    )
    .expect("opening ready projection");
    assert_eq!(opening["ready"], true);
    assert_eq!(opening["openingProgress"]["total"], 3);
    assert_eq!(opening["openingProgress"]["completed"], 0);

    let connection = state.manager.open(&project_id, "", NOW).expect("open db");
    let chapter_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM chapters WHERE project_id=?1 AND chapter_no BETWEEN 1 AND 3",
            [&project_id],
            |row| row.get(0),
        )
        .expect("chapter count");
    assert_eq!(
        chapter_count, 3,
        "opening ready must create all golden-three topology"
    );
    let budget_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM chapter_production_budgets WHERE project_id=?1 AND chapter_id IN (SELECT chapter_id FROM chapters WHERE project_id=?1 AND chapter_no BETWEEN 1 AND 3)",
            [&project_id],
            |row| row.get(0),
        )
        .expect("budget count");
    assert_eq!(budget_count, 3);
    let volume_plan_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='VolumePlan'",
            [&project_id],
            |row| row.get(0),
        )
        .expect("volume plan count");
    assert_eq!(volume_plan_count, 0);
    drop(connection);

    command(
        &state,
        &project_id,
        "b01:start-creating",
        "opening_start_creating",
        json!({}),
    );

    let mut final_refs = Vec::new();
    for expected_chapter in 1..=3 {
        let center = query_production_center_impl(
            &state,
            json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
        )
        .expect("production center");
        assert_eq!(center["openingProgress"]["completed"], expected_chapter - 1);
        assert_eq!(center["openingProgress"]["total"], 3);
        assert_eq!(
            center["openingProgress"]["currentChapter"],
            expected_chapter
        );
        assert_eq!(
            center["openingProgress"]["previousCommitted"],
            expected_chapter > 1
        );
        assert_eq!(center["openingProgress"]["nextPrepared"], true);
        assert_eq!(center["nextChapter"]["number"], expected_chapter);
        assert_eq!(center["nextChapter"]["executionMode"], "OPENING_BOOTSTRAP");

        let result = command(
            &state,
            &project_id,
            &format!("b01:production:{expected_chapter}"),
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
        assert_eq!(result["finalStatus"], "committed");
        final_refs.push(
            result["finalChapterRef"]["artifactId"]
                .as_str()
                .unwrap()
                .to_owned(),
        );

        if expected_chapter == 2 {
            let connection = state
                .manager
                .open(&project_id, "", NOW)
                .expect("context db");
            let scope = json!({
                "level":"CHAPTER",
                "chapterId":center["nextChapter"]["id"]
            })
            .to_string();
            let context_raw: String = connection
                .query_row(
                    "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ContextManifest' AND h.scope_level='CHAPTER' AND h.scope_key=?2",
                    [&project_id, &scope],
                    |row| row.get(0),
                )
                .expect("chapter two context");
            let context: Value = serde_json::from_str(&context_raw).expect("context json");
            let source_refs = context["narrativeStateSources"]
                .as_array()
                .expect("narrative state sources");
            assert!(source_refs
                .iter()
                .any(|source| source["ref"] == "fact:b01:door-open"));
        }
    }

    let connection = state.manager.open(&project_id, "", NOW).expect("reopen db");
    let final_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='FinalChapter' AND artifact_status='COMMITTED'",
            [&project_id],
            |row| row.get(0),
        )
        .expect("final count");
    assert_eq!(final_count, 3);
    let receipt_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='ProductionReceipt' AND artifact_status='COMMITTED'",
            [&project_id],
            |row| row.get(0),
        )
        .expect("receipt count");
    assert_eq!(receipt_count, 3);
    let committed_numbers: Vec<i64> = {
        let mut statement = connection
            .prepare("SELECT chapter_no FROM chapters WHERE project_id=?1 AND status='COMMITTED' ORDER BY chapter_no")
            .expect("chapter numbers");
        statement
            .query_map([&project_id], |row| row.get(0))
            .expect("chapter number rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("chapter numbers collect")
    };
    assert_eq!(committed_numbers, vec![1, 2, 3]);
    assert_eq!(final_refs.len(), 3);
}
