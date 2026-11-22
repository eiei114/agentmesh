use std::path::Path;
use std::process::{Command, Stdio};

use agentmesh_ops_escape_sensitive_edits::handle;
use agentmesh_ops_kit::{check_parity, load_parity_cases};

#[test]
fn matches_every_python_fixture() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let count = check_parity(&testdata, handle).expect("Python parity cases must match");
    assert!(count >= 30, "expected coverage for all ported helpers");
}

#[test]
fn fixtures_cover_every_op() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let cases = load_parity_cases(&testdata).expect("all Python parity fixtures must load");
    for op in [
        "is_js_ts_path",
        "command_targets_js_ts",
        "command_mentions_escape_literal",
        "flag_unsafe_js_ts_shell_edit",
        "scan_physical_newline_in_strings",
        "scan_escape_corruption",
    ] {
        assert!(
            cases.iter().any(|case| case.input["op"] == op),
            "no fixture for {op}"
        );
    }
}

#[test]
fn stdio_binary_matches_every_python_fixture() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let cases = load_parity_cases(&testdata).expect("all Python parity fixtures must load");
    for case in cases {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-escape-sensitive-edits"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("stdio binary must start");
        serde_json::to_writer(child.stdin.as_mut().expect("stdin is piped"), &case.input)
            .expect("fixture must serialize");
        drop(child.stdin.take());
        let output = child.wait_with_output().expect("stdio binary must finish");
        assert_eq!(output.status.code(), Some(0), "case {}", case.name);
        let actual: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("binary stdout must be JSON");
        assert_eq!(actual, case.expected, "case {}", case.name);
    }
}

#[test]
fn missing_command_matches_null_command() {
    let null = serde_json::json!({"op": "flag_unsafe_js_ts_shell_edit", "command": null});
    let omitted = serde_json::json!({"op": "flag_unsafe_js_ts_shell_edit"});
    assert_eq!(handle(null).expect("null is valid"), serde_json::json!([]));
    assert_eq!(
        handle(omitted).expect("omitted is valid"),
        serde_json::json!([])
    );
}

#[test]
fn non_string_fields_are_invalid_requests() {
    assert!(handle(serde_json::json!({"op": "is_js_ts_path", "path": 1})).is_err());
    assert!(handle(serde_json::json!({"op": "scan_escape_corruption", "content": 1})).is_err());
    assert!(
        handle(serde_json::json!({"op": "scan_escape_corruption", "content": "", "path": 1}))
            .is_err()
    );
}
