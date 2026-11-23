use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use agentmesh_ops_daily_delivery_common::handle;
use agentmesh_ops_kit::{check_parity, load_parity_cases, EXIT_INVALID_REQUEST};
use serde_json::{json, Value};

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

#[test]
fn matches_python_fixtures() {
    check_parity(&testdata(), handle).unwrap();
}

fn stdio(input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-daily-delivery-common"))
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
    for input in [
        "not json",
        r#"{"op":"unknown"}"#,
        r#"{"op":"daily_target","remaining":1,"now":"2026-01-01T00:00:00","deadline":"2026-01-02T00:00:00Z"}"#,
        r#"{"op":"managed_delivery_scope","project":null,"policy":{}} {}"#,
    ] {
        let output = stdio(input.as_bytes());
        assert_eq!(output.status.code(), Some(i32::from(EXIT_INVALID_REQUEST)));
        assert!(!output.stderr.is_empty());
        let document: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(document["error"]["kind"], json!("invalid_request"));
    }
}
