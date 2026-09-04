//! B13 Long-form Health deterministic projection for `LongFormHealthProjectionR1`.
//!
//! The health page is a read-only, rebuildable view.  This module owns policy
//! and aggregation only; all database access is delegated to
//! `repositories::health::HealthRepository`.

use crate::narrative::{evaluate_plan_stale, PlanStaleStatus};
use crate::repositories::health::{
    HealthRepository, HealthRepositoryError, HealthSnapshot, QualitySample, RouteInput,
};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};

pub const FACTOR_CODES: [&str; 8] = [
    "OBLIGATION_OVERDUE",
    "FORESHADOW_AGING",
    "ROUTE_DRIFT",
    "TASK_BLOCKAGE",
    "CONTEXT_GROWTH",
    "MODEL_COST_DRIFT",
    "READER_DESIRE",
    "QUALITY_DRIFT",
];

#[derive(Debug, Clone, PartialEq)]
pub struct HealthFactorResult {
    pub factor_code: String,
    pub truth_class: String,
    pub status: String,
    pub severity: String,
    pub summary: String,
    pub detail: String,
    pub source_refs: Vec<Value>,
    pub recommended_destination: String,
}

impl HealthFactorResult {
    pub fn evaluated(factor_code: &str, truth_class: &str, severity: &str) -> Self {
        Self {
            factor_code: factor_code.to_string(),
            truth_class: truth_class.to_string(),
            status: "EVALUATED".to_string(),
            severity: severity.to_string(),
            summary: String::new(),
            detail: String::new(),
            source_refs: Vec::new(),
            recommended_destination: "卷工作区".to_string(),
        }
    }

    fn insufficient(factor_code: &str, truth_class: &str, destination: &str) -> Self {
        Self {
            factor_code: factor_code.to_string(),
            truth_class: truth_class.to_string(),
            status: "INSUFFICIENT_DATA".to_string(),
            severity: "attention".to_string(),
            summary: "数据仍在积累".to_string(),
            detail: "当前样本不足，暂不判断长期趋势。".to_string(),
            source_refs: Vec::new(),
            recommended_destination: destination.to_string(),
        }
    }

    fn unavailable(factor_code: &str, truth_class: &str, destination: &str) -> Self {
        Self {
            factor_code: factor_code.to_string(),
            truth_class: truth_class.to_string(),
            status: "SOURCE_UNAVAILABLE".to_string(),
            severity: "attention".to_string(),
            summary: "暂时无法完成这一项检查".to_string(),
            detail: "来源记录无法可靠读取，请稍后重新检查。".to_string(),
            source_refs: Vec::new(),
            recommended_destination: destination.to_string(),
        }
    }

    fn with_copy(mut self, summary: &str, detail: &str, destination: &str) -> Self {
        self.summary = summary.to_string();
        self.detail = detail.to_string();
        self.recommended_destination = destination.to_string();
        self
    }

    fn with_sources(mut self, refs: Vec<Value>) -> Self {
        self.source_refs = refs;
        self
    }

    fn as_value(&self) -> Value {
        json!({
            "factorCode": self.factor_code,
            "truthClass": self.truth_class,
            "status": self.status,
            "severity": self.severity,
            "summary": self.summary,
            "detail": self.detail,
            "sourceRefs": self.source_refs,
            "recommendedDestination": self.recommended_destination,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HealthError {
    #[error("repository: {0}")]
    Repository(#[from] HealthRepositoryError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
struct HealthIssue {
    issue_id: String,
    factor_code: String,
    severity: String,
    title: String,
    detail: String,
    location: String,
    due_chapter: Option<i64>,
    recommended_destination: String,
    source_refs: Vec<Value>,
}

fn severity_rank(severity: &str) -> i32 {
    match severity {
        "risk" => 0,
        "attention" => 1,
        _ => 2,
    }
}

fn factor_rank(code: &str) -> usize {
    FACTOR_CODES
        .iter()
        .position(|candidate| *candidate == code)
        .unwrap_or(FACTOR_CODES.len())
}

fn source(kind: &str, reference: impl Into<String>) -> Value {
    json!({"kind": kind, "ref": reference.into()})
}

fn median_i64(values: &[i64]) -> Option<i64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    Some(sorted[(sorted.len() - 1) / 2])
}

fn median_or_insufficient(
    factor_code: &str,
    baseline: &[i64],
    recent: &[i64],
) -> Option<(i64, i64)> {
    if baseline.len() != 8 || recent.len() != 8 {
        return None;
    }
    let baseline_median = median_i64(baseline)?;
    let recent_median = median_i64(recent)?;
    if baseline_median <= 0 {
        let _ = factor_code;
        return None;
    }
    Some((baseline_median, recent_median))
}

/// Classifies the fixed 8 + 8 context window.  The first slice is the older
/// baseline and the second slice is the recent window.
pub fn classify_context_growth(baseline: &[i64], recent: &[i64]) -> HealthFactorResult {
    let Some((baseline_median, recent_median)) =
        median_or_insufficient("CONTEXT_GROWTH", baseline, recent)
    else {
        return HealthFactorResult::insufficient(
            "CONTEXT_GROWTH",
            "DETERMINISTIC_RUNTIME",
            "创作中心",
        );
    };
    let severity = if (recent_median as i128) * 100 >= (baseline_median as i128) * 150 {
        "risk"
    } else if (recent_median as i128) * 100 >= (baseline_median as i128) * 125 {
        "attention"
    } else {
        "healthy"
    };
    HealthFactorResult::evaluated("CONTEXT_GROWTH", "DETERMINISTIC_RUNTIME", severity).with_copy(
        "上下文规模趋势已完成检查",
        &format!(
            "近期中位数约为基线的 {}%。",
            recent_median * 100 / baseline_median
        ),
        "创作中心",
    )
}

/// Classifies the fixed eight completed cost samples.  The first slice is
/// predicted cost and the second slice is actual cost.
pub fn classify_cost_drift(
    predicted: &[i64],
    actual: &[i64],
    over_limit: bool,
) -> HealthFactorResult {
    if over_limit {
        return HealthFactorResult::evaluated("MODEL_COST_DRIFT", "DETERMINISTIC_RUNTIME", "risk")
            .with_copy(
                "预算占用需要立即处理",
                "当前存在尚未解决的预算超限记录。",
                "创作中心",
            );
    }
    if predicted.len() != 8 || actual.len() != 8 {
        return HealthFactorResult::insufficient(
            "MODEL_COST_DRIFT",
            "DETERMINISTIC_RUNTIME",
            "创作中心",
        );
    }
    let predicted_total: i64 = predicted.iter().copied().sum();
    let actual_total: i64 = actual.iter().copied().sum();
    if predicted_total <= 0 {
        return HealthFactorResult::insufficient(
            "MODEL_COST_DRIFT",
            "DETERMINISTIC_RUNTIME",
            "创作中心",
        );
    }
    let severity = if (actual_total as i128) * 100 >= (predicted_total as i128) * 150 {
        "risk"
    } else if (actual_total as i128) * 100 >= (predicted_total as i128) * 125 {
        "attention"
    } else {
        "healthy"
    };
    HealthFactorResult::evaluated("MODEL_COST_DRIFT", "DETERMINISTIC_RUNTIME", severity).with_copy(
        "模型与成本趋势已完成检查",
        "近期实际成本与预估成本正在持续对照。",
        "创作中心",
    )
}

/// Classifies the fixed 10 + 10 quality sample windows.  The first slice is
/// the older baseline and the second slice is recent.
pub fn classify_quality_drift(baseline: &[f64], recent: &[f64]) -> HealthFactorResult {
    if baseline.len() != 10 || recent.len() != 10 {
        return HealthFactorResult::insufficient("QUALITY_DRIFT", "SOFT_ADVISORY", "创作中心");
    }
    let baseline_mean = baseline.iter().sum::<f64>() / baseline.len() as f64;
    let recent_mean = recent.iter().sum::<f64>() / recent.len() as f64;
    let drop = baseline_mean - recent_mean;
    let severity = if drop >= 20.0 {
        "risk"
    } else if drop >= 10.0 {
        "attention"
    } else {
        "healthy"
    };
    HealthFactorResult::evaluated("QUALITY_DRIFT", "SOFT_ADVISORY", severity).with_copy(
        "质量趋势已完成检查",
        "近期质量记录与较早样本的变化仅作创作建议参考。",
        "创作中心",
    )
}

fn parse_json(raw: &str) -> Option<Value> {
    serde_json::from_str(raw).ok()
}

fn number_from(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_i64)
            .or_else(|| value.get(*key).and_then(Value::as_f64).map(|n| n as i64))
    })
}

fn status_from_json(value: &Value) -> Option<String> {
    value
        .get("status")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn route_pressure(route: &RouteInput) -> (Option<i64>, Option<i64>) {
    let pressure = parse_json(&route.planning_pressure_json).unwrap_or(Value::Null);
    let capacity = route.remaining_chapter_capacity.or_else(|| {
        number_from(
            &pressure,
            &["remainingChapterCapacity", "remaining_capacity"],
        )
    });
    let required = number_from(
        &pressure,
        &["requiredTasksRemaining", "required_tasks_remaining"],
    );
    (capacity, required)
}

fn factor_with_sources(
    factor_code: &str,
    truth_class: &str,
    severity: &str,
    summary: &str,
    detail: &str,
    destination: &str,
    refs: Vec<Value>,
) -> HealthFactorResult {
    HealthFactorResult::evaluated(factor_code, truth_class, severity)
        .with_copy(summary, detail, destination)
        .with_sources(refs)
}

fn volume_location(
    volume_no: Option<i64>,
    title: Option<&str>,
    source_unavailable: bool,
) -> String {
    let Some(volume_no) = volume_no.filter(|number| *number > 0) else {
        return "相关分卷".to_string();
    };
    let title = title.map(str::trim).filter(|value| !value.is_empty());
    if source_unavailable {
        return "相关分卷".to_string();
    }
    title.map_or_else(
        || format!("第 {volume_no} 卷"),
        |title| format!("第 {volume_no} 卷 · {title}"),
    )
}

fn route_is_formally_stale(route: &RouteInput, snapshot: &HealthSnapshot) -> bool {
    if route.formal_stale {
        return true;
    }
    match (
        route.compiled_state_digest.as_deref(),
        route.current_state_digest.as_deref(),
    ) {
        (Some(compiled), Some(current)) => matches!(
            evaluate_plan_stale(
                route.generated_revision,
                compiled,
                snapshot.project_revision,
                current,
            ),
            PlanStaleStatus::PlanStale
        ),
        _ => false,
    }
}

fn obligation_factor(snapshot: &HealthSnapshot) -> (HealthFactorResult, Vec<HealthIssue>) {
    let mut issues = Vec::new();
    for obligation in &snapshot.obligations {
        if obligation.source_unavailable {
            issues.push(HealthIssue {
                issue_id: format!("health:obligation:{}", obligation.obligation_id),
                factor_code: "OBLIGATION_OVERDUE".to_string(),
                severity: "attention".to_string(),
                title: "剧情义务来源暂不可用".to_string(),
                detail: "关联章节记录暂时无法读取，未将其误判为逾期。".to_string(),
                location: "相关章节".to_string(),
                due_chapter: None,
                recommended_destination: "卷工作区".to_string(),
                source_refs: vec![source("obligation", &obligation.obligation_id)],
            });
            continue;
        }
        let Some(due) = obligation.due_chapter_no else {
            continue;
        };
        if due > snapshot.current_chapter_no {
            continue;
        }
        let severity = if obligation.authority_level == "C" {
            "attention"
        } else {
            "risk"
        };
        issues.push(HealthIssue {
            issue_id: format!("health:obligation:{}", obligation.obligation_id),
            factor_code: "OBLIGATION_OVERDUE".to_string(),
            severity: severity.to_string(),
            title: "剧情义务需要处理".to_string(),
            detail: if obligation.description.trim().is_empty() {
                "这项剧情安排已经到达预定处理节点。".to_string()
            } else {
                obligation.description.clone()
            },
            location: format!("第 {due} 章附近"),
            due_chapter: Some(due),
            recommended_destination: "卷工作区".to_string(),
            source_refs: vec![source("obligation", &obligation.obligation_id)],
        });
    }
    let status = snapshot.unavailable_obligations > 0;
    let severity = if snapshot.overdue_obligations_authority_ab > 0 {
        "risk"
    } else if snapshot.overdue_obligations_authority_c > 0 || status {
        "attention"
    } else {
        "healthy"
    };
    let mut factor = if status {
        HealthFactorResult::unavailable("OBLIGATION_OVERDUE", "DETERMINISTIC_RUNTIME", "卷工作区")
    } else {
        HealthFactorResult::evaluated("OBLIGATION_OVERDUE", "DETERMINISTIC_RUNTIME", severity)
    };
    factor = factor
        .with_copy(
            if severity == "healthy" {
                "剧情义务节奏稳定"
            } else if status {
                "部分剧情义务来源需要核对"
            } else {
                "有剧情义务已到处理节点"
            },
            &format!(
                "当前有 {} 项未完成剧情义务，其中 {} 项已到预定处理章节。",
                snapshot.open_obligations, snapshot.overdue_obligations
            ),
            "卷工作区",
        )
        .with_sources(
            snapshot
                .obligations
                .iter()
                .take(51)
                .map(|item| source("obligation", &item.obligation_id))
                .collect(),
        );
    (factor, issues)
}

fn foreshadow_factor(snapshot: &HealthSnapshot) -> (HealthFactorResult, Vec<HealthIssue>) {
    let mut issues = Vec::new();
    for foreshadow in &snapshot.foreshadows {
        if foreshadow.source_unavailable {
            issues.push(HealthIssue {
                issue_id: format!("health:foreshadow:{}", foreshadow.foreshadow_id),
                factor_code: "FORESHADOW_AGING".to_string(),
                severity: "attention".to_string(),
                title: "伏笔来源暂不可用".to_string(),
                detail: "关联回收章节记录暂时无法读取，未将其误判为逾期。".to_string(),
                location: "相关章节".to_string(),
                due_chapter: None,
                recommended_destination: "卷工作区".to_string(),
                source_refs: vec![source("foreshadow", &foreshadow.foreshadow_id)],
            });
            continue;
        }
        let due = foreshadow.end_chapter_no;
        let overdue = due.is_some_and(|chapter| chapter <= snapshot.current_chapter_no);
        let stale_before_window = foreshadow.status == "STALE"
            && due.is_some_and(|chapter| chapter > snapshot.current_chapter_no);
        if !overdue && !stale_before_window {
            continue;
        }
        let severity = if overdue { "risk" } else { "attention" };
        issues.push(HealthIssue {
            issue_id: format!("health:foreshadow:{}", foreshadow.foreshadow_id),
            factor_code: "FORESHADOW_AGING".to_string(),
            severity: severity.to_string(),
            title: if overdue {
                "伏笔已到回收节点"
            } else {
                "伏笔需要重新关注"
            }
            .to_string(),
            detail: if foreshadow.intent.trim().is_empty() {
                "这条伏笔需要在卷工作区核对后续安排。".to_string()
            } else {
                foreshadow.intent.clone()
            },
            location: due.map_or_else(
                || "当前卷".to_string(),
                |chapter| format!("第 {chapter} 章附近"),
            ),
            due_chapter: due,
            recommended_destination: "卷工作区".to_string(),
            source_refs: vec![source("foreshadow", &foreshadow.foreshadow_id)],
        });
    }
    let severity = if snapshot.overdue_foreshadows > 0 {
        "risk"
    } else if snapshot.stale_not_overdue_foreshadows > 0 || snapshot.unavailable_foreshadows > 0 {
        "attention"
    } else {
        "healthy"
    };
    let mut factor = if snapshot.unavailable_foreshadows > 0 {
        HealthFactorResult::unavailable("FORESHADOW_AGING", "DETERMINISTIC_RUNTIME", "卷工作区")
    } else {
        HealthFactorResult::evaluated("FORESHADOW_AGING", "DETERMINISTIC_RUNTIME", severity)
    };
    factor = factor
        .with_copy(
            if severity == "healthy" {
                "伏笔回收节奏稳定"
            } else if snapshot.unavailable_foreshadows > 0 {
                "部分伏笔来源需要核对"
            } else {
                "有伏笔需要安排回收"
            },
            &format!(
                "当前有 {} 项活跃伏笔，其中 {} 项已越过明确回收窗口。",
                snapshot.open_foreshadows, snapshot.overdue_foreshadows
            ),
            "卷工作区",
        )
        .with_sources(
            snapshot
                .foreshadows
                .iter()
                .take(51)
                .map(|item| source("foreshadow", &item.foreshadow_id))
                .collect(),
        );
    (factor, issues)
}

fn route_factor(snapshot: &HealthSnapshot) -> (HealthFactorResult, Vec<HealthIssue>) {
    if snapshot.route_scope_count == 0 {
        return (
            HealthFactorResult::insufficient("ROUTE_DRIFT", "DETERMINISTIC_RUNTIME", "卷工作区")
                .with_copy(
                    "规划样本仍在积累",
                    "建立卷级规划后，这项检查会按正式路线继续更新。",
                    "卷工作区",
                ),
            Vec::new(),
        );
    }
    let mut issues = Vec::new();
    let mut risk =
        snapshot.route_stale_scope_count > 0 || snapshot.route_capacity_risk_scope_count > 0;
    let mut attention =
        snapshot.route_capacity_attention_scope_count > 0 || snapshot.unavailable_routes > 0;
    let mut refs = Vec::new();
    for route in &snapshot.routes {
        let (capacity, required) = route_pressure(route);
        let stale = route_is_formally_stale(route, snapshot);
        let capacity_risk = capacity == Some(0) && required.is_some_and(|value| value > 0);
        let capacity_attention = capacity.is_some_and(|value| value > 0 && value <= 8)
            && required.is_some_and(|value| value > 0);
        if route.source_unavailable || stale || capacity_risk || capacity_attention {
            let severity = if stale || capacity_risk {
                "risk"
            } else {
                "attention"
            };
            risk |= severity == "risk";
            attention |= severity == "attention";
            let detail = if route.source_unavailable {
                "关联分卷记录暂时无法读取，未将其误判为路线风险。"
            } else if stale {
                "当前规划版本落后于项目最新变更，需要在卷工作区重新核对。"
            } else if capacity_risk {
                "剩余章节容量已用尽，但仍有必需任务未完成。"
            } else {
                "剩余章节容量较少，仍有必需任务需要安排。"
            };
            issues.push(HealthIssue {
                issue_id: format!("health:route:{}", route.volume_id),
                factor_code: "ROUTE_DRIFT".to_string(),
                severity: severity.to_string(),
                title: if route.source_unavailable {
                    "路线来源暂不可用"
                } else if stale {
                    "规划路线需要重新核对"
                } else {
                    "卷内安排需要留出空间"
                }
                .to_string(),
                detail: detail.to_string(),
                location: volume_location(
                    route.volume_no,
                    route.volume_title.as_deref(),
                    route.source_unavailable,
                ),
                due_chapter: capacity,
                recommended_destination: "卷工作区".to_string(),
                source_refs: vec![source("volume-plan", &route.volume_id)],
            });
        }
        refs.push(source("volume-plan", &route.volume_id));
    }
    let severity = if risk {
        "risk"
    } else if attention {
        "attention"
    } else {
        "healthy"
    };
    let mut factor = if snapshot.unavailable_routes > 0 {
        HealthFactorResult::unavailable("ROUTE_DRIFT", "DETERMINISTIC_RUNTIME", "卷工作区")
    } else {
        HealthFactorResult::evaluated("ROUTE_DRIFT", "DETERMINISTIC_RUNTIME", severity)
    };
    factor = factor
        .with_copy(
            if severity == "healthy" {
                "规划路线与当前进度相符"
            } else if snapshot.unavailable_routes > 0 {
                "部分路线来源需要核对"
            } else {
                "规划路线需要复核"
            },
            "这项结果只报告既有规划投影的状态，不会自动重新规划。",
            "卷工作区",
        )
        .with_sources(refs);
    (factor, issues)
}

#[derive(Debug, Clone)]
struct TaskInput {
    task_id: String,
    class: String,
    status: String,
    volume_no: Option<i64>,
    volume_title: Option<String>,
    source_unavailable: bool,
}

fn collect_tasks(snapshot: &HealthSnapshot) -> Vec<TaskInput> {
    let mut tasks: BTreeMap<String, TaskInput> = BTreeMap::new();
    let mut ledger_status: HashMap<String, String> = HashMap::new();
    for route in &snapshot.routes {
        if let Some(Value::Object(entries)) = parse_json(&route.task_status_json) {
            for (task_id, value) in entries {
                let status = value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .or_else(|| status_from_json(&value))
                    .unwrap_or_else(|| "NOT_STARTED".to_string());
                ledger_status.insert(task_id, status);
            }
        }
        if let Some(Value::Array(entries)) = route
            .route_payload_json
            .as_deref()
            .and_then(parse_json)
            .and_then(|value| value.get("tasks").cloned())
        {
            for task in entries {
                let Some(task_id) = task.get("taskId").and_then(Value::as_str) else {
                    continue;
                };
                let class = task
                    .get("class")
                    .and_then(Value::as_str)
                    .unwrap_or("FLEXIBLE")
                    .to_string();
                tasks.insert(
                    task_id.to_string(),
                    TaskInput {
                        task_id: task_id.to_string(),
                        class,
                        status: ledger_status
                            .get(task_id)
                            .cloned()
                            .unwrap_or_else(|| "NOT_STARTED".to_string()),
                        volume_no: route.volume_no,
                        volume_title: route.volume_title.clone(),
                        source_unavailable: route.source_unavailable,
                    },
                );
            }
        }
    }
    for reality in &snapshot.task_reality {
        let Some(value) = parse_json(&reality.value_json) else {
            continue;
        };
        let task_id = value
            .get("taskId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| reality.state_id.clone());
        let status = status_from_json(&value).unwrap_or_else(|| "NOT_STARTED".to_string());
        if let Some(task) = tasks.get_mut(&task_id) {
            task.status = status;
        } else {
            // A state row without a route task is not allowed to become a hard
            // blockage. It remains visible only after a valid route is present.
        }
    }
    tasks.into_values().collect()
}

fn task_factor(snapshot: &HealthSnapshot) -> (HealthFactorResult, Vec<HealthIssue>, i64, i64) {
    let tasks = collect_tasks(snapshot);
    if snapshot.route_scope_count == 0 {
        return (
            HealthFactorResult::insufficient("TASK_BLOCKAGE", "DETERMINISTIC_RUNTIME", "卷工作区"),
            Vec::new(),
            0,
            0,
        );
    }
    let mut issues = Vec::new();
    let blocked_required = snapshot.blocked_required_tasks;
    let blocked_optional = snapshot.blocked_optional_tasks;
    for task in tasks.iter().filter(|task| task.status == "BLOCKED") {
        let required = matches!(task.class.as_str(), "REQUIRED" | "CLIMAX" | "RESERVED");
        issues.push(HealthIssue {
            issue_id: format!("health:task:{}", task.task_id),
            factor_code: "TASK_BLOCKAGE".to_string(),
            severity: if required { "risk" } else { "attention" }.to_string(),
            title: if required {
                "必需任务被阻塞"
            } else {
                "可选任务被阻塞"
            }
            .to_string(),
            detail: "请回到卷工作区核对任务条件与现实进度。".to_string(),
            location: format!(
                "{} 的卷内任务",
                volume_location(
                    task.volume_no,
                    task.volume_title.as_deref(),
                    task.source_unavailable
                )
            ),
            due_chapter: None,
            recommended_destination: "卷工作区".to_string(),
            source_refs: vec![source("task", &task.task_id)],
        });
    }
    let severity = if blocked_required > 0 {
        "risk"
    } else if blocked_optional > 0 {
        "attention"
    } else {
        "healthy"
    };
    let factor = factor_with_sources(
        "TASK_BLOCKAGE",
        "DETERMINISTIC_RUNTIME",
        severity,
        if severity == "healthy" {
            "任务推进正常"
        } else {
            "有任务需要解除阻塞"
        },
        &format!(
            "当前发现 {} 项必需任务、{} 项可选任务处于阻塞状态。",
            blocked_required, blocked_optional
        ),
        "卷工作区",
        tasks
            .iter()
            .take(201)
            .map(|task| source("task", &task.task_id))
            .collect(),
    );
    (factor, issues, blocked_required, blocked_optional)
}

fn context_factor(snapshot: &HealthSnapshot) -> HealthFactorResult {
    if snapshot.context_samples.len() < 16 {
        return HealthFactorResult::insufficient(
            "CONTEXT_GROWTH",
            "DETERMINISTIC_RUNTIME",
            "创作中心",
        )
        .with_copy(
            "上下文样本仍在积累",
            "完成更多相同能力与模型的正式运行后再判断趋势。",
            "创作中心",
        );
    }
    let recent = snapshot.context_samples[..8]
        .iter()
        .map(|sample| sample.input_tokens)
        .collect::<Vec<_>>();
    let baseline = snapshot.context_samples[8..16]
        .iter()
        .map(|sample| sample.input_tokens)
        .collect::<Vec<_>>();
    let refs = snapshot
        .context_samples
        .iter()
        .take(16)
        .map(|sample| source("model-run", &sample.model_run_id))
        .collect();
    classify_context_growth(&baseline, &recent)
        .with_sources(refs)
        .with_copy(
            if baseline == recent {
                "上下文规模趋势稳定"
            } else {
                "上下文规模趋势已完成检查"
            },
            "只比较同一能力、Provider 与模型的最近八次及之前八次完成记录。",
            "创作中心",
        )
}

fn cost_factor(snapshot: &HealthSnapshot) -> HealthFactorResult {
    let predicted = snapshot
        .cost_samples
        .iter()
        .map(|sample| sample.predicted_cost_cents)
        .collect::<Vec<_>>();
    let actual = snapshot
        .cost_samples
        .iter()
        .map(|sample| sample.actual_cost_cents)
        .collect::<Vec<_>>();
    let refs = snapshot
        .cost_samples
        .iter()
        .take(8)
        .map(|sample| source("model-run", &sample.model_run_id))
        .collect();
    classify_cost_drift(&predicted, &actual, snapshot.has_over_limit_reservation)
        .with_sources(refs)
        .with_copy(
            if snapshot.has_over_limit_reservation {
                "预算占用需要立即处理"
            } else {
                "模型与成本趋势已完成检查"
            },
            "成本趋势只使用已完成运行及其正式用量记录，不会触发模型调用。",
            "创作中心",
        )
}

fn quality_score(sample: &QualitySample) -> Result<f64, ()> {
    let value = parse_json(&sample.quality_vector_json).ok_or(())?;
    value
        .get("qualityScore")
        .or_else(|| value.get("quality_score"))
        .and_then(Value::as_f64)
        .filter(|score| score.is_finite() && *score >= 0.0 && *score <= 100.0)
        .ok_or(())
}

fn quality_factor(snapshot: &HealthSnapshot) -> HealthFactorResult {
    let mut scores = Vec::new();
    for sample in &snapshot.quality_samples {
        match quality_score(sample) {
            Ok(score) => scores.push((score, sample)),
            Err(()) => {
                return HealthFactorResult::unavailable(
                    "QUALITY_DRIFT",
                    "SOFT_ADVISORY",
                    "创作中心",
                )
                .with_copy(
                    "暂时无法完成质量趋势检查",
                    "已有质量记录的格式无法可靠读取。",
                    "创作中心",
                );
            }
        }
    }
    if scores.len() < 20 {
        return HealthFactorResult::insufficient("QUALITY_DRIFT", "SOFT_ADVISORY", "创作中心")
            .with_copy(
                "质量样本仍在积累",
                "完成更多章节质量记录后再判断趋势。",
                "创作中心",
            );
    }
    let recent = scores[..10]
        .iter()
        .map(|(score, _)| *score)
        .collect::<Vec<_>>();
    let baseline = scores[10..20]
        .iter()
        .map(|(score, _)| *score)
        .collect::<Vec<_>>();
    let refs = scores
        .iter()
        .take(20)
        .map(|(_, sample)| source("quality", &sample.sample_chapter_id))
        .collect();
    classify_quality_drift(&baseline, &recent)
        .with_sources(refs)
        .with_copy(
            "质量趋势已完成检查",
            "该结果是创作建议，不会改变正式章节状态。",
            "创作中心",
        )
}

fn reader_desire_factor(snapshot: &HealthSnapshot) -> (HealthFactorResult, Vec<HealthIssue>) {
    let mut issues = Vec::new();
    for desire in &snapshot.reader_desires {
        if desire.source_unavailable {
            issues.push(HealthIssue {
                issue_id: format!("health:reader-desire:{}", desire.desire_id),
                factor_code: "READER_DESIRE".to_string(),
                severity: "attention".to_string(),
                title: "读者期待来源暂不可用".to_string(),
                detail: "关联回应章节记录暂时无法读取，未将其误判为逾期。".to_string(),
                location: "相关章节".to_string(),
                due_chapter: None,
                recommended_destination: "卷工作区".to_string(),
                source_refs: vec![source("reader-desire", &desire.desire_id)],
            });
            continue;
        }
        let Some(due) = desire.payoff_chapter_no else {
            continue;
        };
        if due > snapshot.current_chapter_no {
            continue;
        }
        issues.push(HealthIssue {
            issue_id: format!("health:reader-desire:{}", desire.desire_id),
            factor_code: "READER_DESIRE".to_string(),
            severity: "attention".to_string(),
            title: "读者期待需要回应".to_string(),
            detail: format!(
                "{} 已到预期回应章节，可在卷工作区核对安排。",
                desire.desire_type
            ),
            location: format!("第 {due} 章附近"),
            due_chapter: Some(due),
            recommended_destination: "卷工作区".to_string(),
            source_refs: vec![source("reader-desire", &desire.desire_id)],
        });
    }
    let severity = if snapshot.overdue_reader_desires > 0 || snapshot.unavailable_reader_desires > 0
    {
        "attention"
    } else {
        "healthy"
    };
    let mut factor = if snapshot.unavailable_reader_desires > 0 {
        HealthFactorResult::unavailable("READER_DESIRE", "SOFT_ADVISORY", "卷工作区")
    } else {
        HealthFactorResult::evaluated("READER_DESIRE", "SOFT_ADVISORY", severity)
    };
    factor = factor
        .with_copy(
            if severity == "healthy" {
                "读者期待记录稳定"
            } else if snapshot.unavailable_reader_desires > 0 {
                "部分读者期待来源需要核对"
            } else {
                "有读者期待到了回应节点"
            },
            &format!(
                "当前有 {} 项未结束的读者期待，其中 {} 项已到预期回应章节。",
                snapshot.open_reader_desires, snapshot.overdue_reader_desires
            ),
            "卷工作区",
        )
        .with_sources(
            snapshot
                .reader_desires
                .iter()
                .take(51)
                .map(|item| source("reader-desire", &item.desire_id))
                .collect(),
        );
    (factor, issues)
}

fn sort_issues(issues: &mut [HealthIssue]) {
    issues.sort_by(|left, right| {
        severity_rank(&left.severity)
            .cmp(&severity_rank(&right.severity))
            .then_with(|| factor_rank(&left.factor_code).cmp(&factor_rank(&right.factor_code)))
            .then_with(|| {
                left.due_chapter
                    .unwrap_or(i64::MAX)
                    .cmp(&right.due_chapter.unwrap_or(i64::MAX))
            })
            .then_with(|| left.issue_id.cmp(&right.issue_id))
    });
}

fn bounded_page(issues: Vec<HealthIssue>, cursor: Option<&str>, requested_limit: u64) -> Value {
    let start = cursor
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let limit = if requested_limit == 0 {
        30
    } else {
        requested_limit.min(50) as usize
    };
    let total = issues.len();
    let items = issues
        .iter()
        .skip(start)
        .take(limit)
        .map(|issue| {
            json!({
                "issueId": issue.issue_id,
                "factorCode": issue.factor_code,
                "severity": issue.severity,
                "title": issue.title,
                "detail": issue.detail,
                "location": issue.location,
                "dueChapter": issue.due_chapter,
                "recommendedDestination": issue.recommended_destination,
                "sourceRefs": issue.source_refs,
            })
        })
        .collect::<Vec<_>>();
    let next_cursor = (start + items.len() < total).then(|| (start + items.len()).to_string());
    json!({"items":items,"nextCursor":next_cursor,"total":total})
}

fn factor_score(factor: &HealthFactorResult) -> Option<i64> {
    (factor.status == "EVALUATED").then_some(match factor.severity.as_str() {
        "risk" => 20,
        "attention" => 60,
        _ => 100,
    })
}

fn build_projection(snapshot: HealthSnapshot, cursor: Option<&str>, limit: u64) -> Value {
    let (obligation, mut obligation_issues) = obligation_factor(&snapshot);
    let (foreshadow, mut foreshadow_issues) = foreshadow_factor(&snapshot);
    let (route, mut route_issues) = route_factor(&snapshot);
    let (task, mut task_issues, blocked_required, blocked_optional) = task_factor(&snapshot);
    let context = context_factor(&snapshot);
    let cost = cost_factor(&snapshot);
    let (reader, mut reader_issues) = reader_desire_factor(&snapshot);
    let quality = quality_factor(&snapshot);
    let factors = [
        obligation,
        foreshadow,
        route,
        task,
        context.clone(),
        cost.clone(),
        reader,
        quality.clone(),
    ];
    let mut issues = Vec::new();
    issues.append(&mut obligation_issues);
    issues.append(&mut foreshadow_issues);
    issues.append(&mut route_issues);
    issues.append(&mut task_issues);
    issues.append(&mut reader_issues);
    if quality.status == "EVALUATED" && quality.severity != "healthy" {
        issues.push(HealthIssue {
            issue_id: "health:quality:trend".to_string(),
            factor_code: "QUALITY_DRIFT".to_string(),
            severity: quality.severity.clone(),
            title: "质量趋势需要关注".to_string(),
            detail: quality.detail.clone(),
            location: "近期章节".to_string(),
            due_chapter: None,
            recommended_destination: "创作中心".to_string(),
            source_refs: quality.source_refs.clone(),
        });
    }
    if context.status == "EVALUATED" && context.severity != "healthy" {
        issues.push(HealthIssue {
            issue_id: "health:context:trend".to_string(),
            factor_code: "CONTEXT_GROWTH".to_string(),
            severity: context.severity.clone(),
            title: "上下文规模需要关注".to_string(),
            detail: context.detail.clone(),
            location: "近期模型运行".to_string(),
            due_chapter: None,
            recommended_destination: "创作中心".to_string(),
            source_refs: context.source_refs.clone(),
        });
    }
    if cost.status == "EVALUATED" && cost.severity != "healthy" {
        issues.push(HealthIssue {
            issue_id: "health:cost:trend".to_string(),
            factor_code: "MODEL_COST_DRIFT".to_string(),
            severity: cost.severity.clone(),
            title: "模型与成本趋势需要关注".to_string(),
            detail: cost.detail.clone(),
            location: "近期模型运行".to_string(),
            due_chapter: None,
            recommended_destination: "创作中心".to_string(),
            source_refs: cost.source_refs.clone(),
        });
    }
    sort_issues(&mut issues);

    let evaluated_scores = factors.iter().filter_map(factor_score).collect::<Vec<_>>();
    let score = if evaluated_scores.is_empty() {
        100
    } else {
        (evaluated_scores.iter().sum::<i64>() as f64 / evaluated_scores.len() as f64).round() as i64
    };
    let deterministic_risk = factors.iter().any(|factor| {
        factor.truth_class == "DETERMINISTIC_RUNTIME"
            && factor.status == "EVALUATED"
            && factor.severity == "risk"
    });
    let attention = factors.iter().any(|factor| {
        factor.status == "SOURCE_UNAVAILABLE"
            || (factor.status == "EVALUATED"
                && (factor.severity == "attention" || factor.severity == "risk"))
    });
    let level = if deterministic_risk {
        "risk"
    } else if attention {
        "attention"
    } else {
        "healthy"
    };
    let evaluated = factors
        .iter()
        .filter(|factor| factor.status == "EVALUATED")
        .count();
    let insufficient = factors
        .iter()
        .filter(|factor| factor.status == "INSUFFICIENT_DATA")
        .count();
    let unavailable = factors
        .iter()
        .filter(|factor| factor.status == "SOURCE_UNAVAILABLE")
        .count();
    let factor_sources = factors
        .iter()
        .map(|factor| (factor.factor_code.clone(), factor.source_refs.clone()))
        .collect::<BTreeMap<_, _>>();
    let factor_sources = factor_sources
        .into_iter()
        .map(|(code, refs)| (code, Value::Array(refs)))
        .collect::<Map<String, Value>>();

    json!({
        "payloadSchema":"long-form-health-projection.r1",
        "projectId": null,
        "projectRevision": snapshot.project_revision,
        "checkedAt": snapshot.checked_at,
        "score": score,
        "level": level,
        "coverage": {"totalFactors":8,"evaluatedFactors":evaluated,"insufficientFactors":insufficient,"unavailableFactors":unavailable},
        "factors": factors.iter().map(HealthFactorResult::as_value).collect::<Vec<_>>(),
        "issues": bounded_page(issues, cursor, limit),
        "counters": {
            "openObligations": snapshot.open_obligations,
            "overdueObligations": snapshot.overdue_obligations,
            "openForeshadows": snapshot.open_foreshadows,
            "overdueForeshadows": snapshot.overdue_foreshadows,
            "blockedRequiredTasks": blocked_required,
            "blockedOptionalTasks": blocked_optional,
            "overdueReaderDesires": snapshot.overdue_reader_desires,
            "qualitySamples": snapshot.quality_samples.len(),
            "comparableContextRuns": snapshot.context_samples.len(),
            "comparableCostRuns": snapshot.cost_samples.len()
        },
        "provenance": {"queryMode":"ON_DEMAND","cacheMode":"PROJECTION_ONLY","sourceRevision":snapshot.project_revision,"bounded":true,"fullManuscriptScan":false,"factorSources":factor_sources}
    })
}

/// Formal B13 repository-backed query.  `projectId` is injected after the
/// policy assembly so it cannot be confused with a source ref.
pub fn query_book_health(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    cursor: Option<&str>,
    limit: u64,
) -> Result<Value, HealthError> {
    let snapshot = HealthRepository::load(connection, project_id)?;
    let mut projection = build_projection(snapshot, cursor, limit);
    projection["projectId"] = json!(project_id);
    Ok(projection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_uses_only_evaluated_factors() {
        let factors = [
            HealthFactorResult::evaluated("A", "DETERMINISTIC_RUNTIME", "healthy"),
            HealthFactorResult::insufficient("B", "DETERMINISTIC_RUNTIME", "卷工作区"),
        ];
        let scores = factors.iter().filter_map(factor_score).collect::<Vec<_>>();
        assert_eq!(scores, vec![100]);
    }
}
