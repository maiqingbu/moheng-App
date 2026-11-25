//! Craft rules that sit between the length range and the v2 skills: what
//! the planner is given, what a usable plan must contain, and which signals
//! the later steps choose their skills by.

use crate::chapter_length::ChapterLengthRange;
use crate::skills_v2::StepSignals;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub const CHAPTER_ROLES: [&str; 7] = ["普通", "过渡", "重要", "升级", "回报", "高潮前", "高潮"];

/// Where in the user's range a chapter of this role should land.
pub fn role_range(role: &str, range: ChapterLengthRange) -> ChapterLengthRange {
    let span = range.max - range.min;
    let at = |quarters: i64| range.min + span * quarters / 4;
    let (min, max) = match role {
        "过渡" => (at(0), at(2)),
        "普通" => (at(1), at(3)),
        "重要" | "升级" | "回报" | "高潮前" => (at(2), at(4)),
        "高潮" => (at(3), at(4)),
        _ => (range.min, range.max),
    };
    ChapterLengthRange { min, max }
}

/// The selected v2 skills for one step, as handed to the model.
pub fn guidance(stage: &str, signals: &StepSignals) -> Result<Value, String> {
    let skills =
        crate::skills_v2::select_for_step(stage, signals, crate::skills_v2::stage_budget(stage))?;
    Ok(json!({
        "skillsVersion":crate::skills_v2::SKILLS_V2_VERSION,
        "stage":stage,
        "skills":skills
    }))
}

/// P2: guidance for a step before any chapter is written: story directions,
/// story setup, book plan or volume plan. Story setup designs the cast and
/// the mechanism; a volume plan stands at a volume boundary.
pub fn setup_guidance(stage: &str) -> Result<Value, String> {
    let events: &[&str] = match stage {
        "story_setup" => &["new_major_character", "new_ability"],
        "volume_plan" => &["volume_boundary"],
        _ => &[],
    };
    let signals = StepSignals {
        events: events.iter().map(|event| event.to_string()).collect(),
        ..Default::default()
    };
    guidance(stage, &signals)
}

/// The ids of the skills in a `guidance` value, in order.
pub fn guidance_skill_ids(guidance: &Value) -> Vec<String> {
    guidance["skills"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|skill| skill.get("skillId").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

/// Extra planner input: chapter position, what the recent chapters already
/// did, the role options with their sub-ranges, and the selected skills.
pub fn planning_inputs(
    chapter_number: Option<u32>,
    recent_chapters: &[Value],
    range: ChapterLengthRange,
) -> Result<Value, String> {
    let signals = planning_signals(chapter_number, recent_chapters);
    let roles = CHAPTER_ROLES
        .iter()
        .map(|role| {
            let sub = role_range(role, range);
            json!({"role":role,"min":sub.min,"max":sub.max})
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "chapterNumber":chapter_number,
        "recentChapters":recent_chapters,
        "chapterRoleOptions":roles,
        "sceneTypeOptions":crate::skills_v2::scene_type_vocabulary()?,
        "craftGuidance":guidance("chapter_plan", &signals)?
    }))
}

fn normalized_goal(goal: &str) -> String {
    goal.chars()
        .filter(|c| {
            !c.is_whitespace()
                && !c.is_ascii_punctuation()
                && !"，。、；：！？“”‘’《》（）…—".contains(*c)
        })
        .collect()
}

pub(crate) fn goal_repeats<'a>(goal: &str, mut prior: impl Iterator<Item = &'a str>) -> bool {
    let goal = normalized_goal(goal);
    !goal.is_empty() && prior.any(|previous| normalized_goal(previous) == goal)
}

/// Rejects a plan that cannot be written well: a missing role or a goal
/// that repeats a recent chapter's.
pub fn validate_plan_craft(plan: &Value, recent_chapters: &[Value]) -> Result<(), String> {
    if !plan
        .get("chapterRole")
        .and_then(Value::as_str)
        .is_some_and(|role| CHAPTER_ROLES.contains(&role))
    {
        return Err("B08_PLAN_ROLE_INVALID".into());
    }
    let prior = recent_chapters
        .iter()
        .filter_map(|chapter| chapter["chapterGoal"].as_str());
    if goal_repeats(plan["chapterGoal"].as_str().unwrap_or_default(), prior) {
        return Err("B08_PLAN_GOAL_REPEATED".into());
    }
    Ok(())
}

/// Signals for the steps after planning, read from the committed plan.
pub fn plan_signals(plan: &Value, chapter_number: Option<u32>) -> StepSignals {
    StepSignals {
        chapter_number,
        chapter_role: plan
            .get("chapterRole")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        scene_types: plan
            .get("sceneExecution")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|scene| scene.get("sceneType").and_then(Value::as_str))
            .map(crate::skills_v2::canonical_scene_type)
            .collect(),
        ..StepSignals::default()
    }
}

/// Signals for planning a chapter: its position, and the role and scene
/// types of the previous chapter (`recent_chapters` is newest first), since
/// this chapter's own are not chosen yet.
pub fn planning_signals(chapter_number: Option<u32>, recent_chapters: &[Value]) -> StepSignals {
    let previous = recent_chapters.first();
    StepSignals {
        chapter_number,
        chapter_role: previous
            .and_then(|chapter| chapter.get("chapterRole"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        scene_types: previous
            .and_then(|chapter| chapter.get("sceneTypes"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(crate::skills_v2::canonical_scene_type)
            .collect(),
        ..StepSignals::default()
    }
}

/// Plan rejections the planner can fix itself, with what to tell it.
const REPLANNABLE: [(&str, &str); 4] = [
    (
        "ROLLING_HORIZON_CAPACITY_INVALID",
        "horizonPreview 条数不符合 horizonPreviewCapacity：容量为 0～2 时返回不超过容量的条数，否则返回 3 到 min(8, 容量) 条，每条都是 {\"objective\":string,\"tentative\":true}。",
    ),
    ("B08_PLAN_LENGTH_OUT_OF_RANGE", "每个场景都必须给出整数 estimatedCharacters，且所有场景之和要落在 chapterLengthRange 内：按区间调整场景和节拍数量，让内容自然撑起篇幅。"),
    ("B08_PLAN_ROLE_INVALID", "chapterRole 缺失或不在 chapterRoleOptions 中：从中选择一个本章角色，并让场景篇幅总和落在该角色的区间内。"),
    ("B08_PLAN_GOAL_REPEATED", "本章目标与 recentChapters 中已写章节的目标重复：从最近一章的结尾继续，给本章一个带来新推进的目标。"),
];

/// The feedback for a plan rejection the planner can fix, or `None` when
/// planning again would not help.
pub fn plan_feedback(code: &str) -> Option<Value> {
    // VD-1: a rejected direction is told so in its own words.
    if let Some(problems) = code
        .strip_prefix(crate::book_direction::DIRECTION_ERROR)
        .and_then(|rest| rest.strip_prefix(':'))
    {
        return Some(json!({
            "rejectedFor":crate::book_direction::DIRECTION_ERROR,
            "instruction":"上一版方向没有通过检查：逐条修正 problems 里的问题，返回完整的新方向。",
            "problems":problems.split('；').collect::<Vec<_>>()
        }));
    }
    // CRAFT v3: the r2 plan checks list every problem for one corrected plan.
    if let Some(problems) = code
        .strip_prefix(crate::craft_plan::PLAN_CRAFT_ERROR)
        .and_then(|rest| rest.strip_prefix(':'))
    {
        return Some(json!({
            "rejectedFor":crate::craft_plan::PLAN_CRAFT_ERROR,
            "instruction":"上一版计划没有通过写作决定检查：逐条修正 problems 里的问题，返回完整的新计划。",
            "problems":problems.split('；').collect::<Vec<_>>()
        }));
    }
    // QC-1 (fourth acceptance run): any contract violation the checks above
    // missed is sent back too, so a new schema rule never fails a chapter
    // without a replan. Validator messages can quote the whole plan; the end
    // says what is wrong.
    if let Some(violation) = ["B08_PLANNING_CONTRACT:", "CHAPTER_PLAN_CONTRACT:"]
        .iter()
        .find_map(|prefix| code.strip_prefix(prefix))
    {
        let violation = violation
            .strip_prefix("CHAPTER_PLAN_CONTRACT:")
            .unwrap_or(violation);
        let count = violation.chars().count();
        let tail = violation
            .chars()
            .skip(count.saturating_sub(PLAN_VIOLATION_CHARACTERS))
            .collect::<String>();
        return Some(json!({
            "rejectedFor":"CHAPTER_PLAN_CONTRACT",
            "instruction":"上一版计划不符合计划格式：按 problems 修正，返回完整的新计划。",
            "problems":[if count > PLAN_VIOLATION_CHARACTERS { format!("…{tail}") } else { tail }]
        }));
    }
    REPLANNABLE
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, instruction)| json!({"rejectedFor":code,"instruction":instruction}))
}

/// How much of a plan contract violation goes back to the planner.
const PLAN_VIOLATION_CHARACTERS: usize = 160;

/// Adds the craft planning inputs to a planner request.
pub fn extend_planning_input(
    input: &mut Value,
    chapter_number: Option<u32>,
    recent_chapters: &[Value],
    range: ChapterLengthRange,
) -> Result<(), String> {
    if let (Some(input), Value::Object(extra)) = (
        input.as_object_mut(),
        planning_inputs(chapter_number, recent_chapters, range)?,
    ) {
        input.extend(extra);
    }
    Ok(())
}

/// CRAFT v3 plans (`chapter-plan.r2`) carry writing decisions instead of
/// scene/beat length estimates, so the r1 length and beat checks skip them.
pub fn is_craft_plan(plan: &Value) -> bool {
    plan.get("payloadSchema").and_then(Value::as_str) == Some("chapter-plan.r2")
}

/// The length and craft checks a plan must pass before it is written.
pub fn check_plan(
    plan: &Value,
    range: ChapterLengthRange,
    recent_chapters: &[Value],
) -> Result<(), String> {
    crate::chapter_length::validate_plan_length(plan, range)?;
    validate_plan_craft(plan, recent_chapters)
}

/// LD-2: prose with a paragraph over 300 characters, a chapter barely
/// broken into paragraphs (real run round 22: one paragraph; round 26: four
/// of 400 to 600 characters, where a chapter's run 40 to 150).
pub fn is_unbroken(body: &str) -> bool {
    body.lines()
        .any(|paragraph| paragraph.chars().count() > 300)
}

/// Real run round 32: Markdown the writer leaves in the prose, which the
/// signing check rejects (F1-2): a line of only `*`, `-` or `_` (a scene
/// break) goes, leaving the blank line; heading, bold and list marks go.
fn strip_markdown(body: &str) -> String {
    let lines = body
        .lines()
        .filter(|line| {
            let line = line.trim();
            line.is_empty()
                || !line
                    .chars()
                    .all(|c| matches!(c, '*' | '-' | '_' | '＊' | ' '))
        })
        .map(|line| {
            let indent = &line[..line.len() - line.trim_start().len()];
            let text = line.trim_start();
            let text = text.trim_start_matches('#');
            let text = if text.len() < line.trim_start().len() {
                text.trim_start()
            } else {
                text
            };
            let text = text
                .strip_prefix("- ")
                .or_else(|| text.strip_prefix("* "))
                .unwrap_or(text);
            format!("{indent}{}", text.replace("**", ""))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut stripped = lines;
    while stripped.contains("\n\n\n") {
        stripped = stripped.replace("\n\n\n", "\n\n");
    }
    stripped
}

/// Chinese typography for committed prose (owner rule: signing checks forbid
/// mixed full/half-width punctuation and repeated punctuation): half-width
/// punctuation becomes full-width, and a period touching ASCII letters or
/// digits becomes `．` so labels, decimals and dates survive.
pub fn normalize_typography(body: &str) -> String {
    let body = strip_markdown(body);
    let chars = body.chars().collect::<Vec<_>>();
    let ascii_alnum = |index: Option<usize>| {
        index
            .and_then(|index| chars.get(index))
            .is_some_and(char::is_ascii_alphanumeric)
    };
    let mut converted = String::with_capacity(body.len());
    let mut index = 0;
    while index < chars.len() {
        let character = chars[index];
        let digit = |index: Option<usize>| {
            index
                .and_then(|index| chars.get(index))
                .is_some_and(char::is_ascii_digit)
        };
        // A decimal point or clock colon between digits is not punctuation.
        if matches!(character, '.' | ':') && digit(index.checked_sub(1)) && digit(Some(index + 1)) {
            converted.push(character);
            index += 1;
            continue;
        }
        if character == '.' {
            let run = chars[index..].iter().take_while(|c| **c == '.').count();
            if run >= 2 {
                converted.push_str("……");
            } else if ascii_alnum(index.checked_sub(1)) || ascii_alnum(Some(index + 1)) {
                converted.push('．');
            } else {
                converted.push('。');
            }
            index += run;
            continue;
        }
        converted.push(match character {
            ',' => '，',
            '!' => '！',
            '?' => '？',
            ':' => '：',
            ';' => '；',
            other => other,
        });
        index += 1;
    }
    let mut normalized = String::with_capacity(converted.len());
    let mut newlines = 0;
    let mut previous = None;
    for character in converted.chars() {
        if character == '\n' {
            newlines += 1;
            if newlines > 2 {
                continue;
            }
        } else {
            newlines = 0;
        }
        if previous == Some(character) && "。，！？；：".contains(character) {
            continue;
        }
        normalized.push(character);
        previous = Some(character);
    }
    normalized
}

/// The body split into sentences, each with its closing mark.
fn sentences(body: &str) -> Vec<String> {
    sentence_spans(body)
        .into_iter()
        .map(|(start, end)| body[start..end].to_string())
        .collect()
}

/// The byte spans of `sentences`, trimmed, in order.
pub(crate) fn sentence_spans(body: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut push = |start: usize, end: usize| {
        let text = &body[start..end];
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            let from = start + (text.len() - text.trim_start().len());
            spans.push((from, from + trimmed.len()));
        }
    };
    let mut start = 0;
    let mut characters = body.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if character == '\n' {
            push(start, index);
            start = index + 1;
        } else if matches!(character, '。' | '！' | '？' | '…') {
            // A closing quote or a doubled mark after the mark belongs to
            // this sentence (2026-10-25: split off alone, it matched every
            // repeated line).
            let mut end = index + character.len_utf8();
            while let Some((next, mark)) = characters
                .next_if(|&(_, mark)| matches!(mark, '”' | '’' | '」' | '』' | '…' | '！' | '？'))
            {
                end = next + mark.len_utf8();
            }
            push(start, end);
            start = end;
        }
    }
    push(start, body.len());
    spans
}

/// Words the writer leans on across chapters (2026-10-19 reading).
const BODY_TICS: [&str; 8] = [
    "喉结",
    "指尖",
    "指腹",
    "指节",
    "铁锈",
    "瞳孔",
    "攥紧",
    "深吸一口气",
];
const SIMILE_MARKERS: [&str; 6] = ["像", "仿佛", "如同", "宛如", "似的", "好似"];
const NOT_SIMILES: [&str; 16] = [
    "图像",
    "影像",
    "录像",
    "摄像",
    "成像",
    "画像",
    "肖像",
    "偶像",
    "镜像",
    "头像",
    "像素",
    "好像",
    "不像",
    "想像",
    "像往常",
    "类似",
];
const MEASURE_UNITS: [&str; 10] = [
    "毫米", "厘米", "公分", "赫兹", "分钟", "公里", "秒", "步", "米", "度",
];

pub(crate) fn is_numeral(character: char) -> bool {
    character.is_ascii_digit() || "零〇一二两三四五六七八九十百千万".contains(character)
}

/// Pseudo-precise measures in a sentence: a number right before a unit.
fn measure_count(sentence: &str) -> usize {
    let chars = sentence.chars().collect::<Vec<_>>();
    (1..chars.len())
        .filter(|&index| is_numeral(chars[index - 1]) && !is_numeral(chars[index]))
        .filter(|&index| {
            let rest = chars[index..].iter().collect::<String>();
            MEASURE_UNITS.iter().any(|unit| rest.starts_with(unit))
        })
        .count()
}

fn simile_count(sentence: &str) -> usize {
    let mut cleaned = sentence.to_string();
    for word in NOT_SIMILES {
        cleaned = cleaned.replace(word, "");
    }
    SIMILE_MARKERS
        .iter()
        .map(|marker| cleaned.matches(marker).count())
        .sum()
}

/// Body tics the recent chapters used twice or more, for the writer to
/// leave out of the next chapter.
pub fn overused_tics(history: &[String]) -> Vec<String> {
    BODY_TICS
        .iter()
        .filter(|tic| {
            history
                .iter()
                .map(|text| text.matches(*tic).count())
                .sum::<usize>()
                >= 2
        })
        .map(|tic| tic.to_string())
        .collect()
}

/// How many of the model prose habits the rule findings look for appear in
/// `text`: similes, body tics, pseudo-precise measures and panel brackets.
pub fn habit_count(text: &str) -> usize {
    simile_count(text)
        + BODY_TICS
            .iter()
            .map(|tic| text.matches(tic).count())
            .sum::<usize>()
        + measure_count(text)
        + text.matches('【').count()
        + STOCK_TROPES
            .iter()
            .map(|trope| text.matches(trope).count())
            .sum::<usize>()
}

/// Sentences that occur once in the body, so an edit can quote them.
fn unique_sentences(body: &str) -> Vec<String> {
    sentences(body)
        .into_iter()
        .filter(|sentence| body.matches(sentence.as_str()).count() == 1)
        .collect()
}

/// One rule finding, in the shape the review findings use.
fn rule_finding(
    code: &str,
    severity: &str,
    quote: &str,
    problem: String,
    suggestion: &str,
) -> Value {
    json!({"code":code,"severity":severity,"quote":quote,"problem":problem,"suggestion":suggestion,"source":"rule"})
}

const CLAUSE_SEPARATORS: [char; 7] = ['，', '。', '；', '！', '？', '、', '：'];

/// The clauses of `texts` with at least `min_chars` characters.
fn long_clauses(texts: &[String], min_chars: usize) -> impl Iterator<Item = &str> {
    texts
        .iter()
        .flat_map(|text| text.split(CLAUSE_SEPARATORS))
        .map(str::trim)
        .filter(move |clause| clause.chars().count() >= min_chars)
}

/// Owner decision (2026-10-19): habits of model prose the review model keeps
/// missing, found by rule and sent to the rewrite with the review findings:
/// plan wording copied into the prose, pseudo-precise measures, simile
/// overload, body tics used more than once, and system-panel brackets.
/// Each finding quotes one sentence that occurs once.
pub fn rule_findings(plan: &Value, body: &str) -> Vec<Value> {
    let sentences = unique_sentences(body);
    let mut findings = Vec::new();
    let mut flagged = std::collections::BTreeSet::new();
    let mut flag =
        |sentence: &str, code: &str, severity: &str, problem: String, suggestion: &str| {
            if flagged.insert(sentence.to_string()) {
                findings.push(rule_finding(code, severity, sentence, problem, suggestion));
            }
        };
    // Plan wording: any clause of 8 or more characters taken over verbatim.
    let mut plan_texts = vec![plan
        .get("chapterGoal")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()];
    for scene in plan
        .get("sceneExecution")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for key in ["goal", "conflict", "turn", "result"] {
            plan_texts.extend(
                scene
                    .get(key)
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            );
        }
        plan_texts.extend(
            scene
                .get("beats")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned),
        );
    }
    // CRAFT v3 plans carry their decisions under `craft`.
    collect_strings(&plan["craft"], &mut plan_texts);
    for clause in long_clauses(&plan_texts, 8) {
        if let Some(sentence) = sentences.iter().find(|sentence| sentence.contains(clause)) {
            flag(
                sentence,
                "summary_where_scene_needed",
                "HIGH",
                format!("照搬了章节计划的表述「{clause}」"),
                "用动作、细节和对话把这个变化演出来",
            );
        }
    }
    // Measures beyond the first two.
    let mut measures = 0;
    for sentence in &sentences {
        let count = measure_count(sentence);
        measures += count;
        if count > 0 && measures > 2 {
            flag(
                sentence,
                "prose_wordy",
                "MEDIUM",
                "精确到单位的数字过多，像在读仪器读数".into(),
                "去掉不必要的数字，只留对情节有用的",
            );
        }
    }
    // Similes beyond three per chapter, or three per thousand characters.
    let allowed = (body.chars().count() * 3 / 1000).max(3);
    let mut similes = 0;
    for sentence in &sentences {
        let count = simile_count(sentence);
        similes += count;
        if count > 0 && similes > allowed {
            flag(
                sentence,
                "prose_wordy",
                "MEDIUM",
                "比喻过密".into(),
                "删去这个比喻，或换成直接的动作或细节",
            );
        }
    }
    // Body tics: every use after the first in this chapter (2026-10-20).
    for tic in BODY_TICS {
        if body.matches(tic).count() >= 2 {
            for sentence in sentences
                .iter()
                .filter(|sentence| sentence.contains(tic))
                .skip(1)
            {
                flag(
                    sentence,
                    "word_repetition",
                    "MEDIUM",
                    format!("「{tic}」在本章和最近几章反复出现"),
                    "换成这个人物此刻独有的反应",
                );
            }
        }
    }
    // System-panel brackets beyond three.
    for sentence in sentences
        .iter()
        .filter(|sentence| sentence.contains('【'))
        .skip(3)
    {
        flag(
            sentence,
            "info_dump",
            "MEDIUM",
            "系统提示框式文字过多".into(),
            "把信息融进人物的观察或对话",
        );
    }
    findings.truncate(8);
    findings
}

/// Every string in a JSON value, depth first.
pub(crate) fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| collect_strings(item, out)),
        Value::Object(map) => map.values().for_each(|item| collect_strings(item, out)),
        _ => {}
    }
}

/// Stock devices and stock phrasings readers recognise at once
/// (CRAFT_V3_DESIGN §4.4 trope blacklist).
const STOCK_TROPES: &[&str] = &[
    "灰衣人",
    "灰色外套的男人跟",
    "如果你看到这张纸条",
    "如果你看到这个",
    "警笛声由远及近",
    "匿名短信",
    "不要相信任何人",
    "别相信任何人",
    "倒吸一口凉气",
    "嘴角勾起一抹",
    "空气仿佛凝固",
    "眼中闪过一丝",
    "心中一凛",
];
/// Narrator asides that tell what the point-of-view character cannot know.
const AUTHOR_ASIDES: &[&str] = &[
    "他不知道的是",
    "她不知道的是",
    "殊不知",
    "他并不知道",
    "她并不知道",
];
/// A phrase of this many characters used a third time in one chapter is a loop.
const LOOP_CHARACTERS: usize = 8;
/// Clauses of this many characters copied from the story bible.
const BIBLE_CLAUSE_CHARACTERS: usize = 10;

/// The post-write rule checks of a CRAFT v3 chapter: the r1 rules (with the
/// plan's craft wording), stock tropes, author asides under a strict point of
/// view, in-chapter loops and wording copied from the story bible.
pub fn craft_rule_findings(
    plan: &Value,
    body: &str,
    bible: &Value,
    style_card: Option<&Value>,
) -> Vec<Value> {
    let mut findings = rule_findings(plan, body);
    let unique = unique_sentences(body);
    let mut flag = |sentence: &str, code: &str, problem: String, suggestion: &str| {
        if !findings.iter().any(|finding| finding["quote"] == sentence) {
            findings.push(rule_finding(code, "MEDIUM", sentence, problem, suggestion));
        }
    };
    for trope in STOCK_TROPES {
        if let Some(sentence) = unique.iter().find(|sentence| sentence.contains(trope)) {
            flag(
                sentence,
                "repeated_pattern",
                format!("「{trope}」是读者一眼认得出的套路"),
                "换成这个故事独有的事件或反应",
            );
        }
    }
    let strict = style_card.is_some_and(|card| card["narration"]["povPolicy"] == "STRICT");
    if strict {
        for aside in AUTHOR_ASIDES {
            for sentence in unique.iter().filter(|sentence| sentence.contains(aside)) {
                flag(
                    sentence,
                    "pov_info_leak",
                    format!("「{aside}」写了主角不知道的事"),
                    "删掉，或改成主角此刻能看到、听到的线索",
                );
            }
        }
    }
    for (phrase, second_use) in looped_phrases(body) {
        if let Some(sentence) = unique.iter().find(|sentence| {
            sentence.contains(&phrase)
                && body
                    .find(sentence.as_str())
                    .is_some_and(|start| start > second_use)
        }) {
            flag(
                sentence,
                "redundancy",
                format!("「{phrase}」在本章第三次出现"),
                "删去或换一种说法，不要反复确认同一件事",
            );
        }
    }
    let mut bible_texts = Vec::new();
    collect_strings(bible, &mut bible_texts);
    for clause in long_clauses(&bible_texts, BIBLE_CLAUSE_CHARACTERS) {
        if let Some(sentence) = unique.iter().find(|sentence| sentence.contains(clause)) {
            flag(
                sentence,
                "summary_where_scene_needed",
                format!("照搬了故事底稿的表述「{clause}」"),
                "用这一刻的具体动作和对话来写",
            );
        }
    }
    findings.truncate(8);
    findings
}

/// Phrases of `LOOP_CHARACTERS` characters (no punctuation) used three or
/// more times in the body, in first-use order, with the byte offset of the
/// second use; one pass over the body.
fn looped_phrases(body: &str) -> Vec<(String, usize)> {
    let characters = body.char_indices().collect::<Vec<_>>();
    let mut uses: std::collections::HashMap<String, Vec<usize>> = Default::default();
    let mut order = Vec::new();
    for window in characters.windows(LOOP_CHARACTERS) {
        if window
            .iter()
            .any(|(_, character)| "，。！？、；：…“”\n ".contains(*character))
        {
            continue;
        }
        let phrase = window
            .iter()
            .map(|(_, character)| character)
            .collect::<String>();
        let positions = uses.entry(phrase.clone()).or_default();
        if positions.is_empty() {
            order.push(phrase);
        }
        positions.push(window[0].0);
    }
    order
        .into_iter()
        .filter_map(|phrase| {
            let positions = &uses[&phrase];
            (positions.len() >= 3).then(|| (phrase.clone(), positions[1]))
        })
        .collect()
}

/// Chapter-level rhythm compared with the style card's targets. These are
/// notes for the receipt and the prose optimizer, not quote edits.
pub fn rhythm_notes(body: &str, style_card: &Value) -> Vec<String> {
    let metrics = crate::style_card::measure(body);
    let target = |key: &str, bound: &str| style_card["targets"][key][bound].as_f64();
    let mut notes = Vec::new();
    if let Some(max) = target("avgParagraphChars", "max") {
        if metrics.avg_paragraph_chars > max + 6.0 {
            notes.push(format!(
                "段落偏长：平均每段 {} 字，风格卡上限 {max} 字",
                metrics.avg_paragraph_chars
            ));
        }
    }
    if let Some(min) = target("shortParagraphPct", "min") {
        if metrics.short_paragraph_pct < min {
            notes.push(format!(
                "短段太少：只占 {}%，风格卡至少 {min}%",
                metrics.short_paragraph_pct
            ));
        }
    }
    if metrics.exclaim_per_1k == 0.0
        && metrics.question_per_1k == 0.0
        && metrics.ellipsis_per_1k == 0.0
    {
        notes.push("语气平：全章没有感叹、问句和省略".into());
    }
    notes
}

/// Characters in a phrase the signing check counts as a repetition.
pub(crate) const PHRASE_CHARACTERS: usize = 12;
/// The use of a phrase window that fails signing F1-5.
pub(crate) const REPEATED_PHRASE_TIMES: usize = 3;

/// F1-5 checks the complete opening. Each text is counted separately so no
/// window is formed across the boundary between chapters.
pub(crate) fn signing_repeated_phrase_windows<'a>(
    bodies: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let texts = bodies
        .into_iter()
        .map(|body| body.chars().collect())
        .collect::<Vec<_>>();
    let mut windows = phrase_window_counts(&texts)
        .into_iter()
        .filter(|(_, count)| *count >= REPEATED_PHRASE_TIMES)
        .map(|(window, _)| window.iter().collect())
        .collect::<Vec<_>>();
    windows.sort_unstable();
    windows
}

/// The exact contiguous duplicate threshold used by signing check C0-3.
pub(crate) const ORIGINALITY_SPAN_CHARS: usize = 100;

fn originality_windows(body: &str) -> impl Iterator<Item = &str> {
    let offsets = body
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(body.len()))
        .collect::<Vec<_>>();
    (0..offsets.len().saturating_sub(ORIGINALITY_SPAN_CHARS))
        .map(move |start| &body[offsets[start]..offsets[start + ORIGINALITY_SPAN_CHARS]])
}

/// Check each candidate against earlier candidate windows and the reference
/// corpus. Duplicates that occur only inside the reference corpus are irrelevant.
pub(crate) fn originality_duplicate<'a>(
    references: impl IntoIterator<Item = &'a str>,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let mut seen: HashSet<&str> = references
        .into_iter()
        .flat_map(originality_windows)
        .collect();
    candidates
        .into_iter()
        .flat_map(originality_windows)
        .find(|span| !seen.insert(*span))
}

pub(crate) fn duplicate_in_chapter(body: &str) -> Option<&str> {
    originality_duplicate(std::iter::empty(), [body])
}

/// A window mostly of ASCII letters, digits and hyphens is an identifier
/// (an archive number, an account name), not a repeated phrase (owner
/// decision 2026-10-24).
pub(crate) fn is_identifier_window(window: &[char]) -> bool {
    window
        .iter()
        .filter(|c| c.is_ascii_alphanumeric() || **c == '-')
        .count()
        * 2
        >= window.len()
}

/// Phrases of `PHRASE_CHARACTERS` or more characters that already occur at
/// least twice in `texts`, so the writer can avoid a third use.
pub fn repeated_phrases(texts: &[String]) -> Vec<String> {
    phrases_used(texts, 0, 2)
}

/// Phrases of `text` whose third (or later) use, counting `history`, would
/// fail the signing repetition check (F1-5).
pub fn overused_phrases(history: &[String], text: &str) -> Vec<String> {
    let texts = history
        .iter()
        .cloned()
        .chain([text.to_string()])
        .collect::<Vec<_>>();
    phrases_used(&texts, history.len(), REPEATED_PHRASE_TIMES)
}

/// An exact F1-5 window that the edit added another instance of. Existing
/// repeats in the saved draft are left for the prose review to address.
pub(crate) fn added_overused_phrase(
    history: &[String],
    before: &str,
    after: &str,
) -> Option<String> {
    let mut texts = history
        .iter()
        .map(|text| text.chars().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    texts.push(before.chars().collect());
    let old_counts = phrase_window_counts(&texts);
    let mut edited = texts[..history.len()].to_vec();
    let after = after.chars().collect::<Vec<_>>();
    edited.push(after);
    let new_counts = phrase_window_counts(&edited);
    edited
        .last()?
        .windows(PHRASE_CHARACTERS)
        .find(|&window| {
            let new = new_counts.get(window).copied().unwrap_or(0);
            new >= REPEATED_PHRASE_TIMES && new > old_counts.get(window).copied().unwrap_or(0)
        })
        .map(|window| window.iter().collect())
}

/// Measure the whole candidate, independent of the display limit and how
/// the same covered characters are grouped for rewrite suggestions.
pub(crate) fn repeated_character_count(history: &[String], body: &str) -> usize {
    let texts = history
        .iter()
        .map(String::as_str)
        .chain([body])
        .map(|text| text.chars().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let counts = phrase_window_counts(&texts);
    repeated_coverage(
        texts.last().expect("candidate included"),
        &counts,
        REPEATED_PHRASE_TIMES,
    )
    .into_iter()
    .filter(|covered| *covered)
    .count()
}

/// Rule findings the rewrite must fix, one for each sentence of `body` that
/// uses one of `phrases` (from `overused_phrases`): given only the phrases,
/// a real rewrite left them alone (2026-10-24). A sentence used more than
/// once in the chapter is quoted with the sentences before it until the
/// quote is unique (2026-10-25), so the rewrite can find each use.
pub fn repetition_findings(phrases: &[String], body: &str) -> Vec<Value> {
    if phrases.is_empty() {
        return Vec::new();
    }
    let spans = sentence_spans(body);
    spans
        .iter()
        .enumerate()
        .filter_map(|(index, &(start, end))| {
            let sentence = &body[start..end];
            let phrase = phrases
                .iter()
                .find(|phrase| uses_phrase(sentence, phrase))?;
            let quote = unique_quote_ending_at(body, &spans, index)?;
            Some(rule_finding(
                "word_repetition",
                "MEDIUM",
                quote,
                format!("「{phrase}」在最近几章已经出现两次，这是第三次"),
                "换一种说法写这句，意思和信息不变",
            ))
        })
        .collect()
}

/// Sentence `index` of `body` (spans from `sentence_spans`), with the
/// sentences before it until the quote occurs only once.
fn unique_quote_ending_at<'a>(
    body: &'a str,
    spans: &[(usize, usize)],
    index: usize,
) -> Option<&'a str> {
    let end = spans[index].1;
    (0..=index)
        .rev()
        .map(|from| &body[spans[from].0..end])
        .find(|quote| body.matches(quote).nth(1).is_none())
}

/// QC-1.2 (fifth acceptance run): when chapter 1's opening does not show
/// its planned threat, a finding on the first sentence asks the rewrite to
/// add it right after, once; what the rewrite leaves is reported.
pub fn opening_threat_finding(plan: &Value, body: &str) -> Option<Value> {
    if opening_threat_on_page(plan, body) {
        return None;
    }
    let threat = threat_as_planned(plan)?;
    let spans = sentence_spans(body);
    if spans.is_empty() {
        return None;
    }
    Some(rule_finding(
        "opening_threat",
        "HIGH",
        unique_quote_ending_at(body, &spans, 0)?,
        format!("开头 600 字内没有写出迫近的威胁：{threat}"),
        "保留这句原文，紧接着用一两句写出这件事和期限（按开头写法：通知、电话或有人当面说出），其余不变",
    ))
}

/// A Chinese character of the common block.
pub(crate) fn is_han(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

/// Characters too common to tie a passage to a fact.
const COMMON_CHARACTERS: &str =
    "的了着过在是有不没也都就还又和与及把被给让向对从到为他她它我你们这那个一上下里中正很再已";

/// The pairs of adjacent Chinese characters in `text` that can tie a
/// passage to it.
pub(crate) fn telling_pairs(text: &str) -> BTreeSet<(char, char)> {
    let chars = text.chars().collect::<Vec<_>>();
    chars
        .windows(2)
        .filter(|pair| {
            pair.iter()
                .all(|c| is_han(*c) && !COMMON_CHARACTERS.contains(*c))
        })
        .map(|pair| (pair[0], pair[1]))
        .collect()
}

/// QC-1.5: the most contradictions a chapter's check may keep, and the
/// most it may report before the whole check is distrusted.
const MAX_CONTRADICTIONS: usize = 5;
const UNTRUSTED_CONTRADICTIONS: usize = 15;

/// QC-1.5 (2026-10-26, third acceptance: 169 reported, nearly all
/// restatements, omissions or facts of the same chapter): the reported
/// contradictions that hold up, as `{sentence, chapter, established,
/// conflict}`. The check cites a sentence and an `earlierFacts` entry by
/// number; a report is kept only when both exist, the fact is from an
/// earlier chapter than `chapter`, the check says the two cannot both be
/// true, they share a telling character pair (the same matter), the
/// sentence does not just restate the fact, and the sentence was not kept
/// already; at most `MAX_CONTRADICTIONS`. A check reporting more than
/// `UNTRUSTED_CONTRADICTIONS` is not trusted at all (`Err`).
pub fn credible_contradictions(
    reported: &[Value],
    earlier_facts: &[Value],
    sentences: &[String],
    chapter: i64,
) -> Result<Vec<Value>, String> {
    if reported.len() > UNTRUSTED_CONTRADICTIONS {
        return Err(format!("QC1_CONTINUITY_UNTRUSTED:{}", reported.len()));
    }
    let squeeze = |text: &str| text.chars().filter(|c| is_han(*c)).collect::<String>();
    let mut kept = Vec::<Value>::new();
    for item in reported {
        if item["impossibleTogether"] != json!(true) {
            continue;
        }
        let Some(number) = item["sentence"].as_u64() else {
            continue;
        };
        let (Some(sentence), Some(fact)) = (
            sentences.get(number as usize),
            item["fact"]
                .as_u64()
                .and_then(|index| earlier_facts.get(index as usize)),
        ) else {
            continue;
        };
        let from = fact["chapter"].as_i64().unwrap_or(chapter);
        let established = fact["fact"].as_str().unwrap_or("");
        let (said, fixed) = (squeeze(sentence), squeeze(established));
        let restates = said.contains(&fixed) || fixed.contains(&said);
        if from >= chapter
            || restates
            || telling_pairs(sentence).is_disjoint(&telling_pairs(established))
            || kept.iter().any(|other| other["sentence"] == json!(number))
        {
            continue;
        }
        kept.push(json!({
            "sentence":number,
            "chapter":from,
            "established":established,
            "conflict":item["conflict"]
        }));
        if kept.len() == MAX_CONTRADICTIONS {
            break;
        }
    }
    Ok(kept)
}

/// QC-1.5: what earlier chapters fixed, numbered for the continuity check
/// to cite (`{n, chapter, subject, fact}`): the established details, then
/// the ledger's facts that name the chapter they come from.
pub fn earlier_facts(established: &Value, ledger: &[Value]) -> Vec<Value> {
    let details = established.as_array().into_iter().flatten().map(|detail| {
        (
            detail["chapter"].clone(),
            detail["subject"].clone(),
            detail["detail"].as_str().unwrap_or("").to_owned(),
        )
    });
    let facts = ledger
        .iter()
        .filter(|fact| fact["chapter"].is_i64())
        .map(|fact| {
            let mut parts = Vec::new();
            collect_strings(&fact["value"], &mut parts);
            (
                fact["chapter"].clone(),
                fact["subject"].clone(),
                format!(
                    "{}：{}",
                    fact["predicate"].as_str().unwrap_or(""),
                    parts.join("，")
                ),
            )
        });
    details
        .chain(facts)
        .filter(|(_, _, fact)| !fact.is_empty())
        .enumerate()
        .map(|(n, (chapter, subject, fact))| {
            json!({"n":n,"chapter":chapter,"subject":subject,"fact":fact})
        })
        .collect()
}

/// QC-1 (owner decision 2026-10-30): how often one object or detail may be
/// written: in a chapter (as the writer is told), and in a chapter of a run
/// of four that all write it more than `DETAIL_WINDOW_TOTAL` times in all.
const DETAIL_PER_CHAPTER: usize = 3;
const DETAIL_PER_CHAPTER_IN_A_RUN: usize = 2;
const DETAIL_WINDOW_TOTAL: usize = 10;
/// Characters that make a phrase an action or a grammar fragment (把纤维,
/// 他伸手, 了一下) rather than an object or a detail. Not `COMMON_CHARACTERS`:
/// that set holds 一, 上, 下 and 里, which details such as 左手小指第一节 use.
const NOT_A_DETAIL: &str = "把被给让了着过他她它我你们的地得在是";
/// The most detail findings a chapter sends to the rewrite, so they leave
/// room for the others.
pub(crate) const MAX_DETAIL_FINDINGS: usize = 6;

/// QC-1 (owner decision 2026-10-30; fifth acceptance run: 存钱罐 eleven
/// times in a chapter, 左手小指第一节 in every chapter): each sentence of
/// `body` that writes an object or detail (3 to 6 Chinese characters, never
/// a person's name, an action or one of `catchphrases`, which keep a
/// stricter rule) past its allowance (`DETAIL_PER_CHAPTER`, or
/// `DETAIL_PER_CHAPTER_IN_A_RUN` when the last three chapters and this one
/// all write it and more than `DETAIL_WINDOW_TOTAL` times together), for the
/// rewrite to word it differently or leave it out; the object stays. At most
/// `MAX_DETAIL_FINDINGS`, taken from the phrases in turn. Also returns the
/// details that ran through the four chapters, for the next writer.
pub fn detail_repetition_findings(
    history: &[String],
    body: &str,
    names: &[&str],
    catchphrases: &[String],
) -> (Vec<Value>, Vec<String>) {
    let skip = |phrase: &str| {
        phrase.chars().any(|c| NOT_A_DETAIL.contains(c))
            || names
                .iter()
                .any(|name| phrase.contains(name) || name.contains(phrase))
            || catchphrases.iter().any(|catchphrase| {
                catchphrase.contains(phrase) || phrase.contains(catchphrase.as_str())
            })
    };
    let mut over = Vec::new();
    for (phrase, here, earlier) in repeated_han_phrases(history, body, 2, skip) {
        let in_a_run = earlier.len() == CATCHPHRASE_WINDOW
            && earlier.iter().all(|count| *count > 0)
            && earlier.iter().sum::<usize>() + here > DETAIL_WINDOW_TOTAL;
        let allowed = if in_a_run {
            DETAIL_PER_CHAPTER_IN_A_RUN
        } else {
            DETAIL_PER_CHAPTER
        };
        if here > allowed {
            over.push((phrase, allowed, in_a_run));
        }
    }
    // The longest phrase speaks for those inside it.
    over.sort_by_key(|(phrase, _, _)| std::cmp::Reverse(phrase.chars().count()));
    let mut chosen = Vec::<(String, usize, bool)>::new();
    for item in over {
        if !chosen
            .iter()
            .any(|(longer, _, _)| longer.contains(item.0.as_str()))
        {
            chosen.push(item);
        }
    }
    let allowances = chosen
        .iter()
        .map(|(phrase, allowed, in_a_run)| {
            let problem = if *in_a_run {
                format!("「{phrase}」最近几章每章都在写，本章写 {allowed} 次就够")
            } else {
                format!("「{phrase}」本章已经写了 {allowed} 次")
            };
            (phrase.clone(), *allowed, problem)
        })
        .collect::<Vec<_>>();
    let mut per_phrase = excess_use_findings(
        &allowances,
        body,
        "detail_repetition",
        "换个说法提到它（代称、动作或干脆省去），物件本身和信息不变",
    )
    .into_iter()
    .map(Vec::into_iter)
    .collect::<Vec<_>>();
    // The slots go round the phrases, so one cannot take them all.
    let mut findings = Vec::new();
    while findings.len() < MAX_DETAIL_FINDINGS {
        let before = findings.len();
        for phrase_findings in &mut per_phrase {
            if findings.len() < MAX_DETAIL_FINDINGS {
                findings.extend(phrase_findings.next());
            }
        }
        if findings.len() == before {
            break;
        }
    }
    let running = chosen
        .into_iter()
        .filter(|(_, _, in_a_run)| *in_a_run)
        .map(|(phrase, _, _)| phrase)
        .collect();
    (findings, running)
}

/// RP-1 §5 (owner decision 2026-11-02, option A): how often a two-character
/// word must appear in the chapter, in how many of this and the recent
/// chapters, and how many candidates the continuity check judges.
const MOTIF_MIN_HERE: usize = 5;
const MOTIF_SPREAD: usize = 3;
const MAX_MOTIF_CANDIDATES: usize = 12;

/// RP-1 §5: two-character words (鳞斑, 金星) the chapter writes at least
/// `MOTIF_MIN_HERE` times that also ran through the recent chapters (in at
/// least `MOTIF_SPREAD` of the four), never part of a name nor a grammar
/// pair, most used first. Many are ordinary words (起来, 声音), so the
/// continuity check says which are images written to excess.
/// `{word, thisChapter, chapters}` each.
pub fn motif_candidates(history: &[String], body: &str, names: &[&str]) -> Vec<Value> {
    let chars = body.chars().collect::<Vec<_>>();
    let mut counts = BTreeMap::<String, usize>::new();
    for pair in chars.windows(2) {
        if pair
            .iter()
            .all(|c| is_han(*c) && !COMMON_CHARACTERS.contains(*c) && !NOT_A_DETAIL.contains(*c))
        {
            *counts.entry(pair.iter().collect()).or_default() += 1;
        }
    }
    let mut candidates = counts
        .into_iter()
        .filter(|(word, here)| {
            *here >= MOTIF_MIN_HERE && !names.iter().any(|name| name.contains(word.as_str()))
        })
        .map(|(word, here)| {
            let chapters = 1 + history
                .iter()
                .take(CATCHPHRASE_WINDOW)
                .filter(|text| text.contains(word.as_str()))
                .count();
            (word, here, chapters)
        })
        .filter(|(_, _, chapters)| *chapters >= MOTIF_SPREAD)
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(_, here, _)| std::cmp::Reverse(*here));
    candidates
        .into_iter()
        .take(MAX_MOTIF_CANDIDATES)
        .map(|(word, here, chapters)| json!({"word":word,"thisChapter":here,"chapters":chapters}))
        .collect()
}

/// RP-1 §5: each sentence writing one of `motifs` (the images the
/// continuity check found overused) past `DETAIL_PER_CHAPTER`, for the
/// rewrite to word differently, as an overused detail is.
pub fn motif_findings(motifs: &[String], body: &str) -> Vec<Value> {
    let allowances = motifs
        .iter()
        .map(|motif| {
            (
                motif.clone(),
                DETAIL_PER_CHAPTER,
                format!("「{motif}」这几章反复在写，本章已经写了 {DETAIL_PER_CHAPTER} 次"),
            )
        })
        .collect::<Vec<_>>();
    excess_use_findings(
        &allowances,
        body,
        "detail_repetition",
        "换个说法提到它（代称、动作或干脆省去），物件本身和信息不变",
    )
    .into_iter()
    .flatten()
    .collect()
}

/// QC-1: how long a sentence copied from a recent chapter must be to count,
/// and how many chapters back copies are looked for.
const COPIED_SENTENCE_CHARACTERS: usize = 15;
const COPIED_SENTENCE_WINDOW: usize = 3;

/// An expansion must not increase the number of copies of a complete sentence
/// already present in its draft.
pub(crate) fn added_copied_sentence<'a>(before: &str, after: &'a str) -> Option<&'a str> {
    let mut old_counts = std::collections::HashMap::new();
    for (start, end) in sentence_spans(before) {
        *old_counts.entry(&before[start..end]).or_insert(0) += 1;
    }
    let mut new_counts = std::collections::HashMap::new();
    for (start, end) in sentence_spans(after) {
        let sentence = &after[start..end];
        if sentence.chars().count() >= COPIED_SENTENCE_CHARACTERS {
            let count = new_counts.entry(sentence).or_insert(0);
            *count += 1;
            if *count >= 2 && *count > old_counts.get(sentence).copied().unwrap_or(0) {
                return Some(sentence);
            }
        }
    }
    None
}

/// QC-1 (third acceptance: 14 sentences repeated word for word across
/// chapters; sixth: one written twice within chapter 1): each sentence of
/// `body` of at least `COPIED_SENTENCE_CHARACTERS` characters that occurs
/// word for word earlier in the chapter or in one of the recent chapters,
/// for the rewrite to word anew.
pub fn copied_sentence_findings(history: &[String], body: &str) -> Vec<Value> {
    let spans = sentence_spans(body);
    spans
        .iter()
        .enumerate()
        .filter_map(|(index, &(start, end))| {
            let sentence = &body[start..end];
            if sentence.chars().count() < COPIED_SENTENCE_CHARACTERS {
                return None;
            }
            let problem = if body[..start].contains(sentence) {
                "这句和本章前面的一句一字不差"
            } else if history
                .iter()
                .take(COPIED_SENTENCE_WINDOW)
                .any(|text| text.contains(sentence))
            {
                "这句和最近几章里的一句一字不差"
            } else {
                return None;
            };
            Some(rule_finding(
                "copied_sentence",
                "MEDIUM",
                unique_quote_ending_at(body, &spans, index)?,
                problem.into(),
                "换一种写法，或删去重复的交代，意思和信息不变",
            ))
        })
        .collect()
}

/// All that follows 迫近的威胁 in chapter 1's first scene.
fn threat_as_planned(plan: &Value) -> Option<&str> {
    let (_, threat) = plan
        .pointer("/craft/scenes/0/conflict")
        .and_then(Value::as_str)?
        .split_once(crate::craft_plan::OPENING_THREAT_MARK)?;
    Some(threat)
}

/// Chapter 1's planned threat: the event and its deadline that follow
/// 迫近的威胁 in its first scene, without how the opening shows it (whose
/// props alone would not show the threat).
fn planned_threat(plan: &Value) -> Option<String> {
    let threat = threat_as_planned(plan)?;
    let threat = threat
        .split_once(crate::craft_plan::THREAT_ON_PAGE)
        .map_or(threat, |(before, _)| before);
    Some(threat.replace(crate::craft_plan::THREAT_DEADLINE, ""))
}

/// QC-1.2: whether chapter 1's planned threat (the first scene's
/// 迫近的威胁) shows within the first `OPENING_THREAT_WITHIN` characters of
/// `body`, the place the writer is told to put it: at least `THREAT_PAIRS`
/// telling character pairs of it are there. Without a planned threat there
/// is nothing to check.
pub fn opening_threat_on_page(plan: &Value, body: &str) -> bool {
    const THREAT_PAIRS: usize = 3;
    let Some(threat) = planned_threat(plan) else {
        return true;
    };
    let opening = body
        .chars()
        .take(crate::craft_plan::OPENING_THREAT_WITHIN)
        .collect::<String>();
    telling_pairs(&threat)
        .intersection(&telling_pairs(&opening))
        .count()
        >= THREAT_PAIRS
}

/// QC-1.4: how many chapters back catchphrases are looked for.
const CATCHPHRASE_WINDOW: usize = 3;
/// RP-1: in how many of this and the recent chapters a catchphrase
/// candidate appears at least.
const MIN_CATCHPHRASE_SPREAD: usize = 3;
/// RP-1: how many candidates the continuity check judges.
const MAX_CATCHPHRASE_CANDIDATES: usize = 20;
/// QC-1.4 (sixth acceptance: 的时候 taken for a catchphrase): whether a
/// grammar fragment may begin or end with `c` (的时候, 了一下): the
/// characters of `NOT_A_DETAIL` and a few more. Only the edges are looked
/// at, so tics such as 愣了一下 stay candidates.
fn is_grammar_edge(c: char) -> bool {
    NOT_A_DETAIL.contains(c) || "和与就也都又".contains(c)
}

/// QC-1.4 (owner decision 2026-10-26): short phrases (3 to 6 Chinese
/// characters) the chapter uses that also ran through the recent chapters
/// (in at least 3 of the 4; RP-1: once a chapter is enough), the most
/// widespread first and at most `MAX_CATCHPHRASE_CANDIDATES`, the longest
/// of overlapping ones, never containing a name nor found in
/// `story_text` (what the bible, the plan and earlier facts name: objects,
/// places, devices). Which of them are
/// catchphrases rather than things the story names is for the continuity
/// check to say. `{phrase, thisChapter, recentChapters}` each.
pub fn catchphrase_candidates(
    history: &[String],
    body: &str,
    names: &[&str],
    story_text: &str,
) -> Vec<Value> {
    let mut found = BTreeMap::<String, (usize, usize, usize)>::new();
    for (phrase, here, earlier) in repeated_han_phrases(history, body, 1, |phrase| {
        phrase.starts_with(is_grammar_edge)
            || phrase.ends_with(is_grammar_edge)
            || names.iter().any(|name| phrase.contains(name))
            || story_text.contains(phrase)
    }) {
        let total = here + earlier.iter().sum::<usize>();
        let chapters = 1 + earlier.iter().filter(|count| **count > 0).count();
        // RP-1: a phrase used once a chapter counts too.
        if chapters >= MIN_CATCHPHRASE_SPREAD {
            found.insert(phrase, (here, total - here, chapters));
        }
    }
    let mut candidates = found
        .iter()
        .filter(|(phrase, (_, recent, _))| {
            !found.iter().any(|(longer, (_, longer_recent, _))| {
                longer.len() > phrase.len()
                    && longer.contains(phrase.as_str())
                    && longer_recent >= recent
            })
        })
        .collect::<Vec<_>>();
    // Longer phrases before common three-character ones (点点头) of the same
    // spread, so those never crowd out a real catchphrase.
    candidates.sort_by_key(|(phrase, (here, recent, chapters))| {
        std::cmp::Reverse((*chapters, phrase.chars().count(), here + recent))
    });
    candidates
        .into_iter()
        .take(MAX_CATCHPHRASE_CANDIDATES)
        .map(|(phrase, (here, recent, chapters))| {
            json!({"phrase":phrase,"thisChapter":here,"recentChapters":recent,"chapters":chapters})
        })
        .collect()
}

/// RP-1: how many recent chapters' endings, and how much of each ending,
/// a chapter's ending is compared with.
const ENDING_WINDOW: usize = 3;
const ENDING_COMPARED: usize = 200;

fn ending(body: &str) -> String {
    let count = body.chars().count();
    body.chars()
        .skip(count.saturating_sub(ENDING_COMPARED))
        .collect()
}

/// Whether at least `tenths` in 10 of the telling pairs of the shorter of
/// `a` and `b` are also in the other.
pub(crate) fn pairs_overlap(
    a: &BTreeSet<(char, char)>,
    b: &BTreeSet<(char, char)>,
    tenths: usize,
) -> bool {
    let shorter = a.len().min(b.len());
    shorter > 0 && a.intersection(b).count() * 10 >= shorter * tenths
}

/// RP-1 (real runs 13-15): a rewrite finding on the last sentence when the
/// chapter closes much like one of the latest chapters (`bodies` newest
/// first): at least 3 in 10 telling pairs of the shorter ending also in the
/// other.
pub fn ending_repetition_findings(bodies: &[String], body: &str) -> Vec<Value> {
    let here = telling_pairs(&ending(body));
    let Some(back) = bodies
        .iter()
        .take(ENDING_WINDOW)
        .position(|earlier| pairs_overlap(&here, &telling_pairs(&ending(earlier)), 3))
    else {
        return Vec::new();
    };
    let spans = sentence_spans(body);
    spans
        .len()
        .checked_sub(1)
        .and_then(|last| unique_quote_ending_at(body, &spans, last))
        .map(|quote| {
            rule_finding(
                "ending_repetition",
                "MEDIUM",
                quote,
                format!("章末收尾和往前数第 {} 章的收尾几乎一样", back + 1),
                "换一个动作、物件和画面收尾，章末钩子的事实不变",
            )
        })
        .into_iter()
        .collect()
}

/// QC-1.4: each sentence using one of `catchphrases` after the chapter's
/// first use of it, for the rewrite to word differently.
pub fn catchphrase_findings(catchphrases: &[String], body: &str) -> Vec<Value> {
    let allowances = catchphrases
        .iter()
        .map(|phrase| {
            (
                phrase.clone(),
                1,
                format!("「{phrase}」是这几章反复出现的口头禅，本章已经用过一次"),
            )
        })
        .collect::<Vec<_>>();
    excess_use_findings(
        &allowances,
        body,
        "catchphrase",
        "换一种说法写这句，或删掉这个词，意思和信息不变",
    )
    .into_iter()
    .flatten()
    .collect()
}

/// The distinct runs of 3 to 6 Chinese characters `body` uses at least
/// `min_here` times that `skip` does not refuse, each with its count in `body` and in
/// each of the recent chapters of `history` (newest first).
fn repeated_han_phrases(
    history: &[String],
    body: &str,
    min_here: usize,
    skip: impl Fn(&str) -> bool,
) -> Vec<(String, usize, Vec<usize>)> {
    let chars = body.chars().collect::<Vec<_>>();
    let mut seen = BTreeSet::new();
    let mut phrases = Vec::new();
    for size in 3..=6 {
        for window in chars.windows(size) {
            if !window.iter().all(|c| is_han(*c)) {
                continue;
            }
            let phrase = window.iter().collect::<String>();
            if !seen.insert(phrase.clone()) || skip(&phrase) {
                continue;
            }
            let here = body.matches(phrase.as_str()).count();
            if here < min_here {
                continue;
            }
            let earlier = history
                .iter()
                .take(CATCHPHRASE_WINDOW)
                .map(|text| text.matches(phrase.as_str()).count())
                .collect();
            phrases.push((phrase, here, earlier));
        }
    }
    phrases
}

/// For each `(phrase, allowed, problem)`, a finding on each sentence of
/// `body` using the phrase past its first `allowed` uses, one finding a
/// sentence across all phrases; the findings of each phrase in order.
fn excess_use_findings(
    allowances: &[(String, usize, String)],
    body: &str,
    code: &str,
    fix: &str,
) -> Vec<Vec<Value>> {
    let spans = sentence_spans(body);
    let mut flagged = BTreeSet::new();
    allowances
        .iter()
        .map(|(phrase, allowed, problem)| {
            let mut uses = 0;
            let mut findings = Vec::new();
            for (index, &(start, end)) in spans.iter().enumerate() {
                if !body[start..end].contains(phrase.as_str()) {
                    continue;
                }
                uses += 1;
                if uses <= *allowed || !flagged.insert(index) {
                    continue;
                }
                if let Some(quote) = unique_quote_ending_at(body, &spans, index) {
                    findings.push(rule_finding(code, "MEDIUM", quote, problem.clone(), fix));
                }
            }
            findings
        })
        .collect()
}

/// QC-1.5: a rewrite finding for each sentence the continuity check found
/// contradicting an earlier chapter (`{sentence, chapter, established,
/// conflict}`, `sentence` an index of `sentence_spans`).
pub fn contradiction_findings(contradictions: &[Value], body: &str) -> Vec<Value> {
    let spans = sentence_spans(body);
    contradictions
        .iter()
        .filter_map(|item| {
            let index = usize::try_from(item["sentence"].as_u64()?).ok()?;
            if index >= spans.len() {
                return None;
            }
            Some(rule_finding(
                "continuity_contradiction",
                "HIGH",
                unique_quote_ending_at(body, &spans, index)?,
                format!(
                    "和第 {} 章：{} 矛盾（{}）",
                    item["chapter"],
                    item["established"].as_str().unwrap_or(""),
                    item["conflict"].as_str().unwrap_or("")
                ),
                "改写这句，让它和前文一致；如果情况确实变了，写出变化的原因",
            ))
        })
        .collect()
}

pub(crate) fn numbered_body_sentences(body: &str) -> Vec<Value> {
    sentence_spans(body)
        .into_iter()
        .enumerate()
        .map(|(number, (start, end))| json!({"number":number,"text":&body[start..end]}))
        .collect()
}

pub(crate) fn continuity_anchors(body: &str) -> Vec<String> {
    const MAX_ANCHORS: usize = 96;
    let candidates = body
        .split(['。', '！', '？', '，', '；', '：', '、', '\n'])
        .map(|clause| {
            let chars = clause.chars().collect::<Vec<_>>();
            chars
                .windows(4)
                .filter(|window| window.iter().copied().all(is_han))
                .filter_map(|window| {
                    let anchor = window.iter().collect::<String>();
                    let specificity = telling_pairs(&anchor).len();
                    (specificity >= 2).then_some((anchor, specificity))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut frequency = BTreeMap::new();
    for (anchor, _) in candidates.iter().flatten() {
        *frequency.entry(anchor.as_str()).or_insert(0_usize) += 1;
    }
    let mut seen = BTreeSet::new();
    let mut selected = Vec::new();
    for sentence in &candidates {
        let mut ranked = sentence
            .iter()
            .filter(|(anchor, _)| frequency[anchor.as_str()] <= 2)
            .collect::<Vec<_>>();
        ranked.sort_by_key(|(anchor, specificity)| {
            (frequency[anchor.as_str()], std::cmp::Reverse(*specificity))
        });
        selected.extend(
            ranked
                .into_iter()
                .filter_map(|(anchor, _)| seen.insert(anchor).then_some(anchor))
                .take(2),
        );
    }
    if selected.len() > MAX_ANCHORS {
        return (0..MAX_ANCHORS)
            .map(|i| selected[i * selected.len() / MAX_ANCHORS].clone())
            .collect();
    }
    selected.into_iter().cloned().collect()
}

pub(crate) fn add_recalled_continuity_prose(
    prior: &mut Vec<Value>,
    chapters: &[&crate::repositories::ops::ContinuityChapter],
    body: &str,
) {
    const MAX_WINDOWS: usize = 12;
    const MAX_QUOTED_CHARS: usize = 4_800;
    const MAX_SENTENCE_CHARS: usize = 240;
    let current_chars = body.chars().collect::<Vec<_>>();
    let phrases = current_chars
        .windows(4)
        .filter(|window| window.iter().copied().all(is_han))
        .map(|window| window.iter().collect::<String>())
        .filter(|phrase| telling_pairs(phrase).len() >= 2)
        .collect::<BTreeSet<_>>();
    let current_pairs = sentence_spans(body)
        .into_iter()
        .map(|(start, end)| telling_pairs(&body[start..end]))
        .filter(|pairs| pairs.len() >= 4)
        .collect::<Vec<_>>();
    let mut passages = Vec::new();
    let mut outcome_sentences = BTreeMap::new();
    for (chapter_index, chapter) in chapters.iter().enumerate() {
        let existing = prior
            .iter()
            .find(|entry| entry["chapterNo"] == chapter.chapter_no);
        if existing.is_some_and(|entry| entry["chapterId"] != chapter.chapter_id) {
            continue;
        }
        let visible = existing
            .and_then(|entry| entry["sentences"].as_array())
            .map_or(0, Vec::len);
        let spans = sentence_spans(&chapter.body);
        let outcome_byte = chapter
            .outcome_end
            .and_then(|end| chapter.body.char_indices().nth(end.saturating_sub(1)))
            .map(|(byte, _)| byte);
        for (number, &(start, end)) in spans.iter().enumerate().skip(visible) {
            let sentence = &chapter.body[start..end];
            if sentence.chars().count() > MAX_SENTENCE_CHARS {
                continue;
            }
            if outcome_byte.is_some_and(|byte| (start..end).contains(&byte)) {
                // A recorded outcome needs no vocabulary overlap to reach review.
                passages.push((usize::MAX, 0, chapter.chapter_no, chapter_index, number));
                outcome_sentences.insert(chapter_index, number);
                continue;
            }
            let chars = sentence.chars().collect::<Vec<_>>();
            let hits = chars
                .windows(4)
                .filter(|window| window.iter().copied().all(is_han))
                .map(|window| window.iter().collect::<String>())
                .filter(|phrase| phrases.contains(phrase))
                .count();
            if hits == 0 {
                continue;
            }
            let pairs = telling_pairs(sentence);
            let shared = current_pairs
                .iter()
                .map(|other| pairs.intersection(other).count())
                .max()
                .unwrap_or_default();
            if shared >= 4 {
                passages.push((shared, hits, chapter.chapter_no, chapter_index, number));
            }
        }
    }
    passages.sort_by_key(|&(shared, hits, chapter, _, number)| {
        std::cmp::Reverse((shared, hits, chapter, number))
    });
    let mut windows: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    let mut used = 0;
    for (_, _, _, chapter, number) in passages {
        let selected = windows.entry(chapter).or_default();
        if selected.len() >= 15 || selected.contains(&number) {
            continue;
        }
        selected.extend(number.saturating_sub(1)..=number.saturating_add(1));
        used += 1;
        if used == MAX_WINDOWS {
            break;
        }
    }
    let mut quote_chars = 0;
    for (index, numbers) in windows {
        let chapter = &chapters[index];
        let existing = prior
            .iter_mut()
            .find(|entry| entry["chapterNo"] == chapter.chapter_no);
        if existing
            .as_ref()
            .is_some_and(|entry| entry["chapterId"] != chapter.chapter_id)
        {
            continue;
        }
        let visible = existing
            .as_ref()
            .and_then(|entry| entry["sentences"].as_array())
            .map_or(0, Vec::len);
        let sentences = sentence_spans(&chapter.body)
            .into_iter()
            .enumerate()
            .filter(|(number, _)| *number >= visible && numbers.contains(number))
            .filter_map(|(number, (start, end))| {
                let text = &chapter.body[start..end];
                let len = text.chars().count();
                if len > MAX_SENTENCE_CHARS || quote_chars + len > MAX_QUOTED_CHARS {
                    return None;
                }
                quote_chars += len;
                Some(json!({"number":number,"text":text}))
            })
            .collect::<Vec<_>>();
        if sentences.is_empty() {
            continue;
        }
        let outcome_sentence = outcome_sentences.get(&index).copied().filter(|number| {
            sentences
                .iter()
                .any(|sentence| sentence["number"] == json!(number))
        });
        let source = json!({"chapterId":chapter.chapter_id,"finalRef":chapter.final_ref});
        let entry = if let Some(entry) = existing {
            if let Some(existing) = entry["sentences"].as_array_mut() {
                existing.extend(sentences);
                existing.sort_by_key(|sentence| sentence["number"].as_u64());
            }
            entry["source"] = source;
            entry["excerpts"] = json!(true);
            entry
        } else {
            prior.push(
                json!({"chapterNo":chapter.chapter_no,"chapterId":chapter.chapter_id,
                             "source":source,"excerpts":true,"sentences":sentences}),
            );
            prior
                .last_mut()
                .expect("the recalled chapter was just added")
        };
        if let Some(number) = outcome_sentence {
            entry["recordedOutcomeSentence"] = json!(number);
        }
    }
}

fn numbered_prior_sentence(chapter: &Value, number: usize) -> Option<&str> {
    chapter["sentences"]
        .as_array()?
        .iter()
        .find(|sentence| sentence["number"].as_u64() == Some(number as u64))?["text"]
        .as_str()
}

/// Candidate pairs for the model to judge, including actions outside the
/// verb list and restatements that rearrange the same result.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReusedPassage {
    chapter: i64,
    prior_sentence: usize,
    sentence: usize,
}

pub(crate) fn recurring_outcome_candidates(
    prior: &[Value],
    current: &[Value],
    minimum_pairs: usize,
) -> Vec<ReusedPassage> {
    let number = |sentence: &Value| usize::try_from(sentence["number"].as_u64()?).ok();
    let earlier = prior
        .iter()
        .flat_map(|chapter| {
            chapter["sentences"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|sentence| {
                    let pairs = telling_pairs(sentence["text"].as_str()?);
                    (pairs.len() >= minimum_pairs).then_some((
                        chapter["chapterNo"].as_i64()?,
                        number(sentence)?,
                        pairs,
                    ))
                })
        })
        .collect::<Vec<_>>();
    let mut matches = current
        .iter()
        .filter_map(|sentence| {
            let pairs = telling_pairs(sentence["text"].as_str()?);
            if pairs.len() < minimum_pairs {
                return None;
            }
            let (score, chapter, prior_sentence) = earlier
                .iter()
                .filter_map(|(chapter, number, prior_pairs)| {
                    let shared = pairs.intersection(prior_pairs).count();
                    let shorter = pairs.len().min(prior_pairs.len());
                    (shared >= minimum_pairs && shared * 2 >= shorter)
                        .then_some((shared, *chapter, *number))
                })
                .max_by_key(|(shared, chapter, _)| (*shared, *chapter))?;
            Some((
                score,
                ReusedPassage {
                    chapter,
                    prior_sentence,
                    sentence: number(sentence)?,
                },
            ))
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|(shared, _)| std::cmp::Reverse(*shared));
    matches
        .into_iter()
        .take(8)
        .map(|(_, candidate)| candidate)
        .collect()
}

/// A scene with two material beats in the same order merits a local rewrite
/// even when they are not among the fixed physical transition verbs.
pub(crate) fn replayed_passage_findings(prior: &[Value], body: &str) -> Vec<Value> {
    let spans = sentence_spans(body);
    let mut matches = recurring_outcome_candidates(prior, &numbered_body_sentences(body), 6);
    matches.sort_by_key(|hit| hit.sentence);
    let mut findings = Vec::new();
    let mut next = 0;
    for (index, first) in matches.iter().enumerate() {
        let start = first.sentence;
        if start < next {
            continue;
        }
        for second in &matches[index + 1..] {
            let end = second.sentence;
            if end - start > 8 {
                break;
            }
            if first.chapter != second.chapter
                || !second
                    .prior_sentence
                    .checked_sub(first.prior_sentence)
                    .is_some_and(|gap| (1..=8).contains(&gap))
            {
                continue;
            }
            let quote = &body[spans[start].0..spans[end].1];
            let context = &body[spans[start.saturating_sub(1)].0..spans[end].1];
            if quote.chars().count() > 320
                || body.matches(quote).count() != 1
                || ["回忆", "回想", "想起", "复述", "转述"]
                    .iter()
                    .any(|word| context.contains(word))
            {
                continue;
            }
            let earlier = prior
                .iter()
                .find(|chapter| chapter["chapterNo"] == first.chapter);
            let earlier_quote = |index| {
                earlier
                    .and_then(|chapter| numbered_prior_sentence(chapter, index))
                    .unwrap_or("")
            };
            let first_quote = earlier_quote(first.prior_sentence);
            let last_quote = earlier_quote(second.prior_sentence);
            let mut finding = rule_finding("repeated_event", "HIGH", quote,
                format!("第 {} 章已依次写过「{first_quote}」和「{last_quote}」。请核对这段是否重演了同一组动作或结果。", first.chapter),
                "从已经发生的场面之后写起，保留这里新增的行动、证据或后果。若是有意重试，写清新的触发原因。");
            finding["comparison"] = json!({
                "chapter":first.chapter,"firstSentence":first.prior_sentence,"lastSentence":second.prior_sentence
            });
            findings.push(finding);
            next = end + 1;
            break;
        }
        if findings.len() == 2 {
            break;
        }
    }
    findings
}

pub(crate) fn recurring_transition_candidates(prior: &[Value], current: &[Value]) -> Vec<Value> {
    const ACTIONS: &[&str] = &[
        "弹开",
        "打开",
        "开启",
        "解开",
        "揭开",
        "发现",
        "取出",
        "交出",
        "承认",
        "夺回",
        "坍塌",
        "烧起来",
        "脱手飞出",
        "露出",
    ];
    fn action_context(text: &str, action: &str) -> Option<String> {
        let (before, _) = text.rsplit_once(action)?;
        let clause = before.rsplit(['，', '。', '；', '！', '？']).next()?;
        let chars = clause.chars().collect::<Vec<_>>();
        Some(chars[chars.len().saturating_sub(12)..].iter().collect())
    }
    let history = prior
        .iter()
        .flat_map(|chapter| {
            chapter["sentences"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|sentence| {
                    (
                        &chapter["chapterNo"],
                        &sentence["number"],
                        sentence["text"].as_str().unwrap_or(""),
                    )
                })
        })
        .collect::<Vec<_>>();
    current
        .iter()
        .filter_map(|sentence| {
            let text = sentence["text"].as_str()?;
            ACTIONS.iter().find_map(|action| {
                let subject = action_context(text, action)?;
                let chars = subject.chars().collect::<Vec<_>>();
                let width = if *action == "脱手飞出" { 2 } else { 4 };
                let anchors = chars
                    .windows(width)
                    .filter(|word| word.iter().copied().all(is_han))
                    .map(|word| word.iter().collect::<String>())
                    .collect::<Vec<_>>();
                history
                    .iter()
                    .find_map(|(chapter, prior_sentence, earlier)| {
                        let context = action_context(earlier, action)?;
                        let anchor = anchors
                            .iter()
                            .find(|anchor| context.contains(anchor.as_str()))?;
                        Some(json!({"sentence":sentence["number"],"chapter":chapter,
                            "priorSentence":prior_sentence,"action":action,"sharedWording":anchor}))
                    })
            })
        })
        .take(8)
        .collect()
}

/// Two matching outcomes in the same narrow interval merit a rewrite even
/// when the model overlooks the repeated sequence.
pub(crate) fn replayed_transition_findings(prior: &[Value], body: &str) -> Vec<Value> {
    let spans = sentence_spans(body);
    let current = numbered_body_sentences(body);
    let candidates = recurring_transition_candidates(prior, &current);
    candidates.windows(2).filter_map(|pair| {
        let (first, second) = (&pair[0], &pair[1]);
        if first["chapter"] != second["chapter"] || first["action"] == second["action"] {
            return None;
        }
        let destructive = [first, second].iter().any(|event| matches!(event["action"].as_str(), Some("烧起来" | "坍塌")));
        let nearby = |key: &str| second[key].as_u64()?.checked_sub(first[key].as_u64()?).filter(|gap| (1..=3).contains(gap));
        if !destructive || nearby("sentence").is_none() || nearby("priorSentence").is_none() {
            return None;
        }
        let from = usize::try_from(first["sentence"].as_u64()?).ok()?;
        let to = usize::try_from(second["sentence"].as_u64()?).ok()?;
        let passage = &body[spans[from].0..spans[to].1];
        let context = &body[spans[from.saturating_sub(1)].0..spans[to].1];
        if body.matches(passage).count() != 1
            || ["回忆", "回想", "想起", "如同上次"].iter().any(|mark| context.contains(mark))
        {
            return None;
        }
        let mut finding = rule_finding("repeated_event", "HIGH", passage,
            format!("第 {} 章已经先后写了「{}」和「{}」，本章同一小段再次按原顺序发生。", first["chapter"], first["action"].as_str()?, second["action"].as_str()?),
            "从前章已发生的结果往前写。若故事确实安排再次发生，交代造成重演的变化以及新的后果。");
        finding["comparison"] = json!({
            "chapter":first["chapter"],"firstSentence":first["priorSentence"],"lastSentence":second["priorSentence"]
        });
        Some(finding)
    }).take(2).collect()
}

struct SpokenParagraph {
    start: usize,
    end: usize,
    words: BTreeSet<(char, char)>,
    question: bool,
}

fn spoken_paragraphs(body: &str) -> Vec<SpokenParagraph> {
    let mut offset = 0;
    body.split_inclusive('\n')
        .filter_map(|line| {
            let start = offset;
            offset += line.len();
            let speech = line
                .split('“')
                .skip(1)
                .filter_map(|segment| segment.split_once('”').map(|(text, _)| text))
                .collect::<String>();
            let chars = speech
                .chars()
                .filter(|&c| {
                    is_han(c) && (!COMMON_CHARACTERS.contains(c) || matches!(c, '不' | '没'))
                })
                .collect::<Vec<_>>();
            if chars.len() < 5 {
                return None;
            }
            Some(SpokenParagraph {
                start,
                end: start + line.trim_end().len(),
                words: chars.windows(2).map(|pair| (pair[0], pair[1])).collect(),
                question: speech.ends_with('？'),
            })
        })
        .collect()
}

/// Repeated disclosures can be restated without repeating a physical verb.
/// These matches request a rewrite and still require independent confirmation
/// before a chapter can be held for review.
pub(crate) fn replayed_dialogue_findings(
    history: &[Value],
    bodies: &[String],
    body: &str,
) -> Vec<Value> {
    let prior = history
        .iter()
        .zip(bodies)
        .map(|(chapter, text)| {
            let excerpt_end = text
                .char_indices()
                .nth(4_000)
                .map_or(text.len(), |(index, _)| index);
            let text = &text[..excerpt_end];
            (&chapter["chapterNo"], text, spoken_paragraphs(text))
        })
        .collect::<Vec<_>>();
    let current = spoken_paragraphs(body);
    let overlap = |left: &SpokenParagraph, right: &SpokenParagraph, minimum, tenths| {
        left.words.intersection(&right.words).count() >= minimum
            && pairs_overlap(&left.words, &right.words, tenths)
    };
    let mut next_offset = 0;
    current
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            if line.start < next_offset {
                return None;
            }
            let (chapter, prior_body, previous, end) =
                prior.iter().find_map(|(chapter, text, lines)| {
                    lines.iter().enumerate().find_map(|(prior_index, earlier)| {
                        if line.question && earlier.question && overlap(line, earlier, 4, 4) {
                            let answer = current.get(index + 1)?;
                            let prior_answer = lines.get(prior_index + 1)?;
                            if !answer.question
                                && !prior_answer.question
                                && answer.start - line.end <= 150
                                && prior_answer.start - earlier.end <= 150
                                && overlap(answer, prior_answer, 4, 4)
                            {
                                return Some((
                                    chapter,
                                    *text,
                                    &text[earlier.start..prior_answer.end],
                                    answer.end,
                                ));
                            }
                        }
                        if !line.question && !earlier.question && overlap(line, earlier, 7, 8) {
                            return Some((
                                chapter,
                                *text,
                                &text[earlier.start..earlier.end],
                                line.end,
                            ));
                        }
                        None
                    })
                })?;
            let quote = &body[line.start..end];
            if body.matches(quote).count() != 1
                || ["回忆", "回想", "复述", "转述"]
                    .iter()
                    .any(|word| quote.contains(word))
            {
                return None;
            }
            next_offset = end;
            let mut finding = rule_finding(
                "repeated_event",
                "HIGH",
                quote,
                format!(
                    "第 {} 章已在对话中交代过「{previous}」。请核对本章是否重新揭示了同一事实。",
                    chapter
                ),
                "保留对话真正带来的新信息和后果。若角色是在转述已知事实，写明转述的对象与目的。",
            );
            finding["comparison"] = json!({"chapter":chapter,"quote":previous});
            if let Some((first, last)) = numbered_passage_range(prior_body, previous) {
                finding["comparison"]["firstSentence"] = json!(first);
                finding["comparison"]["lastSentence"] = json!(last);
            }
            Some(finding)
        })
        .take(3)
        .collect()
}

pub(crate) fn exact_punctuation_matched_quote<'a>(
    sentence: &'a str,
    reported: &str,
) -> Option<&'a str> {
    let normalize = |text: &str| {
        text.chars()
            .filter(|character| !character.is_whitespace())
            .map(|character| match character {
                '，' => ',',
                '。' => '.',
                '！' => '!',
                '？' => '?',
                '：' => ':',
                '；' => ';',
                '“' | '”' => '"',
                other => other,
            })
            .collect::<String>()
    };
    let needle = normalize(reported);
    if needle.is_empty() {
        return None;
    }
    let normalized = normalize(sentence);
    let mut matches = normalized.match_indices(&needle);
    let (offset, _) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let mut original = sentence
        .char_indices()
        .filter(|(_, character)| !character.is_whitespace());
    let (start, _) = original.nth(normalized[..offset].chars().count())?;
    let end = match needle.chars().count() - 1 {
        0 => start + sentence[start..].chars().next()?.len_utf8(),
        rest => original
            .nth(rest - 1)
            .map(|(end, character)| end + character.len_utf8())?,
    };
    Some(&sentence[start..end])
}

pub(crate) fn numbered_passage_range(body: &str, passage: &str) -> Option<(usize, usize)> {
    let exact = exact_punctuation_matched_quote(body, passage)?;
    let start = body.find(exact)?;
    let end = start + exact.len();
    let spans = sentence_spans(body);
    let first = spans
        .iter()
        .position(|&(from, to)| from <= start && start < to)?;
    let last = spans
        .iter()
        .position(|&(from, to)| from < end && end <= to)?;
    Some((first, last))
}

pub(crate) fn has_grounded_continuity_proof(issue: &Value) -> bool {
    let evidence = &issue["hardEvidence"];
    let relation_matches = matches!(
        (issue["kind"].as_str(), evidence["relation"].as_str()),
        (Some("REPEATED_EVENT"), Some("COMPLETED_OUTCOME_REPLAYED"))
            | (Some("STATE_CONFLICT"), Some("INCOMPATIBLE_STATES"))
    );
    if !relation_matches {
        return false;
    }
    if issue["kind"] == "REPEATED_EVENT" {
        let persistence = |text: &str| {
            [
                "还在原处",
                "仍在原处",
                "还在原位",
                "仍在原位",
                "还留在原处",
                "仍留在原处",
                "没有挪动",
            ]
            .iter()
            .any(|state| text.contains(state))
        };
        let outcome = evidence["currentOutcome"].as_str().unwrap_or_default();
        let passage = issue["currentQuote"].as_str().unwrap_or_default();
        let earlier_clause = exact_punctuation_matched_quote(passage, outcome)
            .and_then(|exact| passage.find(exact))
            .and_then(|start| {
                passage[..start]
                    .rsplit(['。', '！', '？', '；', '但', '却'])
                    .next()
            })
            .unwrap_or_default();
        // The result can be phrased as a short continuation of the
        // preceding state clause. A separate later action has its own proof.
        if persistence(outcome) || (!outcome.contains(['但', '却']) && persistence(earlier_clause))
        {
            return false;
        }
    }
    let exact_outcome = |quote: &str, outcome: &str| {
        let outcome = evidence[outcome].as_str().unwrap_or("");
        outcome.chars().count() >= 4
            && issue[quote]
                .as_str()
                .is_some_and(|text| exact_punctuation_matched_quote(text, outcome).is_some())
    };
    if !exact_outcome("priorQuote", "priorOutcome")
        || !exact_outcome("currentQuote", "currentOutcome")
    {
        return false;
    }
    if issue["kind"] != "STATE_CONFLICT" {
        return true;
    }
    let Some(subject) = evidence["subject"]
        .as_str()
        .filter(|name| name.chars().count() >= 2)
    else {
        return false;
    };
    let subject_tail = |key: &str| {
        let quote = issue[key].as_str()?;
        let exact = exact_punctuation_matched_quote(quote, subject)?;
        let tail = quote.get(quote.find(exact)? + exact.len()..)?;
        Some(tail)
    };
    let (Some(prior), Some(current)) = (subject_tail("priorQuote"), subject_tail("currentQuote"))
    else {
        return false;
    };
    let component = |tail: &str| {
        let ordinal = tail.strip_prefix('第')?;
        let boundary = ordinal.find(|character: char| {
            !character.is_ascii_digit() && !"一二三四五六七八九十两".contains(character)
        })?;
        let (number, rest) = ordinal.split_at(boundary);
        let unit = rest.chars().next()?;
        (!number.is_empty() && "节层排室格指枚段".contains(unit)).then(|| (number.to_owned(), unit))
    };
    if let (Some((prior_number, prior_unit)), Some((current_number, current_unit))) =
        (component(prior), component(current))
    {
        if prior_unit == current_unit && prior_number != current_number {
            return false;
        }
    }
    evidence["priorOutcome"] != evidence["currentOutcome"]
}

/// An independent reviewer may clear a suspicion when its two exact passages
/// show a transition, a different referent, or a material new consequence.
pub(crate) fn grounded_continuity_dismissals(
    reported: &[Value],
    candidates: &[Value],
    prior: &[Value],
) -> BTreeSet<usize> {
    let locate = exact_punctuation_matched_quote;
    reported
        .iter()
        .filter_map(|item| {
            if !matches!(
                item["relation"].as_str(),
                Some(
                    "SHOWN_TRANSITION"
                        | "DIFFERENT_REFERENT"
                        | "NEW_MATERIAL_OUTCOME"
                        | "ESTABLISHED_PREMISE"
                )
            ) {
                return None;
            }
            let index = usize::try_from(item["number"].as_u64()?).ok()?;
            let candidate = candidates
                .iter()
                .find(|candidate| candidate["candidateNumber"] == item["number"])?;
            locate(
                candidate["currentPassage"].as_str()?,
                item["currentQuote"].as_str()?,
            )?;
            let evidence = &candidate["earlierEvidence"];
            let chapter = prior_chapter_number(evidence)?.as_i64()?;
            let source = prior.iter().find(|entry| entry["chapterNo"] == chapter)?;
            if evidence
                .get("priorSource")
                .is_some_and(|expected| expected != &source["source"])
            {
                return None;
            }
            let quote = item["earlierQuote"].as_str()?;
            let sentences = source["sentences"].as_array()?;
            sentences
                .iter()
                .any(|sentence| {
                    sentence["text"]
                        .as_str()
                        .is_some_and(|text| locate(text, quote).is_some())
                        && sentence["number"].as_u64().is_some_and(|number| {
                            continuity_prior_passage_matches(evidence, number, quote)
                        })
                })
                .then_some(index)
        })
        .collect()
}

pub(crate) struct ContinuityConfirmation {
    pub blockers: Vec<Value>,
    pub dismissals: Vec<Value>,
    pub needs_evidence: BTreeSet<u64>,
}

pub(crate) fn ground_continuity_confirmation(
    review: &Value,
    candidates: &[Value],
    prior: &[Value],
    body: &str,
    current_chapter: Option<u32>,
) -> ContinuityConfirmation {
    let mut blockers: Vec<Value> = Vec::new();
    let mut needs_evidence = BTreeSet::new();
    let verdicts = review["dismissedCandidates"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let cleared = grounded_continuity_dismissals(verdicts, candidates, prior);
    let mut disputed = BTreeSet::new();
    let bind = |issue: &Value| {
        narrative_continuity_findings(std::slice::from_ref(issue), prior, body)
            .into_iter()
            .find(|finding| {
                candidates
                    .iter()
                    .any(|candidate| confirms_same_continuity_pair(candidate, &finding["evidence"]))
            })
    };
    for reported in review["narrativeIssues"].as_array().into_iter().flatten() {
        let mut issue = reported.clone();
        let uses_current_number = current_chapter
            .is_some_and(|chapter| issue["chapter"].as_u64() == Some(u64::from(chapter)));
        // A reviewer can spell out the current chapter number instead of
        // returning null. The original candidate and both exact sentence
        // positions must still bind the confirmation.
        if uses_current_number {
            issue["chapter"] = Value::Null;
        }
        let issue = &issue;
        if has_grounded_continuity_proof(issue) {
            let finding = bind(issue).or_else(|| {
                if !uses_current_number {
                    return None;
                }
                // The current number can also be echoed in place of the
                // earlier source. Recover only a unique, exact candidate pair.
                let chapters = candidates
                    .iter()
                    .filter_map(|candidate| {
                        prior_chapter_number(&candidate["earlierEvidence"])?.as_i64()
                    })
                    .collect::<BTreeSet<_>>();
                let mut matched = chapters.into_iter().filter_map(|chapter| {
                    let mut sourced = issue.clone();
                    sourced["chapter"] = json!(chapter);
                    bind(&sourced)
                });
                let first = matched.next()?;
                matched.next().is_none().then_some(first)
            });
            if let Some(finding) = finding {
                let conflicts = candidates
                    .iter()
                    .filter(|candidate| {
                        confirms_same_continuity_pair(candidate, &finding["evidence"])
                    })
                    .filter_map(|candidate| candidate["candidateNumber"].as_u64())
                    .filter(|number| cleared.contains(&(*number as usize)))
                    .collect::<Vec<_>>();
                if !conflicts.is_empty() {
                    disputed.extend(conflicts);
                } else if !blockers
                    .iter()
                    .any(|prior| prior["evidence"] == finding["evidence"])
                {
                    blockers.push(finding);
                }
                continue;
            }
        }
        for candidate in candidates {
            let same_chapter =
                prior_chapter_number(&candidate["earlierEvidence"]) == issue.get("chapter");
            let range = &candidate["currentSentenceRange"];
            let in_range = match (
                range["firstSentence"].as_u64(),
                range["lastSentence"].as_u64(),
            ) {
                (Some(first), Some(last)) => issue["sentence"]
                    .as_u64()
                    .is_some_and(|number| (first..=last).contains(&number)),
                _ => false,
            };
            let overlaps = match (
                candidate["currentPassage"].as_str(),
                issue["currentQuote"].as_str(),
            ) {
                (Some(passage), Some(quote)) => overlapping_continuity_quote(passage, quote),
                _ => false,
            };
            if same_chapter && (in_range || overlaps) {
                needs_evidence.extend(candidate["candidateNumber"].as_u64());
            }
        }
    }
    let mut dismissals = Vec::new();
    for verdict in verdicts {
        let Some(number) = verdict["number"].as_u64() else {
            continue;
        };
        if cleared.contains(&(number as usize)) && !disputed.contains(&number) {
            dismissals.push(verdict.clone());
        } else if candidates
            .iter()
            .any(|candidate| candidate["candidateNumber"] == verdict["number"])
        {
            needs_evidence.insert(number);
        }
    }
    needs_evidence.retain(|number| !cleared.contains(&(*number as usize)));
    needs_evidence.extend(disputed);
    ContinuityConfirmation {
        blockers,
        dismissals,
        needs_evidence,
    }
}

/// Owner decision 2026-11-25: a lasting state described again is a reminder
/// and never holds the chapter; an action or a disclosure replayed does.
pub(crate) fn release_restated_states(blockers: &mut Vec<Value>) {
    blockers.retain(|finding| {
        let proof = &finding["evidence"]["hardEvidence"];
        proof["relation"] != "COMPLETED_OUTCOME_REPLAYED" || proof["replays"] != "STATE"
    });
}

/// How many of the findings a repair was given are still among those `left`
/// in the revised chapter: the same passage, or the same earlier passage or
/// fact again. Each one left stands for one given; the others are not the
/// repair's.
pub(crate) fn named_findings_left(
    given: (&[Value], &[Value]),
    left: (&[Value], &[Value]),
) -> usize {
    let count = |given: &[Value], left: &[Value], same: &[fn(&Value, &Value) -> bool]| {
        let mut left = left.iter().collect::<Vec<_>>();
        let mut given = given.iter().collect::<Vec<_>>();
        let before = given.len();
        for same in same {
            given.retain(|finding| {
                let found = left.iter().position(|other| same(finding, other));
                found.map(|index| left.swap_remove(index)).is_none()
            });
        }
        before - given.len()
    };
    let same_quote = |one: &Value, other: &Value| one["quote"] == other["quote"];
    let same_fact = |one: &Value, other: &Value| one["established"] == other["established"];
    count(given.0, left.0, &[same_quote, same_earlier_passage])
        + count(given.1, left.1, &[same_fact])
}

/// Whether two findings cite the same earlier passage: overlapping sentences
/// of one chapter, or, within the chapter itself, the same quote.
fn same_earlier_passage(one: &Value, other: &Value) -> bool {
    let cited = |finding: &Value| {
        let evidence = finding.get("comparison").unwrap_or(&finding["evidence"]);
        let sentence = |key: &str| {
            evidence[key]
                .as_u64()
                .or(evidence["priorSentence"].as_u64())
        };
        (
            prior_chapter_number(evidence).and_then(Value::as_i64),
            sentence("firstSentence").zip(sentence("lastSentence")),
            evidence["priorQuote"].as_str().map(ToOwned::to_owned),
        )
    };
    let (chapter, sentences, quote) = cited(one);
    let (other_chapter, other_sentences, other_quote) = cited(other);
    match (chapter, other_chapter) {
        (Some(chapter), Some(other_chapter)) => {
            chapter == other_chapter
                && sentences.zip(other_sentences).is_some_and(
                    |((first, last), (other_first, other_last))| {
                        first <= other_last && other_first <= last
                    },
                )
        }
        (None, None) => quote.is_some() && quote == other_quote,
        _ => false,
    }
}

fn prior_chapter_number(evidence: &Value) -> Option<&Value> {
    evidence
        .get("priorChapter")
        .or_else(|| evidence.get("chapter"))
}

fn overlapping_continuity_quote(left: &str, right: &str) -> bool {
    !left.is_empty()
        && !right.is_empty()
        && (exact_punctuation_matched_quote(left, right).is_some()
            || exact_punctuation_matched_quote(right, left).is_some())
}

fn continuity_prior_passage_matches(expected: &Value, number: u64, quote: &str) -> bool {
    if let Some(prior) = expected["priorSentence"].as_u64() {
        if number != prior {
            return false;
        }
    }
    let expected_quote = expected["priorQuote"]
        .as_str()
        .or_else(|| expected["quote"].as_str())
        .or_else(|| expected["fact"].as_str());
    if let (Some(first), Some(last)) = (
        expected["firstSentence"].as_u64(),
        expected["lastSentence"].as_u64(),
    ) {
        return (first..=last).contains(&number)
            && expected_quote.is_none_or(|text| overlapping_continuity_quote(text, quote));
    }
    expected_quote.is_some_and(|text| overlapping_continuity_quote(text, quote))
}

pub(crate) fn confirms_same_continuity_pair(candidate: &Value, reported: &Value) -> bool {
    let expected = &candidate["earlierEvidence"];
    let Some(number) = reported["priorSentence"].as_u64() else {
        return false;
    };
    let Some(prior_quote) = reported["priorQuote"].as_str() else {
        return false;
    };
    let Some(current_quote) = reported["currentQuote"].as_str() else {
        return false;
    };
    let same_range = match (
        candidate["currentSentenceRange"]["firstSentence"].as_u64(),
        candidate["currentSentenceRange"]["lastSentence"].as_u64(),
    ) {
        (Some(first), Some(last)) => reported["currentSentence"]
            .as_u64()
            .is_some_and(|number| (first..=last).contains(&number)),
        _ => true,
    };
    same_range
        && candidate["currentPassage"]
            .as_str()
            .is_some_and(|passage| overlapping_continuity_quote(passage, current_quote))
        && prior_chapter_number(expected)
            .is_some_and(|chapter| chapter == &reported["priorChapter"])
        && expected
            .get("priorSource")
            .is_none_or(|source| source == &reported["priorSource"])
        && continuity_prior_passage_matches(expected, number, prior_quote)
}

/// Require an exact passage from a supplied final chapter or an earlier
/// passage of this chapter before admitting a narrative finding.
pub fn narrative_continuity_findings(
    reported: &[Value],
    prior: &[Value],
    body: &str,
) -> Vec<Value> {
    let spans = sentence_spans(body);
    let mut seen = BTreeSet::new();
    reported
        .iter()
        .filter_map(|item| {
            let index = usize::try_from(item["sentence"].as_u64()?).ok()?;
            let &(start, end) = spans.get(index)?;
            let current = unique_quote_ending_at(body, &spans, index)?;
            let reason = item["reason"].as_str()?;
            let prior_index = usize::try_from(item["priorSentence"].as_u64()?).ok()?;
            let current_quote = item["currentQuote"].as_str()?;
            let prior_quote = item["priorQuote"].as_str()?;
            if [current_quote, prior_quote]
                .iter()
                .any(|quote| !(4..=180).contains(&quote.chars().count()))
                || reason.trim().is_empty()
                || reason.chars().count() > 180
            {
                return None;
            }
            let current_evidence = exact_punctuation_matched_quote(&body[start..end], current_quote)?;
            let (sentence, location, source) = if item.get("chapter")?.is_null() {
                if prior_index >= index {
                    return None;
                }
                let &(from, to) = spans.get(prior_index)?;
                (&body[from..to], "本章前文".to_string(), None)
            } else {
                let chapter = item["chapter"].as_i64()?;
                let entry = prior.iter().find(|entry| entry["chapterNo"] == chapter)?;
                let sentence = numbered_prior_sentence(entry, prior_index)?;
                (sentence, format!("第 {chapter} 章原文"), entry.get("source"))
            };
            let quote = exact_punctuation_matched_quote(sentence, prior_quote)?;
            let (code, fix) = match item["kind"].as_str()? {
                "REPEATED_EVENT" => ("repeated_event", "承接事件已经发生的结果，写出本章真正新增的行动或后果。若是再次尝试或回忆，明确写出区别。"),
                "STATE_CONFLICT" => ("cross_chapter_state_conflict", "承接前文已经确立的状态。若本章确实有状态变化，用现有情节交代变化的过程。"),
                _ => return None,
            };
            if !seen.insert(index) {
                return None;
            }
            let mut finding = rule_finding(code, "HIGH", current, format!("{location}「{quote}」。{reason}"), fix);
            finding["evidence"] = json!({
                "priorChapter":item["chapter"],"priorSentence":prior_index,"priorQuote":quote,
                "currentSentence":index,"currentQuote":current_evidence
            });
            if let Some(source) = source {
                finding["evidence"]["priorSource"] = source.clone();
            }
            if has_grounded_continuity_proof(item) {
                finding["evidence"]["hardEvidence"] = item["hardEvidence"].clone();
            }
            Some(finding)
        })
        .take(3)
        .collect()
}

/// Whether `sentence` is part of `phrase`: one of its windows is in it, or,
/// for a sentence shorter than a window, all of it.
fn uses_phrase(sentence: &str, phrase: &str) -> bool {
    let chars = sentence.chars().collect::<Vec<_>>();
    if chars.len() < PHRASE_CHARACTERS {
        // Punctuation alone is in every phrase that spans a line break.
        return chars.iter().any(|c| c.is_alphanumeric()) && phrase.contains(sentence);
    }
    chars
        .windows(PHRASE_CHARACTERS)
        .any(|window| phrase.contains(&window.iter().collect::<String>()))
}

/// How often each phrase window occurs across `texts`, the one count both
/// the writing checks and signing F1-5 use (2026-10-25): windows across
/// paragraphs and punctuation count, identifier windows do not.
pub(crate) fn phrase_window_counts(
    texts: &[Vec<char>],
) -> std::collections::HashMap<&[char], usize> {
    let mut counts = std::collections::HashMap::<&[char], usize>::new();
    for text in texts {
        for window in text.windows(PHRASE_CHARACTERS) {
            if !is_identifier_window(window) {
                *counts.entry(window).or_default() += 1;
            }
        }
    }
    counts
}

/// Maximal spans in `texts[from..]` covered by phrases used at least
/// `times` times across all of `texts`.
fn phrases_used(texts: &[String], from: usize, times: usize) -> Vec<String> {
    let texts = texts
        .iter()
        .map(|text| text.chars().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let counts = phrase_window_counts(&texts);
    let mut phrases = Vec::new();
    for text in &texts[from..] {
        let covered = repeated_coverage(text, &counts, times);
        let mut start = None;
        for (index, is_covered) in covered.iter().chain([&false]).enumerate() {
            match (start, is_covered) {
                (None, true) => start = Some(index),
                (Some(from), false) => {
                    let phrase = text[from..index].iter().collect::<String>();
                    if !phrases.contains(&phrase) {
                        phrases.push(phrase);
                    }
                    start = None;
                }
                _ => {}
            }
        }
    }
    phrases.truncate(30);
    phrases
}

fn repeated_coverage(
    text: &[char],
    counts: &std::collections::HashMap<&[char], usize>,
    times: usize,
) -> Vec<bool> {
    let mut covered = vec![false; text.len()];
    for (start, window) in text.windows(PHRASE_CHARACTERS).enumerate() {
        if counts.get(window).is_some_and(|count| *count >= times) {
            covered[start..start + PHRASE_CHARACTERS].fill(true);
        }
    }
    covered
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_edit_can_introduce_a_signing_repeat_using_two_instances_from_history() {
        let phrase = "他听见自己心跳漏了整整一拍";
        let history = vec![format!("空白里，{phrase}。起风后，{phrase}，他合上窗。")];
        let before = "他把窗栓拉紧。";
        let after = format!("{before}{phrase}。门铃突然响了。");
        let added = added_overused_phrase(&history, before, &after).unwrap();
        assert!(phrase.contains(&added));
        assert!(added_overused_phrase(&history, &after, &format!("{after}她走进屋。")).is_none());
        assert!(added_overused_phrase(&history, &after, before).is_none());
    }

    #[test]
    fn round58_a_grain_still_in_place_is_not_a_second_fall() {
        let mut issue = json!({
            "kind":"REPEATED_EVENT",
            "priorQuote":"一颗正卡进《青崖药经》虫蛀的窟窿里，把网洞塞住了。",
            "currentQuote":"昨夜卡进窟窿的那粒银灰还在原处，堵着‘灵脉’二字间的网洞，旁边却新添了两粒。",
            "hardEvidence":{
                "relation":"COMPLETED_OUTCOME_REPLAYED",
                "priorOutcome":"一颗正卡进《青崖药经》虫蛀的窟窿里",
                "currentOutcome":"昨夜卡进窟窿的那粒银灰还在原处"
            }
        });
        assert!(!has_grounded_continuity_proof(&issue));
        issue["hardEvidence"]["currentOutcome"] = json!("堵着‘灵脉’二字间的网洞");
        assert!(!has_grounded_continuity_proof(&issue));
        issue["currentQuote"] = json!("新落下一颗银灰，又卡进《青崖药经》同一个窟窿。");
        issue["hardEvidence"]["currentOutcome"] =
            json!("新落下一颗银灰，又卡进《青崖药经》同一个窟窿");
        assert!(has_grounded_continuity_proof(&issue));
    }
    #[test]
    fn round57_a_different_referent_requires_both_dismissal_quotes() {
        let candidates = [json!({
            "candidateNumber":7,
            "currentPassage":"她左耳垂突然浮现细密血丝。",
            "earlierEvidence":{"priorChapter":2,"priorQuote":"他拇指抹过自己左耳垂。"}
        })];
        let prior = [json!({"chapterNo":2,"sentences":numbered_body_sentences(
            "他拇指抹过自己左耳垂。柳青梧原本耳后就有血丝。"
        )})];
        let mut dismissal = json!({
            "number":7,"relation":"DIFFERENT_REFERENT",
            "currentQuote":"她左耳垂突然浮现细密血丝","earlierQuote":"他拇指抹过自己左耳垂"
        });
        assert_eq!(
            grounded_continuity_dismissals(&[dismissal.clone()], &candidates, &prior),
            BTreeSet::from([7])
        );
        dismissal["earlierQuote"] = json!("柳青梧原本耳后就有血丝");
        assert!(grounded_continuity_dismissals(&[dismissal], &candidates, &prior).is_empty());
    }

    #[test]
    fn round61_an_unbound_claim_targets_only_its_candidate_and_preserves_a_grounded_dismissal() {
        let body = "陆照又用斜光确认两页纸出自同一人。赵伯手背的旧裂口更深，有一道正在渗血。";
        let prior = [json!({"chapterNo":2,"sentences":numbered_body_sentences(
            "陆照用斜光确认两页纸出自同一人。赵伯手背浮着几道新添的裂口。"
        )})];
        let candidates = [
            json!({"candidateNumber":3,"currentPassage":"陆照又用斜光确认两页纸出自同一人。",
                "currentSentenceRange":{"firstSentence":0,"lastSentence":0},
                "earlierEvidence":{"chapter":2,"firstSentence":0,"lastSentence":0}}),
            json!({"candidateNumber":7,"currentPassage":"赵伯手背的旧裂口更深，有一道正在渗血。",
                "currentSentenceRange":{"firstSentence":1,"lastSentence":1},
                "earlierEvidence":{"chapter":2,"firstSentence":1,"lastSentence":1}}),
        ];
        let genuine_transition = json!({"number":7,"relation":"NEW_MATERIAL_OUTCOME",
            "currentQuote":"旧裂口更深，有一道正在渗血","earlierQuote":"手背浮着几道新添的裂口"});
        let first = json!({
            "narrativeIssues":[{"kind":"REPEATED_EVENT","chapter":2,"priorSentence":0,"sentence":0,
                "priorQuote":"陆照用斜光确认两页纸出自同一人","currentQuote":"陆照又用斜光确认两页纸出自同一人",
                "reason":"再次确认同一结果","hardEvidence":{"relation":"COMPLETED_OUTCOME_REPLAYED",
                    "priorOutcome":"确认两页纸出自同一人","currentOutcome":"用斜光……同一人"}}],
            "dismissedCandidates":[genuine_transition]
        });
        let initial = ground_continuity_confirmation(&first, &candidates, &prior, body, Some(3));
        assert!(initial.blockers.is_empty());
        assert_eq!(initial.dismissals, [genuine_transition]);
        assert_eq!(initial.needs_evidence, BTreeSet::from([3]));

        let mut corrected = first;
        corrected["narrativeIssues"][0]["hardEvidence"]["currentOutcome"] =
            json!("确认两页纸出自同一人");
        let verified =
            ground_continuity_confirmation(&corrected, &candidates[..1], &prior, body, Some(3));
        assert_eq!(verified.blockers.len(), 1);
        assert!(verified.dismissals.is_empty());
        assert!(verified.needs_evidence.is_empty());
    }

    #[test]
    fn confirmation_cannot_substitute_a_different_earlier_passage_for_the_same_current_sentence() {
        let candidate = json!({
            "currentPassage":"他又一次说出玉珏和棺木的气味相同。",
            "currentSentenceRange":{"firstSentence":9,"lastSentence":9},
            "earlierEvidence":{"priorChapter":2,"priorSentence":19,"priorQuote":"玉珏和棺木的气味相同。"}
        });
        let mut actual = json!({
            "currentSentence":9,"currentQuote":"玉珏和棺木的气味相同",
            "priorChapter":2,"priorSentence":19,"priorQuote":"玉珏和棺木的气味相同"
        });
        assert!(confirms_same_continuity_pair(&candidate, &actual));
        actual["priorSentence"] = json!(22);
        assert!(!confirms_same_continuity_pair(&candidate, &actual));
        actual["priorSentence"] = json!(19);
        actual["priorChapter"] = json!(3);
        assert!(!confirms_same_continuity_pair(&candidate, &actual));
    }

    #[test]
    fn round68_confirmation_binds_the_current_chapter_number_to_its_earlier_sentence() {
        let quote = "他调出系统日志，查到同一时刻有一段时间戳断层。";
        let body = format!("{quote}沈砚抬起头。{quote}");
        let candidates = [json!({
            "candidateNumber":0,
            "currentPassage":format!("沈砚抬起头。{quote}"),
            "currentSentenceRange":{"firstSentence":1,"lastSentence":2},
            "earlierEvidence":{"priorChapter":null,"priorSentence":0,"priorQuote":quote}
        })];
        let mut review = json!({"narrativeIssues":[{
            "kind":"REPEATED_EVENT","chapter":1,"priorSentence":0,"sentence":2,
            "priorQuote":quote,"currentQuote":quote,"reason":"同一次日志发现原句重演",
            "hardEvidence":{"relation":"COMPLETED_OUTCOME_REPLAYED",
                "priorOutcome":"查到同一时刻有一段时间戳断层",
                "currentOutcome":"查到同一时刻有一段时间戳断层"}
        }]});
        assert_eq!(
            ground_continuity_confirmation(&review, &candidates, &[], &body, Some(1))
                .blockers
                .len(),
            1
        );
        assert!(
            ground_continuity_confirmation(&review, &candidates, &[], &body, Some(2))
                .blockers
                .is_empty()
        );
        review["narrativeIssues"][0]["priorSentence"] = json!(2);
        assert!(
            ground_continuity_confirmation(&review, &candidates, &[], &body, Some(1))
                .blockers
                .is_empty()
        );
    }

    #[test]
    fn round69_the_nominated_prior_source_can_recover_a_mislabeled_confirmation() {
        let earlier =
            "他迅速调出系统底层协议日志，发现这13秒里，有人手动执行了三次数据擦除覆盖操作。";
        let body = "他放大底层协议日志，三次擦除覆盖操作的时间戳依次跳出来：13秒、13秒、13秒。";
        let prior = [json!({"chapterNo":1,"sentences":numbered_body_sentences(earlier)})];
        let candidates = [json!({
            "candidateNumber":0,"currentPassage":body,
            "currentSentenceRange":{"firstSentence":0,"lastSentence":0},
            "earlierEvidence":{"priorChapter":1,"priorSentence":0,"priorQuote":earlier}
        })];
        let mut review = json!({"narrativeIssues":[{
            "kind":"REPEATED_EVENT","chapter":2,"priorSentence":0,"sentence":0,
            "priorQuote":earlier,"currentQuote":body,"reason":"同一次日志发现重演",
            "hardEvidence":{"relation":"COMPLETED_OUTCOME_REPLAYED",
                "priorOutcome":"有人手动执行了三次数据擦除覆盖操作",
                "currentOutcome":"三次擦除覆盖操作的时间戳依次跳出来"}
        }]});
        let confirmed = ground_continuity_confirmation(&review, &candidates, &prior, body, Some(2));
        assert_eq!(confirmed.blockers.len(), 1);
        assert_eq!(confirmed.blockers[0]["evidence"]["priorChapter"], 1);
        review["narrativeIssues"][0]["chapter"] = json!(3);
        assert!(
            ground_continuity_confirmation(&review, &candidates, &prior, body, Some(2))
                .blockers
                .is_empty()
        );
    }

    #[test]
    fn round69_conflicting_judgments_on_the_same_pair_require_another_review() {
        let earlier = "他用斜光确认纸页出自同一人。";
        let body = "他再次确认两页纸出自同一人。";
        let prior = [json!({"chapterNo":1,"sentences":numbered_body_sentences(earlier)})];
        let candidates = [json!({
            "candidateNumber":0,"currentPassage":body,
            "currentSentenceRange":{"firstSentence":0,"lastSentence":0},
            "earlierEvidence":{"priorChapter":1,"priorSentence":0,"priorQuote":earlier}
        })];
        let review = json!({
            "narrativeIssues":[{
                "kind":"REPEATED_EVENT","chapter":1,"priorSentence":0,"sentence":0,
                "priorQuote":earlier,"currentQuote":body,"reason":"重复确认",
                "hardEvidence":{"relation":"COMPLETED_OUTCOME_REPLAYED",
                    "priorOutcome":"纸页出自同一人","currentOutcome":"两页纸出自同一人"}
            }],
            "dismissedCandidates":[{"number":0,"relation":"NEW_MATERIAL_OUTCOME","currentQuote":body,"earlierQuote":earlier}]
        });
        let confirmed = ground_continuity_confirmation(&review, &candidates, &prior, body, Some(2));
        assert!(confirmed.blockers.is_empty());
        assert!(confirmed.dismissals.is_empty());
        assert_eq!(confirmed.needs_evidence, BTreeSet::from([0]));
    }

    #[test]
    fn a_recalled_remote_passage_preserves_its_sparse_sentence_and_formal_reference() {
        use crate::generated::types::ArtifactRef;
        use crate::repositories::ops::ContinuityChapter;
        let earlier = "他闻出玉珏上的松脂焦香与师父棺木里的气味一样。";
        let current = "那层松脂焦香贴着鼻腔泛上来，跟师父棺木里的气味完全一样。";
        let body = format!("{}\n{earlier}", "闲叙一段。".repeat(75));
        let source = ContinuityChapter {
            chapter_id: "chap:opening:two".into(),
            chapter_no: 2,
            final_ref: ArtifactRef {
                artifact_id: "art:opening:two".into(),
                version: 1,
                payload_hash: "c".repeat(64),
            },
            body,
            outcome_end: None,
        };
        let anchors = continuity_anchors(current);
        assert!(
            anchors.iter().any(|anchor| earlier.contains(anchor)),
            "{anchors:?}"
        );
        let mut prior = Vec::new();
        add_recalled_continuity_prose(&mut prior, &[&source], current);
        let findings = narrative_continuity_findings(
            &[json!({
                "sentence":0,"chapter":2,"priorSentence":75,
                "priorQuote":earlier,"currentQuote":current,"kind":"REPEATED_EVENT","reason":"把已经认出的气味再次写成首次发现"
            })],
            &prior,
            current,
        );
        assert_eq!(findings.len(), 1, "{prior:?}");
        assert_eq!(
            findings[0]["evidence"]["priorSource"]["finalRef"]["artifactId"],
            "art:opening:two"
        );
        assert_eq!(findings[0]["evidence"]["priorSentence"], 75);
    }
    #[test]
    fn material_outcome_candidates_cover_disclosures_and_actions_outside_the_verb_list() {
        for (chapter, earlier, current) in [
            (
                4,
                "他凑近看，四道刮痕排在一起，前三道毛边朝里，第四道毛边朝外。",
                "银斑血迹边缘，第四道刮痕浮出来了，渗入式的，毛边朝外，跟前三道刮掉的完全不同。",
            ),
            (
                9,
                "陆照接过焦纸，折成三角，塞进青铜断口的凹槽。",
                "陆照把拓片折成三角，塞进青铜断口凹槽。",
            ),
        ] {
            let prior = [json!({"chapterNo":chapter,"sentences":numbered_body_sentences(earlier)})];
            let candidates =
                recurring_outcome_candidates(&prior, &numbered_body_sentences(current), 8);
            assert_eq!(
                json!(candidates),
                json!([{"chapter":chapter,"priorSentence":0,"sentence":0}])
            );
        }
    }

    #[test]
    fn round52_similar_beats_in_the_same_order_flag_the_replayed_scene() {
        let earlier = concat!(
            "“回管事。”陆照忽然伸手，拇指按在桌沿上，压出两道印。\n\n",
            "力道、间距、深浅。跟今早他在门框上按的那两道，分毫不差。\n\n",
            "柳管事瞳孔一缩，袖口那处麻线结猛地绷紧，勒进腕肉里。\n\n",
            "柳管事盯着他看了三息。炭盆里一点余烬爆了声轻响。",
        );
        let current = concat!(
            "“小的手不抖。”他伸出手，拇指按在桌沿，压出两道印。\n\n",
            "力道、间距、深浅，跟今早门框上那两道分毫不差。柳管事瞳孔缩了一下，袖口的麻线结绷紧，勒进肉里。\n\n",
            "柳管事盯他三息。炭盆里余烬爆了声轻响。",
        );
        let prior = [json!({"chapterNo":3,"sentences":numbered_body_sentences(earlier)})];
        let findings = replayed_passage_findings(&prior, current);
        assert!(
            findings
                .iter()
                .any(|finding| finding["code"] == "repeated_event"),
            "{findings:?}"
        );
    }

    #[test]
    fn round62_different_finger_segments_cannot_prove_incompatible_states() {
        let prior = "左手小指第一节裂开一道口子，渗着幽蓝色的血。";
        let current = "左手小指第二节的白点淡了。";
        let issue = json!({
            "kind":"STATE_CONFLICT","priorQuote":prior,"currentQuote":current,
            "hardEvidence":{"relation":"INCOMPATIBLE_STATES",
                "priorOutcome":prior,"currentOutcome":current,
                "subject":"左手小指"}
        });
        assert!(!has_grounded_continuity_proof(&issue));
    }

    #[test]
    fn contradictory_states_of_the_same_seal_can_provide_confirmation_proof() {
        let prior = "库门封条完整，封泥没有缺口。";
        let current = "同一刻，库门封条断成两截，封泥撒满台阶。";
        let issue = json!({
            "kind":"STATE_CONFLICT","priorQuote":prior,"currentQuote":current,
            "hardEvidence":{"relation":"INCOMPATIBLE_STATES",
                "priorOutcome":"库门封条完整","currentOutcome":"库门封条断成两截",
                "subject":"库门封条"}
        });
        assert!(has_grounded_continuity_proof(&issue));
    }

    #[test]
    fn round51_a_guessed_state_does_not_supply_confirmation_proof() {
        let mut issue = json!({
            "kind":"STATE_CONFLICT",
            "priorQuote":"她指腹按下去，痕边泛出微青。",
            "currentQuote":"掌心三道浅痕末端，一道微青细线正缓缓延伸。",
        });
        assert!(!has_grounded_continuity_proof(&issue));
        issue["hardEvidence"] = json!({
            "relation":"INCOMPATIBLE_STATES",
            "priorOutcome":"痕边泛出微青",
            "currentOutcome":"第一次出现细线"
        });
        assert!(!has_grounded_continuity_proof(&issue));
    }

    #[test]
    fn round50_repeated_dialogue_is_checked_even_with_different_wording() {
        let prior = [json!({"chapterNo":7}), json!({"chapterNo":9})];
        let bodies = [
            concat!(
                "陆照沉默了两息，问：“你父亲拓到一半收手，是烧了纸，还是烧了手？”\n\n",
                "柳青梧把袖子放下来，声音很平：“烧了半截小指。剩下的半截，他后来用来按契。”",
            ).into(),
            concat!(
                "“丙寅年冬至子时。”他念，每个字都念得很慢，“你师祖写这八个字的时候，墨里掺了银粉。他说，银粉是从灵脉里取的，灵脉不死，这字就不灭。”\n\n",
                "“阵眼认了。”他说，声音很平，“你填进去的那一处，它认了。”",
            ).into(),
        ];
        let body = concat!(
            "“师父当年拓到一半收手，”他抬头看柳青梧，“是看见字就烧了纸，还是手先烧起来？”\n\n",
            "柳青梧垂下手，袖口盖回腕上，语气听不出起伏：“半截小指没保住。剩下的半截，按了契。”\n\n",
            "“子时。”他念，语速很慢，“你师祖写那八个字的时候，墨里掺银粉。他说，银粉从灵脉里取，灵脉不死，字就不灭。”\n\n",
            "“灵脉没死。”陆照说，“沉眠了。”\n\n",
            "陈砚没回头：“阵眼认了。你填进去那一处，它认了。剩下的，它自己找路。”",
        );
        let findings = replayed_dialogue_findings(&prior, &bodies, body);
        assert_eq!(findings.len(), 3, "{findings:?}");
        for (finding, chapter, fragment) in [
            (&findings[0], "第 7 章", "半截小指"),
            (&findings[1], "第 9 章", "墨里掺银粉"),
            (&findings[2], "第 9 章", "阵眼认了"),
        ] {
            assert!(finding["problem"].as_str().unwrap().contains(chapter));
            assert!(finding["quote"].as_str().unwrap().contains(fragment));
        }
    }

    #[test]
    fn a_repeated_question_that_gets_a_new_answer_is_not_a_replay() {
        let prior = [json!({"chapterNo":7})];
        let bodies =
            ["“谁把租约藏进了井边的石龛？”\n\n“先前回来那个老药童，袖口上糊着碎霜。”".into()];
        let body = "“谁把租约藏进了井边的石龛？”\n\n“周使的信使。他借这事陷害陈砚。”";
        assert!(replayed_dialogue_findings(&prior, &bodies, body).is_empty());
    }

    #[test]
    fn round48_burning_the_same_scabbard_and_losing_the_sword_again_is_flagged() {
        let prior = [json!({"chapterNo":6,"sentences":numbered_body_sentences(
            "鞘口渗出一缕青烟，紧接着整条剑鞘烧起来，幽火顺着鞘身往上爬，热浪掀起袍角。剑柄脱手飞出，撞在对面墙上。"
        )})];
        let current = "他一脚踏进夹道。鞘口渗出一缕青烟，紧接着整条剑鞘烧起来，幽火顺着鞘身往上爬。剑柄脱手飞出，撞在对墙。";
        let findings = replayed_transition_findings(&prior, current);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0]["code"], "repeated_event");
        assert!(replayed_transition_findings(
            &prior,
            &current.replace("他一脚踏进夹道。", "陆照回想起先前的一幕。")
        )
        .is_empty());
    }

    #[test]
    fn a_replayed_mirror_opening_is_flagged_for_review_even_with_new_wording() {
        let prior = [
            json!({"chapterNo":7,"sentences":numbered_body_sentences("镜背夹层无声弹开。里面有星砂结晶。")}),
        ];
        let current = numbered_body_sentences("铜锈簌簌剥落，镜背夹层弹开一条缝。衣柜门也弹开了。");
        assert_eq!(
            recurring_transition_candidates(&prior, &current),
            vec![json!({
                "sentence":0,"chapter":7,"priorSentence":0,"action":"弹开","sharedWording":"镜背夹层"
            })]
        );
    }

    #[test]
    fn evidence_keeps_the_original_paragraph_break_when_the_model_shortens_it() {
        let source = "“谢”。\n\n跟阿沅拼出来的那个字，同一把刀刻的。";
        assert_eq!(
            exact_punctuation_matched_quote(source, "“谢”。\n跟阿沅拼出来的那个字，同一把刀刻的。"),
            Some(source)
        );
        assert!(exact_punctuation_matched_quote(source, "“柳”。跟阿沅拼出来的那个字").is_none());
    }

    #[test]
    fn markdown_left_in_the_prose_is_taken_out_before_signing_reads_it() {
        // Real run round 32: "* * *" scene breaks failed signing (F1-2,
        // leftover Markdown) and no model evaluation ran.
        let body =
            "他抬头看。\n\n* * *\n\n残渣里有光。\n\n---\n\n## 第二幕\n**阿公**没回头。\n- 一\n* 二";
        assert_eq!(
            normalize_typography(body),
            "他抬头看。\n\n残渣里有光。\n\n第二幕\n阿公没回头。\n一\n二"
        );
    }

    #[test]
    fn every_chapter_plan_weighs_the_protagonists_agency_and_the_readers_desire() {
        // Owner decision 2026-11-03: S38 goes into chapter planning, as no
        // check reports a passive protagonist for it to be chosen by.
        let skills =
            guidance_skill_ids(&guidance("chapter_plan", &planning_signals(Some(5), &[])).unwrap());
        assert!(skills.contains(&"S38".to_string()), "{skills:?}");
        // The planning residents all still fit the budget.
        for resident in ["S14", "S25", "S26", "S36", "S39"] {
            assert!(
                skills.contains(&resident.to_string()),
                "{resident}: {skills:?}"
            );
        }
    }

    #[test]
    fn every_skill_can_be_selected_by_some_step() {
        // P2: S04, S07 and S43 were never selected, for want of signals.
        let chapter_stages = ["chapter_plan", "render", "review", "rewrite"];
        let never = crate::skills_v2::catalog()
            .unwrap()
            .iter()
            .filter(|skill| {
                !skill.inject.keys().any(|stage| {
                    // A chapter step's signals can match any trigger.
                    (chapter_stages.contains(&stage.as_str()) && !skill.triggers.is_empty())
                        || guidance_skill_ids(&setup_guidance(stage).unwrap()).contains(&skill.id)
                })
            })
            .map(|skill| skill.id.clone())
            .collect::<Vec<_>>();
        assert!(never.is_empty(), "never selected: {never:?}");
    }

    #[test]
    fn a_chapter_with_a_paragraph_over_300_characters_is_unbroken() {
        // LD-2 (real run round 22): chapter 1 came back as one paragraph;
        // round 26: as four of 400 to 600 characters.
        assert!(is_unbroken(&"正".repeat(401)));
        assert!(is_unbroken(&format!(
            "{}\n{}",
            "正".repeat(100),
            "正".repeat(301)
        )));
        assert!(!is_unbroken(&format!(
            "{}\n{}",
            "正".repeat(300),
            "正".repeat(300)
        )));
    }

    #[test]
    fn a_rejected_direction_is_told_it_is_a_direction() {
        let feedback = plan_feedback(&format!(
            "{}:阶段少了",
            crate::book_direction::DIRECTION_ERROR
        ))
        .unwrap();
        assert_eq!(
            feedback["rejectedFor"],
            crate::book_direction::DIRECTION_ERROR
        );
        assert!(feedback["instruction"]
            .as_str()
            .unwrap()
            .starts_with("上一版方向"));
        assert_eq!(feedback["problems"], serde_json::json!(["阶段少了"]));
    }

    use super::*;

    #[test]
    fn a_state_described_again_never_holds_the_chapter() {
        let finding = |relation: &str, replays: &str| json!({"evidence":{"hardEvidence":{"relation":relation,"replays":replays}}});
        let mut blockers = vec![
            finding("COMPLETED_OUTCOME_REPLAYED", "STATE"),
            finding("COMPLETED_OUTCOME_REPLAYED", "ACTION"),
            finding("INCOMPATIBLE_STATES", "STATE"),
        ];
        release_restated_states(&mut blockers);
        assert_eq!(blockers.len(), 2);
        assert_eq!(blockers[0]["evidence"]["hardEvidence"]["replays"], "ACTION");
    }

    /// Real run 2026-11-25: the repair fixed two of three passages, the check
    /// found three others, and the repair was thrown away.
    #[test]
    fn a_repair_is_judged_by_the_passages_it_was_given() {
        let named = |quote: &str, prior: u64| json!({"quote":quote,"evidence":{"priorChapter":2,"priorSentence":prior}});
        let given = [
            named("拖痕断开一条线。", 106),
            named("柜子露出一角。", 107),
            named("退后半步。", 103),
        ];
        let found = |left: &[Value]| named_findings_left((&given, &[]), (left, &[]));
        // Two passages are fixed; the third is unchanged.
        let others = [
            named("印子发烫。", 35),
            named("印子透红。", 35),
            named("退后半步。", 103),
        ];
        assert_eq!(found(&others), 1);
        // Worded differently but the same earlier passage again.
        assert_eq!(found(&[named("拖痕朝外翻着。", 106)]), 1);
        // Of three findings on one earlier passage, the one left counts once.
        let alike = [named("甲。", 35), named("乙。", 35), named("丙。", 35)];
        assert_eq!(
            named_findings_left((&alike, &[]), (&[named("丙。", 35)], &[])),
            1
        );
        // A rule's finding and the model's are the same when they cite the
        // same sentences.
        let rule =
            json!({"quote":"甲。","comparison":{"chapter":2,"firstSentence":34,"lastSentence":36}});
        assert_eq!(
            named_findings_left((&[rule], &[]), (&[named("乙。", 35)], &[])),
            1
        );
        let fact = json!({"sentence":4,"established":"印子平复了大半"});
        let same = json!({"sentence":9,"established":"印子平复了大半"});
        let facts =
            |left: &[Value]| named_findings_left((&[], &[fact.clone(), fact.clone()]), (&[], left));
        assert_eq!(facts(&[same]), 1);
        assert_eq!(facts(&[]), 0);
    }

    const RANGE: ChapterLengthRange = ChapterLengthRange {
        min: 2000,
        max: 3000,
    };

    fn scene(scene_type: &str, beats: usize, estimate: i64) -> Value {
        json!({
            "sceneType":scene_type,
            "goal":"拿到档案",
            "beats":(0..beats).map(|n| format!("节拍{n}")).collect::<Vec<_>>(),
            "estimatedCharacters":estimate
        })
    }

    fn plan(role: &str, goal: &str, scenes: Vec<Value>) -> Value {
        json!({"chapterRole":role,"chapterGoal":goal,"sceneExecution":scenes})
    }

    #[test]
    fn craft_role_ranges_stay_inside_the_user_range() {
        let range = |min, max| ChapterLengthRange { min, max };
        assert_eq!(role_range("过渡", RANGE), range(2000, 2500));
        assert_eq!(role_range("普通", RANGE), range(2250, 2750));
        assert_eq!(role_range("回报", RANGE), range(2500, 3000));
        assert_eq!(role_range("高潮", RANGE), range(2750, 3000));
        assert_eq!(role_range("未知", RANGE), RANGE);
    }

    fn codes(findings: &[Value]) -> Vec<String> {
        findings
            .iter()
            .map(|finding| finding["code"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn craft_rule_findings_flag_plan_wording_copied_into_the_prose() {
        let plan = json!({"chapterGoal":"沈砚决定追查姐姐的下落","sceneExecution":[
            {"beats":["沈砚眼神从迷茫转为坚定，决定反击"]}]});
        let body = "雨停了。沈砚眼神从迷茫转为坚定，决定反击。他推开门。";
        let findings = rule_findings(&plan, body);
        assert_eq!(codes(&findings), vec!["summary_where_scene_needed"]);
        assert_eq!(findings[0]["severity"], "HIGH");
        assert_eq!(findings[0]["quote"], "沈砚眼神从迷茫转为坚定，决定反击。");
        assert_eq!(findings[0]["source"], "rule");
    }

    #[test]
    fn craft_rule_findings_flag_pseudo_precise_measures_beyond_two() {
        let body = "门开了三公分。他退后两步。灯闪了7秒。脚印六十八厘米。他停在两毫米外。";
        let findings = rule_findings(&json!({}), body);
        assert_eq!(codes(&findings), vec!["prose_wordy"; 3]);
        assert_eq!(findings[0]["quote"], "灯闪了7秒。");
    }

    #[test]
    fn craft_rule_findings_count_similes_but_not_words_that_contain_xiang() {
        let plain = "屏幕上的图像和录像都是像素。".repeat(10);
        assert!(rule_findings(&json!({}), &plain).is_empty());
        let similes = ["他像风。", "雨像针。", "灯像眼。", "门像嘴。", "夜像墨。"]
            .iter()
            .enumerate()
            .map(|(n, s)| format!("{n}{s}"))
            .collect::<String>();
        let findings = rule_findings(&json!({}), &similes);
        assert_eq!(codes(&findings), vec!["prose_wordy"; 2]);
    }

    #[test]
    fn craft_rule_findings_flag_a_body_tic_used_twice_in_a_chapter() {
        let body = "他喉结动了动。她笑了。喉结又滚了一下。";
        let findings = rule_findings(&json!({}), body);
        assert_eq!(codes(&findings), vec!["word_repetition"]);
        assert_eq!(findings[0]["quote"], "喉结又滚了一下。");
        assert!(rule_findings(&json!({}), "他喉结动了动。她笑了。").is_empty());
    }

    #[test]
    fn craft_tics_the_recent_chapters_lean_on_are_listed_for_the_writer() {
        let history = vec![
            "他喉结滚动。她看着他。".to_string(),
            "喉结一紧。指尖发凉。".to_string(),
        ];
        assert_eq!(overused_tics(&history), vec!["喉结".to_string()]);
        assert!(overused_tics(&[]).is_empty());
    }

    #[test]
    fn craft_habit_count_sums_similes_tics_measures_and_panels() {
        assert_eq!(habit_count("他推开门。"), 0);
        assert_eq!(
            habit_count("指尖停在键上，像被磁石吸住，三毫米外是【警告】。"),
            4
        );
        assert_eq!(habit_count("屏幕上的图像全是像素。"), 0);
    }

    #[test]
    fn craft_planner_scene_labels_reach_the_skills_written_for_them() {
        // Labels a real planner used (2026-10-19) that no skill listed.
        let plan = json!({"chapterRole":"重要","sceneExecution":[
            {"sceneType":"动作冲突"},{"sceneType":"过渡"}
        ]});
        let signals = plan_signals(&plan, Some(6));
        assert_eq!(
            signals.scene_types,
            vec!["战斗".to_string(), "转场".to_string()]
        );
        let render = guidance_skill_ids(&guidance("render", &signals).unwrap());
        assert!(render.contains(&"S30".to_string()), "{render:?}");
        assert!(render.contains(&"S34".to_string()), "{render:?}");
    }

    #[test]
    fn craft_planner_is_offered_the_scene_types_the_skills_know() {
        let inputs = planning_inputs(Some(5), &[], RANGE).unwrap();
        let options = inputs["sceneTypeOptions"].as_array().unwrap();
        for known in ["战斗", "信息揭示", "情绪峰值", "转场", "调查", "日常"] {
            assert!(options.contains(&json!(known)), "{known} missing");
        }
    }

    #[test]
    fn craft_chapter_planning_follows_the_previous_chapter_role_and_scenes() {
        let recent = vec![
            json!({"chapterNo":7,"chapterGoal":"潜入档案库","ending":"灯灭了。",
            "chapterRole":"高潮前","sceneTypes":["调查","关系转折"]}),
        ];
        let inputs = planning_inputs(Some(8), &recent, RANGE).unwrap();
        let plan = guidance_skill_ids(&inputs["craftGuidance"]);
        for expected in ["S20", "S22", "S05"] {
            assert!(
                plan.contains(&expected.to_string()),
                "{expected} not in {plan:?}"
            );
        }
    }

    #[test]
    fn craft_planning_inputs_carry_recent_chapters_roles_and_guidance() {
        let recent =
            vec![json!({"chapterNo":1,"chapterGoal":"林默接下委托","ending":"门外有人敲了三下。"})];
        let inputs = planning_inputs(Some(2), &recent, RANGE).unwrap();
        assert_eq!(inputs["chapterNumber"], 2);
        assert_eq!(inputs["recentChapters"], json!(recent));
        let roles = inputs["chapterRoleOptions"].as_array().unwrap();
        assert_eq!(roles.len(), CHAPTER_ROLES.len());
        assert_eq!(roles[0], json!({"role":"普通","min":2250,"max":2750}));
        let skills = inputs["craftGuidance"]["skills"].as_array().unwrap();
        assert!(skills.iter().any(|skill| skill["skillId"] == "S26"));
        assert!(skills.iter().any(|skill| skill["skillId"] == "S13"));
        assert!(skills
            .iter()
            .all(|skill| skill["reason"].as_str().is_some_and(|r| !r.is_empty())));
    }

    #[test]
    fn craft_accepts_a_plan_with_a_role_and_a_new_goal() {
        let ok = plan(
            "普通",
            "查明委托人的记忆为何被覆盖",
            vec![
                scene("调查", 3, 900),
                scene("对峙", 3, 900),
                scene("揭示", 2, 700),
            ],
        );
        assert_eq!(validate_plan_craft(&ok, &[]), Ok(()));
    }

    #[test]
    fn craft_rejects_a_missing_role_but_not_a_scene_without_beats() {
        let no_role = json!({"chapterGoal":"目标","sceneExecution":[scene("调查", 3, 2500)]});
        assert_eq!(
            validate_plan_craft(&no_role, &[]),
            Err("B08_PLAN_ROLE_INVALID".into())
        );
        // Owner decision (2026-10-23): beat counts and per-beat lengths are
        // retired; a chapter is written whole and sized afterwards.
        let one_beat = plan("普通", "目标", vec![scene("调查", 1, 2500)]);
        assert_eq!(validate_plan_craft(&one_beat, &[]), Ok(()));
    }

    #[test]
    fn craft_rejects_a_goal_that_repeats_a_recent_chapter() {
        let recent = vec![json!({"chapterNo":1,"chapterGoal":"林默决定调查委托人。"})];
        let repeated = plan(
            "普通",
            "林默决定调查委托人",
            vec![
                scene("调查", 3, 900),
                scene("对峙", 3, 900),
                scene("揭示", 2, 700),
            ],
        );
        assert_eq!(
            validate_plan_craft(&repeated, &recent),
            Err("B08_PLAN_GOAL_REPEATED".into())
        );
    }

    #[test]
    fn craft_plan_signals_read_role_and_scene_types() {
        let committed = plan(
            "回报",
            "目标",
            vec![scene("战斗", 3, 900), scene("对话为主", 2, 600)],
        );
        let signals = plan_signals(&committed, Some(5));
        assert_eq!(signals.chapter_number, Some(5));
        assert_eq!(signals.chapter_role.as_deref(), Some("回报"));
        assert_eq!(signals.scene_types, vec!["战斗", "对话为主"]);
    }

    #[test]
    fn craft_typography_makes_punctuation_full_width_and_keeps_labels() {
        assert_eq!(
            normalize_typography("他说:走吧,别回头!真的?好...我知道了."),
            "他说：走吧，别回头！真的？好……我知道了。"
        );
        assert_eq!(
            normalize_typography("备注：S.Y. 已归档，倒计时00:07，峰值0.87，日期2013.08.17。"),
            // Decimal points and clock colons stay as they are written.
            "备注：S．Y． 已归档，倒计时00:07，峰值0.87，日期2013.08.17。"
        );
        assert_eq!(
            normalize_typography("什么？？！！不行。。"),
            "什么？！不行。"
        );
        assert_eq!(
            normalize_typography("第一段。\n\n\n\n第二段。"),
            "第一段。\n\n第二段。"
        );
        assert_eq!(normalize_typography("——好吧……"), "——好吧……");
    }

    #[test]
    fn craft_repeated_phrases_lists_twice_used_phrases_once() {
        let texts = vec![
            "他把那张泛黄的旧照片翻过来看。雨还在下。".to_string(),
            "她又提起那张泛黄的旧照片翻过来看的事。".to_string(),
        ];
        let phrases = repeated_phrases(&texts);
        assert_eq!(phrases, vec!["那张泛黄的旧照片翻过来看".to_string()]);
        assert!(repeated_phrases(&["各不相同的一句话。".to_string()]).is_empty());
    }

    #[test]
    fn craft_overused_phrases_lists_third_uses_in_the_new_text_only() {
        let motif = "白裙下摆扬起，褶皱里藏着三颗蓝莓籽";
        let history = vec![format!("第一章{motif}。"), format!("第二章{motif}。")];
        assert_eq!(
            overused_phrases(&history, &format!("他又看见{motif}。")),
            vec![format!("{motif}。")]
        );
        assert!(overused_phrases(&history, "他什么也没看见。").is_empty());
        assert!(overused_phrases(&history[..1], &format!("又是{motif}。")).is_empty());
    }
}
