//! Round 13: repetitions the planner left on its last round are read back
//! from the chapter's plan reminder for the writer.
use ai_novel_production_system_core::project_db::{paths::AppPaths, ProjectManager};
use ai_novel_production_system_core::repositories::runtime::plan_reminder_items;
use serde_json::json;

const NOW: &str = "2026-11-27T00:00:00Z";

#[test]
fn round131_a_chapters_plan_reminder_items_are_read_back() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let db = manager.create("p1", "防重演", NOW).unwrap();
    let insert = |id: &str, category: &str, chapter: &str, item: &str| {
        db.execute(
            "INSERT INTO decisions(decision_id,project_id,decision_type,outcome,reason,source_ref,payload_json,created_at,updated_at) VALUES (?1,'p1','AUDIT_REMINDER','OPEN','r',?2,?3,?4,?4)",
            libsql_rusqlite::params![id, chapter, json!({"category":category,"heldBack":[{"field":"plan","item":item,"reason":"B08_PLAN_CRAFT"}]}).to_string(), NOW],
        )
        .unwrap();
    };
    insert(
        "dec:1",
        "计划提醒",
        "chap:4",
        "异常手法「协议」的用法和之前一模一样，需要新的变化",
    );
    insert(
        "dec:2",
        "审计提醒",
        "chap:4",
        "审计：引用的原文在正文里找不到",
    );
    insert("dec:3", "计划提醒", "chap:5", "别的章");
    assert_eq!(
        plan_reminder_items(&db, "p1", "chap:4").unwrap(),
        ["异常手法「协议」的用法和之前一模一样，需要新的变化"]
    );
}
