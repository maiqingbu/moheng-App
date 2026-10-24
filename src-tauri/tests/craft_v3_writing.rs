//! CRAFT v3 step 3b: an r2 chapter is written in one piece with the story
//! bible and the style card, brought into the length range by one
//! expand/compress call, and its rule findings (tropes included) are edited
//! in place without a model review.

mod craft_v3_support;

use ai_novel_production_system_core::commands::{
    query_craft_setup_impl, query_production_center_impl, KernelState,
};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use craft_v3_support::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct CraftGateway {
    calls: Mutex<Vec<(String, Value)>>,
    /// Chapters written so far, so each chapter's text differs.
    chapters_written: AtomicUsize,
}

/// The inputs of every call to `capability`, in call order.
fn inputs(calls: &[(String, Value)], capability: &str) -> Vec<Value> {
    calls
        .iter()
        .filter(|(name, _)| name == capability)
        .map(|(_, input)| input.clone())
        .collect()
}

const TROPE_SENTENCE: &str = "林满倒吸一口凉气，转身进了面馆。";

/// A chapter in the default 2000–3000 range with one stock phrasing.
fn long_body(chapter: usize) -> String {
    let mut lines = (1..=110)
        .map(|index| format!("货单第{index}页写着{index}箱桐油，老董按了第{chapter}个手印。"))
        .collect::<Vec<_>>();
    lines.insert(40, TROPE_SENTENCE.to_string());
    lines.join("\n")
}

impl ProductionModelGateway for CraftGateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        Ok(ResolvedProviderProfile {
            provider: "craft-fixture-provider".into(),
            model: "craft-fixture-model".into(),
            endpoint: "fixture://craft".into(),
            timeout_ms: 1_000,
        })
    }

    fn enforces_chapter_length(&self) -> bool {
        true
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut calls = self.calls.lock().unwrap();
        calls.push((capability.to_string(), input.clone()));
        let output = match capability {
            "GENERATE_STORY_DIRECTIONS" => {
                let pitches = (0..3)
                    .map(|index| {
                        let mut pitch = json!({"name":format!("方向 {index}")});
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
                        pitch
                    })
                    .collect::<Vec<_>>();
                json!({"pitches":pitches})
            }
            "GENERATE_STORY_BIBLE" => json!({"bibles":[bible(), bible(), bible()]}),
            "PLAN_CHAPTER_CRAFT" => {
                craft_plan(input, input["chapterNumber"].as_i64().unwrap_or(1), false)
            }
            "RENDER_CHAPTER_CRAFT" => json!({"body":"林满推开面馆的门，把账本放在了灶台边。"}),
            "ADJUST_CHAPTER_LENGTH" => {
                let chapter = self.chapters_written.fetch_add(1, Ordering::SeqCst) + 1;
                json!({"body":long_body(chapter)})
            }
            "REWRITE_PASSAGES" => {
                json!({"edits":[{"quote":TROPE_SENTENCE,"replacement":"林满没吭声，转身进了面馆。"}]})
            }
            "AUDIT_EXTRACT" => {
                let mut audit = audit_no_change();
                if input["continuityDetailsWanted"] == true {
                    audit["continuityDetails"] = json!([
                        {"subject":"林满","kind":"PREFERENCE","detail":"喝豆浆不加糖","quote":"货单第1页写着1箱桐油"},
                        {"subject":"林满","kind":"HABIT","detail":"编出来的习惯","quote":"正文里没有这句"}
                    ]);
                }
                audit
            }
            other => return Err(format!("unexpected capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

/// A project with a confirmed story bible and style card, opening started.
fn project_ready_to_write(
    prefix: &str,
) -> (tempfile::TempDir, KernelState, Arc<CraftGateway>, String) {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway::default());
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, prefix);
    command(
        &state,
        &project_id,
        &format!("{prefix}:style"),
        "style_card_generate",
        json!({"source":"MARKET_ARCHETYPE","primary":"A2","secondary":"A1"}),
    );
    let card = query_craft_setup_impl(&state, json!({"projectId":project_id})).unwrap()
        ["styleCard"]["candidates"][0]
        .clone();
    command(
        &state,
        &project_id,
        &format!("{prefix}:style-confirm"),
        "style_card_confirm",
        json!({"candidateId":card["id"],"card":card}),
    );
    command(
        &state,
        &project_id,
        &format!("{prefix}:opening"),
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        &format!("{prefix}:start"),
        "opening_start_creating",
        json!({}),
    );
    (temp, state, gateway, project_id)
}

/// Writes the next opening chapter.
fn write_next_chapter(state: &KernelState, project_id: &str, id: &str) {
    let center = query_production_center_impl(
        state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let result = command(
        state,
        project_id,
        id,
        "production_start",
        json!({
            "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
            "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
        }),
    );
    assert_eq!(result["finalStatus"], "committed");
}

#[test]
fn an_r2_chapter_is_written_whole_with_bible_and_style_card_then_sized_and_edited() {
    let (_temp, state, gateway, project_id) = project_ready_to_write("w3");
    write_next_chapter(&state, &project_id, "w3:production:1");

    let calls = gateway.calls.lock().unwrap();
    let call = |capability: &str| inputs(&calls, capability).into_iter().next();
    assert!(
        call("REVIEW_CHAPTER").is_none(),
        "r2 chapters are not reviewed by a model"
    );
    assert!(
        call("RENDER_CHAPTER").is_none(),
        "r2 chapters use the craft writer"
    );
    let render = call("RENDER_CHAPTER_CRAFT").expect("craft writer");
    assert_eq!(render["storyBible"]["antagonist"]["name"], "彭三");
    assert_eq!(render["styleCard"]["narration"]["povPolicy"], "STRICT");
    assert!(
        render["styleCard"].get("stage").is_none(),
        "the writer gets the card content only"
    );
    assert_eq!(render["chapterNumber"], 1);
    assert_eq!(render["chapterPlan"]["payloadSchema"], "chapter-plan.r2");
    assert!(render.get("beatTargets").is_none() && render.get("partLengthRange").is_none());
    let adjust = call("ADJUST_CHAPTER_LENGTH").expect("length adjustment");
    assert_eq!(adjust["direction"], "EXPAND");
    // Real run 2026-10-24: told only how far over, the model cut about a
    // tenth; it now gets the count it has and the count to reach.
    assert!(adjust["currentChars"].as_i64() < adjust["targetChars"].as_i64());
    assert!(adjust.get("shortBy").is_none() && adjust.get("overBy").is_none());
    let rewrite = call("REWRITE_PASSAGES").expect("rule edits");
    assert!(rewrite["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "repeated_pattern"));
    drop(calls);

    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let body: String = connection
        .query_row(
            "SELECT json_extract(p.payload_json,'$.body') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='FinalChapter'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!body.contains("倒吸一口凉气"));
    assert!(body.contains("林满没吭声"));
    let receipt: String = connection
        .query_row(
            "SELECT p.payload_json FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ProductionReceipt'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let receipt: Value = serde_json::from_str(&receipt).unwrap();
    assert!(
        receipt["craftSkills"]["proseReview"]["rhythmNotes"].is_array(),
        "{}",
        receipt["craftSkills"]
    );
    assert_eq!(receipt["craftSkills"]["proseReview"]["editsApplied"], 1);
}

#[test]
fn habits_and_looks_from_the_audit_reach_the_next_chapter_as_established_facts() {
    let (_temp, state, gateway, project_id) = project_ready_to_write("f4");
    write_next_chapter(&state, &project_id, "f4:production:1");
    write_next_chapter(&state, &project_id, "f4:production:2");

    let calls = gateway.calls.lock().unwrap();
    let inputs = |capability: &str| inputs(&calls, capability);
    let audits = inputs("AUDIT_EXTRACT");
    assert!(audits
        .iter()
        .all(|audit| audit["continuityDetailsWanted"] == true));
    let plans = inputs("PLAN_CHAPTER_CRAFT");
    assert_eq!(plans[0]["establishedFacts"], json!([]));
    let second_plan = plans.last().unwrap();
    assert_eq!(
        second_plan["establishedFacts"],
        json!([{"chapter":1,"subject":"林满","kind":"PREFERENCE","detail":"喝豆浆不加糖"}]),
        "only the quoted detail survives"
    );
    let renders = inputs("RENDER_CHAPTER_CRAFT");
    assert_eq!(renders[1]["establishedFacts"][0]["detail"], "喝豆浆不加糖");
}
