//! Owner decision 2026-11-27 (H00 harness: HFR-002/003/004/007, NFR-03): the
//! project assistant plans its own queries over the registered read
//! capabilities, may query once more when a result comes back empty, and
//! answers only from what those queries returned, each sentence naming the
//! records it rests on. The model never writes SQL or calls anything outside
//! the planner menu; the system checks every step and every citation.

use crate::harness::SupportStatus;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const PLAN_ASSISTANT_QUERY: &str = "PLAN_ASSISTANT_QUERY";
pub const COMPOSE_ASSISTANT_ANSWER: &str = "COMPOSE_ASSISTANT_ANSWER";

/// The most steps the first plan may hold.
pub const MAX_PLANNED_STEPS: usize = 5;
/// The most steps one follow-up round may add.
pub const MAX_FOLLOW_UP_STEPS: usize = 2;
/// How much of one result the composer reads.
pub const RESULT_PROMPT_CHARS: usize = 2_500;

/// One capability the planner may choose: what it answers, the inputs the
/// repository adapter reads for it, and the ones it cannot run without.
pub struct MenuItem {
    pub capability_id: &'static str,
    pub answers: &'static str,
    pub inputs: &'static [&'static str],
    pub required: &'static [&'static str],
}

const fn item(
    capability_id: &'static str,
    answers: &'static str,
    inputs: &'static [&'static str],
    required: &'static [&'static str],
) -> MenuItem {
    MenuItem {
        capability_id,
        answers,
        inputs,
        required,
    }
}

/// The story-facing part of the read-only registry, with each item's inputs
/// as `RepositoryReadAdapter` reads them.
pub const PLANNER_MENU: &[MenuItem] = &[
    item(
        "query_canon",
        "established facts about the named characters, places or items",
        &["entityIds"],
        &[],
    ),
    item(
        "query_relations",
        "current relationships between the named characters",
        &["entityIds"],
        &[],
    ),
    item(
        "query_knowledge",
        "what the named characters know, believe or do not know",
        &["entityIds"],
        &["entityIds"],
    ),
    item(
        "query_events",
        "the events written in one chapter",
        &["chapterId"],
        &["chapterId"],
    ),
    item(
        "query_evidence",
        "the prose passages behind the given record ids",
        &["references"],
        &["references"],
    ),
    item(
        "query_obligations",
        "open plot obligations (promises the story still has to keep)",
        &["entityIds", "references"],
        &[],
    ),
    item(
        "query_foreshadows",
        "foreshadowing planted and not yet paid off",
        &["chapterId", "references"],
        &[],
    ),
    item("query_mysteries", "open mysteries", &["references"], &[]),
    item(
        "query_task_states",
        "progress of the volume's planned tasks",
        &["volumeId"],
        &[],
    ),
    item(
        "query_volume_progress",
        "how far one volume has come",
        &["volumeId"],
        &["volumeId"],
    ),
    item(
        "query_plan_route",
        "the volume route and its stages",
        &["volumeId"],
        &["volumeId"],
    ),
    item(
        "diagnose_planning_constraints",
        "why the volume plan constrains what comes next",
        &["volumeId"],
        &["volumeId"],
    ),
    item(
        "diagnose_knowledge_event",
        "whether what a character knows is backed by a written event",
        &["entityIds", "references"],
        &["entityIds"],
    ),
    item(
        "diagnose_canon_evidence",
        "whether the given facts are backed by prose",
        &["references"],
        &["references"],
    ),
    item(
        "query_project_meta",
        "the work's goals and contracts",
        &[],
        &[],
    ),
];

/// The ids the planner may name: the work's entities, chapters and volumes.
#[derive(Debug, Clone, Default)]
pub struct Directory {
    pub entities: Vec<(String, String, String)>,
    pub chapters: Vec<(String, i64, String, String)>,
}

impl Directory {
    pub fn prompt_view(&self) -> Value {
        json!({
            "entities": self.entities.iter().map(|(id, name, kind)| json!({"id":id,"name":name,"type":kind})).collect::<Vec<_>>(),
            "chapters": self.chapters.iter().map(|(id, number, title, volume)| json!({"id":id,"number":number,"title":title,"volumeId":volume})).collect::<Vec<_>>(),
        })
    }

    /// The part the planner is shown: entities named in the question first,
    /// then the newest, at most `entities` of them, and the latest `chapters`.
    pub fn for_question(&self, question: &str, entities: usize, chapters: usize) -> Directory {
        let (named, rest): (Vec<_>, Vec<_>) = self
            .entities
            .iter()
            .cloned()
            .partition(|(_, name, _)| !name.is_empty() && question.contains(name.as_str()));
        Directory {
            entities: named.into_iter().chain(rest).take(entities).collect(),
            chapters: self.chapters.iter().take(chapters).cloned().collect(),
        }
    }

    fn knows(&self, key: &str, id: &str) -> bool {
        match key {
            "entityIds" => self.entities.iter().any(|(known, ..)| known == id),
            "chapterId" => self.chapters.iter().any(|(known, ..)| known == id),
            "volumeId" => self.chapters.iter().any(|(.., volume)| volume == id),
            // Record ids come from earlier results; the repository bounds them.
            _ => true,
        }
    }
}

/// The planner menu as the model sees it.
pub fn menu_prompt_view() -> Vec<Value> {
    PLANNER_MENU
        .iter()
        .map(|item| json!({"capabilityId":item.capability_id,"answers":item.answers,"inputs":item.inputs,"required":item.required}))
        .collect()
}

/// One checked step of a plan the model proposed.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedCall {
    pub capability_id: String,
    pub input: Value,
}

/// The steps of a planner reply `{"steps":[{"capabilityId","input"}]}` the
/// system accepts: on the menu, at most `limit`, inputs only of the keys the
/// capability takes and ids the directory knows, each step once. Err when
/// nothing usable is left.
pub fn checked_plan(
    reply: &Value,
    directory: &Directory,
    limit: usize,
) -> Result<Vec<PlannedCall>, String> {
    let mut seen = BTreeSet::new();
    let mut calls = Vec::new();
    for step in reply["steps"].as_array().into_iter().flatten() {
        let Some(id) = step["capabilityId"].as_str() else {
            continue;
        };
        let Some(item) = PLANNER_MENU.iter().find(|item| item.capability_id == id) else {
            continue;
        };
        let mut input = serde_json::Map::new();
        for key in item.inputs {
            let value = &step["input"][*key];
            let accepted = match value {
                Value::String(single) if directory.knows(key, single) => Some(json!(single)),
                Value::Array(items) => {
                    let ids = items
                        .iter()
                        .filter_map(Value::as_str)
                        .filter(|item| directory.knows(key, item))
                        .take(20)
                        .collect::<Vec<_>>();
                    (!ids.is_empty()).then(|| json!(ids))
                }
                _ => None,
            };
            if let Some(accepted) = accepted {
                input.insert((*key).into(), accepted);
            }
        }
        // A capability left without an id it cannot run without is dropped.
        if item.required.iter().any(|key| !input.contains_key(*key)) {
            continue;
        }
        let input = Value::Object(input);
        if seen.insert((id.to_string(), input.to_string())) {
            calls.push(PlannedCall {
                capability_id: id.into(),
                input,
            });
        }
        if calls.len() == limit {
            break;
        }
    }
    if calls.is_empty() {
        Err("ASSISTANT_PLAN_EMPTY".into())
    } else {
        Ok(calls)
    }
}

/// One sentence of the answer and the records it rests on.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AnswerSentence {
    pub text: String,
    pub sources: Vec<String>,
    pub inferred: bool,
}

/// The sentences of a composer reply `{"sentences":[{"text","sources","inferred"}]}`
/// that cite at least one record the queries returned, with only those
/// citations, and the support the whole answer has: none kept is
/// INSUFFICIENT, a conflict a diagnosis reported is CONFLICTING, any
/// inference is INFERRED, else SUPPORTED.
pub fn grounded_answer(
    reply: &Value,
    available: &BTreeSet<String>,
    conflicting: bool,
) -> (Vec<AnswerSentence>, SupportStatus) {
    let sentences = reply["sentences"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|sentence| {
            let text = sentence["text"].as_str()?.trim();
            let sources = sentence["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|source| available.contains(*source))
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            (!text.is_empty() && !sources.is_empty()).then(|| AnswerSentence {
                text: text.into(),
                sources,
                inferred: sentence["inferred"].as_bool().unwrap_or(false),
            })
        })
        .collect::<Vec<_>>();
    let status = if sentences.is_empty() {
        SupportStatus::INSUFFICIENT
    } else if conflicting {
        SupportStatus::CONFLICTING
    } else if sentences.iter().any(|sentence| sentence.inferred) {
        SupportStatus::INFERRED
    } else {
        SupportStatus::SUPPORTED
    };
    (sentences, status)
}

/// A result cut to what the composer reads, said to be cut when it is.
pub fn result_prompt_view(
    call_id: &str,
    capability_id: &str,
    result: &Value,
    source_refs: &[String],
    truncated: bool,
) -> Value {
    let text = result.to_string();
    let cut = text.chars().count() > RESULT_PROMPT_CHARS;
    let shown = if cut {
        text.chars().take(RESULT_PROMPT_CHARS).collect::<String>()
    } else {
        text
    };
    json!({
        "callId":call_id,"capabilityId":capability_id,"sourceRefs":source_refs,
        "result":shown,"partial":cut || truncated
    })
}

/// Whether a result holds nothing: the trigger for one follow-up round.
pub fn is_empty_result(result: &Value) -> bool {
    match result {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => {
            map.is_empty() || map.get("status").and_then(Value::as_str) == Some("INSUFFICIENT_DATA")
        }
        _ => false,
    }
}

/// The steps the configured model plans for `question` over the planner
/// menu, checked; `earlier` holds the results of a first round when this
/// is the follow-up round.
pub fn plan_with_model(
    state: &crate::commands::KernelState,
    project_id: &str,
    question: &str,
    directory: &Directory,
    earlier: Option<&[Value]>,
    now: &str,
) -> Result<Vec<PlannedCall>, String> {
    let gateway: &dyn crate::production::ProductionModelGateway = state.production_gateway.as_ref();
    let (round, limit) = match earlier {
        Some(_) => ("follow-up", MAX_FOLLOW_UP_STEPS),
        None => ("first", MAX_PLANNED_STEPS),
    };
    let digest = crate::hash::sha256_canonical(&json!({"question":question,"round":round}))
        .map_err(|error| error.to_string())?;
    let mut input = json!({
        "question":question,
        "questionTrustBoundary":crate::assistant_intent::QUESTION_TRUST_BOUNDARY,
        "capabilities":menu_prompt_view(),
        "directory":directory.prompt_view(),
        "maxSteps":limit,
    });
    if let Some(earlier) = earlier {
        input["earlierResults"] = json!(earlier);
    }
    let (reply, _) = crate::assistant_intent::run_assistant_model(
        state,
        gateway,
        project_id,
        PLAN_ASSISTANT_QUERY,
        format!("assistant-plan:{round}:{}:{now}", &digest[..16]),
        input,
        now,
    )?;
    checked_plan(&reply, directory, limit)
}

/// The configured model's answer to `question` from `results` (each from
/// `result_prompt_view`), as its raw reply.
pub fn compose_with_model(
    state: &crate::commands::KernelState,
    project_id: &str,
    question: &str,
    results: &[Value],
    now: &str,
) -> Result<Value, String> {
    let gateway: &dyn crate::production::ProductionModelGateway = state.production_gateway.as_ref();
    let digest = crate::hash::sha256_canonical(&json!({"question":question,"results":results}))
        .map_err(|error| error.to_string())?;
    let (reply, _) = crate::assistant_intent::run_assistant_model(
        state,
        gateway,
        project_id,
        COMPOSE_ASSISTANT_ANSWER,
        format!("assistant-answer:{}:{now}", &digest[..16]),
        json!({
            "question":question,
            "questionTrustBoundary":crate::assistant_intent::QUESTION_TRUST_BOUNDARY,
            "results":results,
        }),
        now,
    )?;
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> Directory {
        Directory {
            entities: vec![
                ("entity:liu".into(), "柳青堂".into(), "CHARACTER".into()),
                ("entity:shen".into(), "沈无悔".into(), "CHARACTER".into()),
            ],
            chapters: vec![("chapter:3".into(), 3, "半枚玉符".into(), "volume:1".into())],
        }
    }

    #[test]
    fn round130_a_plan_keeps_only_menu_steps_with_known_ids() {
        let reply = json!({"steps":[
            {"capabilityId":"query_knowledge","input":{"entityIds":["entity:liu","entity:ghost"]}},
            {"capabilityId":"drop_tables","input":{}},
            {"capabilityId":"query_events","input":{"chapterId":"chapter:99"}},
            {"capabilityId":"query_foreshadows","input":{"sql":"SELECT *"}},
            {"capabilityId":"query_knowledge","input":{"entityIds":["entity:liu"]}}
        ]});
        let plan = checked_plan(&reply, &directory(), MAX_PLANNED_STEPS).unwrap();
        assert_eq!(
            plan,
            [
                PlannedCall {
                    capability_id: "query_knowledge".into(),
                    input: json!({"entityIds":["entity:liu"]})
                },
                PlannedCall {
                    capability_id: "query_foreshadows".into(),
                    input: json!({})
                },
            ]
        );
        assert!(checked_plan(
            &json!({"steps":[{"capabilityId":"drop_tables"}]}),
            &directory(),
            5
        )
        .is_err());
        let many = json!({"steps":(0..9).map(|_| json!({"capabilityId":"query_mysteries"})).chain([json!({"capabilityId":"query_foreshadows"})]).collect::<Vec<_>>()});
        assert_eq!(checked_plan(&many, &directory(), 1).unwrap().len(), 1);
    }

    #[test]
    fn round130_an_answer_keeps_only_sentences_with_returned_records() {
        let available = BTreeSet::from(["event:7".to_string(), "fact:2".to_string()]);
        let reply = json!({"sentences":[
            {"text":"柳青堂还不知道沈无悔的真名。","sources":["fact:2","fact:404"]},
            {"text":"他可能已经起疑。","sources":["event:7"],"inferred":true},
            {"text":"他明天就会揭穿。","sources":[]},
            {"text":"他是卧底。","sources":["fact:404"]}
        ]});
        let (sentences, status) = grounded_answer(&reply, &available, false);
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0].sources, ["fact:2"]);
        assert_eq!(status, SupportStatus::INFERRED);
        let (_, status) = grounded_answer(
            &json!({"sentences":[{"text":"x","sources":["fact:2"]}]}),
            &available,
            false,
        );
        assert_eq!(status, SupportStatus::SUPPORTED);
        let (_, status) = grounded_answer(
            &json!({"sentences":[{"text":"x","sources":["fact:2"]}]}),
            &available,
            true,
        );
        assert_eq!(status, SupportStatus::CONFLICTING);
        let (none, status) = grounded_answer(&json!({"sentences":[]}), &available, false);
        assert!(none.is_empty());
        assert_eq!(status, SupportStatus::INSUFFICIENT);
    }

    #[test]
    fn round130_every_menu_item_is_a_registered_read_capability() {
        let registry = crate::harness::CapabilityRegistry::read_only_r1();
        for item in PLANNER_MENU {
            assert!(
                registry.specs.contains_key(item.capability_id),
                "{}",
                item.capability_id
            );
            assert!(
                item.required.iter().all(|key| item.inputs.contains(key)),
                "{}",
                item.capability_id
            );
        }
        let named = directory().for_question("柳青堂知道吗", 1, 0);
        assert_eq!(named.entities[0].1, "柳青堂");
        assert!(named.chapters.is_empty());
    }

    #[test]
    fn round130_a_long_result_is_cut_and_said_to_be_partial() {
        let long = json!("字".repeat(RESULT_PROMPT_CHARS + 50));
        let view = result_prompt_view("call:1", "query_canon", &long, &[], false);
        assert_eq!(view["partial"], true);
        assert!(is_empty_result(&json!([])));
        assert!(is_empty_result(&json!({"status":"INSUFFICIENT_DATA"})));
        assert!(!is_empty_result(&json!([{"id":1}])));
    }
}
