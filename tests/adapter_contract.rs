use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

#[test]
fn spec_cli_outputs_protocol_neutral_identity_fields() {
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
    let add_out = String::from_utf8_lossy(&add.stdout);
    assert!(add_out.contains("kind=openapi"));
    assert!(add_out.contains("declared_version=3.0.0"));
    assert!(add_out.contains("format=json"));
    assert!(!add_out.contains("spec_version="));

    let list = run_with_registry(temp.path(), ["spec", "list"]);

    assert!(list.status.success());
    let list_out = String::from_utf8_lossy(&list.stdout);
    assert!(list_out.contains("service\tactive\tdigest\tsource_format\tspec_kind\tdeclared_version\tsource"));
    assert!(list_out.contains("\tyes\t"));
    assert!(list_out.contains("\tjson\topenapi\t3.0.0\t"));
    assert!(!list_out.contains("spec_version"));
}

fn run_with_registry<const N: usize>(registry_root: &Path, args: [&str; N]) -> std::process::Output {
    let binary = env!("CARGO_BIN_EXE_specgate");
    Command::new(binary)
        .args(args)
        .env("SPECGATE_REGISTRY_DIR", registry_root)
        .output()
        .expect("expected command execution")
}
