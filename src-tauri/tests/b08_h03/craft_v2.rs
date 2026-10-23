use super::*;
use std::sync::Mutex;

/// A configured-provider stand-in for the v2 craft rules. Plan N (0-based)
/// is `plans[N]` (the last repeats); writer call N — the chapter, then its
/// length adjustment — returns `body_lengths[N]` characters (the last
/// repeats). Chapters are written whole (owner decision 2026-10-23).
struct CraftGateway {
    plans: Vec<Value>,
    body_lengths: Vec<usize>,
    /// Writes half-width punctuation into each part, as real writers do.
    punctuation_sample: bool,
    /// Times writer call N repeats `MOTIF` (missing calls repeat it none).
    motifs: Vec<usize>,
    /// The first audit reports an unresolved hard problem.
    fail_first_audit: bool,
    /// The review flags the last 12 characters of the chapter as wordy, and
    /// the rewrite replaces them with this text.
    rewrite_ending: Option<String>,
    /// Text writer call N adds to its body (missing calls add none).
    inject: Vec<&'static str>,
    inputs: Mutex<Vec<(String, Value)>>,
}

impl CraftGateway {
    fn new(plans: Vec<Value>, body_lengths: &[usize]) -> Self {
        Self {
            plans,
            body_lengths: body_lengths.to_vec(),
            punctuation_sample: false,
            motifs: Vec::new(),
            fail_first_audit: false,
            rewrite_ending: None,
            inject: Vec::new(),
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

/// A recurring image a writer is known to reuse word for word.
const MOTIF: &str = "白裙下摆扬起，褶皱里藏着三颗蓝莓籽。";

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

    fn resolve_step_profile(
        &self,
        capability: &str,
    ) -> Result<ai_novel_production_system_core::production::ResolvedProviderProfile, String> {
        let mut profile = self.resolve_profile()?;
        match capability {
            "RENDER_CHAPTER" => profile.model = "writer-model".into(),
            "AUDIT_EXTRACT" => profile.model = "audit-model".into(),
            _ => {}
        }
        Ok(profile)
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let plans = self.inputs("PLAN_STRATEGY").len();
        let renders =
            self.inputs("RENDER_CHAPTER").len() + self.inputs("ADJUST_CHAPTER_LENGTH").len();
        self.inputs
            .lock()
            .unwrap()
            .push((capability.to_string(), input.clone()));
        let audits = self.inputs("AUDIT_EXTRACT").len();
        // A length adjustment returns a whole chapter, like the writer.
        let writes = matches!(capability, "RENDER_CHAPTER" | "ADJUST_CHAPTER_LENGTH");
        let mut response =
            RecordingGateway.invoke(if writes { "RENDER_CHAPTER" } else { capability }, input)?;
        if capability == "AUDIT_EXTRACT" && self.fail_first_audit && audits == 1 {
            response.output["unresolvedHard"] = json!(true);
            response.output["auditCompleteness"] = json!("UNRESOLVED_HARD");
        }
        match (capability, &self.rewrite_ending) {
            ("REVIEW_CHAPTER", Some(_)) => {
                let body = input["body"].as_str().unwrap().chars().collect::<Vec<_>>();
                let ending = body[body.len() - 12..].iter().collect::<String>();
                response.output = json!({"findings":[{"code":"prose_wordy","severity":"HIGH",
                    "quote":ending,"problem":"收尾拖沓","suggestion":"收紧"}]});
            }
            ("REWRITE_PASSAGES", Some(replacement)) => {
                response.output = json!({"edits":[{"quote":input["findings"][0]["quote"],"replacement":replacement}]});
            }
            _ => {}
        }
        if capability == "PLAN_STRATEGY" {
            let plan = self
                .plans
                .get(plans)
                .unwrap_or_else(|| self.plans.last().unwrap());
            for (key, value) in plan.as_object().unwrap() {
                response.output[key] = value.clone();
            }
        }
        if writes && self.punctuation_sample {
            // Half-width punctuation the writer is known to produce.
            let mut length = *self
                .body_lengths
                .get(renders)
                .unwrap_or_else(|| self.body_lengths.last().unwrap());
            let opening = response.output["body"].as_str().unwrap().to_string();
            length = length.saturating_sub(opening.chars().count() + 14);
            response.output["body"] = json!(format!(
                "{opening}\n备注:S.Y.已归档,好...{}",
                varied_prose(length)
            ));
            return Ok(response);
        }
        if writes {
            let length = *self
                .body_lengths
                .get(renders)
                .unwrap_or_else(|| self.body_lengths.last().unwrap());
            let motif = format!(
                "{}{}",
                MOTIF.repeat(self.motifs.get(renders).copied().unwrap_or(0)),
                self.inject.get(renders).copied().unwrap_or("")
            );
            let body = format!("{}{motif}", response.output["body"].as_str().unwrap());
            let filler = length.saturating_sub(body.chars().count() + 1);
            response.output["body"] = json!(format!("{body}\n{}", varied_prose(filler)));
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
    assert!(render_input.get("beatTargets").is_none());
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
fn craft_v2_plan_without_a_role_is_replanned_once_with_the_reason() {
    let no_role = json!({"sceneExecution":good_plan()["sceneExecution"].clone()});
    let gateway = Arc::new(CraftGateway::new(vec![no_role, good_plan()], &[2500]));
    let (_temp, _state, _chapter_id, result) = produce(gateway.clone(), "p-craft-replan");
    result.unwrap();
    let plans = gateway.inputs("PLAN_STRATEGY");
    assert_eq!(plans.len(), 2);
    assert!(plans[0].get("planFeedback").is_none());
    assert_eq!(
        plans[1]["planFeedback"]["rejectedFor"],
        "B08_PLAN_ROLE_INVALID"
    );
}

#[test]
fn craft_v2_a_chapter_is_written_once_and_sized_by_one_adjustment() {
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[1200, 2500]));
    let project_id = "p-craft-adjust";
    let (_temp, state, chapter_id, result) = produce(gateway.clone(), project_id);
    result.unwrap();
    let renders = gateway.inputs("RENDER_CHAPTER");
    assert_eq!(renders.len(), 1);
    assert!(renders[0].get("part").is_none() && renders[0].get("beatTargets").is_none());
    let adjust = gateway.inputs("ADJUST_CHAPTER_LENGTH");
    assert_eq!(adjust.len(), 1);
    assert_eq!(adjust[0]["direction"], "EXPAND");
    assert_eq!(adjust[0]["shortBy"], 2000 - 1200);
    assert_eq!(committed_word_count(&state, project_id, &chapter_id), 2500);
}

#[test]
fn craft_v2_a_chapter_in_range_needs_no_adjustment_and_one_still_outside_is_not_committed() {
    let in_range = Arc::new(CraftGateway::new(vec![good_plan()], &[2500]));
    let (_temp, _state, _chapter_id, result) = produce(in_range.clone(), "p-craft-in-range");
    result.unwrap();
    assert!(in_range.inputs("ADJUST_CHAPTER_LENGTH").is_empty());

    let too_long = Arc::new(CraftGateway::new(vec![good_plan()], &[3600, 3400]));
    let (_temp, _state, _chapter_id, result) = produce(too_long.clone(), "p-craft-too-long");
    assert!(result
        .unwrap_err()
        .starts_with("B08_RENDER_LENGTH_OUT_OF_RANGE"));
    assert_eq!(
        too_long.inputs("ADJUST_CHAPTER_LENGTH")[0]["direction"],
        "COMPRESS"
    );
}

fn committed_word_count(state: &KernelState, project_id: &str, chapter_id: &str) -> i64 {
    state
        .manager
        .open(project_id, "Craft v2", NOW)
        .unwrap()
        .query_row(
            "SELECT CAST(json_extract(p.payload_json,'$.wordCount') AS INTEGER) FROM chapters c JOIN artifact_payloads p ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version WHERE c.project_id=?1 AND c.chapter_id=?2",
            [project_id, chapter_id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn craft_v2_retry_writes_the_chapter_again() {
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[600, 600, 2500]));
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
    connection
        .execute(
            "UPDATE chapter_production_budgets SET hard_max_model_runs=20 WHERE project_id=?1",
            [project_id],
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
    // The configured writer never reuses sized prose: the retry writes anew.
    assert_eq!(gateway.inputs("RENDER_CHAPTER").len(), 2);
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
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[900, 900, 2500]));
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
        Some(json!({"used":3,"limit":3,"allowance":16}))
    );
    let center = ProductionRepository::center_projection(&connection, project_id).unwrap();
    assert_eq!(center["nextChapter"]["id"], chapter_id.as_str());
    assert_eq!(
        center["nextChapter"]["quota"],
        json!({"used":3,"limit":3,"allowance":16})
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
        Some(json!({"used":3,"limit":19,"allowance":16}))
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
fn craft_v2_writing_and_audit_use_their_step_models() {
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
    assert_eq!(models("AUDIT_EXTRACT"), vec!["audit-model"]);
    let plan_models = models("PLAN_STRATEGY");
    assert!(!plan_models.contains(&"writer-model".to_string()));
    assert!(!plan_models.contains(&"audit-model".to_string()));
}

#[test]
fn craft_v2_a_chapter_that_uses_exactly_its_run_limit_still_commits() {
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[2500]));
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway);
    let project_id = "p-craft-exact-limit";
    let connection = state
        .manager
        .create(project_id, "Craft limit", NOW)
        .unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    // Plan, the chapter, the prose review and the audit: four runs (a
    // chapter in range needs no adjustment, a clean review no rewrite).
    connection
        .execute(
            "UPDATE chapter_production_budgets SET hard_max_model_runs=4 WHERE project_id=?1",
            [project_id],
        )
        .unwrap();
    drop(connection);
    let result = execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":"cmd:craft:exact","idempotencyKey":"key:craft:exact",
            "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","skillsEnabled":false,"entityIds":["entity:actual:linzhou"]}
        }),
    );
    assert_eq!(result.unwrap()["finalStatus"], "committed");
}

#[test]
fn craft_v2_committed_prose_uses_chinese_punctuation() {
    let gateway = Arc::new(CraftGateway {
        punctuation_sample: true,
        ..CraftGateway::new(vec![good_plan()], &[2500])
    });
    let project_id = "p-craft-typography";
    let (_temp, state, chapter_id, result) = produce(gateway, project_id);
    result.unwrap();
    let connection = state.manager.open(project_id, "Craft v2", NOW).unwrap();
    let body: String = connection
        .query_row(
            "SELECT json_extract(p.payload_json,'$.body') FROM chapters c JOIN artifact_payloads p ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version WHERE c.project_id=?1 AND c.chapter_id=?2",
            [project_id, chapter_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(body.contains("备注：S．Y．已归档，好……"));
    assert!(!body.contains(':') && !body.contains(','));
}

#[test]
fn craft_v2_a_retry_with_new_prose_audits_the_new_prose() {
    let gateway = Arc::new(CraftGateway {
        fail_first_audit: true,
        ..CraftGateway::new(vec![good_plan()], &[2500])
    });
    let project_id = "p-craft-retry-audit";
    let (_temp, state, chapter_id, result) = produce(gateway.clone(), project_id);
    assert_eq!(result.unwrap_err(), "B08_AUDIT_UNRESOLVED_HARD");
    let connection = state.manager.open(project_id, "Craft v2", NOW).unwrap();
    let failed_job: String = connection
        .query_row(
            "SELECT job_id FROM jobs WHERE project_id=?1 AND status='FAILED'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute(
            "UPDATE chapter_production_budgets SET hard_max_model_runs=20 WHERE project_id=?1",
            [project_id],
        )
        .unwrap();
    let revision = state.manager.overview(project_id).unwrap().1;
    drop(connection);
    execute_command_impl(&state, json!({
        "type":"production_start","commandId":"cmd:craft:retry-audit","idempotencyKey":"key:craft:retry-audit",
        "projectId":project_id,"expectedRevision":revision,"requestedAt":NOW,
        "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","entityIds":["entity:actual:linzhou"],"retryOfJobId":failed_job}
    }))
    .unwrap();
    // The retry wrote the chapter again, so its prose gets its own audit.
    assert_eq!(gateway.inputs("RENDER_CHAPTER").len(), 2);
    assert_eq!(gateway.inputs("AUDIT_EXTRACT").len(), 2);
}

#[test]
fn craft_v2_long_form_plan_with_too_short_a_horizon_is_replanned() {
    let temp = tempfile::tempdir().unwrap();
    // A real chapter 7 plan (2026-10-19) previewed two chapters where the
    // volume had room for 29.
    let mut short_horizon = good_plan();
    short_horizon["horizonPreview"] = json!([
        {"objective":"潜入外围","tentative":true},
        {"objective":"被迫交手","tentative":true}
    ]);
    let gateway = Arc::new(CraftGateway::new(vec![short_horizon, good_plan()], &[2500]));
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway.clone());
    let project_id = "p-craft-long-horizon";
    let connection = state.manager.create(project_id, "Craft long", NOW).unwrap();
    let chapter_id = seed_chapter_and_gate_long_form(&connection, project_id);
    let volume_id =
        StableIdentityFactory::from_command(StableIdKind::Volume, "b08-volume").unwrap();
    connection
        .execute(
            "UPDATE volumes SET chapter_end=10 WHERE project_id=?1 AND volume_id=?2",
            libsql_rusqlite::params![project_id, volume_id],
        )
        .unwrap();
    drop(connection);
    let result = execute_command_impl(
        &state,
        json!({
            "type":"planning_compile_chapter_plan","commandId":"cmd:craft:long-horizon","idempotencyKey":"key:craft:long-horizon",
            "projectId":project_id,"expectedRevision":0,"requestedAt":NOW,
            "payload":{"chapterId":chapter_id,"scopeSelection":{"taskRefs":["task:actual"],"responsibilityRefs":[],"reservationRefs":[],"entityRefs":["entity:actual:linzhou"]}}
        }),
    );
    assert_eq!(result.unwrap()["finalStatus"], "committed");
    let plans = gateway.inputs("PLAN_STRATEGY");
    assert_eq!(plans.len(), 2);
    assert_eq!(
        plans[1]["planFeedback"]["rejectedFor"],
        "ROLLING_HORIZON_CAPACITY_INVALID"
    );
}

#[test]
fn craft_v2_receipt_records_the_skills_each_step_received() {
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[2500]));
    let project_id = "p-craft-skill-receipt";
    let (_temp, state, _chapter_id, result) = produce(gateway.clone(), project_id);
    result.unwrap();
    let receipt = receipt_craft(&state, project_id);
    let ids = |value: &Value| {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&receipt["chapterPlan"]),
        skill_ids(&gateway.inputs("PLAN_STRATEGY")[0]["craftGuidance"])
    );
    assert_eq!(
        ids(&receipt["render"]),
        skill_ids(&gateway.inputs("RENDER_CHAPTER")[0]["craftGuidance"])
    );
    assert_eq!(receipt["skillsVersion"], "2.0.0");
}

fn committed_body(state: &KernelState, project_id: &str) -> String {
    state
        .manager
        .open(project_id, "Craft v2", NOW)
        .unwrap()
        .query_row(
            "SELECT json_extract(p.payload_json,'$.body') FROM chapters c JOIN artifact_payloads p ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version WHERE c.project_id=?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap()
}

fn receipt_craft(state: &KernelState, project_id: &str) -> Value {
    let raw: String = state
        .manager
        .open(project_id, "Craft v2", NOW)
        .unwrap()
        .query_row(
            "SELECT json_extract(p.payload_json,'$.craftSkills') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ProductionReceipt'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn craft_v2_review_findings_are_rewritten_in_place_with_the_rewrite_skills() {
    let gateway = Arc::new(CraftGateway {
        rewrite_ending: Some("他收好钥匙，转身离开。".into()),
        ..CraftGateway::new(vec![good_plan()], &[2500])
    });
    let project_id = "p-craft-review";
    let (_temp, state, _chapter_id, result) = produce(gateway.clone(), project_id);
    result.unwrap();
    let reviews = gateway.inputs("REVIEW_CHAPTER");
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0]["craftGuidance"]["stage"], "review");
    assert!(skill_ids(&reviews[0]["craftGuidance"]).contains(&"S37".to_string()));
    assert!(reviews[0]["findingCodes"]
        .as_array()
        .unwrap()
        .contains(&json!("prose_wordy")));
    let rewrites = gateway.inputs("REWRITE_PASSAGES");
    assert_eq!(rewrites.len(), 1);
    let rewrite_skills = skill_ids(&rewrites[0]["craftGuidance"]);
    for expected in ["S40", "S46", "S32"] {
        assert!(
            rewrite_skills.contains(&expected.to_string()),
            "{rewrite_skills:?}"
        );
    }
    // The audit reads the rewritten chapter, which is what commits.
    let audited = gateway.inputs("AUDIT_EXTRACT")[0]["body"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(audited.ends_with("他收好钥匙，转身离开。"));
    assert_eq!(committed_body(&state, project_id), audited);
    let craft = receipt_craft(&state, project_id);
    assert!(craft["review"].as_array().unwrap().contains(&json!("S37")));
    assert!(craft["rewrite"].as_array().unwrap().contains(&json!("S32")));
    assert_eq!(craft["proseReview"]["editsApplied"], 1);
}

#[test]
fn craft_v2_a_chapter_without_review_findings_is_not_rewritten() {
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[2500]));
    let project_id = "p-craft-review-clean";
    let (_temp, state, _chapter_id, result) = produce(gateway.clone(), project_id);
    result.unwrap();
    assert_eq!(gateway.inputs("REVIEW_CHAPTER").len(), 1);
    assert!(gateway.inputs("REWRITE_PASSAGES").is_empty());
    assert_eq!(
        receipt_craft(&state, project_id)["proseReview"]["findings"],
        0
    );
}

#[test]
fn craft_v2_a_rewrite_that_leaves_the_length_range_is_discarded() {
    let gateway = Arc::new(CraftGateway {
        rewrite_ending: Some("长".repeat(2000)),
        ..CraftGateway::new(vec![good_plan()], &[2500])
    });
    let project_id = "p-craft-review-long";
    let (_temp, state, _chapter_id, result) = produce(gateway.clone(), project_id);
    result.unwrap();
    assert_eq!(gateway.inputs("REWRITE_PASSAGES").len(), 1);
    assert!(!committed_body(&state, project_id).contains("长长长"));
    let craft = receipt_craft(&state, project_id);
    assert_eq!(craft["proseReview"]["editsApplied"], 0);
    assert_eq!(craft["proseReview"]["discarded"], "LENGTH_OUT_OF_RANGE");
}

#[test]
fn craft_v2_a_phrase_used_a_third_time_goes_to_the_rewrite() {
    // The chapter uses the image three times, as a real code label did
    // on 2026-10-19.
    let gateway = Arc::new(CraftGateway {
        motifs: vec![3],
        ..CraftGateway::new(vec![good_plan()], &[2500])
    });
    let project_id = "p-craft-review-repeat";
    let (_temp, state, _chapter_id, result) = produce(gateway.clone(), project_id);
    result.unwrap();
    let rewrites = gateway.inputs("REWRITE_PASSAGES");
    assert_eq!(rewrites.len(), 1);
    assert!(rewrites[0]["repeatedPhrases"][0]
        .as_str()
        .unwrap()
        .contains("蓝莓籽"));
    assert!(
        receipt_craft(&state, project_id)["proseReview"]["repeatedPhrases"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn craft_v2_rule_findings_go_to_the_rewrite_with_the_review() {
    let gateway = Arc::new(CraftGateway {
        inject: vec!["他喉结滚动。她没说话。他的喉结又动了一下。门外喉结般的石钮微响。"],
        ..CraftGateway::new(vec![good_plan()], &[2500])
    });
    let project_id = "p-craft-rule-findings";
    let (_temp, state, _chapter_id, result) = produce(gateway.clone(), project_id);
    result.unwrap();
    let rewrites = gateway.inputs("REWRITE_PASSAGES");
    assert_eq!(rewrites.len(), 1);
    let findings = rewrites[0]["findings"].as_array().unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding["code"] == "word_repetition" && finding["source"] == "rule"));
    let review = &receipt_craft(&state, project_id)["proseReview"];
    assert!(review["ruleCodes"]
        .as_array()
        .unwrap()
        .contains(&json!("word_repetition")));
}
