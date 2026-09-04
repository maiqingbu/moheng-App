use ai_novel_production_system_core::commands::{execute_command_impl, query_book_health_impl, KernelState};
use ai_novel_production_system_core::health::{
    classify_context_growth, classify_cost_drift, classify_quality_drift, HealthFactorResult,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::production::UnavailableProductionGateway;
use serde_json::json;
use std::sync::Arc;

const NOW: &str = "2026-09-04T02:00:00Z";

fn state() -> KernelState {
    let temp = tempfile::tempdir().unwrap();
    let root = Box::leak(Box::new(temp)).path().to_path_buf();
    KernelState::with_production_gateway(AppPaths::new(root), Arc::new(UnavailableProductionGateway))
}

fn create_project(state: &KernelState, command_id: &str) -> String {
    execute_command_impl(state, json!({
        "type":"project_create_from_idea","commandId":command_id,
        "idempotencyKey":format!("{command_id}:key"),"requestedAt":NOW,
        "payload":{"title":"B13 健康测试","idea":"一名调查员追查被篡改的长篇档案。","platform":"起点中文网","mode":"DIRECTOR"}
    })).unwrap()["projectId"].as_str().unwrap().to_owned()
}

#[test]
fn b13_context_growth_requires_eight_plus_eight_and_uses_frozen_boundaries() {
    assert_eq!(classify_context_growth(&[100; 8], &[124; 8]).severity, "healthy");
    assert_eq!(classify_context_growth(&[100; 8], &[125; 8]).severity, "attention");
    assert_eq!(classify_context_growth(&[100; 8], &[149; 8]).severity, "attention");
    assert_eq!(classify_context_growth(&[100; 8], &[150; 8]).severity, "risk");
    assert_eq!(classify_context_growth(&[100; 7], &[150; 8]).status, "INSUFFICIENT_DATA");
}

#[test]
fn b13_cost_and_quality_drift_keep_soft_and_deterministic_semantics() {
    assert_eq!(classify_cost_drift(&[100; 8], &[124; 8], false).severity, "healthy");
    assert_eq!(classify_cost_drift(&[100; 8], &[125; 8], false).severity, "attention");
    assert_eq!(classify_cost_drift(&[100; 8], &[150; 8], false).severity, "risk");
    assert_eq!(classify_cost_drift(&[0; 8], &[10; 8], false).status, "INSUFFICIENT_DATA");
    assert_eq!(classify_cost_drift(&[100; 8], &[101; 8], true).severity, "risk");
    assert_eq!(classify_quality_drift(&[100.0; 10], &[91.0; 10]).severity, "healthy");
    assert_eq!(classify_quality_drift(&[100.0; 10], &[90.0; 10]).severity, "attention");
    assert_eq!(classify_quality_drift(&[100.0; 10], &[80.0; 10]).severity, "risk");
    assert_eq!(classify_quality_drift(&[100.0; 9], &[80.0; 10]).status, "INSUFFICIENT_DATA");
}

#[test]
fn b13_query_returns_versioned_eight_factor_projection_and_bounded_issues() {
    let state = state();
    let project_id = create_project(&state, "cmd:b13:query");
    let result = query_book_health_impl(&state, json!({"projectId":project_id,"scope":"health","pageSize":80})).unwrap();
    assert_eq!(result["payloadSchema"], "long-form-health-projection.r1");
    assert_eq!(result["coverage"]["totalFactors"], 8);
    assert_eq!(result["factors"].as_array().unwrap().len(), 8);
    assert!(result["issues"]["items"].as_array().unwrap().len() <= 50);
    assert!(result.get("progressToEnding").is_none());
}

#[test]
fn b13_health_result_shape_keeps_factor_truth_class_explicit() {
    let result = HealthFactorResult::evaluated("OBLIGATION_OVERDUE", "DETERMINISTIC_RUNTIME", "healthy");
    assert_eq!(result.status, "EVALUATED");
    assert_eq!(result.truth_class, "DETERMINISTIC_RUNTIME");
}
