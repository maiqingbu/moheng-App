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
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct CraftGateway {
    calls: Mutex<Vec<(String, Value)>>,
}

const TROPE_SENTENCE: &str = "林满倒吸一口凉气，转身进了面馆。";

/// A chapter in the default 2000–3000 range with one stock phrasing.
fn long_body() -> String {
    let mut lines = (1..=110)
        .map(|index| format!("货单第{index}页写着{index}箱桐油，老董按了手印。"))
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
        self.calls
            .lock()
            .unwrap()
            .push((capability.to_string(), input.clone()));
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
            "ADJUST_CHAPTER_LENGTH" => json!({"body":long_body()}),
            "REWRITE_PASSAGES" => {
                json!({"edits":[{"quote":TROPE_SENTENCE,"replacement":"林满没吭声，转身进了面馆。"}]})
            }
            "AUDIT_EXTRACT" => audit_no_change(),
            other => return Err(format!("unexpected capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

#[test]
fn an_r2_chapter_is_written_whole_with_bible_and_style_card_then_sized_and_edited() {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway::default());
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, "w3");
    command(
        &state,
        &project_id,
        "w3:style",
        "style_card_generate",
        json!({"source":"MARKET_ARCHETYPE","primary":"A2","secondary":"A1"}),
    );
    let card = query_craft_setup_impl(&state, json!({"projectId":project_id})).unwrap()
        ["styleCard"]["candidates"][0]
        .clone();
    command(
        &state,
        &project_id,
        "w3:style-confirm",
        "style_card_confirm",
        json!({"candidateId":card["id"],"card":card}),
    );
    command(
        &state,
        &project_id,
        "w3:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        "w3:start",
        "opening_start_creating",
        json!({}),
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let result = command(
        &state,
        &project_id,
        "w3:production:1",
        "production_start",
        json!({
            "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
            "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
        }),
    );
    assert_eq!(result["finalStatus"], "committed");

    let calls = gateway.calls.lock().unwrap();
    let call = |capability: &str| {
        calls
            .iter()
            .find(|(name, _)| name == capability)
            .map(|(_, input)| input.clone())
    };
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
    assert!(adjust["shortBy"].as_i64().unwrap() > 0);
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
