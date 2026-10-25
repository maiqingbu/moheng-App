//! LD-1 step 1: every model step has one output spec, used both to tell the
//! model the shape and to check its reply; a reply off the spec is sent back
//! once with what was wrong.

mod craft_v3_support;

use ai_novel_production_system_core::commands::{query_production_center_impl, KernelState};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use craft_v3_support::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// The first chapter text comes back as `text` instead of `body`.
#[derive(Default)]
struct OffSpecRenderGateway {
    render_inputs: Mutex<Vec<Value>>,
}

impl ProductionModelGateway for OffSpecRenderGateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        Ok(ResolvedProviderProfile {
            provider: "spec-fixture-provider".into(),
            model: "spec-fixture-model".into(),
            endpoint: "fixture://spec".into(),
            timeout_ms: 1_000,
        })
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let output = match capability {
            "GENERATE_STORY_DIRECTIONS" => directions_output(),
            "GENERATE_STORY_BIBLE" => json!({"bibles":[bible(), bible(), bible()]}),
            "PLAN_CHAPTER_CRAFT" => craft_plan(input, 1, false),
            "RENDER_CHAPTER_CRAFT" => {
                let mut inputs = self.render_inputs.lock().unwrap();
                inputs.push(input.clone());
                if inputs.len() == 1 {
                    json!({"text":"林满推开面馆的门，把账本放在了灶台边。"})
                } else {
                    json!({"body":"林满推开面馆的门，把账本放在了灶台边。"})
                }
            }
            "AUDIT_EXTRACT" => audit_no_change(),
            other => return Err(format!("unexpected capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

#[test]
fn a_chapter_text_off_the_spec_is_written_again_with_what_was_wrong() {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(OffSpecRenderGateway::default());
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, "spec");
    command(
        &state,
        &project_id,
        "spec:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        "spec:start",
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
        "spec:production",
        "production_start",
        json!({
            "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
            "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
        }),
    );
    assert_eq!(result["finalStatus"], "committed");
    let inputs = gateway.render_inputs.lock().unwrap();
    assert_eq!(inputs.len(), 2);
    let feedback = inputs[1]["outputFeedback"].to_string();
    assert!(feedback.contains("body"), "{feedback}");
}
