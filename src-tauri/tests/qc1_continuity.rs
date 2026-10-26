//! QC-1.4 and QC-1.5 (owner decision 2026-10-26): catchphrases a chapter
//! leans on and sentences that contradict what earlier chapters fixed are
//! found before the passage rewrite, which then answers each of them.

use ai_novel_production_system_core::chapter_craft::{
    catchphrase_candidates, catchphrase_findings, contradiction_findings, copied_sentence_findings,
    credible_contradictions, opening_threat_on_page, plan_feedback,
};
use serde_json::json;

const EARLIER: [&str; 3] = [
    "“理论上，这不归我管。”林薇说。理论上他该走了。他低头看表。",
    "“理论上可以。”林薇把咖啡推过去。沈砚低头看了一眼。理论上没人会查。",
    "林薇摇头：“理论上不行。”他低头看着账本。窗外下雨。",
];

#[test]
fn qc1_a_short_phrase_used_across_recent_chapters_is_a_catchphrase_candidate() {
    let history = EARLIER
        .iter()
        .map(|text| text.to_string())
        .collect::<Vec<_>>();
    let body = "“理论上，你不该来。”林薇说。理论上他也不想来。他低头看表。林薇笑了。";
    let candidates = catchphrase_candidates(&history, body, &["林薇", "沈砚"], "");
    let phrases = candidates
        .iter()
        .map(|candidate| candidate["phrase"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(phrases.contains(&"理论上"), "{candidates:?}");
    assert!(
        !phrases.iter().any(|phrase| phrase.contains("林薇")),
        "names are never catchphrases: {candidates:?}"
    );
    let theory = candidates
        .iter()
        .find(|candidate| candidate["phrase"] == "理论上")
        .unwrap();
    assert_eq!(theory["thisChapter"], 2);
}

#[test]
fn qc1_every_use_of_a_catchphrase_after_the_first_is_a_rewrite_finding() {
    let body = "“理论上，你不该来。”林薇说。理论上他也不想来。他低头看表。理论上没人会查。";
    let findings = catchphrase_findings(&["理论上".to_string()], body);
    let quotes = findings
        .iter()
        .map(|finding| finding["quote"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(quotes, ["理论上他也不想来。", "理论上没人会查。"]);
    assert!(findings
        .iter()
        .all(|finding| finding["code"] == "catchphrase"));
}

#[test]
fn qc1_a_contradicting_sentence_is_a_rewrite_finding_naming_what_it_contradicts() {
    let body = "沈砚走进档案室。他抬起左手，用小指在灰上划了一道。林薇没回头。";
    let findings = contradiction_findings(
        &[
            json!({"sentence":1,"chapter":1,"established":"沈砚左手小指触觉永久消失","conflict":"用失去触觉的小指感觉灰尘"}),
        ],
        body,
    );
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["code"], "continuity_contradiction");
    assert_eq!(findings[0]["quote"], "他抬起左手，用小指在灰上划了一道。");
    assert!(findings[0]["problem"]
        .as_str()
        .unwrap()
        .contains("第 1 章：沈砚左手小指触觉永久消失"));
}

fn earlier_facts() -> Vec<serde_json::Value> {
    vec![
        json!({"n":0,"chapter":1,"subject":"沈砚","fact":"左手小指触觉永久消失"}),
        json!({"n":1,"chapter":3,"subject":"沈砚","fact":"右手中指指腹皮肤完全透明"}),
        json!({"n":2,"chapter":5,"subject":"林薇","fact":"林薇不喝咖啡"}),
    ]
}

fn sentences() -> Vec<String> {
    [
        "沈砚走进档案室。",
        "他用左手小指在灰上划了一道，指尖传来粗糙的触感。",
        "右手中指指腹皮肤完全透明。",
        "林薇端起咖啡喝了一大口。",
        "窗外下着雨。",
    ]
    .iter()
    .map(|sentence| sentence.to_string())
    .collect()
}

#[test]
fn qc1_only_a_credible_contradiction_of_an_earlier_chapter_is_kept() {
    // Acceptance 2026-10-26 (third run): 169 contradictions, nearly all
    // restatements, omissions or "earlier" facts from the same chapter.
    let reported = [
        // A real clash with chapter 1.
        json!({"sentence":1,"fact":0,"conflict":"失去触觉的小指感到粗糙","impossibleTogether":true}),
        // Restates the fact.
        json!({"sentence":2,"fact":1,"conflict":"透明","impossibleTogether":true}),
        // The model itself says both can be true.
        json!({"sentence":3,"fact":2,"conflict":"喝咖啡","impossibleTogether":false}),
        // Unrelated sentence.
        json!({"sentence":4,"fact":0,"conflict":"下雨","impossibleTogether":true}),
        // A fact from this very chapter is not earlier.
        json!({"sentence":3,"fact":2,"conflict":"喝咖啡","impossibleTogether":true}),
        // Out of range.
        json!({"sentence":9,"fact":0,"conflict":"x","impossibleTogether":true}),
        // The same sentence again.
        json!({"sentence":1,"fact":0,"conflict":"重复","impossibleTogether":true}),
    ];
    let kept = credible_contradictions(&reported, &earlier_facts(), &sentences(), 5).unwrap();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0]["sentence"], 1);
    assert_eq!(kept[0]["chapter"], 1);
    assert_eq!(kept[0]["established"], "左手小指触觉永久消失");
}

#[test]
fn qc1_a_check_reporting_too_many_contradictions_is_not_trusted() {
    let reported = (0..16)
        .map(|_| json!({"sentence":1,"fact":0,"conflict":"x","impossibleTogether":true}))
        .collect::<Vec<_>>();
    assert!(credible_contradictions(&reported, &earlier_facts(), &sentences(), 5).is_err());
}

#[test]
fn qc1_a_long_sentence_copied_from_a_recent_chapter_is_a_rewrite_finding() {
    let history = vec!["他把搪瓷猫碗放回窗台，碗沿缺耳处的裂口朝外。林薇没回头。".to_string()];
    let body = "沈砚进门。他把搪瓷猫碗放回窗台，碗沿缺耳处的裂口朝外。林薇没回头。";
    let findings = copied_sentence_findings(&history, body);
    assert_eq!(findings.len(), 1, "short sentences may recur: {findings:?}");
    assert_eq!(findings[0]["code"], "copied_sentence");
    assert_eq!(
        findings[0]["quote"],
        "他把搪瓷猫碗放回窗台，碗沿缺耳处的裂口朝外。"
    );
}

#[test]
fn qc1_the_opening_threat_must_show_within_its_first_600_characters() {
    let plan = json!({"craft":{"scenes":[{"conflict":"两人争键盘；迫近的威胁：档案局今晚十二点前清除王女士的全部记忆档案（期限：今晚十二点前）"}]}});
    let early = format!(
        "通知弹出：今晚十二点前，王女士的记忆档案将被清除。{}",
        "他低头记账。".repeat(120)
    );
    let late = format!(
        "{}通知弹出：今晚十二点前，王女士的记忆档案将被清除。",
        "他低头记账。".repeat(120)
    );
    assert!(opening_threat_on_page(&plan, &early));
    assert!(!opening_threat_on_page(&plan, &late));
    assert!(
        opening_threat_on_page(&json!({"craft":{"scenes":[]}}), &late),
        "no planned threat, nothing to check"
    );
}

#[test]
fn qc1_what_the_story_names_is_never_a_catchphrase_candidate() {
    let history = EARLIER
        .iter()
        .map(|text| text.to_string())
        .collect::<Vec<_>>();
    let body = "“理论上，你不该来。”林薇说。理论上他也不想来。他低头看表。林薇笑了。";
    let candidates = catchphrase_candidates(&history, body, &[], "规划：林薇说理论上该走");
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate["phrase"] == "理论上"),
        "{candidates:?}"
    );
}

#[test]
fn qc1_any_plan_contract_violation_goes_back_to_the_planner() {
    let long = format!(
        "B08_PLANNING_CONTRACT:CHAPTER_PLAN_CONTRACT:[{}] has more than 3 items",
        "{}".repeat(200)
    );
    let feedback = plan_feedback(&long).expect("replannable");
    assert_eq!(feedback["rejectedFor"], "CHAPTER_PLAN_CONTRACT");
    let problem = feedback["problems"][0].as_str().unwrap();
    assert!(
        problem.ends_with("has more than 3 items") && problem.chars().count() <= 161,
        "{problem}"
    );
    let short =
        plan_feedback("CHAPTER_PLAN_CONTRACT:\"readerQuestion\" is a required property").unwrap();
    assert_eq!(
        short["problems"][0],
        "\"readerQuestion\" is a required property"
    );
}
