use serde_json::Value;

pub mod runtime;

pub use runtime::{
    build_continuity_threads, build_volume_progress, commit_context_manifest_owner,
    commit_opening_context_owner, compile_context_manifest, evaluate_plan_stale,
    evaluate_resume_condition, relevant_state_digest, ContextCompilerInput, ContinuityThreadView,
    CurrentNarrativeStateView, PlanStaleStatus, ResumeContext, RuntimeBounds, VolumeProgressInput,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadPromotionDecision {
    Promote,
    DoNotPromote,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionSignal {
    #[serde(default)]
    pub explicit_promise: bool,
    #[serde(default)]
    pub unresolved_major_conflict: bool,
    #[serde(default)]
    pub major_relationship_transition: bool,
    #[serde(default)]
    pub mystery_or_reveal_responsibility: bool,
    #[serde(default)]
    pub foreshadow_recovery_responsibility: bool,
    #[serde(default)]
    pub key_future_use_object_or_information: bool,
    #[serde(default)]
    pub goal_contract_responsibility: bool,
    #[serde(default)]
    pub hard_authority_responsibility: bool,
    #[serde(default)]
    pub future_reservation_responsibility: bool,
    #[serde(default)]
    pub ambient_description: bool,
    #[serde(default)]
    pub completed_trivial_event: bool,
    #[serde(default)]
    pub non_continuing_scene_detail: bool,
}

impl PromotionSignal {
    pub fn ordinary_action() -> Self {
        Self::default()
    }

    pub fn explicit_promise() -> Self {
        Self {
            explicit_promise: true,
            ..Self::default()
        }
    }
}

pub fn decide_thread_promotion(signal: PromotionSignal) -> ThreadPromotionDecision {
    if signal.explicit_promise
        || signal.unresolved_major_conflict
        || signal.major_relationship_transition
        || signal.mystery_or_reveal_responsibility
        || signal.foreshadow_recovery_responsibility
        || signal.key_future_use_object_or_information
        || signal.goal_contract_responsibility
        || signal.hard_authority_responsibility
        || signal.future_reservation_responsibility
    {
        ThreadPromotionDecision::Promote
    } else {
        ThreadPromotionDecision::DoNotPromote
    }
}

pub fn validate_task_progress_semantics(value: &Value) -> Result<(), String> {
    let Some(metric) = value.get("metric") else {
        return Ok(());
    };
    let Some(metric) = metric.as_object() else {
        return Ok(());
    };
    let current = metric
        .get("current")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let target = metric
        .get("target")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    if current > target {
        return Err("task progress metric.current exceeds target".into());
    }
    Ok(())
}
