use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use agentmesh_ops_direct_tick_status::handle_request;
use agentmesh_ops_kit::{check_parity, load_parity_cases, EXIT_INVALID_REQUEST};
use serde_json::Value;

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

#[test]
fn python_parity() {
    let count = check_parity(&data(), handle_request).expect("Python assessments must match");
    assert!(count >= 7);
}

fn run(input: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-direct-tick-status"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn stdio_matches_python_fixtures() {
    for case in load_parity_cases(&data()).unwrap() {
        let output = run(&serde_json::to_vec(&case.input).unwrap());
        assert!(output.status.success(), "{}: {:?}", case.name, output);
        assert!(output.stderr.is_empty(), "{}: {:?}", case.name, output);
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            case.expected,
            "{}",
            case.name
        );
    }
}

#[test]
fn stdio_rejects_invalid_clock_or_newest_finish() {
    let mut input = load_parity_cases(&data())
        .unwrap()
        .into_iter()
        .find(|case| case.name == "stale-inside")
        .unwrap()
        .input;
    for pointer in ["/rows/0/finished_at", "/now"] {
        let field = input.pointer_mut(pointer).unwrap();
        let original = std::mem::replace(field, Value::String("bad timestamp".into()));
        let output = run(&serde_json::to_vec(&input).unwrap());
        assert_eq!(output.status.code(), Some(i32::from(EXIT_INVALID_REQUEST)));
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["error"]["kind"], "invalid_request");
        *input.pointer_mut(pointer).unwrap() = original;
    }
}
