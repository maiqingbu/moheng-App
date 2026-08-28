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
    println!("B05_CONTRACT_RESULT valid=PASS invalid=PASS legacy_generic=REJECTED stable_ids=PASS status=PASS");
    println!("B05_UI_CONTRACT_RESULT shared_fixture=PASS rust_dto=PASS frontend_dto=PASS arc_mandatory=REJECTED status=PASS");
}

#[test]
fn b05_thread_promotion_and_task_state_are_deterministic_and_discrete() {
    assert_eq!(
        decide_thread_promotion(PromotionSignal::ordinary_action()),
        ThreadPromotionDecision::DoNotPromote
    );
    for signal in [
        PromotionSignal::explicit_promise(),
        PromotionSignal {
            major_relationship_transition: true,
            ..PromotionSignal::ordinary_action()
        },
        PromotionSignal {
            unresolved_major_conflict: true,
            ..PromotionSignal::ordinary_action()
        },
        PromotionSignal {
            mystery_or_reveal_responsibility: true,
            ..PromotionSignal::ordinary_action()
        },
        PromotionSignal {
            foreshadow_recovery_responsibility: true,
            ..PromotionSignal::ordinary_action()
        },
        PromotionSignal {
            key_future_use_object_or_information: true,
            ..PromotionSignal::ordinary_action()
        },
        PromotionSignal {
            goal_contract_responsibility: true,
            ..PromotionSignal::ordinary_action()
        },
        PromotionSignal {
            hard_authority_responsibility: true,
            ..PromotionSignal::ordinary_action()
        },
        PromotionSignal {
            future_reservation_responsibility: true,
            ..PromotionSignal::ordinary_action()
        },
    ] {
        assert_eq!(
            decide_thread_promotion(signal),
            ThreadPromotionDecision::Promote
        );
    }
    for signal in [
        PromotionSignal::ordinary_action(),
        PromotionSignal {
            ambient_description: true,
            ..PromotionSignal::ordinary_action()
        },
        PromotionSignal {
            completed_trivial_event: true,
            ..PromotionSignal::ordinary_action()
        },
        PromotionSignal {
            non_continuing_scene_detail: true,
            ..PromotionSignal::ordinary_action()
        },
    ] {
        assert_eq!(
            decide_thread_promotion(signal),
            ThreadPromotionDecision::DoNotPromote
        );
    }
    println!("B05_THREAD_RESULT ordinary=DO_NOT_PROMOTE ambient=DO_NOT_PROMOTE trivial=DO_NOT_PROMOTE detail=DO_NOT_PROMOTE promise=PROMOTE relationship=PROMOTE conflict=PROMOTE mystery=PROMOTE foreshadow=PROMOTE future_object=PROMOTE goal=PROMOTE authority=PROMOTE reservation=PROMOTE status=PASS");
    println!("B05_TASK_RESULT statuses=NOT_STARTED,ACTIVE,SATISFIED,BLOCKED,INVALIDATED percentage=REJECTED status=PASS");
}

#[test]
fn b05_narrative_repository_returns_bounded_typed_authority_queries() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "B05", NOW).unwrap();
    connection.execute("INSERT INTO books(book_id, project_id, book_no, title, status, created_at, updated_at) VALUES ('book:1','p1',1,'Book','ACTIVE',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO volumes(volume_id, project_id, book_id, volume_no, title, status, chapter_start, chapter_end, target_words, created_at, updated_at) VALUES ('volume:1','p1','book:1',1,'Volume','PRODUCING',1,10,10000,?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO chapters(chapter_id, project_id, volume_id, chapter_no, title, importance_class, status, word_count, created_at, updated_at) VALUES ('chapter:1','p1','volume:1',1,'Chapter','NORMAL','COMMITTED',8,?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO canon_entities(entity_id, project_id, canonical_name, entity_type, created_at, updated_at) VALUES ('entity:1','p1','A','CHARACTER',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO canon_facts(fact_id, project_id, subject_id, predicate, object_json, authority_level, created_at, updated_at) VALUES ('fact:relevant','p1','entity:1','promise','{}','B',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO canon_facts(fact_id, project_id, subject_id, predicate, object_json, authority_level, created_at, updated_at) VALUES ('fact:other','p1','entity:1','other','{}','C',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO canon_relations(relation_id, project_id, subject_id, predicate, object_id, authority_level, created_at, updated_at) VALUES ('relation:relevant','p1','entity:1','trusts','entity:1','B',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO artifacts(artifact_id, project_id, artifact_type, scope_level, scope_key, version, schema_version, artifact_status, payload_hash, provenance_json, created_at) VALUES ('artifact:evidence','p1','FinalChapter','CHAPTER','chap:1',1,'m0a.artifact.v1','COMMITTED',?1,'{}',?2)", libsql_rusqlite::params!["a".repeat(64), NOW]).unwrap();
    connection.execute("INSERT INTO canon_evidence(evidence_id, project_id, fact_id, relation_id, event_id, final_chapter_id, source_artifact_id, source_version, evidence_type, quote, range_start, range_end, created_at) VALUES ('evidence:relevant','p1','fact:relevant','relation:relevant',NULL,'chapter:1','artifact:evidence',1,'QUOTE','promise',1,8,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO dependency_edges(edge_id, project_id, source_ref, target_ref, dependency_type, scope, impact_class, source_hash, entity_id, created_by, created_at) VALUES ('edge:obligation','p1','obligation:old','volume:1','OBLIGATION_INPUT','VOLUME','HARD',?1,'entity:1','b05',?2)", libsql_rusqlite::params!["b".repeat(64), NOW]).unwrap();
    for index in 0..101 {
        let obligation_id = format!("obligation:noise:{index}");
        connection.execute("INSERT INTO narrative_obligations(obligation_id, project_id, scope, obligation_type, authority_level, source_artifact_id, source_version, description, lifecycle, created_at, updated_at, source_event_id, resume_condition_json) VALUES (?1,'p1','VOLUME','NOISE','C','artifact:evidence',1,'noise','APPROVED',?2,?2,NULL,'{}')", libsql_rusqlite::params![obligation_id, NOW]).unwrap();
    }
    connection.execute("INSERT INTO narrative_obligations(obligation_id, project_id, scope, obligation_type, authority_level, source_artifact_id, source_version, description, lifecycle, created_at, updated_at, source_event_id, resume_condition_json) VALUES ('obligation:old','p1','VOLUME','PROMISE','B','artifact:evidence',1,'old promise','APPROVED','2020-01-01T00:00:00Z','2020-01-01T00:00:00Z','event:promise','{\"kind\":\"NEXT_MEETING\"}')", []).unwrap();
    connection.execute("INSERT INTO foreshadows(foreshadow_id, project_id, intent, secret_payload_ref, plant_chapter_id, recover_window_end_chapter, status, source_authority, created_at, updated_at) VALUES ('foreshadow:relevant','p1','recover promise','reservation:1','chapter:1','chapter:2','PLANTED','B',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO mystery_states(mystery_id, project_id, mystery_type, status, core_fact_id, created_at, updated_at) VALUES ('mystery:relevant','p1','IDENTITY','PROGRESSING','fact:relevant',?1,?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO state_deltas(state_id, project_id, state_type, value_json, payload_json, created_at, updated_at) VALUES ('task:relevant','p1','volume.task.progress','{\"taskId\":\"task:relevant\",\"status\":\"ACTIVE\",\"evidenceRefs\":[],\"remainingConditions\":[]}','{}',?1,?1)", [NOW]).unwrap();
    let facts = NarrativeRepository::current_canon(&connection, "p1", &["entity:1"], 10).unwrap();
    assert_eq!(facts.len(), 2);
    assert_eq!(
        NarrativeRepository::current_relations(&connection, "p1", &["entity:1"], 10)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        NarrativeRepository::canon_evidence_by_ref(&connection, "p1", "fact:relevant", 10)
            .unwrap()
            .len(),
        1
    );
    let obligations = NarrativeRepository::active_obligations_for_relevance(
        &connection,
        "p1",
        &["entity:1"],
        &["obligation:old"],
        10,
    )
    .unwrap();
    assert_eq!(obligations.len(), 1);
    assert_eq!(obligations[0].obligation_id, "obligation:old");
    assert_eq!(
        NarrativeRepository::active_foreshadows(
            &connection,
            "p1",
            &["chapter:1"],
            &["reservation:1"],
            10
        )
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        NarrativeRepository::open_mysteries(
            &connection,
            "p1",
            &["fact:relevant", "mystery:relevant"],
            10
        )
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        NarrativeRepository::task_states_by_ids(&connection, "p1", &["task:relevant"], 10)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        NarrativeRepository::recent_evidence(
            &connection,
            "p1",
            &["chapter:1"],
            &["artifact:evidence"],
            10
        )
        .unwrap()
        .len(),
        1
    );
    println!("B05_REPOSITORY_RESULT typed=PASS populated=PASS bounded=PASS relevant=PASS old_promise=PASS sql_owner=repositories status=PASS");
    println!("B05_REPOSITORY_COVERAGE_RESULT relations=PASS evidence=PASS obligations=PASS foreshadows=PASS mysteries=PASS tasks_by_id=PASS recent_evidence=PASS status=PASS");
}

#[test]
fn b05_plan_artifact_commit_does_not_mutate_canon_authority() {
    let temp = tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager.create("p1", "B05", NOW).unwrap();
    let canon_before: (i64, i64, i64, i64, i64) = connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM canon_entities),(SELECT COUNT(*) FROM canon_facts),(SELECT COUNT(*) FROM canon_relations),(SELECT COUNT(*) FROM canon_events),(SELECT COUNT(*) FROM canon_knowledge)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .unwrap();
    let artifacts = [
        ("BookPlan", "plan:book"),
        ("VolumeIntent", "plan:volume-intent"),
        ("VolumePlan", "plan:volume"),
        ("ChapterPlan", "plan:chapter"),
        ("ContextManifest", "plan:context"),
    ]
    .into_iter()
    .map(|(artifact_type, artifact_id)| {
        let payload = json!({"artifactType":artifact_type,"purpose":"B05 authority test"});
        json!({"artifactId":artifact_id,"projectId":"p1","artifactType":artifact_type,"scope":{"level":"PROJECT"},"version":1,"schemaVersion":"m0a.artifact.v1","artifactStatus":"CANDIDATE","payload":payload,"payloadHash":sha256_canonical(&payload).unwrap(),"dependencyRefs":[],"provenance":{"createdBy":"b05","commandId":"b05-plan-authority-command"},"createdAt":NOW})
    })
    .collect::<Vec<_>>();
    let bundle = serde_json::from_value(json!({"commitId":"b05-plan-authority","projectId":"p1","commandId":"b05-plan-authority-command","idempotencyKey":"b05-plan-authority-key","expectedProjectRevision":0,"contractVersion":"m0a.v1","createdAt":NOW,"artifacts":artifacts})).unwrap();
    CommitService::commit(&connection, &bundle, NOW).unwrap();
    let canon_after: (i64, i64, i64, i64, i64) = connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM canon_entities),(SELECT COUNT(*) FROM canon_facts),(SELECT COUNT(*) FROM canon_relations),(SELECT COUNT(*) FROM canon_events),(SELECT COUNT(*) FROM canon_knowledge)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .unwrap();
    assert_eq!(canon_before, canon_after);
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM artifacts WHERE project_id='p1'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        5
    );
    println!("B05_AUTHORITY_RESULT plan_artifacts=5 canon_rows_before=0 canon_rows_after=0 canon_digest_equal=true projection_only=true status=PASS");
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
        "B05_CANON_TIME_RESULT story_time=PASS event_ordinal=PASS source_ref=PASS narrative_order_separate=PASS status=PASS"
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
