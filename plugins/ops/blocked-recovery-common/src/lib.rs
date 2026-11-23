//! Read-only port of the blocked recovery ownership and assessment checks.

use std::borrow::Cow;

use agentmesh_ops_kit::OpsError;
use serde_json::Value;

const WINDOWS_WRITER: &str = "windows-tick";
const ROUTED_ASSESSOR: &str = "Pi Blocked Recovery Luna Assessor";

/// Whether the supplied policy assigns blocked recovery to the Windows sweep.
pub fn owns_blocked(policy: &Value) -> bool {
    policy.pointer("/sweeper/enabled").and_then(Value::as_bool) == Some(true)
        && policy.pointer("/sweeper/writer").and_then(Value::as_str) == Some(WINDOWS_WRITER)
}

// Like Python's metadata accessor, unwrap one serialized metadata value at
// each layer. Plain strings are not trimmed or recursively decoded.
fn decode_metadata(value: &Value) -> Cow<'_, Value> {
    if let Some(text) = value.as_str() {
        let text =
            text.trim_matches(|ch: char| ch.is_whitespace() || ('\x1c'..='\x1f').contains(&ch));
        if matches!(text, "true" | "false")
            || text.starts_with(['{', '['])
            || (text.starts_with('"') && text.ends_with('"'))
        {
            return serde_json::from_str(text).map_or(Cow::Borrowed(value), Cow::Owned);
        }
    }
    Cow::Borrowed(value)
}

/// Return the decoded context object unchanged if it belongs to the routed
/// Windows assessor. Accept either a bare issue or an `{"issue": {...}}` wrapper.
pub fn assessment_context(issue: &Value) -> Option<Value> {
    let issue = issue
        .get("issue")
        .filter(|inner| inner.is_object())
        .unwrap_or(issue);
    let metadata = decode_metadata(issue.get("metadata")?);
    let context = decode_metadata(metadata.get("blocked_recovery_context")?);
    if context.get("writer").and_then(Value::as_str) == Some(WINDOWS_WRITER)
        && context.get("mode").and_then(Value::as_str) == Some("assess")
        && decode_metadata(metadata.get("routed_agent")?).as_str() == Some(ROUTED_ASSESSOR)
    {
        Some(context.into_owned())
    } else {
        None
    }
}

fn object_field<'a>(request: &'a Value, field: &str) -> Result<&'a Value, OpsError> {
    request
        .get(field)
        .filter(|value| value.is_object())
        .ok_or_else(|| OpsError::invalid_request(format!("{field} must be an object")))
}

/// Evaluate one `owns_blocked` or `assessment_context` JSON request.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    match request.get("op").and_then(Value::as_str) {
        Some("owns_blocked") => Ok(Value::Bool(owns_blocked(object_field(&request, "policy")?))),
        Some("assessment_context") => {
            Ok(assessment_context(object_field(&request, "issue")?).unwrap_or(Value::Null))
        }
        _ => Err(OpsError::invalid_request("unknown or missing op")),
    }
}
