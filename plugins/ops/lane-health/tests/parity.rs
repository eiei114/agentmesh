use std::path::Path;

#[test]
fn python_fixtures_match() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let cases = agentmesh_ops_kit::check_parity(&testdata, agentmesh_ops_lane_health::handle)
        .expect("lane-health cases should match Python output");
    assert!(
        cases >= 8,
        "expected coverage for the documented lane-health decision cases"
    );
}
