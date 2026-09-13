use ai_novel_production_system_core::commands::{
    execute_command_impl, query_production_center_impl, query_story_directions_impl, KernelState,
};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const NOW: &str = "2026-09-13T12:00:00Z";

struct B02Gateway {
    audit_calls: AtomicUsize,
    emit_evidence: bool,
}

impl B02Gateway {
    fn new(emit_evidence: bool) -> Self {
        Self {
            audit_calls: AtomicUsize::new(0),
            emit_evidence,
        }
    }
}

impl ProductionModelGateway for B02Gateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        Ok(ResolvedProviderProfile {
            provider: "b02-deterministic-provider".into(),
            model: "b02-deterministic-model".into(),
            endpoint: "fixture://b02".into(),
            timeout_ms: 1_000,
        })
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let output = match capability {
            "GENERATE_STORY_DIRECTIONS" => json!({
                "pitches": (0..3)
                    .map(|index| json!({
                        "name":format!("B02 方向 {}", index + 1),
                        "promise":"每章回收一个真实线索并留下新的选择",
                        "protagonistDesire":"查明失踪账本的真实去向",
                        "mechanism":"每次追查都会留下可验证代价",
                        "worldConflict":"旧秩序试图掩盖账本中的秘密",
                        "longArc":"从被动躲藏走向主动追查",
                        "openingHook":"雨夜的账本少了一页",
                        "volumeGoal":"找到账本缺失页的去向",
                        "commercialEdge":"悬疑线索与成长选择并行",
                        "originality":format!("B02 原创支点 {index}"),
                        "difficulty":"中等",
                        "failureRisk":"线索回收过早"
                    }))
                    .collect::<Vec<_>>()
            }),
            "PLAN_STRATEGY" => json!({
                "payloadSchema":"chapter-plan.r1",
                "chapterId":input["chapterId"],
                "compiledFromProjectRevision":input["compiledFromProjectRevision"],
                "compiledFromStateDigest":input["compiledFromStateDigest"],
                "contextManifestRef":input["contextManifestRef"],
                "chapterGoal":"让主角完成一次主动追查并留下证据",
                "activeTaskRefs":[],
                "continuityActions":[],
                "requiredStateOutcomes":[],
                "forbiddenTransitions":[],
                "knowledgeBoundaries":[],
                "reservationRefs":[],
                "sceneExecution":[],
                "horizonPreview":[
                    {"slotNo":1,"objective":"追查账本线索","tentative":true},
                    {"slotNo":2,"objective":"承担追查代价","tentative":true},
                    {"slotNo":3,"objective":"留下新的问题","tentative":true}
                ]
            }),
            "RENDER_CHAPTER" => json!({
                "body":format!("林舟在雨夜翻开账本，发现第{}章留下的真实证据。", self.audit_calls.load(Ordering::SeqCst) + 1)
            }),
            "AUDIT_EXTRACT" => {
                let chapter_no = self.audit_calls.fetch_add(1, Ordering::SeqCst) + 1;
                let body = format!("林舟在雨夜翻开账本，发现第{}章留下的真实证据。", chapter_no);
                let canon_candidates = if self.emit_evidence {
                    json!([{
                        "factId":format!("fact:b02:opening:{}", chapter_no),
                        "subjectId":"entity:b02:linzhou",
                        "entityId":"entity:b02:linzhou",
                        "canonicalName":"林舟",
                        "entityType":"CHARACTER",
                        "predicate":format!("found_evidence_{}", chapter_no),
                        "objectJson":{"chapter":chapter_no},
                        "authorityLevel":"C",
                        "evidence":{"start":0,"end":body.chars().count(),"quote":body}
                    }])
                } else {
                    json!([])
                };
                json!({"qualityFindings":[],"canonCandidates":canon_candidates,"knowledgeCandidates":[],"continuityCandidates":[],"taskProgressCandidates":[],"revealedReservationRefs":[],"evidenceRanges":[]})
            }
            other => return Err(format!("unexpected B02 capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

fn state(emit_evidence: bool) -> (KernelState, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("project directory");
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path()),
        Arc::new(B02Gateway::new(emit_evidence)),
    );
    (state, temp)
}

fn revision(state: &KernelState, project_id: &str) -> i64 {
    state
        .manager
        .project_revision(project_id)
        .expect("revision")
}

fn command(
    state: &KernelState,
    project_id: &str,
    command_id: &str,
    command_type: &str,
    payload: Value,
) -> Value {
    execute_command_impl(
        state,
        json!({
            "type":command_type,
            "commandId":command_id,
            "idempotencyKey":format!("{command_id}:key"),
            "projectId":project_id,
            "expectedRevision":revision(state, project_id),
            "requestedAt":NOW,
            "payload":payload
        }),
    )
    .unwrap_or_else(|error| panic!("{command_type} failed: {error}"))
}

fn create_opening_project(state: &KernelState) -> String {
    let created = execute_command_impl(
        state,
        json!({
            "type":"project_create_from_idea",
            "commandId":"b02:create",
            "idempotencyKey":"b02:create:key",
            "requestedAt":NOW,
            "payload":{
                "title":"开篇评估真实链",
                "idea":"一个账房学徒发现失踪账本会改写城中所有人的债务。",
                "platform":"起点中文网",
                "mode":"DIRECTOR",
                "tags":["悬疑","成长"],
                "forbidden":"前三章不揭开账本来源"
            }
        }),
    )
    .expect("create project");
    let project_id = created["projectId"]
        .as_str()
        .expect("project id")
        .to_owned();
    command(
        state,
        &project_id,
        "b02:directions",
        "story_directions_generate",
        json!({}),
    );
    let directions = query_story_directions_impl(
        state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":10}),
    )
    .expect("directions");
    let pitch_id = directions["pitches"][0]["id"].as_str().expect("pitch id");
    command(
        state,
        &project_id,
        "b02:select",
        "pitch_select",
        json!({"pitchIds":[pitch_id]}),
    );
    command(
        state,
        &project_id,
        "b02:setup",
        "story_setup_save",
        json!({"storyCore":{
            "protagonist":"林舟",
            "desire":"查明失踪账本的真实去向",
            "mechanism":"每次追查都会留下可验证代价",
            "conflict":"旧秩序试图掩盖账本秘密",
            "readerPromise":"每章回收一个真实线索并留下新的选择",
            "forbiddenReveals":["前三章不揭开账本来源"]
        }}),
    );
    command(
        state,
        &project_id,
        "b02:opening-ready",
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        state,
        &project_id,
        "b02:start-creating",
        "opening_start_creating",
        json!({}),
    );
    project_id
}

fn commit_three_opening_chapters(state: &KernelState, project_id: &str) {
    for chapter_no in 1..=3 {
        let center = query_production_center_impl(
            state,
            json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
        )
        .expect("production center");
        assert_eq!(center["nextChapter"]["number"], chapter_no);
        command(
            state,
            project_id,
            &format!("b02:production:{chapter_no}"),
            "production_start",
            json!({
                "chapterId":center["nextChapter"]["id"],
                "volumeId":center["nextChapter"]["volumeId"],
                "executionMode":"OPENING_BOOTSTRAP",
                "entityIds":[],
                "references":[],
                "skillsEnabled":false
            }),
        );
    }
}

#[test]
fn b02_real_opening_evaluation_and_book_plan_approval_are_authoritative() {
    let (state, _temp) = state(true);
    let project_id = create_opening_project(&state);
    commit_three_opening_chapters(&state, &project_id);

    let before = state.manager.open(&project_id, "", NOW).expect("open db");
    let canon_before: i64 = before
        .query_row(
            "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .expect("canon count");
    let seed_ref: (String, i64) = before
        .query_row(
            "SELECT h.artifact_id,h.version FROM canonical_heads h WHERE h.project_id=?1 AND h.artifact_type='BookPlan'",
            [&project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("seed head");
    assert_eq!(seed_ref.1, 1);
    drop(before);

    let evaluation = command(
        &state,
        &project_id,
        "b02:opening-evaluate",
        "opening_evaluate",
        json!({}),
    );
    assert_eq!(evaluation["finalStatus"], "committed");
    assert_eq!(evaluation["result"]["status"], "PASS");
    let signing_receipt_id = evaluation["result"]["receiptRef"]["artifactId"]
        .as_str()
        .expect("signing receipt ref");
    let replay = command(
        &state,
        &project_id,
        "b02:opening-evaluate",
        "opening_evaluate",
        json!({}),
    );
    assert_eq!(
        replay["result"]["receiptRef"], evaluation["result"]["receiptRef"],
        "idempotent evaluation must replay the original receipt ref"
    );

    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("production center after evaluation");
    assert_eq!(center["projectPhase"], "SIGNING");
    assert_eq!(center["signing"]["status"], "PASS");
    assert_eq!(center["signing"]["receipt"]["id"], signing_receipt_id);
    assert_eq!(
        center["signing"]["openingChapterRefs"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        center["signing"]["openingCanonRefs"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        center["signing"]["signingEvidenceRefs"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(center["bookPlan"]["status"], "PENDING");

    let explicit_wrong_receipt = execute_command_impl(
        &state,
        json!({
            "type":"book_plan_revise",
            "commandId":"b02:book-plan-revise:wrong-receipt",
            "idempotencyKey":"b02:book-plan-revise:wrong-receipt:key",
            "projectId":project_id,
            "expectedRevision":revision(&state, &project_id),
            "requestedAt":NOW,
            "payload":{"openingCanonRefs":center["signing"]["openingCanonRefs"],"signingEvidenceRefs":["artifact:fake-signing"]}
        }),
    );
    assert_eq!(
        explicit_wrong_receipt.unwrap_err(),
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );

    let approved = command(
        &state,
        &project_id,
        "b02:book-plan-revise",
        "book_plan_revise",
        json!({}),
    );
    assert_eq!(approved["finalStatus"], "committed");
    assert_eq!(approved["result"]["planningMaturity"], "APPROVED");

    let after = state.manager.open(&project_id, "", NOW).expect("reopen db");
    let canon_after: i64 = after
        .query_row(
            "SELECT COUNT(*) FROM canon_facts WHERE project_id=?1",
            [&project_id],
            |row| row.get(0),
        )
        .expect("canon count after plan");
    assert_eq!(
        canon_after, canon_before,
        "plan commit must not mutate Canon"
    );
    let seed_exists: i64 = after
        .query_row(
            "SELECT COUNT(*) FROM artifacts WHERE project_id=?1 AND artifact_type='BookPlan' AND artifact_id=?2 AND version=1",
            libsql_rusqlite::params![project_id, seed_ref.0],
            |row| row.get(0),
        )
        .expect("seed history");
    assert_eq!(seed_exists, 1, "BookPlan SEED history must remain");
    let approved_head: (String, i64) = after
        .query_row(
            "SELECT h.artifact_id,h.version FROM canonical_heads h WHERE h.project_id=?1 AND h.artifact_type='BookPlan'",
            [&project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("approved head");
    assert!(approved_head.1 > 1 || approved_head.0 != seed_ref.0);
    let approved_payload: Value = after
        .query_row(
            "SELECT payload_json FROM artifact_payloads WHERE artifact_id=?1 AND version=?2",
            libsql_rusqlite::params![approved_head.0, approved_head.1],
            |row| {
                let raw: String = row.get(0)?;
                Ok(serde_json::from_str(&raw).unwrap())
            },
        )
        .expect("approved payload");
    for field in [
        "mustAchieve",
        "mustNotBreak",
        "relationshipMacroDirections",
        "majorMysteryGoals",
        "futureReservations",
        "volumeIntentSeeds",
        "globalLengthBudget",
    ] {
        assert!(approved_payload.get(field).is_some(), "missing {field}");
    }
    drop(after);

    let final_center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("final production center");
    assert_eq!(final_center["bookPlan"]["status"], "APPROVED");
    assert_eq!(final_center["bookPlan"]["seedPreserved"], true);
    assert_eq!(final_center["projectPhase"], "POST_SIGNING_PLANNING");
    println!("B02_RESULT opening_evaluation=PASS signing_receipt=PASS real_evidence=PASS approved_book_plan=PASS seed_preserved=PASS canon_unchanged=PASS status=PASS");
}

#[test]
fn b02_opening_evaluation_persists_revise_when_real_evidence_is_incomplete() {
    let (state, _temp) = state(false);
    let project_id = create_opening_project(&state);
    commit_three_opening_chapters(&state, &project_id);

    let evaluation = command(
        &state,
        &project_id,
        "b02:opening-evaluate:revise",
        "opening_evaluate",
        json!({}),
    );
    assert_eq!(evaluation["finalStatus"], "committed");
    assert_eq!(evaluation["result"]["status"], "REVISE");
    assert!(evaluation["result"]["receiptRef"]["artifactId"].is_string());

    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .expect("production center after revise evaluation");
    assert_eq!(center["signing"]["status"], "REVISE");
    assert_eq!(center["signing"]["evaluation"]["status"], "COMPLETED");
    assert_eq!(center["bookPlan"]["status"], "PENDING");
    let finalize = execute_command_impl(
        &state,
        json!({
            "type":"book_plan_revise",
            "commandId":"b02:book-plan-revise:blocked",
            "idempotencyKey":"b02:book-plan-revise:blocked:key",
            "projectId":project_id,
            "expectedRevision":revision(&state, &project_id),
            "requestedAt":NOW,
            "payload":{}
        }),
    );
    assert_eq!(
        finalize.unwrap_err(),
        "BOOK_PLAN_SIGNING_EVIDENCE_UNREACHABLE"
    );
    println!("B02_REVISE_RESULT incomplete_evidence=PASS receipt_committed=PASS plan_blocked=PASS status=PASS");
}
