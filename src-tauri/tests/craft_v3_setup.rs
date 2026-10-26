//! CRAFT v3 step 1c: story bible and style card candidates, user
//! confirmation and the local-only style sample (CRAFT_V3_DESIGN §4.1).

use ai_novel_production_system_core::commands::{
    execute_command_impl, query_craft_setup_impl, query_story_directions_impl, KernelState,
};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::{json, Value};
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const NOW: &str = "2026-10-23T02:00:00Z";
const SAMPLE_LINE: &str =
    "码头的灯一盏一盏灭了，林满把最后一笔账抄进本子，数目对上了，他才松口气。";

#[derive(Default)]
struct CraftGateway {
    ready_sentence: AtomicBool,
    bible_inputs: Mutex<Vec<Value>>,
    /// The next bible reply leaves out a required field.
    missing_field_once: AtomicBool,
    /// The next directions reply has two pitches instead of three.
    short_directions_once: AtomicBool,
}

fn bible(index: usize) -> Value {
    json!({
        "angle":format!("侧重角度 {index}"),
        "normalWorld":"林满在江边码头记账，每天清点货单、和船老大讨价还价，傍晚去姑妈的面馆帮工。",
        "mechanism":{"experience":"触碰旧物时会先听见物主最后一次说话的声音。","rules":[
            {"rule":"每听一次旧物就忘掉自己一天的记忆","numbers":"一次一天，每日最多三次","howReadersSeeIt":"第二天对着账本想不起自己记过的一笔"},
            {"rule":"旧物离开原主超过一年就听不见","numbers":"一年为限","howReadersSeeIt":"当铺里的旧表一声不响"}]},
        "protagonist":{"name":format!("林满{index}"),"personality":"嘴硬心软，算账极细","flaw":"凡事先算代价","wantsNow":"赎回姑妈的面馆","humorStyle":"一本正经地抠字眼","innerVoiceStyle":"像给自己记账，先列数再下结论"},
        "emotionalAnchor":{"object":"姑妈补过三次的围裙","memorySummary":"十岁发烧时姑妈守了他一夜"},
        "antagonist":{"name":"彭三","position":"码头放贷的二当家","wants":"吞下面馆那块地","method":"借据和打手"},
        "cast":[
            {"name":"阿秋","role":"面馆伙计","relation":"发小","privateAgenda":"想去城里学厨","speechStyle":"话少，开口就呛人","contrastWithProtagonist":"冲动","firstAppears":1},
            {"name":"老董","role":"船老大","relation":"主顾","privateAgenda":"欠彭三的钱","speechStyle":"满嘴俗语","contrastWithProtagonist":"认命","firstAppears":2}],
        "openingArc":[
            {"chapter":1,"direction":"账目里出现一笔对不上的货","protagonistWin":"当场揪出调包的货单"},
            {"chapter":2,"direction":"彭三上门催债","protagonistWin":"用旧物的声音找到借据漏洞"},
            {"chapter":3,"direction":"林满主动设局","protagonistWin":"让彭三的人当众认下假账"}]
    })
}

impl ProductionModelGateway for CraftGateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        Ok(ResolvedProviderProfile {
            provider: "fixture-provider".into(),
            model: "fixture-model".into(),
            endpoint: "fixture://provider".into(),
            timeout_ms: 1_000,
        })
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        match capability {
            "GENERATE_STORY_DIRECTIONS" => {
                let pitches = (0..3)
                    .map(|index| {
                        let mut pitch = json!({});
                        for field in [
                            "promise",
                            "protagonistDesire",
                            "mechanism",
                            "worldConflict",
                            "longArc",
                            "openingHook",
                            "volumeGoal",
                            "commercialEdge",
                            "originality",
                            "difficulty",
                            "failureRisk",
                        ] {
                            pitch[field] = json!(format!("{field} {index}"));
                        }
                        pitch["name"] = json!(format!("方向 {index}"));
                        pitch
                    })
                    .collect::<Vec<_>>();
                let pitches = if self.short_directions_once.swap(false, Ordering::SeqCst) {
                    pitches[..2].to_vec()
                } else {
                    pitches
                };
                Ok(ProductionModelResponse::deterministic(
                    json!({"pitches":pitches}),
                ))
            }
            "GENERATE_STORY_BIBLE" => {
                self.bible_inputs.lock().unwrap().push(input.clone());
                let mut bibles = (0..3).map(bible).collect::<Vec<_>>();
                if self.ready_sentence.load(Ordering::SeqCst) {
                    bibles[1]["protagonist"]["innerVoiceStyle"] =
                        json!("常对自己说：“这笔账我记下了。”");
                }
                if self.missing_field_once.swap(false, Ordering::SeqCst) {
                    bibles[0]["mechanism"]["rules"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("howReadersSeeIt");
                }
                Ok(ProductionModelResponse::deterministic(
                    json!({"bibles":bibles}),
                ))
            }
            other => Err(format!("unexpected capability: {other}")),
        }
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    state: KernelState,
    gateway: Arc<CraftGateway>,
    project_id: String,
}

impl Deref for Fixture {
    type Target = KernelState;
    fn deref(&self) -> &KernelState {
        &self.state
    }
}

fn command(fixture: &Fixture, kind: &str, id: &str, payload: Value) -> Result<Value, String> {
    let revision = fixture
        .manager
        .project_revision(&fixture.project_id)
        .unwrap();
    execute_command_impl(
        fixture,
        json!({
            "type":kind,"commandId":id,"idempotencyKey":format!("{id}:key"),
            "projectId":fixture.project_id,"expectedRevision":revision,"requestedAt":NOW,
            "payload":payload
        }),
    )
}

/// Given a project whose story core is saved (StoryConstitution exists).
fn project_with_story_setup() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway::default());
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let created = execute_command_impl(&state, json!({
        "type":"project_create_from_idea","commandId":"craft:create","idempotencyKey":"craft:create:key",
        "requestedAt":NOW,
        "payload":{"title":"码头账房","idea":"码头账房能听见旧物的声音。","platform":"番茄小说","mode":"DIRECTOR","tags":["年代","底层奋斗"]}
    })).unwrap();
    let fixture = Fixture {
        _temp: temp,
        state,
        gateway,
        project_id: created["projectId"].as_str().unwrap().to_owned(),
    };
    command(
        &fixture,
        "story_directions_generate",
        "craft:directions",
        json!({}),
    )
    .unwrap();
    let directions = query_story_directions_impl(
        &fixture,
        json!({"projectId":fixture.project_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    let pitch_id = directions["pitches"][0]["id"].as_str().unwrap().to_owned();
    command(
        &fixture,
        "pitch_select",
        "craft:select",
        json!({"pitchIds":[pitch_id]}),
    )
    .unwrap();
    command(&fixture, "story_setup_save", "craft:setup", json!({"storyCore":{
        "protagonist":"码头账房林满","desire":"赎回姑妈的面馆","mechanism":"听旧物的声音要付出记忆",
        "conflict":"放贷的彭三要吞面馆","readerPromise":"每章用账目和旧物赢一局","forbiddenReveals":[]
    }})).unwrap();
    fixture
}

fn setup(fixture: &Fixture) -> Value {
    query_craft_setup_impl(fixture, json!({"projectId":fixture.project_id})).unwrap()
}

fn count(fixture: &Fixture, sql: &str) -> i64 {
    let connection = fixture.manager.open(&fixture.project_id, "", NOW).unwrap();
    connection
        .query_row(sql, [&fixture.project_id], |row| row.get(0))
        .unwrap()
}

#[test]
fn generating_story_bible_commits_three_candidates_without_a_head() {
    let fixture = project_with_story_setup();
    let revision = fixture
        .manager
        .project_revision(&fixture.project_id)
        .unwrap();

    command(
        &fixture,
        "story_bible_generate",
        "craft:bible",
        json!({"adjustment":"多些人情味"}),
    )
    .unwrap();

    assert_eq!(
        fixture
            .manager
            .project_revision(&fixture.project_id)
            .unwrap(),
        revision + 1
    );
    let view = setup(&fixture);
    let candidates = view["storyBible"]["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 3);
    assert!(candidates
        .iter()
        .all(|item| item["stage"] == "CANDIDATE" && item["id"].is_string()));
    assert!(view["storyBible"]["confirmed"].is_null());
    let inputs = fixture.gateway.bible_inputs.lock().unwrap();
    assert_eq!(
        inputs[0]["input"]["storyCore"]["protagonist"],
        "码头账房林满"
    );
    assert_eq!(inputs[0]["input"]["adjustment"], "多些人情味");
}

#[test]
fn story_bible_candidates_with_ready_sentences_are_rejected_without_writes() {
    let fixture = project_with_story_setup();
    fixture.gateway.ready_sentence.store(true, Ordering::SeqCst);
    let revision = fixture
        .manager
        .project_revision(&fixture.project_id)
        .unwrap();

    let error = command(
        &fixture,
        "story_bible_generate",
        "craft:bible-bad",
        json!({}),
    )
    .unwrap_err();

    assert!(error.contains("STORY_BIBLE_READY_SENTENCE"), "{error}");
    assert_eq!(
        fixture.gateway.bible_inputs.lock().unwrap().len(),
        2,
        "generated again once before giving up"
    );
    assert_eq!(
        fixture
            .manager
            .project_revision(&fixture.project_id)
            .unwrap(),
        revision
    );
    assert_eq!(
        count(
            &fixture,
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='StoryBible'"
        ),
        0
    );
}

#[test]
fn a_story_bible_reply_missing_a_field_is_generated_again_once() {
    // Owner decision (2026-10-25): a setup step whose output fails its
    // checks is generated again once before the user has to.
    let fixture = project_with_story_setup();
    fixture
        .gateway
        .missing_field_once
        .store(true, Ordering::SeqCst);

    command(
        &fixture,
        "story_bible_generate",
        "craft:bible-retry",
        json!({}),
    )
    .unwrap();

    assert_eq!(
        setup(&fixture)["storyBible"]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(fixture.gateway.bible_inputs.lock().unwrap().len(), 2);
    assert_eq!(
        count(
            &fixture,
            "SELECT COUNT(*) FROM model_runs WHERE project_id=?1 AND capability='GENERATE_STORY_BIBLE'"
        ),
        2,
        "the rejected reply stays recorded"
    );
}

#[test]
fn a_story_directions_reply_with_too_few_pitches_is_generated_again_once() {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway::default());
    gateway.short_directions_once.store(true, Ordering::SeqCst);
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let created = execute_command_impl(&state, json!({
        "type":"project_create_from_idea","commandId":"craft:create","idempotencyKey":"craft:create:key",
        "requestedAt":NOW,
        "payload":{"title":"码头账房","idea":"码头账房能听见旧物的声音。","platform":"番茄小说","mode":"DIRECTOR","tags":["年代"]}
    })).unwrap();
    let fixture = Fixture {
        _temp: temp,
        state,
        gateway,
        project_id: created["projectId"].as_str().unwrap().to_owned(),
    };

    command(
        &fixture,
        "story_directions_generate",
        "craft:directions",
        json!({}),
    )
    .unwrap();

    let directions = query_story_directions_impl(
        &fixture,
        json!({"projectId":fixture.project_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    assert_eq!(directions["pitches"].as_array().unwrap().len(), 3);
    assert_eq!(
        count(
            &fixture,
            "SELECT COUNT(*) FROM model_runs WHERE project_id=?1 AND capability='GENERATE_STORY_DIRECTIONS'"
        ),
        2
    );
}

#[test]
fn confirming_an_edited_story_bible_sets_the_head_and_keeps_the_candidate_link() {
    let fixture = project_with_story_setup();
    command(&fixture, "story_bible_generate", "craft:bible", json!({})).unwrap();
    let candidate = setup(&fixture)["storyBible"]["candidates"][1].clone();
    let mut edited = candidate.clone();
    edited["antagonist"]["name"] = json!("彭老三");

    command(
        &fixture,
        "story_bible_confirm",
        "craft:bible-confirm",
        json!({"candidateId":candidate["id"],"bible":edited}),
    )
    .unwrap();

    let confirmed = setup(&fixture)["storyBible"]["confirmed"].clone();
    assert_eq!(confirmed["stage"], "CONFIRMED");
    assert_eq!(confirmed["antagonist"]["name"], "彭老三");
    assert_eq!(confirmed["confirmedFromRef"]["artifactId"], candidate["id"]);
    assert_eq!(confirmed["generation"], candidate["generation"]);
}

#[test]
fn confirming_an_invalid_story_bible_edit_writes_nothing() {
    let fixture = project_with_story_setup();
    command(&fixture, "story_bible_generate", "craft:bible", json!({})).unwrap();
    let candidate = setup(&fixture)["storyBible"]["candidates"][0].clone();
    let mut edited = candidate.clone();
    edited["openingArc"].as_array_mut().unwrap().pop();
    let revision = fixture
        .manager
        .project_revision(&fixture.project_id)
        .unwrap();

    let error = command(
        &fixture,
        "story_bible_confirm",
        "craft:bible-bad-confirm",
        json!({"candidateId":candidate["id"],"bible":edited}),
    )
    .unwrap_err();

    assert!(error.contains("STORY_BIBLE_CONTRACT"), "{error}");
    assert_eq!(
        fixture
            .manager
            .project_revision(&fixture.project_id)
            .unwrap(),
        revision
    );
    assert!(setup(&fixture)["storyBible"]["confirmed"].is_null());
}

#[test]
fn story_bible_generation_requires_a_saved_story_core() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        Arc::new(CraftGateway::default()),
    );
    let created = execute_command_impl(&state, json!({
        "type":"project_create_from_idea","commandId":"craft:bare","idempotencyKey":"craft:bare:key",
        "requestedAt":NOW,"payload":{"title":"空项目","idea":"一个想法。","platform":"番茄小说","mode":"DIRECTOR"}
    })).unwrap();
    let error = execute_command_impl(&state, json!({
        "type":"story_bible_generate","commandId":"craft:bare:bible","idempotencyKey":"craft:bare:bible:key",
        "projectId":created["projectId"],"expectedRevision":0,"requestedAt":NOW,"payload":{}
    })).unwrap_err();
    assert!(error.contains("B11_STORY_SETUP_REQUIRED"), "{error}");
}

#[test]
fn archetype_style_cards_follow_the_project_genre_and_can_be_confirmed() {
    let fixture = project_with_story_setup();

    command(
        &fixture,
        "style_card_generate",
        "craft:style",
        json!({"source":"MARKET_ARCHETYPE"}),
    )
    .unwrap();

    let view = setup(&fixture);
    let candidates = view["styleCard"]["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 3);
    // tags 年代 / 底层奋斗 → the 冷峻市井 archetype leads.
    assert_eq!(candidates[0]["source"]["archetypeIds"][0], "A2");
    assert!(view["archetypes"].as_array().unwrap().len() == 8);

    let chosen = candidates[2].clone();
    let mut edited = chosen.clone();
    edited["rules"]
        .as_array_mut()
        .unwrap()
        .push(json!("每章至少一处钱数变化"));
    command(
        &fixture,
        "style_card_confirm",
        "craft:style-confirm",
        json!({"candidateId":chosen["id"],"card":edited}),
    )
    .unwrap();
    let confirmed = setup(&fixture)["styleCard"]["confirmed"].clone();
    assert_eq!(confirmed["stage"], "CONFIRMED");
    assert_eq!(confirmed["confirmedFromRef"]["artifactId"], chosen["id"]);
    assert!(confirmed["rules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| rule == "每章至少一处钱数变化"));
}

#[test]
fn explicit_archetype_pair_overrides_the_recommendation() {
    let fixture = project_with_story_setup();
    command(
        &fixture,
        "style_card_generate",
        "craft:style-pair",
        json!({"source":"MARKET_ARCHETYPE","primary":"A3","secondary":"A6"}),
    )
    .unwrap();
    let candidates = setup(&fixture)["styleCard"]["candidates"].clone();
    assert_eq!(candidates[0]["variant"], "A3_PURE");
    assert_eq!(candidates[1]["source"]["archetypeIds"], json!(["A3", "A6"]));
}

#[test]
fn style_sample_stays_local_and_only_its_metrics_reach_the_cards() {
    let fixture = project_with_story_setup();
    let sample = format!("{SAMPLE_LINE}\n").repeat(30);
    let revision = fixture
        .manager
        .project_revision(&fixture.project_id)
        .unwrap();

    command(
        &fixture,
        "style_sample_save",
        "craft:sample",
        json!({"text":sample}),
    )
    .unwrap();

    assert_eq!(
        fixture
            .manager
            .project_revision(&fixture.project_id)
            .unwrap(),
        revision
    );
    let view = setup(&fixture);
    assert_eq!(view["sample"]["text"], sample.trim());
    assert!(
        view["sample"]["metrics"]["avgParagraphChars"]
            .as_f64()
            .unwrap()
            > 0.0
    );
    let digest = view["sample"]["digest"].as_str().unwrap().to_owned();

    command(
        &fixture,
        "style_card_generate",
        "craft:style-sample",
        json!({"source":"USER_SAMPLE"}),
    )
    .unwrap();

    let candidates = setup(&fixture)["styleCard"]["candidates"].clone();
    assert_eq!(candidates[0]["source"]["sampleDigest"], digest);
    assert_eq!(
        count(&fixture, &format!(
            "SELECT COUNT(*) FROM artifact_payloads p JOIN artifacts a ON a.artifact_id=p.artifact_id AND a.version=p.version WHERE a.project_id=?1 AND p.payload_json LIKE '%{}%'",
            &SAMPLE_LINE[..15]
        )),
        0
    );
}

#[test]
fn sample_style_cards_require_a_saved_sample() {
    let fixture = project_with_story_setup();
    let error = command(
        &fixture,
        "style_card_generate",
        "craft:style-nosample",
        json!({"source":"USER_SAMPLE"}),
    )
    .unwrap_err();
    assert!(error.contains("STYLE_SAMPLE_MISSING"), "{error}");
}
