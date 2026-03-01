use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use tempfile::tempdir;

#[test]
fn runtime_serve_proxies_request_and_exits_after_max_requests() {
    let temp = tempdir().expect("expected temp dir");
    let spec_file = temp.path().join("petstore.json");
    std::fs::write(
        &spec_file,
        r#"{"openapi":"3.0.0","info":{"title":"pet","version":"1"},"paths":{}}"#,
    )
    .expect("expected spec file write");

    let add = run_with_registry(
        temp.path(),
        [
            "spec",
            "add",
            "--service",
            "pet",
            "--file",
            spec_file.to_str().expect("expected file path"),
        ],
    );
    assert!(add.status.success());

    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let upstream_listener = TcpListener::bind("127.0.0.1:0").expect("expected upstream listener");
    let upstream_addr = upstream_listener
        .local_addr()
        .expect("expected upstream address");

    let upstream_thread = thread::spawn(move || {
        let (mut stream, _) = upstream_listener.accept().expect("expected upstream request");
        let mut buffer = [0u8; 4096];
        let read = stream.read(&mut buffer).expect("expected upstream read");
        let request = String::from_utf8_lossy(&buffer[..read]);

        assert!(request.starts_with("GET /pet/health HTTP/1.1"));

        let body = "upstream-ok";
        let response = format!(
            "HTTP/1.1 201 Created\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("expected upstream write");
    });

    let runtime_port = find_free_port();
    let runtime_addr = format!("127.0.0.1:{runtime_port}");
    let runtime = start_runtime(
        temp.path(),
        &config_file,
        Some(&format!("http://{upstream_addr}")),
        &runtime_addr,
        "proxy",
        "warn",
    );

    let response = send_request_with_retry(
        &runtime_addr,
        "GET /pet/health HTTP/1.1\r\nHost: runtime\r\nConnection: close\r\n\r\n",
    );

    assert!(response.starts_with("HTTP/1.1 201"));
    assert!(response.contains("upstream-ok"));
    assert!(response.contains("x-specgate-service: pet"));
    assert!(response.contains("x-specgate-spec-kind: openapi"));
    assert!(response.contains("x-specgate-validation: warn"));

    upstream_thread
        .join()
        .expect("expected upstream thread completion");

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("runtime stopped after handling 1 request(s)"));
}

#[test]
fn runtime_serve_strict_mode_fails_on_response_validation_error() {
    let temp = tempdir().expect("expected temp dir");
    let spec_file = temp.path().join("petstore.json");
    std::fs::write(
        &spec_file,
        r#"{
            "openapi":"3.0.0",
            "info":{"title":"pet","version":"1"},
            "paths":{
                "/pet/health":{
                    "get":{
                        "responses":{"200":{"description":"ok"}}
                    }
                }
            }
        }"#,
    )
    .expect("expected spec file write");

    let add = run_with_registry(
        temp.path(),
        [
            "spec",
            "add",
            "--service",
            "pet",
            "--file",
            spec_file.to_str().expect("expected file path"),
        ],
    );
    assert!(add.status.success());

    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let upstream_listener = TcpListener::bind("127.0.0.1:0").expect("expected upstream listener");
    let upstream_addr = upstream_listener
        .local_addr()
        .expect("expected upstream address");

    let upstream_thread = thread::spawn(move || {
        let (mut stream, _) = upstream_listener.accept().expect("expected upstream request");
        let mut buffer = [0u8; 4096];
        let _ = stream.read(&mut buffer).expect("expected upstream read");

        let body = "created";
        let response = format!(
            "HTTP/1.1 201 Created\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("expected upstream write");
    });

    let runtime_port = find_free_port();
    let runtime_addr = format!("127.0.0.1:{runtime_port}");
    let runtime = start_runtime(
        temp.path(),
        &config_file,
        Some(&format!("http://{upstream_addr}")),
        &runtime_addr,
        "proxy",
        "strict",
    );

    let response = send_request_with_retry(
        &runtime_addr,
        "GET /pet/health HTTP/1.1\r\nHost: runtime\r\nConnection: close\r\n\r\n",
    );

    assert!(response.starts_with("HTTP/1.1 502"));
    assert!(response.contains("x-specgate-validation: error"));
    assert!(response.contains("response status 201 is not declared"));

    upstream_thread
        .join()
        .expect("expected upstream thread completion");

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());
}

fn run_with_registry<const N: usize>(registry_root: &Path, args: [&str; N]) -> std::process::Output {
    let binary = env!("CARGO_BIN_EXE_specgate");
    Command::new(binary)
        .args(args)
        .env("SPECGATE_REGISTRY_DIR", registry_root)
        .output()
        .expect("expected command execution")
}

#[test]
fn runtime_init_mocks_generates_service_manifests() {
    let temp = tempdir().expect("expected temp dir");
    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n  - prefix: /pet/admin\n    service_id: pet\n  - prefix: /shop\n    service_id: shop\n",
    )
    .expect("expected runtime config write");

    let output = run_with_registry(
        temp.path(),
        [
            "runtime",
            "init-mocks",
            "--config",
            config_file.to_str().expect("expected config path"),
        ],
    );
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("initialized 2 service manifest(s)"));

    let pet_manifest = temp.path().join("mocks").join("pet").join("scenarios.yaml");
    let shop_manifest = temp.path().join("mocks").join("shop").join("scenarios.yaml");
    assert!(pet_manifest.exists());
    assert!(shop_manifest.exists());
}

fn start_runtime(
    registry_root: &Path,
    config_path: &Path,
    upstream_url: Option<&str>,
    listen_addr: &str,
    mode: &str,
    validation_mode: &str,
) -> Child {
    start_runtime_with_fallback(
        registry_root,
        config_path,
        upstream_url,
        listen_addr,
        mode,
        validation_mode,
        None,
    )
}

fn start_runtime_with_fallback(
    registry_root: &Path,
    config_path: &Path,
    upstream_url: Option<&str>,
    listen_addr: &str,
    mode: &str,
    validation_mode: &str,
    mock_partial_fallback: Option<&str>,
) -> Child {
    start_runtime_with_options(
        registry_root,
        config_path,
        upstream_url,
        listen_addr,
        mode,
        validation_mode,
        mock_partial_fallback,
        &[],
    )
}

fn start_runtime_with_options(
    registry_root: &Path,
    config_path: &Path,
    upstream_url: Option<&str>,
    listen_addr: &str,
    mode: &str,
    validation_mode: &str,
    mock_partial_fallback: Option<&str>,
    extra_env: &[(&str, &str)],
) -> Child {
    let binary = env!("CARGO_BIN_EXE_specgate");
    let mut command = Command::new(binary);
    command
        .args([
            "runtime",
            "serve",
            "--config",
            config_path.to_str().expect("expected config path"),
            "--mode",
            mode,
            "--listen",
            listen_addr,
            "--max-requests",
            "1",
            "--validation-mode",
            validation_mode,
        ]);

    if let Some(policy) = mock_partial_fallback {
        command.args(["--mock-partial-fallback", policy]);
    }

    if let Some(url) = upstream_url {
        command.args(["--upstream", url]);
    }

    command
        .env("SPECGATE_REGISTRY_DIR", registry_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        ;

    for (key, value) in extra_env {
        command.env(key, value);
    }

    command.spawn().expect("expected runtime process start")
}

#[test]
fn runtime_serve_mock_mode_returns_scenario_response_without_upstream() {
    let temp = tempdir().expect("expected temp dir");
    let spec_file = temp.path().join("petstore.json");
    std::fs::write(
        &spec_file,
        r#"{
            "openapi":"3.0.0",
            "info":{"title":"pet","version":"1"},
            "paths":{"/pet/health":{"get":{"responses":{"200":{"description":"ok"}}}}}
        }"#,
    )
    .expect("expected spec file write");

    let add = run_with_registry(
        temp.path(),
        [
            "spec",
            "add",
            "--service",
            "pet",
            "--file",
            spec_file.to_str().expect("expected file path"),
        ],
    );
    assert!(add.status.success());

    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let fixture_dir = temp.path().join("mocks").join("pet");
    std::fs::create_dir_all(&fixture_dir).expect("expected fixture directory create");
    std::fs::write(
        fixture_dir.join("scenarios.yaml"),
        "scenarios:\n  - id: health\n    priority: 10\n    when:\n      method: GET\n      path: /pet/health\n    respond:\n      status: 200\n      headers:\n        content-type: application/json\n      body: '{\"source\":\"scenario\"}'\n",
    )
    .expect("expected scenario write");

    let runtime_port = find_free_port();
    let runtime_addr = format!("127.0.0.1:{runtime_port}");
    let runtime = start_runtime(
        temp.path(),
        &config_file,
        None,
        &runtime_addr,
        "mock",
        "warn",
    );

    let response = send_request_with_retry(
        &runtime_addr,
        "GET /pet/health HTTP/1.1\r\nHost: runtime\r\nConnection: close\r\n\r\n",
    );

    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("{\"source\":\"scenario\"}"));
    assert!(response.contains("x-specgate-mock: hit"));
    assert!(response.contains("x-specgate-validation: ok"));

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());
}

#[test]
fn runtime_serve_mock_mode_returns_404_when_manifest_missing() {
    let temp = tempdir().expect("expected temp dir");
    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let runtime_port = find_free_port();
    let runtime_addr = format!("127.0.0.1:{runtime_port}");
    let runtime = start_runtime(
        temp.path(),
        &config_file,
        None,
        &runtime_addr,
        "mock",
        "warn",
    );

    let response = send_request_with_retry(
        &runtime_addr,
        "GET /pet/health HTTP/1.1\r\nHost: runtime\r\nConnection: close\r\n\r\n",
    );

    assert!(response.starts_with("HTTP/1.1 404"));
    assert!(response.contains("no matching scenario"));
    assert!(response.contains("x-specgate-mock: miss"));

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());
}

#[test]
fn runtime_serve_auto_generates_manifest_when_missing() {
    let temp = tempdir().expect("expected temp dir");
    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let runtime_port = find_free_port();
    let runtime_addr = format!("127.0.0.1:{runtime_port}");
    let runtime = start_runtime(
        temp.path(),
        &config_file,
        None,
        &runtime_addr,
        "mock",
        "warn",
    );

    let response = send_request_with_retry(
        &runtime_addr,
        "GET /pet/health HTTP/1.1\r\nHost: runtime\r\nConnection: close\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 404"));

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());

    let generated = temp
        .path()
        .join("mocks")
        .join("pet")
        .join("scenarios.yaml");
    assert!(generated.exists());

    let content = std::fs::read_to_string(generated).expect("expected generated manifest content");
    assert!(content.contains("Auto-generated by specgate"));
    assert!(content.contains("scenarios: []"));
}

#[test]
fn runtime_serve_mock_partial_falls_back_to_upstream_when_manifest_missing() {
    let temp = tempdir().expect("expected temp dir");
    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let upstream_listener = TcpListener::bind("127.0.0.1:0").expect("expected upstream listener");
    let upstream_addr = upstream_listener
        .local_addr()
        .expect("expected upstream address");

    let upstream_thread = thread::spawn(move || {
        let (mut stream, _) = upstream_listener.accept().expect("expected upstream request");
        let mut buffer = [0u8; 4096];
        let read = stream.read(&mut buffer).expect("expected upstream read");
        let request = String::from_utf8_lossy(&buffer[..read]);
        assert!(request.starts_with("GET /pet/health HTTP/1.1"));

        let body = "fallback-ok";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("expected upstream write");
    });

    let runtime_port = find_free_port();
    let runtime_addr = format!("127.0.0.1:{runtime_port}");
    let runtime = start_runtime(
        temp.path(),
        &config_file,
        Some(&format!("http://{upstream_addr}")),
        &runtime_addr,
        "mock-partial",
        "warn",
    );

    let response = send_request_with_retry(
        &runtime_addr,
        "GET /pet/health HTTP/1.1\r\nHost: runtime\r\nConnection: close\r\n\r\n",
    );

    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("fallback-ok"));
    assert!(response.contains("x-specgate-mock: fallback"));

    upstream_thread
        .join()
        .expect("expected upstream thread completion");

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());
}

#[test]
fn runtime_serve_mock_mode_uses_scenario_manifest_query_matching() {
    let temp = tempdir().expect("expected temp dir");
    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let service_mock_dir = temp.path().join("mocks").join("pet");
    std::fs::create_dir_all(&service_mock_dir).expect("expected service mock dir");
    std::fs::write(
        service_mock_dir.join("scenarios.yaml"),
        "scenarios:\n  - id: health-default\n    priority: 1\n    when:\n      method: GET\n      path: /pet/health\n    respond:\n      status: 200\n      headers:\n        content-type: application/json\n      body: '{\"state\":\"default\"}'\n  - id: health-ready\n    priority: 10\n    when:\n      method: GET\n      path: /pet/health\n      query:\n        state: ready\n    respond:\n      status: 200\n      headers:\n        content-type: application/json\n      body: '{\"state\":\"ready\"}'\n",
    )
    .expect("expected scenario manifest write");

    let runtime_port = find_free_port();
    let runtime_addr = format!("127.0.0.1:{runtime_port}");
    let runtime = start_runtime(
        temp.path(),
        &config_file,
        None,
        &runtime_addr,
        "mock",
        "warn",
    );

    let response = send_request_with_retry(
        &runtime_addr,
        "GET /pet/health?state=ready HTTP/1.1\r\nHost: runtime\r\nConnection: close\r\n\r\n",
    );

    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("{\"state\":\"ready\"}"));
    assert!(response.contains("x-specgate-mock: hit"));

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());
}

#[test]
fn runtime_serve_mock_partial_structural_policy_does_not_fallback_on_attribute_mismatch() {
    let temp = tempdir().expect("expected temp dir");
    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let service_mock_dir = temp.path().join("mocks").join("pet");
    std::fs::create_dir_all(&service_mock_dir).expect("expected service mock dir");
    std::fs::write(
        service_mock_dir.join("scenarios.yaml"),
        "scenarios:\n  - id: health-ready\n    priority: 10\n    when:\n      method: GET\n      path: /pet/health\n      query:\n        state: ready\n    respond:\n      status: 200\n      body: '{\"state\":\"ready\"}'\n",
    )
    .expect("expected scenario manifest write");

    let runtime_port = find_free_port();
    let runtime_addr = format!("127.0.0.1:{runtime_port}");
    let runtime = start_runtime_with_fallback(
        temp.path(),
        &config_file,
        Some("http://127.0.0.1:1"),
        &runtime_addr,
        "mock-partial",
        "warn",
        Some("structural"),
    );

    let response = send_request_with_retry(
        &runtime_addr,
        "GET /pet/health?state=missing HTTP/1.1\r\nHost: runtime\r\nConnection: close\r\n\r\n",
    );

    assert!(response.starts_with("HTTP/1.1 404"));
    assert!(response.contains("x-specgate-mock: miss"));

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());
}

#[test]
fn runtime_serve_mock_partial_any_policy_falls_back_on_attribute_mismatch() {
    let temp = tempdir().expect("expected temp dir");
    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let service_mock_dir = temp.path().join("mocks").join("pet");
    std::fs::create_dir_all(&service_mock_dir).expect("expected service mock dir");
    std::fs::write(
        service_mock_dir.join("scenarios.yaml"),
        "scenarios:\n  - id: health-ready\n    priority: 10\n    when:\n      method: GET\n      path: /pet/health\n      query:\n        state: ready\n    respond:\n      status: 200\n      body: '{\"state\":\"ready\"}'\n",
    )
    .expect("expected scenario manifest write");

    let upstream_listener = TcpListener::bind("127.0.0.1:0").expect("expected upstream listener");
    let upstream_addr = upstream_listener
        .local_addr()
        .expect("expected upstream address");

    let upstream_thread = thread::spawn(move || {
        let (mut stream, _) = upstream_listener.accept().expect("expected upstream request");
        let mut buffer = [0u8; 4096];
        let _ = stream.read(&mut buffer).expect("expected upstream read");

        let body = "fallback-any";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("expected upstream write");
    });

    let runtime_port = find_free_port();
    let runtime_addr = format!("127.0.0.1:{runtime_port}");
    let runtime = start_runtime_with_fallback(
        temp.path(),
        &config_file,
        Some(&format!("http://{upstream_addr}")),
        &runtime_addr,
        "mock-partial",
        "warn",
        Some("any"),
    );

    let response = send_request_with_retry(
        &runtime_addr,
        "GET /pet/health?state=missing HTTP/1.1\r\nHost: runtime\r\nConnection: close\r\n\r\n",
    );

    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("fallback-any"));
    assert!(response.contains("x-specgate-mock: fallback"));

    upstream_thread
        .join()
        .expect("expected upstream thread completion");

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());
}

#[test]
fn runtime_serve_mock_mode_applies_persisted_state_transitions() {
    let temp = tempdir().expect("expected temp dir");
    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let service_mock_dir = temp.path().join("mocks").join("pet");
    std::fs::create_dir_all(&service_mock_dir).expect("expected service mock dir");
    std::fs::write(
        service_mock_dir.join("scenarios.yaml"),
        "scenarios:\n  - id: init\n    priority: 20\n    when:\n      method: GET\n      path: /pet/step\n      query:\n        stage: init\n    state:\n      key: flow\n      set: started\n    respond:\n      status: 200\n      body: '{\"step\":\"init\"}'\n  - id: next\n    priority: 10\n    when:\n      method: GET\n      path: /pet/step\n      query:\n        stage: next\n    state:\n      key: flow\n      requires: started\n    respond:\n      status: 200\n      body: '{\"step\":\"next\"}'\n",
    )
    .expect("expected scenario manifest write");

    let first_port = find_free_port();
    let first_addr = format!("127.0.0.1:{first_port}");
    let first_runtime = start_runtime(
        temp.path(),
        &config_file,
        None,
        &first_addr,
        "mock",
        "warn",
    );

    let first_response = send_request_with_retry(
        &first_addr,
        "GET /pet/step?stage=init HTTP/1.1\r\nHost: runtime\r\nConnection: close\r\n\r\n",
    );
    assert!(first_response.starts_with("HTTP/1.1 200"));
    assert!(first_response.contains("{\"step\":\"init\"}"));

    let first_output = first_runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(first_output.status.success());

    let second_port = find_free_port();
    let second_addr = format!("127.0.0.1:{second_port}");
    let second_runtime = start_runtime(
        temp.path(),
        &config_file,
        None,
        &second_addr,
        "mock",
        "warn",
    );

    let second_response = send_request_with_retry(
        &second_addr,
        "GET /pet/step?stage=next HTTP/1.1\r\nHost: runtime\r\nConnection: close\r\n\r\n",
    );
    assert!(second_response.starts_with("HTTP/1.1 200"));
    assert!(second_response.contains("{\"step\":\"next\"}"));

    let second_output = second_runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(second_output.status.success());

    let state_file = temp.path().join("mocks").join("state").join("pet.json");
    let state_content = std::fs::read_to_string(&state_file).expect("expected state file");
    assert!(state_content.contains("\"flow\": \"started\""));
}

#[test]
fn runtime_serve_mock_mode_matches_auth_claims_from_jwt() {
    let temp = tempdir().expect("expected temp dir");
    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let service_mock_dir = temp.path().join("mocks").join("pet");
    std::fs::create_dir_all(&service_mock_dir).expect("expected service mock dir");
    std::fs::write(
        service_mock_dir.join("scenarios.yaml"),
        "scenarios:\n  - id: admin-read\n    priority: 50\n    when:\n      method: GET\n      path: /pet/secure\n      auth:\n        subject: user-42\n        roles: [admin]\n        attributes:\n          tenant: acme\n    respond:\n      status: 200\n      body: '{\"auth\":\"ok\"}'\n",
    )
    .expect("expected scenario manifest write");

    let jwt = test_jwt(&serde_json::json!({
        "sub": "user-42",
        "roles": ["admin", "editor"],
        "tenant": "acme"
    }));

    let runtime_port = find_free_port();
    let runtime_addr = format!("127.0.0.1:{runtime_port}");
    let runtime = start_runtime(
        temp.path(),
        &config_file,
        None,
        &runtime_addr,
        "mock",
        "warn",
    );

    let request = format!(
        "GET /pet/secure HTTP/1.1\r\nHost: runtime\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        jwt
    );
    let response = send_request_with_retry(&runtime_addr, &request);
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("{\"auth\":\"ok\"}"));

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());
}

#[test]
fn runtime_serve_mock_mode_invalid_jwt_does_not_crash_and_misses_auth_match() {
    let temp = tempdir().expect("expected temp dir");
    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let service_mock_dir = temp.path().join("mocks").join("pet");
    std::fs::create_dir_all(&service_mock_dir).expect("expected service mock dir");
    std::fs::write(
        service_mock_dir.join("scenarios.yaml"),
        "scenarios:\n  - id: admin-read\n    priority: 50\n    when:\n      method: GET\n      path: /pet/secure\n      auth:\n        subject: user-42\n    respond:\n      status: 200\n      body: '{\"auth\":\"ok\"}'\n",
    )
    .expect("expected scenario manifest write");

    let runtime_port = find_free_port();
    let runtime_addr = format!("127.0.0.1:{runtime_port}");
    let runtime = start_runtime(
        temp.path(),
        &config_file,
        None,
        &runtime_addr,
        "mock",
        "warn",
    );

    let response = send_request_with_retry(
        &runtime_addr,
        "GET /pet/secure HTTP/1.1\r\nHost: runtime\r\nAuthorization: Bearer broken.token.value\r\nConnection: close\r\n\r\n",
    );
    assert!(response.starts_with("HTTP/1.1 404"));
    assert!(response.contains("x-specgate-mock: miss"));

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());
}

#[test]
fn runtime_serve_mock_mode_matches_expr_and_body_json_predicates() {
    let temp = tempdir().expect("expected temp dir");
    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let service_mock_dir = temp.path().join("mocks").join("pet");
    std::fs::create_dir_all(&service_mock_dir).expect("expected service mock dir");
    std::fs::write(
        service_mock_dir.join("scenarios.yaml"),
        "scenarios:\n  - id: expr-body\n    priority: 10\n    when:\n      method: POST\n      path: /pet/orders\n      body_json:\n        customer.tier: gold\n      expr: query.mode == \"sync\" && header.x-region == \"eu\" && body.customer.id == \"42\"\n    respond:\n      status: 201\n      body: '{\"matched\":true}'\n",
    )
    .expect("expected scenario manifest write");

    let runtime_port = find_free_port();
    let runtime_addr = format!("127.0.0.1:{runtime_port}");
    let runtime = start_runtime(
        temp.path(),
        &config_file,
        None,
        &runtime_addr,
        "mock",
        "warn",
    );

    let body = "{\"customer\":{\"id\":42,\"tier\":\"gold\"}}";
    let request = format!(
        "POST /pet/orders?mode=sync HTTP/1.1\r\nHost: runtime\r\nX-Region: eu\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let response = send_request_with_retry(&runtime_addr, &request);
    assert!(response.starts_with("HTTP/1.1 201"));
    assert!(response.contains("{\"matched\":true}"));

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());
}

#[test]
fn runtime_serve_mock_mode_token_fingerprint_matches_only_when_enabled() {
    let temp = tempdir().expect("expected temp dir");
    let config_file = temp.path().join("runtime.yaml");
    std::fs::write(
        &config_file,
        "service_routes:\n  - prefix: /pet\n    service_id: pet\n",
    )
    .expect("expected runtime config write");

    let token = test_jwt(&serde_json::json!({"sub": "fp-user"}));
    let salt = "specgate-test-salt";
    let fingerprint = token_fingerprint_for_test(&token, salt);

    let service_mock_dir = temp.path().join("mocks").join("pet");
    std::fs::create_dir_all(&service_mock_dir).expect("expected service mock dir");
    std::fs::write(
        service_mock_dir.join("scenarios.yaml"),
        format!(
            "scenarios:\n  - id: fp\n    priority: 10\n    when:\n      method: GET\n      path: /pet/fp\n      auth:\n        token_fingerprint: {}\n    respond:\n      status: 200\n      body: '{{\"fingerprint\":\"ok\"}}'\n",
            fingerprint
        ),
    )
    .expect("expected scenario manifest write");

    let first_port = find_free_port();
    let first_addr = format!("127.0.0.1:{first_port}");
    let first_runtime = start_runtime(
        temp.path(),
        &config_file,
        None,
        &first_addr,
        "mock",
        "warn",
    );

    let first_request = format!(
        "GET /pet/fp HTTP/1.1\r\nHost: runtime\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        token
    );
    let first_response = send_request_with_retry(&first_addr, &first_request);
    assert!(first_response.starts_with("HTTP/1.1 404"));

    let first_output = first_runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(first_output.status.success());

    let second_port = find_free_port();
    let second_addr = format!("127.0.0.1:{second_port}");
    let second_runtime = start_runtime_with_options(
        temp.path(),
        &config_file,
        None,
        &second_addr,
        "mock",
        "warn",
        None,
        &[
            ("SPECGATE_TOKEN_FINGERPRINT", "1"),
            ("SPECGATE_TOKEN_FINGERPRINT_SALT", salt),
        ],
    );

    let second_request = format!(
        "GET /pet/fp HTTP/1.1\r\nHost: runtime\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        token
    );
    let second_response = send_request_with_retry(&second_addr, &second_request);
    assert!(second_response.starts_with("HTTP/1.1 200"));
    assert!(second_response.contains("{\"fingerprint\":\"ok\"}"));

    let second_output = second_runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(second_output.status.success());
}

fn test_jwt(payload: &serde_json::Value) -> String {
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(r#"{"alg":"none","typ":"JWT"}"#);
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(payload).expect("expected payload bytes"));
    format!("{}.{}.", header, payload)
}

fn token_fingerprint_for_test(token: &str, salt: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update(token.as_bytes());
    let digest = hasher.finalize();
    format!("sha256:{:x}", digest)
}

fn send_request_with_retry(address: &str, request: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match TcpStream::connect(address) {
            Ok(mut stream) => {
                stream
                    .write_all(request.as_bytes())
                    .expect("expected request write");
                let mut response = String::new();
                stream
                    .read_to_string(&mut response)
                    .expect("expected response read");
                return response;
            }
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Err(error) => panic!("failed to connect to runtime server: {error}"),
        }
    }
}

fn find_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("expected temporary listener")
        .local_addr()
        .expect("expected local addr")
        .port()
}
