use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use agentmesh_ops_kit::{check_parity, load_parity_cases};
use agentmesh_ops_vault_stale_diff_audit::handle;
use serde_json::Value;

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

#[test]
fn library_matches_python() {
    assert_eq!(check_parity(&testdata(), handle), Ok(3));
}

#[test]
fn stdio_matches_python() {
    for case in load_parity_cases(&testdata()).expect("load Python parity cases") {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-vault-stale-diff-audit"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start stdio binary");
        serde_json::to_writer(child.stdin.take().expect("piped stdin"), &case.input)
            .expect("write request");
        let output = child.wait_with_output().expect("read result");
        assert_eq!(output.status.code(), Some(0), "{}", case.name);
        assert!(output.stderr.is_empty(), "{}", case.name);
        let actual: Value = serde_json::from_slice(&output.stdout).expect("one JSON result");
        assert_eq!(actual, case.expected, "{}", case.name);
    }
}
