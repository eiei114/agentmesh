use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use agentmesh_ops_kit::{check_parity, load_parity_cases};
use agentmesh_ops_task_spec_hash::handle_request;
use serde_json::Value;

#[test]
fn python_fixtures_match() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let cases = check_parity(&testdata, handle_request).expect("Python parity fixtures match");
    assert!(
        cases >= 5,
        "expected vocabulary, fence, operational, CRLF, and null cases"
    );
}

#[test]
fn stdio_binary_matches_every_python_fixture() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let cases = load_parity_cases(&testdata).expect("parity fixtures load");
    let binary = env!("CARGO_BIN_EXE_agentmesh-ops-task-spec-hash");

    for case in cases {
        let mut child = Command::new(binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("stdio binary starts");
        child
            .stdin
            .take()
            .expect("stdin is piped")
            .write_all(case.input.to_string().as_bytes())
            .expect("request is written");
        let output = child.wait_with_output().expect("stdio result is available");
        assert!(
            output.status.success(),
            "case {} failed: {}",
            case.name,
            String::from_utf8_lossy(&output.stderr)
        );
        let actual: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
        assert_eq!(actual, case.expected, "binary parity case {}", case.name);
    }
}
