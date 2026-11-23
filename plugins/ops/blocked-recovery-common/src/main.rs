use std::io::{self, Read, Write};
use std::process::ExitCode;

use agentmesh_ops_blocked_recovery_common::handle_json;
use agentmesh_ops_kit::{ErrorKind, OpsError, EXIT_FAILED, EXIT_INVALID_REQUEST};

fn main() -> ExitCode {
    // The shared run_stdio contract is Value-based and would round numbers
    // before dispatch. Keep its framing and error envelope, but write raw JSON.
    let mut input = String::new();
    let result = io::stdin()
        .lock()
        .read_to_string(&mut input)
        .map_err(|err| OpsError::invalid_request(format!("cannot read stdin: {err}")))
        .and_then(|_| handle_json(&input));
    let mut out = io::stdout().lock();
    let (written, code) = match result {
        Ok(document) => (out.write_all(document.get().as_bytes()), 0),
        Err(err) => {
            eprintln!("{}", err.message);
            let code = match err.kind {
                ErrorKind::InvalidRequest => EXIT_INVALID_REQUEST,
                ErrorKind::Failed => EXIT_FAILED,
            };
            (
                serde_json::to_writer(&mut out, &err.to_json()).map_err(io::Error::from),
                code,
            )
        }
    };
    let written = written
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush());
    ExitCode::from(if written.is_ok() { code } else { EXIT_FAILED })
}
