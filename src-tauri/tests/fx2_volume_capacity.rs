use ai_novel_production_system_core::commands::{
    query_plan_route_impl, query_volume_lifecycle_impl, query_volume_progress_impl, KernelState,
};
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::volume_lifecycle::VolumeLifecycleOwner;
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use tempfile::TempDir;

const NOW: &str = "2026-09-21T00:00:00Z";

struct Fx2Fixture {
    _temp: TempDir,
    state: KernelState,
    project_id: String,
    volume1_id: String,
    volume2_id: String,
    current_chapter_id: String,
}

fn insert_artifact(
    connection: &Connection,
    project_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    scope_key: &str,
    payload: &Value,
) -> String {
    let payload_hash = sha256_canonical(payload).unwrap();
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'m0a.artifact.v1','COMMITTED',?6,'{}',?7,?7)",
            libsql_rusqlite::params![artifact_id, project_id, artifact_type, if artifact_type == "ChapterPlan" || artifact_type == "FinalChapter" { "CHAPTER" } else if artifact_type == "BookPlan" || artifact_type == "MainContract" || artifact_type == "StoryConstitution" { "PROJECT" } else { "VOLUME" }, scope_key, payload_hash, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO artifact_payloads(artifact_id,version,payload_json,payload_hash) VALUES (?1,1,?2,?3)",
            libsql_rusqlite::params![artifact_id, payload.to_string(), payload_hash],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO canonical_heads(project_id,artifact_type,scope_level,scope_key,artifact_id,version,payload_hash) VALUES (?1,?2,?3,?4,?5,1,?6)",
            libsql_rusqlite::params![project_id, artifact_type, if artifact_type == "ChapterPlan" || artifact_type == "FinalChapter" { "CHAPTER" } else if artifact_type == "BookPlan" || artifact_type == "MainContract" || artifact_type == "StoryConstitution" { "PROJECT" } else { "VOLUME" }, scope_key, artifact_id, payload_hash],
        )
        .unwrap();
    payload_hash
}

fn seed_volume_contracts(
    connection: &Connection,
    project_id: &str,
    volume_id: &str,
    responsibility_refs: &[&str],
) -> (String, String) {
    let intent_id = format!("intent:{volume_id}");
    let intent_scope = json!({"level":"VOLUME","volumeId":volume_id}).to_string();
    let intent_hash = insert_artifact(
        connection,
        project_id,
        &intent_id,
        "VolumeIntent",
        &intent_scope,
        &json!({
            "payloadSchema":"volume-intent.r1",
            "volumeFunction":"推进本卷目标",
            "entryConditions":[],
            "mustAchieve":[],
            "mustNotBreak":[],
            "requiredExitConditions":[],
            "futureReservationRefs":[],
            "crossVolumeResponsibilityRefs":responsibility_refs,
            "targetChapterRange":{"min":1,"preferred":5,"max":10},
            "targetWordRange":{"preferred":1000}
        }),
    );
    let plan_id = format!("plan:{volume_id}");
    insert_artifact(
        connection,
        project_id,
        &plan_id,
        "VolumePlan",
        &intent_scope,
        &json!({
            "payloadSchema":"volume-plan.r1",
            "volumeIntentRef":{"artifactId":intent_id,"version":1,"payloadHash":intent_hash},
            "routeVersion":1,
            "tasks":[],
            "routeInvariants":[],
            "requiredExitConditions":[],
            "capacityBudget":{"minChapters":1,"preferredChapters":5,"maxChapters":10},
            "goalContractRefs":[],
            "compiledFromProjectRevision":0,
            "sourceEvidenceRefs":[],
            "semanticDigest":"0".repeat(64)
        }),
    );
    (intent_id, plan_id)
}

fn fx2_capacity_fixture(
    project_id: &str,
    committed_in_volume2: i64,
    include_deadline_thread: bool,
) -> Fx2Fixture {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state
        .manager
        .create(project_id, "FX-2 capacity", NOW)
        .unwrap();
    let book_id = format!("book:{project_id}");
    let volume1_id = format!("volume:{project_id}:1");
    let volume2_id = format!("volume:{project_id}:2");
    connection
        .execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'FX-2','ACTIVE',?3,?3)",
            libsql_rusqlite::params![book_id, project_id, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'第一卷','CLOSED',1,10,10000,?4,?4),(?5,?2,?3,2,'第二卷','PRODUCING',11,20,10000,?4,?4)",
            libsql_rusqlite::params![volume1_id, project_id, book_id, NOW, volume2_id],
        )
        .unwrap();
    insert_artifact(
        &connection,
        project_id,
        &format!("book-plan:{project_id}"),
        "BookPlan",
        &json!({"level":"PROJECT"}).to_string(),
        &json!({"payloadSchema":"book-plan.r1","planningMaturity":"APPROVED","mustAchieve":[],"mustNotBreak":[],"futureReservations":[]}),
    );
    for (volume_no, volume_id) in [(1_i64, &volume1_id), (2_i64, &volume2_id)] {
        seed_volume_contracts(
            &connection,
            project_id,
            volume_id,
            if include_deadline_thread && volume_no == 2 {
                &["thread:fx2:deadline"]
            } else {
                &[]
            },
        );
    }
    let current_chapter_no = 11 + committed_in_volume2;
    for chapter_no in 1_i64..=20 {
        let volume_id = if chapter_no <= 10 {
            &volume1_id
        } else {
            &volume2_id
        };
        let committed = chapter_no <= 10 || chapter_no < 11 + committed_in_volume2;
        let chapter_id = format!("chapter:{project_id}:{chapter_no}");
        connection
            .execute(
                "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'NORMAL',?6,?7,?8,?8)",
                libsql_rusqlite::params![chapter_id, project_id, volume_id, chapter_no, format!("第{chapter_no}章"), if committed { "COMMITTED" } else { "PLANNED" }, if committed { 100 } else { 0 }, NOW],
            )
            .unwrap();
        if committed {
            let final_id = format!("final:{project_id}:{chapter_no}");
            let scope = json!({"level":"CHAPTER","chapterId":chapter_id}).to_string();
            insert_artifact(
                &connection,
                project_id,
                &final_id,
                "FinalChapter",
                &scope,
                &json!({"payloadSchema":"final-chapter.r1","chapterId":chapter_id,"body":"已提交正文","wordCount":5}),
            );
            connection
                .execute(
                    "UPDATE chapters SET final_artifact_id=?1,final_version=1 WHERE project_id=?2 AND chapter_id=?3",
                    libsql_rusqlite::params![final_id, project_id, chapter_id],
                )
                .unwrap();
        }
    }
    let current_chapter_id = format!("chapter:{project_id}:{current_chapter_no}");
    let chapter_plan_id = format!("chapter-plan:{project_id}:{current_chapter_no}");
    let chapter_scope = json!({"level":"CHAPTER","chapterId":current_chapter_id}).to_string();
    insert_artifact(
        &connection,
        project_id,
        &chapter_plan_id,
        "ChapterPlan",
        &chapter_scope,
        &json!({"payloadSchema":"chapter-plan.r1","chapterId":current_chapter_id,"horizonPreview":[]}),
    );
    if include_deadline_thread {
        let intent_id = format!("intent:{volume2_id}");
        connection
            .execute(
                "INSERT INTO narrative_obligations(obligation_id,project_id,scope,obligation_type,authority_level,source_artifact_id,source_version,description,must_resolve_before_chapter,prerequisites_json,lifecycle,last_touched_chapter_id,resume_condition_json,created_at,updated_at) VALUES ('fx2:deadline',?1,'BOOK','PROMISE','B',?2,1,'第十二章前需兑现','12','[]','PROGRESSING',?3,'{}',?4,?4)",
                libsql_rusqlite::params![project_id, intent_id, format!("chapter:{project_id}:10"), NOW],
            )
            .unwrap();
    }
    Fx2Fixture {
        _temp: temp,
        state,
        project_id: project_id.into(),
        volume1_id,
        volume2_id,
        current_chapter_id,
    }
}

#[test]
fn fx2_second_volume_capacity_is_volume_relative() {
    let fixture = fx2_capacity_fixture("p-fx2-capacity", 3, false);
    let connection = fixture
        .state
        .manager
        .open(&fixture.project_id, "FX-2 capacity", NOW)
        .unwrap();
    let first_volume_capacity = ai_novel_production_system_core::planning::volume_capacity_context(
        &connection,
        &fixture.project_id,
        &fixture.volume1_id,
    )
    .unwrap();
    assert_eq!(first_volume_capacity.remaining_chapter_capacity, 0);
    assert_eq!(first_volume_capacity.current_chapter_no, 11);
    assert!(first_volume_capacity.current_chapter_id.is_none());
    let rebuilt = VolumeLifecycleOwner::rebuild_planning_state(
        &connection,
        &fixture.project_id,
        &fixture.volume2_id,
        NOW,
    )
    .unwrap();
    assert_eq!(rebuilt["remainingChapterCapacity"], 7);
    drop(connection);

    let request =
        json!({"projectId":fixture.project_id,"volumeId":fixture.volume2_id,"currentChapter":14});
    let progress = query_volume_progress_impl(&fixture.state, request.clone()).unwrap();
    let route = query_plan_route_impl(&fixture.state, request).unwrap();
    let lifecycle = query_volume_lifecycle_impl(
        &fixture.state,
        json!({"projectId":fixture.project_id,"volumeId":fixture.volume2_id}),
    )
    .unwrap();

    assert_eq!(progress["remainingChapterCapacity"], 7);
    assert_eq!(route["planningPressure"]["remainingChapterCapacity"], 7);
    assert_eq!(lifecycle["planningState"]["remainingChapterCapacity"], 7);
    assert_eq!(
        lifecycle["route"]["planningPressure"]["remainingChapterCapacity"],
        7
    );
}

#[test]
fn fx2_request_current_chapter_is_ignored() {
    let fixture = fx2_capacity_fixture("p-fx2-ignore-request", 3, false);
    let chapter_plan_id = format!("chapter-plan:{}:14", fixture.project_id);
    let mut observations = Vec::new();
    for request in [
        json!({"projectId":fixture.project_id,"volumeId":fixture.volume2_id,"currentChapter":1}),
        json!({"projectId":fixture.project_id,"volumeId":fixture.volume2_id,"currentChapter":999}),
        json!({"projectId":fixture.project_id,"volumeId":fixture.volume2_id}),
    ] {
        let progress = query_volume_progress_impl(&fixture.state, request.clone()).unwrap();
        let route = query_plan_route_impl(&fixture.state, request).unwrap();
        observations.push(json!({
            "capacity":progress["remainingChapterCapacity"],
            "progressPressure":progress["planningPressure"],
            "routePressure":route["planningPressure"],
            "currentChapterPlan":route["currentChapterPlan"]
        }));
    }

    assert_eq!(observations[0], observations[1]);
    assert_eq!(observations[1], observations[2]);
    assert_eq!(observations[0]["capacity"], 7);
    assert_eq!(
        observations[0]["currentChapterPlan"]["artifactId"],
        chapter_plan_id
    );
    assert_eq!(observations[0]["currentChapterPlan"]["version"], 1);
    assert_eq!(
        fixture.current_chapter_id,
        format!("chapter:{}:14", fixture.project_id)
    );
    assert_eq!(
        fixture.volume1_id,
        format!("volume:{}:1", fixture.project_id)
    );
}

#[test]
fn fx2_overdue_uses_global_chapter_numbers() {
    let fixture = fx2_capacity_fixture("p-fx2-global-overdue", 2, true);
    let progress = query_volume_progress_impl(
        &fixture.state,
        json!({"projectId":fixture.project_id,"volumeId":fixture.volume2_id,"currentChapter":2}),
    )
    .unwrap();

    assert_eq!(progress["planningPressure"]["overdueThreadPressure"], 1);
    assert_eq!(progress["planningPressure"]["continuityAging"], 3);
    assert_eq!(
        fixture.current_chapter_id,
        format!("chapter:{}:13", fixture.project_id)
    );
}
