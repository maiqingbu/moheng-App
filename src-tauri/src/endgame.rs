use crate::hash::sha256_canonical;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompletionStatus {
    Ready,
    Blocked,
    NeedsDecision,
    NeedsRecheck,
    InsufficientData,
    SourceUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompletionCheck {
    pub kind: String,
    pub status: CompletionStatus,
    pub summary: String,
    pub blocker_count: i64,
    pub advisory_count: i64,
    pub source_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompletionEvaluation {
    pub status: CompletionStatus,
    pub checks: Vec<CompletionCheck>,
    pub blocker_count: i64,
    pub advisory_count: i64,
}

impl CompletionEvaluation {
    pub fn check(&self, kind: &str) -> &CompletionCheck {
        self.checks
            .iter()
            .find(|check| check.kind == kind)
            .unwrap_or_else(|| panic!("missing B14 completion check: {kind}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompletionInput {
    pub source_available: bool,
    pub mystery_source_available: bool,
    pub requested_ending_confirmed: bool,
    pub endgame_contract_fresh: bool,
    pub unmapped_mysteries: i64,
    pub unresolved_mapped_mysteries: i64,
    pub unresolved_hard_obligations: i64,
    pub unresolved_hard_payoffs: i64,
    pub unfulfilled_hard_reservations: i64,
    pub open_hard_continuity: i64,
    pub blocked_required_tasks: i64,
    pub final_volume_closed: bool,
    pub volume_exit_state_available: bool,
    pub pending_relevant_change: bool,
    pub plan_stale: bool,
    pub export_integrity_failures: i64,
    pub soft_advisories: i64,
    #[serde(default = "default_detail_limit")]
    pub detail_limit: i64,
}

fn default_detail_limit() -> i64 {
    30
}

fn bounded(value: i64) -> i64 {
    value.max(0)
}

fn check(
    kind: &str,
    status: CompletionStatus,
    summary: &str,
    blocker_count: i64,
    advisory_count: i64,
) -> CompletionCheck {
    CompletionCheck {
        kind: kind.to_string(),
        status,
        summary: summary.to_string(),
        blocker_count,
        advisory_count,
        source_refs: vec![format!("b14:{kind}")],
    }
}

pub fn evaluate_completion(input: &CompletionInput) -> CompletionEvaluation {
    if !input.source_available {
        let checks = [
            "OBLIGATION",
            "MYSTERY",
            "PAYOFF",
            "REQUESTED_ENDING",
            "CHANGE_STALE",
            "EXPORT_INTEGRITY",
        ]
        .into_iter()
        .map(|kind| {
            check(
                kind,
                CompletionStatus::SourceUnavailable,
                "当前正式记录暂时无法读取。",
                0,
                0,
            )
        })
        .collect();
        return CompletionEvaluation {
            status: CompletionStatus::SourceUnavailable,
            checks,
            blocker_count: 0,
            advisory_count: 0,
        };
    }

    let unresolved_obligations = bounded(input.unresolved_hard_obligations);
    let unmapped_mysteries = bounded(input.unmapped_mysteries);
    let unresolved_mapped_mysteries = bounded(input.unresolved_mapped_mysteries);
    let unresolved_payoffs = bounded(input.unresolved_hard_payoffs);
    let unfulfilled_reservations = bounded(input.unfulfilled_hard_reservations);
    let open_continuity = bounded(input.open_hard_continuity);
    let blocked_tasks = bounded(input.blocked_required_tasks);
    let mut checks = Vec::with_capacity(6);

    checks.push(check(
        "OBLIGATION",
        if unresolved_obligations > 0 {
            CompletionStatus::Blocked
        } else {
            CompletionStatus::Ready
        },
        if unresolved_obligations > 0 {
            "还有必须解决的正式责任。"
        } else {
            "正式责任已完成检查。"
        },
        unresolved_obligations,
        0,
    ));

    checks.push(check(
        "MYSTERY",
        if !input.mystery_source_available {
            CompletionStatus::InsufficientData
        } else if unmapped_mysteries > 0 {
            CompletionStatus::NeedsDecision
        } else if unresolved_mapped_mysteries > 0 {
            CompletionStatus::Blocked
        } else {
            CompletionStatus::Ready
        },
        if !input.mystery_source_available {
            "没有已记录的重要谜团来源，不能判断谜团是否已收束。"
        } else if unmapped_mysteries > 0 {
            "有重要谜团还没有明确对应到可检查的收束责任。"
        } else if unresolved_mapped_mysteries > 0 {
            "已选择的谜团收束责任尚未达到正式完成状态。"
        } else {
            "重要谜团的正式收束责任已完成检查。"
        },
        unresolved_mapped_mysteries,
        0,
    ));

    checks.push(check(
        "PAYOFF",
        if unresolved_payoffs > 0
            || unfulfilled_reservations > 0
            || open_continuity > 0
            || blocked_tasks > 0
        {
            CompletionStatus::Blocked
        } else {
            CompletionStatus::Ready
        },
        if unresolved_payoffs > 0
            || unfulfilled_reservations > 0
            || open_continuity > 0
            || blocked_tasks > 0
        {
            "仍有关键回报、未来约束、连续性或正式任务未完成。"
        } else if input.soft_advisories > 0 {
            "硬性回报已通过检查，仍有创作建议可关注。"
        } else {
            "关键回报与未来约束已完成检查。"
        },
        unresolved_payoffs + unfulfilled_reservations + open_continuity + blocked_tasks,
        bounded(input.soft_advisories),
    ));

    let requested_status = if !input.requested_ending_confirmed {
        CompletionStatus::NeedsDecision
    } else if !input.endgame_contract_fresh {
        CompletionStatus::Blocked
    } else {
        CompletionStatus::Ready
    };
    checks.push(check(
        "REQUESTED_ENDING",
        requested_status,
        if !input.requested_ending_confirmed {
            "需要先确认本次完结目标。"
        } else if !input.endgame_contract_fresh {
            "当前作品状态已经变化，需要重新检查完结准备。"
        } else {
            "用户确认的完结目标与当前终局约束一致。"
        },
        i64::from(!input.endgame_contract_fresh && input.requested_ending_confirmed),
        0,
    ));

    let stale_count = i64::from(input.pending_relevant_change) + i64::from(input.plan_stale);
    checks.push(check(
        "CHANGE_STALE",
        if stale_count > 0 {
            CompletionStatus::Blocked
        } else {
            CompletionStatus::Ready
        },
        if stale_count > 0 {
            "规划或正式变更状态已经影响完结检查。"
        } else {
            "当前规划与变更状态可用于完结检查。"
        },
        stale_count,
        0,
    ));

    let export_failures = bounded(input.export_integrity_failures)
        + i64::from(!input.final_volume_closed)
        + i64::from(!input.volume_exit_state_available);
    checks.push(check(
        "EXPORT_INTEGRITY",
        if export_failures > 0 {
            CompletionStatus::Blocked
        } else {
            CompletionStatus::Ready
        },
        if export_failures > 0 {
            "成品完整性预检尚未通过。"
        } else {
            "正式章节目录、最终卷状态与成品引用完整。"
        },
        export_failures,
        0,
    ));

    let blocker_count = checks.iter().map(|check| check.blocker_count).sum();
    let advisory_count = checks.iter().map(|check| check.advisory_count).sum();
    let status = if checks
        .iter()
        .any(|check| check.status == CompletionStatus::NeedsDecision)
    {
        CompletionStatus::NeedsDecision
    } else if blocker_count > 0 {
        CompletionStatus::Blocked
    } else if checks
        .iter()
        .any(|check| check.status == CompletionStatus::InsufficientData)
    {
        CompletionStatus::NeedsDecision
    } else {
        CompletionStatus::Ready
    };
    CompletionEvaluation {
        status,
        checks,
        blocker_count,
        advisory_count,
    }
}

pub fn relevant_state_digest(input: &CompletionInput) -> Result<String, serde_json::Error> {
    let value = json!({
        "sourceAvailable": input.source_available,
        "unmappedMysteries": input.unmapped_mysteries,
        "unresolvedMappedMysteries": input.unresolved_mapped_mysteries,
        "unresolvedHardObligations": input.unresolved_hard_obligations,
        "unresolvedHardPayoffs": input.unresolved_hard_payoffs,
        "unfulfilledHardReservations": input.unfulfilled_hard_reservations,
        "openHardContinuity": input.open_hard_continuity,
        "blockedRequiredTasks": input.blocked_required_tasks,
        "finalVolumeClosed": input.final_volume_closed,
        "volumeExitStateAvailable": input.volume_exit_state_available,
        "pendingRelevantChange": input.pending_relevant_change,
        "planStale": input.plan_stale,
        "exportIntegrityFailures": input.export_integrity_failures,
        "softAdvisories": input.soft_advisories,
    });
    sha256_canonical(&value)
}
