use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use agentmesh_ops_kit::{check_parity, load_parity_cases};
use agentmesh_ops_vault_steward::handle;

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

#[test]
fn library_matches_every_python_fixture() {
    let count = check_parity(&testdata(), handle).expect("Python parity cases must match");
    assert!(count >= 6, "expected coverage for all result scenarios");
}

#[test]
fn stdio_matches_every_python_fixture() {
    for case in load_parity_cases(&testdata()).expect("Python parity fixtures must load") {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-vault-steward"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("stdio binary must start");
        serde_json::to_writer(child.stdin.take().expect("stdin is piped"), &case.input)
            .expect("fixture must serialize");
        let output = child.wait_with_output().expect("stdio binary must finish");
        assert_eq!(output.status.code(), Some(0), "case {}", case.name);
        assert!(output.stderr.is_empty(), "case {}", case.name);
        let actual: serde_json::Value =
            serde_json::from_slice(&output.stdout).expect("stdout must be one JSON document");
        assert_eq!(actual, case.expected, "case {}", case.name);
    }
}
