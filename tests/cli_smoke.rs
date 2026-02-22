use std::process::Command;

#[test]
fn version_command_prints_expected_output() {
    let binary = env!("CARGO_BIN_EXE_specgate");

    let output = Command::new(binary)
        .arg("version")
        .output()
        .expect("expected to execute specgate binary");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("specgate {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unknown_command_exits_with_code_two() {
    let binary = env!("CARGO_BIN_EXE_specgate");

    let output = Command::new(binary)
        .arg("unknown")
        .output()
        .expect("expected to execute specgate binary");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown command: unknown"));
}
