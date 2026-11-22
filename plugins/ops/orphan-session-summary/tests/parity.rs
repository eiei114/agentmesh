use std::path::Path;
use std::process::{Command, Stdio};

use agentmesh_ops_kit::{check_parity, load_parity_cases};
use agentmesh_ops_orphan_session_summary::handle;
use serde_json::json;

fn testdata() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

#[test]
fn matches_every_python_fixture() {
    let count = check_parity(&testdata(), handle).expect("Python parity cases must match");
    assert!(count >= 55, "expected coverage for all ported helpers");
}

#[test]
fn fixtures_cover_every_op() {
    let cases = load_parity_cases(&testdata()).expect("all Python parity fixtures must load");
    for op in [
        "default_orphan_token_threshold",
        "normalize_path",
        "compact_orphan_session",
        "select_top_orphan_sessions",
        "build_orphan_session_observation_event",
        "sum_event_field",
        "compare_log_aware_summaries",
    ] {
        assert!(
            cases.iter().any(|case| case.input["op"] == op),
            "no fixture for {op}"
        );
    }
}

#[test]
fn fixtures_cover_listed_boundary_cases() {
    let cases = load_parity_cases(&testdata()).expect("all Python parity fixtures must load");
    for name in [
        "select-ties-keep-input-order",
        "select-missing-usage-with-nonzero-size",
        "select-normalized-matched-paths",
        "select-malformed-total-tokens",
        "select-malformed-size-bytes-list",
        "compare-malformed-metric-values-become-zero",
        "compare-absent-sections",
        "compare-null-sections",
        "compare-six-decimal-rounding",
    ] {
        assert!(
            cases.iter().any(|case| case.name == name),
            "no fixture {name}"
        );
    }
    let malformed = cases
        .iter()
        .filter(|case| case.expected.pointer("/error/message").is_some())
        .count();
    assert!(malformed >= 8, "malformed-input cases must be pinned");
}

#[test]
fn stdio_binary_matches_every_python_fixture() {
    let cases = load_parity_cases(&testdata()).expect("all Python parity fixtures must load");
    for case in cases {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-orphan-session-summary"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("stdio binary must start");
        serde_json::to_writer(child.stdin.as_mut().expect("stdin is piped"), &case.input)
            .expect("fixture must serialize");
        drop(child.stdin.take());
        let output = child.wait_with_output().expect("stdio binary must finish");
        let expect_error = case.expected.pointer("/error/kind").is_some();
        assert_eq!(
            output.status.code(),
            Some(if expect_error { 2 } else { 0 }),
            "case {}",
            case.name
        );
        let actual: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("binary stdout must be JSON");
        assert_eq!(actual, case.expected, "case {}", case.name);
    }
}

#[test]
fn event_never_carries_conversation_fields() {
    let event = handle(json!({
        "op": "build_orphan_session_observation_event",
        "session": {
            "session_id": "s", "session_file": "/s/a.jsonl", "total_tokens": 9,
            "message": "raw", "content": "raw", "messages": ["raw"]
        },
        "ts": "2026-07-06T12:00:00+09:00"
    }))
    .expect("valid request");
    for key in ["message", "content", "messages"] {
        assert!(event.get(key).is_none(), "{key} leaked into the event");
    }
}

#[test]
fn invalid_requests_are_rejected() {
    assert!(handle(json!([])).is_err());
    assert!(handle(json!({"op": "nope"})).is_err());
    assert!(handle(json!({"op": "normalize_path", "value": 5})).is_err());
    assert!(handle(json!({"op": "select_top_orphan_sessions"})).is_err());
    assert!(handle(
        json!({"op": "select_top_orphan_sessions", "session_records": [], "top_n": "3"})
    )
    .is_err());
    assert!(
        handle(json!({"op": "build_orphan_session_observation_event", "session": {}})).is_err()
    );
    assert!(
        handle(json!({"op": "compare_log_aware_summaries", "baseline": [], "current": {}}))
            .is_err()
    );
}
