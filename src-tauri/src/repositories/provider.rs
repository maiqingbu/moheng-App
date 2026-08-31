use crate::project_db::paths::AppPaths;
use libsql_rusqlite::Connection;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ProviderSettingsError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite: {0}")]
    Sqlite(#[from] libsql_rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct ProviderSettingsRepository;

impl ProviderSettingsRepository {
    fn global_path(paths: &AppPaths) -> PathBuf {
        paths.root().join("provider-settings.json")
    }

    pub fn default_projection() -> Value {
        json!({
            "provider":"OpenAI Compatible","model":"gpt-4o-mini","endpoint":"",
            "monthlyBudget":200,"chapterBudget":3,"pauseBeforeOverrun":true,
            "privacyMode":"local-first","sendDraftOnly":true,"exportFormat":"DOCX",
            "platform":"","theme":"system","providerId":"openai-compatible",
            "modelId":"gpt-4o-mini","enabled":false,"secretStatus":"NOT_CONFIGURED",
            "capabilityStatus":"UNAVAILABLE"
        })
    }

    fn sanitized(settings: &Value) -> Value {
        let mut result = Self::default_projection();
        let Some(source) = settings.as_object() else {
            return result;
        };
        let allowed = [
            "provider",
            "model",
            "endpoint",
            "monthlyBudget",
            "chapterBudget",
            "pauseBeforeOverrun",
            "privacyMode",
            "sendDraftOnly",
            "exportFormat",
            "platform",
            "theme",
            "providerId",
            "modelId",
            "enabled",
            "timeoutMs",
        ];
        for key in allowed {
            if let Some(value) = source.get(key) {
                result[key] = value.clone();
            }
        }
        result
    }

    pub fn read_global(paths: &AppPaths) -> Result<Value, ProviderSettingsError> {
        let path = Self::global_path(paths);
        if !path.is_file() {
            return Ok(Self::default_projection());
        }
        let raw = fs::read_to_string(path)?;
        Ok(Self::sanitized(&serde_json::from_str(&raw)?))
    }

    pub fn write_global(
        paths: &AppPaths,
        settings: &Value,
    ) -> Result<Value, ProviderSettingsError> {
        let value = Self::sanitized(settings);
        fs::create_dir_all(paths.root())?;
        fs::write(Self::global_path(paths), serde_json::to_vec_pretty(&value)?)?;
        Ok(value)
    }

    pub fn read_project(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Value, ProviderSettingsError> {
        let mut value = Self::default_projection();
        let mut statement = connection.prepare("SELECT setting_key, setting_value FROM project_settings WHERE project_id=?1 AND setting_key IN ('provider','model','endpoint','monthlyBudget','chapterBudget','pauseBeforeOverrun','privacyMode','sendDraftOnly','exportFormat','platform','theme','providerId','modelId','enabled','timeoutMs') ORDER BY setting_key")?;
        let rows = statement.query_map([project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (key, raw) = row?;
            let parsed = serde_json::from_str(&raw).unwrap_or(Value::String(raw));
            value[key] = parsed;
        }
        Ok(value)
    }

    pub fn write_project(
        connection: &Connection,
        project_id: &str,
        settings: &Value,
        now: &str,
    ) -> Result<Value, ProviderSettingsError> {
        let value = Self::sanitized(settings);
        for key in [
            "provider",
            "model",
            "endpoint",
            "monthlyBudget",
            "chapterBudget",
            "pauseBeforeOverrun",
            "privacyMode",
            "sendDraftOnly",
            "exportFormat",
            "platform",
            "theme",
            "providerId",
            "modelId",
            "enabled",
            "timeoutMs",
        ] {
            if let Some(setting) = value.get(key) {
                connection.execute("INSERT INTO project_settings(project_id,setting_key,setting_value,updated_at) VALUES (?1,?2,?3,?4) ON CONFLICT(project_id,setting_key) DO UPDATE SET setting_value=excluded.setting_value,updated_at=excluded.updated_at", libsql_rusqlite::params![project_id,key,setting.to_string(),now])?;
            }
        }
        Ok(value)
    }
}
