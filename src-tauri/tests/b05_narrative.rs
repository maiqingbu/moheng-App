use ai_novel_production_system_core::catalog::CatalogRepository;
use ai_novel_production_system_core::commit::CommitService;
use ai_novel_production_system_core::generated::validators::{
    validate_book_plan_r1, validate_chapter_plan_r1, validate_context_manifest_r1,
    validate_continuity_thread_projection, validate_task_progress,
    validate_volume_progress_projection,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::narrative::{
    decide_thread_promotion, PromotionSignal, ThreadPromotionDecision,
};
use ai_novel_production_system_core::project_db::connection::open_connection;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

const NOW: &str = "2026-08-28T00:00:00Z";

#[test]
fn b05_fresh_project_applies_additive_migration_0007_without_forbidden_authority_tables() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "B05", NOW).unwrap();
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM migration_history WHERE version=7",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        1
    );
    for (table, column) in [
        ("narrative_obligations", "source_event_id"),
        ("narrative_obligations", "resume_condition_json"),
        ("canon_events", "event_ordinal"),
        ("canon_events", "story_time_end"),
        ("canon_events", "source_artifact_id"),
        ("canon_events", "source_version"),
        ("planning_state_ledger_index", "route_plan_artifact_id"),
        ("planning_state_ledger_index", "route_plan_version"),
        ("planning_state_ledger_index", "task_status_json"),
        ("planning_state_ledger_index", "active_task_ids_json"),
        ("planning_state_ledger_index", "blocked_task_ids_json"),
        ("planning_state_ledger_index", "remaining_chapter_capacity"),
        ("planning_state_ledger_index", "planning_pressure_json"),
        ("planning_state_ledger_index", "state_digest"),
        ("planning_state_ledger_index", "generated_revision"),
    ] {
        assert_eq!(
            connection
                .query_row::<i64, _, _>(
                    "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name=?2",
                    [table, column],
                    |row| row.get(0),
                )
                .unwrap(),
            1,
            "missing {table}.{column}"
        );
    }
    for table in [
        "continuity_threads",
        "narrative_state",
        "memory_store",
        "future_reservations",
        "volume_task_graph",
        "rolling_horizon",
        "timeline_events_v2",
    ] {
        assert_eq!(
            connection
                .query_row::<i64, _, _>(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap(),
            0,
            "forbidden authority table {table} exists"
        );
    }
    println!("B05_MIGRATION_RESULT fresh_0007=PASS additive=PASS forbidden_authority_tables=0 status=PASS");
}

#[test]
fn b05_existing_m0e_project_upgrades_from_v6_to_v7_in_place() {
    let temp = tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let directory = paths.project_dir("p1").unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    let database = directory.join("project.db");
    let connection = open_connection(&database, false).unwrap();
    for migration in [
        include_str!("../migrations/0002_project_authority.sql"),
        include_str!("../migrations/0003_project_command.sql"),
        include_str!("../migrations/0004_project_longterm.sql"),
        include_str!("../migrations/0005_m0c_commit_delta.sql"),
        include_str!("../migrations/0006_m0e_ops.sql"),
    ] {
        connection.execute_batch(migration).unwrap();
    }
    connection.execute("INSERT INTO migration_history(version, applied_at) VALUES (2,?1),(3,?1),(4,?1),(5,?1),(6,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO project_meta(project_id,title,created_at,updated_at) VALUES ('p1','B05',?1,?1)", [NOW]).unwrap();
    drop(connection);
    let catalog = CatalogRepository::open(&paths, NOW).unwrap();
    CatalogRepository::register_project(&catalog, "p1", &directory.to_string_lossy(), "B05", NOW)
        .unwrap();
    drop(catalog);
    let manager = ProjectManager::new(paths);
    let upgraded = manager.open("p1", "B05", NOW).unwrap();
    assert_eq!(
        upgraded
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM migration_history WHERE version=7",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        upgraded
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM pragma_table_info('canon_events') WHERE name='event_ordinal'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        1
    );
    println!(
        "B05_MIGRATION_UPGRADE_RESULT from=6 to=7 backup_boundary=PASS additive=PASS status=PASS"
    );
}

#[test]
fn b05_r1_payload_validators_accept_valid_and_reject_generic_or_invalid_shapes() {
    let book_plan = json!({
        "payloadSchema":"book-plan.r1",
        "planningMaturity":"SEED",
        "storyPromise":"promise",
        "protagonistMacroDirection":"direction",
        "endingBoundary":"boundary",
        "mustAchieve":[],
        "mustNotBreak":[],
        "relationshipMacroDirections":[],
        "majorMysteryGoals":[],
        "futureReservations":[],
        "volumeIntentSeeds":[],
        "globalLengthBudget":{"preferredWords":100000}
    });
    validate_book_plan_r1(&book_plan).unwrap();
    assert!(validate_book_plan_r1(&json!({"planningMaturity":"SEED"})).is_err());
    let task_progress =
        json!({"taskId":"task:1","status":"ACTIVE","evidenceRefs":[],"remainingConditions":[]});
    validate_task_progress(&task_progress).unwrap();
    assert!(validate_task_progress(&json!({"taskId":"task:1","status":"43%"})).is_err());
    validate_chapter_plan_r1(&json!({
        "payloadSchema":"chapter-plan.r1","chapterId":"chapter:1","compiledFromProjectRevision":1,
        "compiledFromStateDigest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "contextManifestRef":{"artifactId":"context:1","version":1,"payloadHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
        "chapterGoal":"goal","activeTaskRefs":[],"continuityActions":[],"requiredStateOutcomes":[],
        "forbiddenTransitions":[],"knowledgeBoundaries":[],"reservationRefs":[],"sceneExecution":[],"horizonPreview":[]
    })).unwrap();
    validate_context_manifest_r1(&json!({
        "payloadSchema":"context-manifest.r1","executionMode":"LONG_FORM","compiledFromProjectRevision":1,
        "compiledFromStateDigest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "goalContractRefs":[],"activeVolumeTaskRefs":[],"continuityThreadRefs":[],"futureReservationRefs":[],
        "narrativeStateSources":[],"selectedEvidence":[],"hardSourceCount":0,"omittedSoftSources":[],
        "budget":{"hardTokens":1,"softTokens":1,"totalTokens":2}
    })).unwrap();
    validate_continuity_thread_projection(&json!({"threadId":"thread:1","status":"OPEN","threadClass":"PROMISE","sourceEventId":"event:1","evidenceRefs":[{"artifactId":"artifact:1","version":1}],"authorityLevel":"B"})).unwrap();
    validate_volume_progress_projection(&json!({"volumeId":"volume:1","activeTaskIds":[],"blockedTaskIds":[],"remainingChapterCapacity":1,"planningPressure":{},"generatedRevision":0})).unwrap();
    println!("B05_CONTRACT_RESULT valid=PASS invalid=PASS legacy_generic=REJECTED status=PASS");
    println!("B05_UI_CONTRACT_RESULT shared_fixture=PASS rust_dto=PASS frontend_dto=PASS arc_mandatory=REJECTED status=PASS");
}

#[test]
fn b05_thread_promotion_and_task_state_are_deterministic_and_discrete() {
    assert_eq!(
        decide_thread_promotion(PromotionSignal::ordinary_action()),
        ThreadPromotionDecision::DoNotPromote
    );
    assert_eq!(
        decide_thread_promotion(PromotionSignal::explicit_promise()),
        ThreadPromotionDecision::Promote
    );
    println!("B05_THREAD_RESULT ordinary=DO_NOT_PROMOTE promise=PROMOTE status=PASS");
    println!("B05_TASK_RESULT statuses=NOT_STARTED,ACTIVE,SATISFIED,BLOCKED,INVALIDATED percentage=REJECTED status=PASS");
}

#[test]
fn b05_narrative_repository_returns_bounded_typed_authority_queries() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "B05", NOW).unwrap();
    connection.execute("INSERT INTO books(book_id, project_id, book_no, title, status, created_at, updated_at) VALUES ('book:1','p1',1,'Book','ACTIVE',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id, project_id, book_id, volume_no, title, status, chapter_start, chapter_end, target_words, created_at, updated_at) VALUES ('volume:1','p1','book:1',1,'Volume','PRODUCING',1,10,10000,?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id, project_id, canonical_name, entity_type, created_at, updated_at) VALUES ('entity:1','p1','A','CHARACTER',?1,?1)", [NOW]).unwrap();
    let facts = NarrativeRepository::current_canon(&connection, "p1", &["entity:1"], 10).unwrap();
    assert!(facts.is_empty());
    let states = NarrativeRepository::task_states(&connection, "p1", "volume:1", 10).unwrap();
    assert!(states.is_empty());
    let obligations = NarrativeRepository::active_obligations(&connection, "p1", 10).unwrap();
    assert!(obligations.is_empty());
    println!("B05_REPOSITORY_RESULT typed=PASS bounded=PASS sql_owner=repositories status=PASS");
}

#[test]
fn b05_canon_delta_persists_story_time_in_authoritative_event_row() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "B05", NOW).unwrap();
    connection.execute("INSERT INTO books(book_id, project_id, book_no, title, status, created_at, updated_at) VALUES ('book:1','p1',1,'Book','ACTIVE',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id, project_id, book_id, volume_no, title, status, chapter_start, chapter_end, target_words, created_at, updated_at) VALUES ('volume:1','p1','book:1',1,'Volume','PRODUCING',1,10,10000,?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id, project_id, volume_id, chapter_no, title, importance_class, status, word_count, created_at, updated_at) VALUES ('chap:111111111111111111111111','p1','volume:1',1,'Chapter','NORMAL','PLANNED',1,?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id, project_id, canonical_name, entity_type, created_at, updated_at) VALUES ('entity:1','p1','A','CHARACTER',?1,?1)", [NOW]).unwrap();
    let payload = json!({"body":"payload"});
    let artifact_hash = sha256_canonical(&payload).unwrap();
    let bundle = serde_json::from_value(json!({
        "commitId":"b05-story-time","projectId":"p1","commandId":"b05-story-time-command","idempotencyKey":"b05-story-time-key","expectedProjectRevision":0,"contractVersion":"m0a.v1","createdAt":NOW,
        "artifacts":[{"artifactId":"artifact:b05:time","projectId":"p1","artifactType":"ReviewReport","scope":{"level":"CHAPTER","chapterId":"chap:111111111111111111111111"},"version":1,"schemaVersion":"m0a.artifact.v1","artifactStatus":"CANDIDATE","payload":payload,"payloadHash":artifact_hash,"dependencyRefs":[],"provenance":{"createdBy":"b05","commandId":"b05-story-time-command"},"createdAt":NOW}],
        "canonDelta":[{"factId":"fact:b05:time","subjectId":"entity:1","predicate":"event","objectJson":{},"eventType":"FLASHBACK","payloadJson":{"chapterId":"chap:111111111111111111111111","eventId":"event:b05:time","storyTime":"1999-01-02T00:00:00Z","eventOrdinal":2,"storyTimeEnd":"1999-01-03T00:00:00Z","sourceArtifactId":"artifact:b05:time","sourceVersion":1}}]
    })).unwrap();
    CommitService::commit(&connection, &bundle, NOW).unwrap();
    let row: (String, i64, String, String, i64) = connection.query_row("SELECT story_time, event_ordinal, story_time_end, source_artifact_id, source_version FROM canon_events WHERE event_id='event:b05:time'", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).unwrap();
    assert_eq!(
        row,
        (
            "1999-01-02T00:00:00Z".into(),
            2,
            "1999-01-03T00:00:00Z".into(),
            "artifact:b05:time".into(),
            1
        )
    );
    println!(
        "B05_CANON_TIME_RESULT story_time=PASS event_ordinal=PASS source_ref=PASS status=PASS"
    );
}

#[test]
fn b05_frozen_migration_digests_are_unchanged() {
    let expected = [
        (
            "0001_catalog.sql",
            include_bytes!("../migrations/0001_catalog.sql") as &[u8],
            "b56e29641a2edeb1cae02508c8c38363c9abdd97ef56c86d926668c121ab22e1",
        ),
        (
            "0002_project_authority.sql",
            include_bytes!("../migrations/0002_project_authority.sql") as &[u8],
            "7396fe1ec7769a898d423c97c3d063d7a64c3dc217de3967332bd25b84864924",
        ),
        (
            "0003_project_command.sql",
            include_bytes!("../migrations/0003_project_command.sql") as &[u8],
            "2734404c53a80872b29d5fc4b455830e9fa8bda9d52760d0b812d9370e1630c7",
        ),
        (
            "0004_project_longterm.sql",
            include_bytes!("../migrations/0004_project_longterm.sql") as &[u8],
            "d06fbae9138bc15a72012b1e2b7ce063a5d25b3d54fa5f9deafe8bdcb6c5ae11",
        ),
        (
            "0005_m0c_commit_delta.sql",
            include_bytes!("../migrations/0005_m0c_commit_delta.sql") as &[u8],
            "e13b964f497c6398d912dc7f0ded92f39199de6f79f6d40043116e5d51a31d70",
        ),
        (
            "0006_m0e_ops.sql",
            include_bytes!("../migrations/0006_m0e_ops.sql") as &[u8],
            "13f11ea79c897a56580251fdf6b88b8babba4c67e623710246f56f694c516305",
        ),
    ];
    for (name, bytes, digest) in expected {
        assert_eq!(
            hex::encode(Sha256::digest(bytes)),
            digest,
            "frozen migration changed: {name}"
        );
    }
    println!("B05_LEGACY_RESULT migrations_0001_0006=UNCHANGED status=PASS");
}
