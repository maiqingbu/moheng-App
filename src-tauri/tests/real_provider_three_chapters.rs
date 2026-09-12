use ai_novel_production_system_core::commands::{
    execute_command_impl, query_story_directions_impl, KernelState,
};
use ai_novel_production_system_core::production::ConfiguredProductionGateway;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::provider::{OpenAiCompatibleProvider, ProviderConfig};
use ai_novel_production_system_core::repositories::provider::ProviderSettingsRepository;
use ai_novel_production_system_core::repositories::runtime::{OsKeychainSecretStore, SecretStore};
use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const NOW: &str = "2026-09-12T06:00:00Z";
const EXPECTED_ENDPOINT: &str =
    "https://ws-a4ht0p7fdc83x4x4.cn-beijing.maas.aliyuncs.com/compatible-mode/v1";
const EXPECTED_MODEL: &str = "qwen3.8-flash";

fn required_app_data_root() -> PathBuf {
    let raw = std::env::var("MOHENG_REAL_APP_DATA_DIR")
        .expect("MOHENG_REAL_APP_DATA_DIR must point to the configured desktop app data directory");
    let root = PathBuf::from(raw);
    assert!(
        root.is_absolute(),
        "MOHENG_REAL_APP_DATA_DIR must be absolute"
    );
    root
}

fn current_revision(state: &KernelState, project_id: &str) -> i64 {
    state.manager.overview(project_id).unwrap().1
}

fn execute(state: &KernelState, request: serde_json::Value) -> serde_json::Value {
    let response = execute_command_impl(state, request).unwrap();
    assert_eq!(response["finalStatus"], "committed");
    response
}

#[test]
#[ignore = "real provider acceptance: creates a persistent project and spends configured API budget"]
fn configured_provider_generates_and_reloads_three_real_chapters() {
    assert_eq!(
        std::env::var("MOHENG_RUN_REAL_THREE_CHAPTERS").as_deref(),
        Ok("1"),
        "set MOHENG_RUN_REAL_THREE_CHAPTERS=1 to acknowledge real API use"
    );
    let root = required_app_data_root();
    let paths = AppPaths::new(&root);
    let settings = ProviderSettingsRepository::read_global(&paths).unwrap();
    assert_eq!(settings["endpoint"], EXPECTED_ENDPOINT);
    assert_eq!(settings["modelId"], EXPECTED_MODEL);
    assert_eq!(settings["enabled"], true);
    assert!(
        settings.get("apiKey").is_none(),
        "settings file must stay secret-free"
    );
    let secret_store =
        OsKeychainSecretStore::new("com.moheng.ai-novel-production", "provider-api-key");
    let secret = secret_store.get("openai-compatible").unwrap();
    assert!(!secret.trim().is_empty());
    drop(secret);

    let run = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let prefix = format!("real-qwen-three-chapters:{run}");
    let title = format!("Harness 真实三章验收 {run}");
    let gateway = ConfiguredProductionGateway::new(
        ProviderConfig {
            provider_id: "openai-compatible".into(),
            model_id: EXPECTED_MODEL.into(),
            endpoint: EXPECTED_ENDPOINT.into(),
            enabled: true,
            timeout_ms: 120_000,
        },
        Arc::new(OpenAiCompatibleProvider::new(EXPECTED_ENDPOINT)),
        Arc::new(secret_store),
    );
    let state = KernelState::with_production_gateway(paths.clone(), Arc::new(gateway));

    let created = execute(
        &state,
        json!({
            "type":"project_create_from_idea",
            "commandId":format!("{prefix}:create"),
            "idempotencyKey":format!("{prefix}:create:key"),
            "requestedAt":NOW,
            "payload":{
                "title":title,
                "idea":"一名负责修复城市记忆档案的年轻调查员，发现每修复一段被篡改的记忆，现实中就会失去一个与自己有关的真实痕迹。前三章必须完成首次委托、发现代价、主动追查幕后来源。",
                "platform":"长篇网络小说",
                "mode":"DIRECTOR",
                "tags":["都市悬疑","成长","记忆谜案"],
                "forbidden":"不使用梦境醒来推翻前文，不用系统面板，不提前揭露幕后真凶"
            }
        }),
    );
    let project_id = created["projectId"].as_str().unwrap().to_owned();

    execute(
        &state,
        json!({
            "type":"story_directions_generate",
            "commandId":format!("{prefix}:directions"),
            "idempotencyKey":format!("{prefix}:directions:key"),
            "projectId":project_id,
            "expectedRevision":current_revision(&state, &project_id),
            "requestedAt":NOW,
            "payload":{}
        }),
    );
    let directions = query_story_directions_impl(
        &state,
        json!({"projectId":project_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    let pitches = directions["pitches"].as_array().unwrap();
    assert_eq!(pitches.len(), 3);
    let pitch_id = pitches[0]["id"].as_str().unwrap();

    execute(
        &state,
        json!({
            "type":"pitch_select",
            "commandId":format!("{prefix}:select"),
            "idempotencyKey":format!("{prefix}:select:key"),
            "projectId":project_id,
            "expectedRevision":current_revision(&state, &project_id),
            "requestedAt":NOW,
            "payload":{"pitchIds":[pitch_id],"mode":"single"}
        }),
    );
    execute(
        &state,
        json!({
            "type":"story_setup_save",
            "commandId":format!("{prefix}:setup"),
            "idempotencyKey":format!("{prefix}:setup:key"),
            "projectId":project_id,
            "expectedRevision":current_revision(&state, &project_id),
            "requestedAt":NOW,
            "payload":{"storyCore":{
                "protagonist":"城市记忆档案调查员沈砚",
                "desire":"找回被人从公共档案中抹去的姐姐",
                "mechanism":"修复被篡改的记忆会同步抹去修复者的一段现实痕迹",
                "conflict":"维护城市稳定的档案局要求掩盖真相，沈砚必须在自我消失前留下证据",
                "readerPromise":"每章解决一个可验证的小谜题，同时让失去痕迹的代价持续升级",
                "forbiddenReveals":["前三章不得揭晓最终幕后者","不得证明姐姐已经死亡"]
            }}
        }),
    );
    execute(
        &state,
        json!({
            "type":"opening_ready",
            "commandId":format!("{prefix}:opening"),
            "idempotencyKey":format!("{prefix}:opening:key"),
            "projectId":project_id,
            "expectedRevision":current_revision(&state, &project_id),
            "requestedAt":NOW,
            "payload":{"approved":true}
        }),
    );

    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let (volume_id, first_chapter_id): (String, String) = connection
        .query_row(
            "SELECT v.volume_id,c.chapter_id FROM volumes v JOIN chapters c ON c.project_id=v.project_id AND c.volume_id=v.volume_id WHERE v.project_id=?1 AND c.chapter_no=1",
            [&project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let mut chapter_ids = vec![first_chapter_id];
    for chapter_no in 2..=3 {
        let chapter_id = StableIdentityFactory::from_command(
            StableIdKind::Chapter,
            &format!("{prefix}:chapter:{chapter_no}"),
        )
        .unwrap();
        connection
            .execute(
                "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'IMPORTANT','PLANNED',0,?6,?6)",
                libsql_rusqlite::params![chapter_id, project_id, volume_id, chapter_no, format!("第{chapter_no}章"), NOW],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO chapter_production_budgets(budget_id,project_id,chapter_id,importance_class,hard_max_model_runs,cost_budget_cents,frozen_at) VALUES (?1,?2,?3,'IMPORTANT',12,500,?4)",
                libsql_rusqlite::params![format!("budget:{project_id}:opening:{chapter_no}"), project_id, chapter_id, NOW],
            )
            .unwrap();
        chapter_ids.push(chapter_id);
    }
    drop(connection);

    for (index, chapter_id) in chapter_ids.iter().enumerate() {
        let receipt = execute(
            &state,
            json!({
                "type":"production_start",
                "commandId":format!("{prefix}:produce:{}", index + 1),
                "idempotencyKey":format!("{prefix}:produce:{}:key", index + 1),
                "projectId":project_id,
                "expectedRevision":current_revision(&state, &project_id),
                "requestedAt":NOW,
                "payload":{
                    "chapterId":chapter_id,
                    "executionMode":"OPENING_BOOTSTRAP",
                    "skillsEnabled":false,
                    "targetChineseCharacters":1800
                }
            }),
        );
        assert_eq!(receipt["stage"], "COMPLETED");
        assert!(receipt["finalChapterRef"]["artifactId"].as_str().is_some());
    }
    drop(state);

    let reloaded = ProjectManager::new(paths);
    let connection = reloaded.open(&project_id, "", NOW).unwrap();
    let committed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM chapters WHERE project_id=?1 AND status='COMMITTED' AND final_artifact_id IS NOT NULL",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let receipts: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='ProductionReceipt' AND artifact_status='COMMITTED'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let completed_runs: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM model_runs WHERE project_id=?1 AND provider='openai-compatible' AND model=?2 AND capability IN ('PLAN_STRATEGY','RENDER_CHAPTER','AUDIT_EXTRACT') AND status='COMPLETED'",
            libsql_rusqlite::params![project_id, EXPECTED_MODEL],
            |row| row.get(0),
        )
        .unwrap();
    let mut statement = connection
        .prepare(
            "SELECT CAST(json_extract(p.payload_json,'$.wordCount') AS INTEGER) FROM chapters c JOIN artifact_payloads p ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version WHERE c.project_id=?1 ORDER BY c.chapter_no",
        )
        .unwrap();
    let word_counts = statement
        .query_map([&project_id], |row| row.get::<_, i64>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(committed, 3);
    assert_eq!(receipts, 3);
    assert_eq!(completed_runs, 9);
    assert_eq!(word_counts.len(), 3);
    assert!(word_counts.iter().all(|count| *count >= 800));
    println!(
        "REAL_THREE_CHAPTER_RESULT projectId={} finalChapters={} productionReceipts={} completedModelRuns={} wordCounts={:?} status=PASS",
        project_id, committed, receipts, completed_runs, word_counts
    );
}
