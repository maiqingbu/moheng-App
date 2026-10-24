//! CRAFT v3 step 3a: post-write rule checks for r2 chapters — stock tropes,
//! author asides under a strict point of view, in-chapter loops, wording
//! copied from the plan or the story bible, and rhythm notes against the
//! style card (CRAFT_V3_DESIGN §4.4).

use ai_novel_production_system_core::chapter_craft::{craft_rule_findings, rhythm_notes};
use serde_json::{json, Value};

fn plan() -> Value {
    json!({"payloadSchema":"chapter-plan.r2","chapterGoal":"林满揪出一笔假账","craft":{
        "bigTurn":"林满发现货单是彭三的人连夜调包的","smallDiscovery":"老董也欠着彭三一笔钱",
        "scenes":[{"place":"江边码头","events":["老董把货单递过来交给林满","林满点出货单上少了两箱"]}]}})
}

fn bible() -> Value {
    json!({"normalWorld":"林满在江边码头记账，每天清点货单和船老大讨价还价到天黑",
        "protagonist":{"name":"林满","innerVoiceStyle":"像给自己记账，先列数再下结论"}})
}

fn card(pov: &str) -> Value {
    let range = |min: f64, max: f64| json!({"min":min,"max":max});
    json!({"narration":{"person":"THIRD","povPolicy":pov,"voice":"话少"},"targets":{
        "avgParagraphChars":range(20.0, 30.0),"shortParagraphPct":range(30.0, 50.0),
        "dialogueParagraphPct":range(30.0, 60.0),"similePer1k":range(0.0, 1.0),
        "explainPer1k":range(0.0, 1.5),"exclaimPer1k":range(1.0, 4.0),
        "questionPer1k":range(2.0, 6.0),"ellipsisPer1k":range(1.0, 6.0)}})
}

fn codes_for<'a>(findings: &'a [Value], needle: &str) -> Vec<&'a str> {
    findings
        .iter()
        .filter(|finding| finding["quote"].as_str().unwrap_or("").contains(needle))
        .filter_map(|finding| finding["code"].as_str())
        .collect()
}

#[test]
fn stock_tropes_are_flagged_with_the_sentence_that_uses_them() {
    let body =
        "林满把账本合上。巷口站着一个穿灰色外套的男人跟了他三条街。他倒吸一口凉气，转身进了面馆。";
    let findings = craft_rule_findings(&plan(), body, &bible(), None);
    assert_eq!(
        codes_for(&findings, "灰色外套的男人跟"),
        vec!["repeated_pattern"]
    );
    assert_eq!(
        codes_for(&findings, "倒吸一口凉气"),
        vec!["repeated_pattern"]
    );
    assert!(findings.iter().all(|finding| finding["source"] == "rule"));
}

#[test]
fn author_asides_break_a_strict_point_of_view_only() {
    let body = "林满走进码头。他不知道的是，彭三早就在船上等着了。老董递来货单。";
    let strict = craft_rule_findings(&plan(), body, &bible(), Some(&card("STRICT")));
    assert_eq!(codes_for(&strict, "他不知道的是"), vec!["pov_info_leak"]);
    let loose = craft_rule_findings(&plan(), body, &bible(), Some(&card("SHORT_CUTS_ALLOWED")));
    assert!(codes_for(&loose, "他不知道的是").is_empty());
}

#[test]
fn the_third_use_of_a_phrase_within_the_chapter_is_flagged() {
    let body = "他又看了一眼那本旧账本的封皮。门外有人喊。他又看了一眼那本旧账本的封皮。老董笑了。最后他又看了一眼那本旧账本的封皮。";
    let findings = craft_rule_findings(&plan(), body, &bible(), None);
    let looping = findings
        .iter()
        .filter(|finding| finding["code"] == "redundancy")
        .collect::<Vec<_>>();
    assert_eq!(looping.len(), 1, "{findings:?}");
    assert!(looping[0]["quote"].as_str().unwrap().starts_with("最后"));
}

#[test]
fn wording_copied_from_the_plan_or_the_bible_is_flagged() {
    let body = "天刚亮。林满发现货单是彭三的人连夜调包的。他想，每天清点货单和船老大讨价还价到天黑，日子也就这样。";
    let findings = craft_rule_findings(&plan(), body, &bible(), None);
    assert_eq!(
        codes_for(&findings, "连夜调包"),
        vec!["summary_where_scene_needed"]
    );
    assert_eq!(
        codes_for(&findings, "讨价还价"),
        vec!["summary_where_scene_needed"]
    );
}

#[test]
fn rhythm_notes_compare_the_chapter_with_the_style_card() {
    let flat =
        "林满走到码头的时候天还没有亮，江面上的雾很重，船老大们三三两两地蹲在岸边抽烟等着开工。\n"
            .repeat(20);
    let notes = rhythm_notes(&flat, &card("STRICT"));
    assert!(
        notes.iter().any(|note| note.contains("段落偏长")),
        "{notes:?}"
    );
    assert!(
        notes.iter().any(|note| note.contains("语气平")),
        "{notes:?}"
    );
    let lively =
        "“谁动了货单？”\n林满没回答。\n他把账本摔在桌上！\n老董愣了……\n“你别瞎说。”\n".repeat(10);
    let lively_notes = rhythm_notes(&lively, &card("STRICT"));
    assert!(
        !lively_notes.iter().any(|note| note.contains("语气平")),
        "{lively_notes:?}"
    );
    assert!(
        !lively_notes.iter().any(|note| note.contains("段落偏长")),
        "{lively_notes:?}"
    );
}

#[test]
fn a_repeated_archive_number_is_not_an_overused_phrase() {
    use ai_novel_production_system_core::chapter_craft::overused_phrases;
    let line = "他调出档案B-4-1998-07-22，登录账号ZhouYQ-0724。".to_string();
    assert!(overused_phrases(&[line.clone(), line.clone()], &line).is_empty());
    let prose = "他把那张泛黄的旧照片翻过来看了很久。".to_string();
    assert!(!overused_phrases(&[prose.clone(), prose.clone()], &prose).is_empty());
}

#[test]
fn a_sentence_used_a_third_time_is_a_finding_the_rewrite_must_fix() {
    // Real run 2026-10-24: given only the phrase, the rewrite fixed nine
    // findings and left the third use alone.
    use ai_novel_production_system_core::chapter_craft::{overused_phrases, repetition_findings};
    let history = vec![
        "他想起那只搪瓷杯。今天早上，它不在玻璃柜里。门外有人喊他。".to_string(),
        "镜子起了雾。今天早上，它不在玻璃柜里。他记得昨晚还在。".to_string(),
    ];
    let body = "他想起姐姐的杯子。今天早上，它不在玻璃柜里。林薇在客厅喊他。";
    let findings = repetition_findings(&overused_phrases(&history, body), body);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["code"], "word_repetition");
    assert_eq!(findings[0]["quote"], "今天早上，它不在玻璃柜里。");
    assert_eq!(findings[0]["source"], "rule");
    assert!(repetition_findings(&overused_phrases(&history[..1], body), body).is_empty());
}

#[test]
fn a_stock_trope_counts_as_a_habit_so_a_rewrite_cannot_introduce_one() {
    use ai_novel_production_system_core::chapter_craft::habit_count;
    assert_eq!(habit_count("林满没吭声，转身进了面馆。"), 0);
    assert_eq!(habit_count("林满倒吸一口凉气，转身进了面馆。"), 1);
}

#[test]
fn camera_and_thermal_imager_words_are_not_similes() {
    // Real run 2026-10-25: 摄像头 and 热成像仪 were counted as similes.
    use ai_novel_production_system_core::chapter_craft::habit_count;
    assert_eq!(habit_count("他打开摄像头，林薇举起热成像仪。"), 0);
}

#[test]
fn a_short_line_repeated_as_its_own_paragraph_is_a_finding_the_rewrite_must_fix() {
    // Real run 2026-10-25: “他也在看。” stood alone four times in chapter 2;
    // signing F1-5 counts windows across paragraphs, the rewrite did not.
    use ai_novel_production_system_core::chapter_craft::{overused_phrases, repetition_findings};
    let body = "他展开便签。\n\n“他也在看。”\n\n沈砚把便签翻过来。\n\n他走到通风口。\n\n“他也在看。”\n\n沈砚抬头。\n\n他走出面馆。\n\n“他也在看。”\n\n沈砚抬头。";
    let findings = repetition_findings(&overused_phrases(&[], body), body);
    assert!(findings.len() >= 2, "{findings:?}");
    for finding in &findings {
        let quote = finding["quote"].as_str().unwrap();
        assert!(quote.contains("他也在看"), "{quote}");
        assert_eq!(body.matches(quote).count(), 1, "a rewrite can locate {quote}");
    }
}
