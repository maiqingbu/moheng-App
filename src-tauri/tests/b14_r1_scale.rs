use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use libsql_rusqlite::Connection;
use serde_json::json;

const NOW: &str = "2026-09-06T00:00:00Z";
const CHAPTERS: i64 = 4_000;
const CHARS_PER_CHAPTER: usize = 2_500;

fn seed_scale_database(connection: &Connection, project_id: &str) {
    connection.execute(
        "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:r1:scale',?1,1,'B14 R1 scale','COMPLETED',?2,?2)",
        libsql_rusqlite::params![project_id, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:r1:scale',?1,'book:r1:scale',1,'规模卷','CLOSED',1,?2,10000000,?3,?3)",
        libsql_rusqlite::params![project_id, CHAPTERS, NOW],
    ).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    let body: String = std::iter::repeat('长').take(CHARS_PER_CHAPTER).collect();
    for chapter_no in 1..=CHAPTERS {
        let chapter_id = format!("chapter:r1:scale:{chapter_no:04}");
        let artifact_id = format!("final:r1:scale:{chapter_no:04}");
        let scope =
            serde_json::to_string(&json!({"level":"CHAPTER","chapterId":chapter_id})).unwrap();
        let payload = json!({"payloadSchema":"final-chapter.r1","chapterId":chapter_id,"body":body,"wordCount":CHARS_PER_CHAPTER});
        let hash = sha256_canonical(&payload).unwrap();
        connection.execute(
            "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,status,word_count,final_artifact_id,final_version,created_at,updated_at) VALUES (?1,?2,'volume:r1:scale',?3,?4,'COMMITTED',?5,?6,1,?7,?7)",
            libsql_rusqlite::params![chapter_id, project_id, chapter_no, format!("第 {chapter_no} 章"), CHARS_PER_CHAPTER as i64, artifact_id, NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'FinalChapter','CHAPTER',?3,1,'m0a.artifact.v1','COMMITTED','CANON',?4,'{}',?5,?5)",
            libsql_rusqlite::params![artifact_id, project_id, scope, hash, NOW],
        ).unwrap();
        connection.execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            libsql_rusqlite::params![artifact_id, payload.to_string(), hash],
        ).unwrap();
        connection.execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'FinalChapter','CHAPTER',?2,?3,1,?4)",
            libsql_rusqlite::params![project_id, scope, artifact_id, hash],
        ).unwrap();
    }
    connection.execute_batch("COMMIT").unwrap();
}

#[test]
fn b14_r1_materializes_real_ten_million_character_scale_and_query_plans() {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("proj-b14-r1-scale", "B14 R1 scale", NOW)
        .unwrap();
    seed_scale_database(&connection, "proj-b14-r1-scale");
    let chapters: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM chapters WHERE project_id='proj-b14-r1-scale'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let characters: i64 = connection.query_row("SELECT COALESCE(SUM(LENGTH(p.payload_json)),0) FROM artifact_payloads p JOIN artifacts a ON a.artifact_id=p.artifact_id AND a.version=p.version WHERE a.project_id='proj-b14-r1-scale' AND a.artifact_type='FinalChapter'", [], |row| row.get(0)).unwrap();
    assert_eq!(chapters, CHAPTERS);
    assert!(characters >= 10_000_000);
    for sql in [
        "SELECT COUNT(*) FROM narrative_obligations WHERE project_id='proj-b14-r1-scale'",
        "SELECT COUNT(*) FROM chapters WHERE project_id='proj-b14-r1-scale' AND status='COMMITTED'",
        "SELECT COUNT(*) FROM canonical_heads WHERE project_id='proj-b14-r1-scale' AND artifact_type='FinalChapter'",
    ] {
        let plan: String = connection.query_row(&format!("EXPLAIN QUERY PLAN {sql}"), [], |row| row.get(3)).unwrap();
        println!("B14_R1_QUERY_PLAN|{sql}|{plan}");
        assert!(!plan.trim().is_empty());
    }
}
