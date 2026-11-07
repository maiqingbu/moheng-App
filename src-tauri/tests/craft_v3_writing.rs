//! CRAFT v3 step 3b: an r2 chapter is written in one piece with the story
//! bible and the style card, brought into the length range by one
//! expand or cut call, and its rule findings (tropes included) are edited
//! in place without a model review.

mod craft_v3_support;

use ai_novel_production_system_core::commands::{
    query_craft_setup_impl, query_production_center_impl, KernelState,
};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use craft_v3_support::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct CraftGateway {
    calls: Mutex<Vec<(String, Value)>>,
    /// Chapters written so far, so each chapter's text differs.
    chapters_written: AtomicUsize,
    /// The continuity check finds a contradiction in every chapter, also
    /// after the rewrite.
    contradiction_always: bool,
    /// Every continuity check call fails.
    check_fails: bool,
    /// Every passage rewrite call fails.
    rewrite_fails: bool,
    /// Every chapter has `COUGH_LINE`, and its first rewrite adds a simile.
    cough_line: bool,
    cough_rewrites: AtomicUsize,
}

/// The inputs of every call to `capability`, in call order.
fn inputs(calls: &[(String, Value)], capability: &str) -> Vec<Value> {
    calls
        .iter()
        .filter(|(name, _)| name == capability)
        .map(|(_, input)| input.clone())
        .collect()
}

const TROPE_SENTENCE: &str = "林满倒吸一口凉气，转身进了面馆。";
/// Round 16: one line in each opening chapter, a third use in chapter 3.
const COUGH_LINE: &str = "陈婆的咳声从甬道那头传下来，一下，两下，第三下咽回去了。";
/// Clashes with chapter 1's detail that 林满 takes his soy milk unsweetened.
const SUGAR_SENTENCE: &str = "林满往豆浆里加了两勺糖。";

/// A chapter in the 番茄 range (1800–2200, LD-1.5) with one stock phrasing.
fn long_body(chapter: usize) -> String {
    let mut lines = (1..=80)
        .map(|index| format!("货单第{index}页写着{index}箱桐油，老董按了第{chapter}个手印。"))
        .collect::<Vec<_>>();
    lines.insert(40, TROPE_SENTENCE.to_string());
    lines.insert(10, SUGAR_SENTENCE.to_string());
    lines.join("\n")
}

impl ProductionModelGateway for CraftGateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        Ok(ResolvedProviderProfile {
            provider: "craft-fixture-provider".into(),
            model: "craft-fixture-model".into(),
            endpoint: "fixture://craft".into(),
            timeout_ms: 1_000,
        })
    }

    fn enforces_chapter_length(&self) -> bool {
        true
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut calls = self.calls.lock().unwrap();
        calls.push((capability.to_string(), input.clone()));
        let output = match capability {
            "GENERATE_STORY_DIRECTIONS" => {
                let pitches = (0..3)
                    .map(|index| {
                        let mut pitch = json!({"name":format!("方向 {index}")});
                        for field in [
                            "promise",
                            "protagonistDesire",
                            "mechanism",
                            "worldConflict",
                            "longArc",
                            "openingHook",
                            "volumeGoal",
                            "commercialEdge",
                            "originality",
                            "difficulty",
                            "failureRisk",
                        ] {
                            pitch[field] = json!(format!("{field} {index}"));
                        }
                        pitch
                    })
                    .collect::<Vec<_>>();
                json!({"pitches":pitches})
            }
            "GENERATE_STORY_BIBLE" => json!({"bibles":[bible(), bible(), bible()]}),
            "PLAN_CHAPTER_CRAFT" => {
                craft_plan(input, input["chapterNumber"].as_i64().unwrap_or(1), false)
            }
            "RENDER_CHAPTER_CRAFT" => json!({"body":"林满推开面馆的门，把账本放在了灶台边。"}),
            "ADJUST_CHAPTER_LENGTH" => {
                let chapter = self.chapters_written.fetch_add(1, Ordering::SeqCst) + 1;
                let mut body = long_body(chapter);
                if self.cough_line {
                    body = body.replacen('\n', &format!("\n{COUGH_LINE}\n"), 1);
                }
                json!({"body":body})
            }
            "REWRITE_PASSAGES"
                if self.cough_line && input["findings"].to_string().contains("第三下咽回去了") =>
            {
                // The rewrite with all its findings adds a simile; the one
                // with the repeats alone does not.
                let repeats_only = input["findings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|finding| finding["code"] == "word_repetition");
                self.cough_rewrites.fetch_add(1, Ordering::SeqCst);
                let replacement = if repeats_only {
                    "甬道那头，陈婆的咳声断断续续，后来没了动静。"
                } else {
                    "陈婆的咳声从甬道那头传下来，像破风箱一样拉了三下。"
                };
                json!({"edits":[{"quote":COUGH_LINE,"replacement":replacement}]})
            }
            "REWRITE_PASSAGES" if self.rewrite_fails => {
                return Err("P01_PROVIDER_HTTP_STATUS:503".into());
            }
            "REWRITE_PASSAGES" => {
                json!({"edits":[{"quote":TROPE_SENTENCE,"replacement":"林满没吭声，转身进了面馆。"}]})
            }
            // QC-1.5: with `contradiction_always`, the sugar sentence
            // contradicts chapter 1's first detail however it is rewritten.
            "CHECK_CONTINUITY" if self.check_fails => {
                return Err("P01_PROVIDER_HTTP_STATUS:503".into());
            }
            "CHECK_CONTINUITY" => {
                let contradicting = input["bodySentences"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|sentence| sentence == SUGAR_SENTENCE)
                    .filter(|_| self.contradiction_always);
                json!({
                    "contradictions":contradicting.map(|index| vec![json!({"sentence":index,"fact":0,"conflict":"他往豆浆里加了糖","impossibleTogether":true})]).unwrap_or_default(),
                    "catchphrases":[]
                })
            }
            "AUDIT_EXTRACT" => {
                let mut audit = audit_no_change();
                if input["continuityDetailsWanted"] == true {
                    audit["continuityDetails"] = json!([
                        {"subject":"林满","kind":"PREFERENCE","detail":"喝豆浆不加糖","quote":"货单第1页写着1箱桐油"},
                        {"subject":"林满","kind":"HABIT","detail":"编出来的习惯","quote":"正文里没有这句"}
                    ]);
                }
                audit
            }
            other => return Err(format!("unexpected capability: {other}")),
        };
        Ok(ProductionModelResponse::deterministic(output))
    }
}

/// A project with a confirmed story bible and style card, opening started.
fn project_ready_to_write(
    prefix: &str,
) -> (tempfile::TempDir, KernelState, Arc<CraftGateway>, String) {
    project_ready_to_write_with(prefix, CraftGateway::default())
}

fn project_ready_to_write_with(
    prefix: &str,
    gateway: CraftGateway,
) -> (tempfile::TempDir, KernelState, Arc<CraftGateway>, String) {
    let temp = tempfile::tempdir().unwrap();
    let gateway = Arc::new(gateway);
    let state = KernelState::with_production_gateway(
        AppPaths::new(temp.path().to_path_buf()),
        gateway.clone(),
    );
    let project_id = project_with_confirmed_bible(&state, prefix);
    command(
        &state,
        &project_id,
        &format!("{prefix}:style"),
        "style_card_generate",
        json!({"source":"MARKET_ARCHETYPE","primary":"A2","secondary":"A1"}),
    );
    let card = query_craft_setup_impl(&state, json!({"projectId":project_id})).unwrap()
        ["styleCard"]["candidates"][0]
        .clone();
    command(
        &state,
        &project_id,
        &format!("{prefix}:style-confirm"),
        "style_card_confirm",
        json!({"candidateId":card["id"],"card":card}),
    );
    command(
        &state,
        &project_id,
        &format!("{prefix}:opening"),
        "opening_ready",
        json!({"approved":true}),
    );
    command(
        &state,
        &project_id,
        &format!("{prefix}:start"),
        "opening_start_creating",
        json!({}),
    );
    (temp, state, gateway, project_id)
}

/// Writes the next opening chapter.
fn write_next_chapter(state: &KernelState, project_id: &str, id: &str) {
    let center = query_production_center_impl(
        state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let result = command(
        state,
        project_id,
        id,
        "production_start",
        json!({
            "chapterId":center["nextChapter"]["id"],"volumeId":center["nextChapter"]["volumeId"],
            "executionMode":"OPENING_BOOTSTRAP","entityIds":[],"references":[],"skillsEnabled":false
        }),
    );
    assert_eq!(result["finalStatus"], "committed");
}

#[test]
fn an_r2_chapter_is_written_whole_with_bible_and_style_card_then_sized_and_edited() {
    let (_temp, state, gateway, project_id) = project_ready_to_write("w3");
    write_next_chapter(&state, &project_id, "w3:production:1");

    let calls = gateway.calls.lock().unwrap();
    let call = |capability: &str| inputs(&calls, capability).into_iter().next();
    assert!(
        call("REVIEW_CHAPTER").is_none(),
        "r2 chapters are not reviewed by a model"
    );
    assert!(
        call("RENDER_CHAPTER").is_none(),
        "r2 chapters use the craft writer"
    );
    let render = call("RENDER_CHAPTER_CRAFT").expect("craft writer");
    assert_eq!(render["storyBible"]["antagonist"]["name"], "彭三");
    // LD-1.5: a 番茄 work writes 90%–110% of 2000 characters.
    assert_eq!(render["chapterLengthRange"], json!({"min":1800,"max":2200}));
    assert_eq!(render["styleCard"]["narration"]["povPolicy"], "STRICT");
    assert!(
        render["styleCard"].get("stage").is_none(),
        "the writer gets the card content only"
    );
    assert_eq!(render["chapterNumber"], 1);
    assert_eq!(render["chapterPlan"]["payloadSchema"], "chapter-plan.r2");
    assert!(render.get("beatTargets").is_none() && render.get("partLengthRange").is_none());
    // LD-7 (real run 2026-11-01, round 11): writers given a scene's
    // estimate wrote two thirds of it; the estimates size the plan only.
    assert!(render["chapterPlan"]["craft"]["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|scene| scene.get("estimatedCharacters").is_none()));
    let adjust = call("ADJUST_CHAPTER_LENGTH").expect("length adjustment");
    // The writer gets the count it has and the count to reach.
    assert!(adjust["currentChars"].as_i64() < adjust["targetChars"].as_i64());
    assert!(adjust.get("shortBy").is_none() && adjust.get("overBy").is_none());
    let rewrite = call("REWRITE_PASSAGES").expect("rule edits");
    assert!(rewrite["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "repeated_pattern"));
    // QC-1.2: the opening never shows the planned threat, so the rewrite is
    // asked to put it after the first sentence before anything else.
    assert_eq!(rewrite["findings"][0]["code"], "opening_threat");
    assert!(rewrite["findings"][0]["problem"]
        .as_str()
        .unwrap()
        .contains("彭三的伙计把催债单拍在面馆柜台上"));
    // QC-1.4/1.5: chapter 1 has no earlier facts or phrases to check.
    assert!(inputs(&calls, "CHECK_CONTINUITY").is_empty());
    drop(calls);

    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let body: String = connection
        .query_row(
            "SELECT json_extract(p.payload_json,'$.body') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='FinalChapter'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!body.contains("倒吸一口凉气"));
    assert!(body.contains("林满没吭声"));
    let receipt: String = connection
        .query_row(
            "SELECT p.payload_json FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ProductionReceipt'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let receipt: Value = serde_json::from_str(&receipt).unwrap();
    assert!(
        receipt["craftSkills"]["proseReview"]["rhythmNotes"].is_array(),
        "{}",
        receipt["craftSkills"]
    );
    assert_eq!(receipt["craftSkills"]["proseReview"]["editsApplied"], 1);
    // QC-1 (2026-10-30): the fixture writes 桐油 on every line, past the
    // detail allowance, and the rewrite is asked to word it differently.
    assert_eq!(receipt["craftSkills"]["proseReview"]["detailFindings"], 6);
    // The fixture's body never shows the planned threat in its first third.
    assert_eq!(
        receipt["craftSkills"]["continuity"]["unresolved"][0]["reason"],
        json!("QC1_OPENING_THREAT_LATE")
    );
}

#[test]
fn qc1_a_contradiction_the_rewrite_leaves_waits_in_the_chapter_reminder() {
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "qc15",
        CraftGateway {
            contradiction_always: true,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "qc15:production:1");
    write_next_chapter(&state, &project_id, "qc15:production:2");
    let calls = gateway.calls.lock().unwrap();
    // Chapter 2 is checked against chapter 1's details by sentence; the
    // contradiction leads the findings and the rewrite is checked again.
    let checks = inputs(&calls, "CHECK_CONTINUITY");
    assert_eq!(checks.len(), 2, "a check and a recheck");
    assert!(checks[0]["bodySentences"].as_array().unwrap().len() > 40);
    assert_eq!(checks[0]["earlierFacts"][0]["fact"], "喝豆浆不加糖");
    // The last rewrite with all the findings (RP-1 may follow it with one
    // for the signing repeats alone).
    let rewrite = inputs(&calls, "REWRITE_PASSAGES")
        .into_iter()
        .rev()
        .find(|input| input["findings"][0]["code"] != "word_repetition")
        .unwrap();
    assert_eq!(rewrite["findings"][0]["code"], "continuity_contradiction");
    drop(calls);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let payload: String = connection
        .query_row(
            "SELECT payload_json FROM decisions WHERE project_id=?1 AND decision_type='AUDIT_REMINDER' AND json_extract(payload_json,'$.title') LIKE '第 2 章%'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(payload.contains("QC1_CONTRADICTION"), "{payload}");
}

#[test]
fn habits_and_looks_from_the_audit_reach_the_next_chapter_as_established_facts() {
    let (_temp, state, gateway, project_id) = project_ready_to_write("f4");
    write_next_chapter(&state, &project_id, "f4:production:1");
    write_next_chapter(&state, &project_id, "f4:production:2");

    let calls = gateway.calls.lock().unwrap();
    let inputs = |capability: &str| inputs(&calls, capability);
    let audits = inputs("AUDIT_EXTRACT");
    assert!(audits
        .iter()
        .all(|audit| audit["continuityDetailsWanted"] == true));
    let plans = inputs("PLAN_CHAPTER_CRAFT");
    assert_eq!(plans[0]["establishedFacts"], json!([]));
    let second_plan = plans.last().unwrap();
    assert_eq!(
        second_plan["establishedFacts"],
        json!([{"chapter":1,"subject":"林满","kind":"PREFERENCE","detail":"喝豆浆不加糖"}]),
        "only the quoted detail survives"
    );
    let renders = inputs("RENDER_CHAPTER_CRAFT");
    assert_eq!(renders[1]["establishedFacts"][0]["detail"], "喝豆浆不加糖");
}

#[test]
fn qc1_a_failed_continuity_check_never_stops_the_chapter() {
    // The check only advises: its failure is recorded, the chapter commits.
    let (_temp, state, _gateway, project_id) = project_ready_to_write_with(
        "qcfail",
        CraftGateway {
            check_fails: true,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "qcfail:production:1");
    write_next_chapter(&state, &project_id, "qcfail:production:2");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let errors: String = connection
        .query_row(
            "SELECT json_extract(p.payload_json,'$.craftSkills.continuity.errors') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ProductionReceipt' AND json_array_length(json_extract(p.payload_json,'$.craftSkills.continuity.errors'))>0",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(errors.contains("unavailable"), "{errors}");
}

#[test]
fn qc1_a_failed_rewrite_never_stops_the_chapter() {
    let (_temp, state, _gateway, project_id) = project_ready_to_write_with(
        "rwfail",
        CraftGateway {
            rewrite_fails: true,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "rwfail:production:1");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let review: String = connection
        .query_row(
            "SELECT json_extract(p.payload_json,'$.craftSkills.proseReview') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ProductionReceipt'",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    let review: Value = serde_json::from_str(&review).unwrap();
    assert!(review["rewriteError"].is_string(), "{review}");
    assert_eq!(review["editsApplied"], 0);
}

#[test]
fn wm1_a_chapter_s_details_are_kept_in_the_work_s_memory() {
    let (_temp, state, _gateway, project_id) = project_ready_to_write("wm1");
    write_next_chapter(&state, &project_id, "wm1:production:1");
    write_next_chapter(&state, &project_id, "wm1:production:2");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let kept = connection
        .query_row(
            "SELECT chapter_no, subject, kind, content FROM work_memory WHERE project_id=?1 ORDER BY chapter_no LIMIT 1",
            [&project_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        kept,
        (1, "林满".into(), "PREFERENCE".into(), "喝豆浆不加糖".into())
    );
}

#[test]
fn wm1_the_memory_lets_go_of_a_chapter_no_longer_committed() {
    let (_temp, state, _gateway, project_id) = project_ready_to_write("wm1gone");
    write_next_chapter(&state, &project_id, "wm1gone:production:1");
    write_next_chapter(&state, &project_id, "wm1gone:production:2");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let count = |connection: &libsql_rusqlite::Connection| -> i64 {
        connection
            .query_row(
                "SELECT COUNT(*) FROM work_memory WHERE project_id=?1 AND chapter_no=1",
                [&project_id],
                |row| row.get(0),
            )
            .unwrap()
    };
    assert!(count(&connection) > 0);
    let (first, later): (String, String) = connection
        .query_row(
            "SELECT (SELECT chapter_id FROM chapters WHERE project_id=?1 AND chapter_no=1), (SELECT chapter_id FROM chapters WHERE project_id=?1 AND chapter_no=3)",
            [&project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    connection
        .execute(
            "UPDATE chapters SET status='PLANNED' WHERE project_id=?1 AND chapter_id=?2",
            [&project_id, &first],
        )
        .unwrap();
    let recalled = ai_novel_production_system_core::repositories::work_memory::earlier_memories(
        &connection,
        &project_id,
        &later,
    )
    .unwrap();
    assert!(recalled.iter().all(|memory| memory.chapter != 1));
    assert_eq!(count(&connection), 0);
}

#[test]
fn rp1_the_second_chapter_is_written_knowing_how_the_first_closed() {
    let (_temp, state, gateway, project_id) = project_ready_to_write("rp1");
    write_next_chapter(&state, &project_id, "rp1:production:1");
    write_next_chapter(&state, &project_id, "rp1:production:2");
    let calls = gateway.calls.lock().unwrap();
    let renders = inputs(&calls, "RENDER_CHAPTER_CRAFT");
    let ending = renders[1]["recentChapters"][0]["ending"].as_str().unwrap();
    assert_eq!(ending.chars().count(), 200);
}

#[test]
fn rp1_a_repeat_the_rewrite_could_not_fix_is_rewritten_once_more() {
    // Real run round 16: the opening failed signing F1-5 because the only
    // edit of its third repeat added a simile and was refused.
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "cough",
        CraftGateway {
            cough_line: true,
            ..CraftGateway::default()
        },
    );
    for chapter in 1..=3 {
        write_next_chapter(&state, &project_id, &format!("cough:production:{chapter}"));
    }
    assert!(gateway.cough_rewrites.load(Ordering::SeqCst) >= 2);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let body: String = connection
        .query_row(
            "SELECT json_extract(f.payload_json,'$.body') FROM chapters c JOIN artifact_payloads f ON f.artifact_id=c.final_artifact_id AND f.version=c.final_version WHERE c.project_id=?1 AND c.chapter_no=3",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!body.contains("第三下咽回去了"), "{body}");
    assert!(body.contains("后来没了动静"));
}
