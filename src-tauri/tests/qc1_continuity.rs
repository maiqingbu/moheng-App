//! QC-1.4 and QC-1.5 (owner decision 2026-10-26): catchphrases a chapter
//! leans on and sentences that contradict what earlier chapters fixed are
//! found before the passage rewrite, which then answers each of them.

use ai_novel_production_system_core::chapter_craft::{
    catchphrase_candidates, catchphrase_findings, contradiction_findings,
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
    let candidates = catchphrase_candidates(&history, body, &["林薇", "沈砚"]);
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
