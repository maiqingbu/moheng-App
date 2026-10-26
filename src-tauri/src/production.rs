//! B08 chapter production composition root.
//!
//! This module deliberately reuses the existing Runtime, ModelRunner and
//! CommitBundle V2 authorities.  It does not introduce a second scheduler or a
//! second narrative-delta table.  The renderer and audit adapters below are the
//! deterministic local boundary used by the formal command path; a provider
//! adapter can replace their transport without changing the commit protocol.

use crate::commit::CommitService;
use crate::generated::types::{
    ArtifactEnvelope, ArtifactEnvelopeProvenance, ArtifactRef, ArtifactStatus, ArtifactType,
    CommitBundle, ProjectCommandReceiptControlResult,
};
use crate::hash::sha256_canonical;
use crate::narrative::{PromotionSignal, ThreadResponsibilityLevel};
use crate::project_db::{
    ProductionControlRequest, ProjectCommandOutcome, ProjectCommandSpec, ProjectHandle,
    ProjectManager,
};
use crate::provider::{
    OpenAiCompatibleProvider, ProviderAdapter, ProviderAttemptDiagnostic, ProviderConfig,
    ProviderError, ProviderErrorKind, ProviderRequest, ProviderResponse,
};
use crate::repositories::runtime::{
    DurableJobService, DurableModelRunner, JobSpec, ModelRunOutcome, ModelRunSpec, ModelTransport,
    ModelTransportDiagnostic, OsKeychainSecretStore, RuntimeError, SecretStore, TransportFailure,
    TransportResponse, TransportUsage,
};
use crate::stable_id::{StableIdKind, StableIdentityFactory};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const STAGES: &[&str] = &[
    "CONTEXT",
    "PLAN",
    "RENDER",
    "CANDIDATE",
    "AUDIT",
    "GATE",
    "COMMIT",
    "RECONCILE",
    "COMPLETED",
];

#[derive(Debug, Clone)]
pub struct ProductionModelResponse {
    pub output: Value,
    /// Exact provider body for configured gateways.  Deterministic local
    /// gateways leave this unset and use `output` as their already-structured
    /// transport value.
    pub raw_response: Option<String>,
    pub usage: TransportUsage,
}

impl ProductionModelResponse {
    pub fn deterministic(output: Value) -> Self {
        Self {
            output,
            raw_response: None,
            usage: TransportUsage {
                input_tokens: 1,
                output_tokens: 1,
                cached_tokens: None,
                provider_reported_cost_cents: Some(0),
                latency_ms: 0,
                provider_request_id: Some("deterministic-production-gateway".into()),
            },
        }
    }
}

impl From<ProviderAttemptDiagnostic> for ModelTransportDiagnostic {
    fn from(diagnostic: ProviderAttemptDiagnostic) -> Self {
        Self {
            provider_send_started_at_unix_ms: Some(diagnostic.started_at_unix_ms),
            elapsed_ms: Some(diagnostic.elapsed_ms),
            timeout_ms: Some(diagnostic.timeout_ms),
            client_timeout_ms: Some(diagnostic.client_timeout_ms),
            phase: diagnostic.phase.as_str().into(),
            provider_error_code: diagnostic.provider_error_code,
            http_status: diagnostic.http_status,
            response_read: diagnostic.response_read.as_str().into(),
            response_bytes: diagnostic.response_bytes,
            provider_request_id: diagnostic.provider_request_id,
        }
    }
}

pub trait ProductionModelGateway: Send + Sync {
    fn provider(&self) -> &str {
        "configured-production-gateway"
    }

    fn model(&self, capability: &str) -> String {
        format!("production:{capability}")
    }

    fn model_pricing_catalog(
        &self,
        _now: &str,
    ) -> Result<crate::model_cost::ModelPricingCatalog, String> {
        Ok(crate::model_cost::ModelPricingCatalog::default())
    }

    fn fault_injection(&self) -> Option<&str> {
        None
    }

    fn transport_diagnostics_enabled(&self) -> bool {
        false
    }

    /// FX-18: configured providers must plan and write each chapter within
    /// its length range. Deterministic gateways return fixed short fixtures
    /// and skip the length rules.
    fn enforces_chapter_length(&self) -> bool {
        false
    }

    /// Whether a written chapter is reviewed and rewritten with the craft
    /// review and rewrite skills (the configured writer path).
    fn reviews_chapter_prose(&self) -> bool {
        self.enforces_chapter_length()
    }

    /// FX-21: configured providers classify project assistant questions;
    /// deterministic gateways keep the keyword routing.
    fn classifies_assistant_intent(&self) -> bool {
        false
    }

    /// Deterministic gateways can model a concurrent writer after the gate
    /// snapshot has been checked and immediately before CommitService runs.
    /// Configured providers keep this no-op.
    fn before_commit_attempt(
        &self,
        _connection: &libsql_rusqlite::Connection,
        _attempt: usize,
    ) -> Result<(), String> {
        Ok(())
    }

    /// Deterministic gateways can model a write after audit and before the
    /// first current-revision snapshot is read. Configured providers keep this
    /// no-op.
    fn before_commit_snapshot(
        &self,
        _connection: &libsql_rusqlite::Connection,
        _attempt: usize,
    ) -> Result<(), String> {
        Ok(())
    }

    /// Test gateways may observe the bounded gate recheck attempt without
    /// changing production behavior.
    fn on_chapter_gate_revalidation(&self, _attempt: usize) {}

    /// The profile for one production step (owner decisions 2026-10-17/18:
    /// writing and audit may use their own models); by default the
    /// production profile.
    fn resolve_step_profile(&self, _capability: &str) -> Result<ResolvedProviderProfile, String> {
        self.resolve_profile()
    }

    /// Resolve the provider once at the beginning of a production attempt.
    /// The returned profile is the non-secret snapshot used for every new
    /// ModelRun in that attempt.  Test/deterministic gateways retain their
    /// capability-specific compatibility model by using the default.
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        let provider = self.provider().trim().to_string();
        let model = self.model("RENDER_CHAPTER").trim().to_string();
        if provider.is_empty() {
            return Err("MODEL_CONFIG_MISSING:provider".into());
        }
        if model.is_empty() {
            return Err("MODEL_CONFIG_MISSING:model".into());
        }
        Ok(ResolvedProviderProfile {
            provider,
            model,
            endpoint: String::new(),
            timeout_ms: crate::provider::DEFAULT_PROVIDER_TIMEOUT_MS,
        })
    }

    fn invoke_typed_with_profile(
        &self,
        capability: &str,
        input: &Value,
        _profile: &ResolvedProviderProfile,
    ) -> Result<ProductionModelResponse, TransportFailure> {
        self.invoke_typed(capability, input)
    }

    fn invoke_typed_with_diagnostic(
        &self,
        capability: &str,
        input: &Value,
        profile: Option<&ResolvedProviderProfile>,
    ) -> (
        Result<ProductionModelResponse, TransportFailure>,
        Option<ProviderAttemptDiagnostic>,
    ) {
        let result = match profile {
            Some(profile) => self.invoke_typed_with_profile(capability, input, profile),
            None => self.invoke_typed(capability, input),
        };
        (result, None)
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String>;

    /// Preserve uncertainty from the configured provider until the durable
    /// runtime can apply its UNKNOWN_REMOTE_OUTCOME contract.  Legacy
    /// deterministic gateways keep the compatibility fallback to confirmed
    /// unavailability.
    fn invoke_typed(
        &self,
        capability: &str,
        input: &Value,
    ) -> Result<ProductionModelResponse, TransportFailure> {
        self.invoke(capability, input)
            .map_err(|_| TransportFailure::Unavailable)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProviderProfile {
    pub provider: String,
    pub model: String,
    pub endpoint: String,
    pub timeout_ms: u64,
}

pub struct UnavailableProductionGateway;

impl ProductionModelGateway for UnavailableProductionGateway {
    fn invoke(&self, _capability: &str, _input: &Value) -> Result<ProductionModelResponse, String> {
        Err("B08_PROVIDER_NOT_CONFIGURED".into())
    }
}

pub struct ConfiguredProductionGateway {
    config: ProviderConfig,
    adapter: std::sync::Arc<dyn ProviderAdapter>,
    secret_store: std::sync::Arc<dyn SecretStore>,
    settings_paths: Option<crate::project_db::paths::AppPaths>,
}

/// The user message for a provider: an input's `prompt` instruction comes
/// first, followed by every other field as JSON so no data is dropped.
fn serialize_provider_input(input: &Value) -> String {
    let Some(prompt) = input.get("prompt").and_then(Value::as_str) else {
        return input.to_string();
    };
    let mut data = input.clone();
    if let Some(object) = data.as_object_mut() {
        object.remove("prompt");
        if object.is_empty() {
            return prompt.to_owned();
        }
    }
    format!("{prompt}\n\n{data}")
}

impl ConfiguredProductionGateway {
    pub fn new(
        config: ProviderConfig,
        adapter: std::sync::Arc<dyn ProviderAdapter>,
        secret_store: std::sync::Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            config,
            adapter,
            secret_store,
            settings_paths: None,
        }
    }

    pub fn for_desktop(paths: crate::project_db::paths::AppPaths) -> Self {
        let provider_id =
            std::env::var("MOHENG_PROVIDER_ID").unwrap_or_else(|_| "openai-compatible".into());
        let model_id = std::env::var("MOHENG_PROVIDER_MODEL").unwrap_or_default();
        let endpoint = std::env::var("MOHENG_PROVIDER_ENDPOINT").unwrap_or_default();
        let mut gateway = Self::new(
            ProviderConfig {
                provider_id: provider_id.clone(),
                model_id,
                endpoint: endpoint.clone(),
                enabled: !endpoint.is_empty(),
                timeout_ms: crate::provider::DEFAULT_PROVIDER_TIMEOUT_MS,
            },
            std::sync::Arc::new(OpenAiCompatibleProvider::new(endpoint)),
            std::sync::Arc::new(OsKeychainSecretStore::new(
                "com.moheng.ai-novel-production",
                "provider-api-key",
            )),
        );
        gateway.settings_paths = Some(paths);
        gateway
    }

    fn current_config(&self) -> Result<ProviderConfig, ProviderError> {
        if let Some(paths) = &self.settings_paths {
            let settings =
                crate::repositories::provider::ProviderSettingsRepository::read_global(paths)
                    .map_err(|error| {
                        ProviderError::new(
                            ProviderErrorKind::Configuration,
                            format!("provider settings unavailable: {error}"),
                        )
                    })?;
            ProviderConfig::from_settings(&settings)
        } else {
            Ok(self.config.clone())
        }
    }

    fn validate_config(config: &ProviderConfig) -> Result<(), ProviderError> {
        if !config.enabled {
            return Err(ProviderError::new(
                ProviderErrorKind::Configuration,
                "MODEL_CONFIG_DISABLED",
            ));
        }
        if config.model_id.trim().is_empty() {
            return Err(ProviderError::new(
                ProviderErrorKind::Configuration,
                "MODEL_CONFIG_MISSING:model",
            ));
        }
        if config.endpoint.trim().is_empty() {
            return Err(ProviderError::new(
                ProviderErrorKind::Configuration,
                "MODEL_ENDPOINT_MISSING",
            ));
        }
        Ok(())
    }

    fn invoke_provider_with_config(
        &self,
        config: &ProviderConfig,
        capability: &str,
        input: &Value,
    ) -> Result<ProviderResponse, ProviderError> {
        Self::validate_config(config)?;
        let secret = self.secret_store.get(&config.provider_id).map_err(|_| {
            ProviderError::new(
                ProviderErrorKind::Authentication,
                "provider secret unavailable",
            )
        })?;
        let request = ProviderRequest {
            capability: capability.into(),
            model_id: config.model_id.clone(),
            prompt: serialize_provider_input(input),
            timeout_ms: config.timeout_ms,
        };
        if self.settings_paths.is_some() {
            OpenAiCompatibleProvider::new(config.endpoint.clone()).invoke(&request, &secret)
        } else {
            self.adapter.invoke(&request, &secret)
        }
    }

    fn invoke_provider(
        &self,
        capability: &str,
        input: &Value,
    ) -> Result<ProviderResponse, ProviderError> {
        let config = self.current_config()?;
        self.invoke_provider_with_config(&config, capability, input)
    }

    fn response_from_provider(response: ProviderResponse) -> ProductionModelResponse {
        ProductionModelResponse {
            // Configured providers are deliberately not parsed here.  The
            // exact body travels through GatewayTransport and is parsed only
            // after DurableModelRunner has persisted it.
            output: Value::Null,
            raw_response: Some(response.raw_response),
            usage: TransportUsage {
                input_tokens: response.usage.input_tokens,
                output_tokens: response.usage.output_tokens,
                cached_tokens: response.usage.cached_tokens,
                provider_reported_cost_cents: response.usage.provider_reported_cost_cents,
                latency_ms: 0,
                provider_request_id: response.provider_request_id,
            },
        }
    }

    fn transport_failure(error: &ProviderError) -> TransportFailure {
        match error.kind {
            ProviderErrorKind::Timeout => TransportFailure::Timeout,
            ProviderErrorKind::UnknownRemoteOutcome => TransportFailure::UnknownRemoteOutcome,
            _ => TransportFailure::Unavailable,
        }
    }

    fn typed_provider_result(
        result: Result<ProviderResponse, ProviderError>,
    ) -> (
        Result<ProductionModelResponse, TransportFailure>,
        Option<ProviderAttemptDiagnostic>,
    ) {
        match result {
            Ok(response) => {
                let diagnostic = response.diagnostic.clone();
                (Ok(Self::response_from_provider(response)), diagnostic)
            }
            Err(error) => {
                let failure = Self::transport_failure(&error);
                let diagnostic = error.diagnostic.map(|diagnostic| *diagnostic);
                (Err(failure), diagnostic)
            }
        }
    }
}

impl ProductionModelGateway for ConfiguredProductionGateway {
    fn provider(&self) -> &str {
        &self.config.provider_id
    }

    fn model(&self, _capability: &str) -> String {
        self.config.model_id.clone()
    }

    fn transport_diagnostics_enabled(&self) -> bool {
        true
    }

    fn enforces_chapter_length(&self) -> bool {
        true
    }

    fn classifies_assistant_intent(&self) -> bool {
        true
    }

    fn model_pricing_catalog(
        &self,
        now: &str,
    ) -> Result<crate::model_cost::ModelPricingCatalog, String> {
        match &self.settings_paths {
            Some(paths) => crate::model_cost::ModelPricingCatalog::read_from_paths(paths, now),
            None => Ok(crate::model_cost::ModelPricingCatalog::default()),
        }
    }

    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        let config = self.current_config().map_err(|error| error.detail)?;
        Self::validate_config(&config).map_err(|error| error.detail)?;
        Ok(ResolvedProviderProfile {
            provider: config.provider_id,
            model: config.model_id,
            endpoint: config.endpoint,
            timeout_ms: config.timeout_ms,
        })
    }

    fn resolve_step_profile(&self, capability: &str) -> Result<ResolvedProviderProfile, String> {
        let mut profile = self.resolve_profile()?;
        let setting = match capability {
            "RENDER_CHAPTER" | "REWRITE_PASSAGES" => "renderModelId",
            // Owner decision (2026-10-24): r2 plans use the audit model.
            "AUDIT_EXTRACT"
            | "REVIEW_CHAPTER"
            | "CHECK_CONTINUITY"
            | crate::craft_plan::PLAN_CHAPTER_CRAFT => "auditModelId",
            _ => return Ok(profile),
        };
        if let Some(paths) = &self.settings_paths {
            let settings =
                crate::repositories::provider::ProviderSettingsRepository::read_global(paths)
                    .map_err(|error| format!("provider settings unavailable: {error}"))?;
            if let Some(model) = settings
                .get(setting)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|model| !model.is_empty())
            {
                profile.model = model.to_owned();
            }
        }
        Ok(profile)
    }

    fn invoke_typed_with_profile(
        &self,
        capability: &str,
        input: &Value,
        profile: &ResolvedProviderProfile,
    ) -> Result<ProductionModelResponse, TransportFailure> {
        self.invoke_typed_with_diagnostic(capability, input, Some(profile))
            .0
    }

    fn invoke_typed_with_diagnostic(
        &self,
        capability: &str,
        input: &Value,
        profile: Option<&ResolvedProviderProfile>,
    ) -> (
        Result<ProductionModelResponse, TransportFailure>,
        Option<ProviderAttemptDiagnostic>,
    ) {
        let config = match profile {
            Some(profile) => Ok(ProviderConfig {
                provider_id: profile.provider.clone(),
                model_id: profile.model.clone(),
                endpoint: profile.endpoint.clone(),
                enabled: true,
                timeout_ms: profile.timeout_ms,
            }),
            None => self.current_config(),
        };
        match config {
            Ok(config) => Self::typed_provider_result(
                self.invoke_provider_with_config(&config, capability, input),
            ),
            Err(error) => Self::typed_provider_result(Err(error)),
        }
    }

    fn invoke(&self, capability: &str, input: &Value) -> Result<ProductionModelResponse, String> {
        self.invoke_provider(capability, input)
            .map(Self::response_from_provider)
            .map_err(|error| format!("P01_{}:{}", error.kind.code(), error.detail))
    }

    fn invoke_typed(
        &self,
        capability: &str,
        input: &Value,
    ) -> Result<ProductionModelResponse, TransportFailure> {
        self.invoke_typed_with_diagnostic(capability, input, None).0
    }
}

pub struct GatewayTransport<'a> {
    gateway: &'a dyn ProductionModelGateway,
    profile: Option<&'a ResolvedProviderProfile>,
    capability: &'a str,
    input: Value,
    raw_persist_failure: bool,
    diagnostic: Option<ModelTransportDiagnostic>,
}

impl<'a> GatewayTransport<'a> {
    pub fn new(gateway: &'a dyn ProductionModelGateway, capability: &'a str, input: Value) -> Self {
        Self {
            gateway,
            profile: None,
            capability,
            input,
            raw_persist_failure: false,
            diagnostic: None,
        }
    }

    pub fn with_profile(
        gateway: &'a dyn ProductionModelGateway,
        profile: &'a ResolvedProviderProfile,
        capability: &'a str,
        input: Value,
    ) -> Self {
        Self {
            gateway,
            profile: Some(profile),
            capability,
            input,
            raw_persist_failure: false,
            diagnostic: None,
        }
    }

    #[doc(hidden)]
    pub fn with_raw_persist_failure(
        gateway: &'a dyn ProductionModelGateway,
        capability: &'a str,
        input: Value,
    ) -> Self {
        Self {
            gateway,
            profile: None,
            capability,
            input,
            raw_persist_failure: true,
            diagnostic: None,
        }
    }
}

impl ModelTransport for GatewayTransport<'_> {
    fn diagnostics_enabled(&self) -> bool {
        self.gateway.transport_diagnostics_enabled()
    }

    fn diagnostic_timeout_ms(&self) -> Option<u64> {
        self.profile.map(|profile| profile.timeout_ms)
    }

    fn take_diagnostic(&mut self) -> Option<ModelTransportDiagnostic> {
        self.diagnostic.take()
    }

    fn defer_usage_until_raw_persist(&self) -> bool {
        true
    }

    fn usage_after_raw_persist(
        &mut self,
        _connection: &libsql_rusqlite::Connection,
        _spec: &ModelRunSpec,
        raw_response: &str,
        usage: &TransportUsage,
    ) -> Result<TransportUsage, RuntimeError> {
        let Ok(envelope) = serde_json::from_str::<Value>(raw_response) else {
            return Ok(usage.clone());
        };
        let Some(provider_usage) = envelope.get("usage") else {
            return Ok(usage.clone());
        };
        let input_tokens = provider_usage
            .get("prompt_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0);
        let output_tokens = provider_usage
            .get("completion_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0);
        Ok(TransportUsage {
            input_tokens,
            output_tokens,
            cached_tokens: provider_usage
                .pointer("/prompt_tokens_details/cached_tokens")
                .and_then(Value::as_i64)
                .map(|value| value.max(0)),
            provider_reported_cost_cents: usage.provider_reported_cost_cents,
            latency_ms: usage.latency_ms,
            provider_request_id: envelope
                .get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| usage.provider_request_id.clone()),
        })
    }

    fn before_raw_persist(
        &mut self,
        _connection: &libsql_rusqlite::Connection,
        _spec: &ModelRunSpec,
    ) -> Result<(), RuntimeError> {
        if self.raw_persist_failure {
            return Err(RuntimeError::RawResponsePersistenceFailed);
        }
        Ok(())
    }

    fn send(
        &mut self,
        _spec: &ModelRunSpec,
        _credential: &str,
    ) -> Result<TransportResponse, TransportFailure> {
        let (response, diagnostic) =
            self.gateway
                .invoke_typed_with_diagnostic(self.capability, &self.input, self.profile);
        self.diagnostic = diagnostic.map(Into::into);
        let response = response?;
        Ok(TransportResponse {
            raw_response: response
                .raw_response
                .unwrap_or_else(|| response.output.to_string()),
            usage: response.usage,
        })
    }
}

fn parse_provider_model_payload(value: Value) -> Result<Value, String> {
    let Some(choices) = value.get("choices") else {
        return Ok(value);
    };
    if !choices.is_array() {
        return Err("P01_PROVIDER_MALFORMED_ENVELOPE:choices must be an array".into());
    }
    if choices.as_array().is_some_and(|items| items.is_empty()) {
        return Err("P01_PROVIDER_CONTENT_MISSING:choices is empty".into());
    }
    let content = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| "P01_PROVIDER_CONTENT_MISSING:message.content missing".to_string())?;
    if content.trim().is_empty() {
        return Err("P01_PROVIDER_EMPTY_CONTENT:message.content is empty".into());
    }
    let content = content.trim();
    let content = if let Some(fenced) = content.strip_prefix("```") {
        let fenced = fenced
            .strip_prefix("json")
            .or_else(|| fenced.strip_prefix("JSON"))
            .unwrap_or(fenced);
        fenced
            .trim_start()
            .strip_suffix("```")
            .map(str::trim)
            .unwrap_or(content)
    } else {
        content
    };
    serde_json::from_str(content).map_err(|error| format!("P01_MODEL_JSON_INVALID:{error}"))
}

/// Some compatible endpoints have returned a complete body value but omitted
/// only the final JSON string/object delimiters. Recover that narrow renderer
/// shape without relaxing planner/audit contracts or accepting trailing
/// content; the original raw response remains durably recorded.
fn recover_truncated_renderer_payload(value: &Value) -> Option<Value> {
    let content = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|content| content.starts_with("{\"body\":\""))?;
    // Writers at a creative temperature also put raw line breaks inside the
    // body string; escaping them changes no characters of the prose.
    let content = escape_raw_control_characters(content);
    let payload = serde_json::from_str::<Value>(&content)
        .or_else(|_| serde_json::from_str::<Value>(&format!("{content}\"}}")))
        .ok()?;
    payload
        .get("body")
        .and_then(Value::as_str)
        .filter(|body| !body.trim().is_empty())?;
    Some(payload)
}

/// Escapes control characters that appear unescaped inside JSON strings.
fn escape_raw_control_characters(json_text: &str) -> String {
    let mut escaped = String::with_capacity(json_text.len());
    let (mut in_string, mut after_backslash) = (false, false);
    for character in json_text.chars() {
        if in_string && !after_backslash && character.is_control() && (character as u32) < 0x20 {
            match character {
                '\n' => escaped.push_str("\\n"),
                '\r' => escaped.push_str("\\r"),
                '\t' => escaped.push_str("\\t"),
                other => escaped.push_str(&format!("\\u{:04x}", other as u32)),
            }
            continue;
        }
        if in_string && character == '\\' && !after_backslash {
            after_backslash = true;
        } else {
            if character == '"' && !after_backslash {
                in_string = !in_string;
            }
            after_backslash = false;
        }
        escaped.push(character);
    }
    escaped
}

/// FX-19 (owner decision 2026-10-17): a quote joining passages with an
/// ellipsis binds to the full body passage when every part occurs exactly
/// once in the body, in order, and at most 300 characters are omitted.
fn bounded_ellipsis_span(body: &str, quote: &str) -> Option<(usize, usize)> {
    let normalized = quote.replace("……", "...").replace('…', "...");
    let parts = normalized
        .split("...")
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 2 || parts.iter().any(|part| body.matches(part).count() != 1) {
        return None;
    }
    let (start, end) = ellipsis_quote_span(body, quote)?;
    let quoted = parts.iter().map(|part| part.chars().count()).sum::<usize>();
    (end - start <= quoted + 300).then_some((start, end))
}

fn ellipsis_quote_span(body: &str, quote: &str) -> Option<(usize, usize)> {
    let normalized = quote.replace("……", "...").replace('…', "...");
    let segments = normalized
        .split("...")
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.len() < 2 {
        return None;
    }
    let mut selected = Vec::new();
    let mut cursor = 0usize;
    let mut previous_end = None;
    for segment in segments {
        let matches = body
            .match_indices(segment)
            .filter(|(start, _)| *start >= cursor)
            .filter(|(start, _)| {
                previous_end.is_none_or(|end| body[end..*start].chars().count() <= 512)
            })
            .take(2)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return None;
        }
        let (byte_start, matched) = matches[0];
        let byte_end = byte_start + matched.len();
        selected.push((byte_start, byte_end));
        cursor = byte_end;
        previous_end = Some(byte_end);
    }
    let (first, _) = *selected.first()?;
    let (_, last) = *selected.last()?;
    Some((body[..first].chars().count(), body[..last].chars().count()))
}

pub(crate) fn parse_provider_model_payload_for_capability(
    value: Value,
    capability: &str,
) -> Result<Value, String> {
    match parse_provider_model_payload(value.clone()) {
        Ok(payload) => Ok(payload),
        Err(error)
            if matches!(
                capability,
                "RENDER_CHAPTER" | "RENDER_CHAPTER_CRAFT" | "ADJUST_CHAPTER_LENGTH"
            ) =>
        {
            recover_truncated_renderer_payload(&value).ok_or(error)
        }
        Err(error) if capability == "AUDIT_EXTRACT" => {
            Err(format!("B08_AUDIT_TRUNCATED_OR_INVALID:{error}"))
        }
        Err(error) => Err(error),
    }
}

fn classify_provider_body(raw: &str) -> Option<&'static str> {
    if raw.trim().is_empty() {
        return Some("P01_PROVIDER_EMPTY_BODY");
    }
    serde_json::from_str::<Value>(raw)
        .is_err()
        .then_some("P01_PROVIDER_MALFORMED_ENVELOPE")
}

/// Execute one model stage through the formal configured production
/// composition.  DurableModelRunner persists the exact transport body first;
/// only the returned, durably parsed envelope is converted into the model
/// payload used by Plan/Render/Audit owners.
pub fn execute_production_model(
    connection: &libsql_rusqlite::Connection,
    gateway: &dyn ProductionModelGateway,
    spec: &ModelRunSpec,
    capability: &str,
    input: Value,
    now: &str,
) -> Result<ModelRunOutcome, String> {
    let spec = priced_model_run_spec(gateway, spec, capability, &input, now)?;
    let mut transport = GatewayTransport::new(gateway, capability, input);
    DurableModelRunner::prepare(connection, &spec, now).map_err(|error| error.to_string())?;
    let mut outcome = match DurableModelRunner::execute(connection, &mut transport, &spec, now) {
        Ok(outcome) => outcome,
        Err(error) => {
            let raw = DurableModelRunner::persisted_raw_response(connection, &spec.model_run_id)
                .map_err(|runtime_error| runtime_error.to_string())?;
            if let Some(code) = raw.as_deref().and_then(classify_provider_body) {
                return Err(code.into());
            }
            return Err(error.to_string());
        }
    };
    if outcome.replayed && outcome.parsed.is_none() {
        outcome.parsed = Some(
            DurableModelRunner::persisted_parsed_response(connection, &spec.model_run_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "MODEL_REPLAY_RESULT_MISSING".to_string())?,
        );
    }
    if let Some(parsed) = outcome.parsed.take() {
        outcome.parsed = Some(
            match parse_provider_model_payload_for_capability(parsed, capability).and_then(
                |payload| {
                    // LD-1: a fresh reply must be on its step's output spec.
                    if outcome.replayed {
                        Ok(payload)
                    } else {
                        crate::model_output::check(capability, &payload).map(|()| payload)
                    }
                },
            ) {
                Ok(payload) => payload,
                Err(error) => {
                    DurableModelRunner::mark_model_payload_parse_failure(
                        connection, &spec, &error, now,
                    )
                    .map_err(|runtime_error| runtime_error.to_string())?;
                    return Err(error);
                }
            },
        );
    }
    Ok(outcome)
}

/// FX-19 (owner decision 2026-10-17): a reply the provider cut off in the
/// middle of the JSON (reported as a normal stop), or one that is not JSON at
/// all, is resent once as a new model run; the bad run stays recorded as
/// failed.
pub fn execute_production_model_with_profile(
    connection: &libsql_rusqlite::Connection,
    gateway: &dyn ProductionModelGateway,
    profile: &ResolvedProviderProfile,
    spec: &ModelRunSpec,
    capability: &str,
    input: Value,
    now: &str,
) -> Result<ModelRunOutcome, String> {
    resend_once_on_invalid_json(spec, |spec, off_spec| {
        let mut input = input.clone();
        if let Some(error) = off_spec {
            input["outputFeedback"] = json!({
                "rejectedFor":error,
                "instruction":"上一次回复不符合输出规范：按 rejectedFor 列出的问题修正，返回完整回复。"
            });
        }
        execute_production_model_with_profile_once(
            connection, gateway, profile, spec, capability, input, now,
        )
    })
}

/// Runs `run` with `spec`; a reply that is not valid JSON (FX-19) or not on
/// its step's output spec (LD-1) is resent once as a new model run, the
/// latter with what was off the spec; the bad run stays recorded as failed.
pub fn resend_once_on_invalid_json(
    spec: &ModelRunSpec,
    run: impl Fn(&ModelRunSpec, Option<&str>) -> Result<ModelRunOutcome, String>,
) -> Result<ModelRunOutcome, String> {
    match run(spec, None) {
        Err(error) if error.contains("P01_MODEL_JSON_INVALID:") => {
            run(&follow_up_spec(spec, ":truncated-resend", 1)?, None)
        }
        Err(error) if error.contains(crate::model_output::OFF_SPEC) => {
            run(&follow_up_spec(spec, ":off-spec-resend", 1)?, Some(&error))
        }
        result => result,
    }
}

/// A new model run following `spec`: its ids carry `suffix` and its attempt
/// number is `later` attempts after the original.
pub(crate) fn follow_up_spec(
    spec: &ModelRunSpec,
    suffix: &str,
    later: i64,
) -> Result<ModelRunSpec, String> {
    Ok(ModelRunSpec {
        model_run_id: StableIdentityFactory::from_command(
            StableIdKind::ModelRun,
            &format!("{}{suffix}", spec.model_run_id),
        )?,
        idempotency_key: format!("{}{suffix}", spec.idempotency_key),
        attempt_no: spec.attempt_no + later,
        ..spec.clone()
    })
}

/// How many times a rejected plan is planned again within one job.
const PLAN_FEEDBACK_ROUNDS: i64 = 2;

/// Craft rule (2026-10-17): a plan rejected for a reason the planner can fix
/// (see `chapter_craft::plan_feedback`) is planned again with the reason, up
/// to `PLAN_FEEDBACK_ROUNDS` times (owner decision 2026-10-25); the rejected
/// runs stay recorded. Returns the accepted plan and the run that produced it.
pub fn plan_chapter_with_feedback(
    connection: &libsql_rusqlite::Connection,
    gateway: &dyn ProductionModelGateway,
    profile: &ResolvedProviderProfile,
    spec: &ModelRunSpec,
    input: Value,
    now: &str,
    accept: &dyn Fn(Value, bool) -> Result<(Value, Vec<String>), String>,
) -> Result<(Value, String, Vec<String>), String> {
    let plan = |spec: &ModelRunSpec, input: Value| {
        execute_production_model_with_profile(
            connection,
            gateway,
            profile,
            spec,
            &spec.capability,
            input,
            now,
        )?
        .parsed
        .ok_or_else(|| "B08_PLANNING_OUTPUT_INVALID".to_string())
    };
    let mut input = input;
    let mut run_id = spec.model_run_id.clone();
    let mut last = plan(spec, input.clone())?;
    let mut result = accept(last.clone(), false);
    for round in 1..=PLAN_FEEDBACK_ROUNDS {
        let Some(feedback) = result
            .as_ref()
            .err()
            .and_then(|error| crate::chapter_craft::plan_feedback(error))
        else {
            break;
        };
        // The first replan keeps its original id so earlier runs replay.
        let suffix = match round {
            1 => ":plan-feedback".to_string(),
            round => format!(":plan-feedback-{round}"),
        };
        let replan = follow_up_spec(spec, &suffix, round)?;
        input["planFeedback"] = feedback;
        last = plan(&replan, input.clone())?;
        result = accept(last.clone(), false);
        run_id = replan.model_run_id;
    }
    // LD-1 step 2: problems the planner was told about and could not fix go
    // on with the last plan as a reminder; anything else still fails.
    let (plan, problems) = match result {
        Ok(accepted) => accepted,
        Err(error) if crate::chapter_craft::plan_feedback(&error).is_some() => accept(last, true)?,
        Err(error) => return Err(error),
    };
    Ok((plan, run_id, problems))
}

/// In a lenient plan check (LD-1 step 2), a problem the planner could fix is
/// noted in `problems` instead of failing; everything else still fails.
pub(crate) fn note_if_lenient(
    lenient: bool,
    problems: &mut Vec<String>,
    result: Result<(), String>,
) -> Result<(), String> {
    match result {
        Err(error) if lenient && crate::chapter_craft::plan_feedback(&error).is_some() => {
            problems.push(error);
            Ok(())
        }
        other => other,
    }
}

fn execute_production_model_with_profile_once(
    connection: &libsql_rusqlite::Connection,
    gateway: &dyn ProductionModelGateway,
    profile: &ResolvedProviderProfile,
    spec: &ModelRunSpec,
    capability: &str,
    input: Value,
    now: &str,
) -> Result<ModelRunOutcome, String> {
    let spec = priced_model_run_spec(gateway, spec, capability, &input, now)?;
    let mut transport = GatewayTransport::with_profile(gateway, profile, capability, input);
    DurableModelRunner::prepare(connection, &spec, now).map_err(|error| error.to_string())?;
    let mut outcome = match DurableModelRunner::execute(connection, &mut transport, &spec, now) {
        Ok(outcome) => outcome,
        Err(error) => {
            let raw = DurableModelRunner::persisted_raw_response(connection, &spec.model_run_id)
                .map_err(|runtime_error| runtime_error.to_string())?;
            if let Some(code) = raw.as_deref().and_then(classify_provider_body) {
                return Err(code.into());
            }
            return Err(error.to_string());
        }
    };
    if outcome.replayed && outcome.parsed.is_none() {
        outcome.parsed = Some(
            DurableModelRunner::persisted_parsed_response(connection, &spec.model_run_id)
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "MODEL_REPLAY_RESULT_MISSING".to_string())?,
        );
    }
    if let Some(parsed) = outcome.parsed.take() {
        outcome.parsed = Some(
            match parse_provider_model_payload_for_capability(parsed, capability).and_then(
                |payload| {
                    // LD-1: a fresh reply must be on its step's output spec.
                    if outcome.replayed {
                        Ok(payload)
                    } else {
                        crate::model_output::check(capability, &payload).map(|()| payload)
                    }
                },
            ) {
                Ok(payload) => payload,
                Err(error) => {
                    DurableModelRunner::mark_model_payload_parse_failure(
                        connection, &spec, &error, now,
                    )
                    .map_err(|runtime_error| runtime_error.to_string())?;
                    return Err(error);
                }
            },
        );
    }
    Ok(outcome)
}

fn priced_model_run_spec(
    gateway: &dyn ProductionModelGateway,
    spec: &ModelRunSpec,
    capability: &str,
    input: &Value,
    now: &str,
) -> Result<ModelRunSpec, String> {
    let catalog = gateway.model_pricing_catalog(now)?;
    let snapshot = catalog.cost_snapshot(&spec.provider, &spec.model, capability, input);
    let mut priced = spec.clone();
    priced.predicted_cost_cents = snapshot.predicted_cost_cents;
    priced.cost_snapshot = Some(snapshot);
    Ok(priced)
}

fn require(value: &Value, path: &str) -> Result<String, String> {
    value
        .pointer(path)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("B08_INVALID:{path}"))
}

fn chapter_scope(chapter_id: &str) -> Value {
    json!({"level":"CHAPTER","chapterId":chapter_id})
}

fn execute_reconciliation_action(
    handle: &ProjectHandle,
    project_id: &str,
    volume_id: &str,
    source_chapter_id: &str,
    action: &str,
    command_id: &str,
    now: &str,
) -> Result<Value, String> {
    if action != "NO_ACTION" {
        let payload =
            json!({"sourceChapterId":source_chapter_id,"status":"DEFERRED","reason":action});
        DurableJobService::record_reconciliation_action(
            handle, project_id, command_id, action, &payload, now,
        )
        .map_err(|error| error.to_string())?;
        return Ok(payload);
    }

    if let Some(chapter_id) =
        crate::repositories::narrative::NarrativeRepository::next_unsubmitted_chapter_id(
            handle, project_id, volume_id,
        )
        .map_err(|error| error.to_string())?
    {
        let chapter_no = crate::repositories::narrative::NarrativeRepository::chapter_number(
            handle,
            project_id,
            &chapter_id,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "CHAPTER_NOT_FOUND".to_string())?;
        let payload = json!({
            "sourceChapterId":source_chapter_id,
            "nextChapterId":chapter_id,
            "nextChapterNo":chapter_no,
            "status":"ALREADY_PREPARED"
        });
        DurableJobService::record_reconciliation_action(
            handle, project_id, command_id, action, &payload, now,
        )
        .map_err(|error| error.to_string())?;
        return Ok(payload);
    }

    let next_no = crate::repositories::narrative::NarrativeRepository::next_chapter_number(
        handle, project_id, volume_id,
    )
    .map_err(|error| error.to_string())?;
    let Some(next_no) = next_no else {
        let payload = json!({"sourceChapterId":source_chapter_id,"status":"NO_NEXT_CHAPTER"});
        DurableJobService::record_reconciliation_action(
            handle, project_id, command_id, action, &payload, now,
        )
        .map_err(|error| error.to_string())?;
        return Ok(payload);
    };
    let chapter_id = StableIdentityFactory::from_command(
        StableIdKind::Chapter,
        &format!("{project_id}:{volume_id}:chapter:{next_no}"),
    )?;
    let budget_id = format!("budget:{chapter_id}");
    let prepare_command_id = format!("{command_id}:reconciliation:prepare");
    let prepare_request = json!({
        "type":"planning_prepare_next_chapter",
        "projectId":project_id,
        "commandId":prepare_command_id,
        "idempotencyKey":format!("{prepare_command_id}:key"),
        "requestedAt":now,
        "payload":{"volumeId":volume_id}
    });
    let prepare_digest = sha256_canonical(&prepare_request).map_err(|error| error.to_string())?;
    let prepare_idempotency_key = format!("{prepare_command_id}:key");
    let prepare_spec = ProjectCommandSpec {
        project_id,
        result_project_id: project_id,
        command_id: &prepare_command_id,
        idempotency_key: &prepare_idempotency_key,
        command_type: "planning_prepare_next_chapter",
        request_digest: &prepare_digest,
        expected_revision: Some(
            crate::repositories::narrative::NarrativeRepository::project_revision(
                handle, project_id,
            )
            .map_err(|error| error.to_string())?,
        ),
        now,
        advance_revision: true,
    };
    let prepare_outcome =
        CommitService::commit_project_command(handle, prepare_spec, |connection| {
            crate::repositories::narrative::NarrativeRepository::ensure_planned_chapter(
                connection,
                project_id,
                volume_id,
                &chapter_id,
                next_no,
                &format!("第{next_no}章 · 当前卷推进"),
                &budget_id,
                now,
            )
            .map_err(|error| crate::commit::CommitError::ProjectCommandApply(error.to_string()))?;
            Ok(json!({"chapterId":chapter_id,"chapterNo":next_no,"volumeId":volume_id}))
        })
        .map_err(|error| error.to_string())?;
    let prepare_receipt = match prepare_outcome {
        ProjectCommandOutcome::Replayed(receipt) => receipt,
        ProjectCommandOutcome::Committed { receipt, .. } => receipt,
    };
    let payload = json!({
        "sourceChapterId":source_chapter_id,
        "nextChapterId":chapter_id,
        "nextChapterNo":next_no,
        "status":"PREPARED",
        "prepareReceiptId":prepare_receipt.receipt_id
    });
    DurableJobService::record_reconciliation_action(
        handle, project_id, command_id, action, &payload, now,
    )
    .map_err(|error| error.to_string())?;
    Ok(payload)
}

#[allow(clippy::too_many_arguments)]
fn artifact(
    project_id: &str,
    artifact_id: String,
    artifact_type: ArtifactType,
    chapter_id: &str,
    payload: Value,
    command_id: &str,
    job_id: &str,
    model_run_id: Option<&str>,
    now: &str,
    dependency_refs: Vec<ArtifactRef>,
) -> Result<ArtifactEnvelope, String> {
    let payload_hash = sha256_canonical(&payload).map_err(|error| error.to_string())?;
    Ok(ArtifactEnvelope {
        artifact_id,
        project_id: project_id.to_string(),
        artifact_type,
        scope: serde_json::from_value(chapter_scope(chapter_id)).map_err(|e| e.to_string())?,
        version: 1,
        // ArtifactEnvelope V2 remains the frozen L3 envelope; B08 payloads are
        // versioned inside the payloadSchema field rather than introducing a
        // parallel envelope contract.
        schema_version: "m0a.artifact.v1".into(),
        artifact_status: ArtifactStatus::Candidate,
        semantic_lifecycle: None,
        authority_level: None,
        payload,
        payload_hash,
        dependency_refs,
        provenance: ArtifactEnvelopeProvenance {
            created_by: "B08::ChapterProductionRuntime".into(),
            command_id: command_id.into(),
            job_id: Some(job_id.into()),
            step_id: None,
            model_run_id: model_run_id.map(ToOwned::to_owned),
            prompt_ref: None,
            context_manifest_ref: None,
            decision_refs: None,
            gate_certificate_refs: None,
        },
        created_at: now.into(),
        committed_at: None,
    })
}

fn reference(artifact: &ArtifactEnvelope) -> ArtifactRef {
    ArtifactRef {
        artifact_id: artifact.artifact_id.clone(),
        version: artifact.version,
        payload_hash: artifact.payload_hash.clone(),
    }
}

struct B08ChangeRequestContext<'a> {
    project_id: &'a str,
    command_id: &'a str,
    job_id: &'a str,
    requested_at: &'a str,
    current_revision: i64,
    final_id: &'a str,
    chapter_id: &'a str,
    audit: &'a ArtifactEnvelope,
    candidate: &'a ArtifactEnvelope,
}

fn create_b08_change_request_proposal(
    context: &B08ChangeRequestContext<'_>,
    index: usize,
    item: &ContinuityCandidate,
    summary: &str,
    reason: &str,
) -> Result<(ArtifactEnvelope, Value, Value), String> {
    let artifact_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("b08-change-request:{}:{index}", context.final_id),
    )
    .map_err(|error| error.to_string())?;
    let payload = json!({
        "payloadSchema":"b12-change-request.r1",
        "changeRequestId":artifact_id,
        "changeType":"CHANGE",
        "status":"PROPOSED",
        "summary":summary,
        "requestedChange":{"kind":"WITHDRAW_RESPONSIBILITY","responsibilityRef":item.thread_ref},
        "responsibilityRef":item.thread_ref,
        "basedOnRevision":context.current_revision,
        "sourceAuditRef":reference(context.audit),
        "sourceChapterRef":reference(context.candidate),
        "evidence":item.evidence.as_ref().map(|evidence| {
            json!({"start":evidence.start,"end":evidence.end,"quote":evidence.quote})
        })
    });
    let artifact = artifact(
        context.project_id,
        artifact_id.clone(),
        ArtifactType::ChangeRequest,
        context.chapter_id,
        payload,
        context.command_id,
        context.job_id,
        None,
        context.requested_at,
        vec![reference(context.audit), reference(context.candidate)],
    )?;
    let artifact_head = json!({
        "artifactType":"ChangeRequest",
        "scope":chapter_scope(context.chapter_id),
        "artifactId":artifact.artifact_id,
        "version":artifact.version,
        "payloadHash":artifact.payload_hash
    });
    let decision_id = StableIdentityFactory::from_command(
        StableIdKind::Decision,
        &format!("b08-thread-change-request:{}:{index}", context.final_id),
    )
    .map_err(|error| error.to_string())?;
    let decision = json!({
        "decisionId":decision_id,
        "decisionType":"NO_LONGER_APPLICABLE_CHANGE_REQUEST",
        "outcome":"PROPOSED",
        "reason":reason,
        "sourceRef":item.thread_ref,
        "payloadJson":{"threadRef":item.thread_ref,"changeRequestRef":reference(&artifact),"sourceAuditRef":reference(context.audit)}
    });
    Ok((artifact, artifact_head, decision))
}

fn load_artifact_payload(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    reference: &ArtifactRef,
) -> Result<Value, String> {
    crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
        connection, project_id, reference,
    )
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "B08_BOUND_ARTIFACT_NOT_FOUND".to_string())
}

fn authority_request(
    request: &Value,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    execution_mode: &str,
) -> Value {
    json!({
        "projectId": project_id,
        "chapterId": chapter_id,
        "volumeId": volume_id,
        "executionMode": execution_mode,
        "scope": "CHAPTER",
        "entityIds": request.pointer("/payload/entityIds").cloned().unwrap_or_else(|| json!([])),
        "references": request.pointer("/payload/references").cloned().unwrap_or_else(|| json!([])),
        "scopeSelection": request.pointer("/payload/scopeSelection").cloned().unwrap_or(Value::Null),
        "presentEntityIds": request.pointer("/payload/presentEntityIds").cloned().unwrap_or_else(|| json!([])),
        "acquiredFactIds": request.pointer("/payload/acquiredFactIds").cloned().unwrap_or_else(|| json!([])),
        "satisfiedTaskIds": request.pointer("/payload/satisfiedTaskIds").cloned().unwrap_or_else(|| json!([])),
        "includeSoftSources": false
    })
}

fn authority_context_payload(
    input: &crate::narrative::ContextCompilerInput,
) -> Result<Value, String> {
    serde_json::to_value(input).map_err(|error| format!("B06_CONTEXT_SERIALIZATION:{error}"))
}

fn audit_policy_contract(
    policy_ref: &ArtifactRef,
    gate_policy_payload: &Value,
) -> Result<Value, String> {
    let actual_payload_hash = sha256_canonical(gate_policy_payload)
        .map_err(|error| format!("B08_GATE_POLICY_INVALID:{error}"))?;
    if actual_payload_hash != policy_ref.payload_hash {
        return Err("B08_GATE_POLICY_INVALID_HASH".into());
    }
    let policy = crate::canon_semantics::CanonSemanticPolicy::from_gate_policy(gate_policy_payload)
        .map_err(str::to_owned)?;
    let semantics = gate_policy_payload
        .get("canonSemantics")
        .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS")?;
    let canon_semantics = json!({
        "policyVersion":policy.version.clone(),
        "rules":semantics
            .get("rules")
            .cloned()
            .ok_or("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS")?
    });

    let audit_policy_contract = json!({
        "gatePolicyRef":policy_ref,
        "gatePolicyPayloadHash":policy_ref.payload_hash.clone(),
        "canonSemanticPolicyVersion":policy.version.clone(),
        "canonSemanticPolicy":canon_semantics
    });
    validate_audit_policy_contract_size(
        &audit_policy_contract,
        AUDIT_POLICY_HARD_TOKEN_LIMIT,
        AUDIT_POLICY_HARD_SERIALIZED_BYTE_LIMIT,
    )?;
    Ok(audit_policy_contract)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AuditPolicyContractSize {
    estimated_tokens: usize,
    serialized_bytes: usize,
}

const AUDIT_POLICY_HARD_TOKEN_LIMIT: i64 = 1024;
const AUDIT_POLICY_HARD_SERIALIZED_BYTE_LIMIT: usize = 8192;

fn audit_policy_contract_size(contract: &Value) -> Result<AuditPolicyContractSize, String> {
    let serialized = serde_json::to_string(contract).map_err(|error| error.to_string())?;
    let ascii_bytes = serialized.bytes().filter(u8::is_ascii).count();
    let non_ascii_scalars = serialized
        .chars()
        .filter(|character| !character.is_ascii())
        .count();
    Ok(AuditPolicyContractSize {
        estimated_tokens: ascii_bytes.div_ceil(3) + non_ascii_scalars,
        serialized_bytes: serialized.len(),
    })
}

fn validate_audit_policy_contract_size(
    contract: &Value,
    token_limit: i64,
    byte_limit: usize,
) -> Result<AuditPolicyContractSize, String> {
    let size = audit_policy_contract_size(contract)?;
    if size.estimated_tokens as i64 > token_limit || size.serialized_bytes > byte_limit {
        return Err(format!(
            "B08_GATE_POLICY_HARD_BUDGET_EXCEEDED|scope=AUDIT_POLICY_CONTRACT|usedTokens={}|limitTokens={token_limit}|serializedBytes={}|limitBytes={byte_limit}",
            size.estimated_tokens,
            size.serialized_bytes,
        ));
    }
    Ok(size)
}

fn current_authority_digest(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
    chapter_scope: (&str, &str),
    execution_mode: &str,
    chapter_plan: &Value,
    revision: i64,
) -> Result<String, String> {
    let (chapter_id, volume_id) = chapter_scope;
    let mut authority =
        authority_request(request, project_id, chapter_id, volume_id, execution_mode);
    authority["chapterPlan"] = chapter_plan.clone();
    let input = crate::commands::context_input_from_authority(
        connection, &authority, project_id, revision,
    )?;
    crate::narrative::relevant_state_digest(&input)
}

#[allow(clippy::too_many_arguments)]
fn validate_plan_is_current(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    execution_mode: &str,
    plan_ref: &ArtifactRef,
    plan: &Value,
) -> Result<(), String> {
    let revision = crate::repositories::narrative::NarrativeRepository::project_revision(
        connection, project_id,
    )
    .map_err(|error| error.to_string())?;
    let digest = current_authority_digest(
        connection,
        request,
        project_id,
        (chapter_id, volume_id),
        execution_mode,
        plan,
        revision,
    )?;
    let compiled_revision = plan
        .get("compiledFromProjectRevision")
        .and_then(Value::as_i64)
        .ok_or_else(|| "B08_PLAN_REVISION_REQUIRED".to_string())?;
    let compiled_digest = plan
        .get("compiledFromStateDigest")
        .and_then(Value::as_str)
        .ok_or_else(|| "B08_PLAN_STATE_DIGEST_REQUIRED".to_string())?;
    let context_ref = plan
        .get("contextManifestRef")
        .and_then(|value| serde_json::from_value::<ArtifactRef>(value.clone()).ok());
    let mut stale_refs = vec![plan_ref.artifact_id.as_str(), chapter_id];
    if let Some(context_ref) = context_ref.as_ref() {
        stale_refs.push(context_ref.artifact_id.as_str());
    }
    if crate::repositories::change_retcon::has_pending_stale_for_refs(
        connection,
        project_id,
        &stale_refs,
    )
    .map_err(|error| error.to_string())?
    {
        return Err("PLAN_STALE:RECOMPILE_REQUIRED".into());
    }
    if matches!(
        crate::narrative::evaluate_plan_stale(
            compiled_revision,
            compiled_digest,
            revision,
            &digest
        ),
        crate::narrative::PlanStaleStatus::PlanStale
    ) {
        return Err("PLAN_STALE:RECOMPILE_REQUIRED".into());
    }
    Ok(())
}

fn same_retry_context_except_gate_policy(
    previous: &Value,
    current: &Value,
    gate_policy_artifact_id: &str,
) -> bool {
    let normalize = |value: &Value| {
        let mut value = value.clone();
        if let Some(object) = value.as_object_mut() {
            object.remove("compiledFromProjectRevision");
            object.remove("compiledFromStateDigest");
        }
        if let Some(refs) = value
            .get_mut("goalContractRefs")
            .and_then(Value::as_array_mut)
        {
            refs.retain(|reference| {
                reference.get("artifactId").and_then(Value::as_str) != Some(gate_policy_artifact_id)
            });
        }
        value
    };
    normalize(previous) == normalize(current)
}

#[allow(clippy::too_many_arguments)]
fn reusable_opening_plan_for_retry(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    previous_job_id: &str,
    current_policy_ref: &ArtifactRef,
    context_id: &str,
    job_id: &str,
    command_id: &str,
    idempotency_key: &str,
    now: &str,
    current_revision: &mut i64,
) -> Result<Option<(ArtifactRef, Value, ArtifactRef, Value)>, String> {
    let mut completed_plan = None;
    for capability in [crate::craft_plan::PLAN_CHAPTER_CRAFT, "PLAN_STRATEGY"] {
        completed_plan = DurableModelRunner::completed_result_for_job(
            connection,
            project_id,
            previous_job_id,
            capability,
        )
        .map_err(|error| error.to_string())?;
        if completed_plan.is_some() {
            break;
        }
    }
    let Some((planning_model_run_id, _)) = completed_plan else {
        return Ok(None);
    };
    let Some(plan_ref) = crate::repositories::narrative::NarrativeRepository::canonical_head_ref(
        connection,
        project_id,
        "ChapterPlan",
        chapter_id,
    )
    .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    // A completed planner run proves nothing about the head: its plan may
    // have been rejected, leaving an earlier plan (such as the opening
    // placeholder) in place. Only a plan that run produced is reused.
    let produced_by = crate::repositories::narrative::NarrativeRepository::artifact_model_run_id(
        connection, project_id, &plan_ref,
    )
    .map_err(|error| error.to_string())?;
    if produced_by.as_deref() != Some(planning_model_run_id.as_str()) {
        return Ok(None);
    }
    let mut plan_payload = load_artifact_payload(connection, project_id, &plan_ref)?;
    if plan_payload.get("chapterId").and_then(Value::as_str) != Some(chapter_id) {
        return Ok(None);
    }
    let Some(context_ref) = plan_payload
        .get("contextManifestRef")
        .cloned()
        .and_then(|value| serde_json::from_value::<ArtifactRef>(value).ok())
    else {
        return Ok(None);
    };
    let previous_context_payload = load_artifact_payload(connection, project_id, &context_ref)?;
    let compiler_input = crate::commands::context_input_from_authority(
        connection,
        &authority_request(
            request,
            project_id,
            chapter_id,
            volume_id,
            "OPENING_BOOTSTRAP",
        ),
        project_id,
        *current_revision,
    )?;
    let manifest = crate::narrative::compile_context_manifest(compiler_input)?;
    let context_payload = serde_json::to_value(&manifest).map_err(|error| error.to_string())?;
    if same_retry_context_except_gate_policy(
        &previous_context_payload,
        &context_payload,
        &current_policy_ref.artifact_id,
    ) {
        let context_command_id = format!("{command_id}:retry-context");
        let mut context = artifact(
            project_id,
            context_id.to_owned(),
            ArtifactType::ContextManifest,
            chapter_id,
            context_payload.clone(),
            &context_command_id,
            job_id,
            None,
            now,
            vec![],
        )?;
        context.provenance.created_by = "B06::ContextCompilerOwner".into();
        let context_ref = reference(&context);
        let context_bundle: CommitBundle = serde_json::from_value(json!({
            "commitId":format!("commit:{context_command_id}"),
            "projectId":project_id,
            "commandId":context_command_id,
            "idempotencyKey":format!("{idempotency_key}:retry-context"),
            "expectedProjectRevision":*current_revision,
            "contractVersion":"m0a.v1",
            "createdAt":now,
            "artifacts":[context],
            "headUpdates":[{"artifactType":"ContextManifest","scope":chapter_scope(chapter_id),"artifactId":context_ref.artifact_id,"version":context_ref.version,"payloadHash":context_ref.payload_hash}]
        }))
        .map_err(|error| format!("B08_CONTEXT_BUNDLE_INVALID:{error}"))?;
        let context_receipt = CommitService::commit(connection, &context_bundle, now)
            .map_err(|error| error.to_string())?;
        *current_revision = context_receipt.after_revision;

        plan_payload["compiledFromProjectRevision"] = json!(*current_revision + 1);
        plan_payload["compiledFromStateDigest"] = json!(manifest.compiled_from_state_digest);
        plan_payload["contextManifestRef"] =
            serde_json::to_value(&context_ref).map_err(|error| error.to_string())?;
        let plan_command_id = format!("{command_id}:retry-plan-rebind");
        let planning_receipt = crate::planning::recompile_chapter_plan_owner(
            connection,
            project_id,
            chapter_id,
            &plan_payload,
            &authority_request(
                request,
                project_id,
                chapter_id,
                volume_id,
                "OPENING_BOOTSTRAP",
            ),
            &plan_command_id,
            &format!("{idempotency_key}:retry-plan-rebind"),
            *current_revision,
            now,
            Some(&planning_model_run_id),
        )?;
        *current_revision = planning_receipt.after_revision;
        let plan_ref = planning_receipt
            .committed_artifact_refs
            .first()
            .cloned()
            .ok_or_else(|| "B08_PLANNING_OWNER_NO_RECEIPT".to_string())?;
        return Ok(Some((context_ref, context_payload, plan_ref, plan_payload)));
    }
    Ok(Some((
        context_ref,
        previous_context_payload,
        plan_ref,
        plan_payload,
    )))
}

#[allow(clippy::too_many_arguments)]
fn commit_chapter_after_revision_recheck(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    execution_mode: &str,
    plan_ref: &ArtifactRef,
    plan: &Value,
    bundle: &mut CommitBundle,
    now: &str,
    gateway: &dyn ProductionModelGateway,
) -> Result<crate::generated::types::CommitReceipt, String> {
    const MAX_REVISION_RETRIES: usize = 3;
    let original_expected_project_revision = bundle.expected_project_revision;

    for attempt in 0..=MAX_REVISION_RETRIES {
        gateway.before_commit_snapshot(connection, attempt)?;
        let revision = crate::repositories::narrative::NarrativeRepository::project_revision(
            connection, project_id,
        )
        .map_err(|error| error.to_string())?;
        validate_plan_is_current(
            connection,
            request,
            project_id,
            chapter_id,
            volume_id,
            execution_mode,
            plan_ref,
            plan,
        )?;
        if attempt > 0 || original_expected_project_revision != Some(revision) {
            gateway.on_chapter_gate_revalidation(attempt);
            revalidate_chapter_gate_on_current_snapshot(
                connection, project_id, chapter_id, plan, bundle, now,
            )?;
        }
        bundle.expected_project_revision = Some(revision);
        gateway.before_commit_attempt(connection, attempt)?;
        match CommitService::commit_b08_chapter_with_deferred_b09_projection(
            connection, bundle, volume_id, now,
        ) {
            Ok(receipt) => return Ok(receipt),
            Err(crate::commit::CommitError::RevisionConflict) if attempt < MAX_REVISION_RETRIES => {
                continue;
            }
            Err(crate::commit::CommitError::RevisionConflict) => {
                return Err("REVISION_CONFLICT".into());
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("REVISION_CONFLICT".into())
}

fn revalidate_chapter_gate_on_current_snapshot(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    chapter_id: &str,
    plan: &Value,
    bundle: &mut CommitBundle,
    now: &str,
) -> Result<(), String> {
    let artifacts = &bundle.artifacts;
    let audit_artifact = artifacts
        .iter()
        .find(|artifact| artifact.artifact_type == ArtifactType::ReviewReport)
        .ok_or_else(|| "B08_AUDIT_OUTPUT_INVALID".to_string())?;
    let candidate = artifacts
        .iter()
        .find(|artifact| artifact.artifact_type == ArtifactType::ChapterCandidate)
        .ok_or_else(|| "B08_CANDIDATE_NOT_FOUND".to_string())?;
    let certificate = bundle
        .gate_certificates
        .as_ref()
        .and_then(|certificates| certificates.first())
        .ok_or_else(|| "B08_GATE_CERTIFICATE_REQUIRED".to_string())?;
    let candidate_ref: ArtifactRef = audit_artifact
        .payload
        .get("candidateRef")
        .cloned()
        .ok_or_else(|| "B08_AUDIT_CANDIDATE_BINDING_MISMATCH".to_string())
        .and_then(|value| {
            serde_json::from_value(value)
                .map_err(|_| "B08_AUDIT_CANDIDATE_BINDING_MISMATCH".to_string())
        })?;
    validate_audit_candidate_binding(&audit_artifact.payload, &candidate_ref)?;

    let audit_payload = audit_artifact
        .payload
        .get("audit")
        .cloned()
        .ok_or_else(|| "B08_AUDIT_OUTPUT_INVALID".to_string())?;
    let mut extraction: AuditExtraction = serde_json::from_value(audit_payload.clone())
        .map_err(|error| format!("B08_AUDIT_INCOMPLETE:{error}"))?;
    validate_audit_completeness(&audit_payload, &extraction)?;
    let body = candidate
        .payload
        .get("body")
        .and_then(Value::as_str)
        .ok_or_else(|| "B08_CANDIDATE_BODY_REQUIRED".to_string())?;
    let gate_policy = load_artifact_payload(connection, project_id, &certificate.gate_policy_ref)?;
    validate_actual_extraction(
        connection,
        project_id,
        chapter_id,
        body,
        plan,
        &gate_policy,
        &mut extraction,
    )?;

    let certificate = make_certificate(
        project_id,
        certificate.gate_policy_ref.clone(),
        certificate.subject_ref.clone(),
        certificate.evidence_refs.clone(),
        now,
    )?;
    let certificates = bundle
        .gate_certificates
        .as_mut()
        .ok_or_else(|| "B08_GATE_CERTIFICATE_REQUIRED".to_string())?;
    certificates[0] = certificate;
    Ok(())
}

fn make_certificate(
    project_id: &str,
    policy: ArtifactRef,
    subject: ArtifactRef,
    evidence: Vec<ArtifactRef>,
    now: &str,
) -> Result<crate::generated::types::GateCertificate, String> {
    let source_digest = sha256_canonical(&json!({
        "gatePolicyRef": policy,
        "subjectRef": subject,
        "evidenceRefs": evidence,
    }))
    .map_err(|error| error.to_string())?;
    let certificate_id = format!("cert:b08:{}", subject.artifact_id.replace(':', "-"));
    let mut certificate = crate::generated::types::GateCertificate {
        certificate_id,
        project_id: project_id.into(),
        gate_policy_ref: policy,
        subject_ref: subject,
        evidence_refs: evidence,
        source_digest,
        passed_at: now.into(),
        certificate_digest: String::new(),
    };
    let mut value = serde_json::to_value(&certificate).map_err(|e| e.to_string())?;
    value["certificateDigest"] = Value::String(String::new());
    certificate.certificate_digest = sha256_canonical(&value).map_err(|e| e.to_string())?;
    Ok(certificate)
}

/// Error used to unwind at a step boundary after observing a control signal.
/// The signal remains pending until its CommitService receipt is durable.
const PRODUCTION_CONTROL_APPLIED: &str = "PRODUCTION_CONTROL_APPLIED";

fn update_stage(
    manager: &ProjectManager,
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    job_id: &str,
    stage: &str,
    now: &str,
) -> Result<(), String> {
    if !STAGES.contains(&stage) {
        return Err(format!("B08_INVALID_STAGE:{stage}"));
    }
    // FX-15: step boundaries are where a pause/cancel request takes effect;
    // an in-flight model call is never interrupted.
    if !matches!(stage, "CONTEXT" | "COMPLETED") {
        apply_production_control(manager, project_id, job_id)?;
    }
    DurableJobService::update_stage(connection, project_id, job_id, stage, now)
        .map_err(|error| error.to_string())
}

fn apply_production_control(
    manager: &ProjectManager,
    project_id: &str,
    job_id: &str,
) -> Result<(), String> {
    let guard = manager
        .lock_production_control(project_id)
        .map_err(|error| error.to_string())?;
    let Some(request) = manager
        .peek_production_control(&guard, project_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    if !request.accepted || request.job_id != job_id {
        return Ok(());
    }
    Err(PRODUCTION_CONTROL_APPLIED.into())
}

fn update_commit_stage(
    manager: &ProjectManager,
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    job_id: &str,
    now: &str,
) -> Result<crate::project_db::ProductionControlGuard, String> {
    let guard = manager
        .lock_production_control(project_id)
        .map_err(|error| error.to_string())?;
    if let Some(request) = manager
        .peek_production_control(&guard, project_id)
        .map_err(|error| error.to_string())?
    {
        if request.accepted && request.job_id == job_id {
            return Err(PRODUCTION_CONTROL_APPLIED.into());
        }
    }
    DurableJobService::update_stage(connection, project_id, job_id, "COMMIT", now)
        .map_err(|error| error.to_string())?;
    manager
        .run_production_control_commit_test_hook()
        .map_err(|error| error.to_string())?;
    Ok(guard)
}

fn block_for_recompile(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    job_id: &str,
    now: &str,
) -> Result<(), String> {
    DurableJobService::block_for_plan_stale(connection, project_id, job_id, now)
        .map_err(|error| error.to_string())
}

#[derive(Debug)]
pub(crate) struct ProductionControlFinalization {
    pub request: ProductionControlRequest,
    pub control_result: Option<ProjectCommandReceiptControlResult>,
}

/// Commits the pending control and its project-command receipt atomically.
/// The sidecar is cleared only after CommitService succeeds.
pub(crate) fn finalize_pending_production_control(
    manager: &ProjectManager,
    project_id: &str,
) -> Result<Option<ProductionControlFinalization>, String> {
    let guard = manager
        .lock_production_control(project_id)
        .map_err(|error| error.to_string())?;
    let Some(request) = manager
        .peek_production_control(&guard, project_id)
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let command_type = match request.action.as_str() {
        "PAUSE" => "production_pause",
        "CANCEL" => "production_cancel",
        _ => return Err("PRODUCTION_CONTROL_ACTION_INVALID".into()),
    };
    let spec = ProjectCommandSpec {
        project_id,
        result_project_id: project_id,
        command_id: &request.command_id,
        idempotency_key: &request.idempotency_key,
        command_type,
        request_digest: &request.request_digest,
        // Production advances project revision between the click and this
        // boundary; the original revision must not make a valid control stale.
        expected_revision: None,
        now: &request.requested_at,
        advance_revision: true,
    };
    let outcome = manager
        .commit_project_command_with_control_result(spec, |connection| {
            let latest =
                DurableJobService::latest_job(connection, project_id).map_err(|error| {
                    crate::project_db::ManagerError::ProjectCommand(error.to_string())
                })?;
            let active_target = request.accepted
                && latest
                    .as_ref()
                    .is_some_and(|(latest_id, job_type, status)| {
                        latest_id == &request.job_id
                            && job_type == "CHAPTER"
                            && matches!(status.as_str(), "RUNNING" | "WAITING_MODEL")
                    });
            if !active_target {
                return Ok((
                    false,
                    Some(crate::project_db::production_control_result(
                        &request.action,
                        &request.job_id,
                        false,
                    )),
                ));
            }
            let (status, run_state, reason) = if request.action == "CANCEL" {
                ("CANCELLED", "FAILED", None)
            } else {
                ("PAUSED", "PAUSED", Some("USER_PAUSE"))
            };
            DurableJobService::transition(
                connection,
                project_id,
                &request.job_id,
                status,
                &request.requested_at,
            )
            .map_err(|error| crate::project_db::ManagerError::ProjectCommand(error.to_string()))?;
            DurableJobService::release_lease(
                connection,
                project_id,
                &request.job_id,
                &request.requested_at,
            )
            .map_err(|error| crate::project_db::ManagerError::ProjectCommand(error.to_string()))?;
            DurableJobService::set_runtime_state(
                connection,
                project_id,
                run_state,
                reason,
                &request.requested_at,
            )
            .map_err(|error| crate::project_db::ManagerError::ProjectCommand(error.to_string()))?;
            Ok((
                true,
                Some(crate::project_db::production_control_result(
                    &request.action,
                    &request.job_id,
                    true,
                )),
            ))
        })
        .map_err(|error| error.to_string())?;
    let control_result = match outcome {
        ProjectCommandOutcome::Committed { receipt, .. }
        | ProjectCommandOutcome::Replayed(receipt) => receipt.control_result,
    };
    manager
        .clear_production_control(&guard, project_id, &request.idempotency_key)
        .map_err(|error| error.to_string())?;
    Ok(Some(ProductionControlFinalization {
        request,
        control_result,
    }))
}

fn control_result_for_production_start(
    command_id: &str,
    project_id: &str,
    result: &ProjectCommandReceiptControlResult,
) -> Value {
    json!({
        "commandId":command_id,
        "projectId":project_id,
        "jobId":result.job_id,
        "controlResult":result,
        "finalStatus":if result.applied { "committed" } else { "blocked" },
        "actionStatus":result.action_status,
        "control":result.control,
        "message":result.message
    })
}

/// Execute one formal B08 chapter production run. Any error after a durable
/// job has been created is reflected in that job's FAILED state so a caller
/// may safely submit a retry with explicit lineage.
pub fn run_chapter_production(
    manager: &ProjectManager,
    gateway: &dyn ProductionModelGateway,
    request: &Value,
) -> Result<Value, String> {
    if let (Some(project_id), Some(command_id)) = (
        request.pointer("/projectId").and_then(Value::as_str),
        request.pointer("/commandId").and_then(Value::as_str),
    ) {
        if let Some(finalized) = finalize_pending_production_control(manager, project_id)? {
            let requested_job_id =
                StableIdentityFactory::from_command(StableIdKind::Job, command_id)
                    .map_err(|error| error.to_string())?;
            if finalized.request.job_id == requested_job_id {
                if let Some(control_result) = finalized.control_result.as_ref() {
                    return Ok(control_result_for_production_start(
                        command_id,
                        project_id,
                        control_result,
                    ));
                }
            }
        }
    }
    let result = run_chapter_production_inner(manager, gateway, request);
    let finalization = match request.pointer("/projectId").and_then(Value::as_str) {
        Some(project_id) => match finalize_pending_production_control(manager, project_id) {
            Ok(finalization) => finalization,
            Err(_) if result.is_ok() => return result,
            Err(error) => return Err(error),
        },
        None => None,
    };
    if let Some(finalized) = finalization.as_ref() {
        let control_unwind = result
            .as_ref()
            .err()
            .is_some_and(|error| error == PRODUCTION_CONTROL_APPLIED);
        if result.is_err()
            && finalized
                .control_result
                .as_ref()
                .is_some_and(|control_result| control_result.applied || control_unwind)
        {
            let command_id = request
                .pointer("/commandId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let project_id = request
                .pointer("/projectId")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if let Some(control_result) = finalized.control_result.as_ref() {
                return Ok(control_result_for_production_start(
                    command_id,
                    project_id,
                    control_result,
                ));
            }
        }
    }
    if let Err(error) = &result {
        let project_id = request.pointer("/projectId").and_then(Value::as_str);
        let command_id = request.pointer("/commandId").and_then(Value::as_str);
        let now = request.pointer("/requestedAt").and_then(Value::as_str);
        if let (Some(project_id), Some(command_id), Some(now)) = (project_id, command_id, now) {
            if let Ok(job_id) = StableIdentityFactory::from_command(StableIdKind::Job, command_id) {
                if let Ok(connection) = manager.open(project_id, "", now) {
                    // These keep their detail: the plan problems say why
                    // the job's last plan was rejected.
                    let failure_code = if error.starts_with(
                        "B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS:predicate=",
                    ) || error
                        .starts_with(crate::craft_plan::PLAN_CRAFT_ERROR)
                    {
                        error.as_str()
                    } else {
                        error
                            .split(':')
                            .next()
                            .filter(|code| !code.trim().is_empty())
                            .unwrap_or("B08_PRODUCTION_FAILED")
                    };
                    let _ = DurableJobService::fail_active_job(
                        &connection,
                        project_id,
                        &job_id,
                        failure_code,
                        now,
                    );
                }
            }
        }
    }
    result
}

fn run_chapter_production_inner(
    manager: &ProjectManager,
    gateway: &dyn ProductionModelGateway,
    request: &Value,
) -> Result<Value, String> {
    let project_id = require(request, "/projectId")?;
    let command_id = require(request, "/commandId")?;
    let idempotency_key = require(request, "/idempotencyKey")?;
    let now = require(request, "/requestedAt")?;
    let chapter_id = require(request, "/payload/chapterId")?;
    let requested_execution_mode = request
        .pointer("/payload/executionMode")
        .and_then(Value::as_str)
        .unwrap_or("LONG_FORM");
    if !matches!(requested_execution_mode, "OPENING_BOOTSTRAP" | "LONG_FORM") {
        return Err("B08_INVALID:/payload/executionMode".into());
    }
    let worker_id = request
        .pointer("/payload/workerId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("b08-worker");
    let retry_of_job_id = request
        .pointer("/payload/retryOfJobId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned);
    let job_id = StableIdentityFactory::from_command(StableIdKind::Job, &command_id)?;
    let handle = manager
        .open(&project_id, "", &now)
        .map_err(|error| error.to_string())?;
    let connection: &libsql_rusqlite::Connection = &handle;
    if let Some(previous) = CommitService::find_receipt(connection, &project_id, &idempotency_key)
        .map_err(|error| error.to_string())?
    {
        return Ok(json!({
            "commandId":command_id,
            "projectId":project_id,
            "finalStatus":"committed",
            "message":"B08 章节生产幂等重放",
            "stage":"COMPLETED",
            "replayed":true,
            "receipt":previous
        }));
    }
    let chapter_volume = crate::repositories::narrative::NarrativeRepository::chapter_volume_id(
        connection,
        &project_id,
        &chapter_id,
    )
    .map_err(|error| error.to_string())?;
    if chapter_volume.is_none() {
        return Err("B08_CHAPTER_NOT_FOUND".into());
    }
    if crate::repositories::narrative::NarrativeRepository::chapter_status(
        connection,
        &project_id,
        &chapter_id,
    )
    .map_err(|error| error.to_string())?
    .as_deref()
        == Some("COMMITTED")
    {
        return Err("B08_CHAPTER_ALREADY_COMMITTED".into());
    }
    let volume_id = chapter_volume
        .as_deref()
        .ok_or_else(|| "B08_CHAPTER_NOT_FOUND".to_string())?;
    let execution_mode =
        crate::planning::resolve_chapter_execution_mode(connection, &project_id, &chapter_id)?;
    if requested_execution_mode != execution_mode {
        return Err("B08_EXECUTION_MODE_MISMATCH".into());
    }
    if execution_mode == "LONG_FORM" {
        let book_plan_ref =
            crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
                connection,
                &project_id,
                "BookPlan",
            )
            .map_err(|error| error.to_string())?;
        let approved_book_plan = if let Some(reference) = book_plan_ref {
            crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
                connection,
                &project_id,
                &reference,
            )
            .map_err(|error| error.to_string())?
            .and_then(|payload| {
                (payload.get("planningMaturity").and_then(Value::as_str) == Some("APPROVED"))
                    .then_some(())
            })
            .is_some()
        } else {
            false
        };
        if approved_book_plan {
            crate::signing::require_current_signing_for_long_form_if_present(
                connection,
                &project_id,
            )?;
        }
        crate::gate_policy::require_long_form_policy_supported(connection, &project_id)?;
    }
    if execution_mode == "LONG_FORM"
        && crate::repositories::narrative::NarrativeRepository::next_unsubmitted_chapter_id(
            connection,
            &project_id,
            volume_id,
        )
        .map_err(|error| error.to_string())?
        .as_deref()
            != Some(chapter_id.as_str())
    {
        return Err("B08_CHAPTER_ORDER_REQUIRED".into());
    }
    if execution_mode == "OPENING_BOOTSTRAP" {
        let next_opening_chapter =
            crate::repositories::project_entry::next_opening_chapter_id(connection, &project_id)
                .map_err(|error| error.to_string())?;
        if next_opening_chapter.as_deref() != Some(chapter_id.as_str()) {
            return Err("B08_OPENING_ORDER_REQUIRED".into());
        }
    }
    let canonical_policy_ref =
        crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
            connection,
            &project_id,
            "GatePolicy",
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "B08_GATE_POLICY_REQUIRED".to_string())?;
    let policy_ref = if let Some(policy) = request.pointer("/payload/gatePolicyRef").cloned() {
        serde_json::from_value::<ArtifactRef>(policy)
            .map_err(|error| format!("B08_GATE_POLICY_INVALID:{error}"))?
    } else {
        canonical_policy_ref.clone()
    };
    if policy_ref != canonical_policy_ref {
        return Err("B08_GATE_POLICY_STALE".into());
    }
    let gate_policy_payload = load_artifact_payload(connection, &project_id, &policy_ref)?;
    // A missing or invalid provider fails the attempt before anything runs.
    gateway.resolve_profile()?;
    if let Some(previous_job_id) = &retry_of_job_id {
        DurableJobService::validate_retry_source(
            connection,
            &project_id,
            previous_job_id,
            &chapter_id,
        )
        .map_err(|error| match error {
            RuntimeError::RetryJobNotFound => "B08_RETRY_JOB_NOT_FOUND".to_string(),
            RuntimeError::RetryJobInvalid => "B08_RETRY_JOB_INVALID".to_string(),
            other => other.to_string(),
        })?;
    }
    let production_budget_id =
        DurableModelRunner::chapter_budget_id(connection, &project_id, &chapter_id)
            .map_err(|_| "B08_PRODUCTION_BUDGET_REQUIRED".to_string())?;
    DurableJobService::start_job(
        connection,
        &JobSpec {
            job_id: job_id.clone(),
            job_type: "CHAPTER".into(),
            parent_job_id: None,
            book_id: None,
            volume_id: None,
            chapter_id: Some(chapter_id.clone()),
            stages: STAGES.iter().map(|stage| (*stage).into()).collect(),
        },
        &now,
    )
    .map_err(|error| error.to_string())?;
    if let Some(previous_job_id) = &retry_of_job_id {
        DurableJobService::record_retry_lineage(
            connection,
            &project_id,
            &job_id,
            previous_job_id,
            &now,
        )
        .map_err(|error| error.to_string())?;
    }
    DurableJobService::transition_for_worker(
        connection,
        &project_id,
        &job_id,
        "RUNNING",
        worker_id,
        &now,
    )
    .map_err(|error| error.to_string())?;
    DurableJobService::set_runtime_state(connection, &project_id, "ACTIVE", None, &now)
        .map_err(|error| error.to_string())?;

    update_stage(manager, connection, &project_id, &job_id, "CONTEXT", &now)?;
    if execution_mode == "OPENING_BOOTSTRAP" {
        crate::planning::preflight_chapter_plan_owner(connection, &project_id, &chapter_id)?;
    }
    let mut current_revision =
        crate::repositories::narrative::NarrativeRepository::project_revision(
            connection,
            &project_id,
        )
        .map_err(|error| error.to_string())?;
    let volume_id = chapter_volume.ok_or_else(|| "B08_CHAPTER_NOT_FOUND".to_string())?;
    let context_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("{command_id}:context"),
    )?;
    let candidate_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("{command_id}:candidate"),
    )?;
    let audit_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("{command_id}:audit"),
    )?;
    let final_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("{command_id}:final"),
    )?;
    let exit_id =
        StableIdentityFactory::from_command(StableIdKind::Artifact, &format!("{command_id}:exit"))?;
    let receipt_artifact_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("{command_id}:receipt"),
    )?;
    let new_audit_run_id = StableIdentityFactory::from_command(
        StableIdKind::ModelRun,
        &format!("{command_id}:audit"),
    )?;

    let reusable_opening_plan = if execution_mode == "OPENING_BOOTSTRAP" {
        retry_of_job_id
            .as_deref()
            .map(|previous_job_id| {
                reusable_opening_plan_for_retry(
                    connection,
                    request,
                    &project_id,
                    &chapter_id,
                    &volume_id,
                    previous_job_id,
                    &policy_ref,
                    &context_id,
                    &job_id,
                    &command_id,
                    &idempotency_key,
                    &now,
                    &mut current_revision,
                )
            })
            .transpose()?
            .flatten()
    } else {
        None
    };
    // FX-18: the length range this chapter is planned and written for.
    let project_settings = crate::repositories::provider::ProviderSettingsRepository::read_project(
        connection,
        &project_id,
    )
    .map_err(|error| error.to_string())?;
    // A long-form chapter's range was fixed when its plan was compiled.
    let chosen_range = if execution_mode == "LONG_FORM" {
        None
    } else {
        crate::chapter_length::ChapterLengthRange::from_payload(
            request.get("payload").unwrap_or(&Value::Null),
        )?
    };
    let length_range = crate::chapter_length::ChapterLengthRange::resolve(
        chosen_range,
        crate::repositories::provider::ProviderSettingsRepository::chapter_length_range(
            connection,
            &project_id,
            &chapter_id,
        )
        .map_err(|error| error.to_string())?
        .as_ref(),
        project_settings.get("chapterLengthRange"),
        crate::chapter_length::work_platform(connection, &project_id, &project_settings)?
            .as_deref(),
    )?;
    if let Some(range) = chosen_range {
        crate::repositories::provider::ProviderSettingsRepository::save_chapter_length_range(
            connection,
            &project_id,
            &chapter_id,
            &range.to_json(),
            &now,
        )
        .map_err(|error| error.to_string())?;
    }
    // Craft v2: what the chapters before this one already did.
    let chapter_history =
        crate::repositories::ops::chapter_history(connection, &project_id, &chapter_id, 3)
            .map_err(|error| error.to_string())?;
    let chapter_number = chapter_history.number();
    let (context_ref, context_payload, plan_ref, plan_payload, authority_context, audit_contract) =
        if let Some((context_ref, context_payload, plan_ref, plan_payload)) = reusable_opening_plan
        {
            let compiler_input = crate::commands::context_input_from_authority(
                connection,
                &authority_request(
                    request,
                    &project_id,
                    &chapter_id,
                    &volume_id,
                    execution_mode,
                ),
                &project_id,
                current_revision,
            )?;
            let authority_context = authority_context_payload(&compiler_input)?;
            let audit_contract = audit_policy_contract(&policy_ref, &gate_policy_payload)?;
            (
                context_ref,
                context_payload,
                plan_ref,
                plan_payload,
                authority_context,
                audit_contract,
            )
        } else if execution_mode == "OPENING_BOOTSTRAP" {
            let bootstrap_command_id = format!("{command_id}:bootstrap");
            let compiler_input = crate::commands::context_input_from_authority(
                connection,
                &authority_request(
                    request,
                    &project_id,
                    &chapter_id,
                    &volume_id,
                    execution_mode,
                ),
                &project_id,
                current_revision,
            )?;
            let authority_context = authority_context_payload(&compiler_input)?;
            let audit_contract = audit_policy_contract(&policy_ref, &gate_policy_payload)?;
            let manifest = crate::narrative::compile_context_manifest(compiler_input.clone())?;
            let context_payload = serde_json::to_value(&manifest).map_err(|e| e.to_string())?;
            let mut context = artifact(
                &project_id,
                context_id,
                ArtifactType::ContextManifest,
                &chapter_id,
                context_payload.clone(),
                &bootstrap_command_id,
                &job_id,
                None,
                &now,
                vec![],
            )?;
            context.provenance.created_by = "B06::ContextCompilerOwner".into();
            let context_ref = reference(&context);
            let context_command_id = format!("{bootstrap_command_id}:context");
            context.provenance.command_id = context_command_id.clone();
            let context_bundle: CommitBundle = serde_json::from_value(json!({
            "commitId":format!("commit:{context_command_id}"),"projectId":project_id,
            "commandId":context_command_id,"idempotencyKey":format!("{idempotency_key}:context"),
            "expectedProjectRevision":current_revision,"contractVersion":"m0a.v1","createdAt":now,
            "artifacts":[context],"headUpdates":[{"artifactType":"ContextManifest","scope":chapter_scope(&chapter_id),
                "artifactId":context_ref.artifact_id,"version":context_ref.version,"payloadHash":context_ref.payload_hash}]
        })).map_err(|error| format!("B08_CONTEXT_BUNDLE_INVALID:{error}"))?;
            let context_receipt = CommitService::commit(connection, &context_bundle, &now)
                .map_err(|error| error.to_string())?;
            current_revision = context_receipt.after_revision;

            update_stage(manager, connection, &project_id, &job_id, "PLAN", &now)?;
            let planner = crate::craft_plan::ChapterPlanner::for_chapter(
                connection,
                &project_id,
                &chapter_id,
                chapter_number,
            )?;
            let planning_run_id = StableIdentityFactory::from_command(
                StableIdKind::ModelRun,
                &format!("{command_id}:plan"),
            )?;
            let planning_profile = gateway.resolve_step_profile(planner.capability())?;
            let planning_spec = ModelRunSpec {
                model_run_id: planning_run_id.clone(),
                job_id: Some(job_id.clone()),
                step_id: None,
                provider: planning_profile.provider.clone(),
                model: planning_profile.model.clone(),
                idempotency_key: format!("{idempotency_key}:plan"),
                capability: planner.capability().into(),
                prompt_ref: None,
                context_manifest_ref: Some(context_ref.artifact_id.clone()),
                budget_id: Some(production_budget_id.clone()),
                predicted_cost_cents: 0,
                cost_snapshot: None,
                attempt_no: 1,
                repair_round: 0,
            };
            let horizon_capacity = crate::planning::horizon_capacity_after_chapter(
                connection,
                &project_id,
                &chapter_id,
            )?;
            let mut planning_input = json!({
                "projectId":project_id,
                "chapterId":chapter_id,
                "executionMode":execution_mode,
                "compiledFromProjectRevision":current_revision + 1,
                "compiledFromStateDigest":manifest.compiled_from_state_digest,
                "contextManifestRef":context_ref,
                "authorityContext":authority_context.clone(),
                "activeTaskRefs":compiler_input.active_task_refs,
                "continuityThreadRefs":compiler_input.continuity_threads.iter().map(|thread| thread.thread_id.clone()).collect::<Vec<_>>(),
                "horizonPreviewCapacity":horizon_capacity,
                "chapterLengthRange":length_range.to_json(),
                "chapterGoalHint":request.pointer("/payload/chapterGoalHint").and_then(Value::as_str).filter(|value| !value.trim().is_empty()).unwrap_or("推进当前章节计划，并让主角完成一次可验证的主动选择。")
            });
            crate::chapter_craft::extend_planning_input(
                &mut planning_input,
                chapter_number,
                &chapter_history.recent,
                length_range,
            )?;
            planner.extend_input(&mut planning_input);
            let triggered_threads = compiler_input
                .continuity_threads
                .iter()
                .filter(|thread| matches!(thread.authority_level.as_str(), "A" | "B"))
                .map(|thread| thread.thread_id.clone())
                .collect::<Vec<_>>();
            let hard_scope = crate::narrative::context_hard_scope(&compiler_input);
            let accept = |raw: Value, lenient: bool| -> Result<(Value, Vec<String>), String> {
                let mut problems = Vec::new();
                let mut plan = crate::planning::normalize_chapter_plan_model_output(
                    raw,
                    &triggered_threads,
                    &planning_input,
                );
                plan["activeTaskRefs"] = hard_scope["taskRefs"].clone();
                plan["entityRefs"] = hard_scope["entityRefs"].clone();
                plan["reservationRefs"] = hard_scope["reservationRefs"].clone();
                plan["hardScope"] = hard_scope.clone();
                note_if_lenient(
                    lenient,
                    &mut problems,
                    planner.check_craft(&plan, horizon_capacity),
                )?;
                if lenient {
                    problems.extend(crate::planning::fit_lenient_plan_shape(
                        &mut plan,
                        horizon_capacity,
                    ));
                }
                crate::planning::validate_chapter_plan(&plan)
                    .map_err(|e| format!("B08_PLANNING_CONTRACT:{e}"))?;
                crate::planning::validate_current_chapter_plan(
                    &plan,
                    &chapter_id,
                    &compiler_input.active_task_refs,
                    &triggered_threads,
                    horizon_capacity,
                )?;
                note_if_lenient(
                    lenient,
                    &mut problems,
                    planner.check_r1(|| {
                        if gateway.enforces_chapter_length() {
                            crate::chapter_craft::check_plan(
                                &plan,
                                length_range,
                                &chapter_history.recent,
                            )
                        } else {
                            Ok(())
                        }
                    }),
                )?;
                Ok((plan, problems))
            };
            let (plan_payload, planning_run_id, problems) = plan_chapter_with_feedback(
                connection,
                gateway,
                &planning_profile,
                &planning_spec,
                planning_input.clone(),
                &now,
                &accept,
            )?;
            let planning_command_id = format!("{bootstrap_command_id}:planning-owner");
            // The plan's reminder is committed with the plan, so a retry that
            // reuses the plan does not lose it (LD-1 step 2).
            let plan_reminder = plan_reminder_decision(
                &planning_command_id,
                &chapter_id,
                chapter_number,
                &problems,
            )?;
            let planning_receipt = crate::planning::recompile_chapter_plan_owner_with_decisions(
                connection,
                &project_id,
                &chapter_id,
                &plan_payload,
                &authority_request(
                    request,
                    &project_id,
                    &chapter_id,
                    &volume_id,
                    execution_mode,
                ),
                &planning_command_id,
                &format!("{idempotency_key}:planning-owner"),
                current_revision,
                &now,
                Some(&planning_run_id),
                &plan_reminder,
            )?;
            let plan_ref = planning_receipt
                .committed_artifact_refs
                .first()
                .cloned()
                .ok_or_else(|| "B08_PLANNING_OWNER_NO_RECEIPT".to_string())?;
            current_revision = planning_receipt.after_revision;
            (
                context_ref,
                context_payload,
                plan_ref,
                plan_payload,
                authority_context,
                audit_contract,
            )
        } else {
            let plan_ref = crate::repositories::narrative::NarrativeRepository::canonical_head_ref(
                connection,
                &project_id,
                "ChapterPlan",
                &chapter_id,
            )
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "B08_LONG_FORM_CHAPTER_PLAN_REQUIRED".to_string())?;
            let plan_payload = load_artifact_payload(connection, &project_id, &plan_ref)?;
            let context_ref: ArtifactRef = serde_json::from_value(
                plan_payload
                    .get("contextManifestRef")
                    .cloned()
                    .ok_or_else(|| "B08_PLAN_CONTEXT_REF_REQUIRED".to_string())?,
            )
            .map_err(|e| format!("B08_PLAN_CONTEXT_REF_INVALID:{e}"))?;
            let context_payload = load_artifact_payload(connection, &project_id, &context_ref)?;
            let mut chapter_authority = authority_request(
                request,
                &project_id,
                &chapter_id,
                &volume_id,
                execution_mode,
            );
            chapter_authority["chapterPlan"] = plan_payload.clone();
            let authority_input = crate::commands::context_input_from_authority(
                connection,
                &chapter_authority,
                &project_id,
                current_revision,
            )?;
            let authority_context = authority_context_payload(&authority_input)?;
            let audit_contract = audit_policy_contract(&policy_ref, &gate_policy_payload)?;
            (
                context_ref,
                context_payload,
                plan_ref,
                plan_payload,
                authority_context,
                audit_contract,
            )
        };
    if gateway.enforces_chapter_length() && !crate::chapter_craft::is_craft_plan(&plan_payload) {
        // A plan sized off the range was reported when it was made (LD-1
        // step 2) and is written anyway; the body's length is still checked.
        if let Err(error) = crate::chapter_length::validate_plan_length(&plan_payload, length_range)
        {
            if crate::chapter_craft::plan_feedback(&error).is_none() {
                return Err(error);
            }
        }
    }
    if let Err(error) = validate_plan_is_current(
        connection,
        request,
        &project_id,
        &chapter_id,
        &volume_id,
        execution_mode,
        &plan_ref,
        &plan_payload,
    ) {
        if error.starts_with("PLAN_STALE") {
            block_for_recompile(connection, &project_id, &job_id, &now)?;
        }
        return Err(error);
    }
    let state_digest = current_authority_digest(
        connection,
        request,
        &project_id,
        (&chapter_id, &volume_id),
        execution_mode,
        &plan_payload,
        current_revision,
    )?;
    // Owner decision (2026-10-24): the v1.2 enrichment, its quality report
    // and S40 candidate repair are retired from chapter production; the v2
    // craft skills and the rule checks guide the writing instead.

    update_stage(manager, connection, &project_id, &job_id, "RENDER", &now)?;
    let mut render_input = json!({"chapterId":chapter_id,"chapterPlanRef":plan_ref,"chapterPlan":plan_payload,"contextManifestRef":context_ref,"contextManifest":context_payload,"authorityContext":authority_context.clone(),"chapterLengthRange":length_range.to_json()});
    let avoid_phrases = crate::chapter_craft::repeated_phrases(&chapter_history.bodies);
    if !avoid_phrases.is_empty() {
        render_input["avoidPhrases"] = json!(avoid_phrases);
    }
    let avoid_tics = crate::chapter_craft::overused_tics(&chapter_history.bodies);
    if !avoid_tics.is_empty() {
        render_input["avoidTics"] = json!(avoid_tics);
    }
    let mut craft_signals = crate::chapter_craft::plan_signals(&plan_payload, chapter_number);
    render_input["craftGuidance"] = crate::chapter_craft::guidance("render", &craft_signals)?;
    // The skills each step received, for the receipt. Chapter planning
    // selects from the chapter position and the previous chapter, so the
    // selection is reproducible for plans compiled by an earlier command.
    use crate::chapter_craft::guidance_skill_ids as skill_ids;
    let mut craft_skills = json!({
        "skillsVersion":crate::skills_v2::SKILLS_V2_VERSION,
        "chapterPlan":skill_ids(&crate::chapter_craft::guidance(
            "chapter_plan",
            &crate::chapter_craft::planning_signals(chapter_number, &chapter_history.recent),
        )?),
        "render":skill_ids(&render_input["craftGuidance"])
    });
    if crate::chapter_craft::is_craft_plan(&plan_payload) {
        let planned = craft_skills["chapterPlan"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        craft_skills["chapterPlanDecisions"] =
            crate::craft_plan::skill_decisions(&plan_payload, &planned);
    }
    // CRAFT v3: an r2 chapter is written whole from its craft decisions with
    // the story bible and the style card (CRAFT_V3_DESIGN §4.4).
    let craft_writing = crate::chapter_craft::is_craft_plan(&plan_payload)
        .then(|| {
            crate::craft_plan::WritingContext::for_chapter(
                connection,
                &project_id,
                &chapter_id,
                chapter_number,
            )
        })
        .transpose()?;
    if let Some(writing) = &craft_writing {
        writing.extend_render_input(&mut render_input, &chapter_history.recent);
        // QC-1.4: catchphrases the continuity check named in recent chapters.
        if !chapter_history.catchphrases.is_empty() {
            let mut tics = render_input["avoidTics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            tics.extend(
                chapter_history
                    .catchphrases
                    .iter()
                    .map(|phrase| json!(phrase)),
            );
            render_input["avoidTics"] = json!(tics);
        }
    }
    // QC-1.5: what earlier chapters fixed, for the continuity check.
    let established_facts = render_input
        .get("establishedFacts")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let render_capability = if craft_writing.is_some() {
        "RENDER_CHAPTER_CRAFT"
    } else {
        "RENDER_CHAPTER"
    };
    let render_profile = gateway.resolve_step_profile("RENDER_CHAPTER")?;
    // One model call of a craft step; `suffix` names the call within this
    // job. Returns the run id and its outcome.
    let craft_call = |suffix: &str,
                      capability: &str,
                      profile: &ResolvedProviderProfile,
                      attempt_no: i64,
                      input: Value|
     -> Result<(String, ModelRunOutcome), String> {
        let model_run_id = StableIdentityFactory::from_command(
            StableIdKind::ModelRun,
            &format!("{command_id}:{suffix}"),
        )?;
        let spec = ModelRunSpec {
            model_run_id: model_run_id.clone(),
            job_id: Some(job_id.clone()),
            step_id: None,
            provider: profile.provider.clone(),
            model: profile.model.clone(),
            idempotency_key: format!("{idempotency_key}:{suffix}"),
            capability: capability.into(),
            prompt_ref: None,
            context_manifest_ref: Some(context_ref.artifact_id.clone()),
            budget_id: Some(production_budget_id.clone()),
            predicted_cost_cents: 0,
            cost_snapshot: None,
            attempt_no,
            repair_round: 0,
        };
        let outcome = execute_production_model_with_profile(
            connection, gateway, profile, &spec, capability, input, &now,
        )?;
        Ok((model_run_id, outcome))
    };
    use crate::chapter_length::character_count;
    let length_error = |body: &str| {
        format!(
            "B08_RENDER_LENGTH_OUT_OF_RANGE:{}:{}-{}",
            character_count(body),
            length_range.min,
            length_range.max
        )
    };
    // CRAFT v3 (owner decision 2026-10-23): a chapter is written in one
    // call. A retry reuses the earlier prose only where it is committed as
    // written; where the prose is sized and rewritten afterwards, a retry
    // writes it again so the audit always reads what is committed.
    let reusable_render = retry_of_job_id
        .as_deref()
        .filter(|_| !gateway.enforces_chapter_length())
        .map(|previous_job_id| {
            DurableModelRunner::completed_result_for_job(
                connection,
                &project_id,
                previous_job_id,
                render_capability,
            )
        })
        .transpose()
        .map_err(|error| error.to_string())?
        .flatten();
    // An earlier audit only fits a retry that reuses that attempt's prose.
    let render_reused = reusable_render.is_some();
    let (render_run_id, mut body) = if let Some((model_run_id, parsed)) = reusable_render {
        let payload = parse_provider_model_payload_for_capability(parsed, render_capability)?;
        let body = payload
            .get("body")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| "B08_RENDER_OUTPUT_INVALID".to_string())?;
        (model_run_id, body)
    } else {
        let (model_run_id, result) = craft_call(
            "render",
            render_capability,
            &render_profile,
            1,
            render_input,
        )?;
        let body = result
            .parsed
            .as_ref()
            .and_then(|payload| payload.get("body"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| "B08_RENDER_OUTPUT_INVALID".to_string())?;
        (model_run_id, body)
    };
    // Craft v2 (owner decision 2026-10-19): the written chapter is read once
    // with the review skills, and passages it flags are rewritten in place
    // with the rewrite skills. Only edits whose passage occurs once apply,
    // and a rewrite that leaves the length range is dropped whole.
    let mut prose_run_ids = Vec::new();
    // A chapter outside the length range gets one expand or compress call;
    // it is never committed outside the range.
    if gateway.enforces_chapter_length() {
        let in_range = |text: &str| length_range.accepts(character_count(text));
        body = crate::chapter_craft::normalize_typography(body.trim());
        if !in_range(&body) {
            let length = character_count(&body);
            let (run_id, outcome) = craft_call(
                "adjust-length",
                "ADJUST_CHAPTER_LENGTH",
                &render_profile,
                2,
                json!({
                    "chapterId":chapter_id,"chapterPlan":plan_payload,"body":body,
                    "direction":if length < length_range.min { "EXPAND" } else { "COMPRESS" },
                    "currentChars":length,
                    "targetChars":length_range.target(),
                    "chapterLengthRange":length_range.to_json()
                }),
            )?;
            prose_run_ids.push(run_id);
            body = outcome
                .parsed
                .as_ref()
                .and_then(|payload| payload.get("body"))
                .and_then(Value::as_str)
                .map(|text| crate::chapter_craft::normalize_typography(text.trim()))
                .ok_or_else(|| "B08_ADJUST_CHAPTER_LENGTH_OUTPUT_INVALID".to_string())?;
        }
        if !in_range(&body) {
            return Err(length_error(&body));
        }
    }
    if gateway.reviews_chapter_prose() {
        let mut craft_step = |suffix: &str,
                              capability: &str,
                              profile: &ResolvedProviderProfile,
                              input: Value|
         -> Result<Value, String> {
            let (run_id, outcome) = craft_call(suffix, capability, profile, 1, input)?;
            prose_run_ids.push(run_id);
            outcome
                .parsed
                .ok_or_else(|| format!("B08_{capability}_OUTPUT_INVALID"))
        };
        let codes = crate::skills_v2::finding_vocabulary()?;
        // CRAFT v3 chapters are checked by rule only (CRAFT_V3_DESIGN §4.4);
        // model editing belongs to the prose optimizer.
        let review = if craft_writing.is_some() {
            json!({})
        } else {
            let review_guidance = crate::chapter_craft::guidance("review", &craft_signals)?;
            craft_skills["review"] = json!(skill_ids(&review_guidance));
            craft_step(
                "review",
                "REVIEW_CHAPTER",
                &gateway.resolve_step_profile("REVIEW_CHAPTER")?,
                json!({"chapterId":chapter_id,"chapterGoal":plan_payload.get("chapterGoal"),"sceneExecution":plan_payload.get("sceneExecution"),"body":body,"findingCodes":codes,"craftGuidance":review_guidance}),
            )?
        };
        let findings = review
            .get("findings")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|finding| {
                finding
                    .get("code")
                    .and_then(Value::as_str)
                    .is_some_and(|code| codes.iter().any(|known| known == code))
                    && matches!(
                        finding.get("severity").and_then(Value::as_str),
                        Some("HIGH" | "MEDIUM" | "LOW")
                    )
                    && finding
                        .get("quote")
                        .and_then(Value::as_str)
                        .is_some_and(|quote| locate_unique_quote(&body, quote).is_some())
            })
            .take(6)
            .collect::<Vec<_>>();
        // Rule findings (plan wording, measures, similes, tics, panels)
        // join the review's, one per sentence.
        let mut rules = match &craft_writing {
            Some(writing) => crate::chapter_craft::craft_rule_findings(
                &plan_payload,
                &body,
                &writing.bible,
                writing.style_card.as_ref(),
            ),
            None => crate::chapter_craft::rule_findings(&plan_payload, &body),
        };
        // Phrases this chapter uses a third time across the recent chapters
        // (signing check F1-5) go to the rewrite as well: each sentence using
        // one is a finding like the others, so the rewrite has to answer it.
        // They lead every finding: the rewrite applies at most eight edits
        // (real run 2026-10-25).
        let repeated = crate::chapter_craft::overused_phrases(&chapter_history.bodies, &body);
        // QC-1.4/1.5 (owner decision 2026-10-26): the continuity check names
        // the sentences contradicting earlier chapters and which repeated
        // short phrases are catchphrases; both lead the findings too.
        let check_continuity = |craft_step: &mut CraftStep,
                                suffix: &str,
                                text: &str,
                                candidates: &[Value]|
         -> Result<Option<Value>, String> {
            let Some(writing) = &craft_writing else {
                return Ok(None);
            };
            let (present, _) = writing.chapter_people(&plan_payload);
            let ledger_facts =
                crate::repositories::ops::subject_facts(connection, &project_id, &present, 40)
                    .map_err(|error| error.to_string())?;
            craft_step(
                suffix,
                "CHECK_CONTINUITY",
                &gateway.resolve_step_profile("CHECK_CONTINUITY")?,
                json!({
                    "chapterNumber":chapter_number,
                    "bodySentences":body_sentences(text).into_iter().map(|(_, _, sentence)| sentence).collect::<Vec<_>>(),
                    "establishedFacts":established_facts,
                    "ledgerFacts":ledger_facts,
                    "phraseCandidates":candidates
                }),
            )
            .map(Some)
        };
        let candidates = match &craft_writing {
            Some(writing) => {
                let (_, names) = writing.chapter_people(&plan_payload);
                let names = names.iter().map(String::as_str).collect::<Vec<_>>();
                crate::chapter_craft::catchphrase_candidates(&chapter_history.bodies, &body, &names)
            }
            None => Vec::new(),
        };
        let checked = check_continuity(&mut craft_step, "continuity", &body, &candidates)?;
        let contradictions = checked
            .as_ref()
            .and_then(|checked| checked["contradictions"].as_array().cloned())
            .unwrap_or_default();
        let catchphrases = checked
            .as_ref()
            .and_then(|checked| checked["catchphrases"].as_array())
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|phrase| {
                candidates
                    .iter()
                    .any(|candidate| candidate["phrase"] == *phrase)
            })
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        // A sentence the check flagged is fixed for that reason first; a
        // repeat already flagged by a rule keeps the rule's finding.
        let mut leading_findings =
            crate::chapter_craft::contradiction_findings(&contradictions, &body);
        leading_findings.extend(crate::chapter_craft::catchphrase_findings(
            &catchphrases,
            &body,
        ));
        rules.retain(|rule| {
            !leading_findings
                .iter()
                .any(|finding| finding["quote"] == rule["quote"])
        });
        for finding in crate::chapter_craft::repetition_findings(&repeated, &body) {
            if !leading_findings
                .iter()
                .chain(rules.iter())
                .any(|other| other["quote"] == finding["quote"])
            {
                leading_findings.push(finding);
            }
        }
        let leading = leading_findings.len();
        rules.splice(0..0, leading_findings);
        let to_fix = rules[..leading]
            .iter()
            .chain(
                findings
                    .iter()
                    .copied()
                    .filter(|finding| finding["severity"] != "LOW"),
            )
            .chain(rules[leading..].iter().filter(|rule| {
                !findings.iter().any(|finding| {
                    finding["quote"].as_str().is_some_and(|quote| {
                        rule["quote"]
                            .as_str()
                            .is_some_and(|sentence| sentence.contains(quote))
                    })
                })
            }))
            .collect::<Vec<_>>();
        let mut applied = 0;
        let mut in_range = true;
        let mut repeats_grew = false;
        if !to_fix.is_empty() || !repeated.is_empty() {
            craft_signals.findings = to_fix
                .iter()
                .filter_map(|finding| finding["code"].as_str().map(ToOwned::to_owned))
                .chain(
                    (!repeated.is_empty()
                        && !to_fix.iter().any(|f| f["code"] == "word_repetition"))
                    .then(|| "word_repetition".to_string()),
                )
                .collect();
            let rewrite_guidance = crate::chapter_craft::guidance("rewrite", &craft_signals)?;
            craft_skills["rewrite"] = json!(skill_ids(&rewrite_guidance));
            let rewrite_profile = gateway.resolve_step_profile("REWRITE_PASSAGES")?;
            let rewrite = craft_step(
                "rewrite",
                "REWRITE_PASSAGES",
                &rewrite_profile,
                json!({"chapterId":chapter_id,"body":body,"findings":to_fix,"repeatedPhrases":repeated,"chapterLengthRange":length_range.to_json(),"craftGuidance":rewrite_guidance}),
            )?;
            let (revised, count) = apply_unique_edits(&body, &rewrite);
            let revised = crate::chapter_craft::normalize_typography(revised.trim());
            let length = character_count(&revised);
            in_range = (length_range.min..=length_range.ceiling()).contains(&length);
            repeats_grew =
                crate::chapter_craft::overused_phrases(&chapter_history.bodies, &revised).len()
                    > repeated.len();
            applied = count;
            if applied > 0 && in_range && !repeats_grew {
                body = revised;
            }
        }
        let discarded = if applied == 0 {
            Value::Null
        } else if !in_range {
            json!("LENGTH_OUT_OF_RANGE")
        } else if repeats_grew {
            json!("MORE_REPEATED_PHRASES")
        } else {
            Value::Null
        };
        // What the rewrite left: contradictions the check still finds, and
        // catchphrases used more than once, wait in the chapter's reminder.
        let mut unresolved = catchphrases
            .iter()
            .filter(|phrase| body.matches(phrase.as_str()).count() > 1)
            .map(|phrase| json!({"field":"continuity","item":phrase,"reason":"QC1_CATCHPHRASE"}))
            .collect::<Vec<_>>();
        if !contradictions.is_empty() {
            let rechecked = check_continuity(&mut craft_step, "continuity-recheck", &body, &[])?;
            unresolved.extend(
                rechecked
                    .as_ref()
                    .and_then(|checked| checked["contradictions"].as_array())
                    .into_iter()
                    .flatten()
                    .map(|item| json!({"field":"continuity","item":item,"reason":"QC1_CONTRADICTION"})),
            );
        }
        craft_skills["continuity"] = json!({
            "contradictions":contradictions,
            "catchphrases":catchphrases,
            "unresolved":unresolved
        });
        craft_skills["proseReview"] = json!({
            "findings":findings.len(),
            "codes":findings.iter().map(|finding| finding["code"].clone()).collect::<Vec<_>>(),
            "repeatedPhrases":repeated.len(),
            "ruleCodes":rules.iter().map(|rule| rule["code"].clone()).collect::<Vec<_>>(),
            "editsApplied":if discarded.is_null() { applied } else { 0 },
            "discarded":discarded
        });
        if let Some(writing) = &craft_writing {
            craft_skills["proseReview"]["rhythmNotes"] = json!(writing
                .style_card
                .as_ref()
                .map(|card| crate::chapter_craft::rhythm_notes(&body, card))
                .unwrap_or_default());
        }
    }
    let body_digest = sha256_canonical(&json!(body)).map_err(|e| e.to_string())?;
    validate_candidate_evidence_range(&body, 0, body.chars().count())?;
    update_stage(manager, connection, &project_id, &job_id, "CANDIDATE", &now)?;
    let candidate_payload = json!({
        "payloadSchema":"chapter-candidate.r1","chapterId":chapter_id,
        "chapterPlanRef":plan_ref.clone(),"contextManifestRef":context_ref.clone(),
        "body":body.clone(),"bodyDigest":body_digest,"evidenceRanges":[],
        "renderer":{"modelRunId":render_run_id,"capability":render_capability,"provider":render_profile.provider.clone(),"model":render_profile.model.clone()},
        "creativeEnrichment":Value::Null
    });
    let mut candidate = artifact(
        &project_id,
        candidate_id.clone(),
        ArtifactType::ChapterCandidate,
        &chapter_id,
        candidate_payload,
        &command_id,
        &job_id,
        Some(&render_run_id),
        &now,
        vec![plan_ref.clone(), context_ref.clone()],
    )?;
    candidate.provenance.context_manifest_ref = Some(context_ref.artifact_id.clone());

    let candidate_ref = reference(&candidate);
    update_stage(manager, connection, &project_id, &job_id, "AUDIT", &now)?;
    let mut audit_input = json!({"chapterId":chapter_id,"chapterPlanRef":plan_ref,"chapterPlan":audit_plan_view(&plan_payload),"contextManifestRef":context_ref,"authorityContext":authority_context.clone(),"auditPolicyContract":audit_contract.clone(),"candidateRef":candidate_ref.clone(),"body":body,"qualityReportRef":Value::Null});
    // QC-1.1: evidence is cited by the number of its sentence.
    audit_input["bodySentences"] = json!(body_sentences(&body)
        .into_iter()
        .map(|(_, _, text)| text)
        .collect::<Vec<_>>());
    // CRAFT v3 (CRAFT_V3_DESIGN §4.3): the same audit call also records the
    // habits, looks and objects later chapters must keep consistent.
    if craft_writing.is_some() {
        audit_input["continuityDetailsWanted"] = json!(true);
    }
    let reusable_audit = if !render_reused {
        None
    } else {
        retry_of_job_id
            .as_deref()
            .map(|previous_job_id| {
                DurableModelRunner::completed_result_for_job(
                    connection,
                    &project_id,
                    previous_job_id,
                    "AUDIT_EXTRACT",
                )
            })
            .transpose()
            .map_err(|error| error.to_string())?
            .flatten()
    };
    let audit_reused = reusable_audit.is_some();
    let audit_profile = gateway.resolve_step_profile("AUDIT_EXTRACT")?;
    let (mut audit_run_id, raw_audit) = if let Some((model_run_id, parsed)) = reusable_audit {
        (
            model_run_id,
            parse_provider_model_payload_for_capability(parsed, "AUDIT_EXTRACT")?,
        )
    } else {
        let audit_spec = ModelRunSpec {
            model_run_id: new_audit_run_id.clone(),
            job_id: Some(job_id.clone()),
            step_id: None,
            provider: audit_profile.provider.clone(),
            model: audit_profile.model.clone(),
            idempotency_key: format!("{idempotency_key}:audit"),
            capability: "AUDIT_EXTRACT".into(),
            prompt_ref: None,
            context_manifest_ref: Some(context_ref.artifact_id.clone()),
            budget_id: Some(production_budget_id.clone()),
            predicted_cost_cents: 0,
            cost_snapshot: None,
            attempt_no: 1,
            repair_round: 0,
        };
        let result = execute_production_model_with_profile(
            connection,
            gateway,
            &audit_profile,
            &audit_spec,
            "AUDIT_EXTRACT",
            audit_input.clone(),
            &now,
        )?;
        (
            new_audit_run_id,
            result
                .parsed
                .ok_or_else(|| "B08_AUDIT_OUTPUT_INVALID".to_string())?,
        )
    };
    // Threads the audit may name: those it was shown and those the plan names.
    let known_threads = authority_context
        .get("continuityThreads")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|thread| thread.get("threadId").and_then(Value::as_str))
        .chain(planned_continuity_refs(&plan_payload))
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    let prepare_audit = |mut raw: Value| -> Result<Value, String> {
        let assigned = assign_audit_ids(&mut raw, &chapter_id, &known_threads);
        let mut audit_payload = normalize_audit_model_output(raw, &body);
        audit_payload["normalization"]["assignedIds"] = assigned;
        cap_new_threads(&mut audit_payload, &known_threads);
        let unplanned_task_ids = unplanned_task_candidate_ids(&audit_payload, &plan_payload);
        let known_task_ids = crate::repositories::narrative::NarrativeRepository::known_task_ids(
            connection,
            &project_id,
            &unplanned_task_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        )
        .map_err(|error| error.to_string())?;
        drop_invented_task_candidates(&mut audit_payload, &unplanned_task_ids, &known_task_ids);
        // A reveal of a reservation neither this plan nor the BookPlan names
        // is an audit invention: it is dropped and recorded. A real
        // reservation revealed outside the plan still fails the chapter.
        let unplanned_reveals = audit_payload
            .get("revealedReservationRefs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|reference| {
                !plan_payload
                    .get("reservationRefs")
                    .and_then(Value::as_array)
                    .is_some_and(|refs| refs.iter().any(|item| item == reference))
            })
            .collect::<Vec<_>>();
        let known_reservations =
            crate::repositories::narrative::NarrativeRepository::known_reservation_refs(
                connection,
                &project_id,
                &unplanned_reveals,
            )
            .map_err(|error| error.to_string())?;
        let invented_reveals = unplanned_reveals
            .into_iter()
            .filter(|reference| !known_reservations.contains(*reference))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if let Some(reveals) = audit_payload
            .get_mut("revealedReservationRefs")
            .and_then(Value::as_array_mut)
        {
            reveals.retain(|item| {
                !item
                    .as_str()
                    .is_some_and(|r| invented_reveals.iter().any(|i| i == r))
            });
        }
        if !invented_reveals.is_empty() {
            audit_payload["normalization"]["droppedInventedReservations"] = json!(invented_reveals);
        }
        Ok(audit_payload)
    };
    let validate_audit = |audit_payload: &Value| -> Result<AuditExtraction, String> {
        let mut extraction: AuditExtraction = serde_json::from_value(audit_payload.clone())
            .map_err(|error| format!("B08_AUDIT_INCOMPLETE:{error}"))?;
        validate_audit_completeness(audit_payload, &extraction)?;
        validate_actual_extraction(
            connection,
            &project_id,
            &chapter_id,
            &body,
            &plan_payload,
            &gate_policy_payload,
            &mut extraction,
        )?;
        Ok(extraction)
    };
    let check_audit = |raw: Value| -> Result<(Value, AuditExtraction), String> {
        let audit_payload = prepare_audit(raw)?;
        let extraction = validate_audit(&audit_payload)?;
        Ok((audit_payload, extraction))
    };
    // Craft rule (2026-10-18): an audit whose evidence cannot be located in
    // the body (for example quoting the plan) is run once more with the
    // unlocatable quotes; a second failure still blocks the chapter. Owner
    // decision (2026-10-19): so is an audit using an unregistered predicate,
    // with the registered ones; the policy itself stays fail-closed.
    let mut last_raw = raw_audit.clone();
    let checked = match check_audit(last_raw.clone()) {
        Err(error)
            if !audit_reused
                && (matches!(
                    error.as_str(),
                    "B08_AUDIT_INCOMPLETE_EVIDENCE_REBOUND"
                        | "B08_AUDIT_INCOMPLETE_NORMALIZATION_DROPPED_CONTENT"
                        | "B08_AUDIT_INCOMPLETE_KNOWLEDGE_TIME"
                ) || error
                    .starts_with("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS")) =>
        {
            let rerun_id = StableIdentityFactory::from_command(
                StableIdKind::ModelRun,
                &format!("{command_id}:audit-feedback"),
            )?;
            let mut input = audit_input;
            input["auditFeedback"] = json!({
                "rejectedFor":error,
                "unlocatableQuotes":unlocatable_audit_quotes(&raw_audit, &body),
                "unboundCitations":normalize_audit_model_output(raw_audit.clone(), &body)["normalization"]["unboundCitations"].take(),
                "incompleteCandidates":incomplete_audit_candidates(&raw_audit),
                "instruction":"上次提取不完整：unboundCitations 中的条目指认的句子（sentence）或引文和条目内容对不上，unlocatableQuotes 中的引文在正文中找不到，incompleteCandidates 中的条目缺少必填字段或 evidence。请重新完整提取：每个条目都要写全必填字段，evidence.sentence 填写 bodySentences 中真正写到这件事的那一句的序号。"
            });
            if error.starts_with("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS") {
                input["auditFeedback"]["registeredPredicates"] = json!(
                    crate::canon_semantics::CanonSemanticPolicy::from_gate_policy(
                        &gate_policy_payload
                    )
                    .map_err(str::to_owned)?
                    .predicates()
                );
                input["auditFeedback"]["instruction"] = json!("上次提取的 canonCandidates 用了未登记的 predicate（见 rejectedFor）。请重新完整提取：每条事实的 predicate 只能取 registeredPredicates 中的一个，表达不了的变化不要写成事实。");
            }
            let rerun_spec = ModelRunSpec {
                model_run_id: rerun_id.clone(),
                job_id: Some(job_id.clone()),
                step_id: None,
                provider: audit_profile.provider.clone(),
                model: audit_profile.model.clone(),
                idempotency_key: format!("{idempotency_key}:audit-feedback"),
                capability: "AUDIT_EXTRACT".into(),
                prompt_ref: None,
                context_manifest_ref: Some(context_ref.artifact_id.clone()),
                budget_id: Some(production_budget_id.clone()),
                predicted_cost_cents: 0,
                cost_snapshot: None,
                attempt_no: 2,
                repair_round: 0,
            };
            let result = execute_production_model_with_profile(
                connection,
                gateway,
                &audit_profile,
                &rerun_spec,
                "AUDIT_EXTRACT",
                input,
                &now,
            )?;
            audit_run_id = rerun_id;
            let rerun_raw = result
                .parsed
                .ok_or_else(|| "B08_AUDIT_OUTPUT_INVALID".to_string())?;
            last_raw = rerun_raw.clone();
            match check_audit(rerun_raw.clone()) {
                Err(error) if error == "B08_AUDIT_INCOMPLETE_EVIDENCE_REBOUND" => {
                    // Ids first, so what is kept still names the facts it named.
                    let mut assigned = rerun_raw.clone();
                    assign_audit_ids(&mut assigned, &chapter_id, &known_threads);
                    let (kept, dropped) = drop_unlocatable_audit_candidates(&assigned, &body);
                    // Real run 2026-10-24: every fact was dropped and the
                    // chapter committed with no canon. A chapter is not
                    // committed without the facts its audit reported.
                    let has_canon = |value: &Value| {
                        value["canonCandidates"]
                            .as_array()
                            .is_some_and(|items| !items.is_empty())
                    };
                    if has_canon(&rerun_raw) && !has_canon(&kept) {
                        Err(error)
                    } else {
                        check_audit(kept).map(|(mut payload, extraction)| {
                            payload["normalization"]["droppedUnlocatable"] = dropped;
                            (payload, extraction)
                        })
                    }
                }
                other => other,
            }
        }
        other => other,
    };
    // LD-1 step 2: what the ledger cannot take is held back and reported; the
    // chapter commits with the rest.
    let (mut audit_payload, extraction) = match checked {
        Ok(checked) => checked,
        Err(error) if error.starts_with("B08_") => {
            hold_back_audit_items(prepare_audit(last_raw)?, &validate_audit)?
        }
        Err(other) => return Err(other),
    };
    if !extraction.dropped_unbound_knowledge.is_empty() {
        audit_payload["normalization"]["droppedUnboundKnowledge"] =
            json!(extraction.dropped_unbound_knowledge);
    }
    if !extraction.downgraded_task_claims.is_empty() {
        audit_payload["normalization"]["downgradedTaskClaims"] =
            json!(extraction.downgraded_task_claims);
    }
    let planned_continuity_refs = planned_continuity_refs(&plan_payload);
    let thread_promotion_decisions = extraction
        .continuity_candidates
        .iter()
        .map(|candidate| {
            let signal = candidate.promotion_signals;
            let authority_level = crate::narrative::classify_thread_responsibility(
                signal,
                candidate
                    .evidence
                    .as_ref()
                    .is_some_and(|evidence| !evidence.rebound),
                candidate.description.as_deref(),
                candidate.fulfillment_object.as_deref(),
                candidate.resume_condition.as_ref(),
            );
            let authority_level = match authority_level {
                ThreadResponsibilityLevel::B => "B",
                ThreadResponsibilityLevel::C => "C",
            };
            json!({
                "threadRef": candidate.thread_ref,
                "ruleVersion": crate::narrative::THREAD_PROMOTION_RULE_VERSION,
                "decision": if authority_level == "B" {"PROMOTE"} else {"RETAIN_C"},
                "authorityLevel": authority_level,
                "newResponsibility": !planned_continuity_refs.contains(candidate.thread_ref.as_str()),
                "description": candidate.description,
                "fulfillmentObject": candidate.fulfillment_object,
                "resumeCondition": candidate.resume_condition,
                "promotionSignals": signal,
                "evidence": candidate.evidence.as_ref().map(|evidence| json!({"start":evidence.start,"end":evidence.end,"quote":evidence.quote}))
            })
        })
        .collect::<Vec<_>>();
    let promotion_decision_for = |thread_ref: &str| {
        thread_promotion_decisions
            .iter()
            .find(|item| item.get("threadRef").and_then(Value::as_str) == Some(thread_ref))
            .and_then(|item| item.get("decision").and_then(Value::as_str))
            .unwrap_or("RETAIN_C")
    };
    let promotion_authority_for = |thread_ref: &str| {
        thread_promotion_decisions
            .iter()
            .find(|item| item.get("threadRef").and_then(Value::as_str) == Some(thread_ref))
            .and_then(|item| item.get("authorityLevel").and_then(Value::as_str))
            .unwrap_or("C")
    };
    let review_payload = json!({"payloadSchema":"actual-narrative-audit.r1","candidateRef":candidate_ref.clone(),"gatePolicyRef":policy_ref.clone(),"canonSemanticPolicyVersion":gate_policy_payload.pointer("/canonSemantics/policyVersion").and_then(Value::as_str),"threadPromotionRuleVersion":crate::narrative::THREAD_PROMOTION_RULE_VERSION,"audit":audit_payload,"threadPromotionDecisions":thread_promotion_decisions.clone()});
    let mut audit = artifact(
        &project_id,
        audit_id,
        ArtifactType::ReviewReport,
        &chapter_id,
        review_payload,
        &command_id,
        &job_id,
        Some(&audit_run_id),
        &now,
        vec![candidate_ref.clone()],
    )?;
    audit.provenance.context_manifest_ref = Some(context_ref.artifact_id.clone());
    validate_audit_candidate_binding(&audit.payload, &candidate_ref)?;
    update_stage(manager, connection, &project_id, &job_id, "GATE", &now)?;
    if let Err(error) = validate_plan_is_current(
        connection,
        request,
        &project_id,
        &chapter_id,
        &volume_id,
        execution_mode,
        &plan_ref,
        &plan_payload,
    ) {
        if error.starts_with("PLAN_STALE") {
            block_for_recompile(connection, &project_id, &job_id, &now)?;
        }
        return Err(error);
    }
    let mut obligation_delta = Vec::new();
    let mut foreshadow_delta = Vec::new();
    let mut decision_updates = Vec::new();
    let held = audit_payload
        .pointer("/normalization/heldBack")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            craft_skills
                .pointer("/continuity/unresolved")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
        .cloned()
        .collect::<Vec<_>>();
    if !held.is_empty() {
        decision_updates.push(audit_reminder(
            &final_id,
            &chapter_id,
            chapter_number,
            &held,
        )?);
    }
    let mut change_request_artifacts = Vec::new();
    let mut change_request_heads = Vec::new();
    let mut obligation_evidence_records = Vec::new();
    let mut new_responsibility_evidence = Vec::new();
    let mut effective_continuity_refs = BTreeSet::new();
    let change_request_context = B08ChangeRequestContext {
        project_id: &project_id,
        command_id: &command_id,
        job_id: &job_id,
        requested_at: &now,
        current_revision,
        final_id: &final_id,
        chapter_id: &chapter_id,
        audit: &audit,
        candidate: &candidate,
    };
    for (index, item) in extraction.continuity_candidates.iter().enumerate() {
        let promotion_decision = promotion_decision_for(&item.thread_ref);
        let authority_level = promotion_authority_for(&item.thread_ref);
        let authority_id = item
            .thread_ref
            .strip_prefix("thread:")
            .unwrap_or(&item.thread_ref);
        let obligation = crate::repositories::narrative::NarrativeRepository::obligation_authority(
            connection,
            &project_id,
            authority_id,
        )
        .map_err(|error| error.to_string())?;
        if let Some(obligation) = obligation {
            let lifecycle = match item.disposition.as_str() {
                "RESOLVE" | "RESOLVED" => "RESOLVED",
                "ADVANCE" | "PROGRESSING" | "DEFER_WITH_BRIDGE" => "PROGRESSING",
                "NO_LONGER_APPLICABLE_CHANGE_REQUEST" => {
                    let (change_artifact, head, decision) = create_b08_change_request_proposal(
                        &change_request_context,
                        index,
                        item,
                        "审计发现既有连续性责任可能不再适用，等待用户确认。",
                        "正文报告既有责任可能不再适用；责任仍保持有效，等待正式变更确认。",
                    )?;
                    decision_updates.push(decision);
                    change_request_heads.push(head);
                    change_request_artifacts.push(change_artifact);
                    continue;
                }
                _ => return Err("B08_THREAD_DISPOSITION_INVALID".into()),
            };
            let mut obligation_payload = json!({
                "sourceArtifactId":final_id,
                "sourceVersion":1,
                "resumeCondition":obligation.resume_condition,
                "promotionDecision":promotion_decision,
                "deltaMode":"EXISTING_AUTHORITY_UPDATE"
            });
            if let Some(source_event_id) = &item.source_event_id {
                obligation_payload["sourceEventId"] = json!(source_event_id);
            }
            obligation_delta.push(json!({
                "obligationId":authority_id,"scope":obligation.scope,"obligationType":obligation.obligation_type,
                "authorityLevel":obligation.authority_level,"lifecycle":lifecycle,"description":obligation.description,
                "mustResolveBeforeChapter":obligation.must_resolve_before_chapter,
                "prerequisites":obligation.prerequisites,
                "decisionRef":obligation.decision_ref,
                "payloadJson":obligation_payload
            }));
            effective_continuity_refs.insert(item.thread_ref.clone());
            if let Some(evidence) = &item.evidence {
                obligation_evidence_records.push(json!({
                    "evidenceId":format!("obligation-evidence:{}:{}",final_id,index+1),
                    "obligationId":authority_id,
                    "evidenceType":if lifecycle == "RESOLVED" {"COMPLETION"} else {"PROGRESS"},
                    "quote":evidence.quote,"rangeStart":evidence.start,"rangeEnd":evidence.end
                }));
            }
            if item.disposition == "DEFER_WITH_BRIDGE" {
                let bridge = item
                    .bridge
                    .as_deref()
                    .or_else(|| {
                        plan_payload
                            .get("continuityActions")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .find(|action| {
                                action.get("threadRef").and_then(Value::as_str)
                                    == Some(item.thread_ref.as_str())
                            })
                            .and_then(|action| action.get("bridge"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or_default();
                let decision_id = StableIdentityFactory::from_command(
                    StableIdKind::Decision,
                    &format!("b08-thread-defer:{final_id}:{index}"),
                )
                .map_err(|error| error.to_string())?;
                decision_updates.push(json!({
                    "decisionId":decision_id,
                    "decisionType":"THREAD_DEFER_WITH_BRIDGE",
                    "outcome":"DEFERRED",
                    "reason":"连续性责任已通过桥接继续保留。",
                    "sourceRef":item.thread_ref,
                    "payloadJson":{"bridge":bridge,"sourceChapterRef":reference(&candidate),"sourceAuditRef":reference(&audit),"evidence":item.evidence.as_ref().map(|evidence| json!({"start":evidence.start,"end":evidence.end,"quote":evidence.quote}))}
                }));
            }
            continue;
        }
        let foreshadow = crate::repositories::narrative::NarrativeRepository::foreshadow_authority(
            connection,
            &project_id,
            authority_id,
        )
        .map_err(|error| error.to_string())?;
        if let Some(foreshadow) = foreshadow {
            if item.disposition == "NO_LONGER_APPLICABLE_CHANGE_REQUEST" {
                let (change_artifact, head, decision) = create_b08_change_request_proposal(
                    &change_request_context,
                    index,
                    item,
                    "审计发现既有伏笔责任可能不再适用，等待用户确认。",
                    "正文报告伏笔责任可能不再适用；责任仍保持有效，等待正式变更确认。",
                )?;
                decision_updates.push(decision);
                change_request_heads.push(head);
                change_request_artifacts.push(change_artifact);
                continue;
            }
            foreshadow_delta.push(json!({
                "foreshadowId":authority_id,"intent":foreshadow.intent,
                "status":if matches!(item.disposition.as_str(),"RESOLVE"|"RESOLVED") {"RECOVERED"} else {"REINFORCED"},
                "plantChapterId":foreshadow.plant_chapter_id,"recoverWindowEndChapter":foreshadow.recover_window_end_chapter,
                "secretPayloadRef":foreshadow.secret_payload_ref,
                "payloadJson":{"sourceAuthority":"B08_ACTUAL_NARRATIVE","sourceArtifactId":final_id,"sourceVersion":1,"promotionDecision":promotion_decision,"deltaMode":"EXISTING_AUTHORITY_UPDATE",
                    "resumeCondition":json!({"kind":"ENTITY_PRESENT","value":foreshadow.secret_payload_ref}),
                    "evidence":item.evidence.as_ref().map(|evidence| json!({"start":evidence.start,"end":evidence.end,"quote":evidence.quote}))}
            }));
            effective_continuity_refs.insert(item.thread_ref.clone());
            if item.disposition == "DEFER_WITH_BRIDGE" {
                let bridge = item
                    .bridge
                    .as_deref()
                    .or_else(|| {
                        plan_payload
                            .get("continuityActions")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .find(|action| {
                                action.get("threadRef").and_then(Value::as_str)
                                    == Some(item.thread_ref.as_str())
                            })
                            .and_then(|action| action.get("bridge"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or_default();
                let decision_id = StableIdentityFactory::from_command(
                    StableIdKind::Decision,
                    &format!("b08-thread-defer:{final_id}:{index}"),
                )
                .map_err(|error| error.to_string())?;
                decision_updates.push(json!({"decisionId":decision_id,"decisionType":"THREAD_DEFER_WITH_BRIDGE","outcome":"DEFERRED","reason":"伏笔责任已通过桥接继续保留。","sourceRef":item.thread_ref,"payloadJson":{"bridge":bridge,"sourceChapterRef":reference(&candidate),"sourceAuditRef":reference(&audit)}}));
            }
            continue;
        }
        let description = item
            .description
            .as_deref()
            .filter(|description| !description.trim().is_empty())
            .or_else(|| {
                item.evidence
                    .as_ref()
                    .map(|evidence| evidence.quote.as_str())
            })
            .unwrap_or(&item.thread_ref);
        let resume_condition = item.resume_condition.clone().unwrap_or(Value::Null);
        let lifecycle = if authority_level == "B" {
            "PROGRESSING"
        } else {
            "PROPOSED"
        };
        let promotion_decision_id = StableIdentityFactory::from_command(
            StableIdKind::Decision,
            &format!("thread-promotion:{final_id}:{index}"),
        )
        .map_err(|error| error.to_string())?;
        let promotion_decision_ref = promotion_decision_id.clone();
        decision_updates.push(json!({
            "decisionId":promotion_decision_id,
            "decisionType":"THREAD_PROMOTION_RULE",
            "outcome":authority_level,
            "reason":if authority_level == "B" {"精确证据、兑现对象、兑现条件与定级信号均齐备。"} else {"证据、兑现对象或兑现条件不完整，保留为 C 级候选。"},
            "sourceRef":item.thread_ref,
            "payloadJson":{"ruleVersion":crate::narrative::THREAD_PROMOTION_RULE_VERSION,"promotionSignals":item.promotion_signals,"evidence":item.evidence.as_ref().map(|evidence| json!({"start":evidence.start,"end":evidence.end,"quote":evidence.quote})),"fulfillmentObject":item.fulfillment_object,"resumeCondition":resume_condition}
        }));
        let mut obligation_payload = json!({
            "sourceAuthority":"B08_ACTUAL_NARRATIVE",
            "sourceArtifactId":final_id,
            "sourceVersion":1,
            "resumeCondition":resume_condition,
            "fulfillmentObject":item.fulfillment_object,
            "ruleVersion":crate::narrative::THREAD_PROMOTION_RULE_VERSION,
            "promotionDecision":promotion_decision,
            "deltaMode":"NEW_CONTINUITY"
        });
        if let Some(source_event_id) = &item.source_event_id {
            obligation_payload["sourceEventId"] = json!(source_event_id);
        }
        obligation_delta.push(json!({
            "obligationId":authority_id,
            "scope":"CHAPTER",
            "obligationType":if authority_level == "B" {"PROMOTED_CONTINUITY"} else {"CONTINUITY_CANDIDATE"},
            "authorityLevel":authority_level,
            "lifecycle":lifecycle,
            "description":description,
            "mustResolveBeforeChapter":Value::Null,
            "prerequisites":[],
            "decisionRef":promotion_decision_ref,
            "payloadJson":obligation_payload
        }));
        if let Some(evidence) = &item.evidence {
            new_responsibility_evidence.push(json!({
                "obligationId":authority_id,
                "quote":evidence.quote,"rangeStart":evidence.start,"rangeEnd":evidence.end
            }));
        }
        if authority_level == "B" {
            effective_continuity_refs.insert(item.thread_ref.clone());
        }
    }
    let evidence_records = extraction
        .canon_candidates
        .iter()
        .enumerate()
        .map(|(index, item)| {
            json!({
                "evidenceId":format!("evidence:{}:{}", final_id, index + 1),
                "factId":item.fact_id,
                "eventId":item.event_type.as_ref().map(|_| format!("event:{}", item.fact_id)),
                "quote":item.evidence.quote,
                "rangeStart":item.evidence.start,
                "rangeEnd":item.evidence.end,
                "evidenceType":"ACTUAL_NARRATIVE"
            })
        })
        .collect::<Vec<_>>();
    let final_payload = json!({"payloadSchema":"final-chapter.r1","chapterId":chapter_id,"body":candidate.payload["body"],"bodyDigest":candidate.payload["bodyDigest"],"candidateRef":candidate_ref.clone(),"chapterPlanRef":plan_ref.clone(),"contextManifestRef":context_ref.clone(),"auditRef":reference(&audit),"qualityReportRef":Value::Null,"tasteProfileRef":Value::Null,"wordCount":candidate.payload["body"].as_str().map(|s| s.chars().count()).unwrap_or(0),"stateDigest":state_digest,"validatedEvidence":evidence_records,"validatedObligationEvidence":obligation_evidence_records,"newResponsibilityEvidence":new_responsibility_evidence});
    let mut final_artifact = artifact(
        &project_id,
        final_id,
        ArtifactType::FinalChapter,
        &chapter_id,
        final_payload,
        &command_id,
        &job_id,
        Some(&render_run_id),
        &now,
        vec![candidate_ref.clone(), reference(&audit)],
    )?;
    final_artifact.provenance.context_manifest_ref = Some(context_ref.artifact_id.clone());
    let task_states = extraction.task_progress_candidates.iter().map(|item| json!({
        "taskId":item.task_id,
        "status":item.status,
        "evidenceRefs":[{"artifactId":final_artifact.artifact_id,"version":1,"rangeStart":item.evidence.start,"rangeEnd":item.evidence.end,"quote":item.evidence.quote}],
        "remainingConditions":[]
    })).collect::<Vec<_>>();
    let canon_event_refs = extraction
        .canon_candidates
        .iter()
        .filter(|item| item.event_type.is_some())
        .map(|item| format!("event:{}", item.fact_id))
        .collect::<Vec<_>>();
    let exit_payload = json!({"payloadSchema":"chapter-exit-state.r1","chapterId":chapter_id,"closedAt":now,"stateDigest":state_digest,"activeThreadRefs":extraction.continuity_candidates.iter().filter(|item| item.disposition != "RESOLVED" && effective_continuity_refs.contains(&item.thread_ref)).map(|item| item.thread_ref.clone()).collect::<Vec<_>>(),"taskStates":task_states,"canonEventRefs":canon_event_refs,"evidenceRefs":[]});
    let exit_artifact = artifact(
        &project_id,
        exit_id,
        ArtifactType::ChapterExitState,
        &chapter_id,
        exit_payload,
        &command_id,
        &job_id,
        None,
        &now,
        vec![reference(&audit)],
    )?;
    let task_reality = if let Some(route_ref) =
        crate::repositories::narrative::NarrativeRepository::volume_plan_route(
            connection,
            &project_id,
            &volume_id,
        )
        .map_err(|error| error.to_string())?
    {
        let route = crate::repositories::narrative::NarrativeRepository::artifact_payload_by_ref(
            connection,
            &project_id,
            &route_ref,
        )
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "VOLUME_PLAN_PAYLOAD_NOT_FOUND".to_string())?;
        let route_tasks = route
            .get("tasks")
            .and_then(Value::as_array)
            .ok_or_else(|| "VOLUME_PLAN_TASKS_INVALID".to_string())?;
        let route_task_ids = route_tasks
            .iter()
            .map(|task| {
                task.get("taskId")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| "VOLUME_PLAN_TASKS_INVALID".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let route_task_id_refs = route_task_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let current_task_states =
            crate::repositories::narrative::NarrativeRepository::task_states_by_ids_complete(
                connection,
                &project_id,
                &route_task_id_refs,
            )
            .map_err(|error| error.to_string())?;
        let state_by_id = current_task_states
            .into_iter()
            .map(|state| {
                let status = state
                    .value
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("NOT_STARTED")
                    .to_owned();
                (state.state_id, status)
            })
            .collect::<BTreeMap<_, _>>();
        let audit_status_by_id = extraction
            .task_progress_candidates
            .iter()
            .map(|candidate| (candidate.task_id.as_str(), candidate.status.as_str()))
            .collect::<BTreeMap<_, _>>();
        route_tasks
            .iter()
            .zip(route_task_ids.iter())
            .map(|(task, task_id)| {
                let class = task
                    .get("class")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "VOLUME_PLAN_TASKS_INVALID".to_string())?;
                let estimated_min_cost = task
                    .pointer("/estimatedChapterCost/min")
                    .and_then(Value::as_i64)
                    .or_else(|| {
                        task.pointer("/estimatedChapterCost/preferred")
                            .and_then(Value::as_i64)
                    })
                    .unwrap_or(1);
                let status = audit_status_by_id
                    .get(task_id.as_str())
                    .copied()
                    .or_else(|| state_by_id.get(task_id).map(String::as_str))
                    .unwrap_or("NOT_STARTED")
                    .to_owned();
                let expected = plan_payload
                    .get("requiredStateOutcomes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .find(|condition| {
                        condition.get("value").and_then(Value::as_str) == Some(task_id.as_str())
                    })
                    .and_then(|condition| condition.get("kind").and_then(Value::as_str))
                    .map(ToOwned::to_owned);
                Ok(ReconciliationTaskReality {
                    task_id: task_id.clone(),
                    class: class.to_owned(),
                    status,
                    estimated_min_cost,
                    expected,
                })
            })
            .collect::<Result<Vec<_>, String>>()?
    } else {
        Vec::new()
    };
    let mut responsibility_reality = extraction
        .continuity_candidates
        .iter()
        .map(|item| ReconciliationResponsibilityReality {
            thread_ref: item.thread_ref.clone(),
            observed: true,
            disposition: Some(item.disposition.clone()),
        })
        .collect::<Vec<_>>();
    for reference in plan_payload
        .pointer("/hardScope/responsibilityRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !responsibility_reality.iter().any(|item| {
            item.thread_ref == reference
                || item.thread_ref.strip_prefix("thread:") == Some(reference)
                || reference.strip_prefix("thread:") == Some(item.thread_ref.as_str())
        }) {
            responsibility_reality.push(ReconciliationResponsibilityReality {
                thread_ref: reference.to_owned(),
                observed: false,
                disposition: None,
            });
        }
    }
    let post_commit_capacity =
        crate::planning::horizon_capacity_after_chapter(connection, &project_id, &chapter_id)?;
    let reconciliation = classify_plan_reality(&ReconciliationFacts {
        task_reality,
        remaining_chapter_capacity: post_commit_capacity,
        canon_gate_passed: true,
        responsibility_reality,
    });
    let mut reality = extraction
        .task_progress_candidates
        .iter()
        .map(|item| {
            let expected = plan_payload
                .get("requiredStateOutcomes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|condition| {
                    condition.get("value").and_then(Value::as_str)
                        == Some(item.task_id.as_str())
                })
                .and_then(|condition| condition.get("kind").and_then(Value::as_str))
                .unwrap_or("UNSPECIFIED");
            json!({"taskId":item.task_id,"plannedExpected":expected,"actualObserved":item.status,"evidence":{"start":item.evidence.start,"end":item.evidence.end,"quote":item.evidence.quote}})
        })
        .collect::<Vec<_>>();
    // Reality is the post-audit observation ledger, not only task status.  A
    // chapter can advance Canon/Knowledge/Event state even when the model did
    // not emit a valid task-evidence span; those committed observations must
    // remain visible to reconciliation and the next planning context.
    reality.extend(extraction.canon_candidates.iter().map(|item| {
        json!({
            "kind":"CANON_FACT",
            "factId":item.fact_id,
            "subjectId":item.subject_id,
            "predicate":item.predicate,
            "actualObserved":"COMMITTED",
            "evidence":{"start":item.evidence.start,"end":item.evidence.end,"quote":item.evidence.quote}
        })
    }));
    reality.extend(extraction.knowledge_candidates.iter().map(|item| {
        json!({
            "kind":"KNOWLEDGE",
            "knowledgeId":item.knowledge_id,
            "factId":item.fact_id,
            "knowerId":item.knower_id,
            "actualObserved":item.epistemic_state,
            "evidence":{"start":item.evidence.start,"end":item.evidence.end,"quote":item.evidence.quote}
        })
    }));
    reality.extend(extraction.continuity_candidates.iter().filter(|item| effective_continuity_refs.contains(&item.thread_ref)).map(|item| {
        json!({
            "kind":"CONTINUITY",
            "threadRef":item.thread_ref,
            "actualObserved":item.disposition,
            "evidence":item.evidence.as_ref().map(|evidence| json!({"start":evidence.start,"end":evidence.end,"quote":evidence.quote}))
        })
    }));
    let mut model_run_refs = vec![render_run_id.clone(), audit_run_id.clone()];
    model_run_refs.extend(prose_run_ids.iter().cloned());
    let mut receipt_payload = json!({"payloadSchema":"production-receipt.r1","executionMode":execution_mode,"projectId":project_id,"chapterId":chapter_id,"jobId":job_id,"contextManifestRef":context_ref,"chapterPlanRef":plan_ref.clone(),"gatePolicyRef":policy_ref.clone(),"canonSemanticPolicyVersion":gate_policy_payload.pointer("/canonSemantics/policyVersion").and_then(Value::as_str),"threadPromotionRuleVersion":crate::narrative::THREAD_PROMOTION_RULE_VERSION,"candidateRef":candidate_ref.clone(),"finalChapterRef":reference(&final_artifact),"auditRef":reference(&audit),"qualityReportRef":Value::Null,"tasteProfileRef":Value::Null,"creativeEnrichment":Value::Null,"modelRunRefs":model_run_refs,"reconciliation":reconciliation,"reality":reality,"threadPromotionDecisions":thread_promotion_decisions,"craftSkills":craft_skills});
    if craft_writing.is_some() {
        receipt_payload["continuityDetails"] = json!(crate::craft_plan::continuity_details(
            &audit.payload["audit"],
            &body
        ));
    }
    let receipt_artifact = artifact(
        &project_id,
        receipt_artifact_id,
        ArtifactType::ProductionReceipt,
        &chapter_id,
        receipt_payload,
        &command_id,
        &job_id,
        None,
        &now,
        vec![reference(&final_artifact), reference(&exit_artifact)],
    )?;
    let certificate_evidence = vec![reference(&audit)];
    let certificate = make_certificate(
        &project_id,
        policy_ref,
        reference(&final_artifact),
        certificate_evidence,
        &now,
    )?;
    let mut heads = vec![
        json!({"artifactType":"FinalChapter","scope":chapter_scope(&chapter_id),"artifactId":final_artifact.artifact_id,"version":1,"payloadHash":final_artifact.payload_hash}),
        json!({"artifactType":"ChapterExitState","scope":chapter_scope(&chapter_id),"artifactId":exit_artifact.artifact_id,"version":1,"payloadHash":exit_artifact.payload_hash}),
        json!({"artifactType":"ProductionReceipt","scope":chapter_scope(&chapter_id),"artifactId":receipt_artifact.artifact_id,"version":1,"payloadHash":receipt_artifact.payload_hash}),
    ];
    heads.extend(change_request_heads);
    let mut bundle_artifacts = Vec::new();
    let final_ref = reference(&final_artifact);
    let canon_delta = extraction.canon_candidates.iter().map(|item| json!({
        "entityId":item.entity_id,
        "factId":item.fact_id,
        "subjectId":item.subject_id,
        "predicate":item.predicate,
        "objectJson":item.object_json,
        "authorityLevel":item.authority_level,
        "eventType":item.event_type,
        "validFromChapterId":chapter_id,
        "payloadJson":{
            "chapterId":chapter_id,
            "eventId":item.event_type.as_ref().map(|_| format!("event:{}", item.fact_id)),
            "storyTime":item.story_time,
            "canonicalName":item.canonical_name,
            "entityType":item.entity_type,
            "sourceArtifactId":final_ref.artifact_id,
            "sourceVersion":final_ref.version,
            "evidenceRange":{"start":item.evidence.start,"end":item.evidence.end,"quote":item.evidence.quote},
            "supersedesFactId":extraction.canon_transitions.get(&item.fact_id),
            "alsoSupersedesFactIds":extraction.canon_duplicate_transitions.get(&item.fact_id)
        }
    })).collect::<Vec<_>>();
    let knowledge_delta = extraction.knowledge_candidates.iter().map(|item| json!({
        "knowledgeId":item.knowledge_id,
        "knowerId":item.knower_id,
        "factId":item.fact_id,
        "epistemicState":item.epistemic_state,
        "visibilityScope":item.visibility_scope,
        "fromChapterId":chapter_id,
        "payloadJson":{"sourceArtifactId":final_ref.artifact_id,"sourceVersion":final_ref.version}
    })).collect::<Vec<_>>();
    let state_delta = extraction.task_progress_candidates.iter().map(|item| json!({
        "stateId":item.task_id,
        "stateType":"volume.task.progress",
        "chapterId":chapter_id,
        "valueJson":{
            "taskId":item.task_id,
            "status":item.status,
            "evidenceRefs":[{"artifactId":final_ref.artifact_id,"version":final_ref.version,"rangeStart":item.evidence.start,"rangeEnd":item.evidence.end,"quote":item.evidence.quote}],
            "remainingConditions":[]
        },
        "payloadJson":{"actualNarrative":true,"sourceArtifactId":final_ref.artifact_id,"sourceVersion":final_ref.version}
    })).collect::<Vec<_>>();
    bundle_artifacts.push(candidate);
    bundle_artifacts.extend([audit, final_artifact, exit_artifact, receipt_artifact]);
    bundle_artifacts.extend(change_request_artifacts);
    let dependency_source_hash = if gateway.fault_injection() == Some("dependency_source_hash") {
        "0".repeat(64)
    } else {
        plan_ref.payload_hash.clone()
    };
    let mut bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":format!("commit:{command_id}"),"projectId":project_id,"commandId":command_id,"idempotencyKey":idempotency_key,
        "expectedProjectRevision":current_revision,"contractVersion":"m0a.v1","createdAt":now,
        "artifacts":bundle_artifacts,"headUpdates":heads,
        "canonDelta":canon_delta,
        "knowledgeDelta":knowledge_delta,
        "obligationDelta":obligation_delta,
        "foreshadowDelta":foreshadow_delta,
        "decisionUpdates":decision_updates,
        "stateDelta":state_delta,
        "dependencyEdges":[{"edgeId":format!("edge:{}:plan",final_ref.artifact_id),"sourceRef":plan_ref.artifact_id,"targetRef":final_ref.artifact_id,"dependencyType":"PLAN_INPUT","scope":"CHAPTER","impactClass":"HARD","sourceHash":dependency_source_hash,"payloadJson":{"createdBy":"B08::ChapterProductionRuntime"}}],
        "jobUpdates":[{"jobId":job_id,"status":"COMPLETED","currentStage":"COMPLETED","attemptNo":1}],
        "gateCertificates":[certificate],"receiptDraft":{"domain":"B08_CHAPTER_PRODUCTION","artifactRefs":[],"payload":{"executionMode":execution_mode,"chapterId":chapter_id,"reconciliation":reconciliation,"droppedUnboundKnowledge":extraction.dropped_unbound_knowledge}}
    })).map_err(|error| format!("B08_BUNDLE_INVALID:{error}"))?;
    let _commit_control_guard =
        update_commit_stage(manager, connection, &project_id, &job_id, &now)?;
    let receipt = match commit_chapter_after_revision_recheck(
        connection,
        request,
        &project_id,
        &chapter_id,
        &volume_id,
        execution_mode,
        &plan_ref,
        &plan_payload,
        &mut bundle,
        &now,
        gateway,
    ) {
        Ok(receipt) => receipt,
        Err(error) if error.starts_with("PLAN_STALE") => {
            block_for_recompile(connection, &project_id, &job_id, &now)?;
            return Err(error);
        }
        Err(error) => return Err(error),
    };
    drop(_commit_control_guard);
    // B09 owns the rebuildable volume planning projection. Refresh it only
    // after B08's authoritative chapter CommitBundle succeeds. Its failure is
    // recoverable and does not reverse or mislabel the committed chapter. The
    // commit wrote a persistent DIRTY marker; the B09 owner atomically persists
    // its projection and refreshes Health before returning success.
    let b09_rebuild = crate::volume_lifecycle::VolumeLifecycleOwner::rebuild_planning_state(
        connection,
        &project_id,
        &volume_id,
        &now,
    );
    let health_projection_ready =
        crate::repositories::health::HealthRepository::projection_is_current(
            connection,
            &project_id,
        )
        .unwrap_or(false);
    let (planning_projection_status, planning_projection_error) = match b09_rebuild {
        Ok(_) if health_projection_ready => ("UPDATED", None),
        Ok(_) => (
            "DEFERRED_REBUILDABLE",
            Some("HEALTH_PROJECTION_NOT_READY".to_string()),
        ),
        Err(error) => ("DEFERRED_REBUILDABLE", Some(error)),
    };
    let reconciliation_execution = if planning_projection_status != "UPDATED" {
        json!({"status":"DEFERRED_REBUILDABLE","reason":"B09_PLANNING_PROJECTION_NOT_READY"})
    } else if execution_mode == "LONG_FORM" {
        match execute_reconciliation_action(
            &handle,
            &project_id,
            &volume_id,
            &chapter_id,
            reconciliation,
            &command_id,
            &now,
        ) {
            Ok(value) => value,
            Err(error) => json!({"status":"FAILED","error":error}),
        }
    } else {
        json!({"status":"SKIPPED","reason":"OPENING_BOOTSTRAP"})
    };
    DurableJobService::release_lease(connection, &project_id, &job_id, &now)
        .map_err(|error| error.to_string())?;
    DurableJobService::set_runtime_state(connection, &project_id, "COMPLETED", None, &now)
        .map_err(|error| error.to_string())?;
    if execution_mode == "OPENING_BOOTSTRAP" {
        let opening_progress =
            crate::repositories::project_entry::opening_progress(connection, &project_id)
                .map_err(|error| error.to_string())?;
        let phase = if opening_progress.completed >= opening_progress.total {
            "SIGNING"
        } else {
            "OPENING_PRODUCTION"
        };
        DurableJobService::set_project_phase(connection, &project_id, phase, &now)
            .map_err(|error| error.to_string())?;
    }
    update_stage(manager, connection, &project_id, &job_id, "COMPLETED", &now)?;
    Ok(
        json!({"commandId":command_id,"projectId":project_id,"finalStatus":"committed","message":if planning_projection_status=="UPDATED" {"B08 章节已完成原子提交"} else {"B08 章节已提交，B09 投影待维护重试"},"stage":"COMPLETED","executionMode":execution_mode,"jobId":job_id,"candidateRef":candidate_ref,"finalChapterRef":final_ref,"planningProjectionStatus":planning_projection_status,"healthProjectionStatus":if health_projection_ready {"READY"} else {"DIRTY_OR_UNAVAILABLE"},"planningProjectionError":planning_projection_error,"reconciliationExecution":reconciliation_execution,"receipt":receipt}),
    )
}

pub fn validate_candidate_evidence_range(
    body: &str,
    start: usize,
    end: usize,
) -> Result<(), String> {
    if start > end || end > body.chars().count() {
        return Err("B08_EVIDENCE_RANGE_INVALID".into());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationTaskReality {
    pub task_id: String,
    pub class: String,
    pub status: String,
    pub estimated_min_cost: i64,
    pub expected: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationResponsibilityReality {
    pub thread_ref: String,
    pub observed: bool,
    pub disposition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationFacts {
    pub task_reality: Vec<ReconciliationTaskReality>,
    pub remaining_chapter_capacity: i64,
    pub canon_gate_passed: bool,
    pub responsibility_reality: Vec<ReconciliationResponsibilityReality>,
}

pub fn classify_plan_reality(facts: &ReconciliationFacts) -> &'static str {
    if !facts.canon_gate_passed
        || facts.responsibility_reality.iter().any(|responsibility| {
            responsibility.disposition.as_deref() == Some("NO_LONGER_APPLICABLE_CHANGE_REQUEST")
        })
    {
        return "CHANGE_REQUEST";
    }
    let unsatisfied_required_tasks = facts.task_reality.iter().filter(|task| {
        matches!(task.class.as_str(), "REQUIRED" | "CLIMAX") && task.status != "SATISFIED"
    });
    let mut has_unsatisfied_required_task = false;
    let mut unsatisfied_required_cost = 0_i64;
    let mut has_invalidated_required_task = false;
    for task in unsatisfied_required_tasks {
        has_unsatisfied_required_task = true;
        has_invalidated_required_task |= task.status == "INVALIDATED";
        unsatisfied_required_cost =
            unsatisfied_required_cost.saturating_add(task.estimated_min_cost);
    }
    if has_invalidated_required_task
        || unsatisfied_required_cost > facts.remaining_chapter_capacity
        || (facts.remaining_chapter_capacity <= 0 && has_unsatisfied_required_task)
    {
        return "REPLAN_VOLUME_ROUTE";
    }
    if facts
        .responsibility_reality
        .iter()
        .any(|responsibility| !responsibility.observed)
        || facts
            .task_reality
            .iter()
            .any(|task| task.status == "BLOCKED")
    {
        return "RECOMPILE_ROLLING_WINDOW";
    }
    if facts.task_reality.iter().any(|task| {
        task.expected.as_deref().is_some_and(|expected| {
            expected != "UNSPECIFIED"
                && expected.strip_prefix("TASK_").unwrap_or(expected) != task.status
        })
    }) {
        return "RECOMPILE_NEXT_CHAPTER";
    }
    "NO_ACTION"
}

pub fn reconciliation_for_actual_state(facts: &ReconciliationFacts) -> &'static str {
    classify_plan_reality(facts)
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceSpan {
    start: usize,
    end: usize,
    quote: String,
    #[serde(default)]
    rebound: bool,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonCandidate {
    fact_id: String,
    /// The model's own label for the fact (LD-1.2: the system names it).
    #[serde(default)]
    model_label: Option<String>,
    subject_id: String,
    #[serde(default)]
    entity_id: Option<String>,
    #[serde(default)]
    canonical_name: Option<String>,
    #[serde(default)]
    entity_type: Option<String>,
    predicate: String,
    object_json: Value,
    authority_level: String,
    #[serde(default)]
    event_type: Option<String>,
    #[serde(default)]
    story_time: Option<String>,
    #[serde(default)]
    valid_from_chapter_id: Option<String>,
    #[serde(default)]
    valid_to_chapter_id: Option<String>,
    evidence: EvidenceSpan,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct KnowledgeCandidate {
    knowledge_id: String,
    knower_id: String,
    fact_id: String,
    epistemic_state: String,
    #[serde(default)]
    visibility_scope: Option<String>,
    #[serde(default)]
    acquired_story_time: Option<String>,
    evidence: EvidenceSpan,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ContinuityCandidate {
    thread_ref: String,
    disposition: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    evidence: Option<EvidenceSpan>,
    #[serde(default)]
    promotion_signals: PromotionSignal,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    fulfillment_object: Option<String>,
    #[serde(default)]
    resume_condition: Option<Value>,
    #[serde(default)]
    bridge: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskProgressCandidate {
    task_id: String,
    status: String,
    evidence: EvidenceSpan,
}

#[derive(Debug, Clone, Copy, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum AuditCompleteness {
    CompleteNoChange,
    CompleteWithDelta,
    Incomplete,
    Truncated,
    UnresolvedHard,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditExtraction {
    quality_findings: Vec<Value>,
    canon_candidates: Vec<CanonCandidate>,
    knowledge_candidates: Vec<KnowledgeCandidate>,
    continuity_candidates: Vec<ContinuityCandidate>,
    task_progress_candidates: Vec<TaskProgressCandidate>,
    revealed_reservation_refs: Vec<String>,
    evidence_ranges: Vec<EvidenceSpan>,
    audit_completeness: AuditCompleteness,
    unresolved_hard: bool,
    /// FX-17: knowledge whose knower/fact is neither existing nor extracted in
    /// this audit; dropped from the commit and kept here as a durable record.
    #[serde(default)]
    dropped_unbound_knowledge: Vec<Value>,
    /// Completion claims whose dependencies or predicate are unmet; kept as
    /// progress (ACTIVE) and recorded here.
    #[serde(default)]
    downgraded_task_claims: Vec<Value>,
    /// FX-17: new fact id -> the open fact from an earlier chapter it replaces.
    #[serde(default)]
    canon_transitions: BTreeMap<String, String>,
    /// New fact id -> further open facts with the same value it replaces.
    #[serde(default)]
    canon_duplicate_transitions: BTreeMap<String, Vec<String>>,
}

fn validate_audit_candidate_binding(
    review_payload: &Value,
    effective_candidate_ref: &ArtifactRef,
) -> Result<(), String> {
    let expected = serde_json::to_value(effective_candidate_ref)
        .map_err(|error| format!("B08_AUDIT_CANDIDATE_BINDING:{error}"))?;
    validate_audit_candidate_binding_value(review_payload, &expected)
}

fn validate_audit_candidate_binding_value(
    review_payload: &Value,
    effective_candidate_ref: &Value,
) -> Result<(), String> {
    if review_payload.get("candidateRef") != Some(effective_candidate_ref) {
        return Err("B08_AUDIT_CANDIDATE_BINDING_MISMATCH".into());
    }
    Ok(())
}

/// Evidence quotes of a raw audit reply that occur nowhere in the body, even
/// ignoring whitespace or across an ellipsis.
fn unlocatable_audit_quotes(raw: &Value, body: &str) -> Vec<String> {
    let squeezed = |text: &str| {
        text.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
    };
    let body_squeezed = squeezed(body);
    let mut quotes = Vec::new();
    let fields = [
        "canonCandidates",
        "knowledgeCandidates",
        "continuityCandidates",
        "taskProgressCandidates",
    ];
    let candidate_quotes = fields.iter().flat_map(|field| {
        raw.get(*field)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.pointer("/evidence/quote").and_then(Value::as_str))
    });
    let range_quotes = raw
        .get("evidenceRanges")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("quote").and_then(Value::as_str));
    for quote in candidate_quotes.chain(range_quotes) {
        let found = body.contains(quote)
            || body_squeezed.contains(&squeezed(quote))
            || locate_quote_span(body, quote).is_some();
        if !quote.is_empty() && !found && !quotes.iter().any(|known| known == quote) {
            quotes.push(quote.to_owned());
        }
    }
    quotes
}

/// Candidates of a raw audit reply missing a required field or evidence,
/// named so a rerun can complete them.
fn incomplete_audit_candidates(raw: &Value) -> Value {
    let requirements: [(&str, &str, &[&str]); 4] = [
        (
            "canonCandidates",
            "factId",
            &["factId", "subjectId", "predicate", "objectJson", "evidence"],
        ),
        (
            "knowledgeCandidates",
            "knowledgeId",
            &[
                "knowledgeId",
                "knowerId",
                "factId",
                "epistemicState",
                "acquiredStoryTime",
                "visibilityScope",
                "evidence",
            ],
        ),
        (
            "continuityCandidates",
            "threadRef",
            &["threadRef", "disposition"],
        ),
        (
            "taskProgressCandidates",
            "taskId",
            &["taskId", "status", "evidence"],
        ),
    ];
    let mut incomplete = Vec::new();
    for (field, id_key, required) in requirements {
        for item in raw
            .get(field)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let missing = required
                .iter()
                .filter(|key| item.get(**key).is_none())
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                incomplete.push(json!({"field":field,"id":item.get(id_key),"missing":missing}));
            }
        }
    }
    json!(incomplete)
}

/// The chapter plan as the audit sees it: scenes keep only their type,
/// size and reveal markers, so no plan prose can be quoted as evidence.
fn audit_plan_view(plan: &Value) -> Value {
    let mut view = plan.clone();
    if let Some(scenes) = view.get_mut("sceneExecution").and_then(Value::as_array_mut) {
        for scene in scenes {
            if let Some(object) = scene.as_object_mut() {
                object.retain(|key, _| {
                    matches!(
                        key.as_str(),
                        "sceneType" | "estimatedCharacters" | "action" | "reservationRef"
                    )
                });
            }
        }
    }
    view
}

fn validate_audit_completeness(
    audit_payload: &Value,
    audit: &AuditExtraction,
) -> Result<(), String> {
    let dropped = audit_payload
        .pointer("/normalization/droppedCandidates")
        .and_then(Value::as_object)
        .map(|items| items.values().any(|value| value.as_u64().unwrap_or(0) > 0))
        .unwrap_or(false)
        || audit_payload
            .pointer("/normalization/droppedEvidenceRanges")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0;
    if dropped {
        return Err("B08_AUDIT_INCOMPLETE_NORMALIZATION_DROPPED_CONTENT".into());
    }
    if audit_payload
        .pointer("/normalization/invalidContinuityEvidence")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        > 0
    {
        return Err("B08_AUDIT_INCOMPLETE_NORMALIZATION_DROPPED_CONTENT".into());
    }
    if audit_payload
        .pointer("/normalization/reboundEvidence")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        > 0
    {
        return Err("B08_AUDIT_INCOMPLETE_EVIDENCE_REBOUND".into());
    }
    if audit.unresolved_hard || audit.audit_completeness == AuditCompleteness::UnresolvedHard {
        return Err("B08_AUDIT_UNRESOLVED_HARD".into());
    }
    match audit.audit_completeness {
        AuditCompleteness::Incomplete => Err("B08_AUDIT_INCOMPLETE".into()),
        AuditCompleteness::Truncated => Err("B08_AUDIT_TRUNCATED".into()),
        AuditCompleteness::CompleteNoChange => {
            if audit.canon_candidates.is_empty()
                && audit.knowledge_candidates.is_empty()
                && audit.continuity_candidates.is_empty()
                && audit.task_progress_candidates.is_empty()
                && audit.revealed_reservation_refs.is_empty()
                && (!audit.quality_findings.is_empty() || audit.evidence_ranges.is_empty())
            {
                Ok(())
            } else {
                Err("B08_AUDIT_COMPLETENESS_MISMATCH".into())
            }
        }
        AuditCompleteness::CompleteWithDelta => {
            if audit.canon_candidates.is_empty()
                && audit.knowledge_candidates.is_empty()
                && audit.continuity_candidates.is_empty()
                && audit.task_progress_candidates.is_empty()
                && audit.revealed_reservation_refs.is_empty()
            {
                Err("B08_AUDIT_COMPLETENESS_MISMATCH".into())
            } else {
                Ok(())
            }
        }
        AuditCompleteness::UnresolvedHard => unreachable!("handled above"),
    }
}

fn normalize_audit_model_output(mut payload: Value, body: &str) -> Value {
    let unbound_citations = bind_cited_evidence(&mut payload, body);
    let body_chars = body.chars().collect::<Vec<_>>();
    let mut rebound_evidence = 0_u64;
    let mut invalid_continuity_evidence = 0_u64;
    let normalize_evidence = |value: &Value| -> Option<Value> {
        // FX-17: a verbatim quote that occurs exactly once identifies the
        // evidence exactly, whatever offsets the model computed; offsets are
        // only trusted when they reproduce the supplied quote. Anything else
        // (ambiguous, absent or ellipsis quotes) stays rebound and incomplete.
        let supplied_quote = value
            .get("quote")
            .and_then(Value::as_str)
            .filter(|quote| !quote.is_empty());
        let supplied_offsets = value
            .get("start")
            .and_then(Value::as_u64)
            .zip(value.get("end").and_then(Value::as_u64))
            .map(|(start, end)| (start as usize, end as usize));
        if let Some(quote) = supplied_quote {
            let offsets_reproduce_quote = supplied_offsets.is_some_and(|(start, end)| {
                start < end
                    && end <= body_chars.len()
                    && body_chars[start..end].iter().collect::<String>() == quote
            });
            // A quote that identifies one passage (verbatim, or loosely: lost
            // whitespace, an ellipsis, left-out words or a few changed
            // characters) is bound to that passage.
            if !offsets_reproduce_quote {
                if let Some((start, end)) = locate_quote_span(body, quote) {
                    let located = body_chars[start..end].iter().collect::<String>();
                    let mut evidence =
                        json!({"start":start,"end":end,"quote":located,"rebound":false});
                    if located != quote {
                        evidence["suppliedQuote"] = json!(quote);
                    }
                    return Some(evidence);
                }
            }
        }
        let start = value.get("start")?.as_u64()? as usize;
        let end = value.get("end")?.as_u64()? as usize;
        if start < end && end <= body_chars.len() {
            let quote = body_chars[start..end].iter().collect::<String>();
            let supplied_quote = value.get("quote").and_then(Value::as_str);
            return Some(json!({
                "start":start,
                "end":end,
                "quote":quote,
                "rebound":supplied_quote != Some(quote.as_str())
            }));
        }
        // A unique exact match may be retained for diagnosis, but validation
        // marks it incomplete so a mismatched range cannot authorize a commit.
        let quote = value
            .get("quote")
            .and_then(Value::as_str)
            .filter(|quote| !quote.is_empty())?;
        let matches = body.match_indices(quote).take(2).collect::<Vec<_>>();
        if matches.len() == 1 {
            let (byte_start, _) = matches[0];
            let char_start = body[..byte_start].chars().count();
            return Some(json!({
                "start":char_start,
                "end":char_start + quote.chars().count(),
                "quote":quote,
                "rebound":true
            }));
        }
        let (char_start, char_end) = ellipsis_quote_span(body, quote)?;
        let actual_quote = body
            .chars()
            .skip(char_start)
            .take(char_end - char_start)
            .collect::<String>();
        Some(json!({
            "start":char_start,
            "end":char_end,
            "quote":actual_quote,
            "rebound":true
        }))
    };
    let candidate_requirements = [
        (
            "canonCandidates",
            &[
                "factId",
                "subjectId",
                "predicate",
                "objectJson",
                "authorityLevel",
            ][..],
        ),
        (
            "knowledgeCandidates",
            &["knowledgeId", "knowerId", "factId", "epistemicState"][..],
        ),
        ("continuityCandidates", &["threadRef", "disposition"][..]),
        ("taskProgressCandidates", &["taskId", "status"][..]),
    ];
    let mut dropped = serde_json::Map::new();
    let mut without_evidence = Vec::new();
    for (field, required) in candidate_requirements {
        let Some(items) = payload.get_mut(field).and_then(Value::as_array_mut) else {
            continue;
        };
        let before = items.len();
        let unquoted_before = without_evidence.len();
        items.retain_mut(|item| {
            let Some(object) = item.as_object_mut() else {
                return false;
            };
            // Without a stated authority a fact gets the lowest level, as the
            // commit already assumes.
            if field == "canonCandidates" {
                object.entry("authorityLevel").or_insert_with(|| json!("C"));
                // A fact object stated as a bare value keeps it under `value`.
                if let Some(value) = object.get_mut("objectJson") {
                    if !value.is_object() && !value.is_null() {
                        *value = json!({"value":value.take()});
                    }
                }
            }
            if !required.iter().all(|key| object.contains_key(*key)) {
                return false;
            }
            if field == "continuityCandidates" {
                if let Some(raw_evidence) = object.get("evidence") {
                    match normalize_evidence(raw_evidence) {
                        Some(evidence) => {
                            if evidence.get("rebound").and_then(Value::as_bool) == Some(true) {
                                rebound_evidence += 1;
                            }
                            object.insert("evidence".into(), evidence);
                        }
                        None => {
                            invalid_continuity_evidence += 1;
                            object.remove("evidence");
                        }
                    }
                }
                return true;
            }
            // Owner decision (2026-10-18): a candidate the model gave no
            // evidence for cannot be verified; it is left out and recorded.
            if !object.contains_key("evidence") {
                let id = ["factId", "knowledgeId", "taskId"]
                    .iter()
                    .find_map(|key| object.get(*key))
                    .cloned()
                    .unwrap_or(Value::Null);
                without_evidence.push(json!({"field":field,"id":id}));
                return false;
            }
            let Some(evidence) = object.get("evidence").and_then(normalize_evidence) else {
                return false;
            };
            if evidence.get("rebound").and_then(Value::as_bool) == Some(true) {
                rebound_evidence += 1;
            }
            object.insert("evidence".into(), evidence);
            if field == "knowledgeCandidates" {
                let Some(state) = object.get("epistemicState").and_then(Value::as_str) else {
                    return false;
                };
                let normalized_state = match state {
                    "KNOWS"
                    | "KNOWS_WITH_CONTEXT"
                    | "KNOWS_WITH_EVIDENCE"
                    | "KNOWS_FACT"
                    | "REMEMBERS"
                    | "REMEMBERED"
                    | "RECALLS"
                    | "RECALLED"
                    | "REALIZES"
                    | "REALIZED"
                    | "LEARNS"
                    | "LEARNED"
                    | "DISCOVERS"
                    | "DISCOVERED" => "KNOWS",
                    "BELIEVES_TRUE" | "BELIEVES" => "BELIEVES_TRUE",
                    "DOUBTS" | "INFERS" => "SUSPECTS",
                    "HIDES" => "CONCEALS",
                    "FORGETS" | "FORGOT" | "FORGOTTEN" => "UNKNOWN",
                    "BELIEVES_FALSE" | "SUSPECTS" | "CONCEALS" | "UNKNOWN" => state,
                    _ => return false,
                };
                object.insert("epistemicState".into(), json!(normalized_state));
            }
            true
        });
        let unquoted = without_evidence.len() - unquoted_before;
        dropped.insert(field.into(), json!(before - items.len() - unquoted));
    }
    let (normalized_ranges, dropped_ranges) = payload
        .get("evidenceRanges")
        .and_then(Value::as_array)
        .map(|items| {
            let ranges = items
                .iter()
                .filter_map(&normalize_evidence)
                .collect::<Vec<_>>();
            rebound_evidence += ranges
                .iter()
                .filter(|range| range.get("rebound").and_then(Value::as_bool) == Some(true))
                .count() as u64;
            let dropped = items.len() - ranges.len();
            (Some(ranges), dropped)
        })
        .unwrap_or((None, 0));
    if let Some(ranges) = normalized_ranges {
        payload["evidenceRanges"] = json!(ranges);
    }
    relabel_audit_completeness(&mut payload);
    payload["normalization"] = json!({
        "policy":"DROP_UNBOUND_AUTHORITY_CANDIDATES",
        "droppedCandidates":dropped,
        "droppedEvidenceRanges":dropped_ranges,
        "droppedWithoutEvidence":without_evidence,
        "reboundEvidence":rebound_evidence,
        "invalidContinuityEvidence":invalid_continuity_evidence,
        "unboundCitations":unbound_citations
    });
    payload
}

fn unplanned_task_candidate_ids(audit: &Value, plan: &Value) -> BTreeSet<String> {
    let planned = plan
        .get("activeTaskRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    audit
        .get("taskProgressCandidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|candidate| candidate.get("taskId").and_then(Value::as_str))
        .filter(|task_id| !planned.contains(task_id))
        .map(ToOwned::to_owned)
        .collect()
}

/// FX-17: a task ID that is neither planned nor a real project task was
/// invented by the model ("do not invent authority refs") and is dropped like
/// other unbound candidates. A real task outside the plan still reaches the
/// Task Scope Gate and blocks.
fn drop_invented_task_candidates(
    audit: &mut Value,
    unplanned: &BTreeSet<String>,
    known: &BTreeSet<String>,
) {
    let mut dropped = 0_u64;
    if let Some(candidates) = audit
        .get_mut("taskProgressCandidates")
        .and_then(Value::as_array_mut)
    {
        candidates.retain(|candidate| {
            let task_id = candidate
                .get("taskId")
                .and_then(Value::as_str)
                .unwrap_or("");
            let invented = unplanned.contains(task_id) && !known.contains(task_id);
            dropped += u64::from(invented);
            !invented
        });
    }
    if let Some(normalization) = audit
        .get_mut("normalization")
        .and_then(Value::as_object_mut)
    {
        normalization.insert("droppedInventedTasks".into(), json!(dropped));
    }
}

/// Char span of the only passage of `body` equal to `quote` once whitespace
/// is ignored on both sides; None when absent or ambiguous.
fn whitespace_insensitive_unique_span(body: &[char], quote: &str) -> Option<(usize, usize)> {
    let needle = quote
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<Vec<_>>();
    if needle.is_empty() {
        return None;
    }
    let positions = (0..body.len())
        .filter(|&index| !body[index].is_whitespace())
        .collect::<Vec<_>>();
    let compact = positions
        .iter()
        .map(|&index| body[index])
        .collect::<Vec<_>>();
    let mut found = compact
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle.as_slice())
        .map(|(offset, _)| offset);
    let offset = found.next()?;
    if found.next().is_some() {
        return None;
    }
    Some((positions[offset], positions[offset + needle.len() - 1] + 1))
}

/// Applies the rewrite's `edits` whose passage occurs once in `body` and
/// whose replacement adds no model habit, each spliced at its own span;
/// returns the new body and how many applied.
fn apply_unique_edits(body: &str, rewrite: &Value) -> (String, usize) {
    let mut revised = body.to_string();
    let mut applied = 0;
    for edit in rewrite
        .get("edits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(8)
    {
        let (Some(quote), Some(replacement)) = (
            edit.get("quote").and_then(Value::as_str),
            edit.get("replacement").and_then(Value::as_str),
        ) else {
            continue;
        };
        // A rewrite must not trade one model habit for another.
        if replacement.trim().is_empty()
            || crate::chapter_craft::habit_count(replacement)
                > crate::chapter_craft::habit_count(quote)
        {
            continue;
        }
        if let Some((start, end)) = locate_unique_quote(&revised, quote) {
            let mut chars = revised.chars().collect::<Vec<_>>();
            chars.splice(start..end, replacement.chars());
            revised = chars.into_iter().collect();
            applied += 1;
        }
    }
    (revised, applied)
}

/// The id the system gives the `n`th (from 1) new audit item of a kind
/// (`canon`, `know` or `thread`) in a chapter (LD-1.2).
pub fn new_audit_item_id(kind: &str, chapter_id: &str, n: usize) -> String {
    format!("{kind}:{}:{n}", chapter_id_digest(chapter_id))
}

/// The part of an id that names the chapter an audit item comes from.
fn chapter_id_digest(chapter_id: &str) -> String {
    sha256_canonical(&json!(chapter_id))
        .map(|digest| digest.chars().take(24).collect::<String>())
        .unwrap_or_default()
}

/// The thread refs the chapter plan's continuity actions name.
fn planned_continuity_refs(plan: &Value) -> BTreeSet<&str> {
    plan.get("continuityActions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|action| action.get("threadRef").and_then(Value::as_str))
        .collect()
}

/// The audit lists a chapter's ledger changes are made of, in the order one
/// may name another (knowledge and threads name facts).
const AUDIT_LEDGER_LISTS: [&str; 5] = [
    "canonCandidates",
    "knowledgeCandidates",
    "continuityCandidates",
    "taskProgressCandidates",
    "revealedReservationRefs",
];

/// "Complete with delta" and "complete, no change" only summarize whether
/// anything was extracted, so the label follows the candidates.
fn relabel_audit_completeness(payload: &mut Value) {
    if matches!(
        payload.get("auditCompleteness").and_then(Value::as_str),
        Some("COMPLETE_NO_CHANGE" | "COMPLETE_WITH_DELTA")
    ) {
        let has_delta = AUDIT_LEDGER_LISTS.iter().any(|field| {
            payload
                .get(*field)
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
        });
        payload["auditCompleteness"] = json!(if has_delta {
            "COMPLETE_WITH_DELTA"
        } else {
            "COMPLETE_NO_CHANGE"
        });
    }
}

/// LD-1 step 2 (owner decision 2026-10-25): an audit the ledger cannot take
/// as a whole is taken item by item. Starting from no changes, each fact,
/// knowledge, thread, task and reveal is added and kept only if the ledger
/// still validates; an audit that reported itself incomplete counts as
/// complete with what it has. What is left out is recorded as `heldBack`
/// (the field, the item and why) for the chapter's reminder.
fn hold_back_audit_items(
    prepared: Value,
    validate: &dyn Fn(&Value) -> Result<AuditExtraction, String>,
) -> Result<(Value, AuditExtraction), String> {
    let mut held = prepared["normalization"]["heldBack"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut kept = prepared.clone();
    for field in AUDIT_LEDGER_LISTS {
        kept[field] = json!([]);
    }
    let completeness = kept["auditCompleteness"].clone();
    if !matches!(
        completeness.as_str(),
        Some("COMPLETE_NO_CHANGE" | "COMPLETE_WITH_DELTA")
    ) || kept["unresolvedHard"] == json!(true)
    {
        held.push(json!({"field":"auditCompleteness","item":completeness,"reason":"B08_AUDIT_UNRESOLVED_HARD"}));
        kept["auditCompleteness"] = json!("COMPLETE_NO_CHANGE");
        kept["unresolvedHard"] = json!(false);
    }
    // What normalization already left out (evidence it could not place) is
    // reported here instead of failing the audit; evidence it could only
    // place by the model's offsets is not trusted and its item is held back.
    let counters = [
        "droppedCandidates",
        "droppedEvidenceRanges",
        "invalidContinuityEvidence",
        "reboundEvidence",
    ];
    let dropped = counters
        .iter()
        .filter(|key| {
            let value = &kept["normalization"][**key];
            value.as_u64().is_some_and(|count| count > 0)
                || value.as_object().is_some_and(|counts| {
                    counts.values().any(|count| count.as_u64().unwrap_or(0) > 0)
                })
        })
        .map(|key| (key.to_string(), kept["normalization"][*key].clone()))
        .collect::<serde_json::Map<_, _>>();
    if !dropped.is_empty() {
        held.push(
            json!({"field":"evidence","item":dropped,"reason":"B08_AUDIT_INCOMPLETE_EVIDENCE"}),
        );
        for key in counters {
            kept["normalization"][key] = json!(0);
        }
    }
    let rebound = |item: &Value| item.pointer("/evidence/rebound") == Some(&json!(true));
    if let Some(ranges) = kept["evidenceRanges"].as_array_mut() {
        ranges.retain(|range| range.get("rebound") != Some(&json!(true)));
    }
    relabel_audit_completeness(&mut kept);
    for field in AUDIT_LEDGER_LISTS {
        for item in prepared[field].as_array().into_iter().flatten() {
            if rebound(item) {
                held.push(json!({"field":field,"item":item,"reason":"B08_AUDIT_INCOMPLETE_EVIDENCE_REBOUND"}));
                continue;
            }
            let mut trial = kept.clone();
            trial[field]
                .as_array_mut()
                .into_iter()
                .for_each(|items| items.push(item.clone()));
            relabel_audit_completeness(&mut trial);
            match validate(&trial) {
                Ok(_) => kept = trial,
                Err(reason) if reason.starts_with("B08_") => {
                    held.push(json!({"field":field,"item":item,"reason":reason}));
                }
                Err(other) => return Err(other),
            }
        }
    }
    // With nothing recorded, ranges stand alone only beside quality findings.
    let nothing_kept = AUDIT_LEDGER_LISTS
        .iter()
        .all(|field| kept[*field].as_array().is_none_or(Vec::is_empty));
    if nothing_kept && kept["qualityFindings"].as_array().is_none_or(Vec::is_empty) {
        kept["evidenceRanges"] = json!([]);
    }
    let extraction = validate(&kept)?;
    // Knowledge about a fact that was held back is dropped with it.
    held.extend(extraction.dropped_unbound_knowledge.iter().map(
        |item| json!({"field":"knowledgeCandidates","item":item,"reason":"B08_KNOWLEDGE_UNBOUND"}),
    ));
    kept["normalization"]["heldBack"] = json!(held);
    Ok((kept, extraction))
}

/// One model call of a craft prose step: suffix, capability, profile and
/// input to the parsed reply.
type CraftStep<'a> =
    dyn FnMut(&str, &str, &ResolvedProviderProfile, Value) -> Result<Value, String> + 'a;

/// QC-1.3: how many new threads a chapter may open.
const MAX_NEW_THREADS: usize = 3;

/// QC-1.3 (owner decision 2026-10-26): a chapter opens at most
/// `MAX_NEW_THREADS` threads, those with the most reasons for later chapters
/// to pay them off (promotion signals for, less those against); the others
/// are held back as candidates for the reminder. Threads already known are
/// advanced or resolved without limit.
fn cap_new_threads(audit: &mut Value, known: &BTreeSet<String>) {
    let Some(items) = audit["continuityCandidates"].as_array_mut() else {
        return;
    };
    let mut new = (0..items.len())
        .filter(|index| {
            items[*index]["threadRef"]
                .as_str()
                .is_some_and(|thread| !known.contains(thread))
        })
        .collect::<Vec<_>>();
    new.sort_by_cached_key(|index| {
        -serde_json::from_value::<crate::narrative::PromotionSignal>(
            items[*index]["promotionSignals"].clone(),
        )
        .unwrap_or_default()
        .weight()
    });
    let over = new.split_off(new.len().min(MAX_NEW_THREADS));
    if over.is_empty() {
        return;
    }
    let (kept, held): (Vec<_>, Vec<_>) = std::mem::take(items)
        .into_iter()
        .enumerate()
        .partition(|(index, _)| !over.contains(index));
    *items = kept.into_iter().map(|(_, item)| item).collect();
    let held = held.into_iter().map(|(_, item)| {
        json!({"field":"continuityCandidates","item":item,"reason":"QC1_THREAD_OVER_LIMIT"})
    });
    let list = &mut audit["normalization"]["heldBack"];
    if !list.is_array() {
        *list = json!([]);
    }
    if let Some(list) = list.as_array_mut() {
        list.extend(held);
    }
}

/// Why an audit item was held back, for the reader of the reminder.
fn held_back_reason(code: &str) -> &'static str {
    match code {
        "QC1_CONTRADICTION" => "改写后这句仍和前面章节的内容矛盾，请确认是否要改",
        "QC1_CATCHPHRASE" => "改写后这个口头禅在本章仍出现不止一次",
        "QC1_THREAD_OVER_LIMIT" => {
            "本章新开的伏笔超过 3 条，这条重要性靠后，只作为候选，没有记入档案"
        }
        "B08_KNOWLEDGE_UNBOUND" => "这条认知指向的事实没有记入档案",
        code if code.starts_with("B08_CANON_CONFLICT") => "与已确认的事实冲突",
        code if code.starts_with("B08_CANON_SEMANTICS") || code.starts_with("B08_CANON_TIME") => {
            "按设定规则无法判断这条事实的类别、时间或范围"
        }
        code if code.starts_with("B08_KNOWLEDGE")
            || code.starts_with("B08_AUDIT_INCOMPLETE_KNOWLEDGE") =>
        {
            "人物何时、在什么范围内知道这件事不清楚"
        }
        code if code.starts_with("B08_THREAD") => "剧情线索的推进不在本章计划内，或推进方式不对",
        code if code.starts_with("B08_TASK") => "任务进度与本卷计划不符",
        code if code.starts_with("B08_RESERVATION") => "提前揭示了后续章节预留的内容",
        code if code.starts_with("B08_AUDIT_UNRESOLVED_HARD") => "审计自己报告有内容没能整理完",
        code if code.contains("EVIDENCE") || code.contains("QUOTE") => "引用的原文在正文里找不到",
        _ => "没有通过作品档案检查",
    }
}

/// Reminder entries for the plan problems a chapter was written with: each
/// listed craft problem on its own, any other problem by its instruction.
fn plan_reminder_items(problems: &[String]) -> Vec<Value> {
    problems
        .iter()
        .filter_map(|problem| crate::chapter_craft::plan_feedback(problem))
        .flat_map(|feedback| {
            let reason = feedback["rejectedFor"].clone();
            let items = match feedback["problems"].as_array() {
                Some(problems) => problems.clone(),
                None => vec![feedback["instruction"].clone()],
            };
            items
                .into_iter()
                .map(move |item| json!({"field":"plan","item":item,"reason":reason}))
        })
        .collect()
}

/// The reminder committed with a plan about the problems its planner could
/// not fix, if any.
pub(crate) fn plan_reminder_decision(
    command_id: &str,
    chapter_id: &str,
    chapter_number: Option<u32>,
    problems: &[String],
) -> Result<Vec<Value>, String> {
    let items = plan_reminder_items(problems);
    if items.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![audit_reminder(
        command_id,
        chapter_id,
        chapter_number,
        &items,
    )?])
}

/// The chapter's one reminder about audit items that were not recorded.
fn audit_reminder(
    final_id: &str,
    chapter_id: &str,
    chapter_number: Option<u32>,
    held: &[Value],
) -> Result<Value, String> {
    let field_label = |field: &str| match field {
        "canonCandidates" => "事实",
        "knowledgeCandidates" => "人物认知",
        "continuityCandidates" => "剧情线索",
        "taskProgressCandidates" => "任务进度",
        "revealedReservationRefs" => "后续预留",
        _ => "审计",
    };
    let lines = held
        .iter()
        .map(|entry| {
            if entry["field"] == "plan" {
                return format!("计划：{}", entry["item"].as_str().unwrap_or(""));
            }
            let quote = entry
                .pointer("/item/evidence/quote")
                .and_then(Value::as_str)
                .map(|quote| format!("「{quote}」"))
                .unwrap_or_default();
            format!(
                "{}{quote}：{}",
                field_label(entry["field"].as_str().unwrap_or("")),
                held_back_reason(entry["reason"].as_str().unwrap_or(""))
            )
        })
        .collect::<Vec<_>>();
    let chapter = chapter_number.map_or_else(|| "本章".to_owned(), |n| format!("第 {n} 章"));
    let decision_id = StableIdentityFactory::from_command(
        StableIdKind::Decision,
        &format!("ld2-audit-reminder:{final_id}"),
    )
    .map_err(|error| error.to_string())?;
    Ok(json!({
        "decisionId":decision_id,
        "decisionType":"AUDIT_REMINDER",
        "outcome":"OPEN",
        "reason":lines.join("；"),
        "sourceRef":chapter_id,
        "payloadJson":{
            "category":if held.iter().all(|entry| entry["field"] == "plan") { "计划提醒" } else { "审计提醒" },
            "title":format!("{chapter}有 {} 条内容需要你留意", held.len()),
            "chapterId":chapter_id,
            "heldBack":held,
            "allowedActions":["ACKNOWLEDGE"]
        }
    }))
}

/// LD-1.2: the system names everything new an audit reports, so a model's
/// own labels never collide with earlier chapters (on 2026-10-25 chapter 4's
/// `know-1` overwrote chapter 1's, and every chapter's audit restarted at
/// `thread-1`). New facts become `canon:{chapter digest}:{n}` and knowledge
/// `know:{chapter digest}:{n}`; knowledge names its fact by `factIndex` (or
/// the model's own fact label) and a continuity candidate its source event by
/// `sourceFactIndex` (or `event:{label}`). A thread ref that names a thread
/// in `known` (with or without `thread:`) keeps it; any other is a new thread
/// `thread:{chapter digest}:{n}`. Returns the labels that were replaced.
fn assign_audit_ids(raw: &mut Value, chapter_id: &str, known: &BTreeSet<String>) -> Value {
    fn items<'a>(raw: &'a mut Value, array: &str) -> impl Iterator<Item = &'a mut Value> {
        raw.get_mut(array)
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
    }
    let mut fact_ids = Vec::new();
    let mut by_label = BTreeMap::<String, String>::new();
    for (index, fact) in items(raw, "canonCandidates").enumerate() {
        let id = new_audit_item_id("canon", chapter_id, index + 1);
        if let Some(label) = fact
            .get("factId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
        {
            by_label.entry(label.clone()).or_insert_with(|| id.clone());
            // A plan may name the fact by the model's label (CANON_FACT_MATCH).
            if fact.get("modelLabel").is_none() {
                fact["modelLabel"] = json!(label);
            }
        }
        fact["factId"] = json!(id);
        fact_ids.push(id);
    }
    // An index is used once: after it the fact is named by its id, so a later
    // pass (after candidates were dropped) follows the fact, not the position.
    let fact_at = |item: &mut Value, index_key: &str, label: Option<String>| {
        let index = item
            .as_object_mut()
            .and_then(|item| item.remove(index_key))
            .and_then(|index| index.as_u64());
        index
            .and_then(|index| fact_ids.get(index as usize).cloned())
            .or_else(|| label.and_then(|label| by_label.get(&label).cloned()))
    };
    for (index, item) in items(raw, "knowledgeCandidates").enumerate() {
        item["knowledgeId"] = json!(new_audit_item_id("know", chapter_id, index + 1));
        let label = item
            .get("factId")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let fact = fact_at(item, "factIndex", label);
        if let Some(fact) = fact {
            item["factId"] = json!(fact);
        }
    }
    let mut new_threads = Vec::new();
    let mut thread_by_label = BTreeMap::<String, String>::new();
    for item in items(raw, "continuityCandidates") {
        let event_label = item
            .get("sourceEventId")
            .and_then(Value::as_str)
            .and_then(|event| event.strip_prefix("event:"))
            .map(ToOwned::to_owned);
        if let Some(fact) = fact_at(item, "sourceFactIndex", event_label) {
            item["sourceEventId"] = json!(format!("event:{fact}"));
        }
        let given = item.get("threadRef").and_then(Value::as_str).unwrap_or("");
        let prefixed = format!("thread:{given}");
        let thread_ref = if known.contains(given) {
            given.to_owned()
        } else if known.contains(&prefixed) {
            prefixed
        } else if let Some(id) = thread_by_label.get(given).filter(|_| !given.is_empty()) {
            id.clone()
        } else {
            let id = new_audit_item_id("thread", chapter_id, new_threads.len() + 1);
            new_threads.push(json!({"from":given,"to":id}));
            thread_by_label.insert(given.to_owned(), id.clone());
            id
        };
        item["threadRef"] = json!(thread_ref);
    }
    json!({"factLabels":by_label,"newThreads":new_threads})
}

/// Owner decision (2026-10-18): a quote that left words out of one body
/// passage binds to that whole passage when its characters occur there in
/// order, cover at least two thirds of it, and no other passage matches.
fn omission_unique_span(body: &[char], quote: &str) -> Option<(usize, usize)> {
    let needle = quote
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<Vec<_>>();
    if needle.len() < 8 {
        return None;
    }
    let limit = needle.len() * 3 / 2;
    let mut spans = Vec::new();
    for start in (0..body.len()).filter(|&index| body[index] == needle[0]) {
        let (mut matched, mut length, mut index) = (0, 0, start);
        while index < body.len() && matched < needle.len() && length <= limit {
            if !body[index].is_whitespace() {
                length += 1;
                if body[index] == needle[matched] {
                    matched += 1;
                }
            }
            index += 1;
        }
        if matched == needle.len() && length <= limit {
            spans.push((start, index));
        }
    }
    // Keep only the tightest spans; one must remain.
    let tight = spans
        .iter()
        .filter(|outer| {
            !spans
                .iter()
                .any(|inner| inner != *outer && outer.0 <= inner.0 && inner.1 <= outer.1)
        })
        .copied()
        .collect::<Vec<_>>();
    match tight.as_slice() {
        [span] => Some(*span),
        _ => None,
    }
}

/// Whether `quote` identifies exactly one passage of `body`.
/// Owner decision (2026-10-18): after the audit rerun, candidates whose
/// quote still identifies no single body passage are dropped and named, as
/// facts without evidence are; evidence ranges that fail the same way go.
fn drop_unlocatable_audit_candidates(raw: &Value, body: &str) -> (Value, Value) {
    let unlocatable = |quote: Option<&str>| {
        quote.is_some_and(|quote| !quote.is_empty() && locate_quote_span(body, quote).is_none())
    };
    let mut kept = raw.clone();
    bind_cited_evidence(&mut kept, body);
    let mut dropped = Vec::new();
    for (field, id_key) in [
        ("canonCandidates", "factId"),
        ("knowledgeCandidates", "knowledgeId"),
        ("continuityCandidates", "threadRef"),
        ("taskProgressCandidates", "taskId"),
    ] {
        if let Some(items) = kept.get_mut(field).and_then(Value::as_array_mut) {
            items.retain(|item| {
                let quote = item.pointer("/evidence/quote").and_then(Value::as_str);
                // QC-1.1: evidence `bind_cited_evidence` unbound.
                let unbound = item["evidence"].get("unbound").is_some();
                if !unbound && !unlocatable(quote) {
                    return true;
                }
                dropped.push(json!({"field":field,"id":item.get(id_key),"label":item.get("modelLabel"),"quote":quote}));
                false
            });
        }
    }
    if let Some(ranges) = kept.get_mut("evidenceRanges").and_then(Value::as_array_mut) {
        ranges.retain(|range| !unlocatable(range.get("quote").and_then(Value::as_str)));
    }
    (kept, json!(dropped))
}

fn exact_quote(body: &str, evidence: &EvidenceSpan) -> Result<(), String> {
    validate_candidate_evidence_range(body, evidence.start, evidence.end)?;
    let actual = body
        .chars()
        .skip(evidence.start)
        .take(evidence.end - evidence.start)
        .collect::<String>();
    if actual != evidence.quote {
        return Err("B08_EVIDENCE_QUOTE_MISMATCH".into());
    }
    Ok(())
}

/// Where a verbatim quote occurs exactly once in `body` (ignoring only
/// whitespace), as character offsets; `None` when absent or ambiguous.
pub fn locate_unique_quote(body: &str, quote: &str) -> Option<(usize, usize)> {
    if quote.is_empty() {
        return None;
    }
    let matches = body.match_indices(quote).take(2).collect::<Vec<_>>();
    if matches.len() == 1 {
        let start = body[..matches[0].0].chars().count();
        return Some((start, start + quote.chars().count()));
    }
    if !matches.is_empty() {
        return None;
    }
    whitespace_insensitive_unique_span(&body.chars().collect::<Vec<_>>(), quote)
}

/// QC-1.1: the sentences of `body` the audit cites by number (their index),
/// as character spans with their text.
pub(crate) fn body_sentences(body: &str) -> Vec<(usize, usize, String)> {
    crate::chapter_craft::sentence_spans(body)
        .into_iter()
        .map(|(start, end)| {
            let from = body[..start].chars().count();
            let text = body[start..end].to_owned();
            (from, from + text.chars().count(), text)
        })
        .collect()
}

/// Characters too common to tie a passage to a fact.
const COMMON_CHARACTERS: &str =
    "的了着过在是有不没也都就还又和与及把被给让向对从到为他她它我你们这那个一上下里中正很再已";

/// The pairs of adjacent Chinese characters in `text` that can tie a
/// passage to it.
fn telling_pairs(text: &str) -> BTreeSet<(char, char)> {
    let chars = text.chars().collect::<Vec<_>>();
    chars
        .windows(2)
        .filter(|pair| {
            pair.iter()
                .all(|c| ('\u{4e00}'..='\u{9fff}').contains(c) && !COMMON_CHARACTERS.contains(*c))
        })
        .map(|pair| (pair[0], pair[1]))
        .collect()
}

/// What an audit item says, to check the passage cited for it: a fact's
/// values, else the names it is about; a knowledge item, its fact's. Empty
/// when the item says nothing a passage can be checked against.
fn audit_item_words(field: &str, item: &Value, facts: &[Value]) -> String {
    let fact_words = |fact: &Value| {
        let mut parts = Vec::new();
        crate::chapter_craft::collect_strings(&fact["objectJson"], &mut parts);
        let mut words = parts.concat();
        if telling_pairs(&words).is_empty() {
            for key in ["canonicalName", "subjectId"] {
                words.push_str(fact[key].as_str().unwrap_or(""));
            }
        }
        words
    };
    match field {
        "canonCandidates" => fact_words(item),
        "knowledgeCandidates" => item["factIndex"]
            .as_u64()
            .and_then(|index| facts.get(index as usize))
            .map(fact_words)
            .unwrap_or_default(),
        "continuityCandidates" => item["modelLabel"].as_str().unwrap_or("").to_owned(),
        _ => String::new(),
    }
}

/// QC-1.1 (owner decision 2026-10-26): evidence cited by sentence number
/// becomes that sentence, and a quote that only loosely matches is bound to
/// its passage, when the passage or a sentence next to it shares a telling
/// character pair with what the item says. Evidence that fails this is
/// unbound, so the item cannot reach the ledger on a wrong passage; returns
/// what was unbound, for the audit feedback.
pub(crate) fn bind_cited_evidence(raw: &mut Value, body: &str) -> Vec<Value> {
    let sentences = body_sentences(body);
    let facts = raw["canonCandidates"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let near = |start: usize, end: usize| {
        let first = sentences
            .iter()
            .position(|sentence| sentence.1 > start)
            .unwrap_or(sentences.len());
        let last = sentences
            .iter()
            .rposition(|sentence| sentence.0 < end)
            .unwrap_or(first);
        sentences[first.saturating_sub(1)..(last + 2).min(sentences.len())]
            .iter()
            .map(|sentence| sentence.2.as_str())
            .collect::<String>()
    };
    let mut unbound = Vec::new();
    for range in raw
        .get_mut("evidenceRanges")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some((start, end, text)) = range
            .get("sentence")
            .and_then(Value::as_u64)
            .and_then(|number| sentences.get(number as usize))
        {
            *range = json!({"start":start,"end":end,"quote":text,"sentence":range["sentence"]});
        }
    }
    for field in [
        "canonCandidates",
        "knowledgeCandidates",
        "continuityCandidates",
        "taskProgressCandidates",
    ] {
        let Some(items) = raw.get_mut(field).and_then(Value::as_array_mut) else {
            continue;
        };
        for item in items {
            let words = telling_pairs(&audit_item_words(field, item, &facts));
            let about = item_label(item);
            let fits = |start: usize, end: usize| {
                words.is_empty() || !telling_pairs(&near(start, end)).is_disjoint(&words)
            };
            let Some(evidence) = item.get_mut("evidence").filter(|value| value.is_object()) else {
                continue;
            };
            if let Some(number) = evidence.get("sentence").and_then(Value::as_u64) {
                if let Some((start, end, text)) = sentences
                    .get(number as usize)
                    .filter(|(start, end, _)| fits(*start, *end))
                {
                    *evidence = json!({"start":start,"end":end,"quote":text,"sentence":number});
                } else {
                    unbound.push(json!({"field":field,"sentence":number,"about":about}));
                    *evidence = json!({"unbound":number});
                }
                continue;
            }
            let Some(quote) = evidence.get("quote").and_then(Value::as_str) else {
                continue;
            };
            if locate_unique_quote(body, quote).is_some() {
                continue;
            }
            if let Some((start, end)) = locate_quote_span(body, quote) {
                if !fits(start, end) {
                    unbound.push(json!({"field":field,"quote":quote,"about":about}));
                    *evidence = json!({"unbound":quote});
                }
            }
        }
    }
    unbound
}

/// A short name for an audit item in feedback and reminders.
fn item_label(item: &Value) -> Value {
    ["modelLabel", "factId", "knowledgeId", "taskId", "threadRef"]
        .iter()
        .find_map(|key| item.get(*key))
        .cloned()
        .unwrap_or(Value::Null)
}

/// Where a quote from a model stands in `body`, in characters: verbatim
/// and unique, or else the one passage it loosely names (lost whitespace, an
/// ellipsis, left-out words, or LD-1.3 a few changed characters).
pub fn locate_quote_span(body: &str, quote: &str) -> Option<(usize, usize)> {
    if let Some(span) = locate_unique_quote(body, quote) {
        return Some(span);
    }
    if body.contains(quote) {
        return None;
    }
    let body_chars = body.chars().collect::<Vec<_>>();
    bounded_ellipsis_span(body, quote)
        .or_else(|| omission_unique_span(&body_chars, quote))
        .or_else(|| near_unique_span(&body_chars, quote))
}

/// Shortest quote matched loosely by its characters (a tenth of it, one
/// character, may differ).
const NEAR_QUOTE_MIN_CHARS: usize = 10;

/// LD-1.3 (real signing evaluation 2026-10-25: 他→沈砚, 攥着→攥紧): the one
/// passage of `body` the quote differs from by at most a tenth of its
/// characters (edits: change, add or drop a character). Nothing when the
/// quote is short, too far off, or as close to another passage.
fn near_unique_span(body: &[char], quote: &str) -> Option<(usize, usize)> {
    let quote = quote.chars().collect::<Vec<_>>();
    if quote.len() < NEAR_QUOTE_MIN_CHARS {
        return None;
    }
    let allowed = quote.len() / 10;
    // Approximate substring search: row[j] is the fewest edits turning the
    // quote so far into a passage ending before body[j]; starts[j] where it
    // begins.
    let mut row = vec![0_usize; body.len() + 1];
    let mut starts = (0..=body.len()).collect::<Vec<_>>();
    for (i, quote_char) in quote.iter().enumerate() {
        let mut next = vec![i + 1; body.len() + 1];
        let mut next_starts = vec![0_usize; body.len() + 1];
        for j in 1..=body.len() {
            let replace = row[j - 1] + usize::from(*quote_char != body[j - 1]);
            let skip_quote = row[j] + 1;
            let skip_body = next[j - 1] + 1;
            // On a tie, drop a quote character rather than change a body
            // one, so the passage does not take in the character before it.
            (next[j], next_starts[j]) = if skip_quote <= replace && skip_quote <= skip_body {
                (skip_quote, starts[j])
            } else if replace <= skip_body {
                (replace, starts[j - 1])
            } else {
                (skip_body, next_starts[j - 1])
            };
        }
        row = next;
        starts = next_starts;
    }
    let best = (1..=body.len()).min_by_key(|&end| row[end])?;
    if row[best] > allowed {
        return None;
    }
    // Another passage as close (ending away from this one) makes it
    // ambiguous; for a longer quote, so does one only an edit further off.
    let margin = usize::from(allowed >= 2);
    let elsewhere = (1..=body.len())
        .any(|end| row[end] <= row[best] + margin && end.abs_diff(best) > quote.len() / 2);
    if elsewhere {
        return None;
    }
    // Equal edits can start the passage a character early or late (沈砚 is
    // as close to 。他 as to 他); a quote is a whole clause, so its start is
    // moved to the clause boundary, within the edits allowed.
    let boundary = |c: char| c.is_whitespace() || "。！？，、；：…—“”‘’「」『』（）".contains(c);
    let (mut start, mut end) = (starts[best], best);
    while start < end && boundary(body[start]) && quote[0] != body[start] {
        start += 1;
    }
    // Only a start the quote changed is widened (a quote that merely begins
    // mid-clause keeps its start).
    let mut widened = 0;
    while start > 0 && body[start] != quote[0] && !boundary(body[start - 1]) && widened < allowed {
        start -= 1;
        widened += 1;
    }
    while end > start && body[end - 1].is_whitespace() {
        end -= 1;
    }
    Some((start, end))
}

pub fn exact_quote_span(body: &str, start: usize, end: usize, quote: &str) -> Result<(), String> {
    exact_quote(
        body,
        &EvidenceSpan {
            start,
            end,
            quote: quote.to_owned(),
            rebound: false,
        },
    )
}

fn validate_actual_extraction(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    chapter_id: &str,
    body: &str,
    plan: &Value,
    gate_policy: &Value,
    audit: &mut AuditExtraction,
) -> Result<(), String> {
    let canon_semantics = if audit.canon_candidates.is_empty() {
        None
    } else {
        Some(
            crate::canon_semantics::CanonSemanticPolicy::from_gate_policy(gate_policy)
                .map_err(str::to_owned)?,
        )
    };
    if audit.canon_candidates.len() > 100 {
        return Err("B08_CANON_SEMANTICS_UNRESOLVED_TOO_MANY_CANDIDATES".into());
    }
    let mut pending_canon_facts = BTreeMap::<
        (String, String),
        Vec<crate::repositories::narrative::SemanticCanonFactRecord>,
    >::new();
    for evidence in &audit.evidence_ranges {
        exact_quote(body, evidence)?;
    }
    let active_tasks = plan
        .get("activeTaskRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let continuity = planned_continuity_refs(plan);
    let reservations = plan
        .get("reservationRefs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let allowed_reveals = plan
        .get("sceneExecution")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("action").and_then(Value::as_str) == Some("REVEAL"))
        .filter_map(|item| item.get("reservationRef").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    let current_chapter_no = crate::repositories::narrative::NarrativeRepository::chapter_number(
        connection, project_id, chapter_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or("B08_AUDIT_INCOMPLETE_KNOWLEDGE_TIME")?;

    let mut canon_transitions = BTreeMap::<String, String>::new();
    let mut duplicate_transitions = BTreeMap::<String, Vec<String>>::new();
    // FX-19: process Canon candidates in reading order so a later value of a
    // single-valued fact in this chapter can replace an earlier one.
    audit
        .canon_candidates
        .sort_by_key(|candidate| candidate.evidence.start);
    let mut pending_evidence = BTreeMap::<String, (usize, usize)>::new();
    for candidate in &audit.canon_candidates {
        canon_semantics
            .as_ref()
            .expect("canon semantics is required for Canon candidates")
            .validate_predicate(&candidate.predicate)?;
        exact_quote(body, &candidate.evidence)?;
        let subject_exists =
            crate::repositories::narrative::NarrativeRepository::canon_entity_exists(
                connection,
                project_id,
                &candidate.subject_id,
            )
            .map_err(|error| error.to_string())?;
        let subject_created_in_commit =
            candidate.entity_id.as_deref() == Some(candidate.subject_id.as_str());
        if !subject_exists && !subject_created_in_commit {
            return Err("B08_CANON_SUBJECT_NOT_FOUND".into());
        }
        let (current, truncated) = crate::repositories::narrative::NarrativeRepository::current_canon_facts_for_semantic_key_bounded(
            connection,
            project_id,
            &candidate.subject_id,
            &candidate.predicate,
            100,
        )
        .map_err(|error| error.to_string())?;
        if truncated {
            return Err("B08_CANON_SEMANTICS_UNRESOLVED_TOO_MANY_FACTS".into());
        }
        let candidate_from_chapter_no =
            crate::repositories::narrative::NarrativeRepository::chapter_number(
                connection,
                project_id,
                candidate
                    .valid_from_chapter_id
                    .as_deref()
                    .unwrap_or(chapter_id),
            )
            .map_err(|error| error.to_string())?;
        let candidate_to_chapter_no =
            crate::repositories::narrative::NarrativeRepository::chapter_number(
                connection,
                project_id,
                candidate
                    .valid_to_chapter_id
                    .as_deref()
                    .unwrap_or(chapter_id),
            )
            .map_err(|error| error.to_string())?;
        let semantic_key = (candidate.subject_id.clone(), candidate.predicate.clone());
        let policy = canon_semantics
            .as_ref()
            .expect("canon semantics is required for Canon candidates");
        let candidate_input = crate::canon_semantics::CandidateSemanticInput {
            object_json: &candidate.object_json,
            story_time: candidate.story_time.as_deref(),
            valid_from_chapter_no: candidate_from_chapter_no,
            valid_to_chapter_no: candidate_to_chapter_no,
        };
        let ended = canon_transitions
            .values()
            .chain(duplicate_transitions.values().flatten())
            .cloned()
            .collect::<BTreeSet<_>>();
        let current = current
            .into_iter()
            .filter(|fact| !ended.contains(&fact.fact_id))
            .collect::<Vec<_>>();
        let mut replaced = Vec::new();
        for fact in pending_canon_facts.get(&semantic_key).into_iter().flatten() {
            let earlier_ends_first = pending_evidence
                .get(&fact.fact_id)
                .is_some_and(|(_, end)| *end <= candidate.evidence.start);
            if !ended.contains(&fact.fact_id)
                && earlier_ends_first
                && policy.in_chapter_transition(
                    &candidate.predicate,
                    fact,
                    candidate_input.clone(),
                )?
            {
                replaced.push(fact.fact_id.clone());
            }
        }
        for fact in &current {
            if fact.fact_id != candidate.fact_id
                && policy.narrative_transition(
                    &candidate.predicate,
                    fact,
                    candidate_input.clone(),
                )?
            {
                replaced.push(fact.fact_id.clone());
            }
        }
        if replaced.len() > 1 {
            // Equal open values (a restated fact) are one state and are all
            // replaced together; different values stay ambiguous.
            let object_of = |fact_id: &String| {
                current
                    .iter()
                    .chain(pending_canon_facts.get(&semantic_key).into_iter().flatten())
                    .find(|fact| &fact.fact_id == fact_id)
                    .map(|fact| fact.object_json.clone())
            };
            let first = object_of(&replaced[0]);
            if first.is_none() || replaced.iter().any(|fact_id| object_of(fact_id) != first) {
                return Err("B08_CANON_TIME_UNRESOLVED".into());
            }
            duplicate_transitions.insert(candidate.fact_id.clone(), replaced[1..].to_vec());
        }
        if let Some(old_fact_id) = replaced.first() {
            canon_transitions.insert(candidate.fact_id.clone(), old_fact_id.clone());
        }
        for fact in current
            .iter()
            .chain(pending_canon_facts.get(&semantic_key).into_iter().flatten())
            .filter(|fact| !replaced.contains(&fact.fact_id) && !ended.contains(&fact.fact_id))
        {
            if let Some(error) = policy
                .evaluate(&candidate.predicate, fact, candidate_input.clone())
                .error_code()
            {
                return Err(error.into());
            }
        }
        pending_evidence.insert(
            candidate.fact_id.clone(),
            (candidate.evidence.start, candidate.evidence.end),
        );
        pending_canon_facts.entry(semantic_key).or_default().push(
            crate::repositories::narrative::SemanticCanonFactRecord {
                fact_id: candidate.fact_id.clone(),
                object_json: candidate.object_json.clone(),
                valid_from_chapter_id: candidate.valid_from_chapter_id.clone(),
                valid_to_chapter_id: candidate.valid_to_chapter_id.clone(),
                story_time_from: candidate.story_time.clone(),
                story_time_to: candidate.story_time.clone(),
                valid_from_chapter_no: candidate_from_chapter_no,
                valid_to_chapter_no: candidate_to_chapter_no,
            },
        );
    }
    audit.canon_transitions = canon_transitions;
    audit.canon_duplicate_transitions = duplicate_transitions;
    let mut downgraded = Vec::new();
    // Completion predicates read the rest of this audit (its canon).
    let canon_view = audit.clone();
    for candidate in &mut audit.task_progress_candidates {
        exact_quote(body, &candidate.evidence)?;
        if !active_tasks.contains(candidate.task_id.as_str()) {
            return Err("B08_TASK_SCOPE_VIOLATION".into());
        }
        if !matches!(
            candidate.status.as_str(),
            "NOT_STARTED" | "ACTIVE" | "SATISFIED" | "BLOCKED" | "INVALIDATED"
        ) {
            return Err("B08_TASK_STATUS_INVALID".into());
        }
        // The volume plan's predicate decides completion: an audit claim it
        // does not support only records progress (2026-10-19).
        if candidate.status == "SATISFIED" {
            if let Err(error) = validate_task_completion_claim(
                connection,
                project_id,
                chapter_id,
                candidate,
                &canon_view,
            ) {
                if error != "B08_TASK_COMPLETION_PREDICATE_UNSATISFIED" {
                    return Err(error);
                }
                candidate.status = "ACTIVE".into();
                downgraded
                    .push(json!({"taskId":candidate.task_id,"claimed":"SATISFIED","reason":error}));
            }
        }
    }
    audit.downgraded_task_claims = downgraded;
    for candidate in &audit.continuity_candidates {
        let planned = continuity.contains(candidate.thread_ref.as_str());
        if let Some(evidence) = &candidate.evidence {
            exact_quote(body, evidence)?;
        } else if planned {
            return Err("B08_AUDIT_INCOMPLETE".into());
        }
        if let Some(source_event_id) = candidate.source_event_id.as_deref() {
            let event_in_audit = audit.canon_candidates.iter().any(|item| {
                item.event_type.is_some() && source_event_id == format!("event:{}", item.fact_id)
            });
            let event_already_committed =
                crate::repositories::narrative::NarrativeRepository::canon_event_exists(
                    connection,
                    project_id,
                    Some(source_event_id),
                    None,
                )
                .map_err(|error| error.to_string())?;
            if !event_in_audit && !event_already_committed {
                return Err("B08_THREAD_SOURCE_EVENT_INVALID".into());
            }
        }
        if !matches!(
            candidate.disposition.as_str(),
            "ADVANCE" | "RESOLVE" | "DEFER_WITH_BRIDGE" | "NO_LONGER_APPLICABLE_CHANGE_REQUEST"
        ) {
            return Err("B08_THREAD_DISPOSITION_INVALID".into());
        }
        if !planned {
            let authority_id = candidate
                .thread_ref
                .strip_prefix("thread:")
                .ok_or("B08_THREAD_DISPOSITION_INVALID")?;
            if crate::repositories::narrative::NarrativeRepository::continuity_thread_exists(
                connection,
                project_id,
                authority_id,
            )
            .map_err(|error| error.to_string())?
            {
                return Err("B08_THREAD_TRANSITION_OUTSIDE_PLAN".into());
            }
        }
        if candidate.disposition == "DEFER_WITH_BRIDGE" {
            let bridge = candidate
                .bridge
                .as_deref()
                .or_else(|| {
                    plan.get("continuityActions")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .find(|action| {
                            action.get("threadRef").and_then(Value::as_str)
                                == Some(candidate.thread_ref.as_str())
                        })
                        .and_then(|action| action.get("bridge"))
                        .and_then(Value::as_str)
                })
                .filter(|bridge| !bridge.trim().is_empty());
            if bridge.is_none() {
                return Err("B08_THREAD_DISPOSITION_INVALID".into());
            }
        }
    }
    for reference in &audit.revealed_reservation_refs {
        if !reservations.contains(reference.as_str())
            || !allowed_reveals.contains(reference.as_str())
        {
            return Err("B08_RESERVATION_VIOLATION".into());
        }
    }
    let pending_entities = audit
        .canon_candidates
        .iter()
        .filter_map(|item| item.entity_id.clone())
        .collect::<BTreeSet<_>>();
    let pending_facts = audit
        .canon_candidates
        .iter()
        .map(|item| item.fact_id.clone())
        .collect::<BTreeSet<_>>();
    let knowledge_candidates = std::mem::take(&mut audit.knowledge_candidates);
    let mut bound_knowledge_candidates = Vec::new();
    for mut candidate in knowledge_candidates {
        exact_quote(body, &candidate.evidence)?;
        let knower_exists =
            crate::repositories::narrative::NarrativeRepository::canon_entity_exists(
                connection,
                project_id,
                &candidate.knower_id,
            )
            .map_err(|error| error.to_string())?;
        let fact_exists = crate::repositories::narrative::NarrativeRepository::canon_fact_exists(
            connection,
            project_id,
            &candidate.fact_id,
        )
        .map_err(|error| error.to_string())?;
        if !knowledge_authority_is_bound(
            knower_exists,
            fact_exists,
            &candidate.knower_id,
            &candidate.fact_id,
            &pending_entities,
            &pending_facts,
        ) {
            audit.dropped_unbound_knowledge.push(json!({
                "knowledgeId":candidate.knowledge_id,"knowerId":candidate.knower_id,
                "factId":candidate.fact_id,"reason":"KNOWLEDGE_UNBOUND"
            }));
            continue;
        }
        let pending_fact = audit
            .canon_candidates
            .iter()
            .find(|fact| fact.fact_id == candidate.fact_id);
        let fact_timing = if let Some(pending_fact) = pending_fact {
            Some(crate::repositories::narrative::CanonFactTimingRecord {
                story_time: pending_fact.story_time.clone(),
                source_chapter_no: Some(current_chapter_no),
            })
        } else {
            crate::repositories::narrative::NarrativeRepository::canon_fact_timing(
                connection,
                project_id,
                &candidate.fact_id,
            )
            .map_err(|error| error.to_string())?
        };
        let fact_timing = fact_timing.ok_or("B08_AUDIT_INCOMPLETE_KNOWLEDGE_TIME")?;
        // FX-17 (RV-07: Narrative Order and World Time are separate rules):
        // an explicit UNKNOWN World Time leaves the Narrative Order check below
        // to decide future knowledge; World Time is compared only when both
        // sides state one. A missing acquiredStoryTime or a malformed time
        // still fails closed (FX-4); a fact without storyTime is unknown.
        let world_time = |time: Option<&str>, missing_is_unknown: bool| match time.map(str::trim) {
            Some("UNKNOWN") => Ok(None),
            Some(time) if !time.is_empty() => crate::canon_semantics::parse_world_time(time)
                .map(Some)
                .map_err(|_| "B08_AUDIT_INCOMPLETE_KNOWLEDGE_TIME"),
            _ if missing_is_unknown => Ok(None),
            _ => Err("B08_AUDIT_INCOMPLETE_KNOWLEDGE_TIME"),
        };
        let acquired_time = world_time(candidate.acquired_story_time.as_deref(), false)?;
        let fact_time = world_time(fact_timing.story_time.as_deref(), true)?;
        let world_times = acquired_time.zip(fact_time);
        if fact_timing
            .source_chapter_no
            .is_some_and(|source| source > current_chapter_no)
        {
            return Err("B08_KNOWLEDGE_FUTURE_EVENT_CONTRADICTION".into());
        }
        if candidate
            .visibility_scope
            .as_deref()
            .is_none_or(|scope| scope.trim().is_empty() || scope == "UNKNOWN")
        {
            return Err("B08_AUDIT_INCOMPLETE_KNOWLEDGE_TIME".into());
        }
        if candidate.visibility_scope.as_deref() == Some("PRIVATE") {
            candidate.visibility_scope = Some(format!("CHARACTER:{}", candidate.knower_id));
        }
        let visibility_scope = candidate.visibility_scope.as_deref().unwrap_or_default();
        let valid_visibility_scope = visibility_scope == "PUBLIC"
            || visibility_scope.strip_prefix("CHARACTER:") == Some(candidate.knower_id.as_str())
            || if let Some(visible_chapter) = visibility_scope.strip_prefix("CHAPTER:") {
                crate::repositories::narrative::NarrativeRepository::chapter_number(
                    connection,
                    project_id,
                    visible_chapter,
                )
                .map_err(|error| error.to_string())?
                .is_some()
            } else {
                false
            };
        if !valid_visibility_scope {
            return Err("B08_AUDIT_INCOMPLETE_KNOWLEDGE_VISIBILITY".into());
        }
        if world_times.is_some_and(|(acquired_time, fact_time)| acquired_time < fact_time) {
            return Err("B08_KNOWLEDGE_FUTURE_EVENT_CONTRADICTION".into());
        }
        bound_knowledge_candidates.push(candidate);
    }
    audit.knowledge_candidates = bound_knowledge_candidates;
    Ok(())
}

fn validate_task_completion_claim(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    chapter_id: &str,
    candidate: &TaskProgressCandidate,
    audit: &AuditExtraction,
) -> Result<(), String> {
    let volume_id = crate::repositories::narrative::NarrativeRepository::chapter_volume_id(
        connection, project_id, chapter_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or("B08_TASK_COMPLETION_PREDICATE_UNSATISFIED")?;
    let route = crate::repositories::narrative::NarrativeRepository::artifact_payload(
        connection,
        project_id,
        "VolumePlan",
        &volume_id,
    )
    .map_err(|error| error.to_string())?
    .ok_or("B08_TASK_COMPLETION_PREDICATE_UNSATISFIED")?;
    let task = route
        .get("tasks")
        .and_then(Value::as_array)
        .and_then(|tasks| {
            tasks.iter().find(|task| {
                task.get("taskId").and_then(Value::as_str) == Some(candidate.task_id.as_str())
            })
        })
        .ok_or("B08_TASK_COMPLETION_PREDICATE_UNSATISFIED")?;
    let dependencies = task
        .get("dependsOnTaskIds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    let dependency_states =
        crate::repositories::narrative::NarrativeRepository::task_states_by_ids_complete(
            connection,
            project_id,
            &dependencies,
        )
        .map_err(|error| error.to_string())?;
    if dependencies.iter().any(|dependency| {
        dependency_states
            .iter()
            .find(|state| state.state_id == *dependency)
            .and_then(|state| state.value.get("status").and_then(Value::as_str))
            != Some("SATISFIED")
    }) {
        return Err("B08_TASK_COMPLETION_PREDICATE_UNSATISFIED".into());
    }
    let predicate = task
        .get("completionPredicate")
        .ok_or("B08_TASK_COMPLETION_PREDICATE_UNSATISFIED")?;
    let mut reality = json!({
        "status":candidate.status,
        "evidenceRefs":[{"rangeStart":candidate.evidence.start,"rangeEnd":candidate.evidence.end}],
        "lastAdvancedChapterId":chapter_id
    });
    if predicate.get("kind").and_then(Value::as_str) != Some("TASK_EVIDENCE_RECORDED") {
        reality["predicateSatisfied"] = Value::Bool(b03_completion_predicate_satisfied(
            connection, project_id, predicate, audit,
        )?);
    }
    let satisfied =
        crate::planning::evaluate_b03_completion_predicate(predicate, &candidate.task_id, &reality)
            .map_err(|_| "B08_TASK_COMPLETION_PREDICATE_INVALID")?;
    if !satisfied {
        return Err("B08_TASK_COMPLETION_PREDICATE_UNSATISFIED".into());
    }
    Ok(())
}

fn b03_completion_predicate_satisfied(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    predicate: &Value,
    audit: &AuditExtraction,
) -> Result<bool, String> {
    let value = predicate.get("value").unwrap_or(&Value::Null);
    let result = match predicate.get("kind").and_then(Value::as_str) {
        Some("CANON_FACT_MATCH") => {
            let fact_id = value.get("factId").and_then(Value::as_str);
            let expected = value.get("expected");
            if let (Some(fact_id), Some(expected)) = (fact_id, expected) {
                if let Some(candidate) = audit.canon_candidates.iter().find(|item| {
                    item.fact_id == fact_id || item.model_label.as_deref() == Some(fact_id)
                }) {
                    &candidate.object_json == expected
                } else {
                    crate::repositories::narrative::NarrativeRepository::canon_fact_object_matches(
                        connection, project_id, fact_id, expected,
                    )
                    .map_err(|error| error.to_string())?
                }
            } else {
                false
            }
        }
        Some("CANON_EVENT_EXISTS") => {
            let event_id = value.get("eventId").and_then(Value::as_str);
            let event_type = value.get("eventType").and_then(Value::as_str);
            let present_in_audit = audit.canon_candidates.iter().any(|item| {
                item.event_type.as_deref().is_some_and(|observed| {
                    event_type == Some(observed)
                        || event_id.is_some_and(|id| id == format!("event:{}", item.fact_id))
                })
            });
            present_in_audit
                || crate::repositories::narrative::NarrativeRepository::canon_event_exists(
                    connection, project_id, event_id, event_type,
                )
                .map_err(|error| error.to_string())?
        }
        Some("RELATION_STATE_MATCH") => {
            let relation_id = value.get("relationId").and_then(Value::as_str);
            let expected_transition = value.get("expectedTransition");
            let observed_in_chapter =
                relation_id
                    .zip(expected_transition)
                    .is_some_and(|(relation_id, expected)| {
                        audit.canon_candidates.iter().any(|candidate| {
                            candidate
                                .object_json
                                .get("relationId")
                                .and_then(Value::as_str)
                                == Some(relation_id)
                                && candidate.object_json.get("transition") == Some(expected)
                        })
                    });
            if observed_in_chapter {
                true
            } else if expected_transition.and_then(Value::as_str) == Some("EXISTS") {
                if let Some(relation_id) = relation_id {
                    crate::repositories::narrative::NarrativeRepository::canon_relation_exists(
                        connection,
                        project_id,
                        relation_id,
                    )
                    .map_err(|error| error.to_string())?
                } else {
                    false
                }
            } else {
                false
            }
        }
        Some("KNOWLEDGE_STATE_MATCH") => {
            let knowledge_id = value.get("knowledgeId").and_then(Value::as_str);
            let epistemic_state = value.get("epistemicState").and_then(Value::as_str);
            if let (Some(knowledge_id), Some(epistemic_state)) = (knowledge_id, epistemic_state) {
                audit.knowledge_candidates.iter().any(|item| {
                    item.knowledge_id == knowledge_id && item.epistemic_state == epistemic_state
                }) || crate::repositories::narrative::NarrativeRepository::canon_knowledge_state_matches(
                    connection,
                    project_id,
                    knowledge_id,
                    epistemic_state,
                )
                .map_err(|error| error.to_string())?
            } else {
                false
            }
        }
        Some("FUTURE_RESERVATION_STATUS") => {
            let reservation_id = value.get("reservationId").and_then(Value::as_str);
            let required_status = value.get("requiredStatus").and_then(Value::as_str);
            matches!((reservation_id, required_status), (Some(id), Some("FULFILLED" | "REVEALED")) if audit.revealed_reservation_refs.iter().any(|reference| reference == id))
        }
        Some("OBLIGATION_STATUS") => {
            let obligation_id = value.get("obligationId").and_then(Value::as_str);
            let required_status = value.get("requiredStatus").and_then(Value::as_str);
            if let (Some(obligation_id), Some(required_status)) = (obligation_id, required_status) {
                let observed = audit.continuity_candidates.iter().find_map(|item| {
                    (item.thread_ref == format!("thread:{obligation_id}")).then_some({
                        match item.disposition.as_str() {
                            "RESOLVE" => "RESOLVED",
                            "ADVANCE" | "DEFER_WITH_BRIDGE" => "PROGRESSING",
                            _ => "",
                        }
                    })
                });
                if observed == Some(required_status) {
                    true
                } else {
                    crate::repositories::narrative::NarrativeRepository::obligation_lifecycle_matches(
                        connection,
                        project_id,
                        obligation_id,
                        required_status,
                    )
                    .map_err(|error| error.to_string())?
                }
            } else {
                false
            }
        }
        Some("TASK_EVIDENCE_RECORDED") | None => false,
        Some(_) => false,
    };
    Ok(result)
}

fn knowledge_authority_is_bound(
    knower_exists: bool,
    fact_exists: bool,
    knower_id: &str,
    fact_id: &str,
    pending_entities: &BTreeSet<String>,
    pending_facts: &BTreeSet<String>,
) -> bool {
    (knower_exists || pending_entities.contains(knower_id))
        && (fact_exists || pending_facts.contains(fact_id))
}

#[cfg(test)]
mod tests {
    #[test]
    fn fx18_configured_provider_enforces_the_chapter_length_range() {
        let paths = crate::project_db::paths::AppPaths::new(std::env::temp_dir());
        assert!(super::ConfiguredProductionGateway::for_desktop(paths).enforces_chapter_length());
    }

    use super::*;

    #[test]
    fn fx5_classify_plan_reality_table() {
        let task = |task_id: &str,
                    class: &str,
                    status: &str,
                    estimated_min_cost: i64,
                    expected: Option<&str>| ReconciliationTaskReality {
            task_id: task_id.into(),
            class: class.into(),
            status: status.into(),
            estimated_min_cost,
            expected: expected.map(ToOwned::to_owned),
        };
        let facts =
            |task_reality, remaining_chapter_capacity, canon_gate_passed| ReconciliationFacts {
                task_reality,
                remaining_chapter_capacity,
                canon_gate_passed,
                responsibility_reality: vec![],
            };
        let cases = [
            (
                "optional unfinished at zero capacity",
                facts(
                    vec![task("task:optional", "OPTIONAL", "ACTIVE", 5, None)],
                    0,
                    true,
                ),
                "NO_ACTION",
            ),
            (
                "flexible unfinished at zero capacity",
                facts(
                    vec![task("task:flexible", "FLEXIBLE", "ACTIVE", 5, None)],
                    0,
                    true,
                ),
                "NO_ACTION",
            ),
            (
                "reserved unfinished at zero capacity",
                facts(
                    vec![task("task:reserved", "RESERVED", "ACTIVE", 5, None)],
                    0,
                    true,
                ),
                "NO_ACTION",
            ),
            (
                "required cost exceeds remaining capacity",
                facts(
                    vec![task("task:required", "REQUIRED", "ACTIVE", 5, None)],
                    3,
                    true,
                ),
                "REPLAN_VOLUME_ROUTE",
            ),
            (
                "climax cost exceeds remaining capacity",
                facts(
                    vec![task("task:climax", "CLIMAX", "ACTIVE", 5, None)],
                    3,
                    true,
                ),
                "REPLAN_VOLUME_ROUTE",
            ),
            (
                "required invalidated",
                facts(
                    vec![task("task:required", "REQUIRED", "INVALIDATED", 1, None)],
                    3,
                    true,
                ),
                "REPLAN_VOLUME_ROUTE",
            ),
            (
                "required blocked and over capacity",
                facts(
                    vec![task("task:required", "REQUIRED", "BLOCKED", 5, None)],
                    3,
                    true,
                ),
                "REPLAN_VOLUME_ROUTE",
            ),
            (
                "required blocked with enough capacity",
                facts(
                    vec![task("task:required", "REQUIRED", "BLOCKED", 1, None)],
                    3,
                    true,
                ),
                "RECOMPILE_ROLLING_WINDOW",
            ),
            (
                "canon gate failed",
                facts(vec![], 3, false),
                "CHANGE_REQUEST",
            ),
            (
                "actual differs from planned expected state",
                facts(
                    vec![task(
                        "task:required",
                        "REQUIRED",
                        "ACTIVE",
                        1,
                        Some("TASK_SATISFIED"),
                    )],
                    3,
                    true,
                ),
                "RECOMPILE_NEXT_CHAPTER",
            ),
            (
                "all task expectations satisfied",
                facts(
                    vec![task(
                        "task:required",
                        "REQUIRED",
                        "SATISFIED",
                        1,
                        Some("TASK_SATISFIED"),
                    )],
                    0,
                    true,
                ),
                "NO_ACTION",
            ),
        ];

        for (scenario, facts, expected) in cases {
            assert_eq!(classify_plan_reality(&facts), expected, "{scenario}");
        }
    }

    fn synthetic_audit_policy_contract(artifact_id: String, policy_version: String) -> Value {
        json!({
            "gatePolicyRef":{"artifactId":artifact_id,"version":1,"payloadHash":"a".repeat(64)},
            "gatePolicyPayloadHash":"a".repeat(64),
            "canonSemanticPolicyVersion":policy_version.clone(),
            "canonSemanticPolicy":{"policyVersion":policy_version,"rules":[{"predicate":"opened","cardinality":"MULTI","scope":{"kind":"FIXED","value":"PROJECT"}}]}
        })
    }

    fn contract_with_exact_serialized_bytes(target_bytes: usize) -> Value {
        for artifact_id_length in 1..=200 {
            let artifact_id = "x".repeat(artifact_id_length);
            let base = synthetic_audit_policy_contract(artifact_id.clone(), String::new());
            let base_bytes = audit_policy_contract_size(&base).unwrap().serialized_bytes;
            let Some(extra_bytes) = target_bytes.checked_sub(base_bytes) else {
                continue;
            };
            if extra_bytes % 2 != 0 {
                continue;
            }
            let version = "v".repeat(extra_bytes / 2);
            let contract = synthetic_audit_policy_contract(artifact_id, version);
            if audit_policy_contract_size(&contract)
                .is_ok_and(|size| size.serialized_bytes == target_bytes)
            {
                return contract;
            }
        }
        panic!("could not build the requested serialized byte boundary")
    }

    #[test]
    fn audit_policy_contract_token_limit_accepts_equal_and_rejects_next_value() {
        let exact = (0..=3000)
            .find_map(|length| {
                let contract =
                    synthetic_audit_policy_contract("gate:test".into(), "v".repeat(length));
                let size = audit_policy_contract_size(&contract).unwrap();
                (size.estimated_tokens == AUDIT_POLICY_HARD_TOKEN_LIMIT as usize
                    && size.serialized_bytes <= AUDIT_POLICY_HARD_SERIALIZED_BYTE_LIMIT)
                    .then_some((length, contract, size))
            })
            .expect("ASCII policy version can reach the exact token threshold");
        assert_eq!(
            validate_audit_policy_contract_size(
                &exact.1,
                AUDIT_POLICY_HARD_TOKEN_LIMIT,
                AUDIT_POLICY_HARD_SERIALIZED_BYTE_LIMIT,
            )
            .unwrap(),
            exact.2
        );

        let over = (exact.0 + 1..=3000)
            .find_map(|length| {
                let contract =
                    synthetic_audit_policy_contract("gate:test".into(), "v".repeat(length));
                let size = audit_policy_contract_size(&contract).unwrap();
                (size.estimated_tokens > AUDIT_POLICY_HARD_TOKEN_LIMIT as usize)
                    .then_some((contract, size))
            })
            .expect("the next token boundary must be reachable");
        let error = validate_audit_policy_contract_size(
            &over.0,
            AUDIT_POLICY_HARD_TOKEN_LIMIT,
            AUDIT_POLICY_HARD_SERIALIZED_BYTE_LIMIT,
        )
        .unwrap_err();
        assert!(error.contains(&format!("usedTokens={}", over.1.estimated_tokens)));
        assert!(error.contains("limitTokens=1024"));
    }

    #[test]
    fn audit_policy_contract_byte_limit_accepts_equal_and_rejects_next_byte() {
        let exact = contract_with_exact_serialized_bytes(AUDIT_POLICY_HARD_SERIALIZED_BYTE_LIMIT);
        let exact_size = audit_policy_contract_size(&exact).unwrap();
        assert_eq!(
            exact_size.serialized_bytes,
            AUDIT_POLICY_HARD_SERIALIZED_BYTE_LIMIT
        );
        validate_audit_policy_contract_size(&exact, 4096, 8192).unwrap();

        let over =
            contract_with_exact_serialized_bytes(AUDIT_POLICY_HARD_SERIALIZED_BYTE_LIMIT + 1);
        let over_size = audit_policy_contract_size(&over).unwrap();
        assert_eq!(
            over_size.serialized_bytes,
            AUDIT_POLICY_HARD_SERIALIZED_BYTE_LIMIT + 1
        );
        let error = validate_audit_policy_contract_size(&over, 4096, 8192).unwrap_err();
        assert!(error.contains("serializedBytes=8193"));
        assert!(error.contains("limitBytes=8192"));
    }

    #[test]
    fn audit_policy_contract_size_uses_json_escaped_and_utf8_bytes() {
        let contract =
            synthetic_audit_policy_contract("gate:策略\\\"ref".into(), "版本\\\"策略\n🌟".into());
        let serialized = serde_json::to_string(&contract).unwrap();
        assert!(serialized.contains("\\\\\\\""));
        assert!(serialized.contains("\\n"));
        let ascii_bytes = serialized.bytes().filter(u8::is_ascii).count();
        let non_ascii_scalars = serialized
            .chars()
            .filter(|character| !character.is_ascii())
            .count();
        let expected_tokens = ascii_bytes.div_ceil(3) + non_ascii_scalars;
        let measured = audit_policy_contract_size(&contract).unwrap();
        assert_eq!(measured.estimated_tokens, expected_tokens);
        assert_eq!(measured.serialized_bytes, serialized.len());
    }

    #[test]
    fn provider_payload_parser_accepts_a_json_code_fence_without_accepting_extra_text() {
        let value = json!({
            "choices": [{
                "message": {"content": "```json\n{\"ok\":true}\n```"}
            }]
        });
        assert_eq!(
            parse_provider_model_payload(value).unwrap(),
            json!({"ok": true})
        );
    }

    #[test]
    fn renderer_payload_recovery_only_closes_a_truncated_body_object() {
        let value = json!({
            "choices": [{
                "message": {"content": "{\"body\":\"完整正文"}
            }]
        });
        assert_eq!(
            parse_provider_model_payload_for_capability(value, "RENDER_CHAPTER").unwrap(),
            json!({"body":"完整正文"})
        );
    }

    #[test]
    fn craft_renderer_payload_escapes_raw_control_characters_in_the_body() {
        let value = json!({
            "choices": [{
                "message": {"content": "{\"body\":\"第一段。\n\t第二段。\"}"}
            }]
        });
        assert_eq!(
            parse_provider_model_payload_for_capability(value, "RENDER_CHAPTER").unwrap(),
            json!({"body":"第一段。\n\t第二段。"})
        );
        let truncated = json!({
            "choices": [{
                "message": {"content": "{\"body\":\"第一段。\n第二段"}
            }]
        });
        assert_eq!(
            parse_provider_model_payload_for_capability(truncated, "RENDER_CHAPTER").unwrap(),
            json!({"body":"第一段。\n第二段"})
        );
        let plan = json!({
            "choices": [{
                "message": {"content": "{\"chapterGoal\":\"一\n二\"}"}
            }]
        });
        assert!(parse_provider_model_payload_for_capability(plan, "PLAN_STRATEGY").is_err());
    }

    #[test]
    fn planner_payload_never_uses_renderer_recovery() {
        let value = json!({
            "choices": [{
                "message": {"content": "{\"body\":\"不应作为规划\""}
            }]
        });
        assert!(parse_provider_model_payload_for_capability(value, "PLAN_STRATEGY").is_err());
    }

    #[test]
    fn audit_payload_never_recovers_truncated_json_as_a_success_path() {
        let value = json!({
            "choices": [{
                "message": {"content": "{\"canonCandidates\":[]"}
            }]
        });
        assert!(
            parse_provider_model_payload_for_capability(value, "AUDIT_EXTRACT")
                .unwrap_err()
                .starts_with("B08_AUDIT_TRUNCATED_OR_INVALID")
        );
        let value = json!({
            "choices": [{
                "message": {"content": "{\"canonCandidates\":[]]"}
            }]
        });
        assert!(parse_provider_model_payload_for_capability(value, "AUDIT_EXTRACT").is_err());
        assert!(parse_provider_model_payload_for_capability(
            json!({
                "choices": [{
                    "message": {"content": "{\"canonCandidates\":[]"}
                }]
            }),
            "PLAN_STRATEGY"
        )
        .is_err());
    }

    #[test]
    fn audit_contract_requires_all_categories_and_explicit_completeness() {
        let missing = json!({
            "qualityFindings":[], "canonCandidates":[], "knowledgeCandidates":[],
            "continuityCandidates":[], "taskProgressCandidates":[],
            "revealedReservationRefs":[]
        });
        assert!(serde_json::from_value::<AuditExtraction>(missing).is_err());

        let unresolved: AuditExtraction = serde_json::from_value(json!({
            "qualityFindings":[], "canonCandidates":[], "knowledgeCandidates":[],
            "continuityCandidates":[], "taskProgressCandidates":[],
            "revealedReservationRefs":[], "evidenceRanges":[],
            "auditCompleteness":"UNRESOLVED_HARD", "unresolvedHard":true
        }))
        .unwrap();
        assert_eq!(
            validate_audit_completeness(&json!({}), &unresolved).unwrap_err(),
            "B08_AUDIT_UNRESOLVED_HARD"
        );

        let standalone_ranges: AuditExtraction = serde_json::from_value(json!({
            "qualityFindings":[], "canonCandidates":[], "knowledgeCandidates":[],
            "continuityCandidates":[], "taskProgressCandidates":[],
            "revealedReservationRefs":[], "evidenceRanges":[{"start":0,"end":2,"quote":"证据"}],
            "auditCompleteness":"COMPLETE_NO_CHANGE", "unresolvedHard":false
        }))
        .unwrap();
        assert_eq!(
            validate_audit_completeness(&json!({}), &standalone_ranges).unwrap_err(),
            "B08_AUDIT_COMPLETENESS_MISMATCH"
        );
    }

    #[test]
    fn audit_candidate_binding_rejects_a_review_for_a_different_candidate_version() {
        let candidate_a =
            json!({"artifactId":"candidate:a","version":1,"payloadHash":"a".repeat(64)});
        let candidate_b =
            json!({"artifactId":"candidate:a","version":2,"payloadHash":"b".repeat(64)});
        let report = json!({"candidateRef":candidate_a});
        assert!(validate_audit_candidate_binding_value(&report, &candidate_a).is_ok());
        assert_eq!(
            validate_audit_candidate_binding_value(&report, &candidate_b).unwrap_err(),
            "B08_AUDIT_CANDIDATE_BINDING_MISMATCH"
        );
    }

    #[test]
    fn knowledge_candidate_can_bind_to_entity_and_fact_from_same_audit() {
        let pending_entities = ["ent:new".to_string()].into_iter().collect();
        let pending_facts = ["fact:new".to_string()].into_iter().collect();
        assert!(knowledge_authority_is_bound(
            false,
            false,
            "ent:new",
            "fact:new",
            &pending_entities,
            &pending_facts,
        ));
        assert!(!knowledge_authority_is_bound(
            false,
            false,
            "ent:missing",
            "fact:new",
            &pending_entities,
            &pending_facts,
        ));
    }

    #[test]
    fn fx17_invented_task_is_dropped_but_real_unplanned_task_is_kept_for_the_gate() {
        let mut audit = json!({
            "taskProgressCandidates":[
                {"taskId":"task:first_memory_repair","status":"SATISFIED"},
                {"taskId":"task:real_future","status":"ACTIVE"},
                {"taskId":"task:planned","status":"ACTIVE"}
            ],
            "normalization":{"policy":"DROP_UNBOUND_AUTHORITY_CANDIDATES"}
        });
        let plan = json!({"activeTaskRefs":["task:planned"]});
        let unplanned = unplanned_task_candidate_ids(&audit, &plan);
        assert_eq!(
            unplanned,
            BTreeSet::from([
                "task:first_memory_repair".to_string(),
                "task:real_future".to_string()
            ])
        );
        drop_invented_task_candidates(
            &mut audit,
            &unplanned,
            &BTreeSet::from(["task:real_future".to_string()]),
        );
        let kept = audit["taskProgressCandidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["taskId"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(kept, ["task:real_future", "task:planned"]);
        assert_eq!(audit["normalization"]["droppedInventedTasks"], 1);
    }

    #[test]
    fn evidence_ranges_are_bounded_by_actual_candidate_text() {
        assert!(validate_candidate_evidence_range("你好世界", 0, 4).is_ok());
        assert!(validate_candidate_evidence_range("你好世界", 3, 2).is_err());
        assert!(validate_candidate_evidence_range("你好世界", 0, 5).is_err());
    }

    #[test]
    fn audit_normalization_drops_unbound_authority_candidates_and_rebuilds_exact_quotes() {
        let payload = json!({
            "qualityFindings":[{"description":"节奏清晰"}],
            "canonCandidates":[{"type":"EVENT","content":"无法绑定正式实体"}],
            "knowledgeCandidates":[{"topic":"未知事实","detail":"无法绑定知情者"}],
            "continuityCandidates":[{"entity":"沈砚","stateChange":"继续追查"}],
            "taskProgressCandidates":[{"taskId":"task:outside","status":"ACTIVE"}],
            "evidenceRanges":[{"start":0,"end":2,"context":"不可信模型摘录"}]
        });
        let normalized = normalize_audit_model_output(payload, "你好世界");
        assert_eq!(normalized["canonCandidates"], json!([]));
        assert_eq!(normalized["knowledgeCandidates"], json!([]));
        assert_eq!(normalized["continuityCandidates"], json!([]));
        assert_eq!(normalized["taskProgressCandidates"], json!([]));
        assert_eq!(normalized["evidenceRanges"][0]["quote"], "你好");
        assert_eq!(
            normalized["normalization"]["policy"],
            "DROP_UNBOUND_AUTHORITY_CANDIDATES"
        );
    }

    #[test]
    fn audit_normalization_rebinds_out_of_range_provider_spans_by_exact_quote() {
        let normalized = normalize_audit_model_output(
            json!({
                "canonCandidates": [{
                    "factId":"fact:quote",
                    "subjectId":"entity:quote",
                    "predicate":"OBSERVED",
                    "objectJson":{},
                    "authorityLevel":"C",
                    "evidence":{"start":900,"end":1200,"quote":"关键证据"}
                }]
            }),
            "正文中的关键证据已经出现。",
        );
        assert_eq!(normalized["canonCandidates"][0]["evidence"]["start"], 4);
        assert_eq!(normalized["canonCandidates"][0]["evidence"]["end"], 8);
        assert_eq!(
            normalized["canonCandidates"][0]["evidence"]["quote"],
            "关键证据"
        );
    }

    fn fx17_real_audit() -> (String, Value) {
        let fixture: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/fx17_audit_real_evidence.json"
        ))
        .unwrap();
        (
            fixture["body"].as_str().unwrap().to_owned(),
            fixture["audit"].clone(),
        )
    }

    #[test]
    fn fx17_audit_unique_quote_with_wrong_offsets_is_located_exactly() {
        let body = "沈砚站在“城市记忆档案局”第七分部的走廊尽头，指尖摩挲着档案。";
        let normalized = normalize_audit_model_output(
            json!({"evidenceRanges":[{"start":20,"end":24,"quote":"“城市记忆档案局”第七分部的走廊尽头"}]}),
            body,
        );
        let evidence = &normalized["evidenceRanges"][0];
        assert_eq!(
            evidence["rebound"], false,
            "a unique verbatim quote is exact evidence"
        );
        assert_eq!(evidence["start"], 4);
        assert_eq!(
            evidence["end"],
            4 + "“城市记忆档案局”第七分部的走廊尽头".chars().count()
        );
        assert_eq!(normalized["normalization"]["reboundEvidence"], 0);
    }

    #[test]
    fn fx17_audit_quote_missing_only_paragraph_breaks_is_located_exactly() {
        let body = "他调出了“回溯仪”。\n\n“开始修复。”他低声说道。";
        let normalized = normalize_audit_model_output(
            json!({"evidenceRanges":[
                {"start":0,"end":3,"quote":"调出了“回溯仪”。“开始修复。”他低声说道"},
                {"start":0,"end":3,"quote":"不存在的引文"}
            ]}),
            body,
        );
        let evidence = &normalized["evidenceRanges"][0];
        assert_eq!(evidence["rebound"], false);
        assert_eq!(evidence["start"], 1);
        assert_eq!(
            evidence["quote"],
            "调出了“回溯仪”。\n\n“开始修复。”他低声说道"
        );
        let chars = body.chars().collect::<Vec<_>>();
        assert_eq!(
            chars[1..evidence["end"].as_u64().unwrap() as usize]
                .iter()
                .collect::<String>(),
            evidence["quote"].as_str().unwrap()
        );
        assert_eq!(normalized["evidenceRanges"][1]["rebound"], true);
        assert_eq!(normalized["normalization"]["reboundEvidence"], 1);
    }

    #[test]
    fn fx19_ellipsis_quote_binds_to_the_full_passage_when_parts_are_unique_and_close() {
        let body = format!(
            "开头。他撕下一页纸，抄下坐标。{}他把纸条藏进口袋。结尾。",
            "雨".repeat(40)
        );
        let normalized = normalize_audit_model_output(
            json!({"evidenceRanges":[
                {"start":0,"end":2,"quote":"他撕下一页纸，抄下坐标...他把纸条藏进口袋。"},
                {"start":0,"end":2,"quote":"开头……结尾。"}
            ]}),
            &body,
        );
        let evidence = &normalized["evidenceRanges"][0];
        assert_eq!(evidence["rebound"], false, "{normalized}");
        assert_eq!(
            evidence["quote"],
            format!(
                "他撕下一页纸，抄下坐标。{}他把纸条藏进口袋。",
                "雨".repeat(40)
            )
        );
        let chars = body.chars().collect::<Vec<_>>();
        let (start, end) = (
            evidence["start"].as_u64().unwrap() as usize,
            evidence["end"].as_u64().unwrap() as usize,
        );
        assert_eq!(
            chars[start..end].iter().collect::<String>(),
            evidence["quote"].as_str().unwrap()
        );
        // "开头" + ... + "结尾。" spans the whole body but is still within 300.
        assert_eq!(normalized["evidenceRanges"][1]["rebound"], false);
    }

    #[test]
    fn fx19_ellipsis_quote_with_a_wide_gap_or_repeated_part_stays_rebound() {
        let wide = format!("甲乙丙。{}丁戊己。", "雨".repeat(301));
        let normalized = normalize_audit_model_output(
            json!({"evidenceRanges":[{"start":0,"end":1,"quote":"甲乙丙...丁戊己。"}]}),
            &wide,
        );
        assert_eq!(normalized["evidenceRanges"][0]["rebound"], true);
        let repeated = "甲乙丙。中间。甲乙丙。丁戊己。";
        let normalized = normalize_audit_model_output(
            json!({"evidenceRanges":[{"start":0,"end":1,"quote":"甲乙丙...丁戊己。"}]}),
            repeated,
        );
        assert_eq!(normalized["evidenceRanges"][0]["rebound"], true);
    }

    #[test]
    fn ld1_new_facts_and_knowledge_get_system_ids_and_knowledge_follows_its_fact() {
        let mut raw = json!({
            "canonCandidates":[{"factId":"fact-1","predicate":"located_at"},{"predicate":"owns_item"}],
            "knowledgeCandidates":[{"knowledgeId":"know-1","factIndex":1},{"factId":"fact-1"}],
            "continuityCandidates":[{"sourceEventId":"event:fact-1"},{"sourceFactIndex":1}]
        });
        assign_audit_ids(&mut raw, "chap:1", &BTreeSet::new());
        let fact = |n: usize| new_audit_item_id("canon", "chap:1", n);
        assert_eq!(raw["canonCandidates"][0]["factId"], fact(1));
        assert_eq!(raw["canonCandidates"][1]["factId"], fact(2));
        assert_eq!(
            raw["knowledgeCandidates"][0]["knowledgeId"],
            new_audit_item_id("know", "chap:1", 1)
        );
        assert_eq!(raw["knowledgeCandidates"][0]["factId"], fact(2));
        assert_eq!(raw["knowledgeCandidates"][1]["factId"], fact(1));
        assert_eq!(
            raw["continuityCandidates"][0]["sourceEventId"],
            format!("event:{}", fact(1))
        );
        assert_eq!(
            raw["continuityCandidates"][1]["sourceEventId"],
            format!("event:{}", fact(2))
        );
    }

    fn qc1_thread(id: &str, signals: &[&str]) -> Value {
        let mut promotion = serde_json::Map::new();
        for signal in signals {
            promotion.insert((*signal).into(), json!(true));
        }
        json!({"threadRef":id,"disposition":"ADVANCE","promotionSignals":promotion})
    }

    #[test]
    fn qc1_a_chapter_keeps_its_three_strongest_new_threads_and_holds_back_the_rest() {
        // QC-1.3 (owner decision 2026-10-26): only threads the story needs.
        let known = BTreeSet::from(["thread:known".to_string()]);
        let mut audit = json!({"continuityCandidates":[
            qc1_thread("thread:new:1", &["ambientDescription"]),
            qc1_thread("thread:known", &[]),
            qc1_thread("thread:new:2", &["explicitPromise", "mysteryOrRevealResponsibility"]),
            qc1_thread("thread:new:3", &["unresolvedMajorConflict"]),
            qc1_thread("thread:new:4", &["keyFutureUseObjectOrInformation", "explicitPromise", "goalContractResponsibility"]),
            qc1_thread("thread:new:5", &["majorRelationshipTransition"])
        ],"normalization":{}});
        cap_new_threads(&mut audit, &known);
        let kept = audit["continuityCandidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["threadRef"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            kept,
            [
                "thread:known",
                "thread:new:2",
                "thread:new:3",
                "thread:new:4"
            ]
        );
        let held = audit["normalization"]["heldBack"].as_array().unwrap();
        assert_eq!(held.len(), 2);
        assert!(held
            .iter()
            .all(|entry| entry["reason"] == "QC1_THREAD_OVER_LIMIT"));
        assert_eq!(
            held_back_reason("QC1_THREAD_OVER_LIMIT"),
            "本章新开的伏笔超过 3 条，这条重要性靠后，只作为候选，没有记入档案"
        );
    }

    const QC1_BODY: &str = "沈砚把搪瓷猫碗放回窗台。缺口和我的碗缺耳对得上。我记东西靠形状。\n他低头看右手，虎口那道划痕淡得快看不见了。林薇没说话。";

    fn qc1_fact(evidence: Value) -> Value {
        json!({"canonCandidates":[{
            "factId":"canon:qc1:0","subjectId":"沈砚","entityId":"沈砚","canonicalName":"沈砚",
            "entityType":"PERSON","predicate":"status","authorityLevel":"A",
            "objectJson":{"value":"右手虎口划痕正在淡化"},"evidence":evidence
        }]})
    }

    #[test]
    fn qc1_evidence_cited_by_sentence_number_is_that_sentence() {
        // QC-1.1: the model names the sentence; the system takes its text.
        let mut raw = qc1_fact(json!({"sentence":3}));
        let unbound = bind_cited_evidence(&mut raw, QC1_BODY);
        assert!(unbound.is_empty(), "{unbound:?}");
        let normalized = normalize_audit_model_output(raw, QC1_BODY);
        let evidence = &normalized["canonCandidates"][0]["evidence"];
        assert_eq!(
            evidence["quote"],
            "他低头看右手，虎口那道划痕淡得快看不见了。"
        );
        assert_eq!(evidence["rebound"], false);
    }

    #[test]
    fn qc1_a_sentence_number_naming_nothing_of_the_fact_is_not_bound() {
        // Acceptance 2026-10-26, chapter 5: 虎口 bound to the bowl sentence.
        let mut raw = qc1_fact(json!({"sentence":1}));
        let unbound = bind_cited_evidence(&mut raw, QC1_BODY);
        assert_eq!(unbound.len(), 1, "{unbound:?}");
        assert_eq!(unbound[0]["sentence"], 1);
        let normalized = normalize_audit_model_output(raw, QC1_BODY);
        assert_eq!(normalized["canonCandidates"], json!([]));
    }

    #[test]
    fn qc1_a_near_quote_landing_on_an_unrelated_passage_is_not_bound() {
        let mut raw = qc1_fact(json!({"start":12,"end":24,"quote":"缺口和我的碗缺耳对不上。"}));
        let unbound = bind_cited_evidence(&mut raw, QC1_BODY);
        assert_eq!(unbound.len(), 1, "{unbound:?}");
        let normalized = normalize_audit_model_output(raw, QC1_BODY);
        assert_eq!(normalized["canonCandidates"], json!([]));
    }

    #[test]
    fn qc1_a_near_quote_of_the_fact_is_still_bound() {
        let mut raw = qc1_fact(json!({"quote":"他低头看右手，虎口那道划痕淡得看不见了。"}));
        assert!(bind_cited_evidence(&mut raw, QC1_BODY).is_empty());
        let normalized = normalize_audit_model_output(raw, QC1_BODY);
        assert_eq!(
            normalized["canonCandidates"][0]["evidence"]["quote"],
            "他低头看右手，虎口那道划痕淡得快看不见了。"
        );
    }

    #[test]
    fn qc1_the_audit_request_numbers_the_body_sentences() {
        let sentences = body_sentences(QC1_BODY);
        assert_eq!(sentences.len(), 5);
        assert_eq!(sentences[1].2, "缺口和我的碗缺耳对得上。");
        let (start, end, _) = sentences[3];
        assert_eq!(
            QC1_BODY
                .chars()
                .skip(start)
                .take(end - start)
                .collect::<String>(),
            "他低头看右手，虎口那道划痕淡得快看不见了。"
        );
    }

    #[test]
    fn ld2_rebound_evidence_and_dropped_counts_are_held_back_not_failed() {
        let prepared = json!({
            "qualityFindings":[],"knowledgeCandidates":[],"continuityCandidates":[],
            "taskProgressCandidates":[],"revealedReservationRefs":[],"evidenceRanges":[],
            "auditCompleteness":"COMPLETE_WITH_DELTA","unresolvedHard":false,
            "canonCandidates":[
                {"factId":"good","evidence":{"rebound":false}},
                {"factId":"weak","evidence":{"rebound":true}}
            ],
            "normalization":{"droppedCandidates":{"canonCandidates":1},"reboundEvidence":1}
        });
        // Accepts anything but non-zero normalization counters.
        let validate = |payload: &Value| -> Result<AuditExtraction, String> {
            if payload["normalization"]["reboundEvidence"] != json!(0) {
                return Err("B08_AUDIT_INCOMPLETE_EVIDENCE_REBOUND".into());
            }
            Err(format!("kept {}", payload["canonCandidates"]))
        };
        let error = hold_back_audit_items(prepared, &validate).unwrap_err();
        // Only the item with trusted evidence was tried and kept.
        assert!(error.contains("good") && !error.contains("weak"), "{error}");
    }

    #[test]
    fn ld1_a_quote_a_character_or_two_off_binds_to_its_passage() {
        // Real signing evaluation 2026-10-25: 他→沈砚 and 攥着→攥紧.
        let body = "雨停了。\n他低头看饼干。油渍在纸袋底洇成一个不规则的圆，像一枚被压扁的铃铛。他把饼干收好。\n他攥着铃舌，掌心伤口贴着金属缺口，血和锈混在一起。\n巷口没有人。";
        let span = |quote: &str| {
            locate_quote_span(body, quote).map(|(start, end)| {
                body.chars()
                    .skip(start)
                    .take(end - start)
                    .collect::<String>()
            })
        };
        assert_eq!(
            span("沈砚低头看饼干。油渍在纸袋底洇成一个不规则的圆，像一枚被压扁的铃铛。").as_deref(),
            Some("他低头看饼干。油渍在纸袋底洇成一个不规则的圆，像一枚被压扁的铃铛。")
        );
        assert_eq!(
            span("他攥紧铃舌，掌心伤口贴着金属缺口，血和锈混在一起。").as_deref(),
            Some("他攥着铃舌，掌心伤口贴着金属缺口，血和锈混在一起。")
        );
    }

    #[test]
    fn ld1_a_quote_too_far_off_or_matching_two_passages_is_not_located() {
        let body = "他推开门，看见院子里有一棵树。她推开门，看见院子里有一棵树。";
        // Two passages are equally close.
        assert_eq!(
            locate_quote_span(body, "你推开门，看见院子里有一棵树。"),
            None
        );
        // More than a tenth of the quote differs.
        assert_eq!(
            locate_quote_span("他把信交给了门口的守卫。", "她把钥匙递给了窗边的老人。"),
            None
        );
    }

    #[test]
    fn ld1_ids_survive_dropping_a_fact_and_assigning_again() {
        // The unlocatable-quote rerun drops candidates and checks the rest.
        let mut raw = json!({
            "canonCandidates":[{"factId":"door"},{"factId":"hall"}],
            "knowledgeCandidates":[{"factIndex":1}],
            "continuityCandidates":[]
        });
        assign_audit_ids(&mut raw, "chap:1", &BTreeSet::new());
        assert_eq!(raw["canonCandidates"][1]["modelLabel"], "hall");
        assert!(raw["knowledgeCandidates"][0].get("factIndex").is_none());
        raw["canonCandidates"].as_array_mut().unwrap().remove(0);
        assign_audit_ids(&mut raw, "chap:1", &BTreeSet::new());
        let hall = new_audit_item_id("canon", "chap:1", 1);
        assert_eq!(raw["canonCandidates"][0]["factId"], hall);
        assert_eq!(raw["canonCandidates"][0]["modelLabel"], "hall");
        assert_eq!(raw["knowledgeCandidates"][0]["factId"], hall);
    }

    #[test]
    fn ld1_one_new_thread_label_used_twice_is_one_thread() {
        let mut raw =
            json!({"continuityCandidates":[{"threadRef":"thread-1"},{"threadRef":"thread-1"}]});
        assign_audit_ids(&mut raw, "chap:3", &BTreeSet::new());
        assert_eq!(
            raw["continuityCandidates"][0]["threadRef"],
            raw["continuityCandidates"][1]["threadRef"]
        );
    }

    #[test]
    fn ld1_a_thread_ref_names_a_known_thread_or_else_a_new_system_thread() {
        // Real audits on 2026-10-25 wrote `thread-1` in every chapter.
        let known = BTreeSet::from(["thread:old-promise".to_string()]);
        let mut raw = json!({"continuityCandidates":[
            {"threadRef":"thread:old-promise"},
            {"threadRef":"old-promise"},
            {"threadRef":"thread-1"},
            {"threadRef":"new_thread:sister_trace"},
            {}
        ]});
        assign_audit_ids(&mut raw, "chap:2", &known);
        let thread = |n: usize| new_audit_item_id("thread", "chap:2", n);
        let refs = raw["continuityCandidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["threadRef"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            refs,
            [
                "thread:old-promise".to_string(),
                "thread:old-promise".to_string(),
                thread(1),
                thread(2),
                thread(3),
            ]
        );
    }

    #[test]
    fn fx17_audit_whitespace_insensitive_quote_must_still_be_unique() {
        let body = "他说：好。\n他说：好。";
        let normalized = normalize_audit_model_output(
            json!({"evidenceRanges":[{"start":0,"end":1,"quote":"他说： 好。"}]}),
            body,
        );
        assert_eq!(normalized["evidenceRanges"][0]["rebound"], true);
    }

    #[test]
    fn fx17_real_audit_keeps_only_ambiguous_evidence_as_rebound() {
        let (body, audit) = fx17_real_audit();
        let normalized = normalize_audit_model_output(audit, &body);
        let mut exact = 0;
        let mut rebound = Vec::new();
        for category in [
            "canonCandidates",
            "knowledgeCandidates",
            "continuityCandidates",
            "taskProgressCandidates",
        ] {
            for item in normalized[category].as_array().unwrap() {
                let evidence = &item["evidence"];
                let quote = evidence["quote"].as_str().unwrap();
                let start = evidence["start"].as_u64().unwrap() as usize;
                let end = evidence["end"].as_u64().unwrap() as usize;
                // Whatever is kept must point at exactly its quote in the body.
                assert_eq!(
                    body.chars()
                        .skip(start)
                        .take(end - start)
                        .collect::<String>(),
                    quote
                );
                if evidence["rebound"] == true {
                    rebound.push(quote.to_owned());
                } else {
                    exact += 1;
                }
            }
        }
        // 12 model offsets were all wrong: the 9 unique verbatim quotes and
        // (FX-19) the ellipsis quote with unique, close parts are now exact;
        // the two bare names still fail closed (RV-16).
        assert_eq!(exact, 10, "rebound: {rebound:?}");
        assert_eq!(rebound.len(), 2);
        assert!(
            normalized["normalization"]["reboundEvidence"]
                .as_u64()
                .unwrap()
                > 0
        );
    }

    #[test]
    fn audit_evidence_rebinding_is_marked_and_makes_audit_incomplete() {
        let normalized = normalize_audit_model_output(
            json!({
                "qualityFindings":[],
                "canonCandidates":[],
                "knowledgeCandidates":[],
                "continuityCandidates":[],
                "taskProgressCandidates":[],
                "revealedReservationRefs":[],
                "evidenceRanges":[{"start":1,"end":3,"quote":"证据"}],
                "auditCompleteness":"COMPLETE_NO_CHANGE",
                "unresolvedHard":false
            }),
            "证据在前，证据在后。",
        );
        assert_eq!(normalized["evidenceRanges"][0]["rebound"], true);
        assert_eq!(normalized["normalization"]["reboundEvidence"], 1);
        let audit: AuditExtraction = serde_json::from_value(normalized.clone()).unwrap();
        assert_eq!(
            validate_audit_completeness(&normalized, &audit).unwrap_err(),
            "B08_AUDIT_INCOMPLETE_EVIDENCE_REBOUND"
        );
    }

    #[test]
    fn audit_normalization_rebinds_ellipsis_quote_only_when_segments_are_present() {
        let normalized = normalize_audit_model_output(
            json!({
                "canonCandidates": [{
                    "factId":"fact:ellipsis",
                    "subjectId":"entity:ellipsis",
                    "predicate":"OBSERVED",
                    "objectJson":{},
                    "authorityLevel":"C",
                    "evidence":{"start":800,"end":900,"quote":"第一处证据...第二处证据"}
                }]
            }),
            "前文。第一处证据，中间过程。第二处证据。后文。",
        );
        assert_eq!(normalized["canonCandidates"].as_array().unwrap().len(), 1);
        let evidence = &normalized["canonCandidates"][0]["evidence"];
        assert_eq!(evidence["quote"], "第一处证据，中间过程。第二处证据");
        let missing = normalize_audit_model_output(
            json!({
                "canonCandidates": [{
                    "factId":"fact:missing",
                    "subjectId":"entity:missing",
                    "predicate":"OBSERVED",
                    "objectJson":{},
                    "authorityLevel":"C",
                    "evidence":{"start":800,"end":900,"quote":"第一处证据...不存在"}
                }]
            }),
            "前文。第一处证据，中间过程。第二处证据。后文。",
        );
        assert_eq!(missing["canonCandidates"], json!([]));
    }

    #[test]
    fn audit_normalization_maps_typed_knowledge_state_aliases() {
        let normalized = normalize_audit_model_output(
            json!({
                "knowledgeCandidates":[{
                    "knowledgeId":"know:alias",
                    "knowerId":"entity:hero",
                    "factId":"fact:hero",
                    "epistemicState":"KNOWS_WITH_CONTEXT",
                    "evidence":{"start":0,"end":2,"quote":"你好"}
                }]
            }),
            "你好世界",
        );
        assert_eq!(
            normalized["knowledgeCandidates"][0]["epistemicState"],
            "KNOWS"
        );
        let unknown = normalize_audit_model_output(
            json!({
                "knowledgeCandidates":[{
                    "knowledgeId":"know:bad",
                    "knowerId":"entity:hero",
                    "factId":"fact:hero",
                    "epistemicState":"INVENTED",
                    "evidence":{"start":0,"end":2,"quote":"你好"}
                }]
            }),
            "你好世界",
        );
        assert_eq!(unknown["knowledgeCandidates"], json!([]));
    }

    #[test]
    fn audit_normalization_maps_common_knowledge_state_synonyms() {
        // Seen on 2026-10-18: a real audit reported REMEMBERED.
        for (state, expected) in [
            ("REMEMBERED", "KNOWS"),
            ("RECALLS", "KNOWS"),
            ("REALIZES", "KNOWS"),
            ("LEARNED", "KNOWS"),
            ("DOUBTS", "SUSPECTS"),
            ("HIDES", "CONCEALS"),
            ("FORGOTTEN", "UNKNOWN"),
        ] {
            let normalized = normalize_audit_model_output(
                json!({
                    "knowledgeCandidates":[{
                        "knowledgeId":"know:synonym",
                        "knowerId":"entity:hero",
                        "factId":"fact:hero",
                        "epistemicState":state,
                        "evidence":{"start":0,"end":2,"quote":"你好"}
                    }]
                }),
                "你好世界",
            );
            assert_eq!(
                normalized["knowledgeCandidates"][0]["epistemicState"], expected,
                "{state}"
            );
        }
    }

    #[test]
    fn provider_input_keeps_every_field_next_to_an_instruction() {
        // The signing evaluation once sent only its instruction, without the
        // chapters it was asked to evaluate.
        let sent = serialize_provider_input(&json!({
            "prompt":"执行黄金三章评估",
            "chapters":[{"chapterNo":1,"body":"正文第一句"}]
        }));
        assert!(sent.starts_with("执行黄金三章评估"));
        assert!(sent.contains("正文第一句"));
        assert!(!sent.contains("\"prompt\""));
        assert_eq!(
            serialize_provider_input(&json!({"prompt":"只有指令"})),
            "只有指令"
        );
        assert_eq!(serialize_provider_input(&json!({"a":1})), "{\"a\":1}");
    }

    #[test]
    fn omission_span_binds_a_quote_that_left_out_words_to_its_one_passage() {
        let body = "雨停了。沈砚没动。他慢慢从内袋掏出信封，纸面泛黄卷边。门开了。"
            .chars()
            .collect::<Vec<_>>();
        let (start, end) =
            omission_unique_span(&body, "沈砚从内袋掏出信封，纸面泛黄卷边。").unwrap();
        assert_eq!(
            body[start..end].iter().collect::<String>(),
            "沈砚没动。他慢慢从内袋掏出信封，纸面泛黄卷边。"
        );
        // Reordered or invented text never binds.
        assert!(omission_unique_span(&body, "纸面泛黄卷边。沈砚从内袋掏出信封，").is_none());
        assert!(omission_unique_span(&body, "确立对抗档案局掩盖真相的决心。").is_none());
    }

    #[test]
    fn unlocatable_candidates_are_dropped_and_named() {
        let body = "林舟推开石门。他看见一盏灯。";
        let (kept, dropped) = drop_unlocatable_audit_candidates(
            &json!({
                "canonCandidates":[
                    {"factId":"fact:door","evidence":{"start":0,"end":7,"quote":"林舟推开石门。"}},
                    {"factId":"fact:plan","evidence":{"start":0,"end":5,"quote":"确立对抗的决心。"}}
                ],
                "evidenceRanges":[{"start":0,"end":5,"quote":"确立对抗的决心。"}]
            }),
            body,
        );
        assert_eq!(kept["canonCandidates"].as_array().unwrap().len(), 1);
        assert_eq!(kept["evidenceRanges"], json!([]));
        assert_eq!(
            dropped,
            json!([{"field":"canonCandidates","id":"fact:plan","label":null,"quote":"确立对抗的决心。"}])
        );
    }

    #[test]
    fn rewrite_edits_that_add_model_habits_are_not_applied() {
        let body = "他没点保存键。门开了。";
        let rewrite = json!({"edits":[
            {"quote":"他没点保存键。","replacement":"指尖悬在保存键上方，像被磁石吸住。"},
            {"quote":"门开了。","replacement":"门轴吱呀一声，冷风灌进来。"}
        ]});
        let (revised, applied) = apply_unique_edits(body, &rewrite);
        assert_eq!(applied, 1);
        assert_eq!(revised, "他没点保存键。门轴吱呀一声，冷风灌进来。");
    }

    #[test]
    fn audit_normalization_wraps_a_scalar_canon_object_in_a_map() {
        let normalized = normalize_audit_model_output(
            json!({
                "canonCandidates":[{
                    "factId":"fact:awake","subjectId":"entity:hero","predicate":"is_awake",
                    "objectJson":true,"authorityLevel":"C",
                    "evidence":{"start":0,"end":2,"quote":"你好"}
                }]
            }),
            "你好世界",
        );
        assert_eq!(
            normalized["canonCandidates"][0]["objectJson"],
            json!({"value":true})
        );
    }

    #[test]
    fn audit_normalization_gives_a_canon_candidate_without_authority_the_lowest_level() {
        let normalized = normalize_audit_model_output(
            json!({
                "canonCandidates":[{
                    "factId":"fact:item","subjectId":"entity:hero","predicate":"owns_item",
                    "objectJson":{"item":"钢笔"},
                    "evidence":{"start":0,"end":2,"quote":"你好"}
                }]
            }),
            "你好世界",
        );
        assert_eq!(normalized["canonCandidates"][0]["authorityLevel"], "C");
        assert_eq!(
            normalized["normalization"]["droppedCandidates"]["canonCandidates"],
            0
        );
    }

    #[test]
    fn audit_feedback_names_candidates_missing_fields_or_evidence() {
        let raw = json!({
            "canonCandidates":[
                {"factId":"fact:ok","subjectId":"s","predicate":"p","objectJson":{},"evidence":{"quote":"你好"}},
                {"factId":"fact:bare","subjectId":"s","predicate":"p","objectJson":{}}
            ],
            "knowledgeCandidates":[{"knowledgeId":"know:1","factId":"fact:ok","evidence":{"quote":"你好"}}]
        });
        assert_eq!(
            incomplete_audit_candidates(&raw),
            json!([
                {"field":"canonCandidates","id":"fact:bare","missing":["evidence"]},
                {"field":"knowledgeCandidates","id":"know:1","missing":["knowerId","epistemicState","acquiredStoryTime","visibilityScope"]}
            ])
        );
    }

    #[test]
    fn audit_normalization_derives_the_complete_label_from_the_candidates() {
        let candidate = json!({
            "factId":"fact:item","subjectId":"entity:hero","predicate":"owns_item",
            "objectJson":{"item":"钢笔"},"authorityLevel":"C",
            "evidence":{"start":0,"end":2,"quote":"你好"}
        });
        // Seen on 2026-10-18: nineteen candidates labeled COMPLETE_NO_CHANGE.
        let with_delta = normalize_audit_model_output(
            json!({"canonCandidates":[candidate],"auditCompleteness":"COMPLETE_NO_CHANGE"}),
            "你好世界",
        );
        assert_eq!(with_delta["auditCompleteness"], "COMPLETE_WITH_DELTA");
        let no_change = normalize_audit_model_output(
            json!({"canonCandidates":[],"auditCompleteness":"COMPLETE_WITH_DELTA"}),
            "你好世界",
        );
        assert_eq!(no_change["auditCompleteness"], "COMPLETE_NO_CHANGE");
        let incomplete = normalize_audit_model_output(
            json!({"canonCandidates":[],"auditCompleteness":"INCOMPLETE"}),
            "你好世界",
        );
        assert_eq!(incomplete["auditCompleteness"], "INCOMPLETE");
    }

    #[test]
    fn audit_normalization_drops_and_records_candidates_without_evidence() {
        // Owner decision (2026-10-18): an unverifiable fact is left out and
        // recorded instead of failing the chapter.
        let normalized = normalize_audit_model_output(
            json!({
                "canonCandidates":[
                    {"factId":"fact:quoted","subjectId":"s","predicate":"owns_item","objectJson":{},"authorityLevel":"C","evidence":{"start":0,"end":2,"quote":"你好"}},
                    {"factId":"fact:bare","subjectId":"s","predicate":"owns_item","objectJson":{},"authorityLevel":"C"}
                ],
                "qualityFindings":[],"knowledgeCandidates":[],"continuityCandidates":[],
                "taskProgressCandidates":[],"revealedReservationRefs":[],"evidenceRanges":[],
                "auditCompleteness":"COMPLETE_WITH_DELTA","unresolvedHard":false
            }),
            "你好世界",
        );
        assert_eq!(normalized["canonCandidates"].as_array().unwrap().len(), 1);
        assert_eq!(
            normalized["normalization"]["droppedCandidates"]["canonCandidates"],
            0
        );
        assert_eq!(
            normalized["normalization"]["droppedWithoutEvidence"],
            json!([{"field":"canonCandidates","id":"fact:bare"}])
        );
        let audit: AuditExtraction = serde_json::from_value(normalized.clone()).unwrap();
        assert!(validate_audit_completeness(&normalized, &audit).is_ok());
    }

    #[test]
    fn audit_sees_the_plan_without_prose_it_could_quote() {
        // Seen on 2026-10-18: audits quoted plan beats instead of the body.
        let plan = json!({
            "chapterGoal":"查明来源",
            "continuityActions":[{"threadRef":"thread:1","disposition":"ADVANCE"}],
            "sceneExecution":[{
                "sceneType":"调查","goal":"拿到线索","conflict":"守卫","turn":"放行","result":"被盯上",
                "beats":["沈砚接入深层网络，面对破碎的记忆碎片进行重组"],"estimatedCharacters":900
            }]
        });
        let view = audit_plan_view(&plan);
        assert_eq!(view["chapterGoal"], "查明来源");
        assert_eq!(view["continuityActions"], plan["continuityActions"]);
        assert_eq!(
            view["sceneExecution"],
            json!([{"sceneType":"调查","estimatedCharacters":900}])
        );
        assert!(!view.to_string().contains("深层网络"));
    }

    #[test]
    fn production_model_input_carries_bounded_authority_context() {
        let input = crate::narrative::ContextCompilerInput {
            execution_mode: "LONG_FORM".into(),
            project_revision: 7,
            goal_contract_refs: Vec::new(),
            goal_contract_fields: Vec::new(),
            state: crate::narrative::CurrentNarrativeStateView::from_records(
                crate::narrative::RuntimeBounds {
                    project_id: "project".into(),
                    entity_ids: Vec::new(),
                    volume_id: Some("volume".into()),
                    chapter_id: Some("chapter".into()),
                    limit: 100,
                },
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                false,
            ),
            continuity_threads: Vec::new(),
            plan_task_refs: Vec::new(),
            plan_task_contexts: Vec::new(),
            task_states: Vec::new(),
            active_task_refs: vec!["task:current".into()],
            future_reservation_refs: Vec::new(),
            selected_evidence: Vec::new(),
            omitted_soft_sources: Vec::new(),
            hard_tokens: 1024,
            soft_tokens: 0,
        };
        let payload = authority_context_payload(&input).expect("authority context serializes");
        assert_eq!(payload["projectRevision"], 7);
        assert_eq!(payload["activeTaskRefs"][0], "task:current");
        assert!(payload.get("state").is_some());
    }
}
