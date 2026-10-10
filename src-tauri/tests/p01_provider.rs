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
    DurableModelRunner, MemorySecretStore, ModelRunSpec, RuntimeError, SecretStore,
};
use ai_novel_production_system_core::stable_id::{StableIdKind, StableIdentityFactory};
use libsql_rusqlite::Connection;
use serde_json::json;
#[cfg(target_os = "macos")]
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(target_os = "macos")]
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_os = "macos")]
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::time::{SystemTime, UNIX_EPOCH};

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
    let api_key = ["fixture-token-", "must-not-persist"].concat();
    let input = json!({
        "providerId":"fixture",
        "modelId":"fixture-model",
        "provider":"OpenAI Compatible",
        "model":"fixture-model",
        "endpoint":"http://127.0.0.1:1",
        "enabled":true,
        "apiKey":api_key
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
        read_local_http_request(&mut stream);
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

fn read_local_http_request(stream: &mut TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    while let Ok(read) = stream.read(&mut buffer) {
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if request.len() >= header_end + 4 + content_length {
            break;
        }
    }
}

fn local_provider_server_with_calls(status: u16, body: &'static str) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&calls);
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_local_http_request(&mut stream);
        observed_calls.fetch_add(1, Ordering::SeqCst);
        let response = format!(
            "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
    });
    (format!("http://{address}"), calls)
}

fn local_provider_server_with_truncated_body() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&calls);
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_local_http_request(&mut stream);
        observed_calls.fetch_add(1, Ordering::SeqCst);
        let response =
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 24\r\nConnection: close\r\n\r\n{\"partial\":";
        let _ = stream.write_all(response.as_bytes());
    });
    (format!("http://{address}"), calls)
}

#[test]
fn p01_provider_502_503_504_and_body_read_failure_are_unknown_without_retry() {
    for status in [502, 503, 504] {
        let (endpoint, calls) = local_provider_server_with_calls(status, "upstream failure");
        let error = OpenAiCompatibleProvider::new(endpoint)
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
        assert_eq!(
            error.kind,
            ProviderErrorKind::UnknownRemoteOutcome,
            "status={status}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "status={status}");
    }

    let (endpoint, calls) = local_provider_server_with_truncated_body();
    let error = OpenAiCompatibleProvider::new(endpoint)
        .invoke(
            &ProviderRequest {
                capability: "TEST_PROVIDER".into(),
                model_id: "fixture-model".into(),
                prompt: "body read failure".into(),
                timeout_ms: 2_000,
            },
            "fixture-token-ephemeral",
        )
        .unwrap_err();
    assert_eq!(error.kind, ProviderErrorKind::UnknownRemoteOutcome);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

fn disconnect_after_provider_request() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let Ok(read) = stream.read(&mut buffer) else {
                return;
            };
            if read == 0 {
                return;
            }
            request.extend_from_slice(&buffer[..read]);
            let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if request.len() >= header_end + 4 + content_length {
                break;
            }
        }
        // Dropping the stream after the full request creates a post-send unknown outcome.
    });
    format!("http://{address}")
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct SecretProcessObservation {
    process_name_matches: usize,
    process_count: usize,
    argv_canary_matches: usize,
    environment_dump_canary_matches: usize,
}

#[cfg(target_os = "macos")]
impl SecretProcessObservation {
    fn merge(&mut self, other: Self) {
        self.process_name_matches += other.process_name_matches;
        self.process_count += other.process_count;
        self.argv_canary_matches += other.argv_canary_matches;
        self.environment_dump_canary_matches += other.environment_dump_canary_matches;
    }
}

#[cfg(target_os = "macos")]
fn runtime_secret_canary() -> String {
    let mut random = [0_u8; 24];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .expect("OS random source is available");
    format!("MOHENG_SECRET_CANARY_{}", hex::encode(random))
}

#[cfg(target_os = "macos")]
fn process_arguments_and_environment(pid: u32) -> Option<(Vec<u8>, Vec<u8>)> {
    use std::ffi::c_void;

    unsafe extern "C" {
        fn sysctl(
            name: *mut i32,
            name_len: u32,
            old_value: *mut c_void,
            old_value_len: *mut usize,
            new_value: *mut c_void,
            new_value_len: usize,
        ) -> i32;
    }

    const CTL_KERN: i32 = 1;
    const KERN_PROCARGS2: i32 = 49;
    let mut mib = [CTL_KERN, KERN_PROCARGS2, pid as i32];
    let mut data = vec![0_u8; 512 * 1024];
    let mut data_len = data.len();
    // SAFETY: The MIB identifies this process query; `data` is a writable buffer,
    // and `data_len` supplies its capacity and receives the initialized length.
    let result = unsafe {
        sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            data.as_mut_ptr().cast::<c_void>(),
            &mut data_len,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || data_len < std::mem::size_of::<i32>() {
        return None;
    }
    data.truncate(data_len);

    let argument_count = i32::from_ne_bytes(data[..4].try_into().ok()?);
    if argument_count <= 0 {
        return None;
    }
    let executable_end = data[4..].iter().position(|byte| *byte == 0)? + 4;
    let mut arguments_start = executable_end + 1;
    while data.get(arguments_start) == Some(&0) {
        arguments_start += 1;
    }

    let mut arguments_end = arguments_start;
    for _ in 0..argument_count {
        let next_end = data
            .get(arguments_end..)?
            .iter()
            .position(|byte| *byte == 0)?;
        arguments_end += next_end + 1;
    }
    let mut environment_start = arguments_end;
    while data.get(environment_start) == Some(&0) {
        environment_start += 1;
    }
    Some((
        data[arguments_start..arguments_end].to_vec(),
        data[environment_start..].to_vec(),
    ))
}

#[cfg(target_os = "macos")]
fn observe_child_processes(
    parent_pid: u32,
    process_name: &str,
    marker: &str,
    canary: &str,
) -> SecretProcessObservation {
    let mut observed = SecretProcessObservation::default();
    let Ok(listing) = ProcessCommand::new("/bin/ps")
        .args(["-ww", "-axo", "pid=,ppid=,command="])
        .output()
    else {
        return observed;
    };
    for line in String::from_utf8_lossy(&listing.stdout).lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 3 {
            continue;
        }
        let (Ok(pid), Ok(ppid)) = (fields[0].parse::<u32>(), fields[1].parse::<u32>()) else {
            continue;
        };
        let command_line = fields[2..].join(" ");
        if ppid != parent_pid {
            continue;
        }
        if !command_line.contains(process_name) {
            continue;
        }
        observed.process_name_matches += 1;
        let Some((arguments, environment)) = process_arguments_and_environment(pid) else {
            continue;
        };
        if !arguments
            .windows(marker.len())
            .any(|window| window == marker.as_bytes())
        {
            continue;
        }
        observed.process_count += 1;
        observed.argv_canary_matches += usize::from(
            arguments
                .windows(canary.len())
                .any(|window| window == canary.as_bytes()),
        );
        observed.environment_dump_canary_matches += usize::from(
            environment
                .windows(canary.len())
                .any(|window| window == canary.as_bytes()),
        );
    }
    observed
}

#[cfg(target_os = "macos")]
fn gated_local_provider_server() -> (String, Receiver<String>, Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (request_tx, request_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let Ok(read) = stream.read(&mut buffer) else {
                return;
            };
            if read == 0 {
                return;
            }
            request.extend_from_slice(&buffer[..read]);
            let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..end]).into_owned();
            if request_tx.send(headers).is_err() {
                return;
            }
            break;
        }
        if release_rx.recv_timeout(Duration::from_secs(4)).is_err() {
            return;
        }
        let body = r#"{"ok":true}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
    });
    (format!("http://{address}"), request_rx, release_tx)
}

#[cfg(target_os = "macos")]
#[test]
fn p01_b205_provider_canary_stays_out_of_child_argv_and_environment() {
    let canary = runtime_secret_canary();
    let (endpoint, request_rx, release_tx) = gated_local_provider_server();
    let marker = endpoint.trim_start_matches("http://").to_string();
    let provider = OpenAiCompatibleProvider::new(endpoint);
    let request = ProviderRequest {
        capability: "TEST_PROVIDER".into(),
        model_id: "fixture-model".into(),
        prompt: "local secret-boundary fixture".into(),
        timeout_ms: 4_000,
    };
    let secret = canary.clone();
    let invocation = thread::spawn(move || provider.invoke(&request, &secret));
    let headers = request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("local server received the Provider request");
    let expected_header = format!("authorization: bearer {}", canary.to_ascii_lowercase());
    let header_received = headers.to_ascii_lowercase().contains(&expected_header);
    let observation = observe_child_processes(std::process::id(), "curl", &marker, &canary);
    let parent_environment_contains_canary =
        std::env::vars().any(|(_, value)| value.contains(&canary));
    let _ = release_tx.send(());
    let response = invocation
        .join()
        .expect("Provider request thread completed");
    println!(
        "B_R2_05_PROVIDER_PROCESS_SCAN={{\"headerReceived\":{},\"matchedChildren\":{},\"argvCanaryMatches\":{},\"environmentDumpCanaryMatches\":{},\"parentEnvironmentCanaryMatches\":{}}}",
        header_received,
        observation.process_count,
        observation.argv_canary_matches,
        observation.environment_dump_canary_matches,
        parent_environment_contains_canary
    );
    assert!(
        header_received,
        "local HTTP server did not receive the expected Authorization header"
    );
    assert_eq!(response.unwrap().raw_response, r#"{"ok":true}"#);
    assert_eq!(
        observation.argv_canary_matches, 0,
        "Provider credential appeared in child argv"
    );
    assert_eq!(
        observation.environment_dump_canary_matches, 0,
        "Provider credential appeared in a process environment dump"
    );
    assert!(
        !parent_environment_contains_canary,
        "Provider credential appeared in the test process environment"
    );
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "uses disposable items in the current macOS user Keychain"]
fn p01_b205_keychain_roundtrip_survives_process_reopen_and_delete() {
    use ai_novel_production_system_core::repositories::runtime::OsKeychainSecretStore;

    const PRIMARY_PROVIDER: &str = "provider-primary";
    const SECONDARY_PROVIDER: &str = "provider-secondary";
    struct Cleanup {
        service: String,
        account: &'static str,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let store = OsKeychainSecretStore::new(self.service.clone(), self.account);
            let _ = store.delete(PRIMARY_PROVIDER);
            let _ = store.delete(SECONDARY_PROVIDER);
        }
    }

    let primary_secret = runtime_secret_canary();
    let secondary_secret = runtime_secret_canary();
    let unique = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let service = format!("com.moheng.b-r2-05.green.{unique}");
    let account_base = "provider-api-key";
    let _cleanup = Cleanup {
        service: service.clone(),
        account: account_base,
    };
    let worker_service = service.clone();
    let primary_for_worker = primary_secret.clone();
    let secondary_for_worker = secondary_secret.clone();
    let worker = thread::spawn(move || {
        let store = OsKeychainSecretStore::new(worker_service, account_base);
        for _ in 0..8 {
            store.set(PRIMARY_PROVIDER, &primary_for_worker)?;
            assert_eq!(store.get(PRIMARY_PROVIDER)?, primary_for_worker);
            store.set(SECONDARY_PROVIDER, &secondary_for_worker)?;
            assert_eq!(store.get(SECONDARY_PROVIDER)?, secondary_for_worker);
        }
        Ok::<_, RuntimeError>(())
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut observation = SecretProcessObservation::default();
    while !worker.is_finished() && std::time::Instant::now() < deadline {
        observation.merge(observe_child_processes(
            std::process::id(),
            "security",
            &service,
            &primary_secret,
        ));
        thread::sleep(Duration::from_millis(2));
    }
    worker.join().expect("Keychain worker completed").unwrap();

    let expected_digest = hex::encode(Sha256::digest(primary_secret.as_bytes()));
    let mut child = ProcessCommand::new(std::env::current_exe().expect("test executable path"))
        .args([
            "--exact",
            "p01_b205_keychain_reopen_child",
            "--ignored",
            "--nocapture",
        ])
        .env("MOHENG_B_R2_05_REOPEN", "1")
        .env("MOHENG_B_R2_05_KEYCHAIN_SERVICE", &service)
        .env("MOHENG_B_R2_05_KEYCHAIN_ACCOUNT", account_base)
        .env("MOHENG_B_R2_05_KEYCHAIN_PROVIDER", PRIMARY_PROVIDER)
        .env("MOHENG_B_R2_05_EXPECTED_DIGEST", expected_digest)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("restart-style Keychain reader process started");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut child_argv_canary_matches = None;
    let mut child_environment_canary_matches = None;
    while std::time::Instant::now() < deadline {
        if let Some((arguments, environment)) = process_arguments_and_environment(child.id()) {
            child_argv_canary_matches = Some(usize::from(
                arguments
                    .windows(primary_secret.len())
                    .any(|window| window == primary_secret.as_bytes()),
            ));
            child_environment_canary_matches = Some(usize::from(
                environment
                    .windows(primary_secret.len())
                    .any(|window| window == primary_secret.as_bytes()),
            ));
            break;
        }
        if child.try_wait().unwrap().is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    drop(child.stdin.take());
    let child_output = child
        .wait_with_output()
        .expect("restart-style Keychain reader completed");
    let child_stdout = String::from_utf8_lossy(&child_output.stdout);
    let child_stderr = String::from_utf8_lossy(&child_output.stderr);
    let parent_environment_contains_primary = std::env::vars()
        .any(|(_, value)| value.contains(&primary_secret) || value.contains(&secondary_secret));
    println!(
        "B_R2_05_KEYCHAIN_PROCESS_SCAN={{\"securityCliChildren\":{},\"childArgvCanaryMatches\":{},\"childEnvironmentCanaryMatches\":{},\"parentEnvironmentCanaryMatches\":{},\"reopenMatched\":{}}}",
        observation.process_name_matches,
        child_argv_canary_matches.unwrap_or(0),
        child_environment_canary_matches.unwrap_or(0),
        parent_environment_contains_primary,
        child_stdout.contains("B_R2_05_KEYCHAIN_REOPEN={\"matched\":true}")
    );
    assert!(
        child_output.status.success(),
        "reopen process failed: {child_stderr}"
    );
    assert!(
        child_stdout.contains("B_R2_05_KEYCHAIN_REOPEN={\"matched\":true}"),
        "reopen process did not read the original Keychain value"
    );
    assert_eq!(child_argv_canary_matches, Some(0));
    assert_eq!(child_environment_canary_matches, Some(0));
    assert_eq!(observation.process_name_matches, 0);
    assert!(
        !parent_environment_contains_primary,
        "Keychain credential appeared in the parent process environment"
    );

    let store = OsKeychainSecretStore::new(service, account_base);
    assert_eq!(store.get(SECONDARY_PROVIDER).unwrap(), secondary_secret);
    store.delete(PRIMARY_PROVIDER).unwrap();
    store.delete(SECONDARY_PROVIDER).unwrap();
    assert!(store.get(PRIMARY_PROVIDER).is_err());
    assert!(store.get(SECONDARY_PROVIDER).is_err());
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "invoked as a child process by the Keychain lifecycle acceptance test"]
fn p01_b205_keychain_reopen_child() {
    use ai_novel_production_system_core::repositories::runtime::OsKeychainSecretStore;

    if std::env::var("MOHENG_B_R2_05_REOPEN").as_deref() != Ok("1") {
        return;
    }
    let service = std::env::var("MOHENG_B_R2_05_KEYCHAIN_SERVICE").unwrap();
    let account = std::env::var("MOHENG_B_R2_05_KEYCHAIN_ACCOUNT").unwrap();
    let provider = std::env::var("MOHENG_B_R2_05_KEYCHAIN_PROVIDER").unwrap();
    let expected_digest = std::env::var("MOHENG_B_R2_05_EXPECTED_DIGEST").unwrap();
    let store = OsKeychainSecretStore::new(service, account);
    let matched = store
        .get(&provider)
        .map(|secret| hex::encode(Sha256::digest(secret.as_bytes())) == expected_digest)
        .unwrap_or(false);
    println!("B_R2_05_KEYCHAIN_REOPEN={{\"matched\":{matched}}}");
    assert!(
        matched,
        "reopened process did not read the expected Keychain entry"
    );
    let mut wait_for_parent = String::new();
    std::io::stdin()
        .read_to_string(&mut wait_for_parent)
        .expect("read parent release signal");
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

#[test]
fn p01_openai_compatible_adapter_maps_post_send_disconnect_to_unknown_remote_outcome() {
    let endpoint = disconnect_after_provider_request();
    let canary = runtime_secret_canary();
    let error = OpenAiCompatibleProvider::new(endpoint)
        .invoke(
            &ProviderRequest {
                capability: "TEST_PROVIDER".into(),
                model_id: "fixture-model".into(),
                prompt: "disconnect after request body is received".into(),
                timeout_ms: 2_000,
            },
            &canary,
        )
        .unwrap_err();
    assert_eq!(error.kind, ProviderErrorKind::UnknownRemoteOutcome);
    assert!(!error.detail.contains(&canary));
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

fn scan_project_files_for_secret(root: &std::path::Path, secret: &str) -> (usize, usize) {
    let mut scanned = 0;
    let mut matches = 0;
    for entry in std::fs::read_dir(root).expect("read isolated project files") {
        let entry = entry.expect("enumerate isolated project files");
        let file_type = entry.file_type().expect("inspect isolated project file");
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let (nested_scanned, nested_matches) =
                scan_project_files_for_secret(&entry.path(), secret);
            scanned += nested_scanned;
            matches += nested_matches;
        } else if file_type.is_file() {
            scanned += 1;
            let contents = std::fs::read(entry.path()).expect("read isolated project file");
            matches += usize::from(
                contents
                    .windows(secret.len())
                    .any(|window| window == secret.as_bytes()),
            );
        }
    }
    (scanned, matches)
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
        budget_id: Some(budget_id.map(ToOwned::to_owned).unwrap_or_else(|| {
            ai_novel_production_system_core::runtime::DurableModelRunner::project_budget_id(
                "p01-formal",
            )
        })),
        predicted_cost_cents,
        cost_snapshot: None,
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
    assert!(error.contains("P01_MODEL_JSON_INVALID"));
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
        Err(error) if error.contains("BUDGET_EXCEEDED")
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
    assert_eq!(connection.query_row::<i64, _, _>("SELECT cost_cents FROM model_cost_ledger WHERE model_run_id=?1 AND cost_type='ACTUAL'", [&successful.model_run_id], |row| row.get(0)).unwrap(), 0);
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT json_extract(payload_json,'$.actualCostStatus') FROM production_events WHERE event_type='MODEL_USAGE_RECORDED' AND json_extract(payload_json,'$.modelRunId')=?1",
                [&successful.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        "UNPRICED"
    );

    let spent_id =
        StableIdentityFactory::from_command(StableIdKind::ModelRun, "p01-formal:spent").unwrap();
    connection.execute("INSERT INTO model_runs(model_run_id,project_id,provider,model,idempotency_key,capability,predicted_cost_cents,reserved_cost_cents,status,created_at,updated_at) VALUES (?1,'p01-formal','fixture','fixture-model','p01-formal-spent','RENDER_CHAPTER',15,15,'COMPLETED','2026-08-28T00:00:00Z','2026-08-28T00:00:00Z')", libsql_rusqlite::params![spent_id]).unwrap();
    connection.execute("INSERT INTO model_usage(usage_id,project_id,model_run_id,input_tokens,output_tokens,total_tokens,latency_ms,cost_cents,recorded_at) VALUES ('usage:p01:spent','p01-formal',?1,1,1,2,1,15,'2026-08-28T00:00:00Z')", libsql_rusqlite::params![spent_id]).unwrap();
    connection.execute("INSERT INTO model_cost_ledger(ledger_id,project_id,model_run_id,budget_id,cost_cents,token_total,cost_type,created_at) VALUES ('ledger:p01:spent','p01-formal',?1,'budget:p01:hard',15,2,'ACTUAL','2026-08-28T00:00:00Z')", libsql_rusqlite::params![spent_id]).unwrap();
    ai_novel_production_system_core::repositories::provider::ProviderSettingsRepository::write_project(
        &connection,
        "p01-formal",
        &json!({"chapterBudget":0.2}),
        "cmd:p01:set-chapter-budget",
        "2026-08-28T00:00:00Z",
    )
    .unwrap();
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
        Err(error) if error.contains("MODEL_PRICE_REQUIRED")
    ));
    assert_eq!(*calls.lock().unwrap(), 1);
}

#[test]
fn p01_formal_real_openai_compatible_usage_settles_after_exact_raw_persist() {
    let (temp, _manager, connection) = formal_project();
    seed_formal_budget(&connection, "budget:p01:real", 3, 20);
    let body = r#"{"id":"req:local","choices":[{"message":{"content":"{\"body\":\"ok\"}"}}],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8,"prompt_tokens_details":{"cached_tokens":2}}}"#;
    let (endpoint, calls) = local_provider_server_with_calls(200, body);
    let secrets = MemorySecretStore::new();
    let canary = runtime_secret_canary();
    secrets.put("fixture", &canary).unwrap();
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
    assert_eq!(
        connection
            .query_row::<i64, _, _>(
                "SELECT json_extract(payload_json,'$.cachedTokens') FROM production_events WHERE event_type='MODEL_USAGE_RECORDED' AND json_extract(payload_json,'$.modelRunId')=?1",
                [&spec.model_run_id],
                |row| row.get(0),
            )
            .unwrap(),
        2
    );
    let (project_files_scanned, project_canary_file_matches) =
        scan_project_files_for_secret(temp.path(), &canary);
    println!(
        "B_R2_05_PROJECT_SECRET_SCAN={{\"projectFilesScanned\":{},\"projectCanaryFileMatches\":{}}}",
        project_files_scanned, project_canary_file_matches
    );
    assert_eq!(project_canary_file_matches, 0);
}
