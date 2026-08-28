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
    pub future_reservation_responsibility: bool,
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
        || signal.future_reservation_responsibility
    {
        ThreadPromotionDecision::Promote
    } else {
        ThreadPromotionDecision::DoNotPromote
    }
}

pub fn validate_task_progress(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "task progress must be an object".to_string())?;
    let allowed = [
        "taskId",
        "status",
        "lastAdvancedChapterId",
        "evidenceRefs",
        "remainingConditions",
        "metric",
    ];
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("task progress contains an unknown field".into());
    }
    let task_id = object
        .get("taskId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "task progress requires taskId".to_string())?;
    if task_id.is_empty() {
        return Err("task progress requires taskId".into());
    }
    let status = object
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "task progress requires status".to_string())?;
    if !matches!(
        status,
        "NOT_STARTED" | "ACTIVE" | "SATISFIED" | "BLOCKED" | "INVALIDATED"
    ) {
        return Err("task progress status must be discrete".into());
    }
    for field in ["evidenceRefs", "remainingConditions"] {
        if !object.get(field).is_some_and(Value::is_array) {
            return Err(format!("task progress requires {field} array"));
        }
    }
    if let Some(metric) = object.get("metric") {
        let metric = metric
            .as_object()
            .ok_or_else(|| "task progress metric must be an object".to_string())?;
        let current = metric
            .get("current")
            .and_then(Value::as_i64)
            .filter(|value| *value >= 0)
            .ok_or_else(|| "task progress metric.current is invalid".to_string())?;
        let target = metric
            .get("target")
            .and_then(Value::as_i64)
            .filter(|value| *value > 0)
            .ok_or_else(|| "task progress metric.target is invalid".to_string())?;
        if current > target {
            return Err("task progress metric.current exceeds target".into());
        }
        if metric
            .keys()
            .any(|key| !matches!(key.as_str(), "current" | "target"))
        {
            return Err("task progress metric contains an unknown field".into());
        }
    }
    Ok(())
}
