//! Deterministic request acceptance-evidence coverage App.

use agentmesh_fixture_support::run_fixture;
use agentmesh_markdown_request_validator::request_acceptance_evidence_coverage::{
    coverage_report, APP_VERSION,
};
use agentmesh_proto::rpc::RunResult;
use std::process::ExitCode;

fn main() -> ExitCode {
    run_fixture(
        APP_VERSION,
        &[
            "request_acceptance_evidence_coverage",
            "per_criterion_evidence_coverage",
            "bounded_normalized_findings",
        ],
        Box::new(|params| {
            Ok(RunResult {
                payload: coverage_report(&params.input),
            })
        }),
    )
}
