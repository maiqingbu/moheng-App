//! CRAFT v3 step 1b: prose metrics, the archetype library and the three
//! deterministic style-card candidates.

use ai_novel_production_system_core::style_card::{
    archetype, archetype_candidates, archetypes, measure, recommend_archetypes, sample_candidates,
    MIN_SAMPLE_CHARS,
};

const NOW: &str = "2026-10-23T00:00:00Z";
const DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn measure_counts_paragraphs_dialogue_and_markers_per_thousand_chars() {
    // 4 paragraphs, 45 non-space chars; one dialogue paragraph, three short ones.
    let text = "他推门进去，屋里像是没人住过一样安静极了。\n“有人吗？”\n没人回答……\n他站了很久，因为腿麻了！";
    let metrics = measure(text);
    assert_eq!(metrics.chars, 45);
    assert_eq!(metrics.dialogue_paragraph_pct, 25.0);
    assert_eq!(metrics.short_paragraph_pct, 75.0);
    assert!(metrics.simile_per_1k > 0.0);
    assert!(metrics.explain_per_1k > 0.0);
    assert!(metrics.question_per_1k > 0.0);
    assert!(metrics.exclaim_per_1k > 0.0);
    assert!(metrics.ellipsis_per_1k > 0.0);
}

#[test]
fn every_archetype_pair_yields_three_valid_distinct_cards() {
    let ids = archetypes()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 8);
    for primary in &ids {
        for secondary in ids.iter().filter(|id| *id != primary) {
            let cards = archetype_candidates(primary, secondary, 1, NOW).unwrap();
            assert_eq!(cards.len(), 3);
            let variants = cards
                .iter()
                .map(|card| card["variant"].as_str().unwrap())
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(variants.len(), 3);
        }
    }
}

#[test]
fn rhythm_candidate_takes_the_secondary_rhythm_and_keeps_primary_rules() {
    let cards = archetype_candidates("A2", "A3", 1, NOW).unwrap();
    let (a2, a3) = (archetype("A2").unwrap(), archetype("A3").unwrap());
    assert_eq!(
        cards[1]["targets"]["shortParagraphPct"],
        a3["targets"]["shortParagraphPct"]
    );
    assert_eq!(
        cards[1]["targets"]["similePer1k"],
        a2["targets"]["similePer1k"]
    );
    assert_eq!(cards[1]["rules"], a2["rules"]);
    assert_eq!(cards[2]["narration"], a3["narration"]);
    assert!(archetype_candidates("A2", "A2", 1, NOW).is_err());
}

#[test]
fn recommends_archetypes_by_genre_and_falls_back_to_general_ones() {
    assert_eq!(recommend_archetypes("悬疑灵异 民俗").unwrap()[0], "A3");
    assert_eq!(recommend_archetypes("现代言情").unwrap()[0], "A6");
    assert_eq!(recommend_archetypes("完全未知").unwrap(), vec!["A1", "A2"]);
}

#[test]
fn sample_candidates_keep_only_metrics_and_digest_never_sample_text() {
    let paragraph = "林满把账本合上，数了数抽屉里的零钱，一共四十二块。“明天再说。”他对自己讲。\n";
    let sample = paragraph.repeat(MIN_SAMPLE_CHARS / 30 + 2);
    let cards = sample_candidates(&sample, DIGEST, 1, NOW).unwrap();
    assert_eq!(cards.len(), 3);
    let metrics = measure(&sample);
    let measured = &cards[0]["targets"]["avgParagraphChars"];
    assert!(measured["min"].as_f64().unwrap() <= metrics.avg_paragraph_chars);
    assert!(measured["max"].as_f64().unwrap() >= metrics.avg_paragraph_chars);
    for card in &cards {
        assert_eq!(card["source"]["kind"], "USER_SAMPLE");
        assert_eq!(card["source"]["sampleDigest"], DIGEST);
        let serialized = card.to_string();
        assert!(!serialized.contains("林满把账本合上"));
    }
    assert_eq!(
        sample_candidates("太短了", DIGEST, 1, NOW).unwrap_err(),
        "STYLE_SAMPLE_TOO_SHORT"
    );
}

#[test]
fn metric_keys_match_the_style_card_schema_targets() {
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../contracts/schemas/narrative/style-card-r1.schema.json"
    ))
    .unwrap();
    let mut required = schema["properties"]["targets"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| key.as_str().unwrap())
        .collect::<Vec<_>>();
    let mut keys = measure("").by_target().map(|(key, _)| key).to_vec();
    required.sort();
    keys.sort();
    assert_eq!(keys, required);
}
