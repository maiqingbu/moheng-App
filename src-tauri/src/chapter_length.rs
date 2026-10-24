//! FX-18: chapter length range (owner decisions 2026-10-17).
//!
//! A chapter is written for a character range chosen per work (project
//! setting `chapterLengthRange`) and optionally per chapter (production
//! payload). The plan must be sized for the range and the committed body must
//! fall inside it, allowing up to 10% above the maximum.

use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChapterLengthRange {
    pub min: i64,
    pub max: i64,
}

pub const DEFAULT_CHAPTER_LENGTH_RANGE: ChapterLengthRange = ChapterLengthRange {
    min: 2_000,
    max: 3_000,
};

const SMALLEST_MIN: i64 = 500;
const LARGEST_MAX: i64 = 10_000;

impl ChapterLengthRange {
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let bound = |name: &str| value.get(name).and_then(Value::as_i64);
        match (bound("min"), bound("max")) {
            (Some(min), Some(max)) if SMALLEST_MIN <= min && min < max && max <= LARGEST_MAX => {
                Ok(Self { min, max })
            }
            _ => Err("CHAPTER_LENGTH_RANGE_INVALID".into()),
        }
    }

    /// The range a command payload chooses for its chapter: an explicit
    /// `chapterLengthRange`, or the legacy single `targetChineseCharacters`.
    pub fn from_payload(payload: &Value) -> Result<Option<Self>, String> {
        if let Some(range) = payload.get("chapterLengthRange") {
            return Self::from_value(range).map(Some);
        }
        if let Some(target) = payload
            .get("targetChineseCharacters")
            .and_then(Value::as_i64)
        {
            return Self::from_value(&json!({"min":target * 4 / 5,"max":target})).map(Some);
        }
        Ok(None)
    }

    /// A range chosen now for this chapter, then the range saved for this
    /// chapter earlier, then the work's setting, then the default.
    pub fn resolve(
        chosen: Option<Self>,
        chapter_setting: Option<&Value>,
        work_setting: Option<&Value>,
    ) -> Result<Self, String> {
        if let Some(range) = chosen {
            return Ok(range);
        }
        match chapter_setting
            .filter(|setting| !setting.is_null())
            .or(work_setting.filter(|setting| !setting.is_null()))
        {
            Some(setting) => Self::from_value(setting),
            None => Ok(DEFAULT_CHAPTER_LENGTH_RANGE),
        }
    }

    /// Bodies may run up to 10% past the maximum.
    pub fn ceiling(&self) -> i64 {
        self.max + self.max / 10
    }

    /// The length a chapter aims for: the middle of the range.
    pub fn target(&self) -> i64 {
        (self.min + self.max) / 2
    }

    pub fn accepts(&self, characters: i64) -> bool {
        self.min <= characters && characters <= self.ceiling()
    }

    pub fn to_json(self) -> Value {
        json!({"min":self.min,"max":self.max})
    }
}

/// The count shown to the user as the chapter's word count.
pub fn character_count(text: &str) -> i64 {
    text.chars().count() as i64
}

/// Sum of the plan's per-scene `estimatedCharacters`; None when any scene
/// lacks a positive estimate or there are no scenes.
pub fn plan_estimate(plan: &Value) -> Option<i64> {
    let scenes = plan.get("sceneExecution")?.as_array()?;
    if scenes.is_empty() {
        return None;
    }
    scenes.iter().try_fold(0_i64, |sum, scene| {
        scene
            .get("estimatedCharacters")
            .and_then(Value::as_i64)
            .filter(|estimate| *estimate > 0)
            .map(|estimate| sum + estimate)
    })
}

/// A plan is sized for the range when its scene estimates add up to a total
/// within [min, max].
pub fn validate_plan_length(plan: &Value, range: ChapterLengthRange) -> Result<(), String> {
    match plan_estimate(plan) {
        Some(total) if range.min <= total && total <= range.max => Ok(()),
        _ => Err("B08_PLAN_LENGTH_OUT_OF_RANGE".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fx18_range_resolution_prefers_chosen_then_saved_chapter_then_work_then_default() {
        let work = json!({"min":3000,"max":4000});
        let saved = json!({"min":2500,"max":3500});
        let range = |min, max| ChapterLengthRange { min, max };
        assert_eq!(
            ChapterLengthRange::from_payload(
                &json!({"chapterLengthRange":{"min":4000,"max":5000}})
            ),
            Ok(Some(range(4000, 5000)))
        );
        assert_eq!(
            ChapterLengthRange::from_payload(&json!({"targetChineseCharacters":1800})),
            Ok(Some(range(1440, 1800)))
        );
        assert_eq!(ChapterLengthRange::from_payload(&json!({})), Ok(None));
        assert_eq!(
            ChapterLengthRange::resolve(Some(range(4000, 5000)), Some(&saved), Some(&work)),
            Ok(range(4000, 5000))
        );
        assert_eq!(
            ChapterLengthRange::resolve(None, Some(&saved), Some(&work)),
            Ok(range(2500, 3500))
        );
        assert_eq!(
            ChapterLengthRange::resolve(None, None, Some(&work)),
            Ok(range(3000, 4000))
        );
        assert_eq!(
            ChapterLengthRange::resolve(None, None, None),
            Ok(DEFAULT_CHAPTER_LENGTH_RANGE)
        );
    }

    #[test]
    fn fx18_invalid_ranges_are_rejected() {
        for bad in [
            json!({"min":3000,"max":3000}),
            json!({"min":400,"max":1000}),
            json!({"min":2000,"max":12000}),
            json!({"min":"2000","max":3000}),
            json!({"max":3000}),
        ] {
            assert_eq!(
                ChapterLengthRange::from_value(&bad),
                Err("CHAPTER_LENGTH_RANGE_INVALID".into()),
                "{bad}"
            );
        }
    }

    #[test]
    fn fx18_body_may_exceed_the_maximum_by_ten_percent_only() {
        let range = ChapterLengthRange {
            min: 2000,
            max: 3000,
        };
        assert!(!range.accepts(1999));
        assert!(range.accepts(2000));
        assert!(range.accepts(3300));
        assert!(!range.accepts(3301));
        assert_eq!(character_count("林舟\n推门"), 5);
    }

    #[test]
    fn fx18_plan_must_be_sized_for_the_range() {
        let range = ChapterLengthRange {
            min: 2000,
            max: 3000,
        };
        let plan = |estimates: &[Value]| json!({"sceneExecution":estimates.iter().map(|estimate| json!({"objective":"场景","estimatedCharacters":estimate})).collect::<Vec<_>>()});
        assert!(validate_plan_length(&plan(&[json!(800), json!(900), json!(700)]), range).is_ok());
        assert_eq!(
            validate_plan_length(&plan(&[json!(250), json!(250)]), range),
            Err("B08_PLAN_LENGTH_OUT_OF_RANGE".into())
        );
        assert_eq!(
            validate_plan_length(&plan(&[json!(2000), json!(1500)]), range),
            Err("B08_PLAN_LENGTH_OUT_OF_RANGE".into())
        );
        assert_eq!(
            validate_plan_length(&json!({"sceneExecution":[{"objective":"无估计"}]}), range),
            Err("B08_PLAN_LENGTH_OUT_OF_RANGE".into())
        );
        assert_eq!(
            validate_plan_length(&json!({"sceneExecution":[]}), range),
            Err("B08_PLAN_LENGTH_OUT_OF_RANGE".into())
        );
    }
}
