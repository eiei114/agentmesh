use std::path::Path;

use agentmesh_ops_kit::check_parity;
use agentmesh_ops_provider_quota_common::decide;

#[test]
fn python_reference_fixtures_match() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let count = check_parity(&fixture_dir, decide).expect("parity fixtures must match");
    assert!(
        count >= 10,
        "fixtures should cover policy and label edge cases"
    );
}
