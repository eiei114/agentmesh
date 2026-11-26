//! Read-only port of the blocked recovery ownership and assessment checks.

use std::borrow::Cow;
use std::collections::BTreeMap;

use agentmesh_ops_kit::OpsError;
use serde_json::{value::RawValue, Value};

const WINDOWS_WRITER: &str = "windows-tick";
const ROUTED_ASSESSOR: &str = "Pi Blocked Recovery Luna Assessor";

/// Whether the supplied policy assigns blocked recovery to the Windows sweep.
pub fn owns_blocked(policy: &Value) -> bool {
    serde_json::value::to_raw_value(policy)
        .ok()
        .and_then(|raw| object(&raw).map(|fields| owns_blocked_raw(&fields)))
        .unwrap_or(false)
}

type Object<'a> = BTreeMap<String, &'a RawValue>;

fn object(value: &RawValue) -> Option<Object<'_>> {
    serde_json::from_str(value.get()).ok()
}

fn string(value: &RawValue) -> Option<String> {
    serde_json::from_str(value.get()).ok()
}

fn owns_blocked_raw(policy: &Object<'_>) -> bool {
    let Some(sweeper) = policy.get("sweeper").and_then(|raw| object(raw)) else {
        return false;
    };
    sweeper
        .get("enabled")
        .is_some_and(|raw| raw.get() == "true")
        && sweeper.get("writer").and_then(|raw| string(raw)).as_deref() == Some(WINDOWS_WRITER)
}

// Like Python, unwrap one encoded value at each metadata layer. Keep the
// decoded JSON raw so that unrelated fields never go through numeric parsing.
fn decode_metadata(value: &RawValue) -> Cow<'_, RawValue> {
    if let Some(text) = string(value) {
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
/// Windows assessor, as a `Value`. Use [`handle_json`] for lossless JSON input
/// and output when the context contains numbers outside `Value`'s precision.
pub fn assessment_context(issue: &Value) -> Option<Value> {
    let raw = serde_json::value::to_raw_value(issue).ok()?;
    let context = assessment_context_raw(&object(&raw)?)?;
    serde_json::from_str(context.get()).ok()
}

fn assessment_context_raw(issue: &Object<'_>) -> Option<Box<RawValue>> {
    let inner = issue.get("issue").and_then(|raw| object(raw));
    let issue = inner.as_ref().unwrap_or(issue);
    let metadata = decode_metadata(issue.get("metadata")?);
    let metadata = object(&metadata)?;
    let context = decode_metadata(metadata.get("blocked_recovery_context")?);
    let fields = object(&context)?;
    if string(fields.get("writer")?).as_deref() == Some(WINDOWS_WRITER)
        && string(fields.get("mode")?).as_deref() == Some("assess")
        && string(&decode_metadata(metadata.get("routed_agent")?)).as_deref()
            == Some(ROUTED_ASSESSOR)
    {
        Some(context.into_owned())
    } else {
        None
    }
}

/// Evaluate a raw JSON request, returning context JSON without rounding numbers.
pub fn handle_json(input: &str) -> Result<Box<RawValue>, OpsError> {
    let request: Object<'_> = serde_json::from_str(input).map_err(|err| {
        OpsError::invalid_request(format!("request must be a JSON object: {err}"))
    })?;
    let required_object = |name| {
        request
            .get(name)
            .and_then(|raw| object(raw))
            .ok_or_else(|| OpsError::invalid_request(format!("{name} must be an object")))
    };
    match request.get("op").and_then(|raw| string(raw)).as_deref() {
        Some("owns_blocked") => Ok(if owns_blocked_raw(&required_object("policy")?) {
            RawValue::TRUE.to_owned()
        } else {
            RawValue::FALSE.to_owned()
        }),
        Some("assessment_context") => Ok(assessment_context_raw(&required_object("issue")?)
            .unwrap_or_else(|| RawValue::NULL.to_owned())),
        _ => Err(OpsError::invalid_request("unknown or missing op")),
    }
}

/// Evaluate a request already represented as a `Value`. For lossless
/// passthrough of wide JSON numbers, use [`handle_json`] instead.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    let input = serde_json::value::to_raw_value(&request)
        .map_err(|err| OpsError::invalid_request(err.to_string()))?;
    let output = handle_json(input.get())?;
    serde_json::from_str(output.get()).map_err(|err| OpsError::failed(err.to_string()))
}
