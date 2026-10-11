use ai_novel_production_system_core::commands::{
    query_completion_status_impl, query_endgame_status_impl, KernelState,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
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
    connection.execute("INSERT INTO canon_events(event_id,project_id,chapter_id,event_type,event_ordinal,payload_json,created_at) VALUES ('event:b14:r2:scale',?1,'chapter:b14:r2:0001','PROMISE_MADE',1,'{}',?2)", libsql_rusqlite::params![PROJECT_ID,NOW]).unwrap();
    connection.execute("UPDATE narrative_obligations SET source_event_id='event:b14:r2:scale' WHERE project_id=?1 AND obligation_id='obligation:b14:r2:001'", [PROJECT_ID]).unwrap();
    connection.execute("INSERT INTO foreshadows(foreshadow_id,project_id,intent,status,source_authority,created_at,updated_at) VALUES ('foreshadow:b14:r2:scale',?1,'规模伏笔','PLANTED','B06',?2,?2)", libsql_rusqlite::params![PROJECT_ID,NOW]).unwrap();
    connection.execute("INSERT INTO mystery_states(mystery_id,project_id,mystery_type,status,created_at,updated_at) VALUES ('mystery:b14:r2:scale',?1,'规模谜团','OPEN',?2,?2)", libsql_rusqlite::params![PROJECT_ID,NOW]).unwrap();
    connection.execute("INSERT INTO canon_evidence(evidence_id,project_id,fact_id,event_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,created_at) VALUES ('evidence:b14:r2:scale',?1,'fact:b14:r2:scale','event:b14:r2:scale',?2,?3,1,'CANON','规模证据',?4)", libsql_rusqlite::params![PROJECT_ID,"chapter:b14:r2:0001","final:b14:r2:0001",NOW]).unwrap();
    connection.execute_batch("COMMIT").unwrap();
}

#[test]
fn b14_r2_runs_formal_completion_queries_at_ten_million_scale_without_page_drift() {
    let temp = tempfile::tempdir().unwrap();
    #[cfg(debug_assertions)]
    let state = KernelState::new(AppPaths::new(temp.path()));
    #[cfg(not(debug_assertions))]
    let state = KernelState::for_scale_bench(AppPaths::new(temp.path()));
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
    let continuity =
        NarrativeRepository::continuity_threads_complete(&connection, PROJECT_ID).unwrap();
    assert!(continuity
        .iter()
        .any(|thread| thread.thread_id == "thread:foreshadow:b14:r2:scale"));
    assert!(continuity
        .iter()
        .any(|thread| thread.thread_id == "thread:mystery:b14:r2:scale"));
    assert!(continuity
        .iter()
        .any(|thread| !thread.evidence_refs.is_empty()));
    let query_plans = [
        ("QP-01-obligation-aggregate", "SELECT COUNT(*) FROM narrative_obligations WHERE project_id='proj-b14-r2-scale'"),
        ("QP-02-continuity-projection", "SELECT obligation_id,authority_level,lifecycle FROM narrative_obligations WHERE project_id='proj-b14-r2-scale' ORDER BY obligation_id"),
        ("QP-03-volume-plan-task-class", "SELECT p.payload_json FROM canonical_heads h JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id='proj-b14-r2-scale' AND h.artifact_type='VolumePlan'"),
        ("QP-04-task-reality-chunk", "SELECT state_id,value_json FROM state_deltas WHERE project_id='proj-b14-r2-scale' AND state_type='volume.task.progress' ORDER BY updated_at,state_id LIMIT 100"),
        ("QP-05-mapping-lookup", "SELECT artifact_id,version FROM canonical_heads WHERE project_id='proj-b14-r2-scale' AND artifact_type IN ('BookPlan','FinalChapter')"),
        ("QP-06-change-instance-ref", "SELECT change_request_id,impact_seed_json FROM change_requests WHERE project_id='proj-b14-r2-scale' AND status IN ('DRAFT','APPROVED')"),
        ("QP-07-formal-stale", "SELECT target_ref FROM stale_queue WHERE project_id='proj-b14-r2-scale' AND processed=0"),
        ("QP-08-final-chapter-inventory", "SELECT chapter_id,chapter_no,final_artifact_id,final_version FROM chapters WHERE project_id='proj-b14-r2-scale' ORDER BY chapter_no,chapter_id"),
        ("QP-09-structured-receipt", "SELECT a.artifact_id,r.receipt_id FROM artifacts a JOIN commit_receipts r ON r.project_id=a.project_id AND r.command_id=json_extract(a.provenance_json,'$.commandId') WHERE a.project_id='proj-b14-r2-scale'"),
        ("QP-10-latest-completion", "SELECT artifact_id,version FROM canonical_heads WHERE project_id='proj-b14-r2-scale' AND artifact_type='BookCompletionReport' ORDER BY version DESC LIMIT 1"),
        ("QP-CONT-02-foreshadows-complete", "SELECT foreshadow_id,intent,status FROM foreshadows WHERE project_id='proj-b14-r2-scale' AND status IN ('PLANTED','REINFORCED','STALE') ORDER BY updated_at DESC,foreshadow_id LIMIT 100 OFFSET 100"),
        ("QP-CONT-03-mysteries-complete", "SELECT mystery_id,mystery_type,status FROM mystery_states WHERE project_id='proj-b14-r2-scale' AND status IN ('OPEN','PROGRESSING') ORDER BY updated_at DESC,mystery_id LIMIT 100 OFFSET 100"),
        ("QP-CONT-04-evidence-complete", "SELECT evidence_id,fact_id,final_chapter_id FROM canon_evidence WHERE project_id='proj-b14-r2-scale' ORDER BY created_at DESC,evidence_id LIMIT 100 OFFSET 100"),
        ("QP-MAP-01-reservation-related-evidence", "SELECT a.artifact_id FROM artifacts a JOIN canonical_heads h ON h.project_id=a.project_id AND h.artifact_id=a.artifact_id AND h.version=a.version JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id='proj-b14-r2-scale' AND a.artifact_type='FinalChapter' AND json_extract(p.payload_json,'$.reservationId')='reservation:b14:r2:scale'"),
        ("QP-MAP-02-payoff-related-evidence", "SELECT a.artifact_id FROM artifacts a JOIN canonical_heads h ON h.project_id=a.project_id AND h.artifact_id=a.artifact_id AND h.version=a.version JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id='proj-b14-r2-scale' AND a.artifact_type='FinalChapter' AND json_extract(p.payload_json,'$.payoffId')='payoff:b14:r2:scale'"),
    ];
    let mut has_multi_row_plan = false;
    for (name, sql) in query_plans {
        let mut statement = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap();
        let rows = statement
            .query_map([], |row| {
                Ok(json!({
                    "id": row.get::<_, i64>(0)?,
                    "parent": row.get::<_, i64>(1)?,
                    "notUsed": row.get::<_, i64>(2)?,
                    "detail": row.get::<_, String>(3)?
                }))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(!rows.is_empty());
        let plan = rows[0]["detail"].as_str().unwrap_or_default().to_string();
        has_multi_row_plan |= rows.len() > 1;
        let sql_digest = sha256_canonical(&json!({"sql":sql})).unwrap();
        let plan_digest = sha256_canonical(&json!({"rows":rows.clone()})).unwrap();
        println!("B14_R3_QUERY_PLAN|name={name}|sqlDigest={sql_digest}|planDigest={plan_digest}|plan={plan}|rowCount={}|rows={}", rows.len(), serde_json::to_string(&rows).unwrap());
        assert!(!plan.trim().is_empty());
    }
    assert!(has_multi_row_plan);
    println!("B14_R5_QUERY_PLANS|allRows=true|hasMultiRowPlan=true|queryCount=15");
    let snapshots = [1, 30, 50].map(|page_size| {
        let request = json!({"projectId":PROJECT_ID,"scope":"completion","pageSize":page_size});
        let start = std::time::Instant::now();
        let completion = query_completion_status_impl(&state, request.clone()).unwrap();
        let endgame = query_endgame_status_impl(&state, request).unwrap();
        println!("B14_R2_SCALE|chapters={CHAPTERS}|characters={characters}|tasks={task_count}|continuity={continuity_count}|continuityThreads={}|pageSize={page_size}|elapsedMs={}|status={}|blockers={}", continuity.len(), start.elapsed().as_millis(), completion["status"], completion["blockerCount"]);
        assert_eq!(completion["status"], endgame["status"]);
        (completion["status"].clone(), completion["checks"].clone(), completion["blockerCount"].clone(), completion["relevantStateDigest"].clone())
    });
    assert_eq!(snapshots[0], snapshots[1]);
    assert_eq!(snapshots[1], snapshots[2]);
}
