use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use agentmesh_ops_kit::{check_parity, load_parity_cases, EXIT_INVALID_REQUEST};
use agentmesh_ops_review_queue_common::handle;
use serde_json::Value;

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

fn stdio(input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-review-queue-common"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn library_matches_python() {
    let count = check_parity(&testdata(), handle).unwrap();
    assert!(count >= 4);
}

#[test]
fn binary_matches_python() {
    for case in load_parity_cases(&testdata()).unwrap() {
        let output = stdio(&serde_json::to_vec(&case.input).unwrap());
        assert!(output.status.success(), "{}: {:?}", case.name, output);
        assert!(output.stderr.is_empty(), "{}", case.name);
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(actual, case.expected, "{}", case.name);
    }
}

#[test]
fn binary_rejects_invalid_requests() {
    for input in [
        "not json",
        "{}",
        r#"{"rows":[]} {"rows":[]}"#,
        r#"{"rows":[],"complete":"yes"}"#,
        r#"{"rows":[],"deferred":-1}"#,
        r#"{"rows":[{"role":"child","project":null,"key":null,"reason":null,"issue_age_hours":[]}]}"#,
    ] {
        let output = stdio(input.as_bytes());
        assert_eq!(output.status.code(), Some(i32::from(EXIT_INVALID_REQUEST)));
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(actual["error"]["kind"], "invalid_request", "{input}");
        assert!(!output.stderr.is_empty());
    }
}
