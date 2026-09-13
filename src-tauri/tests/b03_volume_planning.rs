use ai_novel_production_system_core::commands::{
    execute_command_impl, execute_volume_planning_with_observer, query_volume_detail_impl,
    KernelState,
};
use ai_novel_production_system_core::generated::types::ArtifactRef;
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::planning::validate_volume_plan_semantics;
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::repositories::narrative::NarrativeRepository;
use libsql_rusqlite::Connection;
use serde_json::{json, Value};

const NOW: &str = "2026-09-13T00:00:00Z";
const PROJECT: &str = "b03-project";
const BOOK: &str = "bk:333333333333333333333333";
const VOLUME: &str = "vol:111111111111111111111111";
const CHAPTER: &str = "chap:222222222222222222222222";
const PROJECT_SCOPE: &str = "{\"level\":\"PROJECT\"}";
const VOLUME_SCOPE: &str = "{\"level\":\"VOLUME\",\"volumeId\":\"vol:111111111111111111111111\"}";

fn insert_artifact(
    connection: &Connection,
    artifact_id: &str,
    artifact_type: &str,
    scope_level: &str,
    scope_key: &str,
    payload: &Value,
) -> ArtifactRef {
    let payload_hash = sha256_canonical(payload).unwrap();
    connection
        .execute(
            "INSERT INTO artifacts(artifact_id,project_id,artifact_type,scope_level,scope_key,version,schema_version,artifact_status,payload_hash,provenance_json,created_at,committed_at) VALUES (?1,?2,?3,?4,?5,1,'m0a.artifact.v1','COMMITTED',?6,'{}',?7,?7)",
            libsql_rusqlite::params![artifact_id, PROJECT, artifact_type, scope_level, scope_key, payload_hash, NOW],
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
            libsql_rusqlite::params![PROJECT, artifact_type, scope_level, scope_key, artifact_id, payload_hash],
        )
        .unwrap();
    ArtifactRef {
        artifact_id: artifact_id.into(),
        version: 1,
        payload_hash,
    }
}

fn book_plan(maturity: &str) -> Value {
    json!({
        "payloadSchema":"book-plan.r1",
        "planningMaturity":maturity,
        "storyPromise":"查明账本缺页与失踪证人的关系",
        "protagonistMacroDirection":"林舟从被动调查转为主动追查",
        "endingBoundary":"第一卷只解决账本来源，不揭示幕后全貌",
        "mustAchieve":[{"id":"goal:ledger","description":"查明账本缺页的直接来源"}],
        "mustNotBreak":[{"id":"rule:knowledge","description":"不得让顾清凭空知道账本内容"}],
        "relationshipMacroDirections":[{"id":"relation:trust","description":"林舟与顾清的互不信任必须有证据变化"}],
        "majorMysteryGoals":[{"id":"mystery:ledger","description":"追查被夺账本的去向"}],
        "futureReservations":[{"reservationId":"reservation:ledger-source","description":"保留账本真正来源","reservedForVolumeNo":1,"notBeforeVolumeNo":1,"prerequisites":["fact:b03:ledger"],"mayForeshadow":true,"mayReveal":false,"authorityLevel":"A"}],
        "volumeIntentSeeds":[{"seedId":"seed:volume-1","description":"第一卷围绕账本缺页推进查证"}],
        "openingHypotheses":[],
        "globalLengthBudget":{"preferredWords":120000}
    })
}

fn seed_project() -> (tempfile::TempDir, KernelState) {
    let temp = tempfile::tempdir().unwrap();
    let state = KernelState::new(AppPaths::new(temp.path()));
    let connection = state.manager.create(PROJECT, "B03 semantic", NOW).unwrap();
    connection
        .execute(
            "INSERT INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES (?1,?2,1,'B03 Book','ACTIVE',?3,?3)",
            libsql_rusqlite::params![BOOK, PROJECT, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES (?1,?2,?3,1,'第一卷','PLANNING',1,12,12000,?4,?4)",
            libsql_rusqlite::params![VOLUME, PROJECT, BOOK, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES (?1,?2,?3,1,'开篇','IMPORTANT','COMMITTED',1200,?4,?4)",
            libsql_rusqlite::params![CHAPTER, PROJECT, VOLUME, NOW],
        )
        .unwrap();
    for (id, name) in [
        ("entity:b03:linzhou", "林舟"),
        ("entity:b03:guqing", "顾清"),
    ] {
        connection
            .execute(
                "INSERT INTO canon_entities(entity_id,project_id,canonical_name,entity_type,first_chapter_id,created_at,updated_at) VALUES (?1,?2,?3,'CHARACTER',?4,?5,?5)",
                libsql_rusqlite::params![id, PROJECT, name, CHAPTER, NOW],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO canon_facts(fact_id,project_id,subject_id,predicate,object_json,valid_from_chapter_id,source_artifact_id,source_version,source_chapter_id,evidence_range_json,authority_level,created_at,updated_at) VALUES ('fact:b03:ledger',?1,'entity:b03:linzhou','持有','{\"text\":\"账本缺页\"}',?2,'book-plan:b03',1,?2,'{\"start\":1,\"end\":20}','A',?3,?3)",
            libsql_rusqlite::params![PROJECT, CHAPTER, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO canon_relations(relation_id,project_id,subject_id,predicate,object_id,valid_from_chapter_id,source_artifact_id,source_version,authority_level,created_at,updated_at) VALUES ('relation:b03:trust',?1,'entity:b03:linzhou','互不信任','entity:b03:guqing',?2,'book-plan:b03',1,'A',?3,?3)",
            libsql_rusqlite::params![PROJECT, CHAPTER, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO canon_knowledge(knowledge_id,project_id,knower_id,fact_id,epistemic_state,visibility_scope,from_chapter_id,source_artifact_id,source_version,created_at,updated_at) VALUES ('knowledge:b03:ledger',?1,'entity:b03:linzhou','fact:b03:ledger','KNOWS','CHARACTER:entity:b03:linzhou',?2,'book-plan:b03',1,?3,?3)",
            libsql_rusqlite::params![PROJECT, CHAPTER, NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO canon_events(event_id,project_id,chapter_id,event_type,subject_id,object_json,story_time,payload_json,created_at) VALUES ('event:b03:stolen',?1,?2,'LEDGER_STOLEN','entity:b03:guqing','{\"text\":\"账本被顾清夺走\"}','T3','{\"cause\":\"追债\"}',?3)",
            libsql_rusqlite::params![PROJECT, CHAPTER, NOW],
        )
        .unwrap();
    let constitution = json!({"payloadSchema":"story-constitution.r1","storyCore":{"protagonist":"林舟","desire":"查明账本缺页","mechanism":"证据会改变关系","conflict":"顾清试图掩盖账本来源","readerPromise":"每次查证都付出关系代价","forbiddenReveals":["第一卷不得揭示幕后全貌"]}});
    let main = json!({"payloadSchema":"main-contract.r1","scope":"BOOK","hardRules":["主角必须主动选择","不得违反已知知识边界"],"softPreferences":[]});
    insert_artifact(
        &connection,
        "story-constitution:b03",
        "StoryConstitution",
        "PROJECT",
        PROJECT_SCOPE,
        &constitution,
    );
    insert_artifact(
        &connection,
        "main-contract:b03",
        "MainContract",
        "PROJECT",
        PROJECT_SCOPE,
        &main,
    );
    let plan = book_plan("APPROVED");
    insert_artifact(
        &connection,
        "book-plan:b03",
        "BookPlan",
        "PROJECT",
        PROJECT_SCOPE,
        &plan,
    );
    let evidence_rows = [
        (
            "evidence:b03:fact",
            "fact:b03:ledger",
            "",
            "",
            "账本缺页",
            1,
            20,
        ),
        (
            "evidence:b03:relation",
            "",
            "relation:b03:trust",
            "",
            "林舟不信任顾清",
            21,
            40,
        ),
        (
            "evidence:b03:event",
            "",
            "",
            "event:b03:stolen",
            "账本被顾清夺走",
            41,
            60,
        ),
    ];
    for (evidence_id, fact_id, relation_id, event_id, quote, start, end) in evidence_rows {
        connection.execute("INSERT INTO canon_evidence(evidence_id,project_id,fact_id,relation_id,event_id,final_chapter_id,source_artifact_id,source_version,evidence_type,quote,range_start,range_end,created_at) VALUES (?1,?2,?3,?4,?5,?6,'book-plan:b03',1,'OPENING',?7,?8,?9,?10)", libsql_rusqlite::params![evidence_id, PROJECT, if fact_id.is_empty(){None::<String>}else{Some(fact_id.into())}, if relation_id.is_empty(){None::<String>}else{Some(relation_id.into())}, if event_id.is_empty(){None::<String>}else{Some(event_id.into())}, CHAPTER, quote, start, end, NOW]).unwrap();
    }
    (temp, state)
}

fn request(command_type: &str, command_id: &str, expected_revision: i64) -> Value {
    json!({"type":command_type,"commandId":command_id,"idempotencyKey":format!("{command_id}:key"),"projectId":PROJECT,"expectedRevision":expected_revision,"requestedAt":NOW,"payload":{"volumeId":VOLUME}})
}

fn payload_for(state: &KernelState, artifact_type: &str) -> Value {
    let connection = state.manager.open(PROJECT, "", NOW).unwrap();
    let reference =
        NarrativeRepository::canonical_head_ref(&connection, PROJECT, artifact_type, VOLUME)
            .unwrap()
            .unwrap();
    NarrativeRepository::artifact_payload_by_ref(&connection, PROJECT, &reference)
        .unwrap()
        .unwrap()
}

fn setup_intent(state: &KernelState) -> Value {
    execute_command_impl(
        state,
        request("planning_create_volume_intent", "b03-intent", 0),
    )
    .unwrap()
}

#[test]
fn b03_volume_intent_compiles_non_empty_semantics_from_approved_authority() {
    let (_temp, state) = seed_project();
    let result = setup_intent(&state);
    assert_eq!(result["finalStatus"], "committed");
    let intent = payload_for(&state, "VolumeIntent");
    assert!(!intent["mustAchieve"].as_array().unwrap().is_empty());
    assert!(!intent["mustNotBreak"].as_array().unwrap().is_empty());
    assert!(!intent["requiredExitConditions"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(intent["sourceBookPlanRef"]["artifactId"], "book-plan:b03");
    assert!(intent["sourceCanonRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "fact:b03:ledger"));
    assert!(intent["sourceEvidenceRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "evidence:b03:event"));
    let semantic = serde_json::to_string(&intent).unwrap();
    assert!(semantic.contains("账本缺页"));
    assert!(semantic.contains("互不信任"));
    assert!(semantic.contains("账本被顾清夺走"));
}

#[test]
fn b03_goal_contract_fails_closed_for_each_missing_mandatory_authority() {
    for (missing, expected) in [
        (
            "StoryConstitution",
            "GOAL_CONTRACT_STORY_CONSTITUTION_REQUIRED",
        ),
        ("MainContract", "GOAL_CONTRACT_MAIN_CONTRACT_REQUIRED"),
        ("BookPlan", "GOAL_CONTRACT_BOOK_PLAN_APPROVED_REQUIRED"),
        ("VolumeIntent", "GOAL_CONTRACT_VOLUME_INTENT_REQUIRED"),
    ] {
        let (_temp, state) = seed_project();
        if missing == "VolumeIntent" {
            let connection = state.manager.open(PROJECT, "", NOW).unwrap();
            let intent = json!({"payloadSchema":"volume-intent.r1","volumeFunction":"测试卷","entryConditions":[{"kind":"VOLUME_EXISTS","value":VOLUME}],"mustAchieve":[{"id":"goal:test","description":"测试目标"}],"mustNotBreak":[{"id":"rule:test","description":"测试约束"}],"requiredExitConditions":[{"kind":"GOAL_ACHIEVED","value":"goal:test"}],"futureReservationRefs":[],"crossVolumeResponsibilityRefs":[],"targetChapterRange":{"min":1,"preferred":12,"max":12},"targetWordRange":{"preferred":12000}});
            insert_artifact(
                &connection,
                "volume-intent:b03",
                "VolumeIntent",
                "VOLUME",
                VOLUME_SCOPE,
                &intent,
            );
        } else {
            setup_intent(&state);
            let connection = state.manager.open(PROJECT, "", NOW).unwrap();
            connection
                .execute(
                    "DELETE FROM canonical_heads WHERE project_id=?1 AND artifact_type=?2",
                    libsql_rusqlite::params![PROJECT, missing],
                )
                .unwrap();
        }
        let result = execute_command_impl(
            &state,
            request(
                "planning_create_volume_plan",
                &format!("b03-plan-missing-{missing}"),
                1,
            ),
        );
        assert_eq!(result.unwrap_err(), expected, "missing {missing}");
    }
}

#[test]
fn b03_goal_contract_rejects_book_plan_seed_maturity() {
    let (_temp, state) = seed_project();
    setup_intent(&state);
    let connection = state.manager.open(PROJECT, "", NOW).unwrap();
    connection
        .execute(
            "DELETE FROM canonical_heads WHERE project_id=?1 AND artifact_type='BookPlan'",
            [PROJECT],
        )
        .unwrap();
    let seed = book_plan("SEED");
    insert_artifact(
        &connection,
        "book-plan:b03-seed",
        "BookPlan",
        "PROJECT",
        PROJECT_SCOPE,
        &seed,
    );
    drop(connection);
    let result = execute_command_impl(
        &state,
        request("planning_create_volume_plan", "b03-plan-seed", 1),
    );
    assert_eq!(
        result.unwrap_err(),
        "GOAL_CONTRACT_BOOK_PLAN_APPROVED_REQUIRED"
    );
}

#[test]
fn b03_volume_plan_compiles_task_route_with_dependencies_predicates_and_capacity() {
    let (_temp, state) = seed_project();
    setup_intent(&state);
    let before = state.manager.open(PROJECT, "", NOW).unwrap();
    let canon_before: i64 = before
        .query_row("SELECT COUNT(*) FROM canon_facts", [], |row| row.get(0))
        .unwrap();
    drop(before);
    let result = execute_command_impl(
        &state,
        request("planning_create_volume_plan", "b03-plan", 1),
    )
    .unwrap();
    assert_eq!(result["finalStatus"], "committed");
    let route = payload_for(&state, "VolumePlan");
    let tasks = route["tasks"].as_array().unwrap();
    assert!(!tasks.is_empty());
    assert!(tasks.iter().any(|task| task["class"] == "REQUIRED"));
    assert!(!route["routeInvariants"].as_array().unwrap().is_empty());
    assert!(!route["requiredExitConditions"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(route["capacityBudget"]["minChapters"].as_i64().unwrap() > 0);
    assert!(
        route["capacityBudget"]["preferredChapters"]
            .as_i64()
            .unwrap()
            > 0
    );
    assert!(
        route["capacityBudget"]["maxChapters"].as_i64().unwrap()
            >= route["capacityBudget"]["preferredChapters"]
                .as_i64()
                .unwrap()
    );
    let ids = tasks
        .iter()
        .map(|task| task["taskId"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    for task in tasks {
        assert!(!task["objective"].as_str().unwrap().is_empty());
        assert!(!task["completionPredicate"]["kind"]
            .as_str()
            .unwrap()
            .is_empty());
        for dependency in task["dependsOnTaskIds"].as_array().unwrap() {
            assert!(ids.contains(dependency.as_str().unwrap()));
        }
        assert!(task.get("status").is_none());
    }
    assert!(tasks.iter().any(|task| {
        task["completionPredicate"]["kind"] == "CANON_EVIDENCE_PRESENT"
            && ["fact:b03:ledger", "event:b03:stolen", "relation:b03:trust"]
                .iter()
                .any(|reference| task["completionPredicate"]["value"] == *reference)
    }));
    assert!(tasks.iter().any(|task| {
        task["completionPredicate"]["kind"] == "MYSTERY_RESOLVED"
            && task["completionPredicate"]["value"] == "mystery:ledger"
    }));
    let after = state.manager.open(PROJECT, "", NOW).unwrap();
    let canon_after: i64 = after
        .query_row("SELECT COUNT(*) FROM canon_facts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(canon_before, canon_after);
}

#[test]
fn b03_volume_plan_rejects_dependency_cycles() {
    let (_temp, state) = seed_project();
    setup_intent(&state);
    execute_command_impl(
        &state,
        request("planning_create_volume_plan", "b03-cycle-plan", 1),
    )
    .unwrap();
    let mut route = payload_for(&state, "VolumePlan");
    let tasks = route["tasks"].as_array_mut().unwrap();
    assert!(tasks.len() >= 2);
    let first_id = tasks[0]["taskId"].as_str().unwrap().to_owned();
    let second_id = tasks[1]["taskId"].as_str().unwrap().to_owned();
    tasks[0]["dependsOnTaskIds"] = json!([second_id]);
    tasks[1]["dependsOnTaskIds"] = json!([first_id]);
    assert_eq!(
        validate_volume_plan_semantics(&route).unwrap_err(),
        "VOLUME_PLAN_DEPENDENCY_CYCLE"
    );
}

#[test]
fn b03_runtime_phases_are_backend_projected_and_terminal_active_is_zero() {
    let (_temp, state) = seed_project();
    let mut intent_phases = Vec::new();
    let intent_request = request("planning_create_volume_intent", "b03-phase-intent", 0);
    execute_volume_planning_with_observer(&state, &intent_request, &mut |phase| {
        intent_phases.push(phase.to_owned());
        let projection =
            query_volume_detail_impl(&state, json!({"projectId":PROJECT,"volumeId":VOLUME}))
                .unwrap();
        let operation = &projection["runtimeOperation"];
        if phase != "COMPLETED" {
            assert_eq!(operation["active"], true);
            assert_eq!(operation["kind"], "B03::VOLUME_INTENT");
            assert_eq!(
                operation["steps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|step| step["status"] == "active")
                    .count(),
                1
            );
        } else {
            assert_eq!(operation["active"], false);
        }
    })
    .unwrap();
    assert_eq!(
        intent_phases,
        vec![
            "READ_BOOK_PLAN",
            "READ_CURRENT_STATE",
            "BUILD_VOLUME_GOAL",
            "VALIDATE_VOLUME_GOAL",
            "COMMIT_VOLUME_INTENT",
            "COMPLETED"
        ]
    );

    let mut plan_phases = Vec::new();
    let plan_request = request("planning_create_volume_plan", "b03-phase-plan", 1);
    execute_volume_planning_with_observer(&state, &plan_request, &mut |phase| {
        plan_phases.push(phase.to_owned());
        let projection =
            query_volume_detail_impl(&state, json!({"projectId":PROJECT,"volumeId":VOLUME}))
                .unwrap();
        let operation = &projection["runtimeOperation"];
        if phase != "COMPLETED" {
            assert_eq!(operation["active"], true);
            assert_eq!(operation["kind"], "B03::VOLUME_PLAN");
            assert_eq!(
                operation["steps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|step| step["status"] == "active")
                    .count(),
                1
            );
        } else {
            assert_eq!(operation["active"], false);
        }
    })
    .unwrap();
    assert_eq!(
        plan_phases,
        vec![
            "READ_VOLUME_INTENT",
            "BUILD_TASK_ROUTE",
            "VALIDATE_DEPENDENCIES",
            "CALCULATE_CAPACITY",
            "VALIDATE_ROUTE",
            "COMMIT_VOLUME_PLAN",
            "COMPLETED"
        ]
    );
}
