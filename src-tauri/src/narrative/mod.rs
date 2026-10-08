use serde_json::Value;

pub mod responsibility;
pub mod runtime;

pub use runtime::{
    build_continuity_threads, build_volume_progress, commit_context_manifest_owner,
    commit_opening_context_owner, compile_context_manifest, evaluate_plan_stale,
    evaluate_resume_condition, measure_context_hard_budget, relevant_state_digest,
    validate_context_hard_budget, validate_context_hard_completeness, ContextCompilerInput,
    ContextHardBudgetUsage, ContextHardSource, ContinuityThreadView, CurrentNarrativeStateView,
    PlanStaleStatus, ResumeContext, RuntimeBounds, VolumeProgressInput,
    CONTEXT_HARD_SERIALIZED_BYTE_LIMIT, CONTEXT_HARD_TOKEN_LIMIT,
};
pub use runtime::{project_applicable_future_reservations, ContextPlanTask, GoalContractField};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadPromotionDecision {
    Promote,
    DoNotPromote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadResponsibilityLevel {
    B,
    C,
}

pub const THREAD_PROMOTION_RULE_VERSION: &str = "thread-promotion.r1";

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

pub fn classify_thread_responsibility(
    signal: PromotionSignal,
    has_exact_evidence: bool,
    description: Option<&str>,
    fulfillment_object: Option<&str>,
    resume_condition: Option<&Value>,
) -> ThreadResponsibilityLevel {
    let has_promoting_signal = decide_thread_promotion(signal) == ThreadPromotionDecision::Promote;
    let has_description = description.is_some_and(|value| !value.trim().is_empty());
    let has_fulfillment_object = fulfillment_object.is_some_and(|value| !value.trim().is_empty());
    let has_typed_resume_condition = resume_condition
        .is_some_and(|condition| runtime::validate_resume_condition(condition).is_ok());
    let has_only_positive_signals = !signal.ambient_description
        && !signal.completed_trivial_event
        && !signal.non_continuing_scene_detail;

    if has_promoting_signal
        && has_only_positive_signals
        && has_exact_evidence
        && has_description
        && has_fulfillment_object
        && has_typed_resume_condition
    {
        ThreadResponsibilityLevel::B
    } else {
        ThreadResponsibilityLevel::C
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

pub use runtime::{
    context_hard_scope, ContextHardSourceSize, CONTEXT_COMPILER_VERSION,
    CONTEXT_DIGEST_ALGORITHM_VERSION, CONTEXT_HARD_BUDGET_POLICY_VERSION,
    CONTEXT_HARD_CONTRACT_CLASSIFICATION_VERSION, CONTEXT_TOKEN_ESTIMATOR_VERSION,
    LONG_FORM_HARD_CONTRACT_FIELDS, LONG_FORM_OPENING_CANON_REFERENCE_RULES,
    OPENING_HARD_CONTRACT_FIELDS,
};

#[cfg(test)]
mod promotion_tests {
    use super::{classify_thread_responsibility, PromotionSignal, ThreadResponsibilityLevel};
    use serde_json::json;

    #[test]
    fn explicit_supported_responsibility_is_b_and_incomplete_evidence_is_c() {
        let signal = PromotionSignal {
            explicit_promise: true,
            ..PromotionSignal::default()
        };
        let condition = json!({"kind":"ENTITY_PRESENT","value":"entity:gugu"});
        assert_eq!(
            classify_thread_responsibility(
                signal,
                true,
                Some("顾清归来时交还玉符"),
                Some("item:jade-token"),
                Some(&condition),
            ),
            ThreadResponsibilityLevel::B
        );
        assert_eq!(
            classify_thread_responsibility(
                signal,
                false,
                Some("顾清归来时交还玉符"),
                Some("item:jade-token"),
                Some(&condition),
            ),
            ThreadResponsibilityLevel::C
        );
    }

    #[test]
    fn hint_only_or_untyped_responsibility_is_c_and_system_has_no_a_result() {
        let signal = PromotionSignal {
            mystery_or_reveal_responsibility: true,
            ..PromotionSignal::default()
        };
        assert_eq!(
            classify_thread_responsibility(signal, true, Some("谜团仍未揭开"), None, None),
            ThreadResponsibilityLevel::C
        );
        let unsupported = json!({"kind":"FREE_FORM","value":"顾清归来"});
        assert_eq!(
            classify_thread_responsibility(
                signal,
                true,
                Some("顾清归来时交还玉符"),
                Some("item:jade-token"),
                Some(&unsupported),
            ),
            ThreadResponsibilityLevel::C
        );
    }
}
