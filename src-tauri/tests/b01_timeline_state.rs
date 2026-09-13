use ai_novel_production_system_core::commands::{query_production_center_impl, KernelState};
use ai_novel_production_system_core::production::UnavailableProductionGateway;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use libsql_rusqlite::params;
use serde_json::json;
use std::sync::Arc;

const NOW: &str = "2026-09-13T10:00:00Z";

fn timeline_statuses(projection: &serde_json::Value) -> Vec<String> {
    projection["timeline"]
        .as_array()
        .expect("timeline")
        .iter()
        .map(|item| item["status"].as_str().expect("timeline status").to_owned())
        .collect()
}

#[test]
fn b01_timeline_state_machine_keeps_idle_and_completed_truthful() {
    let temp = tempfile::tempdir().expect("temp directory");
    let paths = AppPaths::new(temp.path());
    let manager = ai_novel_production_system_core::project_db::ProjectManager::new(paths.clone());
    let connection = manager
        .create("b01-timeline", "B01 timeline", NOW)
        .expect("create project");
    drop(connection);
    let state = KernelState::with_production_gateway(paths, Arc::new(UnavailableProductionGateway));

    let idle = query_production_center_impl(&state, json!({"projectId":"b01-timeline"}))
        .expect("idle projection");
    assert!(timeline_statuses(&idle)
        .iter()
        .all(|status| status == "pending"));

    let connection = state
        .manager
        .open("b01-timeline", "", NOW)
        .expect("open project");
    connection
        .execute(
            "INSERT INTO jobs(job_id,project_id,job_type,status,current_stage,created_at,updated_at) VALUES (?1,?2,'CHAPTER','RUNNING','RENDER',?3,?3)",
            params!["job:b01:timeline", "b01-timeline", NOW],
        )
        .expect("running job");
    drop(connection);
    let running = query_production_center_impl(&state, json!({"projectId":"b01-timeline"}))
        .expect("running projection");
    let running_statuses = timeline_statuses(&running);
    assert_eq!(&running_statuses[..3], &["done", "done", "active"]);
    assert!(running_statuses[3..]
        .iter()
        .all(|status| status == "pending"));
    assert!(running["tasks"][0]["progress"].is_null());

    let connection = state
        .manager
        .open("b01-timeline", "", NOW)
        .expect("reopen project");
    connection
        .execute(
            "UPDATE jobs SET status='COMPLETED',current_stage='COMPLETED',updated_at=?1 WHERE job_id=?2",
            params![NOW, "job:b01:timeline"],
        )
        .expect("complete job");
    drop(connection);
    let completed = query_production_center_impl(&state, json!({"projectId":"b01-timeline"}))
        .expect("completed projection");
    assert!(timeline_statuses(&completed)
        .iter()
        .all(|status| status == "done"));
    assert!(timeline_statuses(&completed)
        .iter()
        .all(|status| status != "active"));
    assert_eq!(completed["tasks"][0]["progress"], 100);
}
