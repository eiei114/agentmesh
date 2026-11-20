//! Deterministic request canonical summary manifest App plugin.

use agentmesh_fixture_support::run_fixture;
use agentmesh_markdown_request_validator::request_canonical_summary_manifest::{
    canonical_summary_manifest, CANONICAL_SUMMARY_MANIFEST_VERSION,
};
use agentmesh_proto::rpc::RunResult;
use std::process::ExitCode;

fn main() -> ExitCode {
    run_fixture(
        CANONICAL_SUMMARY_MANIFEST_VERSION,
        &[
            "request_canonical_summary_manifest",
            "deterministic_summary_digests",
            "adapter_neutral_common_fields",
        ],
        Box::new(|params| {
            Ok(RunResult {
                payload: canonical_summary_manifest(&params.input),
            })
        }),
    )
}
