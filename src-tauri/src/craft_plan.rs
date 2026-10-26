//! CRAFT v3 chapter plans (`chapter-plan.r2`, CRAFT_V3_DESIGN §4.2–4.3): the
//! deterministic plan checks, the creation ledger rebuilt from earlier
//! chapters' plans, and which skill bound which plan decision.

use crate::craft_setup::QUOTE_MARKS;
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// The planner capability of CRAFT v3 chapters (`chapter-plan.r2`).
pub const PLAN_CHAPTER_CRAFT: &str = "PLAN_CHAPTER_CRAFT";

/// The error prefix of a plan rejected by these checks; the planner gets the
/// problems and up to two more tries (see `chapter_craft::plan_feedback`).
pub const PLAN_CRAFT_ERROR: &str = "B08_PLAN_CRAFT";
const MIN_DECISION_CHARS: usize = 6;
/// How many recent endings and losses the planner is shown; the checks
/// still compare against the whole ledger.
const PROMPT_LEDGER_RECENT: usize = 10;
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

/// The plan's scene shape, as chapter-plan-r2 states it: the checks report
/// a plan off it and the last lenient pass fits a plan to it.
pub(crate) const SCENE_COUNT: std::ops::RangeInclusive<usize> = 2..=3;
pub(crate) const MIN_SCENE_PEOPLE: usize = 2;
pub(crate) const SCENE_EVENTS: std::ops::RangeInclusive<usize> = 3..=5;

/// QC-1.2: what marks chapter 1's imminent threat in its first scene.
const OPENING_THREAT_MARK: &str = "迫近的威胁：";

/// QC-1.2 (owner decision 2026-10-26): the planner's `openingThreat` for
/// chapter 1 (the danger and how soon it falls) joins the first scene's
/// conflict, where the writer reads it, so the plan keeps its contract.
pub fn fold_opening_threat(plan: &mut Value) {
    let Some(threat) = plan
        .as_object_mut()
        .and_then(|object| object.remove("openingThreat"))
    else {
        return;
    };
    let (danger, deadline) = (text(&threat["danger"]), text(&threat["deadline"]));
    // A vague danger is left out, so the check sends the plan back for it.
    if danger.chars().count() < MIN_DECISION_CHARS || deadline.is_empty() {
        return;
    }
    if let Some(conflict) = plan.pointer_mut("/craft/scenes/0/conflict") {
        let line = format!("{OPENING_THREAT_MARK}{danger}（期限：{deadline}）");
        *conflict = json!(match text(conflict) {
            "" => line,
            before => format!("{before}；{line}"),
        });
    }
}

pub(crate) fn text(value: &Value) -> &str {
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

/// The creation ledger (CRAFT_V3_DESIGN §4.3): what the committed chapters
/// already used, rebuilt from their r2 plans in chapter order.
/// `prior` holds `(chapter_no, craft)` pairs; r1 chapters have no craft and
/// are simply absent. Only story-bible people count as introduced: walk-ons
/// stay in their scene.
pub fn creation_ledger(prior: &[(i64, Value)], bible: &Value) -> Value {
    let bible_people = bible_people(bible);
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
        "introducedPeople":people.into_iter().filter(|name| bible_people.contains(name.as_str())).collect::<Vec<_>>(),
        "losses":losses,
        "lastChapterPlaces":last_places
    })
}

/// Everyone the story bible names: protagonist, antagonist and cast.
fn bible_people(bible: &Value) -> BTreeSet<&str> {
    [&bible["protagonist"]["name"], &bible["antagonist"]["name"]]
        .into_iter()
        .chain(
            bible["cast"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|member| &member["name"]),
        )
        .map(text)
        .filter(|name| !name.is_empty())
        .collect()
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
    // The plan shape, checked here rather than only by the contract so the
    // planner gets these back with the other problems.
    let scene_list = scenes(craft);
    if chapter == 1
        && !scene_list
            .first()
            .is_some_and(|scene| text(&scene["conflict"]).contains(OPENING_THREAT_MARK))
    {
        problems.push(
            "第一章需要迫在眉睫的威胁：在 openingThreat 里写明什么危险、多久之内会落到主角身上"
                .into(),
        );
    }
    if !SCENE_COUNT.contains(&scene_list.len()) {
        problems.push(format!(
            "需要 {} 到 {} 个场景，现在有 {} 个",
            SCENE_COUNT.start(),
            SCENE_COUNT.end(),
            scene_list.len()
        ));
    }
    for (index, scene) in scene_list.iter().enumerate() {
        let people = scene["people"].as_array().map_or(0, Vec::len);
        if people < MIN_SCENE_PEOPLE {
            problems.push(format!(
                "第 {} 场只有 {people} 个人物，每场至少两人在场且有冲突",
                index + 1
            ));
        }
        let events = scene["events"].as_array().map_or(0, Vec::len);
        if !SCENE_EVENTS.contains(&events) {
            problems.push(format!(
                "第 {} 场有 {events} 条事件，每场写 {} 到 {} 条",
                index + 1,
                SCENE_EVENTS.start(),
                SCENE_EVENTS.end()
            ));
        }
    }
    if !matches!(text(&craft["device"]["status"]), "NEW" | "ESCALATED") {
        problems.push("异常手法的 status 只能是 NEW 或 ESCALATED".into());
    }
    if text(&craft["situationChange"]["before"]) == text(&craft["situationChange"]["after"]) {
        problems.push("处境变化前后相同，本章结束时主角的处境必须变了".into());
    }
    let device = text(&craft["device"]["name"]);
    let earlier_uses = ledger["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| text(&item["name"]) == device)
        .collect::<Vec<_>>();
    let used_before = !earlier_uses.is_empty();
    match text(&craft["device"]["status"]) {
        "NEW" if used_before => problems.push(format!(
            "异常手法「{device}」之前已经用过，标为新手法不对；要么换新手法，要么写清楚怎么升级"
        )),
        "ESCALATED" if !used_before => {
            problems.push(format!("异常手法「{device}」之前没用过，不能标为升级"))
        }
        _ => {}
    }
    if earlier_uses
        .iter()
        .any(|item| text(&item["howUsed"]) == text(&craft["device"]["howUsed"]))
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
    // Owner decision (2026-10-25): earlier places may come back as long as
    // the chapter goes somewhere new.
    let places = scenes(craft)
        .iter()
        .map(|scene| text(&scene["place"]))
        .collect::<BTreeSet<_>>();
    if !last_places.is_empty() && places.is_subset(&last_places) {
        problems.push(format!(
            "本章的地点（{}）上一章都去过，至少换一个新地点",
            places.into_iter().collect::<Vec<_>>().join("、")
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
    // Owner decision (2026-10-24): walk-ons outside the bible may appear;
    // bible cast still waits for the chapter it is scheduled to appear in.
    let on_stage = scene_people(craft)
        .map(|person| text(&person["name"]))
        .collect::<BTreeSet<_>>();
    for member in bible["cast"].as_array().into_iter().flatten() {
        let first = member["firstAppears"].as_i64().unwrap_or(1);
        let name = text(&member["name"]);
        if first > chapter && on_stage.contains(name) {
            problems.push(format!(
                "人物「{name}」在故事底稿里安排到第 {first} 章才出场，本章不能提前出现；路人可以不在底稿里"
            ));
        }
    }
    if chapter == 3 {
        let mut all = Vec::new();
        crate::chapter_craft::collect_strings(craft, &mut all);
        let antagonist = text(&bible["antagonist"]["name"]);
        if !antagonist.is_empty() && !all.iter().any(|item| item.contains(antagonist)) {
            problems.push(format!(
                "第 3 章要写明主角的具体行动以及他和对手「{antagonist}」的关系怎么变了"
            ));
        }
    }
    problems
}

/// Takes the quote marks out of every craft value (owner decision
/// 2026-10-24): a planner quoting a word is not a reason to plan again.
pub fn strip_plan_quotes(plan: &mut Value) {
    fn strip(value: &mut Value) {
        match value {
            Value::String(text) if text.contains(QUOTE_MARKS) => {
                *text = text.replace(QUOTE_MARKS, "");
            }
            Value::Array(items) => items.iter_mut().for_each(strip),
            Value::Object(map) => map.values_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut plan["craft"]);
}

/// `Err(B08_PLAN_CRAFT:…)` listing every problem, or `Ok`.
pub fn check_craft_plan(
    plan: &Value,
    chapter: i64,
    bible: &Value,
    ledger: &Value,
) -> Result<(), String> {
    craft_result(plan_problems(plan, chapter, bible, ledger))
}

fn craft_result(problems: Vec<String>) -> Result<(), String> {
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

/// The ledger as the planner sees it: each device once with its latest use,
/// the recent endings and losses, everyone introduced and the last places.
/// It stays bounded however long the book gets.
pub fn prompt_ledger(ledger: &Value) -> Value {
    let recent = |key: &str| {
        let items = ledger[key].as_array().map(Vec::as_slice).unwrap_or(&[]);
        json!(items[items.len().saturating_sub(PROMPT_LEDGER_RECENT)..])
    };
    let mut devices: Vec<Value> = Vec::new();
    for item in ledger["devices"].as_array().into_iter().flatten() {
        devices.retain(|kept| kept["name"] != item["name"]);
        devices.push(item.clone());
    }
    json!({
        "devices":devices,
        "endings":recent("endings"),
        "introducedPeople":ledger["introducedPeople"],
        "losses":recent("losses"),
        "lastChapterPlaces":ledger["lastChapterPlaces"]
    })
}

/// The story-bible fields the planner reads; the rest is metadata.
const BIBLE_CONTENT: &[&str] = &[
    "normalWorld",
    "mechanism",
    "protagonist",
    "emotionalAnchor",
    "antagonist",
    "cast",
    "openingArc",
];

/// What a CRAFT v3 chapter is planned against: the confirmed story bible and
/// the ledger of the chapters before it.
struct CraftPlanning {
    bible: Value,
    ledger: Value,
    established_facts: Vec<Value>,
}

/// The continuity details of the committed chapters before `chapter_id`.
fn prior_facts(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    chapter_id: &str,
) -> Result<Vec<(i64, Value)>, String> {
    crate::repositories::craft::prior_continuity_details(connection, project_id, chapter_id)
}

/// The names of the people and places a plan puts on stage.
fn plan_focus(plan: &Value) -> Vec<String> {
    let craft = &plan["craft"];
    scenes(craft)
        .iter()
        .map(|scene| text(&scene["place"]).to_string())
        .chain(scene_people(craft).map(|person| text(&person["name"]).to_string()))
        .collect()
}

/// How one chapter is planned: as r2 once the project has a confirmed story
/// bible, otherwise as r1. Both planning entry points use it the same way.
pub struct ChapterPlanner {
    craft: Option<CraftPlanning>,
    chapter: u32,
}

/// The content fields of a confirmed project-level head, or `None` before one
/// is confirmed.
fn confirmed_content(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    artifact_type: &str,
    fields: &[&str],
) -> Result<Option<Value>, String> {
    Ok(
        crate::repositories::project_entry::canonical_head(connection, project_id, artifact_type)
            .map_err(|error| error.to_string())?
            .map(|head| {
                Value::Object(
                    fields
                        .iter()
                        .map(|field| (field.to_string(), head.payload[*field].clone()))
                        .collect(),
                )
            }),
    )
}

/// The style-card fields the writer follows; the rest is metadata.
const STYLE_CARD_CONTENT: &[&str] = &[
    "narration",
    "targets",
    "openingStructure",
    "rules",
    "forbidden",
    "examples",
];

/// What a CRAFT v3 chapter is written with: the confirmed story bible and,
/// when confirmed, the style card (CRAFT_V3_DESIGN §4.4).
pub struct WritingContext {
    pub bible: Value,
    pub style_card: Option<Value>,
    facts: Vec<(i64, Value)>,
    chapter: u32,
}

impl WritingContext {
    pub fn for_chapter(
        connection: &libsql_rusqlite::Connection,
        project_id: &str,
        chapter_id: &str,
        chapter_number: Option<u32>,
    ) -> Result<Self, String> {
        let bible = confirmed_content(connection, project_id, "StoryBible", BIBLE_CONTENT)?
            .ok_or_else(|| "B08_STORY_BIBLE_REQUIRED".to_string())?;
        let style_card =
            confirmed_content(connection, project_id, "StyleCard", STYLE_CARD_CONTENT)?;
        Ok(Self {
            bible,
            style_card,
            facts: prior_facts(connection, project_id, chapter_id)?,
            chapter: chapter_number.unwrap_or(1),
        })
    }

    /// Adds what the craft writer reads to a render request.
    pub fn extend_render_input(&self, input: &mut Value, recent_chapters: &[Value]) {
        input["storyBible"] = self.bible.clone();
        if let Some(card) = &self.style_card {
            input["styleCard"] = card.clone();
        }
        input["chapterNumber"] = json!(self.chapter);
        input["recentChapters"] = json!(recent_chapters);
        input["establishedFacts"] = json!(established_facts(
            &self.facts,
            &plan_focus(&input["chapterPlan"])
        ));
    }
}

impl ChapterPlanner {
    pub fn for_chapter(
        connection: &libsql_rusqlite::Connection,
        project_id: &str,
        chapter_id: &str,
        chapter_number: Option<u32>,
    ) -> Result<Self, String> {
        let chapter = chapter_number.unwrap_or(1);
        let Some(bible) = confirmed_content(connection, project_id, "StoryBible", BIBLE_CONTENT)?
        else {
            return Ok(Self {
                craft: None,
                chapter,
            });
        };
        let prior =
            crate::repositories::craft::prior_plan_crafts(connection, project_id, chapter_id)
                .map_err(|error| error.to_string())?;
        let ledger = creation_ledger(&prior, &bible);
        Ok(Self {
            craft: Some(CraftPlanning {
                bible,
                ledger,
                established_facts: established_facts(
                    &prior_facts(connection, project_id, chapter_id)?,
                    &[],
                ),
            }),
            chapter,
        })
    }

    pub fn capability(&self) -> &'static str {
        if self.craft.is_some() {
            PLAN_CHAPTER_CRAFT
        } else {
            "PLAN_STRATEGY"
        }
    }

    /// Adds the story bible, the planner's view of the ledger and the chapter
    /// number to an r2 planning request.
    pub fn extend_input(&self, input: &mut Value) {
        if let Some(craft) = &self.craft {
            input["storyBible"] = craft.bible.clone();
            input["creationLedger"] = prompt_ledger(&craft.ledger);
            input["establishedFacts"] = json!(craft.established_facts);
            input["chapterNumber"] = json!(self.chapter);
        }
    }

    /// The r2 checks against the whole ledger, with the horizon count among
    /// the problems so one replan fixes all of them (a plan without craft
    /// decisions is sent back too); nothing for r1 planning. Run before the
    /// contract so shape problems reach the planner for a replan.
    pub fn check_craft(&self, plan: &Value, horizon_capacity: i64) -> Result<(), String> {
        let Some(craft) = &self.craft else {
            return Ok(());
        };
        if !crate::chapter_craft::is_craft_plan(plan) {
            return Err(format!(
                "{PLAN_CRAFT_ERROR}:计划缺少 craft 写作决定，请按 chapter-plan.r2 返回"
            ));
        }
        let mut problems =
            plan_problems(plan, i64::from(self.chapter), &craft.bible, &craft.ledger);
        if crate::planning::validate_chapter_plan_horizon(plan, horizon_capacity).is_err() {
            let (minimum, capacity) = crate::planning::horizon_slot_range(horizon_capacity);
            problems.push(format!(
                "horizonPreview 要有 {minimum} 到 {capacity} 条，每条都是 {{\"objective\":string,\"tentative\":true}}，这一版有 {} 条",
                plan["horizonPreview"].as_array().map_or(0, Vec::len)
            ));
        }
        craft_result(problems)
    }

    /// Runs `r1_check` for r1 planning only.
    pub fn check_r1(&self, r1_check: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        if self.craft.is_some() {
            Ok(())
        } else {
            r1_check()
        }
    }
}

/// Kinds of continuity detail the audit records for CRAFT v3 chapters
/// (CRAFT_V3_DESIGN §4.3): what later chapters must keep the same.
pub const CONTINUITY_KINDS: &[&str] = &[
    "HABIT",
    "PREFERENCE",
    "APPEARANCE",
    "OBJECT",
    "PLACE",
    "RELATION",
];
const MAX_DETAILS_PER_CHAPTER: usize = 15;
/// How many established facts the planner and the writer are given.
pub const MAX_ESTABLISHED_FACTS: usize = 40;

/// The continuity details of an audit reply that are complete, of a known
/// kind and quoted from the chapter body (whitespace ignored).
pub fn continuity_details(audit: &Value, body: &str) -> Vec<Value> {
    let squeeze = |text: &str| {
        text.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
    };
    let body = squeeze(body);
    audit["continuityDetails"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| CONTINUITY_KINDS.contains(&text(&item["kind"])))
        .filter(|item| !text(&item["subject"]).is_empty() && !text(&item["detail"]).is_empty())
        .filter(|item| {
            let quote = squeeze(text(&item["quote"]));
            !quote.is_empty() && body.contains(&quote)
        })
        .take(MAX_DETAILS_PER_CHAPTER)
        .map(|item| json!({"subject":text(&item["subject"]),"kind":text(&item["kind"]),"detail":text(&item["detail"])}))
        .collect()
}

/// The established facts for the next chapter: the earlier chapters' details,
/// newest first and without duplicates, those about `focus` (the people and
/// places of this chapter) ahead of the rest, at most `MAX_ESTABLISHED_FACTS`.
pub fn established_facts(prior: &[(i64, Value)], focus: &[String]) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    let mut facts = prior
        .iter()
        .rev()
        .flat_map(|(chapter, details)| {
            details
                .as_array()
                .into_iter()
                .flatten()
                .map(move |detail| (chapter, detail))
        })
        .filter(|(_, detail)| {
            seen.insert((
                text(&detail["subject"]).to_string(),
                text(&detail["kind"]).to_string(),
                text(&detail["detail"]).to_string(),
            ))
        })
        .map(|(chapter, detail)| {
            json!({"chapter":chapter,"subject":detail["subject"],"kind":detail["kind"],"detail":detail["detail"]})
        })
        .collect::<Vec<_>>();
    facts.sort_by_key(|fact| !focus.iter().any(|name| name == text(&fact["subject"])));
    facts.truncate(MAX_ESTABLISHED_FACTS);
    facts
}
