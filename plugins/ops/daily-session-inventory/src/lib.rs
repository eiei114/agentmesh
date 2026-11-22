//! Port of the deterministic daily session inventory decision.
//!
//! This covers `DAILY_ORPHAN_TOP`, `DAILY_ORPHAN_TOKEN_THRESHOLD`,
//! `daily_session_inventory_args` and the JSON-result handling of
//! `run_daily_session_inventory` from the vault's `daily-ai-kickoff.py`.
//!
//! The runner is an input seam: the caller supplies the runner's exit status,
//! stdout and stderr. The crate never starts `summarize-autopilot-metrics.py`,
//! a subprocess, or an `--apply` operation, and it touches no file, Multica,
//! network or git state. The Python controller remains authoritative.

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime, Time, UtcOffset};

/// Number of orphan sessions the inventory reports.
pub const DAILY_ORPHAN_TOP: u32 = 8;
/// Token count above which a session counts as an orphan.
pub const DAILY_ORPHAN_TOKEN_THRESHOLD: u32 = 250_000;
/// Script the Python controller hands the arguments to.
pub const RUNNER_SCRIPT: &str = "summarize-autopilot-metrics.py";

const INVALID_JSON_PREFIX: &str = "invalid session inventory JSON: ";

/// Python `datetime.isoformat()` of a UTC time: microseconds only when nonzero.
fn isoformat_utc(at: OffsetDateTime) -> String {
    let at = at.to_offset(UtcOffset::UTC);
    let micros = at.microsecond();
    let fraction = if micros == 0 {
        String::new()
    } else {
        format!(".{micros:06}")
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{fraction}+00:00",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second(),
    )
}

/// Parse `now`: an ISO 8601 timestamp that carries an explicit UTC offset.
///
/// Python would read an offset-less timestamp in the machine's local zone, so
/// the port rejects it instead of guessing.
pub fn parse_now(text: &str) -> Result<OffsetDateTime, OpsError> {
    OffsetDateTime::parse(text, &Rfc3339).map_err(|err| {
        OpsError::invalid_request(format!(
            "now must be an ISO 8601 timestamp with a UTC offset: {err}"
        ))
    })
}

/// Arguments of the inventory run: the bounded UTC window and the orphan limits.
///
/// The window starts at midnight of the previous day in `now`'s own offset and
/// ends at `now`, both rendered in UTC.
pub fn daily_session_inventory_args(now: OffsetDateTime) -> Result<Vec<String>, OpsError> {
    let previous = now
        .checked_sub(Duration::days(1))
        .ok_or_else(|| OpsError::invalid_request("now is out of the supported range"))?;
    let start = previous.replace_time(Time::MIDNIGHT);
    Ok([
        "--since",
        &isoformat_utc(start),
        "--until",
        &isoformat_utc(now),
        "--include-sessions",
        "--emit-orphan-session-events",
        "--orphan-top",
        &DAILY_ORPHAN_TOP.to_string(),
        "--orphan-token-threshold",
        &DAILY_ORPHAN_TOKEN_THRESHOLD.to_string(),
        "--apply",
        "--json",
    ]
    .iter()
    .map(|part| (*part).to_owned())
    .collect())
}

/// Python truthiness of a JSON value.
fn is_falsy(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(flag) => !flag,
        Value::Number(number) => number.as_f64() == Some(0.0),
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
    }
}

/// The `(status, summary, stderr)` tuple `run_daily_session_inventory` returns.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub status: i64,
    pub summary: Value,
    pub stderr: String,
}

impl Decision {
    /// The tuple as the JSON object the stdio handler returns.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({ "status": self.status, "summary": self.summary, "stderr": self.stderr })
    }
}

/// Decide from the runner's result, as Python does after the runner returns.
///
/// A nonzero status passes through with an empty summary. Empty stdout is an
/// empty payload. Invalid JSON fails closed with status 2. Otherwise the
/// summary is `orphan_session_observations`, or `{}` when it is absent or falsy.
pub fn decide(status: i64, stdout: &str, stderr: &str) -> Result<Decision, OpsError> {
    if status != 0 {
        return Ok(Decision {
            status,
            summary: json!({}),
            stderr: stderr.to_owned(),
        });
    }
    let payload = if stdout.is_empty() {
        json!({})
    } else {
        match serde_json::from_str::<Value>(stdout) {
            Ok(value) => value,
            Err(err) => {
                return Ok(Decision {
                    status: 2,
                    summary: json!({}),
                    stderr: format!("{INVALID_JSON_PREFIX}{err}"),
                });
            }
        }
    };
    let Value::Object(map) = payload else {
        return Err(OpsError::invalid_request(
            "runner stdout must be a JSON object",
        ));
    };
    let summary = match map.get("orphan_session_observations") {
        Some(value) if !is_falsy(value) => value.clone(),
        _ => json!({}),
    };
    Ok(Decision {
        status: 0,
        summary,
        stderr: stderr.to_owned(),
    })
}

fn string_field<'a>(object: &'a Value, key: &str) -> Result<&'a str, OpsError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| OpsError::invalid_request(format!("{key} must be a string")))
}

/// Stdio handler.
///
/// `{"op": "args", "now": ...}` returns `{"script", "args"}`.
/// `{"op": "decide", "now": ..., "runner": {"status", "stdout", "stderr"}}`
/// returns `{"status", "summary", "stderr"}`.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    let op = string_field(&request, "op")?;
    let now = parse_now(string_field(&request, "now")?)?;
    let args = daily_session_inventory_args(now)?;
    match op {
        "args" => Ok(json!({ "script": RUNNER_SCRIPT, "args": args })),
        "decide" => {
            let runner = request
                .get("runner")
                .filter(|value| value.is_object())
                .ok_or_else(|| OpsError::invalid_request("runner must be an object"))?;
            let status = runner
                .get("status")
                .and_then(Value::as_i64)
                .ok_or_else(|| OpsError::invalid_request("runner.status must be an integer"))?;
            let stdout = string_field(runner, "stdout")?;
            let stderr = string_field(runner, "stderr")?;
            Ok(decide(status, stdout, stderr)?.to_json())
        }
        other => Err(OpsError::invalid_request(format!("unknown op: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The decoder detail after the invalid JSON prefix is parser specific
    /// (Python and serde word it differently), so both sides replace it.
    fn normalize(result: Result<Value, OpsError>) -> Result<Value, OpsError> {
        result.map(|mut value| {
            if let Some(stderr) = value.get("stderr").and_then(Value::as_str) {
                if stderr.starts_with(INVALID_JSON_PREFIX) {
                    value["stderr"] = json!(format!("{INVALID_JSON_PREFIX}<decoder detail>"));
                }
            }
            value
        })
    }

    #[test]
    fn parity_fixtures_match() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
        let count = agentmesh_ops_kit::check_parity(&dir, |request| normalize(handle(request)))
            .unwrap_or_else(|err| panic!("{err}"));
        assert!(count >= 20, "expected at least 20 fixtures, found {count}");
    }

    fn at(text: &str) -> OffsetDateTime {
        parse_now(text).unwrap()
    }

    #[test]
    fn args_use_bounded_utc_window() {
        let args = daily_session_inventory_args(at("2026-08-13T07:00:00+09:00")).unwrap();
        assert_eq!(args[1], "2026-08-11T15:00:00+00:00");
        assert_eq!(args[3], "2026-08-12T22:00:00+00:00");
        assert_eq!(args.last().map(String::as_str), Some("--json"));
        assert!(!args.contains(&"--ignore-session-watermark".to_owned()));
    }

    #[test]
    fn args_keep_microseconds_on_until_only() {
        let args = daily_session_inventory_args(at("2026-08-13T00:00:00.123456+00:00")).unwrap();
        assert_eq!(args[1], "2026-08-12T00:00:00+00:00");
        assert_eq!(args[3], "2026-08-13T00:00:00.123456+00:00");
    }

    #[test]
    fn naive_now_is_rejected() {
        let err = handle(json!({"op": "args", "now": "2026-08-13T07:00:00"})).unwrap_err();
        assert!(err.message.contains("UTC offset"), "{}", err.message);
    }

    #[test]
    fn nonzero_status_passes_stderr_through() {
        let decision = decide(3, r#"{"orphan_session_observations": {"a": 1}}"#, "boom").unwrap();
        assert_eq!(decision.status, 3);
        assert_eq!(decision.summary, json!({}));
        assert_eq!(decision.stderr, "boom");
    }

    #[test]
    fn invalid_json_fails_closed() {
        let decision = decide(0, "not-json", "").unwrap();
        assert_eq!(decision.status, 2);
        assert_eq!(decision.summary, json!({}));
        assert!(decision.stderr.starts_with(INVALID_JSON_PREFIX));
    }

    #[test]
    fn falsy_or_missing_observations_become_empty() {
        for stdout in [
            "",
            "{}",
            r#"{"orphan_session_observations": null}"#,
            r#"{"orphan_session_observations": []}"#,
            r#"{"orphan_session_observations": 0}"#,
        ] {
            let decision = decide(0, stdout, "warn").unwrap();
            assert_eq!(decision.summary, json!({}), "{stdout}");
            assert_eq!(decision.stderr, "warn");
        }
    }

    #[test]
    fn non_object_payload_is_rejected() {
        assert!(decide(0, "[1]", "").is_err());
    }

    #[test]
    fn unknown_op_is_rejected() {
        let err = handle(json!({"op": "run", "now": "2026-08-13T00:00:00Z"})).unwrap_err();
        assert!(err.message.contains("unknown op"));
    }
}
