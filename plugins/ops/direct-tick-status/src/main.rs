fn main() -> std::process::ExitCode {
    agentmesh_ops_kit::run_stdio(agentmesh_ops_direct_tick_status::handle_request)
}
