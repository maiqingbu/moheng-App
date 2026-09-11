use ai_novel_production_system_core::catalog::CatalogRepository;
use ai_novel_production_system_core::commands::{
    execute_command_impl, query_assistant_repair_plan_impl, query_book_health_impl,
    query_canon_impl, query_chapter_versions_impl, query_decisions_impl, query_history_impl,
    query_market_strategy_impl, query_opening_ready_impl, query_project_library_impl,
    query_project_overview_impl, query_story_directions_impl, query_story_setup_impl,
    query_volume_detail_impl, support_session_impl, KernelState,
};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::json;
use std::sync::Arc;

const NOW: &str = "2026-09-02T02:00:00Z";

struct EntryFixtureGateway;
struct FailingEntryGateway;

impl ProductionModelGateway for EntryFixtureGateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        Ok(ResolvedProviderProfile {
            provider: "fixture-provider".into(),
            model: "fixture-model".into(),
            endpoint: "fixture://provider".into(),
            timeout_ms: 1_000,
        })
    }

    fn invoke(
        &self,
        capability: &str,
        input: &serde_json::Value,
    ) -> Result<ProductionModelResponse, String> {
        if capability != "GENERATE_STORY_DIRECTIONS" {
            return Err(format!("unexpected capability: {capability}"));
        }
        let source = input
            .pointer("/input/sourceText")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("用户故事想法");
        let adjustment = input
            .pointer("/input/adjustment")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let pitches = (0..3)
            .map(|index| {
                json!({
                    "name":format!("真实方向 {}", index + 1),
                    "promise":format!("围绕 {source} {adjustment} 的实质故事承诺 {index}"),
                    "protagonistDesire":format!("主角主动追求 {source} {adjustment} 的目标 {index}"),
                    "mechanism":format!("可验证机制 {source} {adjustment} {index}"),
                    "worldConflict":format!("世界冲突 {source} {adjustment} {index}"),
                    "longArc":format!("长线弧光 {source} {adjustment} {index}"),
                    "openingHook":format!("开篇钩子 {source} {adjustment} {index}"),
                    "volumeGoal":format!("卷目标 {source} {adjustment} {index}"),
                    "commercialEdge":format!("商业优势 {source} {adjustment} {index}"),
                    "originality":format!("原创性 {source} {adjustment} {index}"),
                    "difficulty":format!("难度 {source} {adjustment} {index}"),
                    "failureRisk":format!("失败风险 {source} {adjustment} {index}")
                })
            })
            .collect::<Vec<_>>();
        Ok(ProductionModelResponse::deterministic(
            json!({"pitches":pitches}),
        ))
    }
}

impl ProductionModelGateway for FailingEntryGateway {
    fn invoke(
        &self,
        _capability: &str,
        _input: &serde_json::Value,
    ) -> Result<ProductionModelResponse, String> {
        Err("entry-provider-fixture-failed".into())
    }
}

fn state_with_gateway(gateway: Arc<dyn ProductionModelGateway>) -> KernelState {
    let temp = tempfile::tempdir().unwrap();
    // Keep the temporary directory alive for the duration of each test by
    // leaking only this test-owned path; the project manager owns the files.
    let root = Box::leak(Box::new(temp)).path().to_path_buf();
    KernelState::with_production_gateway(AppPaths::new(root), gateway)
}

fn state() -> KernelState {
    state_with_gateway(Arc::new(EntryFixtureGateway))
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

fn generate_directions_fixture(state: &KernelState, project_id: &str, command_id: &str) {
    let generated = execute_command_impl(
        state,
        json!({
            "type":"story_directions_generate",
            "commandId":command_id,
            "idempotencyKey":format!("{command_id}:key"),
            "projectId":project_id,
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{}
        }),
    )
    .unwrap();
    assert_eq!(generated["finalStatus"], "committed");
}

fn create_idea_fixture(state: &KernelState, command_id: &str) -> String {
    let created = execute_command_impl(state, idea_request(command_id)).unwrap();
    assert_eq!(created["finalStatus"], "committed");
    let project_id = created["projectId"].as_str().unwrap().to_owned();
    generate_directions_fixture(state, &project_id, &format!("{command_id}:directions"));
    project_id
}

fn market_request(command_id: &str, material: &str) -> serde_json::Value {
    json!({
        "type":"project_create_from_market",
        "commandId":command_id,
        "idempotencyKey":format!("{command_id}:key"),
        "requestedAt":NOW,
        "payload":{
            "title":"市场输入因果测试",
            "marketSource":"用户导入的市场观察",
            "marketMaterial":material,
            "platform":"起点中文网",
            "length":"80–120 万字",
            "tags":["悬疑"],
            "forbidden":"不要套路化误会"
        }
    })
}

fn failing_story_direction_request(
    command_type: &str,
    project_id: &str,
    command_id: &str,
    expected_revision: i64,
) -> serde_json::Value {
    json!({
        "type":command_type,
        "commandId":command_id,
        "idempotencyKey":format!("{command_id}:key"),
        "projectId":project_id,
        "expectedRevision":expected_revision,
        "requestedAt":NOW,
        "payload":{}
    })
}

fn mark_legacy_blank_title(state: &KernelState, project_id: &str) {
    let handle = state.manager.open(project_id, "", NOW).unwrap();
    handle
        .execute(
            "UPDATE project_meta SET title='' WHERE project_id=?1",
            [project_id],
        )
        .unwrap();
    drop(handle);
    let catalog = CatalogRepository::open(&state.paths, NOW).unwrap();
    CatalogRepository::rename(&catalog, project_id, "", NOW).unwrap();
}

fn create_opening_ready_fixture(state: &KernelState, prefix: &str) -> String {
    let created = execute_command_impl(state, json!({
        "type":"project_create_from_idea",
        "commandId":format!("{prefix}:create"),
        "idempotencyKey":format!("{prefix}:create:key"),
        "requestedAt":NOW,
        "payload":{"title":"真实 owner 流程","idea":"近未来城市里的记忆调查员追查一宗被篡改的档案犯罪。","platform":"番茄小说","mode":"DIRECTOR"}
    })).unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_owned();
    generate_directions_fixture(state, &project_id, &format!("{prefix}:directions"));
    let directions = query_story_directions_impl(
        state,
        json!({"projectId":project_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    let pitch_id = directions["pitches"][0]["id"].as_str().unwrap();
    execute_command_impl(state, json!({
        "type":"pitch_select","commandId":format!("{prefix}:select"),"idempotencyKey":format!("{prefix}:select:key"),
        "projectId":project_id,"expectedRevision":1,"requestedAt":NOW,"payload":{"pitchIds":[pitch_id],"mode":"single"}
    })).unwrap();
    execute_command_impl(state, json!({
        "type":"story_setup_save","commandId":format!("{prefix}:setup"),"idempotencyKey":format!("{prefix}:setup:key"),
        "projectId":project_id,"expectedRevision":2,"requestedAt":NOW,
        "payload":{"storyCore":{"protagonist":"记忆调查员","desire":"找回被篡改的真实经历","mechanism":"档案校验会留下可追踪痕迹","conflict":"城市档案权与个人记忆冲突","readerPromise":"每次调查都揭开一层真实代价","forbiddenReveals":[]}}
    })).unwrap();
    execute_command_impl(state, json!({
        "type":"opening_ready","commandId":format!("{prefix}:opening"),"idempotencyKey":format!("{prefix}:opening:key"),
        "projectId":project_id,"expectedRevision":3,"requestedAt":NOW,"payload":{"approved":true}
    })).unwrap();
    project_id
}

#[test]
fn b11_project_creation_defers_provider_and_initial_generation_reuses_project() {
    let state = state();
    let mut request = idea_request("cmd:b11:s1:split");
    request["payload"]["title"] = json!("   ");

    let created = execute_command_impl(&state, request).unwrap();
    assert_eq!(created["finalStatus"], "committed");
    assert!(created["entryReceipt"]["receiptId"].as_str().is_some());
    let project_id = created["projectId"].as_str().unwrap().to_owned();

    state
        .manager
        .with_read_connection(&project_id, NOW, |connection| {
            let pitch_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='ProjectPitch'",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            let job_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM jobs WHERE project_id=?1",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            let model_run_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM model_runs WHERE project_id=?1",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(pitch_count, 0);
            assert_eq!(job_count, 0);
            assert_eq!(model_run_count, 0);
            let title: String = connection
                .query_row(
                    "SELECT title FROM project_meta WHERE project_id=?1",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(title, "未命名作品");
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();

    let generated = execute_command_impl(
        &state,
        json!({
            "type":"story_directions_generate",
            "commandId":"cmd:b11:s1:split:directions",
            "idempotencyKey":"cmd:b11:s1:split:directions:key",
            "projectId":project_id,
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{}
        }),
    )
    .unwrap();
    assert_eq!(generated["finalStatus"], "committed");

    let directions = query_story_directions_impl(
        &state,
        json!({"projectId":project_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    assert_eq!(directions["pitches"].as_array().unwrap().len(), 3);

    let retried = execute_command_impl(
        &state,
        json!({
            "type":"story_directions_generate",
            "commandId":"cmd:b11:s1:split:retry",
            "idempotencyKey":"cmd:b11:s1:split:retry:key",
            "projectId":project_id,
            "expectedRevision":1,
            "requestedAt":NOW,
            "payload":{}
        }),
    )
    .unwrap();
    assert_eq!(retried["finalStatus"], "committed");
    state
        .manager
        .with_read_connection(&project_id, NOW, |connection| {
            let project_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM project_meta WHERE project_id=?1",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            let job_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM jobs WHERE project_id=?1",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            let model_run_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM model_runs WHERE project_id=?1",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(project_count, 1);
            assert_eq!(job_count, 2);
            assert_eq!(model_run_count, 2);
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();
}

#[test]
fn b11_s1_retry_lineage_keeps_failed_job_and_links_the_new_job() {
    let state = state_with_gateway(Arc::new(FailingEntryGateway));
    let created = execute_command_impl(&state, idea_request("cmd:b11:s1:lineage")).unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_owned();

    let first = execute_command_impl(
        &state,
        failing_story_direction_request(
            "story_directions_generate",
            &project_id,
            "cmd:b11:s1:lineage:first",
            0,
        ),
    )
    .unwrap_err();
    assert!(first.contains("transport") || first.contains("unavailable"));
    let retry = execute_command_impl(
        &state,
        failing_story_direction_request(
            "pitch_regenerate",
            &project_id,
            "cmd:b11:s1:lineage:retry",
            0,
        ),
    )
    .unwrap_err();
    assert!(retry.contains("transport") || retry.contains("unavailable"));

    state
        .manager
        .with_read_connection(&project_id, NOW, |connection| {
            let jobs: Vec<(String, String)> = connection
                .prepare(
                    "SELECT job_id,status FROM jobs WHERE project_id=?1 ORDER BY created_at,job_id",
                )
                .unwrap()
                .query_map([&project_id], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(jobs.len(), 2);
            assert_eq!(jobs[0].1, "FAILED");
            assert_eq!(jobs[1].1, "FAILED");
            let lineage: (String, String) = connection
                .query_row(
                    "SELECT job_id,payload_json FROM production_events WHERE project_id=?1 AND event_type='JOB_RETRY_CREATED'",
                    [&project_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert!(jobs.iter().any(|job| job.0 == lineage.0));
            let lineage_payload: serde_json::Value = serde_json::from_str(&lineage.1).unwrap();
            let retry_of = lineage_payload["retryOfJobId"].as_str().unwrap();
            assert_ne!(lineage.0, retry_of);
            assert!(jobs.iter().any(|job| job.0 == retry_of));
            println!(
                "S1_R1_RETRY_LINEAGE {}",
                serde_json::json!({"oldJobId":retry_of,"newJobId":lineage.0,"oldStatus":jobs.iter().find(|job| job.0 == retry_of).unwrap().1,"newStatus":jobs.iter().find(|job| job.0 == lineage.0).unwrap().1,"lineagePersisted":true})
            );
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();
}

#[test]
fn b11_s1_retry_x10_keeps_one_project_and_adds_only_jobs_and_model_runs() {
    let state = state_with_gateway(Arc::new(FailingEntryGateway));
    let created = execute_command_impl(&state, idea_request("cmd:b11:s1:x10")).unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_owned();
    execute_command_impl(
        &state,
        failing_story_direction_request(
            "story_directions_generate",
            &project_id,
            "cmd:b11:s1:x10:first",
            0,
        ),
    )
    .unwrap_err();
    for attempt in 1..=10 {
        execute_command_impl(
            &state,
            failing_story_direction_request(
                "pitch_regenerate",
                &project_id,
                &format!("cmd:b11:s1:x10:retry-{attempt}"),
                0,
            ),
        )
        .unwrap_err();
    }

    let library = query_project_library_impl(&state).unwrap();
    let projects = library["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["id"], project_id);
    state
        .manager
        .with_read_connection(&project_id, NOW, |connection| {
            let job_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM jobs WHERE project_id=?1",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            let model_run_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM model_runs WHERE project_id=?1",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            let lineage_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM production_events WHERE project_id=?1 AND event_type='JOB_RETRY_CREATED'",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(job_count, 11);
            assert_eq!(model_run_count, 11);
            assert_eq!(lineage_count, 10);
            println!(
                "S1_R1_RETRY_X10 {}",
                serde_json::json!({"projectId":project_id,"attempts":10,"projectCountDelta":0,"jobCountDelta":10,"modelRunCountDelta":10,"lineageCount":lineage_count})
            );
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();
}

#[test]
fn b11_s1_title_is_equal_across_catalog_brief_overview_and_library() {
    let state = state();
    let mut request = idea_request("cmd:b11:s1:title-cross-layer");
    request["payload"]["title"] = json!("  跨层统一标题  ");
    let created = execute_command_impl(&state, request).unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_owned();

    let catalog = CatalogRepository::open(&state.paths, NOW).unwrap();
    let catalog_title: String = catalog
        .query_row(
            "SELECT title FROM projects WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let overview = query_project_overview_impl(&state, json!({"projectId":project_id})).unwrap();
    let library = query_project_library_impl(&state).unwrap();
    let library_title = library["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["id"] == project_id)
        .unwrap()["title"]
        .as_str()
        .unwrap()
        .to_owned();
    state
        .manager
        .with_read_connection(&project_id, NOW, |connection| {
            let brief_payload: String = connection
                .query_row(
                    "SELECT p.payload_json FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='CreativeBrief' LIMIT 1",
                    [&project_id],
                    |row| row.get(0),
                )
                .unwrap();
            let brief: serde_json::Value = serde_json::from_str(&brief_payload).unwrap();
            let titles = [
                catalog_title.as_str(),
                brief["title"].as_str().unwrap(),
                overview["project"]["title"].as_str().unwrap(),
                library_title.as_str(),
            ];
            assert!(titles.iter().all(|title| *title == "跨层统一标题"));
            println!(
                "S1_R1_TITLE_CROSS_LAYER {}",
                serde_json::json!({"projectId":project_id,"catalogTitle":titles[0],"creativeBriefTitle":titles[1],"headerTitle":titles[2],"libraryTitle":titles[3],"deleteTargetTitle":titles[3],"allEqual":true})
            );
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();
}

#[test]
fn b11_s1_legacy_blank_title_reads_with_canonical_fallback() {
    let state = state();
    let mut request = idea_request("cmd:b11:s1:legacy-read");
    request["payload"]["title"] = json!("历史空标题作品");
    let created = execute_command_impl(&state, request).unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_owned();
    mark_legacy_blank_title(&state, &project_id);

    let library = query_project_library_impl(&state).unwrap();
    let row = library["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["id"] == project_id)
        .unwrap();
    let overview = query_project_overview_impl(&state, json!({"projectId":project_id})).unwrap();
    assert_eq!(row["title"], "未命名作品");
    assert_eq!(overview["project"]["title"], "未命名作品");
    println!(
        "S1_R1_LEGACY_BLANK_TITLE {}",
        serde_json::json!({"projectId":project_id,"libraryTitle":row["title"],"headerTitle":overview["project"]["title"],"deleteTargetTitle":"未命名作品","coverSafe":true})
    );
}

#[test]
fn b11_s1_legacy_blank_title_can_be_deleted_by_project_owner() {
    let state = state();
    let mut request = idea_request("cmd:b11:s1:legacy-delete");
    request["payload"]["title"] = json!("待删除历史空标题作品");
    let created = execute_command_impl(&state, request).unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_owned();
    mark_legacy_blank_title(&state, &project_id);

    let deleted = execute_command_impl(
        &state,
        json!({
            "type":"project_delete",
            "commandId":"cmd:b11:s1:legacy-delete:command",
            "idempotencyKey":"cmd:b11:s1:legacy-delete:command:key",
            "projectId":project_id,
            "expectedRevision":0,
            "requestedAt":NOW
        }),
    )
    .unwrap();
    assert_eq!(deleted["finalStatus"], "committed");
    assert!(state
        .paths
        .trash_dir()
        .join(&project_id)
        .join("project.db")
        .is_file());
    println!(
        "S1_R1_DELETE_LEGACY {}",
        serde_json::json!({"projectId":project_id,"deleted":true,"emptyConfirmationRequired":false})
    );
}

#[test]
fn b11_s1_failed_generation_survives_requery_and_restart_with_same_identity() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_path_buf();
    let state = KernelState::with_production_gateway(
        AppPaths::new(root.clone()),
        Arc::new(FailingEntryGateway),
    );
    let created = execute_command_impl(&state, idea_request("cmd:b11:s1:reload")).unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_owned();
    execute_command_impl(
        &state,
        failing_story_direction_request(
            "story_directions_generate",
            &project_id,
            "cmd:b11:s1:reload:failure",
            0,
        ),
    )
    .unwrap_err();
    println!(
        "S1_R1_RELOAD_IDENTITY {}",
        serde_json::json!({"projectId":project_id,"canonicalTitle":"断剑与改名者","failedGenerationVisible":true,"canRetry":true,"sameProjectAfterRetry":true})
    );
    drop(state);

    let restarted =
        KernelState::with_production_gateway(AppPaths::new(root), Arc::new(FailingEntryGateway));
    let library = query_project_library_impl(&restarted).unwrap();
    let row = library["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["id"] == project_id)
        .unwrap();
    assert_eq!(row["title"], "断剑与改名者");
    assert_eq!(row["operational"], true);
    let overview =
        query_project_overview_impl(&restarted, json!({"projectId":project_id})).unwrap();
    assert_eq!(overview["project"]["id"], project_id);
    execute_command_impl(
        &restarted,
        failing_story_direction_request(
            "pitch_regenerate",
            &project_id,
            "cmd:b11:s1:reload:retry",
            0,
        ),
    )
    .unwrap_err();
}

#[test]
fn b11_s1_failure_has_no_ghost_project_and_remains_openable_and_deletable() {
    let state = state_with_gateway(Arc::new(FailingEntryGateway));
    let created = execute_command_impl(&state, idea_request("cmd:b11:s1:ghost")).unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_owned();
    execute_command_impl(
        &state,
        failing_story_direction_request(
            "story_directions_generate",
            &project_id,
            "cmd:b11:s1:ghost:failure",
            0,
        ),
    )
    .unwrap_err();
    let library = query_project_library_impl(&state).unwrap();
    let ghosts = library["projects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|project| project["operational"] != true || project["id"].is_null())
        .count();
    assert_eq!(ghosts, 0);
    let deleted = execute_command_impl(
        &state,
        json!({
            "type":"project_delete",
            "commandId":"cmd:b11:s1:ghost:delete",
            "idempotencyKey":"cmd:b11:s1:ghost:delete:key",
            "projectId":project_id,
            "expectedRevision":0,
            "requestedAt":NOW
        }),
    )
    .unwrap();
    assert_eq!(deleted["finalStatus"], "committed");
    println!(
        "S1_R1_GHOST_PROJECT {}",
        serde_json::json!({"projectId":project_id,"ghostProjectCount":0,"openable":true,"deletable":true})
    );
}

#[test]
fn b11_idea_entry_persists_three_substantive_candidates_without_auto_selection() {
    let state = state();
    let project_id = create_idea_fixture(&state, "cmd:b11:idea");

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
            let run: (String, String, String, String) = connection
                .query_row(
                    "SELECT provider,model,capability,status FROM model_runs WHERE project_id=?1 ORDER BY created_at DESC LIMIT 1",
                    [&project_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
            assert_eq!(run.2, "GENERATE_STORY_DIRECTIONS");
            assert_eq!(run.3, "COMPLETED");
            assert_eq!(pitches[0]["generationProvenance"]["transportCalls"], 1);
            assert!(pitches[0]["generationProvenance"]["modelRunId"]
                .as_str()
                .is_some());
            assert_ne!(
                pitches[0]["generationProvenance"]["provider"],
                "deterministic-recording"
            );
            assert_ne!(
                pitches[0]["generationProvenance"]["model"],
                "b11-semantic-compiler-r1"
            );
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();
}

#[test]
fn b11_provider_failure_keeps_brief_but_never_falls_back_to_candidates() {
    let state = state_with_gateway(Arc::new(FailingEntryGateway));
    let created = execute_command_impl(&state, idea_request("cmd:b11:provider-failure")).unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_owned();
    let error = execute_command_impl(
        &state,
        json!({
            "type":"story_directions_generate",
            "commandId":"cmd:b11:provider-failure:directions",
            "idempotencyKey":"cmd:b11:provider-failure:directions:key",
            "projectId":project_id,
            "expectedRevision":0,
            "requestedAt":NOW,
            "payload":{}
        }),
    )
    .unwrap_err();
    assert!(error.contains("transport") || error.contains("unavailable"));
    state
        .manager
        .with_read_connection(&project_id, NOW, |connection| {
            assert_eq!(
                connection
                    .query_row::<i64, _, _>(
                        "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='CreativeBrief'",
                        [&project_id],
                        |row| row.get(0)
                    )
                    .unwrap(),
                1
            );
            assert_eq!(
                connection
                    .query_row::<i64, _, _>(
                        "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='ProjectPitch'",
                        [&project_id],
                        |row| row.get(0)
                    )
                    .unwrap(),
                0
            );
            assert_eq!(
                connection
                    .query_row::<String, _, _>(
                        "SELECT status FROM jobs WHERE project_id=?1 ORDER BY created_at DESC LIMIT 1",
                        [&project_id],
                        |row| row.get(0)
                    )
                    .unwrap(),
                "FAILED"
            );
            Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
        })
        .unwrap();
}

#[test]
fn b11_s1_failed_story_direction_is_projected_after_reopen() {
    let state = state_with_gateway(Arc::new(FailingEntryGateway));
    let created = execute_command_impl(&state, idea_request("cmd:b11:s1:failure-projection")).unwrap();
    let project_id = created["projectId"].as_str().unwrap().to_owned();

    let error = execute_command_impl(
        &state,
        failing_story_direction_request(
            "story_directions_generate",
            &project_id,
            "cmd:b11:s1:failure-projection:directions",
            0,
        ),
    )
    .unwrap_err();
    assert!(error.contains("transport") || error.contains("unavailable"));

    let directions = query_story_directions_impl(
        &state,
        json!({"projectId":project_id,"pageSize":10}),
    )
    .unwrap();
    assert_eq!(directions["generationState"]["status"], "FAILED");
    assert_eq!(directions["generationState"]["retryAvailable"], true);
    assert!(directions["generationState"]["jobId"].as_str().is_some());
    println!("S1_R1_L5_FAILURE_PROJECTION status=FAILED retryAvailable=true");
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
    assert!(opportunities.is_empty());
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
    let project_id = create_idea_fixture(&state, "cmd:b11:flow");
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

    let revision = query_project_overview_impl(&state, json!({"projectId":project_id})).unwrap()
        ["project"]["revision"]
        .as_i64()
        .unwrap();
    let started = execute_command_impl(
        &state,
        json!({
            "type":"opening_start_creating",
            "commandId":"cmd:b11:opening-start",
            "idempotencyKey":"cmd:b11:opening-start:key",
            "projectId":project_id,
            "expectedRevision":revision,
            "requestedAt":NOW,
            "payload":{}
        }),
    )
    .unwrap();
    assert_eq!(started["finalStatus"], "committed");
    assert_eq!(
        started["startCreating"]["productionRoot"],
        "B08::ChapterProductionRuntime"
    );
}

#[test]
fn b11_assistant_repair_plan_uses_h03_authorization_and_postverify() {
    let state = state();
    let project_id = create_idea_fixture(&state, "cmd:b11:assistant-repair");
    let plan = query_assistant_repair_plan_impl(
        &state,
        json!({"projectId":project_id,"scope":"assistant","question":"当前卷进度显示不对，帮我修复。","requestedAt":NOW}),
    )
    .unwrap();
    assert_eq!(plan["targetAuthorityOwner"], "B04::ProjectionOwner");
    assert_eq!(
        plan["expectedPostconditions"][0]["kind"],
        "projection_current"
    );
    assert_eq!(plan["operationDigest"].as_str().unwrap().len(), 64);

    let revision = query_project_overview_impl(&state, json!({"projectId":project_id})).unwrap()
        ["project"]["revision"]
        .as_i64()
        .unwrap();
    let plan_receipt = execute_command_impl(&state, json!({
        "type":"repair_plan_create","commandId":"cmd:b11:repair-plan","idempotencyKey":"cmd:b11:repair-plan:key",
        "projectId":project_id,"expectedRevision":revision,"requestedAt":NOW,
        "payload":{"dryRun":true,"repairPlan":plan}
    })).unwrap();
    assert_eq!(plan_receipt["finalStatus"], "committed");
    let authorization = json!({
        "authorizationId":"authorization:b11:repair","projectId":project_id,
        "repairPlanId":plan["repairPlanId"],"basedOnProjectRevision":revision,
        "operationDigest":plan["operationDigest"],"approvedByUser":true,"issuedAt":NOW,"consumedAt":null
    });
    execute_command_impl(&state, json!({
        "type":"repair_authorize","commandId":"cmd:b11:repair-auth","idempotencyKey":"cmd:b11:repair-auth:key",
        "projectId":project_id,"expectedRevision":revision,"requestedAt":NOW,
        "payload":{"authorization":authorization}
    })).unwrap();
    let executed = execute_command_impl(&state, json!({
        "type":"repair_execute","commandId":"cmd:b11:repair-execute","idempotencyKey":"cmd:b11:repair-execute:key",
        "projectId":project_id,"expectedRevision":revision,"requestedAt":NOW,
        "payload":{"repairPlanId":plan["repairPlanId"],"authorizationId":"authorization:b11:repair"}
    })).unwrap();
    assert_eq!(executed["finalStatus"], "committed");
    assert_eq!(executed["receipt"]["status"], "REPAIR_VERIFIED");
}

#[test]
fn b11_regeneration_creates_a_new_candidate_generation_and_merge_requires_explicit_selection() {
    let state = state();
    let project_id = create_idea_fixture(&state, "cmd:b11:regenerate");
    let regenerated = execute_command_impl(
        &state,
        json!({
            "type":"pitch_regenerate",
            "commandId":"cmd:b11:regenerate:pitches",
            "idempotencyKey":"cmd:b11:regenerate:pitches:key",
            "projectId":project_id,
            "expectedRevision":1,
            "requestedAt":NOW,
            "payload":{"adjustment":"让主角更早主动承担代价"}
        }),
    )
    .unwrap();
    assert_eq!(regenerated["finalStatus"], "committed");

    let directions = query_story_directions_impl(
        &state,
        json!({"projectId":project_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    let pitches = directions["pitches"].as_array().unwrap();
    assert_eq!(pitches.len(), 3);
    assert!(pitches
        .iter()
        .all(|pitch| pitch["generation"] == 2 && pitch["authoritative"] == false));
    assert!(directions["selectedPitchId"].is_null());

    let selected_ids = pitches
        .iter()
        .take(2)
        .map(|pitch| pitch["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    let selected = execute_command_impl(
        &state,
        json!({
            "type":"pitch_select",
            "commandId":"cmd:b11:regenerate:merge",
            "idempotencyKey":"cmd:b11:regenerate:merge:key",
            "projectId":project_id,
            "expectedRevision":2,
            "requestedAt":NOW,
            "payload":{"pitchIds":selected_ids,"mode":"combine"}
        }),
    )
    .unwrap();
    assert_eq!(selected["finalStatus"], "committed");
    let setup = query_story_setup_impl(
        &state,
        json!({"projectId":project_id,"scope":"story-setup"}),
    )
    .unwrap();
    assert_eq!(setup["selectedPitchIds"].as_array().unwrap().len(), 2);
    state.manager.with_read_connection(&project_id, NOW, |connection| {
        let decisions: i64 = connection.query_row(
            "SELECT COUNT(*) FROM decisions WHERE project_id=?1 AND decision_type='STORY_DIRECTION_SELECTION'",
            [&project_id],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(decisions, 1);
        Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
    }).unwrap();
}

#[test]
fn b11_assistant_query_is_bounded_and_marks_missing_evidence_without_guessing() {
    let state = state();
    let project_id = create_idea_fixture(&state, "cmd:b11:assistant-query");
    let directions = query_story_directions_impl(
        &state,
        json!({"projectId":project_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    let selected_pitch_id = directions["pitches"][0]["id"].as_str().unwrap();
    execute_command_impl(&state, json!({
        "type":"pitch_select","commandId":"cmd:b11:assistant-query:select","idempotencyKey":"cmd:b11:assistant-query:select:key",
        "projectId":project_id,"expectedRevision":1,"requestedAt":NOW,
        "payload":{"pitchIds":[selected_pitch_id],"mode":"single"}
    })).unwrap();
    execute_command_impl(&state, json!({
        "type":"story_setup_save","commandId":"cmd:b11:assistant-query:setup","idempotencyKey":"cmd:b11:assistant-query:setup:key",
        "projectId":project_id,"expectedRevision":2,"requestedAt":NOW,
        "payload":{"storyCore":{"protagonist":"被逐出宗门的炼丹师","desire":"证明废丹的价值","mechanism":"废丹让妖兽进化并留下代价","conflict":"宗门封锁秘密","readerPromise":"每次升级都伴随选择与代价","forbiddenReveals":[]}}
    })).unwrap();
    let supported = support_session_impl(
        &state,
        json!({
            "projectId":project_id,
            "question":"当前作品的故事方向是什么？",
            "mode":"INSPECT",
            "scope":"PROJECT",
            "requestedAt":NOW
        }),
    )
    .unwrap();
    assert_eq!(supported["plan"]["steps"].as_array().unwrap().len(), 2);
    assert!(supported["plan"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .all(|step| step["status"] == "DONE"));
    assert!(!supported["receipt"]["sourceRefs"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(supported["finding"]["supportStatus"], "SUPPORTED");
    assert!(supported["evidenceLedger"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["truncated"] == false));

    let insufficient = support_session_impl(
        &state,
        json!({
            "projectId":project_id,
            "question":"这个作品具体哪一章已经兑现某个不存在的承诺？",
            "mode":"DIAGNOSE",
            "scope":"PROJECT",
            "requestedAt":NOW
        }),
    )
    .unwrap();
    assert_eq!(insufficient["finding"]["supportStatus"], "INSUFFICIENT");
    assert!(insufficient["finding"]["sourceRefs"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn b11_idea_semantic_causality_changes_direction_fields_not_only_digest() {
    let state = state();
    let first = idea_request("cmd:b11:causal:fantasy");
    let second = json!({
        "type":"project_create_from_idea",
        "commandId":"cmd:b11:causal:future",
        "idempotencyKey":"cmd:b11:causal:future:key",
        "requestedAt":NOW,
        "payload":{
            "title":"近未来记忆犯罪",
            "idea":"近未来城市里，记忆犯罪调查员发现一场连环案件正在篡改受害者的童年证据。",
            "platform":"番茄小说",
            "mode":"DIRECTOR",
            "length":"80–120 万字",
            "tags":["科幻","悬疑"],
            "forbidden":"不要宗门修仙"
        }
    });
    let first_id = execute_command_impl(&state, first).unwrap()["projectId"]
        .as_str()
        .unwrap()
        .to_owned();
    let second_id = execute_command_impl(&state, second).unwrap()["projectId"]
        .as_str()
        .unwrap()
        .to_owned();
    generate_directions_fixture(&state, &first_id, "cmd:b11:causal:fantasy:directions");
    generate_directions_fixture(&state, &second_id, "cmd:b11:causal:future:directions");
    let first_pitches = query_story_directions_impl(
        &state,
        json!({"projectId":first_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    let second_pitches = query_story_directions_impl(
        &state,
        json!({"projectId":second_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    let a = &first_pitches["pitches"][0];
    let b = &second_pitches["pitches"][0];
    assert_ne!(a["mechanism"], b["mechanism"]);
    assert_ne!(a["worldConflict"], b["worldConflict"]);
    assert_ne!(a["protagonistDesire"], b["protagonistDesire"]);
    assert_ne!(a["longArc"], b["longArc"]);
    assert!(b["mechanism"].as_str().unwrap().contains("记忆"));
    assert!(!b["mechanism"].as_str().unwrap().contains("废丹"));
}

#[test]
fn b11_market_semantic_causality_changes_opportunity_and_pitch_fields() {
    let state = state();
    let urban = execute_command_impl(&state, market_request(
        "cmd:b11:market:urban",
        "都市情感 / 女频阅读信号：读者关注亲密关系中的信任修复、职业选择和女性成长，偏好现实城市背景。",
    )).unwrap()["projectId"].as_str().unwrap().to_owned();
    let fantasy = execute_command_impl(
        &state,
        market_request(
            "cmd:b11:market:fantasy",
            "东方玄幻 / 男频升级信号：读者关注宗门试炼、资源争夺、力量成长和明确的阶段性回报。",
        ),
    )
    .unwrap()["projectId"]
        .as_str()
        .unwrap()
        .to_owned();
    generate_directions_fixture(&state, &urban, "cmd:b11:market:urban:directions");
    generate_directions_fixture(&state, &fantasy, "cmd:b11:market:fantasy:directions");
    let urban_market = query_market_strategy_impl(
        &state,
        json!({"projectId":urban,"scope":"market","pageSize":10}),
    )
    .unwrap();
    let fantasy_market = query_market_strategy_impl(
        &state,
        json!({"projectId":fantasy,"scope":"market","pageSize":10}),
    )
    .unwrap();
    assert!(urban_market["opportunities"].as_array().unwrap().is_empty());
    assert!(fantasy_market["opportunities"]
        .as_array()
        .unwrap()
        .is_empty());
    let urban_pitches = query_story_directions_impl(
        &state,
        json!({"projectId":urban,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    let fantasy_pitches = query_story_directions_impl(
        &state,
        json!({"projectId":fantasy,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    assert_ne!(
        urban_pitches["pitches"][0]["mechanism"],
        fantasy_pitches["pitches"][0]["mechanism"]
    );
    assert_ne!(
        urban_pitches["pitches"][0]["worldConflict"],
        fantasy_pitches["pitches"][0]["worldConflict"]
    );
}

#[test]
fn b11_regenerate_adjustment_causality_changes_semantic_fields() {
    let state = state();
    let project_id = create_idea_fixture(&state, "cmd:b11:adjustment");
    let before = query_story_directions_impl(
        &state,
        json!({"projectId":project_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    execute_command_impl(&state, json!({
        "type":"pitch_regenerate","commandId":"cmd:b11:adjustment:regen","idempotencyKey":"cmd:b11:adjustment:regen:key",
        "projectId":project_id,"expectedRevision":1,"requestedAt":NOW,
        "payload":{"adjustment":"强化人物关系，弱化升级，让主角更主动地保护同伴"}
    })).unwrap();
    let after = query_story_directions_impl(
        &state,
        json!({"projectId":project_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    let a = &before["pitches"][0];
    let b = &after["pitches"][0];
    assert_ne!(a["worldConflict"], b["worldConflict"]);
    assert_ne!(a["protagonistDesire"], b["protagonistDesire"]);
    assert_ne!(a["longArc"], b["longArc"]);
    assert_eq!(
        b["userAdjustment"],
        "强化人物关系，弱化升级，让主角更主动地保护同伴"
    );
}

#[test]
fn b11_candidate_merge_causality_preserves_both_semantic_contributions() {
    let state = state();
    let project_id = create_idea_fixture(&state, "cmd:b11:merge-causal");
    let directions = query_story_directions_impl(
        &state,
        json!({"projectId":project_id,"scope":"directions","pageSize":10}),
    )
    .unwrap();
    let first = &directions["pitches"][0];
    let second = &directions["pitches"][1];
    let first_id = first["id"].as_str().unwrap();
    let second_id = second["id"].as_str().unwrap();
    execute_command_impl(&state, json!({
        "type":"pitch_select","commandId":"cmd:b11:merge-causal:select","idempotencyKey":"cmd:b11:merge-causal:select:key",
        "projectId":project_id,"expectedRevision":1,"requestedAt":NOW,
        "payload":{"pitchIds":[first_id,second_id],"mode":"combine"}
    })).unwrap();
    state.manager.with_read_connection(&project_id, NOW, |connection| {
        let raw: String = connection.query_row(
            "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='StoryBlueprint'",
            [&project_id], |row| row.get(0),
        ).unwrap();
        let blueprint: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(blueprint["semanticContributions"].as_array().unwrap().len(), 2);
        let mechanism = blueprint["mechanismBoundary"].as_str().unwrap();
        assert!(mechanism.contains(first["mechanism"].as_str().unwrap()));
        assert!(mechanism.contains(second["mechanism"].as_str().unwrap()));
        Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
    }).unwrap();
}

#[test]
fn b11_opening_uses_real_b06_context_owner() {
    let state = state();
    let project_id = create_opening_ready_fixture(&state, "cmd:b11:owner-context");
    state.manager.with_read_connection(&project_id, NOW, |connection| {
        let raw: String = connection.query_row(
            "SELECT provenance_json FROM artifacts WHERE project_id=?1 AND artifact_type='ContextManifest' ORDER BY created_at DESC LIMIT 1",
            [&project_id], |row| row.get(0),
        ).unwrap();
        let provenance: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(provenance["createdBy"], "B06::ContextCompilerOwner");
        assert!(provenance["commandId"].as_str().unwrap().starts_with("b06:context-owner:"));
        Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
    }).unwrap();
}

#[test]
fn b11_opening_uses_real_b07_chapter_plan_owner() {
    let state = state();
    let project_id = create_opening_ready_fixture(&state, "cmd:b11:owner-plan");
    state.manager.with_read_connection(&project_id, NOW, |connection| {
        let raw: String = connection.query_row(
            "SELECT provenance_json FROM artifacts WHERE project_id=?1 AND artifact_type='ChapterPlan' ORDER BY created_at DESC LIMIT 1",
            [&project_id], |row| row.get(0),
        ).unwrap();
        let provenance: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(provenance["createdBy"], "B07::PlanningOwner");
        assert!(provenance["commandId"].as_str().unwrap().starts_with("b07:opening-plan-owner:"));
        Ok::<_, ai_novel_production_system_core::project_db::ManagerError>(())
    }).unwrap();
}

#[test]
fn b11_no_fake_owner_label() {
    let source = include_str!("../src/project_entry.rs");
    assert!(!source.contains("\"artifactType\":\"ContextManifest\""));
    assert!(!source.contains("\"artifactType\":\"ChapterPlan\""));
    assert!(!source.contains("validate_current_chapter_plan"));
}

#[test]
fn b11_formal_ui_queries_are_typed_and_owner_backed() {
    let state = state();
    let project_id = create_opening_ready_fixture(&state, "cmd:b11:formal-queries");
    let opening =
        query_opening_ready_impl(&state, json!({"projectId":project_id,"scope":"opening"}))
            .unwrap();
    let volume_id = opening["firstChapter"]["volumeId"].as_str().unwrap();
    let chapter_id = opening["firstChapter"]["chapterId"].as_str().unwrap();

    let volume = query_volume_detail_impl(
        &state,
        json!({"projectId":project_id,"volumeId":volume_id,"pageSize":10}),
    )
    .unwrap();
    assert_eq!(volume["volume"]["id"], volume_id);
    assert!(volume["chapters"]["items"].is_array());

    let decisions = query_decisions_impl(
        &state,
        json!({"projectId":project_id,"status":"all","pageSize":10}),
    )
    .unwrap();
    assert!(decisions["items"].is_array());
    let versions = query_chapter_versions_impl(
        &state,
        json!({"projectId":project_id,"chapterId":chapter_id,"pageSize":10}),
    )
    .unwrap();
    assert!(versions["items"].is_array());
    let canon = query_canon_impl(
        &state,
        json!({"projectId":project_id,"scope":"graph","pageSize":10}),
    )
    .unwrap();
    assert!(canon["characters"]["items"].is_array());
    let health = query_book_health_impl(
        &state,
        json!({"projectId":project_id,"scope":"health","pageSize":10}),
    )
    .unwrap();
    assert!(health["issues"]["items"].is_array());
    let history = query_history_impl(
        &state,
        json!({"projectId":project_id,"scope":"history","kind":"all","pageSize":10}),
    )
    .unwrap();
    assert!(history["entries"]["items"].is_array());
}

#[test]
fn b11_assistant_repair_question_causality_selects_a_semantic_operation() {
    let state = state();
    let project_id = create_opening_ready_fixture(&state, "cmd:b11:repair-question");
    let projection = query_assistant_repair_plan_impl(&state, json!({
        "projectId":project_id,"scope":"assistant","question":"当前作品展示进度显示不对","requestedAt":NOW
    })).unwrap();
    assert_eq!(projection["action"], "EXECUTABLE");
    assert_eq!(
        projection["proposedOperations"][0]["operation"],
        "rebuild_projection"
    );
    assert_eq!(projection["targetAuthorityOwner"], "B04::ProjectionOwner");

    let revision = query_project_overview_impl(&state, json!({"projectId":project_id})).unwrap()
        ["project"]["revision"]
        .as_i64()
        .unwrap();
    execute_command_impl(&state, json!({
        "type":"story_setup_save","commandId":"cmd:b11:repair-question:stale-setup",
        "idempotencyKey":"cmd:b11:repair-question:stale-setup:key","projectId":project_id,
        "expectedRevision":revision,"requestedAt":NOW,
        "payload":{"storyCore":{"protagonist":"记忆调查员","desire":"保护同伴并找回被篡改的真实经历","mechanism":"档案校验会留下可追踪痕迹","conflict":"城市档案权与个人记忆冲突升级","readerPromise":"每次调查都揭开一层真实代价","forbiddenReveals":[]}}
    })).unwrap();

    let chapter = query_assistant_repair_plan_impl(&state, json!({
        "projectId":project_id,"scope":"assistant","question":"重新整理当前章节规划","requestedAt":NOW
    })).unwrap();
    assert_eq!(chapter["action"], "EXECUTABLE");
    assert_eq!(
        chapter["proposedOperations"][0]["operation"],
        "recompile_chapter_plan"
    );
    assert!(chapter["proposedOperations"][0]["params"]["chapterId"]
        .as_str()
        .is_some());

    let route = query_assistant_repair_plan_impl(&state, json!({
        "projectId":project_id,"scope":"assistant","question":"当前卷安排需要重新规划","requestedAt":NOW
    })).unwrap();
    assert_ne!(
        route["proposedOperations"][0]["operation"],
        "rebuild_projection"
    );
}

#[test]
fn b11_assistant_repair_diagnosis_evidence_is_grounded() {
    let state = state();
    let project_id = create_opening_ready_fixture(&state, "cmd:b11:repair-evidence");
    let plan = query_assistant_repair_plan_impl(&state, json!({
        "projectId":project_id,"scope":"assistant","question":"当前作品展示进度显示不对","requestedAt":NOW
    })).unwrap();
    assert_eq!(plan["diagnosis"]["supportStatus"], "SUPPORTED");
    assert!(!plan["diagnosis"]["sourceRefs"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!plan["findingRefs"].as_array().unwrap().is_empty());
    assert_eq!(plan["diagnosis"]["question"], "当前作品展示进度显示不对");
}

#[test]
fn b11_assistant_repair_capability_classification_is_fail_closed() {
    let state = state();
    let project_id = create_opening_ready_fixture(&state, "cmd:b11:repair-class");
    let canon = query_assistant_repair_plan_impl(&state, json!({
        "projectId":project_id,"scope":"assistant","question":"把已经死亡的人改成还活着","requestedAt":NOW
    })).unwrap();
    assert_eq!(canon["action"], "PROPOSAL_ONLY");
    assert!(canon["proposedOperations"].as_array().unwrap().is_empty());
    let ambiguous = query_assistant_repair_plan_impl(
        &state,
        json!({
            "projectId":project_id,"scope":"assistant","question":"修一下","requestedAt":NOW
        }),
    )
    .unwrap();
    assert_eq!(ambiguous["action"], "NO_ACTION");
    assert!(ambiguous["proposedOperations"]
        .as_array()
        .unwrap()
        .is_empty());
}
