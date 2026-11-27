//! CRAFT v3 step 3b: an r2 chapter is written in one piece with the story
//! bible and the style card, brought into the length range by one
//! expand or cut call, and its rule findings (tropes included) are edited
//! in place without a model review.

mod craft_v3_support;

use ai_novel_production_system_core::commands::{
    execute_command_impl, query_craft_setup_impl, query_production_center_impl, KernelState,
};
use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::runtime::TransportFailure;
use craft_v3_support::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct CraftGateway {
    calls: Mutex<Vec<(String, Value)>>,
    /// Chapters written so far, so each chapter's text differs.
    chapters_written: AtomicUsize,
    /// The continuity check finds a contradiction in every chapter, also
    /// after the rewrite.
    contradiction_always: bool,
    keep_replayed_event: AtomicBool,
    omit_replay_proof: bool,
    first_confirmation_splices_quote: bool,
    first_confirmation_exceeds_quote_limit: bool,
    /// Every continuity check call fails.
    check_fails: bool,
    continuity_outcome_unknown: bool,
    /// Every passage rewrite call fails.
    rewrite_fails: bool,
    /// Every chapter has `COUGH_LINE`, and its first rewrite adds a simile.
    cough_line: bool,
    cough_rewrites: AtomicUsize,
    /// An audited place changes after the sentence the audit cited.
    place_state: bool,
    /// Every chapter's audit records the same look of 林满.
    worn_look: bool,
    repeated_event: bool,
    replay_in_polish: bool,
    fail_changed_prose_check: bool,
    selects_event: bool,
    replay_from_old_chapter_tail: bool,
    paraphrase_old_event: bool,
    repeated_dialogue: bool,
    replanning_backslide: bool,
    fail_audit_schema: AtomicBool,
    copy_prior_chapter_on_render: bool,
    repeat_prior_phrase_on_render: bool,
    /// Told of the repeat, the writer repeats it again; the rewrite fixes it.
    keeps_the_repeat: bool,
    length_adjustments: Vec<String>,
    length_insertions: Vec<Value>,
}

/// The inputs of every call to `capability`, in call order. The fixture
/// writer's first draft is one sentence, so each chapter is drawn again
/// for its length (round 11) before the adjustment writes it; those redraws
/// are left out, the length suites cover them.
fn inputs(calls: &[(String, Value)], capability: &str) -> Vec<Value> {
    calls
        .iter()
        .filter(|(name, input)| {
            name == capability
                && !input["outputFeedback"]["rejectedFor"]
                    .as_str()
                    .is_some_and(|error| error.starts_with("B08_RENDER_LENGTH_OUT_OF_RANGE"))
        })
        .map(|(_, input)| input.clone())
        .collect()
}

const TROPE_SENTENCE: &str = "林满倒吸一口凉气，转身进了面馆。";
/// Round 16: one line in each opening chapter, a third use in chapter 3.
const COUGH_LINE: &str = "陈婆的咳声从甬道那头传下来，一下，两下，第三下咽回去了。";
/// Clashes with chapter 1's detail that 林满 takes his soy milk unsweetened.
const SUGAR_SENTENCE: &str = "林满往豆浆里加了两勺糖。";
const PLACE_QUOTE: &str = "坛沿浊黄水光到了最高处，就是不溢。";
const PLACE_FOLLOWING: &str = "水光向上拱起，漫过沿口。";
const COMPLETED_EVENT: &str = "镜背夹层无声弹开。";
const REPEATED_EVENT: &str = "铜锈簌簌剥落，镜背夹层弹开一条缝。";
const PARAPHRASED_EVENT: &str = "背面的小格滑出一截，露出藏在里面的纸角。";
const CONTINUED_EVENT: &str = "他从敞开的夹层里抽出一枚断裂的铜签。";
const FIRST_DISCLOSURE: &str = "陆照沉默了两息，问：“你父亲拓到一半收手，是烧了纸，还是烧了手？”\n\n柳青梧把袖子放下来，声音很平：“烧了半截小指。剩下的半截，他后来用来按契。”";
const REPEATED_DISCLOSURE: &str = "“师父当年拓到一半收手，”他抬头看柳青梧，“是看见字就烧了纸，还是手先烧起来？”\n\n柳青梧垂下手，袖口盖回腕上，语气听不出起伏：“半截小指没保住。剩下的半截，按了契。”";
const NEXT_DISCOVERY: &str = "他举起拓片，第一次认出了租约上的指印。";
const R69_REPEATED_PHRASE: &str = "用针尖沿着凸起纹路重新描了一遍。";

/// A chapter in the 番茄 range. Disjoint character sequences fill the mock
/// prose so only the deliberate repetitions exercise the repetition rules.
fn long_body(chapter: usize) -> String {
    let mut lines = (0..80)
        .map(|index| {
            if chapter == 1 && index == 0 {
                return "货单第1页写着1箱桐油，老董按了第1个手印。".into();
            }
            let offset = ((chapter * 80 + index) * 24) as u32;
            let filler = (offset..offset + 20)
                .map(|n| char::from_u32(0x4e00 + n).unwrap())
                .collect::<String>();
            format!("桐油记录{filler}。")
        })
        .collect::<Vec<_>>();
    if chapter <= 2 {
        lines.insert(40, TROPE_SENTENCE.to_string());
        lines.insert(10, SUGAR_SENTENCE.to_string());
    }
    lines.join("\n")
}

/// The start of `text` holding `length` characters, line breaks not counted.
fn counted(text: &str, length: usize) -> String {
    let mut left = length;
    text.chars()
        .take_while(|character| {
            let keep = left > 0;
            left -= usize::from(keep && !character.is_whitespace());
            keep
        })
        .collect()
}

impl ProductionModelGateway for CraftGateway {
    fn selects_main_event(&self) -> bool {
        self.selects_event
    }

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

    fn invoke_typed(
        &self,
        capability: &str,
        input: &Value,
    ) -> Result<ProductionModelResponse, TransportFailure> {
        if self.continuity_outcome_unknown && capability == "CHECK_CONTINUITY" {
            self.calls
                .lock()
                .unwrap()
                .push((capability.to_owned(), input.clone()));
            return Err(TransportFailure::UnknownRemoteOutcome);
        }
        self.invoke(capability, input)
            .map_err(|_| TransportFailure::Unavailable)
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        let mut calls = self.calls.lock().unwrap();
        calls.push((capability.to_string(), input.clone()));
        let replay_text = if self.paraphrase_old_event {
            PARAPHRASED_EVENT
        } else {
            REPEATED_EVENT
        };
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
                let number = input["chapterNumber"].as_i64().unwrap_or(1);
                let mut plan = craft_plan(input, number, false);
                if self.replanning_backslide && number == 2 {
                    if input["planFeedback"]["rejectedFor"] == "B08_PLAN_GOAL_REPEATED" {
                        plan["craft"]["scenes"]
                            .as_array_mut()
                            .unwrap()
                            .push(json!("readerQuestion"));
                    } else {
                        plan["chapterGoal"] = json!("林满揪出调包的货单");
                    }
                }
                plan
            }
            "RENDER_CHAPTER_CRAFT"
                if self.copy_prior_chapter_on_render
                    && input["chapterNumber"] == 2
                    && input.get("outputFeedback").is_none() =>
            {
                json!({"body":input["recentCommittedProse"][0]["body"]})
            }
            "RENDER_CHAPTER_CRAFT" if self.keeps_the_repeat && input["chapterNumber"] == 2 => {
                json!({"body":format!(
                    "林满{R69_REPEATED_PHRASE}\n老董{R69_REPEATED_PHRASE}\n{}",
                    long_body(2)
                )})
            }
            "REWRITE_PASSAGES" if self.keeps_the_repeat => json!({"edits":input["findings"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|finding| finding["quote"].as_str())
                .filter(|quote| quote.contains(R69_REPEATED_PHRASE))
                .map(|quote| json!({"quote":quote,
                    "replacement":quote.replace(R69_REPEATED_PHRASE, "合上账本，走到窗边去。")}))
                .collect::<Vec<_>>()}),
            "RENDER_CHAPTER_CRAFT"
                if self.repeat_prior_phrase_on_render
                    && input["chapterNumber"] == 2
                    && input.get("outputFeedback").is_none() =>
            {
                json!({"body":format!("他拆开铁盒。林满{R69_REPEATED_PHRASE}灯忽然灭了。他{R69_REPEATED_PHRASE}窗外传来一声响。")})
            }
            "RENDER_CHAPTER_CRAFT" => json!({"body":"林满推开面馆的门，把账本放在了灶台边。"}),
            "ADJUST_CHAPTER_LENGTH" => {
                let chapter = self.chapters_written.fetch_add(1, Ordering::SeqCst) + 1;
                if let Some(body) = self.length_adjustments.get(chapter - 1) {
                    return Ok(ProductionModelResponse::deterministic(json!({"body":body})));
                }
                if let Some(reply) = self
                    .length_insertions
                    .get(chapter - 1 - self.length_adjustments.len())
                {
                    return Ok(ProductionModelResponse::deterministic(reply.clone()));
                }
                let mut body = long_body(chapter);
                if self.repeated_event && chapter > 2 {
                    body = body.replacen('\n', &format!("\n{TROPE_SENTENCE}\n"), 1);
                }
                if self.repeat_prior_phrase_on_render && chapter == 1 {
                    body = body.replacen('\n', &format!("\n老董{R69_REPEATED_PHRASE}\n"), 1);
                }
                if self.cough_line && chapter <= 2 {
                    body = body.replacen('\n', &format!("\n{COUGH_LINE}\n"), 1);
                }
                if self.place_state {
                    body = body.replacen('\n', &format!("\n{PLACE_QUOTE}{PLACE_FOLLOWING}\n"), 1);
                }
                if self.repeated_event || (self.replay_in_polish && chapter == 1) {
                    let event = if chapter == 1 {
                        COMPLETED_EVENT
                    } else {
                        replay_text
                    };
                    if chapter == 1 && self.replay_from_old_chapter_tail {
                        let gap = if self.paraphrase_old_event {
                            "远寺传来暮鼓。\n石阶下掠过孤雁。\n油灯渐渐暗下。\n"
                        } else {
                            ""
                        };
                        body = format!("{body}\n{}\n{}\n{gap}{event}", long_body(8), long_body(9));
                    } else {
                        body = body.replacen('\n', &format!("\n{event}\n"), 1);
                    }
                }
                if self.repeated_dialogue {
                    let dialogue = if chapter == 1 {
                        FIRST_DISCLOSURE.to_owned()
                    } else {
                        format!("{REPEATED_DISCLOSURE}\n\n{NEXT_DISCOVERY}")
                    };
                    body = body.replacen('\n', &format!("\n{dialogue}\n"), 1);
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
            "REWRITE_PASSAGES"
                if self.replay_in_polish && self.chapters_written.load(Ordering::SeqCst) == 2 =>
            {
                json!({"edits":[{"quote":TROPE_SENTENCE,"replacement":REPEATED_EVENT}]})
            }
            "REWRITE_PASSAGES" if self.rewrite_fails => {
                return Err("P01_PROVIDER_HTTP_STATUS:503".into());
            }
            "REWRITE_PASSAGES"
                if self.repeated_event
                    && input["continuityRepair"] == true
                    && !self.keep_replayed_event.load(Ordering::SeqCst) =>
            {
                json!({"edits":[{"quote":REPEATED_EVENT,"replacement":CONTINUED_EVENT}]})
            }
            "REWRITE_PASSAGES"
                if self.repeated_dialogue
                    && input["findings"].as_array().unwrap().iter().any(|finding| {
                        finding["code"] == "repeated_event"
                            && finding["quote"] == REPEATED_DISCLOSURE
                    }) =>
            {
                json!({"edits":[{"quote":REPEATED_DISCLOSURE,"replacement":"柳青梧把之前说过的口供按在拓片旁，让他对着契纸自行核实。"}]})
            }
            "REWRITE_PASSAGES" => {
                json!({"edits":[{"quote":TROPE_SENTENCE,"replacement":"林满没吭声，转身进了面馆。"}]})
            }
            // QC-1.5: with `contradiction_always`, the sugar sentence
            // contradicts chapter 1's first detail however it is rewritten.
            "CHECK_CONTINUITY" if self.check_fails => {
                return Err("P01_PROVIDER_HTTP_STATUS:503".into());
            }
            "CHECK_CONTINUITY"
                if self.fail_changed_prose_check
                    && input["bodySentences"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|line| line["text"] == REPEATED_EVENT) =>
            {
                return Err("P01_PROVIDER_HTTP_STATUS:503".into());
            }
            "CHECK_CONTINUITY" | "CONFIRM_NARRATIVE_CONTINUITY" => {
                let contradicting = input["bodySentences"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|sentence| sentence["text"] == SUGAR_SENTENCE)
                    .filter(|_| {
                        self.contradiction_always && input["earlierFacts"].get(0).is_some()
                    });
                let replay = input["bodySentences"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|sentence| sentence["text"] == replay_text)
                    .filter(|_| self.repeated_event || self.replay_in_polish);
                let prior = input["priorChapters"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|chapter| chapter["chapterNo"] == 1)
                    .and_then(|chapter| chapter["sentences"].as_array())
                    .and_then(|sentences| {
                        sentences
                            .iter()
                            .find(|sentence| sentence["text"] == COMPLETED_EVENT)
                    })
                    .and_then(|sentence| sentence["number"].as_u64());
                let narrative = replay
                    .zip(prior)
                    .map(|(index, prior)| {
                        let mut issue = json!({
                            "sentence":index,"chapter":1,"priorSentence":prior,
                            "currentQuote":replay_text,"priorQuote":COMPLETED_EVENT,
                            "kind":"REPEATED_EVENT","reason":"把第一章已打开的夹层再次写成新开启"
                        });
                        if capability == "CONFIRM_NARRATIVE_CONTINUITY" && !self.omit_replay_proof {
                            if self.first_confirmation_exceeds_quote_limit
                                && input.get("outputFeedback").is_none()
                            {
                                issue["currentQuote"] = json!(replay_text.repeat(20));
                            }
                            let outcome = if self.first_confirmation_splices_quote
                                && input.get("evidenceFeedback").is_none()
                            {
                                "铜锈簌簌剥落……弹开一条缝。"
                            } else {
                                replay_text
                            };
                            issue["hardEvidence"] = json!({
                                "relation":"COMPLETED_OUTCOME_REPLAYED",
                                "priorOutcome":"镜背夹层无声弹开",
                                "currentOutcome":outcome,
                            });
                        }
                        vec![issue]
                    })
                    .unwrap_or_default();
                if capability == "CONFIRM_NARRATIVE_CONTINUITY" {
                    json!({"narrativeIssues":narrative})
                } else {
                    json!({
                        "contradictions":contradicting.map(|index| vec![json!({"sentence":index,"fact":0,"conflict":"他往豆浆里加了糖","impossibleTogether":true})]).unwrap_or_default(),
                        "catchphrases":[],"narrativeIssues":narrative
                    })
                }
            }
            "SELECT_MAIN_EVENT" => {
                let outcome = if self.paraphrase_old_event {
                    input["bodySentences"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|sentence| sentence["text"] == COMPLETED_EVENT)
                        .and_then(|sentence| sentence["number"].as_u64())
                        .unwrap_or(0)
                } else {
                    0
                };
                json!({"firstSentence":outcome,"outcomeSentence":outcome,"eventType":"DISCOVERY"})
            }
            "AUDIT_EXTRACT" if self.fail_audit_schema.load(Ordering::SeqCst) => json!({}),
            "AUDIT_EXTRACT" => {
                let mut audit = audit_no_change();
                if input["continuityDetailsWanted"] == true {
                    audit["continuityDetails"] = json!([
                        {"subject":"林满","kind":"PREFERENCE","detail":"喝豆浆不加糖","quote":"货单第1页写着1箱桐油"},
                        {"subject":"林满","kind":"HABIT","detail":"编出来的习惯","quote":"正文里没有这句"}
                    ]);
                    if self.place_state {
                        audit["continuityDetails"].as_array_mut().unwrap().push(json!({
                            "subject":"废丹坛","kind":"PLACE","detail":"水光不溢","quote":PLACE_QUOTE
                        }));
                    }
                    if self.worn_look {
                        audit["continuityDetails"].as_array_mut().unwrap().push(json!({
                            "subject":"林满","kind":"APPEARANCE","detail":"左手腕缠着一圈蓝布条","quote":"林满没吭声，转身进了面馆。"
                        }));
                    }
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

const R65_DUPLICATED_SPAN: &str = "他继续向上，每一步踏响都比前一步多拖长零点三秒；抵达顶层平台时，他解开衬衫最上两粒纽扣，露出锁骨下方一道浅褐色旧疤，形状像半枚未闭合的括号；他用左手食指沿疤痕边缘描摹一遍，指腹停在末端凹陷处不动，喉结上下滑动一次，缓慢，无声";

#[test]
fn second_length_adjustment_with_signing_length_repeat_never_reaches_audit() {
    let chapters = long_body(1);
    let short = counted(&chapters, 1450);
    let first = format!("{short}\n{R65_DUPLICATED_SPAN}。");
    let second = json!({"insertions":[
        {"afterSentence":0,"text":format!("{R65_DUPLICATED_SPAN}；他忽然开口。")}
    ]});
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "adjust-repeat-stop",
        CraftGateway {
            length_adjustments: vec![first],
            length_insertions: vec![second],
            ..CraftGateway::default()
        },
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let error = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"adjust-repeat-stop:produce",
        "idempotencyKey":"adjust-repeat-stop:produce:key","projectId":project_id,
        "expectedRevision":state.manager.project_revision(&project_id).unwrap(),"requestedAt":NOW,
        "payload":{"chapterId":center["nextChapter"]["id"],"executionMode":"OPENING_BOOTSTRAP"}
    })).unwrap_err();
    assert_eq!(error, "B08_RENDER_DUPLICATE_SPAN");
    let calls = gateway.calls.lock().unwrap();
    assert_eq!(inputs(&calls, "ADJUST_CHAPTER_LENGTH").len(), 2);
    assert!(inputs(&calls, "AUDIT_EXTRACT").is_empty());
}

#[test]
fn a_clean_second_adjustment_uses_the_original_body_after_a_duplicate_reply() {
    let first = format!("林满翻开账页。\n{R65_DUPLICATED_SPAN}。\n{R65_DUPLICATED_SPAN}。");
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "adjust-repeat-recover",
        CraftGateway {
            length_adjustments: vec![first, long_body(1)],
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "adjust-repeat-recover:produce");
    let calls = gateway.calls.lock().unwrap();
    let adjustments = inputs(&calls, "ADJUST_CHAPTER_LENGTH");
    assert_eq!(adjustments.len(), 2);
    assert_eq!(adjustments[0]["body"], adjustments[1]["body"]);
    assert_eq!(
        adjustments[1]["outputFeedback"]["code"],
        "B08_RENDER_DUPLICATE_SPAN"
    );
    assert!(!inputs(&calls, "AUDIT_EXTRACT").is_empty());
}

#[test]
fn an_opening_patch_keeps_independent_insertions_when_one_repeats_a_long_span() {
    let base = format!(
        "{R65_DUPLICATED_SPAN}。\n{}",
        counted(&long_body(1), 1400).trim_end()
    );
    let first = "巷口的自行车铃响了两声。掌柜拿起账封对着黄灯，印鉴右下角有一道细缺。他从柜脚抽出铺契，叫账房比对落款。";
    let second = "吴嫂在隔壁灶台找到还热的铜钥匙。她绕过拴狗的柱子，把钥匙藏进装豆子的布袋，沿河堤送给守船的少年。";
    let final_addition = concat!(
        "林满从仓门外转进后院，抬手敲了一下最矮那扇窗。窗里的人把红绳放在桌沿，等他拿出收票才亮灯。",
        "他沿着收票上画出的折线找去，在竹筐底下摸到一枚有凹痕的铁扣。扣背刻着渡口的旧编号，他用纸包好，放进衣领缝里。",
        "门闩又响了一声。林满先把筐放回石板，再推门问守夜的人，下午是谁送来这一批新筐。守夜人取出点货薄，指着一行被水溅湿的小字。",
        "那行字把送货的时辰记成拂晓。林满伸手按住账薄，请他把当班的另外两个人也喊来，当面认一认封泥上的纹样。"
    );
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "opening-partial-insertions",
        CraftGateway {
            length_adjustments: vec![base.clone()],
            length_insertions: vec![
                json!({"insertions":[
                    {"afterSentence":0,"text":format!("{R65_DUPLICATED_SPAN}。")},
                    {"afterSentence":8,"text":first},
                    {"afterSentence":14,"text":second}
                ]}),
                json!({"insertions":[{"afterSentence":0,"text":final_addition}]}),
            ],
            ..CraftGateway::default()
        },
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let chapter = center["nextChapter"]["id"].as_str().unwrap();
    let request = |id: &str, retry: Option<&str>| {
        execute_command_impl(
            &state,
            json!({
                "type":"production_start","commandId":id,"idempotencyKey":format!("{id}:key"),
                "projectId":project_id,"expectedRevision":state.manager.project_revision(&project_id).unwrap(),
                "requestedAt":NOW,"payload":{"chapterId":chapter,"executionMode":"OPENING_BOOTSTRAP","retryOfJobId":retry}
            }),
        )
    };
    assert_eq!(
        request("opening-partial-insertions:write", None).unwrap_err(),
        "B08_RENDER_DUPLICATE_SPAN"
    );
    let conn = state.manager.open(&project_id, "", NOW).unwrap();
    let (job, raw): (String, String) = conn
        .query_row(
            "SELECT j.job_id,k.boundary_json FROM jobs j JOIN job_checkpoints k USING(job_id)
         WHERE j.chapter_id=?1 AND j.status='FAILED' AND k.stage='RENDER'",
            [chapter],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let checkpoint: Value = serde_json::from_str(&raw).unwrap();
    let body = checkpoint["body"].as_str().unwrap();
    assert!(body.contains(first) && body.contains(second));
    assert_eq!(body.matches(R65_DUPLICATED_SPAN).count(), 1);
    assert_eq!(
        checkpoint["craft_skills"]["lengthAdjustmentFeedback"]["code"],
        "B08_RENDER_DUPLICATE_SPAN"
    );
    drop(conn);
    let reply = request("opening-partial-insertions:resume", Some(&job)).unwrap();
    assert_eq!(reply["finalStatus"], "committed");
    let calls = gateway.calls.lock().unwrap();
    let sizing = inputs(&calls, "ADJUST_CHAPTER_LENGTH");
    assert_eq!(
        sizing[2]["outputFeedback"]["code"],
        "B08_RENDER_DUPLICATE_SPAN"
    );
    for audit in inputs(&calls, "AUDIT_EXTRACT") {
        let audited = audit["body"].as_str().unwrap();
        assert!(
            audited.contains(first) && audited.contains(second) && audited.contains(final_addition)
        );
        assert_eq!(audited.matches(R65_DUPLICATED_SPAN).count(), 1);
    }
}

#[test]
fn opening_full_body_expansion_gets_signing_feedback_before_audit() {
    let clean = long_body(1);
    let repeated = format!(
        "{R69_REPEATED_PHRASE}\n{clean}\n{R69_REPEATED_PHRASE}灯灭了。{R69_REPEATED_PHRASE}"
    );
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "opening-full-expansion-f1",
        CraftGateway {
            length_adjustments: vec![repeated, clean],
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "opening-full-expansion-f1:produce");
    let calls = gateway.calls.lock().unwrap();
    let adjustments = inputs(&calls, "ADJUST_CHAPTER_LENGTH");
    assert_eq!(adjustments.len(), 2);
    assert_eq!(adjustments[0]["body"], adjustments[1]["body"]);
    assert_eq!(
        adjustments[1]["outputFeedback"]["code"],
        "B08_RENDER_REPEATED_PHRASES"
    );
    let audits = inputs(&calls, "AUDIT_EXTRACT");
    assert!(!audits.is_empty());
    assert!(audits.iter().all(|audit| !audit["body"]
        .as_str()
        .unwrap()
        .contains(R69_REPEATED_PHRASE)));
}

#[test]
fn an_old_audit_checkpoint_with_repetition_is_rechecked_as_prose_before_audit() {
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "audit-checkpoint-f1",
        CraftGateway {
            fail_audit_schema: AtomicBool::new(true),
            ..CraftGateway::default()
        },
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let chapter = center["nextChapter"]["id"].as_str().unwrap();
    let produce = |command_id: &str, previous: Option<&str>| {
        execute_command_impl(
            &state,
            json!({
                "type":"production_start","commandId":command_id,"idempotencyKey":format!("{command_id}:key"),
                "projectId":project_id,"expectedRevision":state.manager.project_revision(&project_id).unwrap(),
                "requestedAt":NOW,"payload":{"chapterId":chapter,"executionMode":"OPENING_BOOTSTRAP","retryOfJobId":previous}
            }),
        )
    };
    assert!(produce("audit-checkpoint-f1:first", None).is_err());
    let job = replace_failed_audit_checkpoint(&state, &project_id, chapter, |body| {
        format!("{body}\n{R69_REPEATED_PHRASE}\n{R69_REPEATED_PHRASE}\n{R69_REPEATED_PHRASE}")
    });
    gateway.fail_audit_schema.store(false, Ordering::SeqCst);
    let previous_audits = inputs(&gateway.calls.lock().unwrap(), "AUDIT_EXTRACT").len();
    let error = produce("audit-checkpoint-f1:retry", Some(&job)).unwrap_err();
    assert!(
        error.starts_with("B08_RENDER_SIGNING_REPETITION"),
        "{error}"
    );
    assert_eq!(
        inputs(&gateway.calls.lock().unwrap(), "AUDIT_EXTRACT").len(),
        previous_audits
    );
    let conn = state.manager.open(&project_id, "", NOW).unwrap();
    let final_ref: Option<String> = conn
        .query_row(
            "SELECT final_artifact_id FROM chapters WHERE chapter_id=?1",
            [chapter],
            |row| row.get(0),
        )
        .unwrap();
    assert!(final_ref.is_none());
}

fn replace_failed_audit_checkpoint(
    state: &KernelState,
    project_id: &str,
    chapter: &str,
    edit: impl FnOnce(&str) -> String,
) -> String {
    let conn = state.manager.open(project_id, "", NOW).unwrap();
    let (job, raw): (String, String) = conn
        .query_row(
            "SELECT j.job_id,k.boundary_json FROM jobs j JOIN job_checkpoints k USING(job_id)
         WHERE j.chapter_id=?1 AND j.status='FAILED' AND k.stage='AUDIT'",
            [chapter],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let mut checkpoint: Value = serde_json::from_str(&raw).unwrap();
    let body = edit(checkpoint["body"].as_str().unwrap());
    checkpoint["body"] = json!(body);
    checkpoint["body_digest"] = json!(ai_novel_production_system_core::hash::sha256_canonical(
        &checkpoint["body"]
    )
    .unwrap());
    conn.execute(
        "UPDATE job_checkpoints SET boundary_json=?1 WHERE job_id=?2 AND stage='AUDIT'",
        [checkpoint.to_string(), job.clone()],
    )
    .unwrap();
    job
}

#[test]
fn opening_expansion_does_not_save_a_third_use_of_a_short_passage() {
    check_opening_expansion_does_not_add_a_copy(2);
}

#[test]
fn round68_opening_expansion_cannot_replay_a_sentence_for_a_second_time() {
    check_opening_expansion_does_not_add_a_copy(1);
}

fn check_opening_expansion_does_not_add_a_copy(original_uses: usize) {
    let repeat = "粉笔灰簌簌落在酱渍上，那点褐色竟微微发亮，仿佛被唤醒的微弱回响。";
    let draft = format!(
        "{}\n{}林满锁上铺门。",
        counted(&long_body(1), 1490),
        format!("{repeat}\n").repeat(original_uses)
    );
    let clean = concat!(
        "林满把信封拆开，里面叠着一张收据。墨迹还没干，尾款一栏空着。门外自行车的铃响了两声，掌柜站直了，盯着窗上蒙的纸。",
        "送货人从门缝里递进一只布袋，说点数的人换了。从前他们当着买主清点，今天连磅秤也被抬进后院。林满提过布袋，底下渗着水。",
        "他取出油纸裹的账册，递给等在长凳上的姑娘。姑娘翻到六月初七那页，指着手印下面用浓墨涂黑的地方，问他还有哪家铺子收过货。",
        "林满没接话，把一张空白货单伸到柜台底下，沿着木板上的凹痕描了一遍。那一行有六个字，落款是街口卖伞的刘师傅。",
        "他从钥匙圈拆下仓库那把，交给姑娘，约她在河堤下碰面。人走出去后，窗纸上映出掌柜抬手的影子，林满把油灯挪到了窗边。",
    );
    let rejected = format!("{repeat}{clean}");
    let bad_patch = json!({"insertions":[{"afterSentence":0,"text":rejected}]});
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "adjust-short-repeat",
        CraftGateway {
            length_adjustments: vec![draft.clone()],
            length_insertions: vec![
                bad_patch,
                json!({"insertions":[{"afterSentence":0,"text":clean}]}),
            ],
            ..CraftGateway::default()
        },
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let error = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"adjust-short-repeat:produce",
        "idempotencyKey":"adjust-short-repeat:produce:key","projectId":project_id,
        "expectedRevision":state.manager.project_revision(&project_id).unwrap(),"requestedAt":NOW,
        "payload":{"chapterId":center["nextChapter"]["id"],"executionMode":"OPENING_BOOTSTRAP"}
    })).unwrap_err();
    assert_eq!(error, "B08_RENDER_REPEATED_PHRASES");
    assert!(inputs(&gateway.calls.lock().unwrap(), "AUDIT_EXTRACT").is_empty());
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let (previous_job, checkpoint): (String, String) = connection.query_row(
        "SELECT job_id,boundary_json FROM job_checkpoints WHERE stage='RENDER' ORDER BY rowid DESC LIMIT 1",
        [], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&checkpoint).unwrap()["body"],
        draft
    );
    drop(connection);
    let receipt = execute_command_impl(&state, json!({
        "type":"production_start","commandId":"adjust-short-repeat:retry",
        "idempotencyKey":"adjust-short-repeat:retry:key","projectId":project_id,
        "expectedRevision":state.manager.project_revision(&project_id).unwrap(),"requestedAt":NOW,
        "payload":{"chapterId":center["nextChapter"]["id"],"executionMode":"OPENING_BOOTSTRAP",
                   "retryOfJobId":previous_job}
    })).unwrap();
    assert_eq!(receipt["finalStatus"], "committed");
    let calls = gateway.calls.lock().unwrap();
    let adjustments = inputs(&calls, "ADJUST_CHAPTER_LENGTH");
    assert_eq!(
        adjustments[1]["bodySentences"],
        adjustments[2]["bodySentences"]
    );
    assert!(repeat.contains(
        adjustments[2]["outputFeedback"]["repeatedPhrase"]
            .as_str()
            .unwrap()
            .trim()
    ));
    let audits = inputs(&calls, "AUDIT_EXTRACT");
    assert_eq!(
        audits[0]["body"].as_str().unwrap().matches(repeat).count(),
        original_uses
    );
}

#[test]
fn an_opening_retry_chain_reuses_the_plan_and_inserts_only_the_missing_prose() {
    let draft_chars = 1743;
    let draft = counted(&long_body(1), draft_chars).trim_end().to_owned();
    let expected_error = format!("B08_RENDER_LENGTH_OUT_OF_RANGE:{draft_chars}:1800-2200");
    let addition = "林满用袖口遮住桌沿的账册，把压在册底的欠条推到掌柜面前。掌柜伸手想拿，他按住欠条一角，指着下面那道墨印，等着对方当众解释这笔账。";
    let unusable = json!({"insertions":[{"afterSentence":99999,"text":addition}]});
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "opening-prose-retry",
        CraftGateway {
            length_adjustments: vec![draft.clone()],
            length_insertions: vec![
                unusable.clone(),
                unusable.clone(),
                unusable,
                json!({"insertions":[{"afterSentence":0,"text":addition}]}),
            ],
            ..CraftGateway::default()
        },
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let chapter_id = &center["nextChapter"]["id"];
    let produce = |id: &str, previous: Option<&str>| {
        execute_command_impl(
            &state,
            json!({
                "type":"production_start","commandId":id,"idempotencyKey":format!("{id}:key"),
                "projectId":project_id,"expectedRevision":state.manager.project_revision(&project_id).unwrap(),"requestedAt":NOW,
                "payload":{"chapterId":chapter_id,"executionMode":"OPENING_BOOTSTRAP","retryOfJobId":previous}
            }),
        )
    };
    let latest_failed_job = || {
        let conn = state.manager.open(&project_id, "", NOW).unwrap();
        conn.query_row::<String,_,_>(
            "SELECT job_id FROM jobs WHERE project_id=?1 AND chapter_id=?2 AND status='FAILED' ORDER BY rowid DESC LIMIT 1",
            [project_id.as_str(), chapter_id.as_str().unwrap()], |row| row.get(0),
        ).unwrap()
    };
    assert_eq!(
        produce("opening-prose-retry:first", None).unwrap_err(),
        expected_error
    );
    let plan_calls = inputs(&gateway.calls.lock().unwrap(), "PLAN_CHAPTER_CRAFT").len();
    assert_eq!(
        produce("opening-prose-retry:second", Some(&latest_failed_job())).unwrap_err(),
        expected_error
    );
    {
        let conn = state.manager.open(&project_id, "", NOW).unwrap();
        let chapter = chapter_id.as_str().unwrap();
        let quota = ai_novel_production_system_core::repositories::production::ProductionRepository::chapter_quota(
            &conn, &project_id, chapter,
        ).unwrap().unwrap();
        conn.execute(
            "UPDATE chapter_production_budgets SET hard_max_model_runs=?1 WHERE project_id=?2 AND chapter_id=?3",
            libsql_rusqlite::params![quota["used"].as_i64().unwrap(),project_id,chapter],
        ).unwrap();
    }
    assert_eq!(
        produce("opening-prose-retry:budget", Some(&latest_failed_job())).unwrap_err(),
        "BUDGET_EXCEEDED"
    );
    command(
        &state,
        &project_id,
        "opening-prose-retry:quota",
        "production_reset_chapter_quota",
        json!({"chapterId":chapter_id}),
    );
    let receipt = produce(
        "opening-prose-retry:after-budget",
        Some(&latest_failed_job()),
    )
    .unwrap();
    assert_eq!(receipt["finalStatus"], "committed");
    let calls = gateway.calls.lock().unwrap();
    assert_eq!(inputs(&calls, "PLAN_CHAPTER_CRAFT").len(), plan_calls);
    assert_eq!(inputs(&calls, "RENDER_CHAPTER_CRAFT").len(), 1);
    let adjustments = inputs(&calls, "ADJUST_CHAPTER_LENGTH");
    assert_eq!(adjustments.len(), 5);
    assert_eq!(adjustments[4]["currentChars"], draft_chars);
    assert_eq!(adjustments[4]["patchOnly"], true);
    let audits = inputs(&calls, "AUDIT_EXTRACT");
    assert!(audits.last().unwrap()["body"]
        .as_str()
        .unwrap()
        .contains(addition));
}

#[test]
fn a_292_character_gap_is_filled_without_rewriting_the_saved_story() {
    let draft = counted(&long_body(1), 1508);
    let first = "掌柜伸手想拿货单，林满先把木尺横在纸上。他把仓门钥匙和交接册一并推到桌边，请旁边的账房看两处签记。账房捻起页角，发现刻在钥匙柄上的花纹在交接册里也有一份。他没再问仓门是谁开的，转身去叫那天当值的人。林满收起木尺，把空出来的那把椅子往桌前拖了一步。";
    let second = "老董擦了擦茶碗上的水，端起来又放下。他说那晚天黑得早，码头的人收摊之前，他在屋檐下等了半个时辰。林满没有追问名字，先请他指出交货的人从哪边过来。老董抬起手，指向关得严严的侧门。账房伸手一摸门横木，从沟里捻出一截还沾着浆糊的封纸。";
    let third = "掌柜把手伸回袖子，说愿意去后院取存根。林满给他让出过道，自己留在桌边，叫账房看着留下的两件东西。等掌柜掀帘出去，他才把那截封纸压在册子下面。封纸上有一处印泥没干透，他用空白的纸角轻轻沾了一下，把印出来的形状留给账房看。";
    assert!(first.chars().count() + second.chars().count() + third.chars().count() >= 292);
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "opening-larger-insertion",
        CraftGateway {
            length_adjustments: vec![draft.clone()],
            length_insertions: vec![json!({"insertions":[
                {"afterSentence":0,"text":first},
                {"afterSentence":10,"text":second},
                {"afterSentence":40,"text":third}
            ]})],
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "opening-larger-insertion:produce");
    let calls = gateway.calls.lock().unwrap();
    let sizing = inputs(&calls, "ADJUST_CHAPTER_LENGTH");
    assert_eq!(sizing.len(), 2);
    assert_eq!(sizing[1]["patchOnly"], true);
    assert_eq!(sizing[1]["currentChars"], 1508);
    assert!(sizing[1].get("body").is_none());
    let audits = inputs(&calls, "AUDIT_EXTRACT");
    let accepted = audits.last().unwrap()["body"].as_str().unwrap();
    assert!([first, second, third]
        .iter()
        .all(|addition| accepted.contains(addition)));
    assert!(accepted.ends_with(
        &draft
            .chars()
            .rev()
            .take(20)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    ));
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
    // The first chapter has nothing to compare against except its own prose.
    let checks = inputs(&calls, "CHECK_CONTINUITY");
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[0]["priorChapters"], json!([]));
    assert_eq!(checks[0]["bodySentences"][0]["number"], 0);
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
    let final_check = &receipt["craftSkills"]["continuity"]["finalCheck"];
    assert_eq!(
        final_check["bodyDigest"],
        ai_novel_production_system_core::hash::sha256_canonical(&json!(body)).unwrap()
    );
    let checked_run = final_check["modelRunId"].as_str().unwrap();
    assert!(receipt["modelRunRefs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|run| run == checked_run));
    let last_checked_run: String = connection.query_row(
        "SELECT model_run_id FROM model_runs WHERE project_id=?1 AND capability='CHECK_CONTINUITY' AND status='COMPLETED' ORDER BY rowid DESC LIMIT 1",
        [&project_id], |row| row.get(0),
    ).unwrap();
    assert_eq!(checked_run, last_checked_run);
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
    let checks = inputs(&calls, "CHECK_CONTINUITY")
        .into_iter()
        .filter(|input| input["chapterNumber"] == 2)
        .collect::<Vec<_>>();
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
fn a_replayed_event_is_repaired_against_the_committed_previous_chapter() {
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "replayed-event",
        CraftGateway {
            repeated_event: true,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "replay:production:1");
    write_next_chapter(&state, &project_id, "replay:production:2");
    let calls = gateway.calls.lock().unwrap();
    for capability in ["PLAN_CHAPTER_CRAFT", "RENDER_CHAPTER_CRAFT"] {
        let input = inputs(&calls, capability)
            .into_iter()
            .find(|input| input["chapterNumber"] == 2)
            .unwrap();
        assert!(input["recentActualEvents"].as_array().unwrap().is_empty());
        assert_eq!(input["recentCommittedProse"][0]["chapterNo"], 1);
        assert!(input["recentCommittedProse"][0]["body"]
            .as_str()
            .unwrap()
            .contains(COMPLETED_EVENT));
    }
    let checks = inputs(&calls, "CHECK_CONTINUITY")
        .into_iter()
        .filter(|input| input["chapterNumber"] == 2)
        .collect::<Vec<_>>();
    let prior = checks[0]["priorChapters"][0]["sentences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sentence| sentence["text"] == COMPLETED_EVENT)
        .unwrap();
    assert!(checks[0]["bodySentences"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["text"] == REPEATED_EVENT));
    assert!(checks[0]["possibleRepeatedTransitions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|candidate| {
            candidate["chapter"] == 1
                && candidate["priorSentence"] == prior["number"]
                && candidate["sharedWording"] == "镜背夹层"
        }));
    let rewrites = inputs(&calls, "REWRITE_PASSAGES");
    assert!(rewrites
        .iter()
        .any(|input| input["continuityRepair"] == true
            && input["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|finding| finding["code"] == "repeated_event")));
    drop(calls);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let body: String = connection.query_row(
        "SELECT json_extract(p.payload_json,'$.body') FROM chapters c JOIN artifact_payloads p ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version WHERE c.project_id=?1 AND c.chapter_no=2 AND c.status='COMMITTED'",
        [&project_id], |row| row.get(0),
    ).unwrap();
    let review: String = connection.query_row(
        "SELECT json_extract(p.payload_json,'$.craftSkills') FROM canonical_heads h JOIN chapters c ON c.chapter_id=json_extract(h.scope_key,'$.chapterId') JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ProductionReceipt' AND c.chapter_no=2",
        [&project_id], |row| row.get(0),
    ).unwrap();
    assert!(body.contains(CONTINUED_EVENT), "{review}");
    assert!(!body.contains(REPEATED_EVENT));
    assert_eq!(checks.len(), 3);
}

#[test]
fn scarce_allowance_skips_polish_but_still_runs_the_submission_checks() {
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "polish-budget",
        CraftGateway {
            selects_event: true,
            ..CraftGateway::default()
        },
    );
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    connection
        .execute(
            "UPDATE chapter_production_budgets SET hard_max_model_runs=11 WHERE project_id=?1",
            [&project_id],
        )
        .unwrap();
    drop(connection);
    write_next_chapter(&state, &project_id, "polish-budget:first");
    // Review 2026-11-27: a skipped rewrite says why in the receipt.
    let review = prose_review(&state, &project_id);
    assert_eq!(review["rewriteSkipped"], "NO_MODEL_RUNS_LEFT", "{review}");
    let calls = gateway.calls.lock().unwrap();
    assert!(inputs(&calls, "REWRITE_PASSAGES").is_empty());
    assert_eq!(inputs(&calls, "CHECK_CONTINUITY").len(), 1);
    assert!(!inputs(&calls, "AUDIT_EXTRACT").is_empty());
    assert!(!inputs(&calls, "SELECT_MAIN_EVENT").is_empty());
}

#[test]
fn a_replay_introduced_by_polish_is_checked_before_submission() {
    check_polish_replay(false);
}

#[test]
fn a_failed_recheck_reverts_polish_before_auditing_the_checked_prose() {
    check_polish_replay(true);
}

#[test]
fn an_unknown_continuity_run_waits_for_decision_before_auditing_or_publishing() {
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "unknown-continuity",
        CraftGateway {
            continuity_outcome_unknown: true,
            ..CraftGateway::default()
        },
    );
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let result = ai_novel_production_system_core::commands::execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":"unknown-continuity:write","idempotencyKey":"unknown-continuity:write:key",
            "projectId":project_id,"expectedRevision":state.manager.project_revision(&project_id).unwrap(),"requestedAt":NOW,
            "payload":{"chapterId":center["nextChapter"]["id"],"executionMode":"OPENING_BOOTSTRAP"}
        }),
    );
    assert!(result.unwrap_err().contains("UNKNOWN_REMOTE_OUTCOME"));
    let calls = gateway.calls.lock().unwrap();
    assert_eq!(calls.last().unwrap().0, "CHECK_CONTINUITY");
    drop(calls);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let (status, final_chapter): (String, Option<String>) = connection.query_row(
        "SELECT j.status,c.final_artifact_id FROM jobs j JOIN chapters c USING(chapter_id) WHERE j.project_id=?1",
        [&project_id], |row| Ok((row.get(0)?,row.get(1)?)),
    ).unwrap();
    assert_eq!(status, "WAITING_DECISION");
    assert!(final_chapter.is_none());
    let decision: String = connection.query_row(
        "SELECT d.outcome FROM decisions d JOIN model_runs r ON r.project_id=d.project_id AND r.model_run_id=d.source_ref
         WHERE d.project_id=?1 AND d.decision_type='UNKNOWN_REMOTE_OUTCOME' AND r.capability='CHECK_CONTINUITY'",
        [&project_id], |row| row.get(0),
    ).unwrap();
    assert_eq!(decision, "OPEN");
}

fn check_polish_replay(fail_changed_prose_check: bool) {
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "polish-replay",
        CraftGateway {
            replay_in_polish: true,
            fail_changed_prose_check,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "polish-replay:first");
    let center = || {
        query_production_center_impl(
            &state,
            json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
        )
        .unwrap()
    };
    let next = center()["nextChapter"].clone();
    let result = ai_novel_production_system_core::commands::execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":"polish-replay:next","idempotencyKey":"polish-replay:next:key",
            "projectId":project_id,"expectedRevision":state.manager.project_revision(&project_id).unwrap(),"requestedAt":NOW,
            "payload":{"chapterId":next["id"],"executionMode":"OPENING_BOOTSTRAP"}
        }),
    );
    let checks = inputs(&gateway.calls.lock().unwrap(), "CHECK_CONTINUITY");
    let checks = checks
        .iter()
        .filter(|input| input["chapterNumber"] == 2)
        .collect::<Vec<_>>();
    assert!(!checks[0]["bodySentences"]
        .to_string()
        .contains(REPEATED_EVENT));
    assert!(checks[1]["bodySentences"]
        .to_string()
        .contains(REPEATED_EVENT));
    if fail_changed_prose_check {
        assert_eq!(result.unwrap()["finalStatus"], "committed");
        let audits = inputs(&gateway.calls.lock().unwrap(), "AUDIT_EXTRACT");
        let last_audit = audits.last().unwrap()["body"].as_str().unwrap();
        assert!(last_audit.contains(TROPE_SENTENCE));
        assert!(!last_audit.contains(REPEATED_EVENT));
        let connection = state.manager.open(&project_id, "", NOW).unwrap();
        let (receipt, run_id): (String, String) = connection.query_row(
            "SELECT p.payload_json,m.model_run_id FROM canonical_heads h JOIN artifact_payloads p USING(artifact_id,version)
             JOIN jobs j ON j.project_id=h.project_id AND j.job_id=json_extract(p.payload_json,'$.jobId')
             JOIN model_runs m ON m.project_id=j.project_id AND m.job_id=j.job_id
             WHERE h.project_id=?1 AND h.artifact_type='ProductionReceipt'
             AND json_extract(h.scope_key,'$.chapterId')=?2
             AND m.capability='CHECK_CONTINUITY' AND m.status='COMPLETED' ORDER BY m.rowid DESC LIMIT 1",
            libsql_rusqlite::params![project_id,next["id"].as_str().unwrap()], |row| Ok((row.get(0)?,row.get(1)?)),
        ).unwrap();
        let receipt: Value = serde_json::from_str(&receipt).unwrap();
        assert_eq!(
            receipt["craftSkills"]["continuity"]["finalCheck"],
            json!({"bodyDigest":ai_novel_production_system_core::hash::sha256_canonical(&json!(last_audit)).unwrap(),
                   "modelRunId":run_id})
        );
    } else {
        assert_eq!(
            result.err().as_deref(),
            Some("B08_NARRATIVE_REVIEW_REQUIRED")
        );
        assert!(center()["heldDraft"]["body"]
            .as_str()
            .unwrap()
            .contains(REPEATED_EVENT));
    }
}

#[test]
fn a_replay_of_a_clue_beyond_the_prior_prose_excerpt_holds_with_its_formal_source() {
    check_replay_beyond_prior_prose_excerpt(false, false);
}

#[test]
fn an_archived_main_event_is_checked_when_a_replay_shares_no_phrase() {
    let chars = COMPLETED_EVENT.chars().collect::<Vec<_>>();
    assert!(chars
        .windows(4)
        .all(|span| !PARAPHRASED_EVENT.contains(&span.iter().collect::<String>())));
    check_replay_beyond_prior_prose_excerpt(true, false);
}

#[test]
fn an_archived_event_with_an_invalid_quote_does_not_supply_continuity_evidence() {
    check_replay_beyond_prior_prose_excerpt(true, true);
}

fn check_replay_beyond_prior_prose_excerpt(paraphrase: bool, invalid_event_quote: bool) {
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "long-replay",
        CraftGateway {
            repeated_event: true,
            replay_from_old_chapter_tail: true,
            paraphrase_old_event: paraphrase,
            selects_event: paraphrase,
            keep_replayed_event: AtomicBool::new(true),
            ..CraftGateway::default()
        },
    );
    let center = || {
        query_production_center_impl(
            &state,
            json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
        )
        .unwrap()
    };
    let first = center()["nextChapter"].clone();
    let result = command(
        &state,
        &project_id,
        "long-replay:first",
        "production_start",
        json!({
            "chapterId":first["id"],"executionMode":"OPENING_BOOTSTRAP",
            "chapterLengthRange":{"min":5400,"max":6500}
        }),
    );
    let first_final = result["finalChapterRef"].clone();
    if paraphrase {
        let connection = state.manager.open(&project_id, "", NOW).unwrap();
        let event_quote = connection.query_row::<String, _, _>(
            "SELECT json_extract(payload_json,'$.event.outcomeQuote') FROM canon_events WHERE project_id=?1 AND json_extract(payload_json,'$.event.eventTypeSource')='model_selection'",
            [&project_id], |row| row.get(0),
        ).unwrap();
        assert!(COMPLETED_EVENT.contains(&event_quote), "{event_quote}");
        if invalid_event_quote {
            connection.execute(
                "UPDATE canon_events SET payload_json=json_set(payload_json,'$.evidenceRange.quote','无效旧证据') WHERE project_id=?1",
                [&project_id],
            ).unwrap();
        }
    }
    let next = center()["nextChapter"].clone();
    let result = ai_novel_production_system_core::commands::execute_command_impl(
        &state,
        json!({
            "type":"production_start","commandId":"long-replay:next","idempotencyKey":"long-replay:next:key",
            "projectId":project_id,"expectedRevision":state.manager.project_revision(&project_id).unwrap(),"requestedAt":NOW,
            "payload":{"chapterId":next["id"],"executionMode":"OPENING_BOOTSTRAP"}
        }),
    );
    let calls = gateway.calls.lock().unwrap();
    let checks = inputs(&calls, "CHECK_CONTINUITY");
    let next_check = checks
        .iter()
        .find(|input| input["chapterNumber"] == 2)
        .unwrap();
    let source = &next_check["priorChapters"][0];
    let recalled = source["sentences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sentence| sentence["text"] == COMPLETED_EVENT);
    if invalid_event_quote {
        assert!(recalled.is_none());
        assert!(source["recordedOutcomeSentence"].is_null());
        assert_eq!(result.unwrap()["finalStatus"], "committed");
        return;
    }
    assert!(
        recalled.is_some(),
        "source={}, sentences={}, matching draft={}",
        source["source"],
        source["sentences"].as_array().unwrap().len(),
        next_check["bodySentences"]
            .as_array()
            .unwrap()
            .iter()
            .any(|sentence| sentence["text"]
                == if paraphrase {
                    PARAPHRASED_EVENT
                } else {
                    REPEATED_EVENT
                })
    );
    if paraphrase {
        assert_eq!(
            source["recordedOutcomeSentence"],
            recalled.unwrap()["number"]
        );
    }
    drop(calls);
    assert_eq!(
        result.err().as_deref(),
        Some("B08_NARRATIVE_REVIEW_REQUIRED")
    );
    let draft = center()["heldDraft"].clone();
    let evidence = &draft["findings"][0]["evidence"];
    assert_eq!(evidence["priorChapter"], 1);
    assert_eq!(evidence["priorQuote"], COMPLETED_EVENT);
    assert_eq!(evidence["priorSource"]["finalRef"], first_final);
    assert!(inputs(
        &gateway.calls.lock().unwrap(),
        "CONFIRM_NARRATIVE_CONTINUITY"
    )
    .iter()
    .any(|input| input["confirmNarrativeIssues"]
        .to_string()
        .contains(COMPLETED_EVENT)));
}

#[test]
fn a_repeated_disclosure_is_repaired_and_the_new_discovery_commits() {
    let (_temp, state, _gateway, project_id) = project_ready_to_write_with(
        "repeated-disclosure",
        CraftGateway {
            repeated_dialogue: true,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "repeated-disclosure:production:1");
    write_next_chapter(&state, &project_id, "repeated-disclosure:production:2");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let body: String = connection.query_row(
        "SELECT json_extract(p.payload_json,'$.body') FROM chapters c JOIN artifact_payloads p ON p.artifact_id=c.final_artifact_id AND p.version=c.final_version WHERE c.project_id=?1 AND c.chapter_no=2 AND c.status='COMMITTED'",
        [&project_id], |row| row.get(0),
    ).unwrap();
    assert!(body.contains(NEXT_DISCOVERY));
    assert!(!body.contains(REPEATED_DISCLOSURE));
}

#[test]
fn round56_a_replan_does_not_discard_valid_new_progress_when_the_last_goal_repeats() {
    let (_temp, state, _gateway, project_id) = project_ready_to_write_with(
        "replanning-backslide",
        CraftGateway {
            replanning_backslide: true,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "replanning-backslide:production:1");
    write_next_chapter(&state, &project_id, "replanning-backslide:production:2");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let goal: String = connection.query_row(
        "SELECT json_extract(p.payload_json,'$.chapterGoal') FROM canonical_heads h JOIN artifact_payloads p USING(artifact_id,version) JOIN chapters c ON c.project_id=h.project_id AND c.chapter_id=json_extract(h.scope_key,'$.chapterId') WHERE h.project_id=?1 AND h.artifact_type='ChapterPlan' AND c.chapter_no=2",
        [&project_id], |row| row.get(0),
    ).unwrap();
    assert_eq!(goal, "林满在当铺找到借据的漏洞");
}

#[test]
fn a_review_that_only_repeats_the_suspicion_leaves_a_reminder_without_holding_the_chapter() {
    let (_temp, state, _gateway, project_id) = project_ready_to_write_with(
        "unsupported-review",
        CraftGateway {
            repeated_event: true,
            keep_replayed_event: AtomicBool::new(true),
            omit_replay_proof: true,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "unsupported-review:production:1");
    write_next_chapter(&state, &project_id, "unsupported-review:production:2");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let unresolved: String = connection.query_row(
        "SELECT p.payload_json FROM canonical_heads h JOIN chapters c ON c.chapter_id=json_extract(h.scope_key,'$.chapterId') JOIN artifact_payloads p ON p.artifact_id=h.artifact_id AND p.version=h.version WHERE h.project_id=?1 AND h.artifact_type='ProductionReceipt' AND c.chapter_no=2",
        [&project_id], |row| row.get(0),
    ).unwrap();
    assert!(unresolved.contains("QC1_NARRATIVE_CONTINUITY"));
}

#[test]
fn confirmed_replay_preserves_a_draft_and_requires_an_explicit_retry() {
    check_confirmed_replay_hold(false, false);
}

#[test]
fn round61_confirmation_repairs_a_spliced_quote_before_holding_the_draft() {
    check_confirmed_replay_hold(true, false);
}

#[test]
fn round68_schema_resend_can_still_hold_a_confirmed_replay() {
    check_confirmed_replay_hold(false, true);
}

#[test]
fn round68_a_full_copy_is_regenerated_before_chapter_two_reaches_audit() {
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "rendered-prior-copy",
        CraftGateway {
            copy_prior_chapter_on_render: true,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "prior-copy:one");
    write_next_chapter(&state, &project_id, "prior-copy:two");
    let calls = gateway.calls.lock().unwrap();
    let renders = inputs(&calls, "RENDER_CHAPTER_CRAFT");
    assert_eq!(renders.len(), 3);
    assert_eq!(
        renders[2]["outputFeedback"]["rejectedFor"],
        "B08_RENDER_PRIOR_CHAPTER_COPY"
    );
    assert_eq!(renders[1]["chapterPlan"], renders[2]["chapterPlan"]);
    let audits = inputs(&calls, "AUDIT_EXTRACT");
    let prior = renders[1]["recentCommittedProse"][0]["body"]
        .as_str()
        .unwrap();
    let second_chapter_audits = audits
        .iter()
        .filter(|audit| audit["chapterId"] == renders[1]["chapterId"])
        .collect::<Vec<_>>();
    assert!(!second_chapter_audits.is_empty());
    assert!(second_chapter_audits
        .iter()
        .all(|audit| audit["body"] != prior));
}

#[test]
fn round69_an_initial_opening_render_cannot_add_the_third_use_of_a_phrase() {
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "rendered-third-phrase",
        CraftGateway {
            repeat_prior_phrase_on_render: true,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "third-phrase:one");
    write_next_chapter(&state, &project_id, "third-phrase:two");
    let calls = gateway.calls.lock().unwrap();
    let renders = inputs(&calls, "RENDER_CHAPTER_CRAFT");
    assert_eq!(renders.len(), 3);
    assert_eq!(
        renders[2]["outputFeedback"]["rejectedFor"],
        "B08_RENDER_SIGNING_REPETITION"
    );
    assert!(R69_REPEATED_PHRASE.contains(
        renders[2]["outputFeedback"]["repeatedPhrase"]
            .as_str()
            .unwrap()
    ));
    let adjustments = inputs(&calls, "ADJUST_CHAPTER_LENGTH");
    assert_eq!(adjustments.len(), 2);
    assert!(!adjustments[1]["body"]
        .as_str()
        .unwrap()
        .contains(R69_REPEATED_PHRASE));
}

/// Real run 2026-11-25: chapter 3 quoted a clue a third time, both drafts
/// were refused, and the chapter failed before the rewrite could word it
/// differently.
#[test]
fn a_second_draft_that_still_repeats_goes_on_to_the_steps_that_fix_it() {
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "rendered-third-phrase-twice",
        CraftGateway {
            repeat_prior_phrase_on_render: true,
            keeps_the_repeat: true,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "third-phrase-twice:one");
    write_next_chapter(&state, &project_id, "third-phrase-twice:two");
    let calls = gateway.calls.lock().unwrap();
    assert_eq!(inputs(&calls, "RENDER_CHAPTER_CRAFT").len(), 3);
    assert!(!inputs(&calls, "AUDIT_EXTRACT").last().unwrap()["body"]
        .as_str()
        .unwrap()
        .contains(R69_REPEATED_PHRASE));
}

#[test]
fn round61_confirmation_does_not_spend_the_reserved_submission_allowance() {
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "review-budget",
        CraftGateway {
            repeated_event: true,
            keep_replayed_event: AtomicBool::new(true),
            first_confirmation_splices_quote: true,
            selects_event: true,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "review-budget:production:1");
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let chapter_id: String = connection
        .query_row(
            "SELECT chapter_id FROM chapters WHERE project_id=?1 AND chapter_no=2",
            [&project_id],
            |row| row.get(0),
        )
        .unwrap();
    connection
        .execute(
            "UPDATE chapter_production_budgets SET hard_max_model_runs=9 WHERE project_id=?1
         AND chapter_id=?2",
            libsql_rusqlite::params![project_id, chapter_id],
        )
        .unwrap();
    drop(connection);
    write_next_chapter(&state, &project_id, "review-budget:production:2");
    let calls = gateway.calls.lock().unwrap();
    assert_eq!(inputs(&calls, "CONFIRM_NARRATIVE_CONTINUITY").len(), 1);
    assert!(inputs(&calls, "AUDIT_EXTRACT")
        .iter()
        .any(|request| request["chapterId"] == chapter_id));
    assert!(inputs(&calls, "SELECT_MAIN_EVENT")
        .iter()
        .any(|request| request["chapterId"] == chapter_id));
    drop(calls);
    let connection = state.manager.open(&project_id, "", NOW).unwrap();
    let errors: String = connection.query_row(
        "SELECT json_extract(p.payload_json,'$.craftSkills.continuity.errors')
         FROM canonical_heads h JOIN chapters c ON c.project_id=h.project_id AND c.chapter_id=json_extract(h.scope_key,'$.chapterId')
         JOIN artifact_payloads p USING(artifact_id,version)
         WHERE h.project_id=?1 AND h.artifact_type='ProductionReceipt' AND c.chapter_no=2",
        [&project_id], |row| row.get(0),
    ).unwrap();
    assert!(
        errors.contains("QC1_CONFIRMATION_EVIDENCE_DEFERRED_FOR_BUDGET"),
        "{errors}"
    );
}

fn check_confirmed_replay_hold(
    first_confirmation_splices_quote: bool,
    first_confirmation_exceeds_quote_limit: bool,
) {
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "review-hold",
        CraftGateway {
            repeated_event: true,
            keep_replayed_event: AtomicBool::new(true),
            first_confirmation_splices_quote,
            first_confirmation_exceeds_quote_limit,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "review-hold:production:1");
    let center = || {
        query_production_center_impl(
            &state,
            json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
        )
        .unwrap()
    };
    let next = center()["nextChapter"].clone();
    let execute = |id: &str, retry: Option<&Value>| {
        let mut payload = json!({
            "chapterId":next["id"],"volumeId":next["volumeId"],"executionMode":"OPENING_BOOTSTRAP",
            "entityIds":[],"references":[],"skillsEnabled":false
        });
        if let Some(retry) = retry {
            payload["retryOfJobId"] = retry.clone();
        }
        ai_novel_production_system_core::commands::execute_command_impl(
            &state,
            json!({
                "type":"production_start","commandId":id,"idempotencyKey":format!("{id}:key"),
                "projectId":project_id,"expectedRevision":state.manager.project_revision(&project_id).unwrap(),
                "requestedAt":NOW,"payload":payload
            }),
        )
    };
    assert_eq!(
        execute("review-hold:production:2", None).unwrap_err(),
        "B08_NARRATIVE_REVIEW_REQUIRED"
    );
    let held = center();
    assert_eq!(held["jobStatus"], "BLOCKED");
    assert_eq!(held["gate"]["status"], "BLOCKED");
    assert_eq!(held["productionStage"], "CANDIDATE");
    assert_eq!(held["nextChapter"]["id"], next["id"]);
    assert!(held["finalChapterRef"].is_null());
    assert!(held["heldDraft"]["body"]
        .as_str()
        .unwrap()
        .contains(REPEATED_EVENT));
    assert_eq!(held["heldDraft"]["findings"][0]["code"], "repeated_event");
    if first_confirmation_exceeds_quote_limit {
        let confirmations = inputs(
            &gateway.calls.lock().unwrap(),
            "CONFIRM_NARRATIVE_CONTINUITY",
        );
        assert_eq!(confirmations.len(), 2);
        let error = confirmations[1]["outputFeedback"]["rejectedFor"]
            .as_str()
            .unwrap();
        assert!(error.starts_with("P01_MODEL_OUTPUT_SPEC:"));
        let conn = state.manager.open(&project_id, "", NOW).unwrap();
        let attempts = conn
            .prepare(
                "SELECT m.status FROM model_runs m WHERE m.job_id=?1
             AND m.capability='CONFIRM_NARRATIVE_CONTINUITY' ORDER BY m.attempt_no",
            )
            .unwrap()
            .query_map([held["heldDraft"]["jobId"].as_str().unwrap()], |row| {
                row.get::<_, String>(0)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(attempts, ["FAILED", "COMPLETED"]);
    }
    if first_confirmation_splices_quote {
        let confirmations = inputs(
            &gateway.calls.lock().unwrap(),
            "CONFIRM_NARRATIVE_CONTINUITY",
        );
        assert_eq!(confirmations.len(), 2);
        let correction = &confirmations[1];
        assert!(correction.get("evidenceFeedback").is_some());
        for candidate in correction["confirmNarrativeIssues"].as_array().unwrap() {
            assert!(confirmations[0]["confirmNarrativeIssues"]
                .as_array()
                .unwrap()
                .contains(candidate));
        }
        assert_eq!(
            held["heldDraft"]["findings"][0]["evidence"]["hardEvidence"]["currentOutcome"],
            REPEATED_EVENT
        );
    }
    assert!(inputs(
        &gateway.calls.lock().unwrap(),
        "CONFIRM_NARRATIVE_CONTINUITY"
    )
    .iter()
    .any(|input| input["confirmNarrativeIssues"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|candidate| !candidate["earlierEvidence"].is_null()
            && candidate["candidateNumber"].is_u64()
            && candidate["currentSentenceRange"]["firstSentence"]
                .as_u64()
                .is_some_and(|number| number > 0)
            && candidate["currentPassage"]
                .as_str()
                .is_some_and(|text| text.contains(REPEATED_EVENT)))));
    assert_eq!(
        execute("review-hold:production:skip", None).unwrap_err(),
        "B08_NARRATIVE_REVIEW_REQUIRED"
    );
    command(
        &state,
        &project_id,
        "review-hold:quota",
        "production_reset_chapter_quota",
        json!({"chapterId":next["id"]}),
    );
    gateway.keep_replayed_event.store(false, Ordering::SeqCst);
    let result = execute(
        "review-hold:production:retry",
        Some(&held["heldDraft"]["jobId"]),
    )
    .unwrap();
    assert_eq!(result["finalStatus"], "committed");
    assert!(center()["heldDraft"].is_null());
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
fn round100_a_look_both_latest_chapters_recorded_is_a_detail_to_avoid_not_a_fact() {
    // 假如前两章的审核都记下林满手腕缠着蓝布条，
    // 当第三章规划和写作时，它不再作为已知事实发给模型，写手收到的是“少写”的细节。
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "worn-look",
        CraftGateway {
            worn_look: true,
            ..CraftGateway::default()
        },
    );
    for chapter in 1..=3 {
        write_next_chapter(
            &state,
            &project_id,
            &format!("worn-look:production:{chapter}"),
        );
    }
    let calls = gateway.calls.lock().unwrap();
    let look = "左手腕缠着一圈蓝布条";
    let renders = inputs(&calls, "RENDER_CHAPTER_CRAFT");
    assert!(renders[1]["establishedFacts"].to_string().contains(look));
    let third = renders.last().unwrap();
    assert!(
        !third["establishedFacts"].to_string().contains(look),
        "{third}"
    );
    assert!(third["avoidDetails"]
        .as_array()
        .unwrap()
        .contains(&json!(format!("林满：{look}"))));
    let plan = inputs(&calls, "PLAN_CHAPTER_CRAFT").last().unwrap().clone();
    assert!(!plan["establishedFacts"].to_string().contains(look));
}

#[test]
fn a_place_s_following_text_reaches_the_next_chapter_through_the_receipt() {
    // 假如审核引用地点的早期状态，当下一章开始规划和写作时，
    // 那么回执中保留的后续原文应作为未确认材料进入两次模型输入。
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "place-following",
        CraftGateway {
            place_state: true,
            ..CraftGateway::default()
        },
    );
    write_next_chapter(&state, &project_id, "place-following:production:1");
    write_next_chapter(&state, &project_id, "place-following:production:2");
    let calls = gateway.calls.lock().unwrap();
    for capability in ["PLAN_CHAPTER_CRAFT", "RENDER_CHAPTER_CRAFT"] {
        let calls = inputs(&calls, capability);
        let facts = calls.last().unwrap()["establishedFacts"]
            .as_array()
            .unwrap();
        let place = facts
            .iter()
            .find(|fact| fact["subject"] == "废丹坛")
            .unwrap();
        assert!(place["detail"].as_str().unwrap().contains(PLACE_FOLLOWING));
        assert!(place["detail"].as_str().unwrap().contains("未确认章末状态"));
    }
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
    let review = prose_review(&state, &project_id);
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
fn rp1_a_repeat_in_a_legacy_checkpoint_is_rewritten_once_more() {
    // Real run round 16: the opening failed signing F1-5 because the only
    // edit of its third repeat added a simile and was refused.
    let (_temp, state, gateway, project_id) = project_ready_to_write_with(
        "cough",
        CraftGateway {
            cough_line: true,
            ..CraftGateway::default()
        },
    );
    for chapter in 1..=2 {
        write_next_chapter(&state, &project_id, &format!("cough:production:{chapter}"));
    }
    let center = query_production_center_impl(
        &state,
        json!({"projectId":project_id,"scope":"PROJECT","pageSize":20}),
    )
    .unwrap();
    let chapter = center["nextChapter"]["id"].as_str().unwrap();
    let produce = |command_id: &str, previous: Option<&str>| {
        execute_command_impl(
            &state,
            json!({
                "type":"production_start","commandId":command_id,"idempotencyKey":format!("{command_id}:key"),
                "projectId":project_id,"expectedRevision":state.manager.project_revision(&project_id).unwrap(),
                "requestedAt":NOW,"payload":{"chapterId":chapter,"executionMode":"OPENING_BOOTSTRAP","retryOfJobId":previous}
            }),
        )
    };
    gateway.fail_audit_schema.store(true, Ordering::SeqCst);
    let error = produce("cough:production:3", None).unwrap_err();
    assert!(error.starts_with("P01_MODEL_OUTPUT_SPEC"), "{error}");
    let job = replace_failed_audit_checkpoint(&state, &project_id, chapter, |body| {
        body.replacen('\n', &format!("\n{COUGH_LINE}\n"), 1)
    });
    gateway.fail_audit_schema.store(false, Ordering::SeqCst);
    let before = gateway.cough_rewrites.load(Ordering::SeqCst);
    let receipt = produce("cough:production:3:retry", Some(&job)).unwrap();
    assert_eq!(receipt["finalStatus"], "committed");
    assert_eq!(gateway.cough_rewrites.load(Ordering::SeqCst) - before, 2);
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

/// The prose review of the project's production receipt.
fn prose_review(state: &KernelState, project_id: &str) -> Value {
    let review: String = state
        .manager
        .open(project_id, "", NOW)
        .unwrap()
        .query_row(
            "SELECT json_extract(p.payload_json,'$.craftSkills.proseReview') FROM artifacts a JOIN artifact_payloads p ON p.artifact_id=a.artifact_id AND p.version=a.version WHERE a.project_id=?1 AND a.artifact_type='ProductionReceipt'",
            [project_id],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str(&review).unwrap()
}
