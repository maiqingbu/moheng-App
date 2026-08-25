//! B01-P0 (§七): the project library must never fabricate `revision = 0` for a
//! project whose authoritative DB was unreadable — that is a fail-open that turns
//! a real read failure into a normal-looking, destructive-command-eligible entry.
//!
//! LIBRARY-REV-01: after a command that advances the revision, the library row
//! reports the DB's *actual* revision (not a hard-coded 0).
//! LIBRARY-REV-02: a library-projection read failure yields a non-operational
//! entry with the real error, never an operational revision=0 entry.

use ai_novel_production_system_core::commands::{query_project_library_impl, KernelState};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::json;
use tempfile::tempdir;

const NOW: &str = "2026-08-26T00:00:00Z";

fn create_state(temp: &tempfile::TempDir) -> KernelState {
    KernelState::new(AppPaths::new(temp.path()))
}

fn create_project(state: &KernelState, id: &str, title: &str) {
    let request = json!({
        "type": "project_create_from_idea",
        "commandId": format!("cmd:create:{id}"),
        "idempotencyKey": format!("idem:create:{id}"),
        "requestedAt": NOW,
        "payload": { "title": title },
    });
    state
        .manager
        .create(id, title, NOW)
        .expect("create project");
    // Record a command receipt so the project is a fully-formed authoritative row.
    state
        .manager
        .commit_project_command(
            ai_novel_production_system_core::project_db::ProjectCommandSpec {
                project_id: id,
                result_project_id: id,
                command_id: &format!("cmd:create:{id}"),
                idempotency_key: &format!("idem:create:{id}"),
                command_type: "project_create_from_idea",
                request_digest: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
                expected_revision: None,
                now: NOW,
                advance_revision: false,
            },
            |_| Ok(()),
        )
        .expect("record create receipt");
    let _ = request;
}

#[test]
fn library_revision_matches_db_after_an_advancing_command() {
    let temp = tempdir().unwrap();
    let state = create_state(&temp);
    create_project(&state, "p1", "作品甲");

    // Rename advances the authoritative revision (advance_revision = true) → 1.
    state
        .manager
        .commit_project_rename(
            ai_novel_production_system_core::project_db::ProjectCommandSpec {
                project_id: "p1",
                result_project_id: "p1",
                command_id: "cmd:rename:p1",
                idempotency_key: "idem:rename:p1",
                command_type: "project_rename",
                request_digest: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                expected_revision: Some(0),
                now: NOW,
                advance_revision: true,
            },
            "作品甲·改",
        )
        .unwrap();

    let db_revision = state.manager.project_revision("p1").unwrap();
    assert_eq!(db_revision, 1, "rename must advance the DB revision to 1");

    let library = query_project_library_impl(&state).unwrap();
    let projects = library["projects"].as_array().unwrap();
    let p1 = projects
        .iter()
        .find(|p| p["id"] == "p1")
        .expect("p1 in library");
    assert_eq!(
        p1["revision"], 1,
        "library must report the DB's actual revision, not a fabricated 0"
    );
    assert_eq!(p1["operational"], true, "a readable project is operational");
    // Not leaking a fabricated 0 anywhere.
    assert_ne!(p1["revision"], 0);
}

#[test]
fn library_marks_unreadable_project_nonoperational_with_error() {
    let temp = tempdir().unwrap();
    let state = create_state(&temp);
    create_project(&state, "p1", "作品甲");

    // Corrupt the project DB so library_projection fails: drop the project_meta
    // table out from under a to-be-read DB, or simply remove the DB file leaving a
    // broken registered path. Removing the whole project dir makes the read fail
    // with ProjectNotFound-ish; the entry must be operational=false, not a
    // fabricated operational revision=0.
    let db = temp.path().join("projects/p1/project.db");
    assert!(db.is_file(), "project.db exists before corruption");
    std::fs::remove_file(&db).unwrap();

    let library = query_project_library_impl(&state).unwrap();
    let projects = library["projects"].as_array().unwrap();
    // The catalog still lists p1 (catalog is independent of the project DB), so it
    // appears in the library — but as non-operational.
    let p1 = projects
        .iter()
        .find(|p| p["id"] == "p1")
        .expect("p1 still listed");
    assert_eq!(
        p1["operational"], false,
        "an unreadable project must never be presented as operational"
    );
    assert!(
        p1["readError"].as_str().is_some_and(|s| !s.is_empty()),
        "readError must carry the real error, not a silent default"
    );
    // The totals must not count an unreadable project as producing/completed.
    let totals = &library["totals"];
    let active = totals["active"].as_i64().unwrap_or(0);
    // p1's lifecycle is ACTIVE; the operational=false entry is still listed but not
    // counted as a healthy producing/completed project. It is listed in projects
    // but the totals derive producing/waiting/completed from runtime state, which
    // was never read, so those are zero.
    assert_eq!(totals["producing"].as_i64().unwrap_or(0), 0);
    assert!(
        active >= 1,
        "cataloged ACTIVE project still appears in active count"
    );
}
