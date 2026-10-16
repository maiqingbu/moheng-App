//! Deterministic Canon semantic conflict evaluation owned by the existing
//! versioned GatePolicy authority. This module has no SQL and no write path.

use crate::hash::sha256_canonical;
use crate::repositories::narrative::SemanticCanonFactRecord;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticOutcome {
    NoConflict(&'static str),
    Conflict(&'static str),
    Unresolved(&'static str),
}

impl SemanticOutcome {
    pub fn error_code(&self) -> Option<&'static str> {
        match self {
            Self::NoConflict(_) => None,
            Self::Conflict(_) => Some("B08_CANON_CONFLICT_CHANGE_REQUEST_REQUIRED"),
            Self::Unresolved(reason) => Some(reason),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CandidateSemanticInput<'a> {
    pub object_json: &'a Value,
    pub story_time: Option<&'a str>,
    pub valid_from_chapter_no: Option<i64>,
    pub valid_to_chapter_no: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct CanonSemanticPolicy {
    pub version: String,
    rules: BTreeMap<String, PredicateRule>,
}

#[derive(Debug, Clone)]
struct PredicateRule {
    cardinality: Cardinality,
    scope: ScopeRule,
    time_basis: Option<TimeBasis>,
    mutual_exclusions: Vec<(Value, Value)>,
    polarity_field: Option<String>,
    equivalence_sets: Vec<Vec<Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cardinality {
    Single,
    Multi,
}

#[derive(Debug, Clone)]
enum ScopeRule {
    Fixed(String),
    ObjectField(String),
}

#[derive(Debug, Clone, Copy)]
enum TimeBasis {
    WorldTime,
    ValidityChapter,
}

impl CanonSemanticPolicy {
    pub fn from_gate_policy(payload: &Value) -> Result<Self, &'static str> {
        let semantics = payload
            .get("canonSemantics")
            .and_then(Value::as_object)
            .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS")?;
        let version = semantics
            .get("policyVersion")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS")?
            .to_owned();
        let rule_values = semantics
            .get("rules")
            .and_then(Value::as_array)
            .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS")?;
        let mut rules = BTreeMap::new();
        for value in rule_values {
            let object = value
                .as_object()
                .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS")?;
            let predicate = object
                .get("predicate")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS")?
                .to_owned();
            let cardinality = match object.get("cardinality").and_then(Value::as_str) {
                Some("SINGLE") => Cardinality::Single,
                Some("MULTI") => Cardinality::Multi,
                _ => return Err("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS"),
            };
            let scope = match object.get("scope").and_then(Value::as_object) {
                Some(scope) if scope.get("kind").and_then(Value::as_str) == Some("FIXED") => {
                    ScopeRule::Fixed(
                        scope
                            .get("value")
                            .and_then(Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                            .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_SCOPE")?
                            .to_owned(),
                    )
                }
                Some(scope)
                    if scope.get("kind").and_then(Value::as_str) == Some("OBJECT_FIELD") =>
                {
                    ScopeRule::ObjectField(
                        scope
                            .get("field")
                            .and_then(Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                            .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_SCOPE")?
                            .to_owned(),
                    )
                }
                _ => return Err("B08_CANON_SEMANTICS_UNRESOLVED_SCOPE"),
            };
            let time_basis = match object.get("timeBasis").and_then(Value::as_str) {
                None => None,
                Some("WORLD_TIME") => Some(TimeBasis::WorldTime),
                Some("VALIDITY_CHAPTER") => Some(TimeBasis::ValidityChapter),
                _ => return Err("B08_CANON_SEMANTICS_UNRESOLVED_TIME"),
            };
            let mutual_exclusions = object
                .get("mutualExclusions")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| {
                            let pair = item
                                .as_object()
                                .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_EQUIVALENCE")?;
                            Ok::<(Value, Value), &'static str>((
                                pair.get("left")
                                    .cloned()
                                    .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_EQUIVALENCE")?,
                                pair.get("right")
                                    .cloned()
                                    .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_EQUIVALENCE")?,
                            ))
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default();
            let equivalence_sets = object
                .get("equivalenceSets")
                .and_then(Value::as_array)
                .map(|sets| {
                    sets.iter()
                        .map(|set| {
                            set.as_array()
                                .filter(|items| !items.is_empty())
                                .cloned()
                                .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_EQUIVALENCE")
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default();
            if rules
                .insert(
                    predicate,
                    PredicateRule {
                        cardinality,
                        scope,
                        time_basis,
                        mutual_exclusions,
                        polarity_field: object
                            .get("polarityField")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        equivalence_sets,
                    },
                )
                .is_some()
            {
                return Err("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS");
            }
        }
        Ok(Self { version, rules })
    }

    pub fn validate_predicate(&self, predicate: &str) -> Result<(), String> {
        self.rules
            .contains_key(predicate)
            .then_some(())
            .ok_or_else(|| {
                format!("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS:predicate={predicate}")
            })
    }

    /// FX-17 (owner decision 2026-10-16): a single-valued, chapter-validity
    /// fact that is still open and was established in an earlier chapter is
    /// replaced by a different value from a later chapter — a narrative
    /// transition, not a contradiction. A dead subject coming back to life
    /// still requires a change request; same-chapter conflicts are unchanged.
    pub fn narrative_transition(
        &self,
        predicate: &str,
        current: &SemanticCanonFactRecord,
        candidate: CandidateSemanticInput<'_>,
    ) -> Result<bool, &'static str> {
        let Some(rule) = self.rules.get(predicate) else {
            return Ok(false);
        };
        let later_chapter = current
            .valid_from_chapter_no
            .zip(candidate.valid_from_chapter_no)
            .is_some_and(|(from, candidate_from)| from < candidate_from);
        let same_scope = resolve_scope(&rule.scope, &current.object_json).ok()
            == resolve_scope(&rule.scope, candidate.object_json).ok();
        // A World Time rule with no World Time on either side cannot be
        // compared and falls back to Narrative Order, as Knowledge does.
        let unknown = |time: Option<&str>| {
            time.is_none_or(|time| time.trim().is_empty() || time == "UNKNOWN")
        };
        let narrative_order = match rule.time_basis {
            Some(TimeBasis::ValidityChapter) => true,
            Some(TimeBasis::WorldTime) => {
                unknown(current.story_time_from.as_deref())
                    && unknown(current.story_time_to.as_deref())
                    && unknown(candidate.story_time)
            }
            None => false,
        };
        if rule.cardinality != Cardinality::Single
            || !narrative_order
            || current.valid_to_chapter_no.is_some()
            || !later_chapter
            || !same_scope
            || equivalent(
                &rule.equivalence_sets,
                &current.object_json,
                candidate.object_json,
            )
        {
            return Ok(false);
        }
        let alive = |object: &Value| {
            object
                .get("value")
                .or_else(|| object.get("alive"))
                .and_then(Value::as_bool)
        };
        if predicate == "alive"
            && alive(&current.object_json) == Some(false)
            && alive(candidate.object_json) != Some(false)
        {
            return Err("B08_CANON_CONFLICT_CHANGE_REQUEST_REQUIRED");
        }
        Ok(true)
    }

    pub fn evaluate(
        &self,
        predicate: &str,
        current: &SemanticCanonFactRecord,
        candidate: CandidateSemanticInput<'_>,
    ) -> SemanticOutcome {
        let Some(rule) = self.rules.get(predicate) else {
            return SemanticOutcome::Unresolved(
                "B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS",
            );
        };
        let candidate_scope = match resolve_scope(&rule.scope, candidate.object_json) {
            Ok(scope) => scope,
            Err(code) => return SemanticOutcome::Unresolved(code),
        };
        let current_scope = match resolve_scope(&rule.scope, &current.object_json) {
            Ok(scope) => scope,
            Err(code) => return SemanticOutcome::Unresolved(code),
        };
        if candidate_scope != current_scope {
            return SemanticOutcome::NoConflict("NO_CONFLICT_DIFFERENT_SCOPE");
        }
        if equivalent(
            &rule.equivalence_sets,
            &current.object_json,
            candidate.object_json,
        ) {
            return SemanticOutcome::NoConflict("EQUIVALENT_DUPLICATE");
        }
        let requires_time = rule.cardinality == Cardinality::Single
            || !rule.mutual_exclusions.is_empty()
            || rule.polarity_field.is_some();
        if requires_time {
            let time = match rule.time_basis {
                Some(basis) => match compare_time(basis, current, &candidate) {
                    Ok(relation) => relation,
                    Err(code) => return SemanticOutcome::Unresolved(code),
                },
                None => return SemanticOutcome::Unresolved("B08_CANON_TIME_UNRESOLVED"),
            };
            if time == TimeRelation::Disjoint {
                return SemanticOutcome::NoConflict("NO_CONFLICT_DISJOINT_TIME");
            }
        }
        if let Some(field) = &rule.polarity_field {
            let current_polarity = current.object_json.get(field).and_then(Value::as_str);
            let candidate_polarity = candidate.object_json.get(field).and_then(Value::as_str);
            match (current_polarity, candidate_polarity) {
                (Some(left), Some(right)) if left != right => {
                    return SemanticOutcome::Conflict("CONFLICT_POLARITY")
                }
                (Some(_), Some(_)) => {}
                _ => return SemanticOutcome::Unresolved("B08_CANON_SEMANTICS_UNRESOLVED_POLARITY"),
            }
        }
        if rule.mutual_exclusions.iter().any(|(left, right)| {
            pair_matches(left, right, &current.object_json, candidate.object_json)
        }) {
            return SemanticOutcome::Conflict("CONFLICT_MUTUAL_EXCLUSION");
        }
        match rule.cardinality {
            Cardinality::Single => SemanticOutcome::Conflict("CONFLICT_SINGLE"),
            Cardinality::Multi => SemanticOutcome::NoConflict("NO_CONFLICT_MULTI"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeRelation {
    Overlap,
    Disjoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct WorldTime {
    day_number: i64,
    second_of_day: u32,
    nanosecond: u32,
}

pub(crate) fn parse_world_time(value: &str) -> Result<WorldTime, ()> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return Err(());
    }

    let number = |start: usize, end: usize| -> Result<u32, ()> {
        let digits = bytes.get(start..end).ok_or(())?;
        if digits.iter().any(|byte| !byte.is_ascii_digit()) {
            return Err(());
        }
        digits
            .iter()
            .try_fold(0_u32, |value, byte| {
                value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
            })
            .ok_or(())
    };

    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    if year == 0 || !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
        return Err(());
    }

    let (zone_start, offset_seconds) = if bytes.last() == Some(&b'Z') {
        (bytes.len() - 1, 0_i64)
    } else {
        let start = bytes.len().checked_sub(6).ok_or(())?;
        let sign = *bytes.get(start).ok_or(())?;
        if !matches!(sign, b'+' | b'-') || bytes.get(start + 3) != Some(&b':') {
            return Err(());
        }
        let offset_hour = number(start + 1, start + 3)?;
        let offset_minute = number(start + 4, start + 6)?;
        if offset_hour > 23 || offset_minute > 59 {
            return Err(());
        }
        let magnitude = i64::from(offset_hour * 60 + offset_minute) * 60;
        let offset_seconds = if sign == b'+' { magnitude } else { -magnitude };
        (start, offset_seconds)
    };
    if zone_start < 19 {
        return Err(());
    }

    let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_lengths = [
        31,
        if leap_year { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let month_length = month_lengths[(month - 1) as usize];
    if day == 0 || day > month_length {
        return Err(());
    }

    let fraction = bytes.get(19..zone_start).ok_or(())?;
    let nanosecond = if fraction.is_empty() {
        0
    } else if fraction.first() == Some(&b'.') {
        let fraction = &fraction[1..];
        if fraction.is_empty()
            || fraction.len() > 9
            || fraction.iter().any(|byte| !byte.is_ascii_digit())
        {
            return Err(());
        }
        let parsed = fraction
            .iter()
            .try_fold(0_u32, |value, byte| {
                value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
            })
            .ok_or(())?;
        parsed * 10_u32.pow((9 - fraction.len()) as u32)
    } else {
        return Err(());
    };

    let previous_year = i64::from(year - 1);
    let mut day_number =
        previous_year * 365 + previous_year / 4 - previous_year / 100 + previous_year / 400;
    for prior_month in 1..month {
        day_number += i64::from(month_lengths[(prior_month - 1) as usize]);
    }
    day_number += i64::from(day - 1);
    let utc_seconds =
        day_number * 86_400 + i64::from(hour * 3_600 + minute * 60 + second) - offset_seconds;
    Ok(WorldTime {
        day_number: utc_seconds.div_euclid(86_400),
        second_of_day: utc_seconds.rem_euclid(86_400) as u32,
        nanosecond,
    })
}

fn resolve_scope(rule: &ScopeRule, object: &Value) -> Result<String, &'static str> {
    match rule {
        ScopeRule::Fixed(value) => Ok(value.clone()),
        ScopeRule::ObjectField(field) => object
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(ToOwned::to_owned)
            .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_SCOPE"),
    }
}

fn canonical(value: &Value) -> Option<String> {
    sha256_canonical(value).ok()
}

fn equivalent(sets: &[Vec<Value>], left: &Value, right: &Value) -> bool {
    let Some(left_hash) = canonical(left) else {
        return false;
    };
    let Some(right_hash) = canonical(right) else {
        return false;
    };
    if left_hash == right_hash {
        return true;
    }
    sets.iter().any(|set| {
        let hashes = set.iter().filter_map(canonical).collect::<Vec<_>>();
        hashes.contains(&left_hash) && hashes.contains(&right_hash)
    })
}

fn pair_matches(left: &Value, right: &Value, current: &Value, candidate: &Value) -> bool {
    (equivalent(&[], left, current) && equivalent(&[], right, candidate))
        || (equivalent(&[], right, current) && equivalent(&[], left, candidate))
}

fn compare_time(
    basis: TimeBasis,
    current: &SemanticCanonFactRecord,
    candidate: &CandidateSemanticInput<'_>,
) -> Result<TimeRelation, &'static str> {
    match basis {
        TimeBasis::WorldTime => {
            let start = parse_world_time(
                current
                    .story_time_from
                    .as_deref()
                    .ok_or("B08_CANON_TIME_UNRESOLVED")?,
            )
            .map_err(|_| "B08_CANON_TIME_UNRESOLVED")?;
            let end = parse_world_time(
                current
                    .story_time_to
                    .as_deref()
                    .ok_or("B08_CANON_TIME_UNRESOLVED")?,
            )
            .map_err(|_| "B08_CANON_TIME_UNRESOLVED")?;
            let point = parse_world_time(candidate.story_time.ok_or("B08_CANON_TIME_UNRESOLVED")?)
                .map_err(|_| "B08_CANON_TIME_UNRESOLVED")?;
            if start > end {
                return Err("B08_CANON_TIME_UNRESOLVED");
            }
            Ok(if point < start || point > end {
                TimeRelation::Disjoint
            } else {
                TimeRelation::Overlap
            })
        }
        TimeBasis::ValidityChapter => {
            let start = current
                .valid_from_chapter_no
                .ok_or("B08_CANON_TIME_UNRESOLVED")?;
            let end = current
                .valid_to_chapter_no
                .ok_or("B08_CANON_TIME_UNRESOLVED")?;
            let candidate_start = candidate
                .valid_from_chapter_no
                .ok_or("B08_CANON_TIME_UNRESOLVED")?;
            let candidate_end = candidate.valid_to_chapter_no.unwrap_or(candidate_start);
            if start > end || candidate_start > candidate_end {
                return Err("B08_CANON_TIME_UNRESOLVED");
            }
            Ok(if candidate_end < start || candidate_start > end {
                TimeRelation::Disjoint
            } else {
                TimeRelation::Overlap
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(object: Value, from: Option<&str>, to: Option<&str>) -> SemanticCanonFactRecord {
        SemanticCanonFactRecord {
            fact_id: "fact:current".into(),
            object_json: object,
            valid_from_chapter_id: None,
            valid_to_chapter_id: None,
            story_time_from: from.map(ToOwned::to_owned),
            story_time_to: to.map(ToOwned::to_owned),
            valid_from_chapter_no: None,
            valid_to_chapter_no: None,
        }
    }

    fn policy(rule: Value) -> CanonSemanticPolicy {
        CanonSemanticPolicy::from_gate_policy(
            &json!({"canonSemantics":{"policyVersion":"v1","rules":[rule]}}),
        )
        .unwrap()
    }

    fn fx17_open_fact_from_chapter(object: Value, chapter_no: i64) -> SemanticCanonFactRecord {
        SemanticCanonFactRecord {
            valid_from_chapter_no: Some(chapter_no),
            ..record(object, None, None)
        }
    }

    fn fx17_candidate(object: &Value, chapter_no: i64) -> CandidateSemanticInput<'_> {
        CandidateSemanticInput {
            object_json: object,
            story_time: None,
            valid_from_chapter_no: Some(chapter_no),
            valid_to_chapter_no: Some(chapter_no),
        }
    }

    #[test]
    fn fx17_later_chapter_value_change_is_a_narrative_transition() {
        let single = policy(
            json!({"predicate":"status","cardinality":"SINGLE","scope":{"kind":"FIXED","value":"PROJECT"},"timeBasis":"VALIDITY_CHAPTER"}),
        );
        let current = fx17_open_fact_from_chapter(json!({"value":"小指透明"}), 1);
        let changed = json!({"value":"小指及无名指透明"});
        assert_eq!(
            single.narrative_transition("status", &current, fx17_candidate(&changed, 2)),
            Ok(true)
        );
        // same chapter, equivalent value, or a closed current fact: not a transition
        assert_eq!(
            single.narrative_transition("status", &current, fx17_candidate(&changed, 1)),
            Ok(false)
        );
        assert_eq!(
            single.narrative_transition(
                "status",
                &current,
                fx17_candidate(&json!({"value":"小指透明"}), 2)
            ),
            Ok(false)
        );
        let closed = SemanticCanonFactRecord {
            valid_to_chapter_no: Some(1),
            ..current.clone()
        };
        assert_eq!(
            single.narrative_transition("status", &closed, fx17_candidate(&changed, 2)),
            Ok(false)
        );
        let multi = policy(
            json!({"predicate":"opened","cardinality":"MULTI","scope":{"kind":"FIXED","value":"PROJECT"}}),
        );
        assert_eq!(
            multi.narrative_transition("opened", &current, fx17_candidate(&changed, 2)),
            Ok(false)
        );
    }

    #[test]
    fn fx17_world_time_rule_without_any_world_time_falls_back_to_narrative_order() {
        let world = policy(
            json!({"predicate":"location","cardinality":"SINGLE","scope":{"kind":"FIXED","value":"PROJECT"},"timeBasis":"WORLD_TIME"}),
        );
        let current = fx17_open_fact_from_chapter(json!({"place":"stone-door"}), 2);
        let moved = json!({"place":"river"});
        assert_eq!(
            world.narrative_transition("location", &current, fx17_candidate(&moved, 5)),
            Ok(true)
        );
        let timed = SemanticCanonFactRecord {
            story_time_from: Some("2020-01-01T00:00:00Z".into()),
            story_time_to: Some("2021-01-01T00:00:00Z".into()),
            ..current.clone()
        };
        assert_eq!(
            world.narrative_transition("location", &timed, fx17_candidate(&moved, 5)),
            Ok(false)
        );
        let mut timed_candidate = fx17_candidate(&moved, 5);
        timed_candidate.story_time = Some("2020-06-01T00:00:00Z");
        assert_eq!(
            world.narrative_transition("location", &current, timed_candidate),
            Ok(false)
        );
    }

    #[test]
    fn fx17_dead_subject_coming_back_to_life_is_still_a_conflict() {
        let alive = policy(
            json!({"predicate":"alive","cardinality":"SINGLE","scope":{"kind":"FIXED","value":"PROJECT"},"timeBasis":"VALIDITY_CHAPTER"}),
        );
        let dead = fx17_open_fact_from_chapter(json!({"value":false}), 1);
        assert_eq!(
            alive.narrative_transition("alive", &dead, fx17_candidate(&json!({"value":true}), 3)),
            Err("B08_CANON_CONFLICT_CHANGE_REQUEST_REQUIRED")
        );
        let living = fx17_open_fact_from_chapter(json!({"value":true}), 1);
        assert_eq!(
            alive.narrative_transition(
                "alive",
                &living,
                fx17_candidate(&json!({"value":false}), 3)
            ),
            Ok(true)
        );
    }

    #[test]
    fn multi_equivalence_scope_time_and_unknowns_are_deterministic() {
        let multi = policy(
            json!({"predicate":"owns","cardinality":"MULTI","scope":{"kind":"FIXED","value":"PROJECT"}}),
        );
        let current = record(json!({"item":"sword"}), None, None);
        assert_eq!(
            multi.evaluate(
                "owns",
                &current,
                CandidateSemanticInput {
                    object_json: &json!({"item":"map"}),
                    story_time: None,
                    valid_from_chapter_no: None,
                    valid_to_chapter_no: None
                }
            ),
            SemanticOutcome::NoConflict("NO_CONFLICT_MULTI")
        );
        assert_eq!(
            multi.evaluate(
                "owns",
                &current,
                CandidateSemanticInput {
                    object_json: &json!({"item":"sword"}),
                    story_time: None,
                    valid_from_chapter_no: None,
                    valid_to_chapter_no: None
                }
            ),
            SemanticOutcome::NoConflict("EQUIVALENT_DUPLICATE")
        );
        assert_eq!(
            multi.evaluate(
                "unknown",
                &current,
                CandidateSemanticInput {
                    object_json: &json!({"item":"map"}),
                    story_time: None,
                    valid_from_chapter_no: None,
                    valid_to_chapter_no: None
                }
            ),
            SemanticOutcome::Unresolved("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS")
        );
        assert_eq!(
            multi.validate_predicate("unknown").unwrap_err(),
            "B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS:predicate=unknown"
        );
    }

    #[test]
    fn single_world_time_scope_polarity_and_exclusions_fail_closed_correctly() {
        let single = policy(
            json!({"predicate":"residence","cardinality":"SINGLE","scope":{"kind":"OBJECT_FIELD","field":"scope"},"timeBasis":"WORLD_TIME","polarityField":"polarity","mutualExclusions":[{"left":{"scope":"alpha","value":"A","polarity":"POSITIVE"},"right":{"scope":"alpha","value":"B","polarity":"NEGATIVE"}}]}),
        );
        let current = record(
            json!({"scope":"alpha","value":"A","polarity":"POSITIVE"}),
            Some("2020-01-01T00:00:00Z"),
            Some("2021-01-01T00:00:00Z"),
        );
        assert_eq!(
            single.evaluate(
                "residence",
                &current,
                CandidateSemanticInput {
                    object_json: &json!({"scope":"beta","value":"B","polarity":"NEGATIVE"}),
                    story_time: Some("2020-06-01T00:00:00Z"),
                    valid_from_chapter_no: None,
                    valid_to_chapter_no: None
                }
            ),
            SemanticOutcome::NoConflict("NO_CONFLICT_DIFFERENT_SCOPE")
        );
        assert_eq!(
            single.evaluate(
                "residence",
                &current,
                CandidateSemanticInput {
                    object_json: &json!({"scope":"alpha","value":"B","polarity":"NEGATIVE"}),
                    story_time: Some("2030-01-01T00:00:00Z"),
                    valid_from_chapter_no: None,
                    valid_to_chapter_no: None
                }
            ),
            SemanticOutcome::NoConflict("NO_CONFLICT_DISJOINT_TIME")
        );
        assert_eq!(
            single.evaluate(
                "residence",
                &current,
                CandidateSemanticInput {
                    object_json: &json!({"scope":"alpha","value":"B","polarity":"NEGATIVE"}),
                    story_time: Some("2020-06-01T00:00:00Z"),
                    valid_from_chapter_no: None,
                    valid_to_chapter_no: None
                }
            ),
            SemanticOutcome::Conflict("CONFLICT_POLARITY")
        );
        assert_eq!(
            single.evaluate(
                "residence",
                &current,
                CandidateSemanticInput {
                    object_json: &json!({"scope":"alpha","value":"B","polarity":"POSITIVE"}),
                    story_time: None,
                    valid_from_chapter_no: None,
                    valid_to_chapter_no: None
                }
            ),
            SemanticOutcome::Unresolved("B08_CANON_TIME_UNRESOLVED")
        );
    }

    #[test]
    fn world_time_requires_valid_utc_and_never_orders_malformed_strings() {
        let single = policy(json!({
            "predicate":"location",
            "cardinality":"SINGLE",
            "scope":{"kind":"FIXED","value":"PROJECT"},
            "timeBasis":"WORLD_TIME"
        }));
        let current = record(
            json!({"place":"A"}),
            Some("2020-10-01T00:00:00Z"),
            Some("2020-2-01T00:00:00Z"),
        );
        let malformed_interval = single.evaluate(
            "location",
            &current,
            CandidateSemanticInput {
                object_json: &json!({"place":"B"}),
                story_time: Some("2020-12-01T00:00:00Z"),
                valid_from_chapter_no: None,
                valid_to_chapter_no: None,
            },
        );
        assert_eq!(
            malformed_interval,
            SemanticOutcome::Unresolved("B08_CANON_TIME_UNRESOLVED")
        );

        let valid_interval = record(
            json!({"place":"A"}),
            Some("2020-01-01T00:00:00Z"),
            Some("2021-01-01T00:00:00Z"),
        );
        let malformed_candidate = single.evaluate(
            "location",
            &valid_interval,
            CandidateSemanticInput {
                object_json: &json!({"place":"B"}),
                story_time: Some("later"),
                valid_from_chapter_no: None,
                valid_to_chapter_no: None,
            },
        );
        assert_eq!(
            malformed_candidate,
            SemanticOutcome::Unresolved("B08_CANON_TIME_UNRESOLVED")
        );
    }

    #[test]
    fn fx4_flashback_event_passes() {
        let single = policy(json!({
            "predicate":"location",
            "cardinality":"SINGLE",
            "scope":{"kind":"FIXED","value":"PROJECT"},
            "timeBasis":"WORLD_TIME"
        }));
        let mut chapter_ten_event = record(
            json!({"place":"A"}),
            Some("2030-01-01T00:00:00Z"),
            Some("2030-01-01T00:00:00Z"),
        );
        chapter_ten_event.valid_from_chapter_no = Some(10);
        chapter_ten_event.valid_to_chapter_no = Some(10);
        let earlier_chapter_twenty_event = json!({"place":"B"});

        assert_eq!(
            single.evaluate(
                "location",
                &chapter_ten_event,
                CandidateSemanticInput {
                    object_json: &earlier_chapter_twenty_event,
                    story_time: Some("2020-01-01T00:00:00Z"),
                    valid_from_chapter_no: Some(20),
                    valid_to_chapter_no: Some(20),
                }
            ),
            SemanticOutcome::NoConflict("NO_CONFLICT_DISJOINT_TIME")
        );
    }

    #[test]
    fn fx4_world_time_normalizes_numeric_timezone_offsets() {
        assert_eq!(
            parse_world_time("2030-01-01T08:00:00+08:00"),
            parse_world_time("2030-01-01T00:00:00Z")
        );
        assert_eq!(
            parse_world_time("2029-12-31T19:00:00-05:00"),
            parse_world_time("2030-01-01T00:00:00Z")
        );
        assert_eq!(
            parse_world_time("2030-01-01T08:00:00.125000+08:00"),
            parse_world_time("2030-01-01T00:00:00.125Z")
        );
    }

    #[test]
    fn fx4_world_time_rejects_malformed_numeric_timezone_offsets() {
        for invalid in [
            "2030-01-01T08:00:00+8:00",
            "2030-01-01T08:00:00+08:60",
            "2030-01-01T08:00:00+24:00",
            "2030-01-01T08:00:00",
        ] {
            assert_eq!(parse_world_time(invalid), Err(()), "{invalid}");
        }
    }
}
