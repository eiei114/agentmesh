use std::path::Path;
use std::process::{Command, Stdio};

use agentmesh_ops_kit::{check_parity, load_parity_cases};
use agentmesh_ops_vault_split_plan::handle;

#[test]
fn matches_every_python_fixture() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let count = check_parity(&testdata, handle).expect("Python parity cases must match");
    assert!(count >= 6, "expected coverage for all ported helpers");
}

#[test]
fn stdio_binary_matches_every_python_fixture() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let cases = load_parity_cases(&testdata).expect("all Python parity fixtures must load");
    for case in cases {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-vault-split-plan"))
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
fn empty_today_matches_omitted_today() {
    let entries = serde_json::json!({
        "entries": [{"path": "AGENTS.md", "raw": " M AGENTS.md"}]
    });
    let mut empty_today = entries.clone();
    empty_today["op"] = serde_json::json!("build_bucket_plans");
    empty_today["today"] = serde_json::json!("");

    let mut omitted_today = entries;
    omitted_today["op"] = serde_json::json!("build_bucket_plans");

    assert_eq!(
        handle(empty_today).expect("empty today is valid"),
        handle(omitted_today).expect("omitted today is valid")
    );
}
