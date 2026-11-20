//! Deterministic request materialization receipt App plugin.

use agentmesh_fixture_support::run_fixture;
use agentmesh_markdown_request_validator::request_materialization_receipt::{
    create_materialization_receipt, RECEIPT_CONTRACT_VERSION,
};
use agentmesh_proto::rpc::RunResult;
use std::process::ExitCode;

fn main() -> ExitCode {
    run_fixture(
        RECEIPT_CONTRACT_VERSION,
        &[
            "request_materialization_receipt",
            "deterministic_drift_comparison",
            "bounded_post_write_evidence",
        ],
        Box::new(|params| {
            Ok(RunResult {
                payload: create_materialization_receipt(&params.input),
            })
        }),
    )
}
