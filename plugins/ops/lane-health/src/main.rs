use std::process::ExitCode;

fn main() -> ExitCode {
    agentmesh_ops_kit::run_stdio(agentmesh_ops_lane_health::handle)
}
