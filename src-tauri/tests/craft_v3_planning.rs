//! CRAFT v3 step 2c: a project with a confirmed story bible plans its
//! chapters as `chapter-plan.r2` with the bible and the creation ledger, the
//! r2 checks send problems back for one replan, and the production receipt
//! records which skill decided which plan field.

mod craft_v3_support;

use ai_novel_production_system_core::commands::{query_production_center_impl, KernelState};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use craft_v3_support::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct CraftGateway {
    plan_inputs: Mutex<Vec<Value>>,
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

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let output = match capability {
            "GENERATE_STORY_DIRECTIONS" => directions_output(),
            "GENERATE_STORY_BIBLE" => json!({"bibles":[bible(), bible(), bible()]}),
            "PLAN_CHAPTER_CRAFT" => {
                self.plan_inputs.lock().unwrap().push(input.clone());
                let chapter = input["chapterNumber"].as_i64().unwrap_or(1);
                let first_try_of_chapter_one = chapter == 1 && input.get("planFeedback").is_none();
                craft_plan(input, chapter, first_try_of_chapter_one)
            }
            "RENDER_CHAPTER_CRAFT" => json!({"body":"林满推开面馆的门，把账本放在了灶台边。"}),
            "AUDIT_EXTRACT" => audit_no_change(),
            other => return Err(format!("unexpected capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

#[test]
fn a_project_with_a_confirmed_story_bible_plans_r2_chapters_with_bible_and_ledger() {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway::default());
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, "v3");
    command(
        &state,
        &project_id,
        "v3:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        "v3:start",
        "opening_start_creating",
        json!({}),
    );

    for chapter in 1..=2 {
        let center = query_production_center_impl(
            &state,
            json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
        )
        .unwrap();
        let result = command(
            &state,
            &project_id,
            &format!("v3:production:{chapter}"),
            "production_start",
            json!({
                "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
                "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
            }),
        );
        assert_eq!(result["finalStatus"], "committed", "chapter {chapter}");
    }

    let inputs = gateway.plan_inputs.lock().unwrap();
    assert_eq!(
        inputs.len(),
        3,
        "chapter 1 is replanned once, chapter 2 passes first time"
    );
    let first = &inputs[0];
    assert_eq!(first["chapterNumber"], 1);
    assert_eq!(first["storyBible"]["antagonist"]["name"], "彭三");
    assert!(
        first["storyBible"].get("stage").is_none(),
        "the planner gets the bible content only"
    );
    assert_eq!(first["creationLedger"]["devices"], json!([]));
    let feedback = &inputs[1]["planFeedback"];
    assert_eq!(feedback["rejectedFor"], "B08_PLAN_CRAFT");
    assert!(feedback["problems"][0]
        .as_str()
        .unwrap()
        .contains("温度或幽默"));
    let second_chapter = &inputs[2];
    assert_eq!(second_chapter["chapterNumber"], 2);
    assert_eq!(
        second_chapter["creationLedger"]["devices"][0]["name"],
        "听旧物"
    );
    assert_eq!(
        second_chapter["creationLedger"]["lastChapterPlaces"],
        json!(["江边码头", "姑妈的面馆"])
    );
    drop(inputs);

    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let plan_schemas: Vec<String> = connection
        .prepare("SELECT json_extract(p.payload_json,'$.payloadSchema') FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan' AND h.scope_level='CHAPTER'")
        .unwrap()
        .query_map([&project_id], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        plan_schemas
            .iter()
            .filter(|schema| *schema == "chapter-plan.r2")
            .count()
            >= 2,
        "{plan_schemas:?}"
    );
    let receipt: String = connection
        .query_row(
            "SELECT p.payload_json FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ProductionReceipt' ORDER BY a.created_at, a.artifact_id LIMIT 1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let receipt: Value = serde_json::from_str(&receipt).unwrap();
    let decisions = receipt["craftSkills"]["chapterPlanDecisions"]
        .as_array()
        .expect("chapterPlanDecisions");
    let planned = receipt["craftSkills"]["chapterPlan"].as_array().unwrap();
    assert_eq!(
        decisions.len(),
        planned
            .iter()
            .filter(|id| decisions.iter().any(|d| d["skillId"] == **id))
            .count()
    );
    if let Some(golden) = decisions.iter().find(|item| item["skillId"] == "S13") {
        let win = golden["decisions"]["protagonistWin"].as_str().unwrap();
        assert!(win.ends_with("章林满当众把假账揪了出来"), "{win}");
    }
}
