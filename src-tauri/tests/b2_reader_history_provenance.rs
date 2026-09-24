use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::{ManagerError, ProjectManager};
use ai_novel_production_system_core::repositories::ui_projection;
use libsql_rusqlite::params;
use serde_json::{json, Value};
use tempfile::tempdir;

const PROJECT_ID: &str = "p-b2-reader-history";
const NOW: &str = "2026-09-23T10:00:00Z";

fn insert_change(manager: &ProjectManager, change_id: &str, status: &str, source: Option<&str>) {
    let mut requested_change = json!({"description":"chapter feedback"});
    if let Some(source) = source {
        requested_change["source"] = json!(source);
    }
    let seed = json!({"requestedChange":requested_change}).to_string();
    let handle = manager.open(PROJECT_ID, "", NOW).unwrap();
    handle
        .execute(
            "INSERT INTO change_requests(change_request_id,project_id,change_type,scope,summary,status,impact_seed_json,created_at,updated_at) VALUES (?1,?2,'CHANGE','PROJECT',?3,?4,?5,?6,?6)",
            params![change_id, PROJECT_ID, change_id, status, seed, NOW],
        )
        .unwrap();
}

fn insert_materialized_result(
    manager: &ProjectManager,
    change_id: &str,
    artifact_id: &str,
    target_class: &str,
    authority_type: Option<&str>,
) {
    let payload = json!({
        "changeRequestId": change_id,
        "targetClass": target_class,
        "authorityType": authority_type,
    })
    .to_string();
    let digest = "a".repeat(64);
    let handle = manager.open(PROJECT_ID, "", NOW).unwrap();
    handle
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'ChangeRequest','PROJECT',?2,1,'m0a.artifact.v1','COMMITTED',?3,'{}',?4,?4)",
            params![artifact_id, PROJECT_ID, digest, NOW],
        )
        .unwrap();
    handle
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            params![artifact_id, payload, digest],
        )
        .unwrap();
}

fn insert_receipt(manager: &ProjectManager, receipt_id: &str) {
    let handle = manager.open(PROJECT_ID, "", NOW).unwrap();
    handle
        .execute(
            "INSERT INTO commands(project_id,command_id,idempotency_key,request_digest,status,created_at) VALUES (?1,?2,?2,?3,'COMMITTED',?4)",
            params![PROJECT_ID, receipt_id, "a".repeat(64), NOW],
        )
        .unwrap();
    handle
        .execute(
            "INSERT INTO commit_receipts(receipt_id,project_id,commit_id,command_id,idempotency_key,before_revision,after_revision,bundle_digest,receipt_json,receipt_digest,committed_at) VALUES (?1,?2,?1,?1,?1,0,1,?3,'{}',?3,?4)",
            params![receipt_id, PROJECT_ID, "a".repeat(64), NOW],
        )
        .unwrap();
}

#[test]
fn history_carries_reader_source_and_materialized_target_class_for_every_row() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    manager
        .create(PROJECT_ID, "B2 Reader history", NOW)
        .unwrap();

    insert_change(
        &manager,
        "change:reader-pending",
        "IMPACT_READY",
        Some("MANUSCRIPT_READER"),
    );
    insert_change(
        &manager,
        "change:reader-projection",
        "APPLIED",
        Some("MANUSCRIPT_READER"),
    );
    insert_materialized_result(
        &manager,
        "change:reader-projection",
        "change:reader-projection",
        "PROJECTION_ONLY",
        None,
    );
    insert_change(
        &manager,
        "change:reader-plan",
        "APPLIED",
        Some("MANUSCRIPT_READER"),
    );
    insert_materialized_result(
        &manager,
        "change:reader-plan",
        "change:reader-plan",
        "PLAN_CHANGE",
        Some("ChapterPlan"),
    );
    insert_change(
        &manager,
        "change:reader-rejected",
        "REJECTED",
        Some("MANUSCRIPT_READER"),
    );
    insert_change(&manager, "change:ordinary", "APPLIED", None);

    let history = manager
        .with_read_connection(PROJECT_ID, NOW, |connection| {
            ui_projection::history(connection, PROJECT_ID, Some("all"), None, 40)
                .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
        })
        .unwrap();
    let entries = history["entries"]["items"].as_array().unwrap();
    assert_eq!(
        entries.len(),
        5,
        "the all filter must return every change request"
    );
    let entry = |id: &str| entries.iter().find(|item| item["id"] == id).unwrap();

    assert_eq!(
        entry("change:reader-pending")["source"],
        "MANUSCRIPT_READER"
    );
    assert_eq!(entry("change:reader-pending")["status"], "impact_ready");
    assert_eq!(
        entry("change:reader-projection")["source"],
        "MANUSCRIPT_READER"
    );
    assert_eq!(
        entry("change:reader-projection")["targetClass"],
        "PROJECTION_ONLY"
    );
    assert!(entry("change:reader-projection")["authorityType"].is_null());
    assert_eq!(entry("change:reader-plan")["source"], "MANUSCRIPT_READER");
    assert_eq!(entry("change:reader-plan")["targetClass"], "PLAN_CHANGE");
    assert_eq!(entry("change:reader-plan")["authorityType"], "ChapterPlan");
    assert_eq!(
        entry("change:reader-rejected")["source"],
        "MANUSCRIPT_READER"
    );
    assert_eq!(entry("change:reader-rejected")["status"], "rejected");
    assert!(entry("change:ordinary")["source"].is_null());
}

#[test]
fn history_all_paginates_one_stable_order_across_versions_and_changes() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    manager
        .create(PROJECT_ID, "B2 Reader history", NOW)
        .unwrap();

    for index in 0..25 {
        insert_receipt(&manager, &format!("receipt:{index:02}"));
        insert_change(
            &manager,
            &format!("change:{index:02}"),
            "IMPACT_READY",
            Some("MANUSCRIPT_READER"),
        );
    }

    let fetch = |cursor: Option<&str>| {
        manager
            .with_read_connection(PROJECT_ID, NOW, |connection| {
                ui_projection::history(connection, PROJECT_ID, Some("all"), cursor, 40)
                    .map_err(|error| ManagerError::ProjectCommand(error.to_string()))
            })
            .unwrap()
    };
    let first = fetch(None);
    let next_cursor = first["entries"]["nextCursor"].as_str().unwrap();
    let second = fetch(Some(next_cursor));
    let first_ids = first["entries"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            format!(
                "{}:{}",
                item["kind"].as_str().unwrap(),
                item["id"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>();
    let second_ids = second["entries"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            format!(
                "{}:{}",
                item["kind"].as_str().unwrap(),
                item["id"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(first["entries"]["total"], 50);
    assert_eq!(first_ids.len(), 40);
    assert_eq!(second_ids.len(), 10);
    assert_eq!(second["entries"]["nextCursor"], Value::Null);
    let all_ids = first_ids
        .iter()
        .chain(&second_ids)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        all_ids.len(),
        50,
        "pages must contain each history row exactly once"
    );
    assert_eq!(first_ids[0], "VERSION:receipt:24");
    assert_eq!(first_ids[25], "CHANGE_REQUEST:change:24");
    assert_eq!(second["entries"]["items"][9]["source"], "MANUSCRIPT_READER");

    let repeat_first = fetch(None);
    let repeat_ids = repeat_first["entries"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            format!(
                "{}:{}",
                item["kind"].as_str().unwrap(),
                item["id"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        first_ids, repeat_ids,
        "same-timestamp tie breaks must be deterministic"
    );
}
