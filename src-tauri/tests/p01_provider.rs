use ai_novel_production_system_core::production::{
    ConfiguredProductionGateway, ProductionModelGateway,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::provider::OpenAiCompatibleProvider;
use ai_novel_production_system_core::provider::{
    ProviderAdapter, ProviderConfig, ProviderErrorKind, ProviderRequest, ProviderResponse,
    ProviderUsage, ScriptedProvider,
};
use ai_novel_production_system_core::repositories::provider::ProviderSettingsRepository;
use ai_novel_production_system_core::runtime::MemorySecretStore;
use serde_json::json;
use std::io::Write;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

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
            "fixture-token-ephemeral",
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
            "invalid-fixture-token",
        )
        .unwrap_err();
    assert_eq!(error.kind, ProviderErrorKind::Authentication);
}

#[test]
fn p01_configured_gateway_requires_secret_and_never_returns_it() {
    let secret_store = MemorySecretStore::new();
    secret_store
        .put("fixture", "fixture-token-not-output")
        .unwrap();
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
    assert!(!response
        .output
        .to_string()
        .contains("fixture-token-not-output"));
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
    let provider =
        ScriptedProvider::counting(Arc::clone(&calls), Err(ProviderErrorKind::RateLimited));
    let result = provider.invoke_with_budget(
        &ProviderRequest {
            capability: "RENDER_CHAPTER".into(),
            model_id: settings.model_id.clone(),
            prompt: String::new(),
            timeout_ms: settings.timeout_ms,
        },
        "fixture-token",
        true,
    );
    assert_eq!(result.unwrap_err().kind, ProviderErrorKind::BudgetExceeded);
    assert_eq!(*calls.lock().unwrap(), 0);
}

#[test]
fn p01_settings_persist_without_api_key_and_provider_adapter_maps_transport_error() {
    let temp = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(temp.path());
    let input = json!({
        "providerId":"fixture",
        "modelId":"fixture-model",
        "provider":"OpenAI Compatible",
        "model":"fixture-model",
        "endpoint":"http://127.0.0.1:1",
        "enabled":true,
        "apiKey":"fixture-token-must-not-persist"
    });
    let saved = ProviderSettingsRepository::write_global(&paths, &input).unwrap();
    assert!(saved.get("apiKey").is_none());
    let raw = std::fs::read_to_string(paths.root().join("provider-settings.json")).unwrap();
    assert!(!raw.contains("fixture-token-must-not-persist"));
    assert_eq!(
        ProviderSettingsRepository::read_global(&paths).unwrap()["providerId"],
        "fixture"
    );

    let error = OpenAiCompatibleProvider::new("http://127.0.0.1:1")
        .invoke(
            &ProviderRequest {
                capability: "TEST_PROVIDER".into(),
                model_id: "fixture-model".into(),
                prompt: "ping".into(),
                timeout_ms: 500,
            },
            "fixture-token",
        )
        .unwrap_err();
    assert!(matches!(
        error.kind,
        ProviderErrorKind::Transport | ProviderErrorKind::Timeout
    ));
}

fn local_provider_server(status: u16, body: &'static str, delay: Duration) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        let response = format!(
            "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
    });
    format!("http://{address}")
}

#[test]
fn p01_openai_compatible_adapter_maps_real_http_success_auth_and_timeout() {
    let success_endpoint = local_provider_server(
        200,
        r#"{"id":"req:local","choices":[{"message":{"content":"{\"ok\":true}"}}],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}"#,
        Duration::ZERO,
    );
    let response = OpenAiCompatibleProvider::new(success_endpoint)
        .invoke(
            &ProviderRequest {
                capability: "TEST_PROVIDER".into(),
                model_id: "fixture-model".into(),
                prompt: "ping".into(),
                timeout_ms: 2_000,
            },
            "fixture-token-ephemeral",
        )
        .unwrap();
    assert_eq!(response.output["ok"], true);
    assert_eq!(response.usage.total_tokens, 8);
    assert_eq!(response.provider_request_id.as_deref(), Some("req:local"));

    let auth_endpoint = local_provider_server(401, r#"{"error":"unauthorized"}"#, Duration::ZERO);
    let auth_error = OpenAiCompatibleProvider::new(auth_endpoint)
        .invoke(
            &ProviderRequest {
                capability: "TEST_PROVIDER".into(),
                model_id: "fixture-model".into(),
                prompt: "ping".into(),
                timeout_ms: 2_000,
            },
            "fixture-token-ephemeral",
        )
        .unwrap_err();
    assert_eq!(auth_error.kind, ProviderErrorKind::Authentication);

    let timeout_endpoint = local_provider_server(200, r#"{"ok":true}"#, Duration::from_secs(2));
    let timeout_error = OpenAiCompatibleProvider::new(timeout_endpoint)
        .invoke(
            &ProviderRequest {
                capability: "TEST_PROVIDER".into(),
                model_id: "fixture-model".into(),
                prompt: "ping".into(),
                timeout_ms: 500,
            },
            "fixture-token-ephemeral",
        )
        .unwrap_err();
    assert_eq!(timeout_error.kind, ProviderErrorKind::Timeout);
}
