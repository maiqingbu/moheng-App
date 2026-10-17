use super::*;
use std::sync::Mutex;

/// A configured-provider stand-in for the v2 craft rules. Plan N (0-based)
/// is `plans[N]` (the last repeats); render N returns `body_lengths[N]`
/// characters (the last repeats).
struct CraftGateway {
    plans: Vec<Value>,
    body_lengths: Vec<usize>,
    inputs: Mutex<Vec<(String, Value)>>,
}

impl CraftGateway {
    fn new(plans: Vec<Value>, body_lengths: &[usize]) -> Self {
        Self {
            plans,
            body_lengths: body_lengths.to_vec(),
            inputs: Mutex::new(Vec::new()),
        }
    }

    fn inputs(&self, capability: &str) -> Vec<Value> {
        self.inputs
            .lock()
            .unwrap()
            .iter()
            .filter(|(name, _)| name == capability)
            .map(|(_, input)| input.clone())
            .collect()
    }
}

fn scene(scene_type: &str, beats: usize, estimate: i64) -> Value {
    json!({
        "objective":"推进场景","sceneType":scene_type,"goal":"拿到线索","conflict":"守卫阻拦",
        "turn":"守卫放行","result":"拿到线索但被盯上",
        "beats":(0..beats).map(|n| format!("节拍{n}")).collect::<Vec<_>>(),
        "estimatedCharacters":estimate
    })
}

fn good_plan() -> Value {
    json!({"chapterRole":"普通","sceneExecution":[scene("调查", 3, 900), scene("战斗", 3, 900), scene("对话为主", 2, 700)]})
}

impl ProductionModelGateway for CraftGateway {
    fn enforces_chapter_length(&self) -> bool {
        true
    }

    fn resolve_render_profile(
        &self,
    ) -> Result<ai_novel_production_system_core::production::ResolvedProviderProfile, String> {
        let mut profile = self.resolve_profile()?;
        profile.model = "writer-model".into();
        Ok(profile)
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let plans = self.inputs("PLAN_STRATEGY").len();
        let renders = self.inputs("RENDER_CHAPTER").len();
        self.inputs
            .lock()
            .unwrap()
            .push((capability.to_string(), input.clone()));
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "PLAN_STRATEGY" {
            let plan = self
                .plans
                .get(plans)
                .unwrap_or_else(|| self.plans.last().unwrap());
            for (key, value) in plan.as_object().unwrap() {
                response.output[key] = value.clone();
            }
        }
        if capability == "RENDER_CHAPTER" {
            let length = *self
                .body_lengths
                .get(renders)
                .unwrap_or_else(|| self.body_lengths.last().unwrap());
            let body = response.output["body"].as_str().unwrap().to_string();
            let filler = length.saturating_sub(body.chars().count() + 1);
            response.output["body"] = json!(format!("{body}\n{}", "雨".repeat(filler)));
        }
        Ok(response)
    }
}

fn produce(
    gateway: Arc<CraftGateway>,
    project_id: &str,
) -> (
    tempfile::TempDir,
    KernelState,
    String,
    Result<Value, String>,
) {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway);
    let connection = state.manager.create(project_id, "Craft v2", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    drop(connection);
    let result = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":format!("cmd:{project_id}"),
            "idempotencyKey":format!("key:{project_id}"),"projectId":project_id,
            "expectedRevision":0,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]}
        }),
    );
    (temp, state, chapter_id, result)
}

fn skill_ids(guidance: &Value) -> Vec<String> {
    guidance["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|skill| skill["skillId"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn craft_v2_guidance_reaches_the_planner_and_the_writer() {
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[2500]));
    let (_temp, _state, _chapter_id, result) = produce(gateway.clone(), "p-craft-guidance");
    result.unwrap();
    let plan_input = &gateway.inputs("PLAN_STRATEGY")[0];
    assert_eq!(plan_input["chapterNumber"], 1);
    assert_eq!(plan_input["recentChapters"], json!([]));
    assert_eq!(
        plan_input["chapterRoleOptions"].as_array().unwrap().len(),
        7
    );
    let plan_skills = skill_ids(&plan_input["craftGuidance"]);
    assert!(plan_skills.contains(&"S26".to_string()), "{plan_skills:?}");
    assert!(plan_skills.contains(&"S13".to_string()), "{plan_skills:?}");
    let render_input = &gateway.inputs("RENDER_CHAPTER")[0];
    assert!(render_input.get("creativeEnrichment").is_none());
    assert_eq!(render_input["craftGuidance"]["stage"], "render");
    assert_eq!(
        render_input["beatTargets"][0]["beats"][0]["targetCharacters"],
        300
    );
    let render_skills = skill_ids(&render_input["craftGuidance"]);
    assert!(
        render_skills.contains(&"S27".to_string()),
        "{render_skills:?}"
    );
    assert!(
        render_skills.contains(&"S30".to_string()),
        "{render_skills:?}"
    );
    assert!(
        render_skills.contains(&"S28".to_string()),
        "{render_skills:?}"
    );
}

#[test]
fn craft_v2_plan_without_beats_is_replanned_once_with_the_reason() {
    let thin = json!({"chapterRole":"普通","sceneExecution":[
        {"objective":"推进","estimatedCharacters":900},{"objective":"推进","estimatedCharacters":900},{"objective":"推进","estimatedCharacters":700}
    ]});
    let gateway = Arc::new(CraftGateway::new(vec![thin, good_plan()], &[2500]));
    let (_temp, _state, _chapter_id, result) = produce(gateway.clone(), "p-craft-replan");
    result.unwrap();
    let plans = gateway.inputs("PLAN_STRATEGY");
    assert_eq!(plans.len(), 2);
    assert!(plans[0].get("planFeedback").is_none());
    assert_eq!(
        plans[1]["planFeedback"]["rejectedFor"],
        "B08_PLAN_BEATS_INVALID"
    );
    assert!(plans[1]["planFeedback"]["instruction"]
        .as_str()
        .unwrap()
        .contains("节拍"));
}

#[test]
fn craft_v2_rewrite_regenerates_from_the_plan_with_the_gap_instead_of_the_short_draft() {
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[1500, 2600]));
    let (_temp, _state, _chapter_id, result) = produce(gateway.clone(), "p-craft-rewrite");
    result.unwrap();
    let renders = gateway.inputs("RENDER_CHAPTER");
    assert_eq!(renders.len(), 2);
    assert!(renders[1].get("previousBody").is_none());
    assert_eq!(renders[1]["lengthFeedback"]["previousCharacterCount"], 1500);
    assert_eq!(renders[1]["lengthFeedback"]["shortBy"], 500);
    assert_eq!(
        renders[1]["lengthFeedback"]["scenes"][0]["beats"][0]["targetCharacters"],
        300
    );
}

#[test]
fn craft_v2_retry_rewrites_a_reused_short_draft_from_the_plan() {
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[900, 900, 900, 2500]));
    let project_id = "p-craft-retry";
    let (_temp, state, chapter_id, result) = produce(gateway.clone(), project_id);
    assert!(result
        .unwrap_err()
        .starts_with("B08_RENDER_LENGTH_OUT_OF_RANGE"));
    let connection = state.manager.open(project_id, "Craft v2", NOW).unwrap();
    let failed_job: String = connection
        .query_row(
            "SELECT job_id FROM jobs WHERE project_id=?1 AND status='FAILED'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    let revision = state.manager.overview(project_id).unwrap().1;
    drop(connection);
    execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:craft:retry","idempotencyKey":"key:craft:retry",
        "projectId":project_id,"expectedRevision":revision,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"],"retryOfJobId":failed_job}
    }))
    .unwrap();
    // The reused short draft goes straight to a rewrite that carries only the
    // gap, never the draft itself.
    let renders = gateway.inputs("RENDER_CHAPTER");
    assert_eq!(renders.len(), 4);
    assert_eq!(renders[3]["lengthFeedback"]["previousCharacterCount"], 900);
    assert!(renders[3].get("previousBody").is_none());
}

#[test]
fn craft_v2_long_form_plan_compile_gets_guidance_and_replans_a_rejected_plan() {
    let temp = tempfile::tempdir().unwrap();
    let no_role =
        json!({"chapterRole":null,"sceneExecution":good_plan()["sceneExecution"].clone()});
    let gateway = Arc::new(CraftGateway::new(vec![no_role, good_plan()], &[2500]));
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let project_id = "p-craft-long";
    let connection = state.manager.create(project_id, "Craft long", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate_long_form(&connection, project_id);
    let volume_id =
        StableIdentityFactory::from_command(StableIdKind::Volume, "b08-volume").unwrap();
    connection
        .execute(
            "UPDATE volumes SET chapter_end=4 WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id],
        )
        .unwrap();
    drop(connection);
    let result = execute_command_impl(
        &state,
        json!({
            "type":"planning_compile_chapter_plan","commandId":"cmd:craft:long","idempotencyKey":"key:craft:long",
            "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"scopeSelection":{"taskRefs":["task:actual"],"responsibilityRefs":[],"reservationRefs":[],"entityRefs":["entity:actual:linzhou"]}}
        }),
    );
    assert_eq!(result.unwrap()["finalStatus"], "committed");
    let plans = gateway.inputs("PLAN_STRATEGY");
    assert_eq!(plans.len(), 2);
    assert!(plans[0]["chapterNumber"].is_number());
    assert!(skill_ids(&plans[0]["craftGuidance"]).contains(&"S25".to_string()));
    assert_eq!(
        plans[1]["planFeedback"]["rejectedFor"],
        "B08_PLAN_ROLE_INVALID"
    );
}

#[test]
fn craft_v2_manual_quota_reset_lets_an_exhausted_chapter_be_retried() {
    use ai_novel_production_system_core::repositories::production::ProductionRepository;
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[900, 900, 900, 2500]));
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let project_id = "p-craft-quota";
    let connection = state
        .manager
        .create(project_id, "Craft quota", NOW)
        .unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    connection
        .execute(
            "UPDATE chapter_production_budgets SET hard_max_model_runs=3 WHERE project_id=?1 AND chapter_id=?2",
            [project_id, chapter_id.as_str()],
        )
        .unwrap();
    drop(connection);
    let start = |command: &str, retry_of: Option<&str>, revision: i64| {
        let mut payload = json!({"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"]});
        if let Some(job_id) = retry_of {
            payload["retryOfJobId"] = json!(job_id);
        }
        execute_command_impl(
            &state,
            json!({
                "type":"production_start","commandId":format!("cmd:{command}"),"idempotencyKey":format!("key:{command}"),
                "projectId":project_id,"expectedRevision":revision,"requestedAt":NOW,"payload":payload
            }),
        )
    };
    assert!(start("craft:quota:first", None, 0).is_err());
    let connection = state.manager.open(project_id, "Craft quota", NOW).unwrap();
    assert_eq!(
        ProductionRepository::chapter_quota(&connection, project_id, &chapter_id).unwrap(),
        Some(json!({"used":3,"limit":3,"allowance":12}))
    );
    let center = ProductionRepository::center_projection(&connection, project_id).unwrap();
    assert_eq!(center["nextChapter"]["id"], chapter_id.as_str());
    assert_eq!(
        center["nextChapter"]["quota"],
        json!({"used":3,"limit":3,"allowance":12})
    );
    drop(connection);

    let revision = state.manager.overview(project_id).unwrap().1;
    execute_command_impl(&state, json!({
        "type":"production_reset_chapter_quota","commandId":"cmd:craft:quota:reset","idempotencyKey":"key:craft:quota:reset",
        "projectId":project_id,"expectedRevision":revision,"requestedAt":NOW,"payload":{"chapterId":chapter_id}
    }))
    .unwrap();
    let connection = state.manager.open(project_id, "Craft quota", NOW).unwrap();
    assert_eq!(
        ProductionRepository::chapter_quota(&connection, project_id, &chapter_id).unwrap(),
        Some(json!({"used":3,"limit":15,"allowance":12}))
    );
    let decisions: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM decisions WHERE project_id=?1 AND decision_type='CHAPTER_QUOTA_RESET'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(decisions, 1);
    let blocked_job: String = connection
        .query_row(
            "SELECT job_id FROM jobs WHERE project_id=?1 AND chapter_id=?2 ORDER BY created_at DESC, job_id DESC LIMIT 1",
            [project_id, chapter_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);

    let revision = state.manager.overview(project_id).unwrap().1;
    let result = start("craft:quota:retry", Some(&blocked_job), revision).unwrap();
    assert_eq!(result["finalStatus"], "committed");
}

#[test]
fn craft_v2_only_the_writer_uses_the_writing_model() {
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[2500]));
    let project_id = "p-craft-writer-model";
    let (_temp, state, _chapter_id, result) = produce(gateway, project_id);
    result.unwrap();
    let connection = state.manager.open(project_id, "Craft v2", NOW).unwrap();
    let models = |capability: &str| {
        connection
            .prepare("SELECT DISTINCT model FROM model_runs WHERE project_id=?1 AND capability=?2")
            .unwrap()
            .query_map([project_id, capability], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(models("RENDER_CHAPTER"), vec!["writer-model"]);
    assert!(!models("PLAN_STRATEGY").contains(&"writer-model".to_string()));
    assert!(!models("AUDIT_EXTRACT").contains(&"writer-model".to_string()));
}
