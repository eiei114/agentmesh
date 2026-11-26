//! Offline evidence from one native run. Python keeps ownership of recovery.

mod stamp;

use std::borrow::Cow;
use std::sync::LazyLock;

use agentmesh_ops_kit::OpsError;
use regex::Regex;
use serde_json::{json, Value};

const BLOCK_REASON: &str = "native_stall_retry_budget_exhausted";

static RUNTIME_FAILURE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)session file[^\n]*already in use|daemon (?:shut ?down|stopp?ed|exited)|",
        r"pi RPC exited before agent_settled"
    ))
    .unwrap()
});
static SIBLING_HOLD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)sibling|兄弟|並行.{0,24}(?:待|実行)|親.{0,24}待|",
        r"(?:別|他).{0,24}\brun\b.{0,24}(?:待|queued|running)"
    ))
    .unwrap()
});
static IDENTITY_HOLD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)identity.{0,30}(?:mismatch|不一致|食い違)|mismatch.{0,30}identity|本人性.{0,16}不一致")
        .unwrap()
});

fn text(value: &Value) -> Cow<'_, str> {
    match value {
        Value::String(s) => Cow::Borrowed(s),
        Value::Null => Cow::Borrowed(""),
        other => Cow::Owned(other.to_string()),
    }
}

fn classification(run: &Value, status: &str, provider_match: bool) -> &'static str {
    if status == "completed" {
        let output = text(&run["result"]["output"]);
        if SIBLING_HOLD.is_match(&output) {
            "sibling_hold"
        } else if IDENTITY_HOLD.is_match(&output) {
            "identity_hold"
        } else {
            "completed_hold"
        }
    } else if RUNTIME_FAILURE.is_match(&text(&run["error"])) {
        "infrastructure"
    } else if provider_match {
        "provider"
    } else {
        "terminal_failure"
    }
}

fn blocked_reason(issue: &Value, latest_run: &Value, marker: &Value) -> Option<&'static str> {
    (latest_run.as_object().is_some_and(|run| !run.is_empty())
        && marker["reason"] == BLOCK_REASON
        && marker["source_run"] == latest_run["id"]
        && marker["agent"] == issue["assignee_id"])
        .then_some(BLOCK_REASON)
}

/// Read the caller-supplied run, issue, decoded metadata marker, provider
/// matcher result and timestamp. See `testdata/README.md` for the wire shape.
pub fn evaluate(request: Value) -> Result<Value, OpsError> {
    let run = &request["latest_run"];
    let status = run["status"]
        .as_str()
        .ok_or_else(|| OpsError::invalid_request("latest_run.status must be a string"))?;
    let provider_match = request["provider_match"]
        .as_bool()
        .ok_or_else(|| OpsError::invalid_request("provider_match must be a boolean"))?;
    let issue = &request["issue"];
    if !issue.is_object() {
        return Err(OpsError::invalid_request("issue must be an object"));
    }
    let timestamp = request["timestamp"]
        .as_str()
        .and_then(stamp::parse)
        .ok_or_else(|| OpsError::invalid_request("native_stall_timestamp_unknown"))?;
    Ok(json!({
        "classification": classification(run, status, provider_match),
        "timestamp": timestamp,
        "blocked_reason": blocked_reason(issue, run, &request["decoded_metadata"]),
    }))
}
