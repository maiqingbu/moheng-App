//! WM-1 phase 1 (owner decision 2026-11-01, WORK_MEMORY_DESIGN §3): each
//! work's long-term memory of the details its chapters fixed (habits, looks,
//! preferences, objects, places, relations), kept in the work's own
//! database and recalled by what a chapter plans rather than only by how
//! recent it is. State facts (where someone is, what they hold) stay in the
//! canon ledger, which already keeps only the current value of each. The
//! memory's table is kept by `repositories::work_memory`.

use serde_json::{json, Value};
use std::collections::BTreeSet;

/// One detail an earlier chapter fixed, as its production receipt states it.
#[derive(Debug, Clone, PartialEq)]
pub struct Memory {
    pub chapter: i64,
    pub subject: String,
    pub kind: String,
    pub detail: String,
}

/// WM-2a: one of the owner's corrections. `subject` is the person or thing
/// it holds for; none for a writing rule of the whole work.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Correction {
    pub correction_id: String,
    pub subject: Option<String>,
    pub content: String,
    pub source_decision_id: Option<String>,
    pub created_at: String,
}

impl Correction {
    /// What the planner and the writer are told of it.
    pub fn prompt_view(&self) -> Value {
        json!({"subject":self.subject,"content":self.content})
    }
}

/// What a chapter is about: who is in it, everyone the story names as a
/// person, and the plan's text.
pub struct RecallQuery<'a> {
    pub on_stage: &'a [String],
    pub people: &'a [String],
    pub text: &'a str,
}

/// Kinds that are a person's own (how they act, look and like things):
/// never recalled for a person absent from the chapter, so they cannot be
/// written onto someone else (§4, the worst error).
const PERSONAL_KINDS: [&str; 4] = ["HABIT", "APPEARANCE", "PREFERENCE", "RELATION"];
/// How many telling character pairs a detail shares with the plan to count
/// as what the chapter brings back.
const RELEVANT_PAIRS: usize = 2;

/// How many telling pairs (beside the person's name) two looks share to be
/// the same look recorded twice.
const SAME_LOOK_PAIRS: usize = 3;

/// Kinds a person shows on the page: how they look and what they do.
const SHOWN_KINDS: [&str; 2] = ["APPEARANCE", "HABIT"];

/// A look or habit with its telling pairs beside the person's name.
type Look<'a> = (&'a Memory, BTreeSet<(char, char)>);

/// `memory` as a look, when it is a look or a habit.
fn look(memory: &Memory) -> Option<Look<'_>> {
    SHOWN_KINDS.contains(&memory.kind.as_str()).then(|| {
        let pairs = &crate::chapter_craft::telling_pairs(&memory.detail)
            - &crate::chapter_craft::telling_pairs(&memory.subject);
        (memory, pairs)
    })
}

/// Whether two looks are the same look or habit of one person recorded
/// twice, however worded.
fn same_look((one, one_pairs): &Look<'_>, (other, other_pairs): &Look<'_>) -> bool {
    one.subject == other.subject && one_pairs.intersection(other_pairs).count() >= SAME_LOOK_PAIRS
}

/// Round 10 (real run round 10): looks and habits both the newest chapter
/// and the one before it recorded (a red mark spreading, a sleeve sliding
/// down), at their newest. The chapters keep writing them, so they are not
/// recalled as facts again; the writer is told to stop dwelling on them.
fn worn_memories(memories: &[Memory]) -> Vec<Look<'_>> {
    let Some(newest) = memories.iter().map(|memory| memory.chapter).max() else {
        return Vec::new();
    };
    let before = memories
        .iter()
        .filter(|memory| memory.chapter == newest - 1)
        .filter_map(look)
        .collect::<Vec<_>>();
    memories
        .iter()
        .filter(|memory| memory.chapter == newest)
        .filter_map(look)
        .filter(|look| before.iter().any(|earlier| same_look(look, earlier)))
        .collect()
}

/// The worn looks and habits as `subject：detail`, for the writer's
/// details to stop dwelling on.
pub fn worn(memories: &[Memory]) -> Vec<String> {
    worn_memories(memories)
        .into_iter()
        .map(|(memory, _)| format!("{}：{}", memory.subject, memory.detail))
        .collect()
}

/// At most `limit` of `memories` for the chapter `query` describes, as
/// `{chapter, subject, kind, detail}`, each detail once at its newest:
/// first what the people of the chapter (on stage or named in the plan)
/// fixed, then the things and places the plan brings back, then the newest
/// of the rest. An absent person's own traits never come in.
pub fn recall(memories: &[Memory], query: &RecallQuery<'_>, limit: usize) -> Vec<Value> {
    let plan_pairs = crate::chapter_craft::telling_pairs(query.text);
    let in_chapter = |subject: &str| {
        query
            .on_stage
            .iter()
            .any(|name| subject == name || subject.contains(name.as_str()))
            || (!subject.is_empty() && query.text.contains(subject))
    };
    let a_person = |subject: &str| {
        query.people.iter().any(|name| {
            !name.is_empty() && (subject.contains(name.as_str()) || name.contains(subject))
        })
    };
    let mut newest_first = memories.iter().collect::<Vec<_>>();
    newest_first.sort_by_key(|memory| std::cmp::Reverse(memory.chapter));
    // Real run round 9: a look recorded again each chapter (a ribbon on a
    // finger, bleeding) was recalled as many facts and written every chapter;
    // a look like a newer one of the same person is left out, and a worn one
    // is left out altogether.
    let worn = worn_memories(memories);
    let mut seen = BTreeSet::new();
    let mut looks: Vec<Look<'_>> = Vec::new();
    let mut ranked = newest_first
        .into_iter()
        .filter(|memory| {
            seen.insert((
                memory.subject.as_str(),
                memory.kind.as_str(),
                memory.detail.as_str(),
            ))
        })
        .filter(|memory| {
            let Some(look) = look(memory) else {
                return true;
            };
            let known = worn
                .iter()
                .chain(&looks)
                .any(|other| same_look(other, &look));
            if !known {
                looks.push(look);
            }
            !known
        })
        .filter_map(|memory| {
            let tier = if in_chapter(&memory.subject) {
                0
            } else if a_person(&memory.subject) && PERSONAL_KINDS.contains(&memory.kind.as_str()) {
                return None;
            } else if crate::chapter_craft::telling_pairs(&format!(
                "{}{}",
                memory.subject, memory.detail
            ))
            .intersection(&plan_pairs)
            .count()
                >= RELEVANT_PAIRS
            {
                1
            } else {
                2
            };
            Some((tier, memory))
        })
        .collect::<Vec<_>>();
    // Stable, so each tier stays newest first.
    ranked.sort_by_key(|(tier, _)| *tier);
    ranked
        .into_iter()
        .take(limit)
        .map(|(_, memory)| {
            json!({"chapter":memory.chapter,"subject":memory.subject,"kind":memory.kind,"detail":memory.detail})
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory(chapter: i64, kind: &str, detail: &str) -> Memory {
        Memory {
            chapter,
            subject: "沈砚".into(),
            kind: kind.into(),
            detail: detail.into(),
        }
    }

    #[test]
    fn round90_a_state_recorded_every_chapter_is_recalled_once_at_its_newest() {
        let memories = [
            memory(
                4,
                "APPEARANCE",
                "左小指缠着七圈蓝丝带，根部有一道细而深的新痕",
            ),
            memory(5, "APPEARANCE", "左手小指缠着蓝丝带，旧痕被辣油染成暗红色"),
            memory(7, "APPEARANCE", "左手小指缠着蓝丝带，末端洇出暗红血迹"),
            memory(1, "APPEARANCE", "戴眼镜，镜片会被蒸汽糊住"),
            memory(1, "HABIT", "习惯把帆布包挂在椅背"),
            memory(1, "HABIT", "习惯把铅笔别在耳后"),
        ];
        let on_stage = ["沈砚".to_string()];
        let query = RecallQuery {
            on_stage: &on_stage,
            people: &on_stage,
            text: "",
        };
        let recalled = recall(&memories, &query, 40)
            .into_iter()
            .map(|item| item["detail"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            recalled,
            [
                "左手小指缠着蓝丝带，末端洇出暗红血迹",
                "戴眼镜，镜片会被蒸汽糊住",
                "习惯把帆布包挂在椅背",
                "习惯把铅笔别在耳后"
            ]
        );
    }

    #[test]
    fn round100_a_trait_written_in_both_latest_chapters_is_worn_not_recalled() {
        // Real run round 10: the red marks and the sleeve sliding down were
        // recorded every chapter, recalled every chapter, and written five
        // times in a chapter.
        let memories = [
            memory(7, "APPEARANCE", "左手腕内侧有第五道红痕，颜色比前四道深"),
            memory(8, "APPEARANCE", "左手腕有第六道红痕，蔓延至肘弯上方"),
            memory(7, "APPEARANCE", "袖口会自然下滑露出手腕"),
            memory(8, "HABIT", "袖口常滑下去露出手腕"),
            memory(8, "HABIT", "习惯把手机扣在桌上"),
            memory(1, "APPEARANCE", "戴眼镜，镜片会被蒸汽糊住"),
        ];
        let on_stage = ["沈砚".to_string()];
        let query = RecallQuery {
            on_stage: &on_stage,
            people: &on_stage,
            text: "",
        };
        let recalled = recall(&memories, &query, 40)
            .into_iter()
            .map(|item| item["detail"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(recalled, ["习惯把手机扣在桌上", "戴眼镜，镜片会被蒸汽糊住"]);
        assert_eq!(
            worn(&memories),
            [
                "沈砚：左手腕有第六道红痕，蔓延至肘弯上方",
                "沈砚：袖口常滑下去露出手腕"
            ]
        );
    }
}
