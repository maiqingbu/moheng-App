#![cfg(feature = "e2e-test")]

use ai_novel_production_system_core::commands::{
    execute_command_impl, query_assistant_repair_plan_impl, query_book_health_impl,
    query_project_library_impl, query_project_overview_impl, query_story_directions_impl,
    query_volume_list_impl, support_session_impl, KernelState,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::{json, Value};
use std::env;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

fn dispatch(state: &KernelState, command: &str, body: Value) -> Result<Value, String> {
    let request = body.get("request").cloned().unwrap_or(body.clone());
    if matches!(command, "support_session" | "query_assistant_repair_plan") {
        let question = request
            .get("question")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if question.contains("[R4_UNAVAILABLE]") {
            return Err("IPC_UNSUPPORTED:query_assistant_repair_plan".into());
        }
        if question.contains("[R4_BACKEND_ERROR]") {
            return Err("R4_TEST_BACKEND_ERROR".into());
        }
    }
    match command {
        "query_project_library" => query_project_library_impl(state),
        "query_project_overview" => query_project_overview_impl(state, request),
        "query_story_directions" => query_story_directions_impl(state, request),
        "query_volume_list" => query_volume_list_impl(state, request),
        "query_book_health" => query_book_health_impl(state, request),
        "support_session" => support_session_impl(state, request),
        "query_assistant_repair_plan" => query_assistant_repair_plan_impl(state, request),
        "execute_command" => execute_command_impl(state, request),
        other => Err(format!("IPC_UNSUPPORTED:{other}")),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = env::var_os("MOHENG_R4_FIXTURE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join(format!("moheng-r4-{}", std::process::id())));
    std::fs::create_dir_all(&root)?;
    let state = KernelState::new(AppPaths::new(root));
    let seed = execute_command_impl(
        &state,
        json!({
            "type":"project_create_from_idea",
            "commandId":"r4-seed-project",
            "idempotencyKey":"r4-seed-project-key",
            "requestedAt":"2026-09-08T00:00:00Z",
            "payload":{"title":"R4 Production E2E Fixture","idea":"用于正式助手生产链路验收的隔离测试作品"}
        }),
    )?;
    let project_id = seed["projectId"].as_str().unwrap_or_default().to_owned();
    let directions = query_story_directions_impl(
        &state,
        json!({"projectId": project_id, "scope":"directions", "pageSize":10}),
    )?;
    let pitch_id = directions["pitches"][0]["id"].as_str().unwrap_or_default();
    execute_command_impl(
        &state,
        json!({
            "type":"pitch_select","commandId":"r4-seed-pitch","idempotencyKey":"r4-seed-pitch-key",
            "projectId":project_id,"expectedRevision":0,"requestedAt":"2026-09-08T00:00:01Z",
            "payload":{"pitchIds":[pitch_id],"mode":"single"}
        }),
    )?;
    execute_command_impl(
        &state,
        json!({
            "type":"story_setup_save","commandId":"r4-seed-story","idempotencyKey":"r4-seed-story-key",
            "projectId":project_id,"expectedRevision":1,"requestedAt":"2026-09-08T00:00:02Z",
            "payload":{"storyCore":{"protagonist":"被逐出宗门的炼丹师","desire":"证明废丹的价值","mechanism":"废丹让妖兽进化并留下代价","conflict":"宗门封锁秘密","readerPromise":"每次升级都伴随选择与代价","forbiddenReveals":[]}}
        }),
    )?;
    println!(
        "{}",
        serde_json::to_string(&json!({"event":"ready","projectId":project_id}))?
    );
    io::stdout().flush()?;
    for line in io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let envelope: Value = serde_json::from_str(&line)?;
        let command = envelope["command"].as_str().unwrap_or_default();
        let body = envelope.get("payload").cloned().unwrap_or(Value::Null);
        let response = match dispatch(&state, command, body) {
            Ok(value) => json!({"ok":true,"value":value}),
            Err(error) => json!({"ok":false,"error":error}),
        };
        println!("{}", serde_json::to_string(&response)?);
        io::stdout().flush()?;
    }
    Ok(())
}
