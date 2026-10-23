//! CRAFT v3 chapter plans (`chapter-plan.r2`, CRAFT_V3_DESIGN §4.2–4.3): the
//! deterministic plan checks, the creation ledger rebuilt from earlier
//! chapters' plans, and which skill bound which plan decision.

use serde_json::{json, Value};
use std::collections::BTreeSet;

/// The error prefix of a plan rejected by these checks; the planner gets the
/// problems and one more try (see `chapter_craft::plan_feedback`).
pub const PLAN_CRAFT_ERROR: &str = "B08_PLAN_CRAFT";
const MIN_DECISION_CHARS: usize = 6;
const SIMILE_MARKERS: &[&str] = &["像是", "仿佛", "如同", "宛如", "好似", "犹如", "似的"];
const QUOTE_MARKS: &[char] = &['“', '”', '「', '」', '"'];
/// Plan decisions each planning skill is bound to (CRAFT_V3_DESIGN §4.2).
const SKILL_FIELDS: &[(&str, &[&str])] = &[
    ("S10", &["endHook", "readerQuestions"]),
    ("S11", &["people"]),
    ("S13", &["protagonistWin", "bigTurn"]),
    ("S14", &["endHook"]),
    ("S20", &["situationChange", "newLosses"]),
    ("S21", &["bigTurn", "smallDiscovery"]),
    ("S22", &["smallDiscovery", "readerQuestions"]),
    ("S24", &["protagonistWin"]),
    ("S04", &["people"]),
    ("S05", &["people"]),
    ("S28", &["conflicts"]),
    ("S29", &["emotionalBeat"]),
    ("S47", &["warmth"]),
];

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("").trim()
}

fn scenes(craft: &Value) -> &[Value] {
    craft["scenes"].as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn scene_people(craft: &Value) -> impl Iterator<Item = &Value> {
    scenes(craft)
        .iter()
        .flat_map(|scene| scene["people"].as_array().into_iter().flatten())
}

/// Every string in the plan's craft decisions, for the wording checks.
fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(map) => map.values().for_each(|item| strings(item, out)),
        _ => {}
    }
}

/// The creation ledger (CRAFT_V3_DESIGN §4.3): what the committed chapters
/// already used, rebuilt from their r2 plans in chapter order.
/// `prior` holds `(chapter_no, craft)` pairs; r1 chapters have no craft and
/// are simply absent.
pub fn creation_ledger(prior: &[(i64, Value)]) -> Value {
    let mut devices = Vec::new();
    let mut endings = Vec::new();
    let mut people = BTreeSet::new();
    let mut losses = Vec::new();
    let mut last_places = Vec::new();
    for (chapter, craft) in prior {
        devices.push(json!({"chapter":chapter,"name":text(&craft["device"]["name"]),"howUsed":text(&craft["device"]["howUsed"])}));
        endings.push(json!({"chapter":chapter,"after":text(&craft["situationChange"]["after"]),"endHook":text(&craft["endHook"])}));
        people.extend(scene_people(craft).map(|person| text(&person["name"]).to_string()));
        losses.extend(
            craft["newLosses"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|loss| json!({"chapter":chapter,"loss":text(loss)})),
        );
        last_places = scenes(craft)
            .iter()
            .map(|scene| text(&scene["place"]).to_string())
            .collect();
    }
    json!({
        "devices":devices,
        "endings":endings,
        "introducedPeople":people.into_iter().filter(|name| !name.is_empty()).collect::<Vec<_>>(),
        "losses":losses,
        "lastChapterPlaces":last_places
    })
}

/// People the story bible allows on stage by this chapter.
fn allowed_people(bible: &Value, chapter: i64, ledger: &Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for path in ["/protagonist/name", "/antagonist/name"] {
        if let Some(name) = bible.pointer(path).and_then(Value::as_str) {
            names.insert(name.trim().to_string());
        }
    }
    for member in bible["cast"].as_array().into_iter().flatten() {
        if member["firstAppears"].as_i64().unwrap_or(1) <= chapter {
            names.insert(text(&member["name"]).to_string());
        }
    }
    for name in ledger["introducedPeople"].as_array().into_iter().flatten() {
        names.insert(text(name).to_string());
    }
    names
}

/// The deterministic checks of an r2 plan (CRAFT_V3_DESIGN §4.2). Returns
/// every problem found, in Chinese, so the planner can fix them in one pass.
pub fn plan_problems(plan: &Value, chapter: i64, bible: &Value, ledger: &Value) -> Vec<String> {
    let craft = &plan["craft"];
    let mut problems = Vec::new();
    for (path, label) in [
        ("/bigTurn", "大转折"),
        ("/smallDiscovery", "小发现"),
        ("/protagonistWin", "主角赢的一局"),
        ("/warmth", "温度或幽默"),
        ("/emotionalBeat", "情感时刻"),
        ("/endHook", "章末钩子"),
        ("/situationChange/before", "处境变化（前）"),
        ("/situationChange/after", "处境变化（后）"),
        ("/device/howUsed", "异常手法的用法"),
    ] {
        if craft.pointer(path).map(text).unwrap_or("").chars().count() < MIN_DECISION_CHARS {
            problems.push(format!(
                "{label}写得太空，需要具体到人和事（至少 {MIN_DECISION_CHARS} 个字）"
            ));
        }
    }
    if text(&craft["situationChange"]["before"]) == text(&craft["situationChange"]["after"]) {
        problems.push("处境变化前后相同，本章结束时主角的处境必须变了".into());
    }
    let device = text(&craft["device"]["name"]);
    let used_before = ledger["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| text(&item["name"]) == device);
    match text(&craft["device"]["status"]) {
        "NEW" if used_before => problems.push(format!(
            "异常手法「{device}」之前已经用过，标为新手法不对；要么换新手法，要么写清楚怎么升级"
        )),
        "ESCALATED" if !used_before => {
            problems.push(format!("异常手法「{device}」之前没用过，不能标为升级"))
        }
        _ => {}
    }
    if ledger["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|item| {
            text(&item["name"]) == device
                && text(&item["howUsed"]) == text(&craft["device"]["howUsed"])
        })
    {
        problems.push(format!(
            "异常手法「{device}」的用法和之前一模一样，需要新的变化"
        ));
    }
    let last_places = ledger["lastChapterPlaces"]
        .as_array()
        .into_iter()
        .flatten()
        .map(text)
        .collect::<BTreeSet<_>>();
    let shared = scenes(craft)
        .iter()
        .map(|scene| text(&scene["place"]))
        .filter(|place| last_places.contains(place))
        .collect::<BTreeSet<_>>();
    if shared.len() > 1 {
        problems.push(format!(
            "与上一章共用了 {} 个地点（{}），最多共用 1 个",
            shared.len(),
            shared.into_iter().collect::<Vec<_>>().join("、")
        ));
    }
    for ending in ledger["endings"].as_array().into_iter().flatten() {
        if text(&ending["after"]) == text(&craft["situationChange"]["after"]) {
            problems.push("本章结束的局面和之前某章完全相同，需要新的局面".into());
        }
        if text(&ending["endHook"]) == text(&craft["endHook"]) {
            problems.push("章末钩子和之前某章完全相同".into());
        }
    }
    let allowed = allowed_people(bible, chapter, ledger);
    let unplanned = scene_people(craft)
        .map(|person| text(&person["name"]))
        .filter(|name| !name.is_empty() && !allowed.contains(*name))
        .collect::<BTreeSet<_>>();
    if !allowed.is_empty() && !unplanned.is_empty() {
        problems.push(format!(
            "人物「{}」没有在故事底稿里安排到本章出场，请改用已安排的人物，或先在底稿里加上",
            unplanned.into_iter().collect::<Vec<_>>().join("、")
        ));
    }
    if chapter == 3 {
        let antagonist = text(&bible["antagonist"]["name"]);
        let mut all = Vec::new();
        strings(craft, &mut all);
        if !antagonist.is_empty() && !all.iter().any(|item| item.contains(antagonist)) {
            problems.push(format!(
                "第 3 章要写明主角的具体行动以及他和对手「{antagonist}」的关系怎么变了"
            ));
        }
    }
    let mut all = Vec::new();
    strings(craft, &mut all);
    if all.iter().any(|item| item.contains(QUOTE_MARKS)) {
        problems.push("计划里写了带引号的台词；计划只写发生什么，台词留给正文".into());
    }
    if all
        .iter()
        .any(|item| SIMILE_MARKERS.iter().any(|marker| item.contains(marker)))
    {
        problems.push("计划里用了比喻；计划只写发生什么，不写修辞".into());
    }
    problems
}

/// `Err(B08_PLAN_CRAFT:…)` listing every problem, or `Ok`.
pub fn check_craft_plan(
    plan: &Value,
    chapter: i64,
    bible: &Value,
    ledger: &Value,
) -> Result<(), String> {
    let problems = plan_problems(plan, chapter, bible, ledger);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("{PLAN_CRAFT_ERROR}:{}", problems.join("；")))
    }
}

fn decision(craft: &Value, field: &str) -> Value {
    let collect = |key: &str| {
        scenes(craft)
            .iter()
            .map(|scene| scene[key].clone())
            .collect::<Vec<_>>()
    };
    match field {
        "readerQuestions" => json!(collect("readerQuestion")),
        "conflicts" => json!(collect("conflict")),
        "people" => json!(scene_people(craft).cloned().collect::<Vec<_>>()),
        other => craft[other].clone(),
    }
}

/// For the receipt: which selected planning skill decided which plan field,
/// with the decision the plan made (CRAFT_V3_DESIGN §4.2, "技能 → 字段决定").
pub fn skill_decisions(plan: &Value, skill_ids: &[String]) -> Value {
    let craft = &plan["craft"];
    json!(skill_ids
        .iter()
        .filter_map(|skill_id| {
            SKILL_FIELDS
                .iter()
                .find(|(id, _)| id == skill_id)
                .map(|(_, fields)| {
                    let decisions = fields
                        .iter()
                        .map(|field| (field.to_string(), decision(craft, field)))
                        .collect::<serde_json::Map<_, _>>();
                    json!({"skillId":skill_id,"decisions":decisions})
                })
        })
        .collect::<Vec<_>>())
}
