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
