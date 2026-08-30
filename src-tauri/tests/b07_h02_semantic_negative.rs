use ai_novel_production_system_core::commands::{execute_command_impl, KernelState};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use libsql_rusqlite::params;
use serde_json::json;

#[test]
fn b07_g3_actual_planned_golden_three_is_rejected_by_production_command() {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state
        .manager
        .create("semantic-p1", "semantic negative", "2026-08-30T00:00:00Z")
        .unwrap();
    connection.execute("INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:semantic','semantic-p1',1,'Book','ACTIVE',?1,?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:semantic','semantic-p1','book:semantic',1,'Volume','PLANNING',1,3,3000,?1,?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    for index in 1..=3 {
        connection.execute("INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,created_at,updated_at) VALUES (?1,'semantic-p1',?2,'CHARACTER',?3,?3)", params![format!("entity:semantic:{index}"), format!("Entity {index}"), "2026-08-30T00:00:00Z"]).unwrap();
        connection.execute("INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,authority_level,created_at,updated_at) VALUES (?1,'semantic-p1',?2,'opening','{}','A',?3,?3)", params![format!("fact:semantic:{index}"), format!("entity:semantic:{index}"), "2026-08-30T00:00:00Z"]).unwrap();
        connection.execute("INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,'semantic-p1','volume:semantic',?2,?3,'NORMAL','PLANNED',0,?4,?4)", params![format!("chapter:semantic:{index}"), index, format!("Chapter {index}"), "2026-08-30T00:00:00Z"]).unwrap();
    }
    connection.execute("INSERT INTO project_runtime_state(project_id,signing_status,updated_at) VALUES ('semantic-p1','PASS',?1)", ["2026-08-30T00:00:00Z"]).unwrap();
    let signing_payload = json!({"status":"PASS","openingChapterRefs":["chapter:semantic:1","chapter:semantic:2","chapter:semantic:3"],"openingCanonRefs":["fact:semantic:1","fact:semantic:2","fact:semantic:3"]});
    let signing_hash = sha256_canonical(&signing_payload).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at) VALUES ('artifact:semantic:signing','semantic-p1','SimulatedSigningReceipt','PROJECT',?1,1,'m0a.artifact.v1','COMMITTED',?2,'{}',?3)", params!["{\"level\":\"PROJECT\"}", signing_hash, "2026-08-30T00:00:00Z"]).unwrap();
    connection.execute("INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES ('artifact:semantic:signing',1,?1,?2)", params![signing_payload.to_string(), signing_hash]).unwrap();
    for index in 1..=3 {
        connection.execute("INSERT INTO canon_evidence(evidence_id,project_id,fact_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,range_start,range_end,created_at) VALUES (?1,'semantic-p1',?2,?3,'artifact:semantic:signing',1,'OPENING','opening evidence',1,10,?4)", params![format!("evidence:semantic:{index}"), format!("fact:semantic:{index}"), format!("chapter:semantic:{index}"), "2026-08-30T00:00:00Z"]).unwrap();
    }
    drop(connection);
    let seed = json!({"payloadSchema":"book-plan.r1","planningMaturity":"SEED","storyPromise":"promise","protagonistMacroDirection":"direction","endingBoundary":"end","mustAchieve":[{"id":"goal:semantic","description":"goal"}],"mustNotBreak":[],"relationshipMacroDirections":[],"majorMysteryGoals":[],"futureReservations":[],"volumeIntentSeeds":[],"openingHypotheses":[],"globalLengthBudget":{"preferredWords":1000}});
    let result = execute_command_impl(
        &state,
        json!({"type":"planning_finalize_book_plan","commandId":"semantic:g3:planned","idempotencyKey":"semantic:g3:planned","projectId":"semantic-p1","expectedRevision":0,"requestedAt":"2026-08-30T00:00:00Z","payload":{"plan":seed,"signingStatus":"PASS","openingCanonRefs":["fact:semantic:1","fact:semantic:2","fact:semantic:3"],"signingEvidenceRefs":["artifact:semantic:signing"]}}),
    );
    assert_eq!(
        result.unwrap_err(),
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );
    println!("B07_SEMANTIC_NEGATIVE_TEST_RESULT case=G3-ACTUAL-01 planned_golden_three_rejected=PASS production_command=PASS status=PASS");
}
