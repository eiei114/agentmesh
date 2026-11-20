use std::path::Path;

#[test]
fn python_fixtures_match() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let cases = agentmesh_ops_kit::check_parity(&testdata, agentmesh_ops_project_focus::handle)
        .expect("project-focus cases should match Python output");
    assert!(
        cases >= 6,
        "expected coverage for the documented edge cases"
    );
}
