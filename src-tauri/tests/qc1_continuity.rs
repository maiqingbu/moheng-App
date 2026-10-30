//! QC-1.4 and QC-1.5 (owner decision 2026-10-26): catchphrases a chapter
//! leans on and sentences that contradict what earlier chapters fixed are
//! found before the passage rewrite, which then answers each of them.

use ai_novel_production_system_core::chapter_craft::{
    catchphrase_candidates, catchphrase_findings, contradiction_findings, copied_sentence_findings,
    credible_contradictions, detail_repetition_findings, opening_threat_on_page, plan_feedback,
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

#[test]
fn qc1_the_props_of_how_the_threat_shows_are_not_the_threat() {
    let plan = json!({"craft":{"scenes":[{"conflict":"争键盘；迫近的威胁：档案局要清除王女士的全部记忆档案（期限：今晚十二点前；开头写法：伙计把催债单拍在柜台上骂骂咧咧）"}]}});
    let props_only = format!(
        "伙计把催债单拍在柜台上骂骂咧咧。{}",
        "他低头记账。".repeat(120)
    );
    assert!(!opening_threat_on_page(&plan, &props_only));
}

#[test]
fn qc1_an_object_mentioned_past_three_times_in_a_chapter_is_reworded_from_the_fourth() {
    // Owner decision 2026-10-30; fifth acceptance run: 存钱罐 eleven times in
    // one chapter. The rewrite words the excess differently or leaves it
    // out; the object stays.
    let body =
        "他拿起存钱罐。存钱罐很旧。林薇看着存钱罐。他把存钱罐放下。存钱罐里有响声。窗外下雨。";
    let (findings, _) = detail_repetition_findings(&[], body, &["林薇"], &[]);
    let quotes = findings
        .iter()
        .map(|finding| finding["quote"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(quotes, ["他把存钱罐放下。", "存钱罐里有响声。"]);
    assert!(findings
        .iter()
        .all(|finding| finding["code"] == "detail_repetition"));
}

#[test]
fn qc1_a_detail_running_through_four_chapters_is_kept_twice() {
    // Fifth acceptance run: 左手小指第一节 in every chapter.
    let earlier = "他看了看左手小指第一节。左手小指第一节很凉。左手小指第一节透明。".to_string();
    let history = vec![earlier.clone(), earlier.clone(), earlier];
    let body = "左手小指第一节发麻。左手小指第一节发烫。他把左手小指第一节藏进口袋。";
    let (findings, running) = detail_repetition_findings(&history, body, &[], &[]);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["quote"], "他把左手小指第一节藏进口袋。");
    // The next writer hears which details ran through the chapters.
    assert!(
        running.iter().any(|phrase| phrase.contains("小指第一")),
        "{running:?}"
    );
}

#[test]
fn qc1_actions_and_grammar_fragments_are_not_details() {
    let body =
        "他伸手。他伸手。他伸手。他伸手。他把纤维捏住。他把纤维放下。他把纤维收好。他把纤维扔掉。";
    assert!(detail_repetition_findings(&[], body, &[], &[]).0.is_empty());
}

#[test]
fn qc1_names_of_people_are_never_capped() {
    let body = "林薇说话。林薇走了。林薇回来。林薇笑了。林薇哭了。";
    assert!(detail_repetition_findings(&[], body, &["林薇"], &[])
        .0
        .is_empty());
}

#[test]
fn qc1_detail_slots_go_round_the_phrases_and_skip_catchphrases() {
    let body = "存钱罐。存钱罐。存钱罐。存钱罐。存钱罐。存钱罐。存钱罐。存钱罐。帆布包。帆布包。帆布包。帆布包。一模一样。一模一样。一模一样。一模一样。";
    let (findings, _) = detail_repetition_findings(&[], body, &[], &["一模一样".to_string()]);
    let problems = findings
        .iter()
        .map(|finding| finding["problem"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(findings.len() <= 6, "{problems:?}");
    // 帆布包 gets its turn right after 存钱罐's first, not after all of them.
    assert!(problems[1].contains("帆布包"), "{problems:?}");
    assert!(
        !problems.iter().any(|problem| problem.contains("模一样")),
        "{problems:?}"
    );
}
