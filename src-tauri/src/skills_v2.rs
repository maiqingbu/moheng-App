//! Creative skills v2: reviewed craft methods embedded at build time and
//! selected per production step from the step's signals. Only a skill's
//! stage-specific "注入要点" section is handed to the model.

use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/skills_v2_embedded.rs"));
}

pub const SKILLS_V2_VERSION: &str = "2.0.0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectSpec {
    pub budget_chars: usize,
    pub section: String,
}

#[derive(Clone, Debug)]
pub struct Skill {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub stages: Vec<String>,
    pub triggers: BTreeMap<String, Vec<String>>,
    pub inject: BTreeMap<String, InjectSpec>,
    body: String,
}

fn list_value(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn inject_value(value: &str) -> Option<InjectSpec> {
    let inner = value.trim().strip_prefix('{')?.strip_suffix('}')?;
    let mut budget_chars = None;
    let mut section = None;
    for part in inner.split(',') {
        let (key, value) = part.split_once(':')?;
        match key.trim() {
            "budget_chars" => budget_chars = value.trim().parse().ok(),
            "section" => section = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    Some(InjectSpec {
        budget_chars: budget_chars?,
        section: section?,
    })
}

impl Skill {
    /// Parses the reviewed v2 front matter subset: scalars, inline lists and
    /// the two nested maps `triggers` and `inject`.
    pub fn parse(text: &str) -> Result<Skill, String> {
        let rest = text
            .strip_prefix("---\n")
            .ok_or("SKILLS_V2_FRONT_MATTER_MISSING")?;
        let (front, body) = rest
            .split_once("\n---\n")
            .ok_or("SKILLS_V2_FRONT_MATTER_UNTERMINATED")?;
        let mut scalars = BTreeMap::new();
        let mut triggers = BTreeMap::new();
        let mut inject = BTreeMap::new();
        let mut nested = "";
        for line in front.lines() {
            if let Some(child) = line.strip_prefix("  ") {
                let Some((key, value)) = child.split_once(':') else {
                    continue;
                };
                match nested {
                    "triggers" => {
                        triggers.insert(key.trim().to_owned(), list_value(value));
                    }
                    "inject" => {
                        let spec = inject_value(value)
                            .ok_or_else(|| format!("SKILLS_V2_INJECT_INVALID:{line}"))?;
                        inject.insert(key.trim().to_owned(), spec);
                    }
                    _ => {}
                }
            } else if let Some((key, value)) = line.split_once(':') {
                nested = if value.trim().is_empty() {
                    key.trim()
                } else {
                    ""
                };
                scalars.insert(key.trim().to_owned(), value.trim().to_owned());
            }
        }
        let scalar = |key: &str| {
            scalars
                .get(key)
                .filter(|value| !value.is_empty())
                .cloned()
                .ok_or_else(|| format!("SKILLS_V2_FIELD_MISSING:{key}"))
        };
        let skill = Skill {
            id: scalar("id")?,
            slug: scalar("slug")?,
            title: scalar("title")?,
            stages: list_value(&scalar("stages")?),
            triggers,
            inject,
            body: body.to_owned(),
        };
        for spec in skill.inject.values() {
            if skill.section(&spec.section).is_none() {
                return Err(format!(
                    "SKILLS_V2_SECTION_MISSING:{}:{}",
                    skill.id, spec.section
                ));
            }
        }
        Ok(skill)
    }

    /// The body of `## <name>` without its blockquote notes.
    pub fn section(&self, name: &str) -> Option<String> {
        let heading = format!("\n## {name}\n");
        let start = self.body.find(&heading)? + heading.len();
        let rest = &self.body[start..];
        let end = rest.find("\n## ").unwrap_or(rest.len());
        let text = rest[..end]
            .lines()
            .filter(|line| !line.starts_with('>'))
            .collect::<Vec<_>>()
            .join("\n");
        Some(text.trim().to_owned())
    }
}

/// The embedded skills, parsed once per process.
pub fn catalog() -> Result<&'static [Skill], String> {
    static CATALOG: OnceLock<Result<Vec<Skill>, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            embedded::SKILL_FILES
                .iter()
                .map(|text| Skill::parse(text))
                .collect()
        })
        .as_deref()
        .map_err(Clone::clone)
}

/// Every scene type some skill is written for, so the planner can label
/// scenes in words the skills know.
pub fn scene_type_vocabulary() -> Result<&'static [String], String> {
    static VOCABULARY: OnceLock<Result<Vec<String>, String>> = OnceLock::new();
    cached_vocabulary(&VOCABULARY, "scene_types")
}

/// Every review finding code some skill is written to fix, so a review
/// reports problems in words the rewrite skills know.
pub fn finding_vocabulary() -> Result<&'static [String], String> {
    static VOCABULARY: OnceLock<Result<Vec<String>, String>> = OnceLock::new();
    cached_vocabulary(&VOCABULARY, "findings")
}

fn cached_vocabulary(
    cell: &'static OnceLock<Result<Vec<String>, String>>,
    key: &str,
) -> Result<&'static [String], String> {
    cell.get_or_init(|| {
        let mut values = std::collections::BTreeSet::new();
        for skill in catalog()? {
            if let Some(listed) = skill.triggers.get(key) {
                values.extend(listed.iter().cloned());
            }
        }
        Ok(values.into_iter().collect())
    })
    .as_deref()
    .map_err(Clone::clone)
}

/// The skill vocabulary's word for a scene label a planner may use instead.
pub fn canonical_scene_type(label: &str) -> String {
    match label.trim() {
        "动作冲突" | "动作" | "打斗" | "冲突" => "战斗",
        "过渡" | "场景过渡" => "转场",
        "揭秘" | "真相揭示" => "信息揭示",
        "悬疑" | "探查" | "搜查" => "调查",
        "情感高潮" | "情绪爆发" => "情绪峰值",
        other => other,
    }
    .to_string()
}

/// What is known about the step being performed.
#[derive(Clone, Debug, Default)]
pub struct StepSignals {
    pub chapter_number: Option<u32>,
    pub chapter_role: Option<String>,
    pub scene_types: Vec<String>,
    pub findings: Vec<String>,
    pub events: Vec<String>,
    pub final_volume: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedSkill {
    pub skill_id: String,
    pub title: String,
    pub reason: String,
    pub guidance: String,
}

/// Total injected characters allowed per step.
pub fn stage_budget(stage: &str) -> usize {
    match stage {
        "chapter_plan" | "book_plan" => 5000,
        "render" | "volume_plan" => 4000,
        "story_setup" | "review" | "rewrite" => 3000,
        _ => 1500,
    }
}

fn chapter_positions(signals: &StepSignals) -> Vec<&'static str> {
    let mut positions = Vec::new();
    if let Some(number) = signals.chapter_number {
        if number == 1 {
            positions.push("第1章");
        }
        if number <= 3 {
            positions.push("开篇1-3章");
        }
        if number <= 10 {
            positions.push("开篇1-10章");
        }
    }
    if signals.final_volume {
        positions.push("终卷");
    }
    if matches!(signals.chapter_role.as_deref(), Some("高潮前" | "高潮")) {
        positions.push("高潮前后");
    }
    positions
}

/// Scores a skill for the step; returns the score and the human reason.
fn match_skill(skill: &Skill, stage: &str, signals: &StepSignals) -> Option<(u32, String)> {
    let trigger = |key: &str| skill.triggers.get(key).map(Vec::as_slice).unwrap_or(&[]);
    if trigger("always_in").iter().any(|item| item == stage) {
        return Some((1000, "本环节常驻".into()));
    }
    let mut score = 0;
    let mut reasons = Vec::new();
    let mut hit = |key: &str, label: &str, weight: u32, values: Vec<&str>| {
        for value in values {
            if trigger(key).iter().any(|item| item == value) {
                score += weight;
                reasons.push(format!("{label}：{value}"));
            }
        }
    };
    hit("findings", "审核问题", 300, strs(&signals.findings));
    hit("events", "事件", 200, strs(&signals.events));
    hit(
        "chapter_roles",
        "本章角色",
        100,
        signals.chapter_role.as_deref().into_iter().collect(),
    );
    hit("scene_types", "场景类型", 100, strs(&signals.scene_types));
    hit(
        "chapter_position",
        "章节位置",
        100,
        chapter_positions(signals),
    );
    (score > 0).then(|| (score, reasons.join("；")))
}

fn strs(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

fn id_number(id: &str) -> u32 {
    id.trim_start_matches('S').parse().unwrap_or(u32::MAX)
}

/// Picks the skills for one step: residents first, then the best matches,
/// as long as their stage guidance fits the budget.
pub fn select_for_step(
    stage: &str,
    signals: &StepSignals,
    budget_chars: usize,
) -> Result<Vec<SelectedSkill>, String> {
    let mut candidates = Vec::new();
    for skill in catalog()? {
        let Some(spec) = skill.inject.get(stage) else {
            continue;
        };
        let Some((score, reason)) = match_skill(skill, stage, signals) else {
            continue;
        };
        let guidance = skill.section(&spec.section).unwrap_or_default();
        candidates.push((score, id_number(&skill.id), skill, reason, guidance));
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut used = 0;
    let mut selected = Vec::new();
    for (_, _, skill, reason, guidance) in candidates {
        let size = guidance.chars().filter(|c| !c.is_whitespace()).count();
        if used + size > budget_chars {
            continue;
        }
        used += size;
        selected.push(SelectedSkill {
            skill_id: skill.id.clone(),
            title: skill.title.clone(),
            reason,
            guidance,
        });
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "---\nid: S99\nslug: sample-skill\ntitle: 样例\nversion: 2.0.0\ncoverage: HIGH\nstages: [chapter_plan, render]\ntriggers:\n  always_in: [chapter_plan]\n  scene_types: [战斗, 追逐]\ninject:\n  chapter_plan: { budget_chars: 300, section: 注入要点·章节 }\n  render: { budget_chars: 200, section: 注入要点·写作 }\nsources:\n  - 来源 · 小节\n---\n\n# 样例\n\n## 方法\n\n正文。\n\n## 注入要点·章节\n\n> 章节规划时注入，不超过 300 字。\n\n规划要点一。\n规划要点二。\n\n## 注入要点·写作\n\n写作要点。\n";

    #[test]
    fn v2_parse_reads_front_matter_triggers_and_inject_specs() {
        let skill = Skill::parse(SAMPLE).unwrap();
        assert_eq!(skill.id, "S99");
        assert_eq!(skill.slug, "sample-skill");
        assert_eq!(skill.title, "样例");
        assert_eq!(skill.stages, vec!["chapter_plan", "render"]);
        assert_eq!(skill.triggers["scene_types"], vec!["战斗", "追逐"]);
        assert_eq!(
            skill.inject["chapter_plan"],
            InjectSpec {
                budget_chars: 300,
                section: "注入要点·章节".into()
            }
        );
    }

    #[test]
    fn v2_section_strips_blockquote_notes_and_stops_at_next_heading() {
        let skill = Skill::parse(SAMPLE).unwrap();
        assert_eq!(
            skill.section("注入要点·章节").unwrap(),
            "规划要点一。\n规划要点二。"
        );
        assert_eq!(skill.section("注入要点·写作").unwrap(), "写作要点。");
        assert!(skill.section("不存在").is_none());
    }

    #[test]
    fn v2_parse_rejects_missing_front_matter_or_dangling_inject_section() {
        assert!(Skill::parse("# no front matter").is_err());
        let dangling = SAMPLE.replace("section: 注入要点·写作", "section: 注入要点·缺失");
        assert!(Skill::parse(&dangling).is_err());
    }

    fn ids(selected: &[SelectedSkill]) -> Vec<&str> {
        selected
            .iter()
            .map(|skill| skill.skill_id.as_str())
            .collect()
    }

    fn reason_of<'a>(selected: &'a [SelectedSkill], id: &str) -> &'a str {
        &selected
            .iter()
            .find(|skill| skill.skill_id == id)
            .unwrap()
            .reason
    }

    #[test]
    fn v2_first_chapter_plan_selects_resident_and_opening_skills_with_reasons() {
        let signals = StepSignals {
            chapter_number: Some(1),
            ..StepSignals::default()
        };
        let selected =
            select_for_step("chapter_plan", &signals, stage_budget("chapter_plan")).unwrap();
        let chosen = ids(&selected);
        for resident in ["S25", "S26", "S14"] {
            assert!(chosen.contains(&resident), "{resident} missing: {chosen:?}");
            assert!(reason_of(&selected, resident).contains("常驻"));
        }
        assert!(chosen.contains(&"S13"), "{chosen:?}");
        assert!(reason_of(&selected, "S13").contains("开篇1-3章"));
        assert!(chosen.contains(&"S09"));
        assert!(selected.iter().all(|skill| !skill.guidance.is_empty()));
    }

    #[test]
    fn v2_later_chapters_do_not_get_opening_skills() {
        let signals = StepSignals {
            chapter_number: Some(12),
            ..StepSignals::default()
        };
        let selected =
            select_for_step("chapter_plan", &signals, stage_budget("chapter_plan")).unwrap();
        assert!(!ids(&selected).contains(&"S13"));
        assert!(!ids(&selected).contains(&"S09"));
    }

    #[test]
    fn v2_render_follows_scene_types_and_chapter_role() {
        let signals = StepSignals {
            chapter_number: Some(8),
            chapter_role: Some("回报".into()),
            scene_types: vec!["战斗".into(), "对话为主".into()],
            ..StepSignals::default()
        };
        let selected = select_for_step("render", &signals, stage_budget("render")).unwrap();
        let chosen = ids(&selected);
        assert!(chosen.contains(&"S27"));
        assert!(chosen.contains(&"S30"), "{chosen:?}");
        assert!(reason_of(&selected, "S30").contains("战斗"));
        assert!(chosen.contains(&"S28"), "{chosen:?}");
        assert!(chosen.contains(&"S24"), "{chosen:?}");
        assert!(reason_of(&selected, "S24").contains("回报"));
    }

    #[test]
    fn v2_rewrite_selects_by_review_findings() {
        let signals = StepSignals {
            findings: vec!["beat_underdeveloped".into()],
            ..StepSignals::default()
        };
        let selected = select_for_step("rewrite", &signals, stage_budget("rewrite")).unwrap();
        let chosen = ids(&selected);
        assert!(chosen.contains(&"S40"));
        assert!(chosen.contains(&"S46"));
    }

    #[test]
    fn v2_selection_stays_within_budget_and_keeps_residents_first() {
        let signals = StepSignals {
            chapter_number: Some(2),
            chapter_role: Some("高潮".into()),
            scene_types: vec!["战斗".into(), "情绪峰值".into(), "揭示".into()],
            ..StepSignals::default()
        };
        let selected = select_for_step("render", &signals, 1500).unwrap();
        let used = selected
            .iter()
            .map(|skill| {
                skill
                    .guidance
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .count()
            })
            .sum::<usize>();
        assert!(used <= 1500, "{used}");
        assert_eq!(ids(&selected)[0..2], ["S27", "S32"]);
        assert!(select_for_step("unknown_stage", &signals, 9999)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn v2_catalog_embeds_all_47_skills_with_injectable_text_within_budget() {
        let skills = catalog().unwrap();
        assert_eq!(skills.len(), 47);
        let mut ids = skills
            .iter()
            .map(|skill| skill.id.clone())
            .collect::<Vec<_>>();
        ids.sort_by_key(|id| id[1..].parse::<u32>().unwrap());
        assert_eq!(ids.first().map(String::as_str), Some("S01"));
        assert_eq!(ids.last().map(String::as_str), Some("S47"));
        ids.dedup();
        assert_eq!(ids.len(), 47);
        for skill in skills {
            for (stage, spec) in &skill.inject {
                let text = skill.section(&spec.section).unwrap();
                assert!(!text.is_empty(), "{} {stage}", skill.id);
                assert!(
                    text.chars().filter(|c| !c.is_whitespace()).count() <= spec.budget_chars,
                    "{} {stage} over budget",
                    skill.id
                );
            }
        }
    }
}
