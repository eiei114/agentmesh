use std::path::Path;

use agentmesh_ops_kit::check_parity;

#[test]
fn python_reference_fixtures_match() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let count = check_parity(&fixture_dir, agentmesh_ops_pr_link_heal::handle)
        .expect("PR-link-heal cases should match Python output");
    assert!(count >= 13, "expected all PR-link-heal guard fixtures");
}
