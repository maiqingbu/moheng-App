//! CRAFT v3 step 2b: deterministic r2 plan checks, the creation ledger and
//! the skill → plan-field decisions (CRAFT_V3_DESIGN §4.2–4.3).

use ai_novel_production_system_core::chapter_craft::plan_feedback;
use ai_novel_production_system_core::craft_plan::{
    check_craft_plan, creation_ledger, plan_problems, skill_decisions,
};
use serde_json::{json, Value};

fn bible() -> Value {
    let path = format!(
        "{}/../contracts/fixtures/valid/narrative-story-bible-r1.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn person(name: &str) -> Value {
    json!({"name":name,"wants":"要回被扣下的货款","hides":"自己也拿过一回回扣","whyHere":"每天这个时辰来码头对账"})
}

fn scene(place: &str) -> Value {
    json!({"place":place,"people":[person("林满"),person("老董")],"conflict":"货单上的箱数和船上对不上",
        "events":["老董把货单递过来","林满点出少了两箱","两人在船边争起来"],"readerQuestion":"少的两箱去了哪里"})
}

/// A chapter-1 plan that passes every check against the fixture bible.
fn plan() -> Value {
    json!({"payloadSchema":"chapter-plan.r2","craft":{
        "bigTurn":"林满发现货单是彭三的人连夜调包的",
        "smallDiscovery":"老董自己也欠着彭三一笔钱",
        "protagonistWin":"林满当着众人把调包的货单揪了出来",
        "device":{"name":"听旧物","status":"NEW","howUsed":"摸到货箱上的麻绳，听见调包人的说话声"},
        "warmth":"姑妈在灶上给他留了一碗热面",
        "emotionalBeat":"他看见姑妈围裙上又多了一块新补丁",
        "situationChange":{"before":"只管码头记账的小账房","after":"被彭三的人盯上的账房"},
        "scenes":[scene("江边码头"),scene("姑妈的面馆")],
        "newLosses":["前一天的记忆"],
        "endHook":"面馆门板上贴了一张盖着红印的借据"
    }})
}

fn has(problems: &[String], needle: &str) -> bool {
    problems.iter().any(|problem| problem.contains(needle))
}

#[test]
fn a_concrete_first_chapter_plan_passes() {
    let problems = plan_problems(&plan(), 1, &bible(), &creation_ledger(&[]));
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn vague_decisions_and_an_unchanged_situation_are_reported() {
    let mut plan = plan();
    plan["craft"]["warmth"] = json!("温暖");
    plan["craft"]["situationChange"] =
        json!({"before":"被彭三盯上的账房","after":"被彭三盯上的账房"});
    let problems = plan_problems(&plan, 1, &bible(), &creation_ledger(&[]));
    assert!(has(&problems, "温度或幽默写得太空"), "{problems:?}");
    assert!(has(&problems, "处境变化前后相同"), "{problems:?}");
}

#[test]
fn devices_must_be_new_or_a_real_escalation_of_an_earlier_one() {
    let prior = [(1, plan()["craft"].clone())];
    let ledger = creation_ledger(&prior);
    let repeated = plan_problems(&plan(), 2, &bible(), &ledger);
    assert!(has(&repeated, "之前已经用过"), "{repeated:?}");
    assert!(has(&repeated, "用法和之前一模一样"), "{repeated:?}");

    let mut escalated = plan();
    escalated["craft"]["device"] =
        json!({"name":"听旧物","status":"ESCALATED","howUsed":"隔着一层木板也能听见旧物里的声音"});
    escalated["craft"]["scenes"] = json!([scene("当铺后院"), scene("江边码头")]);
    escalated["craft"]["situationChange"]["after"] = json!("拿到了彭三借据的漏洞");
    escalated["craft"]["endHook"] = json!("当铺掌柜把一只旧表推到他面前");
    let ok = plan_problems(&escalated, 2, &bible(), &ledger);
    assert!(ok.is_empty(), "{ok:?}");

    let mut fake = plan();
    fake["craft"]["device"]["status"] = json!("ESCALATED");
    assert!(has(
        &plan_problems(&fake, 1, &bible(), &creation_ledger(&[])),
        "之前没用过"
    ));
}

#[test]
fn reusing_two_places_or_an_ending_of_an_earlier_chapter_is_reported() {
    let ledger = creation_ledger(&[(1, plan()["craft"].clone())]);
    let mut next = plan();
    next["craft"]["device"] =
        json!({"name":"读账本","status":"NEW","howUsed":"翻到旧账本时看见记账人当时的手"});
    let problems = plan_problems(&next, 2, &bible(), &ledger);
    assert!(has(&problems, "与上一章共用了 2 个地点"), "{problems:?}");
    assert!(
        has(&problems, "结束的局面和之前某章完全相同"),
        "{problems:?}"
    );
    assert!(has(&problems, "章末钩子和之前某章完全相同"), "{problems:?}");
}

#[test]
fn people_must_be_scheduled_by_the_story_bible() {
    let mut early = plan();
    early["craft"]["scenes"][0]["people"][1] = person("阿秋");
    early["craft"]["scenes"][1]["people"][1] = person("陌生人");
    let mut bible = bible();
    bible["cast"][0]["firstAppears"] = json!(2);
    let problems = plan_problems(&early, 1, &bible, &creation_ledger(&[]));
    assert!(
        has(
            &problems,
            "人物「阿秋、陌生人」没有在故事底稿里安排到本章出场"
        ),
        "{problems:?}"
    );
    assert!(plan_problems(
        &early,
        2,
        &bible,
        &creation_ledger(&[(1, json!({"scenes":[{"people":[person("陌生人")]}]}))])
    )
    .is_empty());
}

#[test]
fn chapter_three_must_name_what_changes_with_the_antagonist() {
    let mut third = plan();
    third["craft"]["bigTurn"] = json!("林满发现货单被人连夜调包了");
    third["craft"]["situationChange"]["after"] = json!("被人盯上的账房");
    third["craft"]["smallDiscovery"] = json!("老董自己也欠着别人一笔钱");
    let problems = plan_problems(&third, 3, &bible(), &creation_ledger(&[]));
    assert!(
        has(&problems, "和对手「彭三」的关系怎么变了"),
        "{problems:?}"
    );
    assert!(plan_problems(&plan(), 3, &bible(), &creation_ledger(&[])).is_empty());
}

#[test]
fn quoted_lines_and_similes_do_not_belong_in_a_plan() {
    let mut wordy = plan();
    wordy["craft"]["emotionalBeat"] = json!("他说“这笔账我记下了”，心像被攥紧了似的");
    let problems = plan_problems(&wordy, 1, &bible(), &creation_ledger(&[]));
    assert!(has(&problems, "带引号的台词"), "{problems:?}");
    assert!(has(&problems, "用了比喻"), "{problems:?}");
}

#[test]
fn the_ledger_collects_devices_endings_people_losses_and_last_places() {
    let mut second = plan()["craft"].clone();
    second["scenes"] = json!([scene("当铺后院"), scene("渡口")]);
    let ledger = creation_ledger(&[(1, plan()["craft"].clone()), (2, second)]);
    assert_eq!(ledger["devices"].as_array().unwrap().len(), 2);
    assert_eq!(ledger["endings"][0]["chapter"], 1);
    assert_eq!(ledger["introducedPeople"], json!(["林满", "老董"]));
    assert_eq!(ledger["losses"][1]["chapter"], 2);
    assert_eq!(ledger["lastChapterPlaces"], json!(["当铺后院", "渡口"]));
}

#[test]
fn a_rejected_plan_returns_every_problem_to_the_planner() {
    let mut bad = plan();
    bad["craft"]["warmth"] = json!("温暖");
    let error = check_craft_plan(&bad, 1, &bible(), &creation_ledger(&[])).unwrap_err();
    assert!(error.starts_with("B08_PLAN_CRAFT:"), "{error}");
    let feedback = plan_feedback(&error).unwrap();
    assert_eq!(feedback["rejectedFor"], "B08_PLAN_CRAFT");
    assert!(feedback["problems"][0]
        .as_str()
        .unwrap()
        .contains("温度或幽默"));
    check_craft_plan(&plan(), 1, &bible(), &creation_ledger(&[])).unwrap();
}

#[test]
fn skill_decisions_name_the_plan_fields_each_selected_skill_decided() {
    let decisions = skill_decisions(&plan(), &["S13".into(), "S22".into(), "S99".into()]);
    let decisions = decisions.as_array().unwrap();
    assert_eq!(decisions.len(), 2);
    assert_eq!(decisions[0]["skillId"], "S13");
    assert_eq!(
        decisions[0]["decisions"]["protagonistWin"],
        "林满当着众人把调包的货单揪了出来"
    );
    assert_eq!(
        decisions[1]["decisions"]["readerQuestions"],
        json!(["少的两箱去了哪里", "少的两箱去了哪里"])
    );
}

#[test]
fn the_planner_sees_a_bounded_ledger_while_checks_use_the_whole_history() {
    use ai_novel_production_system_core::craft_plan::prompt_ledger;
    let prior = (1..=15)
        .map(|chapter| {
            let mut craft = plan()["craft"].clone();
            craft["device"]["howUsed"] = json!(format!("第{chapter}次的用法"));
            craft["situationChange"]["after"] = json!(format!("第{chapter}章的局面"));
            (chapter, craft)
        })
        .collect::<Vec<_>>();
    let ledger = creation_ledger(&prior);
    let view = prompt_ledger(&ledger);
    assert_eq!(
        view["devices"],
        json!([{"chapter":15,"name":"听旧物","howUsed":"第15次的用法"}])
    );
    assert_eq!(view["endings"].as_array().unwrap().len(), 10);
    assert_eq!(view["endings"][0]["chapter"], 6);
    assert_eq!(view["losses"].as_array().unwrap().len(), 10);
    let mut early = plan();
    early["craft"]["device"] =
        json!({"name":"听旧物","status":"ESCALATED","howUsed":"第2次的用法"});
    assert!(has(
        &plan_problems(&early, 16, &bible(), &ledger),
        "用法和之前一模一样"
    ));
}

#[test]
fn scene_counts_people_and_events_outside_the_plan_shape_are_reported_for_a_replan() {
    // Real run 2026-10-24: a scene with seven events and one with a single
    // person failed the contract with no chance to replan.
    let mut crowded = plan();
    crowded["craft"]["scenes"][0]["events"] = json!(["一", "二", "三", "四", "五", "六", "七"]);
    crowded["craft"]["scenes"][1]["people"] = json!([person("林满")]);
    let problems = plan_problems(&crowded, 1, &bible(), &creation_ledger(&[]));
    assert!(has(&problems, "第 1 场有 7 条事件"), "{problems:?}");
    assert!(has(&problems, "第 2 场只有 1 个人物"), "{problems:?}");
    let mut single = plan();
    single["craft"]["scenes"] = json!([scene("江边码头")]);
    single["craft"]["device"]["status"] = json!("OLD");
    let problems = plan_problems(&single, 1, &bible(), &creation_ledger(&[]));
    assert!(has(&problems, "需要 2 到 3 个场景"), "{problems:?}");
    assert!(
        has(&problems, "status 只能是 NEW 或 ESCALATED"),
        "{problems:?}"
    );
}
