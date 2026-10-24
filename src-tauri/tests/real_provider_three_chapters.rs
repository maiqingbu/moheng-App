use ai_novel_production_system_core::commands::{
    execute_command_impl, query_story_directions_impl, KernelState,
};
use ai_novel_production_system_core::production::{
    ConfiguredProductionGateway, ProductionModelGateway, ProductionModelResponse,
    ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::repositories::provider::ProviderSettingsRepository;
use ai_novel_production_system_core::repositories::runtime::TransportFailure;
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

/// A setup step that calls the model, generated again (as the user would
/// press regenerate) when the model's JSON comes back cut off. Seen on
/// 2026-10-24: qwen3.8-flash stopped mid-object with finish_reason stop.
fn execute_model_step(state: &KernelState, request: serde_json::Value) -> serde_json::Value {
    const ATTEMPTS: usize = 3;
    for attempt in 1..=ATTEMPTS {
        let mut request = request.clone();
        if attempt > 1 {
            for key in ["commandId", "idempotencyKey"] {
                request[key] = json!(format!("{}:try{attempt}", request[key].as_str().unwrap()));
            }
            let project_id = request["projectId"].as_str().unwrap().to_owned();
            request["expectedRevision"] = json!(current_revision(state, &project_id));
        }
        match execute_command_impl(state, request) {
            Ok(response) => {
                assert_eq!(response["finalStatus"], "committed");
                return response;
            }
            Err(error) if error.contains("P01_MODEL_JSON_INVALID") && attempt < ATTEMPTS => {
                println!("REAL_SETUP_REGENERATE attempt={attempt} error={error}");
            }
            Err(error) => panic!("real setup failed: {error}"),
        }
    }
    unreachable!("the last attempt returns or panics")
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
        // 2026-10-24: the audit named a new thread "thread-1" although the
        // prompt asks for "thread:<id>"; written again like other variance.
        "B08_THREAD_DISPOSITION_INVALID",
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

/// Writes the writer model (`MOHENG_REAL_RENDER_MODEL`) and the default
/// model (`MOHENG_REAL_MAIN_MODEL`) into the global provider settings for one
/// run and restores the previous settings when dropped, even when the run
/// fails. The settings file is secret-free; the API key stays in the keychain.
struct ModelOverride {
    paths: AppPaths,
    original: serde_json::Value,
}

impl ModelOverride {
    fn from_env(paths: &AppPaths) -> Option<Self> {
        let render = std::env::var("MOHENG_REAL_RENDER_MODEL").ok();
        // Owner decision (2026-10-24): acceptance may run its default steps on
        // another model while the configured one keeps cutting output short.
        let main = std::env::var("MOHENG_REAL_MAIN_MODEL").ok();
        if render.is_none() && main.is_none() {
            return None;
        }
        let original = ProviderSettingsRepository::read_global(paths).unwrap();
        let mut settings = original.clone();
        if let Some(model) = &render {
            settings["renderModelId"] = json!(model);
        }
        if let Some(model) = &main {
            settings["modelId"] = json!(model);
            settings["model"] = json!(model);
        }
        ProviderSettingsRepository::write_global(paths, &settings).unwrap();
        println!("REAL_MODEL_OVERRIDE renderModelId={render:?} modelId={main:?}");
        Some(Self {
            paths: paths.clone(),
            original,
        })
    }
}

impl Drop for ModelOverride {
    fn drop(&mut self) {
        ProviderSettingsRepository::write_global(&self.paths, &self.original).unwrap();
    }
}

/// CRAFT v3: generates and confirms the first story bible and the first
/// genre-recommended style card, so the project's chapters plan as r2.
fn confirm_craft_setup(state: &KernelState, prefix: &str, project_id: &str) {
    let command = |kind: &str, suffix: &str, payload: serde_json::Value| {
        execute_model_step(
            state,
            json!({
                "type":kind,"commandId":format!("{prefix}:{suffix}"),
                "idempotencyKey":format!("{prefix}:{suffix}:key"),"projectId":project_id,
                "expectedRevision":current_revision(state, project_id),"requestedAt":NOW,
                "payload":payload
            }),
        );
    };
    let setup = || {
        ai_novel_production_system_core::commands::query_craft_setup_impl(
            state,
            json!({"projectId":project_id}),
        )
        .unwrap()
    };
    command("story_bible_generate", "bible", json!({}));
    let bible = setup()["storyBible"]["candidates"][0].clone();
    command(
        "story_bible_confirm",
        "bible-confirm",
        json!({"candidateId":bible["id"],"bible":bible}),
    );
    command(
        "style_card_generate",
        "style",
        json!({"source":"MARKET_ARCHETYPE"}),
    );
    let card = setup()["styleCard"]["candidates"][0].clone();
    command(
        "style_card_confirm",
        "style-confirm",
        json!({"candidateId":card["id"],"card":card}),
    );
    println!(
        "REAL_CRAFT_SETUP bibleAngle={} styleCard={} archetypes={}",
        bible["angle"], card["label"], card["source"]["archetypeIds"]
    );
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

    execute_model_step(
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
    let (paths, state) = real_state();
    let _models = ModelOverride::from_env(&paths);
    let run = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let prefix = format!("real-ten-chapters:{run}");
    let title = format!("Harness 真实十章验收 {run}");
    let (project_id, opening) = create_real_project(&state, &prefix, &title);
    confirm_craft_setup(&state, &prefix, &project_id);
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
    if let Ok(out) = std::env::var("MOHENG_CRAFT_LAB_OUT") {
        write_ten_chapter_evidence(&connection, &project_id, std::path::Path::new(&out));
    }
    println!(
        "REAL_TEN_CHAPTER_RESULT projectId={project_id} wordCounts={word_counts:?} modelRuns={runs} status=PASS"
    );
}

/// For evaluation: each chapter's prose, and per chapter the plan revision,
/// rule findings, rhythm notes, recorded continuity details and the token
/// usage by model.
fn write_ten_chapter_evidence(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    out: &std::path::Path,
) {
    std::fs::create_dir_all(out).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT c.chapter_no, json_extract(f.payload_json,'$.body'), r.payload_json, json_extract(pl.payload_json,'$.payloadSchema') \
             FROM chapters c JOIN artifact_payloads f ON f.artifact_id=c.final_artifact_id AND f.version=c.final_version \
             JOIN artifacts ra ON ra.project_id=c.project_id AND ra.artifact_type='ProductionReceipt' AND ra.artifact_status='COMMITTED' \
             JOIN artifact_payloads r ON r.artifact_id=ra.artifact_id AND r.version=ra.version AND json_extract(r.payload_json,'$.finalChapterRef.artifactId')=c.final_artifact_id \
             JOIN artifact_payloads pl ON pl.artifact_id=json_extract(f.payload_json,'$.chapterPlanRef.artifactId') AND pl.version=json_extract(f.payload_json,'$.chapterPlanRef.version') \
             WHERE c.project_id=?1 ORDER BY c.chapter_no",
        )
        .unwrap();
    let chapters = statement
        .query_map([project_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let mut summary = Vec::new();
    for (number, body, receipt, schema) in chapters {
        std::fs::write(out.join(format!("chapter-{number:02}.txt")), &body).unwrap();
        let receipt: serde_json::Value = serde_json::from_str(&receipt).unwrap();
        let review = &receipt["craftSkills"]["proseReview"];
        summary.push(json!({
            "chapter":number,"planSchema":schema,"characters":body.chars().count(),
            "ruleCodes":review["ruleCodes"],"editsApplied":review["editsApplied"],
            "discarded":review["discarded"],"rhythmNotes":review["rhythmNotes"],
            "skillDecisions":receipt["craftSkills"]["chapterPlanDecisions"].as_array().map(Vec::len),
            "continuityDetails":receipt["continuityDetails"].as_array().map(Vec::len)
        }));
    }
    let usage = connection
        .prepare(
            "SELECT r.model, r.capability, COUNT(*), SUM(u.input_tokens), SUM(u.output_tokens) FROM model_runs r LEFT JOIN model_usage u ON u.model_run_id=r.model_run_id \
             WHERE r.project_id=?1 GROUP BY r.model, r.capability ORDER BY r.model, r.capability",
        )
        .unwrap()
        .query_map([project_id], |row| {
            Ok(json!({"model":row.get::<_, String>(0)?,"capability":row.get::<_, String>(1)?,
                "runs":row.get::<_, i64>(2)?,"inputTokens":row.get::<_, Option<i64>>(3)?,
                "outputTokens":row.get::<_, Option<i64>>(4)?}))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    std::fs::write(
        out.join("summary.json"),
        serde_json::to_string_pretty(
            &json!({"projectId":project_id,"chapters":summary,"usage":usage}),
        )
        .unwrap(),
    )
    .unwrap();
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
        Arc::new(NoCraftGateway(ConfiguredProductionGateway::for_desktop(
            paths,
        ))),
    );
    let run = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let prefix = format!("real-craft-ab:{run}");
    // One setup, copied, so both arms write from the same story.
    let (on_project, _) =
        create_real_project(&with_craft, &prefix, &format!("Harness 技能对照 {run}"));
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
        println!(
            "REAL_CRAFT_AB arm={arm} projectId={project_id} evaluation={}",
            evaluation["finalStatus"]
        );
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
    let content = raw["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim();
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
                    .entry(format!(
                        "{}:{}",
                        dimension["name"].as_str().unwrap_or("?"),
                        arm(&dimension["winner"])
                    ))
                    .or_default() += 1;
            }
            *tally
                .entry(format!("overall:{}", arm(&verdict["overall"])))
                .or_default() += 1;
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

#[test]
#[ignore = "real provider: one tiny call per candidate writer model"]
fn configured_provider_probes_candidate_writer_models() {
    assert_eq!(
        std::env::var("MOHENG_RUN_REAL_CRAFT_AB").as_deref(),
        Ok("1")
    );
    let models = std::env::var("MOHENG_PROBE_MODELS").expect("MOHENG_PROBE_MODELS");
    let (paths, _) = real_state();
    let gateway = ConfiguredProductionGateway::for_desktop(paths);
    for model in models.split(',') {
        let mut profile = gateway.resolve_profile().unwrap();
        profile.model = model.trim().to_string();
        let result = gateway.invoke_typed_with_profile(
            "TEST_PROVIDER",
            &json!({"prompt":"只返回 JSON：{\"ok\":true}"}),
            &profile,
        );
        println!("PROBE model={} ok={}", profile.model, result.is_ok());
        if let Err(failure) = result {
            println!("PROBE model={} failure={failure:?}", profile.model);
        }
    }
}

/// One model call of the craft lab: `prompt` plus `data` to `model`,
/// returning the reply's JSON object.
fn lab_call(
    gateway: &ConfiguredProductionGateway,
    model: &str,
    prompt: &str,
    data: serde_json::Value,
) -> serde_json::Value {
    let mut profile = gateway.resolve_profile().unwrap();
    profile.model = model.to_string();
    let mut input = data;
    input["prompt"] = json!(prompt);
    let response = gateway
        .invoke_typed_with_profile("CRAFT_LAB", &input, &profile)
        .unwrap_or_else(|failure| panic!("lab call to {model} failed: {failure:?}"));
    let raw: serde_json::Value =
        serde_json::from_str(response.raw_response.as_deref().unwrap_or("null")).unwrap();
    let content = raw["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim();
    let content = content
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    serde_json::from_str(content).unwrap_or_else(|_| json!({"unparsed":content}))
}

const LAB_SETUP: &str = "主角：城市记忆档案调查员沈砚。欲望：找回被人从公共档案中抹去的姐姐。核心机制：修复被篡改的记忆，会同步抹去修复者自己的一段现实痕迹。冲突：维护城市稳定的档案局要求掩盖真相，沈砚必须在自我消失前留下证据。读者承诺：每章解决一个可验证的小谜题，同时让失去痕迹的代价持续升级。禁区：前三章不揭晓最终幕后者，不证明姐姐已死；不用梦境醒来推翻前文，不用系统面板。类型：都市悬疑，长篇网络小说。";

const LAB_STYLE_PROMPT: &str = "你是网文编辑。阅读 exemplar（一段已发表网文的开篇），只提炼它的写法，不复述情节和人物。返回 JSON：{\"rhythm\":string,\"narrativeDistance\":string,\"howInformationIsReleased\":string,\"howCharactersAreDistinguished\":string,\"dialogue\":string,\"description\":string,\"whatItAvoids\":[string],\"rules\":[string]}。rules 是 6 到 10 条可以直接执行的写作规则，用中文，具体到句子和段落层面。";

const LAB_HOOK_PROMPT: &str = "你是网文主编。根据 setup 和 chapterGoal，为第一章构思 5 种不同的开场：每种给出开场一百字内发生的具体异常事件、读者此刻会问的问题、这个异常如何与核心机制相关。再为每种标注 typicality（1=很少见，5=网文里到处都是）。最后选出一种 typicality 尽量低、但逻辑可信且能马上进入冲突的。返回 JSON：{\"options\":[{\"event\":string,\"readerQuestion\":string,\"link\":string,\"typicality\":number}],\"chosen\":number,\"why\":string}。";

const LAB_SCENE_PROMPT: &str = "你是网文主编，为第一章写场景卡。使用 chosenHook 作为开场。整章 2 到 3 个场景，总字数约 2500 字。每个场景卡只写事实，不写描写、比喻、情绪形容词。返回 JSON：{\"scenes\":[{\"place\":string,\"people\":[{\"name\":string,\"wants\":string,\"hides\":string,\"voice\":string}],\"protagonistTries\":string,\"whyItFailsOrCosts\":string,\"events\":[string],\"readerQuestionOpened\":string,\"readerQuestionAnswered\":string,\"endState\":string}],\"chapterHook\":string}。events 每条是一个改变局面的具体事实（谁做了什么、发现了什么），3 到 6 条。至少一个场景里要有人物之间的分歧或猜疑。主角的推理要能从读者看得到的线索得出。";

const LAB_WRITE_PROMPT: &str = "你是成熟的网文作者，按 scenes 写第一章正文。要求：1 每段一到两句，长短交错；2 开头三行内出现异常；3 每隔几行给读者一个新信息或新疑问；4 推理和判断由人物在对白或短促的自问里说出，叙述者不解释、不总结、不点明情绪；5 比喻全章不超过两个，不写喉结、指尖、呼吸之类的身体小动作；6 每个人物说话方式不同，对白里带着他想要和隐瞒的东西；7 用具体的物件和动作，不用抽象形容；8 章末停在一个让人想翻下一页的事实上；9 全章 2000 到 3000 字，不分小节，不写标题。styleCard 是要学习的写法。返回 JSON：{\"body\":string}。";

#[test]
#[ignore = "real provider craft lab: chapter one through style card, hook choice and scene cards; a few paid calls"]
fn configured_provider_craft_lab_writes_chapter_one() {
    assert_eq!(
        std::env::var("MOHENG_RUN_REAL_CRAFT_AB").as_deref(),
        Ok("1")
    );
    let out = PathBuf::from(std::env::var("MOHENG_CRAFT_LAB_OUT").expect("MOHENG_CRAFT_LAB_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    // A local, private style reference; never committed.
    let exemplar =
        std::fs::read_to_string(std::env::var("MOHENG_CRAFT_LAB_EXEMPLAR").unwrap()).unwrap();
    let models = std::env::var("MOHENG_CRAFT_LAB_MODELS").unwrap();
    let (paths, _) = real_state();
    let gateway = ConfiguredProductionGateway::for_desktop(paths);
    let goal = OPENING_GOALS[0];
    for model in models.split(',').map(str::trim) {
        let style = lab_call(
            &gateway,
            model,
            LAB_STYLE_PROMPT,
            json!({"exemplar":exemplar}),
        );
        let hook = lab_call(
            &gateway,
            model,
            LAB_HOOK_PROMPT,
            json!({"setup":LAB_SETUP,"chapterGoal":goal}),
        );
        let chosen = hook["chosen"].as_u64().unwrap_or(1) as usize;
        let chosen_hook = hook["options"]
            .as_array()
            .and_then(|options| options.get(chosen.saturating_sub(1)).or(options.first()))
            .cloned()
            .unwrap_or(json!(null));
        let scenes = lab_call(
            &gateway,
            model,
            LAB_SCENE_PROMPT,
            json!({"setup":LAB_SETUP,"chapterGoal":goal,"chosenHook":chosen_hook}),
        );
        let slug = model.replace(['.', '/'], "_");
        std::fs::write(
            out.join(format!("{slug}-plan.json")),
            serde_json::to_string_pretty(&json!({"style":style,"hook":hook,"scenes":scenes}))
                .unwrap(),
        )
        .unwrap();
        for (variant, extra) in [
            ("card", json!({})),
            (
                "card-exemplar",
                json!({"exemplar":exemplar,
                "exemplarUse":"exemplar 只用来体会节奏和叙述距离，不得照搬其中任何句子、人物、情节或设定"}),
            ),
        ] {
            let mut data = json!({"setup":LAB_SETUP,"scenes":scenes,"styleCard":style});
            for (key, value) in extra.as_object().unwrap() {
                data[key] = value.clone();
            }
            let written = lab_call(&gateway, model, LAB_WRITE_PROMPT, data);
            let body = written["body"].as_str().unwrap_or("").to_string();
            std::fs::write(out.join(format!("{slug}-{variant}.txt")), &body).unwrap();
            println!(
                "CRAFT_LAB model={model} variant={variant} chars={}",
                body.chars().count()
            );
        }
    }
}

const LAB2_STYLE_PROMPT: &str = "你是网文编辑。阅读 exemplar，只提炼它的写法。不得出现 exemplar 里的任何人名、物件、场景、情节或原句。返回 JSON：{\"rhythm\":string,\"narrativeDistance\":string,\"howInformationIsReleased\":string,\"howCharactersAreDistinguished\":string,\"dialogue\":string,\"rules\":[string]}，rules 6 到 10 条，具体到句子和段落。";

const LAB2_PLAIN_PLAN_PROMPT: &str = "你是网文作者，为本章写场景卡。setup 是设定，chapterGoal 是本章目标，previous 是前文状态（第一章为空）。返回 JSON：{\"scenes\":[{\"place\":string,\"events\":[string]}],\"chapterHook\":string,\"endState\":string}。";

const LAB2_HOOK_PROMPT: &str = "你是网文主编。依据 craftGuidance 中的方法，为本章构思 5 种不同的开场：每种写出开场三行内发生的具体异常或冲突、读者此刻会问的问题。为每种标注 typicality（1=很少见，5=网文里到处都是）。选出 typicality 低、逻辑可信、能立刻进入冲突、并与 previous 衔接的一种，把它的文字原样放进 chosen。返回 JSON：{\"options\":[{\"opening\":string,\"readerQuestion\":string,\"typicality\":number}],\"chosen\":{\"opening\":string,\"readerQuestion\":string}}。";

const LAB2_SKILL_PLAN_PROMPT: &str = "你是网文主编，依据 craftGuidance 中的方法为本章写场景卡。以 chosenOpening 开场，承接 previous。整章 2 到 3 个场景，只写事实，不写描写、比喻、情绪形容词。每个字段都是一个具体的决定，不能空泛。返回 JSON：{\"openingAnomaly\":string,\"protagonistChoice\":string,\"costThisChapter\":string,\"costComparedToBefore\":string,\"scenes\":[{\"place\":string,\"people\":[{\"name\":string,\"wants\":string,\"hides\":string,\"voice\":string}],\"conflict\":string,\"events\":[string],\"readerQuestionOpened\":string,\"readerQuestionAnswered\":string,\"endState\":string}],\"chapterHook\":string,\"endState\":string}。openingAnomaly 是正文前三行就要发生的事；protagonistChoice 是主角本章做出的、会改变局面的主动选择；costComparedToBefore 说明本章代价比前文更重在哪里（第一章写与开场相比）；每个场景至少两人在场，conflict 写两人目的如何冲突；events 3 到 6 条，每条是谁做了什么或发现了什么。";

const LAB2_WRITE_PROMPT: &str = "你是成熟的网文作者，按 plan 写本章正文，承接 previous。要求：1 每段一到两句，长短交错；2 开头三行内出现异常或冲突；3 每隔几行给读者一个新信息或新疑问；4 推理和判断由人物在对白或短促的自问里说出，叙述者不解释、不总结、不点明情绪；5 比喻全章不超过两个，不写喉结、指尖、呼吸之类的身体小动作；6 每个人物说话方式不同；7 用具体的物件和动作；8 章末停在一个让人想翻下一页的事实上；9 全章 2000 到 3000 字，不分小节，不写标题。styleCard 是要学习的写法；exemplar 只用来体会节奏和叙述距离，不得照搬其中任何句子、人物、情节或设定。返回 JSON：{\"body\":string}。";

/// Checks of the skill-bound plan fields; returns what is missing.
fn lab2_plan_gaps(plan: &serde_json::Value, chapter: usize) -> Vec<String> {
    let text = |value: &serde_json::Value| {
        value
            .as_str()
            .is_some_and(|text| text.trim().chars().count() >= 6)
    };
    let mut gaps = Vec::new();
    for field in [
        "openingAnomaly",
        "protagonistChoice",
        "costThisChapter",
        "chapterHook",
        "endState",
    ] {
        if !text(&plan[field]) {
            gaps.push(format!("{field} 缺失或空泛"));
        }
    }
    if chapter > 1 && !text(&plan["costComparedToBefore"]) {
        gaps.push("costComparedToBefore 缺失：本章代价必须比前文更重".into());
    }
    let scenes = plan["scenes"].as_array().cloned().unwrap_or_default();
    if !(2..=3).contains(&scenes.len()) {
        gaps.push(format!("场景数 {} 不在 2 到 3 之间", scenes.len()));
    }
    for (index, scene) in scenes.iter().enumerate() {
        let people = scene["people"].as_array().cloned().unwrap_or_default();
        if people.len() < 2 {
            gaps.push(format!("场景 {} 在场人物少于两人", index + 1));
        }
        for person in &people {
            for key in ["wants", "hides", "voice"] {
                if !text(&person[key]) {
                    gaps.push(format!(
                        "场景 {} 人物 {} 的 {key} 缺失或空泛",
                        index + 1,
                        person["name"].as_str().unwrap_or("?")
                    ));
                }
            }
        }
        for key in ["conflict", "readerQuestionOpened"] {
            if !text(&scene[key]) {
                gaps.push(format!("场景 {} 的 {key} 缺失或空泛", index + 1));
            }
        }
        let events = scene["events"].as_array().map(Vec::len).unwrap_or(0);
        if !(3..=6).contains(&events) {
            gaps.push(format!(
                "场景 {} 的事件数 {events} 不在 3 到 6 之间",
                index + 1
            ));
        }
    }
    gaps
}

#[test]
#[ignore = "real provider craft lab 2: the golden three chapters with and without skill-bound planning; paid calls"]
fn configured_provider_craft_lab_compares_skill_bound_planning() {
    use ai_novel_production_system_core::chapter_craft::{guidance, guidance_skill_ids};
    use ai_novel_production_system_core::skills_v2::StepSignals;
    assert_eq!(
        std::env::var("MOHENG_RUN_REAL_CRAFT_AB").as_deref(),
        Ok("1")
    );
    let out = PathBuf::from(std::env::var("MOHENG_CRAFT_LAB_OUT").expect("MOHENG_CRAFT_LAB_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    // A local, private style reference; never committed.
    let exemplar =
        std::fs::read_to_string(std::env::var("MOHENG_CRAFT_LAB_EXEMPLAR").unwrap()).unwrap();
    let model = std::env::var("MOHENG_CRAFT_LAB_MODELS").unwrap();
    let (paths, _) = real_state();
    let gateway = ConfiguredProductionGateway::for_desktop(paths);
    let style = lab_call(
        &gateway,
        &model,
        LAB2_STYLE_PROMPT,
        json!({"exemplar":exemplar}),
    );
    std::fs::write(
        out.join("style-card.json"),
        serde_json::to_string_pretty(&style).unwrap(),
    )
    .unwrap();
    let mut ledger = String::from("# skill 落实表\n");
    for arm in ["plain", "skills"] {
        let mut previous = json!(null);
        for chapter in 1..=3usize {
            let goal = OPENING_GOALS[chapter - 1];
            let plan = if arm == "plain" {
                lab_call(
                    &gateway,
                    &model,
                    LAB2_PLAIN_PLAN_PROMPT,
                    json!({"setup":LAB_SETUP,"chapterGoal":goal,"previous":previous}),
                )
            } else {
                let signals = StepSignals {
                    chapter_number: Some(chapter as u32),
                    ..Default::default()
                };
                let craft = guidance("chapter_plan", &signals).unwrap();
                let hook = lab_call(
                    &gateway,
                    &model,
                    LAB2_HOOK_PROMPT,
                    json!({"setup":LAB_SETUP,"chapterGoal":goal,"previous":previous,"craftGuidance":craft}),
                );
                let input = json!({"setup":LAB_SETUP,"chapterGoal":goal,"previous":previous,
                    "chosenOpening":hook["chosen"],"craftGuidance":craft});
                let mut plan = lab_call(&gateway, &model, LAB2_SKILL_PLAN_PROMPT, input.clone());
                let mut gaps = lab2_plan_gaps(&plan, chapter);
                if !gaps.is_empty() {
                    let mut retry = input;
                    retry["planFeedback"] = json!({"missing":gaps,"instruction":"上一版场景卡未通过检查：补全并具体化 missing 中的每一项，返回完整场景卡。"});
                    plan = lab_call(&gateway, &model, LAB2_SKILL_PLAN_PROMPT, retry);
                    gaps = lab2_plan_gaps(&plan, chapter);
                }
                ledger.push_str(&format!(
                    "\n## 第 {chapter} 章\n- 使用的 skill：{}\n- 开场候选：{}\n- 选中的开场（黄金三章/开场）：{}\n- 开场异常：{}\n- 主角主动选择：{}\n- 本章代价（升级）：{} ／ 比前文更重：{}\n- 章末钩子：{}\n- 规则检查遗留问题：{}\n",
                    guidance_skill_ids(&craft).join("、"),
                    hook["options"].as_array().map(|o| o.iter().map(|x| format!("[{}]{}", x["typicality"], x["opening"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("；")).unwrap_or_default(),
                    hook["chosen"]["opening"].as_str().unwrap_or(""),
                    plan["openingAnomaly"].as_str().unwrap_or(""),
                    plan["protagonistChoice"].as_str().unwrap_or(""),
                    plan["costThisChapter"].as_str().unwrap_or(""),
                    plan["costComparedToBefore"].as_str().unwrap_or(""),
                    plan["chapterHook"].as_str().unwrap_or(""),
                    if gaps.is_empty() { "无".to_string() } else { gaps.join("；") },
                ));
                for (index, scene) in plan["scenes"].as_array().into_iter().flatten().enumerate() {
                    ledger.push_str(&format!(
                        "- 场景 {}（{}）冲突：{}；读者疑问：{}；人物：{}\n",
                        index + 1,
                        scene["place"].as_str().unwrap_or(""),
                        scene["conflict"].as_str().unwrap_or(""),
                        scene["readerQuestionOpened"].as_str().unwrap_or(""),
                        scene["people"]
                            .as_array()
                            .map(|p| p
                                .iter()
                                .map(|x| format!(
                                    "{}（想要{}，隐瞒{}，{}）",
                                    x["name"].as_str().unwrap_or(""),
                                    x["wants"].as_str().unwrap_or(""),
                                    x["hides"].as_str().unwrap_or(""),
                                    x["voice"].as_str().unwrap_or("")
                                ))
                                .collect::<Vec<_>>()
                                .join("；"))
                            .unwrap_or_default(),
                    ));
                }
                plan
            };
            std::fs::write(
                out.join(format!("{arm}-plan-{chapter}.json")),
                serde_json::to_string_pretty(&plan).unwrap(),
            )
            .unwrap();
            let written = lab_call(
                &gateway,
                &model,
                LAB2_WRITE_PROMPT,
                json!({"setup":LAB_SETUP,"previous":previous,"plan":plan,"styleCard":style,"exemplar":exemplar}),
            );
            let body = written["body"].as_str().unwrap_or("").to_string();
            std::fs::write(out.join(format!("{arm}-chapter-{chapter}.txt")), &body).unwrap();
            println!(
                "CRAFT_LAB2 arm={arm} chapter={chapter} chars={}",
                body.chars().count()
            );
            let tail = body
                .chars()
                .rev()
                .take(600)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>();
            previous = json!({"chapter":chapter,"endState":plan["endState"],"chapterHook":plan["chapterHook"],"ending":tail});
        }
    }
    std::fs::write(out.join("skills-ledger.md"), ledger).unwrap();
}

const LAB3_BIBLE_PROMPT: &str = "你是网文主编，为 setup 建故事底稿，供前三章使用。返回 JSON：{\"normalWorld\":string,\"worldRules\":[{\"rule\":string,\"howReadersSeeIt\":string,\"cost\":string}],\"protagonist\":{\"name\":string,\"personality\":string,\"habit\":string,\"speech\":string,\"flaw\":string,\"wantsNow\":string},\"sisterAnchor\":{\"memory\":string,\"object\":string},\"cast\":[{\"name\":string,\"role\":string,\"relation\":string,\"contrastWithProtagonist\":string,\"privateAgenda\":string,\"speech\":string}],\"anomalyDevices\":[{\"device\":string,\"escalation\":string}],\"arc\":{\"chapter1\":string,\"chapter2\":string,\"chapter3\":string}}。要求：normalWorld 写主角平常一天的工作是什么样、修复记忆具体怎么做；worldRules 3 到 5 条，读者能从具体场面里看懂；protagonist 要有让读者喜欢或好奇的性格侧面，不是冷面工具人；sisterAnchor 是主角和姐姐之间一个具体的往事场面和一件物品；cast 3 到 4 人，每人与主角性格反差明显、有自己的私心；anomalyDevices 3 到 4 种不同的异常表现，各有升级方式；arc 第一章先立正常世界和主角能力、章末出现第一个异常，第二章让代价落在一个具体的人身上，第三章主角做出改变局面的主动选择。";

const LAB3_PLAN_PROMPT: &str = "你是网文主编，依据 bible 和 craftGuidance 为第 chapter 章写计划，承接 previous，完成 bible.arc 中本章的任务。usedDevices 是前文用过的异常手法：不得原样再用，只能用新的或写明如何升级。整章 2 到 3 个场景，只写事实。返回 JSON：{\"bigTurn\":string,\"smallDiscovery\":string,\"deviceUsed\":{\"device\":string,\"newOrEscalated\":string},\"warmthOrHumor\":string,\"emotionalBeat\":string,\"scenes\":[{\"purpose\":string,\"place\":string,\"people\":[{\"name\":string,\"wants\":string,\"hides\":string}],\"conflict\":string,\"events\":[string],\"readerQuestion\":string}],\"chapterHook\":string,\"endState\":string}。bigTurn 全章只有这一个大转折；smallDiscovery 只有一个；warmthOrHumor 是一处人与人之间的温度或幽默；emotionalBeat 是全章唯一需要放慢写的情感时刻；events 每场 3 到 5 条，写谁做了什么或发现了什么；不要在计划里写比喻和情绪形容词。";

const LAB3_WRITE_PROMPT: &str = "你是成熟的网文作者，按 plan 写第 chapter 章正文，人物、规则以 bible 为准，承接 previous。要求：1 信息和动作段落短，情绪和氛围段落可以写长，长短交错，不要通篇短句；2 只写 plan 里的一个大转折和一个小发现，不额外制造反转；3 写出 warmthOrHumor 这一处人情或幽默，人物说话按 bible 里各自的 speech；4 在 emotionalBeat 处放慢，用具体的往事、物件和动作写，不写情绪名词；5 推理由人物说出，叙述者不解释、不总结；6 比喻全章不超过两个，不写喉结、指尖、呼吸之类的身体小动作；7 章末停在 chapterHook 的事实上；8 全章 2200 到 3000 字，不分小节，不写标题。styleCard 是要学习的写法；exemplar 只用来体会节奏和叙述距离，不得照搬其中任何句子、人物、情节或设定。返回 JSON：{\"body\":string}。";

fn lab3_plan_gaps(plan: &serde_json::Value, used: &[String]) -> Vec<String> {
    let text = |value: &serde_json::Value| {
        value
            .as_str()
            .is_some_and(|text| text.trim().chars().count() >= 6)
    };
    let mut gaps = Vec::new();
    for field in [
        "bigTurn",
        "smallDiscovery",
        "warmthOrHumor",
        "emotionalBeat",
        "chapterHook",
        "endState",
    ] {
        if !text(&plan[field]) {
            gaps.push(format!("{field} 缺失或空泛"));
        }
    }
    let device = plan["deviceUsed"]["device"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    if device.is_empty() {
        gaps.push("deviceUsed 缺失".into());
    } else if used.iter().any(|old| old == &device) && !text(&plan["deviceUsed"]["newOrEscalated"])
    {
        gaps.push(format!(
            "异常手法「{device}」前文已用过，必须写明如何升级或换新手法"
        ));
    }
    let scenes = plan["scenes"].as_array().cloned().unwrap_or_default();
    if !(2..=3).contains(&scenes.len()) {
        gaps.push(format!("场景数 {} 不在 2 到 3 之间", scenes.len()));
    }
    for (index, scene) in scenes.iter().enumerate() {
        let events = scene["events"].as_array().map(Vec::len).unwrap_or(0);
        if !(3..=5).contains(&events) {
            gaps.push(format!(
                "场景 {} 的事件数 {events} 不在 3 到 5 之间",
                index + 1
            ));
        }
        if !text(&scene["conflict"]) {
            gaps.push(format!("场景 {} 的 conflict 缺失", index + 1));
        }
    }
    gaps
}

#[test]
#[ignore = "real provider craft lab 3: story bible, turn budget and device ledger; paid calls"]
fn configured_provider_craft_lab_writes_from_a_story_bible() {
    use ai_novel_production_system_core::chapter_craft::guidance;
    use ai_novel_production_system_core::skills_v2::StepSignals;
    assert_eq!(
        std::env::var("MOHENG_RUN_REAL_CRAFT_AB").as_deref(),
        Ok("1")
    );
    let out = PathBuf::from(std::env::var("MOHENG_CRAFT_LAB_OUT").expect("MOHENG_CRAFT_LAB_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let exemplar =
        std::fs::read_to_string(std::env::var("MOHENG_CRAFT_LAB_EXEMPLAR").unwrap()).unwrap();
    let style: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("MOHENG_CRAFT_LAB_STYLE").unwrap()).unwrap(),
    )
    .unwrap();
    let model = std::env::var("MOHENG_CRAFT_LAB_MODELS").unwrap();
    let (paths, _) = real_state();
    let gateway = ConfiguredProductionGateway::for_desktop(paths);
    let bible = lab_call(
        &gateway,
        &model,
        LAB3_BIBLE_PROMPT,
        json!({"setup":LAB_SETUP}),
    );
    std::fs::write(
        out.join("bible.json"),
        serde_json::to_string_pretty(&bible).unwrap(),
    )
    .unwrap();
    let mut used = Vec::<String>::new();
    let mut previous = json!(null);
    let mut ledger = String::from("# 计划落实表\n");
    for chapter in 1..=3usize {
        let craft = guidance(
            "chapter_plan",
            &StepSignals {
                chapter_number: Some(chapter as u32),
                ..Default::default()
            },
        )
        .unwrap();
        let input = json!({"chapter":chapter,"bible":bible,"previous":previous,"usedDevices":used,"craftGuidance":craft});
        let mut plan = lab_call(&gateway, &model, LAB3_PLAN_PROMPT, input.clone());
        let mut gaps = lab3_plan_gaps(&plan, &used);
        if !gaps.is_empty() {
            let mut retry = input;
            retry["planFeedback"] = json!({"missing":gaps,"instruction":"上一版计划未通过检查：逐项修正 missing，返回完整计划。"});
            plan = lab_call(&gateway, &model, LAB3_PLAN_PROMPT, retry);
            gaps = lab3_plan_gaps(&plan, &used);
        }
        std::fs::write(
            out.join(format!("plan-{chapter}.json")),
            serde_json::to_string_pretty(&plan).unwrap(),
        )
        .unwrap();
        let write_input = json!({"chapter":chapter,"bible":bible,"previous":previous,"plan":plan,"styleCard":style,"exemplar":exemplar});
        let mut body = lab_call(&gateway, &model, LAB3_WRITE_PROMPT, write_input.clone())["body"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let length = body.chars().filter(|c| !c.is_whitespace()).count();
        if length < 2000 {
            let mut retry = write_input;
            retry["lengthFeedback"] = json!(format!("上一版只有 {length} 字，不足 2200：在 emotionalBeat 处放慢、把每个 events 写成完整的场面，不加新反转，重写全章。"));
            body = lab_call(&gateway, &model, LAB3_WRITE_PROMPT, retry)["body"]
                .as_str()
                .unwrap_or("")
                .to_string();
        }
        std::fs::write(out.join(format!("chapter-{chapter}.txt")), &body).unwrap();
        println!(
            "CRAFT_LAB3 chapter={chapter} chars={}",
            body.chars().count()
        );
        ledger.push_str(&format!(
            "\n## 第 {chapter} 章\n- 大转折：{}\n- 小发现：{}\n- 异常手法：{}（{}）\n- 温度或幽默：{}\n- 放慢的情感时刻：{}\n- 章末钩子：{}\n- 检查遗留：{}\n",
            plan["bigTurn"].as_str().unwrap_or(""),
            plan["smallDiscovery"].as_str().unwrap_or(""),
            plan["deviceUsed"]["device"].as_str().unwrap_or(""),
            plan["deviceUsed"]["newOrEscalated"].as_str().unwrap_or(""),
            plan["warmthOrHumor"].as_str().unwrap_or(""),
            plan["emotionalBeat"].as_str().unwrap_or(""),
            plan["chapterHook"].as_str().unwrap_or(""),
            if gaps.is_empty() { "无".to_string() } else { gaps.join("；") },
        ));
        if let Some(device) = plan["deviceUsed"]["device"].as_str() {
            used.push(device.trim().to_string());
        }
        let tail = body
            .chars()
            .rev()
            .take(600)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>();
        previous = json!({"chapter":chapter,"endState":plan["endState"],"chapterHook":plan["chapterHook"],"ending":tail});
    }
    std::fs::write(out.join("plan-ledger.md"), ledger).unwrap();
}

const LAB4_BIBLE_PROMPT: &str = "你是网文主编，为 setup 建故事底稿，供前三章使用。返回 JSON：{\"normalWorld\":string,\"repairExperience\":string,\"worldRules\":[{\"rule\":string,\"numbers\":string,\"howReadersSeeIt\":string}],\"protagonist\":{\"name\":string,\"personality\":string,\"humor\":string,\"flaw\":string,\"wantsNow\":string,\"innerVoiceSamples\":[string]},\"sisterAnchor\":{\"memory\":string,\"object\":string},\"antagonist\":{\"name\":string,\"position\":string,\"wants\":string,\"method\":string},\"cast\":[{\"name\":string,\"role\":string,\"relation\":string,\"contrastWithProtagonist\":string,\"privateAgenda\":string,\"speech\":string,\"catchphrase\":string}],\"anomalyDevices\":[{\"device\":string,\"escalation\":string}],\"arc\":{\"chapter1\":string,\"chapter2\":string,\"chapter3\":string},\"wins\":{\"chapter1\":string,\"chapter2\":string,\"chapter3\":string}}。要求：repairExperience 写进入别人记忆时的独特画面和规则，让读者看见这个世界的新意；worldRules 3 到 5 条，numbers 给出具体数字（代价每次多少、到多少会怎样）；protagonist 要有幽默和判断力，innerVoiceSamples 写 3 句他的内心独白示例；antagonist 是有名有姓、有职位的人，前三章至少露面一次；cast 3 人，catchphrase 是口头禅（可为空）；arc 第一章先立正常世界和主角能力、章末出现第一个异常，第二章代价落在一个具体的人身上，第三章主角做出改变局面的主动选择；wins 每章一次主角凭本事赢下的局面。";

const LAB4_PLAN_PROMPT: &str = "你是网文主编，依据 bible 和 craftGuidance 为第 chapter 章写计划，承接 previous，完成 bible.arc 与 bible.wins 中本章的任务。ledger 记录前文：usedDevices（异常手法，不得原样再用，只能升级或换新）、usedPlaces（至少一个场景用新地点）、usedEndSituations（本章结尾局面不得与之相同）、lostThings（主角已经失去的，本章不得让他想起或使用）、introducedPeople（已登场人物）。整章 2 到 3 个场景，只写事实。返回 JSON：{\"bigTurn\":string,\"smallDiscovery\":string,\"win\":string,\"deviceUsed\":{\"device\":string,\"newOrEscalated\":string},\"warmthOrHumor\":string,\"emotionalBeat\":string,\"newLosses\":[string],\"scenes\":[{\"place\":string,\"people\":[{\"name\":string,\"wants\":string,\"hides\":string,\"whyHere\":string}],\"conflict\":string,\"events\":[string],\"readerQuestion\":string}],\"chapterHook\":string,\"endSituation\":string,\"endState\":string}。win 是主角凭本事赢下的具体一局；whyHere 写该人物为何此刻在场，必须基于前文已有的安排或本章事件，不能靠巧合；newLosses 是本章主角新失去的东西；events 每场 3 到 5 条；不要写比喻和情绪形容词。";

const LAB4_WRITE_PROMPT: &str = "你是成熟的网文作者，按 plan 写第 chapter 章正文，人物与规则以 bible 为准，从 previous.endState 接着写。要求：1 信息和动作段落短，情绪和氛围段落可以写长；2 只写 plan 的一个大转折、一个小发现，写出主角凭本事赢下 win 的过程；3 主角有自己的内心声音：全章至少 3 处他的判断、吐槽或自问，参照 bible.protagonist.innerVoiceSamples 的口吻；4 anchorUse 说明姐姐往事怎么用；5 lostThings 中的东西主角已经失去，不得回忆或使用；6 口头禅每个人全章最多出现两次；不要用“某某看着他”这类空反应句填充对话间隙；7 推理由人物说出，叙述者不解释、不总结；8 比喻全章不超过两个，不写喉结、指尖、呼吸之类的身体小动作；9 不重复前文已经写过的句子；10 章末停在 chapterHook 的事实上；11 全章 2200 到 3000 字，不分小节，不写标题。styleCard 是要学习的写法；exemplar 只用来体会节奏和叙述距离，不得照搬其中任何句子、人物、情节或设定。返回 JSON：{\"body\":string}。";

const LAB4_EDIT_PROMPT: &str = "你是网文编辑，按 findings 修改 body：每条 finding 给出一处要改的原文 quote 和问题。只改这些地方，返回 JSON：{\"edits\":[{\"quote\":string,\"replacement\":string}]}。quote 必须逐字取自 body 且只出现一次（可以扩到整句）；replacement 保持情节、人物和视角不变，长度相近；不得新增比喻（像、仿佛、如同）、喉结指尖之类的身体小动作、带单位的数字或【】。重复句改成新的表达或直接删去（replacement 写成意思相同的更短说法）。";

fn lab4_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if c == '\n' {
            if !current.trim().is_empty() {
                out.push(current.trim().to_string());
            }
            current.clear();
            continue;
        }
        current.push(c);
        if matches!(c, '。' | '！' | '？') {
            out.push(current.trim().to_string());
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

/// Rule findings for the lab edit pass: model habits, sentences already
/// written in earlier chapters, and catchphrases past two uses.
fn lab4_findings(
    plan: &serde_json::Value,
    body: &str,
    earlier: &[String],
    catchphrases: &[String],
) -> Vec<serde_json::Value> {
    let mut findings = ai_novel_production_system_core::chapter_craft::rule_findings(plan, body);
    let unique = |sentence: &str| body.matches(sentence).count() == 1;
    for sentence in lab4_sentences(body) {
        if sentence.chars().count() >= 10
            && unique(&sentence)
            && earlier.iter().any(|old| old.contains(&sentence))
        {
            findings.push(json!({"code":"redundancy","quote":sentence,"problem":"与前文原句重复"}));
        }
    }
    for phrase in catchphrases
        .iter()
        .filter(|phrase| phrase.chars().count() >= 2)
    {
        let hits = lab4_sentences(body)
            .into_iter()
            .filter(|s| s.contains(phrase.as_str()) && unique(s))
            .collect::<Vec<_>>();
        for sentence in hits.into_iter().skip(2) {
            findings.push(json!({"code":"word_repetition","quote":sentence,"problem":format!("口头禅「{phrase}」超过两次")}));
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    findings.retain(|finding| seen.insert(finding["quote"].as_str().unwrap_or("").to_string()));
    findings.truncate(14);
    findings
}

fn lab4_apply(body: &str, edits: &serde_json::Value) -> (String, usize) {
    use ai_novel_production_system_core::chapter_craft::habit_count;
    let mut revised = body.to_string();
    let mut applied = 0;
    for edit in edits["edits"].as_array().into_iter().flatten() {
        let (Some(quote), Some(replacement)) =
            (edit["quote"].as_str(), edit["replacement"].as_str())
        else {
            continue;
        };
        if quote.is_empty()
            || revised.matches(quote).count() != 1
            || habit_count(replacement) > habit_count(quote)
        {
            continue;
        }
        revised = revised.replacen(quote, replacement, 1);
        applied += 1;
    }
    (revised, applied)
}

#[test]
#[ignore = "real provider craft lab 4: story bible with numbers and wins, ledgers, and a rule-driven edit pass; paid calls"]
fn configured_provider_craft_lab_writes_with_ledgers_and_edits() {
    use ai_novel_production_system_core::chapter_craft::guidance;
    use ai_novel_production_system_core::skills_v2::StepSignals;
    assert_eq!(
        std::env::var("MOHENG_RUN_REAL_CRAFT_AB").as_deref(),
        Ok("1")
    );
    let out = PathBuf::from(std::env::var("MOHENG_CRAFT_LAB_OUT").expect("MOHENG_CRAFT_LAB_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let exemplar =
        std::fs::read_to_string(std::env::var("MOHENG_CRAFT_LAB_EXEMPLAR").unwrap()).unwrap();
    let style: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("MOHENG_CRAFT_LAB_STYLE").unwrap()).unwrap(),
    )
    .unwrap();
    let model = std::env::var("MOHENG_CRAFT_LAB_MODELS").unwrap();
    let (paths, _) = real_state();
    let gateway = ConfiguredProductionGateway::for_desktop(paths);
    let bible = lab_call(
        &gateway,
        &model,
        LAB4_BIBLE_PROMPT,
        json!({"setup":LAB_SETUP}),
    );
    std::fs::write(
        out.join("bible.json"),
        serde_json::to_string_pretty(&bible).unwrap(),
    )
    .unwrap();
    let catchphrases = bible["cast"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|person| person["catchphrase"].as_str())
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>();
    let mut ledger = json!({"usedDevices":[],"usedPlaces":[],"usedEndSituations":[],"lostThings":[],"introducedPeople":[]});
    let mut bodies: Vec<String> = Vec::new();
    let mut previous = json!(null);
    let mut report = String::from("# 计划与修改落实表\n");
    for chapter in 1..=3usize {
        let craft = guidance(
            "chapter_plan",
            &StepSignals {
                chapter_number: Some(chapter as u32),
                ..Default::default()
            },
        )
        .unwrap();
        let input = json!({"chapter":chapter,"bible":bible,"previous":previous,"ledger":ledger,"craftGuidance":craft});
        let mut plan = lab_call(&gateway, &model, LAB4_PLAN_PROMPT, input.clone());
        let used_places = ledger["usedPlaces"].as_array().cloned().unwrap_or_default();
        let new_place = plan["scenes"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|scene| !used_places.contains(&scene["place"]));
        let mut gaps = lab3_plan_gaps(&plan, &[]);
        if !plan["win"].as_str().is_some_and(|w| w.chars().count() >= 6) {
            gaps.push("win 缺失：本章要有主角凭本事赢下的一局".into());
        }
        if chapter > 1 && !new_place {
            gaps.push("所有场景都在用过的地点，至少换一个新地点".into());
        }
        if ledger["usedEndSituations"]
            .as_array()
            .is_some_and(|ends| ends.contains(&plan["endSituation"]))
        {
            gaps.push("结尾局面与前文相同".into());
        }
        for scene in plan["scenes"].as_array().into_iter().flatten() {
            for person in scene["people"].as_array().into_iter().flatten() {
                if !person["whyHere"]
                    .as_str()
                    .is_some_and(|w| w.chars().count() >= 4)
                {
                    gaps.push(format!(
                        "{} 的 whyHere 缺失",
                        person["name"].as_str().unwrap_or("?")
                    ));
                }
            }
        }
        if !gaps.is_empty() {
            let mut retry = input;
            retry["planFeedback"] = json!({"missing":gaps,"instruction":"上一版计划未通过检查：逐项修正 missing，返回完整计划。"});
            plan = lab_call(&gateway, &model, LAB4_PLAN_PROMPT, retry);
        }
        std::fs::write(
            out.join(format!("plan-{chapter}.json")),
            serde_json::to_string_pretty(&plan).unwrap(),
        )
        .unwrap();
        let anchor_use = if chapter == 1 {
            "姐姐往事在本章 emotionalBeat 处完整写一次"
        } else {
            "姐姐往事前文已完整写过：只能用一个新的细节或物件呼应，不得复述原来的场面和原话"
        };
        let write_input = json!({"chapter":chapter,"bible":bible,"previous":{"endState":previous["endState"],"chapterHook":previous["chapterHook"]},
            "plan":plan,"anchorUse":anchor_use,"lostThings":ledger["lostThings"],"styleCard":style,"exemplar":exemplar});
        let mut body = lab_call(&gateway, &model, LAB4_WRITE_PROMPT, write_input.clone())["body"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let length = body.chars().filter(|c| !c.is_whitespace()).count();
        if length < 2000 {
            let mut retry = write_input;
            retry["lengthFeedback"] = json!(format!("上一版只有 {length} 字，不足 2200：把每个 events 写成完整的场面，不加新反转，重写全章。"));
            body = lab_call(&gateway, &model, LAB4_WRITE_PROMPT, retry)["body"]
                .as_str()
                .unwrap_or("")
                .to_string();
        }
        std::fs::write(out.join(format!("draft-{chapter}.txt")), &body).unwrap();
        let findings = lab4_findings(&plan, &body, &bodies, &catchphrases);
        let mut applied = 0;
        if !findings.is_empty() {
            let edits = lab_call(
                &gateway,
                &model,
                LAB4_EDIT_PROMPT,
                json!({"body":body,"findings":findings}),
            );
            let (revised, count) = lab4_apply(&body, &edits);
            let length = revised.chars().filter(|c| !c.is_whitespace()).count();
            if count > 0 && length >= 2000 {
                body = revised;
                applied = count;
            }
        }
        std::fs::write(out.join(format!("chapter-{chapter}.txt")), &body).unwrap();
        println!(
            "CRAFT_LAB4 chapter={chapter} chars={} findings={} applied={applied}",
            body.chars().count(),
            findings.len()
        );
        report.push_str(&format!("\n## 第 {chapter} 章\n- 大转折：{}\n- 小发现：{}\n- 主角赢下的一局：{}\n- 异常手法：{}（{}）\n- 温度或幽默：{}\n- 情感时刻：{}\n- 新失去：{}\n- 结尾局面：{}\n- 写后检查：{} 条问题，采纳修改 {} 处\n",
            plan["bigTurn"].as_str().unwrap_or(""), plan["smallDiscovery"].as_str().unwrap_or(""), plan["win"].as_str().unwrap_or(""),
            plan["deviceUsed"]["device"].as_str().unwrap_or(""), plan["deviceUsed"]["newOrEscalated"].as_str().unwrap_or(""),
            plan["warmthOrHumor"].as_str().unwrap_or(""), plan["emotionalBeat"].as_str().unwrap_or(""),
            plan["newLosses"].as_array().map(|l| l.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join("、")).unwrap_or_default(),
            plan["endSituation"].as_str().unwrap_or(""), findings.len(), applied));
        for (key, value) in [
            ("usedDevices", plan["deviceUsed"]["device"].clone()),
            ("usedEndSituations", plan["endSituation"].clone()),
        ] {
            ledger[key].as_array_mut().unwrap().push(value);
        }
        for scene in plan["scenes"].as_array().into_iter().flatten() {
            ledger["usedPlaces"]
                .as_array_mut()
                .unwrap()
                .push(scene["place"].clone());
            for person in scene["people"].as_array().into_iter().flatten() {
                if !ledger["introducedPeople"]
                    .as_array()
                    .unwrap()
                    .contains(&person["name"])
                {
                    ledger["introducedPeople"]
                        .as_array_mut()
                        .unwrap()
                        .push(person["name"].clone());
                }
            }
        }
        for loss in plan["newLosses"].as_array().into_iter().flatten() {
            ledger["lostThings"]
                .as_array_mut()
                .unwrap()
                .push(loss.clone());
        }
        bodies.push(body.clone());
        previous = json!({"chapter":chapter,"endState":plan["endState"],"chapterHook":plan["chapterHook"]});
    }
    std::fs::write(out.join("plan-ledger.md"), report).unwrap();
    std::fs::write(
        out.join("ledger.json"),
        serde_json::to_string_pretty(&ledger).unwrap(),
    )
    .unwrap();
}

const LAB5_BIBLE_PROMPT: &str = "你是网文主编，为 setup 建故事底稿，供前三章使用。底稿里只写描述，不写任何可以直接放进正文的句子、台词或独白原文。返回 JSON：{\"normalWorld\":string,\"repairExperience\":string,\"worldRules\":[{\"rule\":string,\"numbers\":string,\"howReadersSeeIt\":string}],\"protagonist\":{\"name\":string,\"personality\":string,\"humorStyle\":string,\"innerVoiceStyle\":string,\"flaw\":string,\"wantsNow\":string},\"sisterAnchor\":{\"memorySummary\":string,\"object\":string},\"antagonist\":{\"name\":string,\"position\":string,\"wants\":string,\"method\":string},\"cast\":[{\"name\":string,\"role\":string,\"relation\":string,\"contrastWithProtagonist\":string,\"privateAgenda\":string,\"speechStyle\":string,\"firstAppears\":number}],\"arc\":{\"chapter1\":string,\"chapter2\":string,\"chapter3\":string},\"wins\":{\"chapter1\":string,\"chapter2\":string,\"chapter3\":string}}。要求：normalWorld 写主角平常一天的工作和他熟悉的人（如常去的小店、同事）；repairExperience 写进入别人记忆时的独特画面和规则；worldRules 3 到 5 条，numbers 给出具体数字；protagonist 的 humorStyle 和 innerVoiceStyle 用一句话描述风格，不要写示例句；antagonist 有名有姓有职位；cast 3 到 4 人，firstAppears 写他在第几章首次出场（1 到 3）；arc 第一章先写主角日常一天和人情温度、立住能力，章末出现第一个异常；第二章代价落在一个具体的人身上，主角与对手当面交锋；第三章主角做出改变局面的主动选择；wins 每章一次主角凭本事赢下的局面。";

const LAB5_PLAN_PROMPT: &str = "你是网文主编，依据 bible 和 craftGuidance 为第 chapter 章写计划，承接 previous，完成 bible.arc 与 bible.wins 中本章的任务。ledger 记录前文：usedDevices（不得原样再用，只能升级或换新）、usedPlaces（至少一个场景用新地点）、usedEndSituations（结尾局面不得相同）、lostThings（主角已失去，本章不得想起或使用）、introducedPeople（已登场人物）。出场人物只能是 bible 中 firstAppears 不晚于本章的人，或 introducedPeople 中的人。计划里只写发生什么，不写台词原句、独白原句、比喻和情绪形容词。整章 2 到 3 个场景。返回 JSON：{\"normalWorldScene\":string,\"bigTurn\":string,\"smallDiscovery\":string,\"win\":string,\"deviceUsed\":{\"device\":string,\"newOrEscalated\":string},\"warmthOrHumor\":string,\"emotionalBeat\":string,\"newLosses\":[string],\"scenes\":[{\"place\":string,\"people\":[{\"name\":string,\"wants\":string,\"hides\":string,\"whyHere\":string}],\"conflict\":string,\"events\":[string],\"readerQuestion\":string}],\"chapterHook\":string,\"endSituation\":string,\"endState\":string}。normalWorldScene 第一章必填（主角日常工作与熟人之间的一场戏），其他章可写空字符串；win 是主角凭本事赢下的具体一局；whyHere 写人物为何此刻在场，不能靠巧合；events 每场 3 到 5 条。";

const LAB5_WRITE_PROMPT: &str = "你是成熟的网文作者，按 plan 写第 chapter 章正文，人物与规则以 bible 为准，从 previous.endState 接着写。要求：1 情节推进优先：环境描写每个场景最多两三句，只写对情节有用的东西；动作和信息段落短，只有 emotionalBeat 处可以放慢写长；2 只写 plan 的一个大转折、一个小发现，写出主角赢下 win 的过程；3 第一章先写 normalWorldScene 的日常和人情，再出现异常；4 主角有自己的内心声音，按 bible.protagonist 的 innerVoiceStyle 和 humorStyle 现写，全章 3 到 5 处，每处说法都不同；5 只写主角在场看到、听到、想到的事，不写他不在场的画面，不写“他不知道”之类的作者旁白；6 anchorUse 说明姐姐往事怎么用；lostThings 中的东西主角已失去，不得回忆或使用；7 推理由人物说出，叙述者不解释、不总结；8 同一个物件或现象全章最多提三次，不要反复确认同一件事；9 比喻全章不超过两个，不写喉结、指尖、呼吸之类的身体小动作；10 章末停在 chapterHook 的事实上；11 全章 2200 到 3000 字，不分小节，不写标题。styleCard 是要学习的写法；exemplar 只用来体会节奏和叙述距离，不得照搬其中任何句子、人物、情节或设定。返回 JSON：{\"body\":string}。";

/// 10-character passages the body shares with the planning texts, and
/// 6-character passages it repeats three times or more.
fn lab5_findings(body: &str, sources: &str) -> Vec<serde_json::Value> {
    let chars = body.chars().collect::<Vec<_>>();
    let source_windows = {
        let src = sources.chars().collect::<Vec<_>>();
        (0..src.len().saturating_sub(9))
            .map(|i| src[i..i + 10].iter().collect::<String>())
            .collect::<std::collections::BTreeSet<_>>()
    };
    let sentences = lab4_sentences(body);
    let mut findings = Vec::new();
    for sentence in &sentences {
        if body.matches(sentence.as_str()).count() != 1 {
            continue;
        }
        let sc = sentence.chars().collect::<Vec<_>>();
        if (0..sc.len().saturating_sub(9))
            .any(|i| source_windows.contains(&sc[i..i + 10].iter().collect::<String>()))
        {
            findings.push(json!({"code":"redundancy","quote":sentence,"problem":"照搬了计划或设定里的原句，改成人物在场景中的新说法"}));
        }
    }
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for i in 0..chars.len().saturating_sub(5) {
        let w = chars[i..i + 6].iter().collect::<String>();
        if w.chars()
            .all(|c| !c.is_whitespace() && !"，。！？“”：、".contains(c))
        {
            *counts.entry(w).or_default() += 1;
        }
    }
    for (window, count) in counts.into_iter().filter(|(_, n)| *n >= 3) {
        for sentence in sentences
            .iter()
            .filter(|s| s.contains(&window) && body.matches(s.as_str()).count() == 1)
            .skip(1)
        {
            findings.push(json!({"code":"redundancy","quote":sentence,"problem":format!("「{window}」在本章出现 {count} 次，原地重复，删去或换成推进情节的内容")}));
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    findings.retain(|finding| seen.insert(finding["quote"].as_str().unwrap_or("").to_string()));
    findings
}

#[test]
#[ignore = "real provider craft lab 5: no ready-made sentences, POV discipline, loop and copy checks; paid calls"]
fn configured_provider_craft_lab_writes_round_five() {
    use ai_novel_production_system_core::chapter_craft::guidance;
    use ai_novel_production_system_core::skills_v2::StepSignals;
    assert_eq!(
        std::env::var("MOHENG_RUN_REAL_CRAFT_AB").as_deref(),
        Ok("1")
    );
    let out = PathBuf::from(std::env::var("MOHENG_CRAFT_LAB_OUT").expect("MOHENG_CRAFT_LAB_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let exemplar =
        std::fs::read_to_string(std::env::var("MOHENG_CRAFT_LAB_EXEMPLAR").unwrap()).unwrap();
    let style: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("MOHENG_CRAFT_LAB_STYLE").unwrap()).unwrap(),
    )
    .unwrap();
    let model = std::env::var("MOHENG_CRAFT_LAB_MODELS").unwrap();
    let (paths, _) = real_state();
    let gateway = ConfiguredProductionGateway::for_desktop(paths);
    let bible = lab_call(
        &gateway,
        &model,
        LAB5_BIBLE_PROMPT,
        json!({"setup":LAB_SETUP}),
    );
    std::fs::write(
        out.join("bible.json"),
        serde_json::to_string_pretty(&bible).unwrap(),
    )
    .unwrap();
    let known = |chapter: usize, ledger: &serde_json::Value| -> Vec<String> {
        let mut names = vec![
            bible["protagonist"]["name"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            bible["antagonist"]["name"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        ];
        for person in bible["cast"].as_array().into_iter().flatten() {
            if person["firstAppears"].as_u64().unwrap_or(1) as usize <= chapter {
                names.push(person["name"].as_str().unwrap_or("").to_string());
            }
        }
        names.extend(
            ledger["introducedPeople"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|n| n.as_str())
                .map(ToOwned::to_owned),
        );
        names
    };
    let mut ledger = json!({"usedDevices":[],"usedPlaces":[],"usedEndSituations":[],"lostThings":[],"introducedPeople":[]});
    let mut bodies: Vec<String> = Vec::new();
    let mut previous = json!(null);
    let mut report = String::from("# 第五轮计划与修改落实表\n");
    for chapter in 1..=3usize {
        let craft = guidance(
            "chapter_plan",
            &StepSignals {
                chapter_number: Some(chapter as u32),
                ..Default::default()
            },
        )
        .unwrap();
        let input = json!({"chapter":chapter,"bible":bible,"previous":previous,"ledger":ledger,"craftGuidance":craft});
        let mut plan = lab_call(&gateway, &model, LAB5_PLAN_PROMPT, input.clone());
        let check = |plan: &serde_json::Value| -> Vec<String> {
            let mut gaps = lab3_plan_gaps(plan, &[]);
            if !plan["win"].as_str().is_some_and(|w| w.chars().count() >= 6) {
                gaps.push("win 缺失".into());
            }
            if chapter == 1
                && !plan["normalWorldScene"]
                    .as_str()
                    .is_some_and(|w| w.chars().count() >= 10)
            {
                gaps.push("第一章 normalWorldScene 缺失：先写主角日常和熟人".into());
            }
            let used_places = ledger["usedPlaces"].as_array().cloned().unwrap_or_default();
            if chapter > 1
                && !plan["scenes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|scene| !used_places.contains(&scene["place"]))
            {
                gaps.push("至少换一个新地点".into());
            }
            let names = known(chapter, &ledger);
            for scene in plan["scenes"].as_array().into_iter().flatten() {
                for person in scene["people"].as_array().into_iter().flatten() {
                    let name = person["name"].as_str().unwrap_or("");
                    if !names.iter().any(|known| {
                        !known.is_empty() && (name.contains(known.as_str()) || known.contains(name))
                    }) {
                        gaps.push(format!(
                            "{name} 未在 bible 中安排到本章或之前出场，换成已铺垫的人物"
                        ));
                    }
                    if !person["whyHere"]
                        .as_str()
                        .is_some_and(|w| w.chars().count() >= 4)
                    {
                        gaps.push(format!("{name} 的 whyHere 缺失"));
                    }
                }
            }
            gaps
        };
        let gaps = check(&plan);
        if !gaps.is_empty() {
            let mut retry = input;
            retry["planFeedback"] = json!({"missing":gaps,"instruction":"上一版计划未通过检查：逐项修正 missing，返回完整计划。"});
            plan = lab_call(&gateway, &model, LAB5_PLAN_PROMPT, retry);
        }
        std::fs::write(
            out.join(format!("plan-{chapter}.json")),
            serde_json::to_string_pretty(&plan).unwrap(),
        )
        .unwrap();
        let anchor_use = if chapter == 1 {
            "姐姐往事在 emotionalBeat 处现写一次，用具体场面和物件"
        } else {
            "姐姐往事前文已写过：只能用一个新的细节呼应，不得复述原来的场面"
        };
        let write_input = json!({"chapter":chapter,"bible":bible,"previous":{"endState":previous["endState"],"chapterHook":previous["chapterHook"]},
            "plan":plan,"anchorUse":anchor_use,"lostThings":ledger["lostThings"],"styleCard":style,"exemplar":exemplar});
        let mut body = lab_call(&gateway, &model, LAB5_WRITE_PROMPT, write_input.clone())["body"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let length = body.chars().filter(|c| !c.is_whitespace()).count();
        if !(2000..=3200).contains(&length) {
            let mut retry = write_input;
            retry["lengthFeedback"] = json!(if length < 2000 {
                format!("上一版只有 {length} 字，不足 2200：把每个 events 写成完整的场面，不加新反转，重写全章。")
            } else {
                format!("上一版 {length} 字，超过 3000：删去重复确认和多余描写，重写全章到 2600 字左右。")
            });
            body = lab_call(&gateway, &model, LAB5_WRITE_PROMPT, retry)["body"]
                .as_str()
                .unwrap_or("")
                .to_string();
        }
        std::fs::write(out.join(format!("draft-{chapter}.txt")), &body).unwrap();
        let sources = format!(
            "{}\n{}",
            serde_json::to_string(&bible).unwrap(),
            serde_json::to_string(&plan).unwrap()
        );
        let mut findings = lab4_findings(&plan, &body, &bodies, &[]);
        findings.extend(lab5_findings(&body, &sources));
        let mut seen = std::collections::BTreeSet::new();
        findings.retain(|finding| seen.insert(finding["quote"].as_str().unwrap_or("").to_string()));
        findings.truncate(16);
        let mut applied = 0;
        if !findings.is_empty() {
            let edits = lab_call(
                &gateway,
                &model,
                LAB4_EDIT_PROMPT,
                json!({"body":body,"findings":findings}),
            );
            let (revised, count) = lab4_apply(&body, &edits);
            if count > 0 && revised.chars().filter(|c| !c.is_whitespace()).count() >= 2000 {
                body = revised;
                applied = count;
            }
        }
        std::fs::write(out.join(format!("chapter-{chapter}.txt")), &body).unwrap();
        println!(
            "CRAFT_LAB5 chapter={chapter} chars={} findings={} applied={applied}",
            body.chars().count(),
            findings.len()
        );
        report.push_str(&format!("\n## 第 {chapter} 章\n- 日常戏：{}\n- 大转折：{}\n- 小发现：{}\n- 主角赢下的一局：{}\n- 异常手法：{}（{}）\n- 温度或幽默：{}\n- 情感时刻：{}\n- 新失去：{}\n- 结尾局面：{}\n- 写后检查：{} 条，采纳修改 {} 处\n",
            plan["normalWorldScene"].as_str().unwrap_or(""), plan["bigTurn"].as_str().unwrap_or(""), plan["smallDiscovery"].as_str().unwrap_or(""), plan["win"].as_str().unwrap_or(""),
            plan["deviceUsed"]["device"].as_str().unwrap_or(""), plan["deviceUsed"]["newOrEscalated"].as_str().unwrap_or(""),
            plan["warmthOrHumor"].as_str().unwrap_or(""), plan["emotionalBeat"].as_str().unwrap_or(""),
            plan["newLosses"].as_array().map(|l| l.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join("、")).unwrap_or_default(),
            plan["endSituation"].as_str().unwrap_or(""), findings.len(), applied));
        for (key, value) in [
            ("usedDevices", plan["deviceUsed"]["device"].clone()),
            ("usedEndSituations", plan["endSituation"].clone()),
        ] {
            ledger[key].as_array_mut().unwrap().push(value);
        }
        for scene in plan["scenes"].as_array().into_iter().flatten() {
            ledger["usedPlaces"]
                .as_array_mut()
                .unwrap()
                .push(scene["place"].clone());
            for person in scene["people"].as_array().into_iter().flatten() {
                if !ledger["introducedPeople"]
                    .as_array()
                    .unwrap()
                    .contains(&person["name"])
                {
                    ledger["introducedPeople"]
                        .as_array_mut()
                        .unwrap()
                        .push(person["name"].clone());
                }
            }
        }
        for loss in plan["newLosses"].as_array().into_iter().flatten() {
            ledger["lostThings"]
                .as_array_mut()
                .unwrap()
                .push(loss.clone());
        }
        bodies.push(body.clone());
        previous = json!({"chapter":chapter,"endState":plan["endState"],"chapterHook":plan["chapterHook"]});
    }
    std::fs::write(out.join("plan-ledger.md"), report).unwrap();
}

const LAB6_FACTS_PROMPT: &str = "你是连载编辑，从 body（第 chapter 章正文）里提取后文必须保持一致的事实。返回 JSON：{\"facts\":[string]}。每条一句，写清对象和事实，只写正文里明确写出的：人物的性别、年龄、身份、外貌特征和与主角的关系；关键物件的样子和下落；记忆场景里出现的人和物；地点；已交代的规则、数字和期限；主角已经知道的秘密。15 到 30 条。";

const LAB6_PLAN_EXTRA: &str = "额外要求：1 establishedFacts 是前文已写定的事实，计划不得与之矛盾；2 previousScenePlaces 是上一章的场景地点，本章最多沿用其中一个；previousHelperRoles 记录上一章配角承担的作用，同一个配角不得连续两章只负责递线索或提醒；3 返回的 JSON 另加两个字段：\"situationChange\":{\"before\":string,\"after\":string}（本章开始与结束时主角处境的具体差别），\"helperRoles\":[{\"name\":string,\"role\":string}]；第三章的 situationChange 必须写主角做了什么具体行动、他和对手的关系因此怎样改变。";

const LAB6_WRITE_EXTRA: &str = "额外要求：establishedFacts 是前文已写定的事实，正文不得与之矛盾（人物性别、物件样子、地点细节都要一致）。";

const LAB6_COMPRESS_PROMPT: &str = "你是网文编辑。body 太长，压缩到 2600 到 2900 字：删去走路、坐电梯、开门刷卡这类过程性流水账和重复确认，合并重复信息；保留全部情节、对话里的关键信息、人物反应和章末钩子；不改情节、人物和视角，不新增比喻。返回 JSON：{\"body\":string}。";

#[test]
#[ignore = "real provider craft lab 6: facts ledger, scene variety, situation change and compression; paid calls"]
fn configured_provider_craft_lab_writes_round_six() {
    use ai_novel_production_system_core::chapter_craft::guidance;
    use ai_novel_production_system_core::skills_v2::StepSignals;
    assert_eq!(
        std::env::var("MOHENG_RUN_REAL_CRAFT_AB").as_deref(),
        Ok("1")
    );
    let out = PathBuf::from(std::env::var("MOHENG_CRAFT_LAB_OUT").expect("MOHENG_CRAFT_LAB_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let exemplar =
        std::fs::read_to_string(std::env::var("MOHENG_CRAFT_LAB_EXEMPLAR").unwrap()).unwrap();
    let style: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("MOHENG_CRAFT_LAB_STYLE").unwrap()).unwrap(),
    )
    .unwrap();
    let model = std::env::var("MOHENG_CRAFT_LAB_MODELS").unwrap();
    let (paths, _) = real_state();
    let gateway = ConfiguredProductionGateway::for_desktop(paths);
    let bible = lab_call(
        &gateway,
        &model,
        LAB5_BIBLE_PROMPT,
        json!({"setup":LAB_SETUP}),
    );
    std::fs::write(
        out.join("bible.json"),
        serde_json::to_string_pretty(&bible).unwrap(),
    )
    .unwrap();
    let plan_prompt = format!("{LAB5_PLAN_PROMPT}{LAB6_PLAN_EXTRA}");
    let write_prompt = format!("{LAB5_WRITE_PROMPT}{LAB6_WRITE_EXTRA}");
    let known = |chapter: usize, ledger: &serde_json::Value| -> Vec<String> {
        let mut names = vec![
            bible["protagonist"]["name"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            bible["antagonist"]["name"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        ];
        for person in bible["cast"].as_array().into_iter().flatten() {
            if person["firstAppears"].as_u64().unwrap_or(1) as usize <= chapter {
                names.push(person["name"].as_str().unwrap_or("").to_string());
            }
        }
        names.extend(
            ledger["introducedPeople"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|n| n.as_str())
                .map(ToOwned::to_owned),
        );
        names
    };
    let mut ledger = json!({"usedDevices":[],"usedPlaces":[],"usedEndSituations":[],"lostThings":[],"introducedPeople":[]});
    let mut facts: Vec<serde_json::Value> = Vec::new();
    let mut previous_places: Vec<serde_json::Value> = Vec::new();
    let mut previous_roles = json!([]);
    let mut bodies: Vec<String> = Vec::new();
    let mut previous = json!(null);
    let mut report = String::from("# 第六轮计划与修改落实表\n");
    for chapter in 1..=3usize {
        let craft = guidance(
            "chapter_plan",
            &StepSignals {
                chapter_number: Some(chapter as u32),
                ..Default::default()
            },
        )
        .unwrap();
        let input = json!({"chapter":chapter,"bible":bible,"previous":previous,"ledger":ledger,"establishedFacts":facts,
            "previousScenePlaces":previous_places,"previousHelperRoles":previous_roles,"craftGuidance":craft});
        let check = |plan: &serde_json::Value| -> Vec<String> {
            let mut gaps = lab3_plan_gaps(plan, &[]);
            if !plan["win"].as_str().is_some_and(|w| w.chars().count() >= 6) {
                gaps.push("win 缺失".into());
            }
            if chapter == 1
                && !plan["normalWorldScene"]
                    .as_str()
                    .is_some_and(|w| w.chars().count() >= 10)
            {
                gaps.push("第一章 normalWorldScene 缺失".into());
            }
            for key in ["before", "after"] {
                if !plan["situationChange"][key]
                    .as_str()
                    .is_some_and(|w| w.chars().count() >= 8)
                {
                    gaps.push(format!("situationChange.{key} 缺失或空泛"));
                }
            }
            let places = plan["scenes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|scene| scene["place"].clone())
                .collect::<Vec<_>>();
            let reused = places
                .iter()
                .filter(|place| previous_places.contains(place))
                .count();
            if reused > 1 {
                gaps.push(format!("沿用上一章地点 {reused} 个，最多一个"));
            }
            let names = known(chapter, &ledger);
            for scene in plan["scenes"].as_array().into_iter().flatten() {
                for person in scene["people"].as_array().into_iter().flatten() {
                    let name = person["name"].as_str().unwrap_or("");
                    if !names
                        .iter()
                        .any(|k| !k.is_empty() && (name.contains(k.as_str()) || k.contains(name)))
                    {
                        gaps.push(format!("{name} 未在 bible 中安排到本章或之前出场"));
                    }
                }
            }
            gaps
        };
        let mut plan = lab_call(&gateway, &model, &plan_prompt, input.clone());
        let gaps = check(&plan);
        if !gaps.is_empty() {
            let mut retry = input;
            retry["planFeedback"] = json!({"missing":gaps,"instruction":"上一版计划未通过检查：逐项修正 missing，返回完整计划。"});
            plan = lab_call(&gateway, &model, &plan_prompt, retry);
        }
        std::fs::write(
            out.join(format!("plan-{chapter}.json")),
            serde_json::to_string_pretty(&plan).unwrap(),
        )
        .unwrap();
        let anchor_use = if chapter == 1 {
            "姐姐往事在 emotionalBeat 处现写一次，用具体场面和物件"
        } else {
            "姐姐往事前文已写过：只能用一个新的细节呼应，不得复述原来的场面"
        };
        let write_input = json!({"chapter":chapter,"bible":bible,"previous":{"endState":previous["endState"],"chapterHook":previous["chapterHook"]},
            "plan":plan,"anchorUse":anchor_use,"lostThings":ledger["lostThings"],"establishedFacts":facts,"styleCard":style,"exemplar":exemplar});
        let mut body = lab_call(&gateway, &model, &write_prompt, write_input.clone())["body"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let count = |text: &str| text.chars().filter(|c| !c.is_whitespace()).count();
        if count(&body) < 2000 {
            let mut retry = write_input;
            retry["lengthFeedback"] = json!(format!(
                "上一版只有 {} 字，不足 2200：把每个 events 写成完整的场面，不加新反转，重写全章。",
                count(&body)
            ));
            body = lab_call(&gateway, &model, &write_prompt, retry)["body"]
                .as_str()
                .unwrap_or("")
                .to_string();
        }
        if count(&body) > 3100 {
            let compressed = lab_call(&gateway, &model, LAB6_COMPRESS_PROMPT, json!({"body":body}))
                ["body"]
                .as_str()
                .unwrap_or("")
                .to_string();
            if (2200..=3200).contains(&count(&compressed)) {
                body = compressed;
            }
        }
        std::fs::write(out.join(format!("draft-{chapter}.txt")), &body).unwrap();
        let sources = format!(
            "{}\n{}",
            serde_json::to_string(&bible).unwrap(),
            serde_json::to_string(&plan).unwrap()
        );
        let mut findings = lab4_findings(&plan, &body, &bodies, &[]);
        findings.extend(lab5_findings(&body, &sources));
        let mut seen = std::collections::BTreeSet::new();
        findings.retain(|finding| seen.insert(finding["quote"].as_str().unwrap_or("").to_string()));
        findings.truncate(16);
        let mut applied = 0;
        if !findings.is_empty() {
            let edits = lab_call(
                &gateway,
                &model,
                LAB4_EDIT_PROMPT,
                json!({"body":body,"findings":findings}),
            );
            let (revised, n) = lab4_apply(&body, &edits);
            if n > 0 && count(&revised) >= 2000 {
                body = revised;
                applied = n;
            }
        }
        std::fs::write(out.join(format!("chapter-{chapter}.txt")), &body).unwrap();
        let extracted = lab_call(
            &gateway,
            &model,
            LAB6_FACTS_PROMPT,
            json!({"chapter":chapter,"body":body}),
        );
        facts.extend(
            extracted["facts"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|fact| json!(format!("第{chapter}章：{}", fact.as_str().unwrap_or("")))),
        );
        println!(
            "CRAFT_LAB6 chapter={chapter} chars={} findings={} applied={applied} facts={}",
            count(&body),
            findings.len(),
            facts.len()
        );
        report.push_str(&format!("\n## 第 {chapter} 章\n- 处境变化：{} → {}\n- 大转折：{}\n- 主角赢下的一局：{}\n- 场景：{}\n- 配角作用：{}\n- 温度或幽默：{}\n- 写后检查：{} 条，采纳修改 {} 处\n",
            plan["situationChange"]["before"].as_str().unwrap_or(""), plan["situationChange"]["after"].as_str().unwrap_or(""),
            plan["bigTurn"].as_str().unwrap_or(""), plan["win"].as_str().unwrap_or(""),
            plan["scenes"].as_array().map(|s| s.iter().filter_map(|x| x["place"].as_str()).collect::<Vec<_>>().join(" → ")).unwrap_or_default(),
            plan["helperRoles"].as_array().map(|r| r.iter().map(|x| format!("{}：{}", x["name"].as_str().unwrap_or(""), x["role"].as_str().unwrap_or(""))).collect::<Vec<_>>().join("；")).unwrap_or_default(),
            plan["warmthOrHumor"].as_str().unwrap_or(""), findings.len(), applied));
        for (key, value) in [
            ("usedDevices", plan["deviceUsed"]["device"].clone()),
            ("usedEndSituations", plan["endSituation"].clone()),
        ] {
            ledger[key].as_array_mut().unwrap().push(value);
        }
        previous_places = plan["scenes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|scene| scene["place"].clone())
            .collect();
        previous_roles = plan["helperRoles"].clone();
        for scene in plan["scenes"].as_array().into_iter().flatten() {
            ledger["usedPlaces"]
                .as_array_mut()
                .unwrap()
                .push(scene["place"].clone());
            for person in scene["people"].as_array().into_iter().flatten() {
                if !ledger["introducedPeople"]
                    .as_array()
                    .unwrap()
                    .contains(&person["name"])
                {
                    ledger["introducedPeople"]
                        .as_array_mut()
                        .unwrap()
                        .push(person["name"].clone());
                }
            }
        }
        for loss in plan["newLosses"].as_array().into_iter().flatten() {
            ledger["lostThings"]
                .as_array_mut()
                .unwrap()
                .push(loss.clone());
        }
        bodies.push(body.clone());
        previous = json!({"chapter":chapter,"endState":plan["endState"],"chapterHook":plan["chapterHook"]});
    }
    std::fs::write(out.join("plan-ledger.md"), report).unwrap();
    std::fs::write(
        out.join("facts.json"),
        serde_json::to_string_pretty(&facts).unwrap(),
    )
    .unwrap();
}
