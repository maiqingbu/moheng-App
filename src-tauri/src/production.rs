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
    CommitBundle,
};
use crate::hash::sha256_canonical;
use crate::narrative::{PromotionSignal, ThreadPromotionDecision};
use crate::project_db::{ProjectCommandOutcome, ProjectCommandSpec, ProjectHandle, ProjectManager};
use crate::provider::{
    OpenAiCompatibleProvider, ProviderAdapter, ProviderConfig, ProviderError, ProviderErrorKind,
    ProviderRequest, ProviderResponse,
};
use crate::repositories::runtime::{
    DurableJobService, DurableModelRunner, JobSpec, ModelRunOutcome, ModelRunSpec, ModelTransport,
    OsKeychainSecretStore, RuntimeError, SecretStore, TransportFailure, TransportResponse,
    TransportUsage,
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

pub trait ProductionModelGateway: Send + Sync {
    fn provider(&self) -> &str {
        "configured-production-gateway"
    }

    fn model(&self, capability: &str) -> String {
        format!("production:{capability}")
    }

    /// Bounded provider/model cost policy used by the existing Budget
    /// Authority.  A production stage may not silently become an unpriced,
    /// zero-cost model run.
    fn predicted_cost_cents(&self, _capability: &str) -> i64 {
        1
    }

    fn fault_injection(&self) -> Option<&str> {
        None
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
            timeout_ms: 30_000,
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
                timeout_ms: 30_000,
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
        let prompt = input
            .get("prompt")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| input.to_string());
        let request = ProviderRequest {
            capability: capability.into(),
            model_id: config.model_id.clone(),
            prompt,
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
}

impl ProductionModelGateway for ConfiguredProductionGateway {
    fn provider(&self) -> &str {
        &self.config.provider_id
    }

    fn model(&self, _capability: &str) -> String {
        self.config.model_id.clone()
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

    fn invoke_typed_with_profile(
        &self,
        capability: &str,
        input: &Value,
        profile: &ResolvedProviderProfile,
    ) -> Result<ProductionModelResponse, TransportFailure> {
        let config = ProviderConfig {
            provider_id: profile.provider.clone(),
            model_id: profile.model.clone(),
            endpoint: profile.endpoint.clone(),
            enabled: true,
            timeout_ms: profile.timeout_ms,
        };
        self.invoke_provider_with_config(&config, capability, input)
            .map(Self::response_from_provider)
            .map_err(|error| match error.kind {
                ProviderErrorKind::Timeout => TransportFailure::Timeout,
                ProviderErrorKind::UnknownRemoteOutcome => TransportFailure::UnknownRemoteOutcome,
                _ => TransportFailure::Unavailable,
            })
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
        self.invoke_provider(capability, input)
            .map(Self::response_from_provider)
            .map_err(|error| match error.kind {
                ProviderErrorKind::Timeout => TransportFailure::Timeout,
                ProviderErrorKind::UnknownRemoteOutcome => TransportFailure::UnknownRemoteOutcome,
                _ => TransportFailure::Unavailable,
            })
    }
}

pub struct GatewayTransport<'a> {
    gateway: &'a dyn ProductionModelGateway,
    profile: Option<&'a ResolvedProviderProfile>,
    capability: &'a str,
    input: Value,
    raw_persist_failure: bool,
}

impl<'a> GatewayTransport<'a> {
    pub fn new(gateway: &'a dyn ProductionModelGateway, capability: &'a str, input: Value) -> Self {
        Self {
            gateway,
            profile: None,
            capability,
            input,
            raw_persist_failure: false,
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
        }
    }
}

impl ModelTransport for GatewayTransport<'_> {
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
        let response = match self.profile {
            Some(profile) => {
                self.gateway
                    .invoke_typed_with_profile(self.capability, &self.input, profile)?
            }
            None => self.gateway.invoke_typed(self.capability, &self.input)?,
        };
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
    let repaired = format!("{content}\"}}");
    let payload = serde_json::from_str::<Value>(&repaired).ok()?;
    payload
        .get("body")
        .and_then(Value::as_str)
        .filter(|body| !body.trim().is_empty())?;
    Some(payload)
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
    let mut cursor = 0usize;
    let mut first = None;
    let mut last = None;
    for segment in segments {
        let byte_start = body.get(cursor..)?.find(segment)? + cursor;
        let byte_end = byte_start + segment.len();
        if let Some(previous_end) = last {
            if body[previous_end..byte_start].chars().count() > 512 {
                return None;
            }
        } else {
            first = Some(body[..byte_start].chars().count());
        }
        last = Some(byte_end);
        cursor = byte_end;
    }
    Some((first?, body[..last?].chars().count()))
}

fn parse_provider_model_payload_for_capability(
    value: Value,
    capability: &str,
) -> Result<Value, String> {
    match parse_provider_model_payload(value.clone()) {
        Ok(payload) => Ok(payload),
        Err(error) if capability == "RENDER_CHAPTER" => {
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
    let mut transport = GatewayTransport::new(gateway, capability, input);
    DurableModelRunner::prepare(connection, spec, now).map_err(|error| error.to_string())?;
    let mut outcome = match DurableModelRunner::execute(connection, &mut transport, spec, now) {
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
    if let Some(parsed) = outcome.parsed.take() {
        outcome.parsed = Some(
            match parse_provider_model_payload_for_capability(parsed, capability) {
                Ok(payload) => payload,
                Err(error) => {
                    DurableModelRunner::mark_model_payload_parse_failure(
                        connection, spec, &error, now,
                    )
                    .map_err(|runtime_error| runtime_error.to_string())?;
                    return Err(error);
                }
            },
        );
    }
    Ok(outcome)
}

pub fn execute_production_model_with_profile(
    connection: &libsql_rusqlite::Connection,
    gateway: &dyn ProductionModelGateway,
    profile: &ResolvedProviderProfile,
    spec: &ModelRunSpec,
    capability: &str,
    input: Value,
    now: &str,
) -> Result<ModelRunOutcome, String> {
    let mut transport = GatewayTransport::with_profile(gateway, profile, capability, input);
    DurableModelRunner::prepare(connection, spec, now).map_err(|error| error.to_string())?;
    let mut outcome = match DurableModelRunner::execute(connection, &mut transport, spec, now) {
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
    if let Some(parsed) = outcome.parsed.take() {
        outcome.parsed = Some(
            match parse_provider_model_payload_for_capability(parsed, capability) {
                Ok(payload) => payload,
                Err(error) => {
                    DurableModelRunner::mark_model_payload_parse_failure(
                        connection, spec, &error, now,
                    )
                    .map_err(|runtime_error| runtime_error.to_string())?;
                    return Err(error);
                }
            },
        );
    }
    Ok(outcome)
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
    if !matches!(action, "NO_ACTION" | "RECOMPILE_NEXT_CHAPTER") {
        let payload =
            json!({"sourceChapterId":source_chapter_id,"status":"DEFERRED","reason":action});
        DurableJobService::record_reconciliation_action(
            handle, project_id, command_id, action, &payload, now,
        )
        .map_err(|error| error.to_string())?;
        return Ok(payload);
    }

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
    let prepare_spec = ProjectCommandSpec {
        project_id,
        result_project_id: project_id,
        command_id: &prepare_command_id,
        idempotency_key: &format!("{prepare_command_id}:key"),
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
    let current_revision = prepare_receipt.after_revision;
    let authority = json!({"projectId":project_id,"chapterId":chapter_id,"volumeId":volume_id,"executionMode":"LONG_FORM","chapterGoalHint":"根据 Reality t+1 重新评估下一步行动。"});
    let context_command_id = format!("{command_id}:reconciliation:context");
    let context_input = crate::commands::context_input_from_authority(
        handle,
        &authority,
        project_id,
        current_revision,
    )?;
    let context_ref = crate::narrative::commit_context_manifest_owner(
        handle,
        project_id,
        &authority,
        current_revision,
        &context_command_id,
        &format!("{context_command_id}:key"),
        "LONG_FORM",
        now,
    )?
    .1;
    let plan_revision =
        crate::repositories::narrative::NarrativeRepository::project_revision(handle, project_id)
            .map_err(|error| error.to_string())?;
    let plan_digest = crate::narrative::relevant_state_digest(&context_input)?;
    let plan = json!({
        "payloadSchema":"chapter-plan.r1",
        "chapterId":chapter_id,
        "compiledFromProjectRevision":plan_revision,
        "compiledFromStateDigest":plan_digest,
        "contextManifestRef":context_ref,
        "chapterGoal":"根据 Reality t+1 重新评估下一步行动。",
        "activeTaskRefs":context_input.active_task_refs,
        "continuityActions":context_input.continuity_threads.iter().map(|thread| json!({"threadRef":thread.thread_id,"disposition":"DEFER_WITH_BRIDGE","bridge":"reconciliation next plan"})).collect::<Vec<_>>(),
        "requiredStateOutcomes":[],"forbiddenTransitions":[],"knowledgeBoundaries":[],"reservationRefs":[],"sceneExecution":[],
        "horizonPreview":[
            {"slotNo":1,"objective":"根据本章结果重新评估下一步行动。","tentative":true},
            {"slotNo":2,"objective":"保持长期悬念，并为后续冲突留出空间。","tentative":true},
            {"slotNo":3,"objective":"让新的选择产生可验证的叙事后果。","tentative":true}
        ]
    });
    let plan_command_id = format!("{command_id}:reconciliation:plan");
    let plan_receipt = crate::planning::recompile_chapter_plan_owner(
        handle,
        project_id,
        &chapter_id,
        &plan,
        &authority,
        "LONG_FORM",
        &plan_command_id,
        &format!("{plan_command_id}:key"),
        plan_revision,
        now,
        None,
    )?;
    let payload = json!({
        "sourceChapterId":source_chapter_id,
        "nextChapterId":chapter_id,
        "nextChapterNo":next_no,
        "status":"EXECUTED",
        "prepareReceiptId":prepare_receipt.receipt_id,
        "contextManifestRef":context_ref,
        "chapterPlanRefs":plan_receipt.committed_artifact_refs,
        "planRevision":plan_receipt.after_revision
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

fn existing_canon_semantic_keys(authority_context: &Value) -> Value {
    authority_context
        .pointer("/state/facts")
        .and_then(Value::as_array)
        .map(|facts| {
            facts
                .iter()
                .filter_map(|fact| {
                    Some(json!({
                        "subjectId":fact.get("subjectId")?.clone(),
                        "predicate":fact.get("predicate")?.clone()
                    }))
                })
                .collect::<Vec<_>>()
        })
        .map(Value::Array)
        .unwrap_or_else(|| json!([]))
}

fn current_authority_digest(
    connection: &libsql_rusqlite::Connection,
    request: &Value,
    project_id: &str,
    chapter_id: &str,
    volume_id: &str,
    execution_mode: &str,
    revision: i64,
) -> Result<String, String> {
    let input = crate::commands::context_input_from_authority(
        connection,
        &authority_request(request, project_id, chapter_id, volume_id, execution_mode),
        project_id,
        revision,
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
        chapter_id,
        volume_id,
        execution_mode,
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
    let certificate_id = format!("cert:b08:{}", &subject.artifact_id.replace(':', "-"));
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

fn update_stage(
    connection: &libsql_rusqlite::Connection,
    project_id: &str,
    job_id: &str,
    stage: &str,
    now: &str,
) -> Result<(), String> {
    if !STAGES.contains(&stage) {
        return Err(format!("B08_INVALID_STAGE:{stage}"));
    }
    DurableJobService::update_stage(connection, project_id, job_id, stage, now)
        .map_err(|error| error.to_string())
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

fn b10_repair_target(body: &str, repairable_dimensions: &[String]) -> Option<(String, String)> {
    if repairable_dimensions
        .iter()
        .any(|dimension| dimension == "hook")
    {
        return Some(("hook".into(), "门后传来一声轻响。".into()));
    }
    if repairable_dimensions
        .iter()
        .any(|dimension| dimension == "dialogue")
    {
        return Some(("dialogue".into(), "“你还要继续吗？”他问。".into()));
    }
    if repairable_dimensions
        .iter()
        .any(|dimension| dimension == "exposition")
    {
        return Some((
            "exposition".into(),
            "他停了一下，把解释留到行动之后。".into(),
        ));
    }
    let _ = body;
    None
}

/// Execute one formal B08 chapter production run. Any error after a durable
/// job has been created is reflected in that job's FAILED state so a caller
/// may safely submit a retry with explicit lineage.
pub fn run_chapter_production(
    manager: &ProjectManager,
    gateway: &dyn ProductionModelGateway,
    skill_bundle_loader: &crate::creative_skills::SkillBundleLoader,
    request: &Value,
) -> Result<Value, String> {
    let result = run_chapter_production_inner(manager, gateway, skill_bundle_loader, request);
    if let Err(error) = &result {
        let project_id = request.pointer("/projectId").and_then(Value::as_str);
        let command_id = request.pointer("/commandId").and_then(Value::as_str);
        let now = request.pointer("/requestedAt").and_then(Value::as_str);
        if let (Some(project_id), Some(command_id), Some(now)) = (project_id, command_id, now) {
            if let Ok(job_id) = StableIdentityFactory::from_command(StableIdKind::Job, command_id) {
                if let Ok(connection) = manager.open(project_id, "", now) {
                    let failure_code = error
                        .split(':')
                        .next()
                        .filter(|code| !code.trim().is_empty())
                        .unwrap_or("B08_PRODUCTION_FAILED");
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
    skill_bundle_loader: &crate::creative_skills::SkillBundleLoader,
    request: &Value,
) -> Result<Value, String> {
    let project_id = require(request, "/projectId")?;
    let command_id = require(request, "/commandId")?;
    let idempotency_key = require(request, "/idempotencyKey")?;
    let now = require(request, "/requestedAt")?;
    let chapter_id = require(request, "/payload/chapterId")?;
    let execution_mode = request
        .pointer("/payload/executionMode")
        .and_then(Value::as_str)
        .unwrap_or("LONG_FORM");
    if !matches!(execution_mode, "OPENING_BOOTSTRAP" | "LONG_FORM") {
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
    if execution_mode == "OPENING_BOOTSTRAP" {
        let next_opening_chapter =
            crate::repositories::project_entry::next_opening_chapter_id(connection, &project_id)
                .map_err(|error| error.to_string())?;
        if next_opening_chapter.as_deref() != Some(chapter_id.as_str()) {
            return Err("B08_OPENING_ORDER_REQUIRED".into());
        }
    }
    let provider_profile = gateway.resolve_profile()?;
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
    let policy = request
        .pointer("/payload/gatePolicyRef")
        .cloned()
        .or_else(|| {
            crate::repositories::narrative::NarrativeRepository::canonical_project_head_ref(
                connection,
                &project_id,
                "GatePolicy",
            )
            .ok()
            .flatten()
            .and_then(|reference| serde_json::to_value(reference).ok())
        })
        .ok_or_else(|| "B08_GATE_POLICY_REQUIRED".to_string())?;
    let policy_ref: ArtifactRef =
        serde_json::from_value(policy).map_err(|e| format!("B08_GATE_POLICY_INVALID:{e}"))?;
    let gate_policy_payload = load_artifact_payload(connection, &project_id, &policy_ref)?;

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

    update_stage(connection, &project_id, &job_id, "CONTEXT", &now)?;
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
    let taste_profile_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("{command_id}:taste-profile"),
    )?;
    let quality_report_id = StableIdentityFactory::from_command(
        StableIdKind::Artifact,
        &format!("{command_id}:quality-report"),
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
    let render_run_id = StableIdentityFactory::from_command(
        StableIdKind::ModelRun,
        &format!("{command_id}:render"),
    )?;
    let audit_run_id = StableIdentityFactory::from_command(
        StableIdKind::ModelRun,
        &format!("{command_id}:audit"),
    )?;

    let (context_ref, context_payload, plan_ref, plan_payload, authority_context) =
        if execution_mode == "OPENING_BOOTSTRAP" {
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

            update_stage(connection, &project_id, &job_id, "PLAN", &now)?;
            let planning_run_id = StableIdentityFactory::from_command(
                StableIdKind::ModelRun,
                &format!("{command_id}:plan"),
            )?;
            let planning_spec = ModelRunSpec {
                model_run_id: planning_run_id.clone(),
                job_id: Some(job_id.clone()),
                step_id: None,
                provider: provider_profile.provider.clone(),
                model: provider_profile.model.clone(),
                idempotency_key: format!("{idempotency_key}:plan"),
                capability: "PLAN_STRATEGY".into(),
                prompt_ref: None,
                context_manifest_ref: Some(context_ref.artifact_id.clone()),
                budget_id: Some(production_budget_id.clone()),
                predicted_cost_cents: gateway.predicted_cost_cents("PLAN_STRATEGY").max(1),
                attempt_no: 1,
                repair_round: 0,
            };
            let planning_input = json!({
                "projectId":project_id,
                "chapterId":chapter_id,
                "executionMode":execution_mode,
                "compiledFromProjectRevision":current_revision + 1,
                "compiledFromStateDigest":manifest.compiled_from_state_digest,
                "contextManifestRef":context_ref,
                "authorityContext":authority_context.clone(),
                "activeTaskRefs":compiler_input.active_task_refs,
                "continuityThreadRefs":compiler_input.continuity_threads.iter().map(|thread| thread.thread_id.clone()).collect::<Vec<_>>(),
                "chapterGoalHint":request.pointer("/payload/chapterGoalHint").and_then(Value::as_str).filter(|value| !value.trim().is_empty()).unwrap_or("推进当前章节计划，并让主角完成一次可验证的主动选择。")
            });
            let planning_result = execute_production_model_with_profile(
                connection,
                gateway,
                &provider_profile,
                &planning_spec,
                "PLAN_STRATEGY",
                planning_input.clone(),
                &now,
            )?;
            let plan_payload = planning_result
                .parsed
                .ok_or_else(|| "B08_PLANNING_OUTPUT_INVALID".to_string())?;
            let plan_payload = crate::planning::normalize_chapter_plan_model_output(
                plan_payload,
                &compiler_input
                    .continuity_threads
                    .iter()
                    .filter(|thread| matches!(thread.authority_level.as_str(), "A" | "B"))
                    .map(|thread| thread.thread_id.clone())
                    .collect::<Vec<_>>(),
                &planning_input,
            );
            crate::generated::validators::validate_chapter_plan_r1(&plan_payload)
                .map_err(|e| format!("B08_PLANNING_CONTRACT:{e}"))?;
            crate::planning::validate_current_chapter_plan(
                &plan_payload,
                &chapter_id,
                &compiler_input.active_task_refs,
                &compiler_input
                    .continuity_threads
                    .iter()
                    .filter(|thread| matches!(thread.authority_level.as_str(), "A" | "B"))
                    .map(|thread| thread.thread_id.clone())
                    .collect::<Vec<_>>(),
            )?;
            let planning_command_id = format!("{bootstrap_command_id}:planning-owner");
            let planning_receipt = crate::planning::recompile_chapter_plan_owner(
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
                execution_mode,
                &planning_command_id,
                &format!("{idempotency_key}:planning-owner"),
                current_revision,
                &now,
                Some(&planning_run_id),
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
            let authority_input = crate::commands::context_input_from_authority(
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
            let authority_context = authority_context_payload(&authority_input)?;
            (
                context_ref,
                context_payload,
                plan_ref,
                plan_payload,
                authority_context,
            )
        };
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
        &chapter_id,
        &volume_id,
        execution_mode,
        current_revision,
    )?;
    let existing_canon_semantic_keys = existing_canon_semantic_keys(&authority_context);

    // B10 is an enrichment/quality layer on the existing B08 composition
    // root. Disabling it is an explicit compatibility path and still uses the
    // exact same renderer, audit, CommitBundle and FinalChapter owners.
    let creative_enrichment = if request
        .pointer("/payload/skillsEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(true)
    {
        let default_primary = if execution_mode == "OPENING_BOOTSTRAP" {
            "S14"
        } else {
            "S21"
        };
        let default_support = if execution_mode == "OPENING_BOOTSTRAP" {
            vec!["S10".to_string()]
        } else {
            vec!["S23".to_string()]
        };
        let primary = request
            .pointer("/payload/primarySkillId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(default_primary);
        let support = request
            .pointer("/payload/supportSkillIds")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or(default_support);
        let required_resources = request
            .pointer("/payload/skillResourcePaths")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Some(
            crate::creative_skills::enrich_chapter_with_loader(
                &project_id,
                &chapter_id,
                current_revision,
                &context_ref.artifact_id,
                &plan_payload,
                execution_mode,
                primary,
                support,
                required_resources,
                &now,
                skill_bundle_loader,
            )
            .map_err(|error| format!("B10_SKILL_RUNTIME:{error}"))?,
        )
    } else {
        None
    };
    let creative_enrichment_payload = creative_enrichment
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|error| format!("B10_SKILL_PAYLOAD:{error}"))?;

    update_stage(connection, &project_id, &job_id, "RENDER", &now)?;
    let render_spec = ModelRunSpec {
        model_run_id: render_run_id.clone(),
        job_id: Some(job_id.clone()),
        step_id: None,
        provider: provider_profile.provider.clone(),
        model: provider_profile.model.clone(),
        idempotency_key: format!("{idempotency_key}:render"),
        capability: "RENDER_CHAPTER".into(),
        prompt_ref: None,
        context_manifest_ref: Some(context_ref.artifact_id.clone()),
        budget_id: Some(production_budget_id.clone()),
        predicted_cost_cents: gateway.predicted_cost_cents("RENDER_CHAPTER").max(1),
        attempt_no: 1,
        repair_round: 0,
    };
    let target_chinese_characters = request
        .pointer("/payload/targetChineseCharacters")
        .and_then(Value::as_i64)
        .unwrap_or(1_800)
        .clamp(800, 5_000);
    let mut render_input = json!({"chapterId":chapter_id,"chapterPlanRef":plan_ref,"chapterPlan":plan_payload,"contextManifestRef":context_ref,"contextManifest":context_payload,"authorityContext":authority_context.clone(),"existingCanonSemanticKeys":existing_canon_semantic_keys.clone(),"targetChineseCharacters":target_chinese_characters});
    if let Some(enrichment) = creative_enrichment_payload.clone() {
        render_input["creativeEnrichment"] = enrichment;
    }
    let render_result = execute_production_model_with_profile(
        connection,
        gateway,
        &provider_profile,
        &render_spec,
        "RENDER_CHAPTER",
        render_input,
        &now,
    )?;
    let body = render_result
        .parsed
        .and_then(|value| {
            value
                .get("body")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .ok_or_else(|| "B08_RENDER_OUTPUT_INVALID".to_string())?;
    let body_digest = sha256_canonical(&json!(body)).map_err(|e| e.to_string())?;
    validate_candidate_evidence_range(&body, 0, body.chars().count())?;
    update_stage(connection, &project_id, &job_id, "CANDIDATE", &now)?;
    let candidate_payload = json!({
        "payloadSchema":"chapter-candidate.r1","chapterId":chapter_id,
        "chapterPlanRef":plan_ref.clone(),"contextManifestRef":context_ref.clone(),
        "body":body.clone(),"bodyDigest":body_digest,"evidenceRanges":[],
        "renderer":{"modelRunId":render_run_id,"capability":"RENDER_CHAPTER","provider":provider_profile.provider.clone(),"model":provider_profile.model.clone()},
        "creativeEnrichment":creative_enrichment_payload.clone().unwrap_or(Value::Null)
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

    let original_candidate_ref = reference(&candidate);
    let mut effective_candidate = candidate.clone();
    let mut effective_body = body.clone();
    let mut repaired_candidate = None;
    let mut quality_report = None;
    let mut repair_evidence = Value::Null;
    if let Some(enrichment) = creative_enrichment.as_ref() {
        let initial_report = crate::creative_skills::evaluate_narrative_quality(
            &original_candidate_ref,
            &project_id,
            current_revision,
            &body,
            &enrichment.taste_profile,
            enrichment
                .invocations
                .iter()
                .map(|invocation| invocation.invocation_id.clone())
                .collect(),
        )
        .map_err(|error| format!("B10_QUALITY_EVALUATION:{error}"))?;
        let repair_limit = request
            .pointer("/payload/repairRunLimit")
            .and_then(Value::as_i64)
            .unwrap_or(enrichment.context.budget.repair_runs);
        if let Some((dimension, replacement)) =
            b10_repair_target(&body, &initial_report.repairable_dimensions)
        {
            let patch = crate::creative_skills::repair_candidate(
                crate::creative_skills::RepairCandidateRequest {
                    candidate_ref: candidate.artifact_id.clone(),
                    candidate_status: "CANDIDATE".into(),
                    body: body.clone(),
                    finding_refs: initial_report
                        .findings
                        .iter()
                        .map(|finding| finding.finding_id.clone())
                        .collect(),
                    affected_dimensions: vec![dimension.clone()],
                    start: body.chars().count(),
                    end: body.chars().count(),
                    replacement,
                    skill_id: "S40".into(),
                    skill_version: crate::creative_skills::CREATIVE_SKILLS_VERSION.into(),
                    model: "deterministic-candidate-repair".into(),
                    repair_run: 1,
                    repair_run_limit: repair_limit,
                    context_tokens: enrichment.context.budget.context_tokens,
                    model_calls: 0,
                },
            )
            .map_err(|error| format!("B10_CANDIDATE_REPAIR:{error}"))?;
            if patch.status == "ADVISORY_ESCALATION" {
                let _ = DurableJobService::transition_for_worker(
                    connection,
                    &project_id,
                    &job_id,
                    "FAILED",
                    worker_id,
                    &now,
                );
                let _ = DurableJobService::release_lease(connection, &project_id, &job_id, &now);
                return Err("B10_REPAIR_LIMIT_REACHED:ESCALATE".into());
            }
            effective_body = patch.body.clone();
            let mut repaired_payload = candidate.payload.clone();
            repaired_payload["body"] = Value::String(effective_body.clone());
            repaired_payload["bodyDigest"] = Value::String(
                sha256_canonical(&json!(effective_body)).map_err(|error| error.to_string())?,
            );
            repaired_payload["candidatePatch"] = json!({
                "kind":"CandidatePatch",
                "status":"CANDIDATE",
                "originalCandidateRef":original_candidate_ref.clone(),
                "findingRefs":patch.finding_refs,
                "affectedDimensions":[dimension.clone()],
                "repairRun":patch.repair_run,
                "repairRunLimit":patch.repair_run_limit,
                "transportCalls":patch.transport_calls,
                "owner":"B10::CandidateRepairOwner"
            });
            let repaired_candidate_id = StableIdentityFactory::from_command(
                StableIdKind::Artifact,
                &format!("{command_id}:candidate-repair:1"),
            )?;
            let mut repaired = artifact(
                &project_id,
                repaired_candidate_id,
                ArtifactType::ChapterCandidate,
                &chapter_id,
                repaired_payload,
                &command_id,
                &job_id,
                Some(&render_run_id),
                &now,
                vec![original_candidate_ref.clone()],
            )?;
            repaired.provenance.context_manifest_ref = Some(context_ref.artifact_id.clone());
            effective_candidate = repaired.clone();
            repaired_candidate = Some(repaired);
            let mut rechecked = crate::creative_skills::recheck_affected_dimensions(
                &initial_report,
                &effective_body,
                &enrichment.taste_profile,
                vec![dimension.clone()],
            )
            .map_err(|error| format!("B10_QUALITY_RECHECK:{error}"))?;
            let effective_ref = reference(&effective_candidate);
            crate::creative_skills::rebind_quality_candidate(
                &mut rechecked,
                effective_ref.clone(),
            )?;
            rechecked.repair = Some(json!({
                "status":"REPAIRED",
                "run":patch.repair_run,
                "limit":patch.repair_run_limit,
                "transportCalls":patch.transport_calls,
                "candidateBefore":original_candidate_ref.clone(),
                "candidateAfter":effective_ref,
                "affectedDimensions":[dimension.clone()],
                "recheckedDimensions":rechecked.rechecked_dimensions.clone(),
                "escalation":Value::Null
            }));
            crate::creative_skills::rebind_quality_candidate(
                &mut rechecked,
                reference(&effective_candidate),
            )?;
            quality_report = Some(rechecked);
            repair_evidence = json!({
                "status":"REPAIRED",
                "run":patch.repair_run,
                "limit":patch.repair_run_limit,
                "transportCalls":patch.transport_calls,
                "effectiveCandidateRef":reference(&effective_candidate),
                "affectedDimensions":[dimension.clone()]
            });
        } else {
            let mut report = initial_report;
            report.repair = Some(json!({
                "status":"NO_REPAIR",
                "run":0,
                "limit":repair_limit,
                "transportCalls":0,
                "candidateBefore":original_candidate_ref.clone(),
                "candidateAfter":original_candidate_ref.clone(),
                "affectedDimensions":[],
                "recheckedDimensions":[],
                "escalation":Value::Null
            }));
            quality_report = Some(report);
            repair_evidence =
                json!({"status":"NO_REPAIR","run":0,"limit":repair_limit,"transportCalls":0});
        }
    }
    let effective_candidate_ref = reference(&effective_candidate);
    let (taste_artifact, quality_artifact, taste_ref, quality_ref) =
        if let Some(enrichment) = creative_enrichment.as_ref() {
            let taste_payload = serde_json::to_value(&enrichment.taste_profile)
                .map_err(|error| format!("B10_TASTE_PAYLOAD:{error}"))?;
            let mut taste_payload = taste_payload;
            taste_payload["payloadSchema"] = json!("narrative-taste-profile.r1");
            let mut taste = artifact(
                &project_id,
                taste_profile_id,
                ArtifactType::NarrativeTasteProfile,
                &chapter_id,
                taste_payload,
                &command_id,
                &job_id,
                None,
                &now,
                vec![context_ref.clone()],
            )?;
            taste.provenance.created_by = "B10::NarrativeTasteProfileOwner".into();
            let taste_ref = reference(&taste);
            let report = quality_report
                .take()
                .ok_or_else(|| "B10_QUALITY_REPORT_MISSING".to_string())?;
            let report_value = serde_json::to_value(&report)
                .map_err(|error| format!("B10_QUALITY_PAYLOAD:{error}"))?;
            let mut quality_payload = report_value;
            quality_payload["payloadSchema"] = json!("narrative-quality-report.r1");
            quality_payload["invocationProvenance"] = serde_json::to_value(&enrichment.invocations)
                .map_err(|error| format!("B10_INVOCATION_PAYLOAD:{error}"))?;
            quality_payload["skillSelection"] = serde_json::to_value(&enrichment.selection)
                .map_err(|error| format!("B10_SELECTION_PAYLOAD:{error}"))?;
            quality_payload["repairEvidence"] = repair_evidence.clone();
            let mut quality = artifact(
                &project_id,
                quality_report_id,
                ArtifactType::NarrativeQualityReport,
                &chapter_id,
                quality_payload,
                &command_id,
                &job_id,
                None,
                &now,
                vec![effective_candidate_ref.clone(), taste_ref.clone()],
            )?;
            quality.provenance.created_by = "B10::NarrativeQualityOwner".into();
            let quality_ref = reference(&quality);
            (
                Some(taste),
                Some(quality),
                Some(taste_ref),
                Some(quality_ref),
            )
        } else {
            (None, None, None, None)
        };
    update_stage(connection, &project_id, &job_id, "AUDIT", &now)?;
    let audit_spec = ModelRunSpec {
        model_run_id: audit_run_id.clone(),
        job_id: Some(job_id.clone()),
        step_id: None,
        provider: provider_profile.provider.clone(),
        model: provider_profile.model.clone(),
        idempotency_key: format!("{idempotency_key}:audit"),
        capability: "AUDIT_EXTRACT".into(),
        prompt_ref: None,
        context_manifest_ref: Some(context_ref.artifact_id.clone()),
        budget_id: Some(production_budget_id),
        predicted_cost_cents: gateway.predicted_cost_cents("AUDIT_EXTRACT").max(1),
        attempt_no: 1,
        repair_round: 0,
    };
    let audit_result = execute_production_model_with_profile(
        connection,
        gateway,
        &provider_profile,
        &audit_spec,
        "AUDIT_EXTRACT",
        json!({"chapterId":chapter_id,"chapterPlanRef":plan_ref,"chapterPlan":plan_payload,"contextManifestRef":context_ref,"authorityContext":authority_context.clone(),"existingCanonSemanticKeys":existing_canon_semantic_keys,"candidateRef":effective_candidate_ref.clone(),"body":effective_body.clone(),"qualityReportRef":quality_ref}),
        &now,
    )?;
    let audit_payload = audit_result
        .parsed
        .ok_or_else(|| "B08_AUDIT_OUTPUT_INVALID".to_string())?;
    let audit_payload = normalize_audit_model_output(audit_payload, &effective_body);
    let mut extraction: AuditExtraction = serde_json::from_value(audit_payload.clone())
        .map_err(|error| format!("B08_AUDIT_INCOMPLETE:{error}"))?;
    validate_audit_completeness(&audit_payload, &extraction)?;
    validate_actual_extraction(
        connection,
        &project_id,
        &chapter_id,
        &effective_body,
        &plan_payload,
        &gate_policy_payload,
        &mut extraction,
    )?;
    let thread_promotion_decisions = extraction
        .continuity_candidates
        .iter()
        .map(|candidate| {
            let signal = candidate.promotion_signals;
            let decision = match crate::narrative::decide_thread_promotion(signal) {
                ThreadPromotionDecision::Promote => "PROMOTE",
                ThreadPromotionDecision::DoNotPromote => "DO_NOT_PROMOTE",
            };
            json!({
                "threadRef": candidate.thread_ref,
                "decision": decision,
                "promotionSignals": signal,
                "evidence": {"start": candidate.evidence.start, "end": candidate.evidence.end}
            })
        })
        .collect::<Vec<_>>();
    let promotion_decision_for = |thread_ref: &str| {
        thread_promotion_decisions
            .iter()
            .find(|item| item.get("threadRef").and_then(Value::as_str) == Some(thread_ref))
            .and_then(|item| item.get("decision").and_then(Value::as_str))
            .unwrap_or("DO_NOT_PROMOTE")
    };
    let review_payload = json!({"payloadSchema":"actual-narrative-audit.r1","candidateRef":effective_candidate_ref.clone(),"gatePolicyRef":policy_ref.clone(),"canonSemanticPolicyVersion":gate_policy_payload.pointer("/canonSemantics/policyVersion").and_then(Value::as_str),"audit":audit_payload,"threadPromotionDecisions":thread_promotion_decisions.clone()});
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
        vec![effective_candidate_ref.clone()],
    )?;
    audit.provenance.context_manifest_ref = Some(context_ref.artifact_id.clone());
    validate_audit_candidate_binding(&audit.payload, &effective_candidate_ref)?;
    update_stage(connection, &project_id, &job_id, "GATE", &now)?;
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
    let mut obligation_evidence_records = Vec::new();
    let mut effective_continuity_refs = BTreeSet::new();
    for (index, item) in extraction.continuity_candidates.iter().enumerate() {
        let promotion_decision = promotion_decision_for(&item.thread_ref);
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
                "ADVANCE" | "PROGRESSING" => "PROGRESSING",
                _ => return Err("B08_THREAD_DISPOSITION_INVALID".into()),
            };
            obligation_delta.push(json!({
                "obligationId":authority_id,"scope":obligation.scope,"obligationType":obligation.obligation_type,
                "authorityLevel":obligation.authority_level,"lifecycle":lifecycle,"description":obligation.description,
                "mustResolveBeforeChapter":obligation.must_resolve_before_chapter,
                "prerequisites":obligation.prerequisites,
                "decisionRef":obligation.decision_ref,
                "payloadJson":{"sourceArtifactId":final_id,"sourceVersion":1,"promotionDecision":promotion_decision,"deltaMode":"EXISTING_AUTHORITY_UPDATE"}
            }));
            effective_continuity_refs.insert(item.thread_ref.clone());
            obligation_evidence_records.push(json!({
                "evidenceId":format!("obligation-evidence:{}:{}",final_id,index+1),
                "obligationId":authority_id,
                "evidenceType":if lifecycle == "RESOLVED" {"COMPLETION"} else {"PROGRESS"},
                "quote":item.evidence.quote,"rangeStart":item.evidence.start,"rangeEnd":item.evidence.end
            }));
            continue;
        }
        let foreshadow = crate::repositories::narrative::NarrativeRepository::foreshadow_authority(
            connection,
            &project_id,
            authority_id,
        )
        .map_err(|error| error.to_string())?;
        if let Some(foreshadow) = foreshadow {
            foreshadow_delta.push(json!({
                "foreshadowId":authority_id,"intent":foreshadow.intent,
                "status":if matches!(item.disposition.as_str(),"RESOLVE"|"RESOLVED") {"RECOVERED"} else {"REINFORCED"},
                "plantChapterId":foreshadow.plant_chapter_id,"recoverWindowEndChapter":foreshadow.recover_window_end_chapter,
                "secretPayloadRef":foreshadow.secret_payload_ref,
                "payloadJson":{"sourceAuthority":"B08_ACTUAL_NARRATIVE","sourceArtifactId":final_id,"sourceVersion":1,"promotionDecision":promotion_decision,"deltaMode":"EXISTING_AUTHORITY_UPDATE",
                    "evidence":{"start":item.evidence.start,"end":item.evidence.end,"quote":item.evidence.quote}}
            }));
            effective_continuity_refs.insert(item.thread_ref.clone());
            continue;
        }
        if promotion_decision != "PROMOTE" {
            // Audit evidence is retained, but a non-promoted ordinary action
            // must not mutate Continuity Reality.
            continue;
        }
        obligation_delta.push(json!({
            "obligationId":authority_id,
            "scope":"CHAPTER",
            "obligationType":"PROMOTED_CONTINUITY",
            "authorityLevel":"C",
            "lifecycle":"PROGRESSING",
            "description":format!("由本章审计确认的跨章责任：{}", item.thread_ref),
            "mustResolveBeforeChapter":Value::Null,
            "prerequisites":[],
            "decisionRef":format!("thread-promotion:{}:{}", final_id, index + 1),
            "payloadJson":{"sourceAuthority":"B08_ACTUAL_NARRATIVE","sourceArtifactId":final_id,"sourceVersion":1,"promotionDecision":"PROMOTE","deltaMode":"NEW_CONTINUITY"}
        }));
        effective_continuity_refs.insert(item.thread_ref.clone());
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
    let final_payload = json!({"payloadSchema":"final-chapter.r1","chapterId":chapter_id,"body":effective_candidate.payload["body"],"bodyDigest":effective_candidate.payload["bodyDigest"],"candidateRef":effective_candidate_ref.clone(),"chapterPlanRef":plan_ref.clone(),"contextManifestRef":context_ref.clone(),"auditRef":reference(&audit),"qualityReportRef":quality_ref,"tasteProfileRef":taste_ref,"wordCount":effective_candidate.payload["body"].as_str().map(|s| s.chars().count()).unwrap_or(0),"stateDigest":state_digest,"validatedEvidence":evidence_records,"validatedObligationEvidence":obligation_evidence_records});
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
        vec![effective_candidate_ref.clone(), reference(&audit)],
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
    let required_blocked = extraction
        .task_progress_candidates
        .iter()
        .any(|item| item.status == "BLOCKED");
    let reconciliation = crate::planning::classify_plan_reality(
        extraction.next_chapter_recompile_required,
        required_blocked,
        extraction.capacity_exhausted,
        extraction.change_requested,
    );
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
            "evidence":{"start":item.evidence.start,"end":item.evidence.end,"quote":item.evidence.quote}
        })
    }));
    let receipt_payload = json!({"payloadSchema":"production-receipt.r1","executionMode":execution_mode,"projectId":project_id,"chapterId":chapter_id,"jobId":job_id,"contextManifestRef":context_ref,"chapterPlanRef":plan_ref.clone(),"gatePolicyRef":policy_ref.clone(),"canonSemanticPolicyVersion":gate_policy_payload.pointer("/canonSemantics/policyVersion").and_then(Value::as_str),"candidateRef":effective_candidate_ref.clone(),"finalChapterRef":reference(&final_artifact),"auditRef":reference(&audit),"qualityReportRef":quality_ref,"tasteProfileRef":taste_ref,"creativeEnrichment":creative_enrichment_payload.clone().unwrap_or(Value::Null),"modelRunRefs":[render_run_id,audit_run_id],"reconciliation":reconciliation,"reality":reality,"threadPromotionDecisions":thread_promotion_decisions});
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
    let certificate_evidence = quality_artifact
        .as_ref()
        .map(|quality| vec![reference(&audit), reference(quality)])
        .unwrap_or_else(|| vec![reference(&audit)]);
    let certificate = make_certificate(
        &project_id,
        policy_ref,
        reference(&final_artifact),
        certificate_evidence,
        &now,
    )?;
    let heads = vec![
        json!({"artifactType":"FinalChapter","scope":chapter_scope(&chapter_id),"artifactId":final_artifact.artifact_id,"version":1,"payloadHash":final_artifact.payload_hash}),
        json!({"artifactType":"ChapterExitState","scope":chapter_scope(&chapter_id),"artifactId":exit_artifact.artifact_id,"version":1,"payloadHash":exit_artifact.payload_hash}),
        json!({"artifactType":"ProductionReceipt","scope":chapter_scope(&chapter_id),"artifactId":receipt_artifact.artifact_id,"version":1,"payloadHash":receipt_artifact.payload_hash}),
    ];
    let mut bundle_artifacts = Vec::new();
    let candidate_ref = effective_candidate_ref.clone();
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
            "evidenceRange":{"start":item.evidence.start,"end":item.evidence.end,"quote":item.evidence.quote}
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
    if let Some(taste) = taste_artifact {
        bundle_artifacts.push(taste);
    }
    if let Some(repaired) = repaired_candidate {
        bundle_artifacts.push(repaired);
    }
    if let Some(quality) = quality_artifact {
        bundle_artifacts.push(quality);
    }
    bundle_artifacts.extend([audit, final_artifact, exit_artifact, receipt_artifact]);
    let dependency_source_hash = if gateway.fault_injection() == Some("dependency_source_hash") {
        "0".repeat(64)
    } else {
        plan_ref.payload_hash.clone()
    };
    let bundle: CommitBundle = serde_json::from_value(json!({
        "commitId":format!("commit:{command_id}"),"projectId":project_id,"commandId":command_id,"idempotencyKey":idempotency_key,
        "expectedProjectRevision":current_revision,"contractVersion":"m0a.v1","createdAt":now,
        "artifacts":bundle_artifacts,"headUpdates":heads,
        "canonDelta":canon_delta,
        "knowledgeDelta":knowledge_delta,
        "obligationDelta":obligation_delta,
        "foreshadowDelta":foreshadow_delta,
        "stateDelta":state_delta,
        "dependencyEdges":[{"edgeId":format!("edge:{}:plan",final_ref.artifact_id),"sourceRef":plan_ref.artifact_id,"targetRef":final_ref.artifact_id,"dependencyType":"PLAN_INPUT","scope":"CHAPTER","impactClass":"HARD","sourceHash":dependency_source_hash,"payloadJson":{"createdBy":"B08::ChapterProductionRuntime"}}],
        "jobUpdates":[{"jobId":job_id,"status":"COMPLETED","currentStage":"COMPLETED","attemptNo":1}],
        "gateCertificates":[certificate],"receiptDraft":{"domain":"B08_CHAPTER_PRODUCTION","artifactRefs":[],"payload":{"executionMode":execution_mode,"chapterId":chapter_id,"reconciliation":reconciliation}}
    })).map_err(|error| format!("B08_BUNDLE_INVALID:{error}"))?;
    update_stage(connection, &project_id, &job_id, "COMMIT", &now)?;
    let receipt = CommitService::commit_b08_chapter_with_deferred_b09_projection(
        connection, &bundle, &volume_id, &now,
    )
    .map_err(|error| error.to_string())?;
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
    update_stage(connection, &project_id, &job_id, "COMPLETED", &now)?;
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

pub fn reconciliation_for_actual_state(
    stale: bool,
    required_blocked: bool,
    capacity_exhausted: bool,
    change_requested: bool,
) -> &'static str {
    crate::planning::classify_plan_reality(
        stale,
        required_blocked,
        capacity_exhausted,
        change_requested,
    )
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct EvidenceSpan {
    start: usize,
    end: usize,
    quote: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonCandidate {
    fact_id: String,
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
    evidence: EvidenceSpan,
    #[serde(default)]
    promotion_signals: PromotionSignal,
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
    #[serde(default)]
    capacity_exhausted: bool,
    #[serde(default)]
    change_requested: bool,
    #[serde(default)]
    next_chapter_recompile_required: bool,
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
    let body_chars = body.chars().collect::<Vec<_>>();
    let normalize_evidence = |value: &Value| -> Option<Value> {
        let start = value.get("start")?.as_u64()? as usize;
        let end = value.get("end")?.as_u64()? as usize;
        if start < end && end <= body_chars.len() {
            let quote = body_chars[start..end].iter().collect::<String>();
            return Some(json!({"start":start,"end":end,"quote":quote}));
        }
        // Providers occasionally count tokens/bytes rather than Unicode
        // characters. If the supplied span is outside the body, bind only an
        // exact quote that is actually present; otherwise drop the candidate.
        let quote = value
            .get("quote")
            .and_then(Value::as_str)
            .filter(|quote| !quote.is_empty())?;
        if let Some(byte_start) = body.find(quote) {
            let char_start = body[..byte_start].chars().count();
            return Some(json!({
                "start":char_start,
                "end":char_start + quote.chars().count(),
                "quote":quote
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
            "quote":actual_quote
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
    for (field, required) in candidate_requirements {
        let Some(items) = payload.get_mut(field).and_then(Value::as_array_mut) else {
            continue;
        };
        let before = items.len();
        items.retain_mut(|item| {
            let Some(object) = item.as_object_mut() else {
                return false;
            };
            if !required.iter().all(|key| object.contains_key(*key)) {
                return false;
            }
            let Some(evidence) = object.get("evidence").and_then(&normalize_evidence) else {
                return false;
            };
            object.insert("evidence".into(), evidence);
            if field == "knowledgeCandidates" {
                let Some(state) = object.get("epistemicState").and_then(Value::as_str) else {
                    return false;
                };
                let normalized_state = match state {
                    "KNOWS" | "KNOWS_WITH_CONTEXT" | "KNOWS_WITH_EVIDENCE" | "KNOWS_FACT" => {
                        "KNOWS"
                    }
                    "BELIEVES_TRUE" | "BELIEVES" => "BELIEVES_TRUE",
                    "BELIEVES_FALSE" | "SUSPECTS" | "CONCEALS" | "UNKNOWN" => state,
                    _ => return false,
                };
                object.insert("epistemicState".into(), json!(normalized_state));
            }
            true
        });
        dropped.insert(field.into(), json!(before - items.len()));
    }
    let (normalized_ranges, dropped_ranges) = payload
        .get("evidenceRanges")
        .and_then(Value::as_array)
        .map(|items| {
            let ranges = items
                .iter()
                .filter_map(&normalize_evidence)
                .collect::<Vec<_>>();
            let dropped = items.len() - ranges.len();
            (Some(ranges), dropped)
        })
        .unwrap_or((None, 0));
    if let Some(ranges) = normalized_ranges {
        payload["evidenceRanges"] = json!(ranges);
    }
    payload["normalization"] = json!({
        "policy":"DROP_UNBOUND_AUTHORITY_CANDIDATES",
        "droppedCandidates":dropped,
        "droppedEvidenceRanges":dropped_ranges
    });
    payload
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
    let continuity = plan
        .get("continuityActions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("threadRef").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
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

    for candidate in &audit.canon_candidates {
        canon_semantics
            .as_ref()
            .expect("canon semantics is required for Canon candidates")
            .validate_predicate(&candidate.predicate)
            .map_err(str::to_owned)?;
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
        for fact in current
            .iter()
            .chain(pending_canon_facts.get(&semantic_key).into_iter().flatten())
        {
            if let Some(error) = policy
                .evaluate(&candidate.predicate, fact, candidate_input.clone())
                .error_code()
            {
                return Err(error.into());
            }
        }
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
    for candidate in &audit.task_progress_candidates {
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
    }
    for candidate in &audit.continuity_candidates {
        exact_quote(body, &candidate.evidence)?;
        if !continuity.contains(candidate.thread_ref.as_str()) {
            return Err("B08_THREAD_TRANSITION_OUTSIDE_PLAN".into());
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
    for candidate in knowledge_candidates {
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
            continue;
        }
        let fact_time = audit
            .canon_candidates
            .iter()
            .find(|fact| fact.fact_id == candidate.fact_id)
            .and_then(|fact| fact.story_time.as_deref());
        if matches!((candidate.acquired_story_time.as_deref(), fact_time), (Some(acquired), Some(fact)) if acquired < fact)
        {
            return Err("B08_KNOWLEDGE_FUTURE_EVENT_CONTRADICTION".into());
        }
        bound_knowledge_candidates.push(candidate);
    }
    audit.knowledge_candidates = bound_knowledge_candidates;
    Ok(())
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
    use super::*;

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
    fn production_model_input_carries_bounded_authority_context() {
        let input = crate::narrative::ContextCompilerInput {
            execution_mode: "LONG_FORM".into(),
            project_revision: 7,
            goal_contract_refs: Vec::new(),
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
