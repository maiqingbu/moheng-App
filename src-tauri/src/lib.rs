pub mod catalog;
pub mod commands;
pub mod commit;
pub mod creative_skills;
pub mod generated;
pub mod harness;
pub mod narrative;
pub mod planning;
pub mod production;
pub mod project_db;
pub mod project_entry;
pub mod project_entry_semantic;
pub mod provider;
pub mod repair;
pub mod repositories;
pub mod stable_id;
pub mod volume_lifecycle;

pub use repositories::runtime;

pub mod hash {
    use serde_json::Value;
    use sha2::{Digest, Sha256};

    pub fn normalize_json_newlines(value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(text.replace("\r\n", "\n").replace('\r', "\n")),
            Value::Array(items) => {
                Value::Array(items.iter().map(normalize_json_newlines).collect())
            }
            Value::Object(object) => Value::Object(
                object
                    .iter()
                    .map(|(key, child)| (key.clone(), normalize_json_newlines(child)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    pub fn canonical_bytes(value: &Value) -> Result<Vec<u8>, serde_json::Error> {
        serde_json_canonicalizer::to_vec(&normalize_json_newlines(value))
    }

    pub fn sha256_canonical(value: &Value) -> Result<String, serde_json::Error> {
        let digest = Sha256::digest(canonical_bytes(value)?);
        Ok(hex::encode(digest))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use serde_json::json;

        #[test]
        fn canonical_hash_sorts_nested_object_keys_and_normalizes_newlines() {
            let left = json!({
                "nested": {"z": 1, "a": ["你好", {"d": 4, "c": 3}]},
                "text": "a\r\nb\rc",
            });
            let right = json!({
                "text": "a\nb\nc",
                "nested": {"a": ["你好", {"c": 3, "d": 4}], "z": 1},
            });
            assert_eq!(
                sha256_canonical(&left).unwrap(),
                sha256_canonical(&right).unwrap()
            );
        }

        #[test]
        fn canonical_hash_matches_utf8_golden_value() {
            let value = json!({"text": "你好\n世界", "crlf": "a\nb"});
            assert_eq!(
                sha256_canonical(&value).unwrap(),
                "df3930570f63bc666516e440140086178f1aeb59b8c5da9587a6ba95536363c1"
            );
        }

        #[test]
        fn canonical_hash_uses_rfc8785_number_and_utf16_rules() {
            let value = json!({"values": [1e30, 4.5, 0.000002, 1e-7, -0.0]});
            assert_eq!(
                String::from_utf8(canonical_bytes(&value).unwrap()).unwrap(),
                r#"{"values":[1e+30,4.5,0.000002,1e-7,0]}"#
            );
        }
    }
}
