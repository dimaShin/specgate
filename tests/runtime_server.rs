use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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

fn start_runtime(
    registry_root: &Path,
    config_path: &Path,
    upstream_url: Option<&str>,
    listen_addr: &str,
    mode: &str,
    validation_mode: &str,
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

    if let Some(url) = upstream_url {
        command.args(["--upstream", url]);
    }

    command
        .env("SPECGATE_REGISTRY_DIR", registry_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("expected runtime process start")
}

#[test]
fn runtime_serve_mock_mode_returns_fixture_without_upstream() {
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
        fixture_dir.join("GET__pet__health.json"),
        r#"{
            "status": 200,
            "headers": {"content-type": "application/json"},
            "body": "{\"source\":\"fixture\"}"
        }"#,
    )
    .expect("expected fixture write");

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
    assert!(response.contains("{\"source\":\"fixture\"}"));
    assert!(response.contains("x-specgate-mock: hit"));
    assert!(response.contains("x-specgate-validation: ok"));

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());
}

#[test]
fn runtime_serve_mock_mode_returns_404_when_fixture_missing() {
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
    assert!(response.contains("x-specgate-mock: miss"));

    let output = runtime
        .wait_with_output()
        .expect("expected runtime process completion");
    assert!(output.status.success());
}

#[test]
fn runtime_serve_mock_partial_falls_back_to_upstream_when_fixture_missing() {
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
