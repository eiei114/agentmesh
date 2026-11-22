use std::path::Path;
use std::process::{Command, Stdio};

use agentmesh_ops_kit::{check_parity, load_parity_cases};
use agentmesh_ops_review_guardrail_analysis::handle;
use serde_json::json;

fn testdata() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

#[test]
fn matches_every_python_fixture() {
    let count = check_parity(&testdata(), handle).expect("Python parity cases must match");
    assert!(count >= 85, "expected coverage for all ported helpers");
}

#[test]
fn fixtures_cover_every_op() {
    let cases = load_parity_cases(&testdata()).expect("all Python parity fixtures must load");
    for op in ["parse_time", "occurrence_time", "analyze"] {
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
        "analyze-old-finding-backfilled-later-is-excluded",
        "analyze-new-finding-with-older-observation-is-included",
        "analyze-cutoff-boundary-exact-is-included",
        "analyze-cutoff-boundary-one-microsecond-before-is-excluded",
        "analyze-recurrence-boundary-at-promotion-time",
        "analyze-recurrence-late-backfill-of-old-finding-does-not-count",
        "analyze-threshold-met-makes-candidate",
        "analyze-threshold-not-met-one-repository",
        "analyze-immediate-candidate-without-threshold",
        "analyze-malformed-timestamps-are-skipped",
        "analyze-unclassified-is-never-a-candidate",
        "analyze-candidates-follow-sorted-key-order-not-input-order",
        "occurrence-time-malformed-created-does-not-fall-back",
        "parse-time-malformed-text",
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
    assert!(malformed >= 15, "malformed-input cases must be pinned");
}

#[test]
fn stdio_binary_matches_every_python_fixture() {
    let cases = load_parity_cases(&testdata()).expect("all Python parity fixtures must load");
    for case in cases {
        let mut child = Command::new(env!(
            "CARGO_BIN_EXE_agentmesh-ops-review-guardrail-analysis"
        ))
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
fn analysis_output_carries_no_record_bodies() {
    let result = handle(json!({
        "op": "analyze",
        "records": [{
            "normalized_guardrail_key": "K",
            "repo": "a",
            "finding_created_at": "2026-11-01T00:00:00Z",
            "body": "raw review text",
            "message": "raw review text"
        }],
        "registry": {"guardrails": [{"key": "K"}]},
        "now": "2026-11-22T00:00:00Z"
    }))
    .expect("valid request");
    assert!(!result.to_string().contains("raw review text"));
}

#[test]
fn invalid_requests_are_rejected() {
    assert!(handle(json!([])).is_err());
    assert!(handle(json!({"op": "nope"})).is_err());
    assert!(handle(json!({"op": "parse_time"})).is_err());
    assert!(handle(json!({"op": "occurrence_time"})).is_err());
    assert!(handle(json!({"op": "analyze", "records": [], "registry": {}})).is_err());
    assert!(
        handle(json!({"op": "analyze", "registry": {}, "now": "2026-11-22T00:00:00Z"})).is_err()
    );
}
