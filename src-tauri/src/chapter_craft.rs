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

/// Each beat carries roughly this many characters at most.
const MAX_CHARACTERS_PER_BEAT: i64 = 400;

/// Rejects a plan that cannot be written well: a missing role, scenes
/// without 2–4 beats, too few beats for the range, or a repeated goal.
pub fn validate_plan_craft(
    plan: &Value,
    range: ChapterLengthRange,
    recent_chapters: &[Value],
) -> Result<(), String> {
    if !plan
        .get("chapterRole")
        .and_then(Value::as_str)
        .is_some_and(|role| CHAPTER_ROLES.contains(&role))
    {
        return Err("B08_PLAN_ROLE_INVALID".into());
    }
    let scenes = plan
        .get("sceneExecution")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut beats = 0;
    for scene in scenes {
        let count = scene
            .get("beats")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        if !(2..=4).contains(&count) {
            return Err("B08_PLAN_BEATS_INVALID".into());
        }
        beats += count as i64;
    }
    if beats * MAX_CHARACTERS_PER_BEAT < range.min {
        return Err("B08_PLAN_BEATS_INSUFFICIENT".into());
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
const REPLANNABLE: [(&str, &str); 6] = [
    (
        "ROLLING_HORIZON_CAPACITY_INVALID",
        "horizonPreview 条数不符合 horizonPreviewCapacity：容量为 0～2 时返回不超过容量的条数，否则返回 3 到 min(8, 容量) 条，每条都是 {\"objective\":string,\"tentative\":true}。",
    ),
    ("B08_PLAN_LENGTH_OUT_OF_RANGE", "每个场景都必须给出整数 estimatedCharacters，且所有场景之和要落在 chapterLengthRange 内：按区间调整场景和节拍数量，让内容自然撑起篇幅。"),
    ("B08_PLAN_ROLE_INVALID", "chapterRole 缺失或不在 chapterRoleOptions 中：从中选择一个本章角色，并让场景篇幅总和落在该角色的区间内。"),
    ("B08_PLAN_BEATS_INVALID", "每个场景都必须写出 sceneType、goal、conflict、turn、result，以及 2～4 个节拍（beats），每个节拍说明这一步发生了什么。"),
    ("B08_PLAN_BEATS_INSUFFICIENT", "节拍总数撑不起本章篇幅：增加真正的情节节拍（新的受挫、发现或选择）或增加场景，每个节拍约 250～400 字，不要靠描写凑字数。"),
    ("B08_PLAN_GOAL_REPEATED", "本章目标与 recentChapters 中已写章节的目标重复：从最近一章的结尾继续，给本章一个带来新推进的目标。"),
];

/// The feedback for a plan rejection the planner can fix, or `None` when
/// planning again would not help.
pub fn plan_feedback(code: &str) -> Option<Value> {
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

/// The length and craft checks a plan must pass before it is written.
pub fn check_plan(
    plan: &Value,
    range: ChapterLengthRange,
    recent_chapters: &[Value],
) -> Result<(), String> {
    crate::chapter_length::validate_plan_length(plan, range)?;
    validate_plan_craft(plan, range, recent_chapters)
}

/// Every planned beat with the length it should reach: each scene's
/// estimate split evenly over its beats.
pub fn beat_targets(plan: &Value) -> Value {
    plan.get("sceneExecution")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|scene| {
            let beats = scene
                .get("beats")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let estimate = scene
                .get("estimatedCharacters")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let per_beat = estimate / (beats.len().max(1) as i64);
            json!({
                "sceneType":scene.get("sceneType"),
                "estimatedCharacters":estimate,
                "beats":beats.iter().map(|beat| json!({"beat":beat,"targetCharacters":per_beat})).collect::<Vec<_>>()
            })
        })
        .collect()
}

fn scene_estimates(plan: &Value) -> Vec<i64> {
    plan.get("sceneExecution")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|scene| {
            scene
                .get("estimatedCharacters")
                .and_then(Value::as_i64)
                .unwrap_or(0)
        })
        .collect()
}

/// Where a two-part chapter is split: the first scene of the second part,
/// chosen so both parts carry about half of the estimated length. `None`
/// when the plan has fewer than two scenes.
pub fn part_split(plan: &Value) -> Option<usize> {
    let estimates = scene_estimates(plan);
    let total: i64 = estimates.iter().sum();
    (1..estimates.len()).min_by_key(|&split| {
        let first: i64 = estimates[..split].iter().sum();
        (2 * first - total).abs()
    })
}

/// The writer input for one part of a two-part chapter (owner decision
/// 2026-10-18): the plan narrowed to that part's scenes, where the first
/// part stops, and the whole first part for the second to continue from.
pub fn part_render_input(base: &Value, plan: &Value, part: usize, written: &str) -> Value {
    let split = part_split(plan).unwrap_or(0);
    let scenes = plan
        .get("sceneExecution")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let range = if part == 0 {
        0..split
    } else {
        split..scenes.len()
    };
    let estimate: i64 = scene_estimates(plan)[range.clone()].iter().sum();
    let mut input = base.clone();
    input["chapterPlan"]["sceneExecution"] = json!(scenes[range.clone()]);
    if let Some(targets) = base.get("beatTargets").and_then(Value::as_array) {
        input["beatTargets"] = json!(targets[range].to_vec());
    }
    input["part"] = json!({"index":part + 1,"count":2});
    input["partLengthRange"] = json!({"min":estimate * 4 / 5,"max":estimate * 13 / 10});
    if part == 0 {
        input["stopBefore"] = scenes[split]
            .pointer("/beats/0")
            .cloned()
            .unwrap_or(Value::Null);
    } else {
        input["stopBefore"] = Value::Null;
        input["previousText"] = json!(written);
    }
    input
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
        if matches!(character, '.' | ':') && digit(index.checked_sub(1)) && digit(Some(index + 1))
        {
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
    let mut sentences = Vec::new();
    let mut current = String::new();
    for character in body.chars() {
        if character == '\n' {
            if !current.trim().is_empty() {
                sentences.push(current.trim().to_string());
            }
            current.clear();
            continue;
        }
        current.push(character);
        if matches!(character, '。' | '！' | '？' | '…') {
            sentences.push(current.trim().to_string());
            current.clear();
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current.trim().to_string());
    }
    sentences
}

/// Words the writer leans on across chapters (2026-10-19 reading).
const BODY_TICS: [&str; 8] = [
    "喉结", "指尖", "指腹", "指节", "铁锈", "瞳孔", "攥紧", "深吸一口气",
];
const SIMILE_MARKERS: [&str; 6] = ["像", "仿佛", "如同", "宛如", "似的", "好似"];
const NOT_SIMILES: [&str; 12] = [
    "图像", "影像", "录像", "画像", "肖像", "偶像", "镜像", "头像", "像素", "好像", "不像", "想像",
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

/// Owner decision (2026-10-19): habits of model prose the review model keeps
/// missing, found by rule and sent to the rewrite with the review findings:
/// plan wording copied into the prose, pseudo-precise measures, simile
/// overload, body tics the recent chapters (`history`) also lean on, and
/// system-panel brackets. Each finding quotes one sentence that occurs once.
pub fn rule_findings(plan: &Value, body: &str, history: &[String]) -> Vec<Value> {
    let sentences = sentences(body)
        .into_iter()
        .filter(|sentence| body.matches(sentence.as_str()).count() == 1)
        .collect::<Vec<_>>();
    let mut findings = Vec::new();
    let mut flagged = std::collections::BTreeSet::new();
    let mut flag = |sentence: &str, code: &str, severity: &str, problem: String, suggestion: &str| {
        if flagged.insert(sentence.to_string()) {
            findings.push(json!({"code":code,"severity":severity,"quote":sentence,
                "problem":problem,"suggestion":suggestion,"source":"rule"}));
        }
    };
    // Plan wording: any clause of 8 or more characters taken over verbatim.
    let mut plan_texts = vec![plan.get("chapterGoal").and_then(Value::as_str).unwrap_or("").to_string()];
    for scene in plan
        .get("sceneExecution")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for key in ["goal", "conflict", "turn", "result"] {
            plan_texts.extend(scene.get(key).and_then(Value::as_str).map(ToOwned::to_owned));
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
    for clause in plan_texts
        .iter()
        .flat_map(|text| text.split(['，', '。', '；', '！', '？', '、', '：']))
        .map(str::trim)
        .filter(|clause| clause.chars().count() >= 8)
    {
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
            flag(sentence, "prose_wordy", "MEDIUM", "精确到单位的数字过多，像在读仪器读数".into(), "去掉不必要的数字，只留对情节有用的");
        }
    }
    // Similes beyond three per chapter, or three per thousand characters.
    let allowed = (body.chars().count() * 3 / 1000).max(3);
    let mut similes = 0;
    for sentence in &sentences {
        let count = simile_count(sentence);
        similes += count;
        if count > 0 && similes > allowed {
            flag(sentence, "prose_wordy", "MEDIUM", "比喻过密".into(), "删去这个比喻，或换成直接的动作或细节");
        }
    }
    // Body tics: used three times here, or twice here and four times lately.
    for tic in BODY_TICS {
        let here = body.matches(tic).count();
        let lately = history.iter().map(|text| text.matches(tic).count()).sum::<usize>();
        if here >= 3 || (here >= 2 && lately >= 4) {
            for sentence in sentences.iter().filter(|sentence| sentence.contains(tic)).skip(1) {
                flag(sentence, "word_repetition", "MEDIUM", format!("「{tic}」在本章和最近几章反复出现"), "换成这个人物此刻独有的反应");
            }
        }
    }
    // System-panel brackets beyond three.
    for sentence in sentences.iter().filter(|sentence| sentence.contains('【')).skip(3) {
        flag(sentence, "info_dump", "MEDIUM", "系统提示框式文字过多".into(), "把信息融进人物的观察或对话");
    }
    findings.truncate(8);
    findings
}

/// Characters in a phrase the signing check counts as a repetition.
const PHRASE_CHARACTERS: usize = 12;

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
    phrases_used(&texts, history.len(), 3)
}

/// Maximal spans in `texts[from..]` covered by phrases used at least
/// `times` times across all of `texts`.
fn phrases_used(texts: &[String], from: usize, times: usize) -> Vec<String> {
    let texts = texts
        .iter()
        .map(|text| text.chars().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut counts = std::collections::HashMap::<&[char], usize>::new();
    for text in &texts {
        for window in text.windows(PHRASE_CHARACTERS) {
            if !window.contains(&'\n') {
                *counts.entry(window).or_default() += 1;
            }
        }
    }
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
        let findings = rule_findings(&plan, body, &[]);
        assert_eq!(codes(&findings), vec!["summary_where_scene_needed"]);
        assert_eq!(findings[0]["severity"], "HIGH");
        assert_eq!(findings[0]["quote"], "沈砚眼神从迷茫转为坚定，决定反击。");
        assert_eq!(findings[0]["source"], "rule");
    }

    #[test]
    fn craft_rule_findings_flag_pseudo_precise_measures_beyond_two() {
        let body = "门开了三公分。他退后两步。灯闪了7秒。脚印六十八厘米。他停在两毫米外。";
        let findings = rule_findings(&json!({}), body, &[]);
        assert_eq!(codes(&findings), vec!["prose_wordy"; 3]);
        assert_eq!(findings[0]["quote"], "灯闪了7秒。");
    }

    #[test]
    fn craft_rule_findings_count_similes_but_not_words_that_contain_xiang() {
        let plain = "屏幕上的图像和录像都是像素。".repeat(10);
        assert!(rule_findings(&json!({}), &plain, &[]).is_empty());
        let similes = ["他像风。", "雨像针。", "灯像眼。", "门像嘴。", "夜像墨。"]
            .iter()
            .enumerate()
            .map(|(n, s)| format!("{n}{s}"))
            .collect::<String>();
        let findings = rule_findings(&json!({}), &similes, &[]);
        assert_eq!(codes(&findings), vec!["prose_wordy"; 2]);
    }

    #[test]
    fn craft_rule_findings_flag_a_body_tic_the_recent_chapters_also_repeat() {
        let history = vec!["他喉结滚动。喉结一紧。喉结发干。喉结上下滑动。".to_string()];
        let body = "他喉结动了动。她笑了。喉结又滚了一下。";
        let findings = rule_findings(&json!({}), body, &history);
        assert_eq!(codes(&findings), vec!["word_repetition"]);
        assert_eq!(findings[0]["quote"], "喉结又滚了一下。");
        assert!(rule_findings(&json!({}), body, &[]).is_empty());
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
    fn craft_accepts_a_plan_with_role_and_enough_beats() {
        let ok = plan(
            "普通",
            "查明委托人的记忆为何被覆盖",
            vec![
                scene("调查", 3, 900),
                scene("对峙", 3, 900),
                scene("揭示", 2, 700),
            ],
        );
        assert_eq!(validate_plan_craft(&ok, RANGE, &[]), Ok(()));
    }

    #[test]
    fn craft_rejects_missing_role_bad_beat_counts_and_too_few_beats() {
        let no_role = json!({"chapterGoal":"目标","sceneExecution":[scene("调查", 3, 2500)]});
        assert_eq!(
            validate_plan_craft(&no_role, RANGE, &[]),
            Err("B08_PLAN_ROLE_INVALID".into())
        );
        let one_beat = plan(
            "普通",
            "目标",
            vec![scene("调查", 1, 1200), scene("对峙", 4, 1300)],
        );
        assert_eq!(
            validate_plan_craft(&one_beat, RANGE, &[]),
            Err("B08_PLAN_BEATS_INVALID".into())
        );
        let thin = plan("普通", "目标", vec![scene("调查", 2, 2500)]);
        assert_eq!(
            validate_plan_craft(&thin, RANGE, &[]),
            Err("B08_PLAN_BEATS_INSUFFICIENT".into())
        );
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
            validate_plan_craft(&repeated, RANGE, &recent),
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
    fn craft_beat_targets_split_each_scene_estimate_over_its_beats() {
        let committed = plan(
            "普通",
            "目标",
            vec![scene("调查", 3, 900), scene("对峙", 2, 1000)],
        );
        let targets = beat_targets(&committed);
        assert_eq!(targets[0]["sceneType"], "调查");
        assert_eq!(
            targets[0]["beats"][2],
            json!({"beat":"节拍2","targetCharacters":300})
        );
        assert_eq!(targets[1]["beats"][0]["targetCharacters"], 500);
        assert!(
            plan_feedback("ROLLING_HORIZON_CAPACITY_INVALID").unwrap()["instruction"]
                .as_str()
                .unwrap()
                .contains("horizonPreviewCapacity")
        );
        assert!(plan_feedback("CHAPTER_NOT_FOUND").is_none());
        assert!(
            plan_feedback("B08_PLAN_LENGTH_OUT_OF_RANGE").unwrap()["instruction"]
                .as_str()
                .unwrap()
                .contains("estimatedCharacters")
        );
    }

    #[test]
    fn craft_parts_split_the_estimate_in_half() {
        let four = plan(
            "普通",
            "目标",
            vec![
                scene("调查", 3, 600),
                scene("对峙", 3, 700),
                scene("揭示", 3, 800),
                scene("日常", 3, 600),
            ],
        );
        assert_eq!(part_split(&four), Some(2));
        let two = plan(
            "普通",
            "目标",
            vec![scene("调查", 3, 1200), scene("对峙", 3, 1300)],
        );
        assert_eq!(part_split(&two), Some(1));
        let one = plan("普通", "目标", vec![scene("调查", 3, 2500)]);
        assert_eq!(part_split(&one), None);
    }

    #[test]
    fn craft_part_inputs_narrow_the_plan_and_join_at_the_seam() {
        let committed = plan(
            "普通",
            "目标",
            vec![
                scene("调查", 3, 600),
                scene("对峙", 3, 700),
                scene("揭示", 3, 800),
                scene("日常", 3, 600),
            ],
        );
        let base = json!({"chapterId":"c","chapterPlan":committed.clone(),"beatTargets":beat_targets(&committed)});
        let first = part_render_input(&base, &committed, 0, "");
        assert_eq!(first["part"], json!({"index":1,"count":2}));
        let scenes = first["chapterPlan"]["sceneExecution"].as_array().unwrap();
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[1]["sceneType"], "对峙");
        assert_eq!(first["chapterPlan"]["chapterGoal"], "目标");
        assert_eq!(first["beatTargets"].as_array().unwrap().len(), 2);
        assert_eq!(first["stopBefore"], "节拍0");
        assert_eq!(first["partLengthRange"], json!({"min":1040,"max":1690}));
        assert!(first.get("previousText").is_none());
        let written = "第一段正文。".repeat(300);
        let second = part_render_input(&base, &committed, 1, &written);
        assert_eq!(second["part"], json!({"index":2,"count":2}));
        assert_eq!(
            second["chapterPlan"]["sceneExecution"][0]["sceneType"],
            "揭示"
        );
        assert_eq!(second["previousText"], written);
        assert!(second["stopBefore"].is_null());
        assert_eq!(second["partLengthRange"], json!({"min":1120,"max":1820}));
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
            "【S_Y_042-RECORD-12.07】雨还在下。".to_string(),
            "他翻到【S_Y_042-RECORD-12.08】。".to_string(),
        ];
        let phrases = repeated_phrases(&texts);
        assert_eq!(phrases, vec!["【S_Y_042-RECORD-12.0".to_string()]);
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
