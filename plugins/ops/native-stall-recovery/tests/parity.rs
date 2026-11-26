use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use agentmesh_ops_kit::{check_parity, load_parity_cases, EXIT_INVALID_REQUEST};
use agentmesh_ops_native_stall_recovery::evaluate;
use serde_json::Value;

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

#[test]
fn python_parity() {
    check_parity(&testdata(), evaluate).unwrap();
}

#[test]
fn provider_decision_comes_from_the_caller() {
    let cases = load_parity_cases(&testdata()).unwrap();
    let mut request = cases
        .into_iter()
        .find(|case| case.name == "provider-free")
        .unwrap()
        .input;
    request["provider_match"] = false.into();
    assert_eq!(
        evaluate(request).unwrap()["classification"],
        "terminal_failure"
    );
}

#[test]
fn stdio_parity() {
    for case in load_parity_cases(&testdata()).unwrap() {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-native-stall-recovery"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(case.input.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let expected_code = if case.expected.get("error").is_some() {
            i32::from(EXIT_INVALID_REQUEST)
        } else {
            0
        };
        assert_eq!(output.status.code(), Some(expected_code), "{}", case.name);
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(actual, case.expected, "{}", case.name);
    }
}
