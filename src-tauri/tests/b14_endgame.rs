use ai_novel_production_system_core::endgame::{
    evaluate_completion, relevant_state_digest, CompletionInput, CompletionStatus,
};

fn ready_input() -> CompletionInput {
    CompletionInput {
        source_available: true,
        mystery_source_available: true,
        requested_ending_confirmed: true,
        endgame_contract_fresh: true,
        unmapped_mysteries: 0,
        unresolved_mapped_mysteries: 0,
        unresolved_hard_obligations: 0,
        unresolved_hard_payoffs: 0,
        unfulfilled_hard_reservations: 0,
        open_hard_continuity: 0,
        blocked_required_tasks: 0,
        final_volume_closed: true,
        volume_exit_state_available: true,
        pending_relevant_change: false,
        plan_stale: false,
        export_integrity_failures: 0,
        soft_advisories: 0,
        detail_limit: 30,
    }
}

#[test]
fn hard_obligation_blocks_completion() {
    let mut input = ready_input();
    input.unresolved_hard_obligations = 1;
    let result = evaluate_completion(&input);
    assert_eq!(result.status, CompletionStatus::Blocked);
    assert_eq!(result.check("OBLIGATION").status, CompletionStatus::Blocked);
    assert_eq!(result.check("PAYOFF").blocker_count, 0);
}

#[test]
fn unmapped_mystery_requires_user_decision() {
    let mut input = ready_input();
    input.unmapped_mysteries = 1;
    let result = evaluate_completion(&input);
    assert_eq!(result.status, CompletionStatus::NeedsDecision);
    assert_eq!(
        result.check("MYSTERY").status,
        CompletionStatus::NeedsDecision
    );
}

#[test]
fn soft_advisories_do_not_change_ready_status() {
    let mut input = ready_input();
    input.soft_advisories = 4;
    let result = evaluate_completion(&input);
    assert_eq!(result.status, CompletionStatus::Ready);
    assert_eq!(result.advisory_count, 4);
    assert_eq!(result.check("PAYOFF").status, CompletionStatus::Ready);
}

#[test]
fn relevant_change_and_stale_plan_block_completion() {
    let mut input = ready_input();
    input.pending_relevant_change = true;
    input.plan_stale = true;
    let result = evaluate_completion(&input);
    assert_eq!(result.status, CompletionStatus::Blocked);
    assert_eq!(result.check("CHANGE_STALE").blocker_count, 2);
}

#[test]
fn detail_page_limit_does_not_change_global_truth_or_digest() {
    let mut small_page = ready_input();
    small_page.unresolved_hard_obligations = 7;
    small_page.detail_limit = 1;
    let mut large_page = small_page.clone();
    large_page.detail_limit = 50;
    assert_eq!(
        evaluate_completion(&small_page),
        evaluate_completion(&large_page)
    );
    assert_eq!(
        relevant_state_digest(&small_page).unwrap(),
        relevant_state_digest(&large_page).unwrap()
    );
}

#[test]
fn unavailable_source_never_reports_ready() {
    let mut input = ready_input();
    input.source_available = false;
    let result = evaluate_completion(&input);
    assert_eq!(result.status, CompletionStatus::SourceUnavailable);
}
