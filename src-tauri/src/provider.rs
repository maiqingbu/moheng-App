use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::process::Command;
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
                ProviderError::new(
                    ProviderErrorKind::Configuration,
                    "MODEL_CONFIG_MISSING:provider",
                )
            })?;
        let provider_id = if provider_id.eq_ignore_ascii_case("openai compatible") {
            "openai-compatible"
        } else {
            provider_id
        };
        let model_id = settings
            .get("modelId")
            .and_then(Value::as_str)
            .or_else(|| settings.get("model").and_then(Value::as_str))
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                ProviderError::new(
                    ProviderErrorKind::Configuration,
                    "MODEL_CONFIG_MISSING:model",
                )
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
    /// Exact provider response body.  The production gateway passes this
    /// through unchanged so the durable model runner can persist it before
    /// any provider-envelope or model-payload parsing occurs.
    pub raw_response: String,
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

    fn structured_prompt(capability: &str) -> &'static str {
        match capability {
            "PLAN_STRATEGY" => "Return JSON only, with no Markdown or explanation. Use exactly this object shape: {\"payloadSchema\":string,\"chapterId\":string,\"compiledFromProjectRevision\":number,\"compiledFromStateDigest\":string,\"contextManifestRef\":object,\"chapterGoal\":string,\"activeTaskRefs\":array,\"continuityActions\":array,\"requiredStateOutcomes\":array,\"forbiddenTransitions\":array,\"knowledgeBoundaries\":array,\"reservationRefs\":array,\"sceneExecution\":array,\"horizonPreview\":array}. chapterGoal must be a non-empty Chinese sentence; use chapterGoalHint when supplied. Fill unknown values with empty arrays or null only where the contract permits.",
            "RENDER_CHAPTER" => "Return JSON only, with no Markdown or explanation. Use exactly this object shape: {\"body\":string}. The body must contain complete Chinese chapter prose and should follow targetChineseCharacters when supplied.",
            "AUDIT_EXTRACT" => "Return JSON only, with no Markdown or explanation. Use exactly this object shape: {\"qualityFindings\":array,\"canonCandidates\":array,\"knowledgeCandidates\":array,\"continuityCandidates\":array,\"taskProgressCandidates\":array,\"revealedReservationRefs\":array,\"evidenceRanges\":array}.",
            "GENERATE_STORY_DIRECTIONS" => "Return JSON only, with no Markdown or explanation. Use exactly this object shape: {\"pitches\":[{\"name\":string,\"promise\":string,\"protagonistDesire\":string,\"mechanism\":string,\"worldConflict\":string,\"longArc\":string,\"openingHook\":string,\"volumeGoal\":string,\"commercialEdge\":string,\"originality\":string,\"difficulty\":string,\"failureRisk\":string}]} with exactly three substantially different pitches.",
            "TEST_PROVIDER" => "Return JSON only, with no Markdown or explanation. Use exactly this object shape: {\"ok\":true}.",
            _ => "Return one valid JSON object only. Do not add Markdown, code fences, explanations, or text outside the JSON object.",
        }
    }

    fn is_xiaomi_endpoint(endpoint: &str) -> bool {
        endpoint.to_ascii_lowercase().contains("xiaomimimo.com")
    }

    fn request_payload(endpoint: &str, request: &ProviderRequest) -> Value {
        let mut payload = json!({
            "model": request.model_id,
            "messages": [
                {"role":"system","content":Self::structured_prompt(&request.capability)},
                {"role":"user","content":request.prompt}
            ],
            "temperature": 0
        });
        if Self::is_xiaomi_endpoint(endpoint) {
            payload["temperature"] = json!(1.0);
            payload["thinking"] = json!({"type":"disabled"});
            payload["stream"] = json!(false);
            payload["max_completion_tokens"] = json!(16_384);
        }
        payload
    }

    fn request_headers(endpoint: &str, secret: &str) -> Vec<String> {
        let mut headers = vec!["Content-Type: application/json".into()];
        if Self::is_xiaomi_endpoint(endpoint) {
            headers.push(format!("api-key: {secret}"));
        } else {
            headers.push(format!("Authorization: Bearer {secret}"));
        }
        headers
    }

    fn curl_metadata<'a>(stderr: &'a str, label: &str) -> Option<&'a str> {
        stderr
            .lines()
            .find_map(|line| line.strip_prefix(label))
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn request_payload_digest(
        &self,
        request: &ProviderRequest,
    ) -> Result<String, ProviderError> {
        crate::hash::sha256_canonical(&Self::request_payload(&self.endpoint, request)).map_err(
            |error| ProviderError::new(ProviderErrorKind::MalformedResponse, error.to_string()),
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
        let payload = Self::request_payload(&self.endpoint, request).to_string();
        let timeout_seconds = ((request.timeout_ms.max(1) as f64) / 1000.0).ceil() as u64;
        let mut command = Command::new("/usr/bin/curl");
        command.args(["--silent", "--show-error", "--request", "POST"]);
        for header in Self::request_headers(&self.endpoint, secret) {
            command.arg("--header").arg(header);
        }
        let output = command
            .arg("--data-raw")
            .arg(&payload)
            .arg("--max-time")
            .arg(timeout_seconds.to_string())
            .arg("--write-out")
            .arg(
                "%{stderr}P01_HTTP_STATUS:%{http_code}\nP01_CONTENT_TYPE:%{content_type}\nP01_RESPONSE_BYTES:%{size_download}\n",
            )
            .arg(&url)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| ProviderError::new(ProviderErrorKind::Transport, error.to_string()))?
            .wait_with_output()
            .map_err(|error| ProviderError::new(ProviderErrorKind::Transport, error.to_string()))?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let status = Self::curl_metadata(&stderr, "P01_HTTP_STATUS:").ok_or_else(|| {
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
        let content_type = Self::curl_metadata(&stderr, "P01_CONTENT_TYPE:").unwrap_or("unknown");
        let response_bytes =
            Self::curl_metadata(&stderr, "P01_RESPONSE_BYTES:").unwrap_or("unknown");
        let request_payload_digest = self
            .request_payload_digest(request)
            .unwrap_or_else(|_| "unavailable".into());
        eprintln!(
            "P01_PROVIDER_RESPONSE http_status={} content_type={} response_bytes={} request_payload_digest={}",
            status, content_type, response_bytes, request_payload_digest
        );
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
        let raw_response = String::from_utf8_lossy(&output.stdout).to_string();
        if raw_response.trim().is_empty() {
            eprintln!(
                "P01_PROVIDER_EMPTY_BODY http_status={} content_type={} response_bytes={}",
                status, content_type, response_bytes,
            );
        }
        Ok(ProviderResponse {
            raw_response,
            usage: ProviderUsage::default(),
            provider_request_id: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenAiCompatibleProvider, ProviderRequest};

    #[test]
    fn curl_metadata_extracts_safe_response_facts() {
        let stderr = "curl: note\nP01_HTTP_STATUS:200\nP01_CONTENT_TYPE:application/json\nP01_RESPONSE_BYTES:42\n";
        assert_eq!(
            OpenAiCompatibleProvider::curl_metadata(stderr, "P01_HTTP_STATUS:"),
            Some("200")
        );
        assert_eq!(
            OpenAiCompatibleProvider::curl_metadata(stderr, "P01_CONTENT_TYPE:"),
            Some("application/json")
        );
        assert_eq!(
            OpenAiCompatibleProvider::curl_metadata(stderr, "P01_RESPONSE_BYTES:"),
            Some("42")
        );
        assert_eq!(
            OpenAiCompatibleProvider::curl_metadata(stderr, "P01_API_KEY:"),
            None
        );
    }

    #[test]
    fn xiaomi_production_payload_uses_documented_text_mode_and_disables_thinking() {
        let payload = OpenAiCompatibleProvider::request_payload(
            "https://api.xiaomimimo.com/v1",
            &ProviderRequest {
                capability: "RENDER_CHAPTER".into(),
                model_id: "mimo-v2.5-pro".into(),
                prompt: "{\"chapterId\":\"chapter:1\"}".into(),
                timeout_ms: 1000,
            },
        );
        assert!(payload.get("response_format").is_none());
        assert_eq!(payload["temperature"], 1.0);
        assert_eq!(payload["thinking"]["type"], "disabled");
        assert_eq!(payload["stream"], false);
        assert_eq!(payload["max_completion_tokens"], 16_384);
        assert_eq!(payload["messages"][0]["role"], "system");
        assert!(payload["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("Return JSON only"));
    }

    #[test]
    fn xiaomi_request_payload_digest_is_stable_and_secret_free() {
        let provider = OpenAiCompatibleProvider::new("https://api.xiaomimimo.com/v1");
        let digest = provider
            .request_payload_digest(&ProviderRequest {
                capability: "RENDER_CHAPTER".into(),
                model_id: "mimo-v2.5-pro".into(),
                prompt: "bounded-input".into(),
                timeout_ms: 1000,
            })
            .unwrap();
        assert_eq!(digest.len(), 64);
        assert!(!digest.contains("sk-"));
    }

    #[test]
    fn xiaomi_headers_use_the_documented_api_key_authentication() {
        let headers = OpenAiCompatibleProvider::request_headers(
            "https://api.xiaomimimo.com/v1",
            "test-secret",
        );
        assert!(headers
            .iter()
            .any(|header| header == "api-key: test-secret"));
        assert!(!headers
            .iter()
            .any(|header| header.starts_with("Authorization: Bearer")));
    }
}
