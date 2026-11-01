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
    let mut seen = BTreeSet::new();
    let mut ranked = newest_first
        .into_iter()
        .filter(|memory| {
            seen.insert((
                memory.subject.as_str(),
                memory.kind.as_str(),
                memory.detail.as_str(),
            ))
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
