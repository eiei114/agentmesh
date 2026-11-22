use std::path::Path;

#[test]
fn python_fixtures_match() {
    let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let cases =
        agentmesh_ops_kit::check_parity(&testdata, agentmesh_ops_windows_subprocess::handle)
            .expect("windows-subprocess cases should match Python output");
    assert!(
        cases >= 3,
        "expected non-Windows, Windows, and Windows new-group cases"
    );
}
