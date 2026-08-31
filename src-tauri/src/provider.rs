use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderErrorKind {
    Authentication,
    RateLimited,
    Timeout,
    Transport,
    ProviderRejected,
    MalformedResponse,
    UnknownRemoteOutcome,
    BudgetExceeded,
    Configuration,
}

impl ProviderErrorKind {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Authentication => "AUTHENTICATION_ERROR",
            Self::RateLimited => "RATE_LIMITED",
            Self::Timeout => "TIMEOUT",
            Self::Transport => "TRANSPORT_ERROR",
            Self::ProviderRejected => "PROVIDER_REJECTED",
            Self::MalformedResponse => "MALFORMED_RESPONSE",
            Self::UnknownRemoteOutcome => "UNKNOWN_REMOTE_OUTCOME",
            Self::BudgetExceeded => "BUDGET_EXCEEDED",
            Self::Configuration => "CONFIGURATION_ERROR",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub detail: String,
}

impl ProviderError {
    pub fn new(kind: ProviderErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderConfig {
    pub provider_id: String,
    pub model_id: String,
    pub endpoint: String,
    pub enabled: bool,
    pub timeout_ms: u64,
}

impl ProviderConfig {
    pub fn from_settings(settings: &Value) -> Result<Self, ProviderError> {
        let provider_id = settings
            .get("providerId")
            .and_then(Value::as_str)
            .or_else(|| settings.get("provider").and_then(Value::as_str))
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                ProviderError::new(ProviderErrorKind::Configuration, "provider id is required")
            })?;
        let model_id = settings
            .get("modelId")
            .and_then(Value::as_str)
            .or_else(|| settings.get("model").and_then(Value::as_str))
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                ProviderError::new(ProviderErrorKind::Configuration, "model id is required")
            })?;
        let endpoint = settings
            .get("endpoint")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        Ok(Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            endpoint: endpoint.clone(),
            enabled: settings
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(!endpoint.is_empty()),
            timeout_ms: settings
                .get("timeoutMs")
                .and_then(Value::as_u64)
                .unwrap_or(30_000)
                .clamp(500, 120_000),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderRequest {
    pub capability: String,
    pub model_id: String,
    pub prompt: String,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: Option<i64>,
    pub total_tokens: i64,
    pub provider_reported_cost_cents: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderResponse {
    pub output: Value,
    pub usage: ProviderUsage,
    pub provider_request_id: Option<String>,
}

pub trait ProviderAdapter: Send + Sync {
    fn invoke(
        &self,
        request: &ProviderRequest,
        secret: &str,
    ) -> Result<ProviderResponse, ProviderError>;
}

#[derive(Clone)]
pub struct ScriptedProvider {
    result: Arc<Mutex<Result<ProviderResponse, ProviderErrorKind>>>,
    calls: Option<Arc<Mutex<i64>>>,
}

impl ScriptedProvider {
    pub fn new(result: Result<ProviderResponse, ProviderErrorKind>) -> Self {
        Self {
            result: Arc::new(Mutex::new(result)),
            calls: None,
        }
    }

    pub fn counting(
        calls: Arc<Mutex<i64>>,
        result: Result<ProviderResponse, ProviderErrorKind>,
    ) -> Self {
        Self {
            result: Arc::new(Mutex::new(result)),
            calls: Some(calls),
        }
    }

    pub fn invoke_with_budget(
        &self,
        request: &ProviderRequest,
        secret: &str,
        hard_limit_exceeded: bool,
    ) -> Result<ProviderResponse, ProviderError> {
        if hard_limit_exceeded {
            return Err(ProviderError::new(
                ProviderErrorKind::BudgetExceeded,
                "hard budget exceeded before provider invocation",
            ));
        }
        self.invoke(request, secret)
    }
}

impl ProviderAdapter for ScriptedProvider {
    fn invoke(
        &self,
        _request: &ProviderRequest,
        _secret: &str,
    ) -> Result<ProviderResponse, ProviderError> {
        if let Some(calls) = &self.calls {
            *calls
                .lock()
                .map_err(|_| ProviderError::new(ProviderErrorKind::Transport, "fixture lock"))? +=
                1;
        }
        self.result
            .lock()
            .map_err(|_| ProviderError::new(ProviderErrorKind::Transport, "fixture lock"))?
            .clone()
            .map_err(|kind| ProviderError::new(kind, "scripted provider result"))
    }
}

pub struct OpenAiCompatibleProvider {
    endpoint: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    fn endpoint(configured: &str) -> Result<String, ProviderError> {
        let endpoint = configured.trim().trim_end_matches('/');
        if endpoint.is_empty() {
            return Err(ProviderError::new(
                ProviderErrorKind::Configuration,
                "provider endpoint is required",
            ));
        }
        Ok(if endpoint.ends_with("/chat/completions") {
            endpoint.to_string()
        } else {
            format!("{endpoint}/chat/completions")
        })
    }

    fn curl_config(url: &str, secret: &str, payload: &str, timeout_ms: u64) -> String {
        fn quote(value: &str) -> String {
            format!(
                "\"{}\"",
                value
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
            )
        }
        format!(
            "url = {}\nrequest = post\nheader = {}\nheader = {}\ndata = {}\nmax-time = {}\nsilent\nshow-error\nwrite-out = {}\n",
            quote(url), quote("Content-Type: application/json"), quote(&format!("Authorization: Bearer {secret}")), quote(payload), (timeout_ms.max(1) as f64 / 1000.0).ceil() as u64, quote("\\nP01_HTTP_STATUS:%{http_code}")
        )
    }
}

impl ProviderAdapter for OpenAiCompatibleProvider {
    fn invoke(
        &self,
        request: &ProviderRequest,
        secret: &str,
    ) -> Result<ProviderResponse, ProviderError> {
        let url = Self::endpoint(&self.endpoint)?;
        if secret.trim().is_empty() {
            return Err(ProviderError::new(
                ProviderErrorKind::Authentication,
                "provider secret unavailable",
            ));
        }
        let payload = json!({"model":request.model_id,"messages":[{"role":"user","content":request.prompt}],"temperature":0}).to_string();
        let mut child = Command::new("/usr/bin/curl")
            .args(["--config", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| ProviderError::new(ProviderErrorKind::Transport, error.to_string()))?;
        child
            .stdin
            .take()
            .ok_or_else(|| {
                ProviderError::new(ProviderErrorKind::Transport, "curl stdin unavailable")
            })?
            .write_all(Self::curl_config(&url, secret, &payload, request.timeout_ms).as_bytes())
            .map_err(|error| ProviderError::new(ProviderErrorKind::Transport, error.to_string()))?;
        let output = child
            .wait_with_output()
            .map_err(|error| ProviderError::new(ProviderErrorKind::Transport, error.to_string()))?;
        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        let marker = "P01_HTTP_STATUS:";
        let (body, status) = raw.rsplit_once(marker).ok_or_else(|| {
            if output.status.code() == Some(28) {
                ProviderError::new(ProviderErrorKind::Timeout, "provider request timed out")
            } else {
                ProviderError::new(ProviderErrorKind::Transport, "provider status missing")
            }
        })?;
        let status: u16 = status.trim().parse().map_err(|_| {
            ProviderError::new(
                ProviderErrorKind::MalformedResponse,
                "provider status invalid",
            )
        })?;
        if status == 0 {
            if output.status.code() == Some(28) {
                return Err(ProviderError::new(
                    ProviderErrorKind::Timeout,
                    "provider request timed out",
                ));
            }
            return Err(ProviderError::new(
                ProviderErrorKind::Transport,
                "provider connection failed",
            ));
        }
        let body_value: Value = serde_json::from_str(body.trim()).map_err(|_| {
            ProviderError::new(
                ProviderErrorKind::MalformedResponse,
                "provider response is not JSON",
            )
        })?;
        if status == 401 || status == 403 {
            return Err(ProviderError::new(
                ProviderErrorKind::Authentication,
                "provider authentication rejected",
            ));
        }
        if status == 429 {
            return Err(ProviderError::new(
                ProviderErrorKind::RateLimited,
                "provider rate limited",
            ));
        }
        if status >= 500 {
            return Err(ProviderError::new(
                ProviderErrorKind::Transport,
                "provider server failure",
            ));
        }
        if status >= 400 {
            return Err(ProviderError::new(
                ProviderErrorKind::ProviderRejected,
                "provider rejected request",
            ));
        }
        let choice = body_value
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::new(
                    ProviderErrorKind::MalformedResponse,
                    "provider content missing",
                )
            })?;
        let output = serde_json::from_str(choice).unwrap_or_else(|_| json!({"text":choice}));
        let usage = body_value
            .get("usage")
            .map(|usage| ProviderUsage {
                input_tokens: usage
                    .get("prompt_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                output_tokens: usage
                    .get("completion_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                cached_tokens: usage
                    .pointer("/prompt_tokens_details/cached_tokens")
                    .and_then(Value::as_i64),
                total_tokens: usage
                    .get("total_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                provider_reported_cost_cents: None,
            })
            .unwrap_or_default();
        Ok(ProviderResponse {
            output,
            usage,
            provider_request_id: body_value
                .get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        })
    }
}
