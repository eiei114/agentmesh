use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use agentmesh_ops_kit::{check_parity, load_parity_cases};
use agentmesh_ops_review_lessons_common::handle;
use serde_json::{json, Value};

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

#[test]
fn library_matches_python_fixtures() {
    assert_eq!(check_parity(&testdata(), handle).unwrap(), 19);
}

#[test]
fn stdio_matches_python_fixtures() {
    for case in load_parity_cases(&testdata()).unwrap() {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-review-lessons-common"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        serde_json::to_writer(child.stdin.take().unwrap(), &case.input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{}", case.name);
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(actual, case.expected, "{}", case.name);
    }
}

#[test]
fn rejects_invalid_contract_shapes() {
    for (key, malformed) in [
        ("now", json!("invalid")),
        ("now", json!("0001-01-01")),
        ("registry", json!({"guardrails": false})),
        ("records", json!([false])),
        ("repo_aliases", json!({"alpha": 42})),
    ] {
        let mut request = json!({"op": "build_index", "now": "2026-10-11", "records": [], "registry": {}, "repo_aliases": {}});
        request[key] = malformed;
        assert!(handle(request).is_err(), "{key}");
    }
    assert!(handle(json!({"op": "stamp"})).is_err());
    assert!(handle(json!({"op": "load_records"})).is_err());
}
