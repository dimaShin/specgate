use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::thread;

use tempfile::tempdir;

#[test]
fn add_file_then_list_succeeds() {
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
    assert!(String::from_utf8_lossy(&add.stdout).contains("added spec: service=pet"));

    let list = run_with_registry(temp.path(), ["spec", "list"]);

    assert!(list.status.success());
    let output = String::from_utf8_lossy(&list.stdout);
    assert!(output.contains("service\tactive\tdigest\tsource_format\tspec_kind\tdeclared_version\tsource"));
    assert!(output.contains("pet"));
    assert!(output.contains("\tyes\t"));
    assert!(output.contains("openapi"));
    assert!(output.contains("3.0.0"));
}

#[test]
fn add_second_spec_auto_activates_latest() {
    let temp = tempdir().expect("expected temp dir");
    let first_spec = temp.path().join("petstore-v1.json");
    let second_spec = temp.path().join("petstore-v2.json");

    std::fs::write(
        &first_spec,
        r#"{"openapi":"3.0.0","info":{"title":"pet","version":"1"},"paths":{}}"#,
    )
    .expect("expected first spec file write");
    std::fs::write(
        &second_spec,
        r#"{"openapi":"3.1.0","info":{"title":"pet","version":"2"},"paths":{}}"#,
    )
    .expect("expected second spec file write");

    let first_add = run_with_registry(
        temp.path(),
        [
            "spec",
            "add",
            "--service",
            "pet",
            "--file",
            first_spec.to_str().expect("expected file path"),
        ],
    );
    assert!(first_add.status.success());

    let second_add = run_with_registry(
        temp.path(),
        [
            "spec",
            "add",
            "--service",
            "pet",
            "--file",
            second_spec.to_str().expect("expected file path"),
        ],
    );
    assert!(second_add.status.success());

    let second_digest = String::from_utf8_lossy(&second_add.stdout)
        .split_whitespace()
        .find_map(|token| token.strip_prefix("digest="))
        .expect("expected digest in add output")
        .to_string();

    let list = run_with_registry(temp.path(), ["spec", "list"]);
    assert!(list.status.success());

    let output = String::from_utf8_lossy(&list.stdout);
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 3, "expected header and two entries");

    let active_lines: Vec<&str> = lines
        .iter()
        .skip(1)
        .copied()
        .filter(|line| line.contains("\tyes\t"))
        .collect();
    assert_eq!(active_lines.len(), 1, "expected exactly one active spec");
    assert!(active_lines[0].contains(&second_digest));
}

#[test]
fn add_url_with_bearer_auth_succeeds() {
    let temp = tempdir().expect("expected temp dir");
    let listener = TcpListener::bind("127.0.0.1:0").expect("expected listener");
    let address = listener.local_addr().expect("expected listener addr");

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("expected client connection");
        let mut buffer = [0u8; 4096];
        let read = stream.read(&mut buffer).expect("expected request read");
        let request = String::from_utf8_lossy(&buffer[..read]);

        assert!(request.contains("Authorization: Bearer dev-token"));

        let body = r#"{"openapi":"3.1.0","info":{"title":"svc","version":"1"},"paths":{}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("expected response write");
    });

    let add = run_with_registry(
        temp.path(),
        [
            "spec",
            "add",
            "--service",
            "svc",
            "--url",
            &format!("http://{address}/openapi.json"),
            "--auth-bearer",
            "dev-token",
        ],
    );

    server.join().expect("expected server thread to finish");

    assert!(add.status.success());
    assert!(String::from_utf8_lossy(&add.stdout).contains("declared_version=3.1.0"));
}

#[test]
fn rejects_swagger2_spec() {
    let temp = tempdir().expect("expected temp dir");
    let spec_file = temp.path().join("swagger2.json");
    std::fs::write(
        &spec_file,
        r#"{"swagger":"2.0","info":{"title":"pet","version":"1"},"paths":{}}"#,
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

    assert_eq!(add.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&add.stderr).contains("OpenAPI 2.0 (Swagger)"));
}

fn run_with_registry<const N: usize>(registry_root: &Path, args: [&str; N]) -> std::process::Output {
    let binary = env!("CARGO_BIN_EXE_specgate");
    Command::new(binary)
        .args(args)
        .env("SPECGATE_REGISTRY_DIR", registry_root)
        .output()
        .expect("expected command execution")
}
