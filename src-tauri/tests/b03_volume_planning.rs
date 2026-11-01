use ai_novel_production_system_core::commands::{
    execute_command_impl, execute_volume_planning_with_observer, query_volume_detail_impl,
    KernelState,
};
use ai_novel_production_system_core::generated::types::ArtifactRef;
use ai_novel_production_system_core::hash::sha256_canonical;
use ai_novel_production_system_core::planning::{
    compile_goal_contract, compile_volume_intent_from_authority,
    compile_volume_plan_from_authority, evaluate_b03_completion_predicate,
    is_b03_completion_predicate_kind, validate_volume_plan_semantics,
};
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

fn request_with_plan(
    command_type: &str,
    command_id: &str,
    expected_revision: i64,
    plan: Value,
) -> Value {
    let mut request = request(command_type, command_id, expected_revision);
    request["payload"]["plan"] = plan;
    request
}

fn legacy_route() -> Value {
    json!({
        "payloadSchema":"volume-plan.r1",
        "volumeIntentRef":{"artifactId":"legacy-intent","version":1,"payloadHash":"a".repeat(64)},
        "routeVersion":1,
        "tasks":[{"taskId":"task:legacy","class":"REQUIRED","objective":"legacy","dependsOnTaskIds":[],"continuityThreadRefs":[],"reservationRefs":[],"completionPredicate":{"kind":"TASK_EVIDENCE_RECORDED","value":{"taskId":"task:legacy","evidenceType":"FINAL_CHAPTER","requiresNewEvidence":true}},"estimatedChapterCost":{"preferred":1}}],
        "routeInvariants":["legacy route remains bounded"],
        "requiredExitConditions":[{"kind":"TASK_SATISFIED","value":"task:legacy"}],
        "capacityBudget":{"minChapters":1,"preferredChapters":1,"maxChapters":1}
    })
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
fn b03_r1_supplied_route_cannot_bypass_missing_goal_contract_authorities() {
    for (missing, expected) in [
        (
            "StoryConstitution",
            "GOAL_CONTRACT_STORY_CONSTITUTION_REQUIRED",
        ),
        ("MainContract", "GOAL_CONTRACT_MAIN_CONTRACT_REQUIRED"),
    ] {
        let (_temp, state) = seed_project();
        setup_intent(&state);
        let connection = state.manager.open(PROJECT, "", NOW).unwrap();
        connection
            .execute(
                "DELETE FROM canonical_heads WHERE project_id=?1 AND artifact_type=?2",
                libsql_rusqlite::params![PROJECT, missing],
            )
            .unwrap();
        drop(connection);
        let result = execute_command_impl(
            &state,
            request_with_plan(
                "planning_create_volume_plan",
                &format!("b03-r1-route-{missing}"),
                1,
                legacy_route(),
            ),
        );
        assert_eq!(result.unwrap_err(), expected, "missing {missing}");
    }
}

#[test]
fn b03_r1_supplied_empty_volume_intent_is_rejected_before_commit() {
    let (_temp, state) = seed_project();
    let before = state.manager.open(PROJECT, "", NOW).unwrap();
    let before_artifacts: i64 = before
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE artifact_type='VolumeIntent'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(before);
    let empty_intent = json!({
        "payloadSchema":"volume-intent.r1",
        "volumeFunction":"空壳",
        "entryConditions":[],
        "mustAchieve":[],
        "mustNotBreak":[],
        "requiredExitConditions":[],
        "futureReservationRefs":[],
        "crossVolumeResponsibilityRefs":[],
        "targetChapterRange":{"min":1,"preferred":12,"max":12},
        "targetWordRange":{"preferred":12000}
    });
    let result = execute_command_impl(
        &state,
        request_with_plan(
            "planning_create_volume_intent",
            "b03-r1-empty-intent",
            0,
            empty_intent,
        ),
    );
    assert_eq!(result.unwrap_err(), "VOLUME_INTENT_MUST_ACHIEVE_REQUIRED");
    let after = state.manager.open(PROJECT, "", NOW).unwrap();
    let after_artifacts: i64 = after
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE artifact_type='VolumeIntent'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(before_artifacts, after_artifacts);
    assert!(
        NarrativeRepository::canonical_head_ref(&after, PROJECT, "VolumeIntent", VOLUME)
            .unwrap()
            .is_none()
    );
}

#[test]
fn b03_r1_supplied_intent_requires_book_plan_binding_and_revision() {
    let cases = [
        ("sourceBookPlanRef", "VOLUME_INTENT_BOOK_PLAN_REF_REQUIRED"),
        (
            "compiledFromProjectRevision",
            "VOLUME_INTENT_COMPILED_REVISION_REQUIRED",
        ),
    ];
    for (missing, expected) in cases {
        let (_temp, state) = seed_project();
        let mut intent = json!({
            "payloadSchema":"volume-intent.r1",
            "volumeFunction":"供给语义",
            "entryConditions":[],
            "mustAchieve":[{"id":"goal:supplied","description":"供给目标"}],
            "mustNotBreak":[{"id":"rule:supplied","description":"供给约束"}],
            "requiredExitConditions":[{"kind":"GOAL_ACHIEVED","value":"goal:supplied"}],
            "futureReservationRefs":[],
            "crossVolumeResponsibilityRefs":[],
            "targetChapterRange":{"min":1,"preferred":12,"max":12},
            "targetWordRange":{"preferred":12000},
            "sourceBookPlanRef":{"artifactId":"book-plan:b03","version":1,"payloadHash":"0".repeat(64)},
            "compiledFromProjectRevision":0,
            "sourceEvidenceRefs":["evidence:b03:event"]
        });
        intent.as_object_mut().unwrap().remove(missing);
        let result = execute_command_impl(
            &state,
            request_with_plan(
                "planning_create_volume_intent",
                &format!("b03-r1-binding-{missing}"),
                0,
                intent,
            ),
        );
        assert_eq!(result.unwrap_err(), expected, "missing {missing}");
    }
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
        task["completionPredicate"]["kind"] == "TASK_EVIDENCE_RECORDED"
            && task["completionPredicate"]["value"]["requiresNewEvidence"] == true
    }));
    assert!(tasks.iter().any(|task| {
        task["completionPredicate"]["kind"] == "CANON_EVENT_EXISTS"
            && task["completionPredicate"]["value"]["afterPlanCommit"] == true
    }));
    for task in tasks {
        let predicate = &task["completionPredicate"];
        assert_ne!(predicate["kind"], "TASK_SATISFIED");
        assert!(is_b03_completion_predicate_kind(
            predicate["kind"].as_str().unwrap()
        ));
    }
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
fn b03_r1_unknown_completion_predicate_fails_closed() {
    let (_temp, state) = seed_project();
    setup_intent(&state);
    let connection = state.manager.open(PROJECT, "", NOW).unwrap();
    let intent_ref =
        NarrativeRepository::canonical_head_ref(&connection, PROJECT, "VolumeIntent", VOLUME)
            .unwrap()
            .unwrap();
    drop(connection);
    let mut route = legacy_route();
    route["volumeIntentRef"] = serde_json::to_value(intent_ref).unwrap();
    route["tasks"][0]["completionPredicate"] =
        json!({"kind":"UNKNOWN_PREDICATE","value":{"future":true}});
    assert_eq!(
        execute_command_impl(
            &state,
            request_with_plan(
                "planning_create_volume_plan",
                "b03-r1-unknown-predicate",
                1,
                route
            ),
        )
        .unwrap_err(),
        "VOLUME_PLAN_COMPLETION_PREDICATE_UNKNOWN"
    );
}

#[test]
fn b03_r1_self_referential_completion_predicate_fails_closed() {
    let (_temp, state) = seed_project();
    setup_intent(&state);
    let connection = state.manager.open(PROJECT, "", NOW).unwrap();
    let intent_ref =
        NarrativeRepository::canonical_head_ref(&connection, PROJECT, "VolumeIntent", VOLUME)
            .unwrap()
            .unwrap();
    drop(connection);
    let mut route = legacy_route();
    route["volumeIntentRef"] = serde_json::to_value(intent_ref).unwrap();
    route["tasks"][0]["completionPredicate"] =
        json!({"kind":"TASK_SATISFIED","value":"task:legacy"});
    assert_eq!(
        execute_command_impl(
            &state,
            request_with_plan(
                "planning_create_volume_plan",
                "b03-r1-self-predicate",
                1,
                route
            ),
        )
        .unwrap_err(),
        "VOLUME_PLAN_COMPLETION_PREDICATE_SELF_REFERENCE"
    );
}

#[test]
fn b03_r1_existing_canon_cannot_satisfy_a_new_task_without_reality_evidence() {
    let predicate = json!({
        "kind":"TASK_EVIDENCE_RECORDED",
        "value":{"taskId":"task:future","goalId":"goal:future","evidenceType":"FINAL_CHAPTER","requiresNewEvidence":true}
    });
    let existing_canon_only = json!({"status":"NOT_STARTED"});
    assert!(
        !evaluate_b03_completion_predicate(&predicate, "task:future", &existing_canon_only)
            .unwrap()
    );
    let satisfied_with_evidence = json!({
        "status":"SATISFIED",
        "evidenceRefs":[{"artifactId":"final:future","version":1}],
        "lastAdvancedChapterId":"chapter:future:1"
    });
    assert!(
        evaluate_b03_completion_predicate(&predicate, "task:future", &satisfied_with_evidence)
            .unwrap()
    );
}

#[test]
fn b03_r1_volume_intents_are_scoped_by_volume_seeds() {
    let ref_value = ArtifactRef {
        artifact_id: "book-plan:scope".into(),
        version: 1,
        payload_hash: "b".repeat(64),
    };
    let book_plan = json!({
        "payloadSchema":"book-plan.r1",
        "planningMaturity":"APPROVED",
        "storyPromise":"scope",
        "protagonistMacroDirection":"scope",
        "endingBoundary":"scope",
        "mustAchieve":[{"id":"goal:global-a","description":"全书目标 A"},{"id":"goal:global-b","description":"全书目标 B"}],
        "mustNotBreak":[{"id":"rule:global","description":"全书约束"}],
        "relationshipMacroDirections":[],"majorMysteryGoals":[],
        "futureReservations":[
            {"reservationId":"reservation:volume-1","description":"第一卷预留","reservedForVolumeNo":1,"notBeforeVolumeNo":1,"prerequisites":[],"mayForeshadow":true,"mayReveal":false,"authorityLevel":"A"},
            {"reservationId":"reservation:volume-2","description":"第二卷预留","reservedForVolumeNo":2,"notBeforeVolumeNo":2,"prerequisites":[],"mayForeshadow":true,"mayReveal":false,"authorityLevel":"A"}
        ],
        "volumeIntentSeeds":[{"seedId":"seed:volume-1","description":"第一卷只追查 A"},{"seedId":"seed:volume-2","description":"第二卷转向 B"}],
        "openingHypotheses":[],"globalLengthBudget":{"preferredWords":120000}
    });
    let sources = json!({
        "facts":[{"factId":"fact:scope","predicate":"状态","objectJson":{"text":"可验证"}}],
        "relations":[],"knowledge":[],"events":[],"evidence":[{"evidenceId":"evidence:scope"}],
        "responsibilities":[{"obligationId":"obligation:scope","description":"卷内责任"}]
    });
    let first = compile_volume_intent_from_authority(
        "vol:scope:1",
        1,
        1,
        12,
        12000,
        &ref_value,
        &book_plan,
        &sources,
        0,
    )
    .unwrap();
    let second = compile_volume_intent_from_authority(
        "vol:scope:2",
        2,
        13,
        24,
        12000,
        &ref_value,
        &book_plan,
        &sources,
        0,
    )
    .unwrap();
    let first_text = first.to_string();
    let second_text = second.to_string();
    assert!(first_text.contains("第一卷只追查 A"));
    assert!(second_text.contains("第二卷转向 B"));
    assert!(!first_text.contains("第二卷转向 B"));
    assert!(!second_text.contains("第一卷只追查 A"));
    assert_ne!(first["semanticDigest"], second["semanticDigest"]);
    assert!(first["futureReservationRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "reservation:volume-1"));
    assert!(second["futureReservationRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "reservation:volume-2"));
    assert!(first.to_string().contains("obligation:scope"));
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

/// VD-1b: a BookPlan approved with a direction (VD-1a).
fn directed_book_plan() -> Value {
    let stage = |n: i64, goal: &str, chapters: i64| json!({"id":format!("stage:1:{n}"),"goal":goal,"pressure":"裴砚舟出手","cast":["陆照"],"turn":"段末转折","estimatedChapters":chapters});
    json!({
        "payloadSchema":"book-plan.r1",
        "planningMaturity":"APPROVED",
        "storyPromise":"以命换力",
        "protagonistMacroDirection":"陆照从杂役走到签约人",
        "endingBoundary":"结局",
        "mustAchieve":[
            {"id":"goal:book:1","volumeNo":1,"description":"陆照拿到租约原件"},
            {"id":"goal:book:2","volumeNo":2,"description":"陆照走出青崖宗"}
        ],
        "mustNotBreak":[{"id":"rule:opening","description":"开篇已提交事实不得违背；以 Canon 为准。"}],
        "relationshipMacroDirections":[
            {"id":"relation:book:1","volumeNo":1,"description":"陆照与柳漪：从互相提防走向并肩查账"},
            {"id":"relation:book:2","volumeNo":2,"description":"陆照与陈伯：从师徒走向诀别"}
        ],
        "majorMysteryGoals":[
            {"id":"mystery:book:1","volumeNo":2,"description":"第 1 层：师父怎么死的"},
            {"id":"mystery:book:2","volumeNo":1,"description":"第 2 层：租约里多出的那一条是谁加的"}
        ],
        "futureReservations":[],
        "volumeIntentSeeds":[
            {"seedId":"volume-1","description":"守住丹房，揪出改约的人","stages":[
                stage(1,"陆照查清丹房的账",8),
                stage(2,"陆照找到改约的证据",10),
                stage(3,"陆照在会审上翻案",12)
            ],"climax":"会审上陆照当众拿出原件"},
            {"seedId":"volume-2","description":"走出青崖宗"}
        ],
        "openingHypotheses":[],"globalLengthBudget":{"preferredWords":1200000}
    })
}

fn directed_intent() -> Value {
    let book_ref = ArtifactRef {
        artifact_id: "book-plan:directed".into(),
        version: 1,
        payload_hash: "c".repeat(64),
    };
    let sources = json!({
        "facts":[{"factId":"fact:ledger","predicate":"持有","objectJson":{"text":"账册"}}],
        "relations":[],"knowledge":[],
        "events":[{"eventId":"event:fire","eventType":"FIRE","objectJson":{"text":"丹房起火"}}],
        "evidence":[{"evidenceId":"evidence:directed"}],
        "responsibilities":[{"obligationId":"obligation:debt","description":"欠陈伯的药钱"}]
    });
    compile_volume_intent_from_authority(
        "vol:directed:1",
        1,
        1,
        36,
        120000,
        &book_ref,
        &directed_book_plan(),
        &sources,
        0,
    )
    .unwrap()
}

#[test]
fn vd1b_a_directed_volume_intent_sets_its_stages_before_the_volume_goal() {
    let intent = directed_intent();
    let goals = intent["mustAchieve"]
        .as_array()
        .unwrap()
        .iter()
        .map(|goal| {
            (
                goal["id"].as_str().unwrap(),
                goal["description"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        goals,
        vec![
            ("goal:vol:directed:1:stage:1:1", "陆照查清丹房的账"),
            ("goal:vol:directed:1:stage:1:2", "陆照找到改约的证据"),
            ("goal:vol:directed:1:stage:1:3", "陆照在会审上翻案"),
            ("goal:book:1", "陆照拿到租约原件"),
            (
                "goal:vol:directed:1:responsibility:obligation:debt",
                "推进跨卷责任：欠陈伯的药钱"
            ),
        ]
    );
    assert!(intent["volumeFunction"]
        .as_str()
        .unwrap()
        .starts_with("守住丹房，揪出改约的人"));
}

#[test]
fn vd1b_a_directed_route_chains_the_stages_and_peaks_on_this_volumes_mystery() {
    let intent = directed_intent();
    let intent_ref = ArtifactRef {
        artifact_id: "volume-intent:directed".into(),
        version: 1,
        payload_hash: "d".repeat(64),
    };
    let contract_ref = |id: &str| ArtifactRef {
        artifact_id: id.into(),
        version: 1,
        payload_hash: "e".repeat(64),
    };
    let goal_contract = compile_goal_contract(
        &contract_ref("story"),
        &json!({}),
        &contract_ref("main"),
        &json!({}),
        &contract_ref("book"),
        &directed_book_plan(),
        &intent_ref,
        &intent,
    )
    .unwrap();
    let route =
        compile_volume_plan_from_authority(1, &intent_ref, &intent, &goal_contract, 0).unwrap();
    let tasks = route["tasks"].as_array().unwrap();
    let required = tasks
        .iter()
        .filter(|task| task["class"] == "REQUIRED")
        .collect::<Vec<_>>();
    assert_eq!(required[0]["objective"], "陆照查清丹房的账");
    assert_eq!(required[0]["estimatedChapterCost"]["preferred"], 8);
    assert_eq!(required[1]["estimatedChapterCost"]["preferred"], 10);
    assert_eq!(
        required[1]["dependsOnTaskIds"],
        json!([required[0]["taskId"]])
    );
    assert_eq!(required[3]["estimatedChapterCost"]["preferred"], 1);
    let optional = tasks
        .iter()
        .filter(|task| task["class"] == "OPTIONAL")
        .map(|task| task["taskId"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(optional, vec!["task:optional:relation:book:1"]);
    let climax = tasks.iter().find(|task| task["class"] == "CLIMAX").unwrap();
    assert_eq!(climax["taskId"], "task:climax:mystery:book:2");
    assert_eq!(
        climax["objective"],
        "会审上陆照当众拿出原件；第 2 层：租约里多出的那一条是谁加的"
    );
}
