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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct CraftGateway {
    plan_inputs: Mutex<Vec<Value>>,
    /// The first this-many plans are too vague to pass the r2 checks.
    vague_plans: AtomicUsize,
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
                let forced = self
                    .vague_plans
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                        left.checked_sub(1)
                    })
                    .is_ok();
                let first_try_of_chapter_one = chapter == 1 && input.get("planFeedback").is_none();
                craft_plan(input, chapter, forced || first_try_of_chapter_one)
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

#[test]
fn a_retry_after_a_rejected_plan_plans_again_instead_of_reusing_the_opening_placeholder() {
    // Real run 2026-10-24: both plans of the first attempt were rejected, the
    // retry found the completed planner run and rebound the r1 placeholder
    // that opening preparation had committed.
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway {
        vague_plans: AtomicUsize::new(2),
        ..CraftGateway::default()
    });
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, "retry");
    command(
        &state,
        &project_id,
        "retry:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        "retry:start",
        "opening_start_creating",
        json!({}),
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let payload = json!({
        "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
        "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
    });
    let start = |id: &str, payload: Value| {
        ai_novel_production_system_core::commands::execute_command_impl(
            &state,
            json!({
                "type":"production_start","commandId":id,"idempotencyKey":format!("{id}:key"),
                "projectId":project_id,"expectedRevision":state.manager.project_revision(&project_id).unwrap(),
                "requestedAt":NOW,"payload":payload
            }),
        )
    };
    let error = start("retry:production:1", payload.clone()).unwrap_err();
    assert!(error.starts_with("B08_PLAN_CRAFT"), "{error}");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let failed_job: String = connection
        .query_row(
            "SELECT job_id FROM jobs WHERE project_id=?1 AND status='FAILED' ORDER BY created_at DESC, job_id DESC LIMIT 1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);
    let mut retry = payload;
    retry["retryOfJobId"] = json!(failed_job);
    assert_eq!(
        start("retry:production:2", retry).unwrap()["finalStatus"],
        "committed"
    );

    // Two rejected plans, then the retry plans again (its first try of chapter
    // one is vague in this fixture, so it replans once).
    assert_eq!(
        gateway.plan_inputs.lock().unwrap().len(),
        4,
        "the retry plans again"
    );
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let schema: String = connection
        .query_row(
            "SELECT json_extract(p.payload_json,'$.payloadSchema') FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(schema, "chapter-plan.r2");
}
