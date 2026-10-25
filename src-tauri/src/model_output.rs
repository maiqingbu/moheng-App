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
        &["RENDER_CHAPTER_CRAFT", "ADJUST_CHAPTER_LENGTH"],
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
