use ai_novel_production_system_core::commands::{
    execute_command_impl, query_story_directions_impl, KernelState,
};
use ai_novel_production_system_core::production::{
    ConfiguredProductionGateway, ProductionModelGateway, ProductionModelResponse,
    ResolvedProviderProfile,
};
use ai_novel_production_system_core::repositories::runtime::TransportFailure;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
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
const OPENING_GOALS: [&str; 3] = [
    "完成首次记忆修复委托，并让沈砚亲眼确认修复会抹去自己的现实痕迹。",
    "让沈砚在停止调查与继续追查之间作出主动选择，并找到篡改并非事故的证据。",
    "让沈砚追踪第一条幕后线索，兑现前三章的局部谜题回报，同时保留姐姐去向悬念。",
];

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
        // Every plan rejection the planner can fix (beats, role, length,
        // repeated goal) is retried like model output variance.
        "B08_PLAN_",
        "B08_RENDER_LENGTH_OUT_OF_RANGE",
        "B08_AUDIT_INCOMPLETE",
        "B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS",
        // A declared reservation the audit says this chapter revealed early:
        // the chapter is written again, as the user would.
        "B08_RESERVATION_VIOLATION",
        "B08_AUDIT_TRUNCATED_OR_INVALID",
        "ROLLING_HORIZON_CAPACITY_INVALID",
    ]
    .iter()
    .any(|prefix| error.starts_with(prefix))
}

fn budget_exhausted(error: &str) -> bool {
    error.contains("BUDGET_EXCEEDED") || error.contains("budget hard limit")
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
    // A quota reset costs an attempt, so a chapter gets six.
    for attempt in 0..6 {
        let command_id = if attempt == 0 {
            command_prefix.to_string()
        } else {
            format!("{command_prefix}:retry-{attempt}")
        };
        let mut payload = json!({
            "chapterId":chapter_id,
            "executionMode":execution_mode,
            "skillsEnabled":false,
            "chapterLengthRange":{"min":2000,"max":3000},
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
            Err(error) if budget_exhausted(&error) && attempt < 5 => {
                // What the user does in the app: reset this chapter's quota.
                execute(
                    state,
                    json!({
                        "type":"production_reset_chapter_quota",
                        "commandId":format!("{command_id}:quota-reset"),
                        "idempotencyKey":format!("{command_id}:quota-reset:key"),
                        "projectId":project_id,
                        "expectedRevision":current_revision(state, project_id),
                        "requestedAt":NOW,
                        "payload":{"chapterId":chapter_id}
                    }),
                );
                retry_of_job_id = Some(
                    StableIdentityFactory::from_command(StableIdKind::Job, &command_id).unwrap(),
                );
            }
            Err(error) if error.contains("UNKNOWN_REMOTE_OUTCOME") && attempt < 5 => {
                // What the user does in the app: cancel the job whose model
                // call has an unknown outcome, then produce the chapter again.
                let connection = state.manager.open(project_id, "", NOW).unwrap();
                let decisions = connection
                    .prepare("SELECT decision_id FROM decisions WHERE project_id=?1 AND decision_type='UNKNOWN_REMOTE_OUTCOME' AND outcome IN ('OPEN','PENDING') ORDER BY created_at,decision_id")
                    .unwrap()
                    .query_map([project_id], |row| row.get::<_, String>(0))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                drop(connection);
                for (index, decision_id) in decisions.iter().enumerate() {
                    execute(
                        state,
                        json!({
                            "type":"decision_submit",
                            "commandId":format!("{command_id}:resolve-unknown:{index}"),
                            "idempotencyKey":format!("{command_id}:resolve-unknown:{index}:key"),
                            "projectId":project_id,
                            "expectedRevision":current_revision(state, project_id),
                            "requestedAt":NOW,
                            "payload":{"decisionId":decision_id,"choice":"CANCEL_JOB"}
                        }),
                    );
                }
                retry_of_job_id = None;
            }
            Err(error) if retryable_real_production_error(&error) && attempt < 5 => {
                retry_of_job_id = Some(
                    StableIdentityFactory::from_command(StableIdKind::Job, &command_id).unwrap(),
                );
            }
            Err(error) => panic!("real production failed: {error}"),
        }
    }
    unreachable!("retry loop returns or panics")
}

/// The desktop app's data directory, checked to be configured for the real
/// provider, and a kernel composed the same way as the desktop app.
fn real_state() -> (AppPaths, KernelState) {
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

    // Same composition as the desktop app: provider settings, timeout and the
    // model price table are read from the app data directory.
    let gateway = ConfiguredProductionGateway::for_desktop(paths.clone());
    let state = KernelState::with_production_gateway(paths.clone(), Arc::new(gateway));
    (paths, state)
}

/// A new project from the harness idea, taken through directions, setup
/// and opening readiness; returns it with its three opening chapters.
fn create_real_project(state: &KernelState, prefix: &str, title: &str) -> (String, Vec<String>) {
    let created = execute(
        state,
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
        state,
        json!({
            "type":"story_directions_generate",
            "commandId":format!("{prefix}:directions"),
            "idempotencyKey":format!("{prefix}:directions:key"),
            "projectId":project_id,
            "expectedRevision":current_revision(state, &project_id),
            "requestedAt":NOW,
            "payload":{}
        }),
    );
    let directions = query_story_directions_impl(
        state,
        json!({"projectId":project_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    let pitches = directions["pitches"].as_array().unwrap();
    assert_eq!(pitches.len(), 3);
    let pitch_id = pitches[0]["id"].as_str().unwrap();

    execute(
        state,
        json!({
            "type":"pitch_select",
            "commandId":format!("{prefix}:select"),
            "idempotencyKey":format!("{prefix}:select:key"),
            "projectId":project_id,
            "expectedRevision":current_revision(state, &project_id),
            "requestedAt":NOW,
            "payload":{"pitchIds":[pitch_id],"mode":"single"}
        }),
    );
    execute(
        state,
        json!({
            "type":"story_setup_save",
            "commandId":format!("{prefix}:setup"),
            "idempotencyKey":format!("{prefix}:setup:key"),
            "projectId":project_id,
            "expectedRevision":current_revision(state, &project_id),
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
        state,
        json!({
            "type":"opening_ready",
            "commandId":format!("{prefix}:opening"),
            "idempotencyKey":format!("{prefix}:opening:key"),
            "projectId":project_id,
            "expectedRevision":current_revision(state, &project_id),
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
            libsql_rusqlite::params![
                &project_id,
                ai_novel_production_system_core::repositories::project_entry::OPENING_CHAPTER_COUNT
            ],
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
}

#[test]
#[ignore = "real provider acceptance: creates a persistent project and spends configured API budget"]
fn configured_provider_generates_and_reloads_three_real_chapters() {
    assert_eq!(
        std::env::var("MOHENG_RUN_REAL_THREE_CHAPTERS").as_deref(),
        Ok("1"),
        "set MOHENG_RUN_REAL_THREE_CHAPTERS=1 to acknowledge real API use"
    );
    let (paths, state) = real_state();
    let run = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let prefix = format!("real-qwen-three-chapters:{run}");
    let title = format!("Harness 真实三章验收 {run}");

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
        create_real_project(&state, &prefix, &title)
    };

    for (index, chapter_id) in chapter_ids.iter().enumerate() {
        let chapter_goal = [
            "完成首次记忆修复委托，并让沈砚亲眼确认修复会抹去自己的现实痕迹。",
            "让沈砚在停止调查与继续追查之间作出主动选择，并找到篡改并非事故的证据。",
            "让沈砚追踪第一条幕后线索，兑现前三章的局部谜题回报，同时保留姐姐去向悬念。",
        ][index];
        let committed = state
            .manager
            .open(&project_id, "", NOW)
            .unwrap()
            .query_row(
                "SELECT status='COMMITTED' FROM chapters WHERE project_id=?1 AND chapter_id=?2",
                [project_id.as_str(), chapter_id.as_str()],
                |row| row.get::<_, bool>(0),
            )
            .unwrap();
        if committed {
            // Resume mode: a chapter committed by an earlier real run is kept.
            continue;
        }
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
            // Chapter prose may use the configured writing model (renderModelId).
            "SELECT COUNT(*) FROM model_runs WHERE project_id=?1 AND provider='openai-compatible' AND (model=?2 OR capability='RENDER_CHAPTER') AND capability IN ('PLAN_STRATEGY','RENDER_CHAPTER','AUDIT_EXTRACT') AND status='COMPLETED'",
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
    assert!(word_counts
        .iter()
        .all(|count| (2000..=3300).contains(count)));
    println!(
        "REAL_THREE_CHAPTER_RESULT projectId={} finalChapters={} productionReceipts={} completedModelRuns={} wordCounts={:?} status=PASS",
        project_id, committed, receipts, completed_runs, word_counts
    );
}

fn chapter_plan_exists(state: &KernelState, project_id: &str, chapter_id: &str) -> bool {
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    ai_novel_production_system_core::repositories::narrative::NarrativeRepository::canonical_head_ref(
        &connection,
        project_id,
        "ChapterPlan",
        chapter_id,
    )
    .unwrap()
    .is_some()
}

/// Long-form plans are compiled by the model too, so a fixable rejection is
/// retried like production.
fn compile_real_chapter_plan(
    state: &KernelState,
    project_id: &str,
    chapter_id: &str,
    prefix: &str,
) {
    for attempt in 0..3 {
        let command_id = format!("{prefix}:attempt-{attempt}");
        let result = execute_command_impl(
            state,
            json!({
                "type":"planning_compile_chapter_plan","commandId":command_id,
                "idempotencyKey":format!("{command_id}:key"),"projectId":project_id,
                "expectedRevision":current_revision(state, project_id),"requestedAt":NOW,
                "payload":{"chapterId":chapter_id,"chapterLengthRange":{"min":2000,"max":3000},
                    "chapterGoalHint":"承接上一章结尾，推进主线调查，并让主角的选择产生可核对的后果。"}
            }),
        );
        match result {
            Ok(response) => {
                assert_eq!(response["finalStatus"], "committed");
                return;
            }
            Err(error) if retryable_real_production_error(&error) && attempt < 2 => {}
            Err(error) => panic!("real chapter plan failed: {error}"),
        }
    }
}

#[test]
#[ignore = "real provider acceptance: ten chapters and the signing evaluation; spends configured API budget"]
fn configured_provider_writes_ten_chapters_and_evaluates_signing() {
    assert_eq!(
        std::env::var("MOHENG_RUN_REAL_TEN_CHAPTERS").as_deref(),
        Ok("1"),
        "set MOHENG_RUN_REAL_TEN_CHAPTERS=1 to acknowledge real API use"
    );
    let (_paths, state) = real_state();
    let run = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let prefix = format!("real-ten-chapters:{run}");
    let title = format!("Harness 真实十章验收 {run}");
    let (project_id, opening) = create_real_project(&state, &prefix, &title);
    let opening_goals = OPENING_GOALS;
    for (index, chapter_id) in opening.iter().enumerate() {
        execute_real_production(
            &state,
            &project_id,
            chapter_id,
            None,
            "OPENING_BOOTSTRAP",
            &format!("{prefix}:produce:{}", index + 1),
            opening_goals[index],
        );
    }

    // Signing evaluation of the golden three chapters.
    let evaluation = execute(
        &state,
        json!({
            "type":"opening_evaluate","commandId":format!("{prefix}:opening-evaluate"),
            "idempotencyKey":format!("{prefix}:opening-evaluate:key"),"projectId":project_id,
            "expectedRevision":current_revision(&state, &project_id),"requestedAt":NOW,"payload":{}
        }),
    );
    let signing = {
        let connection = state.manager.open(&project_id, "", NOW).unwrap();
        ai_novel_production_system_core::repositories::production::ProductionRepository::center_projection(
            &connection,
            &project_id,
        )
        .unwrap()["signing"]
            .clone()
    };
    println!("REAL_SIGNING_EVALUATION response={evaluation} signing={signing}");
    // Owner decision (2026-10-18): this throwaway acceptance project is
    // confirmed for Fanqie, the platform the evaluation passed.
    let fanqie = signing["platformMatrix"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item["platformId"] == "FANQIE" && item["status"] == "PASS")
        .expect("Fanqie passes the signing evaluation");
    execute(
        &state,
        json!({
            "type":"signing_confirm","commandId":format!("{prefix}:signing-confirm"),
            "idempotencyKey":format!("{prefix}:signing-confirm:key"),"projectId":project_id,
            "expectedRevision":current_revision(&state, &project_id),"requestedAt":NOW,
            "payload":{"platformId":fanqie["platformId"]}
        }),
    );

    for (step, command) in [
        ("book-plan", "book_plan_revise"),
        ("volume-intent", "planning_create_volume_intent"),
        ("volume-plan", "planning_create_volume_plan"),
    ] {
        let volume_id: String = state
            .manager
            .open(&project_id, "", NOW)
            .unwrap()
            .query_row(
                "SELECT volume_id FROM volumes WHERE project_id=?1 ORDER BY volume_no LIMIT 1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap();
        execute(
            &state,
            json!({
                "type":command,"commandId":format!("{prefix}:{step}"),
                "idempotencyKey":format!("{prefix}:{step}:key"),"projectId":project_id,
                "expectedRevision":current_revision(&state, &project_id),"requestedAt":NOW,
                "payload":{"volumeId":volume_id}
            }),
        );
    }
    let volume_id: String = state
        .manager
        .open(&project_id, "", NOW)
        .unwrap()
        .query_row(
            "SELECT volume_id FROM volumes WHERE project_id=?1 ORDER BY volume_no LIMIT 1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();

    for number in 4..=10_i64 {
        let existing: Option<String> = state
            .manager
            .open(&project_id, "", NOW)
            .unwrap()
            .query_row(
                "SELECT chapter_id FROM chapters WHERE project_id=?1 AND chapter_no=?2",
                libsql_rusqlite::params![project_id, number],
                |row| row.get(0),
            )
            .ok();
        let chapter_id = match existing {
            Some(chapter_id) => chapter_id,
            None => execute(
                &state,
                json!({
                    "type":"planning_prepare_next_chapter","commandId":format!("{prefix}:prepare-{number}"),
                    "idempotencyKey":format!("{prefix}:prepare-{number}:key"),"projectId":project_id,
                    "expectedRevision":current_revision(&state, &project_id),"requestedAt":NOW,
                    "payload":{"volumeId":volume_id}
                }),
            )["chapter"]["chapterId"]
                .as_str()
                .unwrap()
                .to_owned(),
        };
        if !chapter_plan_exists(&state, &project_id, &chapter_id) {
            compile_real_chapter_plan(
                &state,
                &project_id,
                &chapter_id,
                &format!("{prefix}:plan-{number}"),
            );
        }
        execute_real_production(
            &state,
            &project_id,
            &chapter_id,
            Some(&volume_id),
            "LONG_FORM",
            &format!("{prefix}:produce:{number}"),
            "承接上一章结尾，推进主线调查，并让主角的选择产生可核对的后果。",
        );
    }

    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let word_counts = connection
        .prepare(
            "SELECT c.chapter_no, CAST(json_extract(p.payload_json,'$.wordCount') AS INTEGER) FROM chapters c JOIN artifact_payloads p ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version WHERE c.project_id=?1 ORDER BY c.chapter_no",
        )
        .unwrap()
        .query_map([&project_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let runs: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM model_runs WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(word_counts.len(), 10, "{word_counts:?}");
    assert!(word_counts
        .iter()
        .all(|(_, count)| (2000..=3300).contains(count)));
    println!(
        "REAL_TEN_CHAPTER_RESULT projectId={project_id} wordCounts={word_counts:?} modelRuns={runs} status=PASS"
    );
}

/// The configured writer without the v2 craft skills: no skill guidance in
/// any request and no prose review or rewrite. Everything else is the same.
struct NoCraftGateway(ConfiguredProductionGateway);

fn without_craft(input: &serde_json::Value) -> serde_json::Value {
    let mut input = input.clone();
    if let Some(object) = input.as_object_mut() {
        object.remove("craftGuidance");
    }
    input
}

impl ProductionModelGateway for NoCraftGateway {
    fn provider(&self) -> &str {
        self.0.provider()
    }
    fn model(&self, capability: &str) -> String {
        self.0.model(capability)
    }
    fn model_pricing_catalog(
        &self,
        now: &str,
    ) -> Result<ai_novel_production_system_core::model_cost::ModelPricingCatalog, String> {
        self.0.model_pricing_catalog(now)
    }
    fn transport_diagnostics_enabled(&self) -> bool {
        self.0.transport_diagnostics_enabled()
    }
    fn enforces_chapter_length(&self) -> bool {
        self.0.enforces_chapter_length()
    }
    fn reviews_chapter_prose(&self) -> bool {
        false
    }
    fn classifies_assistant_intent(&self) -> bool {
        self.0.classifies_assistant_intent()
    }
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        self.0.resolve_profile()
    }
    fn resolve_step_profile(&self, capability: &str) -> Result<ResolvedProviderProfile, String> {
        self.0.resolve_step_profile(capability)
    }
    fn invoke_typed_with_profile(
        &self,
        capability: &str,
        input: &serde_json::Value,
        profile: &ResolvedProviderProfile,
    ) -> Result<ProductionModelResponse, TransportFailure> {
        self.0
            .invoke_typed_with_profile(capability, &without_craft(input), profile)
    }
    fn invoke_typed_with_diagnostic(
        &self,
        capability: &str,
        input: &serde_json::Value,
        profile: Option<&ResolvedProviderProfile>,
    ) -> (
        Result<ProductionModelResponse, TransportFailure>,
        Option<ai_novel_production_system_core::provider::ProviderAttemptDiagnostic>,
    ) {
        self.0
            .invoke_typed_with_diagnostic(capability, &without_craft(input), profile)
    }
    fn invoke(
        &self,
        capability: &str,
        input: &serde_json::Value,
    ) -> Result<ProductionModelResponse, String> {
        self.0.invoke(capability, &without_craft(input))
    }
    fn invoke_typed(
        &self,
        capability: &str,
        input: &serde_json::Value,
    ) -> Result<ProductionModelResponse, TransportFailure> {
        self.0.invoke_typed(capability, &without_craft(input))
    }
}

fn opening_chapter_ids(state: &KernelState, project_id: &str) -> Vec<String> {
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    let mut statement = connection
        .prepare("SELECT chapter_id FROM chapters WHERE project_id=?1 AND chapter_no<=3 ORDER BY chapter_no")
        .unwrap();
    let ids = statement
        .query_map([project_id], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    ids
}

fn committed_bodies(state: &KernelState, project_id: &str) -> Vec<String> {
    let connection = state.manager.open(project_id, "", NOW).unwrap();
    let mut statement = connection
        .prepare("SELECT json_extract(p.payload_json,'$.body') FROM chapters c JOIN artifact_payloads p ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version WHERE c.project_id=?1 ORDER BY c.chapter_no")
        .unwrap();
    let bodies = statement
        .query_map([project_id], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    bodies
}

#[test]
#[ignore = "real provider A/B: the golden three chapters with and without the v2 craft skills; spends configured API budget"]
fn configured_provider_compares_the_craft_skills_on_and_off() {
    assert_eq!(
        std::env::var("MOHENG_RUN_REAL_CRAFT_AB").as_deref(),
        Ok("1"),
        "set MOHENG_RUN_REAL_CRAFT_AB=1 to acknowledge real API use"
    );
    let out = PathBuf::from(std::env::var("MOHENG_CRAFT_AB_OUT").expect("MOHENG_CRAFT_AB_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let (paths, with_craft) = real_state();
    let without_craft_state = KernelState::with_production_gateway(
        paths.clone(),
        Arc::new(NoCraftGateway(ConfiguredProductionGateway::for_desktop(paths))),
    );
    let run = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let prefix = format!("real-craft-ab:{run}");
    // One setup, copied, so both arms write from the same story.
    let (on_project, _) = create_real_project(&with_craft, &prefix, &format!("Harness 技能对照 {run}"));
    let copy = execute(
        &with_craft,
        json!({
            "type":"project_duplicate","commandId":format!("{prefix}:copy"),
            "idempotencyKey":format!("{prefix}:copy:key"),"projectId":on_project,
            "expectedRevision":current_revision(&with_craft, &on_project),"requestedAt":NOW
        }),
    );
    let off_project = copy["resultProjectId"].as_str().unwrap().to_owned();
    for (arm, state, project_id) in [
        ("on", &with_craft, &on_project),
        ("off", &without_craft_state, &off_project),
    ] {
        for (index, chapter_id) in opening_chapter_ids(state, project_id).iter().enumerate() {
            execute_real_production(
                state,
                project_id,
                chapter_id,
                None,
                "OPENING_BOOTSTRAP",
                &format!("{prefix}:{arm}:produce:{}", index + 1),
                OPENING_GOALS[index],
            );
        }
        let evaluation = execute(
            state,
            json!({
                "type":"opening_evaluate","commandId":format!("{prefix}:{arm}:evaluate"),
                "idempotencyKey":format!("{prefix}:{arm}:evaluate:key"),"projectId":project_id,
                "expectedRevision":current_revision(state, project_id),"requestedAt":NOW,"payload":{}
            }),
        );
        let signing = {
            let connection = state.manager.open(project_id, "", NOW).unwrap();
            ai_novel_production_system_core::repositories::production::ProductionRepository::center_projection(
                &connection,
                project_id,
            )
            .unwrap()["signing"]
                .clone()
        };
        std::fs::write(out.join(format!("{arm}-signing.json")), signing.to_string()).unwrap();
        for (index, body) in committed_bodies(state, project_id).iter().enumerate() {
            std::fs::write(out.join(format!("{arm}-chapter-{}.txt", index + 1)), body).unwrap();
        }
        println!("REAL_CRAFT_AB arm={arm} projectId={project_id} evaluation={}", evaluation["finalStatus"]);
    }
}

const PAIR_JUDGE_PROMPT: &str = "你是网文签约编辑。下面是同一章的两个版本 versionA 与 versionB，情节设定相同，作者匿名。请只比较写作质量，逐项判断哪一版更好：1 AI腔与套话更少（无模板化比喻堆砌、无伪精确数字、无反复的身体小动作）；2 展示而非交代；3 语言自然、不堆砌；4 人物与对话可信、有区分度；5 节奏与悬念；6 整体可读性与追读欲。只返回 JSON：{\"dimensions\":[{\"name\":string,\"winner\":\"A\"|\"B\"|\"TIE\",\"reason\":string}],\"overall\":\"A\"|\"B\"|\"TIE\",\"reason\":string}。reason 用中文短句，可引用原文。";

/// Blind pairwise judgement of one chapter; returns the parsed verdict.
fn judge_pair(
    gateway: &ConfiguredProductionGateway,
    version_a: &str,
    version_b: &str,
) -> serde_json::Value {
    let profile = gateway.resolve_step_profile("AUDIT_EXTRACT").unwrap();
    let response = gateway
        .invoke_typed_with_profile(
            "JUDGE_CHAPTER_PAIR",
            &json!({"prompt":PAIR_JUDGE_PROMPT,"versionA":version_a,"versionB":version_b}),
            &profile,
        )
        .unwrap_or_else(|failure| panic!("judge call failed: {failure:?}"));
    // The configured provider returns the raw completion; its message
    // content is the verdict JSON, possibly fenced.
    let raw: serde_json::Value =
        serde_json::from_str(response.raw_response.as_deref().unwrap_or("null")).unwrap();
    let content = raw["choices"][0]["message"]["content"].as_str().unwrap_or("").trim();
    let content = content
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    serde_json::from_str(content).unwrap_or_else(|_| json!({"unparsed":content}))
}

#[test]
#[ignore = "real provider: blind pairwise judgement of the craft A/B chapters; a few paid calls"]
fn configured_provider_judges_the_craft_ab_chapters() {
    assert_eq!(
        std::env::var("MOHENG_RUN_REAL_CRAFT_AB").as_deref(),
        Ok("1"),
        "set MOHENG_RUN_REAL_CRAFT_AB=1 to acknowledge real API use"
    );
    let out = PathBuf::from(std::env::var("MOHENG_CRAFT_AB_OUT").expect("MOHENG_CRAFT_AB_OUT"));
    let (paths, _) = real_state();
    let gateway = ConfiguredProductionGateway::for_desktop(paths);
    let mut verdicts = Vec::new();
    let mut tally = std::collections::BTreeMap::<String, i64>::new();
    for chapter in 1..=3 {
        let on = std::fs::read_to_string(out.join(format!("on-chapter-{chapter}.txt"))).unwrap();
        let off = std::fs::read_to_string(out.join(format!("off-chapter-{chapter}.txt"))).unwrap();
        // Both orders, twice each, so a preference for a position and the
        // judge's own variance even out.
        for (order, a, b, a_arm, b_arm) in [
            ("on-first", &on, &off, "on", "off"),
            ("off-first", &off, &on, "off", "on"),
            ("on-first", &on, &off, "on", "off"),
            ("off-first", &off, &on, "off", "on"),
        ] {
            let verdict = judge_pair(&gateway, a, b);
            let arm = |winner: &serde_json::Value| match winner.as_str() {
                Some("A") => a_arm,
                Some("B") => b_arm,
                _ => "tie",
            };
            for dimension in verdict["dimensions"].as_array().into_iter().flatten() {
                *tally
                    .entry(format!("{}:{}", dimension["name"].as_str().unwrap_or("?"), arm(&dimension["winner"])))
                    .or_default() += 1;
            }
            *tally.entry(format!("overall:{}", arm(&verdict["overall"]))).or_default() += 1;
            verdicts.push(json!({"chapter":chapter,"order":order,"verdict":verdict}));
        }
    }
    std::fs::write(
        out.join("judge.json"),
        serde_json::to_string_pretty(&json!({"tally":tally,"verdicts":verdicts})).unwrap(),
    )
    .unwrap();
    println!("REAL_CRAFT_JUDGE tally={}", json!(tally));
}
