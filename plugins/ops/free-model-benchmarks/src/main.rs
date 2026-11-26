use std::io::{self, Read, Write};
use std::process::ExitCode;

use agentmesh_ops_free_model_benchmarks::handle_text;
use agentmesh_ops_kit::{ErrorKind, OpsError, EXIT_FAILED, EXIT_INVALID_REQUEST};

fn main() -> ExitCode {
    // Keep the ops-kit stdio contract, but decode this request from the text:
    // its Value-based run_stdio would round large integers before matching.
    let mut input = String::new();
    let result = io::stdin()
        .lock()
        .read_to_string(&mut input)
        .map_err(|err| OpsError::invalid_request(format!("cannot read stdin: {err}")))
        .and_then(|_| handle_text(&input));
    let (document, code) = match result {
        Ok(document) => (document, 0),
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
