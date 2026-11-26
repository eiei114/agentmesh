use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use agentmesh_ops_free_model_benchmarks::handle;
use agentmesh_ops_kit::{check_parity, EXIT_INVALID_REQUEST};
use serde_json::Value;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

#[test]
fn python_parity() {
    assert!(check_parity(&fixtures(), handle).expect("Python scoring parity") >= 16);
}

fn stdio(input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-free-model-benchmarks"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start scoring binary");
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn stdio_returns_python_result() {
    let output = stdio(include_bytes!("../testdata/match-vendor-stale.input.json"));
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    assert!(output.stdout.ends_with(b"\n"));
    let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
    let expected: Value =
        serde_json::from_str(include_str!("../testdata/match-vendor-stale.expected.json")).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn stdio_rejects_invalid_requests() {
    for input in [
        &b"{"[..],
        br#"{"op":"fetch"}"#,
        br#"{"op":"number","value":true}"#,
        br#"{"op":"match_model","model":"m","metadata":{},"aliases":{},"sources":{}}"#,
    ] {
        let output = stdio(input);
        assert_eq!(output.status.code(), Some(i32::from(EXIT_INVALID_REQUEST)));
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["error"]["kind"], "invalid_request");
    }
}
