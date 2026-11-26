use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use agentmesh_ops_kit::check_parity;
use agentmesh_ops_reconcile_review_queue::handle;
use serde_json::Value;

#[test]
fn python_reference_fixtures_match() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let count = check_parity(&dir, handle).expect("repair decisions should match Python");
    assert!(count >= 14, "expected all reconciliation guard fixtures");
}

#[test]
fn stdio_emits_the_inert_orphan_result() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-reconcile-review-queue"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(include_bytes!(
            "../testdata/orphan-matching-artifact.input.json"
        ))
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "../testdata/orphan-matching-artifact.expected.json"
    ))
    .unwrap();
    assert_eq!(actual, expected);
}
