//! Real Tauri command-dispatch E2E.
//!
//! Unlike `tauri_ipc_smoke`, this test drives the payload through the actual
//! `#[tauri::command]` handler registered by `commands::handler()`, on the
//! Tauri `MockRuntime`. That proves the frontend-facing dispatch chain
//! (`invoke -> generate_handler -> command state injection -> serialized
//! response`) rather than only calling the internal `*_impl` functions.
//!
//! The window is never shown and no WebView/WebDriver is required, so it runs
//! deterministically on CI and locally.

use ai_novel_production_system_core::commands::{self, KernelState};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::market_source::{
    FanqieWriterZoneAdapter, MarketSourceAdapter,
};
use ai_novel_production_system_core::production::ConfiguredProductionGateway;
use ai_novel_production_system_core::project_db::connection::open_connection;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::provider::{OpenAiCompatibleProvider, ProviderConfig};
use ai_novel_production_system_core::repositories::provider::ProviderSettingsRepository;
use ai_novel_production_system_core::repositories::runtime::{
    MemorySecretStore, OsKeychainSecretStore, SecretStore,
};
use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::ipc;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;
use tempfile::tempdir;

/// Builds a Tauri `App<MockRuntime>` wired exactly like the production host:
/// a managed `KernelState` plus the real `commands::handler()`.
fn build_app() -> (tauri::App<tauri::test::MockRuntime>, tempfile::TempDir) {
    let temp = tempdir().unwrap();
    let app = build_app_over(temp.path());
    (app, temp)
}

/// Builds the app over an explicit storage root, so a "restart" can reopen the
/// SAME on-disk catalog + project DBs.
fn build_app_over(root: &std::path::Path) -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(KernelState::new(AppPaths::new(root.to_path_buf())))
        .invoke_handler(commands::handler())
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn build_app_over_with_market_source(
    root: &std::path::Path,
    market_source: Arc<dyn MarketSourceAdapter>,
) -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(
            KernelState::new(AppPaths::new(root.to_path_buf()))
                .with_market_source_adapter(market_source),
        )
        .invoke_handler(commands::handler())
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn build_app_with_state(state: KernelState) -> tauri::App<tauri::test::MockRuntime> {
    mock_builder()
        .manage(state)
        .invoke_handler(commands::handler())
        .build(mock_context(noop_assets()))
        .unwrap()
}

fn desktop_provider_config() -> ProviderConfig {
    let root = std::env::var_os("MOHENG_REAL_APP_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                PathBuf::from(home)
                    .join("Library/Application Support/com.moheng.ai-novel-production-system")
            })
        })
        .expect("desktop provider settings root unavailable");
    let settings = ProviderSettingsRepository::read_global(&AppPaths::new(root))
        .expect("desktop provider settings unavailable");
    assert!(
        settings.get("apiKey").is_none(),
        "provider settings must not persist secrets"
    );
    let config =
        ProviderConfig::from_settings(&settings).expect("configured provider profile required");
    assert!(config.enabled, "configured provider must be enabled");
    let secrets = OsKeychainSecretStore::new("com.moheng.ai-novel-production", "provider-api-key");
    let canary_check = secrets
        .get(&config.provider_id)
        .expect("configured provider secret must be available in Keychain");
    assert!(!canary_check.trim().is_empty());
    drop(canary_check);
    config
}

fn live_market_project_state(root: &std::path::Path, config: &ProviderConfig) -> KernelState {
    let gateway = Arc::new(ConfiguredProductionGateway::new(
        config.clone(),
        Arc::new(OpenAiCompatibleProvider::new(config.endpoint.clone())),
        Arc::new(OsKeychainSecretStore::new(
            "com.moheng.ai-novel-production",
            "provider-api-key",
        )),
    ));
    KernelState::with_production_gateway(AppPaths::new(root.to_path_buf()), gateway)
        .with_market_source_adapter(Arc::new(FanqieWriterZoneAdapter))
}

/// Builds the single "main" webview for an app, once, then reuses it for every
/// invoke (the mock runtime forbids creating two webviews with the same label).
fn webview(
    app: &tauri::App<tauri::test::MockRuntime>,
) -> tauri::WebviewWindow<tauri::test::MockRuntime> {
    tauri::WebviewWindowBuilder::new(app, "main", Default::default())
        .build()
        .unwrap()
}

/// Runs an `invoke` against the app's webview and deserializes the response.
fn invoke(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    command: &str,
    body: Value,
) -> Value {
    let request = InvokeRequest {
        cmd: command.into(),
        callback: ipc::CallbackFn(0),
        error: ipc::CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(),
        body: ipc::InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: tauri::test::INVOKE_KEY.to_string(),
    };
    let response = tauri::test::get_ipc_response(webview, request).expect("invoke should succeed");
    response
        .deserialize::<Value>()
        .expect("response body should be JSON")
}

/// Runs an invoke while preserving the Tauri error so protocol tests can assert
/// fail-closed revision/idempotency behavior.
fn try_invoke(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    command: &str,
    body: Value,
) -> Result<Value, String> {
    let request = InvokeRequest {
        cmd: command.into(),
        callback: ipc::CallbackFn(0),
        error: ipc::CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(),
        body: ipc::InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: tauri::test::INVOKE_KEY.to_string(),
    };
    let response =
        tauri::test::get_ipc_response(webview, request).map_err(|error| format!("{error:?}"))?;
    response
        .deserialize::<Value>()
        .map_err(|error| format!("{error:?}"))
}

fn receipt_project_id(receipt: &Value) -> &str {
    receipt["projectId"]
        .as_str()
        .expect("receipt has projectId")
}

#[test]
fn real_ipc_project_commands_are_idempotent_revision_checked_and_receipted() {
    let temp = tempdir().unwrap();
    let app = build_app_over(temp.path());
    let w = webview(&app);

    let create_request = json!({
        "request": {
            "type": "project_create_from_idea",
            "commandId": "pc-create",
            "idempotencyKey": "pc-create-key",
            "requestedAt": "2026-08-25T00:00:00Z",
            "expectedRevision": 0,
            "payload": {"title": "命令协议"}
        }
    });
    let created = invoke(&w, "execute_command", create_request.clone());
    assert_eq!(created["finalStatus"], "committed");
    assert_eq!(created["afterRevision"], 0);
    assert!(created["requestDigest"].as_str().is_some());
    let project_id = receipt_project_id(&created).to_string();

    // A retry after an IPC timeout must return the exact persisted result.
    let replay = invoke(&w, "execute_command", create_request);
    assert_eq!(replay, created);

    let rename_request = json!({
        "request": {
            "type": "project_rename",
            "commandId": "pc-rename",
            "idempotencyKey": "pc-rename-key",
            "projectId": project_id,
            "expectedRevision": 0,
            "requestedAt": "2026-08-25T00:00:01Z",
            "payload": {"title": "命令协议已重命名"}
        }
    });
    let renamed = invoke(&w, "execute_command", rename_request.clone());
    assert_eq!(renamed["afterRevision"], 1);
    let rename_replay = invoke(&w, "execute_command", rename_request);
    assert_eq!(rename_replay, renamed);

    let stale = try_invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_rename",
                "commandId": "pc-stale",
                "idempotencyKey": "pc-stale-key",
                "projectId": project_id,
                "expectedRevision": 0,
                "requestedAt": "2026-08-25T00:00:02Z",
                "payload": {"title": "不应写入"}
            }
        }),
    )
    .unwrap_err();
    assert!(stale.contains("REVISION_CONFLICT"), "{stale}");

    let idempotency_conflict = try_invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_rename",
                "commandId": "pc-conflict",
                "idempotencyKey": "pc-rename-key",
                "projectId": project_id,
                "expectedRevision": 1,
                "requestedAt": "2026-08-25T00:00:03Z",
                "payload": {"title": "同 key 不同请求"}
            }
        }),
    )
    .unwrap_err();
    assert!(
        idempotency_conflict.contains("IDEMPOTENCY_CONFLICT"),
        "{idempotency_conflict}"
    );

    drop(w);
    drop(app);
    let connection = open_connection(
        &temp
            .path()
            .join("projects")
            .join(&project_id)
            .join("project.db"),
        true,
    )
    .unwrap();
    let receipt_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM project_command_receipts WHERE project_id = ?1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(receipt_count, 2);
}

#[test]
#[ignore = "requires live access to the official Fanqie public writer-zone source; no mock adapter"]
fn real_ipc_live_market_refresh_uses_registered_tauri_command_and_survives_app_reopen() {
    let temp = tempdir().unwrap();
    let source: Arc<dyn MarketSourceAdapter> = Arc::new(FanqieWriterZoneAdapter);
    let app = build_app_over_with_market_source(temp.path(), source.clone());
    let w = webview(&app);

    let discovery = invoke(&w, "query_market_opportunities", json!({}));
    assert_eq!(discovery["status"], "READY", "{discovery}");
    assert!(!discovery["opportunities"].as_array().unwrap().is_empty());

    let created = invoke(
        &w,
        "execute_command",
        json!({"request":{
            "type":"project_create_from_idea",
            "commandId":"r2-04a-real-ipc-create",
            "idempotencyKey":"r2-04a-real-ipc-create:key",
            "requestedAt":"2026-10-03T09:00:00Z",
            "expectedRevision":0,
            "payload":{"title":"真实来源 Tauri IPC 验证","idea":"只验证公开市场来源正式链。"}
        }}),
    );
    assert_eq!(created["finalStatus"], "committed", "{created}");
    let project_id = receipt_project_id(&created).to_owned();

    let receipt = invoke(
        &w,
        "execute_command",
        json!({"request":{
            "type":"market_refresh",
            "commandId":"r2-04a-real-ipc-market-refresh",
            "idempotencyKey":"r2-04a-real-ipc-market-refresh:key",
            "projectId":project_id,
            "expectedRevision":0,
            "requestedAt":"2026-10-03T09:00:01Z",
            "payload":{"platformId":"fanqie"}
        }}),
    );
    assert_eq!(receipt["finalStatus"], "committed", "{receipt}");
    assert!(receipt["receiptId"].as_str().is_some(), "{receipt}");
    assert_eq!(receipt["sourceStatus"], "READY", "{receipt}");

    let first = invoke(
        &w,
        "query_market_strategy",
        json!({"request":{"projectId":project_id,"scope":"market","pageSize":20}}),
    );
    assert_eq!(first["status"], "READY", "{first}");
    assert!(first["source"]["digest"].as_str().is_some(), "{first}");
    assert_eq!(first["selectedOpportunityId"], Value::Null);
    drop(w);
    drop(app);

    let reopened = build_app_over_with_market_source(temp.path(), source);
    let reopened_window = webview(&reopened);
    let after_reopen = invoke(
        &reopened_window,
        "query_market_strategy",
        json!({"request":{"projectId":project_id,"scope":"market","pageSize":20}}),
    );
    assert_eq!(after_reopen["source"]["digest"], first["source"]["digest"]);
    assert_eq!(after_reopen["opportunities"], first["opportunities"]);
    assert_eq!(after_reopen["selectedOpportunityId"], Value::Null);
}

#[test]
#[ignore = "one configured paid provider request; requires explicit R2-04A live-provider acknowledgement"]
fn real_ipc_live_market_project_generates_and_selects_pitch_then_survives_app_reopen() {
    assert_eq!(
        std::env::var("MOHENG_RUN_R2_04A_REAL_DIRECTION").as_deref(),
        Ok("1"),
        "set MOHENG_RUN_R2_04A_REAL_DIRECTION=1 to acknowledge one configured provider request"
    );
    let provider_config = desktop_provider_config();
    let temp = tempdir().unwrap();
    let root = temp.path().to_path_buf();
    let state = live_market_project_state(&root, &provider_config);
    let inspector = state.manager.clone();
    let app = build_app_with_state(state);
    let w = webview(&app);

    // Discover from the real public source and explicitly choose one observed topic.
    let discovery = invoke(&w, "query_market_opportunities", json!({}));
    assert_eq!(discovery["status"], "READY", "{discovery}");
    let observed_opportunity = discovery["opportunities"]
        .as_array()
        .and_then(|items| items.first())
        .expect("live source must expose at least one publication category");
    let selected_topic_id = observed_opportunity["id"].as_str().unwrap().to_owned();
    let run_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let create_command = format!("r2-04a-live-market-create-{run_id}");
    let created = invoke(
        &w,
        "execute_command",
        json!({"request":{
            "type":"project_create_from_market",
            "commandId":create_command,
            "idempotencyKey":format!("{create_command}:key"),
            "requestedAt":"2026-10-03T11:40:00Z",
            "payload":{
                "title":format!("真实市场来源方向验证 {run_id}"),
                "marketOpportunityId":selected_topic_id,
                "platform":"番茄小说",
                "mode":"DIRECTOR",
                "length":"40–80 万字",
                "tags":["玄幻脑洞"]
            }
        }}),
    );
    assert_eq!(created["finalStatus"], "committed", "{created}");
    assert!(created["receiptId"].as_str().is_some(), "{created}");
    let project_id = receipt_project_id(&created).to_owned();
    let market = invoke(
        &w,
        "query_market_strategy",
        json!({"request":{"projectId":project_id,"scope":"market","pageSize":20}}),
    );
    assert_eq!(market["sourceStatus"], "READY", "{market}");
    assert_eq!(market["selectedOpportunityId"], selected_topic_id);
    assert!(market["source"]["digest"].as_str().is_some(), "{market}");
    assert!(
        market["source"]["observedAtEpochMs"].as_i64().is_some(),
        "{market}"
    );
    assert!(
        !market["policyReferences"].as_array().unwrap().is_empty(),
        "{market}"
    );

    let before_generation_story = invoke(
        &w,
        "query_story_setup",
        json!({"request":{"projectId":project_id,"scope":"story-setup","pageSize":1}}),
    );
    assert_eq!(before_generation_story["status"], "NOT_SELECTED");
    inspector.with_read_connection(&project_id, "2026-10-03T11:40:00Z", |connection| {
        let story_heads: i64 = connection.query_row(
            "SELECT COUNT(*) FROM canonical_heads WHERE project_id=?1 AND artifact_type='StoryBlueprint'",
            [&project_id], |row| row.get(0),
        ).unwrap();
        let canon_facts: i64 = connection.query_row(
            "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1", [&project_id], |row| row.get(0),
        ).unwrap();
        assert_eq!(story_heads, 0);
        assert_eq!(canon_facts, 0);
        Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
    }).unwrap();

    // This is the single live model request in this scenario. An unknown provider
    // outcome is surfaced; the test deliberately does not retry a paid request.
    let overview = invoke(
        &w,
        "query_project_overview",
        json!({"request":{"projectId":project_id,"scope":"overview","pageSize":8}}),
    );
    let directions_command = format!("r2-04a-live-market-directions-{run_id}");
    let generated = invoke(
        &w,
        "execute_command",
        json!({"request":{
            "type":"story_directions_generate",
            "commandId":directions_command,
            "idempotencyKey":format!("{directions_command}:key"),
            "projectId":project_id,
            "expectedRevision":overview["project"]["revision"],
            "requestedAt":"2026-10-03T11:40:01Z",
            "payload":{}
        }}),
    );
    assert_eq!(generated["finalStatus"], "committed", "{generated}");
    assert!(generated["receiptId"].as_str().is_some(), "{generated}");

    let candidates = invoke(
        &w,
        "query_story_directions",
        json!({"request":{"projectId":project_id,"scope":"directions","pageSize":10}}),
    );
    let pitches = candidates["pitches"]
        .as_array()
        .expect("direction query must return pitches");
    assert_eq!(pitches.len(), 3, "{candidates}");
    let pitch_ids = pitches
        .iter()
        .map(|pitch| pitch["id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let overview = invoke(
        &w,
        "query_project_overview",
        json!({"request":{"projectId":project_id,"scope":"overview","pageSize":8}}),
    );
    let selection_command = format!("r2-04a-live-market-pitch-select-{run_id}");
    let selected = invoke(
        &w,
        "execute_command",
        json!({"request":{
            "type":"pitch_select",
            "commandId":selection_command,
            "idempotencyKey":format!("{selection_command}:key"),
            "projectId":project_id,
            "expectedRevision":overview["project"]["revision"],
            "requestedAt":"2026-10-03T11:40:02Z",
            "payload":{"pitchIds":[pitch_ids[0]]}
        }}),
    );
    assert_eq!(selected["finalStatus"], "committed", "{selected}");
    assert!(selected["receiptId"].as_str().is_some(), "{selected}");
    let story_setup = invoke(
        &w,
        "query_story_setup",
        json!({"request":{"projectId":project_id,"scope":"story-setup","pageSize":1}}),
    );
    assert_eq!(story_setup["status"], "SELECTED", "{story_setup}");
    assert_eq!(story_setup["selectedPitchId"], pitch_ids[0]);
    let decisions = invoke(
        &w,
        "query_decisions",
        json!({"request":{"projectId":project_id,"scope":"decisions","pageSize":50}}),
    );
    let selection_decision = decisions["items"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["decisionType"] == "STORY_DIRECTION_SELECTION")
        })
        .expect("explicit pitch choice must have a queryable Decision");
    let selection_decision_id = selection_decision["id"].as_str().unwrap().to_owned();

    let artifact_refs = inspector.with_read_connection(&project_id, "2026-10-03T11:40:02Z", |connection| {
        let mut statement = connection.prepare(
            "SELECT artifact_type,artifact_id,version,payload_hash FROM artifacts WHERE project_id=?1 AND artifact_status='COMMITTED' AND artifact_type IN ('MarketSnapshot','MarketEvidenceBundle','GenreOpportunity','PlatformPolicy','MarketStrategyContract','ProjectPitch','StoryBlueprint') ORDER BY artifact_type,artifact_id,version"
        )?;
        let rows = statement.query_map([&project_id], |row| Ok(json!({
            "artifactType":row.get::<_,String>(0)?,
            "artifactId":row.get::<_,String>(1)?,
            "version":row.get::<_,i64>(2)?,
            "payloadHash":row.get::<_,String>(3)?
        })))?;
        let values = rows.collect::<Result<Vec<_>,_>>()?;
        Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(values)
    }).unwrap();
    assert!(artifact_refs
        .iter()
        .any(|reference| reference["artifactType"] == "MarketSnapshot"));
    assert!(artifact_refs
        .iter()
        .any(|reference| reference["artifactType"] == "MarketEvidenceBundle"));
    assert!(artifact_refs
        .iter()
        .any(|reference| reference["artifactType"] == "GenreOpportunity"));
    assert!(artifact_refs
        .iter()
        .any(|reference| reference["artifactType"] == "PlatformPolicy"));
    assert!(artifact_refs
        .iter()
        .any(|reference| reference["artifactType"] == "StoryBlueprint"));
    let first_source_digest = market["source"]["digest"].as_str().unwrap().to_owned();
    let first_source_time = market["source"]["observedAtEpochMs"].as_i64().unwrap();
    let first_policy_status = market["policyStatus"].as_str().unwrap().to_owned();
    let market_create_receipt = created["receiptId"].as_str().unwrap().to_owned();
    let directions_receipt = generated["receiptId"].as_str().unwrap().to_owned();
    let selection_receipt = selected["receiptId"].as_str().unwrap().to_owned();
    drop(w);
    drop(app);

    let reopened = build_app_with_state(live_market_project_state(&root, &provider_config));
    let reopened_window = webview(&reopened);
    let after_reopen_market = invoke(
        &reopened_window,
        "query_market_strategy",
        json!({"request":{"projectId":project_id,"scope":"market","pageSize":20}}),
    );
    let after_reopen_directions = invoke(
        &reopened_window,
        "query_story_directions",
        json!({"request":{"projectId":project_id,"scope":"directions","pageSize":10}}),
    );
    let after_reopen_story = invoke(
        &reopened_window,
        "query_story_setup",
        json!({"request":{"projectId":project_id,"scope":"story-setup","pageSize":1}}),
    );
    let after_reopen_decisions = invoke(
        &reopened_window,
        "query_decisions",
        json!({"request":{"projectId":project_id,"scope":"decisions","pageSize":50}}),
    );
    assert_eq!(after_reopen_market["source"]["digest"], first_source_digest);
    assert_eq!(
        after_reopen_market["source"]["observedAtEpochMs"],
        first_source_time
    );
    assert_eq!(
        after_reopen_market["selectedOpportunityId"],
        selected_topic_id
    );
    assert_eq!(after_reopen_directions["pitches"][0]["id"], pitch_ids[0]);
    assert_eq!(after_reopen_story["status"], "SELECTED");
    assert_eq!(after_reopen_story["selectedPitchId"], pitch_ids[0]);
    assert!(after_reopen_decisions["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == selection_decision_id));
    println!(
        "R2_04A_W02_E3_E4={}",
        json!({
            "sourceId":market["source"]["sourceId"],
            "sourceUrl":market["source"]["sourceUrl"],
            "sourceDigest":first_source_digest,
            "observedAtEpochMs":first_source_time,
            "policyStatus":first_policy_status,
            "selectedOpportunityId":selected_topic_id,
            "projectId":project_id,
            "marketCreateReceiptId":market_create_receipt,
            "artifactRefs":artifact_refs,
            "candidateCount":pitch_ids.len(),
            "selectedPitchId":pitch_ids[0],
            "directionsReceiptId":directions_receipt,
            "selectionReceiptId":selection_receipt,
            "selectionDecisionId":selection_decision_id,
            "storyAuthorityStatusAfterExplicitSelection":after_reopen_story["status"],
            "sourceDigestAfterReopen":after_reopen_market["source"]["digest"],
            "providerId":provider_config.provider_id,
            "modelId":provider_config.model_id,
            "liveProviderCalls":1,
            "realUserDatabaseUsed":false
        })
    );
}

#[test]
fn real_ipc_create_query_rename_archive_restore_and_delete() {
    let (app, _temp) = build_app();
    let w = webview(&app);

    // 1. project_create_from_idea through the real command dispatch.
    let created = invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_create_from_idea",
                "commandId": "c1",
                "idempotencyKey": "c1-key",
                "requestedAt": "2026-08-25T00:00:00Z",
                "payload": {"title": "第一部"}
            }
        }),
    );
    assert_eq!(created["finalStatus"], "committed");
    assert!(created["receiptId"].as_str().is_some());
    let project_id = receipt_project_id(&created);

    // 2. Library reflects the new project.
    let library = invoke(&w, "query_project_library", json!({}));
    assert_eq!(library["projects"].as_array().unwrap().len(), 1);
    assert_eq!(library["projects"][0]["id"], project_id);

    // 3. Overview reads title/revision/lifecycle from the project DB.
    let overview = invoke(
        &w,
        "query_project_overview",
        json!({"request": {"projectId": project_id, "scope": "overview", "pageSize": 8}}),
    );
    assert_eq!(overview["project"]["title"], "第一部");
    assert_eq!(overview["project"]["revision"], 0);

    // 4. Rename and archive through real dispatch.
    invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_rename",
                "commandId": "c2",
                "idempotencyKey": "c2-key",
                "projectId": project_id,
                "expectedRevision": 0,
                "requestedAt": "2026-08-25T00:00:01Z",
                "payload": {"title": "第二部"}
            }
        }),
    );
    invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_archive",
                "commandId": "c3",
                "idempotencyKey": "c3-key",
                "projectId": project_id,
                "expectedRevision": 1,
                "requestedAt": "2026-08-25T00:00:02Z"
            }
        }),
    );
    let archived = invoke(&w, "query_project_library", json!({}));
    assert_eq!(archived["projects"][0]["title"], "第二部");
    assert_eq!(archived["projects"][0]["lifecycle"], "ARCHIVED");

    // 5. Restore the archive through real dispatch.
    invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_restore",
                "commandId": "c4",
                "idempotencyKey": "c4-key",
                "projectId": project_id,
                "expectedRevision": 2,
                "requestedAt": "2026-08-25T00:00:03Z"
            }
        }),
    );
    let restored = invoke(&w, "query_project_library", json!({}));
    assert_eq!(restored["projects"][0]["lifecycle"], "ACTIVE");
    assert_eq!(restored["projects"][0]["title"], "第二部");
}

fn f01_create_cost_fixture_project(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    command_id: &str,
    title: &str,
) -> String {
    let receipt = invoke(
        webview,
        "execute_command",
        json!({
            "request": {
                "type": "project_create_from_idea",
                "commandId": command_id,
                "idempotencyKey": format!("{command_id}:key"),
                "requestedAt": "2026-10-04T00:00:00Z",
                "payload": {"title": title}
            }
        }),
    );
    assert_eq!(receipt["finalStatus"], "committed");
    receipt_project_id(&receipt).to_string()
}

/// Seeds only the synthetic test fixture. The business query itself is invoked
/// through the registered Tauri command below, never through a frontend mock.
fn f01_seed_project_cost_cents(root: &Path, project_id: &str, cents: i64) {
    let db = root.join("projects").join(project_id).join("project.db");
    let connection = open_connection(&db, false).expect("open synthetic project fixture");
    let model_run_id = format!("mr:f01:{project_id}");
    let idempotency_key = format!("f01-fixture:{project_id}");
    connection
        .execute(
            "INSERT INTO model_runs(model_run_id,project_id,provider,model,idempotency_key,capability,predicted_cost_cents,reserved_cost_cents,status,created_at,updated_at) VALUES (?1,?2,'f01-synthetic','fixture',?3,'RENDER_CHAPTER',?4,?4,'COMPLETED','2026-10-04T00:00:01Z','2026-10-04T00:00:01Z')",
            libsql_rusqlite::params![model_run_id, project_id, idempotency_key, cents],
        )
        .expect("seed synthetic ModelRun");
    connection
        .execute(
            "INSERT INTO model_usage(usage_id,project_id,model_run_id,input_tokens,output_tokens,total_tokens,latency_ms,cost_cents,recorded_at) VALUES (?1,?2,?3,0,0,0,0,?4,'2026-10-04T00:00:01Z')",
            libsql_rusqlite::params![format!("usage:{model_run_id}"), project_id, model_run_id, cents],
        )
        .expect("seed synthetic usage");
    connection
        .execute(
            "INSERT INTO model_cost_ledger(ledger_id,project_id,model_run_id,budget_id,cost_cents,token_total,cost_type,created_at) VALUES (?1,?2,?3,NULL,?4,0,'ACTUAL','2026-10-04T00:00:01Z')",
            libsql_rusqlite::params![format!("ledger:actual:{model_run_id}"), project_id, model_run_id, cents],
        )
        .expect("seed synthetic actual cost ledger");
    connection
        .execute(
            "INSERT INTO project_runtime_state(project_id,total_cost_cents,updated_at) VALUES (?1,?2,'2026-10-04T00:00:01Z') ON CONFLICT(project_id) DO UPDATE SET total_cost_cents=excluded.total_cost_cents,updated_at=excluded.updated_at",
            libsql_rusqlite::params![project_id, cents],
        )
        .expect("seed synthetic cents projection");
}

fn f01_project_cost_snapshot(root: &Path, project_id: &str) -> (i64, i64, i64, i64) {
    let db = root.join("projects").join(project_id).join("project.db");
    let connection = open_connection(&db, true).expect("read synthetic project fixture");
    let total_cost_cents = connection
        .query_row(
            "SELECT total_cost_cents FROM project_runtime_state WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .expect("read authoritative cents");
    let ledger_cost_cents = connection
        .query_row(
            "SELECT COALESCE(SUM(cost_cents),0) FROM model_cost_ledger WHERE project_id=?1 AND cost_type='ACTUAL'",
            [project_id],
            |row| row.get(0),
        )
        .expect("read cost ledger");
    let usage_cost_cents = connection
        .query_row(
            "SELECT COALESCE(SUM(cost_cents),0) FROM model_usage WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .expect("read usage costs");
    let usage_rows = connection
        .query_row(
            "SELECT COUNT(*) FROM model_usage WHERE project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .expect("read usage row count");
    (
        total_cost_cents,
        ledger_cost_cents,
        usage_cost_cents,
        usage_rows,
    )
}

fn f01_assert_currency_value(value: &Value, cents: i64, label: &str) {
    let actual = value
        .as_f64()
        .unwrap_or_else(|| panic!("{label} must be a JSON number: {value}"));
    let expected = cents as f64 / 100.0;
    assert!(
        (actual - expected).abs() < 1e-12,
        "{label} must be {expected:.2} yuan for {cents} cents, got {actual:.2}"
    );
}

#[test]
fn real_ipc_project_library_reports_currency_units_for_all_cent_values() {
    let (app, temp) = build_app();
    let root = temp.path().to_path_buf();
    let w = webview(&app);
    let cases = [
        ("zero", 0_i64),
        ("one", 1),
        ("four", 4),
        ("one-twenty-three", 123),
    ];
    let mut projects = Vec::new();

    for (name, cents) in cases {
        let project_id = f01_create_cost_fixture_project(
            &w,
            &format!("f01-all-cents-{name}"),
            &format!("F01 {cents} 分合成项目"),
        );
        f01_seed_project_cost_cents(&root, &project_id, cents);
        projects.push((project_id, cents));
    }

    let before: Vec<_> = projects
        .iter()
        .map(|(project_id, _)| f01_project_cost_snapshot(&root, project_id))
        .collect();
    let library = invoke(&w, "query_project_library", json!({}));
    assert_eq!(
        library["projects"].as_array().unwrap().len(),
        projects.len()
    );
    for (project_id, cents) in &projects {
        let row = library["projects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == *project_id)
            .expect("registered library query returns project");
        f01_assert_currency_value(&row["totalCost"], *cents, "project totalCost");
        let overview = invoke(
            &w,
            "query_project_overview",
            json!({"request":{"projectId":project_id,"scope":"overview","pageSize":8}}),
        );
        f01_assert_currency_value(
            &overview["project"]["totalCost"],
            *cents,
            "overview totalCost",
        );
    }
    f01_assert_currency_value(&library["totals"]["totalCost"], 128, "library totalCost");
    let after: Vec<_> = projects
        .iter()
        .map(|(project_id, _)| f01_project_cost_snapshot(&root, project_id))
        .collect();
    assert_eq!(
        before, after,
        "queries must not change cents, ledger, or usage rows"
    );
}

#[test]
fn real_ipc_project_library_aggregates_four_and_one_hundred_twenty_three_cents_and_reloads() {
    let evidence_root = std::env::var_os("MOHENG_F01_NATIVE_APP_DATA_DIR").map(PathBuf::from);
    let temp = if evidence_root.is_none() {
        Some(tempdir().unwrap())
    } else {
        None
    };
    let root = evidence_root.unwrap_or_else(|| temp.as_ref().unwrap().path().to_path_buf());
    if std::env::var_os("MOHENG_F01_NATIVE_APP_DATA_DIR").is_some() {
        assert!(
            !root.exists(),
            "MOHENG_F01_NATIVE_APP_DATA_DIR must be a new, isolated directory"
        );
        std::fs::create_dir_all(&root).unwrap();
    }

    let app = build_app_over(&root);
    let w = webview(&app);
    let four_cents = f01_create_cost_fixture_project(&w, "f01-aggregate-four", "F01 四分项目");
    let one_twenty_three =
        f01_create_cost_fixture_project(&w, "f01-aggregate-123", "F01 一元二角三分项目");
    f01_seed_project_cost_cents(&root, &four_cents, 4);
    f01_seed_project_cost_cents(&root, &one_twenty_three, 123);

    let before_four = f01_project_cost_snapshot(&root, &four_cents);
    let before_123 = f01_project_cost_snapshot(&root, &one_twenty_three);
    let library_before_close = invoke(&w, "query_project_library", json!({}));
    f01_assert_currency_value(
        &library_before_close["totals"]["totalCost"],
        127,
        "two-project library totalCost",
    );
    for (project_id, cents) in [(&four_cents, 4_i64), (&one_twenty_three, 123_i64)] {
        let row = library_before_close["projects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == *project_id)
            .expect("registered library query returns project");
        f01_assert_currency_value(&row["totalCost"], cents, "project card totalCost");
        let overview = invoke(
            &w,
            "query_project_overview",
            json!({"request":{"projectId":project_id,"scope":"overview","pageSize":8}}),
        );
        f01_assert_currency_value(
            &overview["project"]["totalCost"],
            cents,
            "overview/library totalCost consistency",
        );
    }
    let after_first_query_four = f01_project_cost_snapshot(&root, &four_cents);
    let after_first_query_123 = f01_project_cost_snapshot(&root, &one_twenty_three);
    assert_eq!(before_four, after_first_query_four);
    assert_eq!(before_123, after_first_query_123);
    drop(w);
    drop(app);

    let reopened = build_app_over(&root);
    let reopened_webview = webview(&reopened);
    let library_after_reopen = invoke(&reopened_webview, "query_project_library", json!({}));
    f01_assert_currency_value(
        &library_after_reopen["totals"]["totalCost"],
        127,
        "reloaded library totalCost",
    );
    let after_reopen_four = f01_project_cost_snapshot(&root, &four_cents);
    let after_reopen_123 = f01_project_cost_snapshot(&root, &one_twenty_three);
    assert_eq!(before_four, after_reopen_four);
    assert_eq!(before_123, after_reopen_123);
    println!(
        "F01_REGISTERED_LIBRARY_COST_RESULT={}",
        json!({
            "projects":[{"projectId":four_cents,"costCents":4,"displayYuan":0.04},{"projectId":one_twenty_three,"costCents":123,"displayYuan":1.23}],
            "libraryTotalYuan":1.27,
            "query":"registered query_project_library",
            "sameAsOverview":true,
            "databaseBeforeQuery":{"fourCents":before_four,"oneTwentyThreeCents":before_123},
            "databaseAfterFirstQuery":{"fourCents":after_first_query_four,"oneTwentyThreeCents":after_first_query_123},
            "databaseAfterReopenQuery":{"fourCents":after_reopen_four,"oneTwentyThreeCents":after_reopen_123},
            "databaseChangedByQuery":false
        })
    );
    drop(reopened_webview);
    drop(reopened);
    drop(temp);
}

#[test]
fn real_ipc_duplicate_and_delete_to_trash_round_trip() {
    let (app, _temp) = build_app();
    let w = webview(&app);
    let created = invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_create_from_idea",
                "commandId": "d1",
                "idempotencyKey": "d1-key",
                "requestedAt": "2026-08-25T00:00:00Z",
                "payload": {"title": "待复制"}
            }
        }),
    );
    let source = receipt_project_id(&created);

    // Duplicate via real dispatch. The result identity is derived from d2 by
    // the kernel; the caller does not provide a durable target id.
    let expected_copy = StableIdentityFactory::from_command(StableIdKind::Project, "d2").unwrap();
    let dup = invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_duplicate",
                "commandId": "d2",
                "idempotencyKey": "d2-key",
                "projectId": source,
                "expectedRevision": 0,
                "requestedAt": "2026-08-25T00:00:01Z"
            }
        }),
    );
    assert_eq!(dup["projectId"], source);
    assert_eq!(dup["resultProjectId"], expected_copy);
    let library = invoke(&w, "query_project_library", json!({}));
    assert_eq!(library["projects"].as_array().unwrap().len(), 2);

    // Delete (to trash) via real dispatch.
    invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_delete",
                "commandId": "d3",
                "idempotencyKey": "d3-key",
                "projectId": source,
                "expectedRevision": 1,
                "requestedAt": "2026-08-25T00:00:02Z"
            }
        }),
    );
    let after = invoke(&w, "query_project_library", json!({}));
    let source_row = after["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == source)
        .unwrap();
    assert_eq!(source_row["lifecycle"], "DELETING");
    // The duplicate survives untouched: deleting A never affects B.
    let dup_row = after["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == expected_copy)
        .unwrap();
    assert_eq!(dup_row["lifecycle"], "ACTIVE");
}

#[test]
fn real_ipc_survives_restart_and_reloaded_query_matches_commit() {
    let temp = tempdir().unwrap();
    let app_first = build_app_over(temp.path());
    let w_first = webview(&app_first);
    let created = invoke(
        &w_first,
        "execute_command",
        json!({
            "request": {
                "type": "project_create_from_idea",
                "commandId": "r1",
                "idempotencyKey": "r1-key",
                "requestedAt": "2026-08-25T00:00:00Z",
                "payload": {"title": "重启验证"}
            }
        }),
    );
    let project_id = receipt_project_id(&created);
    invoke(
        &w_first,
        "execute_command",
        json!({
            "request": {
                "type": "project_rename",
                "commandId": "r2",
                "idempotencyKey": "r2-key",
                "projectId": project_id,
                "expectedRevision": 0,
                "requestedAt": "2026-08-25T00:00:01Z",
                "payload": {"title": "重命名后"}
            }
        }),
    );
    drop(w_first);
    drop(app_first);

    // "Restart" the application over the SAME on-disk root.
    let app_second = build_app_over(temp.path());
    let w_second = webview(&app_second);
    let library = invoke(&w_second, "query_project_library", json!({}));
    assert_eq!(library["projects"].as_array().unwrap().len(), 1);
    assert_eq!(library["projects"][0]["id"], project_id);
    assert_eq!(library["projects"][0]["title"], "重命名后");
    let overview = invoke(
        &w_second,
        "query_project_overview",
        json!({"request": {"projectId": project_id, "scope": "overview", "pageSize": 8}}),
    );
    assert_eq!(overview["project"]["title"], "重命名后");
    assert_eq!(overview["project"]["revision"], 1);
}

#[test]
fn real_ipc_duplicate_receipt_and_delete_reflect_in_project_db() {
    let (app, _temp) = build_app();
    let w = webview(&app);
    let created = invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_create_from_idea",
                "commandId": "k1",
                "idempotencyKey": "k1-key",
                "requestedAt": "2026-08-25T00:00:00Z",
                "payload": {"title": "持久化"}
            }
        }),
    );
    let project_id = receipt_project_id(&created);

    // The command handler wrote a receipt into the response and the project DB
    // holds the authoritative row; the invoke did not silently fall back.
    assert_eq!(created["message"], "权威事务已完成，界面将重新查询。");
    assert_ne!(project_id, "");

    // A duplicate command targets a brand-new factory-derived id and returns it.
    let expected_copy = StableIdentityFactory::from_command(StableIdKind::Project, "k2").unwrap();
    let dup = invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_duplicate",
                "commandId": "k2",
                "idempotencyKey": "k2-key",
                "projectId": project_id,
                "expectedRevision": 0,
                "requestedAt": "2026-08-25T00:00:01Z"
            }
        }),
    );
    assert_eq!(dup["projectId"], project_id);
    assert_eq!(dup["resultProjectId"], expected_copy);
}

#[test]
fn real_ipc_integrity_query_runs_and_market_opportunities_is_available() {
    let (app, _temp) = build_app();
    let w = webview(&app);
    let created = invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "project_create_from_idea",
                "commandId": "i1",
                "idempotencyKey": "i1-key",
                "requestedAt": "2026-08-25T00:00:00Z",
                "payload": {"title": "完整性"}
            }
        }),
    );
    let project_id = receipt_project_id(&created);
    invoke(
        &w,
        "execute_command",
        json!({
            "request": {
                "type": "integrity_check_run",
                "commandId": "i2",
                "idempotencyKey": "i2-key",
                "projectId": project_id,
                "expectedRevision": 0,
                "requestedAt": "2026-08-25T00:00:01Z"
            }
        }),
    );
    let markets = invoke(&w, "query_market_opportunities", json!({}));
    assert!(
        markets.get("status").and_then(Value::as_str).is_some(),
        "market discovery must expose a typed source state, not a silent empty list: {markets}"
    );
    assert!(markets
        .get("opportunities")
        .and_then(Value::as_array)
        .is_some());
}

const R2_PROVIDER_NOW: &str = "2026-10-04T00:00:00Z";
static R2_REGISTERED_PROVIDER_TEST_LOCK: Mutex<()> = Mutex::new(());

fn r2_runtime_canary() -> String {
    let mut random = [0_u8; 24];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .expect("OS random source is available");
    format!("MOHENG_SECRET_CANARY_{}", hex::encode(random))
}

fn r2_read_http_request(stream: &mut TcpStream) -> Option<(String, Value)> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut request = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        request.extend_from_slice(&buffer[..read]);
        let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let header_text = String::from_utf8_lossy(&request[..header_end]).into_owned();
        let content_length = header_text
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if request.len() >= header_end + 4 + content_length {
            let body = &request[header_end + 4..header_end + 4 + content_length];
            return Some((header_text, serde_json::from_slice(body).ok()?));
        }
    }
}

fn r2_response_envelope(content: Value) -> Vec<u8> {
    let envelope = json!({
        "id":"r2-local-response",
        "choices":[{"message":{"content":content.to_string()}}],
        "usage":{"prompt_tokens":9,"completion_tokens":7,"total_tokens":16,"prompt_tokens_details":{"cached_tokens":1}}
    });
    envelope.to_string().into_bytes()
}

fn r2_model_output(capability: &str, input: &Value, task_id: &str) -> Value {
    match capability {
        "PLAN_STRATEGY" => {
            let active_tasks = input
                .get("activeTaskRefs")
                .and_then(Value::as_array)
                .cloned()
                .or_else(|| {
                    input
                        .pointer("/volumePlanRoute/tasks")
                        .and_then(Value::as_array)
                        .map(|tasks| {
                            tasks
                                .iter()
                                .filter(|task| {
                                    task.get("class").and_then(Value::as_str) == Some("REQUIRED")
                                })
                                .filter_map(|task| task.get("taskId").cloned())
                                .collect()
                        })
                })
                .unwrap_or_else(|| vec![json!(task_id)]);
            json!({
                "payloadSchema":"chapter-plan.r1",
                "chapterId":input["chapterId"],
                "compiledFromProjectRevision":input["compiledFromProjectRevision"],
                "compiledFromStateDigest":input["compiledFromStateDigest"],
                "contextManifestRef":input["contextManifestRef"],
                "chapterGoal":"林舟打开石门并记录可核对线索",
                "activeTaskRefs":active_tasks,
                "continuityActions":[],
                "requiredStateOutcomes":[],
                "forbiddenTransitions":[],
                "knowledgeBoundaries":[],
                "reservationRefs":[],
                "sceneExecution":[],
                "horizonPreview":[
                    {"slotNo":1,"objective":"观察石门后的线索","tentative":true},
                    {"slotNo":2,"objective":"核对线索来源","tentative":true},
                    {"slotNo":3,"objective":"留下下一步调查方向","tentative":true}
                ]
            })
        }
        "RENDER_CHAPTER" => json!({"body":"林舟推开石门，记录下石门之后的唯一可验证线索。"}),
        "AUDIT_EXTRACT" => json!({
            "qualityFindings":[],
            "canonCandidates":[{
                "factId":"fact:r2:door-open","subjectId":"entity:r2:lead","entityId":"entity:r2:lead",
                "predicate":"opened","objectJson":{"target":"stone-door"},"authorityLevel":"C",
                "eventType":"ACTION","storyTime":"2026-01-01T00:00:00Z",
                "evidence":{"start":0,"end":6,"quote":"林舟推开石门"}
            }],
            "knowledgeCandidates":[],"continuityCandidates":[],
            "taskProgressCandidates":[{"taskId":task_id,"status":"ACTIVE","evidence":{"start":0,"end":6,"quote":"林舟推开石门"}}],
            "revealedReservationRefs":[],"evidenceRanges":[{"start":0,"end":6,"quote":"林舟推开石门"}],
            "auditCompleteness":"COMPLETE_WITH_DELTA","unresolvedHard":false
        }),
        _ => json!({"ok":true}),
    }
}

struct R2LocalProviderServer {
    endpoint: String,
    stop: Arc<AtomicBool>,
    requests: Arc<AtomicUsize>,
    auth_matches: Arc<AtomicUsize>,
    process_matches: Arc<AtomicUsize>,
    worker: thread::JoinHandle<()>,
}

fn r2_spawn_local_provider_server(
    canary: String,
    task_id: String,
    expected_requests: usize,
) -> R2LocalProviderServer {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let requests = Arc::new(AtomicUsize::new(0));
    let auth_matches = Arc::new(AtomicUsize::new(0));
    let process_canary_matches = Arc::new(AtomicUsize::new(0));
    let stop_worker = Arc::clone(&stop);
    let requests_worker = Arc::clone(&requests);
    let auth_worker = Arc::clone(&auth_matches);
    let process_worker = Arc::clone(&process_canary_matches);
    let worker = thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while requests_worker.load(Ordering::SeqCst) < expected_requests
            && !stop_worker.load(Ordering::SeqCst)
            && std::time::Instant::now() < deadline
        {
            let (mut stream, _) = match listener.accept() {
                Ok(accepted) => accepted,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(_) => break,
            };
            let Some((headers, body)) = r2_read_http_request(&mut stream) else {
                continue;
            };
            requests_worker.fetch_add(1, Ordering::SeqCst);
            let expected_header = format!("authorization: bearer {}", canary.to_ascii_lowercase());
            if headers.to_ascii_lowercase().contains(&expected_header) {
                auth_worker.fetch_add(1, Ordering::SeqCst);
            }
            if let Ok(processes) = ProcessCommand::new("/bin/ps")
                .args(["-eww", "-axo", "pid=,ppid=,command="])
                .output()
            {
                let canary_matches = String::from_utf8_lossy(&processes.stdout)
                    .matches(&canary)
                    .count();
                process_worker.fetch_add(canary_matches, Ordering::SeqCst);
            }
            let system_prompt = body
                .pointer("/messages/0/content")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let capability = if system_prompt.contains("qualityFindings") {
                "AUDIT_EXTRACT"
            } else if system_prompt.contains("\"body\":string") {
                "RENDER_CHAPTER"
            } else {
                "PLAN_STRATEGY"
            };
            let user_input = body
                .pointer("/messages/1/content")
                .and_then(Value::as_str)
                .and_then(|value| serde_json::from_str::<Value>(value).ok())
                .unwrap_or(Value::Null);
            let content = r2_response_envelope(r2_model_output(capability, &user_input, &task_id));
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                content.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&content);
        }
    });
    R2LocalProviderServer {
        endpoint: format!("http://{address}"),
        stop,
        requests,
        auth_matches,
        process_matches: process_canary_matches,
        worker,
    }
}

fn r2_bounded_invoke(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    command: &'static str,
    body: Value,
    timeout: Duration,
) -> Result<Value, String> {
    let webview = webview.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = catch_unwind(AssertUnwindSafe(|| try_invoke(&webview, command, body)))
            .unwrap_or_else(|_| Err("IPC_HANDLER_PANICKED".into()));
        let _ = sender.send(result);
    });
    receiver
        .recv_timeout(timeout)
        .unwrap_or_else(|_| Err("IPC_RESPONSE_TIMEOUT".into()))
}

fn r2_safe_error_class(error: Option<&String>) -> &'static str {
    let Some(error) = error else {
        return "NONE";
    };
    for (prefix, class) in [
        ("B08_", "B08_ERROR"),
        ("B04_", "B04_ERROR"),
        ("B05_", "B05_ERROR"),
        ("B06_", "B06_ERROR"),
        ("B07_", "B07_ERROR"),
        ("B09_", "B09_ERROR"),
        ("B10_", "B10_ERROR"),
        ("P01_", "P01_ERROR"),
        ("IPC_", "IPC_ERROR"),
        ("MODEL_", "MODEL_ERROR"),
        ("VOLUME_", "VOLUME_ERROR"),
        ("GOAL_", "GOAL_ERROR"),
        ("CONTEXT_", "CONTEXT_ERROR"),
        ("EXPORT_", "EXPORT_ERROR"),
        ("WORKER_FAILED", "WORKER_ERROR"),
    ] {
        if error.contains(prefix) {
            return class;
        }
    }
    "OTHER_HANDLER_ERROR"
}

fn r2_safe_error_detail(error: Option<&String>, canary: &str) -> String {
    let redacted = error
        .map(|value| value.replace(canary, "[CANARY]"))
        .unwrap_or_default();
    redacted
        .chars()
        .take(160)
        .map(|character| {
            if character.is_ascii_alphanumeric()
                || matches!(character, '_' | '-' | ':' | '.' | '[' | ']')
            {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn r2_scan_project_tree(root: &Path, canary: &str) -> (usize, usize, BTreeMap<String, usize>) {
    let mut scanned = 0;
    let mut matches = 0;
    let mut categories = BTreeMap::new();
    for entry in std::fs::read_dir(root).expect("enumerate isolated project surfaces") {
        let entry = entry.expect("read isolated project entry");
        let kind = entry.file_type().expect("inspect isolated project entry");
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            let (nested_scanned, nested_matches, nested_categories) =
                r2_scan_project_tree(&entry.path(), canary);
            scanned += nested_scanned;
            matches += nested_matches;
            for (category, count) in nested_categories {
                *categories.entry(category).or_insert(0) += count;
            }
        } else if kind.is_file() {
            scanned += 1;
            let content = std::fs::read(entry.path()).expect("read isolated project file");
            matches += usize::from(
                content
                    .windows(canary.len())
                    .any(|part| part == canary.as_bytes()),
            );
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            let category = if name.ends_with("-wal") {
                "sqlite_wal"
            } else if name.ends_with("-shm") {
                "sqlite_shm"
            } else if name.ends_with(".db") || name.ends_with(".sqlite") {
                "sqlite_database"
            } else if entry
                .path()
                .components()
                .any(|component| component.as_os_str() == "exports")
            {
                if name == "receipt.json" {
                    "export_receipt"
                } else {
                    "export_output_or_progress"
                }
            } else {
                "other_project_file"
            };
            *categories.entry(category.to_string()).or_insert(0) += 1;
        }
    }
    (scanned, matches, categories)
}

fn r2_insert_artifact(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    scope_level: &str,
    scope_key: &str,
    payload: &Value,
) -> String {
    let digest = sha256_canonical(payload).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'m0a.artifact.v1','COMMITTED',?6,'{}',?7,?7)",
        libsql_rusqlite::params![artifact_id, project_id, artifact_type, scope_level, scope_key, digest, R2_PROVIDER_NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
        libsql_rusqlite::params![artifact_id, payload.to_string(), digest],
    ).unwrap();
    connection.execute(
        "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,?2,?3,?4,?5,1,?6)",
        libsql_rusqlite::params![project_id, artifact_type, scope_level, scope_key, artifact_id, digest],
    ).unwrap();
    digest
}

fn r2_seed_async_provider_project(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
) -> (String, String, String) {
    let book_id =
        StableIdentityFactory::from_command(StableIdKind::Book, &format!("r2:{project_id}:book"))
            .unwrap();
    let volume_id = StableIdentityFactory::from_command(
        StableIdKind::Volume,
        &format!("r2:{project_id}:volume"),
    )
    .unwrap();
    let chapter_id = StableIdentityFactory::from_command(
        StableIdKind::Chapter,
        &format!("r2:{project_id}:chapter-1"),
    )
    .unwrap();
    let entity_id = format!("entity:r2:{project_id}:lead");
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'R2 异步生产','ACTIVE',?3,?3)", libsql_rusqlite::params![book_id,project_id,R2_PROVIDER_NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'本地 Provider 测试卷','PRODUCING',1,3,3000,?4,?4)", libsql_rusqlite::params![volume_id,project_id,book_id,R2_PROVIDER_NOW]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,1,'第一章','NORMAL','PLANNED',0,?4,?4)", libsql_rusqlite::params![chapter_id,project_id,volume_id,R2_PROVIDER_NOW]).unwrap();
    connection.execute("INSERT INTO chapter_production_budgets(budget_id,project_id,chapter_id,importance_class,hard_max_model_runs,cost_budget_cents,frozen_at) VALUES (?1,?2,?3,'NORMAL',12,500,?4)", libsql_rusqlite::params![format!("budget:{chapter_id}"),project_id,chapter_id,R2_PROVIDER_NOW]).unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES (?1,?2,'林舟','CHARACTER',?3,?3)", libsql_rusqlite::params![entity_id,project_id,R2_PROVIDER_NOW]).unwrap();
    connection.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,story_time_from,story_time_to,source_chapter_id,created_at,updated_at) VALUES (?1,?2,?3,'location','{\"place\":\"石门\"}','A','2020-01-01T00:00:00Z','2021-01-01T00:00:00Z',?4,?5,?5)", libsql_rusqlite::params![format!("fact:r2:{project_id}:seed"),project_id,entity_id,chapter_id,R2_PROVIDER_NOW]).unwrap();
    connection.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES ('task:actual',?1,'volume.task.progress','{\"taskId\":\"task:actual\",\"status\":\"ACTIVE\",\"evidenceRefs\":[],\"remainingConditions\":[]}',?2,'{}',?3,?3)", libsql_rusqlite::params![project_id,chapter_id,R2_PROVIDER_NOW]).unwrap();
    let project_scope = r#"{"level":"PROJECT"}"#;
    let book_plan = json!({"payloadSchema":"book-plan.r1","planningMaturity":"APPROVED","futureReservations":[]});
    let book_plan_hash = r2_insert_artifact(
        connection,
        project_id,
        &format!("book-plan:r2:{project_id}"),
        "BookPlan",
        "PROJECT",
        project_scope,
        &book_plan,
    );
    r2_insert_artifact(
        connection,
        project_id,
        &format!("story-constitution:r2:{project_id}"),
        "StoryConstitution",
        "PROJECT",
        project_scope,
        &json!({"payloadSchema":"story-constitution.r1","storyCore":{"protagonist":"林舟","desire":"查明石门后的线索"}}),
    );
    r2_insert_artifact(
        connection,
        project_id,
        &format!("main-contract:r2:{project_id}"),
        "MainContract",
        "PROJECT",
        project_scope,
        &json!({"payloadSchema":"main-contract.r1","hardRules":["保持已证实事实"]}),
    );
    let gate_policy = json!({"payloadSchema":"gate-policy.r1","policyId":"r2-local","rules":["candidate-audit","chapter-final"],"canonSemantics":{"policyVersion":"canon-semantics.r1","rules":[{"predicate":"opened","cardinality":"MULTI","scope":{"kind":"FIXED","value":"PROJECT"}},{"predicate":"location","cardinality":"SINGLE","scope":{"kind":"FIXED","value":"PROJECT"},"timeBasis":"WORLD_TIME"},{"predicate":"remembered","cardinality":"MULTI","scope":{"kind":"FIXED","value":"PROJECT"}},{"predicate":"owns_item","cardinality":"MULTI","scope":{"kind":"FIXED","value":"PROJECT"}},{"predicate":"loadout","cardinality":"MULTI","scope":{"kind":"FIXED","value":"PROJECT"},"timeBasis":"WORLD_TIME","mutualExclusions":[{"left":{"kind":"A"},"right":{"kind":"B"}}]},{"predicate":"allegiance","cardinality":"SINGLE","scope":{"kind":"FIXED","value":"PROJECT"},"timeBasis":"WORLD_TIME","polarityField":"polarity"},{"predicate":"membership","cardinality":"MULTI","scope":{"kind":"OBJECT_FIELD","field":"scope"}}]}});
    r2_insert_artifact(
        connection,
        project_id,
        &format!("gate-policy:r2:{project_id}"),
        "GatePolicy",
        "PROJECT",
        project_scope,
        &gate_policy,
    );
    (chapter_id, volume_id, book_plan_hash)
}

fn r2_seed_planning_project(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
) -> (String, String) {
    let book_id = StableIdentityFactory::from_command(
        StableIdKind::Book,
        &format!("r2-planning:{project_id}:book"),
    )
    .unwrap();
    let volume_id = StableIdentityFactory::from_command(
        StableIdKind::Volume,
        &format!("r2-planning:{project_id}:volume"),
    )
    .unwrap();
    let chapter_id = StableIdentityFactory::from_command(
        StableIdKind::Chapter,
        &format!("r2-planning:{project_id}:chapter-1"),
    )
    .unwrap();
    let entity_id = format!("entity:r2-planning:{project_id}:lead");
    let task_id = "task:actual";
    let final_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("r2-planning:{project_id}:final-1"),
    )
    .unwrap();
    let chapter_scope = json!({"level":"CHAPTER","chapterId":chapter_id}).to_string();
    let final_payload = json!({
        "payloadSchema":"final-chapter.r1",
        "chapterId":chapter_id,
        "body":"林舟在上一章确认石门位置，并留下可核对的现场记录。",
        "evidenceRanges":[]
    });
    let final_hash = sha256_canonical(&final_payload).unwrap();
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'R2 planning book','ACTIVE',?3,?3)",
        libsql_rusqlite::params![book_id,project_id,R2_PROVIDER_NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'R2 planning volume','PRODUCING',1,3,3000,?4,?4)",
        libsql_rusqlite::params![volume_id,project_id,book_id,R2_PROVIDER_NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,final_artifact_id,final_version,created_at,updated_at) VALUES (?1,?2,?3,1,'已提交的上一章','NORMAL','COMMITTED',26,?4,1,?5,?5)",
        libsql_rusqlite::params![chapter_id,project_id,volume_id,final_id,R2_PROVIDER_NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'FinalChapter','CHAPTER',?3,1,'m0a.artifact.v1','COMMITTED','CANON',?4,'{}',?5,?5)",
        libsql_rusqlite::params![final_id,project_id,chapter_scope,final_hash,R2_PROVIDER_NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
        libsql_rusqlite::params![final_id,final_payload.to_string(),final_hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'FinalChapter','CHAPTER',?2,?3,1,?4)",
        libsql_rusqlite::params![project_id,chapter_scope,final_id,final_hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO chapter_production_budgets(budget_id,project_id,chapter_id,importance_class,hard_max_model_runs,cost_budget_cents,frozen_at) VALUES (?1,?2,?3,'NORMAL',12,500,?4)",
        libsql_rusqlite::params![format!("budget:{chapter_id}"),project_id,chapter_id,R2_PROVIDER_NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES (?1,?2,'林舟','CHARACTER',?3,?3)",
        libsql_rusqlite::params![entity_id,project_id,R2_PROVIDER_NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,source_artifact_id,source_version,source_chapter_id,authority_level,created_at,updated_at) VALUES (?1,?2,?3,'location','{\"place\":\"石门\"}',?4,1,?5,'A',?6,?6)",
        libsql_rusqlite::params![format!("fact:r2-planning:{project_id}:seed"),project_id,entity_id,final_id,chapter_id,R2_PROVIDER_NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES (?1,?2,'volume.task.progress','{\"taskId\":\"task:actual\",\"status\":\"ACTIVE\",\"evidenceRefs\":[],\"remainingConditions\":[]}',?3,'{}',?4,?4)",
        libsql_rusqlite::params![task_id,project_id,chapter_id,R2_PROVIDER_NOW],
    ).unwrap();

    let project_scope = r#"{"level":"PROJECT"}"#;
    let book_plan = json!({"payloadSchema":"book-plan.r1","planningMaturity":"APPROVED"});
    let book_plan_id = format!("book-plan:r2-planning:{project_id}");
    let book_plan_hash = r2_insert_artifact(
        connection,
        project_id,
        &book_plan_id,
        "BookPlan",
        "PROJECT",
        project_scope,
        &book_plan,
    );
    r2_insert_artifact(
        connection,
        project_id,
        &format!("story:r2-planning:{project_id}"),
        "StoryConstitution",
        "PROJECT",
        project_scope,
        &json!({"payloadSchema":"story-constitution.r1","storyCore":{"protagonist":"林舟"}}),
    );
    r2_insert_artifact(
        connection,
        project_id,
        &format!("main:r2-planning:{project_id}"),
        "MainContract",
        "PROJECT",
        project_scope,
        &json!({"payloadSchema":"main-contract.r1","hardRules":["保持已证实事实"]}),
    );
    let volume_scope = json!({"level":"VOLUME","volumeId":volume_id}).to_string();
    let book_plan_ref = json!({"artifactId":book_plan_id,"version":1,"payloadHash":book_plan_hash});
    let intent_id = format!("volume-intent:r2-planning:{project_id}");
    let intent = json!({
        "payloadSchema":"volume-intent.r1","volumeFunction":"推进本地验收中的真实调查",
        "entryConditions":[{"kind":"BOOK_PLAN_APPROVED","value":book_plan_ref}],
        "mustAchieve":[{"id":"goal:r2-planning","description":"留下下一步调查所需证据"}],
        "mustNotBreak":[{"id":"rule:r2-planning","description":"不得改变上一章已确认事实"}],
        "requiredExitConditions":[{"kind":"TASK_SATISFIED","value":task_id}],
        "futureReservationRefs":[],"crossVolumeResponsibilityRefs":[],
        "targetChapterRange":{"min":1,"preferred":3,"max":3},"targetWordRange":{"preferred":3000},
        "sourceBookPlanRef":book_plan_ref,"compiledFromProjectRevision":0,
        "sourceCanonRefs":[format!("fact:r2-planning:{project_id}:seed")],
        "sourceEvidenceRefs":[format!("fact:r2-planning:{project_id}:seed")],"semanticDigest":"c".repeat(64)
    });
    let intent_hash = r2_insert_artifact(
        connection,
        project_id,
        &intent_id,
        "VolumeIntent",
        "VOLUME",
        &volume_id,
        &intent,
    );
    let plan = json!({
        "payloadSchema":"volume-plan.r1","volumeIntentRef":{"artifactId":intent_id,"version":1,"payloadHash":intent_hash},"routeVersion":1,
        "tasks":[{"taskId":task_id,"class":"REQUIRED","objective":"核对石门线索","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"TASK_EVIDENCE_RECORDED","value":{"taskId":task_id,"evidenceType":"FINAL_CHAPTER","requiresNewEvidence":true}},"estimatedChapterCost":{"preferred":1}}],
        "routeInvariants":["不得违背已有事实"],"requiredExitConditions":[{"kind":"TASK_SATISFIED","value":task_id}],
        "capacityBudget":{"minChapters":1,"preferredChapters":3,"maxChapters":3}
    });
    let plan_id = format!("volume-plan:r2-planning:{project_id}");
    r2_insert_artifact(
        connection,
        project_id,
        &plan_id,
        "VolumePlan",
        "VOLUME",
        &volume_scope,
        &plan,
    );
    connection.execute(
        "INSERT INTO volume_plan_index(project_id,volume_id,plan_artifact_id,plan_version,plan_state,target_chapters,updated_at) VALUES (?1,?2,?3,1,'APPROVED',3,?4)",
        libsql_rusqlite::params![project_id,volume_id,plan_id,R2_PROVIDER_NOW],
    ).unwrap();
    (volume_id, chapter_id)
}

#[cfg(target_os = "macos")]
#[test]
fn r2_registered_async_production_and_export_keep_runtime_canary_private() {
    let _provider_test_guard = R2_REGISTERED_PROVIDER_TEST_LOCK
        .lock()
        .expect("registered Provider E2E lock is not poisoned");
    let canary = r2_runtime_canary();
    let expected_requests = 4;
    let R2LocalProviderServer {
        endpoint,
        stop,
        requests,
        auth_matches,
        process_matches,
        worker: server,
    } = r2_spawn_local_provider_server(canary.clone(), "task:actual".into(), expected_requests);
    let secret_store = MemorySecretStore::new();
    secret_store.put("fixture", &canary).unwrap();
    let gateway = Arc::new(ConfiguredProductionGateway::new(
        ProviderConfig {
            provider_id: "fixture".into(),
            model_id: "r2-local-fixture".into(),
            endpoint: endpoint.clone(),
            enabled: true,
            timeout_ms: 5_000,
        },
        Arc::new(OpenAiCompatibleProvider::new(endpoint)),
        Arc::new(secret_store),
    ));
    let temp = tempdir().unwrap();
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway);
    let manager = state.manager.clone();
    let app = build_app_with_state(state.clone());
    let webview = webview(&app);
    let created = r2_bounded_invoke(
        &webview,
        "execute_command",
        json!({"request":{
            "type":"project_create_from_idea","commandId":"r2:project:create",
            "idempotencyKey":"r2:project:create:key","requestedAt":R2_PROVIDER_NOW,
            "payload":{"title":"R2 异步 Provider 正式链","idea":"林舟在石门后发现可验证线索。","platform":"本地验收","mode":"DIRECTOR"}
        }}),
        Duration::from_secs(5),
    );
    let project_id = created
        .as_ref()
        .ok()
        .and_then(|value| value.get("projectId"))
        .and_then(Value::as_str)
        .unwrap_or("r2-project-create-failed")
        .to_string();
    let mut chapter_id = String::new();
    let mut volume_id = String::new();
    let mut book_plan_hash = String::new();
    if created.is_ok() {
        let connection = manager
            .open(&project_id, "R2 async provider fixture", R2_PROVIDER_NOW)
            .unwrap();
        (chapter_id, volume_id, book_plan_hash) =
            r2_seed_async_provider_project(&connection, &project_id);
        drop(connection);
    }

    let initial_revision = if created.is_ok() {
        manager.project_revision(&project_id).unwrap()
    } else {
        0
    };
    let book_plan_ref = json!({
        "artifactId":format!("book-plan:r2:{project_id}"),
        "version":1,
        "payloadHash":book_plan_hash
    });
    let volume_intent = json!({
        "payloadSchema":"volume-intent.r1",
        "volumeFunction":"本地 Provider 异步入口与导出验收",
        "entryConditions":[{"kind":"BOOK_PLAN_APPROVED","value":book_plan_ref}],
        "mustAchieve":[{"id":"goal:r2:actual","description":"完成石门线索的下一步核对"}],
        "mustNotBreak":[{"id":"rule:r2:actual","description":"不得违背已有事实"}],
        "requiredExitConditions":[{"kind":"TASK_SATISFIED","value":"task:actual"}],
        "futureReservationRefs":[],"crossVolumeResponsibilityRefs":[],
        "targetChapterRange":{"min":1,"preferred":3,"max":3},
        "targetWordRange":{"preferred":3000},
        "sourceBookPlanRef":book_plan_ref,
        "compiledFromProjectRevision":initial_revision,
        "sourceCanonRefs":[format!("fact:r2:{project_id}:seed")],
        "sourceEvidenceRefs":[format!("fact:r2:{project_id}:seed")],
        "semanticDigest":"c".repeat(64)
    });
    let intent_result = if created.is_ok() {
        r2_bounded_invoke(
            &webview,
            "execute_command",
            json!({"request":{
                "type":"planning_create_volume_intent","commandId":"r2:volume:intent",
                "idempotencyKey":"r2:volume:intent:key","projectId":project_id,
                "expectedRevision":initial_revision,"requestedAt":R2_PROVIDER_NOW,
                "payload":{"volumeId":volume_id,"plan":volume_intent}
            }}),
            Duration::from_secs(5),
        )
    } else {
        Err("PROJECT_FIXTURE_SETUP_FAILED".into())
    };
    let volume_intent_committed = intent_result
        .as_ref()
        .is_ok_and(|result| result["finalStatus"].as_str() == Some("committed"));
    let volume_intent_ref = intent_result
        .as_ref()
        .ok()
        .and_then(|result| result.pointer("/receipt/committedArtifactRefs/0"))
        .cloned()
        .unwrap_or(Value::Null);
    let volume_plan = json!({
        "payloadSchema":"volume-plan.r1",
        "volumeIntentRef":volume_intent_ref,
        "routeVersion":1,
        "tasks":[{
            "taskId":"task:actual","class":"REQUIRED","objective":"核对石门线索",
            "dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],
            "completionPredicate":{"kind":"TASK_EVIDENCE_RECORDED","value":{"taskId":"task:actual","evidenceType":"FINAL_CHAPTER","requiresNewEvidence":true}},
            "estimatedChapterCost":{"preferred":1}
        }],
        "routeInvariants":["不得违背当前事实"],
        "requiredExitConditions":[{"kind":"TASK_SATISFIED","value":"task:actual"}],
        "capacityBudget":{"minChapters":1,"preferredChapters":3,"maxChapters":3},
        "semanticDigest":"d".repeat(64)
    });
    let intent_revision = if volume_intent_committed {
        manager.project_revision(&project_id).unwrap()
    } else {
        initial_revision
    };
    let volume_plan_result = if volume_intent_committed {
        r2_bounded_invoke(
            &webview,
            "execute_command",
            json!({"request":{
                "type":"planning_create_volume_plan","commandId":"r2:volume:plan",
                "idempotencyKey":"r2:volume:plan:key","projectId":project_id,
                "expectedRevision":intent_revision,"requestedAt":R2_PROVIDER_NOW,
                "payload":{"volumeId":volume_id,"plan":volume_plan}
            }}),
            Duration::from_secs(5),
        )
    } else {
        Err("VOLUME_INTENT_NOT_COMMITTED".into())
    };
    let volume_plan_committed = volume_plan_result
        .as_ref()
        .is_ok_and(|result| result["finalStatus"].as_str() == Some("committed"));
    let production_revision = if volume_plan_committed {
        manager.project_revision(&project_id).unwrap()
    } else {
        intent_revision
    };
    let production = if created.is_ok() && volume_plan_committed {
        r2_bounded_invoke(
            &webview,
            "execute_command",
            json!({"request":{
                "type":"production_start","commandId":"r2:production:start",
                "idempotencyKey":"r2:production:start:key","projectId":project_id,
                "expectedRevision":production_revision,"requestedAt":R2_PROVIDER_NOW,
                "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","skillsEnabled":false,"targetChineseCharacters":800,"chapterGoalHint":"推开石门并保存可核对线索。"}
            }}),
            Duration::from_secs(8),
        )
    } else {
        Err("FORMAL_PRECONDITIONS_NOT_READY".into())
    };
    let production_ok = production.as_ref().is_ok_and(|result| {
        result.get("finalStatus").and_then(Value::as_str) == Some("committed")
            && result.get("stage").and_then(Value::as_str) == Some("COMPLETED")
    });
    let production_ipc_outcome = match production.as_ref() {
        Ok(_) => "RETURNED",
        Err(error) if error == "IPC_RESPONSE_TIMEOUT" => "IPC_TIMEOUT",
        Err(error) if error == "IPC_HANDLER_PANICKED" => "HANDLER_PANICKED",
        Err(_) => "HANDLER_ERROR",
    };

    let export = if production_ok {
        r2_bounded_invoke(
            &webview,
            "create_export",
            json!({"request":{"projectId":project_id,"requestId":"r2:export:normal","format":"TXT"}}),
            Duration::from_secs(5),
        )
    } else {
        Err("PRODUCTION_ENTRY_NOT_COMPLETED".into())
    };
    let export_ok = export
        .as_ref()
        .is_ok_and(|receipt| receipt["status"].as_str() == Some("COMPLETED"));
    let mut exported_canary_matches = 0usize;
    let mut receipt_canary_match = false;
    let mut exported_output_id = String::new();
    let mut exported_output_sha256 = String::new();
    let mut exported_output_bytes = 0_u64;
    let mut output_sha256_matches_receipt = false;
    if let Ok(receipt) = &export {
        receipt_canary_match = receipt.to_string().contains(&canary);
        if let Some(path) = receipt["finalPath"].as_str() {
            let path = Path::new(path);
            let project_root = temp.path().join("projects").join(&project_id);
            assert!(path.starts_with(&project_root));
            exported_output_id = path
                .strip_prefix(&project_root)
                .expect("export path is inside the isolated project")
                .to_string_lossy()
                .into_owned();
            let content = std::fs::read(path).expect("read formal export output");
            exported_output_bytes = content.len() as u64;
            exported_output_sha256 = hex::encode(Sha256::digest(&content));
            output_sha256_matches_receipt =
                receipt["sha256"].as_str() == Some(exported_output_sha256.as_str());
            exported_canary_matches = content
                .windows(canary.len())
                .filter(|part| *part == canary.as_bytes())
                .count();
            assert_eq!(
                receipt["outputByteCount"].as_u64(),
                Some(exported_output_bytes)
            );
        }
    }
    let queried_export_receipt = if export_ok {
        r2_bounded_invoke(
            &webview,
            "query_export_receipt",
            json!({"request":{"projectId":project_id,"requestId":"r2:export:normal"}}),
            Duration::from_secs(5),
        )
    } else {
        Err("EXPORT_NOT_COMPLETED".into())
    };
    let export_receipt_query_matches = queried_export_receipt.as_ref().is_ok_and(|receipt| {
        export.as_ref().is_ok_and(|created| {
            receipt["receiptId"] == created["receiptId"]
                && receipt["sha256"] == created["sha256"]
                && receipt["finalPath"] == created["finalPath"]
                && receipt["status"] == "COMPLETED"
        })
    });
    let prepared = if production_ok && volume_plan_committed {
        let current_revision = manager.project_revision(&project_id).unwrap();
        r2_bounded_invoke(
            &webview,
            "execute_command",
            json!({"request":{
                "type":"planning_prepare_next_chapter","commandId":"r2:chapter:prepare-next",
                "idempotencyKey":"r2:chapter:prepare-next:key","projectId":project_id,
                "expectedRevision":current_revision,"requestedAt":R2_PROVIDER_NOW,
                "payload":{"volumeId":volume_id}
            }}),
            Duration::from_secs(5),
        )
    } else {
        Err("PRODUCTION_ENTRY_NOT_COMPLETED".into())
    };
    let next_chapter_id = prepared
        .as_ref()
        .ok()
        .and_then(|result| result.pointer("/chapter/chapterId"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let next_chapter_prepared = prepared.as_ref().is_ok_and(|result| {
        result["finalStatus"].as_str() == Some("committed")
            && result.pointer("/chapter/chapterNo").and_then(Value::as_i64) == Some(2)
    });
    let next_plan = if next_chapter_prepared {
        let current_revision = manager.project_revision(&project_id).unwrap();
        r2_bounded_invoke(
            &webview,
            "execute_command",
            json!({"request":{
                "type":"planning_compile_chapter_plan","commandId":"r2:chapter:compile-next",
                "idempotencyKey":"r2:chapter:compile-next:key","projectId":project_id,
                "expectedRevision":current_revision,"requestedAt":R2_PROVIDER_NOW,
                "payload":{"chapterId":next_chapter_id,"chapterGoalHint":"核对石门线索并保留已证实事实。"}
            }}),
            Duration::from_secs(8),
        )
    } else {
        Err("NEXT_CHAPTER_PREPARE_NOT_COMMITTED".into())
    };
    let next_plan_committed = next_plan.as_ref().is_ok_and(|result| {
        result["finalStatus"].as_str() == Some("committed")
            && result["owner"].as_str() == Some("B07::PlanningOwner")
            && result["modelRunId"].as_str().is_some()
    });
    let (project_files_scanned, project_canary_matches, project_file_categories) =
        r2_scan_project_tree(temp.path(), &canary);
    let persisted_metrics = {
        let connection = manager
            .open(
                &project_id,
                "R2 async Provider evidence query",
                R2_PROVIDER_NOW,
            )
            .expect("reopen isolated project for persisted evidence counts");
        let count = |sql: &str| {
            connection
                .query_row::<i64, _, _>(sql, [&project_id], |row| row.get(0))
                .expect("query persisted Provider evidence count")
        };
        let completed_model_runs =
            count("SELECT COUNT(*) FROM model_runs WHERE project_id=?1 AND status='COMPLETED'");
        let raw_response_rows = count("SELECT COUNT(*) FROM model_responses WHERE project_id=?1");
        let usage_rows = count("SELECT COUNT(*) FROM model_usage WHERE project_id=?1");
        let actual_cost_rows = count(
            "SELECT COUNT(*) FROM model_cost_ledger WHERE project_id=?1 AND cost_type='ACTUAL'",
        );
        let settled_budget_rows = count(
            "SELECT COUNT(*) FROM budget_reservations WHERE project_id=?1 AND status='SETTLED'",
        );
        let command_receipt_rows = connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM project_command_receipts WHERE project_id=?1 AND command_id IN ('r2:production:start','r2:chapter:prepare-next','r2:chapter:compile-next')",
                [&project_id],
                |row| row.get(0),
            )
            .expect("query registered command receipt count");
        let total_usage_tokens = connection
            .query_row::<i64, _, _>(
                "SELECT COALESCE(SUM(total_tokens),0) FROM model_usage WHERE project_id=?1",
                [&project_id],
                |row| row.get(0),
            )
            .expect("query persisted usage token total");
        (
            completed_model_runs,
            raw_response_rows,
            usage_rows,
            actual_cost_rows,
            settled_budget_rows,
            command_receipt_rows,
            total_usage_tokens,
        )
    };
    let parent_environment_match = std::env::vars().any(|(_, value)| value.contains(&canary));
    stop.store(true, Ordering::SeqCst);
    let server_finished = server.join().is_ok();
    let server_requests = requests.load(Ordering::SeqCst);
    let authorized_requests = auth_matches.load(Ordering::SeqCst);
    let process_canary_matches = process_matches.load(Ordering::SeqCst);
    println!(
        "R2_F01_F02_FORMAL_RUNTIME={}",
        json!({
            "projectCreated": created.is_ok(),
            "volumeIntentCommitted": volume_intent_committed,
            "volumePlanCommitted": volume_plan_committed,
            "productionIpcOutcome": production_ipc_outcome,
            "productionErrorClass": r2_safe_error_class(production.as_ref().err()),
            "productionErrorDetail": r2_safe_error_detail(production.as_ref().err(), &canary),
            "productionCompleted": production_ok,
            "exportCompleted": export_ok,
            "exportReceiptQueryMatches": export_receipt_query_matches,
            "exportReceiptId": export.as_ref().ok().and_then(|receipt| receipt["receiptId"].as_str()),
            "exportOutputId": exported_output_id,
            "exportOutputSha256": exported_output_sha256,
            "exportOutputBytes": exported_output_bytes,
            "outputSha256MatchesReceipt": output_sha256_matches_receipt,
            "nextChapterPrepared": next_chapter_prepared,
            "nextChapterPlanCommitted": next_plan_committed,
            "serverRequests": server_requests,
            "authorizedRequests": authorized_requests,
            "completedModelRuns": persisted_metrics.0,
            "rawResponseRows": persisted_metrics.1,
            "usageRows": persisted_metrics.2,
            "actualCostRows": persisted_metrics.3,
            "settledBudgetRows": persisted_metrics.4,
            "formalCommandReceiptRows": persisted_metrics.5,
            "totalUsageTokens": persisted_metrics.6,
            "processArgvEnvironmentCanaryMatches": process_canary_matches,
            "parentEnvironmentCanaryMatches": parent_environment_match,
            "exportCanaryMatches": exported_canary_matches,
            "receiptCanaryMatch": receipt_canary_match,
            "projectFilesScanned": project_files_scanned,
            "projectFileCategories": project_file_categories,
            "projectCanaryMatches": project_canary_matches,
            "serverFinished": server_finished
        })
    );
    assert!(
        created.is_ok(),
        "registered project create precondition failed"
    );
    assert!(
        production_ok,
        "registered production_start did not complete through the local real Provider"
    );
    assert!(
        export_ok,
        "registered export owner did not produce a receipt"
    );
    assert!(export_receipt_query_matches);
    assert!(output_sha256_matches_receipt);
    assert!(
        next_chapter_prepared,
        "registered next-chapter prepare owner did not commit"
    );
    assert!(
        next_plan_committed,
        "registered next-chapter planning owner did not commit through real Provider"
    );
    assert_eq!(server_requests, expected_requests);
    assert_eq!(authorized_requests, expected_requests);
    assert_eq!(persisted_metrics.0, expected_requests as i64);
    assert_eq!(persisted_metrics.1, expected_requests as i64);
    assert_eq!(persisted_metrics.2, expected_requests as i64);
    assert_eq!(persisted_metrics.3, expected_requests as i64);
    assert_eq!(persisted_metrics.4, expected_requests as i64);
    assert!(persisted_metrics.5 >= 1);
    assert!(persisted_metrics.6 > 0);
    assert_eq!(process_canary_matches, 0);
    assert!(!parent_environment_match);
    assert_eq!(exported_canary_matches, 0);
    assert!(!receipt_canary_match);
    assert_eq!(project_canary_matches, 0);
}

#[cfg(target_os = "macos")]
#[test]
fn r2_registered_next_chapter_prepare_then_compile_uses_real_async_provider() {
    let _provider_test_guard = R2_REGISTERED_PROVIDER_TEST_LOCK
        .lock()
        .expect("registered Provider E2E lock is not poisoned");
    let canary = r2_runtime_canary();
    let R2LocalProviderServer {
        endpoint,
        stop,
        requests,
        auth_matches,
        process_matches,
        worker: server,
    } = r2_spawn_local_provider_server(canary.clone(), "task:actual".into(), 1);
    let secrets = MemorySecretStore::new();
    secrets.put("fixture", &canary).unwrap();
    let gateway = Arc::new(ConfiguredProductionGateway::new(
        ProviderConfig {
            provider_id: "fixture".into(),
            model_id: "r2-planning-local".into(),
            endpoint: endpoint.clone(),
            enabled: true,
            timeout_ms: 5_000,
        },
        Arc::new(OpenAiCompatibleProvider::new(endpoint)),
        Arc::new(secrets),
    ));
    let temp = tempdir().unwrap();
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway);
    let manager = state.manager.clone();
    let app = build_app_with_state(state);
    let webview = webview(&app);
    let created = r2_bounded_invoke(
        &webview,
        "execute_command",
        json!({"request":{
            "type":"project_create_from_idea","commandId":"r2:planning:project:create",
            "idempotencyKey":"r2:planning:project:create:key","requestedAt":R2_PROVIDER_NOW,
            "payload":{"title":"R2 下一章规划","idea":"林舟核对石门后继续追查。","platform":"本地验收","mode":"DIRECTOR"}
        }}),
        Duration::from_secs(5),
    );
    let project_id = created
        .as_ref()
        .ok()
        .and_then(|value| value.get("projectId"))
        .and_then(Value::as_str)
        .unwrap_or("r2-planning-project-create-failed")
        .to_owned();
    let setup = if created.is_ok() {
        let connection = manager
            .open(&project_id, "R2 next chapter fixture", R2_PROVIDER_NOW)
            .unwrap();
        let (volume_id, _) = r2_seed_planning_project(&connection, &project_id);
        drop(connection);
        Some(volume_id)
    } else {
        None
    };
    let prepare = if let Some(volume_id) = &setup {
        r2_bounded_invoke(
            &webview,
            "execute_command",
            json!({"request":{
                "type":"planning_prepare_next_chapter","commandId":"r2:planning:prepare",
                "idempotencyKey":"r2:planning:prepare:key","projectId":project_id,
                "expectedRevision":manager.project_revision(&project_id).unwrap(),
                "requestedAt":R2_PROVIDER_NOW,"payload":{"volumeId":volume_id}
            }}),
            Duration::from_secs(5),
        )
    } else {
        Err("PROJECT_FIXTURE_SETUP_FAILED".into())
    };
    let next_chapter_id = prepare
        .as_ref()
        .ok()
        .and_then(|value| value.pointer("/chapter/chapterId"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let prepared = prepare.as_ref().is_ok_and(|value| {
        value["finalStatus"].as_str() == Some("committed")
            && value.pointer("/chapter/chapterNo").and_then(Value::as_i64) == Some(2)
    });
    let compile = if prepared {
        r2_bounded_invoke(
            &webview,
            "execute_command",
            json!({"request":{
                "type":"planning_compile_chapter_plan","commandId":"r2:planning:compile",
                "idempotencyKey":"r2:planning:compile:key","projectId":project_id,
                "expectedRevision":manager.project_revision(&project_id).unwrap(),
                "requestedAt":R2_PROVIDER_NOW,
                "payload":{"chapterId":next_chapter_id,"chapterGoalHint":"核对石门线索并保留已证实事实。"}
            }}),
            Duration::from_secs(8),
        )
    } else {
        Err("NEXT_CHAPTER_PREPARE_NOT_COMMITTED".into())
    };
    let compiled = compile.as_ref().is_ok_and(|value| {
        value["finalStatus"].as_str() == Some("committed")
            && value["owner"].as_str() == Some("B07::PlanningOwner")
            && value["modelRunId"].as_str().is_some()
    });
    let plan_ipc_outcome = match compile.as_ref() {
        Ok(_) => "RETURNED",
        Err(error) if error == "IPC_RESPONSE_TIMEOUT" => "IPC_TIMEOUT",
        Err(error) if error == "IPC_HANDLER_PANICKED" => "HANDLER_PANICKED",
        Err(_) => "HANDLER_ERROR",
    };
    let (files_scanned, project_canary_matches, project_file_categories) =
        r2_scan_project_tree(temp.path(), &canary);
    let parent_environment_match = std::env::vars().any(|(_, value)| value.contains(&canary));
    stop.store(true, Ordering::SeqCst);
    let server_finished = server.join().is_ok();
    let server_requests = requests.load(Ordering::SeqCst);
    let authorized_requests = auth_matches.load(Ordering::SeqCst);
    let process_canary_matches = process_matches.load(Ordering::SeqCst);
    println!(
        "R2_F01_NEXT_CHAPTER_RUNTIME={}",
        json!({
            "projectCreated": created.is_ok(),
            "prepareCommitted": prepared,
            "planIpcOutcome": plan_ipc_outcome,
            "planErrorClass": r2_safe_error_class(compile.as_ref().err()),
            "planCommitted": compiled,
            "serverRequests": server_requests,
            "authorizedRequests": authorized_requests,
            "processArgvEnvironmentCanaryMatches": process_canary_matches,
            "parentEnvironmentCanaryMatches": parent_environment_match,
            "projectFilesScanned": files_scanned,
            "projectFileCategories": project_file_categories,
            "projectCanaryMatches": project_canary_matches,
            "serverFinished": server_finished
        })
    );
    assert!(
        created.is_ok(),
        "registered project create precondition failed"
    );
    assert!(
        prepared,
        "registered planning_prepare_next_chapter did not commit"
    );
    assert!(
        compiled,
        "registered planning_compile_chapter_plan did not commit through the real Provider"
    );
    assert_eq!(server_requests, 1);
    assert_eq!(authorized_requests, 1);
    assert_eq!(process_canary_matches, 0);
    assert!(!parent_environment_match);
    assert_eq!(project_canary_matches, 0);
}
