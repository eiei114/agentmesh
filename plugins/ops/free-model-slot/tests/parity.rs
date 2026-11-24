use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use agentmesh_ops_free_model_slot::handle;
use agentmesh_ops_kit::{check_parity, EXIT_INVALID_REQUEST};
use serde_json::{json, Value};

#[test]
fn python_fixtures_match() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    assert!(check_parity(&testdata, handle).expect("Python parity") >= 40);
}

fn invoke(input: &[u8]) -> (u8, Value) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-free-model-slot"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stdio binary");
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    let code = output.status.code().unwrap().try_into().unwrap();
    (code, serde_json::from_slice(&output.stdout).unwrap())
}

#[test]
fn stdio_preserves_adjacent_float_gate_and_quality_values() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    for name in [
        "passing-v1-float-below-one",
        "passing-v2-float-below-one",
        "winners-v2-adjacent-quality",
    ] {
        let input = std::fs::read(testdata.join(format!("{name}.input.json"))).unwrap();
        let expected = std::fs::read(testdata.join(format!("{name}.expected.json"))).unwrap();
        let expected = serde_json::from_slice(&expected).unwrap();
        assert_eq!(invoke(&input), (0, expected), "{name}");
    }
}

#[test]
fn stdio_returns_one_result_or_one_error() {
    let (code, result) = invoke(include_bytes!("../testdata/winners-v2.input.json"));
    let expected: Value =
        serde_json::from_str(include_str!("../testdata/winners-v2.expected.json")).unwrap();
    assert_eq!((code, result), (0, expected));

    for input in [
        br#"{"op":"winners"} {"op":"winners"}"#.as_slice(),
        br#"{"op":"winners","models":false}"#,
        br#"{"op":"assignment"}"#,
    ] {
        let (code, result) = invoke(input);
        assert_eq!(code, EXIT_INVALID_REQUEST);
        assert_eq!(result["error"]["kind"], json!("invalid_request"));
    }
}
