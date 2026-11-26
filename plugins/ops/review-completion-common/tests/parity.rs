use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use agentmesh_ops_kit::{check_parity, load_parity_cases, EXIT_INVALID_REQUEST};
use agentmesh_ops_review_completion_common::handle;
use serde_json::{json, Value};

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

#[test]
fn matches_python_fixtures() {
    check_parity(&testdata(), handle).unwrap();
}

fn stdio(input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-review-completion-common"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn binary_returns_one_result_per_request() {
    for case in load_parity_cases(&testdata()).unwrap() {
        let output = stdio(&serde_json::to_vec(&case.input).unwrap());
        assert!(output.status.success(), "{}: {output:?}", case.name);
        assert!(output.stderr.is_empty(), "{}: {output:?}", case.name);
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            case.expected,
            "{}",
            case.name
        );
    }
}

#[test]
fn binary_rejects_invalid_wire_requests() {
    for input in ["not json", "{}", r#"{"context":{}} {}"#] {
        let output = stdio(input.as_bytes());
        assert_eq!(output.status.code(), Some(i32::from(EXIT_INVALID_REQUEST)));
        assert!(!output.stderr.is_empty());
        let document: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(document["error"]["kind"], json!("invalid_request"));
    }
}

#[test]
fn binary_does_not_echo_rejected_request_values() {
    let marker = "synthetic-request-value";
    let input = json!({"receipt": {}, "context": {"producer_runs": marker}});
    let output = stdio(&serde_json::to_vec(&input).unwrap());

    assert_eq!(output.status.code(), Some(i32::from(EXIT_INVALID_REQUEST)));
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        (stdout.contains(marker), stderr.contains(marker)),
        (false, false),
        "neither stdout nor stderr should echo the rejected value"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&stdout).unwrap(),
        json!({"error": {"kind": "invalid_request", "message": "invalid subject request"}})
    );
    assert_eq!(stderr.trim_end(), "invalid subject request");
}
