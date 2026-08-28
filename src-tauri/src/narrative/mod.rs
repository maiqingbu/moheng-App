use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadPromotionDecision {
    Promote,
    DoNotPromote,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PromotionSignal {
    pub explicit_promise: bool,
    pub unresolved_major_conflict: bool,
    pub major_relationship_transition: bool,
    pub mystery_or_reveal_responsibility: bool,
    pub foreshadow_recovery_responsibility: bool,
    pub key_future_use_object_or_information: bool,
    pub goal_contract_responsibility: bool,
    pub hard_authority_responsibility: bool,
    pub future_reservation_responsibility: bool,
    pub ambient_description: bool,
    pub completed_trivial_event: bool,
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
