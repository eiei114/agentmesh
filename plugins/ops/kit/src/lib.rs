//! Shared helpers for the Python port crates under `plugins/ops/`.
//!
//! Every port crate is a library plus a stdio binary. The binary reads one JSON
//! request from stdin and writes one JSON result to stdout; see
//! `docs/python-migration-v0.md`. Parity tests compare the library's output with
//! fixtures generated from the Python implementation in the vault.
//!
//! This crate is owned by the coordinator lane. Port PRs depend on it with
//! `agentmesh-ops-kit = { path = "../kit" }` and do not change it.

use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use serde_json::{json, Value};

/// Process exit code for a request that is not JSON or that the port rejects.
pub const EXIT_INVALID_REQUEST: u8 = 2;
/// Process exit code for a failure while handling a valid request.
pub const EXIT_FAILED: u8 = 1;

/// Kind of a port failure. It is written to stdout as `error.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The request could not be read or does not match the port's input shape.
    InvalidRequest,
    /// The request was valid but handling it failed.
    Failed,
}

impl ErrorKind {
    /// Stable wire name of the kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::Failed => "failed",
        }
    }

    const fn exit_code(self) -> u8 {
        match self {
            Self::InvalidRequest => EXIT_INVALID_REQUEST,
            Self::Failed => EXIT_FAILED,
        }
    }
}

/// Error returned by a port handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpsError {
    /// What kind of failure this is.
    pub kind: ErrorKind,
    /// Human-readable message. Keep it free of secrets and absolute paths.
    pub message: String,
}

impl OpsError {
    /// A request the port does not accept.
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidRequest,
            message: message.into(),
        }
    }

    /// A failure while handling a valid request.
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Failed,
            message: message.into(),
        }
    }

    /// The JSON document written to stdout for this error.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({ "error": { "kind": self.kind.as_str(), "message": self.message } })
    }
}

impl fmt::Display for OpsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for OpsError {}

/// Parse one request document.
pub fn parse_request(text: &str) -> Result<Value, OpsError> {
    serde_json::from_str(text)
        .map_err(|err| OpsError::invalid_request(format!("request is not JSON: {err}")))
}

/// Handle one request text and return the stdout document and the exit code.
pub fn respond<F>(input: &str, handler: F) -> (Value, u8)
where
    F: FnOnce(Value) -> Result<Value, OpsError>,
{
    match parse_request(input).and_then(handler) {
        Ok(result) => (result, 0),
        Err(err) => (err.to_json(), err.kind.exit_code()),
    }
}

/// Run a port handler over stdin and stdout with the shared shape.
///
/// Success writes the handler's result and exits 0. A failure writes
/// `{"error": {"kind", "message"}}` to stdout, the message to stderr, and exits
/// with [`EXIT_INVALID_REQUEST`] or [`EXIT_FAILED`].
pub fn run_stdio<F>(handler: F) -> ExitCode
where
    F: FnOnce(Value) -> Result<Value, OpsError>,
{
    let mut input = String::new();
    let (document, code) = match io::stdin().lock().read_to_string(&mut input) {
        Ok(_) => respond(&input, handler),
        Err(err) => {
            let err = OpsError::invalid_request(format!("cannot read stdin: {err}"));
            (err.to_json(), err.kind.exit_code())
        }
    };
    if code != 0 {
        if let Some(message) = document.pointer("/error/message").and_then(Value::as_str) {
            eprintln!("{message}");
        }
    }
    let mut out = io::stdout().lock();
    let written = serde_json::to_writer(&mut out, &document)
        .map_err(io::Error::from)
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush());
    if written.is_err() {
        return ExitCode::from(EXIT_FAILED);
    }
    ExitCode::from(code)
}

/// One parity case: `<name>.input.json` and `<name>.expected.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParityCase {
    /// File stem shared by the two files.
    pub name: String,
    /// The request.
    pub input: Value,
    /// The Python implementation's output for the request.
    pub expected: Value,
}

const INPUT_SUFFIX: &str = ".input.json";
const EXPECTED_SUFFIX: &str = ".expected.json";

/// Load every parity case in `dir`, sorted by name.
///
/// Fails when the directory has no cases, when an input has no expected file
/// or an expected file has no input, or when a file is not JSON.
pub fn load_parity_cases(dir: &Path) -> Result<Vec<ParityCase>, String> {
    let entries = fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))?;
    let mut inputs = Vec::new();
    let mut expected = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("read {}: {err}", dir.display()))?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if let Some(stem) = file_name.strip_suffix(INPUT_SUFFIX) {
            inputs.push(stem.to_owned());
        } else if let Some(stem) = file_name.strip_suffix(EXPECTED_SUFFIX) {
            expected.push(stem.to_owned());
        }
    }
    inputs.sort();
    expected.sort();
    if let Some(orphan) = expected.iter().find(|stem| !inputs.contains(stem)) {
        return Err(format!(
            "{orphan}{EXPECTED_SUFFIX} has no {orphan}{INPUT_SUFFIX}"
        ));
    }
    if inputs.is_empty() {
        return Err(format!("no parity cases in {}", dir.display()));
    }
    inputs
        .into_iter()
        .map(|name| {
            if !expected.contains(&name) {
                return Err(format!(
                    "{name}{INPUT_SUFFIX} has no {name}{EXPECTED_SUFFIX}"
                ));
            }
            Ok(ParityCase {
                input: read_json(&dir.join(format!("{name}{INPUT_SUFFIX}")))?,
                expected: read_json(&dir.join(format!("{name}{EXPECTED_SUFFIX}")))?,
                name,
            })
        })
        .collect()
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("parse {}: {err}", path.display()))
}

/// Run `handler` on every case in `dir` and compare with the expected output.
///
/// Values are compared as JSON values, so key order and whitespace do not
/// matter, but `1` and `1.0` differ. An error result is compared as
/// [`OpsError::to_json`]. Returns the number of cases, or every mismatch.
pub fn check_parity<F>(dir: &Path, handler: F) -> Result<usize, String>
where
    F: Fn(Value) -> Result<Value, OpsError>,
{
    let cases = load_parity_cases(dir)?;
    let mut mismatches = Vec::new();
    for case in &cases {
        let actual = match handler(case.input.clone()) {
            Ok(value) => value,
            Err(err) => err.to_json(),
        };
        if actual != case.expected {
            mismatches.push(format!(
                "case {}:\n  expected {}\n  actual   {}",
                case.name, case.expected, actual
            ));
        }
    }
    if mismatches.is_empty() {
        Ok(cases.len())
    } else {
        Err(mismatches.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn testdata() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
    }

    fn sum(request: Value) -> Result<Value, OpsError> {
        let numbers = request
            .get("numbers")
            .and_then(Value::as_array)
            .ok_or_else(|| OpsError::invalid_request("numbers must be an array"))?;
        let mut total = 0_i64;
        for number in numbers {
            total += number
                .as_i64()
                .ok_or_else(|| OpsError::invalid_request("numbers must be integers"))?;
        }
        Ok(json!({ "total": total, "count": numbers.len() }))
    }

    #[test]
    fn respond_returns_result_and_zero() {
        let (doc, code) = respond(r#"{"numbers":[1,2,3]}"#, sum);
        assert_eq!(code, 0);
        assert_eq!(doc, json!({ "count": 3, "total": 6 }));
    }

    #[test]
    fn respond_rejects_non_json_with_exit_2() {
        let (doc, code) = respond("not json", sum);
        assert_eq!(code, EXIT_INVALID_REQUEST);
        assert_eq!(doc.pointer("/error/kind"), Some(&json!("invalid_request")));
    }

    #[test]
    fn respond_maps_failed_to_exit_1() {
        let (doc, code) = respond("{}", |_| Err(OpsError::failed("boom")));
        assert_eq!(code, EXIT_FAILED);
        assert_eq!(
            doc,
            json!({ "error": { "kind": "failed", "message": "boom" } })
        );
    }

    #[test]
    fn parity_fixtures_match() {
        assert_eq!(check_parity(&testdata(), sum), Ok(2));
    }

    #[test]
    fn parity_reports_mismatch() {
        let err = check_parity(&testdata(), |_| Ok(json!({}))).unwrap_err();
        assert!(err.contains("case sum-two"), "{err}");
        assert!(err.contains("case rejects-text"), "{err}");
    }

    #[test]
    fn parity_requires_cases_and_pairs() {
        let dir = std::env::temp_dir().join(format!("ops-kit-parity-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(load_parity_cases(&dir)
            .unwrap_err()
            .contains("no parity cases"));
        fs::write(dir.join("a.input.json"), "{}").unwrap();
        assert!(load_parity_cases(&dir)
            .unwrap_err()
            .contains("has no a.expected.json"));
        fs::write(dir.join("b.expected.json"), "{}").unwrap();
        assert!(load_parity_cases(&dir)
            .unwrap_err()
            .contains("has no b.input.json"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
