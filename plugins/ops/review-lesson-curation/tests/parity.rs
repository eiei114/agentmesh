use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use agentmesh_ops_kit::{check_parity, load_parity_cases, EXIT_INVALID_REQUEST};
use agentmesh_ops_review_lesson_curation::handle;
use serde_json::Value;

fn testdata() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/testdata"))
}

#[test]
fn matches_python() {
    assert!(check_parity(testdata(), handle).unwrap() > 0);
}

fn run(input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-review-lesson-curation"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn stdio_matches_python() {
    for case in load_parity_cases(testdata()).unwrap() {
        let output = run(&serde_json::to_vec(&case.input).unwrap());
        assert!(output.status.success(), "{}: {:?}", case.name, output);
        assert!(output.stderr.is_empty());
        assert!(output.stdout.ends_with(b"\n"));
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(actual, case.expected, "{}", case.name);
    }
}

#[test]
fn invalid_stdio_request_returns_one_error_document() {
    let output = run(br#"{"op":"week_id","now":"2025-01-05T15:00:00"}"#);
    assert_eq!(output.status.code(), Some(i32::from(EXIT_INVALID_REQUEST)));
    let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(actual["error"]["kind"], "invalid_request");
}
