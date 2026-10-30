//! LD-1: the output spec of each model step (`contracts/model-output/`).
//!
//! One spec per step tells the model the shape of its reply (it is appended
//! to the system prompt) and checks the reply; a reply off the spec is sent
//! back once with what was wrong.

use serde_json::Value;
use std::sync::OnceLock;

/// Error prefix of a reply that does not match its step's spec.
pub const OFF_SPEC: &str = "P01_MODEL_OUTPUT_SPEC";

/// How many spec violations are reported back to the model.
const REPORTED_VIOLATIONS: usize = 5;

/// Each spec and the model steps it covers.
const SPECS: &[(&[&str], &str)] = &[
    (
        &["RENDER_CHAPTER_CRAFT", "ADJUST_CHAPTER_LENGTH", "RENDER_CHAPTER"],
        include_str!("../../contracts/model-output/chapter-body.schema.json"),
    ),
    (
        &["REWRITE_PASSAGES"],
        include_str!("../../contracts/model-output/rewrite-passages.schema.json"),
    ),
    (
        &["AUDIT_EXTRACT"],
        include_str!("../../contracts/model-output/audit-extract.schema.json"),
    ),
    (
        &["CHECK_CONTINUITY"],
        include_str!("../../contracts/model-output/check-continuity.schema.json"),
    ),
    (
        &["GENERATE_STORY_BIBLE"],
        include_str!("../../contracts/model-output/story-bible.schema.json"),
    ),
    (
        &["GENERATE_STORY_DIRECTIONS"],
        include_str!("../../contracts/model-output/story-directions.schema.json"),
    ),
];

/// A spec and its compiled validator.
struct Spec {
    schema: Value,
    validator: jsonschema::Validator,
}

fn spec(capability: &str) -> Option<&'static Spec> {
    static COMPILED: OnceLock<Vec<Spec>> = OnceLock::new();
    let compiled = COMPILED.get_or_init(|| {
        SPECS
            .iter()
            .map(|(_, source)| {
                let schema: Value =
                    serde_json::from_str(source).expect("model output spec is JSON");
                let validator =
                    jsonschema::draft202012::new(&schema).expect("model output spec is a schema");
                Spec { schema, validator }
            })
            .collect()
    });
    SPECS
        .iter()
        .position(|(capabilities, _)| capabilities.contains(&capability))
        .map(|index| &compiled[index])
}

/// `Err(P01_MODEL_OUTPUT_SPEC:…)` listing where `payload` is off the spec.
pub fn check(capability: &str, payload: &Value) -> Result<(), String> {
    let Some(spec) = spec(capability) else {
        return Ok(());
    };
    let violations = spec
        .validator
        .iter_errors(payload)
        .take(REPORTED_VIOLATIONS)
        .map(|error| {
            let path = error.instance_path().to_string();
            format!("{} {error}", if path.is_empty() { "/" } else { &path })
        })
        .collect::<Vec<_>>();
    if violations.is_empty() {
        Ok(())
    } else {
        Err(format!("{OFF_SPEC}:{}", violations.join("; ")))
    }
}

/// The part of the system prompt that states the spec, if the step has one.
pub fn prompt_section(capability: &str) -> Option<String> {
    spec(capability).map(|spec| {
        format!(
            "The reply must be valid against this JSON Schema: {}",
            spec.schema
        )
    })
}

#[cfg(test)]
mod tests {
    /// Model steps without an output spec, and why their replies cannot
    /// fail a run unnoticed.
    const UNSPECIFIED_STEPS: [(&str, &str); 7] = [
        ("PLAN_STRATEGY", "r1 plans: the plan contract sends a reply back to the planner"),
        ("PLAN_CHAPTER_CRAFT", "the plan checks and contract send a reply back to the planner"),
        ("REPAIR_CANDIDATE", "retired from production (owner decision 2026-10-24)"),
        ("CLASSIFY_ASSISTANT_INTENT", "a failed reply falls back to keyword routing"),
        ("OPENING_SIGNING_EVALUATION", "checked against the S35 skill's own outputSchema"),
        ("REVIEW_CHAPTER", "advisory: a failed review is recorded, the chapter goes on"),
        ("TEST_PROVIDER", "a connection test, never part of production"),
    ];

    #[test]
    fn every_model_step_has_an_output_spec_or_a_reason_for_none() {
        // QC-1 (2026-10-30, sixth acceptance run): a step without a spec
        // failed a whole run on one missing field.
        let source = include_str!("provider.rs");
        let steps = source
            .lines()
            .filter_map(|line| {
                let rest = line.trim_start().strip_prefix('"')?;
                let (step, after) = rest.split_once('"')?;
                let prompt = after.trim_start().strip_prefix("=>")?.trim_start();
                (step.chars().all(|c| c.is_ascii_uppercase() || c == '_')
                    && (prompt.starts_with("\"Return JSON") || prompt.starts_with("concat!(")))
                .then_some(step)
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert!(steps.len() >= 15, "{steps:?}");
        for step in steps {
            assert!(
                spec(step).is_some() || UNSPECIFIED_STEPS.iter().any(|(name, _)| *name == step),
                "{step} has no output spec and no reason for none"
            );
        }
    }

    #[test]
    fn the_story_bible_spec_refuses_the_quote_marks_the_setup_refuses() {
        let pattern = spec("GENERATE_STORY_BIBLE").unwrap().schema["properties"]["bibles"]
            ["items"]["properties"]["protagonist"]["properties"]["innerVoiceStyle"]["pattern"]
            .as_str()
            .unwrap()
            .to_owned();
        for mark in crate::craft_setup::QUOTE_MARKS {
            assert!(pattern.contains(*mark), "{mark}");
        }
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn a_reply_without_a_required_field_names_the_field() {
        let error = check("RENDER_CHAPTER_CRAFT", &json!({"text":"正文"})).unwrap_err();
        assert!(
            error.starts_with(OFF_SPEC) && error.contains("body"),
            "{error}"
        );
        assert!(check("RENDER_CHAPTER_CRAFT", &json!({"body":"正文"})).is_ok());
    }

    #[test]
    fn a_step_without_a_spec_is_not_checked() {
        assert!(check("TEST_PROVIDER", &json!(1)).is_ok());
        assert!(prompt_section("TEST_PROVIDER").is_none());
    }

    #[test]
    fn every_spec_compiles() {
        for (capabilities, _) in SPECS {
            assert!(spec(capabilities[0]).is_some());
        }
    }
}
