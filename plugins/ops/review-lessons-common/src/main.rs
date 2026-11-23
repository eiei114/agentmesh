use std::io::{self, Read, Write};
use std::process::ExitCode;

use agentmesh_ops_kit::{ErrorKind, OpsError, EXIT_FAILED, EXIT_INVALID_REQUEST};
use agentmesh_ops_review_lessons_common::handle_json;

fn main() -> ExitCode {
    // ops-kit's Value-based stdio runner rounds large integers before calling
    // its handler. Preserve raw input while keeping the shared error contract.
    let mut input = String::new();
    let response = io::stdin()
        .lock()
        .read_to_string(&mut input)
        .map_err(|err| OpsError::invalid_request(format!("cannot read stdin: {err}")))
        .and_then(|_| handle_json(&input));
    let (document, code) = match response {
        Ok(value) => (value, 0),
        Err(err) => {
            eprintln!("{}", err.message);
            let code = match err.kind {
                ErrorKind::InvalidRequest => EXIT_INVALID_REQUEST,
                ErrorKind::Failed => EXIT_FAILED,
            };
            (err.to_json(), code)
        }
    };
    let mut out = io::stdout().lock();
    let written = serde_json::to_writer(&mut out, &document)
        .map_err(io::Error::from)
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush());
    ExitCode::from(if written.is_ok() { code } else { EXIT_FAILED })
}
