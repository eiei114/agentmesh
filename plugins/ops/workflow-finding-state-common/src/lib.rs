//! Stable identity and recurrence-state helpers for workflow-health findings.
//!
//! The event fold accepts JSONL lines already supplied by its caller. This
//! crate does not read event files or access Multica, Git, or the network.

use std::collections::BTreeMap;

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use agentmesh_ops_kit::OpsError;

/// Parse an ISO event timestamp and convert it to UTC, matching the Python
/// helper's `datetime.fromisoformat(...).astimezone(timezone.utc)` behavior.
pub fn parse_event_dt(value: Option<&str>) -> Option<DateTime<Utc>> {
    let value = value.filter(|value| !value.is_empty())?;
    let normalized = normalize_python_fraction(&normalize_python_offset(&normalize_separator(
        &value.replace('Z', "+00:00"),
    )));

    let parsed = parse_aware_datetime(&normalized)
        .or_else(|| parse_naive_datetime(&normalized).and_then(local_to_utc))?;
    let micros = parsed.timestamp_subsec_micros();
    parsed.with_nanosecond(micros * 1_000)
}

fn parse_aware_datetime(value: &str) -> Option<DateTime<Utc>> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Some(parsed.with_timezone(&Utc));
    }

    let normalized = normalize_separator(value);
    [
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M:%S%:z",
        "%Y-%m-%dT%H:%M%:z",
        "%Y-%m-%dT%H:%M:%S%.f%z",
        "%Y-%m-%dT%H:%M:%S%z",
        "%Y-%m-%dT%H:%M%z",
        "%Y%m%dT%H:%M:%S%.f%:z",
        "%Y%m%dT%H:%M:%S%:z",
        "%Y%m%dT%H:%M%:z",
        "%Y%m%dT%H%M%S%.f%:z",
        "%Y%m%dT%H%M%S%:z",
        "%Y%m%dT%H%M%:z",
        "%Y%m%dT%H:%M:%S%.f%z",
        "%Y%m%dT%H:%M:%S%z",
        "%Y%m%dT%H:%M%z",
        "%Y%m%dT%H%M%S%.f%z",
        "%Y%m%dT%H%M%S%z",
        "%Y%m%dT%H%M%z",
    ]
    .iter()
    .find_map(|format| {
        DateTime::parse_from_str(&normalized, format)
            .ok()
            .map(|parsed| parsed.with_timezone(&Utc))
    })
}

fn parse_naive_datetime(value: &str) -> Option<NaiveDateTime> {
    let normalized = normalize_separator(value);
    [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y%m%dT%H:%M:%S%.f",
        "%Y%m%dT%H:%M:%S",
        "%Y%m%dT%H:%M",
        "%Y%m%dT%H%M%S%.f",
        "%Y%m%dT%H%M%S",
        "%Y%m%dT%H%M",
    ]
    .iter()
    .find_map(|format| NaiveDateTime::parse_from_str(&normalized, format).ok())
    .or_else(|| {
        NaiveDate::parse_from_str(&normalized, "%Y-%m-%d")
            .ok()
            .and_then(|date| date.and_hms_opt(0, 0, 0))
    })
    .or_else(|| {
        NaiveDate::parse_from_str(&normalized, "%Y%m%d")
            .ok()
            .and_then(|date| date.and_hms_opt(0, 0, 0))
    })
}

fn normalize_separator(value: &str) -> String {
    if value.len() > 10 && value.is_char_boundary(10) {
        let (date, rest) = value.split_at(10);
        if date.as_bytes().get(4) == Some(&b'-') && date.as_bytes().get(7) == Some(&b'-') {
            return format!("{date}T{}", rest.chars().skip(1).collect::<String>());
        }
    }
    if value.len() > 8 && value.is_char_boundary(8) {
        let (date, rest) = value.split_at(8);
        if date.bytes().all(|byte| byte.is_ascii_digit()) {
            return format!("{date}T{}", rest.chars().skip(1).collect::<String>());
        }
    }
    value.to_owned()
}

fn normalize_python_fraction(value: &str) -> String {
    let bytes = value.as_bytes();
    let (prefix_end, fraction_start) =
        if bytes.len() > 20 && matches!(bytes.get(19), Some(b'.' | b',')) {
            (19, 20)
        } else if bytes.len() > 16 && matches!(bytes.get(15), Some(b'.' | b',')) {
            (15, 16)
        } else {
            return value.to_owned();
        };
    let fraction_end = bytes[fraction_start..]
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .map_or(bytes.len(), |offset| fraction_start + offset);
    if fraction_end == fraction_start {
        return value.to_owned();
    }
    let retained_end = fraction_start + (fraction_end - fraction_start).min(6);
    format!(
        "{}.{}{}",
        &value[..prefix_end],
        &value[fraction_start..retained_end],
        &value[fraction_end..]
    )
}

fn normalize_python_offset(value: &str) -> String {
    let Some(separator) = value.find('T') else {
        return value.to_owned();
    };
    let Some(offset_start) = value[separator..]
        .rfind(['+', '-'])
        .map(|offset| separator + offset)
    else {
        return value.to_owned();
    };
    let offset = &value[offset_start..];
    if offset.len() == 3
        && offset.as_bytes()[1..]
            .iter()
            .all(|byte| byte.is_ascii_digit())
    {
        format!("{}{}:00", &value[..offset_start], offset)
    } else {
        value.to_owned()
    }
}

fn local_to_utc(value: NaiveDateTime) -> Option<DateTime<Utc>> {
    Local
        .from_local_datetime(&value)
        .earliest()
        .map(|parsed| parsed.with_timezone(&Utc))
}

/// Return the SHA-256 identity for the Python-stable `code` and `path` fields.
/// Evidence, severity, and every other finding property are deliberately
/// ignored.
pub fn finding_fingerprint(finding: &Value) -> String {
    let mut stable = Map::new();
    stable.insert(
        "code".to_owned(),
        finding.get("code").cloned().unwrap_or(Value::Null),
    );
    stable.insert(
        "path".to_owned(),
        finding.get("path").cloned().unwrap_or(Value::Null),
    );
    let serialized = python_json(&Value::Object(stable));
    let digest = Sha256::digest(serialized.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn python_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => {
            serde_json::to_string(value).expect("serializing a string cannot fail")
        }
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(python_json)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!(
                    "{}: {}",
                    python_json(&Value::String(key.clone())),
                    python_json(value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn python_isoformat(value: DateTime<Utc>) -> String {
    let mut formatted = value.format("%Y-%m-%dT%H:%M:%S").to_string();
    let micros = value.timestamp_subsec_micros();
    if micros != 0 {
        formatted.push_str(&format!(".{micros:06}"));
    }
    formatted.push_str("+00:00");
    formatted
}

/// Classify the current state from a prior finding record.
pub fn current_finding_state(previous: Option<&Value>) -> &'static str {
    let Some(previous) = previous.filter(|value| is_truthy(value)) else {
        return "new";
    };
    let prior_state = previous
        .get("finding_state")
        .filter(|value| is_truthy(value))
        .and_then(Value::as_str)
        .unwrap_or("");
    if prior_state == "acknowledged" {
        "acknowledged"
    } else if prior_state == "resolved" {
        "new"
    } else {
        "unchanged"
    }
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|number| number != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

/// Fold caller-supplied synthetic JSONL records into the latest state for each
/// fingerprint. The caller supplies records in the same order that sorted
/// Python event files and their lines would be read.
pub fn prior_finding_state(
    since: DateTime<Utc>,
    event_lines: &[String],
) -> BTreeMap<String, Value> {
    let mut state = BTreeMap::new();
    for line in event_lines {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if event.get("controller").and_then(Value::as_str) != Some("workflow-health-audit") {
            continue;
        }

        let timestamp = event.get("ts").and_then(Value::as_str);
        if parse_event_dt(timestamp).is_some_and(|timestamp| timestamp < since) {
            continue;
        }
        let Some(items) = event.get("finding_fingerprints").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let Some(fingerprint) = item.get("fingerprint") else {
                continue;
            };
            if !is_truthy(fingerprint) {
                continue;
            }
            state.insert(python_str(fingerprint), item.clone());
        }
    }
    state
}

fn python_str(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(python_repr)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(values) => format!(
            "{{{}}}",
            values
                .iter()
                .map(|(key, value)| format!(
                    "{}: {}",
                    python_repr(&Value::String(key.clone())),
                    python_repr(value)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn python_repr(value: &Value) -> String {
    match value {
        Value::String(value) => {
            let quote = if value.contains('\'') && !value.contains('"') {
                '"'
            } else {
                '\''
            };
            let mut result = String::with_capacity(value.len() + 2);
            result.push(quote);
            for ch in value.chars() {
                match ch {
                    '\\' => result.push_str("\\\\"),
                    '\n' => result.push_str("\\n"),
                    '\r' => result.push_str("\\r"),
                    '\t' => result.push_str("\\t"),
                    ch if ch == quote => {
                        result.push('\\');
                        result.push(ch);
                    }
                    ch => result.push(ch),
                }
            }
            result.push(quote);
            result
        }
        _ => python_str(value),
    }
}

/// Handle one stdio JSON request.
pub fn handle_request(request: Value) -> Result<Value, OpsError> {
    let operation = request
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| OpsError::invalid_request("operation must be a string"))?;
    match operation {
        "parse_event_dt" => {
            let value = request.get("value").and_then(Value::as_str);
            let parsed = parse_event_dt(value).map(python_isoformat);
            Ok(json!({ "value": parsed }))
        }
        "finding_fingerprint" => {
            let finding = request
                .get("finding")
                .filter(|value| value.is_object())
                .ok_or_else(|| OpsError::invalid_request("finding must be an object"))?;
            Ok(json!(finding_fingerprint(finding)))
        }
        "current_finding_state" => {
            let previous = request.get("previous");
            Ok(json!(current_finding_state(previous)))
        }
        "prior_finding_state" => {
            let since_text = request
                .get("since")
                .and_then(Value::as_str)
                .ok_or_else(|| OpsError::invalid_request("since must be an ISO timestamp"))?;
            let since = parse_event_dt(Some(since_text))
                .ok_or_else(|| OpsError::invalid_request("since must be a valid ISO timestamp"))?;
            let lines = request
                .get("event_lines")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    OpsError::invalid_request("event_lines must be an array of strings")
                })?;
            let lines = lines
                .iter()
                .map(|line| {
                    line.as_str().map(str::to_owned).ok_or_else(|| {
                        OpsError::invalid_request("event_lines must contain only strings")
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let state = prior_finding_state(since, &lines)
                .into_iter()
                .collect::<Map<String, Value>>();
            Ok(Value::Object(state))
        }
        _ => Err(OpsError::invalid_request("unknown operation")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_event_dt_normalizes_offsets_and_rejects_malformed_values() {
        assert_eq!(
            python_isoformat(
                parse_event_dt(Some("2026-08-02T12:00:00+09:00")).expect("valid timestamp")
            ),
            "2026-08-02T03:00:00+00:00"
        );
        assert_eq!(
            python_isoformat(
                parse_event_dt(Some("2026-08-02T12:00:00.123456789+09:00"))
                    .expect("fractional timestamp")
            ),
            "2026-08-02T03:00:00.123456+00:00"
        );
        assert!(parse_event_dt(Some("not-a-date")).is_none());
        assert!(parse_event_dt(None).is_none());
    }

    #[test]
    fn fingerprint_uses_python_json_spacing_and_stable_fields_only() {
        let first = json!({"code":"audit_error", "path":"src/é.rs", "severity":"warning"});
        let second = json!({"path":"src/é.rs", "code":"audit_error", "detail":{"count":99}});
        assert_eq!(finding_fingerprint(&first), finding_fingerprint(&second));
    }
}
