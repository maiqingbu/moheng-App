//! CRAFT v3 style cards: prose metrics, the market archetype library and the
//! three deterministic style-card candidates (CRAFT_V3_DESIGN §4.1).
//!
//! Candidates are built by rule, never by a model, so a user's sample text
//! stays on this machine: only its metrics and digest enter the card.

use crate::generated::validators::validate_style_card_r1;
use serde_json::{json, Value};
use std::sync::OnceLock;

const ARCHETYPES_JSON: &str =
    include_str!("../../contracts/resources/style-archetypes.r1/archetypes.json");
/// The marker set the market study measured with (STYLE_ARCHETYPES_R1.md §0).
/// It intentionally differs from `chapter_craft`'s simile habit list (which
/// excludes 好像): archetype targets are only comparable under this set.
const MARKET_SIMILE_MARKERS: &[&str] = &[
    "像是",
    "好像",
    "仿佛",
    "如同",
    "宛如",
    "好似",
    "犹如",
    "似的",
    "一般的",
];
const MARKET_EXPLAIN_MARKERS: &[&str] = &["因为", "所以", "因此", "于是", "由于", "之所以", "原来"];
/// Targets a "rhythm" variant takes from the secondary archetype
/// (STYLE_ARCHETYPES_R1.md §3).
const RHYTHM_TARGETS: &[&str] = &[
    "avgParagraphChars",
    "shortParagraphPct",
    "exclaimPer1k",
    "questionPer1k",
    "ellipsisPer1k",
];
/// Minimum sample size before its metrics mean anything.
pub const MIN_SAMPLE_CHARS: usize = 800;

#[derive(Debug, Clone, PartialEq)]
pub struct ProseMetrics {
    pub chars: usize,
    pub avg_paragraph_chars: f64,
    pub short_paragraph_pct: f64,
    pub dialogue_paragraph_pct: f64,
    pub simile_per_1k: f64,
    pub explain_per_1k: f64,
    pub exclaim_per_1k: f64,
    pub question_per_1k: f64,
    pub ellipsis_per_1k: f64,
}

impl ProseMetrics {
    /// The metrics keyed like `style-card.r1` targets.
    pub fn by_target(&self) -> [(&'static str, f64); 8] {
        [
            ("avgParagraphChars", self.avg_paragraph_chars),
            ("shortParagraphPct", self.short_paragraph_pct),
            ("dialogueParagraphPct", self.dialogue_paragraph_pct),
            ("similePer1k", self.simile_per_1k),
            ("explainPer1k", self.explain_per_1k),
            ("exclaimPer1k", self.exclaim_per_1k),
            ("questionPer1k", self.question_per_1k),
            ("ellipsisPer1k", self.ellipsis_per_1k),
        ]
    }
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn ratio(count: usize, total: usize, scale: f64) -> f64 {
    if total == 0 {
        0.0
    } else {
        round1(count as f64 * scale / total as f64)
    }
}

/// Measures prose the same way the market study did
/// (docs/development/STYLE_ARCHETYPES_R1.md §0).
pub fn measure(text: &str) -> ProseMetrics {
    let paragraphs = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let chars = text.chars().filter(|c| !c.is_whitespace()).count();
    let markers = |list: &[&str]| list.iter().map(|m| text.matches(m).count()).sum::<usize>();
    let chars_of = |set: &[char]| text.chars().filter(|c| set.contains(c)).count();
    let paragraphs_where = |test: fn(&str) -> bool| paragraphs.iter().filter(|p| test(p)).count();
    ProseMetrics {
        chars,
        avg_paragraph_chars: ratio(chars, paragraphs.len(), 1.0),
        short_paragraph_pct: ratio(
            paragraphs_where(|p| p.chars().count() <= 20),
            paragraphs.len(),
            100.0,
        ),
        dialogue_paragraph_pct: ratio(
            paragraphs_where(|p| p.contains(['“', '「', '"'])),
            paragraphs.len(),
            100.0,
        ),
        simile_per_1k: ratio(markers(MARKET_SIMILE_MARKERS), chars, 1000.0),
        explain_per_1k: ratio(markers(MARKET_EXPLAIN_MARKERS), chars, 1000.0),
        exclaim_per_1k: ratio(chars_of(&['！', '!']), chars, 1000.0),
        question_per_1k: ratio(chars_of(&['？', '?']), chars, 1000.0),
        ellipsis_per_1k: ratio(chars_of(&['…']), chars, 1000.0),
    }
}

struct Library {
    archetypes: Vec<Value>,
    fallback_ids: Vec<String>,
}

/// Parses the archetype resource and validates every archetype as a
/// style card once, so a bad entry fails at load rather than at selection.
fn load_library() -> Result<Library, String> {
    let invalid = |detail: &str| format!("STYLE_ARCHETYPES_INVALID:{detail}");
    let parsed: Value =
        serde_json::from_str(ARCHETYPES_JSON).map_err(|error| invalid(&error.to_string()))?;
    let archetypes = parsed["archetypes"]
        .as_array()
        .cloned()
        .filter(|list| !list.is_empty())
        .ok_or_else(|| invalid("archetypes"))?;
    for item in &archetypes {
        let check = card(
            "LOAD_CHECK",
            name(item).into(),
            source("MARKET_ARCHETYPE", &[id(item)], None),
            item,
            1,
            "2026-01-01T00:00:00Z",
        );
        validate_style_card_r1(&check)
            .map_err(|error| invalid(&format!("{}:{error}", id(item))))?;
    }
    let fallback_ids = parsed["fallbackIds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|fallback| archetypes.iter().any(|item| id(item) == *fallback))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if fallback_ids.len() < 2 {
        return Err(invalid("fallbackIds"));
    }
    Ok(Library {
        archetypes,
        fallback_ids,
    })
}

fn library() -> Result<&'static Library, String> {
    static LIBRARY: OnceLock<Result<Library, String>> = OnceLock::new();
    LIBRARY
        .get_or_init(load_library)
        .as_ref()
        .map_err(Clone::clone)
}

pub fn archetypes() -> Result<&'static [Value], String> {
    library().map(|lib| lib.archetypes.as_slice())
}

pub fn archetype(archetype_id: &str) -> Result<&'static Value, String> {
    archetypes()?
        .iter()
        .find(|item| id(item) == archetype_id)
        .ok_or_else(|| format!("STYLE_ARCHETYPE_UNKNOWN:{archetype_id}"))
}

fn id(item: &Value) -> &str {
    item["id"].as_str().unwrap_or_default()
}

fn name(item: &Value) -> &str {
    item["name"].as_str().unwrap_or_default()
}

/// Archetype ids whose genres appear in the given genre text, best match
/// first; topped up to two with the library's fallback archetypes.
pub fn recommend_archetypes(genre_text: &str) -> Result<Vec<String>, String> {
    let lib = library()?;
    let mut scored = lib
        .archetypes
        .iter()
        .map(|item| {
            let hits = item["genres"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|genre| genre_text.contains(genre))
                .count();
            (hits, id(item).to_string())
        })
        .filter(|(hits, _)| *hits > 0)
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut ids = scored.into_iter().map(|(_, id)| id).collect::<Vec<_>>();
    for fallback in &lib.fallback_ids {
        if ids.len() < 2 && !ids.contains(fallback) {
            ids.push(fallback.clone());
        }
    }
    Ok(ids)
}

fn source(kind: &str, ids: &[&str], sample_digest: Option<&str>) -> Value {
    json!({"kind":kind,"archetypeIds":ids,"sampleDigest":sample_digest})
}

fn card(
    variant: &str,
    label: String,
    source: Value,
    base: &Value,
    generation: i64,
    now: &str,
) -> Value {
    json!({
        "payloadSchema":"style-card.r1","stage":"CANDIDATE","generation":generation,
        "variant":variant,"label":label,"source":source,"confirmedFromRef":null,
        "targets":base["targets"],"narration":base["narration"],
        "openingStructure":base["openingStructure"],"rules":base["rules"],
        "forbidden":base["forbidden"],"examples":base["examples"],"createdAt":now
    })
}

/// Builds and validates three cards given as `(variant, label, source, base)`.
fn cards(
    specs: [(String, String, Value, &Value); 3],
    generation: i64,
    now: &str,
) -> Result<Vec<Value>, String> {
    specs
        .into_iter()
        .map(|(variant, label, source, base)| {
            let card = card(&variant, label, source, base, generation, now);
            validate_style_card_r1(&card)
                .map_err(|error| format!("STYLE_CARD_CONTRACT:{error}"))?;
            Ok(card)
        })
        .collect()
}

/// Three candidates from market archetypes: the primary as is, the primary
/// with the secondary's rhythm, and the primary with the secondary's
/// narration plus its first two rules.
pub fn archetype_candidates(
    primary: &str,
    secondary: &str,
    generation: i64,
    now: &str,
) -> Result<Vec<Value>, String> {
    let main = archetype(primary)?;
    let other = archetype(secondary)?;
    if primary == secondary {
        return Err("STYLE_ARCHETYPE_PAIR_INVALID".into());
    }
    let mut rhythm = main.clone();
    for key in RHYTHM_TARGETS {
        rhythm["targets"][key] = other["targets"][key].clone();
    }
    let mut narration = main.clone();
    narration["narration"] = other["narration"].clone();
    let mut rules = main["rules"].as_array().cloned().unwrap_or_default();
    rules.extend(
        other["rules"]
            .as_array()
            .into_iter()
            .flatten()
            .take(2)
            .cloned(),
    );
    rules.truncate(10);
    narration["rules"] = Value::Array(rules);
    let pair = source("MARKET_ARCHETYPE", &[primary, secondary], None);
    cards(
        [
            (
                format!("{primary}_PURE"),
                format!("{}（纯版）", name(main)),
                source("MARKET_ARCHETYPE", &[primary], None),
                main,
            ),
            (
                format!("{primary}_RHYTHM_{secondary}"),
                format!("{}·节奏取{}", name(main), name(other)),
                pair.clone(),
                &rhythm,
            ),
            (
                format!("{primary}_NARRATION_{secondary}"),
                format!("{}·叙述取{}", name(main), name(other)),
                pair,
                &narration,
            ),
        ],
        generation,
        now,
    )
}

fn distance(metrics: &ProseMetrics, item: &Value) -> f64 {
    metrics
        .by_target()
        .iter()
        .map(|(key, value)| {
            let min = item["targets"][key]["min"].as_f64().unwrap_or(0.0);
            let max = item["targets"][key]["max"].as_f64().unwrap_or(0.0);
            let span = (max - min).max(1.0);
            if *value < min {
                (min - value) / span
            } else if *value > max {
                (value - max) / span
            } else {
                0.0
            }
        })
        .sum()
}

/// Three candidates from a user sample: its measured targets with the
/// nearest archetype's rules, then the two nearest archetypes as they are.
/// Only the metrics and `sample_digest` leave this function.
pub fn sample_candidates(
    sample: &str,
    sample_digest: &str,
    generation: i64,
    now: &str,
) -> Result<Vec<Value>, String> {
    let metrics = measure(sample);
    if metrics.chars < MIN_SAMPLE_CHARS {
        return Err("STYLE_SAMPLE_TOO_SHORT".into());
    }
    let mut ranked = archetypes()?
        .iter()
        .map(|item| (distance(&metrics, item), item))
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (first, second) = (ranked[0].1, ranked[1].1);
    let mut measured = first.clone();
    for (key, value) in metrics.by_target() {
        measured["targets"][key] = json!({"min":round1(value * 0.85),"max":round1(value * 1.15)});
    }
    let from = |item: &Value| source("USER_SAMPLE", &[id(item)], Some(sample_digest));
    cards(
        [
            (
                "SAMPLE_MEASURED".into(),
                format!("范文实测·规则取{}", name(first)),
                from(first),
                &measured,
            ),
            (
                format!("SAMPLE_NEAREST_{}", id(first)),
                format!("最接近：{}", name(first)),
                from(first),
                first,
            ),
            (
                format!("SAMPLE_NEAREST_{}", id(second)),
                format!("次接近：{}", name(second)),
                from(second),
                second,
            ),
        ],
        generation,
        now,
    )
}
