use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use agentmesh_ops_kit::{check_parity, load_parity_cases};
use agentmesh_ops_review_lessons_common::{handle, handle_json};
use serde_json::{json, Value};

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

#[test]
fn library_matches_python_fixtures() {
    assert_eq!(check_parity(&testdata(), handle).unwrap(), 20);
}

#[test]
fn original_json_matches_python_through_library_and_stdio() {
    let dir = testdata();
    let mut count = 0;
    for dir in [&dir, &dir.join("raw")] {
        for case in load_parity_cases(dir).unwrap() {
            let input =
                std::fs::read_to_string(dir.join(format!("{}.input.json", case.name))).unwrap();
            assert_eq!(handle_json(&input).unwrap(), case.expected, "{}", case.name);
            let mut child = Command::new(env!("CARGO_BIN_EXE_agentmesh-ops-review-lessons-common"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            // Exercise the binary with the original numeric lexemes, not a
            // reserialization that could hide rounding during request parsing.
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert_eq!(output.status.code(), Some(0), "{}", case.name);
            let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(actual, case.expected, "{}", case.name);
            count += 1;
        }
    }
    assert_eq!(count, 21);
}

#[test]
fn rejects_invalid_contract_shapes() {
    for (key, malformed) in [
        ("now", json!("invalid")),
        ("now", json!("0001-01-01")),
        ("registry", json!({"guardrails": false})),
        ("records", json!([false])),
        ("repo_aliases", json!({"alpha": 42})),
    ] {
        let mut request = json!({"op": "build_index", "now": "2026-10-11", "records": [], "registry": {}, "repo_aliases": {}});
        request[key] = malformed;
        assert!(handle(request).is_err(), "{key}");
    }
    assert!(handle(json!({"op": "stamp"})).is_err());
    assert!(handle(json!({"op": "load_records"})).is_err());
}
