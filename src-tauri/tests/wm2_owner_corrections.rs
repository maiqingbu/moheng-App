//! WM-2a (WORK_MEMORY_DESIGN §6.2, §8): the owner's corrections. A writing
//! rule holds for the whole work, a remembered reminder for the person or
//! thing it names; both reach every chapter's planner and writer and win
//! over anything else they are told.

mod craft_v3_support;

use ai_novel_production_system_core::commands::{query_owner_corrections_impl, KernelState};
use ai_novel_production_system_core::craft_plan::{ChapterPlanner, WritingContext};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use craft_v3_support::*;
use serde_json::{json, Value};
use std::sync::Arc;

struct SetupGateway;

impl ProductionModelGateway for SetupGateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        Ok(ResolvedProviderProfile {
            provider: "wm2-provider".into(),
            model: "wm2-model".into(),
            endpoint: "fixture://wm2".into(),
            timeout_ms: 1_000,
        })
    }

    fn invoke(&self, capability: &str, _input: &Value) -> Result<ProductionModelResponse, String> {
        let output = match capability {
            "GENERATE_STORY_DIRECTIONS" => directions_output(),
            "GENERATE_STORY_BIBLE" => json!({"bibles":[bible(), bible(), bible()]}),
            other => return Err(format!("unexpected capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

fn project() -> (tempfile::TempDir, KernelState, String) {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        Arc::new(SetupGateway),
    );
    let project_id = project_with_confirmed_bible(&state, "wm2");
    command(
        &state,
        &project_id,
        "wm2:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    (temp, state, project_id)
}

fn listed(state: &KernelState, project_id: &str) -> Vec<Value> {
    query_owner_corrections_impl(state, json!({"projectId":project_id})).unwrap()["items"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn wm2a_a_writing_rule_and_a_remembered_reminder_are_kept_until_revoked() {
    let (_temp, state, project_id) = project();
    let rule = command(
        &state,
        &project_id,
        "wm2:rule",
        "writing_rule_add",
        json!({"content":"不写主角吐血，代价用记忆缺失来表现"}),
    );
    command(
        &state,
        &project_id,
        "wm2:remember",
        "memory_correction_add",
        json!({"content":"老董从不喊林满的名字，只叫他账房","subject":"老董","decisionId":"decision:qc1"}),
    );
    let items = listed(&state, &project_id);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["subject"], Value::Null);
    assert_eq!(items[0]["content"], "不写主角吐血，代价用记忆缺失来表现");
    assert_eq!(items[1]["subject"], "老董");
    assert_eq!(items[1]["sourceDecisionId"], "decision:qc1");

    command(
        &state,
        &project_id,
        "wm2:revoke",
        "memory_correction_revoke",
        json!({"correctionId":rule["correctionId"]}),
    );
    let items = listed(&state, &project_id);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["subject"], "老董");
}

#[test]
fn wm2a_an_empty_correction_is_refused() {
    let (_temp, state, project_id) = project();
    let refused = ai_novel_production_system_core::commands::execute_command_impl(
        &state,
        json!({"type":"writing_rule_add","commandId":"wm2:empty","idempotencyKey":"wm2:empty:key",
            "projectId":project_id,"requestedAt":NOW,"payload":{"content":"  "}}),
    );
    assert_eq!(refused.unwrap_err(), "WM2_CORRECTION_CONTENT_REQUIRED");
}

#[test]
fn wm2a_the_planner_and_the_writer_are_given_the_corrections() {
    let (_temp, state, project_id) = project();
    command(
        &state,
        &project_id,
        "wm2:rule",
        "writing_rule_add",
        json!({"content":"不写主角吐血"}),
    );
    command(
        &state,
        &project_id,
        "wm2:remember",
        "memory_correction_add",
        json!({"content":"老董从不喊林满的名字","subject":"老董"}),
    );
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let chapter_id: String = connection
        .query_row(
            "SELECT chapter_id FROM chapters WHERE project_id=?1 ORDER BY chapter_no LIMIT 1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let expected = json!([
        {"subject":null,"content":"不写主角吐血"},
        {"subject":"老董","content":"老董从不喊林满的名字"}
    ]);
    let mut plan_input = json!({});
    ChapterPlanner::for_chapter(&connection, &project_id, &chapter_id, Some(1))
        .unwrap()
        .extend_input(&mut plan_input);
    assert_eq!(plan_input["ownerCorrections"], expected);
    let writer =
        WritingContext::for_chapter(&connection, &project_id, &chapter_id, Some(1)).unwrap();
    let mut render_input =
        json!({"chapterPlan":{"craft":{"scenes":[{"people":[person("老董")]}]}}});
    writer.extend_render_input(&mut render_input, &[]);
    assert_eq!(render_input["ownerCorrections"], expected);
    // The writer gets a correction about someone only when the chapter has them.
    let mut without = json!({"chapterPlan":{"craft":{"scenes":[{"people":[person("林满")]}]}}});
    writer.extend_render_input(&mut without, &[]);
    assert_eq!(
        without["ownerCorrections"],
        json!([{"subject":null,"content":"不写主角吐血"}])
    );
}

#[test]
fn wm2a_a_retried_command_records_its_correction_once() {
    let (_temp, state, project_id) = project();
    for _ in 0..2 {
        command(
            &state,
            &project_id,
            "wm2:retry",
            "writing_rule_add",
            json!({"content":"不写主角吐血"}),
        );
    }
    assert_eq!(listed(&state, &project_id).len(), 1);
}
