use ai_novel_production_system_core::planning::{
    normalize_chapter_plan_model_output, validate_chapter_plan_horizon,
};
use serde_json::json;

#[test]
fn chapter_plan_normalization_keeps_empty_horizon_without_synthetic_text() {
    let normalized = normalize_chapter_plan_model_output(
        json!({
            "chapterGoal":"收束本卷",
            "activeTaskRefs":[],
            "continuityActions":[],
            "requiredStateOutcomes":[],
            "forbiddenTransitions":[],
            "knowledgeBoundaries":[],
            "reservationRefs":[],
            "sceneExecution":[],
            "horizonPreview":[]
        }),
        &[],
        &json!({
            "chapterId":"chapter:last",
            "compiledFromProjectRevision":1,
            "compiledFromStateDigest":"a".repeat(64),
            "contextManifestRef":{"artifactId":"context:last","version":1,"payloadHash":"b".repeat(64)}
        }),
    );

    assert_eq!(normalized["horizonPreview"], json!([]));
}

#[test]
fn chapter_horizon_obeys_remaining_volume_capacity() {
    let mut plan = json!({"horizonPreview":[]});
    assert!(validate_chapter_plan_horizon(&plan, 0).is_ok());

    plan["horizonPreview"] = json!([
        {"slotNo":1,"objective":"收束一","tentative":true},
        {"slotNo":2,"objective":"收束二","tentative":true}
    ]);
    assert!(validate_chapter_plan_horizon(&plan, 2).is_ok());
    assert!(validate_chapter_plan_horizon(&plan, 8).is_err());

    plan["horizonPreview"].as_array_mut().unwrap().push(json!({
        "slotNo":3,"objective":"继续主线","tentative":true
    }));
    assert!(validate_chapter_plan_horizon(&plan, 2).is_err());
    assert!(validate_chapter_plan_horizon(&plan, 3).is_ok());
}
