use crate::catalog::CatalogRepository;
use crate::project_db::paths::AppPaths;
use libsql_rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const CATALOG_SETTING_KEY: &str = "modelPricing";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricingEntry {
    pub provider_id: String,
    pub model_id: String,
    pub input_price_per_million_tokens: f64,
    pub output_price_per_million_tokens: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_price_per_million_tokens: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricingVersion {
    pub version: i64,
    pub updated_at: String,
    pub command_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub entries: Vec<ModelPricingEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricingCatalog {
    pub version: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub entries: Vec<ModelPricingEntry>,
    #[serde(default)]
    pub history: Vec<ModelPricingVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelCostSnapshot {
    pub price_table_version: i64,
    pub provider_id: String,
    pub model_id: String,
    pub price_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_price_per_million_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_price_per_million_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_input_price_per_million_tokens: Option<f64>,
    pub estimated_input_tokens: i64,
    pub estimated_output_tokens: i64,
    pub predicted_cost_cents: i64,
}

impl ModelPricingCatalog {
    pub fn read(connection: &Connection) -> Result<Self, String> {
        CatalogRepository::setting(connection, CATALOG_SETTING_KEY)
            .map_err(|error| error.to_string())?
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|error| format!("MODEL_PRICING_CATALOG_INVALID:{error}"))
            })
            .unwrap_or_else(|| Ok(Self::default()))
    }

    pub fn read_from_paths(paths: &AppPaths, now: &str) -> Result<Self, String> {
        let connection = CatalogRepository::open(paths, now).map_err(|error| error.to_string())?;
        Self::read(&connection)
    }

    pub fn save(
        connection: &Connection,
        entries: Value,
        command_id: &str,
        idempotency_key: &str,
        request_digest: &str,
        now: &str,
    ) -> Result<Value, String> {
        let entries = validate_entries(&entries)?;
        let mut catalog = Self::read(connection)?;
        if let Some(previous) = catalog
            .history
            .iter()
            .find(|item| item.idempotency_key == idempotency_key)
        {
            if previous.request_digest != request_digest {
                return Err("IDEMPOTENCY_CONFLICT".into());
            }
            return Ok(json!({
                "commandId":previous.command_id,
                "finalStatus":"committed",
                "message":"模型价格表版本已找到。",
                "version":previous.version,
                "replayed":true,
                "modelPricing":catalog
            }));
        }

        let version = catalog.version + 1;
        let history = ModelPricingVersion {
            version,
            updated_at: now.into(),
            command_id: command_id.into(),
            idempotency_key: idempotency_key.into(),
            request_digest: request_digest.into(),
            entries: entries.clone(),
        };
        catalog.version = version;
        catalog.updated_at = Some(now.into());
        catalog.entries = entries;
        catalog.history.push(history);
        let encoded = serde_json::to_string(&catalog)
            .map_err(|error| format!("MODEL_PRICING_CATALOG_INVALID:{error}"))?;
        CatalogRepository::write_setting(connection, CATALOG_SETTING_KEY, &encoded)
            .map_err(|error| error.to_string())?;
        Ok(json!({
            "commandId":command_id,
            "finalStatus":"committed",
            "message":"模型价格表已保存为新版本。",
            "version":version,
            "replayed":false,
            "modelPricing":catalog
        }))
    }

    pub fn cost_snapshot(
        &self,
        provider_id: &str,
        model_id: &str,
        capability: &str,
        input: &Value,
    ) -> ModelCostSnapshot {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.provider_id == provider_id && entry.model_id == model_id);
        let estimated_input_tokens = estimate_input_tokens(input);
        let estimated_output_tokens = estimate_output_tokens(capability, input);
        let (status, input_price, output_price, cached_price, predicted) = match entry {
            Some(entry) => (
                "PRICED",
                Some(entry.input_price_per_million_tokens),
                Some(entry.output_price_per_million_tokens),
                entry.cached_input_price_per_million_tokens,
                estimate_cost_cents(estimated_input_tokens, estimated_output_tokens, 0, entry),
            ),
            None => ("UNPRICED", None, None, None, 0),
        };
        ModelCostSnapshot {
            price_table_version: self.version,
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            price_status: status.into(),
            input_price_per_million_tokens: input_price,
            output_price_per_million_tokens: output_price,
            cached_input_price_per_million_tokens: cached_price,
            estimated_input_tokens,
            estimated_output_tokens,
            predicted_cost_cents: predicted,
        }
    }
}

impl ModelCostSnapshot {
    pub fn actual_cost_cents(
        &self,
        input_tokens: i64,
        output_tokens: i64,
        cached_tokens: Option<i64>,
    ) -> Option<i64> {
        if self.price_status != "PRICED" {
            return None;
        }
        if input_tokens <= 0 && output_tokens <= 0 {
            return Some(self.predicted_cost_cents);
        }
        let input_price = self.input_price_per_million_tokens?;
        let output_price = self.output_price_per_million_tokens?;
        let input_tokens = input_tokens.max(0);
        let output_tokens = output_tokens.max(0);
        let cached_tokens = cached_tokens.unwrap_or(0).clamp(0, input_tokens);
        let uncached_tokens = input_tokens - cached_tokens;
        let input_cost = uncached_tokens as f64 * input_price
            + cached_tokens as f64
                * self
                    .cached_input_price_per_million_tokens
                    .unwrap_or(input_price);
        Some(round_cost_cents(
            (input_cost + output_tokens as f64 * output_price) / 1_000_000.0,
        ))
    }
}

pub fn validate_entries(value: &Value) -> Result<Vec<ModelPricingEntry>, String> {
    let entries = value
        .as_array()
        .ok_or_else(|| "MODEL_PRICING_ENTRIES_REQUIRED".to_string())?;
    let mut validated = Vec::with_capacity(entries.len());
    let mut keys = BTreeSet::new();
    for entry in entries {
        let provider_id = entry
            .get("providerId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "MODEL_PRICING_ENTRY_INVALID:providerId".to_string())?;
        let model_id = entry
            .get("modelId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "MODEL_PRICING_ENTRY_INVALID:modelId".to_string())?;
        let input_price = price_field(entry, "inputPricePerMillionTokens")?;
        let output_price = price_field(entry, "outputPricePerMillionTokens")?;
        let cached_price = entry
            .get("cachedInputPricePerMillionTokens")
            .filter(|value| !value.is_null())
            .map(|_| price_field(entry, "cachedInputPricePerMillionTokens"))
            .transpose()?;
        if !keys.insert((provider_id.to_string(), model_id.to_string())) {
            return Err("MODEL_PRICING_ENTRY_DUPLICATE".into());
        }
        validated.push(ModelPricingEntry {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            input_price_per_million_tokens: input_price,
            output_price_per_million_tokens: output_price,
            cached_input_price_per_million_tokens: cached_price,
        });
    }
    validated.sort_by(|left, right| {
        left.provider_id
            .cmp(&right.provider_id)
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    Ok(validated)
}

fn price_field(entry: &Value, key: &str) -> Result<f64, String> {
    entry
        .get(key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| format!("MODEL_PRICING_ENTRY_INVALID:{key}"))
}

fn estimate_input_tokens(input: &Value) -> i64 {
    let serialized = serde_json::to_string(input).unwrap_or_default();
    let ascii_bytes = serialized.bytes().filter(u8::is_ascii).count();
    let non_ascii_chars = serialized
        .chars()
        .filter(|character| !character.is_ascii())
        .count();
    (ascii_bytes.div_ceil(3) + non_ascii_chars).max(1) as i64
}

fn estimate_output_tokens(capability: &str, input: &Value) -> i64 {
    match capability {
        // FX-18: a body may run 10% past the chapter length range maximum.
        "RENDER_CHAPTER" => input
            .pointer("/chapterLengthRange/max")
            .and_then(Value::as_i64)
            .map(|max| max + max / 10)
            .or_else(|| input.get("targetChineseCharacters").and_then(Value::as_i64))
            .unwrap_or(1_800)
            .max(1),
        "PLAN_STRATEGY" => 1_500,
        "CLASSIFY_ASSISTANT_INTENT" => 200,
        "AUDIT_EXTRACT" => 1_200,
        "REPAIR_CANDIDATE" => 1_200,
        "REVIEW_CHAPTER" => 800,
        "REWRITE_PASSAGES" => 1_500,
        "GENERATE_STORY_DIRECTIONS" => 2_400,
        "GENERATE_STORY_BIBLE" => 4_000,
        "OPENING_SIGNING_EVALUATION" => 3_000,
        "B14_S44_ENDGAME_ADVICE" | "B14_S45_COMPLETION_QUALITY" => 1_500,
        _ => 1_000,
    }
}

fn estimate_cost_cents(
    input_tokens: i64,
    output_tokens: i64,
    cached_tokens: i64,
    entry: &ModelPricingEntry,
) -> i64 {
    let cached_tokens = cached_tokens.clamp(0, input_tokens.max(0));
    let uncached_tokens = input_tokens.max(0) - cached_tokens;
    let input_cost = uncached_tokens as f64 * entry.input_price_per_million_tokens
        + cached_tokens as f64
            * entry
                .cached_input_price_per_million_tokens
                .unwrap_or(entry.input_price_per_million_tokens);
    let output_cost = output_tokens.max(0) as f64 * entry.output_price_per_million_tokens;
    round_cost_cents((input_cost + output_cost) / 1_000_000.0)
}

fn round_cost_cents(yuan: f64) -> i64 {
    (yuan.max(0.0) * 100.0).ceil() as i64
}

#[cfg(test)]
mod tests {
    #[test]
    fn fx18_render_estimate_uses_the_chapter_length_range_maximum() {
        let input = serde_json::json!({"chapterLengthRange":{"min":3000,"max":4000}});
        assert_eq!(
            super::estimate_output_tokens("RENDER_CHAPTER", &input),
            4_400
        );
        assert_eq!(
            super::estimate_output_tokens(
                "RENDER_CHAPTER",
                &serde_json::json!({"targetChineseCharacters":1800})
            ),
            1_800
        );
    }

    use super::{estimate_cost_cents, validate_entries, ModelPricingEntry};
    use serde_json::json;

    #[test]
    fn price_table_rejects_duplicate_models_and_negative_prices() {
        assert_eq!(
            validate_entries(&json!([
                {"providerId":"qwen","modelId":"plus","inputPricePerMillionTokens":1,"outputPricePerMillionTokens":2},
                {"providerId":"qwen","modelId":"plus","inputPricePerMillionTokens":1,"outputPricePerMillionTokens":2}
            ]))
            .unwrap_err(),
            "MODEL_PRICING_ENTRY_DUPLICATE"
        );
        assert_eq!(
            validate_entries(&json!([{"providerId":"qwen","modelId":"plus","inputPricePerMillionTokens":-1,"outputPricePerMillionTokens":2}]))
                .unwrap_err(),
            "MODEL_PRICING_ENTRY_INVALID:inputPricePerMillionTokens"
        );
    }

    #[test]
    fn model_cost_uses_input_output_and_cached_token_rates() {
        let pricing = ModelPricingEntry {
            provider_id: "qwen".into(),
            model_id: "plus".into(),
            input_price_per_million_tokens: 1.0,
            output_price_per_million_tokens: 2.0,
            cached_input_price_per_million_tokens: Some(0.5),
        };
        assert_eq!(
            estimate_cost_cents(1_000_000, 500_000, 200_000, &pricing),
            190
        );
    }
}
