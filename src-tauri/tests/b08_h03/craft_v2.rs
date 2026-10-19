use super::*;
use std::sync::Mutex;

/// A configured-provider stand-in for the v2 craft rules. Plan N (0-based)
/// is `plans[N]` (the last repeats); writer call N returns `body_lengths[N]`
/// characters (the last repeats). Chapters are written in two parts.
struct CraftGateway {
    plans: Vec<Value>,
    body_lengths: Vec<usize>,
    /// Writes half-width punctuation into each part, as real writers do.
    punctuation_sample: bool,
    /// Times writer call N repeats `MOTIF` (missing calls repeat it none).
    motifs: Vec<usize>,
    /// The first audit reports an unresolved hard problem.
    fail_first_audit: bool,
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
        let renders = self.inputs("RENDER_CHAPTER").len();
        self.inputs
            .lock()
            .unwrap()
            .push((capability.to_string(), input.clone()));
        let audits = self.inputs("AUDIT_EXTRACT").len();
        let mut response = RecordingGateway.invoke(capability, input)?;
        if capability == "AUDIT_EXTRACT" && self.fail_first_audit && audits == 1 {
            response.output["unresolvedHard"] = json!(true);
            response.output["auditCompleteness"] = json!("UNRESOLVED_HARD");
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
        if capability == "RENDER_CHAPTER" && self.punctuation_sample {
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
        if capability == "RENDER_CHAPTER" {
            let length = *self
                .body_lengths
                .get(renders)
                .unwrap_or_else(|| self.body_lengths.last().unwrap());
            let motif = MOTIF.repeat(self.motifs.get(renders).copied().unwrap_or(0));
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
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[1250]));
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
    let gateway = Arc::new(CraftGateway::new(vec![thin, good_plan()], &[1250]));
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
fn craft_v2_second_part_is_written_twice_and_the_fitting_version_is_kept() {
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[1200, 500, 1300]));
    let project_id = "p-craft-parts";
    let (_temp, state, chapter_id, result) = produce(gateway.clone(), project_id);
    result.unwrap();
    let renders = gateway.inputs("RENDER_CHAPTER");
    assert_eq!(renders.len(), 3);
    // Part 1 holds the first scene and stops where the second begins.
    assert_eq!(renders[0]["part"], json!({"index":1,"count":2}));
    assert_eq!(
        renders[0]["chapterPlan"]["sceneExecution"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(renders[0]["stopBefore"], "节拍0");
    // Two versions of part 2, both continuing from the whole first part.
    for second in &renders[1..] {
        assert_eq!(second["part"], json!({"index":2,"count":2}));
        assert_eq!(
            second["previousText"].as_str().unwrap().chars().count(),
            1200
        );
        assert!(second.get("partFeedback").is_none());
    }
    // The 500-character version leaves the chapter short; the other is kept.
    assert_eq!(
        committed_word_count(&state, project_id, &chapter_id),
        1200 + 2 + 1300
    );
}

#[test]
fn craft_v2_two_more_second_parts_follow_the_gap_when_neither_fits() {
    let gateway = Arc::new(CraftGateway::new(
        vec![good_plan()],
        &[1200, 500, 600, 1300, 900],
    ));
    let project_id = "p-craft-parts-gap";
    let (_temp, state, chapter_id, result) = produce(gateway.clone(), project_id);
    result.unwrap();
    let renders = gateway.inputs("RENDER_CHAPTER");
    assert_eq!(renders.len(), 5);
    // The gap is taken from the version nearest the range (600 characters).
    for retry in &renders[3..] {
        assert_eq!(retry["partFeedback"]["previousCharacterCount"], 600);
        assert_eq!(retry["partFeedback"]["shortBy"], 2000 - 1802);
    }
    // Of 1300 and 900, 1300 lands nearest the middle of the range.
    assert_eq!(
        committed_word_count(&state, project_id, &chapter_id),
        1200 + 2 + 1300
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
fn craft_v2_retry_writes_both_parts_again() {
    let gateway = Arc::new(CraftGateway::new(
        vec![good_plan()],
        &[600, 600, 600, 600, 600, 1200, 1300],
    ));
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
    // Two attempts with two versions of part 2 each exceed the fixture's
    // ten-run budget.
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
    let renders = gateway.inputs("RENDER_CHAPTER");
    assert_eq!(renders.len(), 8);
    assert_eq!(renders[5]["part"]["index"], 1);
    assert!(renders[5].get("partFeedback").is_none());
}

#[test]
fn craft_v2_long_form_plan_compile_gets_guidance_and_replans_a_rejected_plan() {
    let temp = tempfile::tempdir().unwrap();
    let no_role =
        json!({"chapterRole":null,"sceneExecution":good_plan()["sceneExecution"].clone()});
    let gateway = Arc::new(CraftGateway::new(vec![no_role, good_plan()], &[1250]));
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
    let gateway = Arc::new(CraftGateway::new(
        vec![good_plan()],
        &[900, 900, 1200, 1300],
    ));
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
fn craft_v2_writing_and_audit_use_their_step_models() {
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[1250]));
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
    let gateway = Arc::new(CraftGateway::new(vec![good_plan()], &[1250]));
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::with_production_gateway(AppPaths::new(temp.path()), gateway);
    let project_id = "p-craft-exact-limit";
    let connection = state
        .manager
        .create(project_id, "Craft limit", NOW)
        .unwrap();
    let chapter_id = seed_chapter_and_gate(&connection, project_id);
    // Plan, part 1, two versions of part 2 and the audit: five runs (no
    // v1.2 repair).
    connection
        .execute(
            "UPDATE chapter_production_budgets SET hard_max_model_runs=5 WHERE project_id=?1",
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
        ..CraftGateway::new(vec![good_plan()], &[1250])
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
fn craft_v2_a_first_part_with_a_third_use_of_a_phrase_is_rewritten_once() {
    let gateway = Arc::new(CraftGateway {
        motifs: vec![3],
        ..CraftGateway::new(vec![good_plan()], &[1000, 1000, 1300])
    });
    let (_temp, _state, _chapter_id, result) = produce(gateway.clone(), "p-craft-overused-1");
    result.unwrap();
    let renders = gateway.inputs("RENDER_CHAPTER");
    assert_eq!(renders.len(), 4);
    assert!(renders[0].get("repeatedPhrases").is_none());
    assert_eq!(renders[1]["part"], json!({"index":1,"count":2}));
    assert!(renders[1]["repeatedPhrases"][0]
        .as_str()
        .unwrap()
        .contains("蓝莓籽"));
    // Part 2 continues from the rewritten first part.
    assert!(!renders[2]["previousText"]
        .as_str()
        .unwrap()
        .contains("蓝莓籽"));
}

#[test]
fn craft_v2_a_second_part_with_a_third_use_of_a_phrase_is_not_kept() {
    // Part 1 uses the image twice; version 2a, nearer the middle, a third time.
    let gateway = Arc::new(CraftGateway {
        motifs: vec![2, 1, 0],
        ..CraftGateway::new(vec![good_plan()], &[1200, 1300, 1600])
    });
    let project_id = "p-craft-overused-2";
    let (_temp, state, chapter_id, result) = produce(gateway.clone(), project_id);
    result.unwrap();
    assert_eq!(gateway.inputs("RENDER_CHAPTER").len(), 3);
    assert_eq!(
        committed_word_count(&state, project_id, &chapter_id),
        1200 + 2 + 1600
    );
}

#[test]
fn craft_v2_two_more_second_parts_avoid_the_phrase_when_both_reuse_it() {
    let gateway = Arc::new(CraftGateway {
        motifs: vec![2, 1, 1, 1, 0],
        ..CraftGateway::new(vec![good_plan()], &[1200, 1300, 1300, 1300, 1600])
    });
    let project_id = "p-craft-overused-3";
    let (_temp, state, chapter_id, result) = produce(gateway.clone(), project_id);
    result.unwrap();
    let renders = gateway.inputs("RENDER_CHAPTER");
    assert_eq!(renders.len(), 5);
    for retry in &renders[3..] {
        assert!(retry["repeatedPhrases"][0]
            .as_str()
            .unwrap()
            .contains("蓝莓籽"));
    }
    assert_eq!(
        committed_word_count(&state, project_id, &chapter_id),
        1200 + 2 + 1600
    );
}

#[test]
fn craft_v2_a_rewritten_first_part_outside_its_length_range_is_not_kept() {
    let gateway = Arc::new(CraftGateway {
        motifs: vec![3],
        ..CraftGateway::new(vec![good_plan()], &[1000, 100, 1300])
    });
    let project_id = "p-craft-overused-short";
    let (_temp, _state, _chapter_id, result) = produce(gateway.clone(), project_id);
    result.unwrap();
    let renders = gateway.inputs("RENDER_CHAPTER");
    assert!(renders[2]["previousText"]
        .as_str()
        .unwrap()
        .contains("蓝莓籽"));
}

#[test]
fn craft_v2_a_retry_with_new_prose_audits_the_new_prose() {
    let gateway = Arc::new(CraftGateway {
        fail_first_audit: true,
        ..CraftGateway::new(vec![good_plan()], &[1000, 1300])
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
    // The retry wrote both parts again, so its prose gets its own audit.
    assert_eq!(gateway.inputs("RENDER_CHAPTER").len(), 6);
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
    let gateway = Arc::new(CraftGateway::new(vec![short_horizon, good_plan()], &[1250]));
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
