use ai_novel_production_system_core::commands::{
    execute_command_impl, query_market_strategy_impl, query_opening_ready_impl,
    query_project_overview_impl, query_story_directions_impl, query_story_setup_impl, KernelState,
};
use ai_novel_production_system_core::production::UnavailableProductionGateway;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::json;
use std::sync::Arc;

const NOW: &str = "2026-09-02T02:00:00Z";

fn state() -> KernelState {
    let temp = tempfile::tempdir().unwrap();
    // Keep the temporary directory alive for the duration of each test by
    // leaking only this test-owned path; the project manager owns the files.
    let root = Box::leak(Box::new(temp)).path().to_path_buf();
    KernelState::with_production_gateway(
        AppPaths::new(root),
        Arc::new(UnavailableProductionGateway),
    )
}

fn idea_request(command_id: &str) -> serde_json::Value {
    json!({
        "type":"project_create_from_idea",
        "commandId":command_id,
        "idempotencyKey":format!("{command_id}:key"),
        "requestedAt":NOW,
        "payload":{
            "title":"断剑与改名者",
            "idea":"一个被逐出宗门的炼丹师发现废丹能让妖兽进化。",
            "platform":"起点中文网",
            "mode":"DIRECTOR",
            "tags":["武侠","悬疑"],
            "forbidden":"不要悲剧结局"
        }
    })
}

#[test]
fn b11_idea_entry_persists_three_substantive_candidates_without_auto_selection() {
    let state = state();
    let created = execute_command_impl(&state, idea_request("cmd:b11:idea")).unwrap();
    assert_eq!(created["finalStatus"], "committed");
    let project_id = created["projectId"].as_str().unwrap().to_string();

    let directions = query_story_directions_impl(
        &state,
        json!({"projectId":project_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    let pitches = directions["pitches"].as_array().unwrap();
    assert_eq!(pitches.len(), 3);
    assert!(directions["selectedPitchId"].is_null());
    assert!(pitches.iter().all(|pitch| {
        pitch["sourceInputDigest"].as_str().is_some()
            && pitch["createdAt"] == NOW
            && pitch["authoritative"] == false
    }));
    assert!(pitches
        .windows(2)
        .all(|pair| pair[0]["mechanism"] != pair[1]["mechanism"]));
    assert!(pitches
        .windows(2)
        .all(|pair| pair[0]["worldConflict"] != pair[1]["worldConflict"]));

    state
        .manager
        .with_read_connection(&project_id, NOW, |connection| {
            let canonical_selection: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM canonical_heads WHERE project_id=?1 AND artifact_type='StoryBlueprint'",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            let canon_facts: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(canonical_selection, 0);
            assert_eq!(canon_facts, 0);
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();
}

#[test]
fn b11_market_entry_requires_user_material_and_keeps_market_evidence_out_of_canon() {
    let state = state();
    let mut request = idea_request("cmd:b11:market-missing");
    request["type"] = json!("project_create_from_market");
    request["payload"] = json!({"title":"没有资料的市场入口"});
    assert_eq!(
        execute_command_impl(&state, request).unwrap_err(),
        "B11_MARKET_SOURCE_REQUIRED"
    );

    let created = execute_command_impl(&state, json!({
        "type":"project_create_from_market",
        "commandId":"cmd:b11:market",
        "idempotencyKey":"cmd:b11:market:key",
        "requestedAt":NOW,
        "payload":{
            "title":"废丹进化录",
            "marketSource":"用户导入的起点题材观察表",
            "marketMaterial":"近期开篇偏好：强目标、明确代价、首章出现可验证异常；仅作为市场资料，不是故事规则。"
        }
    })).unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_string();
    let market = query_market_strategy_impl(
        &state,
        json!({"projectId":project_id,"scope":"market","pageSize":10}),
    )
    .unwrap();
    let opportunities = market["opportunities"].as_array().unwrap();
    assert_eq!(opportunities.len(), 3);
    assert!(opportunities.iter().all(|item| {
        item["source"] == "用户导入的起点题材观察表"
            && item["collectedAt"] == NOW
            && item["provenance"]["inputDigest"].as_str().is_some()
    }));
    state
        .manager
        .with_read_connection(&project_id, NOW, |connection| {
            let market_evidence: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='MarketEvidenceBundle'",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            let canon_facts: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(market_evidence >= 1);
            assert_eq!(canon_facts, 0);
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();
}

#[test]
fn b11_selection_setup_and_opening_ready_require_explicit_user_steps() {
    let state = state();
    let created = execute_command_impl(&state, idea_request("cmd:b11:flow")).unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_string();
    let directions = query_story_directions_impl(
        &state,
        json!({"projectId":project_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    let selected_pitch_id = directions["pitches"][0]["id"].as_str().unwrap();
    let revision = query_project_overview_impl(&state, json!({"projectId":project_id})).unwrap()
        ["project"]["revision"]
        .as_i64()
        .unwrap();

    let selected = execute_command_impl(
        &state,
        json!({
            "type":"pitch_select",
            "commandId":"cmd:b11:select",
            "idempotencyKey":"cmd:b11:select:key",
            "projectId":project_id,
            "expectedRevision":revision,
            "requestedAt":NOW,
            "payload":{"pitchIds":[selected_pitch_id],"mode":"single"}
        }),
    )
    .unwrap();
    assert_eq!(selected["finalStatus"], "committed");
    assert!(selected["receipt"]["receiptId"].as_str().is_some());

    let setup = query_story_setup_impl(
        &state,
        json!({"projectId":project_id,"scope":"story-setup"}),
    )
    .unwrap();
    assert_eq!(setup["selectedPitchId"], selected_pitch_id);
    assert_eq!(setup["status"], "SELECTED");

    let revision = query_project_overview_impl(&state, json!({"projectId":project_id})).unwrap()
        ["project"]["revision"]
        .as_i64()
        .unwrap();
    execute_command_impl(
        &state,
        json!({
            "type":"story_setup_save",
            "commandId":"cmd:b11:setup",
            "idempotencyKey":"cmd:b11:setup:key",
            "projectId":project_id,
            "expectedRevision":revision,
            "requestedAt":NOW,
            "payload":{"storyCore":{
                "protagonist":"被逐出宗门的炼丹师",
                "desire":"证明废丹不是废物并找回师门尊严",
                "mechanism":"废丹会让妖兽进化，但每次进化都会留下可追踪代价",
                "conflict":"宗门想封锁进化秘密，妖兽群开始反向学习炼丹术",
                "worldRules":["进化必须支付代价"],
                "readerPromise":"每次升级都伴随新的选择与代价",
                "forbiddenReveals":["前三章不揭开秘方来源"],
                "style":"紧凑、悬疑、带成长回报",
                "expandableDirections":["宗门旧案","妖兽文明"]
            }}
        }),
    )
    .unwrap();

    let revision = query_project_overview_impl(&state, json!({"projectId":project_id})).unwrap()
        ["project"]["revision"]
        .as_i64()
        .unwrap();
    let ready = execute_command_impl(
        &state,
        json!({
            "type":"opening_ready",
            "commandId":"cmd:b11:opening",
            "idempotencyKey":"cmd:b11:opening:key",
            "projectId":project_id,
            "expectedRevision":revision,
            "requestedAt":NOW,
            "payload":{"approved":true}
        }),
    )
    .unwrap();
    assert_eq!(ready["finalStatus"], "committed");
    let opening =
        query_opening_ready_impl(&state, json!({"projectId":project_id,"scope":"opening"}))
            .unwrap();
    assert_eq!(opening["ready"], true);
    assert!(opening["firstChapterPlan"]["artifactId"].as_str().is_some());
    assert_eq!(opening["firstChapterPlan"]["owner"], "B07::PlanningOwner");
}
