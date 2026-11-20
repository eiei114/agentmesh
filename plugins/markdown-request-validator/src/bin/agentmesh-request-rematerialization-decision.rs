//! Deterministic request re-materialization decision App plugin.

use agentmesh_fixture_support::run_fixture;
use agentmesh_markdown_request_validator::request_rematerialization_decision::{
    decide_request_rematerialization, DECISION_CONTRACT_VERSION,
};
use agentmesh_proto::rpc::RunResult;
use std::process::ExitCode;

fn main() -> ExitCode {
    run_fixture(
        DECISION_CONTRACT_VERSION,
        &[
            "request_rematerialization_decision",
            "deterministic_rewrite_verdict",
            "bounded_input_digests",
        ],
        Box::new(|params| {
            Ok(RunResult {
                payload: decide_request_rematerialization(&params.input),
            })
        }),
    )
}
