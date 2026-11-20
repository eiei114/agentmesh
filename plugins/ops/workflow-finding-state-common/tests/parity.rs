use std::path::Path;

use agentmesh_ops_kit::check_parity;
use serde_json::Value;

#[test]
fn matches_python_parity_fixtures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let count = check_parity(
        &dir,
        agentmesh_ops_workflow_finding_state_common::handle_request,
    )
    .expect("all parity fixtures match Python");
    assert!(count >= 8, "expected the recurrence and edge-case cases");
}

#[test]
fn malformed_request_shape_is_rejected() {
    let error = agentmesh_ops_workflow_finding_state_common::handle_request(Value::Null)
        .expect_err("missing operation is invalid");
    assert_eq!(error.kind, agentmesh_ops_kit::ErrorKind::InvalidRequest);
}
