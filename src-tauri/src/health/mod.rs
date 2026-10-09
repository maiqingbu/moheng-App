//! B13 Long-form Health deterministic projection for `LongFormHealthProjectionR1`.
//!
//! The health page is a read-only, rebuildable view.  This module owns policy
//! and aggregation only; all database access is delegated to
//! `repositories::health::HealthRepository`.

use crate::repositories::health::{
    HealthIssueCursor, HealthIssueRow, HealthRepository, HealthRepositoryError, HealthSnapshot,
    HealthSourcePage, QualitySample,
};
use serde_json::{json, Map, Value};

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
    pub source_refs_total: i64,
    pub source_refs_truncated: bool,
    pub source_refs_next_cursor: Option<String>,
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
            source_refs_total: 0,
            source_refs_truncated: false,
            source_refs_next_cursor: None,
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
            source_refs_total: 0,
            source_refs_truncated: false,
            source_refs_next_cursor: None,
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
            source_refs_total: 0,
            source_refs_truncated: false,
            source_refs_next_cursor: None,
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
        self.source_refs_total = refs.len() as i64;
        self.source_refs = refs;
        self
    }

    fn with_source_page(mut self, page: &HealthSourcePage) -> Self {
        self.source_refs = page
            .items
            .iter()
            .map(|item| source(item.kind.as_str(), item.reference.as_str()))
            .collect();
        self.source_refs_total = page.total;
        self.source_refs_truncated = page.total > self.source_refs.len() as i64;
        self.source_refs_next_cursor = page.next_cursor.as_ref().map(encode_health_issue_cursor);
        self
    }

    fn with_severity(mut self, severity: &str) -> Self {
        self.severity = severity.to_string();
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
            "sourceRefsTotal": self.source_refs_total,
            "sourceRefsTruncated": self.source_refs_truncated,
            "sourceRefsNextCursor": self.source_refs_next_cursor,
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
    #[error("HEALTH_FACTOR_SOURCE_CURSOR_INVALID")]
    InvalidFactorSourceCursor,
    #[error("HEALTH_ISSUE_CURSOR_INVALID")]
    InvalidIssueCursor,
    #[error("HEALTH_FACTOR_CODE_UNSUPPORTED")]
    UnsupportedFactorCode,
    #[error("HEALTH_SOURCE_REVISION_CHANGED_DURING_QUERY")]
    ConcurrentMutation,
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
            .with_severity(severity)
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
        .with_source_page(
            snapshot
                .factor_source_pages
                .get("OBLIGATION_OVERDUE")
                .unwrap_or(&HealthSourcePage::default()),
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
            .with_severity(severity)
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
        .with_source_page(
            snapshot
                .factor_source_pages
                .get("FORESHADOW_AGING")
                .unwrap_or(&HealthSourcePage::default()),
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
    let risk = snapshot.route_stale_scope_count > 0 || snapshot.route_capacity_risk_scope_count > 0;
    let attention =
        snapshot.route_capacity_attention_scope_count > 0 || snapshot.unavailable_routes > 0;
    let severity = if risk {
        "risk"
    } else if attention {
        "attention"
    } else {
        "healthy"
    };
    let mut factor = if !snapshot.health_projection_ready || snapshot.unavailable_routes > 0 {
        HealthFactorResult::unavailable("ROUTE_DRIFT", "DETERMINISTIC_RUNTIME", "卷工作区")
            .with_severity(severity)
    } else {
        HealthFactorResult::evaluated("ROUTE_DRIFT", "DETERMINISTIC_RUNTIME", severity)
    };
    factor = factor
        .with_copy(
            if severity == "healthy" {
                "规划路线与当前进度相符"
            } else if snapshot.unavailable_routes > 0 || !snapshot.health_projection_ready {
                "部分路线来源需要核对"
            } else {
                "规划路线需要复核"
            },
            "这项结果只报告既有规划投影的状态，不会自动重新规划。",
            "卷工作区",
        )
        .with_source_page(
            snapshot
                .factor_source_pages
                .get("ROUTE_DRIFT")
                .unwrap_or(&HealthSourcePage::default()),
        );
    (factor, Vec::new())
}

fn task_factor(snapshot: &HealthSnapshot) -> (HealthFactorResult, Vec<HealthIssue>, i64, i64) {
    if snapshot.route_scope_count == 0 {
        return (
            HealthFactorResult::insufficient("TASK_BLOCKAGE", "DETERMINISTIC_RUNTIME", "卷工作区"),
            Vec::new(),
            0,
            0,
        );
    }
    let blocked_required = snapshot.blocked_required_tasks;
    let blocked_optional = snapshot.blocked_optional_tasks;
    let severity = if blocked_required > 0 {
        "risk"
    } else if blocked_optional > 0 {
        "attention"
    } else {
        "healthy"
    };
    let mut factor = if !snapshot.health_projection_ready || !snapshot.task_summary_available {
        HealthFactorResult::unavailable("TASK_BLOCKAGE", "DETERMINISTIC_RUNTIME", "卷工作区")
            .with_severity(severity)
            .with_copy(
                "任务状态汇总暂不可用",
                "正式任务汇总数据不完整，当前结果不会将未知任务判断为正常。",
                "卷工作区",
            )
    } else {
        HealthFactorResult::evaluated("TASK_BLOCKAGE", "DETERMINISTIC_RUNTIME", severity).with_copy(
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
        )
    };
    factor = factor.with_source_page(
        snapshot
            .factor_source_pages
            .get("TASK_BLOCKAGE")
            .unwrap_or(&HealthSourcePage::default()),
    );
    (factor, Vec::new(), blocked_required, blocked_optional)
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
            .with_severity(severity)
    } else if snapshot.open_reader_desires == 0 {
        HealthFactorResult::insufficient("READER_DESIRE", "SOFT_ADVISORY", "卷工作区").with_copy(
            "读者期待来源不足",
            "当前没有未结束的正式读者期待记录，无法评估回应节点。",
            "卷工作区",
        )
    } else {
        HealthFactorResult::evaluated("READER_DESIRE", "SOFT_ADVISORY", severity)
    };
    if snapshot.open_reader_desires > 0 || snapshot.unavailable_reader_desires > 0 {
        factor = factor.with_copy(
            if snapshot.unavailable_reader_desires > 0 {
                "部分读者期待来源需要核对"
            } else if snapshot.overdue_reader_desires > 0 {
                "有读者期待到了回应节点"
            } else {
                "读者期待记录稳定"
            },
            &format!(
                "当前有 {} 项未结束的读者期待，其中 {} 项已到预期回应章节。",
                snapshot.open_reader_desires, snapshot.overdue_reader_desires
            ),
            "卷工作区",
        );
    }
    factor = factor.with_source_page(
        snapshot
            .factor_source_pages
            .get("READER_DESIRE")
            .unwrap_or(&HealthSourcePage::default()),
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

fn issue_cursor(issue: &HealthIssue, source_revision: i64) -> HealthIssueCursor {
    HealthIssueCursor {
        source_revision,
        severity_rank: severity_rank(&issue.severity) as i64,
        factor_rank: factor_rank(&issue.factor_code) as i64,
        due_sort: issue.due_chapter.unwrap_or(i64::MAX),
        issue_id: issue.issue_id.clone(),
    }
}

fn encode_issue_cursor(issue: &HealthIssue, source_revision: i64) -> String {
    encode_health_issue_cursor(&issue_cursor(issue, source_revision))
}

fn encode_health_issue_cursor(cursor: &HealthIssueCursor) -> String {
    let issue_id = cursor
        .issue_id
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!(
        "h2:{}:{}:{}:{}:{}",
        cursor.source_revision, cursor.severity_rank, cursor.factor_rank, cursor.due_sort, issue_id
    )
}

fn decode_issue_cursor(value: Option<&str>) -> Option<HealthIssueCursor> {
    let mut parts = value?.splitn(6, ':');
    if parts.next()? != "h2" {
        return None;
    }
    let source_revision = parts.next()?.parse().ok()?;
    let severity_rank = parts.next()?.parse().ok()?;
    let factor_rank = parts.next()?.parse().ok()?;
    let due_sort = parts.next()?.parse().ok()?;
    let encoded_id = parts.next()?;
    if encoded_id.len() % 2 != 0 {
        return None;
    }
    let bytes = (0..encoded_id.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&encoded_id[index..index + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    Some(HealthIssueCursor {
        source_revision,
        severity_rank,
        factor_rank,
        due_sort,
        issue_id: String::from_utf8(bytes).ok()?,
    })
}

fn cursor_is_after(issue: &HealthIssue, cursor: &HealthIssueCursor) -> bool {
    let key = issue_cursor(issue, cursor.source_revision);
    (
        key.severity_rank,
        key.factor_rank,
        key.due_sort,
        key.issue_id,
    ) > (
        cursor.severity_rank,
        cursor.factor_rank,
        cursor.due_sort,
        cursor.issue_id.clone(),
    )
}

fn bounded_page(
    mut issues: Vec<HealthIssue>,
    total: i64,
    requested_limit: u64,
    source_revision: i64,
) -> Value {
    let limit = if requested_limit == 0 {
        30
    } else {
        requested_limit.min(50) as usize
    };
    sort_issues(&mut issues);
    let has_more = issues.len() > limit;
    let items = issues
        .iter()
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
    let next_cursor = has_more
        .then(|| {
            issues
                .get(limit - 1)
                .map(|issue| encode_issue_cursor(issue, source_revision))
        })
        .flatten();
    json!({"items":items,"nextCursor":next_cursor,"total":total})
}

fn factor_score(factor: &HealthFactorResult) -> Option<i64> {
    (factor.status == "EVALUATED").then_some(match factor.severity.as_str() {
        "risk" => 20,
        "attention" => 60,
        _ => 100,
    })
}

fn overall_health_level(factors: &[HealthFactorResult], has_formal_activity: bool) -> &'static str {
    if factors.iter().any(|factor| {
        factor.truth_class == "DETERMINISTIC_RUNTIME"
            && factor.status != "INSUFFICIENT_DATA"
            && factor.severity == "risk"
    }) {
        return "risk";
    }
    if factors.iter().any(|factor| {
        factor.status == "EVALUATED" && matches!(factor.severity.as_str(), "attention" | "risk")
    }) {
        return "attention";
    }
    if factors
        .iter()
        .any(|factor| factor.status == "SOURCE_UNAVAILABLE")
    {
        return "source_unavailable";
    }
    if !has_formal_activity || factors.iter().all(|factor| factor.status != "EVALUATED") {
        return "not_evaluated";
    }
    if factors
        .iter()
        .any(|factor| factor.status == "INSUFFICIENT_DATA")
    {
        return "insufficient_data";
    }
    "healthy"
}

fn build_projection(
    snapshot: HealthSnapshot,
    cursor: Option<&HealthIssueCursor>,
    issue_rows: Vec<HealthIssueRow>,
    database_issue_total: i64,
    limit: u64,
) -> Value {
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
    issues.retain(|issue| {
        matches!(
            issue.factor_code.as_str(),
            "QUALITY_DRIFT" | "CONTEXT_GROWTH" | "MODEL_COST_DRIFT"
        )
    });
    let advisory_issue_total = issues.len() as i64;
    if let Some(cursor) = cursor {
        issues.retain(|issue| cursor_is_after(issue, cursor));
    }
    issues.extend(issue_rows.into_iter().map(|row| HealthIssue {
        issue_id: row.issue_id,
        factor_code: row.factor_code,
        severity: row.severity,
        title: row.title,
        detail: row.detail,
        location: row.location,
        due_chapter: row.due_chapter,
        recommended_destination: row.recommended_destination,
        source_refs: vec![source(row.source_kind.as_str(), row.source_ref)],
    }));

    let evaluated_scores = factors.iter().filter_map(factor_score).collect::<Vec<_>>();
    let level = overall_health_level(&factors, snapshot.has_formal_activity);
    let score = (matches!(level, "healthy" | "attention" | "risk") && !evaluated_scores.is_empty())
        .then(|| {
            (evaluated_scores.iter().sum::<i64>() as f64 / evaluated_scores.len() as f64).round()
                as i64
        });
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
        .map(|factor| {
            (
                factor.factor_code.clone(),
                Value::Array(factor.source_refs.clone()),
            )
        })
        .collect::<Map<String, Value>>();
    let factor_source_pages = factors
        .iter()
        .map(|factor| {
            (
                factor.factor_code.clone(),
                json!({
                    "items": factor.source_refs,
                    "total": factor.source_refs_total,
                    "truncated": factor.source_refs_truncated,
                    "nextCursor": factor.source_refs_next_cursor,
                }),
            )
        })
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
        "issues": bounded_page(
            issues,
            database_issue_total + advisory_issue_total,
            limit,
            snapshot.project_revision,
        ),
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
        "provenance": {"queryMode":"ON_DEMAND","cacheMode":"PROJECTION_ONLY","sourceRevision":snapshot.project_revision,"bounded":true,"fullManuscriptScan":false,"factorSources":factor_sources,"factorSourcePages":factor_source_pages}
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
    let cursor = match cursor {
        Some(value) => {
            Some(decode_issue_cursor(Some(value)).ok_or(HealthError::InvalidIssueCursor)?)
        }
        None => None,
    };
    let snapshot = HealthRepository::load(connection, project_id)?;
    let issue_limit = if limit == 0 {
        30
    } else {
        limit.min(50) as usize
    };
    let source_revision = snapshot.project_revision;
    let issue_page = HealthRepository::issue_page(
        connection,
        project_id,
        snapshot.current_chapter_no,
        cursor.as_ref(),
        issue_limit + 4,
    )?;
    if issue_page.source_revision != snapshot.project_revision {
        return Err(HealthError::ConcurrentMutation);
    }
    if snapshot.health_projection_ready
        && !HealthRepository::projection_is_current(connection, project_id)?
    {
        return Err(HealthError::ConcurrentMutation);
    }
    let mut projection = build_projection(
        snapshot,
        cursor.as_ref(),
        issue_page.rows,
        issue_page.total,
        limit,
    );
    if HealthRepository::current_project_revision(connection, project_id)? != source_revision {
        return Err(HealthError::ConcurrentMutation);
    }
    projection["projectId"] = json!(project_id);
    Ok(projection)
}

/// Deterministic continuation for the bounded provenance page attached to a
/// health factor. It reads only the selected factor's scalar issue/source rows.
pub fn query_book_health_factor_sources(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    factor_code: &str,
    cursor: Option<&str>,
    limit: u64,
) -> Result<Value, HealthError> {
    if !FACTOR_CODES.contains(&factor_code) {
        return Err(HealthError::UnsupportedFactorCode);
    }
    let current_chapter_no = HealthRepository::current_chapter_no(connection, project_id)?;
    let cursor = match cursor {
        Some(value) => {
            Some(decode_issue_cursor(Some(value)).ok_or(HealthError::InvalidFactorSourceCursor)?)
        }
        None => None,
    };
    let page = HealthRepository::factor_source_page_after(
        connection,
        project_id,
        current_chapter_no,
        factor_code,
        cursor.as_ref(),
        limit.clamp(1, 50) as usize,
    )?;
    if HealthRepository::current_project_revision(connection, project_id)? != page.source_revision {
        return Err(HealthError::ConcurrentMutation);
    }
    let items = page
        .items
        .iter()
        .map(|item| source(item.kind.as_str(), item.reference.as_str()))
        .collect::<Vec<_>>();
    Ok(json!({
        "items": items,
        "total": page.total,
        "truncated": page.next_cursor.is_some(),
        "nextCursor": page.next_cursor.as_ref().map(encode_health_issue_cursor),
    }))
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

    #[test]
    fn overall_status_distinguishes_unassessed_incomplete_unavailable_and_health_levels() {
        let evaluated =
            |severity: &str| HealthFactorResult::evaluated("A", "DETERMINISTIC_RUNTIME", severity);
        let insufficient =
            HealthFactorResult::insufficient("A", "DETERMINISTIC_RUNTIME", "卷工作区");
        let unavailable = HealthFactorResult::unavailable("A", "DETERMINISTIC_RUNTIME", "卷工作区");

        assert_eq!(
            overall_health_level(std::slice::from_ref(&insufficient), false),
            "not_evaluated"
        );
        assert_eq!(
            overall_health_level(&[evaluated("healthy"), insufficient.clone()], true),
            "insufficient_data"
        );
        assert_eq!(
            overall_health_level(&[unavailable], true),
            "source_unavailable"
        );
        assert_eq!(
            overall_health_level(&[evaluated("healthy"), evaluated("attention")], true),
            "attention"
        );
        assert_eq!(overall_health_level(&[evaluated("risk")], true), "risk");
        assert_eq!(
            overall_health_level(&[evaluated("healthy")], true),
            "healthy"
        );
    }
}
