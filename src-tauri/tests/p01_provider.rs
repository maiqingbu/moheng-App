use ai_novel_production_system_core::provider::{
    ProviderAdapter, ProviderConfig, ProviderErrorKind, ProviderRequest, ProviderResponse,
    ProviderUsage, ScriptedProvider,
};
use ai_novel_production_system_core::production::ConfiguredProductionGateway;
use ai_novel_production_system_core::runtime::MemorySecretStore;
use serde_json::json;
use std::sync::{Arc, Mutex};

#[test]
fn p01_provider_adapter_maps_usage_and_typed_errors() {
    let provider = ScriptedProvider::new(Ok(ProviderResponse {
        output: json!({"body":"ok"}),
        usage: ProviderUsage {
            input_tokens: 11,
            output_tokens: 7,
            cached_tokens: Some(3),
            total_tokens: 18,
            provider_reported_cost_cents: Some(4),
        },
        provider_request_id: Some("req:p01".into()),
    }));
    let response = provider
        .invoke(
            &ProviderRequest {
                capability: "RENDER_CHAPTER".into(),
                model_id: "fixture-model".into(),
                prompt: "bounded".into(),
                timeout_ms: 1000,
            },
            "secret-is-ephemeral",
        )
        .unwrap();
    assert_eq!(response.usage.total_tokens, 18);
    assert_eq!(response.provider_request_id.as_deref(), Some("req:p01"));

    let provider = ScriptedProvider::new(Err(ProviderErrorKind::Authentication));
    let error = provider
        .invoke(
            &ProviderRequest {
                capability: "TEST_PROVIDER".into(),
                model_id: "fixture-model".into(),
                prompt: String::new(),
                timeout_ms: 1000,
            },
            "bad-secret",
        )
        .unwrap_err();
    assert_eq!(error.kind, ProviderErrorKind::Authentication);
}

#[test]
fn p01_configured_gateway_requires_secret_and_never_returns_it() {
    let secret_store = MemorySecretStore::new();
    secret_store.put("fixture", "secret-not-output").unwrap();
    let gateway = ConfiguredProductionGateway::new(
        ProviderConfig {
            provider_id: "fixture".into(),
            model_id: "fixture-model".into(),
            endpoint: "http://127.0.0.1:1".into(),
            enabled: true,
            timeout_ms: 1000,
        },
        Arc::new(ScriptedProvider::new(Ok(ProviderResponse {
            output: json!({"ok":true}),
            usage: ProviderUsage::default(),
            provider_request_id: None,
        }))),
        Arc::new(secret_store.clone()),
    );
    let response = gateway
        .invoke("TEST_PROVIDER", &json!({"prompt":"ping"}))
        .unwrap();
    assert_eq!(response.output["ok"], true);
    assert!(!response.output.to_string().contains("secret-not-output"));
}

#[test]
fn p01_provider_settings_are_non_secret_and_budget_gate_precedes_invocation() {
    let settings = ProviderConfig {
        provider_id: "fixture".into(),
        model_id: "fixture-model".into(),
        endpoint: "http://127.0.0.1:1".into(),
        enabled: true,
        timeout_ms: 1000,
    };
    let encoded = serde_json::to_string(&settings).unwrap();
    assert!(!encoded.contains("apiKey"));
    let calls = Arc::new(Mutex::new(0));
    let provider = ScriptedProvider::counting(Arc::clone(&calls), Err(ProviderErrorKind::RateLimited));
    let result = provider.invoke_with_budget(
        &ProviderRequest {
            capability: "RENDER_CHAPTER".into(),
            model_id: settings.model_id.clone(),
            prompt: String::new(),
            timeout_ms: settings.timeout_ms,
        },
        "secret",
        true,
    );
    assert_eq!(result.unwrap_err().kind, ProviderErrorKind::BudgetExceeded);
    assert_eq!(*calls.lock().unwrap(), 0);
}
