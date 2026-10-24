//! Craft rules that sit between the length range and the v2 skills: what
//! the planner is given, what a usable plan must contain, and which signals
//! the later steps choose their skills by.

use crate::chapter_length::ChapterLengthRange;
use crate::skills_v2::StepSignals;
use serde_json::{json, Value};

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
    let goal = normalized_goal(
        plan.get("chapterGoal")
            .and_then(Value::as_str)
            .unwrap_or(""),
    );
    if recent_chapters.iter().any(|chapter| {
        chapter
            .get("chapterGoal")
            .and_then(Value::as_str)
            .is_some_and(|previous| normalized_goal(previous) == goal)
    }) {
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
    REPLANNABLE
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, instruction)| json!({"rejectedFor":code,"instruction":instruction}))
}

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

/// Chinese typography for committed prose (owner rule: signing checks forbid
/// mixed full/half-width punctuation and repeated punctuation): half-width
/// punctuation becomes full-width, and a period touching ASCII letters or
/// digits becomes `．` so labels, decimals and dates survive.
pub fn normalize_typography(body: &str) -> String {
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
fn sentence_spans(body: &str) -> Vec<(usize, usize)> {
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
    for (index, character) in body.char_indices() {
        if character == '\n' {
            push(start, index);
            start = index + 1;
        } else if matches!(character, '。' | '！' | '？' | '…') {
            push(start, index + character.len_utf8());
            start = index + character.len_utf8();
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

fn is_numeral(character: char) -> bool {
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
            let quote = (0..=index)
                .rev()
                .map(|from| &body[spans[from].0..end])
                .find(|quote| body.matches(quote).nth(1).is_none())?;
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

/// Whether `sentence` is part of `phrase`: one of its windows is in it, or,
/// for a sentence shorter than a window, all of it.
fn uses_phrase(sentence: &str, phrase: &str) -> bool {
    let chars = sentence.chars().collect::<Vec<_>>();
    if chars.len() < PHRASE_CHARACTERS {
        return phrase.contains(sentence);
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
        let mut covered = vec![false; text.len()];
        for (start, window) in text.windows(PHRASE_CHARACTERS).enumerate() {
            if counts.get(window).is_some_and(|count| *count >= times) {
                covered[start..start + PHRASE_CHARACTERS].fill(true);
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
