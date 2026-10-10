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

fn retryable_real_production_error(error: &str) -> bool {
    [
        "B08_CANON_CONFLICT_CHANGE_REQUEST_REQUIRED",
        "P01_MODEL_JSON_INVALID",
        "B08_PLANNING_CONTRACT",
        "B08_AUDIT_CONTRACT",
        "B08_RENDER_OUTPUT_INVALID",
    ]
    .iter()
    .any(|prefix| error.starts_with(prefix))
}

fn execute_real_production(
    state: &KernelState,
    project_id: &str,
    chapter_id: &str,
    volume_id: Option<&str>,
    execution_mode: &str,
    command_prefix: &str,
    chapter_goal: &str,
) -> serde_json::Value {
    let mut retry_of_job_id = None;
    for attempt in 0..4 {
        let command_id = if attempt == 0 {
            command_prefix.to_string()
        } else {
            format!("{command_prefix}:retry-{attempt}")
        };
        let mut payload = json!({
            "chapterId":chapter_id,
            "executionMode":execution_mode,
            "skillsEnabled":false,
            "targetChineseCharacters":1800,
            "chapterGoalHint":chapter_goal
        });
        if let Some(volume_id) = volume_id {
            payload["volumeId"] = json!(volume_id);
        }
        if let Some(previous) = retry_of_job_id.as_deref() {
            payload["retryOfJobId"] = json!(previous);
        }
        let request = json!({
            "type":"production_start",
            "commandId":command_id,
            "idempotencyKey":format!("{command_id}:key"),
            "projectId":project_id,
            "expectedRevision":current_revision(state, project_id),
            "requestedAt":NOW,
            "payload":payload
        });
        match execute_command_impl(state, request) {
            Ok(response) => {
                assert_eq!(response["finalStatus"], "committed");
                assert_eq!(response["stage"], "COMPLETED");
                return response;
            }
            Err(error) if retryable_real_production_error(&error) && attempt < 3 => {
                retry_of_job_id = Some(
                    StableIdentityFactory::from_command(StableIdKind::Job, &command_id).unwrap(),
                );
            }
            Err(error) => panic!("real production failed: {error}"),
        }
    }
    unreachable!("retry loop returns or panics")
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
    if !paths.root().join("provider-settings.json").is_file() {
        ProviderSettingsRepository::write_global(
            &paths,
            &json!({
                "provider":"OpenAI Compatible",
                "providerId":"openai-compatible",
                "model":"qwen3.8-flash",
                "modelId":"qwen3.8-flash",
                "endpoint":EXPECTED_ENDPOINT,
                "enabled":true,
                "timeoutMs":300000
            }),
        )
        .unwrap();
    }
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
            timeout_ms: 300_000,
        },
        Arc::new(OpenAiCompatibleProvider::new(EXPECTED_ENDPOINT)),
        Arc::new(secret_store),
    );
    let state = KernelState::with_production_gateway(paths.clone(), Arc::new(gateway));

    let (project_id, chapter_ids) = if let Ok(project_id) = std::env::var("MOHENG_REAL_PROJECT_ID")
    {
        let connection = state.manager.open(&project_id, "", NOW).unwrap();
        let mut decision_statement = connection
            .prepare("SELECT decision_id FROM decisions WHERE project_id=?1 AND decision_type='UNKNOWN_REMOTE_OUTCOME' AND outcome IN ('OPEN','PENDING') ORDER BY created_at,decision_id")
            .unwrap();
        let unknown_decisions = decision_statement
            .query_map([&project_id], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        drop(decision_statement);
        let mut statement = connection
            .prepare("SELECT job_id FROM jobs WHERE project_id=?1 AND status IN ('RUNNING','PAUSED','BLOCKED') ORDER BY created_at,job_id")
            .unwrap();
        let active_jobs = statement
            .query_map([&project_id], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        drop(statement);
        drop(connection);
        for (index, decision_id) in unknown_decisions.iter().enumerate() {
            execute(
                &state,
                json!({
                    "type":"decision_submit",
                    "commandId":format!("{prefix}:resolve-unknown:{index}"),
                    "idempotencyKey":format!("{prefix}:resolve-unknown:{index}:key"),
                    "projectId":project_id,
                    "expectedRevision":current_revision(&state, &project_id),
                    "requestedAt":NOW,
                    "payload":{"decisionId":decision_id,"choice":"CANCEL_JOB"}
                }),
            );
        }
        for (index, job_id) in active_jobs.iter().enumerate() {
            execute(
                &state,
                json!({
                    "type":"production_cancel",
                    "commandId":format!("{prefix}:cancel-stale:{index}"),
                    "idempotencyKey":format!("{prefix}:cancel-stale:{index}:key"),
                    "projectId":project_id,
                    "expectedRevision":current_revision(&state, &project_id),
                    "requestedAt":NOW,
                    "payload":{"jobId":job_id}
                }),
            );
        }
        let connection = state.manager.open(&project_id, "", NOW).unwrap();
        let mut statement = connection
            .prepare("SELECT chapter_id FROM chapters WHERE project_id=?1 ORDER BY chapter_no")
            .unwrap();
        let chapter_ids = statement
            .query_map([&project_id], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        drop(statement);
        assert_eq!(
            chapter_ids.len(),
            3,
            "resume project must contain three planned chapters"
        );
        (project_id, chapter_ids)
    } else {
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
        let mut statement = connection
            .prepare(
                "SELECT c.chapter_id FROM chapters c JOIN volumes v ON v.project_id=c.project_id AND v.volume_id=c.volume_id WHERE c.project_id=?1 AND v.volume_no=1 AND c.chapter_no BETWEEN 1 AND ?2 ORDER BY c.chapter_no",
            )
            .unwrap();
        let chapter_ids = statement
            .query_map(
                libsql_rusqlite::params![&project_id, ai_novel_production_system_core::repositories::project_entry::OPENING_CHAPTER_COUNT],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        drop(statement);
        assert_eq!(
            chapter_ids.len(),
            3,
            "opening_ready must create the three chapter topology; the harness never bootstraps SQL"
        );
        drop(connection);
        (project_id, chapter_ids)
    };

    for (index, chapter_id) in chapter_ids.iter().enumerate() {
        let chapter_goal = [
            "完成首次记忆修复委托，并让沈砚亲眼确认修复会抹去自己的现实痕迹。",
            "让沈砚在停止调查与继续追查之间作出主动选择，并找到篡改并非事故的证据。",
            "让沈砚追踪第一条幕后线索，兑现前三章的局部谜题回报，同时保留姐姐去向悬念。",
        ][index];
        let receipt = execute_real_production(
            &state,
            &project_id,
            chapter_id,
            None,
            "OPENING_BOOTSTRAP",
            &format!("{prefix}:produce:{}", index + 1),
            chapter_goal,
        );
        assert!(receipt["finalChapterRef"]["artifactId"].as_str().is_some());
    }

    if std::env::var("MOHENG_RUN_REAL_CLOSED_LOOP").as_deref() == Ok("1") {
        // B02/B03 continue from the same three real FinalChapter rows.
        execute(
            &state,
            json!({
                "type":"opening_evaluate","commandId":format!("{prefix}:opening-evaluate"),
                "idempotencyKey":format!("{prefix}:opening-evaluate:key"),"projectId":project_id,
                "expectedRevision":current_revision(&state, &project_id),"requestedAt":NOW,"payload":{}
            }),
        );
        execute(
            &state,
            json!({
                "type":"book_plan_revise","commandId":format!("{prefix}:book-plan"),
                "idempotencyKey":format!("{prefix}:book-plan:key"),"projectId":project_id,
                "expectedRevision":current_revision(&state, &project_id),"requestedAt":NOW,"payload":{}
            }),
        );
        let connection = state.manager.open(&project_id, "", NOW).unwrap();
        let volume_id: String = connection
            .query_row(
                "SELECT volume_id FROM volumes WHERE project_id=?1 ORDER BY volume_no LIMIT 1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap();
        drop(connection);
        execute(
            &state,
            json!({
                "type":"planning_create_volume_intent","commandId":format!("{prefix}:volume-intent"),
                "idempotencyKey":format!("{prefix}:volume-intent:key"),"projectId":project_id,
                "expectedRevision":current_revision(&state, &project_id),"requestedAt":NOW,"payload":{"volumeId":volume_id}
            }),
        );
        execute(
            &state,
            json!({
                "type":"planning_create_volume_plan","commandId":format!("{prefix}:volume-plan"),
                "idempotencyKey":format!("{prefix}:volume-plan:key"),"projectId":project_id,
                "expectedRevision":current_revision(&state, &project_id),"requestedAt":NOW,"payload":{"volumeId":volume_id}
            }),
        );

        let prepared = execute(
            &state,
            json!({
                "type":"planning_prepare_next_chapter","commandId":format!("{prefix}:chapter-4"),
                "idempotencyKey":format!("{prefix}:chapter-4:key"),"projectId":project_id,
                "expectedRevision":current_revision(&state, &project_id),"requestedAt":NOW,"payload":{"volumeId":volume_id}
            }),
        );
        let chapter4 = prepared["chapter"]["chapterId"]
            .as_str()
            .unwrap()
            .to_owned();
        let plan4 = execute(
            &state,
            json!({
                "type":"planning_compile_chapter_plan","commandId":format!("{prefix}:plan-4"),
                "idempotencyKey":format!("{prefix}:plan-4:key"),"projectId":project_id,
                "expectedRevision":current_revision(&state, &project_id),"requestedAt":NOW,"payload":{"chapterId":chapter4,"chapterGoalHint":"用上一阶段的真实事实推进下一项调查。"}
            }),
        );
        let before = {
            let connection = state.manager.open(&project_id, "", NOW).unwrap();
            (
                connection
                    .query_row::<i64, _, _>(
                        "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1",
                        [&project_id],
                        |row| row.get(0),
                    )
                    .unwrap(),
                connection
                    .query_row::<i64, _, _>(
                        "SELECT COUNT(*) FROM canon_knowledge WHERE project_id=?1",
                        [&project_id],
                        |row| row.get(0),
                    )
                    .unwrap(),
            )
        };
        let production4 = execute_real_production(
            &state,
            &project_id,
            &chapter4,
            Some(&volume_id),
            "LONG_FORM",
            &format!("{prefix}:production-4"),
            "推进调查并让真实选择产生可验证后果。",
        );
        assert_eq!(production4["stage"], "COMPLETED");
        let after = {
            let connection = state.manager.open(&project_id, "", NOW).unwrap();
            (
                connection
                    .query_row::<i64, _, _>(
                        "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1",
                        [&project_id],
                        |row| row.get(0),
                    )
                    .unwrap(),
                connection
                    .query_row::<i64, _, _>(
                        "SELECT COUNT(*) FROM canon_knowledge WHERE project_id=?1",
                        [&project_id],
                        |row| row.get(0),
                    )
                    .unwrap(),
            )
        };
        assert!(
            after.0 > before.0 || after.1 > before.1,
            "Reality must change after the real chapter commit"
        );
        let connection = state.manager.open(&project_id, "", NOW).unwrap();
        let chapter5: String = connection
            .query_row(
                "SELECT chapter_id FROM chapters WHERE project_id=?1 AND volume_id=?2 AND chapter_no=5 AND status='PLANNED'",
                libsql_rusqlite::params![project_id, volume_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(plan4["contextManifestRef"]["artifactId"].as_str().is_some());
        let context4: String = connection.query_row(
            "SELECT json_extract(p.payload_json,'$.contextManifestRef.artifactId') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ChapterPlan' AND json_extract(p.payload_json,'$.chapterId')=?2",
            libsql_rusqlite::params![project_id, chapter4], |row| row.get(0),
        ).unwrap();
        let context5: String = connection.query_row(
            "SELECT json_extract(p.payload_json,'$.contextManifestRef.artifactId') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ChapterPlan' AND json_extract(p.payload_json,'$.chapterId')=?2",
            libsql_rusqlite::params![project_id, chapter5], |row| row.get(0),
        ).unwrap();
        let digest4: String = connection.query_row(
            "SELECT json_extract(p.payload_json,'$.compiledFromStateDigest') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ChapterPlan' AND json_extract(p.payload_json,'$.chapterId')=?2",
            libsql_rusqlite::params![project_id, chapter4], |row| row.get(0),
        ).unwrap();
        let digest5: String = connection.query_row(
            "SELECT json_extract(p.payload_json,'$.compiledFromStateDigest') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ChapterPlan' AND json_extract(p.payload_json,'$.chapterId')=?2",
            libsql_rusqlite::params![project_id, chapter5], |row| row.get(0),
        ).unwrap();
        let context_sources: i64 = connection.query_row(
            "SELECT COUNT(*) FROM artifact_payloads WHERE artifact_id IN (?1,?2) AND json_extract(payload_json,'$.hardSourceCount')>0",
            libsql_rusqlite::params![context4, context5], |row| row.get(0),
        ).unwrap();
        assert_ne!(
            digest4, digest5,
            "next ChapterPlan must use Reality t+1 digest"
        );
        assert_eq!(context_sources, 2);
        println!("REAL_LLM_PRODUCT_CLOSED_LOOP_RESULT projectId={} openingChapters=3 longFormChapter=4 nextChapterPlan=5 realityDeltaFacts={} realityDeltaKnowledge={} contextManifests=2 digestChanged=true provider={} model={} finalStatus=PASS", project_id, after.0 - before.0, after.1 - before.1, EXPECTED_ENDPOINT, EXPECTED_MODEL);
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
    let expected_chapters = if std::env::var("MOHENG_RUN_REAL_CLOSED_LOOP").as_deref() == Ok("1") {
        4
    } else {
        3
    };
    assert!(committed >= expected_chapters);
    assert!(receipts >= expected_chapters);
    assert!(completed_runs >= if expected_chapters == 4 { 13 } else { 9 });
    assert!(word_counts.len() >= expected_chapters as usize);
    assert!(word_counts.iter().all(|count| *count >= 800));
    println!(
        "REAL_THREE_CHAPTER_RESULT projectId={} finalChapters={} productionReceipts={} completedModelRuns={} wordCounts={:?} status=PASS",
        project_id, committed, receipts, completed_runs, word_counts
    );
}
