use ai_novel_production_system_core::production::{
    execute_production_model, ConfiguredProductionGateway, GatewayTransport, ProductionModelGateway,
};
use ai_novel_production_system_core::project_db::paths::AppPaths;
use ai_novel_production_system_core::project_db::ProjectManager;
use ai_novel_production_system_core::provider::OpenAiCompatibleProvider;
use ai_novel_production_system_core::provider::{
    ProviderAdapter, ProviderConfig, ProviderError, ProviderErrorKind, ProviderRequest,
    ProviderResponse, ProviderUsage, ScriptedProvider,
};
use ai_novel_production_system_core::repositories::provider::ProviderSettingsRepository;
use ai_novel_production_system_core::runtime::{
    DurableModelRunner, MemorySecretStore, ModelRunSpec, RuntimeError,
};
use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};
use libsql_rusqlite::Connection;
use serde_json::json;
use std::io::Write;
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[test]
fn p01_provider_adapter_maps_usage_and_typed_errors() {
    let provider = ScriptedProvider::new(Ok(ProviderResponse {
        raw_response: json!({"body":"ok"}).to_string(),
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
            raw_response: json!({"ok":true}).to_string(),
            usage: ProviderUsage::default(),
            provider_request_id: None,
        }))),
        Arc::new(secret_store.clone()),
    );
    let response = gateway
        .invoke("TEST_PROVIDER", &json!({"prompt":"ping"}))
        .unwrap();
    assert_eq!(response.raw_response.as_deref(), Some(r#"{"ok":true}"#));
    assert!(!response
        .raw_response
        .as_deref()
        .unwrap_or_default()
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

fn local_provider_server_with_calls(status: u16, body: &'static str) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&calls);
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        observed_calls.fetch_add(1, Ordering::SeqCst);
        let response = format!(
            "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
    });
    (format!("http://{address}"), calls)
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
    assert_eq!(
        response.raw_response,
        r#"{"id":"req:local","choices":[{"message":{"content":"{\"ok\":true}"}}],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}"#
    );
    assert_eq!(response.usage, ProviderUsage::default());
    assert_eq!(response.provider_request_id, None);

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

#[derive(Clone)]
struct FormalProvider {
    result: Result<ProviderResponse, ProviderError>,
    calls: Arc<Mutex<usize>>,
}

impl FormalProvider {
    fn success(raw_response: &str, calls: Arc<Mutex<usize>>) -> Self {
        Self {
            result: Ok(ProviderResponse {
                raw_response: raw_response.into(),
                usage: ProviderUsage {
                    input_tokens: 5,
                    output_tokens: 7,
                    total_tokens: 12,
                    provider_reported_cost_cents: Some(3),
                    ..ProviderUsage::default()
                },
                provider_request_id: Some("p01-formal-request".into()),
            }),
            calls,
        }
    }

    fn failure(kind: ProviderErrorKind, calls: Arc<Mutex<usize>>) -> Self {
        Self {
            result: Err(ProviderError::new(kind, "p01 formal provider fixture")),
            calls,
        }
    }
}

impl ProviderAdapter for FormalProvider {
    fn invoke(
        &self,
        _request: &ProviderRequest,
        _secret: &str,
    ) -> Result<ProviderResponse, ProviderError> {
        *self.calls.lock().unwrap() += 1;
        self.result.clone()
    }
}

fn formal_project() -> (
    tempfile::TempDir,
    ProjectManager,
    ai_novel_production_system_core::project_db::ProjectHandle,
) {
    let temp = tempfile::tempdir().unwrap();
    let manager = ProjectManager::new(AppPaths::new(temp.path()));
    let connection = manager
        .create("p01-formal", "P01 formal", "2026-08-28T00:00:00Z")
        .unwrap();
    (temp, manager, connection)
}

fn formal_spec(command: &str, budget_id: Option<&str>, predicted_cost_cents: i64) -> ModelRunSpec {
    ModelRunSpec {
        model_run_id: StableIdentityFactory::from_command(
            StableIdKind::ModelRun,
            &format!("p01-formal:{command}"),
        )
        .unwrap(),
        job_id: None,
        step_id: None,
        provider: "fixture".into(),
        model: "fixture-model".into(),
        idempotency_key: format!("p01-formal-key:{command}"),
        capability: "RENDER_CHAPTER".into(),
        prompt_ref: None,
        context_manifest_ref: None,
        budget_id: budget_id.map(ToOwned::to_owned),
        predicted_cost_cents,
        attempt_no: 1,
        repair_round: 0,
    }
}

fn formal_gateway(provider: FormalProvider) -> (ConfiguredProductionGateway, Arc<Mutex<usize>>) {
    let calls = Arc::clone(&provider.calls);
    let secrets = MemorySecretStore::new();
    secrets.put("fixture", "fixture-token-formal").unwrap();
    let gateway = ConfiguredProductionGateway::new(
        ProviderConfig {
            provider_id: "fixture".into(),
            model_id: "fixture-model".into(),
            endpoint: "http://fixture.invalid".into(),
            enabled: true,
            timeout_ms: 1000,
        },
        Arc::new(provider),
        Arc::new(secrets),
    );
    (gateway, calls)
}

#[test]
fn p01_formal_provider_raw_response_persists_before_model_payload_parse() {
    let (_temp, _manager, connection) = formal_project();
    let raw = r#"{"id":"raw-formal","choices":[{"message":{"content":"not-json"}}],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}"#;
    let calls = Arc::new(Mutex::new(0));
    let (gateway, calls) = formal_gateway(FormalProvider::success(raw, Arc::clone(&calls)));
    let spec = formal_spec("raw", None, 1);

    let error = execute_production_model(
        &connection,
        &gateway,
        &spec,
        "RENDER_CHAPTER",
        json!({"prompt":"formal raw"}),
        "2026-08-28T00:00:00Z",
    )
    .unwrap_err();
    assert!(error.contains("P01_MODEL_PAYLOAD_INVALID"));
    assert_eq!(*calls.lock().unwrap(), 1);
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT raw_response FROM model_responses WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        raw
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM model_parse_results WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        "SCHEMA_INVALID"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM model_runs WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        "FAILED"
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM artifacts WHERE project_id='p01-formal'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        0
    );
}

#[test]
fn p01_formal_raw_persist_failure_never_invokes_parse_or_authority_mutation() {
    let (_temp, _manager, connection) = formal_project();
    let calls = Arc::new(Mutex::new(0));
    let (gateway, calls) = formal_gateway(FormalProvider::success(
        r#"{"body":"must-not-parse"}"#,
        Arc::clone(&calls),
    ));
    let spec = formal_spec("raw-persist-fault", None, 1);
    let mut transport = GatewayTransport::with_raw_persist_failure(
        &gateway,
        "RENDER_CHAPTER",
        json!({"prompt":"fault"}),
    );

    assert!(matches!(
        DurableModelRunner::execute(&connection, &mut transport, &spec, "2026-08-28T00:00:00Z",),
        Err(RuntimeError::RawResponsePersistenceFailed)
    ));
    assert_eq!(*calls.lock().unwrap(), 1);
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM model_responses", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM model_parse_results", [], |row| row
                .get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM model_usage", [], |row| row.get(0))
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row::<i64, _, _>("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
            .unwrap(),
        0
    );
}

#[test]
fn p01_formal_configured_gateway_timeout_preserves_unknown_remote_outcome() {
    let (_temp, _manager, connection) = formal_project();
    let calls = Arc::new(Mutex::new(0));
    let (gateway, calls) = formal_gateway(FormalProvider::failure(
        ProviderErrorKind::Timeout,
        Arc::clone(&calls),
    ));
    let spec = formal_spec("timeout", None, 1);
    assert!(execute_production_model(
        &connection,
        &gateway,
        &spec,
        "RENDER_CHAPTER",
        json!({"prompt":"timeout"}),
        "2026-08-28T00:00:00Z",
    )
    .unwrap_err()
    .contains("UNKNOWN_REMOTE_OUTCOME"));
    assert_eq!(*calls.lock().unwrap(), 1);
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM model_runs WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0)
            )
            .unwrap(),
        "UNKNOWN_REMOTE_OUTCOME"
    );
    assert_eq!(connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM decisions WHERE decision_type='UNKNOWN_REMOTE_OUTCOME' AND outcome='OPEN'", [], |row| row.get(0)).unwrap(), 1);
    let mut retry = GatewayTransport::new(&gateway, "RENDER_CHAPTER", json!({"prompt":"retry"}));
    assert!(matches!(
        DurableModelRunner::retry(&connection, &mut retry, &spec, "2026-08-28T00:00:01Z"),
        Err(RuntimeError::UnknownRemoteOutcome)
    ));
    assert_eq!(*calls.lock().unwrap(), 1);
}

#[test]
fn p01_formal_configured_gateway_unknown_outcome_preserves_decision_boundary() {
    let (_temp, _manager, connection) = formal_project();
    let calls = Arc::new(Mutex::new(0));
    let (gateway, calls) = formal_gateway(FormalProvider::failure(
        ProviderErrorKind::UnknownRemoteOutcome,
        Arc::clone(&calls),
    ));
    let spec = formal_spec("unknown", None, 1);
    assert!(execute_production_model(
        &connection,
        &gateway,
        &spec,
        "RENDER_CHAPTER",
        json!({"prompt":"unknown"}),
        "2026-08-28T00:00:00Z",
    )
    .unwrap_err()
    .contains("UNKNOWN_REMOTE_OUTCOME"));
    assert_eq!(*calls.lock().unwrap(), 1);
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM model_runs WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0)
            )
            .unwrap(),
        "UNKNOWN_REMOTE_OUTCOME"
    );
    assert_eq!(connection.query_row::<i64, _, _>("SELECT COUNT(*) FROM decisions WHERE decision_type='UNKNOWN_REMOTE_OUTCOME' AND outcome='OPEN'", [], |row| row.get(0)).unwrap(), 1);
}

fn seed_formal_budget(connection: &Connection, budget_id: &str, hard_max: i64, cost_budget: i64) {
    connection.execute("INSERT OR IGNORE INTO books(book_id,project_id,book_no,title,status,created_at,updated_at) VALUES ('book:p01:budget','p01-formal',1,'Book','ACTIVE','2026-08-28T00:00:00Z','2026-08-28T00:00:00Z')", []).unwrap();
    connection.execute("INSERT OR IGNORE INTO volumes(volume_id,project_id,book_id,volume_no,title,status,chapter_start,chapter_end,target_words,created_at,updated_at) VALUES ('volume:p01:budget','p01-formal','book:p01:budget',1,'Volume','PLANNING',1,1,1000,'2026-08-28T00:00:00Z','2026-08-28T00:00:00Z')", []).unwrap();
    connection.execute("INSERT OR IGNORE INTO chapters(chapter_id,project_id,volume_id,chapter_no,title,importance_class,status,word_count,created_at,updated_at) VALUES ('chapter:p01:budget','p01-formal','volume:p01:budget',1,'Chapter','NORMAL','PLANNED',0,'2026-08-28T00:00:00Z','2026-08-28T00:00:00Z')", []).unwrap();
    connection.execute("INSERT INTO chapter_production_budgets(budget_id,project_id,chapter_id,importance_class,hard_max_model_runs,cost_budget_cents,frozen_at) VALUES (?1,'p01-formal','chapter:p01:budget','NORMAL',?2,?3,'2026-08-28T00:00:00Z')", libsql_rusqlite::params![budget_id, hard_max, cost_budget]).unwrap();
}

#[test]
fn p01_formal_budget_binding_blocks_provider_before_send_and_settles_success_cost() {
    let (_temp, _manager, connection) = formal_project();
    seed_formal_budget(&connection, "budget:p01:hard", 1, 0);
    connection.execute("INSERT INTO budget_reservations(reservation_id,project_id,budget_ref,reserved_cents,settled_cents,status,created_at,settled_at) VALUES ('reservation:p01:existing','p01-formal','budget:p01:hard',1,1,'OVER_LIMIT','2026-08-28T00:00:00Z','2026-08-28T00:00:00Z')", []).unwrap();
    let calls = Arc::new(Mutex::new(0));
    let (gateway, calls) = formal_gateway(FormalProvider::success(
        r#"{"body":"must-not-send"}"#,
        Arc::clone(&calls),
    ));
    let blocked = formal_spec("budget-blocked", Some("budget:p01:hard"), 1);
    assert!(matches!(
        execute_production_model(
            &connection,
            &gateway,
            &blocked,
            "PLAN_STRATEGY",
            json!({"prompt":"blocked"}),
            "2026-08-28T00:00:00Z",
        ),
        Err(error) if error.contains("budget hard limit")
    ));
    assert_eq!(*calls.lock().unwrap(), 0);

    connection
        .execute(
            "DELETE FROM budget_reservations WHERE reservation_id='reservation:p01:existing'",
            [],
        )
        .unwrap();
    connection.execute("UPDATE chapter_production_budgets SET hard_max_model_runs=3,cost_budget_cents=20 WHERE budget_id='budget:p01:hard'", []).unwrap();
    let successful = formal_spec("budget-success", Some("budget:p01:hard"), 4);
    execute_production_model(
        &connection,
        &gateway,
        &successful,
        "RENDER_CHAPTER",
        json!({"prompt":"success"}),
        "2026-08-28T00:00:00Z",
    )
    .unwrap();
    assert_eq!(*calls.lock().unwrap(), 1);
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT budget_ref FROM budget_reservations WHERE model_run_id=?1",
                [&successful.model_run_id],
                |row| row.get(0)
            )
            .unwrap(),
        "budget:p01:hard"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM budget_reservations WHERE model_run_id=?1",
                [&successful.model_run_id],
                |row| row.get(0)
            )
            .unwrap(),
        "SETTLED"
    );
    assert_eq!(connection.query_row::<i64, _, _>("SELECT cost_cents FROM model_cost_ledger WHERE model_run_id=?1 AND cost_type='ACTUAL'", [&successful.model_run_id], |row| row.get(0)).unwrap(), 3);

    let spent_id =
        StableIdentityFactory::from_command(StableIdKind::ModelRun, "p01-formal:spent").unwrap();
    connection.execute("INSERT INTO model_runs(model_run_id,project_id,provider,model,idempotency_key,capability,predicted_cost_cents,reserved_cost_cents,status,created_at,updated_at) VALUES (?1,'p01-formal','fixture','fixture-model','p01-formal-spent','RENDER_CHAPTER',15,15,'COMPLETED','2026-08-28T00:00:00Z','2026-08-28T00:00:00Z')", libsql_rusqlite::params![spent_id]).unwrap();
    connection.execute("INSERT INTO model_usage(usage_id,project_id,model_run_id,input_tokens,output_tokens,total_tokens,latency_ms,cost_cents,recorded_at) VALUES ('usage:p01:spent','p01-formal',?1,1,1,2,1,15,'2026-08-28T00:00:00Z')", libsql_rusqlite::params![spent_id]).unwrap();
    connection.execute("INSERT INTO model_cost_ledger(ledger_id,project_id,model_run_id,budget_id,cost_cents,token_total,cost_type,created_at) VALUES ('ledger:p01:spent','p01-formal',?1,'budget:p01:hard',15,2,'ACTUAL','2026-08-28T00:00:00Z')", libsql_rusqlite::params![spent_id]).unwrap();
    let cost_blocked = formal_spec("budget-cost-blocked", Some("budget:p01:hard"), 4);
    assert!(matches!(
        execute_production_model(
            &connection,
            &gateway,
            &cost_blocked,
            "AUDIT_EXTRACT",
            json!({"prompt":"cost blocked"}),
            "2026-08-28T00:00:00Z",
        ),
        Err(error) if error.contains("budget hard limit")
    ));
    assert_eq!(*calls.lock().unwrap(), 1);
}

#[test]
fn p01_formal_real_openai_compatible_usage_settles_after_exact_raw_persist() {
    let (_temp, _manager, connection) = formal_project();
    seed_formal_budget(&connection, "budget:p01:real", 3, 20);
    let body = r#"{"id":"req:local","choices":[{"message":{"content":"{\"body\":\"ok\"}"}}],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}"#;
    let (endpoint, calls) = local_provider_server_with_calls(200, body);
    let secrets = MemorySecretStore::new();
    secrets.put("fixture", "fixture-token-real-http").unwrap();
    let gateway = ConfiguredProductionGateway::new(
        ProviderConfig {
            provider_id: "fixture".into(),
            model_id: "fixture-model".into(),
            endpoint: endpoint.clone(),
            enabled: true,
            timeout_ms: 2_000,
        },
        Arc::new(OpenAiCompatibleProvider::new(endpoint)),
        Arc::new(secrets),
    );
    let spec = formal_spec("real-openai-usage", Some("budget:p01:real"), 1);
    let outcome = execute_production_model(
        &connection,
        &gateway,
        &spec,
        "RENDER_CHAPTER",
        json!({"prompt":"real usage"}),
        "2026-08-28T00:00:00Z",
    )
    .unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(outcome.parsed, Some(json!({"body":"ok"})));
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT raw_response FROM model_responses WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        body
    );
    assert_eq!(
        connection
            .query_row::<(i64, i64, i64), _, _>(
                "SELECT input_tokens, output_tokens, total_tokens FROM model_usage WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap(),
        (5, 3, 8)
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT status FROM budget_reservations WHERE model_run_id=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        "SETTLED"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT budget_id FROM model_cost_ledger WHERE model_run_id=?1 AND cost_type='ACTUAL'",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        "budget:p01:real"
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT json_extract(payload_json,'$.providerRequestId') FROM production_events WHERE event_type='MODEL_USAGE_RECORDED' AND json_extract(payload_json,'$.modelRunId')=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        "req:local"
    );
}
