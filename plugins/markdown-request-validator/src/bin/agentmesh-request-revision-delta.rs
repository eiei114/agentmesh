//! Deterministic request revision delta App plugin.

use agentmesh_fixture_support::run_fixture;
use agentmesh_markdown_request_validator::request_revision_delta::{
    compute_request_revision_delta, DELTA_CONTRACT_VERSION,
};
use agentmesh_proto::rpc::RunResult;
use std::process::ExitCode;

fn main() -> ExitCode {
    run_fixture(
        DELTA_CONTRACT_VERSION,
        &[
            "request_revision_delta",
            "deterministic_field_level_delta",
            "contract_affecting_classification",
        ],
        Box::new(|params| {
            Ok(RunResult {
                payload: compute_request_revision_delta(&params.input),
            })
        }),
    )
}
