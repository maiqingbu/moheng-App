//! CRAFT v3 step 2a: chapter-plan.r2 is accepted wherever a chapter plan is
//! normalized and validated, and r1 plans stay readable.

use ai_novel_production_system_core::planning::{
    normalize_chapter_plan_model_output, validate_chapter_plan,
};
use serde_json::{json, Value};

fn fixture(kind: &str, name: &str) -> Value {
    let path = format!(
        "{}/../contracts/fixtures/{kind}/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn bindings() -> Value {
    json!({"chapterId":"chapter:1","compiledFromProjectRevision":0,
        "compiledFromStateDigest":"a".repeat(64),
        "contextManifestRef":{"artifactId":"context:1","version":1,"payloadHash":"a".repeat(64)}})
}

#[test]
fn model_output_with_craft_becomes_an_r2_plan_without_scene_execution() {
    let mut raw = fixture("valid", "narrative-chapter-plan-r2");
    raw["sceneExecution"] = json!([{"sceneType":"调查"}]);
    raw["payloadSchema"] = json!("chapter-plan.r1");
    let plan = normalize_chapter_plan_model_output(raw, &[], &bindings());
    assert_eq!(plan["payloadSchema"], "chapter-plan.r2");
    assert!(plan.get("sceneExecution").is_none());
    assert_eq!(plan["craft"]["bigTurn"], "货单是彭三的人调包的");
    validate_chapter_plan(&plan).unwrap();
}

#[test]
fn model_output_without_craft_stays_an_r1_plan() {
    let raw = fixture("valid", "narrative-chapter-plan-r1");
    let plan = normalize_chapter_plan_model_output(raw, &[], &bindings());
    assert_eq!(plan["payloadSchema"], "chapter-plan.r1");
    validate_chapter_plan(&plan).unwrap();
}

#[test]
fn validate_chapter_plan_dispatches_on_the_payload_schema() {
    validate_chapter_plan(&fixture("valid", "narrative-chapter-plan-r2")).unwrap();
    validate_chapter_plan(&fixture("valid", "narrative-chapter-plan-r1")).unwrap();
    let error = validate_chapter_plan(&fixture("invalid", "narrative-chapter-plan-r2-invalid"))
        .unwrap_err();
    assert!(error.starts_with("CHAPTER_PLAN_CONTRACT:"), "{error}");
    let mut unknown = fixture("valid", "narrative-chapter-plan-r1");
    unknown["payloadSchema"] = json!("chapter-plan.r9");
    assert!(validate_chapter_plan(&unknown).is_err());
}
