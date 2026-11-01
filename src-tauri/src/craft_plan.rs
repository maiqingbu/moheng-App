//! CRAFT v3 chapter plans (`chapter-plan.r2`, CRAFT_V3_DESIGN §4.2–4.3): the
//! deterministic plan checks, the creation ledger rebuilt from earlier
//! chapters' plans, and which skill bound which plan decision.

use crate::chapter_length::ChapterLengthRange;
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
/// LD-5 (owner decision 2026-11-01): bounds only; how many scenes and
/// events a chapter has follows from what it has to do.
pub(crate) const SCENE_COUNT: std::ops::RangeInclusive<usize> = 1..=3;
/// LD-6: one person alone is a scene too (a search, a flight, reasoning).
pub(crate) const MIN_SCENE_PEOPLE: usize = 1;
pub(crate) const SCENE_EVENTS: std::ops::RangeInclusive<usize> = 1..=5;
/// What is wrong with the device a plan uses, against the earlier uses.
fn device_problems(planned: &Value, ledger: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    let device = text(&planned["name"]);
    let earlier_uses = ledger["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| text(&item["name"]) == device)
        .collect::<Vec<_>>();
    let used_before = !earlier_uses.is_empty();
    match text(&planned["status"]) {
        "NEW" if used_before => problems.push(format!(
            "异常手法「{device}」之前已经用过，标为新手法不对；要么换新手法，要么写清楚怎么升级"
        )),
        "ESCALATED" if !used_before => {
            problems.push(format!("异常手法「{device}」之前没用过，不能标为升级"))
        }
        "NEW" | "ESCALATED" => {}
        _ => problems.push("异常手法的 status 只能是 NEW 或 ESCALATED".into()),
    }
    if earlier_uses
        .iter()
        .any(|item| text(&item["howUsed"]) == text(&planned["howUsed"]))
    {
        problems.push(format!(
            "异常手法「{device}」的用法和之前一模一样，需要新的变化"
        ));
    }
    problems
}

/// LD-6: a beat the model sent empty (blank text, null, an empty list or
/// an object of blanks) was not chosen, so it leaves the plan.
pub fn drop_unchosen_beats(plan: &mut Value) {
    fn blank(value: &Value) -> bool {
        match value {
            Value::Null => true,
            Value::String(text) => text.trim().is_empty(),
            Value::Array(items) => items.iter().all(blank),
            Value::Object(fields) => fields.values().all(blank),
            _ => false,
        }
    }
    if let Some(craft) = plan.get_mut("craft").and_then(Value::as_object_mut) {
        craft.retain(|beat, value| !(CHOSEN_BEATS.contains(&beat.as_str()) && blank(value)));
    }
}

/// LD-6 (owner decision 2026-11-01): the beats a plan chooses for its
/// chapter as it needs them; situationChange, scenes and endHook every
/// chapter has.
const CHOSEN_BEATS: [&str; 7] = [
    "bigTurn",
    "smallDiscovery",
    "protagonistWin",
    "device",
    "warmth",
    "emotionalBeat",
    "newLosses",
];
/// LD-5: the least room an event needs written out with action and
/// dialogue (the planner is told 250 and much more for a confrontation);
/// less and the scene reads as a list (real run 2026-11-01: fifteen events
/// in 2,000 characters).
pub(crate) const MIN_CHARS_PER_EVENT: i64 = 250;

/// QC-1.2: what marks chapter 1's imminent threat in its first scene.
pub(crate) const OPENING_THREAT_MARK: &str = "迫近的威胁：";
/// QC-1.2: what marks the threat's deadline and how the opening shows it.
pub(crate) const THREAT_DEADLINE: &str = "期限：";
pub(crate) const THREAT_ON_PAGE: &str = "开头写法：";
/// QC-1.2: within how many characters chapter 1 shows its threat (the
/// writer's prompt says 600 too).
pub(crate) const OPENING_THREAT_WITHIN: usize = 600;

/// QC-1 (acceptance 2026-10-26, fourth run: a scene without its
/// readerQuestion failed the plan contract): the text fields the plan
/// contract requires besides those `plan_problems` judges for substance,
/// each as `(pointer, what it is)`, scene and person fields included.
fn required_plan_texts(plan: &Value) -> Vec<(String, String)> {
    let mut fields = vec![(
        "/chapterGoal".to_string(),
        "本章目标（chapterGoal）".to_string(),
    )];
    if plan["craft"].get("device").is_some() {
        fields.push((
            "/craft/device/name".into(),
            "异常手法的名字（device.name）".into(),
        ));
    }
    for (index, scene) in scenes(&plan["craft"]).iter().enumerate() {
        let at = |field: &str| format!("/craft/scenes/{index}/{field}");
        let number = index + 1;
        fields.push((at("place"), format!("第 {number} 场的地点")));
        fields.push((at("conflict"), format!("第 {number} 场的冲突")));
        fields.push((
            at("readerQuestion"),
            format!("第 {number} 场留给读者的问题"),
        ));
        for (person_index, person) in scene["people"].as_array().into_iter().flatten().enumerate() {
            let name = text(&person["name"]);
            let who = if name.is_empty() {
                "一个人物".to_string()
            } else {
                format!("人物「{name}」")
            };
            for (field, label) in [
                ("name", "名字"),
                ("wants", "想要的"),
                ("hides", "隐瞒的"),
                ("whyHere", "在场理由"),
            ] {
                fields.push((
                    at(&format!("people/{person_index}/{field}")),
                    format!("第 {number} 场{who}的{label}"),
                ));
            }
        }
    }
    fields
}

/// Whether the plan's text at `pointer` is missing or empty.
fn blank(plan: &Value, pointer: &str) -> bool {
    plan.pointer(pointer).map(text).unwrap_or("").is_empty()
}

/// The required plan fields the planner left out or empty, as problems.
fn missing_plan_fields(plan: &Value) -> Vec<String> {
    required_plan_texts(plan)
        .into_iter()
        .filter(|(pointer, _)| blank(plan, pointer))
        .map(|(_, label)| format!("{label}没写"))
        .collect()
}

/// QC-1: the last lenient pass fills what the planner left out, so the
/// plan keeps its contract; returns what it filled, for the plan reminder.
pub(crate) fn complete_plan_fields(plan: &mut Value) -> Vec<String> {
    // Only an r2 plan has craft decisions to complete; indexing an r1 plan
    // mutably would add a `craft: null` its contract refuses.
    if !plan.get("craft").is_some_and(Value::is_object) {
        return Vec::new();
    }
    let mut filled = Vec::new();
    for (pointer, label) in required_plan_texts(plan) {
        if !blank(plan, &pointer) {
            continue;
        }
        let (parent, field) = pointer.rsplit_once('/').expect("a pointer has a parent");
        let fallback = match field {
            "chapterGoal" => match text(&plan["craft"]["bigTurn"]) {
                "" => text(&plan["craft"]["situationChange"]["after"]),
                turn => turn,
            },
            "readerQuestion" => text(&plan["craft"]["endHook"]),
            _ => "",
        };
        let value = Some(fallback)
            .filter(|fallback| !fallback.is_empty())
            .unwrap_or("（规划未写明）")
            .to_owned();
        if let Some(object) = plan.pointer_mut(parent).and_then(Value::as_object_mut) {
            object.insert(field.to_string(), json!(value));
            filled.push(label);
        }
    }
    filled
}

/// Words that make a deadline a point in story time on their own.
const DEADLINE_WORDS: [&str; 16] = [
    "今晚", "今天", "今夜", "明天", "明早", "明晚", "后天", "天亮", "天黑", "午夜", "凌晨", "黄昏",
    "月底", "周末", "日落", "日出",
];

/// Whether `deadline` names a point in story time: a number of days, hours
/// or the like (三天后, 十二点前, 两小时内) or a word such as 今晚.
fn names_a_time(deadline: &str) -> bool {
    let chars = deadline.chars().collect::<Vec<_>>();
    chars.windows(2).any(|pair| {
        crate::chapter_craft::is_numeral(pair[0]) && "天日时点分秒周月年刻小个".contains(pair[1])
    }) || DEADLINE_WORDS.iter().any(|word| deadline.contains(word))
}

/// QC-1.2 (owner decision 2026-10-26; fifth acceptance run: a threat that
/// was a state, "失去现实存在感", never reached the page): the planner's
/// `openingThreat` for chapter 1 is what happens to the protagonist
/// (`event`, or `danger` in earlier plans), by when (`deadline`, a point in
/// story time) and how the opening shows it (`onPage`: a notice, a call,
/// someone saying it). It joins the first scene's conflict, where the
/// writer reads it, so the plan keeps its contract; a threat missing any of
/// the three is left out, so the check sends the plan back for it.
pub fn fold_opening_threat(plan: &mut Value) {
    let Some(threat) = plan
        .as_object_mut()
        .and_then(|object| object.remove("openingThreat"))
    else {
        return;
    };
    let event = match text(&threat["event"]) {
        "" => text(&threat["danger"]),
        event => event,
    };
    let (deadline, on_page) = (text(&threat["deadline"]), text(&threat["onPage"]));
    if event.chars().count() < MIN_DECISION_CHARS
        || on_page.chars().count() < MIN_DECISION_CHARS
        || !names_a_time(deadline)
    {
        return;
    }
    if let Some(conflict) = plan.pointer_mut("/craft/scenes/0/conflict") {
        let line = format!(
            "{OPENING_THREAT_MARK}{event}（{THREAT_DEADLINE}{deadline}；{THREAT_ON_PAGE}{on_page}）"
        );
        *conflict = json!(match text(conflict) {
            "" => line,
            before => format!("{before}；{line}"),
        });
    }
}

pub(crate) fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("").trim()
}

/// How many events a planned scene has.
pub(crate) fn event_count(scene: &Value) -> usize {
    scene["events"].as_array().map_or(0, Vec::len)
}

fn scenes(craft: &Value) -> &[Value] {
    craft["scenes"].as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn scene_people(craft: &Value) -> impl Iterator<Item = &Value> {
    scenes(craft)
        .iter()
        .flat_map(|scene| scene["people"].as_array().into_iter().flatten())
}

/// The beats `craft` chose, in `CHOSEN_BEATS` order.
fn chosen_beats(craft: &Value) -> Vec<&'static str> {
    CHOSEN_BEATS
        .into_iter()
        .filter(|beat| craft.get(*beat).is_some())
        .collect()
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
    let mut shapes = Vec::new();
    for (chapter, craft) in prior {
        shapes.push(json!({"chapter":chapter,
            "beats":chosen_beats(craft),
            "scenes":scenes(craft).iter().map(|scene| json!({
            "events":event_count(scene),
            "estimatedCharacters":scene["estimatedCharacters"]
        })).collect::<Vec<_>>()}));
        if craft.get("device").is_some() {
            devices.push(json!({"chapter":chapter,"name":text(&craft["device"]["name"]),"howUsed":text(&craft["device"]["howUsed"])}));
        }
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
        "lastChapterPlaces":last_places,
        "shapes":shapes
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
    // LD-6 (owner decision 2026-11-01): a chapter always changes the
    // situation and ends on a hook; a chosen beat (`Some`) is checked only
    // when the plan has it, except the win chapters 1-3 take from their
    // opening arc.
    for (beat, path, label) in [
        (Some("bigTurn"), "/bigTurn", "大转折"),
        (Some("smallDiscovery"), "/smallDiscovery", "小发现"),
        (Some("protagonistWin"), "/protagonistWin", "主角赢的一局"),
        (Some("warmth"), "/warmth", "温度或幽默"),
        (Some("emotionalBeat"), "/emotionalBeat", "情感时刻"),
        (None, "/endHook", "章末钩子"),
        (None, "/situationChange/before", "处境变化（前）"),
        (None, "/situationChange/after", "处境变化（后）"),
        (Some("device"), "/device/howUsed", "异常手法的用法"),
    ] {
        let opening_win = beat == Some("protagonistWin") && chapter <= 3;
        if beat.is_some_and(|beat| craft.get(beat).is_none()) {
            if opening_win {
                problems.push(format!("{label}没写：前三章要写开局走向里这一章赢的那一局"));
            }
            continue;
        }
        if craft.pointer(path).map(text).unwrap_or("").chars().count() < MIN_DECISION_CHARS {
            problems.push(format!(
                "{label}写得太空，需要具体到人和事（至少 {MIN_DECISION_CHARS} 个字）"
            ));
        }
    }
    // The plan shape, checked here rather than only by the contract so the
    // planner gets these back with the other problems.
    problems.extend(missing_plan_fields(plan));
    let scene_list = scenes(craft);
    if chapter == 1
        && !scene_list
            .first()
            .is_some_and(|scene| text(&scene["conflict"]).contains(OPENING_THREAT_MARK))
    {
        problems.push(
            "第一章需要迫在眉睫的威胁：openingThreat 写明 event（外面发生、落到主角身上的具体事，谁要对他做什么，不是身体或心里的状态）、deadline（具体到故事里的时间，如今晚十二点前、三天后）、onPage（开头怎么写出来：一则通知、一通电话或某人当面说出）"
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
            problems.push(format!("第 {} 场没有人物", index + 1));
        }
        let events = event_count(scene);
        if !SCENE_EVENTS.contains(&events) {
            problems.push(format!(
                "第 {} 场有 {events} 条事件，每场写 {} 到 {} 条",
                index + 1,
                SCENE_EVENTS.start(),
                SCENE_EVENTS.end()
            ));
        }
    }
    // LD-7 (real run 2026-11-01, round 11): every chapter came out as three
    // scenes of four events with all seven beats; a chapter shaped exactly
    // like the two before it is planned again.
    let beats = chosen_beats(craft);
    let events = scene_list
        .iter()
        .map(|scene| event_count(scene) as u64)
        .collect::<Vec<_>>();
    let shapes = ledger["shapes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if shapes.len() >= 2
        && shapes[shapes.len() - 2..].iter().all(|earlier| {
            earlier["beats"] == json!(beats)
                && earlier["scenes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|scene| scene["events"].as_u64())
                    .eq(events.iter().copied().map(Some))
        })
    {
        problems.push(format!(
            "本章和前两章是同一个形状（{} 个场景，各场 {} 条事件，同样的 {} 个要素）：按这一章要做的事重新挑选要素、场景和事件",
            scene_list.len(),
            events.iter().map(u64::to_string).collect::<Vec<_>>().join("、"),
            beats.len()
        ));
    }
    if text(&craft["situationChange"]["before"]) == text(&craft["situationChange"]["after"]) {
        problems.push("处境变化前后相同，本章结束时主角的处境必须变了".into());
    }
    if let Some(planned) = craft.get("device") {
        problems.extend(device_problems(planned, ledger));
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

/// LD-5: what is wrong with the room `craft`'s scenes are estimated to
/// take: each scene states its estimate, the estimates add up to the
/// chapter's range, and neither the chapter nor a scene has more events
/// than its room holds.
pub fn sizing_problems(craft: &Value, range: ChapterLengthRange) -> Vec<String> {
    let mut problems = Vec::new();
    // LD-7 (real run 2026-11-01, round 11): told a scene was too crowded,
    // the planner raised its estimates and kept every event, so a chapter
    // with more events than its length holds is told how many to drop.
    let events = scenes(craft).iter().map(event_count).sum::<usize>() as i64;
    let room = range.max / MIN_CHARS_PER_EVENT;
    let over_room = events > room;
    if over_room {
        problems.push(format!(
            "本章 {events} 条事件，{} 字最多写得开 {room} 条（每条 {MIN_CHARS_PER_EVENT} 字以上）：删去或合并 {} 条，留给后面的章节；提高预估字数解决不了",
            range.max,
            events - room
        ));
    }
    let mut total = 0;
    for (index, scene) in scenes(craft).iter().enumerate() {
        let Some(estimate) = scene["estimatedCharacters"]
            .as_i64()
            .filter(|chars| *chars > 0)
        else {
            problems.push(format!(
                "第 {} 场没有预估字数（estimatedCharacters：这场写开要多少字）",
                index + 1
            ));
            continue;
        };
        total += estimate;
        let events = event_count(scene) as i64;
        if !over_room && events > 0 && estimate / events < MIN_CHARS_PER_EVENT {
            problems.push(format!(
                "第 {} 场预估 {estimate} 字写 {events} 条事件，太挤：每条事件写开要 {MIN_CHARS_PER_EVENT} 字以上，删掉一些或留给后面的章节",
                index + 1
            ));
        }
    }
    if problems.is_empty() && !(range.min..=range.max).contains(&total) {
        problems.push(format!(
            "各场预估合计 {total} 字，本章写 {} 到 {} 字：按这章要做的事重新估",
            range.min, range.max
        ));
    }
    problems
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
                    // LD-6: a beat the chapter did not choose was decided by no one.
                    fields
                        .iter()
                        .map(|field| (field.to_string(), decision(craft, field)))
                        .filter(|(_, decided)| !decided.is_null())
                        .collect::<serde_json::Map<_, _>>()
                })
                .filter(|decisions| !decisions.is_empty())
                .map(|decisions| json!({"skillId":skill_id,"decisions":decisions}))
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
        "lastChapterPlaces":ledger["lastChapterPlaces"],
        "shapes":recent("shapes")
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

/// WM-1: the established facts for a chapter, recalled from the work's
/// memory by `craft` (a chapter's craft decisions): its people and places
/// first, then what it brings back, then the newest of the rest, at most
/// `MAX_ESTABLISHED_FACTS`. An absent person's own traits are left out only
/// when `people` names who counts as a person.
fn recall_for(
    memories: &[crate::work_memory::Memory],
    craft: &Value,
    people: &[String],
) -> Vec<Value> {
    let on_stage = scenes(craft)
        .iter()
        .map(|scene| text(&scene["place"]).to_string())
        .chain(scene_people(craft).map(|person| text(&person["name"]).to_string()))
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();
    let plan_text = if craft.is_null() {
        String::new()
    } else {
        craft.to_string()
    };
    crate::work_memory::recall(
        memories,
        &crate::work_memory::RecallQuery {
            on_stage: &on_stage,
            people,
            text: &plan_text,
        },
        MAX_ESTABLISHED_FACTS,
    )
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
    memories: Vec<crate::work_memory::Memory>,
    chapter: u32,
}

impl WritingContext {
    /// QC-1.4/1.5: the people of the chapter's scenes, and everyone the
    /// story bible names besides.
    pub fn chapter_people(&self, plan: &Value) -> (Vec<String>, Vec<String>) {
        let present = scene_people(&plan["craft"])
            .map(|person| text(&person["name"]).to_string())
            .filter(|name| !name.is_empty())
            .collect::<BTreeSet<_>>();
        let everyone = bible_people(&self.bible)
            .into_iter()
            .map(ToOwned::to_owned)
            .chain(present.iter().cloned())
            .collect::<BTreeSet<_>>();
        (
            present.into_iter().collect(),
            everyone.into_iter().collect(),
        )
    }

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
            memories: crate::repositories::work_memory::earlier_memories(
                connection, project_id, chapter_id,
            )?,
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
        // Who the story names as a person: their own traits come in only
        // when they are in the chapter.
        let (_, people) = self.chapter_people(&input["chapterPlan"]);
        input["establishedFacts"] = json!(recall_for(
            &self.memories,
            &input["chapterPlan"]["craft"],
            &people
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
                // The planner decides who appears, so everyone's traits may
                // come in, the last chapter's people first.
                established_facts: recall_for(
                    &crate::repositories::work_memory::earlier_memories(
                        connection, project_id, chapter_id,
                    )?,
                    prior.last().map_or(&Value::Null, |(_, craft)| craft),
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
    pub fn check_craft(
        &self,
        plan: &Value,
        horizon_capacity: i64,
        length_range: ChapterLengthRange,
    ) -> Result<(), String> {
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
        problems.extend(sizing_problems(&plan["craft"], length_range));
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
