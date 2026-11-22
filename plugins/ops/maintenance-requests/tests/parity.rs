use std::path::Path;

#[test]
fn python_fixtures_match() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let cases =
        agentmesh_ops_kit::check_parity(&testdata, agentmesh_ops_maintenance_requests::handle)
            .expect("maintenance-requests cases should match Python output");
    assert!(
        cases >= 100,
        "expected coverage for validation, slugify, rendering and patrol edge cases"
    );
}

#[test]
fn fixtures_cover_every_operation_and_both_outcomes() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let cases = agentmesh_ops_kit::load_parity_cases(&testdata).expect("fixtures should load");
    for op in [
        "validate",
        "slugify",
        "render_source",
        "render_issue",
        "patrol",
    ] {
        let of_op = || cases.iter().filter(|case| case.input["op"] == op);
        assert!(of_op().next().is_some(), "no fixture for {op}");
        if op == "validate" || op.starts_with("render") {
            assert!(
                of_op().any(|case| case.expected.get("error").is_some()),
                "no failing fixture for {op}"
            );
            assert!(
                of_op().any(|case| case.expected.get("error").is_none()),
                "no passing fixture for {op}"
            );
        }
    }
}
