//! B08 chapter production composition root.
//!
//! This module deliberately reuses the existing Runtime, ModelRunner and
//! CommitBundle V2 authorities.  It does not introduce a second scheduler or a
//! second narrative-delta table.  The renderer and audit adapters below are the
//! deterministic local boundary used by the formal command path; a provider
//! adapter can replace their transport without changing the commit protocol.

use crate::chapter_craft::telling_pairs;
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
use crate::repositories::narrative::NarrativeRepository;
use crate::repositories::production::{ProductionRepository, NARRATIVE_REVIEW_REQUIRED};
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

#[derive(serde::Deserialize, serde::Serialize)]
struct ChapterProseCheckpoint {
    input_digest: String,
    body: String,
    body_digest: String,
    renderer: Value,
    prose_run_ids: Vec<String>,
    craft_skills: Value,
}

fn reusable_continuity_sources(
    checkpoint: &ChapterProseCheckpoint,
    craft_chapter: bool,
) -> Option<&Value> {
    let review = &checkpoint.craft_skills["continuity"];
    if review["errors"]
        .as_array()
        .is_some_and(|errors| !errors.is_empty())
        || (craft_chapter
            && (review["reviewVersion"].as_u64() != Some(CONTINUITY_REVIEW_VERSION)
                || review
                    .pointer("/finalCheck/bodyDigest")
                    .and_then(Value::as_str)
                    != Some(&checkpoint.body_digest)
                || review
                    .pointer("/finalCheck/modelRunId")
                    .and_then(Value::as_str)
                    .is_none()))
    {
        return None;
    }
    review.get("recalledSources")
}

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

    /// VD-1a: configured providers plan the book's direction after signing;
    /// deterministic gateways keep the template goals.
    fn plans_book_direction(&self) -> bool {
        self.enforces_chapter_length()
    }

    fn checks_book_continuation(&self) -> bool {
        false
    }

    /// Whether this gateway supports the optional evidenced event selection.
    fn selects_main_event(&self) -> bool {
        false
    }

    /// FX-21: configured providers classify project assistant questions;
    /// deterministic gateways keep the keyword routing.
    fn classifies_assistant_intent(&self) -> bool {
        false
    }

    /// Owner decision 2026-11-27: configured providers plan the assistant's
    /// queries and write its answer; deterministic gateways keep the fixed
    /// queries for each kind of question.
    fn plans_assistant_answers(&self) -> bool {
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

/// The settings step whose model a capability uses: `render` (writing),
/// `audit` (audit, review and plans) or `default`.
pub fn model_step(capability: &str) -> &'static str {
    match capability {
        // Real run round 30: every call on the writing model, so its
        // thinking reaches the craft writer too.
        "RENDER_CHAPTER"
        | "RENDER_CHAPTER_CRAFT"
        | "ADJUST_CHAPTER_LENGTH"
        | "CUT_CHAPTER"
        | "REWRITE_PASSAGES" => "render",
        // Owner decision (2026-10-24): r2 plans use the audit model.
        "AUDIT_EXTRACT"
        | "SELECT_MAIN_EVENT"
        | "VERIFY_STAGE_STEP"
        | "REVIEW_CHAPTER"
        | "CHECK_CONTINUITY"
        | "CONFIRM_NARRATIVE_CONTINUITY"
        | crate::book_direction::CHECK_BOOK_CONTINUATION
        | crate::book_direction::CONFIRM_BOOK_CONTINUATION
        | crate::book_direction::REPAIR_BOOK_PRESSURE
        | crate::craft_plan::PLAN_CHAPTER_CRAFT
        | crate::book_direction::PLAN_BOOK_DIRECTION
        | crate::book_direction::PLAN_VOLUME_DIRECTION => "audit",
        _ => "default",
    }
}

/// P4: the thinking saved for the step `capability` runs in, if any.
pub fn step_thinking(settings: &Value, capability: &str) -> Option<crate::provider::Thinking> {
    serde_json::from_value(settings["thinking"][model_step(capability)].clone()).ok()
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
        // P4: the step's thinking, as saved in the settings.
        let thinking = self
            .settings_paths
            .as_ref()
            .and_then(|paths| {
                crate::repositories::provider::ProviderSettingsRepository::read_global(paths).ok()
            })
            .and_then(|settings| step_thinking(&settings, capability));
        let request = ProviderRequest {
            capability: capability.into(),
            model_id: config.model_id.clone(),
            prompt: serialize_provider_input(input),
            timeout_ms: config.timeout_ms,
            thinking,
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
    fn checks_book_continuation(&self) -> bool {
        true
    }

    fn selects_main_event(&self) -> bool {
        true
    }
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

    fn plans_assistant_answers(&self) -> bool {
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
        let setting = match model_step(capability) {
            "render" => "renderModelId",
            "audit" => "auditModelId",
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
        Ok(mut payload) if capability == crate::craft_setup::GENERATE_STORY_BIBLE => {
            crate::craft_setup::strip_style_quotes(&mut payload);
            Ok(payload)
        }
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
    execute_production_model_with_profile_recorded(
        connection, gateway, profile, spec, capability, input, now,
    )
    .map(|(_, outcome)| outcome)
}

pub(crate) fn execute_production_model_with_profile_recorded(
    connection: &libsql_rusqlite::Connection,
    gateway: &dyn ProductionModelGateway,
    profile: &ResolvedProviderProfile,
    spec: &ModelRunSpec,
    capability: &str,
    input: Value,
    now: &str,
) -> Result<(String, ModelRunOutcome), String> {
    resend_once_on_invalid_json(spec, |spec, resend| {
        let mut input = input.clone();
        let feedback = match resend {
            Resend::OffSpec(error) => Some((
                error,
                "上一次回复不符合输出规范：按 rejectedFor 列出的问题修正，返回完整回复。",
            )),
            // Real run round 12: an audit listing one item a sentence was
            // cut off at the output limit, resent as it was, and cut off again.
            Resend::Unreadable => Some((
                "P01_MODEL_JSON_INVALID",
                "上一次回复不是完整的 JSON，多半是太长被截断了。重新返回完整回复：每个列表只留必需的条目，宁少勿多，不要一句正文对应一条。",
            )),
            Resend::First => None,
        };
        if let Some((rejected_for, instruction)) = feedback {
            input["outputFeedback"] = json!({"rejectedFor":rejected_for,"instruction":instruction});
        }
        let outcome = execute_production_model_with_profile_once(
            connection, gateway, profile, spec, capability, input, now,
        )?;
        Ok((spec.model_run_id.clone(), outcome))
    })
}

/// Which call of a step `resend_once_on_invalid_json` makes.
pub(crate) enum Resend<'a> {
    First,
    /// The first reply was not valid JSON (cut off or malformed).
    Unreadable,
    /// The first reply was off its step's output spec, as this says.
    OffSpec(&'a str),
}

/// Runs `run` with `spec`; a reply that is not valid JSON (FX-19) or not on
/// its step's output spec (LD-1) is resent once as a new model run, told
/// which; the bad run stays recorded as failed.
pub(crate) fn resend_once_on_invalid_json<T>(
    spec: &ModelRunSpec,
    mut run: impl FnMut(&ModelRunSpec, Resend<'_>) -> Result<T, String>,
) -> Result<T, String> {
    match run(spec, Resend::First) {
        Err(error) if error.contains("P01_MODEL_JSON_INVALID:") => run(
            &follow_up_spec(spec, ":truncated-resend", 1)?,
            Resend::Unreadable,
        ),
        Err(error) if error.contains(crate::model_output::OFF_SPEC) => run(
            &follow_up_spec(spec, ":off-spec-resend", 1)?,
            Resend::OffSpec(&error),
        ),
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

// A strict-output step may consume one resend before returning useful work.
const RUNS_PER_REVIEW_STEP: i64 = 2;
const CONTINUITY_REVIEW_VERSION: u64 = 5;
const EXPANSION_PASSAGES: usize = 3;
const EXPANSION_PASSAGE_MAX_CHARS: usize = 600;
const PRIOR_CHAPTER_COPY_ERROR: &str = "B08_RENDER_PRIOR_CHAPTER_COPY";
const RENDER_SIGNING_REPETITION_ERROR: &str = "B08_RENDER_SIGNING_REPETITION";

fn fits_before_final_evidence(used: i64, limit: i64, reserved: i64, proposed: i64) -> bool {
    limit.saturating_sub(used) >= reserved + proposed * RUNS_PER_REVIEW_STEP
}

/// The runs kept for the required steps after each point of a chapter, one
/// each (2026-11-25: two each left the rewrite no room in any chapter).
struct ReservedRuns {
    /// Picking the main event.
    event: i64,
    /// That, and verifying the active stage step.
    outcome: i64,
    /// Those, and the audit (an opening's also answers its feedback).
    submission: i64,
    /// Those, and confirming what the rewrite left.
    polish: i64,
}

impl ReservedRuns {
    fn new(has_stage_step: bool, opening: bool) -> Self {
        let event = 1;
        let outcome = event + i64::from(has_stage_step);
        let submission = outcome + 1 + i64::from(opening);
        Self {
            event,
            outcome,
            submission,
            polish: submission + 1,
        }
    }
}

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
    let craft_chapter = spec.capability == crate::craft_plan::PLAN_CHAPTER_CRAFT;
    let mut earlier_craft_plans = Vec::new();
    let mut rejected_goals = Vec::new();
    for round in 1..=PLAN_FEEDBACK_ROUNDS {
        let Some(mut feedback) = result
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
        if matches!(
            spec.capability.as_str(),
            crate::book_direction::PLAN_BOOK_DIRECTION
                | crate::book_direction::PLAN_VOLUME_DIRECTION
        ) {
            let mut previous = last.clone();
            crate::book_direction::tidy(&mut previous, &input);
            crate::book_direction::drop_replayed_steps(&mut previous, &feedback);
            feedback["previousDirection"] = previous;
        }
        if craft_chapter {
            if let Some(previous) = previous_plan(&last, &feedback) {
                feedback["previousPlan"] = previous;
            } else if !rejected_goals.contains(&last["chapterGoal"]) {
                rejected_goals.push(last["chapterGoal"].clone());
            }
            feedback["rejectedRepeatedGoals"] = json!(rejected_goals);
            earlier_craft_plans.push((run_id.clone(), last.clone()));
        }
        input["planFeedback"] = feedback;
        last = plan(&replan, input.clone())?;
        result = accept(last.clone(), false);
        run_id = replan.model_run_id;
    }
    // LD-1 step 2: problems the planner was told about and could not fix go
    // on with the last plan as a reminder; anything else still fails.
    if craft_chapter
        && result
            .as_ref()
            .is_err_and(|error| error == "B08_PLAN_GOAL_REPEATED")
    {
        // A retry can regress to a duplicate after an earlier draft found a
        // new goal. Reapply the normal acceptance rules to each earlier draft.
        for (candidate_run_id, candidate) in earlier_craft_plans.into_iter().rev() {
            if let Ok((plan, problems)) = accept(candidate, true) {
                return Ok((plan, candidate_run_id, problems));
            }
        }
    }
    let (plan, problems) = match result {
        Ok(accepted) => accepted,
        Err(error) if crate::chapter_craft::plan_feedback(&error).is_some() => accept(last, true)?,
        Err(error) => return Err(error),
    };
    Ok((plan, run_id, problems))
}

/// The rejected plan a replan starts from: without its ending or its scenes
/// when they were the problem, and not at all when its goal repeats a chapter (real run
/// 2026-11-25: given them back, the planner kept them in all three plans).
fn previous_plan(plan: &Value, feedback: &Value) -> Option<Value> {
    let problems = || {
        feedback["problems"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
    };
    if feedback["rejectedFor"] == "B08_PLAN_GOAL_REPEATED"
        || problems().any(crate::craft_plan::is_repeated_goal)
    {
        return None;
    }
    let mut craft = plan["craft"].clone();
    if let Some(craft) = craft.as_object_mut() {
        if problems().any(crate::craft_plan::is_repeated_hook) {
            craft.remove("endHook");
        }
        // Real run round 8: given its scenes back, the planner returned the
        // same plan three times.
        if problems().any(crate::craft_plan::is_scene_problem) {
            craft.remove("scenes");
        }
    }
    Some(json!({
        "chapterGoal":plan["chapterGoal"],"chapterRole":plan["chapterRole"],
        "craft":craft,"horizonPreview":plan["horizonPreview"],
    }))
}

/// In a lenient plan check (LD-1 step 2), a problem the planner could fix is
/// noted in `problems` instead of failing; everything else still fails.
pub(crate) fn note_if_lenient(
    lenient: bool,
    problems: &mut Vec<String>,
    result: Result<(), String>,
) -> Result<(), String> {
    match result {
        Err(error)
            if lenient
                && error != "B08_PLAN_GOAL_REPEATED"
                && crate::chapter_craft::plan_feedback(&error).is_some() =>
        {
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
    // P2: the skills this step was given (setup steps wrap their input).
    let guidance = [&input["craftGuidance"], &input["input"]["craftGuidance"]]
        .into_iter()
        .find(|guidance| guidance.is_object())
        .cloned();
    let mut transport = GatewayTransport::with_profile(gateway, profile, capability, input);
    DurableModelRunner::prepare(connection, &spec, now).map_err(|error| error.to_string())?;
    if let Some(guidance) = guidance {
        crate::repositories::runtime::record_model_craft_skills(
            connection,
            &spec,
            guidance["stage"].as_str().unwrap_or_default(),
            &crate::chapter_craft::guidance_skill_ids(&guidance),
            now,
        )
        .map_err(|error| error.to_string())?;
    }
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
        "evidence":item.evidence.as_ref().map(EvidenceSpan::to_json)
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

/// P1: open C-level threads the audit is shown.
const OPEN_THREADS_SHOWN: usize = 8;
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
    if let Some(expected) = plan["stageStepRulesDigest"].as_str() {
        let chapter_no = NarrativeRepository::chapter_number(connection, project_id, chapter_id)
            .map_err(|error| error.to_string())?
            .ok_or("B08_CHAPTER_NOT_FOUND")?;
        let current =
            crate::book_direction::for_chapter(connection, project_id, chapter_id, chapter_no)?;
        if current.as_ref().and_then(|direction| {
            direction
                .pointer("/currentStage/ruleDigest")
                .and_then(Value::as_str)
        }) != Some(expected)
        {
            return Err("PLAN_STALE:STAGE_STEP_RULES_CHANGED".into());
        }
    }
    let stage_tasks = plan["activeTaskRefs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|id| is_required_stage_step(id))
        .collect::<Vec<_>>();
    if execution_mode == "LONG_FORM" && !stage_tasks.is_empty() {
        if let Some(route) = crate::repositories::narrative::NarrativeRepository::artifact_payload(
            connection,
            project_id,
            "VolumePlan",
            volume_id,
        )
        .map_err(|error| error.to_string())?
        {
            let states =
                crate::planning::route_task_states(connection, project_id, volume_id, &route)?;
            let statuses = states
                .iter()
                .filter_map(|state| {
                    Some((state.state_id.as_str(), state.value["status"].as_str()?))
                })
                .collect::<Vec<_>>();
            crate::planning::validate_task_reality(
                &json!({"activeTaskRefs":stage_tasks}),
                &route,
                &statuses,
            )
            .map_err(|_| "PLAN_STALE:TASK_DEPENDENCY_CHANGED".to_string())?;
        }
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
    let Some(planning_model_run_id) =
        crate::repositories::narrative::NarrativeRepository::artifact_model_run_id(
            connection, project_id, &plan_ref,
        )
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    // A retry that reused a plan has no planner run of its own. The
    // canonical plan must have been accepted on this chapter's retry chain.
    let plan_is_from_retry_chain = ProductionRepository::plan_run_is_on_retry_chain(
        connection,
        project_id,
        previous_job_id,
        chapter_id,
        &planning_model_run_id,
    )
    .map_err(|error| error.to_string())?;
    if !plan_is_from_retry_chain {
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
    if validate_plan_is_current(
        connection,
        request,
        project_id,
        chapter_id,
        volume_id,
        "OPENING_BOOTSTRAP",
        &plan_ref,
        &plan_payload,
    )
    .is_ok()
    {
        return Ok(Some((
            context_ref,
            previous_context_payload,
            plan_ref,
            plan_payload,
        )));
    }
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
            if execution_mode == "LONG_FORM"
                && crate::signing::approved_book_plan_ref(connection, project_id)?.is_some()
            {
                crate::signing::require_current_signing_for_long_form_if_present(
                    connection, project_id,
                )?;
            }
            revalidate_chapter_gate_on_current_snapshot(
                connection, project_id, chapter_id, plan, bundle, now,
            )?;
        }
        if execution_mode == "OPENING_BOOTSTRAP" && gateway.enforces_chapter_length() {
            let final_body = bundle
                .artifacts
                .iter()
                .find(|artifact| artifact.artifact_type == ArtifactType::FinalChapter)
                .and_then(|artifact| artifact.payload["body"].as_str())
                .ok_or("B08_CANDIDATE_BODY_REQUIRED")?;
            let history = crate::repositories::ops::chapter_history(
                connection,
                project_id,
                chapter_id,
                crate::repositories::project_entry::OPENING_CHAPTER_COUNT,
            )
            .map_err(|error| error.to_string())?;
            if let Some(phrase) = crate::chapter_craft::signing_repeated_phrase_windows(
                history
                    .bodies
                    .iter()
                    .map(String::as_str)
                    .chain([final_body]),
            )
            .into_iter()
            .next()
            {
                return Err(format!("{RENDER_SIGNING_REPETITION_ERROR}:{phrase}"));
            }
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
    if let Some(parts) = audit_payload
        .pointer("/normalization/stageStepVerification/evidenceParts")
        .and_then(Value::as_array)
    {
        for part in parts.iter().filter(|part| part.get("source").is_some()) {
            let proof: StageStepEvidence = serde_json::from_value(part.clone())
                .map_err(|error| format!("B08_STAGE_STEP_PROGRESS_INVALID:{error}"))?;
            if !stage_step_source_is_current(connection, project_id, &proof)? {
                return Err("PLAN_STALE:STAGE_STEP_SOURCE_CHANGED".into());
            }
        }
    }
    let continuity_sources = artifacts
        .iter()
        .find(|item| item.artifact_type == ArtifactType::ProductionReceipt)
        .and_then(|item| {
            item.payload
                .pointer("/craftSkills/continuity/recalledSources")
        });
    if !continuity_sources_current(connection, project_id, continuity_sources)? {
        return Err("PLAN_STALE:CONTINUITY_SOURCE_CHANGED".into());
    }
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

fn continuity_sources_current(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    sources: Option<&Value>,
) -> Result<bool, String> {
    use crate::repositories::narrative::NarrativeRepository;
    for source in sources.and_then(Value::as_array).into_iter().flatten() {
        let chapter_id = source["chapterId"]
            .as_str()
            .ok_or("QC1_HISTORY_REFERENCE_INVALID")?;
        let reference: ArtifactRef = serde_json::from_value(source["finalRef"].clone())
            .map_err(|error| format!("QC1_HISTORY_REFERENCE_INVALID:{error}"))?;
        let current =
            NarrativeRepository::committed_final_chapter_head(connection, project_id, chapter_id)
                .map_err(|error| error.to_string())?;
        if current.as_ref() != Some(&reference) {
            return Ok(false);
        }
    }
    Ok(true)
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
        if crate::signing::approved_book_plan_ref(connection, &project_id)?.is_some() {
            crate::signing::require_current_signing_for_long_form_if_present(
                connection,
                &project_id,
            )?;
        }
        crate::book_direction::require_first_volume_direction(
            connection,
            gateway,
            &project_id,
            volume_id,
            true,
        )?;
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
    let continuity_hold = ProductionRepository::active_continuity_hold(connection, &project_id)
        .map_err(|error| error.to_string())?;
    if let Some((held_job, held_chapter)) = &continuity_hold {
        if held_chapter != &chapter_id || retry_of_job_id.as_ref() != Some(held_job) {
            return Err(NARRATIVE_REVIEW_REQUIRED.into());
        }
    }
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
    let enforce_opening_repetition =
        execution_mode == "OPENING_BOOTSTRAP" && gateway.enforces_chapter_length();
    if enforce_opening_repetition {
        if let Some(phrase) = crate::chapter_craft::signing_repeated_phrase_windows(
            chapter_history.bodies.iter().map(String::as_str),
        )
        .into_iter()
        .next()
        {
            return Err(format!("B08_OPENING_HISTORY_SIGNING_REPETITION:{phrase}"));
        }
    }
    let signing_repetition = |body: &str| {
        if !enforce_opening_repetition {
            return None;
        }
        crate::chapter_craft::signing_repeated_phrase_windows(
            chapter_history
                .bodies
                .iter()
                .map(String::as_str)
                .chain([body]),
        )
        .into_iter()
        .next()
    };
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
            if planner.capability() == crate::craft_plan::PLAN_CHAPTER_CRAFT {
                chapter_history.extend_continuation_input(&mut planning_input);
            }
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
                problems.extend(planner.reminders(&plan));
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
    let stage_step = match chapter_number {
        Some(number) if execution_mode == "LONG_FORM" && gateway.selects_main_event() => {
            planned_stage_step(connection, &project_id, &chapter_id, number, &plan_payload)?
        }
        _ => None,
    };
    // Owner decision (2026-10-24): the v1.2 enrichment, its quality report
    // and S40 candidate repair are retired from chapter production; the v2
    // craft skills and the rule checks guide the writing instead.

    update_stage(manager, connection, &project_id, &job_id, "RENDER", &now)?;
    // LD-7 (real run 2026-11-01, round 11): a writer given each scene's
    // estimate wrote about two thirds of it, so the writers aim for the
    // chapter's range. Plans no longer estimate (round 10); older ones may.
    let mut writer_plan = plan_payload.clone();
    for scene in writer_plan
        .pointer_mut("/craft/scenes")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object_mut)
    {
        scene.remove("estimatedCharacters");
    }
    // Shown the coming chapters' goals, a writer writes them early.
    if let Some(plan) = writer_plan.as_object_mut() {
        plan.remove("horizonPreview");
    }
    let mut render_input = json!({"chapterId":chapter_id,"chapterPlanRef":plan_ref,"chapterPlan":writer_plan,"contextManifestRef":context_ref,"contextManifest":context_payload,"authorityContext":authority_context.clone(),"chapterLengthRange":length_range.to_json()});
    if let Some(stage) = &stage_step {
        render_input["stageStep"] = stage.input();
    }
    let mut avoid_phrases = crate::chapter_craft::repeated_phrases(&chapter_history.bodies);
    avoid_phrases.extend(crate::chapter_craft::recent_short_lines(
        &chapter_history.bodies,
    ));
    if !avoid_phrases.is_empty() {
        render_input["avoidPhrases"] = json!(avoid_phrases);
    }
    let mut avoid_tics = crate::chapter_craft::overused_tics(&chapter_history.bodies);
    // QC-1.4: and the catchphrases the continuity check named recently.
    for phrase in &chapter_history.catchphrases {
        if !avoid_tics.contains(phrase) {
            avoid_tics.push(phrase.clone());
        }
    }
    if !avoid_tics.is_empty() {
        render_input["avoidTics"] = json!(avoid_tics);
    }
    // QC-1 (2026-10-30): details the recent chapters kept writing.
    if !chapter_history.overused_details.is_empty() {
        render_input["avoidDetails"] = json!(chapter_history.overused_details);
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
        chapter_history.extend_continuation_input(&mut render_input);
    }
    // QC-1.5: what earlier chapters fixed, for the continuity check.
    let established_facts = render_input
        .get("establishedFacts")
        .cloned()
        .unwrap_or_else(|| json!([]));
    // QC-1.4/1.5: what the rewrite left for the chapter's reminder.
    let mut continuity_held = Vec::new();
    let mut confirmed_continuity_blockers = Vec::new();
    let render_capability = if craft_writing.is_some() {
        "RENDER_CHAPTER_CRAFT"
    } else {
        "RENDER_CHAPTER"
    };
    let render_profile = gateway.resolve_step_profile("RENDER_CHAPTER")?;
    // The project revision in authorityContext can change when a job is
    // retried. The plan, evidence context and authority digest still bind it.
    let prose_input_digest = if gateway.enforces_chapter_length() {
        let mut prose_input = render_input.clone();
        prose_input["authorityContext"] = Value::Null;
        Some(sha256_canonical(&json!({
            "renderInput":prose_input,"stateDigest":state_digest,"gatePolicyRef":policy_ref,
            "previousBodies":chapter_history.bodies,"reviewsProse":gateway.reviews_chapter_prose()
        })).map_err(|error| error.to_string())?)
    } else {
        None
    };
    let copies_prior_chapter = |text: &str| {
        if !gateway.enforces_chapter_length() || chapter_history.bodies.is_empty() {
            return false;
        }
        let normalized = crate::chapter_craft::normalize_typography(text.trim());
        !normalized.is_empty() && chapter_history.bodies.contains(&normalized)
    };
    let prior_copy_feedback = json!({
        "rejectedFor":PRIOR_CHAPTER_COPY_ERROR,
        "instruction":"上一次输出整篇复用了已提交的前文。前文是历史依据，只写本章章纲要求的新行动及结果，接着前章结尾推进。"
    });
    let mut copied_checkpoint = false;
    let mut reuse_checkpoint =
        |stage| match (prose_input_digest.as_ref(), retry_of_job_id.as_deref()) {
            (Some(digest), Some(previous_job_id)) if continuity_hold.is_none() => Ok::<_, String>(
                DurableJobService::checkpoint_boundary(
                    connection,
                    &project_id,
                    previous_job_id,
                    stage,
                )
                .map_err(|error| error.to_string())?
                .map(serde_json::from_value::<ChapterProseCheckpoint>)
                .transpose()
                .map_err(|error| format!("B08_PROSE_CHECKPOINT_INVALID:{error}"))?
                .filter(|checkpoint| &checkpoint.input_digest == digest)
                .filter(|checkpoint| {
                    let copied = copies_prior_chapter(&checkpoint.body);
                    copied_checkpoint |= copied;
                    !copied
                }),
            ),
            _ => Ok(None),
        };
    let audit_checkpoint = reuse_checkpoint("AUDIT")?;
    // Older releases could save a body that fails F1-5 at the audit
    // boundary. Keep that text as a draft and give the prose review a chance.
    let (resumed_prose, audit_draft) = match audit_checkpoint {
        Some(checkpoint) if signing_repetition(&checkpoint.body).is_some() => {
            (None, Some(checkpoint))
        }
        checkpoint => (checkpoint, None),
    };
    let prose_resumed = resumed_prose.is_some();
    let resumed_draft = if prose_resumed || audit_draft.is_some() {
        audit_draft
    } else {
        reuse_checkpoint("RENDER")?
    };
    let review_sources = resumed_prose
        .as_ref()
        .and_then(|checkpoint| reusable_continuity_sources(checkpoint, craft_writing.is_some()));
    let review_resumed = if let Some(sources) = review_sources {
        continuity_sources_current(connection, &project_id, Some(sources))?
    } else {
        false
    };
    let original_renderer = resumed_prose
        .as_ref()
        .or(resumed_draft.as_ref())
        .map(|checkpoint| checkpoint.renderer.clone());
    let mut prose_run_ids = Vec::new();
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
        execute_production_model_with_profile_recorded(
            connection, gateway, profile, &spec, capability, input, &now,
        )
    };
    let reserved = ReservedRuns::new(stage_step.is_some(), execution_mode == "OPENING_BOOTSTRAP");
    let has_review_room = |reserved, proposed| {
        if !gateway.selects_main_event() {
            return Ok(true);
        }
        let quota = ProductionRepository::chapter_quota(connection, &project_id, &chapter_id)
            .map_err(|error| error.to_string())?
            .ok_or("B08_PRODUCTION_BUDGET_REQUIRED")?;
        Ok::<_, String>(fits_before_final_evidence(
            quota["used"].as_i64().unwrap_or(0),
            quota["limit"].as_i64().unwrap_or(0),
            reserved,
            proposed,
        ))
    };
    let rechecking_saved_prose = prose_resumed && !review_resumed;
    let room_for_polish = |proposed| {
        if rechecking_saved_prose {
            Ok(false)
        } else {
            has_review_room(reserved.polish, proposed)
        }
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
    // Gateways without sizing can resume from their initial renderer output.
    // Sized prose uses the verified body saved at the relevant boundary.
    let reusable_render = retry_of_job_id
        .as_deref()
        .filter(|_| !gateway.enforces_chapter_length() && continuity_hold.is_none())
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
    let rendered_body = |result: &ModelRunOutcome| {
        result
            .parsed
            .as_ref()
            .and_then(|payload| payload.get("body"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| "B08_RENDER_OUTPUT_INVALID".to_string())
    };
    let (render_run_id, mut body) = if let Some(checkpoint) = resumed_prose.or(resumed_draft) {
        let body_hash =
            sha256_canonical(&json!(checkpoint.body)).map_err(|error| error.to_string())?;
        if body_hash != checkpoint.body_digest {
            return Err("B08_PROSE_CHECKPOINT_INVALID".into());
        }
        let model_run_id = checkpoint.renderer["modelRunId"]
            .as_str()
            .ok_or("B08_PROSE_CHECKPOINT_INVALID")?
            .to_owned();
        prose_run_ids = checkpoint.prose_run_ids;
        craft_skills = checkpoint.craft_skills;
        continuity_held = craft_skills["continuity"]["unresolved"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let resumed_field = if prose_resumed {
            "proseResumedFromJob"
        } else {
            "draftResumedFromJob"
        };
        craft_skills[resumed_field] = json!(retry_of_job_id);
        (model_run_id, checkpoint.body)
    } else if let Some((model_run_id, parsed)) = reusable_render {
        let payload = parse_provider_model_payload_for_capability(parsed, render_capability)?;
        let body = payload
            .get("body")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| "B08_RENDER_OUTPUT_INVALID".to_string())?;
        (model_run_id, body)
    } else {
        if copied_checkpoint {
            render_input["outputFeedback"] = prior_copy_feedback.clone();
        }
        let (model_run_id, result) = craft_call(
            "render",
            render_capability,
            &render_profile,
            1,
            render_input.clone(),
        )?;
        let body = rendered_body(&result)?;
        let signing_repeat = |text: &str| {
            if !enforce_opening_repetition || craft_writing.is_none() {
                return None;
            }
            let normalized = crate::chapter_craft::normalize_typography(text.trim());
            signing_repetition(&normalized)
        };
        let sized = gateway.enforces_chapter_length();
        let length = character_count(&body);
        // LD-2 (real runs rounds 22 and 26): a chapter barely broken into
        // paragraphs is written again once, told to break it up.
        let render_recovery = if copies_prior_chapter(&body) {
            Some(("render-prior-copy", prior_copy_feedback.clone()))
        } else if let Some(phrase) = signing_repeat(&body) {
            Some((
                "render-signing-repeat",
                json!({
                    "rejectedFor":RENDER_SIGNING_REPETITION_ERROR,
                    "repeatedPhrase":phrase,
                    "instruction":"该短语在本章和已提交的开篇中累计出现三次或更多。保留本章章纲和新情节，重写这些句子，只把已发生的结果作为前提。"
                }),
            ))
        } else if sized && length_range.is_far_off(length) {
            // Round 11 (owner decision 2026-11-26): cutting a quarter of a
            // draft orphaned its lines, and padding one a quarter short
            // repeated a conversation, so either is written again once.
            let (suffix, how) = if length > length_range.max {
                (
                    "render-too-long",
                    "只写章纲列出的事件，每件写一次；前几章写过的场面和对话只作前提，不再演一遍；一场对话只留推动结果的几句",
                )
            } else {
                (
                    "render-too-short",
                    "把章纲的每件事写足：谁做了什么、说了什么、对方怎么回应、付出什么代价；不加新事件，不靠景物描写凑字",
                )
            };
            Some((
                suffix,
                json!({
                    "rejectedFor":length_error(&body),
                    "instruction":format!(
                        "上一稿有 {length} 字，要求 {}–{} 字（不算空格和换行）。重写这一章：{how}。",
                        length_range.min, length_range.max
                    )
                }),
            ))
        } else if sized && length >= length_range.min && crate::chapter_craft::is_unbroken(&body) {
            Some((
                "render-paragraphs",
                json!({
                    "rejectedFor":"正文分段太少：有超过 300 字的段落",
                    "instruction":"上一次的正文段落太长，几百字连成一段。重写这一章，按场景、动作和对话分段，每段一般不超过 150 字，段与段之间用换行（\\n）隔开。"
                }),
            ))
        } else {
            None
        };
        if let Some((suffix, feedback)) = render_recovery {
            let mut input = render_input.clone();
            input["outputFeedback"] = feedback;
            let (model_run_id, result) =
                craft_call(suffix, render_capability, &render_profile, 2, input)?;
            let body = rendered_body(&result)?;
            if copies_prior_chapter(&body) {
                return Err(PRIOR_CHAPTER_COPY_ERROR.into());
            }
            // Owner decision 2026-11-25: a second draft still this long is
            // not cut down; the chapter fails.
            if sized && length_range.is_too_long_to_cut(character_count(&body)) {
                return Err(length_error(&body));
            }
            // A repeat the second draft kept is left to the rewrite; the check
            // before the audit still refuses one it leaves.
            (model_run_id, body)
        } else {
            (model_run_id, body)
        }
    };
    let renderer = original_renderer.unwrap_or_else(|| {
        json!({
            "modelRunId":render_run_id,"capability":render_capability,
            "provider":render_profile.provider,"model":render_profile.model
        })
    });
    let save_prose =
        |stage: &str, text: &str, run_ids: &[String], skills: &Value| -> Result<(), String> {
            if let Some(input_digest) = prose_input_digest.as_ref() {
                let checkpoint = ChapterProseCheckpoint {
                    input_digest: input_digest.clone(),
                    body: text.to_owned(),
                    body_digest: sha256_canonical(&json!(text))
                        .map_err(|error| error.to_string())?,
                    renderer: renderer.clone(),
                    prose_run_ids: run_ids.to_vec(),
                    craft_skills: skills.clone(),
                };
                let stored = serde_json::to_value(checkpoint).map_err(|error| error.to_string())?;
                DurableJobService::save_checkpoint(
                    connection,
                    &project_id,
                    &job_id,
                    stage,
                    0,
                    &stored,
                    &now,
                )
                .map_err(|error| error.to_string())?;
            }
            Ok(())
        };
    // Craft v2 (owner decision 2026-10-19): the written chapter is read once
    // with the review skills, and passages it flags are rewritten in place
    // with the rewrite skills. Only edits whose passage occurs once apply,
    // and a rewrite that leaves the length range is dropped whole.
    // A chapter outside the length range is expanded or cut, in at most two
    // rounds; it is never committed outside the range.
    use crate::chapter_craft::duplicate_in_chapter;
    if gateway.enforces_chapter_length() && !prose_resumed {
        let in_range = |text: &str| length_range.accepts(character_count(text));
        let tidy = |text: &str| {
            crate::chapter_craft::break_long_paragraphs(
                &crate::chapter_craft::normalize_typography(text.trim()),
            )
        };
        let save_draft = |text: &str, run_ids: &[String], skills: &Value| {
            if duplicate_in_chapter(text).is_none() {
                save_prose("RENDER", text, run_ids, skills)?;
            }
            Ok::<_, String>(())
        };
        body = tidy(&body);
        save_draft(&body, &prose_run_ids, &craft_skills)?;
        let mut length = character_count(&body);
        if length_range.is_far_too_long(length) {
            // Round 13 (owner decision 2026-11-27): cutting a fifth or more
            // of a redraw left lines without their setup (real run round
            // 12), so the writer shortens its own draft once; one still far
            // too long fails.
            let mut input = render_input;
            input["outputFeedback"] = json!({
                "rejectedFor":length_error(&body),
                "previousDraft":body,
                "instruction":format!(
                    "上一稿有 {length} 字，要求 {}–{} 字（不算空格和换行）。把 previousDraft 压缩到 {} 字左右：每件事、每段对话的来回和结尾都留下，删走路、等待、重复确认和不改变局面的描写，不加新内容。",
                    length_range.min, length_range.max, length_range.target()
                )
            });
            let (run_id, outcome) = craft_call(
                "compress-length",
                render_capability,
                &render_profile,
                3,
                input,
            )?;
            prose_run_ids.push(run_id);
            body = tidy(&rendered_body(&outcome)?);
            length = character_count(&body);
            if length_range.is_far_too_long(length) {
                return Err(length_error(&body));
            }
            save_draft(&body, &prose_run_ids, &craft_skills)?;
        }
        if length < length_range.min {
            // LD-7 (real run 2026-11-01, round 11): one expansion added
            // 100-300 characters to chapters 300-600 short, so one still
            // too short gets a second round.
            let mut adjustment_feedback = craft_skills["lengthAdjustmentFeedback"].clone();
            for round in 0..2 {
                let length = character_count(&body);
                if length >= length_range.min {
                    break;
                }
                // The first round keeps its original id so earlier runs replay.
                let suffix = match round {
                    0 => "adjust-length".to_string(),
                    round => format!("adjust-length-{round}"),
                };
                let mut adjustment = json!({
                    "chapterId":chapter_id,"chapterPlan":writer_plan,
                    "currentChars":length,
                    "targetChars":length_range.target(),
                    "chapterLengthRange":length_range.to_json()
                });
                let anchors = chapter_expansion_anchors(&body);
                let patch_only = duplicate_in_chapter(&body).is_none() && !anchors.is_empty();
                if patch_only {
                    adjustment["patchOnly"] = json!(true);
                    adjustment["bodySentences"] =
                        json!(crate::chapter_craft::numbered_body_sentences(&body));
                    adjustment["insertionAnchors"] = json!(anchors.keys().collect::<Vec<_>>());
                    adjustment["avoidPhrases"] = json!(crate::chapter_craft::repeated_phrases(
                        &chapter_history
                            .bodies
                            .iter()
                            .cloned()
                            .chain([body.clone()])
                            .collect::<Vec<_>>(),
                    ));
                    let target = (length_range.min + 24).min(length_range.max).min(
                        length
                            + (anchors.len().min(EXPANSION_PASSAGES) * EXPANSION_PASSAGE_MAX_CHARS)
                                as i64,
                    );
                    adjustment["targetChars"] = json!(target);
                } else {
                    adjustment["body"] = json!(body);
                }
                if !adjustment_feedback.is_null() {
                    adjustment["outputFeedback"] = adjustment_feedback.clone();
                }
                let (run_id, outcome) = craft_call(
                    &suffix,
                    "ADJUST_CHAPTER_LENGTH",
                    &render_profile,
                    2 + round,
                    adjustment,
                )?;
                prose_run_ids.push(run_id);
                let payload = outcome
                    .parsed
                    .as_ref()
                    .ok_or("B08_ADJUST_CHAPTER_LENGTH_OUTPUT_INVALID")?;
                let mut expanded = if patch_only {
                    let Some(patched) =
                        insert_chapter_passages(&body, &payload["insertions"], &anchors)
                            .filter(|text| character_count(text) <= length_range.ceiling())
                    else {
                        adjustment_feedback = json!({
                            "code":"B08_ADJUST_CHAPTER_LENGTH_OUTPUT_INVALID",
                            "instruction":"只返回补写内容。afterSentence 只能选 insertionAnchors 中的编号，每个位置只补一次。补写后总字数不能超过上限。"
                        });
                        continue;
                    };
                    patched
                } else {
                    payload["body"]
                        .as_str()
                        .map(|text| crate::chapter_craft::normalize_typography(text.trim()))
                        .ok_or("B08_ADJUST_CHAPTER_LENGTH_OUTPUT_INVALID")?
                };
                let mut retained_feedback = Value::Null;
                if let Some(span) = duplicate_in_chapter(&expanded) {
                    let feedback = json!({
                        "code":"B08_RENDER_DUPLICATE_SPAN","repeatedSpan":span,
                        "instruction":"上次扩写重复了一整段。以本次提供的正文为底稿，只补新的动作、反应或结果。同一段不要出现两次。"
                    });
                    let retained = if patch_only {
                        retain_nonduplicating_insertions(&body, &payload["insertions"], &anchors)
                    } else {
                        None
                    };
                    if let Some(retained) = retained {
                        expanded = retained;
                        retained_feedback = feedback;
                    } else {
                        adjustment_feedback = feedback;
                        if duplicate_in_chapter(&body).is_none() {
                            craft_skills["lengthAdjustmentFeedback"] = adjustment_feedback.clone();
                            save_prose("RENDER", &body, &prose_run_ids, &craft_skills)?;
                        }
                        continue;
                    }
                }
                if enforce_opening_repetition {
                    let repeated = crate::chapter_craft::added_overused_phrase(
                        &chapter_history.bodies,
                        &body,
                        &expanded,
                    )
                    .or_else(|| {
                        if patch_only {
                            crate::chapter_craft::added_copied_sentence(&body, &expanded)
                                .map(ToOwned::to_owned)
                        } else {
                            None
                        }
                    });
                    if let Some(phrase) = repeated {
                        adjustment_feedback = json!({
                            "code":"B08_RENDER_REPEATED_PHRASES","repeatedPhrase":phrase,
                            "instruction":"补写重复了原有整句，或增加了超限短语。保留底稿，仅补新的行动、证据或结果，补写中不要复用所列原文。"
                        });
                        craft_skills["lengthAdjustmentFeedback"] = adjustment_feedback.clone();
                        save_prose("RENDER", &body, &prose_run_ids, &craft_skills)?;
                        continue;
                    }
                }
                adjustment_feedback = if in_range(&expanded) {
                    Value::Null
                } else {
                    retained_feedback
                };
                if character_count(&expanded) <= length {
                    break;
                }
                if adjustment_feedback.is_null() {
                    if let Some(skills) = craft_skills.as_object_mut() {
                        skills.remove("lengthAdjustmentFeedback");
                    }
                } else {
                    craft_skills["lengthAdjustmentFeedback"] = adjustment_feedback.clone();
                }
                body = expanded;
                save_prose("RENDER", &body, &prose_run_ids, &craft_skills)?;
            }
            if adjustment_feedback["code"] == "B08_RENDER_DUPLICATE_SPAN" && !in_range(&body) {
                return Err("B08_RENDER_DUPLICATE_SPAN".into());
            }
            if adjustment_feedback["code"] == "B08_RENDER_REPEATED_PHRASES" && !in_range(&body) {
                return Err("B08_RENDER_REPEATED_PHRASES".into());
            }
        } else {
            let events = crate::chapter_length::planned_events(&plan_payload);
            // LD-4 (real run 2026-11-01): asked to compress, the writer cut
            // 0-10% of chapters a third too long; it now only chooses the
            // sentences, and they are cut here to the range's maximum.
            // It chose too few once in a while, so a cut that still leaves the chapter
            // too long gets a second round.
            for round in 0..2 {
                let length = character_count(&body);
                if length_range.accepts(length) {
                    break;
                }
                let spans = crate::chapter_craft::sentence_spans(&body);
                let (groups, cuttable) = crate::chapter_length::cut_options(&body, &spans, &events);
                // Round 10: the sentences of one conversation share its first
                // number, so the cut chooses it whole or leaves it.
                let sentences = spans.iter()
                    .enumerate()
                    .map(|(number, &(from, to))| json!({
                        "n":number,"chars":character_count(&body[from..to]),"text":&body[from..to],
                        "canCut":cuttable[number],
                        "conversation":(groups[number].len() > 1).then(|| groups[number][0])
                    }))
                    .collect::<Vec<_>>();
                let (run_id, outcome) = craft_call(
                    &format!("cut-length-{round}"),
                    "CUT_CHAPTER",
                    &render_profile,
                    2 + round,
                    json!({
                        "chapterId":chapter_id,"chapterPlan":writer_plan,"sentences":sentences,
                        "currentChars":length,
                        "targetChars":length_range.max,
                        "cutChars":length - length_range.max
                    }),
                )?;
                prose_run_ids.push(run_id);
                let cut = outcome
                    .parsed
                    .as_ref()
                    .and_then(|payload| payload.get("cut"))
                    .and_then(Value::as_array)
                    .ok_or_else(|| "B08_CUT_CHAPTER_OUTPUT_INVALID".to_string())?
                    .iter()
                    .filter_map(Value::as_u64)
                    .map(|number| number as usize)
                    .collect::<Vec<_>>();
                body = crate::chapter_length::cut_sentences(&body, &cut, length_range, &events);
                if duplicate_in_chapter(&body).is_none() {
                    save_prose("RENDER", &body, &prose_run_ids, &craft_skills)?;
                }
                if character_count(&body) == length {
                    break;
                }
            }
        }
        if !in_range(&body) {
            return Err(length_error(&body));
        }
    }
    // A saved body can survive a budget pause with unfinished review. Reserve
    // the final check and up to two confirmations before reopening that work.
    let review_due = gateway.reviews_chapter_prose() && !review_resumed;
    let review_has_room =
        !rechecking_saved_prose || (review_due && has_review_room(reserved.submission, 3)?);
    if review_due && review_has_room {
        let mut checked_bodies = BTreeMap::new();
        let mut craft_step = |suffix: &str,
                              capability: &str,
                              profile: &ResolvedProviderProfile,
                              input: Value,
                              reviewed_body: Option<&str>|
         -> Result<Result<Value, String>, String> {
            let (run_id, outcome) = match craft_call(suffix, capability, profile, 1, input) {
                Ok(result) => result,
                Err(error) if error.contains("UNKNOWN_REMOTE_OUTCOME") => return Err(error),
                Err(error) => return Ok(Err(error)),
            };
            if let Some(text) = reviewed_body.filter(|_| outcome.parsed.is_some()) {
                let digest = sha256_canonical(&json!(text)).map_err(|error| error.to_string())?;
                checked_bodies.insert(digest, run_id.clone());
            }
            prose_run_ids.push(run_id);
            Ok(outcome
                .parsed
                .ok_or_else(|| format!("B08_{capability}_OUTPUT_INVALID")))
        };
        let codes = crate::skills_v2::finding_vocabulary()?;
        // CRAFT v3 chapters are checked by rule only (CRAFT_V3_DESIGN §4.4);
        // model editing belongs to the prose optimizer.
        let review = if craft_writing.is_some() || !room_for_polish(2)? {
            json!({})
        } else {
            let review_guidance = crate::chapter_craft::guidance("review", &craft_signals)?;
            craft_skills["review"] = json!(skill_ids(&review_guidance));
            // The review only advises: when it fails, the chapter goes on
            // with the rule findings and the failure is recorded.
            craft_step(
                "review",
                "REVIEW_CHAPTER",
                &gateway.resolve_step_profile("REVIEW_CHAPTER")?,
                json!({"chapterId":chapter_id,"chapterGoal":plan_payload.get("chapterGoal"),"sceneExecution":plan_payload.get("sceneExecution"),"body":body,"findingCodes":codes,"craftGuidance":review_guidance}),
                None,
            )?
            .unwrap_or_else(|error| json!({"reviewError":error}))
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
        let mut candidates = Vec::new();
        // RP-1 §5: two-character images, for the check to judge as well.
        let mut motif_candidates = Vec::new();
        let mut earlier_facts = Vec::new();
        let mut continuity_input = None;
        let mut continuity_errors = Vec::new();
        let prior_chapters = chapter_history
            .recent
            .iter()
            .zip(&chapter_history.bodies)
            .filter(|(_, body)| !body.is_empty())
            .map(|(chapter, body)| {
                numbered_prior_chapter(&chapter["chapterNo"], &chapter["chapterId"], body)
            })
            .collect::<Vec<_>>();
        // Real run round 8: chapter 8 redid chapter 3's rubbing, out of
        // reach of the three latest chapters; replays are looked for in the
        // opening too.
        let opening_count = crate::repositories::project_entry::OPENING_CHAPTER_COUNT;
        let replay_chapters = with_opening_chapters(
            &prior_chapters,
            &NarrativeRepository::final_bodies_by_chapter(connection, &project_id, opening_count)
                .map_err(|error| error.to_string())?
                .into_iter()
                .filter(|(chapter_no, _)| (1..=opening_count).contains(chapter_no))
                .collect::<Vec<_>>(),
            chapter_number.map(i64::from),
        );
        let covered = prior_chapters
            .iter()
            .filter(|entry| entry["excerpts"] == false)
            .filter_map(|entry| entry["chapterId"].as_str())
            .collect::<Vec<_>>();
        let outcome_chapters = chapter_number
            .filter(|_| craft_writing.is_some())
            .map_or_else(Vec::new, |number| {
                crate::repositories::ops::continuity_chapters_with_outcomes(
                    connection,
                    &project_id,
                    i64::from(number),
                    &covered,
                )
                .unwrap_or_else(|error| {
                    continuity_errors.push(format!("QC1_HISTORY_LOOKUP_FAILED:{error}"));
                    Vec::new()
                })
            });
        let recall_history = |text: &str| -> Result<Vec<Value>, String> {
            let mut history = prior_chapters.clone();
            if let Some(number) = chapter_number.filter(|_| craft_writing.is_some()) {
                let anchors = crate::chapter_craft::continuity_anchors(text);
                let matching_chapters = crate::repositories::ops::continuity_chapters_for_anchors(
                    connection,
                    &project_id,
                    i64::from(number),
                    &anchors,
                    &covered,
                )
                .map_err(|error| format!("QC1_HISTORY_LOOKUP_FAILED:{error}"))?;
                let chapters = outcome_chapters
                    .iter()
                    .chain(matching_chapters.iter().filter(|chapter| {
                        !outcome_chapters
                            .iter()
                            .any(|prior| prior.chapter_id == chapter.chapter_id)
                    }))
                    .collect::<Vec<_>>();
                crate::chapter_craft::add_recalled_continuity_prose(&mut history, &chapters, text);
            }
            Ok(history)
        };
        let mut evidence_history = recall_history(&body).unwrap_or_else(|error| {
            continuity_errors.push(error);
            prior_chapters.clone()
        });
        if let Some(writing) = &craft_writing {
            let (present, names) = writing.chapter_people(&plan_payload);
            let names = names.iter().map(String::as_str).collect::<Vec<_>>();
            let ledger_facts =
                crate::repositories::ops::subject_facts(connection, &project_id, &present, 40)
                    .map_err(|error| error.to_string())?;
            earlier_facts = crate::chapter_craft::earlier_facts(&established_facts, &ledger_facts);
            // What the story names (people, objects, places, devices) is
            // never a catchphrase, however often it recurs.
            let story_text = format!(
                "{}{}{}",
                writing.bible,
                plan_payload["craft"],
                json!(earlier_facts)
            );
            candidates = crate::chapter_craft::catchphrase_candidates(
                &chapter_history.bodies,
                &body,
                &names,
                &story_text,
            );
            motif_candidates =
                crate::chapter_craft::motif_candidates(&chapter_history.bodies, &body, &names);
            continuity_input = Some(json!({
                "chapterNumber":chapter_number,
                "earlierFacts":earlier_facts,
                "motifCandidates":motif_candidates
            }));
        }
        let continuity_profile = gateway.resolve_step_profile("CHECK_CONTINUITY")?;
        // The check's request for `text` (with the phrase candidates when
        // given) and the body's sentences it cites by number.
        let continuity_request = |text: &str, candidates: &[Value], history: &[Value]| {
            continuity_input.clone().map(|mut input| {
                let sentences = body_sentences(text)
                    .into_iter()
                    .map(|(_, _, sentence)| sentence)
                    .collect::<Vec<_>>();
                let numbered = crate::chapter_craft::numbered_body_sentences(text);
                input["priorChapters"] = json!(history);
                input["possibleRepeatedTransitions"] = json!(
                    crate::chapter_craft::recurring_transition_candidates(history, &numbered)
                );
                input["possibleReusedPassages"] = json!(
                    crate::chapter_craft::recurring_outcome_candidates(history, &numbered, 8)
                );
                input["bodySentences"] = json!(numbered);
                input["phraseCandidates"] = json!(candidates);
                (input, sentences)
            })
        };
        // What a check reports that holds up. The check only advises: a
        // failed call or an untrusted reply leaves the chapter as it is and
        // is recorded.
        let judge = |checked: Result<Value, String>,
                     sentences: &[String],
                     candidates: &[Value],
                     errors: &mut Vec<String>| {
            let checked = match checked {
                Ok(checked) => checked,
                Err(error) => {
                    errors.push(error);
                    return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
                }
            };
            let contradictions = crate::chapter_craft::credible_contradictions(
                checked["contradictions"]
                    .as_array()
                    .map_or(&[], Vec::as_slice),
                &earlier_facts,
                sentences,
                chapter_number.map_or(1, i64::from),
            )
            .unwrap_or_else(|error| {
                errors.push(error);
                Vec::new()
            });
            let catchphrases = checked["catchphrases"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|phrase| {
                    candidates
                        .iter()
                        .any(|candidate| candidate["phrase"] == *phrase)
                })
                .map(ToOwned::to_owned)
                .collect();
            let motifs = checked["motifs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter(|word| {
                    motif_candidates
                        .iter()
                        .any(|candidate| candidate["word"] == *word)
                })
                .map(ToOwned::to_owned)
                .collect();
            let narrative_issues = checked["narrativeIssues"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            (contradictions, catchphrases, motifs, narrative_issues)
        };
        let (contradictions, catchphrases, motifs, narrative_issues) =
            match continuity_request(&body, &candidates, &evidence_history) {
                Some((input, sentences)) => judge(
                    craft_step(
                        "continuity",
                        "CHECK_CONTINUITY",
                        &continuity_profile,
                        input,
                        Some(&body),
                    )?,
                    &sentences,
                    &candidates,
                    &mut continuity_errors,
                ),
                None => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
            };
        let narrative_findings = |reported: &[Value], text: &str, history: &[Value]| {
            let mut findings =
                crate::chapter_craft::replayed_passage_findings(&replay_chapters, text);
            let other = crate::chapter_craft::replayed_transition_findings(&replay_chapters, text)
                .into_iter()
                .chain(crate::chapter_craft::replayed_dialogue_findings(
                    &chapter_history.recent,
                    &chapter_history.bodies,
                    text,
                ))
                .chain(crate::chapter_craft::narrative_continuity_findings(
                    reported, history, text,
                ));
            for finding in other {
                let covered = finding["code"] == "repeated_event"
                    && findings.iter().any(|prior| {
                        prior["code"] == finding["code"]
                            && finding["quote"].as_str().is_some_and(|quote| {
                                prior["quote"]
                                    .as_str()
                                    .is_some_and(|passage| passage.contains(quote))
                            })
                    });
                if !covered {
                    findings.push(finding);
                }
            }
            findings
        };
        let narrative_findings_before =
            narrative_findings(&narrative_issues, &body, &evidence_history);
        // A sentence the check flagged is fixed for that reason first; a
        // repeat already flagged by a rule keeps the rule's finding.
        // QC-1.2: chapter 1's missing threat leads them all.
        let mut leading_findings = Vec::new();
        if chapter_number == Some(1) {
            leading_findings.extend(crate::chapter_craft::opening_threat_finding(
                &plan_payload,
                &body,
            ));
        }
        // Its edit adds an event and a deadline, which the habit check would
        // otherwise refuse as a new measure.
        let additions = leading_findings
            .iter()
            .filter_map(|finding| finding["quote"].as_str().map(ToOwned::to_owned))
            .collect::<Vec<_>>();
        leading_findings.extend(crate::chapter_craft::contradiction_findings(
            &contradictions,
            &body,
        ));
        leading_findings.extend(narrative_findings_before.iter().cloned());
        leading_findings.extend(crate::chapter_craft::catchphrase_findings(
            &catchphrases,
            &body,
        ));
        leading_findings.extend(crate::chapter_craft::copied_sentence_findings(
            &chapter_history.bodies,
            &body,
        ));
        leading_findings.extend(crate::chapter_craft::ending_repetition_findings(
            &chapter_history.bodies,
            &body,
        ));
        // QC-1 (owner decision 2026-10-30): an object or detail written past
        // its allowance is worded differently from there on.
        let (mut detail_findings, mut running_details) = match &craft_writing {
            Some(writing) => {
                let (_, names) = writing.chapter_people(&plan_payload);
                let names = names.iter().map(String::as_str).collect::<Vec<_>>();
                crate::chapter_craft::detail_repetition_findings(
                    &chapter_history.bodies,
                    &body,
                    &names,
                    &catchphrases,
                )
            }
            None => (Vec::new(), Vec::new()),
        };
        // RP-1 §5: the images the check found overused are rewritten past
        // their allowance and left out by the next writers.
        detail_findings.extend(crate::chapter_craft::motif_findings(&motifs, &body));
        // Together they keep to the detail findings' share of the rewrite.
        detail_findings.truncate(crate::chapter_craft::MAX_DETAIL_FINDINGS);
        running_details.extend(motifs.iter().cloned());
        // A sentence already flagged keeps its first finding.
        let detail_findings = detail_findings
            .into_iter()
            .filter(|finding| {
                !leading_findings
                    .iter()
                    .any(|other| other["quote"] == finding["quote"])
            })
            .collect::<Vec<_>>();
        let detail_count = detail_findings.len();
        leading_findings.extend(detail_findings);
        rules.retain(|rule| {
            !leading_findings
                .iter()
                .any(|finding| finding["quote"] == rule["quote"])
        });
        let short_lines = crate::chapter_craft::recent_short_lines(&chapter_history.bodies);
        for finding in crate::chapter_craft::repetition_findings(&repeated, &body)
            .into_iter()
            .chain(crate::chapter_craft::short_line_findings(
                &short_lines,
                &body,
            ))
        {
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
        let checked_body = body.clone();
        let mut applied = 0;
        let mut discarded = Value::Null;
        let repeated_chars = |text: &str| {
            crate::chapter_craft::repeated_character_count(&chapter_history.bodies, text)
        };
        let fits = |text: &str| length_range.accepts(character_count(text));
        let mut rewrite_error = None;
        let wants_rewrite = !to_fix.is_empty() || !repeated.is_empty();
        // Review 2026-11-27: a rewrite left out for want of model runs is
        // recorded, not mistaken for a chapter with nothing to fix.
        let has_rewrite_room = wants_rewrite && room_for_polish(2)?;
        let rewrite_skipped = (wants_rewrite && !has_rewrite_room).then_some("NO_MODEL_RUNS_LEFT");
        if has_rewrite_room {
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
            // The rewrite only improves the chapter: when it fails, the
            // chapter goes on as written and the failure is recorded.
            let rewrite = craft_step(
                "rewrite",
                "REWRITE_PASSAGES",
                &rewrite_profile,
                json!({"chapterId":chapter_id,"body":body,"findings":to_fix,"repeatedPhrases":repeated,"chapterLengthRange":length_range.to_json(),"craftGuidance":rewrite_guidance}),
                None,
            )?
            .unwrap_or_else(|error| {
                rewrite_error = Some(error);
                json!({"edits":[]})
            });
            let repeats_before = repeated_chars(&body);
            let repeats = |text: &str| {
                repeated_chars(text) > repeats_before || duplicate_in_chapter(text).is_some()
            };
            let edits = apply_edits_that_fit(&body, &rewrite, &additions, |text| {
                fits(text) && !repeats(text)
            });
            applied = edits.applied;
            if applied > 0 {
                body = edits.body;
            } else if let Some(whole) = &edits.refused_whole {
                discarded = json!(if fits(whole) {
                    "MORE_REPEATED_PHRASES"
                } else {
                    "LENGTH_OUT_OF_RANGE"
                });
            }
            // RP-1: repeats signing F1-5 counts that the rewrite left (its
            // edit refused, say) are rewritten once more, alone.
            let left = crate::chapter_craft::overused_phrases(&chapter_history.bodies, &body);
            if !left.is_empty() && room_for_polish(2)? {
                let retry = craft_step(
                    "rewrite-repeats",
                    "REWRITE_PASSAGES",
                    &rewrite_profile,
                    json!({"chapterId":chapter_id,"body":body,"findings":crate::chapter_craft::repetition_findings(&left, &body),"repeatedPhrases":left,"chapterLengthRange":length_range.to_json(),"craftGuidance":rewrite_guidance}),
                    None,
                )?
                .unwrap_or_else(|error| {
                    rewrite_error.get_or_insert(error);
                    json!({"edits":[]})
                });
                let repeats_before = repeated_chars(&body);
                let edits = apply_edits_that_fit(&body, &retry, &additions, |text| {
                    fits(text)
                        && duplicate_in_chapter(text).is_none()
                        && repeated_chars(text) < repeats_before
                });
                if edits.applied > 0 {
                    body = edits.body;
                    applied += edits.applied;
                    // The chapter now holds a rewrite in range with fewer
                    // repeats, whatever became of the first one.
                    discarded = Value::Null;
                }
            }
        }
        // A rewritten chapter is checked again; one the rewrite left as it
        // was still has what the first check found.
        let mut rewritten = body != checked_body;
        let errors_before = continuity_errors.len();
        let edited_history = if rewritten {
            recall_history(&body).unwrap_or_else(|error| {
                continuity_errors.push(error);
                evidence_history.clone()
            })
        } else {
            Vec::new()
        };
        let (mut left, mut narrative_left) = match continuity_request(&body, &[], &edited_history) {
            Some((input, sentences)) if rewritten => {
                let checked = craft_step(
                    "continuity-recheck",
                    "CHECK_CONTINUITY",
                    &continuity_profile,
                    input,
                    Some(&body),
                )?;
                let (left, _, _, narrative) =
                    judge(checked, &sentences, &[], &mut continuity_errors);
                if continuity_errors.len() != errors_before {
                    body = checked_body;
                    rewritten = false;
                    discarded = json!("CONTINUITY_RECHECK_FAILED");
                    (contradictions.clone(), narrative_findings_before.clone())
                } else {
                    let findings = narrative_findings(&narrative, &body, &edited_history);
                    evidence_history = edited_history;
                    (left, findings)
                }
            }
            _ if rewritten => (Vec::new(), Vec::new()),
            _ => (contradictions.clone(), narrative_findings_before.clone()),
        };
        // A recheck can miss an issue it reported before. Keep the earlier
        // finding if the exact flagged passage is still in the candidate.
        for finding in &narrative_findings_before {
            let unchanged = finding["quote"]
                .as_str()
                .is_some_and(|quote| body.contains(quote));
            if unchanged
                && !narrative_left
                    .iter()
                    .any(|other| other["quote"] == finding["quote"])
            {
                narrative_left.push(finding.clone());
            }
        }
        // A second scan may omit a saved suspicion. When its sources are
        // still current, submit it to confirmation alongside fresh findings.
        if rechecking_saved_prose
            && continuity_sources_current(
                connection,
                &project_id,
                craft_skills.pointer("/continuity/recalledSources"),
            )?
        {
            for entry in &continuity_held {
                let item = &entry["item"];
                match entry["reason"].as_str() {
                    Some("QC1_NARRATIVE_CONTINUITY")
                        if item["quote"]
                            .as_str()
                            .is_some_and(|quote| body.contains(quote))
                            && !narrative_left.iter().any(|finding| {
                                finding["quote"] == item["quote"]
                                    && finding["evidence"] == item["evidence"]
                            }) =>
                    {
                        narrative_left.push(item.clone());
                    }
                    Some("QC1_CONTRADICTION")
                        if item["sentence"].as_u64().is_some()
                            && !left.iter().any(|finding| {
                                finding["sentence"] == item["sentence"]
                                    && finding["established"] == item["established"]
                            }) =>
                    {
                        left.push(item.clone());
                    }
                    _ => {}
                }
            }
        }
        if rewritten && (!left.is_empty() || !narrative_left.is_empty()) && room_for_polish(2)? {
            let remaining = left.len() + narrative_left.len();
            let mut repair_findings = narrative_left.clone();
            repair_findings.extend(crate::chapter_craft::contradiction_findings(&left, &body));
            let profile = gateway.resolve_step_profile("REWRITE_PASSAGES")?;
            match craft_step(
                "rewrite-continuity-leftovers",
                "REWRITE_PASSAGES",
                &profile,
                json!({"chapterId":chapter_id,"body":body,"findings":repair_findings,
                       "chapterLengthRange":length_range.to_json(),"continuityRepair":true}),
                None,
            )? {
                Ok(rewrite) => {
                    let repeats_before = repeated_chars(&body);
                    let edits = apply_edits_that_fit(&body, &rewrite, &[], |text| {
                        fits(text)
                            && duplicate_in_chapter(text).is_none()
                            && repeated_chars(text) <= repeats_before
                    });
                    let (revised, count) = (edits.body, edits.applied);
                    if count > 0 {
                        let errors_before = continuity_errors.len();
                        let revised_history = recall_history(&revised).unwrap_or_else(|error| {
                            continuity_errors.push(error);
                            evidence_history.clone()
                        });
                        if let Some((input, sentences)) =
                            continuity_request(&revised, &[], &revised_history)
                        {
                            let (mut checked, _, _, reported) = judge(
                                craft_step(
                                    "continuity-repair-check",
                                    "CHECK_CONTINUITY",
                                    &continuity_profile,
                                    input,
                                    Some(&revised),
                                )?,
                                &sentences,
                                &[],
                                &mut continuity_errors,
                            );
                            let mut still_present =
                                narrative_findings(&reported, &revised, &revised_history);
                            for finding in &narrative_left {
                                let unchanged = finding["quote"]
                                    .as_str()
                                    .is_some_and(|quote| revised.contains(quote));
                                if unchanged
                                    && !still_present
                                        .iter()
                                        .any(|other| other["quote"] == finding["quote"])
                                {
                                    still_present.push(finding.clone());
                                }
                            }
                            // So is a contradiction whose sentence is unchanged.
                            let before = body_sentences(&body);
                            for item in &left {
                                let sentence = item["sentence"]
                                    .as_u64()
                                    .and_then(|index| before.get(index as usize))
                                    .map(|(_, _, sentence)| sentence);
                                let now = sentence.and_then(|sentence| {
                                    sentences.iter().position(|other| other == sentence)
                                });
                                if let Some(now) = now.filter(|_| {
                                    !checked
                                        .iter()
                                        .any(|other| other["established"] == item["established"])
                                }) {
                                    let mut item = item.clone();
                                    item["sentence"] = json!(now);
                                    checked.push(item);
                                }
                            }
                            if continuity_errors.len() == errors_before
                                && crate::chapter_craft::named_findings_left(
                                    (&narrative_left, &left),
                                    (&still_present, &checked),
                                ) < remaining
                            {
                                left = checked;
                                narrative_left = still_present;
                                body = revised;
                                evidence_history = revised_history;
                                applied += count;
                            }
                        }
                    }
                }
                Err(error) => {
                    rewrite_error.get_or_insert(error);
                }
            }
        }
        let mut dismissed = Vec::new();
        if !narrative_left.is_empty() || !left.is_empty() {
            if let Some((mut input, sentences)) = continuity_request(&body, &[], &evidence_history)
            {
                let narrative_count = narrative_left.len();
                let candidates = narrative_left.iter().map(|finding| json!({
                    "currentPassage":finding["quote"],
                    "earlierEvidence":finding.get("comparison").unwrap_or(&finding["evidence"]),
                })).chain(left.iter().map(|item| {
                    let quote = item["sentence"].as_u64()
                        .and_then(|index| sentences.get(index as usize)).map(String::as_str);
                    json!({
                        "currentPassage":quote,
                        "earlierEvidence":{"chapter":item["chapter"],"fact":item["established"]},
                    })
                })).enumerate().map(|(index, mut candidate)| {
                    candidate["candidateNumber"] = json!(index);
                    if let Some((first, last)) = candidate["currentPassage"].as_str()
                        .and_then(|passage| crate::chapter_craft::numbered_passage_range(&body, passage))
                    {
                        candidate["currentSentenceRange"] = json!({"firstSentence":first,"lastSentence":last});
                    }
                    candidate
                }).collect::<Vec<_>>();
                input["confirmNarrativeIssues"] = json!(candidates);
                if let Some(stage) = &stage_step {
                    input["stageStep"] = stage.input();
                }
                let profile = gateway.resolve_step_profile("CONFIRM_NARRATIVE_CONTINUITY")?;
                let ground = |review: &Value, candidates: &[Value]| {
                    let mut confirmation = crate::chapter_craft::ground_continuity_confirmation(
                        review,
                        candidates,
                        &evidence_history,
                        &body,
                        chapter_number,
                    );
                    crate::chapter_craft::release_restated_states(&mut confirmation.blockers);
                    confirmation
                };
                match craft_step(
                    "continuity-confirm",
                    "CONFIRM_NARRATIVE_CONTINUITY",
                    &profile,
                    input.clone(),
                    None,
                )? {
                    Ok(review) => {
                        let mut confirmation = ground(&review, &candidates);
                        if confirmation.blockers.is_empty()
                            && !confirmation.needs_evidence.is_empty()
                        {
                            if has_review_room(reserved.submission, 1)? {
                                let retry_candidates = candidates
                                    .iter()
                                    .filter(|candidate| {
                                        candidate["candidateNumber"].as_u64().is_some_and(|id| {
                                            confirmation.needs_evidence.contains(&id)
                                        })
                                    })
                                    .cloned()
                                    .collect::<Vec<_>>();
                                input["confirmNarrativeIssues"] = json!(retry_candidates);
                                input["evidenceFeedback"] = json!({
                                    "instruction":"只复查所列候选。当前和前文各引用一句原文，结果引文须是该句中连续出现的原字，不得省略或拼接。状态变化若可与前文同时成立，不得判为倒退。前文已写过的动作若产生了新结果，只核对是否又将旧结果当成首次发生。",
                                });
                                match craft_step(
                                    "continuity-confirm-evidence",
                                    "CONFIRM_NARRATIVE_CONTINUITY",
                                    &profile,
                                    input,
                                    None,
                                )? {
                                    Ok(rechecked) => {
                                        let corrected = ground(&rechecked, &retry_candidates);
                                        confirmation.blockers = corrected.blockers;
                                        confirmation.dismissals.extend(corrected.dismissals);
                                    }
                                    Err(error) if error.contains("BUDGET_EXCEEDED") => {
                                        return Err(error)
                                    }
                                    Err(error) => continuity_errors.push(error),
                                }
                            } else {
                                continuity_errors
                                    .push("QC1_CONFIRMATION_EVIDENCE_DEFERRED_FOR_BUDGET".into());
                            }
                        }
                        confirmed_continuity_blockers = confirmation.blockers;
                        if confirmed_continuity_blockers.is_empty()
                            && !confirmation.dismissals.is_empty()
                        {
                            let cleared = confirmation.dismissals.iter().filter_map(|verdict| {
                                let index = usize::try_from(verdict["number"].as_u64()?).ok()?;
                                dismissed.push(json!({"candidate":candidates.get(index)?,"verdict":verdict}));
                                Some(index)
                            }).collect::<BTreeSet<_>>();
                            narrative_left = narrative_left
                                .into_iter()
                                .enumerate()
                                .filter_map(|(index, item)| {
                                    (!cleared.contains(&index)).then_some(item)
                                })
                                .collect();
                            left = left
                                .into_iter()
                                .enumerate()
                                .filter_map(|(index, item)| {
                                    (!cleared.contains(&(narrative_count + index))).then_some(item)
                                })
                                .collect();
                        }
                    }
                    Err(error) if error.contains("BUDGET_EXCEEDED") => return Err(error),
                    Err(error) => continuity_errors.push(error),
                }
            }
        }
        let mut unresolved = catchphrases
            .iter()
            .filter(|phrase| body.matches(phrase.as_str()).count() > 1)
            .map(|phrase| json!({"field":"continuity","item":phrase,"reason":"QC1_CATCHPHRASE"}))
            .collect::<Vec<_>>();
        unresolved
            .extend(left.into_iter().map(
                |item| json!({"field":"continuity","item":item,"reason":"QC1_CONTRADICTION"}),
            ));
        unresolved.extend(narrative_left.into_iter().map(
            |item| json!({"field":"continuity","item":item,"reason":"QC1_NARRATIVE_CONTINUITY"}),
        ));
        // QC-1.2: chapter 1's threat belongs near its start.
        if chapter_number == Some(1)
            && !crate::chapter_craft::opening_threat_on_page(&plan_payload, &body)
        {
            unresolved.push(json!({
                "field":"opening",
                "item":plan_payload.pointer("/craft/scenes/0/conflict"),
                "reason":"QC1_OPENING_THREAT_LATE"
            }));
        }
        let final_digest = sha256_canonical(&json!(body)).map_err(|error| error.to_string())?;
        let final_check = checked_bodies
            .get(&final_digest)
            .map(|run_id| json!({"bodyDigest":final_digest,"modelRunId":run_id}));
        craft_skills["continuity"] = json!({
            "reviewVersion":CONTINUITY_REVIEW_VERSION,
            "finalCheck":final_check,
            "contradictions":contradictions,
            "narrativeFindings":narrative_findings_before,
            "catchphrases":catchphrases,
            "dismissed":dismissed,
            "unresolved":unresolved,
            "errors":continuity_errors,
            "recalledSources":evidence_history.iter().filter_map(|entry| entry.get("source")).collect::<Vec<_>>()
        });
        continuity_held = unresolved;
        if !rechecking_saved_prose {
            craft_skills["proseReview"] = json!({
                "findings":findings.len(),
                "codes":findings.iter().map(|finding| finding["code"].clone()).collect::<Vec<_>>(),
                "repeatedPhrases":repeated.len(),
                "ruleCodes":rules.iter().map(|rule| rule["code"].clone()).collect::<Vec<_>>(),
                "editsApplied":if discarded.is_null() { applied } else { 0 },
                "discarded":discarded,
                "rewriteError":rewrite_error,
                "rewriteSkipped":rewrite_skipped,
                "detailFindings":detail_count,
                "overusedDetails":running_details,
                "reviewError":review.get("reviewError")
            });
            if let Some(writing) = &craft_writing {
                craft_skills["proseReview"]["rhythmNotes"] = json!(writing
                    .style_card
                    .as_ref()
                    .map(|card| crate::chapter_craft::rhythm_notes(&body, card))
                    .unwrap_or_default());
            }
        }
    } else if review_due {
        let review = &mut craft_skills["continuity"];
        let mut errors = review["errors"].as_array().cloned().unwrap_or_default();
        let deferred = json!("QC1_CONFIRMATION_EVIDENCE_DEFERRED_FOR_BUDGET");
        if !errors.contains(&deferred) {
            errors.push(deferred);
        }
        review["errors"] = json!(errors);
    }
    let body_digest = sha256_canonical(&json!(body)).map_err(|e| e.to_string())?;
    validate_candidate_evidence_range(&body, 0, body.chars().count())?;
    update_stage(manager, connection, &project_id, &job_id, "CANDIDATE", &now)?;
    let candidate_payload = json!({
        "payloadSchema":"chapter-candidate.r1","chapterId":chapter_id,
        "chapterPlanRef":plan_ref.clone(),"contextManifestRef":context_ref.clone(),
        "body":body.clone(),"bodyDigest":body_digest,"evidenceRanges":[],
        "renderer":renderer,
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

    if !confirmed_continuity_blockers.is_empty() {
        if !continuity_sources_current(
            connection,
            &project_id,
            craft_skills.pointer("/continuity/recalledSources"),
        )? {
            block_for_recompile(connection, &project_id, &job_id, &now)?;
            return Err("PLAN_STALE:CONTINUITY_SOURCE_CHANGED".into());
        }
        candidate.payload["continuityReview"] = json!({
            "jobId":job_id,"findings":confirmed_continuity_blockers,"modelRunId":prose_run_ids.last(),
        });
        candidate.payload_hash =
            sha256_canonical(&candidate.payload).map_err(|error| error.to_string())?;
        ProductionRepository::hold_continuity_candidate(connection, &candidate, &job_id, &now)
            .map_err(|error| error.to_string())?;
        return Err(NARRATIVE_REVIEW_REQUIRED.into());
    }
    if execution_mode == "OPENING_BOOTSTRAP"
        && gateway.enforces_chapter_length()
        && duplicate_in_chapter(&body).is_some()
    {
        return Err("B08_RENDER_DUPLICATE_SPAN".into());
    }
    if copies_prior_chapter(&body) {
        return Err(PRIOR_CHAPTER_COPY_ERROR.into());
    }
    if let Some(phrase) = signing_repetition(&body) {
        return Err(format!("{RENDER_SIGNING_REPETITION_ERROR}:{phrase}"));
    }
    let candidate_ref = reference(&candidate);
    update_stage(manager, connection, &project_id, &job_id, "AUDIT", &now)?;
    save_prose("AUDIT", &body, &prose_run_ids, &craft_skills)?;
    let mut audit_input = json!({"chapterId":chapter_id,"chapterPlanRef":plan_ref,"chapterPlan":audit_plan_view(&plan_payload),"contextManifestRef":context_ref,"authorityContext":authority_context.clone(),"auditPolicyContract":audit_contract.clone(),"candidateRef":candidate_ref.clone(),"body":body,"qualityReportRef":Value::Null});
    if let Some(stage) = &stage_step {
        audit_input["stageStep"] = stage.input();
    }
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
    // P1: threads earlier chapters raised, which this chapter may advance or
    // resolve by their ref.
    let open_threads = crate::repositories::narrative::NarrativeRepository::open_candidate_threads(
        connection,
        &project_id,
        OPEN_THREADS_SHOWN,
    )
    .map_err(|error| error.to_string())?;
    audit_input["openThreads"] = json!(open_threads);
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
        let (run_id, result) = execute_production_model_with_profile_recorded(
            connection,
            gateway,
            &audit_profile,
            &audit_spec,
            "AUDIT_EXTRACT",
            audit_input.clone(),
            &now,
        )?;
        (
            run_id,
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
        .chain(
            open_threads
                .iter()
                .filter_map(|thread| thread["threadRef"].as_str()),
        )
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    // QC-1: the chapter's own people, whose facts the fact cap keeps first.
    let chapter_people = craft_writing
        .as_ref()
        .map(|writing| writing.chapter_people(&plan_payload).0)
        .unwrap_or_default();
    let prepare_audit = |mut raw: Value| -> Result<Value, String> {
        let assigned = assign_audit_ids(&mut raw, &chapter_id, &known_threads);
        let mut audit_payload = normalize_audit_model_output(raw, &body);
        hold_back(
            &mut audit_payload,
            assigned["unboundThreadUpdates"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned(),
        );
        audit_payload["normalization"]["assignedIds"] = assigned;
        cap_new_threads(&mut audit_payload, &known_threads);
        cap_new_facts(&mut audit_payload, &chapter_people);
        if gateway.selects_main_event() {
            discard_unsupported_audit_event_labels(&mut audit_payload, &body);
        }
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
        // Real run round 19: an opening chapter's audit reported no change,
        // and signing found no committed facts to read for that chapter.
        if execution_mode == "OPENING_BOOTSTRAP" && extraction.canon_candidates.is_empty() {
            return Err("B08_AUDIT_OPENING_CANON_REQUIRED".into());
        }
        Ok((audit_payload, extraction))
    };
    // Craft rule (2026-10-18): an audit whose evidence cannot be located in
    // the body (for example quoting the plan) is run once more with the
    // unlocatable quotes; a second failure still blocks the chapter. Owner
    // decision (2026-10-19): so is an audit using an unregistered predicate,
    // with the registered ones; the policy itself stays fail-closed.
    let mut last_raw = raw_audit.clone();
    let checked = check_audit(last_raw.clone());
    let audit_feedback_requested = !audit_reused
        && checked.as_ref().is_err_and(|error| {
            matches!(
                error.as_str(),
                "B08_AUDIT_INCOMPLETE_EVIDENCE_REBOUND"
                    | "B08_AUDIT_INCOMPLETE_NORMALIZATION_DROPPED_CONTENT"
                    | "B08_AUDIT_INCOMPLETE_KNOWLEDGE_TIME"
                    | "B08_AUDIT_OPENING_CANON_REQUIRED"
                    | "B08_RELATION_IDENTITY_CONFLICT"
            ) || error.starts_with("B08_CANON_SEMANTICS_UNRESOLVED_PREDICATE_SEMANTICS")
        });
    let audit_feedback_deferred =
        audit_feedback_requested && !has_review_room(reserved.outcome, 1)?;
    let checked = match checked {
        Err(error) if audit_feedback_requested && !audit_feedback_deferred => {
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
                "instruction":"上次提取不完整：unboundCitations 中的条目指认的句子（sentence）或引文和条目内容对不上，unlocatableQuotes 中的引文在正文中找不到，incompleteCandidates 中的条目缺少必填字段或 evidence。请只修正这些条目：补全缺的字段，evidence.sentence 改为 bodySentences 中真正写到这件事的那一句的序号；找不到这样一句的条目直接删掉。其余条目照上次返回，不要新增条目，canonCandidates 和 knowledgeCandidates 各不超过 12 条。"
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
            if error == "B08_AUDIT_OPENING_CANON_REQUIRED" {
                input["auditFeedback"]["instruction"] = json!("这是开篇三章之一，签约评估要读取本章已记下的事实，上次却一条也没有。请重新完整提取：把正文里写到的事件、人物的处境、所在、持有物和关系的变化写成 canonCandidates，每条的 evidence.sentence 指向 bodySentences 里写到它的那一句，不要返回 COMPLETE_NO_CHANGE。");
            }
            if error == "B08_RELATION_IDENTITY_CONFLICT" {
                input["auditFeedback"]["instruction"] = json!("上次提取复用了另一对人物或相反方向的关系编号。请保留有正文证据的事实，关系编号只能用于同一方向的同一对人物；不确定时省略 relationId，由系统分配。不要把该关系标为已有关系任务已完成。");
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
            let (run_id, result) = execute_production_model_with_profile_recorded(
                connection,
                gateway,
                &audit_profile,
                &rerun_spec,
                "AUDIT_EXTRACT",
                input,
                &now,
            )?;
            let rerun_raw = result
                .parsed
                .ok_or_else(|| "B08_AUDIT_OUTPUT_INVALID".to_string())?;
            // Real run round 24: a follow-up that comes back with no facts
            // at all would wipe what the first audit found; what is held
            // back is then taken from the first audit.
            let has_canon = |value: &Value| {
                value["canonCandidates"]
                    .as_array()
                    .is_some_and(|items| !items.is_empty())
            };
            let lost_every_fact = !has_canon(&rerun_raw) && has_canon(&raw_audit);
            last_raw = if lost_every_fact {
                raw_audit.clone()
            } else {
                audit_run_id = run_id;
                rerun_raw.clone()
            };
            // Real run round 28: in a long-form chapter, too, an empty
            // follow-up is not taken; the first audit's facts are held back
            // item by item instead.
            let rechecked = if lost_every_fact {
                Err(error.clone())
            } else {
                check_audit(rerun_raw.clone())
            };
            match rechecked {
                Err(error) if error == "B08_AUDIT_INCOMPLETE_EVIDENCE_REBOUND" => {
                    // Ids first, so what is kept still names the facts it named.
                    let mut assigned = rerun_raw.clone();
                    let assigned_ids = assign_audit_ids(&mut assigned, &chapter_id, &known_threads);
                    let (kept, dropped) = drop_unlocatable_audit_candidates(&assigned, &body);
                    // Real run 2026-10-24: every fact was dropped and the
                    // chapter committed with no canon. A chapter is not
                    // committed without the facts its audit reported.
                    if has_canon(&rerun_raw) && !has_canon(&kept) {
                        Err(error)
                    } else {
                        check_audit(kept).map(|(mut payload, extraction)| {
                            payload["normalization"]["droppedUnlocatable"] = dropped;
                            hold_back(
                                &mut payload,
                                assigned_ids["unboundThreadUpdates"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .cloned(),
                            );
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
    let (mut audit_payload, mut extraction) = match checked {
        Ok(checked) => checked,
        Err(error) if error.starts_with("B08_") => {
            hold_back_audit_items(prepare_audit(last_raw)?, &validate_audit)?
        }
        Err(other) => return Err(other),
    };
    if audit_feedback_deferred {
        audit_payload["normalization"]["feedbackDeferredForBudget"] = json!(true);
    }
    let mut outcome_run_ids = Vec::new();
    let mut main_event = None;
    let needs_main_event = gateway.selects_main_event();
    let mut verified_step = None;
    if let Some(stage) = &stage_step {
        let task_id = &stage.task_id;
        let historical_sources = stage_step_history(connection, &project_id, &volume_id, stage)?;
        let profile = gateway.resolve_step_profile("VERIFY_STAGE_STEP")?;
        let mut input = json!({
            "chapterId":chapter_id,"stageStep":stage.input(),
            "bodySentences":crate::chapter_craft::numbered_body_sentences(&body),
        });
        chapter_history.extend_continuation_input(&mut input);
        if !historical_sources.is_empty() {
            input["priorStageSources"] = json!(historical_sources
                .iter()
                .map(|source| json!({
                    "chapterNo":source.chapter_no,
                    "sentences":crate::chapter_craft::numbered_body_sentences(&source.body),
                }))
                .collect::<Vec<_>>());
            if let Some(recent) = input["recentCommittedProse"].as_array_mut() {
                recent.retain(|entry| {
                    !historical_sources
                        .iter()
                        .any(|source| entry["chapterNo"].as_i64() == Some(source.chapter_no))
                });
            }
        }
        let previous_prose = chapter_history
            .recent
            .iter()
            .zip(&chapter_history.bodies)
            .filter_map(|(chapter, body)| Some((chapter["chapterNo"].as_i64()?, body.as_str())))
            .collect::<Vec<_>>();
        let spec = ModelRunSpec {
            model_run_id: StableIdentityFactory::from_command(
                StableIdKind::ModelRun,
                &format!("{command_id}:verify-stage-step"),
            )?,
            job_id: Some(job_id.clone()),
            step_id: None,
            provider: profile.provider.clone(),
            model: profile.model.clone(),
            idempotency_key: format!("{idempotency_key}:verify-stage-step"),
            capability: "VERIFY_STAGE_STEP".into(),
            prompt_ref: None,
            context_manifest_ref: Some(context_ref.artifact_id.clone()),
            budget_id: Some(production_budget_id.clone()),
            predicted_cost_cents: 0,
            cost_snapshot: None,
            attempt_no: 1,
            repair_round: 0,
        };
        // Set when the first citations prove the step but for their wording.
        let mut second_look = false;
        for (index, spec) in [
            spec.clone(),
            follow_up_spec(&spec, ":evidence-feedback", 1)?,
        ]
        .into_iter()
        .enumerate()
        {
            if index > 0 && !has_review_room(if needs_main_event { reserved.event } else { 0 }, 1)?
            {
                hold_back(
                    &mut audit_payload,
                    [json!({
                        "field":"taskProgressCandidates","taskId":task_id,"objective":stage.objective,
                        "reason":"B08_STAGE_STEP_OUTCOME_UNVERIFIED","feedbackDeferredForBudget":true
                    })],
                );
                break;
            }
            let result = execute_production_model_with_profile_once(
                connection,
                gateway,
                &profile,
                &spec,
                "VERIFY_STAGE_STEP",
                input.clone(),
                &now,
            );
            let run_id = spec.model_run_id;
            if DurableModelRunner::is_recorded(connection, &project_id, &run_id)
                .map_err(|error| error.to_string())?
            {
                outcome_run_ids.push(run_id.clone());
            }
            let malformed = match result {
                Ok(outcome) => {
                    let Some(payload) = outcome.parsed.as_ref() else {
                        break;
                    };
                    let mut selection = payload["stageStepCompletion"].clone();
                    if selection.is_null() {
                        selection = payload["stageStepProgress"].clone();
                    }
                    if let Some(number) = chapter_number {
                        drop_own_source_chapter(&mut selection, number);
                    }
                    if selection.is_null() {
                        hold_back(
                            &mut audit_payload,
                            [json!({
                                "field":"taskProgressCandidates","taskId":task_id,"objective":stage.objective,
                                "reason":"B08_STAGE_STEP_OUTCOME_UNVERIFIED",
                            })],
                        );
                        break;
                    }
                    let verify = |second_look| {
                        evaluate_stage_step(
                            stage,
                            &body,
                            &previous_prose,
                            &historical_sources,
                            &selection,
                            &chapter_id,
                            &run_id,
                            second_look,
                        )
                    };
                    if let Some(result) = verify(second_look) {
                        if result.status() == "SATISFIED" {
                            let source = result.historical_origin(connection, &project_id)?;
                            let origin =
                                source.map_or(chapter_id.as_str(), |source| &source.chapter_id);
                            match validate_task_completion_claim(
                                connection,
                                &project_id,
                                origin,
                                &result.claim(),
                                &extraction,
                            ) {
                                Ok(()) => {}
                                Err(error)
                                    if error == "B08_TASK_COMPLETION_PREDICATE_UNSATISFIED" =>
                                {
                                    hold_back(
                                        &mut audit_payload,
                                        [json!({
                                            "field":"taskProgressCandidates","taskId":task_id,"objective":stage.objective,"reason":error
                                        })],
                                    );
                                    break;
                                }
                                Err(error) => return Err(error),
                            }
                        }
                        verified_step = Some(result);
                        break;
                    }
                    second_look = !second_look && verify(true).is_some();
                    let instruction = if second_look {
                        "引文没有直接写明这一步的结果。再读一遍所引的句子和前后文：这一步要求的动作是不是真的做完了、结论是不是真的得出了？是，就引出写明动作和结论的原文；只是拿到、看见、提起或打算做，只返回 stageStepCompletion 为 null。"
                    } else {
                        "核对 condition 编号及引文的来源章节。复核旧成果时须填写 priorStageSources 里的 sourceChapter，动作和结果都引同一章。只证明一部分时用 stageStepProgress。找不到可核实的结果时只返回 stageStepCompletion 为 null。"
                    };
                    input["evidenceFeedback"] = json!({
                        "instruction":instruction,
                        "previousCitations":stage_step_citations(&selection).iter().map(|part| json!({
                            "firstSentence":part["firstSentence"],"outcomeSentence":part["outcomeSentence"],
                            "outcomeQuoteCharacters":part["outcomeQuote"].as_str().map(|text| text.chars().count()),
                        })).collect::<Vec<_>>()
                    });
                    false
                }
                Err(error)
                    if error.contains(crate::model_output::OFF_SPEC)
                        || error.contains("P01_MODEL_JSON_INVALID:") =>
                {
                    input["outputFeedback"] = json!({
                        "instruction":"上一份回复格式不合格。只返回 JSON。用 stageStepCompletion 或 stageStepProgress 提交一至三条简短的逐字原文；无可核实的新结果时只返回 stageStepCompletion 为 null。"
                    });
                    true
                }
                Err(error) => return Err(error),
            };
            if index == 1 {
                hold_back(
                    &mut audit_payload,
                    [json!({
                        "field":"taskProgressCandidates","taskId":task_id,"objective":stage.objective,
                        "reason":"B08_STAGE_STEP_OUTCOME_UNVERIFIED","malformedReply":malformed,
                    })],
                );
            }
        }
    }
    let mut main_event_fact = None;
    if needs_main_event {
        let run_id = StableIdentityFactory::from_command(
            StableIdKind::ModelRun,
            &format!("{command_id}:main-event"),
        )?;
        let profile = audit_profile.clone();
        let spec = ModelRunSpec {
            model_run_id: run_id.clone(),
            job_id: Some(job_id.clone()),
            step_id: None,
            provider: profile.provider.clone(),
            model: profile.model.clone(),
            idempotency_key: format!("{idempotency_key}:main-event"),
            capability: "SELECT_MAIN_EVENT".into(),
            prompt_ref: None,
            context_manifest_ref: Some(context_ref.artifact_id.clone()),
            budget_id: Some(production_budget_id.clone()),
            predicted_cost_cents: 0,
            cost_snapshot: None,
            attempt_no: 1,
            repair_round: 0,
        };
        let sentences = crate::chapter_craft::numbered_body_sentences(&body);
        let chapter_goal = plan_payload["chapterGoal"].as_str().unwrap_or_default();
        let mut input = json!({"chapterId":chapter_id,"bodySentences":sentences,
            "chapterGoal":chapter_goal,
            "plannedChange":plan_payload.pointer("/craft/situationChange/after")});
        chapter_history.extend_continuation_input(&mut input);
        let mut goal_reviews = Vec::new();
        for selection_spec in [
            spec.clone(),
            follow_up_spec(&spec, ":evidence-feedback", 1)?,
        ] {
            let selected_run = execute_production_model_with_profile_once(
                connection,
                gateway,
                &profile,
                &selection_spec,
                "SELECT_MAIN_EVENT",
                input.clone(),
                &now,
            );
            let run_id = selection_spec.model_run_id;
            if DurableModelRunner::is_recorded(connection, &project_id, &run_id)
                .map_err(|error| error.to_string())?
            {
                outcome_run_ids.push(run_id.clone());
            }
            let mut returned_selection = None;
            let selected = match selected_run {
                Ok(outcome) => {
                    returned_selection = outcome.parsed;
                    returned_selection.as_ref().map(|value| {
                        if let Some(mut review) =
                            bind_chapter_goal_review(chapter_goal, &body, &value["goalReview"])
                        {
                            review["modelRunId"] = json!(run_id);
                            goal_reviews.push(review);
                        }
                        if value["eventType"] == "GAIN" && input.get("allowedEventTypes").is_some()
                        {
                            return Err("EVENT_GAIN_WITHOUT_ACQUISITION".into());
                        }
                        if let Some(excluded) =
                            input.pointer("/selectionFeedback/excludedOutcomeSentence")
                        {
                            if value["firstSentence"].is_number()
                                && &value["outcomeSentence"] == excluded
                            {
                                return Err("EVENT_REUSED_REJECTED_OUTCOME".into());
                            }
                        }
                        select_main_event_evidence(&body, value)
                    })
                }
                Err(error)
                    if error.contains("UNKNOWN_REMOTE_OUTCOME")
                        || error.contains("BUDGET_EXCEEDED") =>
                {
                    return Err(error)
                }
                Err(_) => None,
            };
            match selected {
                Some(Ok(Some(event))) => {
                    main_event_fact = extraction
                        .canon_candidates
                        .iter()
                        .find(|item| {
                            item.event_type.as_deref() == Some(event.event_type.as_str())
                                && item.evidence.start == event.evidence.start
                                && item.evidence.end == event.evidence.end
                                && item.evidence.quote == event.evidence.quote
                        })
                        .map(|item| item.fact_id.clone());
                    let event_id = main_event_fact.as_ref().map_or_else(
                        || format!("event:{final_id}:main"),
                        |fact_id| format!("event:{fact_id}"),
                    );
                    audit_payload["normalization"]["mainEventSelection"] = json!({
                        "modelRunId":run_id,"eventId":event_id,
                        "eventType":event.event_type,"outcomeQuote":event.outcome_quote,
                        "evidence":event.evidence.to_json()
                    });
                    if let Some(fact_id) = &main_event_fact {
                        audit_payload["normalization"]["mainEventSelection"]["factId"] =
                            json!(fact_id);
                    }
                    main_event = Some((event_id, event, run_id));
                    break;
                }
                Some(Ok(None)) if selection_spec.attempt_no == 1 => break,
                Some(Err(reason)) if selection_spec.attempt_no == 1 => {
                    let instruction = match reason.as_str() {
                        "EVENT_QUOTE_NOT_FOUND" => "上次的引文与原文对不上。查看原句编号，复制同一句或相邻两句的原文，不要翻译或添字。",
                        "EVENT_ACTION_ORDER_INVALID" => "上次的句子编号顺序或间距不合法。请选择一至三句连续原文，从第一句到结果句。",
                        "EVENT_GAIN_WITHOUT_ACQUISITION" => "上次标为 GAIN，但原文没有显示获得或转交。此次只在 allowedEventTypes 中选。揭示新信息用 DISCOVERY 或 REVEAL，物体或人的状态确实改变用 SITUATION_CHANGE。",
                        "EVENT_RESULT_IS_REACTION" => "上次引用的只有人物反应。选正文里实际发生的决定、获得的情报或改变的状态。",
                        "EVENT_RESULT_IS_FORECAST" => "上次引用的是预期将发生的事。选择正文已经出现的结果，并引用写到该结果的原句。",
                        _ => "选择有明确结果的事件，核对原文和句子编号后作答。",
                    };
                    let mut feedback = json!({
                        "reason":reason,"instruction":instruction,"previousSelection":returned_selection
                    });
                    if let Some(number) =
                        rejected_numbered_event_outcome(&feedback["previousSelection"], &reason)
                    {
                        feedback["excludedOutcomeSentence"] = json!(number);
                    }
                    input["selectionFeedback"] = feedback;
                    if reason == "EVENT_GAIN_WITHOUT_ACQUISITION" {
                        input["allowedEventTypes"] = json!(MAIN_EVENT_TYPES
                            .iter()
                            .copied()
                            .filter(|event_type| *event_type != "GAIN")
                            .collect::<Vec<_>>());
                    }
                }
                _ => {
                    hold_back(
                        &mut audit_payload,
                        [json!({"field":"events","reason":"QC1_MAIN_EVENT_SELECTION_FAILED"})],
                    );
                    break;
                }
            }
        }
        let mut assessment = match goal_reviews.first() {
            Some(first)
                if goal_reviews.iter().all(|review| {
                    review["status"] == first["status"] && review["missing"] == first["missing"]
                }) =>
            {
                first.clone()
            }
            Some(_) => json!({"status":"UNVERIFIED","reason":"B08_CHAPTER_GOAL_REVIEW_DISAGREES"}),
            None => {
                json!({"status":"UNVERIFIED","reason":"B08_CHAPTER_GOAL_REVIEW_MISSING_OR_UNBOUND"})
            }
        };
        assessment["ruleVersion"] = json!("chapter-goal.r1");
        assessment["chapterGoal"] = json!(chapter_goal);
        assessment["chapterPlanRef"] = json!(plan_ref);
        assessment["bodyDigest"] = candidate.payload["bodyDigest"].clone();
        if assessment["status"] != "SUPPORTED" {
            hold_back(
                &mut audit_payload,
                [json!({
                    "field":"chapterGoalReview","status":assessment["status"],
                    "missing":assessment["missing"],"reason":"B08_CHAPTER_GOAL_NOT_VERIFIED"
                })],
            );
        }
        audit_payload["normalization"]["chapterGoalReview"] = assessment;
    }
    if gateway.selects_main_event() {
        apply_stage_step_verification(&mut extraction, stage_step.as_ref(), verified_step.as_ref());
        audit_payload["taskProgressCandidates"] = json!(extraction.task_progress_candidates);
        relabel_audit_completeness(&mut audit_payload);
        if let Some(stage) = &stage_step {
            let status = verified_step
                .as_ref()
                .map_or("UNVERIFIED", VerifiedStageStep::status);
            audit_payload["normalization"]["stageStepVerification"] = json!({
                "taskId":stage.task_id,"objective":stage.objective,"status":status,
                "outcomeRules":stage_step_rules(&stage.objective),
                "rulesDigest":plan_payload["stageStepRulesDigest"],
                "modelRunId":verified_step.as_ref().map(|result| &result.model_run_id),
                "currentChapterEvidence":verified_step.as_ref().is_some_and(VerifiedStageStep::has_current_evidence),
                "evidenceParts":verified_step.as_ref().map(|result| &result.progress.evidence_parts),
                "remainingObjectives":verified_step.as_ref().map(|result| result.progress.remaining_objectives.clone())
                    .unwrap_or_else(|| stage.progress.as_ref()
                        .map(|progress| progress.remaining_objectives.clone())
                        .unwrap_or_else(|| stage_step_objectives(&stage.objective))),
            });
        }
    }
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
                "evidence": candidate.evidence.as_ref().map(EvidenceSpan::to_json)
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
    let mut held = audit_payload
        .pointer("/normalization/heldBack")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .cloned()
        .chain(continuity_held)
        .collect::<Vec<_>>();
    let mut change_request_artifacts = Vec::new();
    let mut change_request_heads = Vec::new();
    let mut obligation_evidence_records = Vec::new();
    let mut new_responsibility_evidence = Vec::new();
    let mut effective_continuity_refs = BTreeSet::new();
    // P1: the book's mysteries this chapter resolves, for their events.
    let mut resolved_mysteries = Vec::new();
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
            if obligation.obligation_type == "MYSTERY"
                && matches!(item.disposition.as_str(), "RESOLVE" | "RESOLVED")
            {
                resolved_mysteries.push((authority_id.to_owned(), item.evidence.clone()));
            }
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
                    "payloadJson":{"bridge":bridge,"sourceChapterRef":reference(&candidate),"sourceAuditRef":reference(&audit),"evidence":item.evidence.as_ref().map(EvidenceSpan::to_json)}
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
                    "evidence":item.evidence.as_ref().map(EvidenceSpan::to_json)}
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
        let description = thread_description(item);
        // P1: a hint, key object or mystery planted for a later payoff is a
        // foreshadow, which later chapters reinforce and recover.
        if is_foreshadow(item.promotion_signals, item.fulfillment_object.as_deref()) {
            foreshadow_delta.push(json!({
                "foreshadowId":authority_id,"intent":description,"status":"PLANTED",
                "plantChapterId":chapter_id,
                "secretPayloadRef":item.fulfillment_object.as_deref().and_then(payoff_ref),
                "payloadJson":{"sourceAuthority":"B08_ACTUAL_NARRATIVE","sourceArtifactId":final_id,"sourceVersion":1,"deltaMode":"NEW_CONTINUITY",
                    "fulfillmentObject":item.fulfillment_object,
                    "evidence":item.evidence.as_ref().map(EvidenceSpan::to_json)}
            }));
            continue;
        }
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
            "payloadJson":{"ruleVersion":crate::narrative::THREAD_PROMOTION_RULE_VERSION,"promotionSignals":item.promotion_signals,"evidence":item.evidence.as_ref().map(EvidenceSpan::to_json),"fulfillmentObject":item.fulfillment_object,"resumeCondition":resume_condition}
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
    // A mystery event may use only a fact citing the same passage as the
    // mystery resolution; never relabel an unrelated fact as an event.
    let (event_marks, missing_mysteries) =
        resolved_mystery_event_marks(&extraction.canon_candidates, &resolved_mysteries);
    for mystery in missing_mysteries {
        held.push(json!({"field":"events","item":{"mysteryId":mystery},"reason":"QC1_MYSTERY_EVENT_MISSING"}));
    }
    if (needs_main_event
        || (!extraction.canon_candidates.is_empty()
            && !extraction
                .canon_candidates
                .iter()
                .any(|item| item.event_type.is_some())
            && event_marks.is_empty()))
        && main_event.is_none()
    {
        held.push(json!({"field":"events","item":{"chapterId":chapter_id},"reason":"QC1_MAIN_EVENT_MISSING"}));
    }
    if !held.is_empty() {
        decision_updates.push(audit_reminder(
            &final_id,
            &chapter_id,
            chapter_number,
            &held,
        )?);
    }
    // Only a relation whose object is a known entity can be committed and
    // bound to chapter evidence. The audit identity check has already run.
    let audit_entities = extraction
        .canon_candidates
        .iter()
        .filter_map(|item| item.entity_id.as_deref())
        .collect::<BTreeSet<_>>();
    let mut relation_ids = BTreeMap::new();
    for item in &extraction.canon_candidates {
        let Some(relation_id) = candidate_relation_id(item) else {
            continue;
        };
        let object = item.object_json["objectId"].as_str().unwrap_or_default();
        if audit_entities.contains(object)
            || crate::repositories::narrative::NarrativeRepository::canon_entity_exists(
                connection,
                &project_id,
                object,
            )
            .map_err(|error| error.to_string())?
        {
            relation_ids.insert(item.fact_id.clone(), relation_id);
        }
    }
    let mut evidence_records = extraction
        .canon_candidates
        .iter()
        .enumerate()
        .map(|(index, item)| {
            json!({
                "evidenceId":format!("evidence:{}:{}", final_id, index + 1),
                "factId":item.fact_id,
                "relationId":relation_ids.get(&item.fact_id),
                "eventId":(item.event_type.is_some() || event_marks.contains_key(&item.fact_id)).then(|| format!("event:{}", item.fact_id)),
                "quote":item.evidence.quote,
                "rangeStart":item.evidence.start,
                "rangeEnd":item.evidence.end,
                "evidenceType":"ACTUAL_NARRATIVE"
            })
        })
        .collect::<Vec<_>>();
    if let Some((event_id, event, model_run_id)) = &main_event {
        if let Some(fact_id) = &main_event_fact {
            let record = evidence_records
                .iter_mut()
                .find(|record| record["factId"] == *fact_id)
                .ok_or("B08_MAIN_EVENT_FACT_EVIDENCE_MISSING")?;
            record["eventType"] = json!(event.event_type);
            record["modelRunId"] = json!(model_run_id);
            record["outcomeQuote"] = json!(event.outcome_quote);
        } else {
            evidence_records.push(json!({
                "evidenceId":format!("evidence:{final_id}:main-event"),
                "eventId":event_id,"eventType":event.event_type,"modelRunId":model_run_id,
                "outcomeQuote":event.outcome_quote,"quote":event.evidence.quote,
                "rangeStart":event.evidence.start,"rangeEnd":event.evidence.end,
                "evidenceType":"ACTUAL_NARRATIVE"
            }));
        }
    }
    let final_payload = json!({"payloadSchema":"final-chapter.r1","chapterId":chapter_id,"body":candidate.payload["body"],"bodyDigest":candidate.payload["bodyDigest"],"candidateRef":candidate_ref.clone(),"chapterPlanRef":plan_ref.clone(),"contextManifestRef":context_ref.clone(),"auditRef":reference(&audit),"qualityReportRef":Value::Null,"tasteProfileRef":Value::Null,"wordCount":candidate.payload["body"].as_str().map_or(0, crate::chapter_length::character_count),"stateDigest":state_digest,"validatedEvidence":evidence_records,"validatedObligationEvidence":obligation_evidence_records,"newResponsibilityEvidence":new_responsibility_evidence});
    let mut proof_sources = BTreeMap::new();
    if let Some(result) = &verified_step {
        for source in result
            .progress
            .evidence_parts
            .iter()
            .filter_map(|part| part.source.as_ref())
        {
            proof_sources.insert(
                source.final_ref.artifact_id.clone(),
                source.final_ref.clone(),
            );
        }
    }
    let mut final_dependencies = vec![candidate_ref.clone(), reference(&audit)];
    final_dependencies.extend(proof_sources.values().cloned());
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
        final_dependencies,
    )?;
    final_artifact.provenance.context_manifest_ref = Some(context_ref.artifact_id.clone());
    let stored_progress = verified_step
        .as_ref()
        .map(|result| result.recorded_progress(&chapter_id, &reference(&final_artifact)));
    let mut task_states = extraction.task_progress_candidates.iter()
        .filter(|item| verified_step.as_ref().is_none_or(|result| result.task_id != item.task_id))
        .map(|item| {
            json!({"taskId":item.task_id,"status":item.status,
                "evidenceRefs":[task_evidence_ref(&item.evidence, &final_artifact.artifact_id, final_artifact.version)],
                "remainingConditions":[]})
        }).collect::<Vec<_>>();
    let stage_origin = if let (Some(result), Some(progress)) = (&verified_step, &stored_progress) {
        let (references, remaining) = stage_step_proof_fields(progress);
        task_states.push(json!({"taskId":result.task_id,"status":result.status(),
            "evidenceRefs":references,"remainingConditions":remaining}));
        result.historical_origin(connection, &project_id)?
    } else {
        None
    };
    let mut canon_event_refs = extraction
        .canon_candidates
        .iter()
        .filter(|item| item.event_type.is_some() || event_marks.contains_key(&item.fact_id))
        .map(|item| format!("event:{}", item.fact_id))
        .collect::<Vec<_>>();
    if let Some((event_id, _, _)) = &main_event {
        if !canon_event_refs.contains(event_id) {
            canon_event_refs.push(event_id.clone());
        }
    }
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
        let audit_status_by_id = task_states
            .iter()
            .filter_map(|task| Some((task["taskId"].as_str()?, task["status"].as_str()?)))
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
            json!({"taskId":item.task_id,"plannedExpected":expected,"actualObserved":item.status,"evidence":item.evidence.to_json()})
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
            "evidence":item.evidence.to_json()
        })
    }));
    reality.extend(extraction.knowledge_candidates.iter().map(|item| {
        json!({
            "kind":"KNOWLEDGE",
            "knowledgeId":item.knowledge_id,
            "factId":item.fact_id,
            "knowerId":item.knower_id,
            "actualObserved":item.epistemic_state,
            "evidence":item.evidence.to_json()
        })
    }));
    reality.extend(
        extraction
            .continuity_candidates
            .iter()
            .filter(|item| effective_continuity_refs.contains(&item.thread_ref))
            .map(|item| {
                json!({
                    "kind":"CONTINUITY",
                    "threadRef":item.thread_ref,
                    "actualObserved":item.disposition,
                    "evidence":item.evidence.as_ref().map(EvidenceSpan::to_json)
                })
            }),
    );
    let mut model_run_refs = vec![render_run_id.clone(), audit_run_id.clone()];
    model_run_refs.extend(outcome_run_ids);
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
    // Relations last, after the facts that record their entities.
    let mut canon_candidates = extraction.canon_candidates.iter().collect::<Vec<_>>();
    canon_candidates.sort_by_key(|item| relation_ids.contains_key(&item.fact_id));
    let canon_delta = canon_candidates.into_iter().map(|item| json!({
        "entityId":item.entity_id,
        "factId":item.fact_id,
        "subjectId":item.subject_id,
        "predicate":item.predicate,
        "objectJson":item.object_json,
        "authorityLevel":item.authority_level,
        "eventType":if main_event_fact.as_ref() == Some(&item.fact_id) {None} else {
            item.event_type.as_deref().or_else(|| event_marks.get(&item.fact_id).map(|mark| mark.0))
        },
        "relationId":relation_ids.get(&item.fact_id),
        "validFromChapterId":chapter_id,
        "payloadJson":{
            "chapterId":chapter_id,
            "eventId":(item.event_type.is_some() || event_marks.contains_key(&item.fact_id)).then(|| format!("event:{}", item.fact_id)),
            "event":event_marks.get(&item.fact_id).map(|mark| mark.1.clone()),
            "storyTime":item.story_time,
            "canonicalName":item.canonical_name,
            "entityType":item.entity_type.as_deref().map(entity_kind),
            "sourceArtifactId":final_ref.artifact_id,
            "sourceVersion":final_ref.version,
            "evidenceRange":item.evidence.to_json(),
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
    let state_delta = task_states.iter().map(|task| {
        let mut payload = json!({"actualNarrative":true,"sourceArtifactId":final_ref.artifact_id,"sourceVersion":final_ref.version});
        let mut occurrence_chapter = chapter_id.as_str();
        if let Some(progress) = stored_progress.as_ref().filter(|_| {
            verified_step.as_ref().is_some_and(|result| task["taskId"] == result.task_id)
        }) {
            payload["stageStepProgress"] = json!(progress);
            if let Some(source) = stage_origin {
                occurrence_chapter = &source.chapter_id;
                payload["sourceArtifactId"] = json!(source.final_ref.artifact_id);
                payload["sourceVersion"] = json!(source.final_ref.version);
                payload["verificationChapterId"] = json!(chapter_id);
            }
        }
        json!({"stateId":task["taskId"],"stateType":"volume.task.progress",
            "chapterId":occurrence_chapter,"valueJson":task,"payloadJson":payload})
    }).collect::<Vec<_>>();
    bundle_artifacts.push(candidate);
    bundle_artifacts.extend([audit, final_artifact, exit_artifact, receipt_artifact]);
    bundle_artifacts.extend(change_request_artifacts);
    let dependency_source_hash = if gateway.fault_injection() == Some("dependency_source_hash") {
        "0".repeat(64)
    } else {
        plan_ref.payload_hash.clone()
    };
    let mut dependency_edges = vec![json!({
        "edgeId":format!("edge:{}:plan",final_ref.artifact_id),"sourceRef":plan_ref.artifact_id,
        "targetRef":final_ref.artifact_id,"dependencyType":"PLAN_INPUT","scope":"CHAPTER",
        "impactClass":"HARD","sourceHash":dependency_source_hash,
        "payloadJson":{"createdBy":"B08::ChapterProductionRuntime"}
    })];
    dependency_edges.extend(proof_sources.values().map(|source| json!({
        "edgeId":format!("edge:{}:stage-step:{}",final_ref.artifact_id,source.artifact_id),
        "sourceRef":source.artifact_id,"targetRef":final_ref.artifact_id,"dependencyType":"STATE_INPUT",
        "scope":"CHAPTER","impactClass":"HARD","sourceHash":source.payload_hash,
        "payloadJson":{"createdBy":"B08::ChapterProductionRuntime","sourceVersion":source.version}
    })));
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
        "dependencyEdges":dependency_edges,
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

/// Use the same sentence numbers as the writer input. A span boundary inside
/// a quotation or at the chapter ending is unavailable for inserting prose.
fn chapter_expansion_anchors(body: &str) -> BTreeMap<usize, usize> {
    let spans = crate::chapter_craft::sentence_spans(body);
    let mut anchors = BTreeMap::new();
    let mut quote_ends = Vec::new();
    let mut scanned = 0;
    for (number, &(_, end)) in spans.iter().take(spans.len().saturating_sub(1)).enumerate() {
        for character in body[scanned..end].chars() {
            if quote_ends.last() == Some(&character) {
                quote_ends.pop();
            } else if let Some(closing) = match character {
                '“' => Some('”'),
                '‘' => Some('’'),
                '「' => Some('」'),
                '『' => Some('』'),
                '"' => Some('"'),
                _ => None,
            } {
                quote_ends.push(closing);
            }
        }
        scanned = end;
        if quote_ends.is_empty() {
            anchors.insert(number, end);
        }
    }
    anchors
}

/// Salvage inserts from an otherwise valid patch, keeping each edit anchored
/// to the original draft. The merged result still goes through prose review.
fn retain_nonduplicating_insertions(
    body: &str,
    insertions: &Value,
    anchors: &BTreeMap<usize, usize>,
) -> Option<String> {
    let mut selected = Vec::new();
    let mut result = None;
    for insertion in insertions.as_array()? {
        selected.push(insertion.clone());
        let trial = insert_chapter_passages(body, &json!(&selected), anchors)?;
        if crate::chapter_craft::duplicate_in_chapter(&trial).is_some() {
            selected.pop();
        } else {
            result = Some(trial);
        }
    }
    result
}

fn insert_chapter_passages(
    body: &str,
    insertions: &Value,
    anchors: &BTreeMap<usize, usize>,
) -> Option<String> {
    let insertions = insertions.as_array()?;
    if !(1..=EXPANSION_PASSAGES).contains(&insertions.len()) {
        return None;
    }
    let mut edits = BTreeMap::new();
    for insertion in insertions {
        let number = usize::try_from(insertion["afterSentence"].as_u64()?).ok()?;
        let position = *anchors.get(&number)?;
        let text = crate::chapter_craft::normalize_typography(insertion["text"].as_str()?.trim());
        if text.trim().is_empty()
            || text.chars().count() > EXPANSION_PASSAGE_MAX_CHARS
            || edits.insert(position, text).is_some()
        {
            return None;
        }
    }
    let mut result = body.to_owned();
    for (position, text) in edits.into_iter().rev() {
        let separator = if body[position..].starts_with(['\n', '\r']) {
            ""
        } else {
            "\n\n"
        };
        result.insert_str(position, &format!("\n\n{text}{separator}"));
    }
    Some(result)
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

struct SelectedNarrativeEvent {
    event_type: String,
    evidence: EvidenceSpan,
    outcome_quote: String,
}

const MAIN_EVENT_TYPES: &[&str] = &[
    "DISCOVERY",
    "CONFRONTATION",
    "LOSS",
    "GAIN",
    "REVEAL",
    "BETRAYAL",
    "ALLIANCE",
    "DEATH",
    "SITUATION_CHANGE",
];

fn rejected_numbered_event_outcome(selection: &Value, reason: &str) -> Option<u64> {
    if selection["firstSentence"].is_number()
        && matches!(
            reason,
            "EVENT_SELECTION_INVALID" | "EVENT_RESULT_IS_REACTION" | "EVENT_RESULT_IS_FORECAST"
        )
    {
        selection["outcomeSentence"].as_u64()
    } else {
        None
    }
}

pub(crate) fn is_main_event_type(value: &str) -> bool {
    MAIN_EVENT_TYPES.contains(&value)
}

fn discard_unsupported_audit_event_labels(audit: &mut Value, body: &str) {
    let mut deferred = Vec::new();
    for item in audit["canonCandidates"]
        .as_array_mut()
        .into_iter()
        .flatten()
    {
        let Some(event_type) = item["eventType"]
            .as_str()
            .filter(|value| is_main_event_type(value))
        else {
            continue;
        };
        let supported = item.pointer("/evidence/quote").and_then(Value::as_str).is_some_and(|quote| {
            let Some((first, last)) = crate::chapter_craft::numbered_passage_range(body, quote) else {
                return false;
            };
            select_main_event_evidence(body, &json!({
                "actionSentence":first,"outcomeSentence":last,"outcomeQuote":quote,"eventType":event_type
            })).is_ok_and(|event| event.is_some())
        });
        if !supported {
            deferred.push(json!({"factId":item["factId"],"eventType":event_type,
                "reason":"QC1_AUDIT_EVENT_OUTCOME_UNSUPPORTED"}));
            item.as_object_mut()
                .expect("event annotation is an object")
                .remove("eventType");
        }
    }
    if !deferred.is_empty() {
        audit["normalization"]["unsupportedAuditEvents"] = json!(deferred);
    }
}

fn bind_chapter_goal_review(goal: &str, body: &str, review: &Value) -> Option<Value> {
    let status = review["status"].as_str()?;
    let missing = match review.get("missing") {
        Some(Value::Null) | None => None,
        Some(value) => Some(
            value
                .as_str()
                .filter(|gap| (2..=160).contains(&gap.chars().count()) && goal.contains(gap))?,
        ),
    };
    let citations = review["citations"].as_array()?;
    let legal_shape = match status {
        "SUPPORTED" => missing.is_none() && (1..=3).contains(&citations.len()),
        "PARTIAL" => missing.is_some() && (1..=3).contains(&citations.len()),
        "NOT_SHOWN" => missing.is_some() && citations.is_empty(),
        _ => false,
    };
    if !legal_shape {
        return None;
    }
    let sentences = crate::chapter_craft::sentence_spans(body);
    let evidence = citations
        .iter()
        .map(|citation| {
            let number = usize::try_from(citation["sentence"].as_u64()?).ok()?;
            let (start, end) = *sentences.get(number)?;
            let quote = citation["quote"].as_str()?;
            if !(4..=160).contains(&quote.chars().count()) {
                return None;
            }
            let sentence = &body[start..end];
            let mut matches = sentence.match_indices(quote);
            let (offset, _) = matches.next()?;
            if matches.next().is_some() {
                return None;
            }
            let range_start = body[..start + offset].chars().count();
            Some(json!({"sentence":number,"start":range_start,
            "end":range_start + quote.chars().count(),"quote":quote}))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(json!({"status":status,"missing":missing,"evidence":evidence}))
}

fn select_main_event_evidence(
    body: &str,
    selection: &Value,
) -> Result<Option<SelectedNarrativeEvent>, String> {
    const INVALID: &str = "EVENT_SELECTION_INVALID";
    let numbered;
    let mut numbered_range = None;
    let selection = if let Some(first) = selection.get("firstSentence") {
        let outcome = selection.get("outcomeSentence").ok_or(INVALID)?;
        let event_type = selection.get("eventType").ok_or(INVALID)?;
        if first.is_null() && outcome.is_null() && event_type.is_null() {
            return Ok(None);
        }
        let index = |value: &Value| {
            value
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or(INVALID)
        };
        let (first, outcome) = (index(first)?, index(outcome)?);
        if first > outcome {
            return Err("EVENT_ACTION_ORDER_INVALID".into());
        }
        let sentences = crate::chapter_craft::sentence_spans(body);
        let end = sentences.get(outcome).ok_or(INVALID)?.1;
        // The outcome can be proven by a short ending passage. A far-off
        // starting sentence is context, not evidence of its cause.
        let first = (first.max(outcome.saturating_sub(2))..=outcome)
            .find(|&index| body[sentences[index].0..end].chars().count() <= 160)
            .ok_or(INVALID)?;
        let start = sentences[first].0;
        numbered_range = Some((start, end));
        numbered = json!({"actionSentence":first,"outcomeSentence":outcome,
                          "eventType":event_type,"outcomeQuote":&body[start..end]});
        &numbered
    } else {
        selection
    };
    let event_type = selection.get("eventType").ok_or(INVALID)?;
    let outcome_quote = selection.get("outcomeQuote").ok_or(INVALID)?;
    let action = selection.get("actionSentence").ok_or(INVALID)?;
    let outcome = selection.get("outcomeSentence").ok_or(INVALID)?;
    if [event_type, outcome_quote, action, outcome]
        .iter()
        .all(|value| value.is_null())
    {
        return Ok(None);
    }
    let event_type = event_type
        .as_str()
        .filter(|value| is_main_event_type(value))
        .ok_or(INVALID)?;
    let index = |field: &Value| {
        field
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(INVALID)
    };
    let (action, outcome) = (index(action)?, index(outcome)?);
    if outcome.checked_sub(action).is_none_or(|length| length > 8) {
        return Err("EVENT_ACTION_ORDER_INVALID".into());
    }
    let spans = crate::chapter_craft::sentence_spans(body);
    let &(result_start, end) = spans.get(outcome).ok_or(INVALID)?;
    let reported = outcome_quote.as_str().ok_or(INVALID)?;
    let quote = |sentence: &str| {
        crate::chapter_craft::exact_punctuation_matched_quote(sentence, reported).map(str::to_owned)
    };
    let adjacent_quote = |first: usize, second: usize| {
        let &(from, split_before) = spans.get(first)?;
        let &(split_after, to) = spans.get(second)?;
        if quote(&body[from..split_before]).is_some() || quote(&body[split_after..to]).is_some() {
            return None;
        }
        quote(&body[from..to]).map(|exact| (spans[action].0.min(from), to, exact))
    };
    let local = quote(&body[result_start..end])
        .map(|exact| (spans[action].0, end, exact))
        .or_else(|| {
            outcome
                .checked_sub(1)
                .and_then(|previous| adjacent_quote(previous, outcome))
        })
        .or_else(|| adjacent_quote(outcome, outcome + 1));
    let (start, end, outcome_quote) = if let Some((start, end)) = numbered_range {
        (start, end, body[start..end].to_string())
    } else if let Some(exact) = local {
        exact
    } else {
        // If the numbering drifted, keep just the uniquely located outcome.
        // Guessing which earlier action caused it would invent evidence.
        let exact = quote(body).ok_or("EVENT_QUOTE_NOT_FOUND")?;
        let from = body.find(&exact).ok_or("EVENT_QUOTE_NOT_FOUND")?;
        (from, from + exact.len(), exact)
    };
    if outcome_quote.chars().count() > 160
        || outcome_quote
            .chars()
            .filter(|c| c.is_alphanumeric())
            .count()
            < 4
        || body[start..end].chars().count() > 400
    {
        return Err(INVALID.into());
    }
    if event_type == "GAIN" && !explicit_acquisition(&outcome_quote) {
        return Err("EVENT_GAIN_WITHOUT_ACQUISITION".into());
    }
    let outcome_words = outcome_quote.trim_end_matches(['。', '！', '？', '”']);
    if outcome_words.chars().count() <= 36
        && outcome_quote.trim_end_matches('”').ends_with(['？', '?'])
    {
        return Err("EVENT_RESULT_IS_REACTION".into());
    }
    if outcome_words.chars().count() <= 12
        && ["脸色变了", "脸色一变"]
            .iter()
            .any(|ending| outcome_words.ends_with(ending))
    {
        return Err("EVENT_RESULT_IS_REACTION".into());
    }
    let last_sentence = crate::chapter_craft::sentence_spans(&outcome_quote)
        .last()
        .map_or(outcome_quote.as_str(), |&(start, end)| {
            &outcome_quote[start..end]
        })
        .trim()
        .trim_start_matches(['“', '‘', '"']);
    let words = last_sentence.trim_end_matches(['。', '！', '？', '”', '’', '"']);
    if (words.chars().count() <= 36
        && last_sentence
            .trim_end_matches(['”', '’', '"'])
            .ends_with(['？', '?']))
        || [
            "嗯",
            "哦",
            "是",
            "对",
            "我知道",
            "小的知道",
            "我明白",
            "没错",
        ]
        .contains(&words)
        || (words.chars().count() <= 24
            && ["不敢猜", "说不准", "不好说", "不确定"]
                .iter()
                .any(|ending| words.ends_with(ending)))
    {
        return Err("EVENT_RESULT_IS_REACTION".into());
    }
    if matches!(event_type, "DISCOVERY" | "REVEAL" | "SITUATION_CHANGE")
        && last_sentence.chars().count() <= 36
        && !last_sentence.contains(['，', '；', '：', ',', ';', ':'])
        && ["明天", "明日", "今晚", "今夜", "下次"]
            .iter()
            .any(|time| last_sentence.starts_with(time))
        && ["还会", "将会", "就会", "一定会", "即将"]
            .iter()
            .any(|modal| last_sentence.contains(modal))
    {
        return Err("EVENT_RESULT_IS_FORECAST".into());
    }
    Ok(Some(SelectedNarrativeEvent {
        event_type: event_type.to_string(),
        evidence: EvidenceSpan {
            start: body[..start].chars().count(),
            end: body[..end].chars().count(),
            quote: body[start..end].to_string(),
            rebound: false,
        },
        outcome_quote,
    }))
}

pub(crate) fn is_required_stage_step(task_id: &str) -> bool {
    task_id.starts_with("task:required:goal:") && task_id.contains(":step:")
}

const STAGE_STEP_RULE_VERSION: &str = "stage-step.r4";

pub(crate) fn stage_step_rules(objective: &str) -> Value {
    let conditions = stage_step_objectives(objective)
        .into_iter()
        .enumerate()
        .map(|(index, outcome)| json!({"id":index + 1,"requiredOutcome":outcome}))
        .collect::<Vec<_>>();
    json!({"version":STAGE_STEP_RULE_VERSION,"conditions":conditions})
}

struct PlannedStageStep {
    task_id: String,
    objective: String,
    progress: Option<StageStepProgress>,
    recent_unverified_attempts: Vec<Value>,
}

impl PlannedStageStep {
    fn input(&self) -> Value {
        let mut value = json!({
            "taskId":self.task_id,"objective":self.objective,
            "outcomeRules":stage_step_rules(&self.objective),
        });
        if let Some(progress) = &self.progress {
            value["progress"] = json!(progress);
        }
        if !self.recent_unverified_attempts.is_empty() {
            value["recentUnverifiedAttempts"] = json!(self.recent_unverified_attempts);
        }
        value
    }
}

fn planned_stage_step(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    chapter_id: &str,
    chapter_number: u32,
    plan: &Value,
) -> Result<Option<PlannedStageStep>, String> {
    let direction = crate::book_direction::for_chapter(
        connection,
        project_id,
        chapter_id,
        i64::from(chapter_number),
    )?;
    let Some(stage) = direction
        .as_ref()
        .and_then(|value| value.get("currentStage"))
    else {
        return Ok(None);
    };
    let Some(task_id) = stage["taskId"]
        .as_str()
        .filter(|id| is_required_stage_step(id))
    else {
        return Ok(None);
    };
    if !plan["activeTaskRefs"]
        .as_array()
        .is_some_and(|refs| refs.iter().any(|item| item == task_id))
    {
        return Ok(None);
    }
    let objective = stage["goal"]
        .as_str()
        .filter(|objective| !objective.trim().is_empty())
        .ok_or("B08_STAGE_STEP_OBJECTIVE_REQUIRED")?;
    let progress = stage
        .get("progress")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| format!("B08_STAGE_STEP_PROGRESS_INVALID:{error}"))?;
    Ok(Some(PlannedStageStep {
        task_id: task_id.to_owned(),
        objective: objective.to_owned(),
        progress,
        recent_unverified_attempts: stage["recentUnverifiedAttempts"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
    }))
}

fn select_stage_step_evidence(body: &str, selection: &Value) -> Option<EvidenceSpan> {
    let first = selection["firstSentence"].as_u64()?;
    let outcome = selection["outcomeSentence"].as_u64()?;
    if outcome.checked_sub(first)? > 2 {
        return None;
    }
    select_main_event_evidence(
        body,
        &json!({
            "firstSentence":first,"outcomeSentence":outcome,"eventType":"SITUATION_CHANGE"
        }),
    )
    .ok()
    .flatten()
    .map(|event| event.evidence)
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StageStepSource {
    chapter_id: String,
    final_ref: ArtifactRef,
    model_run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    verification_chapter_id: Option<String>,
}

struct StageStepHistory {
    chapter_no: i64,
    chapter_id: String,
    final_ref: ArtifactRef,
    body: String,
}

/// An earlier chapter as the continuity checks read it: its first 4000
/// characters in numbered sentences.
fn numbered_prior_chapter(chapter_no: &Value, chapter_id: &Value, body: &str) -> Value {
    let excerpt_end = body
        .char_indices()
        .nth(4_000)
        .map_or(body.len(), |(index, _)| index);
    let sentences = crate::chapter_craft::sentence_spans(body)
        .into_iter()
        .take_while(|&(_, end)| end <= excerpt_end)
        .enumerate()
        .map(|(number, (start, end))| json!({"number":number,"text":&body[start..end]}))
        .collect::<Vec<_>>();
    json!({"chapterNo":chapter_no,"chapterId":chapter_id,
       "excerpts":excerpt_end < body.len(),"sentences":sentences})
}

/// `prior` with the opening chapters `(number, body)` it lacks that come
/// before chapter `current`.
fn with_opening_chapters(
    prior: &[Value],
    opening: &[(i64, String)],
    current: Option<i64>,
) -> Vec<Value> {
    let mut chapters = prior.to_vec();
    for (chapter_no, body) in opening {
        if current.is_some_and(|current| *chapter_no < current)
            && !prior
                .iter()
                .any(|chapter| chapter["chapterNo"] == *chapter_no)
        {
            chapters.push(numbered_prior_chapter(
                &json!(chapter_no),
                &Value::Null,
                body,
            ));
        }
    }
    chapters
}

/// The committed chapters that may already show the step's result: the
/// opening chapters of its volume (owner decision 2026-11-25) and the
/// chapters that worked on it.
fn stage_step_history(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    volume_id: &str,
    stage: &PlannedStageStep,
) -> Result<Vec<StageStepHistory>, String> {
    let load = |chapter_no: i64, chapter_id: String| -> Result<_, String> {
        let Some(final_ref) =
            NarrativeRepository::committed_final_chapter_head(connection, project_id, &chapter_id)
                .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        let payload = load_artifact_payload(connection, project_id, &final_ref)?;
        Ok(Some(StageStepHistory {
            chapter_no,
            chapter_id,
            body: require(&payload, "/body")?,
            final_ref,
        }))
    };
    let mut history = Vec::new();
    for chapter_no in 1..=crate::repositories::project_entry::OPENING_CHAPTER_COUNT {
        let chapter_id = NarrativeRepository::chapter_id_for_number(
            connection, project_id, volume_id, chapter_no,
        )
        .map_err(|error| error.to_string())?;
        if let Some(chapter_id) = chapter_id {
            history.extend(load(chapter_no, chapter_id)?);
        }
    }
    for attempt in &stage.recent_unverified_attempts {
        let chapter_no = attempt["chapterNo"]
            .as_i64()
            .ok_or("B08_STAGE_SOURCE_CHAPTER_REQUIRED")?;
        let expected: ArtifactRef = serde_json::from_value(attempt["finalRef"].clone())
            .map_err(|error| format!("B08_STAGE_SOURCE_REF_INVALID:{error}"))?;
        let source = load(chapter_no, require(attempt, "/chapterId")?)?
            .filter(|source| source.final_ref == expected)
            .ok_or("PLAN_STALE:STAGE_STEP_SOURCE_CHANGED")?;
        history.push(source);
    }
    Ok(history)
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StageStepEvidence {
    evidence: EvidenceSpan,
    objective_quote: String,
    outcome_quote: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    action_quote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    action_evidence: Option<EvidenceSpan>,
    fulfilled_objectives: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<StageStepSource>,
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct StageStepProgress {
    objective: String,
    evidence_parts: Vec<StageStepEvidence>,
    remaining_objectives: Vec<String>,
}

struct VerifiedStageStep {
    task_id: String,
    progress: StageStepProgress,
    model_run_id: String,
}

impl VerifiedStageStep {
    fn status(&self) -> &'static str {
        if self.progress.remaining_objectives.is_empty() {
            "SATISFIED"
        } else {
            "ACTIVE"
        }
    }

    fn claim(&self) -> TaskProgressCandidate {
        let proof = self
            .progress
            .evidence_parts
            .iter()
            .find(|part| part.source.is_none())
            .unwrap_or(&self.progress.evidence_parts[0]);
        TaskProgressCandidate {
            task_id: self.task_id.clone(),
            status: self.status().into(),
            evidence: proof.evidence.clone(),
        }
    }

    fn has_current_evidence(&self) -> bool {
        self.progress
            .evidence_parts
            .iter()
            .any(|part| part.source.is_none())
    }

    fn historical_origin(
        &self,
        connection: &libsql_rusqlite::Connection,
        project_id: &str,
    ) -> Result<Option<&StageStepSource>, String> {
        if self.has_current_evidence() {
            return Ok(None);
        }
        let mut latest = None;
        for source in self
            .progress
            .evidence_parts
            .iter()
            .filter_map(|part| part.source.as_ref())
        {
            let number =
                NarrativeRepository::chapter_number(connection, project_id, &source.chapter_id)
                    .map_err(|error| error.to_string())?
                    .ok_or("B08_STAGE_SOURCE_CHAPTER_REQUIRED")?;
            if latest.is_none_or(|(latest_number, _)| number > latest_number) {
                latest = Some((number, source));
            }
        }
        Ok(Some(latest.ok_or("B08_STAGE_SOURCE_CHAPTER_REQUIRED")?.1))
    }

    fn recorded_progress(&self, chapter_id: &str, final_ref: &ArtifactRef) -> StageStepProgress {
        let mut progress = self.progress.clone();
        for part in &mut progress.evidence_parts {
            if part.source.is_none() {
                part.source = Some(StageStepSource {
                    chapter_id: chapter_id.into(),
                    final_ref: final_ref.clone(),
                    model_run_id: self.model_run_id.clone(),
                    verification_chapter_id: None,
                });
            }
        }
        progress
    }
}

fn stage_step_objectives(objective: &str) -> Vec<String> {
    let compound = objective.replace("以及", "；").replace("并且", "；");
    // A comparison connects its operands with 与. Keep them in a single
    // condition unless the author specifies separate results.
    let separators: &[_] = if objective.contains("比对") {
        &['；', '且']
    } else {
        &['；', '且', '与']
    };
    let parts = compound
        .split(separators)
        .map(str::trim)
        .filter(|part| {
            part.chars()
                .filter(|&character| crate::chapter_craft::is_han(character))
                .count()
                >= 4
        })
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if parts.len() > 1 {
        parts
    } else {
        vec![objective.to_owned()]
    }
}

fn stage_step_remaining(objective: &str, proofs: &[StageStepEvidence]) -> Vec<String> {
    stage_step_objectives(objective)
        .into_iter()
        .filter(|part| {
            !proofs
                .iter()
                .any(|proof| proof.fulfilled_objectives.contains(part))
        })
        .collect()
}

pub(crate) fn refresh_replaced_stage_progress(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    raw: Value,
    state: &mut Value,
) -> Result<Option<Value>, String> {
    let mut progress: StageStepProgress = serde_json::from_value(raw)
        .map_err(|error| format!("B08_STAGE_STEP_PROGRESS_INVALID:{error}"))?;
    let mut current = Vec::new();
    let original_count = progress.evidence_parts.len();
    for proof in progress.evidence_parts {
        if stage_step_source_is_current(connection, project_id, &proof)? {
            current.push(proof);
        }
    }
    if current.len() == original_count {
        return Ok(None);
    }
    progress.remaining_objectives = stage_step_remaining(&progress.objective, &current);
    if !progress.remaining_objectives.is_empty() {
        state["status"] = json!("ACTIVE");
    }
    progress.evidence_parts = current;
    let (references, remaining) = stage_step_proof_fields(&progress);
    state["evidenceRefs"] = json!(references);
    state["remainingConditions"] = json!(remaining);
    Ok(Some(json!(progress)))
}

fn stage_step_proof_fields(progress: &StageStepProgress) -> (Vec<Value>, Vec<Value>) {
    let mut seen = BTreeSet::new();
    let references = progress
        .evidence_parts
        .iter()
        .filter_map(|part| {
            let reference = &part.source.as_ref()?.final_ref;
            let span = &part.evidence;
            seen.insert((
                &reference.artifact_id,
                reference.version,
                span.start,
                span.end,
            ))
            .then(|| task_evidence_ref(span, &reference.artifact_id, reference.version))
        })
        .collect();
    let remaining = progress
        .remaining_objectives
        .iter()
        .map(|goal| json!({"kind":"STAGE_STEP_OUTCOME","value":goal}))
        .collect();
    (references, remaining)
}

fn task_evidence_ref(span: &EvidenceSpan, artifact_id: &str, version: i64) -> Value {
    json!({"artifactId":artifact_id,"version":version,"rangeStart":span.start,
        "rangeEnd":span.end,"quote":span.quote})
}

pub(crate) fn stage_step_progress_for_chapter(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    volume_id: &str,
    chapter_no: i64,
    task_id: &str,
    objective: &str,
) -> Result<Option<Value>, String> {
    use crate::repositories::narrative::NarrativeRepository;
    if !is_required_stage_step(task_id) {
        return Ok(None);
    }
    let Some(raw) = NarrativeRepository::active_stage_step_progress(
        connection, project_id, volume_id, chapter_no, task_id,
    )
    .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let mut progress: StageStepProgress = serde_json::from_value(raw)
        .map_err(|error| format!("B08_STAGE_STEP_PROGRESS_INVALID:{error}"))?;
    if progress.objective != objective {
        return Ok(None);
    }
    let mut current = Vec::new();
    for proof in progress.evidence_parts {
        if stage_step_source_is_current(connection, project_id, &proof)? {
            current.push(proof);
        }
    }
    if current.is_empty() {
        return Ok(None);
    }
    progress.remaining_objectives = stage_step_remaining(objective, &current);
    progress.evidence_parts = current;
    Ok(Some(json!(progress)))
}

fn stage_step_source_is_current(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    proof: &StageStepEvidence,
) -> Result<bool, String> {
    use crate::repositories::narrative::NarrativeRepository;
    let Some(source) = proof.source.as_ref() else {
        return Ok(false);
    };
    let current = NarrativeRepository::committed_final_chapter_head(
        connection,
        project_id,
        &source.chapter_id,
    )
    .map_err(|error| error.to_string())?;
    if current.as_ref() != Some(&source.final_ref) {
        return Ok(false);
    }
    let payload =
        NarrativeRepository::artifact_payload_by_ref(connection, project_id, &source.final_ref)
            .map_err(|error| error.to_string())?;
    Ok(payload
        .as_ref()
        .and_then(|payload| payload["body"].as_str())
        .is_some_and(|body| {
            exact_quote_span(
                body,
                proof.evidence.start,
                proof.evidence.end,
                &proof.evidence.quote,
            )
            .is_ok()
                && proof.action_evidence.as_ref().is_none_or(|action| {
                    exact_quote_span(body, action.start, action.end, &action.quote).is_ok()
                })
        }))
}

fn different_batches_shown(quote: &str) -> bool {
    ["不同批次", "批次各异", "各自的批次"]
        .iter()
        .any(|label| quote.contains(label))
        || quote.match_indices('批').any(|(index, _)| {
            let mut prefix = quote[..index].trim_end_matches('个').chars().rev();
            prefix
                .next()
                .is_some_and(|number| "二两三四五六七八九十23456789".contains(number))
                && prefix.next() != Some('第')
        })
}

fn stage_step_citations(selection: &Value) -> &[Value] {
    selection["evidenceParts"]
        .as_array()
        .map_or(std::slice::from_ref(selection), Vec::as_slice)
}

/// What a citation's wording alone says about a step's result. Words only
/// point at passages to read (PRODUCTION_RULES §4.2): a citation without the
/// usual ones is read a second time, not refused.
enum StageStepWording {
    Shown,
    Unclear,
    Refuted,
}

fn stage_step_wording(
    goal: &str,
    action: &str,
    outcome: &str,
    passages: [&str; 2],
) -> StageStepWording {
    let contains_any = |text: &str, terms: &[&str]| terms.iter().any(|term| text.contains(term));
    let refuted = contains_any(
        action,
        &["还没有", "尚未", "没有做", "未能", "打算", "想要"],
    ) || contains_any(
        outcome,
        &["还没有比对", "尚未比对", "尚未证实", "未能取得", "尚未查明"],
    ) || outcome.trim_end_matches(['”', '’']).ends_with(['？', '?']);
    let same_origin = goal.contains("确认")
        && goal.contains("同源")
        && !contains_any(goal, &["不同源", "并非同源"]);
    let batches = goal.contains("不同批次");
    if refuted
        || same_origin
            && contains_any(
                outcome,
                &["不同源", "并非同源", "不能证实", "无法确认", "无法证实"],
            )
        || batches
            && outcome.contains("同一批")
            && !contains_any(outcome, &["不是同一批", "非同一批"])
    {
        return StageStepWording::Refuted;
    }
    let compared = !goal.contains("比对")
        || passages.iter().any(|passage| {
            contains_any(
                passage,
                &[
                    "比对",
                    "对照",
                    "对比",
                    "并排对",
                    "逐一核对",
                    "相吻合",
                    "一一对应",
                    "重合",
                    "对得上",
                    "一致",
                    "丝毫不差",
                    "分毫不差",
                    "不同",
                    "差异",
                ],
            )
        });
    let origin_stated = !same_origin
        || contains_any(
            outcome,
            &[
                "同源",
                "出自同一",
                "来自同一",
                "同一个源头",
                "同一来源",
                "一个来处",
            ],
        );
    let batches_shown = !batches
        || passages
            .iter()
            .any(|passage| different_batches_shown(passage));
    if compared && origin_stated && batches_shown {
        StageStepWording::Shown
    } else {
        StageStepWording::Unclear
    }
}

fn grounded_stage_step_parts(
    body: &str,
    objective: &str,
    selection: &Value,
    second_look: bool,
) -> Option<Vec<StageStepEvidence>> {
    let locate = crate::chapter_craft::exact_punctuation_matched_quote;
    let selections = stage_step_citations(selection);
    if !(1..=3).contains(&selections.len()) {
        return None;
    }
    let objectives = stage_step_objectives(objective);
    let proofs = selections
        .iter()
        .filter_map(|part| {
            let reported = part["outcomeQuote"].as_str()?;
            let reported_action = part["actionQuote"].as_str()?;
            if ![reported, reported_action]
                .iter()
                .all(|quote| (4..=160).contains(&quote.chars().count()))
            {
                return None;
            }
            let locate_passage = |quote: &str| {
                let (first, last) = crate::chapter_craft::numbered_passage_range(body, quote)?;
                select_stage_step_evidence(
                    body,
                    &json!({"firstSentence":first,"outcomeSentence":last}),
                )
            };
            let evidence = select_stage_step_evidence(body, part)
                .filter(|selected| locate(&selected.quote, reported).is_some())
                .or_else(|| locate_passage(reported))?;
            let action_evidence = if locate(&evidence.quote, reported_action).is_some() {
                None
            } else {
                Some(locate_passage(reported_action)?)
            };
            let action_span = action_evidence.as_ref().unwrap_or(&evidence);
            let index = usize::try_from(part["condition"].as_u64()?)
                .ok()?
                .checked_sub(1)?;
            let objective_quote = objectives.get(index)?;
            let action = locate(&action_span.quote, reported_action)?.to_owned();
            let outcome_quote = locate(&evidence.quote, reported)?.to_owned();
            let quote_start = |span: &EvidenceSpan, quote: &str| {
                span.quote
                    .find(quote)
                    .map(|offset| span.start + span.quote[..offset].chars().count())
            };
            let outcome_end =
                quote_start(&evidence, &outcome_quote)? + outcome_quote.chars().count();
            let shown = match stage_step_wording(
                objective_quote,
                &action,
                &outcome_quote,
                [&action_span.quote, &evidence.quote],
            ) {
                StageStepWording::Shown => true,
                StageStepWording::Unclear => second_look,
                StageStepWording::Refuted => false,
            };
            if !shown || quote_start(action_span, &action)? >= outcome_end {
                return None;
            }
            Some(StageStepEvidence {
                objective_quote: objective_quote.clone(),
                outcome_quote,
                evidence,
                action_quote: Some(action),
                action_evidence,
                fulfilled_objectives: vec![objective_quote.clone()],
                source: None,
            })
        })
        .collect::<Vec<_>>();
    (!proofs.is_empty()).then_some(proofs)
}

#[cfg(test)]
fn grounded_stage_step_evidence(
    body: &str,
    objective: &str,
    selection: &Value,
) -> Option<Vec<StageStepEvidence>> {
    let proofs = grounded_stage_step_parts(body, objective, selection, false)?;
    stage_step_remaining(objective, &proofs)
        .is_empty()
        .then_some(proofs)
}

/// A citation may name the chapter it belongs to as its source.
fn drop_own_source_chapter(selection: &mut Value, chapter_no: u32) {
    let drop_source = |citation: &mut Value| {
        if citation["sourceChapter"].is_null() || citation["sourceChapter"] == chapter_no {
            if let Some(citation) = citation.as_object_mut() {
                citation.remove("sourceChapter");
            }
        }
    };
    match selection["evidenceParts"].as_array_mut() {
        Some(parts) => parts.iter_mut().for_each(drop_source),
        None => drop_source(selection),
    }
}

#[allow(clippy::too_many_arguments)]
fn evaluate_stage_step(
    stage: &PlannedStageStep,
    body: &str,
    previous_prose: &[(i64, &str)],
    historical_sources: &[StageStepHistory],
    selection: &Value,
    chapter_id: &str,
    model_run_id: &str,
    second_look: bool,
) -> Option<VerifiedStageStep> {
    let citations = stage_step_citations(selection);
    if !(1..=3).contains(&citations.len()) {
        return None;
    }
    let mut citations = citations.iter().collect::<Vec<_>>();
    citations.sort_by_key(|citation| citation["sourceChapter"].as_i64().unwrap_or(i64::MAX));
    let prior = stage
        .progress
        .as_ref()
        .map(|progress| progress.evidence_parts.as_slice())
        .unwrap_or_default();
    let mut accomplished = prior
        .iter()
        .flat_map(|proof| proof.fulfilled_objectives.iter().cloned())
        .collect::<BTreeSet<_>>();
    let mut new_proofs = Vec::new();
    for citation in citations {
        let historical = if let Some(number) = citation.get("sourceChapter") {
            let Some(source) = historical_sources
                .iter()
                .find(|source| number.as_i64() == Some(source.chapter_no))
            else {
                continue;
            };
            Some(source)
        } else {
            None
        };
        let prose = historical.map_or(body, |source| source.body.as_str());
        let Some(mut proof) =
            grounded_stage_step_parts(prose, &stage.objective, citation, second_look)
                .and_then(|proofs| proofs.into_iter().next())
        else {
            continue;
        };
        let source_number = historical.map_or(i64::MAX, |source| source.chapter_no);
        if previous_prose.iter().any(|(number, earlier)| {
            *number < source_number && earlier.contains(&proof.outcome_quote)
        }) {
            continue;
        }
        if let Some(source) = historical {
            proof.source = Some(StageStepSource {
                chapter_id: source.chapter_id.clone(),
                final_ref: source.final_ref.clone(),
                model_run_id: model_run_id.into(),
                verification_chapter_id: Some(chapter_id.into()),
            });
        }
        if proof
            .fulfilled_objectives
            .iter()
            .any(|goal| !accomplished.contains(goal))
        {
            accomplished.extend(proof.fulfilled_objectives.iter().cloned());
            new_proofs.push(proof);
        }
    }
    if new_proofs.is_empty() {
        return None;
    }
    new_proofs.extend_from_slice(prior);
    let remaining_objectives = stage_step_remaining(&stage.objective, &new_proofs);
    Some(VerifiedStageStep {
        task_id: stage.task_id.clone(),
        model_run_id: model_run_id.into(),
        progress: StageStepProgress {
            objective: stage.objective.clone(),
            evidence_parts: new_proofs,
            remaining_objectives,
        },
    })
}

fn apply_stage_step_verification(
    audit: &mut AuditExtraction,
    stage: Option<&PlannedStageStep>,
    verified: Option<&VerifiedStageStep>,
) {
    for claim in &mut audit.task_progress_candidates {
        if claim.status != "SATISFIED" || !is_required_stage_step(&claim.task_id) {
            continue;
        }
        if !verified
            .is_some_and(|result| result.task_id == claim.task_id && result.status() == "SATISFIED")
        {
            claim.status = "ACTIVE".into();
            audit.downgraded_task_claims.push(json!({
                "taskId":claim.task_id,"claimed":"SATISFIED","reason":"B08_STAGE_STEP_OUTCOME_UNVERIFIED"
            }));
        }
    }
    if let Some(result) = verified {
        audit
            .task_progress_candidates
            .retain(|claim| claim.task_id != result.task_id);
        if result.has_current_evidence() {
            audit.task_progress_candidates.push(result.claim());
        }
    } else if let Some(stage) = stage.filter(|step| step.progress.is_some()) {
        // An unverified ACTIVE claim must not replace earlier verified progress.
        audit.task_progress_candidates.retain(|claim| {
            claim.task_id != stage.task_id
                || matches!(claim.status.as_str(), "BLOCKED" | "INVALIDATED")
        });
    }
}

fn explicit_acquisition(outcome: &str) -> bool {
    [
        "获得",
        "得到",
        "到手",
        "入手",
        "收获",
        "接过",
        "交给",
        "换来",
        "赢得",
        "夺得",
        "夺回",
        "拾得",
        "捡到",
        "捡起",
        "赠给",
        "交还",
        "往他手心里一塞",
        "往他掌心里一塞",
    ]
    .iter()
    .any(|marker| outcome.contains(marker))
}

impl EvidenceSpan {
    /// The span as recorded: where it is and what it says.
    fn to_json(&self) -> Value {
        json!({"start":self.start,"end":self.end,"quote":self.quote})
    }
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
    /// P1: the relation this fact states, when it is about another entity.
    #[serde(default)]
    relation_id: Option<String>,
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

/// What a rewrite's edits did to a chapter.
struct FittedEdits {
    body: String,
    applied: usize,
    /// The chapter with every edit, when that was not acceptable.
    refused_whole: Option<String>,
}

/// The rewrite's edits together when the result is `acceptable`, else one by
/// one, leaving out those that make it unacceptable (real run 2026-11-25: 20
/// characters over the limit threw away every edit of the chapter). An edit
/// left out is tried again once the others have made room.
fn apply_edits_that_fit(
    body: &str,
    rewrite: &Value,
    additions: &[String],
    acceptable: impl Fn(&str) -> bool,
) -> FittedEdits {
    let apply = |body: &str, edits: &[Value]| {
        let (revised, applied) = apply_unique_edits(body, &json!({"edits":edits}), additions);
        let revised = crate::chapter_craft::normalize_typography(revised.trim());
        (applied > 0).then_some((revised, applied))
    };
    let edits = rewrite["edits"].as_array().map_or(&[][..], Vec::as_slice);
    let mut fitted = FittedEdits {
        body: body.to_string(),
        applied: 0,
        refused_whole: None,
    };
    match apply(body, edits) {
        Some((revised, applied)) if acceptable(&revised) => {
            fitted.body = revised;
            fitted.applied = applied;
        }
        Some((revised, _)) => {
            fitted.refused_whole = Some(revised);
            let mut pending = edits.iter().take(MAX_EDITS).collect::<Vec<_>>();
            for _ in 0..2 {
                pending.retain(|edit| {
                    let fits = apply(&fitted.body, std::slice::from_ref(edit))
                        .filter(|(text, _)| acceptable(text));
                    let left_out = fits.is_none();
                    if let Some((revised, _)) = fits {
                        fitted.body = revised;
                        fitted.applied += 1;
                    }
                    left_out
                });
            }
        }
        None => {}
    }
    fitted
}

/// How many of a rewrite's edits are applied.
const MAX_EDITS: usize = 8;

/// Applies the rewrite's `edits` whose passage occurs once in `body` and
/// whose replacement adds no model habit (an edit of a quote in `additions`
/// may), each spliced at its own span; returns the new body and how many
/// applied.
fn apply_unique_edits(body: &str, rewrite: &Value, additions: &[String]) -> (String, usize) {
    let mut revised = body.to_string();
    let mut applied = 0;
    for edit in rewrite
        .get("edits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_EDITS)
    {
        let (Some(quote), Some(replacement)) = (
            edit.get("quote").and_then(Value::as_str),
            edit.get("replacement").and_then(Value::as_str),
        ) else {
            continue;
        };
        // A rewrite must not trade one model habit for another.
        if replacement.trim().is_empty()
            || (crate::chapter_craft::habit_count(replacement)
                > crate::chapter_craft::habit_count(quote)
                && !additions.iter().any(|addition| addition == quote))
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

/// P1 (real run round 20: PERSON, CHARACTER, ITEM, ARTIFACT, DANLU…): the
/// one kind an audited entity type stands for.
pub(crate) fn entity_kind(entity_type: &str) -> &'static str {
    match entity_type.trim().to_uppercase().as_str() {
        "PERSON" | "CHARACTER" | "PEOPLE" | "HUMAN" | "NPC" => "PERSON",
        "PLACE" | "LOCATION" | "SITE" | "AREA" | "BUILDING" => "PLACE",
        "ORGANIZATION" | "ORGANISATION" | "ORG" | "SECT" | "FACTION" | "GROUP" | "CLAN" => {
            "ORGANIZATION"
        }
        "CREATURE" | "ANIMAL" | "BEAST" | "MONSTER" | "SPIRIT" => "CREATURE",
        _ => "OBJECT",
    }
}

/// A foreshadow's payoff as its ref, an OpaqueId: no whitespace, 1 to 200
/// characters.
pub(crate) fn payoff_ref(payoff: &str) -> Option<String> {
    Some(
        payoff
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("_")
            .chars()
            .take(200)
            .collect::<String>(),
    )
    .filter(|payoff| !payoff.is_empty())
}

/// What a new thread is about: its description, else the payoff it waits
/// for, else the quote it was found in (real run round 29: quotes read as
/// no description at all).
fn thread_description(item: &ContinuityCandidate) -> String {
    let given = |text: Option<&str>| {
        text.filter(|text| !text.trim().is_empty())
            .map(ToOwned::to_owned)
    };
    given(item.description.as_deref())
        .or_else(|| {
            given(item.fulfillment_object.as_deref()).map(|payoff| format!("待兑现：{payoff}"))
        })
        .or_else(|| {
            item.evidence
                .as_ref()
                .map(|evidence| evidence.quote.clone())
        })
        .unwrap_or_else(|| item.thread_ref.clone())
}

/// P1 (owner decision 2026-11-03): a new thread is a foreshadow when it is
/// a planted hint, or a key object, piece of information or mystery the
/// story will pay off with `fulfillment_object`.
fn is_foreshadow(
    signals: crate::narrative::PromotionSignal,
    fulfillment_object: Option<&str>,
) -> bool {
    signals.foreshadow_recovery_responsibility
        || ((signals.key_future_use_object_or_information
            || signals.mystery_or_reveal_responsibility)
            && fulfillment_object.is_some_and(|payoff| !payoff.trim().is_empty()))
}

/// P1 (real run round 20: no relation was ever recorded): the relation a
/// canon candidate states, when its object names another entity
/// (`objectJson.objectId`): its own relation id, or one derived from who
/// and whom, so a later change of how they stand updates the same relation.
fn candidate_relation_id(candidate: &CanonCandidate) -> Option<String> {
    let object = candidate
        .object_json
        .get("objectId")
        .and_then(Value::as_str)?;
    candidate
        .relation_id
        .clone()
        .or_else(|| {
            candidate
                .object_json
                .get("relationId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            sha256_canonical(&json!([candidate.subject_id, object]))
                .ok()
                .map(|digest| format!("relation:{}", &digest[..24]))
        })
}

fn resolved_mystery_event_marks(
    candidates: &[CanonCandidate],
    mysteries: &[(String, Option<EvidenceSpan>)],
) -> (BTreeMap<String, (&'static str, Value)>, Vec<String>) {
    let mut marks = BTreeMap::new();
    let mut missing = Vec::new();
    for (mystery, evidence) in mysteries {
        let matched = evidence.as_ref().and_then(|evidence| {
            candidates.iter().find(|item| {
                (item.event_type.is_none()
                    || item.event_type.as_deref() == Some("MYSTERY_RESOLVED"))
                    && !marks.contains_key(&item.fact_id)
                    && item.evidence.start == evidence.start
                    && item.evidence.end == evidence.end
                    && item.evidence.quote == evidence.quote
            })
        });
        if let Some(item) = matched {
            marks.insert(
                item.fact_id.clone(),
                ("MYSTERY_RESOLVED", json!({"mysteryId":mystery})),
            );
        } else {
            missing.push(mystery.clone());
        }
    }
    (marks, missing)
}

fn validate_relation_identity(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    candidate: &CanonCandidate,
    pending: &mut BTreeMap<String, (String, String)>,
) -> Result<(), String> {
    let nested = candidate
        .object_json
        .get("relationId")
        .and_then(Value::as_str);
    if candidate
        .relation_id
        .as_deref()
        .zip(nested)
        .is_some_and(|(a, b)| a != b)
    {
        return Err("B08_RELATION_IDENTITY_CONFLICT".into());
    }
    if let Some(relation_id) = candidate_relation_id(candidate) {
        let object_id = candidate.object_json["objectId"]
            .as_str()
            .unwrap_or_default();
        let pair = (candidate.subject_id.clone(), object_id.to_string());
        if pending
            .get(&relation_id)
            .is_some_and(|prior| prior != &pair)
            || crate::repositories::narrative::NarrativeRepository::relation_identity_conflicts(
                connection,
                project_id,
                &relation_id,
                &pair.0,
                &pair.1,
            )
            .map_err(|error| error.to_string())?
        {
            return Err("B08_RELATION_IDENTITY_CONFLICT".into());
        }
        pending.insert(relation_id, pair);
    }
    Ok(())
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
    // Knowledge about a fact that was held back is dropped with it, named
    // once for all of them.
    if !extraction.dropped_unbound_knowledge.is_empty() {
        held.push(json!({
            "field":"knowledgeCandidates",
            "item":extraction.dropped_unbound_knowledge,
            "reason":"B08_KNOWLEDGE_UNBOUND"
        }));
    }
    kept["normalization"]["heldBack"] = json!(held);
    Ok((kept, extraction))
}

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
    let held = split_off_indices(items, &over).into_iter().map(
        |item| json!({"field":"continuityCandidates","item":item,"reason":"QC1_THREAD_OVER_LIMIT"}),
    );
    hold_back(audit, held);
}

/// Takes the items at `indices` out of `items`, keeping the order of both.
fn split_off_indices(items: &mut Vec<Value>, indices: &[usize]) -> Vec<Value> {
    let (kept, taken): (Vec<_>, Vec<_>) = std::mem::take(items)
        .into_iter()
        .enumerate()
        .partition(|(index, _)| !indices.contains(index));
    *items = kept.into_iter().map(|(_, item)| item).collect();
    taken.into_iter().map(|(_, item)| item).collect()
}

/// Adds entries to what the audit holds back for the chapter's reminder.
fn hold_back(audit: &mut Value, entries: impl IntoIterator<Item = Value>) {
    let list = &mut audit["normalization"]["heldBack"];
    if !list.is_array() {
        *list = json!([]);
    }
    if let Some(list) = list.as_array_mut() {
        list.extend(entries);
    }
}

/// QC-1: how many new facts a chapter may record (the audit prompt, its
/// feedback and the reminder text say 12 too).
const MAX_NEW_FACTS: usize = 12;

/// QC-1 (acceptance 2026-10-26: an audit rerun reported 66 facts for one
/// chapter, and the next chapter's context overflowed): a chapter records
/// at most `MAX_NEW_FACTS` facts, those about the chapter's own `people`
/// first, then in the audit's order. The rest, and the knowledge about
/// them, are left out with one summary entry for the reminder.
fn cap_new_facts(audit: &mut Value, people: &[String]) {
    let Some(facts) = audit["canonCandidates"].as_array_mut() else {
        return;
    };
    if facts.len() <= MAX_NEW_FACTS {
        return;
    }
    let mut order = (0..facts.len()).collect::<Vec<_>>();
    order.sort_by_key(|index| {
        !facts[*index]["subjectId"]
            .as_str()
            .is_some_and(|subject| people.iter().any(|person| person == subject))
    });
    let over = order.split_off(MAX_NEW_FACTS);
    let dropped_ids = split_off_indices(facts, &over)
        .iter()
        .filter_map(|fact| fact["factId"].as_str().map(ToOwned::to_owned))
        .collect::<BTreeSet<_>>();
    let mut knowledge_dropped = 0;
    if let Some(knowledge) = audit["knowledgeCandidates"].as_array_mut() {
        let before = knowledge.len();
        knowledge.retain(|item| {
            !item["factId"]
                .as_str()
                .is_some_and(|fact| dropped_ids.contains(fact))
        });
        knowledge_dropped = before - knowledge.len();
    }
    hold_back(
        audit,
        [json!({
            "field":"canonCandidates",
            "item":{"facts":over.len(),"knowledge":knowledge_dropped},
            "reason":"QC1_FACTS_OVER_LIMIT"
        })],
    );
}

/// Why an audit item was held back, for the reader of the reminder.
fn held_back_reason(code: &str) -> &'static str {
    match code {
        "QC1_FACTS_OVER_LIMIT" => {
            "本章抽出的事实超过 12 条，本章人物以外的次要事实和相关认知没有记入档案"
        }
        "QC1_OPENING_THREAT_LATE" => {
            "第一章规划的迫近威胁没有在正文前三分之一写出来，开篇紧迫感不足"
        }
        "QC1_CONTRADICTION" => "这段可能与前文状态不一致，请核对原文和状态变化是否已有交代",
        "QC1_NARRATIVE_CONTINUITY" => "跨章事件或状态衔接还有疑点，请核对前后原文",
        "QC1_MAIN_EVENT_MISSING" => "本章没有提取到有证据的主要事件，正文已保留，请检查事件记录",
        "QC1_MAIN_EVENT_SELECTION_FAILED" => "主要事件补充检查没有得到可用结果，未自动生成事件",
        "QC1_MYSTERY_EVENT_MISSING" => "谜题解决线索未找到引用同一原文的事实，未生成谜题解决事件",
        "QC1_CATCHPHRASE" => "改写后这个口头禅在本章仍出现不止一次",
        "QC1_THREAD_OVER_LIMIT" => {
            "本章新开的伏笔超过 3 条，这条重要性靠后，只作为候选，没有记入档案"
        }
        "B08_KNOWLEDGE_UNBOUND" => "这条认知指向的事实没有记入档案",
        "B08_RELATION_IDENTITY_CONFLICT" => {
            "关系编号已属于另一对人物或相反方向，这条关系未记入档案"
        }
        "B08_THREAD_AUTHORITY_UNBOUND" => "这条伏笔推进缺少可核对的已有线索，已留待复核",
        "B08_CHAPTER_GOAL_NOT_VERIFIED" => "本章目标还有结果未得到正文证据支持，请核对章纲和正文",
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
/// in `known` (with or without `thread:`) keeps it. Openings receive a new
/// system id. Updates without authority are returned with the id assignments
/// so the audit can preserve the other changes and leave a reminder.
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
    let mut unbound_thread_updates = Vec::new();
    if let Some(threads) = raw["continuityCandidates"].as_array_mut() {
        threads.retain(|item| {
            let given = item["threadRef"].as_str().unwrap_or_default();
            let has_authority = known.contains(given) || known.contains(&format!("thread:{given}"));
            let operation = item["threadOperation"].as_str();
            let may_open = match operation {
                Some("OPEN") => !has_authority && item["disposition"] == "ADVANCE",
                Some(_) => false,
                None => {
                    !item.get("threadRef").is_some_and(Value::is_null)
                        && matches!(item["disposition"].as_str(), Some("ADVANCE") | None)
                }
            };
            let allowed = (has_authority && operation != Some("OPEN")) || may_open;
            if !allowed {
                unbound_thread_updates.push(json!({
                    "field":"continuityCandidates","threadRef":item["threadRef"],
                    "disposition":item["disposition"],"reason":"B08_THREAD_AUTHORITY_UNBOUND"
                }));
            }
            allowed
        });
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
    json!({"factLabels":by_label,"newThreads":new_threads,"unboundThreadUpdates":unbound_thread_updates})
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

/// VD-1d (real run round 15): the tasks among `states` that a chapter other
/// than `chapter_id` already satisfied. Progress is one record per task, so
/// a later chapter reporting the same completion would move it to itself.
fn satisfied_elsewhere(
    states: &[crate::repositories::narrative::TaskStateRecord],
    chapter_id: &str,
) -> BTreeSet<String> {
    states
        .iter()
        .filter(|state| {
            state.value["status"] == "SATISFIED" && state.chapter_id.as_deref() != Some(chapter_id)
        })
        .map(|state| state.state_id.clone())
        .collect()
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
    let mut pending_relations = BTreeMap::<String, (String, String)>::new();
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
        validate_relation_identity(connection, project_id, candidate, &mut pending_relations)?;
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
    let reported = audit
        .task_progress_candidates
        .iter()
        .map(|candidate| candidate.task_id.as_str())
        .collect::<Vec<_>>();
    let done = satisfied_elsewhere(
        &crate::repositories::narrative::NarrativeRepository::task_states_by_ids_complete(
            connection, project_id, &reported,
        )
        .map_err(|error| error.to_string())?,
        chapter_id,
    );
    // A satisfied task can still be reported blocked or invalidated later.
    audit.task_progress_candidates.retain(|candidate| {
        !done.contains(&candidate.task_id)
            || matches!(candidate.status.as_str(), "BLOCKED" | "INVALIDATED")
    });
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
                && !crate::repositories::narrative::NarrativeRepository::is_open_candidate_thread(
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
    let dependencies = crate::planning::task_prerequisite_ids(&candidate.task_id, &route)
        .map_err(|_| "B08_TASK_COMPLETION_PREDICATE_UNSATISFIED")?
        .into_iter()
        .collect::<Vec<_>>();
    let states = crate::planning::route_task_states(connection, project_id, &volume_id, &route)?;
    if dependencies.iter().any(|dependency| {
        !states.iter().any(|state| {
            state.state_id == *dependency && crate::planning::dependency_met(&state.value)
        })
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
    fn local_expansion_preserves_existing_prose_and_cannot_replace_the_ending() {
        let original = "林满把账本放平。掌柜挑了挑眉。\n\n铜铃在门外响了。";
        let anchors = chapter_expansion_anchors(original);
        let valid = json!([{"afterSentence":0,"text":"他翻到卷角压住的那页。"}]);
        let patched = insert_chapter_passages(original, &valid, &anchors).unwrap();
        assert_eq!(
            patched,
            "林满把账本放平。\n\n他翻到卷角压住的那页。\n\n掌柜挑了挑眉。\n\n铜铃在门外响了。"
        );
        assert!(insert_chapter_passages(
            original,
            &json!([{"afterSentence":2,"text":"改写收尾。"}]),
            &anchors
        )
        .is_none());
        assert!(insert_chapter_passages(
            original,
            &json!([
                {"afterSentence":0,"text":"第一段。"},{"afterSentence":0,"text":"第二段。"}
            ]),
            &anchors
        )
        .is_none());
    }

    #[test]
    fn local_expansion_cannot_split_a_quote_at_an_ellipsis() {
        let original = "她揭开便签纸，露出铅笔印：‘……修族谱’。沈砚把便签收起来。";
        let anchors = chapter_expansion_anchors(original);
        let inside = json!([{"afterSentence":0,"text":"他推开屋门。"}]);
        assert!(insert_chapter_passages(original, &inside, &anchors).is_none());
        let outside = json!([{"afterSentence":1,"text":"他推开屋门。"}]);
        let patched = insert_chapter_passages(original, &outside, &anchors).unwrap();
        assert!(patched.contains("‘……修族谱’。\n\n他推开屋门。"));
    }

    #[test]
    fn round63_a_discovery_and_an_unmet_control_goal_can_both_be_recorded() {
        let observation = "整条纹路亮了一下，暗了，又亮，又暗——三回。";
        let discovery = "半枚青铜模具，表面刻着一个壬字。";
        let body = format!("{observation}\n{discovery}");
        let event = json!({"firstSentence":1,"outcomeSentence":1,"eventType":"DISCOVERY"});
        assert!(select_main_event_evidence(&body, &event).unwrap().is_some());
        let goal = "确认蚀骨粉纹路与松脂纽扣同源且具可控共振特性";
        let review = json!({"status":"PARTIAL","missing":"可控共振特性",
            "citations":[{"sentence":0,"quote":observation}]});
        let bound = bind_chapter_goal_review(goal, &body, &review).unwrap();
        assert_eq!(bound["status"], "PARTIAL");
        assert_eq!(bound["missing"], "可控共振特性");
        assert_eq!(bound["evidence"][0]["quote"], observation);
    }

    #[test]
    fn a_goal_review_requires_its_own_exact_current_chapter_evidence() {
        let body = "他远远望见了门房。林舟亲手把信交给了门口的守卫。";
        let goal = "林舟把信交给守卫";
        let mut review = json!({"status":"SUPPORTED","missing":null,"citations":[
            {"sentence":0,"quote":"林舟亲手把信交给了门口的守卫。"}
        ]});
        assert!(bind_chapter_goal_review(goal, body, &review).is_none());
        review["citations"][0]["sentence"] = json!(1);
        assert!(bind_chapter_goal_review(goal, body, &review).is_some());
        review["status"] = json!("PARTIAL");
        review["missing"] = json!("约定明天汇合");
        assert!(bind_chapter_goal_review(goal, body, &review).is_none());
    }

    #[test]
    fn a_malformed_goal_review_leaves_a_valid_event_selection_available() {
        let body = "匣里藏着半枚刻有暗记的铜印。";
        let event = json!({
            "firstSentence":0,"outcomeSentence":0,"eventType":"DISCOVERY","goalReview":"错误的回包形状"
        });
        assert!(crate::model_output::check("SELECT_MAIN_EVENT", &event).is_ok());
        assert!(select_main_event_evidence(body, &event).unwrap().is_some());
        assert!(bind_chapter_goal_review("查清铜印来自哪里", body, &event["goalReview"]).is_none());
    }

    #[test]
    fn a_saved_body_does_not_make_a_budget_deferred_review_reusable() {
        let mut checkpoint = ChapterProseCheckpoint {
            input_digest: "input".into(),
            body: "林舟把账页摊在案头。".into(),
            body_digest: "body".into(),
            renderer: Value::Null,
            prose_run_ids: vec![],
            craft_skills: json!({"continuity":{
                "reviewVersion": CONTINUITY_REVIEW_VERSION,
                "finalCheck": {"bodyDigest": "body", "modelRunId":"review:original"},
                "recalledSources": [],
                "errors": ["QC1_CONFIRMATION_EVIDENCE_DEFERRED_FOR_BUDGET"]
            }}),
        };
        assert!(reusable_continuity_sources(&checkpoint, true).is_none());
        checkpoint.craft_skills["continuity"]["errors"] = json!([]);
        assert!(reusable_continuity_sources(&checkpoint, true).is_some());
    }

    #[test]
    fn prose_review_is_reusable_only_for_the_checked_body() {
        let checkpoint = ChapterProseCheckpoint {
            input_digest: "input".into(),
            body: "林舟把账页摊在案头。".into(),
            body_digest: "changed".into(),
            renderer: Value::Null,
            prose_run_ids: vec![],
            craft_skills: json!({"continuity":{
                "reviewVersion": CONTINUITY_REVIEW_VERSION,
                "finalCheck": {"bodyDigest": "original", "modelRunId":"review:original"},
                "recalledSources": [],
                "errors": []
            }}),
        };
        assert!(reusable_continuity_sources(&checkpoint, true).is_none());
    }

    #[test]
    fn edits_that_make_the_chapter_too_long_are_left_out_one_by_one() {
        let body = "甲句在这里。乙句在这里。丙句在这里。";
        let rewrite = json!({"edits":[
            {"quote":"甲句在这里。","replacement":"甲句改好了。"},
            {"quote":"乙句在这里。","replacement":"乙句改得太长太长太长太长太长了。"},
            {"quote":"丙句在这里。","replacement":"丙句改好了。"}
        ]});
        let fitted = apply_edits_that_fit(body, &rewrite, &[], |text| text.chars().count() <= 20);
        assert_eq!(fitted.body, "甲句改好了。乙句在这里。丙句改好了。");
        assert_eq!(fitted.applied, 2);
    }

    #[test]
    fn an_edit_left_out_is_applied_once_the_others_made_room_and_only_once() {
        let body = "甲句在这里。乙句写得又长又啰嗦。丙句。";
        let rewrite = json!({"edits":[
            {"quote":"甲句在这里。","replacement":"甲句在这里。补上一句。"},
            {"quote":"乙句写得又长又啰嗦。","replacement":"乙句短。"},
            {"quote":"丙句。","replacement":"丙句改得太长太长太长太长太长太长了。"}
        ]});
        let fitted = apply_edits_that_fit(body, &rewrite, &[], |text| text.chars().count() <= 20);
        assert_eq!(fitted.body, "甲句在这里。补上一句。乙句短。丙句。");
        assert_eq!(fitted.applied, 2);
    }

    #[test]
    fn round80_a_replay_of_the_opening_is_looked_for_beyond_the_latest_chapters() {
        let recent = numbered_prior_chapter(&json!(7), &json!("chap:7"), "沈砚回到家。");
        let opening = [
            (
                3,
                "他扒开浮土，基座露出来。他用铅笔把铭牌拓下来。".to_string(),
            ),
            (7, "不该重复。".to_string()),
        ];
        let chapters = with_opening_chapters(&[recent], &opening, Some(8));
        assert_eq!(
            chapters
                .iter()
                .map(|c| c["chapterNo"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            [7, 3]
        );
        assert_eq!(
            chapters[1]["sentences"][1]["text"],
            "他用铅笔把铭牌拓下来。"
        );
        // Rewriting an opening chapter compares it with no later one.
        assert!(with_opening_chapters(&[], &opening[..1], Some(2)).is_empty());
        assert!(with_opening_chapters(&[], &opening[..1], Some(3)).is_empty());
    }

    #[test]
    fn a_replan_is_not_given_back_the_goal_or_ending_it_was_rejected_for() {
        let plan = json!({"chapterGoal":"再试一次端口","chapterRole":"普通",
            "craft":{"endHook":"在B-4下划一道竖线","scenes":[]},"horizonPreview":[]});
        let hook = previous_plan(
            &plan,
            &json!({"problems":["章末钩子和第 4 章的太接近（…）"]}),
        )
        .unwrap();
        assert!(hook["craft"].get("endHook").is_none());
        assert_eq!(hook["chapterGoal"], "再试一次端口");
        assert!(previous_plan(
            &plan,
            &json!({"problems":["本章目标和第 4 章的太接近（…）"]})
        )
        .is_none());
        assert!(previous_plan(&plan, &json!({"rejectedFor":"B08_PLAN_GOAL_REPEATED"})).is_none());
        let kept = previous_plan(&plan, &json!({"problems":["第 1 场没有人物"]})).unwrap();
        assert_eq!(kept["craft"], plan["craft"]);
        for problem in [
            "本章的地点（修复室）上一章都去过，至少换一个新地点",
            "第 1 场写了 3 件事：一场只写一件事（发生什么、谁和谁），动作和细节留给写手",
        ] {
            let replanned = previous_plan(&plan, &json!({"problems":[problem]})).unwrap();
            assert!(replanned["craft"].get("scenes").is_none(), "{problem}");
            assert_eq!(replanned["craft"]["endHook"], plan["craft"]["endHook"]);
        }
    }

    /// Real run 2026-11-25: chapters planned in three tries had used 6 of 16
    /// runs when the rewrite came up, and it never ran.
    #[test]
    fn a_chapter_planned_in_three_tries_still_gets_its_rewrite() {
        for (stage_step, opening) in [(true, false), (false, true)] {
            let reserved = ReservedRuns::new(stage_step, opening);
            assert!(fits_before_final_evidence(6, 16, reserved.polish, 2));
            assert!(!fits_before_final_evidence(9, 16, reserved.polish, 2));
        }
    }

    #[test]
    fn a_step_keeps_one_run_for_every_required_step_after_it() {
        let long_form = ReservedRuns::new(true, false);
        assert_eq!(
            (
                long_form.event,
                long_form.outcome,
                long_form.submission,
                long_form.polish
            ),
            (1, 2, 3, 4)
        );
        let opening = ReservedRuns::new(false, true);
        assert_eq!(
            (
                opening.event,
                opening.outcome,
                opening.submission,
                opening.polish
            ),
            (1, 1, 3, 4)
        );
    }

    #[test]
    fn round59_stage_verification_rebinds_the_exact_result_and_checks_the_batch_constraint() {
        let objective = "三张不同批次租约附件全部暴露出朱砂补笔痕迹";
        let mut selection = json!({
            "firstSentence":3,"outcomeSentence":120,
            "condition":1,"actionQuote":"显出了朱砂补笔痕迹",
            "outcomeQuote":"三张不同批次的租约附件上都显出了朱砂补笔痕迹"
        });
        assert!(grounded_stage_step_evidence(
            "三张不同批次的租约附件上都显出了朱砂补笔痕迹。",
            objective,
            &selection,
        )
        .is_some());
        selection["outcomeQuote"] = json!("三张同一批次的租约附件上都显出了朱砂补笔痕迹");
        assert!(grounded_stage_step_evidence(
            "三张同一批次的租约附件上都显出了朱砂补笔痕迹。",
            objective,
            &selection,
        )
        .is_none());
    }
    /// Owner question (2026-11-03): would binding a relation whenever its
    /// sentence names both people bring new problems? These pin down what
    /// the evidence check does today for relations.
    fn relation_audit(object_id: &str, state: &str, sentence: u64) -> Value {
        json!({"canonCandidates":[{"subjectId":"陆照","predicate":"relationship",
            "objectJson":{"objectId":object_id,"state":state},"evidence":{"sentence":sentence}}]})
    }

    #[test]
    fn a_relation_cited_on_a_sentence_that_names_neither_person_is_unbound() {
        // Real run round 27: "他按了按" names no one; binding by both names
        // would not have kept it either.
        let body = "周砚舟把拓片推过桌面。纸边硌着胳膊，他按了按。阿沅在门口等着。";
        let mut audit = relation_audit("周砚舟", "信任", 1);
        assert_eq!(
            bind_cited_evidence(&mut audit, body).len(),
            0,
            "the sentence before names him"
        );
        let body = "雨下了一夜。纸边硌着胳膊，他按了按。药炉还没熄。";
        let mut audit = relation_audit("周砚舟", "信任", 1);
        assert_eq!(bind_cited_evidence(&mut audit, body).len(), 1);
    }

    #[test]
    fn a_relation_cited_on_a_sentence_that_names_the_other_person_is_kept_whatever_it_says() {
        // The counter-example: the sentence names both people but states no
        // trust at all. Today's check already keeps it, as it only asks that
        // the other person be named there or next to it; requiring both names
        // would not catch it either. Whether the relation holds is the
        // audit's judgment, not the evidence check's.
        let body = "雨下了一夜。陆照和阿沅擦肩而过，谁也没看谁。药炉还没熄。";
        let mut audit = relation_audit("阿沅", "信任", 1);
        assert_eq!(bind_cited_evidence(&mut audit, body).len(), 0);
    }

    #[test]
    fn a_thread_without_a_description_is_named_by_its_payoff_before_its_quote() {
        // Real run round 29: foreshadows read "柳莺的目光已经钉在他手上……".
        let candidate =
            |json: Value| -> ContinuityCandidate { serde_json::from_value(json).unwrap() };
        let evidence = json!({"start":0,"end":4,"quote":"他按了按"});
        assert_eq!(
            thread_description(&candidate(
                json!({"threadRef":"t","disposition":"ADVANCE","description":"暗格里藏着什么","fulfillmentObject":"暗格","evidence":evidence})
            )),
            "暗格里藏着什么"
        );
        assert_eq!(
            thread_description(&candidate(
                json!({"threadRef":"t","disposition":"ADVANCE","fulfillmentObject":"地窖暗格","evidence":evidence})
            )),
            "待兑现：地窖暗格"
        );
        assert_eq!(
            thread_description(&candidate(
                json!({"threadRef":"t","disposition":"ADVANCE","description":" ","evidence":evidence})
            )),
            "他按了按"
        );
    }

    #[test]
    fn a_new_thread_about_a_key_object_or_a_mystery_with_its_payoff_is_a_foreshadow() {
        // P1 (owner decision 2026-11-03): the audit never marks a planted
        // hint, but marks key objects and mysteries all the time.
        let signals = |json: Value| -> crate::narrative::PromotionSignal {
            serde_json::from_value(json).unwrap()
        };
        let hint = signals(json!({"foreshadowRecoveryResponsibility":true}));
        let key_object = signals(json!({"keyFutureUseObjectOrInformation":true}));
        let mystery = signals(json!({"mysteryOrRevealResponsibility":true}));
        let conflict = signals(json!({"unresolvedMajorConflict":true}));
        assert!(is_foreshadow(hint, None));
        assert!(is_foreshadow(key_object, Some("半张租约")));
        assert!(is_foreshadow(mystery, Some("师父的死因")));
        // Without a payoff, or about something else, it stays a thread.
        assert!(!is_foreshadow(key_object, None));
        assert!(!is_foreshadow(key_object, Some("  ")));
        assert!(!is_foreshadow(conflict, Some("宗门存亡")));
    }

    #[test]
    fn audited_entity_types_come_down_to_five_kinds() {
        for (raw, kind) in [
            ("CHARACTER", "PERSON"),
            ("person", "PERSON"),
            ("ITEM", "OBJECT"),
            ("ARTIFACT", "OBJECT"),
            ("DANLU", "OBJECT"),
            ("ANIMAL", "CREATURE"),
            ("LOCATION", "PLACE"),
            ("SECT", "ORGANIZATION"),
        ] {
            assert_eq!(super::entity_kind(raw), kind, "{raw}");
        }
    }

    #[test]
    fn only_tasks_another_chapter_satisfied_keep_their_chapter() {
        use crate::repositories::narrative::TaskStateRecord;
        let state = |id: &str, status: &str, chapter: &str| TaskStateRecord {
            state_id: id.into(),
            value: serde_json::json!({"status":status}),
            chapter_id: Some(chapter.into()),
        };
        let states = [
            state("task:a", "SATISFIED", "chap:4"),
            state("task:b", "ACTIVE", "chap:4"),
            state("task:c", "SATISFIED", "chap:6"),
        ];
        assert_eq!(
            super::satisfied_elsewhere(&states, "chap:6"),
            ["task:a".to_string()].into_iter().collect()
        );
    }

    #[test]
    fn fx18_configured_provider_enforces_the_chapter_length_range() {
        let paths = crate::project_db::paths::AppPaths::new(std::env::temp_dir());
        assert!(super::ConfiguredProductionGateway::for_desktop(paths).enforces_chapter_length());
    }

    use super::*;

    fn relation_candidate(subject: &str, object: &str, relation: &str) -> CanonCandidate {
        serde_json::from_value(json!({"factId":"fact:1","subjectId":subject,
            "predicate":"relationship","objectJson":{"objectId":object,"relationId":relation},
            "authorityLevel":"C","evidence":{"start":0,"end":1,"quote":"人"}}))
        .unwrap()
    }

    #[test]
    fn an_audit_cannot_assign_an_existing_relation_to_the_opposite_direction() {
        let conn = crate::repositories::narrative::directed_relation_test_db(true);
        let mut pending = BTreeMap::new();
        assert_eq!(
            validate_relation_identity(
                &conn,
                "p",
                &relation_candidate("柳明漪", "陆照", "r"),
                &mut pending
            )
            .unwrap_err(),
            "B08_RELATION_IDENTITY_CONFLICT"
        );
    }

    #[test]
    fn an_audit_cannot_assign_one_new_relation_id_to_two_directed_pairs() {
        let conn = crate::repositories::narrative::directed_relation_test_db(false);
        let mut pending = BTreeMap::new();
        validate_relation_identity(
            &conn,
            "p",
            &relation_candidate("陆照", "柳明漪", "r"),
            &mut pending,
        )
        .unwrap();
        assert_eq!(
            validate_relation_identity(
                &conn,
                "p",
                &relation_candidate("柳明漪", "陆照", "r"),
                &mut pending
            )
            .unwrap_err(),
            "B08_RELATION_IDENTITY_CONFLICT"
        );
    }

    #[test]
    fn an_audit_can_reuse_a_relation_id_for_the_same_directed_pair() {
        let conn = crate::repositories::narrative::directed_relation_test_db(true);
        assert!(validate_relation_identity(
            &conn,
            "p",
            &relation_candidate("陆照", "柳明漪", "r"),
            &mut BTreeMap::new()
        )
        .is_ok());
    }

    #[test]
    fn an_audit_cannot_claim_a_different_task_relation_in_its_object() {
        let conn = crate::repositories::narrative::directed_relation_test_db(false);
        let mut candidate = relation_candidate("陆照", "柳明漪", "relation:other");
        candidate.relation_id = Some("relation:actual".to_string());
        assert_eq!(
            validate_relation_identity(&conn, "p", &candidate, &mut BTreeMap::new()).unwrap_err(),
            "B08_RELATION_IDENTITY_CONFLICT"
        );
    }

    #[test]
    fn a_resolved_mystery_event_uses_the_fact_with_the_same_evidence() {
        let mut unrelated = relation_candidate("陆照", "柳明漪", "r");
        unrelated.fact_id = "fact:unrelated".into();
        unrelated.evidence = EvidenceSpan {
            start: 2,
            end: 3,
            quote: "旁".into(),
            rebound: false,
        };
        let mut matching = relation_candidate("陆照", "柳明漪", "r");
        matching.fact_id = "fact:matching".into();
        let evidence = matching.evidence.clone();
        let (marks, missing) = resolved_mystery_event_marks(
            &[unrelated, matching],
            &[("mystery:1".into(), Some(evidence))],
        );
        assert_eq!(marks["fact:matching"].1["mysteryId"], "mystery:1");
        assert!(!marks.contains_key("fact:unrelated"));
        assert!(missing.is_empty());
    }

    #[test]
    fn a_resolved_mystery_without_matching_fact_is_not_recorded_as_an_unrelated_event() {
        let unrelated = relation_candidate("陆照", "柳明漪", "r");
        let evidence = EvidenceSpan {
            start: 2,
            end: 3,
            quote: "旁".into(),
            rebound: false,
        };
        let (marks, missing) =
            resolved_mystery_event_marks(&[unrelated], &[("mystery:1".into(), Some(evidence))]);
        assert!(marks.is_empty());
        assert_eq!(missing, vec!["mystery:1"]);
    }

    #[test]
    fn a_prelabelled_mystery_event_keeps_its_mystery_id() {
        let mut candidate = relation_candidate("陆照", "柳明漪", "r");
        candidate.event_type = Some("MYSTERY_RESOLVED".into());
        let evidence = candidate.evidence.clone();
        let (marks, missing) =
            resolved_mystery_event_marks(&[candidate], &[("mystery:1".into(), Some(evidence))]);
        assert_eq!(marks["fact:1"].1["mysteryId"], "mystery:1");
        assert!(missing.is_empty());
    }

    #[test]
    fn a_model_cannot_cite_a_reversed_or_nonexistent_sentence() {
        let body = "陆照打开绳结。盒里露出一封密信。";
        for (action, outcome) in [(1, 0), (0, 9)] {
            let selection = json!({"eventType":"REVEAL", "actionSentence":action,
                "outcomeSentence":outcome,"outcomeQuote":"盒里露出一封密信"});
            assert!(select_main_event_evidence(body, &selection).is_err());
        }
    }

    #[test]
    fn no_supported_event_selection_leaves_no_event() {
        let selection = json!({"actionSentence":null,"outcomeSentence":null,
            "eventType":null,"outcomeQuote":null});
        assert!(select_main_event_evidence("天亮了。", &selection)
            .unwrap()
            .is_none());
    }

    #[test]
    fn round35_event_selection_rejects_destroyed_writing_as_acquisition() {
        let body = "纸面上的字全毁了，墨迹被火舔成一片黑。";
        let selection = json!({"actionSentence":0,"outcomeSentence":0,"eventType":"GAIN",
            "outcomeQuote":"纸面上的字全毁了"});
        assert!(select_main_event_evidence(body, &selection).is_err());
    }

    #[test]
    fn round35_event_selection_rejects_a_gesture_without_a_result() {
        let body = "苏砚的银镊往前送了半分。";
        let selection = json!({"actionSentence":0,"outcomeSentence":0,"eventType":"GAIN",
            "outcomeQuote":body});
        assert!(select_main_event_evidence(body, &selection).is_err());
    }

    #[test]
    fn a_gain_can_use_a_cited_acquisition() {
        let body = "苏砚接过钥匙，把铜环扣紧。";
        let selection = json!({"actionSentence":0,"outcomeSentence":0,"eventType":"GAIN",
            "outcomeQuote":"苏砚接过钥匙"});
        assert_eq!(
            select_main_event_evidence(body, &selection)
                .unwrap()
                .unwrap()
                .event_type,
            "GAIN"
        );
    }

    #[test]
    fn a_main_event_can_be_evidenced_by_the_chapter_without_a_matching_fact() {
        let body =
            "陆照抹开炭粉，把羊皮纸翻了过来。背面露出父亲亲手绘制的地宫路线。他把纸藏进袖里。";
        let selected = select_main_event_evidence(
            body,
            &json!({
                "eventType":"REVEAL","actionSentence":0,"outcomeSentence":1,
                "outcomeQuote":"背面露出父亲亲手绘制的地宫路线"
            }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.event_type, "REVEAL");
        assert_eq!(
            selected.evidence.quote,
            "陆照抹开炭粉，把羊皮纸翻了过来。背面露出父亲亲手绘制的地宫路线。"
        );
        assert_eq!(selected.evidence.start, 0);
    }

    #[test]
    fn round37_reported_punctuation_is_resolved_to_the_exact_outcome() {
        let body = "陆照翻过纸边。纸边底下有个微凸的圆痕，指甲一刮，涩中带黏——松脂。";
        let selected = select_main_event_evidence(
            body,
            &json!({
                "eventType":"DISCOVERY","actionSentence":0,"outcomeSentence":1,
                "outcomeQuote":"纸边底下有个微凸的圆痕,指甲一刮,涩中带黏——松脂."
            }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            selected.outcome_quote,
            "纸边底下有个微凸的圆痕，指甲一刮，涩中带黏——松脂。"
        );
    }

    #[test]
    fn round43_a_result_can_span_two_adjacent_numbered_sentences() {
        let body = "陆照把两样东西并在一起。师父的断玉符。他手里那半截，和砖下这半截，原本是一块。";
        let selected = select_main_event_evidence(
            body,
            &json!({
                "eventType":"DISCOVERY","actionSentence":0,"outcomeSentence":2,
                "outcomeQuote":"师父的断玉符。他手里那半截，和砖下这半截，原本是一块。"
            }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.evidence.quote, body);
        assert_eq!(
            selected.outcome_quote,
            "师父的断玉符。他手里那半截，和砖下这半截，原本是一块。"
        );
    }

    #[test]
    fn round43_a_changed_expression_does_not_prove_a_discovery() {
        let body = "他问柳明微是否碰过断玉符。柳明微脸色变了。";
        let result = select_main_event_evidence(
            body,
            &json!({
                "eventType":"DISCOVERY","actionSentence":0,"outcomeSentence":1,
                "outcomeQuote":"柳明微脸色变了。"
            }),
        );
        assert_eq!(result.err().as_deref(), Some("EVENT_RESULT_IS_REACTION"));
    }

    #[test]
    fn round46_a_handoff_is_bound_without_attaching_the_wrong_numbered_action() {
        let passage =
            "老陶从腰后摸出一块青石片，往他手心里一塞。\n\n“碑基底下垫着的。”他说。石片入手冰凉。";
        let body = format!("他撑开窗。院里下雨。{passage}");
        let selected = select_main_event_evidence(
            &body,
            &json!({
                "eventType":"GAIN", "actionSentence":0, "outcomeSentence":1,
                "outcomeQuote":passage
            }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.evidence.quote, passage);
        assert_eq!(
            selected.evidence.start,
            "他撑开窗。院里下雨。".chars().count()
        );
    }

    #[test]
    fn round47_a_question_about_a_key_is_not_a_discovery() {
        let body = "陆照回过头。老陶问：“那把钥匙呢？”";
        let selected = select_main_event_evidence(
            body,
            &json!({
                "eventType":"DISCOVERY","actionSentence":0,"outcomeSentence":1,
                "outcomeQuote":"“那把钥匙呢？”"
            }),
        );
        assert_eq!(selected.err().as_deref(), Some("EVENT_RESULT_IS_REACTION"));
    }

    #[test]
    fn round51_a_forecast_is_not_a_discovery() {
        let body = "“嗯。”\n\n“你就嗯？”\n\n“明天他们还会来。”";
        let selected = select_main_event_evidence(
            body,
            &json!({
                "eventType":"DISCOVERY","firstSentence":0,"outcomeSentence":2,
            }),
        );
        assert_eq!(selected.err().as_deref(), Some("EVENT_RESULT_IS_FORECAST"));
    }

    #[test]
    fn round52_a_guess_without_a_discovered_fact_is_not_a_main_event() {
        let body = "“闻出来几成？”\n\n“七成。”陆照顿了顿，“剩下三成，小的不敢猜。”";
        let spans = crate::chapter_craft::sentence_spans(body);
        let outcome = spans.len() - 1;
        let selected = select_main_event_evidence(
            body,
            &json!({
                "eventType":"DISCOVERY","firstSentence":0,"outcomeSentence":outcome,
            }),
        );
        assert_eq!(selected.err().as_deref(), Some("EVENT_RESULT_IS_REACTION"));
    }

    #[test]
    fn round62_acquiring_the_button_cannot_complete_the_comparison() {
        let objective = "陆照用松脂纽扣背面四道刻痕比对蚀骨粉纹路，确认二者同源";
        let quote = "陆照拿到一枚松脂纽扣，背面四道刻痕清清楚楚。";
        let body = format!("{quote}蚀骨粉还没有比对，同源也尚未证实。");
        let candidate = json!({"firstSentence":0,"outcomeSentence":0,
            "condition":1,"actionQuote":"拿到一枚松脂纽扣",
            "outcomeQuote":quote});
        assert!(grounded_stage_step_evidence(&body, objective, &candidate).is_none());
    }

    #[test]
    fn round62_approved_aliases_can_support_an_actual_comparison() {
        let objective = "陆照用松脂纽扣背面四道刻痕比对蚀骨粉纹路，确认二者同源";
        let quote = "他把扣饰的四划压到药灰印上，两条线丝毫不差，证实都出自同一处作坊。";
        let body = format!("碗里的药灰正是蚀骨粉。树脂扣饰是他昨日找到的松脂纽扣。{quote}");
        let candidate = json!({"firstSentence":2,"outcomeSentence":2,
            "condition":1,"actionQuote":"把扣饰的四划压到药灰印上",
            "outcomeQuote":quote});
        assert!(grounded_stage_step_evidence(&body, objective, &candidate).is_some());
    }

    #[test]
    fn round75_a_comparison_told_without_the_listed_words_counts_on_a_second_look() {
        // Real run round 6: "一百三十七", "一百三十二", "差五像素" is a
        // comparison, and the step stayed open for seven chapters.
        let stage = PlannedStageStep {
            task_id: "task:required:goal:vol:1:step:1:1:1".into(),
            objective: "沈砚比对三片碎纸残渣与内网扫描件，发现圆珠笔压痕无法被矢量重描".into(),
            progress: None,
            recent_unverified_attempts: Vec::new(),
        };
        let body = "三片碎纸被镊子摆成一线。“一百三十七。”小陈说。“扫描件呢？”“一百三十二。”沈砚把放大镜搁下。“停一下，拖出的墨就多五像素宽。可你给我的图里没有。”";
        let citation = json!({"stageStepProgress":{"evidenceParts":[{
            "condition":1,"firstSentence":0,"outcomeSentence":6,
            "actionQuote":"三片碎纸被镊子摆成一线。",
            "outcomeQuote":"“停一下，拖出的墨就多五像素宽。可你给我的图里没有。”"
        }]}});
        let selection = &citation["stageStepProgress"];
        let look = |second_look| {
            evaluate_stage_step(
                &stage,
                body,
                &[],
                &[],
                selection,
                "chapter:four",
                "run:look",
                second_look,
            )
        };
        assert!(look(false).is_none());
        assert_eq!(look(true).unwrap().status(), "SATISFIED");
    }

    #[test]
    fn round75_a_second_look_still_refuses_a_refuted_or_unfinished_result() {
        for (goal, outcome) in [
            (
                "陆照比对两枚纽扣的纹路，确认二者同源",
                "纹路对不上，二者并非同源。",
            ),
            (
                "三张不同批次租约附件全部暴露出朱砂补笔痕迹",
                "三张同一批次的附件上都有朱砂补笔痕迹。",
            ),
            ("陆照比对两枚纽扣的纹路", "他还没有比对那两枚纽扣的纹路。"),
        ] {
            let selection = json!({"condition":1,"firstSentence":0,"outcomeSentence":0,
                "actionQuote":outcome,"outcomeQuote":outcome});
            assert!(
                grounded_stage_step_parts(outcome, goal, &selection, true).is_none(),
                "{goal}"
            );
        }
    }

    #[test]
    fn round75_a_citation_that_names_its_own_chapter_is_read_from_its_own_text() {
        let mut selection = json!({"evidenceParts":[
            {"condition":1,"sourceChapter":8},{"condition":1,"sourceChapter":6}]});
        drop_own_source_chapter(&mut selection, 8);
        assert!(selection["evidenceParts"][0].get("sourceChapter").is_none());
        assert_eq!(selection["evidenceParts"][1]["sourceChapter"], 6);
        let mut single = json!({"condition":1,"sourceChapter":8});
        drop_own_source_chapter(&mut single, 8);
        assert!(single.get("sourceChapter").is_none());
    }

    #[test]
    fn a_comparison_that_proves_different_origins_cannot_complete_a_common_origin_goal() {
        let objective = "陆照比对松脂纽扣和蚀骨粉的纹路，确认二者同源";
        let body = "陆照比对了纽扣背面的刻痕和蚀骨粉纹路，确认二者并非同源。";
        let candidate = json!({"firstSentence":0,"outcomeSentence":0,
            "condition":1,"actionQuote":"比对了纽扣背面的刻痕和蚀骨粉纹路",
            "outcomeQuote":"确认二者并非同源"});
        assert!(grounded_stage_step_evidence(body, objective, &candidate).is_none());
    }

    #[test]
    fn round63_comparison_and_origin_can_be_cited_at_separate_positions() {
        let objective = "陆照用松脂纽扣背面四道刻痕比对蚀骨粉纹路，确认二者同源";
        let action = "他把纽扣贴在筒壁上，让两组纹路并排对。";
        let outcome = "筒子里这撮，和扣子上四道，一个来处。";
        let body = format!(
            "{action}两组纹路合上了。炉里的火星跳了两下。他翻看竹筒内壁。陈伯进了丹房。{outcome}"
        );
        let candidate = json!({"firstSentence":0,"outcomeSentence":5,
            "condition":1,"actionQuote":action,"outcomeQuote":outcome});
        let proofs = grounded_stage_step_evidence(&body, objective, &candidate).unwrap();
        assert_eq!(proofs[0].action_evidence.as_ref().unwrap().quote, action);
        assert_eq!(proofs[0].evidence.quote, outcome);
    }

    #[test]
    fn round63_recovered_button_comparison_is_bound_to_the_actual_source_body() {
        let action = "他把纽扣贴在筒壁上，让两组纹路并排对。";
        let outcome = "筒子里这撮，和扣子上四道，一个来处。";
        let earlier = format!(
            "{action}两组纹路合上了。炉里的火星跳了两下。他翻看竹筒内壁。陈伯进了丹房。{outcome}"
        );
        let current = "模具背面那四道凿纹，他拿纽扣又对了一遍。对上了。";
        let stage = PlannedStageStep {
            task_id: "task:required:goal:vol:1:step:1:1:1".into(),
            objective: "陆照用松脂纽扣背面四道刻痕比对蚀骨粉纹路，确认二者同源".into(),
            progress: None,
            recent_unverified_attempts: Vec::new(),
        };
        let history = [StageStepHistory {
            chapter_no: 4,
            chapter_id: "chapter:four".into(),
            body: earlier.clone(),
            final_ref: ArtifactRef {
                artifact_id: "final:four".into(),
                version: 1,
                payload_hash: "a".repeat(64),
            },
        }];
        let mut candidate = json!({"sourceChapter":4,"firstSentence":0,"outcomeSentence":5,
            "condition":1,"actionQuote":action,"outcomeQuote":outcome});
        let proof = evaluate_stage_step(
            &stage,
            current,
            &[(4, &earlier)],
            &history,
            &candidate,
            "chapter:five",
            "run:recover",
            false,
        )
        .unwrap();
        assert_eq!(proof.status(), "SATISFIED");
        assert!(!proof.has_current_evidence());
        let source = proof.progress.evidence_parts[0].source.as_ref().unwrap();
        assert_eq!(source.final_ref, history[0].final_ref);
        assert_eq!(
            source.verification_chapter_id.as_deref(),
            Some("chapter:five")
        );
        candidate["sourceChapter"] = json!(3);
        assert!(evaluate_stage_step(
            &stage,
            current,
            &[(4, &earlier)],
            &history,
            &candidate,
            "chapter:five",
            "run:wrong-source",
            false,
        )
        .is_none());
    }

    #[test]
    fn a_stage_step_needs_new_evidence_after_partial_progress() {
        let mut stage = PlannedStageStep {
            task_id: "task:required:goal:vol:1:step:1:1:1".into(),
            objective: "找到线人并且取得账本缺页".into(),
            progress: None,
            recent_unverified_attempts: Vec::new(),
        };
        let body = "林舟找到了线人周娘子。";
        let selection = json!({"firstSentence":0,"outcomeSentence":0,
            "condition":1,"actionQuote":"找到了线人","outcomeQuote":body});
        let first = evaluate_stage_step(
            &stage,
            body,
            &[],
            &[],
            &selection,
            "chapter:test",
            "run:first",
            false,
        )
        .unwrap();
        assert_eq!(first.status(), "ACTIVE");
        stage.progress = Some(first.progress);
        assert!(evaluate_stage_step(
            &stage,
            body,
            &[],
            &[],
            &selection,
            "chapter:test",
            "run:repeat",
            false,
        )
        .is_none());
    }

    #[test]
    fn an_unrecorded_result_in_an_earlier_chapter_cannot_advance_the_current_chapter() {
        let stage = PlannedStageStep {
            task_id: "task:required:goal:vol:1:step:1:1:1".into(),
            objective: "找到账本缺页".into(),
            progress: None,
            recent_unverified_attempts: Vec::new(),
        };
        let body = "周娘子从匣底抽出账本缺页。";
        let earlier = format!("他进了库房。{body}");
        let proof = json!({"firstSentence":0,"outcomeSentence":0,
            "condition":1,"actionQuote":"从匣底抽出","outcomeQuote":body});
        assert!(evaluate_stage_step(
            &stage,
            body,
            &[(2, &earlier)],
            &[],
            &proof,
            "chapter:test",
            "run:repeat",
            false,
        )
        .is_none());
    }

    #[test]
    fn round62_an_overlong_citation_does_not_erase_a_separate_earned_result() {
        let stage = PlannedStageStep {
            task_id: "task:required:goal:vol:1:step:1:1:1".into(),
            objective: "找到线人并且取得账本缺页".into(),
            progress: None,
            recent_unverified_attempts: Vec::new(),
        };
        let valid = "林舟找到了线人周娘子。";
        let body = format!("{valid}林舟未能取得账本缺页。");
        let selection = json!({"evidenceParts":[
            {"firstSentence":0,"outcomeSentence":0,"condition":1,"actionQuote":"找到了线人","outcomeQuote":valid},
            {"firstSentence":1,"outcomeSentence":1,"condition":2,"actionQuote":"取得账本缺页","outcomeQuote":"拉长的引用".repeat(40)}
        ]});
        let checked = evaluate_stage_step(
            &stage,
            &body,
            &[],
            &[],
            &selection,
            "chapter:test",
            "run:chapter5",
            false,
        )
        .unwrap();
        assert_eq!(checked.status(), "ACTIVE");
        assert_eq!(checked.progress.remaining_objectives, ["取得账本缺页"]);
        assert_eq!(checked.progress.evidence_parts.len(), 1);
        assert_eq!(checked.progress.evidence_parts[0].outcome_quote, valid);
    }

    #[test]
    fn round52_a_stage_step_requires_an_outcome_on_the_page() {
        let selection = json!({"firstSentence":0,"outcomeSentence":0});
        assert!(select_stage_step_evidence("“我知道。”", &selection).is_none());
        let body = "陆照指尖压住腕疤，灰线停在锁骨边缘，灼痛一点点退了。";
        assert_eq!(
            select_stage_step_evidence(body, &selection).unwrap().quote,
            body
        );
    }

    #[test]
    fn round57_a_short_rejected_outcome_can_be_excluded_from_the_correction() {
        let selection = json!({"firstSentence":1,"outcomeSentence":1,"eventType":"DISCOVERY"});
        let body = "谢砚舟签下暂缓执行条款，给丹房二十日查证。\n\n不见了。";
        let error = select_main_event_evidence(body, &selection).err().unwrap();
        assert_eq!(rejected_numbered_event_outcome(&selection, &error), Some(1));
        let corrected =
            json!({"firstSentence":0,"outcomeSentence":0,"eventType":"SITUATION_CHANGE"});
        assert!(select_main_event_evidence(body, &corrected)
            .unwrap()
            .is_some());
    }

    #[test]
    fn round56_a_sighting_does_not_prove_that_the_stage_action_was_done() {
        let objective = "阿沅趁柳昭仪离殿时偷换其药囊内三粒普通草籽为金纹种，指尖被芽尖刺破出血";
        let unrelated = "阿沅追上来半步，低声说柳昭仪的袖口暗了一下。陆照停步。";
        let mut selected = json!({
            "firstSentence":0,"outcomeSentence":1,
            "condition":1,"actionQuote":"偷换其药囊内三粒普通草籽为金纹种",
            "outcomeQuote":"柳昭仪的袖口暗了一下"
        });
        assert!(grounded_stage_step_evidence(unrelated, objective, &selected).is_none());
        let completed = "阿沅抖出药囊里的三粒草籽，换进三粒金纹种。芽尖刺破了她的指腹。";
        selected["actionQuote"] = json!("换进三粒金纹种");
        selected["outcomeQuote"] = json!("抖出药囊里的三粒草籽，换进三粒金纹种");
        assert!(grounded_stage_step_evidence(completed, objective, &selected).is_some());
    }

    #[test]
    fn round58_compound_step_requires_broom_and_water_results_even_in_separate_passages() {
        let objective = "陆照连续三日观察阿公扫地时竹帚沾水频率与溪水银灰浓度变化";
        let body = "第三日，溪水比前两日浑，水里的银灰多了两成。村口钟响。薄雾沿石阶散开。陆照把样本封好。阿公这三天扫地时竹帚沾水次数由四次增为五次。";
        let broom = json!({
            "firstSentence":4,"outcomeSentence":4,
            "condition":1,"actionQuote":"竹帚沾水次数由四次增为五次",
            "outcomeQuote":"阿公这三天扫地时竹帚沾水次数由四次增为五次"
        });
        let water = json!({
            "firstSentence":0,"outcomeSentence":0,"condition":2,"actionQuote":"水里的银灰多了两成",
            "outcomeQuote":"溪水比前两日浑，水里的银灰多了两成"
        });
        assert!(grounded_stage_step_evidence(body, objective, &broom).is_none());
        let proofs =
            grounded_stage_step_evidence(body, objective, &json!({"evidenceParts":[broom,water]}))
                .unwrap();
        assert_eq!(proofs.len(), 2);
        assert!(proofs[0].evidence.start > proofs[1].evidence.end);
    }

    #[test]
    fn an_already_found_order_for_tomorrow_can_be_a_discovery() {
        let body = "明天的调令已经压在桌下，上面盖着周掌律的私印。";
        let selected = select_main_event_evidence(
            body,
            &json!({
                "eventType":"DISCOVERY","firstSentence":0,"outcomeSentence":0,
            }),
        );
        assert!(selected.unwrap().is_some());
    }

    #[test]
    fn numbered_main_event_evidence_is_read_verbatim_from_the_final_body() {
        let body = "老陶在院里等了三刻。陆照扒开碎石。\n\n坑底露出半枚印信，边沿刻着失传的符号。";
        let selection = json!({"eventType":"DISCOVERY","firstSentence":1,"outcomeSentence":2});
        let event = select_main_event_evidence(body, &selection)
            .unwrap()
            .unwrap();
        assert_eq!(
            event.evidence.quote,
            "陆照扒开碎石。\n\n坑底露出半枚印信，边沿刻着失传的符号。"
        );
        assert_eq!(event.outcome_quote, event.evidence.quote);
        assert!(select_main_event_evidence(
            body,
            &json!({"eventType":"DISCOVERY","firstSentence":2,"outcomeSentence":1})
        )
        .is_err());
    }

    #[test]
    fn distant_preparation_is_not_included_in_numbered_event_evidence() {
        let outcome = "锈环对着铁门，铁门无声地滑开三寸。门里没有尸气。涌出来的是活水。";
        let body = format!("他低头。先擦干铜环。走到门边。听了一阵。{outcome}");
        let event = select_main_event_evidence(
            &body,
            &json!({
                "eventType":"SITUATION_CHANGE", "firstSentence":0,"outcomeSentence":6
            }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(event.evidence.quote, outcome);
    }

    #[test]
    fn round47_an_identical_chapter_goal_cannot_pass_through_the_soft_plan_fallback() {
        let mut reminders = Vec::new();
        let result = note_if_lenient(true, &mut reminders, Err("B08_PLAN_GOAL_REPEATED".into()));
        assert_eq!(result.unwrap_err(), "B08_PLAN_GOAL_REPEATED");
        assert!(reminders.is_empty());
    }

    #[test]
    fn a_misnumbered_outcome_never_binds_to_an_unrelated_action() {
        let body = "陆照把柴堆好。院墙倒了。他擦窗。纸背露出地宫路线。";
        let selected = select_main_event_evidence(
            body,
            &json!({
                "eventType":"REVEAL","actionSentence":0,"outcomeSentence":1,
                "outcomeQuote":"纸背露出地宫路线。"
            }),
        )
        .unwrap()
        .unwrap();
        assert_eq!(selected.evidence.quote, "纸背露出地宫路线。");
    }

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

    #[test]
    fn qc1_a_chapter_keeps_twelve_facts_its_own_people_first_and_drops_their_knowledge_with_the_rest(
    ) {
        // Acceptance 2026-10-26 (second run): the audit's feedback rerun
        // reported 66 facts and 59 knowledge items for chapter 1.
        let fact = |index: usize, subject: &str| json!({"factId":format!("canon:qc1:{index}"),"subjectId":subject,"predicate":"status","objectJson":{"value":"甲"}});
        let facts = (0..20)
            .map(|index| {
                fact(
                    index,
                    if index % 2 == 0 {
                        "桂花糕"
                    } else {
                        "沈砚"
                    },
                )
            })
            .collect::<Vec<_>>();
        let knowledge = (0..20)
            .map(|index| json!({"knowledgeId":format!("know:qc1:{index}"),"factId":format!("canon:qc1:{index}"),"knowerId":"沈砚"}))
            .collect::<Vec<_>>();
        let mut audit =
            json!({"canonCandidates":facts,"knowledgeCandidates":knowledge,"normalization":{}});
        cap_new_facts(&mut audit, &["沈砚".to_string()]);
        let kept = audit["canonCandidates"].as_array().unwrap();
        assert_eq!(kept.len(), MAX_NEW_FACTS);
        assert_eq!(
            kept.iter()
                .filter(|fact| fact["subjectId"] == "沈砚")
                .count(),
            10
        );
        assert_eq!(
            audit["knowledgeCandidates"].as_array().unwrap().len(),
            MAX_NEW_FACTS
        );
        let held = audit["normalization"]["heldBack"].as_array().unwrap();
        assert_eq!(held.len(), 1, "one summary entry: {held:?}");
        assert_eq!(held[0]["reason"], "QC1_FACTS_OVER_LIMIT");
        assert_eq!(held[0]["item"]["facts"], 8);
        assert_eq!(held[0]["item"]["knowledge"], 8);
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
    fn an_unbound_audit_transition_cannot_create_or_close_a_thread() {
        let mut raw = json!({"continuityCandidates":[
            {"threadRef":null,"disposition":"ADVANCE","description":"误写成已知伏笔进展"},
            {"threadRef":"unknown","disposition":"RESOLVE"},
            {"threadRef":"old-promise","disposition":"RESOLVE"},
            {"disposition":"ADVANCE","description":"丹房钥匙丢了"}
        ]});
        let ids = assign_audit_ids(
            &mut raw,
            "chap:5",
            &BTreeSet::from(["thread:old-promise".into()]),
        );
        assert_eq!(raw["continuityCandidates"].as_array().unwrap().len(), 2);
        assert_eq!(
            raw["continuityCandidates"][0]["threadRef"],
            "thread:old-promise"
        );
        assert_eq!(ids["unboundThreadUpdates"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_null_thread_reference_does_not_invalidate_the_audit_envelope() {
        let raw = json!({
            "qualityFindings":[],"canonCandidates":[{"target":"陈伯"}],"knowledgeCandidates":[],
            "continuityCandidates":[{"threadRef":null,"disposition":"ADVANCE","threadOperation":"TRANSITION"}],
            "taskProgressCandidates":[],"revealedReservationRefs":[],"evidenceRanges":[],
            "auditCompleteness":"COMPLETE_WITH_DELTA","unresolvedHard":false
        });
        assert!(crate::model_output::check("AUDIT_EXTRACT", &raw).is_ok());
        let mut returned = raw.clone();
        let assigned = assign_audit_ids(&mut returned, "chap:5", &BTreeSet::new());
        assert_eq!(
            assigned["unboundThreadUpdates"].as_array().unwrap().len(),
            1
        );
        assert_eq!(returned["continuityCandidates"], json!([]));
        assert_eq!(
            returned["canonCandidates"][0]["target"],
            raw["canonCandidates"][0]["target"]
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
        let (revised, applied) = apply_unique_edits(body, &rewrite, &[]);
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
