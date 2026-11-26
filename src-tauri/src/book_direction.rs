//! VD-1a (VOLUME_DIRECTION_DESIGN §3.1): after signing, the model reads what
//! the three opening chapters actually did and sets the book's direction and
//! the first volume's stages. The approved BookPlan keeps them in its own
//! fields; a direction that still fails its checks after the feedback rounds
//! leaves the template in place with a reminder.

use crate::craft_plan::text;
use crate::production::ProductionModelGateway;
use crate::repositories::runtime::{DurableModelRunner, ModelRunSpec};
use crate::stable_id::{StableIdKind, StableIdentityFactory};
use libsql_rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

pub const PLAN_BOOK_DIRECTION: &str = "PLAN_BOOK_DIRECTION";
pub const CHECK_BOOK_CONTINUATION: &str = "CHECK_BOOK_CONTINUATION";
pub const CONFIRM_BOOK_CONTINUATION: &str = "CONFIRM_BOOK_CONTINUATION";
pub(crate) const REPAIR_BOOK_PRESSURE: &str = "REPAIR_BOOK_PRESSURE";
const BOOK_CONTINUATION_EVIDENCE_INVALID: &str = "B07_BOOK_CONTINUATION_EVIDENCE_INVALID";
const BOOK_PRESSURE_REPAIR_REQUIRED: &str = "B07_BOOK_PRESSURE_REPAIR_REQUIRED";
/// The most candidates one confirmation judges (its schema's limit).
const CONFIRM_CANDIDATES: usize = 20;

/// How much of the end of the opening the planner is shown.
const OPENING_ENDING_CHARS: usize = 800;

pub(crate) const BOOK_DIRECTION_REQUIRED: &str = "B07_BOOK_DIRECTION_REQUIRED";
pub(crate) const VOLUME_DIRECTION_ROUTE_REQUIRED: &str = "B07_VOLUME_DIRECTION_ROUTE_REQUIRED";

/// The error prefix of a direction sent back with its problems.
pub const DIRECTION_ERROR: &str = "B07_DIRECTION";

const NO_CHAPTER_TABLE: &str = "不要写章表，也不要写“第 N 章”：阶段按情节划分，不按章号";

/// How many stages a volume is split into (VD-1i: up to 8 for a long one).
const STAGE_COUNT: std::ops::RangeInclusive<usize> = 3..=8;
/// VD-1h: how many steps a stage is split into, each one or two chapters.
const STEP_COUNT: std::ops::RangeInclusive<usize> = 3..=5;
/// Round 11 (owner decision 2026-11-26): the most characters a step is
/// stated in. A step is where the story stands after it, not the actions
/// that get there (real run round 11: 40-character steps scripted every
/// chapter, and the chapters copied them).
const STEP_CHARS: usize = 30;

/// The problem of a step longer than `STEP_CHARS`, and whether a problem is
/// one: round 12, the planner left a few steps a little long round after
/// round, so on the last round they no longer fail the direction.
fn too_long_step_problem(location: &str, step: &str) -> String {
    format!(
        "{location}「{step}」超过 {STEP_CHARS} 字：只写这一步之后的局面（谁知道了什么、能做什么、和谁站在一起或翻脸），动作和物件留给章节"
    )
}

fn is_too_long_step(problem: &str) -> bool {
    problem.contains(&format!("」超过 {STEP_CHARS} 字："))
}

/// The direction step's input: the confirmed story, what the opening did,
/// the signing conclusions and the first volume's size.
pub fn input(
    bible: &Value,
    story_core: &Value,
    opening: &[(i64, Value)],
    signing: &Value,
    first_volume: (i64, i64),
    opening_chapters: i64,
) -> Value {
    let (start, end) = first_volume;
    json!({
        "storyBible":bible,
        "storyCore":story_core,
        "opening":opening.iter().map(|(chapter, craft)| json!({
            "chapter":chapter,
            "situationChange":craft["situationChange"],
            "bigTurn":craft["bigTurn"],
            "protagonistWin":craft["protagonistWin"],
            "endHook":craft["endHook"]
        })).collect::<Vec<_>>(),
        "signing":signing,
        "firstVolume":{
            "chapters":end - start + 1,
            "chaptersAfterOpening":end - opening_chapters,
            "stageChapters":{"min":least_chapters(end - opening_chapters),"max":end - opening_chapters}
        }
    })
}

fn items<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value[key].as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn names(values: &Value) -> impl Iterator<Item = &str> {
    values.as_array().into_iter().flatten().map(text)
}

/// Whether `text` names a chapter by number, as in 第12章 or 第十章.
fn names_a_chapter(text: &str) -> bool {
    text.split('第').skip(1).any(|rest| {
        let number = rest
            .chars()
            .take_while(|c| {
                c.is_ascii_digit() || c.is_whitespace() || "零一二三四五六七八九十百".contains(*c)
            })
            .collect::<String>();
        !number.trim().is_empty() && rest[number.len()..].starts_with('章')
    })
}

fn has_chapter_table(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key("chapters") || object.values().any(has_chapter_table)
        }
        Value::Array(values) => values.iter().any(has_chapter_table),
        Value::String(text) => names_a_chapter(text),
        _ => false,
    }
}

/// VD-1i: tidies a direction's volume seed before it is checked: drops
/// blank stages and scales the stage estimates into the chapters the
/// volume has, which the model kept missing.
pub fn tidy(direction: &mut Value, input: &Value) {
    if let Some(room) = input["firstVolume"]["chaptersAfterOpening"].as_i64() {
        let seeds = direction["volumeIntentSeeds"].as_array_mut();
        if let Some(first) = seeds
            .into_iter()
            .flatten()
            .find(|seed| seed["volumeNo"] == 1)
        {
            tidy_seed(first, room, &input["storyBible"]);
        }
    } else if let Some(room) = input["volume"]["chapters"].as_i64() {
        tidy_seed(direction, room, &input["storyBible"]);
    }
}

/// The people `bible` names: protagonist, antagonist and cast.
fn known_names(bible: &Value) -> Vec<&str> {
    [&bible["protagonist"]["name"], &bible["antagonist"]["name"]]
        .into_iter()
        .map(text)
        .chain(
            items(bible, "cast")
                .iter()
                .map(|member| text(&member["name"])),
        )
        .filter(|name| !name.is_empty())
        .collect()
}

/// The fewest chapters the stages of a volume with `room` chapters take.
fn least_chapters(room: i64) -> i64 {
    (room + 1) / 2
}

fn tidy_seed(seed: &mut Value, room: i64, bible: &Value) {
    let Some(stages) = seed["stages"].as_array_mut() else {
        return;
    };
    for stage in std::mem::take(stages) {
        if !text(&stage["goal"]).is_empty() && !stages.contains(&stage) {
            stages.push(stage);
        }
    }
    // VD-1j (real run round 22): a person outside the bible is new cast.
    let known = known_names(bible);
    if !known.is_empty() {
        for stage in stages.iter_mut() {
            let (cast, strangers): (Vec<Value>, Vec<Value>) = stage["cast"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .partition(|name| known.contains(&text(name)));
            if strangers.is_empty() {
                continue;
            }
            let mut new_cast = stage["newCast"].as_array().cloned().unwrap_or_default();
            for name in strangers {
                if !new_cast.contains(&name) {
                    new_cast.push(name);
                }
            }
            stage["cast"] = json!(cast);
            stage["newCast"] = json!(new_cast);
        }
    }
    let estimates = stages
        .iter()
        .map(|stage| stage["estimatedChapters"].as_i64().unwrap_or(1).max(1))
        .collect::<Vec<_>>();
    let total = estimates.iter().sum::<i64>();
    let least = least_chapters(room);
    if room <= 0 || total == 0 || (least..=room).contains(&total) {
        return;
    }
    let target = total.clamp(least, room).max(stages.len() as i64);
    let mut scaled = estimates
        .iter()
        .map(|estimate| (estimate * target / total).max(1))
        .collect::<Vec<_>>();
    // Rounding: the largest stages take or give the chapters left over.
    let mut order = (0..scaled.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| std::cmp::Reverse(estimates[*index]));
    let mut gap = target - scaled.iter().sum::<i64>();
    for index in order.iter().cycle().take(order.len() * 4) {
        match gap.signum() {
            1 => {
                scaled[*index] += 1;
                gap -= 1;
            }
            -1 if scaled[*index] > 1 => {
                scaled[*index] -= 1;
                gap += 1;
            }
            _ => {}
        }
    }
    for (stage, chapters) in stages.iter_mut().zip(scaled) {
        stage["estimatedChapters"] = json!(chapters);
    }
}

/// Verbs of speech that end the clause before a quoted line.
const SPEECH_VERBS: &[&str] = &[
    "说", "问", "喊", "答", "指出", "说出", "念出", "喊出", "说道", "问道", "提醒", "补充", "嘀咕",
    "低语",
];

/// Whether `step` quotes a line someone says (real run round 8: a step was
/// a line said while handing over a cloth): a verb of speech right before a
/// quote of a clause, not a quoted name such as 知道“丹房” or 名叫“青禾”.
fn says_a_line(step: &str) -> bool {
    let chars = step.chars().collect::<Vec<_>>();
    chars.iter().enumerate().any(|(index, c)| {
        let before = chars[..index].iter().collect::<String>();
        // Real run round 9: 吴幼安立刻指出‘……’ was a step too.
        let before = before.trim_end_matches(['：', ':', '，']);
        let spoken = SPEECH_VERBS.iter().any(|verb| before.ends_with(verb));
        let line = chars[index + 1..]
            .iter()
            .take_while(|c| !crate::craft_setup::QUOTE_MARKS.contains(c) && **c != '’')
            .collect::<String>();
        (crate::craft_setup::QUOTE_MARKS.contains(c) || *c == '‘')
            && spoken
            && (line.chars().count() >= 6 || line.contains(['，', '！', '？', '。']))
    })
}

/// The problems of a volume seed's `stages` and `climax`: 3 to 6 stages,
/// each with a goal, pressure and turn and cast from `bible`, estimates that
/// fill between half and all of the `room` chapters, and the antagonist
/// acting in at least half of them. `label` names the volume.
fn stage_problems(seed: &Value, bible: &Value, label: &str, room: (&str, i64)) -> Vec<String> {
    let mut problems = Vec::new();
    if text(&seed["climax"]).is_empty() {
        problems.push(format!("{label}要写 climax"));
    }
    let stages = items(seed, "stages");
    if !STAGE_COUNT.contains(&stages.len()) {
        problems.push(format!(
            "{label}要分 {} 到 {} 个阶段，现在是 {} 个",
            STAGE_COUNT.start(),
            STAGE_COUNT.end(),
            stages.len()
        ));
    }
    let known = known_names(bible);
    for (index, stage) in stages.iter().enumerate() {
        let number = index + 1;
        for field in ["goal", "pressure", "turn"] {
            if text(&stage[field]).is_empty() {
                problems.push(format!("第 {number} 个阶段缺 {field}"));
            }
        }
        let steps = names(&stage["steps"])
            .filter(|step| !step.is_empty())
            .count();
        if !STEP_COUNT.contains(&steps) {
            problems.push(format!(
                "第 {number} 个阶段要分 {} 到 {} 步（steps），现在是 {steps} 步",
                STEP_COUNT.start(),
                STEP_COUNT.end()
            ));
        }
        for (step_index, step) in names(&stage["steps"]).enumerate() {
            if says_a_line(step) {
                problems.push(format!(
                    "{}「{step}」只写了一句台词：一步写的是结果，这一步之后人物能做、知道或决定什么",
                    step_location(index, step_index as u64 + 1)
                ));
            } else if step.chars().count() > STEP_CHARS {
                problems.push(too_long_step_problem(
                    &step_location(index, step_index as u64 + 1),
                    step,
                ));
            }
        }
        let strangers = names(&stage["cast"])
            .filter(|name| !known.is_empty() && !known.contains(name))
            .collect::<Vec<_>>();
        if !strangers.is_empty() {
            problems.push(format!(
                "第 {number} 个阶段的 cast 里有故事圣经之外的人：{}，新人物写进 newCast",
                strangers.join("、")
            ));
        }
    }
    let estimated = stages
        .iter()
        .map(|stage| stage["estimatedChapters"].as_i64().unwrap_or(0))
        .sum::<i64>();
    let (room_label, room) = room;
    let least = least_chapters(room);
    if room > 0 && !(least..=room).contains(&estimated) {
        problems.push(format!(
            "各阶段 estimatedChapters 加起来是 {estimated} 章，{room_label} {room} 章，应在 {least} 到 {room} 之间"
        ));
    }
    if let Some(gap) = pressure_shortfall(stages, text(&bible["antagonist"]["name"])) {
        problems.push(gap.problem);
    }
    problems
}

struct PressureShortfall {
    needed_stages: usize,
    eligible_stages: Vec<usize>,
    problem: String,
}

fn pressure_shortfall(stages: &[Value], antagonist: &str) -> Option<PressureShortfall> {
    if antagonist.is_empty() {
        return None;
    }
    let eligible_stages = stages
        .iter()
        .enumerate()
        .filter_map(|(index, stage)| {
            (!text(&stage["pressure"]).contains(antagonist)).then_some(index + 1)
        })
        .collect::<Vec<_>>();
    let acting = stages.len() - eligible_stages.len();
    let minimum = stages.len().div_ceil(2);
    (acting < minimum).then(|| PressureShortfall {
        needed_stages: minimum - acting,
        eligible_stages,
        problem: format!(
            "反派{antagonist}只在 {acting} 个阶段的 pressure 里出手，至少要 {minimum} 个"
        ),
    })
}

fn apply_pressure_repairs(
    stages: &mut [Value],
    antagonist: &str,
    gap: &PressureShortfall,
    reply: &Value,
) -> Option<()> {
    let corrections = reply["pressureCorrections"].as_array()?;
    if corrections.len() != gap.needed_stages {
        return None;
    }
    let mut patched = BTreeSet::new();
    let edits = corrections
        .iter()
        .map(|item| {
            let number = usize::try_from(item["stage"].as_u64()?).ok()?;
            let index = number.checked_sub(1)?;
            let pressure = text(&item["pressure"]);
            let action = pressure.strip_prefix(antagonist)?.trim();
            (gap.eligible_stages.contains(&number) && patched.insert(number) && !action.is_empty())
                .then_some((index, pressure))
        })
        .collect::<Option<Vec<_>>>()?;
    for (index, pressure) in edits {
        stages[index]["pressure"] = json!(pressure);
    }
    Some(())
}

/// Everything wrong with `direction` for `input`, as feedback for the model.
pub fn problems(direction: &Value, input: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    if has_chapter_table(direction) {
        problems.push(NO_CHAPTER_TABLE.to_string());
    }
    if text(&direction["protagonistMacroDirection"]).is_empty() {
        problems.push("protagonistMacroDirection 要写主角整本书从哪里走到哪里".to_string());
    }
    if !items(direction, "majorMysteryGoals")
        .iter()
        .any(|mystery| mystery["answerInVolume"] == 1)
    {
        problems
            .push("majorMysteryGoals 至少要有一层在第一卷揭开（answerInVolume 为 1）".to_string());
    }
    if items(&input["storyCore"], "forbiddenReveals")
        .iter()
        .any(|item| !text(item).is_empty())
        && items(direction, "futureReservations").is_empty()
    {
        problems.push("storyCore.forbiddenReveals 的内容要放进 futureReservations，写明最早第几卷才能揭开（notBeforeVolumeNo）".to_string());
    }
    let Some(first) = items(direction, "volumeIntentSeeds")
        .iter()
        .find(|seed| seed["volumeNo"] == 1)
    else {
        problems.push("volumeIntentSeeds 缺第一卷（volumeNo 为 1）".to_string());
        return problems;
    };
    let room = input["firstVolume"]["chaptersAfterOpening"]
        .as_i64()
        .unwrap_or(0);
    problems.extend(stage_problems(
        first,
        &input["storyBible"],
        "第一卷",
        ("第一卷开篇之后还有", room),
    ));
    problems
}

/// Writes `direction` into the approved plan's goal fields; `prerequisites`
/// bind its reservations to the opening evidence. The system names every
/// item.
pub fn apply(plan: &mut Map<String, Value>, direction: &Value, prerequisites: &[String]) {
    let volume = |item: &Value, key: &str| item[key].as_i64().unwrap_or(1);
    plan.insert(
        "protagonistMacroDirection".into(),
        json!(text(&direction["protagonistMacroDirection"])),
    );
    let numbered = |key: &str, build: &dyn Fn(usize, &Value) -> Value| {
        Value::Array(
            items(direction, key)
                .iter()
                .enumerate()
                .map(|(index, item)| build(index + 1, item))
                .collect(),
        )
    };
    plan.insert(
        "mustAchieve".into(),
        numbered("mustAchieve", &|n, goal| {
            json!({"id":format!("goal:book:{n}"),"volumeNo":volume(goal, "volumeNo"),"description":text(&goal["description"])})
        }),
    );
    plan.insert(
        "relationshipMacroDirections".into(),
        numbered("relationshipMacroDirections", &|n, relation| {
            json!({"id":format!("relation:book:{n}"),"volumeNo":volume(relation, "volumeNo"),
                "description":format!("{}：从{}走向{}",
                    names(&relation["between"]).collect::<Vec<_>>().join("与"),
                    text(&relation["from"]), text(&relation["toward"]))})
        }),
    );
    plan.insert(
        "majorMysteryGoals".into(),
        numbered("majorMysteryGoals", &|n, mystery| {
            json!({"id":format!("mystery:book:{n}"),"volumeNo":volume(mystery, "answerInVolume"),
                "description":format!("第 {} 层：{}", volume(mystery, "layer"), text(&mystery["description"]))})
        }),
    );
    plan.insert(
        "futureReservations".into(),
        numbered("futureReservations", &|n, reservation| {
            let not_before = reservation["notBeforeVolumeNo"].as_i64().unwrap_or(2);
            json!({"reservationId":format!("reservation:book:{n}"),"description":text(&reservation["description"]),
                "notBeforeVolumeNo":not_before,"reservedForVolumeNo":not_before,"prerequisites":prerequisites,
                "mayForeshadow":true,"mayReveal":false,"authorityLevel":"B"})
        }),
    );
    plan.insert(
        "volumeIntentSeeds".into(),
        items(direction, "volumeIntentSeeds")
            .iter()
            .map(|seed| seed_from(seed, volume(seed, "volumeNo")))
            .collect::<Vec<_>>()
            .into(),
    );
}

/// The BookPlan seed of volume `volume_no` from a planned direction's seed:
/// its description, its stages named by the system, and its climax.
fn seed_from(seed: &Value, volume_no: i64) -> Value {
    let mut built =
        json!({"seedId":format!("volume-{volume_no}"),"description":text(&seed["description"])});
    let stages = items(seed, "stages");
    if !stages.is_empty() {
        built["stages"] = stages
            .iter()
            .enumerate()
            .map(|(index, stage)| {
                let mut built = json!({"id":format!("stage:{volume_no}:{}", index + 1),
                "goal":text(&stage["goal"]),"pressure":text(&stage["pressure"]),
                "cast":names(&stage["cast"]).collect::<Vec<_>>(),"turn":text(&stage["turn"]),
                "estimatedChapters":stage["estimatedChapters"].as_i64().unwrap_or(1).max(1)});
                let new_cast = names(&stage["newCast"]).collect::<Vec<_>>();
                if !new_cast.is_empty() {
                    built["newCast"] = json!(new_cast);
                }
                let steps = names(&stage["steps"])
                    .filter(|step| !step.is_empty())
                    .enumerate()
                    .map(|(step, goal)| {
                        json!({"id":format!("step:{volume_no}:{}:{}", index + 1, step + 1),"goal":goal})
                    })
                    .collect::<Vec<_>>();
                if !steps.is_empty() {
                    built["steps"] = json!(steps);
                }
                built
            })
            .collect();
    }
    if !text(&seed["climax"]).is_empty() {
        built["climax"] = json!(text(&seed["climax"]));
    }
    built
}

/// VD-2 (§7): the step that plans each later volume when it opens.
pub const PLAN_VOLUME_DIRECTION: &str = "PLAN_VOLUME_DIRECTION";

/// The volume direction step's input for volume `volume_no` (`chapters` its
/// first and last chapter): the story, what the book plans for it, its seed
/// so far and what the previous volume actually did (`previous`).
pub fn volume_input(
    bible: &Value,
    book_plan: &Value,
    volume_no: i64,
    chapters: (i64, i64),
    previous: &Value,
) -> Value {
    let (start, end) = chapters;
    let this_volume = |key: &str| {
        items(book_plan, key)
            .iter()
            .filter(|item| item["volumeNo"] == volume_no)
            .map(|item| text(&item["description"]))
            .collect::<Vec<_>>()
    };
    json!({
        "storyBible":bible,
        "book":{
            "protagonistMacroDirection":text(&book_plan["protagonistMacroDirection"]),
            "storyPromise":text(&book_plan["storyPromise"]),
            "goals":this_volume("mustAchieve"),
            "mysteries":this_volume("majorMysteryGoals"),
            "reservations":items(book_plan, "futureReservations").iter().map(|reservation| json!({
                "description":text(&reservation["description"]),
                "notBeforeVolumeNo":reservation["notBeforeVolumeNo"]
            })).collect::<Vec<_>>()
        },
        "seed":crate::planning::volume_seeds(book_plan, volume_no)
            .next()
            .map_or("", |seed| text(&seed["description"])),
        "previousVolume":previous,
        "volume":{"volumeNo":volume_no,"chapters":end - start + 1,
            "stageChapters":{"min":least_chapters(end - start + 1),"max":end - start + 1}}
    })
}

/// Everything wrong with a planned volume `direction` for `input`.
pub fn volume_problems(direction: &Value, input: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    if has_chapter_table(direction) {
        problems.push(NO_CHAPTER_TABLE.to_string());
    }
    if text(&direction["description"]).is_empty() {
        problems.push("description 要写这一卷对全书的作用".to_string());
    }
    let volume = &input["volume"];
    problems.extend(stage_problems(
        direction,
        &input["storyBible"],
        &format!("第 {} 卷", volume["volumeNo"]),
        ("这一卷有", volume["chapters"].as_i64().unwrap_or(0)),
    ));
    problems
}

/// `book_plan` with volume `volume_no`'s seed set from `direction`; the
/// other seeds and every other field stay.
pub fn with_volume_seed(book_plan: &Value, volume_no: i64, direction: &Value) -> Value {
    let mut plan = book_plan.clone();
    let seed = seed_from(direction, volume_no);
    if !plan["volumeIntentSeeds"].is_array() {
        plan["volumeIntentSeeds"] = json!([]);
    }
    let seeds = plan["volumeIntentSeeds"]
        .as_array_mut()
        .expect("seeds array");
    // The seed the compilers read for this volume (planning::volume_seeds).
    match seeds.iter_mut().find(|existing| {
        existing["seedId"]
            .as_str()
            .is_some_and(|id| crate::planning::seed_matches_volume(id, volume_no))
    }) {
        Some(existing) => *existing = seed,
        None => seeds.push(seed),
    }
    plan
}

/// What planning the direction gave: the direction and the run that wrote
/// it, or the reminder that the template stays. Gateways that do not plan
/// directions give neither.
#[derive(Default)]
pub struct Planned {
    pub direction: Option<Value>,
    pub model_run_id: Option<String>,
    pub reminder: Option<Value>,
}

/// What the signing evaluation concluded, for the direction step.
fn signing_conclusions(receipt: &Value) -> Value {
    let concerns = items(receipt, "itemResults")
        .iter()
        .filter(|item| item["result"] != "PASS")
        .map(|item| text(&item["reason"]))
        .collect::<Vec<_>>();
    let passed = items(receipt, "platformMatrix")
        .iter()
        .filter(|platform| platform["status"] == "PASS")
        .map(|platform| text(&platform["displayName"]))
        .collect::<Vec<_>>();
    json!({"verdict":text(&receipt["verdict"]),"passedPlatforms":passed,"concerns":concerns})
}

/// Asks the model for the book's direction from the opening and checks it,
/// sending its problems back up to the plan feedback rounds. An invalid
/// direction leaves the opening seed available for another attempt.
pub fn plan(
    connection: &Connection,
    gateway: &dyn ProductionModelGateway,
    project_id: &str,
    command_id: &str,
    idempotency_key: &str,
    now: &str,
) -> Result<Planned, String> {
    if !gateway.plans_book_direction() {
        return Ok(Planned::default());
    }
    let head = |artifact_type: &str| {
        crate::repositories::project_entry::canonical_head(connection, project_id, artifact_type)
            .map(|row| row.map(|row| row.payload).unwrap_or(Value::Null))
            .map_err(|error| error.to_string())
    };
    let bible = crate::craft_plan::confirmed_bible(connection, project_id)?.unwrap_or(Value::Null);
    let opening_chapters = crate::repositories::project_entry::OPENING_CHAPTER_COUNT;
    let opening = crate::repositories::craft::plan_crafts_before(
        connection,
        project_id,
        opening_chapters + 1,
    )
    .map_err(|error| error.to_string())?;
    let first_volume =
        crate::repositories::volume_lifecycle::VolumeLifecycleRepository::volume_chapter_range(
            connection, project_id, 1,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VOLUME_NOT_FOUND".to_string())?;
    let mut input = input(
        &bible,
        &head("StoryConstitution")?["storyCore"],
        &opening,
        &signing_conclusions(&head("SimulatedSigningReceipt")?),
        first_volume,
        opening_chapters,
    );
    let committed_opening =
        crate::repositories::narrative::NarrativeRepository::final_bodies_by_chapter(
            connection,
            project_id,
            opening_chapters,
        )
        .map_err(|error| error.to_string())?;
    let committed_opening = committed_opening
        .iter()
        .filter(|(chapter, _)| (1..=opening_chapters).contains(chapter))
        .collect::<Vec<_>>();
    // The planner is shown where the opening ends, not its scenes: given the
    // three chapters, it planned them again (real runs rounds 6 and 7).
    if let Some((chapter, body)) = committed_opening.last() {
        let characters = body.chars().count();
        input["openingEnding"] = json!({"chapterNo":chapter,
            "text":body.chars().skip(characters.saturating_sub(OPENING_ENDING_CHARS)).collect::<String>()});
    }
    let opening_prose = committed_opening
        .iter()
        .map(|(chapter, body)| json!({"chapterNo":chapter,"body":body.chars().take(4_000).collect::<String>()}))
        .collect::<Vec<_>>();
    let planned = run_direction(
        connection,
        gateway,
        (project_id, command_id, idempotency_key, now),
        (PLAN_BOOK_DIRECTION, "book-direction"),
        input,
        &opening_prose,
        problems,
    )?;
    match planned {
        Ok((direction, model_run_id)) => Ok(Planned {
            direction: Some(direction),
            model_run_id: Some(model_run_id),
            reminder: None,
        }),
        Err(reason) => Err(format!("{BOOK_DIRECTION_REQUIRED}:{reason}")),
    }
}

pub(crate) fn has_first_volume_direction(book: &Value) -> bool {
    first_volume_direction(book).is_some()
}

fn planned_outcome(target: &str) -> &str {
    // An explicit transition can make the preceding state a premise. Check
    // the consequence rather than treating the premise as the planned result.
    let outcome = [
        "但是", "但", "却", "反而", "转而", "随后", "由此", "从而", "进而", "因此",
    ]
    .into_iter()
    .filter_map(|marker| target.rfind(marker).map(|index| index + marker.len()))
    .max()
    .map(|start| target[start..].trim())
    .filter(|outcome| !outcome.is_empty())
    .unwrap_or(target);
    // Ordered preparation, travel, and the ensuing finding can occur in
    // the same step. The finding at the end is the first thing to compare.
    outcome
        .trim_end_matches(['。', '！', '？', '；', '，'])
        .rsplit(['，', '；', '。'])
        .next()
        .map(str::trim)
        .filter(|clause| {
            clause
                .chars()
                .filter(|&c| crate::chapter_craft::is_han(c))
                .count()
                >= 6
        })
        .unwrap_or(outcome)
}

/// How a problem names step `step` (from 1) of the stage at `stage_index`.
fn step_location(stage_index: usize, step: u64) -> String {
    format!("阶段 {} 的第 {step} 步", stage_index + 1)
}

/// How a problem names the goal of the stage at `stage_index`.
fn goal_location(stage_index: usize) -> String {
    format!("阶段 {} 的目标", stage_index + 1)
}

/// Takes the first volume's steps that `feedback` says replay the opening out
/// of the direction handed back: given back, the planner keeps them.
pub(crate) fn drop_replayed_steps(direction: &mut Value, feedback: &Value) {
    let replayed = |stage_index: usize, index: usize| {
        let start = format!("{}「", step_location(stage_index, index as u64 + 1));
        // A step that is only too long stays to be shortened in place.
        items(feedback, "problems")
            .iter()
            .any(|problem| text(problem).starts_with(&start) && !is_too_long_step(text(problem)))
    };
    let seeds = direction["volumeIntentSeeds"].as_array_mut();
    let first = seeds
        .into_iter()
        .flatten()
        .find(|seed| seed["volumeNo"] == 1);
    let stages = first.and_then(|seed| seed["stages"].as_array_mut());
    for (stage_index, stage) in stages.into_iter().flatten().enumerate() {
        if let Some(steps) = stage["steps"].as_array_mut() {
            let mut index = 0..;
            steps.retain(|_| {
                index
                    .next()
                    .is_some_and(|index| !replayed(stage_index, index))
            });
        }
    }
}

fn replay_feedback(location: &str, planned: &str, evidence: &str, reason: &str) -> String {
    format!("{location}「{planned}」重排了开篇已发生的结果「{evidence}」。{reason}。请从该结果推进新的后果。")
}

/// The sentences of the opening a step is compared with at a time.
const RETOLD_SENTENCES: usize = 3;
/// A step may tell a passage again when it shares this many telling pairs
/// with it, and at least 35% of its own.
const RETOLD_PAIRS: usize = 5;

/// The steps worded like a passage of the opening, each with the passage
/// nearest to it, for the confirmation to judge.
fn retold_steps(stages: &[Value], opening: &[Value]) -> Vec<(Value, String)> {
    let pairs = crate::chapter_craft::telling_pairs;
    let passages = opening
        .iter()
        .flat_map(|chapter| {
            let body = text(&chapter["body"]);
            let spans = crate::chapter_craft::sentence_spans(body);
            (0..spans.len())
                .map(|first| {
                    let last = (first + RETOLD_SENTENCES - 1).min(spans.len() - 1);
                    let passage = &body[spans[first].0..spans[last].1];
                    (&chapter["chapterNo"], passage, pairs(passage))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut found = Vec::new();
    let mut goals = Vec::new();
    for (stage_index, stage) in stages.iter().enumerate() {
        // Real run round 8: stage goals retold the opening too.
        let targets = names(&stage["steps"])
            .enumerate()
            .map(|(index, step)| (Some(index as u64 + 1), step))
            .chain(std::iter::once((None, text(&stage["goal"]))));
        for (number, step) in targets {
            let planned = pairs(step);
            let shared = |passage: &BTreeSet<(char, char)>| planned.intersection(passage).count();
            let Some((chapter, passage, nearest)) = passages
                .iter()
                .max_by_key(|(_, _, passage)| shared(passage))
            else {
                continue;
            };
            let shared = shared(nearest);
            if shared < RETOLD_PAIRS || shared * 100 < planned.len() * 35 {
                continue;
            }
            let location = match number {
                Some(number) => step_location(stage_index, number),
                None => goal_location(stage_index),
            };
            // Steps come first, so goals never take their places in the
            // confirmation.
            let list = if number.is_some() {
                &mut found
            } else {
                &mut goals
            };
            list.push((
                json!({
                    "stage":stage_index + 1,"step":number,
                    "plannedText":step,"plannedQuote":step,"plannedOutcome":planned_outcome(step),
                    "openingChapter":chapter,"openingQuote":passage,
                }),
                replay_feedback(&location, step, passage, "两处写的是同一件事"),
            ));
        }
    }
    found.extend(goals);
    found
}

struct OpeningReplayReview {
    candidates: Vec<(Value, String)>,
    unbound: Vec<Value>,
}

fn ground_opening_replays(
    review: &Value,
    stages: &[Value],
    opening: &[Value],
) -> Result<OpeningReplayReview, String> {
    let index = |value: &Value| value.as_u64().and_then(|index| usize::try_from(index).ok());
    let one_based = |value: &Value| index(value).and_then(|index| index.checked_sub(1));
    let issues = review["issues"]
        .as_array()
        .ok_or(BOOK_CONTINUATION_EVIDENCE_INVALID)?;
    let locate = crate::chapter_craft::exact_punctuation_matched_quote;
    let ground = |issue: &Value| {
        let stage_index = one_based(&issue["stage"])?;
        let stage = stages.get(stage_index)?;
        let target = if issue["step"].is_null() {
            text(&stage["goal"])
        } else {
            names(&stage["steps"]).nth(one_based(&issue["step"])?)?
        };
        let planned = locate(target, text(&issue["plannedQuote"]))?;
        let chapter = opening
            .iter()
            .find(|entry| entry["chapterNo"] == issue["openingChapter"])?;
        let body = text(&chapter["body"]);
        let cited = text(&issue["openingQuote"]).trim();
        let spans = crate::chapter_craft::sentence_spans(body);
        let number = index(&issue["openingSentence"]);
        let evidence = if cited.chars().count() < 4 {
            // A short cue can locate the surrounding original prose but
            // cannot rebound to another occurrence elsewhere in the chapter.
            let number = number?;
            let &(start, end) = spans.get(number)?;
            let cue = cited.trim_end_matches(['。', '.', '！', '!', '？', '?', '…']);
            locate(&body[start..end], cue)?;
            let from = spans[number.saturating_sub(2)].0;
            let to = spans[(number + 1).min(spans.len() - 1)].1;
            &body[from..to]
        } else {
            // An ordinary quote can rebound uniquely within the same chapter.
            number
                .and_then(|number| {
                    let &(start, end) = spans.get(number)?;
                    locate(&body[start..end], cited)
                })
                .or_else(|| locate(body, cited))?
        };
        let location = match issue["step"].as_u64() {
            Some(step) => step_location(stage_index, step),
            None => goal_location(stage_index),
        };
        let pair = json!({
            "stage":stage_index + 1,"step":issue["step"],
            "plannedText":target,"plannedQuote":planned,"plannedOutcome":planned_outcome(target),
            "openingChapter":chapter["chapterNo"],"openingQuote":evidence,
        });
        Some((
            pair,
            replay_feedback(&location, planned, evidence, text(&issue["reason"])),
        ))
    };
    let mut result = OpeningReplayReview {
        candidates: Vec::new(),
        unbound: Vec::new(),
    };
    for issue in issues {
        if let Some(candidate) = ground(issue) {
            result.candidates.push(candidate);
        } else {
            result.unbound.push(issue.clone());
        }
    }
    Ok(result)
}

/// Whether `quote` is part of the planned outcome or, quoting the whole step,
/// the opening passage speaks of that outcome and not of its premise alone.
fn quotes_planned_outcome(pair: &Value, quote: &str, opening: &str) -> bool {
    let locate = crate::chapter_craft::exact_punctuation_matched_quote;
    let pairs = crate::chapter_craft::telling_pairs;
    let outcome = text(&pair["plannedOutcome"]);
    locate(outcome, quote).is_some()
        || (locate(text(&pair["plannedText"]), quote).is_some()
            && locate(quote, outcome).is_some()
            && pairs(outcome).intersection(&pairs(opening)).count() >= 2)
}

fn confirmed_opening_replay_problems(
    verdict: &Value,
    candidates: &[(Value, String)],
) -> Result<Vec<String>, String> {
    let confirmed = verdict["confirmedCandidates"]
        .as_array()
        .ok_or(BOOK_CONTINUATION_EVIDENCE_INVALID)?;
    let locate = crate::chapter_craft::exact_punctuation_matched_quote;
    let mut indices = BTreeSet::new();
    for claim in confirmed {
        let index = claim["number"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&index| index < candidates.len())
            .ok_or(BOOK_CONTINUATION_EVIDENCE_INVALID)?;
        let (pair, _) = &candidates[index];
        let opening_outcome = text(&claim["openingOutcomeQuote"]);
        let planned_outcome = text(&claim["plannedOutcomeQuote"]);
        if opening_outcome.chars().count() >= 4
            && planned_outcome.chars().count() >= 4
            && quotes_planned_outcome(pair, planned_outcome, opening_outcome)
            && locate(text(&pair["openingQuote"]), opening_outcome).is_some()
        {
            indices.insert(index);
        }
    }
    Ok(indices
        .into_iter()
        .map(|index| candidates[index].1.clone())
        .collect())
}

fn first_volume_direction(book: &Value) -> Option<&Value> {
    crate::planning::volume_seeds(book, 1).find(|seed| {
        let stages = items(seed, "stages");
        !text(&seed["climax"]).is_empty()
            && STAGE_COUNT.contains(&stages.len())
            && stages.iter().all(|stage| {
                ["goal", "pressure", "turn"]
                    .iter()
                    .all(|field| !text(&stage[*field]).is_empty())
                    && STEP_COUNT.contains(&items(stage, "steps").len())
                    && items(stage, "steps").iter().all(|step| {
                        !text(&step["id"]).is_empty() && !text(&step["goal"]).is_empty()
                    })
            })
    })
}

pub(crate) fn require_first_volume_direction(
    connection: &Connection,
    gateway: &dyn ProductionModelGateway,
    project_id: &str,
    volume_id: &str,
    require_route: bool,
) -> Result<(), String> {
    use crate::repositories::narrative::NarrativeRepository;
    if !gateway.plans_book_direction()
        || NarrativeRepository::volume_number(connection, project_id, volume_id)
            .map_err(|error| error.to_string())?
            != Some(1)
        || !crate::repositories::production::ProductionRepository::expects_first_volume_direction(
            connection, project_id,
        )
        .map_err(|error| error.to_string())?
    {
        return Ok(());
    }
    let book =
        crate::repositories::project_entry::canonical_head(connection, project_id, "BookPlan")
            .map_err(|error| error.to_string())?
            .map(|head| head.payload)
            .filter(|payload| {
                payload["planningMaturity"] == "APPROVED" && has_first_volume_direction(payload)
            })
            .ok_or(BOOK_DIRECTION_REQUIRED)?;
    if require_route {
        let route = NarrativeRepository::volume_plan_route(connection, project_id, volume_id)
            .and_then(|reference| {
                reference
                    .map(|reference| {
                        NarrativeRepository::artifact_payload_by_ref(
                            connection, project_id, &reference,
                        )
                    })
                    .transpose()
                    .map(Option::flatten)
            })
            .map_err(|error| error.to_string())?
            .ok_or(VOLUME_DIRECTION_ROUTE_REQUIRED)?;
        let seed = first_volume_direction(&book).ok_or(BOOK_DIRECTION_REQUIRED)?;
        let has_each_step = items(seed, "stages")
            .iter()
            .flat_map(|stage| items(stage, "steps"))
            .all(|step| {
                let task = crate::planning::stage_task_id(volume_id, text(&step["id"]));
                items(&route, "tasks")
                    .iter()
                    .any(|item| item["taskId"] == task)
            });
        if !has_each_step {
            return Err(VOLUME_DIRECTION_ROUTE_REQUIRED.into());
        }
    }
    Ok(())
}

/// Asks the model (`capability`, its runs named by `suffix`) for a
/// direction from `input`, sending what `check` finds back up to the plan
/// feedback rounds. The direction and its run, or why there is none: what
/// still fails, or a failed run.
fn run_direction(
    connection: &Connection,
    gateway: &dyn ProductionModelGateway,
    (project_id, command_id, idempotency_key, now): (&str, &str, &str, &str),
    (capability, suffix): (&str, &str),
    input: Value,
    opening_prose: &[Value],
    check: fn(&Value, &Value) -> Vec<String>,
) -> Result<Result<(Value, String), String>, String> {
    // P2: the methods selected for the book or the volume plan.
    let mut input = input;
    let stage = if capability == PLAN_VOLUME_DIRECTION {
        "volume_plan"
    } else {
        "book_plan"
    };
    input["craftGuidance"] = crate::chapter_craft::setup_guidance(stage)?;
    let profile = gateway.resolve_step_profile(capability)?;
    let spec = ModelRunSpec {
        model_run_id: StableIdentityFactory::from_command(
            StableIdKind::ModelRun,
            &format!("{command_id}:{suffix}"),
        )?,
        job_id: None,
        step_id: None,
        provider: profile.provider.clone(),
        model: profile.model.clone(),
        idempotency_key: format!("{idempotency_key}:{suffix}"),
        capability: capability.into(),
        prompt_ref: None,
        context_manifest_ref: None,
        budget_id: Some(DurableModelRunner::project_budget_id(project_id)),
        predicted_cost_cents: 0,
        cost_snapshot: None,
        attempt_no: 1,
        repair_round: 0,
    };
    let review_profile = if capability == PLAN_BOOK_DIRECTION && gateway.checks_book_continuation()
    {
        Some(gateway.resolve_step_profile(CHECK_BOOK_CONTINUATION)?)
    } else {
        None
    };
    let opening = opening_prose
        .iter()
        .map(|chapter| {
            json!({"chapterNo":chapter["chapterNo"],
               "sentences":crate::chapter_craft::numbered_body_sentences(text(&chapter["body"]))})
        })
        .collect::<Vec<_>>();
    let pressure_repair_attempted = Cell::new(false);
    let accepted_repair_run = Cell::new(None);
    let accept = |mut direction: Value, lenient: bool| {
        tidy(&mut direction, &input);
        // On the last round a step that is only too long is let through.
        let checked = |direction: &Value| {
            let mut found = check(direction, &input);
            if lenient {
                found.retain(|problem| !is_too_long_step(problem));
            }
            found
        };
        let mut found = checked(&direction);
        let mut repair_run = None;
        if review_profile.is_some() && found.len() == 1 {
            let antagonist = text(&input["storyBible"]["antagonist"]["name"]);
            let first = direction["volumeIntentSeeds"]
                .as_array_mut()
                .into_iter()
                .flatten()
                .find(|seed| seed["volumeNo"] == 1);
            if let Some(stages) = first.and_then(|seed| seed["stages"].as_array_mut()) {
                if let Some(gap) =
                    pressure_shortfall(stages, antagonist).filter(|gap| gap.problem == found[0])
                {
                    if pressure_repair_attempted.replace(true) {
                        return Err(format!("{BOOK_PRESSURE_REPAIR_REQUIRED}:{}", gap.problem));
                    }
                    let repair = ModelRunSpec {
                        capability: REPAIR_BOOK_PRESSURE.into(),
                        ..crate::production::follow_up_spec(&spec, ":pressure-repair", 1)?
                    };
                    let (run_id, outcome) =
                        crate::production::execute_production_model_with_profile_recorded(
                            connection,
                            gateway,
                            &profile,
                            &repair,
                            REPAIR_BOOK_PRESSURE,
                            json!({
                                "antagonist":input["storyBible"]["antagonist"],
                                "neededStages":gap.needed_stages,
                                "eligibleStages":gap.eligible_stages,
                                "stages":stages.iter().enumerate().map(|(index, stage)| json!({
                                    "number":index+1,"goal":stage["goal"],"pressure":stage["pressure"],
                                    "turn":stage["turn"],"cast":stage["cast"],
                                })).collect::<Vec<_>>()
                            }),
                            now,
                        )?;
                    outcome
                        .parsed
                        .as_ref()
                        .and_then(|reply| apply_pressure_repairs(stages, antagonist, &gap, reply))
                        .ok_or_else(|| {
                            format!("{BOOK_PRESSURE_REPAIR_REQUIRED}:{}", gap.problem)
                        })?;
                    found = checked(&direction);
                    if !found.is_empty() {
                        return Err(format!(
                            "{BOOK_PRESSURE_REPAIR_REQUIRED}:{}",
                            found.join("；")
                        ));
                    }
                    repair_run = Some(run_id);
                }
            }
        }
        if let Some(profile) = review_profile.as_ref().filter(|_| found.is_empty()) {
            let first_volume = items(&direction, "volumeIntentSeeds")
                .iter()
                .find(|seed| seed["volumeNo"] == 1)
                .ok_or(BOOK_DIRECTION_REQUIRED)?;
            let stages = items(first_volume, "stages");
            let review_input = json!({"opening":opening,"stages":stages});
            let digest =
                crate::hash::sha256_canonical(&review_input).map_err(|error| error.to_string())?;
            let review_spec = ModelRunSpec {
                capability: CHECK_BOOK_CONTINUATION.into(),
                provider: profile.provider.clone(),
                model: profile.model.clone(),
                ..crate::production::follow_up_spec(&spec, &format!(":check-opening:{digest}"), 0)?
            };
            let review = |spec: &ModelRunSpec, input: Value| {
                crate::production::execute_production_model_with_profile(
                    connection,
                    gateway,
                    profile,
                    spec,
                    CHECK_BOOK_CONTINUATION,
                    input,
                    now,
                )?
                .parsed
                .ok_or_else(|| "B07_BOOK_CONTINUATION_OUTPUT_INVALID".to_string())
            };
            let first = review(&review_spec, review_input.clone())?;
            let OpeningReplayReview {
                mut candidates,
                unbound,
            } = ground_opening_replays(&first, stages, opening_prose)?;
            if !unbound.is_empty() {
                let retry =
                    crate::production::follow_up_spec(&review_spec, ":evidence-feedback", 1)?;
                let mut input = review_input;
                input["evidenceFeedback"] = json!({
                    "instruction":"只复查以下无法定位原文的疑点。核对正文相邻句，引用实际完成结果而非准备动作，逐字复制。若原文不能证明同一结果已经发生，则去掉该疑点。",
                    "previousIssues":unbound
                });
                let same_target = |left: &Value, right: &Value| {
                    ["stage", "step", "openingChapter"]
                        .iter()
                        .all(|field| left[*field] == right[*field])
                };
                let reply = review(&retry, input)?;
                let requested = items(&reply, "issues")
                    .iter()
                    .filter(|issue| unbound.iter().any(|original| same_target(original, issue)))
                    .collect::<Vec<_>>();
                let corrected =
                    ground_opening_replays(&json!({"issues":requested}), stages, opening_prose)?;
                if !corrected.unbound.is_empty() {
                    found.push("开篇复核仍有引文无法定位。请改写对应阶段的小步，明确哪些结果已发生，本阶段要推进什么新后果。".into());
                }
                for original in &unbound {
                    if let Some(candidate) = corrected
                        .candidates
                        .iter()
                        .find(|(pair, _)| same_target(original, pair))
                    {
                        if !candidates.iter().any(|(pair, _)| pair == &candidate.0) {
                            candidates.push(candidate.clone());
                        }
                    }
                }
            }
            for (pair, feedback) in retold_steps(stages, opening_prose) {
                let reviewed = candidates.iter().any(|(other, _)| {
                    other["stage"] == pair["stage"] && other["step"] == pair["step"]
                });
                if !reviewed && candidates.len() < CONFIRM_CANDIDATES {
                    candidates.push((pair, feedback));
                }
            }
            if !candidates.is_empty() {
                let comparisons = candidates
                    .iter()
                    .enumerate()
                    .map(|(index, (pair, _))| {
                        let mut pair = pair.clone();
                        pair["number"] = json!(index);
                        pair
                    })
                    .collect::<Vec<_>>();
                // Exclude the initial review's interpretation. Identical
                // props or methods may lead to genuinely different outcomes.
                let confirmation_profile =
                    gateway.resolve_step_profile(CONFIRM_BOOK_CONTINUATION)?;
                let confirmation = ModelRunSpec {
                    capability: CONFIRM_BOOK_CONTINUATION.into(),
                    provider: confirmation_profile.provider.clone(),
                    model: confirmation_profile.model.clone(),
                    ..crate::production::follow_up_spec(&review_spec, ":confirm-outcomes", 1)?
                };
                let verdict = crate::production::execute_production_model_with_profile(
                    connection,
                    gateway,
                    &confirmation_profile,
                    &confirmation,
                    CONFIRM_BOOK_CONTINUATION,
                    json!({"opening":opening,"confirmCandidates":comparisons}),
                    now,
                )?
                .parsed
                .ok_or("B07_BOOK_CONTINUATION_OUTPUT_INVALID")?;
                found.extend(confirmed_opening_replay_problems(&verdict, &candidates)?);
            }
        }
        match (found.is_empty(), lenient) {
            (true, _) => {
                accepted_repair_run.set(repair_run);
                Ok((direction, found))
            }
            (false, true) => Ok((Value::Null, found)),
            (false, false) => Err(format!("{DIRECTION_ERROR}:{}", found.join("；"))),
        }
    };
    Ok(
        match crate::production::plan_chapter_with_feedback(
            connection,
            gateway,
            &profile,
            &spec,
            input.clone(),
            now,
            &accept,
        ) {
            Ok((direction, model_run_id, _)) if !direction.is_null() => Ok((
                direction,
                accepted_repair_run.take().unwrap_or(model_run_id),
            )),
            Ok((_, _, problems)) => Err(problems.join("；")),
            Err(error) => Err(error),
        },
    )
}

/// VD-2: plans volume `volume_id` when it opens, from the book plan and what
/// the volume before it did. The direction is the book plan with the
/// volume's seed replaced; a direction that still fails, or a failed run,
/// leaves the volume as the handoff compiled it, with a reminder.
pub fn plan_volume(
    connection: &Connection,
    gateway: &dyn ProductionModelGateway,
    project_id: &str,
    volume_id: &str,
    (command_id, idempotency_key, now): (&str, &str, &str),
) -> Result<Planned, String> {
    use crate::repositories::narrative::NarrativeRepository;
    use crate::repositories::volume_lifecycle::VolumeLifecycleRepository;
    if !gateway.plans_book_direction() {
        return Ok(Planned::default());
    }
    let volume = VolumeLifecycleRepository::volume_metadata(connection, project_id, volume_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VOLUME_NOT_FOUND".to_string())?;
    let book_plan =
        NarrativeRepository::canonical_project_head_ref(connection, project_id, "BookPlan")
            .and_then(|reference| {
                reference
                    .map(|reference| {
                        NarrativeRepository::artifact_payload_by_ref(
                            connection, project_id, &reference,
                        )
                    })
                    .transpose()
            })
            .map_err(|error| error.to_string())?
            .flatten()
            .ok_or_else(|| "GOAL_CONTRACT_BOOK_PLAN_APPROVED_REQUIRED".to_string())?;
    let bible = crate::craft_plan::confirmed_bible(connection, project_id)?.unwrap_or(Value::Null);
    let prior = crate::repositories::craft::plan_crafts_before(
        connection,
        project_id,
        volume.chapter_start,
    )
    .map_err(|error| error.to_string())?;
    let previous_number = volume.volume_no - 1;
    let mut previous = json!({
        "stages":[],
        "lastChapters":[],
        "lastAppearances":crate::craft_plan::last_appearances(&prior, &bible)
    });
    if let Some(previous_id) = VolumeLifecycleRepository::volume_id_by_number(
        connection,
        project_id,
        &volume.book_id,
        previous_number,
    )
    .map_err(|error| error.to_string())?
    {
        let completed = NarrativeRepository::satisfied_volume_task_chapters(
            connection,
            project_id,
            &previous_id,
        )
        .map_err(|error| error.to_string())?;
        previous["stages"] = json!(crate::planning::volume_seeds(&book_plan, previous_number)
            .flat_map(|seed| items(seed, "stages"))
            .map(|stage| json!({
                "goal":text(&stage["goal"]),
                "completedInChapter":completed_stage_chapter(stage, &previous_id, &completed)
            }))
            .collect::<Vec<_>>());
        let history = crate::repositories::ops::volume_chapter_history(
            connection,
            project_id,
            &previous_id,
            3,
        )
        .map_err(|error| error.to_string())?;
        previous["lastChapters"] = json!(history.recent);
        history.extend_continuation_input(&mut previous);
        if let Some(reference) = NarrativeRepository::canonical_head_ref(
            connection,
            project_id,
            "VolumeExitState",
            &previous_id,
        )
        .map_err(|error| error.to_string())?
        {
            let exit =
                NarrativeRepository::artifact_payload_by_ref(connection, project_id, &reference)
                    .map_err(|error| error.to_string())?
                    .ok_or("VOLUME_EXIT_STATE_PAYLOAD_NOT_FOUND")?;
            previous["exitState"] = json!({"ref":reference,"payload":exit});
        }
    }
    let input = volume_input(
        &bible,
        &book_plan,
        volume.volume_no,
        (volume.chapter_start, volume.chapter_end),
        &previous,
    );
    let planned = run_direction(
        connection,
        gateway,
        (project_id, command_id, idempotency_key, now),
        (PLAN_VOLUME_DIRECTION, "volume-direction"),
        input,
        &[],
        volume_problems,
    )?;
    Ok(match planned {
        Ok((direction, model_run_id)) => Planned {
            direction: Some(with_volume_seed(&book_plan, volume.volume_no, &direction)),
            model_run_id: Some(model_run_id),
            reminder: None,
        },
        Err(reason) => Planned {
            reminder: Some(template_reminder(
                command_id,
                project_id,
                (
                    "vd2-volume-reminder",
                    &format!("第 {} 卷方向", volume.volume_no),
                    &format!("第 {} 卷方向没有生成，这一卷没有阶段目标", volume.volume_no),
                ),
                &reason,
            )?),
            ..Planned::default()
        },
    })
}

fn completed_stage_chapter(
    stage: &Value,
    volume_id: &str,
    completed: &BTreeMap<String, i64>,
) -> Option<i64> {
    let steps = items(stage, "steps");
    let units = if steps.is_empty() {
        std::slice::from_ref(stage)
    } else {
        steps
    };
    let mut last_chapter = None;
    for unit in units {
        let task_id = crate::planning::stage_task_id(volume_id, unit["id"].as_str()?);
        let chapter = *completed.get(&task_id)?;
        last_chapter = last_chapter.max(Some(chapter));
    }
    last_chapter
}

/// The reminder that a direction (`what`, titled `title`, its decision id
/// from `id`) was not planned and the template stays.
fn template_reminder(
    command_id: &str,
    project_id: &str,
    (id, what, title): (&str, &str, &str),
    reason: &str,
) -> Result<Value, String> {
    Ok(json!({
        "decisionId":StableIdentityFactory::from_command(
            StableIdKind::Decision,
            &format!("{id}:{command_id}"),
        )?,
        "decisionType":"AUDIT_REMINDER",
        "outcome":"OPEN",
        "reason":format!("{what}没有生成，沿用了模板：{reason}"),
        "sourceRef":format!("plan:bookplan:{project_id}"),
        "payloadJson":{
            "category":"规划提醒",
            "title":title,
            "heldBack":[{"field":"plan","item":reason}],
            "allowedActions":["ACKNOWLEDGE"]
        }
    }))
}

/// VD-1c: where chapter `chapter_no` stands in volume `volume_id`'s stages,
/// for the planner: the first stage whose task is not completed, how many
/// chapters it has had since the stage before it ended (the first stage
/// counts from `before_stages`), and the stages after it. `completed` maps
/// each satisfied task to the chapter that completed it. None when the
/// volume has no stages.
pub fn chapter_direction(
    book_plan: &Value,
    route: &Value,
    (volume_id, volume_no): (&str, i64),
    before_stages: i64,
    completed: &BTreeMap<String, i64>,
    chapter_no: i64,
) -> Option<Value> {
    let seed = crate::planning::volume_seeds(book_plan, volume_no).next()?;
    let task_for = |unit: &Value| {
        let task_id = crate::planning::stage_task_id(volume_id, unit["id"].as_str()?);
        items(route, "tasks")
            .iter()
            .any(|task| task["taskId"] == task_id.as_str())
            .then_some(task_id)
    };
    // VD-1h: each stage's route tasks, one per step when it has steps in the
    // route, otherwise the stage's own.
    let stages = items(seed, "stages")
        .iter()
        .filter_map(|stage| {
            let steps = items(stage, "steps")
                .iter()
                .filter_map(|step| Some((step, task_for(step)?)))
                .collect::<Vec<_>>();
            let units = if steps.is_empty() {
                vec![(stage, task_for(stage)?)]
            } else {
                steps
            };
            Some((stage, units))
        })
        .collect::<Vec<_>>();
    if stages.is_empty() {
        return None;
    }
    let ended = |index: usize| {
        stages[..=index]
            .iter()
            .flat_map(|(_, tasks)| tasks)
            .map(|(_, task)| completed.get(task).copied())
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .max()
            // A step the opening already showed is dated there; the stage
            // itself never ends before the stages begin.
            .map(|chapter| chapter.max(before_stages))
    };
    let current = (0..stages.len())
        .position(|index| ended(index).is_none())
        .unwrap_or(stages.len());
    // The chapter the stage before stage `index` ended with.
    let ended_before = |index: usize| {
        index
            .checked_sub(1)
            .and_then(ended)
            .unwrap_or(before_stages)
    };
    let started = ended_before(current);
    // VD-1f: the stage that ended with the chapter before this one, and how
    // many chapters it took.
    let previous_stage = (current > 0 && started == chapter_no - 1).then(|| {
        let stage = stages[current - 1].0;
        json!({"goal":text(&stage["goal"]),"estimatedChapters":stage["estimatedChapters"],
            "chaptersTaken":started - ended_before(current - 1)})
    });
    let current_stage = stages.get(current).map(|(stage, units)| {
        let mut built = (*stage).clone();
        let step = units
            .iter()
            .position(|(_, task)| !completed.contains_key(task))
            .unwrap_or(units.len() - 1);
        let (unit, task) = &units[step];
        if unit["id"] != stage["id"] {
            // The planner sees the step, never the stage's end: its goal,
            // and its turn until the last step.
            let route_objective = items(route, "tasks")
                .iter()
                .find(|entry| entry["taskId"] == task.as_str())
                .and_then(|entry| entry["objective"].as_str())
                .filter(|objective| !objective.trim().is_empty());
            built["goal"] = json!(route_objective.unwrap_or_else(|| text(&unit["goal"])));
            if let Some(stage) = built.as_object_mut() {
                stage.remove("steps");
                if step + 1 < units.len() {
                    stage.remove("turn");
                }
            }
            built["step"] = json!(step + 1);
            built["stepCount"] = json!(units.len());
        }
        built["taskId"] = json!(task);
        built["chaptersSoFar"] = json!((chapter_no - 1 - started).max(0));
        built
    });
    let mut direction = json!({
        "volumeFunction":text(&seed["description"]),
        "currentStage":current_stage,
        "storyPromise":text(&book_plan["storyPromise"]),
        "previousStage":previous_stage
    });
    // Real run round 8: shown the later stages and the climax, plans reached
    // for them early. Only the last stage sees how the volume peaks.
    if current + 1 >= stages.len() {
        direction["climax"] = json!(text(&seed["climax"]));
    }
    Some(direction)
}

/// Whether a goal of the volume intent is the volume's overall goal or a
/// stage's, which the planning direction gives one step at a time.
fn is_hidden_goal(goal_id: &str) -> bool {
    goal_id.starts_with("goal:book:") || goal_id.contains(":stage:")
}

/// The id of a route task (taskId) or of a context's plan task (taskRef).
fn task_id(task: &Value) -> &str {
    match text(&task["taskId"]) {
        "" => text(&task["taskRef"]),
        id => id,
    }
}

/// Whether route task `task` is hidden from a chapter working on
/// `current`: the climax, the volume's goal, and every stage or step but the
/// current one.
fn is_hidden_task(task: &Value, current: &str) -> bool {
    let id = task_id(task);
    task["class"] == "CLIMAX"
        || id.starts_with(crate::planning::CLIMAX_TASK_PREFIX)
        || (id != current
            && id.strip_prefix("task:required:").is_some_and(|goal| {
                goal.starts_with("goal:book:")
                    || goal.contains(":stage:")
                    || goal.contains(":step:")
            }))
}

/// Takes the volume's overall goals, the stage goals, the other stages'
/// tasks and the climax out of a chapter planning request before the last
/// stage (real run round 8: plans copied the volume goal's list of losses
/// into every chapter, reached for a later stage and showed the climax's
/// object early). The chapter works on `current`, the task of its step.
pub(crate) fn hide_volume_end(input: &mut Value, current: &str) {
    let authority = &mut input["authorityContext"];
    if let Some(fields) = authority["goalContractFields"].as_array_mut() {
        fields.retain(|field| {
            !text(&field["fieldPath"])
                .strip_prefix("volumeIntent.mustAchieve[")
                .is_some_and(|goal| is_hidden_goal(goal.trim_end_matches(']')))
        });
    }
    if let Some(tasks) = authority["planTaskContexts"].as_array_mut() {
        tasks.retain(|task| !is_hidden_task(task, current));
    }
    if let Some(tasks) = input["volumePlanRoute"]["tasks"].as_array_mut() {
        tasks.retain(|task| !is_hidden_task(task, current));
    }
    // Real run round 9: given the scene a relationship ends with, plans
    // staged it in chapter after chapter.
    for path in [
        "/authorityContext/planTaskContexts",
        "/volumePlanRoute/tasks",
    ] {
        if let Some(tasks) = input.pointer_mut(path).and_then(Value::as_array_mut) {
            tasks.iter_mut().for_each(hide_relation_end);
        }
    }
}

/// Shortens a relationship task's objective to its direction.
fn hide_relation_end(task: &mut Value) {
    if !task_id(task).starts_with("task:optional:relation:") {
        return;
    }
    for path in ["/objective", "/contract/objective"] {
        if let Some(objective) = task.pointer_mut(path) {
            if let Some(heading) = objective.as_str().map(relation_heading) {
                *objective = json!(heading);
            }
        }
    }
}

/// A relationship's direction without the scene it is to end with: the
/// asides in brackets after its 走向 go.
fn relation_heading(objective: &str) -> String {
    let Some(at) = objective.find("走向") else {
        return objective.to_string();
    };
    let (start, toward) = objective.split_at(at);
    let mut kept = String::from(start);
    let mut depth = 0;
    for c in toward.chars() {
        match c {
            '（' | '(' => depth += 1,
            '）' | ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => kept.push(c),
            _ => {}
        }
    }
    kept
}

/// The direction for planning `chapter_id` (chapter `chapter_no`), from the
/// approved BookPlan and its volume's route; None before the route exists or
/// when the volume has no stages.
pub fn for_chapter(
    connection: &Connection,
    project_id: &str,
    chapter_id: &str,
    chapter_no: i64,
) -> Result<Option<Value>, String> {
    use crate::repositories::narrative::NarrativeRepository;
    let Some(volume_id) =
        NarrativeRepository::chapter_volume_id(connection, project_id, chapter_id)
            .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let Some(volume) =
        crate::repositories::volume_lifecycle::VolumeLifecycleRepository::volume_metadata(
            connection, project_id, &volume_id,
        )
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let book_ref =
        NarrativeRepository::canonical_project_head_ref(connection, project_id, "BookPlan")
            .map_err(|error| error.to_string())?;
    let route_ref =
        NarrativeRepository::canonical_head_ref(connection, project_id, "VolumePlan", &volume_id)
            .map_err(|error| error.to_string())?;
    let (Some(book_ref), Some(route_ref)) = (book_ref, route_ref) else {
        return Ok(None);
    };
    let payload = |reference: &crate::generated::types::ArtifactRef| {
        NarrativeRepository::artifact_payload_by_ref(connection, project_id, reference)
            .map_err(|error| error.to_string())
    };
    let book_plan = payload(&book_ref)?;
    let route = payload(&route_ref)?;
    let (Some(book_plan), Some(route)) = (book_plan, route) else {
        return Ok(None);
    };
    let mut passed =
        NarrativeRepository::satisfied_task_chapters(connection, project_id, chapter_no)
            .map_err(|error| error.to_string())?;
    let waiting = crate::planning::steps_awaiting_evidence(
        connection,
        (project_id, &volume_id),
        &route,
        &passed.keys().map(String::as_str).collect(),
        chapter_no,
    )?;
    passed.extend(waiting);
    // The first volume's stages begin after the opening chapters.
    let before_stages = match volume.volume_no {
        1 => crate::repositories::project_entry::OPENING_CHAPTER_COUNT,
        _ => volume.chapter_start - 1,
    };
    let mut direction = chapter_direction(
        &book_plan,
        &route,
        (&volume_id, volume.volume_no),
        before_stages,
        &passed,
        chapter_no,
    );
    if let Some(stage) = direction
        .as_mut()
        .and_then(|value| value.get_mut("currentStage"))
    {
        if let (Some(task_id), Some(objective)) = (stage["taskId"].as_str(), stage["goal"].as_str())
        {
            let unverified = if stage.get("step").is_some() {
                NarrativeRepository::recent_unverified_stage_attempts(
                    connection,
                    project_id,
                    &volume_id,
                    chapter_no,
                    task_id,
                    objective,
                    route["routeVersion"].as_u64() == Some(1),
                )
                .map_err(|error| error.to_string())?
            } else {
                Vec::new()
            };
            if let Some(progress) = crate::production::stage_step_progress_for_chapter(
                connection, project_id, &volume_id, chapter_no, task_id, objective,
            )? {
                stage["progress"] = progress;
            }
            if !unverified.is_empty() {
                stage["recentUnverifiedAttempts"] = json!(unverified);
            }
            if stage.get("step").is_some() {
                let objective = text(&stage["goal"]);
                stage["outcomeRules"] = crate::production::stage_step_rules(objective);
                stage["ruleDigest"] = json!(crate::hash::sha256_canonical(&json!({
                    "bookPlan":book_ref,"volumePlan":route_ref,"stage":stage
                }))
                .map_err(|error| error.to_string())?);
            }
        }
    }
    Ok(direction)
}

/// VD-1g (real run round 18): the problem with a chapter goal that copies
/// the current stage's goal while the stage is in its first half, so one
/// chapter would finish a stage meant for several.
pub fn stage_goal_problem(direction: &Value, chapter_goal: &str) -> Option<String> {
    let stage = &direction["currentStage"];
    // VD-1h: with steps the planner sees only the step, which it may reach.
    if stage.get("step").is_some() {
        return None;
    }
    let goal = text(&stage["goal"]);
    let so_far = stage["chaptersSoFar"].as_i64()?;
    let estimate = stage["estimatedChapters"].as_i64()?;
    (so_far * 2 < estimate
        && crate::craft_plan::goals_overlap(
            goal,
            &crate::chapter_craft::telling_pairs(chapter_goal),
        ))
    .then(|| {
        format!(
            "本章目标几乎照抄了阶段目标（{goal}）：这一阶段预计写 {estimate} 章，已写 {so_far} 章，本章只推进其中一步，阶段目标留到后面几章达成"
        )
    })
}

/// How many chapters in a row the antagonist may be off the page before
/// the plan is reminded (owner decision 2026-11-02).
const ANTAGONIST_ABSENCE: i64 = 4;

/// VD-1d (§3.6): what the plan for chapter `chapter_no` should be reminded
/// of, given its `direction` and the people `on_stage` in it. Reminders,
/// never a reason to plan again.
pub fn reminders(direction: &Value, on_stage: &BTreeSet<&str>, chapter_no: i64) -> Vec<String> {
    let mut reminders = Vec::new();
    let appearances = items(direction, "lastAppearances");
    let last_seen = |name: &str| {
        appearances
            .iter()
            .find(|person| text(&person["name"]) == name)
            .map(|person| person["lastChapter"].as_i64().unwrap_or(0))
    };
    for antagonist in appearances.iter().filter(|person| person["role"] == "反派") {
        let name = text(&antagonist["name"]);
        let last = antagonist["lastChapter"].as_i64();
        if on_stage.contains(name) || chapter_no - last.unwrap_or(0) < ANTAGONIST_ABSENCE {
            continue;
        }
        reminders.push(match last {
            Some(last) => format!(
                "反派{name}已经连续 {} 章没出场（最后一次在第 {last} 章）",
                chapter_no - last
            ),
            None => format!("反派{name}还没在任何一章的场景里正面出场"),
        });
    }
    let previous = &direction["previousStage"];
    if let (Some(taken), Some(estimate)) = (
        previous["chaptersTaken"].as_i64(),
        previous["estimatedChapters"].as_i64(),
    ) {
        if taken * 2 < estimate {
            reminders.push(format!(
                "上一阶段（{}）预计 {estimate} 章，只用 {taken} 章就完成了，节奏过快：这一阶段放慢，每章只推进一步",
                text(&previous["goal"])
            ));
        }
    }
    let stage = &direction["currentStage"];
    let (Some(so_far), Some(estimate)) = (
        stage["chaptersSoFar"].as_i64(),
        stage["estimatedChapters"].as_i64(),
    ) else {
        return reminders;
    };
    let stage_start = chapter_no - so_far;
    if so_far * 2 >= estimate {
        let missing = names(&stage["cast"])
            .filter(|name| !on_stage.contains(name))
            .filter(|name| last_seen(name).is_some_and(|last| last < stage_start))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            reminders.push(format!(
                "这一阶段该出场的{}到阶段过半还没出场",
                missing.join("、")
            ));
        }
    }
    if so_far >= estimate * 2 {
        reminders.push(format!(
            "这一阶段已经写了 {so_far} 章，预计 {estimate} 章，到了两倍还没完成：往段末转折收"
        ));
    }
    reminders
}

#[cfg(test)]
mod tests {
    #[test]
    fn round54_outcome_confirmation_rejects_the_candidate_review_format() {
        let check = |payload| crate::model_output::check(CONFIRM_BOOK_CONTINUATION, &payload);
        assert!(check(json!({"issues":[]})).is_err());
        assert!(check(json!({"confirmedCandidates":[0]})).is_err());
        assert!(check(json!({"confirmedCandidates":[{
            "number":0,"openingOutcomeQuote":"那道刻度自己亮了","plannedOutcomeQuote":"刻度自行亮起"
        }]}))
        .is_ok());
        assert!(crate::model_output::check(
            CHECK_BOOK_CONTINUATION,
            &json!({"confirmedCandidates":[]})
        )
        .is_err());
    }

    #[test]
    fn round54_a_confirmed_premise_does_not_block_the_new_consequence() {
        let old = "那道刻度自己亮了。亮得稳稳的，不再跳。";
        let target =
            "竹尺第七刻度已自行稳定亮起，陆照发现它不再随呼吸浮动，却开始微幅吸摄四周游离灵息";
        let candidate = |plan| {
            (
                json!({"plannedOutcome":planned_outcome(plan),"openingQuote":old}),
                "已完成结果".into(),
            )
        };
        let verdict = json!({"confirmedCandidates":[{
            "number":0,"plannedOutcomeQuote":"刻度已自行稳定亮起","openingOutcomeQuote":"那道刻度自己亮了"
        }]});
        assert!(
            confirmed_opening_replay_problems(&verdict, &[candidate(target)])
                .unwrap()
                .is_empty()
        );
        let repeated = candidate("他检查石缝，随后刻度已自行稳定亮起。");
        assert_eq!(
            confirmed_opening_replay_problems(&verdict, &[repeated]).unwrap(),
            ["已完成结果"]
        );
    }

    #[test]
    fn round57_a_shared_place_does_not_prove_replay_of_the_planned_result() {
        let target = "他用冷炊饼引开扫地犬，潜入西厢最东头库房，见柳青梧正伏案补描第七条修订稿缺角处松脂弯痕";
        let old = "西厢最东头库房的门半开着。";
        let candidate = (
            json!({"plannedText":target,"plannedOutcome":planned_outcome(target),"openingQuote":old}),
            "重排开篇".into(),
        );
        let verdict = json!({"confirmedCandidates":[{
            "number":0,"plannedOutcomeQuote":target,"openingOutcomeQuote":old
        }]});
        assert!(confirmed_opening_replay_problems(&verdict, &[candidate])
            .unwrap()
            .is_empty());
    }

    use super::*;

    fn bible() -> Value {
        json!({
            "protagonist":{"name":"陆照"},
            "antagonist":{"name":"裴砚舟"},
            "cast":[{"name":"柳漪","firstAppears":1},{"name":"陈伯","firstAppears":1}]
        })
    }

    fn direction_input() -> Value {
        let opening = [(1, json!({"endHook":"账册上悬着陆照的名字"}))];
        input(
            &bible(),
            &json!({"readerPromise":"以命换力","forbiddenReveals":["租约幕后的操盘者"]}),
            &opening,
            &json!({"verdict":"通过"}),
            (1, 36),
            3,
        )
    }

    #[test]
    fn round53_one_bad_issue_does_not_discard_a_rebound_opening_quote() {
        let stages = [json!({"goal":"查清地火","steps":["刻度指向第七日峰值","取回铜印"]})];
        let opening = [json!({
            "chapterNo":1,
            "body":"刻度到哪儿，他的眼睛就到哪儿。第七日峰值。分毫不差。"
        })];
        let review = json!({"issues":[
            {"stage":1,"step":2,"plannedQuote":"取回铜印","openingChapter":1,
             "openingSentence":0,"openingQuote":"他从泥中取回了铜印。","reason":"未见于正文"},
            {"stage":1,"step":1,"plannedQuote":"刻度指向第七日峰值","openingChapter":1,
             "openingSentence":0,"openingQuote":"第七日峰值。分毫不差。","reason":"已证实峰值"}
        ]});
        let review_result = ground_opening_replays(&review, &stages, &opening).unwrap();
        assert_eq!(review_result.candidates.len(), 1);
        assert_eq!(
            review_result.candidates[0].0["openingQuote"],
            "第七日峰值。分毫不差。"
        );
        assert_eq!(review_result.unbound, [review["issues"][0].clone()]);
    }

    #[test]
    fn round75_a_replay_confirmed_with_the_whole_step_is_sent_back() {
        // Real run round 6: four replayed steps were confirmed, each quoting
        // its whole step instead of the clause asked for, and all four stayed
        // in the route.
        let step = "沈砚比对三片碎纸残渣与内网扫描件，发现圆珠笔压痕无法被矢量重描";
        let opening = "末笔右下点有个明显的压痕——圆珠笔滚珠在纸面停了一瞬才提起。";
        let pair = json!({"plannedText":step,"plannedOutcome":planned_outcome(step),"openingQuote":opening});
        let candidates = [(pair, "阶段 1 的第 1 步重排了开篇".to_owned())];
        let problems = |planned: &str, shown: &str| {
            confirmed_opening_replay_problems(
                &json!({"confirmedCandidates":[
                    {"number":0,"openingOutcomeQuote":shown,"plannedOutcomeQuote":planned}]}),
                &candidates,
            )
            .unwrap()
        };
        assert_eq!(problems(step, opening).len(), 1);
        assert_eq!(problems("圆珠笔压痕无法被矢量重描", opening).len(), 1);
        // The premise alone is not the step's result.
        assert!(problems("沈砚比对三片碎纸残渣与内网扫描件", opening).is_empty());
        assert!(problems(step, "压痕已经被系统抹平了。").is_empty());
    }

    #[test]
    fn round77_steps_that_tell_the_opening_again_are_all_put_to_the_confirmation() {
        // Real run round 7: most steps of four stages told the opening again
        // and the review named five at most.
        let stages = [json!({"goal":"查清车库","steps":[
            "沈砚走出车库时，鞋底沾起三滴水，滚入打孔机冲头孔洞蒸干",
            "沈砚撕下‘停止调查’通知单，在背面写下‘A7-3号未归档事件：继续’",
            "沈砚顺着借调名单找到当年的调度员"
        ]})];
        let opening = [
            json!({"chapterNo":2,"body":"沈砚盯着它看了两秒。然后他伸手把通知单撕下来，翻到背面，用断笔芯写：A7-3号未归档事件：继续。"}),
            json!({"chapterNo":3,"body":"坡道尽头站着老陈。沈砚踩过去。鞋底沾起三滴水，走到打孔机冲头孔洞边上时，一滴滚落进去，嗤一声蒸干。“那批借调名单，我经手过。”"}),
        ];
        let found = retold_steps(&stages, &opening);
        assert_eq!(
            found
                .iter()
                .map(|(pair, _)| (
                    pair["step"].as_u64().unwrap(),
                    pair["openingChapter"].as_i64().unwrap()
                ))
                .collect::<Vec<_>>(),
            [(1, 3), (2, 2)]
        );
        let (pair, feedback) = &found[0];
        assert!(pair["openingQuote"]
            .as_str()
            .unwrap()
            .contains("鞋底沾起三滴水"));
        assert!(feedback.starts_with("阶段 1 的第 1 步「"));
    }

    #[test]
    fn round80_a_stage_goal_that_tells_the_opening_again_is_put_to_the_confirmation() {
        let stages = [
            json!({"goal":"定位南坡气象仪基座，拓下铭牌编号和父亲的签名","steps":[
                "沈砚把拓片交给林晚核对",
                "林晚查到档案里少了一页"
            ]}),
        ];
        let opening = [
            json!({"chapterNo":3,"body":"他扒开浮土，露出南坡气象仪基座。铭牌编号先出来，下面是父亲的签名。他用铅笔把铭牌拓下来。"}),
        ];
        let found = retold_steps(&stages, &opening);
        assert_eq!(found.len(), 1, "{found:?}");
        let (pair, feedback) = &found[0];
        assert_eq!(pair["step"], Value::Null);
        assert!(feedback.starts_with("阶段 1 的目标「"), "{feedback}");
    }

    #[test]
    fn round76_a_rejected_replay_is_not_handed_back_to_the_planner() {
        // Real run round 7: the same step came back in the next two plans.
        let mut direction = direction();
        let step = direction["volumeIntentSeeds"][0]["stages"][0]["steps"][1].clone();
        let problem = format!(
            "{}「{}」重排了开篇已发生的结果「旧」。",
            step_location(0, 2),
            step.as_str().unwrap()
        );
        drop_replayed_steps(&mut direction, &json!({"problems":[problem,"别的问题"]}));
        let stages = &direction["volumeIntentSeeds"][0]["stages"];
        assert_eq!(
            stages[0]["steps"],
            json!(["陆照查清丹房的账·第一步", "陆照查清丹房的账·第三步"])
        );
        assert_eq!(stages[1]["steps"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn round62_a_short_opening_cue_locates_evidence_without_proving_a_replay_by_itself() {
        let stages = [json!({"goal":"查明灰中文字","steps":["陆照比对灰烬与饼上的炭字笔锋"]})];
        let opening = [json!({"chapterNo":1,"body":
            "墨迹慢慢晕开，显出半个字来。右上折笔，一横，一竖。壬。陆照盯着那半个字。饼皮上用炭条写着一个字。壬。"
        })];
        let issue = json!({
            "stage":1,"step":1,"plannedQuote":"陆照比对灰烬与饼上的炭字笔锋",
            "openingChapter":1,"openingSentence":2,"openingQuote":"壬。","reason":"可能复现已读到的字"
        });
        let review = json!({"issues":[issue]});
        crate::model_output::check(CHECK_BOOK_CONTINUATION, &review).unwrap();
        let result = ground_opening_replays(&review, &stages, &opening).unwrap();
        assert!(result.unbound.is_empty());
        let opening_passage = result.candidates[0].0["openingQuote"].as_str().unwrap();
        assert!(opening_passage.contains("墨迹慢慢晕开，显出半个字来"));
        assert!(!opening_passage.contains("饼皮上"));
        let verdict = json!({"confirmedCandidates":[{"number":0,"openingOutcomeQuote":"壬。",
            "plannedOutcomeQuote":"陆照比对灰烬与饼上的炭字笔锋"}]});
        assert!(
            confirmed_opening_replay_problems(&verdict, &result.candidates)
                .unwrap()
                .is_empty()
        );
        let mut other = review;
        other["issues"][0]["openingSentence"] = json!(1);
        let missing = ground_opening_replays(&other, &stages, &opening).unwrap();
        assert!(missing.candidates.is_empty());
        assert_eq!(missing.unbound.len(), 1);
    }

    fn stage(goal: &str, pressure: &str, cast: &[&str], chapters: i64) -> Value {
        json!({"goal":goal,"pressure":pressure,"cast":cast,"turn":"段末转折","estimatedChapters":chapters,
            "steps":[format!("{goal}·第一步"),format!("{goal}·第二步"),format!("{goal}·第三步")]})
    }

    fn direction() -> Value {
        json!({
            "protagonistMacroDirection":"陆照从任人差遣的杂役走到替宗门签下新约的人",
            "mustAchieve":[{"volumeNo":1,"description":"陆照拿到租约的原件"}],
            "relationshipMacroDirections":[{"between":["陆照","柳漪"],"from":"互相提防","toward":"并肩查账","volumeNo":1}],
            "majorMysteryGoals":[{"layer":1,"answerInVolume":1,"description":"租约里多出的那一条是谁加的"}],
            "futureReservations":[{"description":"租约幕后的操盘者","notBeforeVolumeNo":3}],
            "volumeIntentSeeds":[
                {"volumeNo":1,"description":"守住丹房，揪出改约的人","stages":[
                    stage("陆照查清丹房的账","裴砚舟扣下陆照的月例",&["陆照","陈伯"],8),
                    stage("陆照找到改约的证据","裴砚舟设局栽赃陆照",&["柳漪"],10),
                    stage("陆照在会审上翻案","三大宗派人压阵",&["陆照"],10)
                ],"climax":"会审上陆照当众拿出原件"},
                {"volumeNo":2,"description":"走出青崖宗"}
            ]
        })
    }

    /// A planner that always returns `original`, a review finding nothing and
    /// the pressure repair below; every call is kept.
    struct DirectionGateway {
        original: Value,
        calls: std::sync::Mutex<Vec<(String, Value)>>,
    }

    impl DirectionGateway {
        fn new(original: Value) -> Self {
            Self {
                original,
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl ProductionModelGateway for DirectionGateway {
        fn checks_book_continuation(&self) -> bool {
            true
        }

        fn resolve_profile(&self) -> Result<crate::production::ResolvedProviderProfile, String> {
            Ok(crate::production::ResolvedProviderProfile {
                provider: "direction-fixture".into(),
                model: "fixed".into(),
                endpoint: "fixture://direction".into(),
                timeout_ms: 1_000,
            })
        }

        fn invoke(
            &self,
            capability: &str,
            input: &Value,
        ) -> Result<crate::production::ProductionModelResponse, String> {
            self.calls
                .lock()
                .unwrap()
                .push((capability.into(), input.clone()));
            let reply = match capability {
                PLAN_BOOK_DIRECTION => self.original.clone(),
                "REPAIR_BOOK_PRESSURE" => json!({"pressureCorrections":[{
                    "stage":2,"pressure":"裴砚舟命守库的执事封住原账，陆照只能去找旧抄本。"
                }]}),
                CHECK_BOOK_CONTINUATION => json!({"issues":[]}),
                _ => return Err(format!("unexpected capability {capability}")),
            };
            Ok(crate::production::ProductionModelResponse::deterministic(
                reply,
            ))
        }
    }

    #[test]
    fn a_lone_pressure_gap_repairs_only_missing_pressure_before_opening_review() {
        use crate::project_db::{paths::AppPaths, ProjectManager};

        let mut original = direction();
        original["volumeIntentSeeds"][0]["stages"][1]["pressure"] = json!("宗门断了丹房的炭");
        let gateway = DirectionGateway::new(original.clone());
        let temp = tempfile::tempdir().unwrap();
        let manager = ProjectManager::new(AppPaths::new(temp.path()));
        let now = "2026-09-13T12:00:00Z";
        let db = manager.create("pressure-fixture", "方向复核", now).unwrap();
        let (accepted, model_run_id) = run_direction(
            &db,
            &gateway,
            (
                "pressure-fixture",
                "pressure-check",
                "pressure-check:key",
                now,
            ),
            (PLAN_BOOK_DIRECTION, "book-direction"),
            direction_input(),
            &[],
            problems,
        )
        .unwrap()
        .unwrap();
        let calls = gateway.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            [
                PLAN_BOOK_DIRECTION,
                "REPAIR_BOOK_PRESSURE",
                CHECK_BOOK_CONTINUATION
            ]
        );
        let mut expected = original;
        expected["volumeIntentSeeds"][0]["stages"][1]["pressure"] =
            json!("裴砚舟命守库的执事封住原账，陆照只能去找旧抄本。");
        tidy(&mut expected, &direction_input());
        assert_eq!(accepted, expected);
        let response = DurableModelRunner::persisted_parsed_response(&db, &model_run_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            response["pressureCorrections"][0]["pressure"],
            expected["volumeIntentSeeds"][0]["stages"][1]["pressure"]
        );
    }

    #[test]
    fn round120_a_step_still_too_long_on_the_last_round_is_accepted() {
        // Real run round 12: every round left two or three steps a little
        // past 30 characters, and the whole setup failed for it.
        use crate::project_db::{paths::AppPaths, ProjectManager};

        let long = "陆照把丹房三年的旧账一页页翻完，找出被改过的那一页并抄下改动人的签名和日期";
        let mut original = direction();
        original["volumeIntentSeeds"][0]["stages"][1]["steps"][1] = json!(long);
        let gateway = DirectionGateway::new(original);
        let temp = tempfile::tempdir().unwrap();
        let manager = ProjectManager::new(AppPaths::new(temp.path()));
        let now = "2026-09-13T12:00:00Z";
        let db = manager
            .create("long-step-fixture", "方向复核", now)
            .unwrap();
        let (accepted, _) = run_direction(
            &db,
            &gateway,
            ("long-step-fixture", "long-step", "long-step:key", now),
            (PLAN_BOOK_DIRECTION, "book-direction"),
            direction_input(),
            &[],
            problems,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            accepted["volumeIntentSeeds"][0]["stages"][1]["steps"][1],
            json!(long)
        );
    }

    #[test]
    fn pressure_correction_cannot_replace_an_existing_action() {
        let original = [
            json!({"pressure":"裴砚舟已经扣下丹房月例"}),
            json!({"pressure":"旧案牵连药库"}),
            json!({"pressure":"丹炉失火"}),
        ];
        let mut stages = original.clone();
        let gap = pressure_shortfall(&stages, "裴砚舟").unwrap();
        let overwrite =
            json!({"pressureCorrections":[{"stage":1,"pressure":"裴砚舟命人烧掉旧账"}]});
        assert!(apply_pressure_repairs(&mut stages, "裴砚舟", &gap, &overwrite).is_none());
        assert_eq!(stages, original);
    }

    /// The approved plan and route VD-1a and VD-1b make from `direction()`.
    fn plan_and_route() -> (Value, Value) {
        let mut plan = Map::new();
        plan.insert("storyPromise".into(), json!("以命换力"));
        apply(&mut plan, &direction(), &[]);
        let stage_task = |n: usize| json!({"taskId":format!("task:required:goal:vol:1:stage:1:{n}"),"class":"REQUIRED"});
        let route = json!({"tasks":[
            stage_task(1), stage_task(2), stage_task(3),
            {"taskId":"task:required:goal:book:1","class":"REQUIRED"}
        ]});
        (Value::Object(plan), route)
    }

    #[test]
    fn chapter_four_starts_the_first_stage() {
        let (plan, route) = plan_and_route();
        let direction =
            chapter_direction(&plan, &route, ("vol:1", 1), 3, &BTreeMap::new(), 4).unwrap();
        assert_eq!(direction["volumeFunction"], "守住丹房，揪出改约的人");
        let stage = &direction["currentStage"];
        assert_eq!(stage["taskId"], "task:required:goal:vol:1:stage:1:1");
        assert_eq!(stage["goal"], "陆照查清丹房的账");
        assert_eq!(stage["pressure"], "裴砚舟扣下陆照的月例");
        assert_eq!(stage["cast"], json!(["陆照", "陈伯"]));
        assert_eq!(stage["turn"], "段末转折");
        assert_eq!(stage["estimatedChapters"], 8);
        assert_eq!(stage["chaptersSoFar"], 0);
        assert_eq!(direction.get("climax"), None);
        assert!(!direction.to_string().contains("陆照找到改约的证据"));
        assert_eq!(direction["storyPromise"], "以命换力");
    }

    #[test]
    fn a_completed_stage_moves_the_chapter_on_and_counts_from_its_end() {
        let (plan, route) = plan_and_route();
        let completed = BTreeMap::from([("task:required:goal:vol:1:stage:1:1".to_string(), 9)]);
        let direction = chapter_direction(&plan, &route, ("vol:1", 1), 3, &completed, 12).unwrap();
        assert_eq!(direction["currentStage"]["goal"], "陆照找到改约的证据");
        assert_eq!(direction["currentStage"]["chaptersSoFar"], 2);
        assert_eq!(direction.get("climax"), None);
    }

    #[test]
    fn round80_a_chapter_before_the_last_stage_does_not_see_the_volume_s_goal() {
        let mut input = json!({
            "authorityContext":{
                "goalContractFields":[
                    {"fieldPath":"volumeIntent.mustAchieve[goal:book:1]","value":"失去三件事物"},
                    {"fieldPath":"volumeIntent.mustAchieve[goal:vol:1:stage:1:5]","value":"传真机"},
                    {"fieldPath":"volumeIntent.mustAchieve[goal:vol:1:responsibility:r1]","value":"跨卷责任"},
                    {"fieldPath":"volumeIntent.mustNotBreak[rule:1]","value":"开篇事实"}
                ],
                "planTaskContexts":[
                    {"taskRef":"task:required:goal:vol:1:step:1:1:1","contract":{}},
                    {"taskRef":"task:required:goal:vol:1:step:1:2:1","contract":{"objective":"断崖"}},
                    {"taskRef":"task:required:goal:book:1","contract":{}},
                    {"taskRef":"task:climax:mystery:book:1","contract":{}},
                    {"taskRef":"task:optional:relation:book:1","contract":{}}
                ]
            },
            "volumePlanRoute":{"tasks":[
                {"taskId":"task:required:goal:vol:1:step:1:1:1"},
                {"taskId":"task:required:goal:vol:1:stage:1:3","objective":"晚报"},
                {"taskId":"task:climax:mystery:book:1","class":"CLIMAX","objective":"铆钉"}
            ]}
        });
        hide_volume_end(&mut input, "task:required:goal:vol:1:step:1:1:1");
        let shown = input.to_string();
        for hidden in [
            "失去三件事物",
            "传真机",
            "断崖",
            "晚报",
            "铆钉",
            "task:required:goal:book:1",
        ] {
            assert!(!shown.contains(hidden), "{hidden}: {shown}");
        }
        for kept in ["开篇事实", "跨卷责任", "step:1:1:1", "relation:book:1"] {
            assert!(shown.contains(kept), "{kept}: {shown}");
        }
    }

    #[test]
    fn round90_a_chapter_sees_where_a_relationship_goes_but_not_its_last_scene() {
        let objective = "沈砚与陈默：从脆弱同盟（共谋隐瞒检修盖异动）走向责任共担（陈默主动撕毁自己疫苗记录备份，换取沈砚暂缓调查其母病历）";
        let mut input = json!({
            "authorityContext":{"planTaskContexts":[
                {"taskRef":"task:optional:relation:book:1","contract":{"objective":objective}}
            ]},
            "volumePlanRoute":{"tasks":[
                {"taskId":"task:optional:relation:book:1","objective":objective}
            ]}
        });
        hide_volume_end(&mut input, "task:required:goal:vol:1:step:1:1:1");
        let heading = "沈砚与陈默：从脆弱同盟（共谋隐瞒检修盖异动）走向责任共担";
        assert_eq!(input["volumePlanRoute"]["tasks"][0]["objective"], heading);
        assert_eq!(
            input["authorityContext"]["planTaskContexts"][0]["contract"]["objective"],
            heading
        );
    }

    #[test]
    fn round80_the_last_stage_sees_how_the_volume_peaks() {
        let (plan, route) = plan_and_route();
        let completed = (1..=2)
            .map(|n| (format!("task:required:goal:vol:1:stage:1:{n}"), 10 * n))
            .collect();
        let direction = chapter_direction(&plan, &route, ("vol:1", 1), 3, &completed, 23).unwrap();
        assert_eq!(direction["currentStage"]["goal"], "陆照在会审上翻案");
        assert_eq!(direction["climax"], "会审上陆照当众拿出原件");
    }

    #[test]
    fn after_the_last_stage_only_the_climax_is_left() {
        let (plan, route) = plan_and_route();
        let completed = (1..=3)
            .map(|n| (format!("task:required:goal:vol:1:stage:1:{n}"), 10 * n))
            .collect();
        let direction = chapter_direction(&plan, &route, ("vol:1", 1), 3, &completed, 33).unwrap();
        assert_eq!(direction["currentStage"], Value::Null);
        assert_eq!(direction["climax"], "会审上陆照当众拿出原件");
    }

    fn reminded(direction: &Value, on_stage: &[&str], chapter_no: i64) -> Vec<String> {
        reminders(direction, &on_stage.iter().copied().collect(), chapter_no)
    }

    fn direction_at(chapter_no: i64, last_seen: &[(&str, Option<i64>)]) -> Value {
        let (plan, route) = plan_and_route();
        let mut direction =
            chapter_direction(&plan, &route, ("vol:1", 1), 3, &BTreeMap::new(), chapter_no)
                .unwrap();
        // The antagonist comes first, as in craft_plan::last_appearances.
        direction["lastAppearances"] = last_seen
            .iter()
            .enumerate()
            .map(|(index, (name, chapter))| {
                let role = if index == 0 { "反派" } else { "配角" };
                json!({"name":name,"role":role,"lastChapter":chapter})
            })
            .collect();
        direction
    }

    #[test]
    fn an_antagonist_off_the_page_for_four_chapters_is_a_reminder() {
        let direction = direction_at(8, &[("裴砚舟", Some(4)), ("陈伯", Some(7))]);
        assert_eq!(
            reminded(&direction, &["陆照"], 8),
            vec!["反派裴砚舟已经连续 4 章没出场（最后一次在第 4 章）".to_string()]
        );
        assert!(reminded(&direction, &["陆照", "裴砚舟"], 8).is_empty());
        let direction = direction_at(7, &[("裴砚舟", Some(4)), ("陈伯", Some(6))]);
        assert!(reminded(&direction, &["陆照"], 7).is_empty());
    }

    #[test]
    fn stage_cast_still_missing_past_mid_stage_is_a_reminder() {
        // The first stage (8 chapters, cast 陆照 and 陈伯) started at chapter 4.
        let direction = direction_at(8, &[("裴砚舟", Some(7)), ("陈伯", Some(2))]);
        assert_eq!(
            reminded(&direction, &["陆照"], 8),
            vec!["这一阶段该出场的陈伯到阶段过半还没出场".to_string()]
        );
        assert!(reminded(&direction, &["陆照", "陈伯"], 8).is_empty());
        let direction = direction_at(6, &[("裴砚舟", Some(5)), ("陈伯", Some(2))]);
        assert!(reminded(&direction, &["陆照"], 6).is_empty());
    }

    #[test]
    fn a_stage_at_twice_its_estimate_is_a_reminder() {
        let direction = direction_at(20, &[("裴砚舟", Some(19)), ("陈伯", Some(19))]);
        assert_eq!(
            reminded(&direction, &["陆照"], 20),
            vec!["这一阶段已经写了 16 章，预计 8 章，到了两倍还没完成：往段末转折收".to_string()]
        );
    }

    #[test]
    fn a_stage_done_in_under_half_its_estimate_is_a_reminder_in_the_next_chapter() {
        // VD-1f (real run round 17): four stages of 6 to 9 chapters were done
        // one a chapter.
        let (plan, route) = plan_and_route();
        let completed = BTreeMap::from([("task:required:goal:vol:1:stage:1:1".to_string(), 4)]);
        let direction = chapter_direction(&plan, &route, ("vol:1", 1), 3, &completed, 5).unwrap();
        assert_eq!(
            direction["previousStage"],
            json!({"goal":"陆照查清丹房的账","estimatedChapters":8,"chaptersTaken":1})
        );
        assert_eq!(
            reminders(&direction, &["裴砚舟"].into_iter().collect(), 5),
            vec!["上一阶段（陆照查清丹房的账）预计 8 章，只用 1 章就完成了，节奏过快：这一阶段放慢，每章只推进一步".to_string()]
        );
        let later = chapter_direction(&plan, &route, ("vol:1", 1), 3, &completed, 6).unwrap();
        assert_eq!(later["previousStage"], Value::Null);
    }

    #[test]
    fn a_chapter_goal_copying_a_young_stage_s_goal_is_a_problem() {
        let (plan, route) = plan_and_route();
        // Stage 1 (8 chapters) at its first chapter.
        let young = chapter_direction(&plan, &route, ("vol:1", 1), 3, &BTreeMap::new(), 4).unwrap();
        assert_eq!(
            stage_goal_problem(&young, "陆照查清丹房的账，揪出改账的人"),
            Some("本章目标几乎照抄了阶段目标（陆照查清丹房的账）：这一阶段预计写 8 章，已写 0 章，本章只推进其中一步，阶段目标留到后面几章达成".to_string())
        );
        assert_eq!(
            stage_goal_problem(&young, "陆照夜里摸进账房，先抄下三月的出库单"),
            None
        );
        // Past half its estimate the stage may be reached.
        let late = chapter_direction(&plan, &route, ("vol:1", 1), 3, &BTreeMap::new(), 9).unwrap();
        assert_eq!(stage_goal_problem(&late, "陆照查清丹房的账"), None);
    }

    #[test]
    fn an_antagonist_never_on_stage_is_reminded_as_such() {
        let direction = direction_at(5, &[("裴砚舟", None), ("陈伯", Some(4))]);
        assert_eq!(
            reminded(&direction, &["陆照"], 5),
            vec!["反派裴砚舟还没在任何一章的场景里正面出场".to_string()]
        );
    }

    #[test]
    fn a_stage_needs_three_to_five_steps() {
        // VD-1h (real run round 20): each chapter planned a whole stage.
        let mut direction = direction();
        direction["volumeIntentSeeds"][0]["stages"][1]["steps"] = json!(["只有一步"]);
        direction["volumeIntentSeeds"][0]["stages"][2]["steps"] =
            json!(["一", "二", "三", "四", "五", "六"]);
        let found = problems(&direction, &direction_input());
        assert!(
            found.contains(&"第 2 个阶段要分 3 到 5 步（steps），现在是 1 步".to_string()),
            "{found:?}"
        );
        assert!(
            found.contains(&"第 3 个阶段要分 3 到 5 步（steps），现在是 6 步".to_string()),
            "{found:?}"
        );
    }

    #[test]
    fn round110_a_step_written_out_as_actions_is_sent_back() {
        // Real run round 11: steps scripted the chapters' actions, and the
        // chapters copied them, so the hero went back to the same rust stain.
        let mut direction = direction();
        let scripted =
            "沈砚将硬化的右手小指按在锈斑中心，指腹渗出的冷凝水沿纹路流向标记点，锈迹突然泛起蓝光";
        direction["volumeIntentSeeds"][0]["stages"][1]["steps"] = json!([
            scripted,
            "陆照知道改约的人在执事堂里",
            "柳漪答应在会审上作证"
        ]);
        let found = problems(&direction, &direction_input());
        let location = format!("阶段 2 的第 1 步「{scripted}」");
        assert!(
            found
                .iter()
                .any(|problem| problem.starts_with(&location) && problem.contains("超过 30 字")),
            "{found:?}"
        );
        assert!(
            !found.iter().any(|p| p.starts_with("阶段 2 的第 2 步")),
            "{found:?}"
        );
        // Real run round 12: taken out of the direction handed back, the
        // long steps were not replaced, and the stages fell below three steps.
        let mut handed_back = direction.clone();
        drop_replayed_steps(&mut handed_back, &json!({"problems":found}));
        assert_eq!(
            handed_back["volumeIntentSeeds"][0]["stages"][1]["steps"],
            direction["volumeIntentSeeds"][0]["stages"][1]["steps"]
        );
    }

    #[test]
    fn round80_a_step_that_is_a_line_of_dialogue_is_sent_back() {
        let mut direction = direction();
        direction["volumeIntentSeeds"][0]["stages"][1]["steps"] = json!([
            "陈伯递来一块粗布，说‘压住边，风不掀’",
            "陆照认出账页上‘丹房’二字是改过的",
            "陆照把改过的账页交给执事"
        ]);
        let found = problems(&direction, &direction_input());
        assert!(
            found.contains(&"阶段 2 的第 1 步「陈伯递来一块粗布，说‘压住边，风不掀’」只写了一句台词：一步写的是结果，这一步之后人物能做、知道或决定什么".to_string()),
            "{found:?}"
        );
        assert!(
            !found.iter().any(|p| p.starts_with("阶段 2 的第 2 步")),
            "{found:?}"
        );
        assert!(says_a_line(
            "吴幼安立刻指出‘这枚铝的密度比琴弦高，会吃掉第三个泛音’"
        ));
        assert!(says_a_line("她说出‘你姐最后弹的是降E，不是C’"));
        for named in [
            "铭牌露出‘云海观景台’四个字",
            "陆照知道“丹房”的秘密",
            "一个名叫“青禾”的组织",
            "陈伯问了一句“谁”",
        ] {
            assert!(!says_a_line(named), "{named}");
        }
        let mut handed_back = direction.clone();
        drop_replayed_steps(&mut handed_back, &json!({"problems":found}));
        assert_eq!(
            handed_back["volumeIntentSeeds"][0]["stages"][1]["steps"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn a_stage_s_steps_are_named_by_the_system() {
        let mut plan = Map::new();
        apply(&mut plan, &direction(), &[]);
        assert_eq!(
            plan["volumeIntentSeeds"][0]["stages"][1]["steps"],
            json!([
                {"id":"step:1:2:1","goal":"陆照找到改约的证据·第一步"},
                {"id":"step:1:2:2","goal":"陆照找到改约的证据·第二步"},
                {"id":"step:1:2:3","goal":"陆照找到改约的证据·第三步"}
            ])
        );
    }

    /// The route VD-1b makes when stage 1 has steps.
    fn stepped_route() -> Value {
        let task = |id: &str| json!({"taskId":format!("task:required:goal:vol:1:{id}"),"class":"REQUIRED"});
        json!({"tasks":[
            task("step:1:1:1"), task("step:1:1:2"), task("step:1:1:3"),
            task("stage:1:2"), task("stage:1:3")
        ]})
    }

    #[test]
    fn a_chapter_is_planned_toward_its_stage_s_current_step() {
        // VD-1h (real run round 20): planners told the stage goal reached it
        // in one chapter.
        let (plan, _) = plan_and_route();
        let direction = chapter_direction(
            &plan,
            &stepped_route(),
            ("vol:1", 1),
            3,
            &BTreeMap::new(),
            4,
        )
        .unwrap();
        let stage = &direction["currentStage"];
        assert_eq!(stage["goal"], "陆照查清丹房的账·第一步");
        assert_eq!(stage["taskId"], "task:required:goal:vol:1:step:1:1:1");
        assert_eq!(stage["step"], 1);
        assert_eq!(stage["stepCount"], 3);
        assert_eq!(stage.get("turn"), None);
        assert!(
            !stage.to_string().contains("\"陆照查清丹房的账\""),
            "{stage}"
        );
        let completed = BTreeMap::from([("task:required:goal:vol:1:step:1:1:1".to_string(), 5)]);
        let direction =
            chapter_direction(&plan, &stepped_route(), ("vol:1", 1), 3, &completed, 6).unwrap();
        assert_eq!(direction["currentStage"]["goal"], "陆照查清丹房的账·第二步");
        assert_eq!(direction["currentStage"]["chaptersSoFar"], 2);
        assert_eq!(direction["previousStage"], Value::Null);
        assert_eq!(
            stage_goal_problem(&direction, "陆照查清丹房的账·第二步"),
            None
        );
    }

    #[test]
    fn a_chapter_uses_the_current_route_objective_for_its_stage_step() {
        let (plan, _) = plan_and_route();
        let mut route = stepped_route();
        route["tasks"][0]["objective"] = json!("陆照从旧出库单上查到漏记的两笔账");
        let direction =
            chapter_direction(&plan, &route, ("vol:1", 1), 3, &BTreeMap::new(), 4).unwrap();
        assert_eq!(
            direction["currentStage"]["goal"],
            route["tasks"][0]["objective"]
        );
    }

    #[test]
    fn a_stage_the_opening_already_showed_ends_where_the_stages_begin() {
        let (plan, _) = plan_and_route();
        let completed = (1..=3)
            .map(|n| (format!("task:required:goal:vol:1:step:1:1:{n}"), n))
            .collect();
        let direction =
            chapter_direction(&plan, &stepped_route(), ("vol:1", 1), 3, &completed, 4).unwrap();
        assert_eq!(direction["currentStage"]["chaptersSoFar"], 0);
        assert_eq!(direction["previousStage"]["chaptersTaken"], 0);
    }

    #[test]
    fn a_stage_ends_with_its_last_step() {
        let (plan, _) = plan_and_route();
        let completed = (1..=3)
            .map(|n| (format!("task:required:goal:vol:1:step:1:1:{n}"), 4 + n))
            .collect();
        let direction =
            chapter_direction(&plan, &stepped_route(), ("vol:1", 1), 3, &completed, 8).unwrap();
        assert_eq!(direction["currentStage"]["goal"], "陆照找到改约的证据");
        assert_eq!(
            direction["previousStage"],
            json!({"goal":"陆照查清丹房的账","estimatedChapters":8,"chaptersTaken":4})
        );
    }

    #[test]
    fn a_blank_stage_is_dropped_and_estimates_are_scaled_into_the_volume() {
        // VD-1i (real run round 21): three directions of 7 stages, the last
        // blank, estimated at 39 or 44 of 33 chapters, kept the template.
        let mut direction = direction();
        let stages = direction["volumeIntentSeeds"][0]["stages"]
            .as_array_mut()
            .unwrap();
        stages.push(json!({"goal":"","pressure":"","cast":[],"turn":"","steps":[]}));
        for (stage, chapters) in stages.iter_mut().zip([12, 14, 13]) {
            stage["estimatedChapters"] = json!(chapters);
        }
        let input = direction_input();
        tidy(&mut direction, &input);
        let stages = direction["volumeIntentSeeds"][0]["stages"]
            .as_array()
            .unwrap();
        assert_eq!(stages.len(), 3);
        let total = stages
            .iter()
            .map(|stage| stage["estimatedChapters"].as_i64().unwrap())
            .sum::<i64>();
        assert_eq!(total, 33);
        assert_eq!(problems(&direction, &input), Vec::<String>::new());
    }

    #[test]
    fn seven_stages_fit_a_long_volume_and_the_input_names_the_chapter_range() {
        let mut direction = direction();
        let stage = direction["volumeIntentSeeds"][0]["stages"][0].clone();
        let stages = direction["volumeIntentSeeds"][0]["stages"]
            .as_array_mut()
            .unwrap();
        while stages.len() < 7 {
            stages.push(stage.clone());
        }
        for stage in stages.iter_mut() {
            stage["estimatedChapters"] = json!(4);
        }
        assert_eq!(
            problems(&direction, &direction_input()),
            Vec::<String>::new()
        );
        assert_eq!(
            direction_input()["firstVolume"]["stageChapters"],
            json!({"min":17,"max":33})
        );
    }

    #[test]
    fn a_volume_without_stages_has_no_direction() {
        let (plan, route) = plan_and_route();
        assert_eq!(
            chapter_direction(&plan, &route, ("vol:2", 2), 36, &BTreeMap::new(), 40),
            None
        );
    }

    /// The approved plan with a description-only second volume seed.
    fn approved_plan() -> Value {
        let mut plan = Map::new();
        plan.insert("storyPromise".into(), json!("以命换力"));
        apply(&mut plan, &direction(), &["canon:1".to_string()]);
        let mut approved = json!({
            "payloadSchema":"book-plan.r1","planningMaturity":"APPROVED","storyPromise":"以命换力",
            "endingBoundary":"结局","mustNotBreak":[],"openingHypotheses":[],
            "globalLengthBudget":{"preferredWords":1_200_000}
        });
        for (key, value) in plan {
            approved[key] = value;
        }
        approved
    }

    fn volume_two_input() -> Value {
        volume_input(
            &bible(),
            &approved_plan(),
            2,
            (37, 72),
            &json!({"stages":[{"goal":"陆照在会审上翻案","completedInChapter":34}]}),
        )
    }

    fn volume_two() -> Value {
        json!({
            "description":"陆照离开青崖宗，去紫霄峰追查租约的来历",
            "stages":[
                stage("陆照混进紫霄峰外门","裴砚舟派人一路追查陆照的下落",&["陆照"],10),
                stage("陆照找到租约的底档","裴砚舟买通紫霄峰执事封锁档房",&["柳漪"],12),
                stage("陆照把底档带出紫霄峰","裴砚舟亲自上山拦截",&["陆照","陈伯"],10)
            ],
            "climax":"山门前陆照当众烧掉伪约"
        })
    }

    #[test]
    fn the_volume_input_holds_the_book_plan_for_it_and_what_the_volume_before_did() {
        let input = volume_two_input();
        assert_eq!(
            input["volume"],
            json!({"volumeNo":2,"chapters":36,"stageChapters":{"min":18,"max":36}})
        );
        assert_eq!(input["seed"], "走出青崖宗");
        assert_eq!(input["book"]["storyPromise"], "以命换力");
        assert_eq!(
            input["book"]["protagonistMacroDirection"],
            "陆照从任人差遣的杂役走到替宗门签下新约的人"
        );
        assert_eq!(
            input["previousVolume"]["stages"][0]["completedInChapter"],
            34
        );
        assert_eq!(input["storyBible"]["antagonist"]["name"], "裴砚舟");
    }

    #[test]
    fn a_volume_direction_with_stages_that_fit_it_has_no_problems() {
        assert_eq!(
            volume_problems(&volume_two(), &volume_two_input()),
            Vec::<String>::new()
        );
        let mut empty = volume_two();
        empty["description"] = json!("");
        empty["stages"] = json!([]);
        let found = volume_problems(&empty, &volume_two_input());
        assert!(
            found.contains(&"description 要写这一卷对全书的作用".to_string()),
            "{found:?}"
        );
        assert!(
            found.contains(&"第 2 卷要分 3 到 8 个阶段，现在是 0 个".to_string()),
            "{found:?}"
        );
        let mut long = volume_two();
        long["stages"][0]["estimatedChapters"] = json!(30);
        assert!(volume_problems(&long, &volume_two_input()).contains(
            &"各阶段 estimatedChapters 加起来是 52 章，这一卷有 36 章，应在 18 到 36 之间"
                .to_string()
        ));
    }

    #[test]
    fn a_planned_volume_replaces_only_its_own_seed() {
        let plan = with_volume_seed(&approved_plan(), 2, &volume_two());
        let seeds = plan["volumeIntentSeeds"].as_array().unwrap();
        assert_eq!(seeds[0], approved_plan()["volumeIntentSeeds"][0]);
        assert_eq!(seeds[1]["seedId"], "volume-2");
        assert_eq!(
            seeds[1]["description"],
            "陆照离开青崖宗，去紫霄峰追查租约的来历"
        );
        assert_eq!(seeds[1]["stages"][2]["id"], "stage:2:3");
        assert_eq!(seeds[1]["climax"], "山门前陆照当众烧掉伪约");
        assert_eq!(plan["mustAchieve"], approved_plan()["mustAchieve"]);
        crate::generated::validators::validate_book_plan_r1(&plan).unwrap();
    }

    #[test]
    fn a_direction_with_three_stages_that_fit_the_volume_has_no_problems() {
        assert_eq!(
            problems(&direction(), &direction_input()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_input_holds_what_the_opening_did_and_the_chapters_left_in_the_volume() {
        let input = direction_input();
        assert_eq!(input["storyBible"]["antagonist"]["name"], "裴砚舟");
        assert_eq!(input["opening"][0]["chapter"], 1);
        assert_eq!(input["opening"][0]["endHook"], "账册上悬着陆照的名字");
        assert_eq!(input["firstVolume"]["chaptersAfterOpening"], 33);
    }

    #[test]
    fn a_chapter_table_or_a_chapter_number_is_a_problem() {
        let mut numbered = direction();
        numbered["volumeIntentSeeds"][0]["stages"][1]["turn"] = json!("第12章裴砚舟倒台");
        let mut table = direction();
        table["chapters"] = json!([{"no":4}]);
        for direction in [numbered, table] {
            assert!(problems(&direction, &direction_input())
                .iter()
                .any(|problem| problem.contains("不要写章表")));
        }
        let mut chinese = direction();
        chinese["volumeIntentSeeds"][0]["climax"] = json!("第十章会审");
        assert!(problems(&chinese, &direction_input())
            .iter()
            .any(|problem| problem.contains("不要写章表")));
    }

    #[test]
    fn two_stages_or_nine_are_a_problem() {
        for count in [2, 9] {
            let mut direction = direction();
            direction["volumeIntentSeeds"][0]["stages"] = json!((0..count)
                .map(|_| stage("目标", "裴砚舟出手", &[], 3))
                .collect::<Vec<_>>());
            assert!(problems(&direction, &direction_input())
                .contains(&format!("第一卷要分 3 到 8 个阶段，现在是 {count} 个")));
        }
    }

    #[test]
    fn a_stage_without_a_turn_is_a_problem() {
        let mut direction = direction();
        direction["volumeIntentSeeds"][0]["stages"][2]["turn"] = json!(" ");
        assert!(
            problems(&direction, &direction_input()).contains(&"第 3 个阶段缺 turn".to_string())
        );
    }

    #[test]
    fn stage_estimates_beyond_the_volume_are_a_problem() {
        let mut direction = direction();
        direction["volumeIntentSeeds"][0]["stages"][0]["estimatedChapters"] = json!(20);
        assert!(problems(&direction, &direction_input()).contains(
            &"各阶段 estimatedChapters 加起来是 40 章，第一卷开篇之后还有 33 章，应在 17 到 33 之间"
                .to_string()
        ));
    }

    #[test]
    fn tidying_moves_a_stage_cast_member_outside_the_bible_into_new_cast() {
        // Real run round 22: three directions in a row failed on this alone.
        let mut direction = direction();
        direction["volumeIntentSeeds"][0]["stages"][1]["cast"] = json!(["柳漪", "沈掌柜"]);
        tidy(&mut direction, &direction_input());
        let stage = &direction["volumeIntentSeeds"][0]["stages"][1];
        assert_eq!(stage["cast"], json!(["柳漪"]));
        assert_eq!(stage["newCast"], json!(["沈掌柜"]));
        assert_eq!(
            problems(&direction, &direction_input()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn tidying_drops_an_exact_duplicate_stage_and_preserves_the_direction() {
        let mut direction = direction();
        let stages = direction["volumeIntentSeeds"][0]["stages"]
            .as_array_mut()
            .unwrap();
        let expected = stages.clone();
        stages.push(stages.last().unwrap().clone());
        tidy(&mut direction, &direction_input());
        assert_eq!(direction["volumeIntentSeeds"][0]["stages"], json!(expected));
        assert!(problems(&direction, &direction_input()).is_empty());
    }

    #[test]
    fn a_stage_cast_member_outside_the_bible_must_be_marked_new() {
        let mut direction = direction();
        direction["volumeIntentSeeds"][0]["stages"][1]["cast"] = json!(["柳漪", "沈掌柜"]);
        assert!(problems(&direction, &direction_input()).contains(
            &"第 2 个阶段的 cast 里有故事圣经之外的人：沈掌柜，新人物写进 newCast".to_string()
        ));
        direction["volumeIntentSeeds"][0]["stages"][1]["cast"] = json!(["柳漪"]);
        direction["volumeIntentSeeds"][0]["stages"][1]["newCast"] = json!(["沈掌柜"]);
        assert_eq!(
            problems(&direction, &direction_input()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_antagonist_acting_in_fewer_than_half_the_stages_is_a_problem() {
        let mut direction = direction();
        direction["volumeIntentSeeds"][0]["stages"][1]["pressure"] = json!("宗门断了丹房的炭");
        assert!(problems(&direction, &direction_input())
            .contains(&"反派裴砚舟只在 1 个阶段的 pressure 里出手，至少要 2 个".to_string()));
    }

    #[test]
    fn forbidden_reveals_need_a_reservation_and_the_first_volume_a_mystery_layer() {
        let mut direction = direction();
        direction["futureReservations"] = json!([]);
        direction["majorMysteryGoals"][0]["answerInVolume"] = json!(2);
        let found = problems(&direction, &direction_input());
        assert!(found
            .iter()
            .any(|problem| problem.contains("forbiddenReveals")));
        assert!(found
            .iter()
            .any(|problem| problem.contains("answerInVolume 为 1")));
    }

    #[test]
    fn a_missing_first_volume_or_climax_is_a_problem() {
        let mut no_climax = direction();
        no_climax["volumeIntentSeeds"][0]
            .as_object_mut()
            .unwrap()
            .remove("climax");
        assert!(problems(&no_climax, &direction_input()).contains(&"第一卷要写 climax".to_string()));
        let mut no_first = direction();
        no_first["volumeIntentSeeds"][0]["volumeNo"] = json!(3);
        assert!(problems(&no_first, &direction_input())
            .contains(&"volumeIntentSeeds 缺第一卷（volumeNo 为 1）".to_string()));
    }

    #[test]
    fn the_direction_fills_the_plan_with_system_named_items() {
        let mut plan = Map::new();
        plan.insert(
            "protagonistMacroDirection".into(),
            json!("青崖宗外门丹房杂役陆照"),
        );
        apply(&mut plan, &direction(), &["canon:1".to_string()]);
        assert_eq!(
            plan["protagonistMacroDirection"],
            "陆照从任人差遣的杂役走到替宗门签下新约的人"
        );
        assert_eq!(
            plan["mustAchieve"],
            json!([{"id":"goal:book:1","volumeNo":1,"description":"陆照拿到租约的原件"}])
        );
        assert_eq!(
            plan["relationshipMacroDirections"][0],
            json!({"id":"relation:book:1","volumeNo":1,"description":"陆照与柳漪：从互相提防走向并肩查账"})
        );
        assert_eq!(
            plan["majorMysteryGoals"][0],
            json!({"id":"mystery:book:1","volumeNo":1,"description":"第 1 层：租约里多出的那一条是谁加的"})
        );
        assert_eq!(
            plan["futureReservations"][0],
            json!({"reservationId":"reservation:book:1","description":"租约幕后的操盘者",
                "notBeforeVolumeNo":3,"reservedForVolumeNo":3,"prerequisites":["canon:1"],
                "mayForeshadow":true,"mayReveal":false,"authorityLevel":"B"})
        );
        let seeds = plan["volumeIntentSeeds"].as_array().unwrap();
        assert_eq!(seeds[0]["seedId"], "volume-1");
        assert_eq!(seeds[0]["climax"], "会审上陆照当众拿出原件");
        assert_eq!(seeds[0]["stages"][1]["id"], "stage:1:2");
        assert_eq!(seeds[0]["stages"][1]["pressure"], "裴砚舟设局栽赃陆照");
        assert_eq!(
            seeds[1],
            json!({"seedId":"volume-2","description":"走出青崖宗"})
        );
        let approved = json!({
            "payloadSchema":"book-plan.r1","planningMaturity":"APPROVED","storyPromise":"以命换力",
            "endingBoundary":"结局","mustNotBreak":[],"openingHypotheses":[],
            "globalLengthBudget":{"preferredWords":1_200_000}
        });
        let mut approved = approved.as_object().unwrap().clone();
        approved.extend(plan);
        crate::generated::validators::validate_book_plan_r1(&Value::Object(approved)).unwrap();
    }
}
