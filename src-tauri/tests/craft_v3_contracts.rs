//! CRAFT v3 step 1: the story bible, style card and chapter-plan r2 contracts.

use ai_novel_production_system_core::generated::types::ArtifactType;
use ai_novel_production_system_core::generated::validators::{
    validate_chapter_plan_r1, validate_chapter_plan_r2, validate_story_bible_r1,
    validate_style_card_r1,
};
use serde_json::Value;

fn fixture(kind: &str, name: &str) -> Value {
    let path = format!(
        "{}/../contracts/fixtures/{kind}/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn story_bible_r1_accepts_three_chapter_arc_and_rejects_short_arc() {
    validate_story_bible_r1(&fixture("valid", "narrative-story-bible-r1")).unwrap();
    assert!(
        validate_story_bible_r1(&fixture("invalid", "narrative-story-bible-r1-invalid")).is_err()
    );
}

#[test]
fn style_card_r1_accepts_archetype_card_and_rejects_too_few_rules() {
    validate_style_card_r1(&fixture("valid", "narrative-style-card-r1")).unwrap();
    assert!(
        validate_style_card_r1(&fixture("invalid", "narrative-style-card-r1-invalid")).is_err()
    );
}

#[test]
fn chapter_plan_r2_requires_craft_with_two_scenes_and_stays_distinct_from_r1() {
    let plan = fixture("valid", "narrative-chapter-plan-r2");
    validate_chapter_plan_r2(&plan).unwrap();
    assert!(validate_chapter_plan_r1(&plan).is_err());
    assert!(validate_chapter_plan_r2(&fixture("valid", "narrative-chapter-plan-r1")).is_err());
    assert!(
        validate_chapter_plan_r2(&fixture("invalid", "narrative-chapter-plan-r2-invalid")).is_err()
    );
}

#[test]
fn story_bible_and_style_card_are_artifact_types() {
    for name in ["StoryBible", "StyleCard"] {
        let parsed: ArtifactType = serde_json::from_value(Value::String(name.into())).unwrap();
        assert_eq!(
            serde_json::to_value(parsed).unwrap(),
            Value::String(name.into())
        );
    }
}
