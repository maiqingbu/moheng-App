use crate::project_db::paths::AppPaths;
use libsql_rusqlite::{Connection, OptionalExtension};
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
    #[error("CHAPTER_LENGTH_RANGE_INVALID")]
    InvalidChapterLengthRange,
}

pub struct ProviderSettingsRepository;

impl ProviderSettingsRepository {
    fn global_path(paths: &AppPaths) -> PathBuf {
        paths.root().join("provider-settings.json")
    }

    pub fn default_projection() -> Value {
        json!({
            "provider":"OpenAI Compatible","model":"gpt-4o-mini","endpoint":"",
            "monthlyBudget":null,"chapterBudget":null,
            "privacyMode":"local-first","sendDraftOnly":true,"exportFormat":"DOCX",
            "platform":"","theme":"system","providerId":"openai-compatible",
            "modelId":"gpt-4o-mini","enabled":false,"secretStatus":"NOT_CONFIGURED",
            "capabilityStatus":"UNAVAILABLE","budgetHistory":[]
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
            "privacyMode",
            "sendDraftOnly",
            "exportFormat",
            "platform",
            "theme",
            "providerId",
            "modelId",
            "renderModelId",
            "auditModelId",
            "enabled",
            "timeoutMs",
        ];
        for key in allowed {
            if let Some(value) = source.get(key) {
                result[key] = value.clone();
            }
        }
        // A saved provider profile must not inherit the historical UI demo
        // model when the caller omitted or blanked the model fields. Keep the
        // global no-file projection friendly for the settings screen, but
        // make every partial persisted profile fail closed at production.
        let model = source
            .get("modelId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                source
                    .get("model")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
            })
            .unwrap_or_default();
        result["model"] = Value::String(model.to_string());
        result["modelId"] = Value::String(model.to_string());
        result
    }

    pub fn read_global(paths: &AppPaths) -> Result<Value, ProviderSettingsError> {
        let path = Self::global_path(paths);
        if !path.is_file() {
            return Ok(Self::global_projection(Self::default_projection()));
        }
        let raw = fs::read_to_string(path)?;
        Ok(Self::global_projection(Self::sanitized(
            &serde_json::from_str(&raw)?,
        )))
    }

    pub fn write_global(
        paths: &AppPaths,
        settings: &Value,
    ) -> Result<Value, ProviderSettingsError> {
        let value = Self::global_projection(Self::sanitized(settings));
        fs::create_dir_all(paths.root())?;
        fs::write(Self::global_path(paths), serde_json::to_vec_pretty(&value)?)?;
        Ok(value)
    }

    fn global_projection(mut settings: Value) -> Value {
        settings["monthlyBudget"] = Value::Null;
        settings["chapterBudget"] = Value::Null;
        settings["budgetHistory"] = json!([]);
        settings
    }

    /// FX-18: the length range saved for one chapter when it was chosen at
    /// production start (opening) or plan compile (long-form).
    pub fn chapter_length_range(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
    ) -> Result<Option<Value>, ProviderSettingsError> {
        let raw = connection
            .query_row(
                "SELECT setting_value FROM project_settings WHERE project_id=?1 AND setting_key=?2",
                libsql_rusqlite::params![project_id, format!("chapterLengthRange:{chapter_id}")],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        raw.map(|raw| serde_json::from_str(&raw))
            .transpose()
            .map_err(Into::into)
    }

    pub fn save_chapter_length_range(
        connection: &Connection,
        project_id: &str,
        chapter_id: &str,
        range: &Value,
        now: &str,
    ) -> Result<(), ProviderSettingsError> {
        connection.execute("INSERT INTO project_settings(project_id,setting_key,setting_value,updated_at) VALUES (?1,?2,?3,?4) ON CONFLICT(project_id,setting_key) DO UPDATE SET setting_value=excluded.setting_value,updated_at=excluded.updated_at", libsql_rusqlite::params![project_id, format!("chapterLengthRange:{chapter_id}"), range.to_string(), now])?;
        Ok(())
    }

    pub fn read_project(
        connection: &Connection,
        project_id: &str,
    ) -> Result<Value, ProviderSettingsError> {
        let mut value = Self::default_projection();
        let mut statement = connection.prepare("SELECT setting_key, setting_value FROM project_settings WHERE project_id=?1 AND setting_key IN ('provider','model','endpoint','monthlyBudget','chapterBudget','budgetHistory','privacyMode','sendDraftOnly','exportFormat','platform','theme','providerId','modelId','enabled','timeoutMs','chapterLengthRange') ORDER BY setting_key")?;
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
        command_id: &str,
        now: &str,
    ) -> Result<Value, ProviderSettingsError> {
        let current = Self::read_project(connection, project_id)?;
        let value = Self::sanitized(settings);
        let source = settings.as_object();
        let mut result = current.clone();
        for key in [
            "provider",
            "model",
            "endpoint",
            "monthlyBudget",
            "chapterBudget",
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
            if source.is_some_and(|source| source.contains_key(key)) {
                let setting = &value[key];
                connection.execute("INSERT INTO project_settings(project_id,setting_key,setting_value,updated_at) VALUES (?1,?2,?3,?4) ON CONFLICT(project_id,setting_key) DO UPDATE SET setting_value=excluded.setting_value,updated_at=excluded.updated_at", libsql_rusqlite::params![project_id,key,setting.to_string(),now])?;
                result[key] = setting.clone();
            }
        }
        // FX-18: the work's default chapter length range (project only).
        if let Some(range) = source.and_then(|source| source.get("chapterLengthRange")) {
            let range = crate::chapter_length::ChapterLengthRange::from_value(range)
                .map_err(|_| ProviderSettingsError::InvalidChapterLengthRange)?
                .to_json();
            connection.execute("INSERT INTO project_settings(project_id,setting_key,setting_value,updated_at) VALUES (?1,'chapterLengthRange',?2,?3) ON CONFLICT(project_id,setting_key) DO UPDATE SET setting_value=excluded.setting_value,updated_at=excluded.updated_at", libsql_rusqlite::params![project_id,range.to_string(),now])?;
            result["chapterLengthRange"] = range;
        }
        let previous_history = current["budgetHistory"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut history = previous_history.clone();
        for key in ["chapterBudget", "monthlyBudget"] {
            if source.is_some_and(|source| source.contains_key(key))
                && current.get(key) != value.get(key)
            {
                history.push(json!({
                    "setting":key,
                    "oldValue":current.get(key).cloned().unwrap_or(Value::Null),
                    "newValue":value.get(key).cloned().unwrap_or(Value::Null),
                    "commandId":command_id,
                    "changedAt":now
                }));
            }
        }
        if history != previous_history {
            let history_json = Value::Array(history);
            connection.execute("INSERT INTO project_settings(project_id,setting_key,setting_value,updated_at) VALUES (?1,'budgetHistory',?2,?3) ON CONFLICT(project_id,setting_key) DO UPDATE SET setting_value=excluded.setting_value,updated_at=excluded.updated_at", libsql_rusqlite::params![project_id,history_json.to_string(),now])?;
            result["budgetHistory"] = history_json;
        }
        Ok(result)
    }
}
