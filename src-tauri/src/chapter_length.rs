//! FX-18: chapter length range (owner decisions 2026-10-17).
//!
//! A chapter is written for a character range chosen per work (project
//! setting `chapterLengthRange`) and optionally per chapter (production
//! payload). The plan must be sized for the range and the committed body must
//! fall inside it, allowing up to 10% above the maximum.

use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChapterLengthRange {
    pub min: i64,
    pub max: i64,
}

pub const DEFAULT_CHAPTER_LENGTH_RANGE: ChapterLengthRange = ChapterLengthRange {
    min: 2_000,
    max: 3_000,
};

/// LD-1.5 (owner decision 2026-10-25): the characters a chapter aims for
/// on each platform (ids as in signing.rs `PLATFORM_IDS`), and the names a
/// work may give the platform. These are
/// trade conventions, not platform rules: platforms count daily updates,
/// not chapter length (番茄 chapters run 2000-2300, paid platforms 2500-3500).
const PLATFORM_CHAPTER_TARGETS: &[(&[&str], i64)] = &[
    (&["FANQIE", "番茄"], 2_000),
    (&["QIMAO", "七猫"], 2_000),
    (&["QIDIAN", "起点"], 3_000),
    (&["ZONGHENG", "纵横"], 3_000),
    (&["JJWXC", "晋江"], 3_000),
];

const SMALLEST_MIN: i64 = 500;
const LARGEST_MAX: i64 = 10_000;

impl ChapterLengthRange {
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let bound = |name: &str| value.get(name).and_then(Value::as_i64);
        match (bound("min"), bound("max")) {
            (Some(min), Some(max)) if SMALLEST_MIN <= min && min < max && max <= LARGEST_MAX => {
                Ok(Self { min, max })
            }
            _ => Err("CHAPTER_LENGTH_RANGE_INVALID".into()),
        }
    }

    /// The range a command payload chooses for its chapter: an explicit
    /// `chapterLengthRange`, or the legacy single `targetChineseCharacters`.
    pub fn from_payload(payload: &Value) -> Result<Option<Self>, String> {
        if let Some(range) = payload.get("chapterLengthRange") {
            return Self::from_value(range).map(Some);
        }
        if let Some(target) = payload
            .get("targetChineseCharacters")
            .and_then(Value::as_i64)
        {
            return Self::from_value(&json!({"min":target * 4 / 5,"max":target})).map(Some);
        }
        Ok(None)
    }

    /// The range for a platform the work names (its id or Chinese name):
    /// 90% to 110% of the platform's target. A text that names more than
    /// one platform names none.
    pub fn for_platform(platform: &str) -> Option<Self> {
        let platform = platform.trim().to_uppercase();
        let exact = PLATFORM_CHAPTER_TARGETS
            .iter()
            .find(|(names, _)| names[0] == platform);
        let mut named = PLATFORM_CHAPTER_TARGETS
            .iter()
            .filter(|(names, _)| names.iter().any(|name| platform.contains(name)));
        exact
            .or_else(|| named.next().filter(|_| named.next().is_none()))
            .map(|(_, target)| Self {
                min: target * 9 / 10,
                max: target * 11 / 10,
            })
    }

    /// A range chosen now for this chapter, then the range saved for this
    /// chapter earlier, then the work's setting, then its platform's, then
    /// the default.
    pub fn resolve(
        chosen: Option<Self>,
        chapter_setting: Option<&Value>,
        work_setting: Option<&Value>,
        platform: Option<&str>,
    ) -> Result<Self, String> {
        if let Some(range) = chosen {
            return Ok(range);
        }
        match chapter_setting
            .filter(|setting| !setting.is_null())
            .or(work_setting.filter(|setting| !setting.is_null()))
        {
            Some(setting) => Self::from_value(setting),
            None => Ok(platform
                .and_then(Self::for_platform)
                .unwrap_or(DEFAULT_CHAPTER_LENGTH_RANGE)),
        }
    }

    /// Bodies may run up to 10% past the maximum.
    pub fn ceiling(&self) -> i64 {
        self.max + self.max / 10
    }

    /// Whether a draft is past 1.3 times the ceiling, too long for cutting
    /// sentences to leave a chapter.
    pub fn is_too_long_to_cut(&self, characters: i64) -> bool {
        characters * 10 > self.ceiling() * 13
    }

    /// Round 11 (owner decision 2026-11-26): whether a draft is more than
    /// 15% past the maximum or short of the minimum, too far off to cut or
    /// pad without harm, so it is written again.
    pub fn is_far_off(&self, characters: i64) -> bool {
        self.is_far_too_long(characters) || characters * 20 < self.min * 17
    }

    /// Whether a draft is more than 15% past the maximum.
    pub fn is_far_too_long(&self, characters: i64) -> bool {
        characters * 20 > self.max * 23
    }

    /// Round 13 (owner decision 2026-11-27): what the writer aims for, the
    /// range's minimum: the deepseek writer runs two or three tenths past
    /// it. A writer that hits its aim lands near the minimum.
    pub fn aim(&self) -> i64 {
        self.min
    }

    /// The length a chapter aims for: the middle of the range.
    pub fn target(&self) -> i64 {
        (self.min + self.max) / 2
    }

    pub fn accepts(&self, characters: i64) -> bool {
        self.min <= characters && characters <= self.ceiling()
    }

    pub fn to_json(self) -> Value {
        json!({"min":self.min,"max":self.max})
    }
}

/// The platform the work is written for: the one confirmed at signing, else
/// the one in the work's settings, else the one it was started for.
pub fn work_platform(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    work_settings: &Value,
) -> Result<Option<String>, String> {
    use crate::repositories::project_entry::{canonical_head, latest_artifact_by_type};
    let named = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .filter(|platform| !platform.trim().is_empty())
            .map(ToOwned::to_owned)
    };
    let signed = canonical_head(connection, project_id, "SimulatedSigningReceipt")
        .map_err(|error| error.to_string())?
        .filter(|head| head.payload.pointer("/confirmation/confirmed") == Some(&Value::Bool(true)))
        .and_then(|head| named(head.payload.pointer("/confirmation/platformId")));
    if signed.is_some() {
        return Ok(signed);
    }
    if let Some(platform) = named(work_settings.get("platform")) {
        return Ok(Some(platform));
    }
    Ok(
        latest_artifact_by_type(connection, project_id, "CreativeBrief")
            .map_err(|error| error.to_string())?
            .and_then(|brief| named(brief.payload.get("platform"))),
    )
}

/// The count shown to the user as the chapter's word count: every character
/// but line breaks and spaces (owner decision 2026-11-25).
pub fn character_count(text: &str) -> i64 {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .count() as i64
}

/// What a cut must keep of the plan's events: the telling pairs of each
/// (but for those of the names of the people there), and the numbers and
/// codes they name, such as a date.
#[derive(Default)]
pub(crate) struct PlannedEvents {
    pairs: Vec<BTreeSet<(char, char)>>,
    codes: BTreeSet<String>,
}

/// How long a run of ASCII letters and digits is before it counts as a code.
const CODE_CHARS: usize = 3;

/// The runs of `CODE_CHARS` or more ASCII letters and digits in `text`.
fn codes(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|run| run.len() >= CODE_CHARS)
}

pub(crate) fn planned_events(plan: &Value) -> PlannedEvents {
    use crate::chapter_craft::telling_pairs;
    let craft = &plan["craft"];
    let names = crate::craft_plan::scene_people(craft)
        .filter_map(|person| person["name"].as_str())
        .flat_map(telling_pairs)
        .collect::<BTreeSet<_>>();
    let events = crate::craft_plan::scene_events(craft).collect::<Vec<_>>();
    PlannedEvents {
        pairs: events
            .iter()
            .map(|event| &telling_pairs(event) - &names)
            .collect(),
        codes: events
            .iter()
            .flat_map(|event| codes(event))
            .map(ToOwned::to_owned)
            .collect(),
    }
}

/// How many telling pairs a sentence shares with a planned event it writes.
const EVENT_PAIRS: usize = 3;

/// LD-4: `body` shortened by deleting its sentences (numbered as in
/// `chapter_craft::sentence_spans`) in the order `cut` names them, until
/// it is at most the range's maximum, which leaves the rewrite the 10% past
/// it. A cut is skipped when it would take
/// the chapter below the minimum, or `can_cut_sentence` refuses it.
pub(crate) fn cut_sentences(
    body: &str,
    cut: &[usize],
    range: ChapterLengthRange,
    events: &PlannedEvents,
) -> String {
    let spans = crate::chapter_craft::sentence_spans(body);
    let (groups, cuttable) = cut_options(body, &spans, events);
    let mut removed = vec![false; spans.len()];
    let mut shortened = body.to_string();
    for &number in cut {
        if character_count(&shortened) <= range.max {
            break;
        }
        // Real run round 9: cutting single lines of an exchange left answers
        // without their questions; an exchange goes whole or not at all.
        // Round 10: and only when most of it was chosen, so one dispensable
        // line never takes a whole conversation with it.
        let Some(group) = groups.get(number) else {
            continue;
        };
        let chosen_lines = group.iter().filter(|n| cut.contains(n)).count();
        if !cuttable[number] || group.iter().any(|&n| removed[n]) || chosen_lines * 2 < group.len()
        {
            continue;
        }
        group.iter().for_each(|&n| removed[n] = true);
        let candidate = without_sentences(body, &spans, &removed);
        if character_count(&candidate) < range.min {
            group.iter().for_each(|&n| removed[n] = false);
        } else {
            shortened = candidate;
        }
    }
    shortened
}

/// For each sentence, the sentences of the exchange it belongs to: those
/// of the lines with a quote in the conversation around its line, or only
/// itself when its line has none. One line of narration between two such
/// lines does not end the conversation (real run round 10: 小杨脚步停了一下
/// split a question from its answer); it goes with the exchange (round 13:
/// cut alone, it left a line answering an action no longer written). A line
/// that must stay (a planned event, by `single`) does end it.
fn exchanges(body: &str, spans: &[(usize, usize)], single: &[bool]) -> Vec<Vec<usize>> {
    let lines = body.split('\n').collect::<Vec<_>>();
    // The run of spoken lines each line is in, blank lines between kept.
    let mut runs = vec![None; lines.len()];
    let mut run = 0;
    let mut line = 0;
    let mut at = 0;
    let line_of = spans
        .iter()
        .map(|&(start, _)| {
            line += body[at..start].matches('\n').count();
            at = start;
            line
        })
        .collect::<Vec<_>>();
    let kept_lines = line_of
        .iter()
        .zip(single)
        .filter(|(_, cuttable)| !**cuttable)
        .map(|(line, _)| *line)
        .collect::<BTreeSet<_>>();
    // Narration lines since the last spoken one; None before any.
    let mut since_spoken = None;
    // The latest narration line.
    let mut narration = 0;
    for (index, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if line.contains(['“', '「']) {
            match since_spoken {
                Some(0) => {}
                Some(1) => runs[narration] = Some(run),
                _ => run += 1,
            }
            runs[index] = Some(run);
            since_spoken = Some(0);
        } else if kept_lines.contains(&index) {
            since_spoken = None;
        } else {
            since_spoken = since_spoken.map(|lines: usize| lines + 1);
            narration = index;
        }
    }
    (0..spans.len())
        .map(|number| match runs[line_of[number]] {
            Some(run) => (0..spans.len())
                .filter(|&other| runs[line_of[other]] == Some(run))
                .collect(),
            None => vec![number],
        })
        .collect()
}

/// For each sentence of `body`, the exchange it belongs to (see
/// `exchanges`) and whether a cut may take it: it and every sentence of its
/// exchange pass `can_cut_sentence`.
pub(crate) fn cut_options(
    body: &str,
    spans: &[(usize, usize)],
    events: &PlannedEvents,
) -> (Vec<Vec<usize>>, Vec<bool>) {
    let single = single_cuts(body, spans, events);
    let groups = exchanges(body, spans, &single);
    let cuttable = groups
        .iter()
        .map(|group| group.iter().all(|&n| single[n]))
        .collect();
    (groups, cuttable)
}

/// For each sentence, whether `can_cut_sentence` lets it go on its own.
fn single_cuts(body: &str, spans: &[(usize, usize)], events: &PlannedEvents) -> Vec<bool> {
    (0..spans.len())
        .map(|number| can_cut_sentence(body, spans, number, events))
        .collect()
}

/// Not the last sentence (the planned ending), not half of a line of
/// dialogue, not one that
/// writes a planned event or names its number or code, and not the opener
/// that names who the next sentence's 他/她/它 is.
pub(crate) fn can_cut_sentence(
    body: &str,
    spans: &[(usize, usize)],
    number: usize,
    events: &PlannedEvents,
) -> bool {
    let Some(&[(start, end), (next_start, next_end)]) =
        spans.get(number..).and_then(<[_]>::first_chunk::<2>)
    else {
        return false;
    };
    let sentence = &body[start..end];
    if sentence.matches('“').count() != sentence.matches('”').count() {
        return false;
    }
    let pairs = crate::chapter_craft::telling_pairs(sentence);
    if events
        .pairs
        .iter()
        .any(|event| event.intersection(&pairs).count() >= EVENT_PAIRS)
        || codes(sentence).any(|code| events.codes.contains(code))
    {
        return false;
    }
    let same_paragraph = !body[end..next_start].contains('\n');
    let next_refers_to_actor = body[next_start..next_end]
        .trim_start()
        .starts_with(['他', '她', '它']);
    !(same_paragraph && next_refers_to_actor)
}

/// `body` without the `removed` sentences; a line they emptied goes, with
/// the blank line after it.
fn without_sentences(body: &str, spans: &[(usize, usize)], removed: &[bool]) -> String {
    let mut kept = String::with_capacity(body.len());
    let mut at = 0;
    for (&(start, end), _) in spans.iter().zip(removed).filter(|(_, removed)| **removed) {
        kept.push_str(&body[at..start]);
        at = end;
    }
    kept.push_str(&body[at..]);
    // Sentences never span a line, so the lines still pair up.
    let mut lines = Vec::new();
    let mut emptied = false;
    for (before, after) in body.split('\n').zip(kept.split('\n')) {
        let was_blank = before.trim().is_empty();
        if after.trim().is_empty() && (!was_blank || emptied) {
            emptied = !was_blank;
            continue;
        }
        emptied = false;
        lines.push(after);
    }
    lines.join("\n")
}

/// Sum of the plan's per-scene `estimatedCharacters`; None when any scene
/// lacks a positive estimate or there are no scenes.
pub fn plan_estimate(plan: &Value) -> Option<i64> {
    let scenes = plan.get("sceneExecution")?.as_array()?;
    if scenes.is_empty() {
        return None;
    }
    scenes.iter().try_fold(0_i64, |sum, scene| {
        scene
            .get("estimatedCharacters")
            .and_then(Value::as_i64)
            .filter(|estimate| *estimate > 0)
            .map(|estimate| sum + estimate)
    })
}

/// A plan is sized for the range when its scene estimates add up to a total
/// within [min, max].
pub fn validate_plan_length(plan: &Value, range: ChapterLengthRange) -> Result<(), String> {
    match plan_estimate(plan) {
        Some(total) if range.min <= total && total <= range.max => Ok(()),
        _ => Err("B08_PLAN_LENGTH_OUT_OF_RANGE".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `count` sentences of ten characters, one a line.
    fn lines_of_ten(count: usize) -> String {
        (0..count)
            .map(|n| format!("第{n:02}句话写在这里。"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn round90_an_exchange_is_cut_whole_and_a_planned_date_is_kept() {
        let body = "“写完要是褪了呢？”\n“那就说明我错了。”\n陈默低头，笔尖落下去。\n他先写2018，横折竖钩一笔一笔压着。\n走廊那头传来推车的声音，越来越近。\n最后一笔收住。";
        let events = planned_events(&json!({"craft":{"scenes":[
            {"people":[{"name":"陈默"}],"events":["陈默在自己手背写下2018.04.12"]}
        ]}}));
        let range = ChapterLengthRange { min: 10, max: 40 };
        let shortened = cut_sentences(body, &[1, 3, 4], range, &events);
        assert!(
            !shortened.contains("那就说明我错了") && !shortened.contains("写完要是褪了呢"),
            "{shortened}"
        );
        assert!(shortened.contains("他先写2018"), "{shortened}");
    }

    /// The number of the sentence of `body` that starts with `text`.
    fn sentence_at(body: &str, text: &str) -> usize {
        crate::chapter_craft::sentence_spans(body)
            .iter()
            .position(|&(start, end)| body[start..end].trim_start().starts_with(text))
            .unwrap()
    }

    #[test]
    fn round100_a_conversation_goes_whole_and_only_when_most_of_it_was_chosen() {
        // Real run round 10: the line before 用量？ went and left it alone,
        // because a narration line split the conversation in two, and a
        // line opening with narration (小吴没过来。“…”) was not counted as
        // spoken at all.
        let body = "第四张锡箔还在他手里。\n“水机在走廊尽头。”沈砚说，“顺便把今晚的用量记一下。”\n小杨脚步停了一下，回头看了他一眼。\n“用量？”\n小吴没过来。“沈哥，这得主任签字。”\n“锡箔片，七片。”\n第四张锡箔落下。";
        let range = ChapterLengthRange { min: 10, max: 30 };
        let first = sentence_at(body, "“水机");
        let shortened = cut_sentences(body, &[first], range, &PlannedEvents::default());
        assert!(shortened.contains("“用量？”"), "{shortened}");
        assert!(shortened.contains("水机在走廊尽头"), "{shortened}");
        let most = ["“水机", "小杨脚步", "“用量", "“锡箔"].map(|text| sentence_at(body, text));
        let shortened = cut_sentences(body, &most, range, &PlannedEvents::default());
        for line in ["水机", "用量", "主任签字", "锡箔片"] {
            assert!(!shortened.contains(line), "{line}: {shortened}");
        }
        assert!(shortened.contains("第四张锡箔落下"), "{shortened}");
    }

    #[test]
    fn round130_the_narration_inside_a_conversation_goes_only_with_it() {
        // Real run round 12: the line where the doctor typed the key went
        // alone and left 屏幕跳出一行提示 with no input before it.
        let body = "诊室里很静。\n“用这六个字。”\n吴医生把六个字符敲进对话框。\n“校准通过了。”\n窗外有车开过。";
        let range = ChapterLengthRange { min: 10, max: 20 };
        let typed = sentence_at(body, "吴医生");
        let shortened = cut_sentences(body, &[typed], range, &PlannedEvents::default());
        assert!(shortened.contains("吴医生把六个字符"), "{shortened}");
        let whole = ["“用这", "吴医生", "“校准"].map(|text| sentence_at(body, text));
        let shortened = cut_sentences(body, &whole, range, &PlannedEvents::default());
        assert!(
            !shortened.contains("吴医生") && !shortened.contains("六个字"),
            "{shortened}"
        );
    }

    #[test]
    fn ld4_a_long_chapter_loses_the_chosen_sentences_in_order_until_it_fits_the_accepted_range() {
        let range = ChapterLengthRange { min: 50, max: 70 };
        let cut = cut_sentences(
            &lines_of_ten(10),
            &[2, 5, 7, 0, 1, 3],
            range,
            &PlannedEvents::default(),
        );
        assert_eq!(
            cut,
            "第00句话写在这里。\n第01句话写在这里。\n第03句话写在这里。\n第04句话写在这里。\n第06句话写在这里。\n第08句话写在这里。\n第09句话写在这里。"
        );
    }

    #[test]
    fn round61_a_cut_stops_before_removing_an_object_introduction_once_short_enough() {
        let body = format!(
            "{}\n一枚青丸滚出来，停在陆照鞋尖前。\n陆照后来把青丸攥进袖口。",
            lines_of_ten(10)
        );
        let length = character_count(&body);
        let max = length * 10 / 11;
        let range = ChapterLengthRange { min: max - 30, max };
        assert!(length > range.ceiling());
        let shortened = cut_sentences(&body, &[0, 1, 10], range, &PlannedEvents::default());
        assert!(range.accepts(character_count(&shortened)));
        assert!(shortened.contains("一枚青丸滚出来"), "{shortened}");
    }

    #[test]
    fn ld4_a_cut_below_the_minimum_of_the_ending_or_of_half_a_line_of_dialogue_is_skipped() {
        let body = "第00句话写在这里。\n“你先走。我留下。”\n第02句话写在这里。";
        let range = ChapterLengthRange { min: 5, max: 10 };
        assert_eq!(
            cut_sentences(body, &[3, 1, 2, 0], range, &PlannedEvents::default()),
            "“你先走。我留下。”\n第02句话写在这里。"
        );
        let range = ChapterLengthRange { min: 100, max: 105 };
        assert_eq!(
            cut_sentences(&lines_of_ten(10), &[0, 1], range, &PlannedEvents::default()),
            lines_of_ten(10)
        );
    }

    #[test]
    fn ld4_a_paragraph_cut_whole_takes_its_blank_line_with_it() {
        let body = "第00句话写在这里。\n\n第01句话写在这里。\n\n第02句话写在这里。";
        let range = ChapterLengthRange { min: 5, max: 10 };
        assert_eq!(
            cut_sentences(body, &[1], range, &PlannedEvents::default()),
            "第00句话写在这里。\n\n第02句话写在这里。"
        );
    }

    #[test]
    fn a_cut_keeps_the_scene_opener_that_identifies_the_next_sentences_actor() {
        let body = "山脚豆腐坊的檐下挂着竹帘，陈婆正把一板豆干从屉里起出来，热气腾腾。她看见陆照，没说话。\n\n檐下的水慢慢滴了下来。\n\n陆照接过碟子。";
        let range = ChapterLengthRange { min: 20, max: 54 };
        let shortened = cut_sentences(body, &[0, 2], range, &PlannedEvents::default());
        assert!(shortened.starts_with("山脚豆腐坊"), "{shortened}");
        assert!(!shortened.contains("水慢慢滴"), "{shortened}");
    }

    /// Real run 2026-11-25: with dialogue protected, only actions could be
    /// cut, and the chapter lost the scene its own dialogue speaks of.
    #[test]
    fn a_cut_keeps_the_planned_event_and_may_take_spare_dialogue() {
        let body =
            "“沈哥，早。”\n“早。”\n书签尖端沿纹路向左疾速刮擦。\n“A7掉线了。”\n“那是你刚才刮的。”";
        let range = ChapterLengthRange { min: 5, max: 30 };
        let events = planned_events(&json!({"craft":{"scenes":[
            {"events":["沈砚手腕下压，书签尖端沿纹路向左疾速刮擦读卡槽"]}
        ]}}));
        let shortened = cut_sentences(body, &[2, 0, 1], range, &events);
        assert_eq!(
            shortened,
            "书签尖端沿纹路向左疾速刮擦。\n“A7掉线了。”\n“那是你刚才刮的。”"
        );
    }

    #[test]
    fn a_chapters_length_leaves_out_line_breaks_and_spaces() {
        assert_eq!(character_count("第一句。\n\n　　第二句。 "), 8);
    }

    #[test]
    fn fx18_range_resolution_prefers_chosen_then_saved_chapter_then_work_then_default() {
        let work = json!({"min":3000,"max":4000});
        let saved = json!({"min":2500,"max":3500});
        let range = |min, max| ChapterLengthRange { min, max };
        assert_eq!(
            ChapterLengthRange::from_payload(
                &json!({"chapterLengthRange":{"min":4000,"max":5000}})
            ),
            Ok(Some(range(4000, 5000)))
        );
        assert_eq!(
            ChapterLengthRange::from_payload(&json!({"targetChineseCharacters":1800})),
            Ok(Some(range(1440, 1800)))
        );
        assert_eq!(ChapterLengthRange::from_payload(&json!({})), Ok(None));
        assert_eq!(
            ChapterLengthRange::resolve(Some(range(4000, 5000)), Some(&saved), Some(&work), None),
            Ok(range(4000, 5000))
        );
        assert_eq!(
            ChapterLengthRange::resolve(None, Some(&saved), Some(&work), None),
            Ok(range(2500, 3500))
        );
        assert_eq!(
            ChapterLengthRange::resolve(None, None, Some(&work), None),
            Ok(range(3000, 4000))
        );
        assert_eq!(
            ChapterLengthRange::resolve(None, None, None, None),
            Ok(DEFAULT_CHAPTER_LENGTH_RANGE)
        );
    }

    #[test]
    fn ld3_a_platform_sets_the_range_at_ninety_to_one_hundred_ten_percent_of_its_target() {
        let range = |min, max| ChapterLengthRange { min, max };
        assert_eq!(
            ChapterLengthRange::for_platform("番茄小说"),
            Some(range(1800, 2200))
        );
        assert_eq!(
            ChapterLengthRange::for_platform("QIMAO"),
            Some(range(1800, 2200))
        );
        assert_eq!(
            ChapterLengthRange::for_platform("起点中文网"),
            Some(range(2700, 3300))
        );
        assert_eq!(
            ChapterLengthRange::for_platform("纵横"),
            Some(range(2700, 3300))
        );
        assert_eq!(
            ChapterLengthRange::for_platform("晋江文学城"),
            Some(range(2700, 3300))
        );
        assert_eq!(ChapterLengthRange::for_platform("某个新平台"), None);
        assert_eq!(
            ChapterLengthRange::for_platform("fanqie"),
            Some(range(1800, 2200))
        );
        assert_eq!(ChapterLengthRange::for_platform("番茄或起点"), None);
        // The work's own setting still wins; the platform comes before the default.
        let work = json!({"min":3000,"max":4000});
        assert_eq!(
            ChapterLengthRange::resolve(None, None, Some(&work), Some("番茄小说")),
            Ok(range(3000, 4000))
        );
        assert_eq!(
            ChapterLengthRange::resolve(None, None, None, Some("番茄小说")),
            Ok(range(1800, 2200))
        );
        assert_eq!(
            ChapterLengthRange::resolve(None, None, None, Some("某个新平台")),
            Ok(DEFAULT_CHAPTER_LENGTH_RANGE)
        );
    }

    #[test]
    fn fx18_invalid_ranges_are_rejected() {
        for bad in [
            json!({"min":3000,"max":3000}),
            json!({"min":400,"max":1000}),
            json!({"min":2000,"max":12000}),
            json!({"min":"2000","max":3000}),
            json!({"max":3000}),
        ] {
            assert_eq!(
                ChapterLengthRange::from_value(&bad),
                Err("CHAPTER_LENGTH_RANGE_INVALID".into()),
                "{bad}"
            );
        }
    }

    #[test]
    fn fx18_body_may_exceed_the_maximum_by_ten_percent_only() {
        let range = ChapterLengthRange {
            min: 2000,
            max: 3000,
        };
        assert!(!range.accepts(1999));
        assert!(range.accepts(2000));
        assert!(range.accepts(3300));
        assert!(!range.accepts(3301));
        assert_eq!(character_count("林舟\n推门"), 4);
    }

    #[test]
    fn fx18_plan_must_be_sized_for_the_range() {
        let range = ChapterLengthRange {
            min: 2000,
            max: 3000,
        };
        let plan = |estimates: &[Value]| json!({"sceneExecution":estimates.iter().map(|estimate| json!({"objective":"场景","estimatedCharacters":estimate})).collect::<Vec<_>>()});
        assert!(validate_plan_length(&plan(&[json!(800), json!(900), json!(700)]), range).is_ok());
        assert_eq!(
            validate_plan_length(&plan(&[json!(250), json!(250)]), range),
            Err("B08_PLAN_LENGTH_OUT_OF_RANGE".into())
        );
        assert_eq!(
            validate_plan_length(&plan(&[json!(2000), json!(1500)]), range),
            Err("B08_PLAN_LENGTH_OUT_OF_RANGE".into())
        );
        assert_eq!(
            validate_plan_length(&json!({"sceneExecution":[{"objective":"无估计"}]}), range),
            Err("B08_PLAN_LENGTH_OUT_OF_RANGE".into())
        );
        assert_eq!(
            validate_plan_length(&json!({"sceneExecution":[]}), range),
            Err("B08_PLAN_LENGTH_OUT_OF_RANGE".into())
        );
    }
}
