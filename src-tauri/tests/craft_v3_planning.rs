//! CRAFT v3 step 2c: a project with a confirmed story bible plans its
//! chapters as `chapter-plan.r2` with the bible and the creation ledger, the
//! r2 checks send problems back for one replan, and the production receipt
//! records which skill decided which plan field.

mod craft_v3_support;

use ai_novel_production_system_core::commands::{query_production_center_impl, KernelState};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use craft_v3_support::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct CraftGateway {
    plan_inputs: Mutex<Vec<Value>>,
    /// The first this-many plans are too vague to pass the r2 checks.
    vague_plans: AtomicUsize,
    /// The first plan previews only two later chapters.
    short_horizon_once: AtomicBool,
    /// Every plan previews only two later chapters.
    short_horizon_always: bool,
    /// Every plan has four scenes, the last with one person in it.
    four_scenes_always: bool,
    /// No plan names chapter 1's imminent threat.
    no_opening_threat: bool,
    /// Every plan leaves required fields out.
    missing_fields_always: bool,
    /// Chapter 1's threat is a state with no deadline or opening.
    vague_threat: bool,
}

impl ProductionModelGateway for CraftGateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        Ok(ResolvedProviderProfile {
            provider: "craft-fixture-provider".into(),
            model: "craft-fixture-model".into(),
            endpoint: "fixture://craft".into(),
            timeout_ms: 1_000,
        })
    }

    fn resolve_step_profile(&self, capability: &str) -> Result<ResolvedProviderProfile, String> {
        let mut profile = self.resolve_profile()?;
        if capability == "PLAN_CHAPTER_CRAFT" {
            profile.model = "craft-plan-model".into();
        }
        Ok(profile)
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let output = match capability {
            "GENERATE_STORY_DIRECTIONS" => directions_output(),
            "GENERATE_STORY_BIBLE" => json!({"bibles":[bible(), bible(), bible()]}),
            "PLAN_CHAPTER_CRAFT" => {
                self.plan_inputs.lock().unwrap().push(input.clone());
                let chapter = input["chapterNumber"].as_i64().unwrap_or(1);
                let forced = self
                    .vague_plans
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                        left.checked_sub(1)
                    })
                    .is_ok();
                let first_try_of_chapter_one = chapter == 1 && input.get("planFeedback").is_none();
                let mut plan = craft_plan(input, chapter, forced || first_try_of_chapter_one);
                if self.short_horizon_once.swap(false, Ordering::SeqCst)
                    || self.short_horizon_always
                {
                    plan["horizonPreview"].as_array_mut().unwrap().truncate(2);
                }
                if self.missing_fields_always {
                    let craft = &mut plan["craft"];
                    craft["scenes"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("readerQuestion");
                    craft["scenes"][1]["people"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("hides");
                    craft["device"].as_object_mut().unwrap().remove("name");
                    craft.as_object_mut().unwrap().remove("newLosses");
                    plan.as_object_mut().unwrap().remove("chapterGoal");
                }
                if self.vague_threat && plan.get("openingThreat").is_some() {
                    plan["openingThreat"] = json!({"event":"沈砚正在失去存在感","deadline":"很快"});
                }
                if self.no_opening_threat {
                    plan.as_object_mut().unwrap().remove("openingThreat");
                }
                if self.four_scenes_always {
                    let scenes = plan["craft"]["scenes"].as_array_mut().unwrap();
                    let mut third = scenes[0].clone();
                    third["place"] = json!("锅炉房检修平台");
                    let mut alone = scenes[1].clone();
                    alone["place"] = json!("远处天台");
                    alone["people"].as_array_mut().unwrap().truncate(1);
                    scenes.extend([third, alone]);
                }
                plan
            }
            "RENDER_CHAPTER_CRAFT" => json!({"body":"林满推开面馆的门，把账本放在了灶台边。"}),
            "AUDIT_EXTRACT" => audit_no_change(),
            other => return Err(format!("unexpected capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

#[test]
fn a_project_with_a_confirmed_story_bible_plans_r2_chapters_with_bible_and_ledger() {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway::default());
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, "v3");
    command(
        &state,
        &project_id,
        "v3:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        "v3:start",
        "opening_start_creating",
        json!({}),
    );

    for chapter in 1..=2 {
        let center = query_production_center_impl(
            &state,
            json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
        )
        .unwrap();
        let result = command(
            &state,
            &project_id,
            &format!("v3:production:{chapter}"),
            "production_start",
            json!({
                "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
                "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
            }),
        );
        assert_eq!(result["finalStatus"], "committed", "chapter {chapter}");
    }

    let inputs = gateway.plan_inputs.lock().unwrap();
    assert_eq!(
        inputs.len(),
        3,
        "chapter 1 is replanned once, chapter 2 passes first time"
    );
    let first = &inputs[0];
    assert_eq!(first["chapterNumber"], 1);
    assert_eq!(first["storyBible"]["antagonist"]["name"], "彭三");
    assert!(
        first["storyBible"].get("stage").is_none(),
        "the planner gets the bible content only"
    );
    assert_eq!(first["creationLedger"]["devices"], json!([]));
    let feedback = &inputs[1]["planFeedback"];
    assert_eq!(feedback["rejectedFor"], "B08_PLAN_CRAFT");
    assert!(feedback["problems"][0]
        .as_str()
        .unwrap()
        .contains("温度或幽默"));
    let second_chapter = &inputs[2];
    assert_eq!(second_chapter["chapterNumber"], 2);
    assert_eq!(
        second_chapter["creationLedger"]["devices"][0]["name"],
        "听旧物"
    );
    assert_eq!(
        second_chapter["creationLedger"]["lastChapterPlaces"],
        json!(["江边码头", "姑妈的面馆"])
    );
    drop(inputs);

    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let plan_models: Vec<String> = connection
        .prepare("SELECT DISTINCT model FROM model_runs WHERE project_id=?1 AND capability='PLAN_CHAPTER_CRAFT'")
        .unwrap()
        .query_map([&project_id], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        plan_models,
        vec!["craft-plan-model"],
        "r2 plans use the planning step model"
    );
    let plans: Vec<(String, Option<String>)> = connection
        .prepare("SELECT json_extract(p.payload_json,'$.payloadSchema'), json_extract(p.payload_json,'$.craft.smallDiscovery') FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan' AND h.scope_level='CHAPTER'")
        .unwrap()
        .query_map([&project_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let r2_discoveries = plans
        .iter()
        .filter(|(schema, _)| schema == "chapter-plan.r2")
        .filter_map(|(_, discovery)| discovery.as_deref())
        .collect::<Vec<_>>();
    assert!(r2_discoveries.len() >= 2, "{plans:?}");
    assert!(
        r2_discoveries
            .iter()
            .all(|text| text.contains("欠彭三") && !text.contains('“')),
        "the committed plan has its quote marks taken out: {r2_discoveries:?}"
    );
    let receipt: String = connection
        .query_row(
            "SELECT p.payload_json FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ProductionReceipt' ORDER BY a.created_at, a.artifact_id LIMIT 1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let receipt: Value = serde_json::from_str(&receipt).unwrap();
    let decisions = receipt["craftSkills"]["chapterPlanDecisions"]
        .as_array()
        .expect("chapterPlanDecisions");
    let planned = receipt["craftSkills"]["chapterPlan"].as_array().unwrap();
    assert_eq!(
        decisions.len(),
        planned
            .iter()
            .filter(|id| decisions.iter().any(|d| d["skillId"] == **id))
            .count()
    );
    if let Some(golden) = decisions.iter().find(|item| item["skillId"] == "S13") {
        let win = golden["decisions"]["protagonistWin"].as_str().unwrap();
        assert!(win.ends_with("章林满当众把假账揪了出来"), "{win}");
    }
}

#[test]
fn a_plan_with_a_short_horizon_and_craft_problems_gets_all_of_them_back_at_once() {
    // Real run 2026-10-25: chapter 8 was rejected for its horizon, then for
    // its craft, and each replan fixed only what it was told and undid the
    // other, until the plans ran out.
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway {
        short_horizon_once: AtomicBool::new(true),
        ..CraftGateway::default()
    });
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, "horizon");
    command(
        &state,
        &project_id,
        "horizon:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        "horizon:start",
        "opening_start_creating",
        json!({}),
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let result = command(
        &state,
        &project_id,
        "horizon:production",
        "production_start",
        json!({
            "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
            "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
        }),
    );
    assert_eq!(result["finalStatus"], "committed");

    let inputs = gateway.plan_inputs.lock().unwrap();
    assert!(inputs[0]["horizonPreviewCapacity"].as_i64().unwrap() >= 3);
    let problems = inputs[1]["planFeedback"]["problems"].to_string();
    assert!(
        problems.contains("horizonPreview") && problems.contains("温度或幽默"),
        "{problems}"
    );
}

#[test]
fn a_plan_still_vague_after_its_replans_is_written_with_a_plan_reminder() {
    // LD-1 step 2: problems the planner could not fix in its replans no
    // longer fail the job; the chapter is written from the last plan and the
    // problems wait in 待处理.
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway {
        vague_plans: AtomicUsize::new(3),
        ..CraftGateway::default()
    });
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, "lenient");
    command(
        &state,
        &project_id,
        "lenient:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        "lenient:start",
        "opening_start_creating",
        json!({}),
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let result = command(
        &state,
        &project_id,
        "lenient:production",
        "production_start",
        json!({
            "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
            "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
        }),
    );
    assert_eq!(result["finalStatus"], "committed");
    // A plan and its two replans.
    assert_eq!(gateway.plan_inputs.lock().unwrap().len(), 3);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let payload: String = connection
        .query_row(
            "SELECT payload_json FROM decisions WHERE project_id=?1 AND decision_type='AUDIT_REMINDER'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let payload: Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(payload["category"], "计划提醒");
    assert!(
        payload["heldBack"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["field"] == "plan"
                && entry["item"].as_str().unwrap().contains("温度或幽默")),
        "{payload}"
    );
}

#[test]
fn a_plan_whose_horizon_stays_short_is_written_with_a_plan_reminder() {
    // LD-1 step 2: the preview is padded with tentative slots so the plan's
    // contract holds, and the short preview is reported.
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway {
        short_horizon_always: true,
        ..CraftGateway::default()
    });
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, "short");
    command(
        &state,
        &project_id,
        "short:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        "short:start",
        "opening_start_creating",
        json!({}),
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let result = command(
        &state,
        &project_id,
        "short:production",
        "production_start",
        json!({
            "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
            "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
        }),
    );
    assert_eq!(result["finalStatus"], "committed");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let payload: String = connection
        .query_row(
            "SELECT payload_json FROM decisions WHERE project_id=?1 AND decision_type='AUDIT_REMINDER' AND json_extract(payload_json,'$.category')='计划提醒'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(payload.contains("horizonPreview"), "{payload}");
}

#[test]
fn a_plan_that_keeps_four_scenes_is_folded_to_three_and_written_with_a_plan_reminder() {
    // Acceptance 2026-10-26, chapter 10: the planner kept a fourth scene with
    // one person in it through its replans. The last lenient pass folds the
    // extra scene into the one before, so the plan holds its contract, and
    // says so in the reminder.
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway {
        four_scenes_always: true,
        ..CraftGateway::default()
    });
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, "four");
    command(
        &state,
        &project_id,
        "four:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        "four:start",
        "opening_start_creating",
        json!({}),
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let result = command(
        &state,
        &project_id,
        "four:production",
        "production_start",
        json!({
            "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
            "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
        }),
    );
    assert_eq!(result["finalStatus"], "committed", "{result}");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let payload: String = connection
        .query_row(
            "SELECT payload_json FROM decisions WHERE project_id=?1 AND decision_type='AUDIT_REMINDER' AND json_extract(payload_json,'$.category')='计划提醒'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(payload.contains("并入"), "{payload}");
}

#[test]
fn qc1_chapter_one_carries_its_imminent_threat_in_the_first_scene() {
    // QC-1.2 (owner decision 2026-10-26): the danger and its deadline join
    // the first scene's conflict, where the writer reads it.
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway::default());
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, "threat");
    command(
        &state,
        &project_id,
        "threat:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        "threat:start",
        "opening_start_creating",
        json!({}),
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let result = command(
        &state,
        &project_id,
        "threat:production",
        "production_start",
        json!({
            "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
            "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
        }),
    );
    assert_eq!(result["finalStatus"], "committed", "{result}");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let conflict: String = connection
        .query_row(
            "SELECT json_extract(p.payload_json,'$.craft.scenes[0].conflict') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ChapterPlan' ORDER BY a.version DESC LIMIT 1",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        conflict.contains("迫近的威胁：彭三要收走姑妈的面馆抵债（期限：三天后的月底；开头写法：彭三的伙计把催债单拍在面馆柜台上）"),
        "{conflict}"
    );
}

#[test]
fn qc1_chapter_one_without_an_imminent_threat_is_written_with_a_plan_reminder() {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway {
        no_opening_threat: true,
        ..CraftGateway::default()
    });
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, "nothreat");
    command(
        &state,
        &project_id,
        "nothreat:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        "nothreat:start",
        "opening_start_creating",
        json!({}),
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let result = command(
        &state,
        &project_id,
        "nothreat:production",
        "production_start",
        json!({
            "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
            "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
        }),
    );
    assert_eq!(result["finalStatus"], "committed", "{result}");
    let inputs = gateway.plan_inputs.lock().unwrap();
    assert!(inputs[1]["planFeedback"]["problems"]
        .as_array()
        .unwrap()
        .iter()
        .any(|problem| problem.as_str().unwrap().contains("迫在眉睫的威胁")));
    drop(inputs);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let payload: String = connection
        .query_row(
            "SELECT payload_json FROM decisions WHERE project_id=?1 AND decision_type='AUDIT_REMINDER' AND json_extract(payload_json,'$.category')='计划提醒'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(payload.contains("迫在眉睫的威胁"), "{payload}");
}

#[test]
fn qc1_a_plan_missing_required_fields_is_sent_back_then_completed_with_a_plan_reminder() {
    // Acceptance 2026-10-26 (fourth run), chapter 7: a scene without its
    // readerQuestion failed the plan contract with no replan.
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway {
        missing_fields_always: true,
        ..CraftGateway::default()
    });
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, "missing");
    command(
        &state,
        &project_id,
        "missing:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        "missing:start",
        "opening_start_creating",
        json!({}),
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let result = command(
        &state,
        &project_id,
        "missing:production",
        "production_start",
        json!({
            "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
            "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
        }),
    );
    assert_eq!(result["finalStatus"], "committed", "{result}");
    let inputs = gateway.plan_inputs.lock().unwrap();
    let problems = inputs[1]["planFeedback"]["problems"].to_string();
    for missing in [
        "留给读者的问题",
        "隐瞒",
        "异常手法的名字",
        "失去的东西",
        "本章目标",
    ] {
        assert!(problems.contains(missing), "{missing}: {problems}");
    }
    drop(inputs);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let payload: String = connection
        .query_row(
            "SELECT payload_json FROM decisions WHERE project_id=?1 AND decision_type='AUDIT_REMINDER' AND json_extract(payload_json,'$.category')='计划提醒'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(payload.contains("系统补上"), "{payload}");
}

#[test]
fn qc1_a_vague_opening_threat_is_sent_back_for_an_event_a_deadline_and_an_opening() {
    // Acceptance 2026-10-26 (fifth run): "失去现实存在感，今晚十二点前"
    // never reached the page; the threat must be something that happens,
    // by a time, shown in the opening.
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(CraftGateway {
        vague_threat: true,
        ..CraftGateway::default()
    });
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, "vague");
    command(
        &state,
        &project_id,
        "vague:opening",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        "vague:start",
        "opening_start_creating",
        json!({}),
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let result = command(
        &state,
        &project_id,
        "vague:production",
        "production_start",
        json!({
            "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
            "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
        }),
    );
    assert_eq!(result["finalStatus"], "committed", "{result}");
    let inputs = gateway.plan_inputs.lock().unwrap();
    let problems = inputs[1]["planFeedback"]["problems"].to_string();
    assert!(
        problems.contains("onPage") && problems.contains("deadline"),
        "{problems}"
    );
}
