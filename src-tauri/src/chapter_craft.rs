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

/// Extra planner input: chapter position, what the recent chapters already
/// did, the role options with their sub-ranges, and the selected skills.
pub fn planning_inputs(
    chapter_number: Option<u32>,
    recent_chapters: &[Value],
    range: ChapterLengthRange,
) -> Result<Value, String> {
    let signals = StepSignals {
        chapter_number,
        ..StepSignals::default()
    };
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
            .map(ToOwned::to_owned)
            .collect(),
        ..StepSignals::default()
    }
}

/// What a rewrite must fix when a body missed the length range: the gap and
/// every planned beat with the length it should reach.
pub fn length_feedback(plan: &Value, range: ChapterLengthRange, count: i64) -> Value {
    let scenes = plan
        .get("sceneExecution")
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
        .collect::<Vec<_>>();
    json!({
        "previousCharacterCount":count,
        "shortBy":(range.min - count).max(0),
        "overBy":(count - range.ceiling()).max(0),
        "scenes":scenes,
        "instruction":"上一稿篇幅不在区间内，已作废。按本章规划重新完整写作：逐个节拍展开到目标篇幅，补足动作、对话、感受和细节；不要重复已有内容，也不要加入与场景目标无关的描写。"
    })
}

/// Plan rejections the planner can fix itself, with what to tell it.
const REPLANNABLE: [(&str, &str); 5] = [
    ("B08_PLAN_LENGTH_OUT_OF_RANGE", "场景篇幅估计之和不在 chapterLengthRange 内：按区间调整场景和节拍数量，让内容自然撑起篇幅。"),
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
    fn craft_length_feedback_names_the_gap_and_per_beat_targets() {
        let committed = plan(
            "普通",
            "目标",
            vec![scene("调查", 3, 900), scene("对峙", 2, 1000)],
        );
        let short = length_feedback(&committed, RANGE, 1500);
        assert_eq!(short["previousCharacterCount"], 1500);
        assert_eq!(short["shortBy"], 500);
        assert_eq!(short["overBy"], 0);
        assert_eq!(
            short["scenes"][0]["beats"][0],
            json!({"beat":"节拍0","targetCharacters":300})
        );
        assert_eq!(short["scenes"][1]["beats"][1]["targetCharacters"], 500);
        assert!(short["instruction"].as_str().unwrap().contains("不要重复"));
        let long = length_feedback(&committed, RANGE, 3400);
        assert_eq!(long["shortBy"], 0);
        assert_eq!(long["overBy"], 100);
    }
}
