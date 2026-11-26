use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use agentmesh_ops_kit::check_parity;
use agentmesh_ops_reconcile_review_queue::handle;
use serde_json::{json, Value};

#[test]
fn python_reference_fixtures_match() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let count = check_parity(&dir, handle).expect("repair decisions should match Python");
    assert!(count >= 14, "expected all reconciliation guard fixtures");
}

fn run_stdio(input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-reconcile-review-queue"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn stdio_emits_the_inert_orphan_result() {
    let output = run_stdio(include_bytes!(
        "../testdata/orphan-matching-artifact.input.json"
    ));
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "../testdata/orphan-matching-artifact.expected.json"
    ))
    .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn stdio_rejects_malformed_run_evidence() {
    let mut request: Value = serde_json::from_str(include_str!(
        "../testdata/orphan-missing-artifact.input.json"
    ))
    .unwrap();
    for runs in [
        json!("unavailable"),
        json!({"runs": "unavailable"}),
        json!({"items": "unavailable"}),
        json!({"runs": "unavailable", "items": []}),
    ] {
        request["runs"] = runs;
        let output = run_stdio(&serde_json::to_vec(&request).unwrap());
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "{}: {actual}",
            request["runs"]
        );
        assert_eq!(actual["error"]["kind"], "invalid_request");
    }
}

#[test]
fn empty_run_evidence_retains_python_compatibility() {
    let mut request: Value = serde_json::from_str(include_str!(
        "../testdata/orphan-missing-artifact.input.json"
    ))
    .unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "../testdata/orphan-missing-artifact.expected.json"
    ))
    .unwrap();
    for runs in [
        Value::Null,
        json!({}),
        json!({"items": null}),
        json!({"runs": null, "items": [{"id": "run"}]}),
        json!({"runs": [], "items": [{"id": "run"}]}),
    ] {
        request["runs"] = runs;
        assert_eq!(
            handle(request.clone()).unwrap(),
            expected,
            "{}",
            request["runs"]
        );
    }
}
