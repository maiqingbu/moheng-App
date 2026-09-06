use ai_novel_production_system_core::commands::{
    query_completion_status_impl, query_endgame_status_impl, KernelState,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use libsql_rusqlite::Connection;
use serde_json::json;

const NOW: &str = "2026-09-06T00:00:00Z";
const PROJECT_ID: &str = "proj-b14-r2-scale";
const CHAPTERS: i64 = 4_000;
const CHARS_PER_CHAPTER: usize = 2_500;

fn head(
    connection: &Connection,
    artifact_id: &str,
    artifact_type: &str,
    scope_level: &str,
    scope_key: String,
    payload: serde_json::Value,
) {
    let hash = sha256_canonical(&payload).unwrap();
    connection.execute(
        "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'m0a.artifact.v1','COMMITTED','CANON',?6,'{}',?7,?7)",
        libsql_rusqlite::params![artifact_id, PROJECT_ID, artifact_type, scope_level, scope_key, hash, NOW],
    ).unwrap();
    connection.execute(
        "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
        libsql_rusqlite::params![artifact_id, payload.to_string(), hash],
    ).unwrap();
    connection.execute(
        "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,?2,?3,?4,?5,1,?6)",
        libsql_rusqlite::params![PROJECT_ID, artifact_type, scope_level, scope_key, artifact_id, hash],
    ).unwrap();
}

fn seed(connection: &Connection) {
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:b14:r2',?1,1,'规模认证','COMPLETED',?2,?2)", libsql_rusqlite::params![PROJECT_ID,NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:b14:r2',?1,'book:b14:r2',1,'终局卷','CLOSED',1,?2,10000000,?3,?3)", libsql_rusqlite::params![PROJECT_ID,CHAPTERS,NOW]).unwrap();
    head(
        connection,
        "book-plan:b14:r2",
        "BookPlan",
        "PROJECT",
        r#"{"level":"PROJECT"}"#.into(),
        json!({"payloadSchema":"book-plan.r1","majorMysteryGoals":[],"futureReservations":[]}),
    );
    head(
        connection,
        "volume-exit:b14:r2",
        "VolumeExitState",
        "VOLUME",
        r#"{"level":"VOLUME","volumeId":"volume:b14:r2"}"#.into(),
        json!({"payloadSchema":"volume-exit-state.r1","volumeId":"volume:b14:r2","status":"CLOSED"}),
    );
    let tasks = (1..=501)
        .map(|index| json!({"taskId":format!("task:b14:r2:{index:03}"),"class":"REQUIRED"}))
        .collect::<Vec<_>>();
    head(
        connection,
        "volume-plan:b14:r2",
        "VolumePlan",
        "VOLUME",
        r#"{"level":"VOLUME","volumeId":"volume:b14:r2"}"#.into(),
        json!({"payloadSchema":"volume-plan.r1","volumeId":"volume:b14:r2","tasks":tasks}),
    );
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    for index in 1..=501 {
        let task_id = format!("task:b14:r2:{index:03}");
        let status = if index == 501 { "BLOCKED" } else { "SATISFIED" };
        connection.execute("INSERT INTO state_deltas(state_id,project_id,state_type,value_json,chapter_id,payload_json,created_at,updated_at) VALUES (?1,?2,'volume.task.progress',?3,NULL,'{}',?4,?4)", libsql_rusqlite::params![task_id,PROJECT_ID,json!({"taskId":task_id,"status":status,"evidenceRefs":[],"remainingConditions":[]}).to_string(),NOW]).unwrap();
        connection.execute("INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,prerequisites_json,lifecycle,created_at,updated_at) VALUES (?1,?2,'BOOK','PROMISE','C','book-plan:b14:r2',1,'规模连续性',?3,'RESOLVED',?4,?4)", libsql_rusqlite::params![format!("obligation:b14:r2:{index:03}"),PROJECT_ID,json!({"kind":"AFTER_CHAPTER","value":0}).to_string(),NOW]).unwrap();
    }
    let body = "长".repeat(CHARS_PER_CHAPTER);
    for chapter_no in 1..=CHAPTERS {
        let chapter_id = format!("chapter:b14:r2:{chapter_no:04}");
        let artifact_id = format!("final:b14:r2:{chapter_no:04}");
        let scope =
            serde_json::to_string(&json!({"level":"CHAPTER","chapterId":chapter_id})).unwrap();
        let payload = json!({"payloadSchema":"final-chapter.r1","chapterId":chapter_id,"body":body,"wordCount":CHARS_PER_CHAPTER});
        let hash = sha256_canonical(&payload).unwrap();
        connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,status,word_count,final_artifact_id,final_version,created_at,updated_at) VALUES (?1,?2,'volume:b14:r2',?3,?4,'COMMITTED',?5,?6,1,?7,?7)", libsql_rusqlite::params![chapter_id,PROJECT_ID,chapter_no,format!("第{chapter_no}章"),CHARS_PER_CHAPTER as i64,artifact_id,NOW]).unwrap();
        connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,semantic_lifecycle,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,'FinalChapter','CHAPTER',?3,1,'m0a.artifact.v1','COMMITTED','CANON',?4,'{}',?5,?5)", libsql_rusqlite::params![artifact_id,PROJECT_ID,scope,hash,NOW]).unwrap();
        connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)", libsql_rusqlite::params![artifact_id,payload.to_string(),hash]).unwrap();
        connection.execute("INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,'FinalChapter','CHAPTER',?2,?3,1,?4)", libsql_rusqlite::params![PROJECT_ID,scope,artifact_id,hash]).unwrap();
    }
    connection.execute_batch("COMMIT").unwrap();
}

#[test]
fn b14_r2_runs_formal_completion_queries_at_ten_million_scale_without_page_drift() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state
        .manager
        .create(PROJECT_ID, "B14 R2 scale", NOW)
        .unwrap();
    seed(&connection);
    let characters: i64 = connection.query_row("SELECT COALESCE(SUM(LENGTH(p.payload_json)),0) FROM artifact_payloads p JOIN artifacts a ON a.artifact_id=p.artifact_id AND a.version=p.version WHERE a.project_id=?1 AND a.artifact_type='FinalChapter'", [PROJECT_ID], |row| row.get(0)).unwrap();
    assert!(characters >= 10_000_000);
    let task_count: i64 = connection.query_row("SELECT COUNT(*) FROM state_deltas WHERE project_id=?1 AND state_type='volume.task.progress'", [PROJECT_ID], |row| row.get(0)).unwrap();
    let continuity_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM narrative_obligations WHERE project_id=?1",
            [PROJECT_ID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(task_count, 501);
    assert_eq!(continuity_count, 501);
    let snapshots = [1, 30, 50].map(|page_size| {
        let request = json!({"projectId":PROJECT_ID,"scope":"completion","pageSize":page_size});
        let start = std::time::Instant::now();
        let completion = query_completion_status_impl(&state, request.clone()).unwrap();
        let endgame = query_endgame_status_impl(&state, request).unwrap();
        println!("B14_R2_SCALE|chapters={CHAPTERS}|characters={characters}|tasks={task_count}|continuity={continuity_count}|pageSize={page_size}|elapsedMs={}|status={}|blockers={}", start.elapsed().as_millis(), completion["status"], completion["blockerCount"]);
        assert_eq!(completion["status"], endgame["status"]);
        (completion["status"].clone(), completion["checks"].clone(), completion["blockerCount"].clone(), completion["relevantStateDigest"].clone())
    });
    assert_eq!(snapshots[0], snapshots[1]);
    assert_eq!(snapshots[1], snapshots[2]);
}
