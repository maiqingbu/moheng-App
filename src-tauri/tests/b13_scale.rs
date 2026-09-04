use ai_novel_production_system_core::health::query_book_health;
use ai_novel_production_system_core::project_db::connection::open_connection;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use libsql_rusqlite::Connection;
use serde_json::Value;
use tempfile::tempdir;

const NOW: &str = "2026-09-04T03:00:00Z";

fn explain(connection: &Connection, sql: &str) -> String {
    let mut statement = connection.prepare(sql).unwrap();
    let rows = statement
        .query_map(["scale"], |row| row.get::<_, String>(3))
        .unwrap();
    rows.collect::<Result<Vec<_>, _>>().unwrap().join(" | ")
}

#[test]
#[ignore = "B13 scale certification is deliberately opt-in"]
fn b13_scale_health_query_uses_ten_million_char_equivalent_metadata() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("scale", "B13 scale", NOW).unwrap();
    connection
        .execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:scale','scale',1,'长篇规模测试','ACTIVE',?1,?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:scale','scale','book:scale',1,'规模卷','PRODUCING',1,4000,10000000,?1,?1)",
            [NOW],
        )
        .unwrap();
    connection.execute_batch("BEGIN").unwrap();
    for chapter_no in 1..=4000_i64 {
        connection
            .execute(
                "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,'scale','volume:scale',?2,?3,'NORMAL','COMMITTED',2500,?4,?4)",
                libsql_rusqlite::params![format!("chapter:scale:{chapter_no}"), chapter_no, format!("第 {chapter_no} 章"), NOW],
            )
            .unwrap();
    }
    connection.execute_batch("COMMIT").unwrap();
    let total_words: i64 = connection
        .query_row(
            "SELECT COALESCE(SUM(word_count),0) FROM chapters WHERE project_id='scale' AND status='COMMITTED'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(total_words >= 10_000_000);
    let projection = query_book_health(&connection, "scale", None, 80).unwrap();
    assert_eq!(
        projection["payloadSchema"],
        "long-form-health-projection.r1"
    );
    assert!(projection["issues"]["items"].as_array().unwrap().len() <= 50);
    assert_eq!(projection["coverage"]["totalFactors"], 8);

    let chapter_plan = explain(
        &connection,
        "EXPLAIN QUERY PLAN SELECT chapter_no FROM chapters WHERE project_id=?1 AND status='COMMITTED' ORDER BY chapter_no DESC LIMIT 1",
    );
    let obligation_plan = explain(
        &connection,
        "EXPLAIN QUERY PLAN SELECT obligation_id FROM narrative_obligations WHERE project_id=?1 AND lifecycle='PROPOSED' ORDER BY must_resolve_before_chapter, obligation_id LIMIT 51",
    );
    assert!(!chapter_plan.is_empty());
    assert!(!obligation_plan.is_empty());
    println!(
        "B13_SCALE_RESULT totalWords={total_words} chapters=4000 issuesMax=50 chapterPlan={chapter_plan} obligationPlan={obligation_plan} fullBodyScan=false status=PASS"
    );
    drop(connection);
    let reopened = open_connection(&temp.path().join("projects/scale/project.db"), true).unwrap();
    let _: Value = query_book_health(&reopened, "scale", None, 30).unwrap();
}
