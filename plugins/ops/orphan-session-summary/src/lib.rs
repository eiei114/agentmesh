//! Orphan-session summary decisions ported from
//! `log_aware_token_efficiency_common.py`.
//!
//! The crate ports `normalize_path`, `_compact_orphan_session`,
//! `select_top_orphan_sessions`, `build_orphan_session_observation_event`,
//! `_sum_event_field`, `compare_log_aware_summaries` and the
//! `DEFAULT_ORPHAN_TOKEN_THRESHOLD` constant. Callers pass already-loaded
//! synthetic records as JSON values. Session-file scanning, event collection,
//! live usage and daemon-log reads stay in the vault wrapper: the crate has no
//! file, shell, network, Multica or git side effects.
//!
//! Python raises `ValueError`, `TypeError` or `AttributeError` on malformed
//! metric values and wrong container shapes. Those cases are reported as an
//! `invalid_request` error with [`MALFORMED_MESSAGE`].
//!
//! Known bounds: integers are held as `i128`, strings accepted by Python's
//! `int()` are limited to ASCII digits, `float()` strings that parse to
//! `nan`/`inf` are rejected, and `normalize_path` does not expand `~` (it reads
//! no environment).

use std::collections::HashSet;

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Map, Value};

/// Token total above which a session counts as an orphan candidate.
pub const DEFAULT_ORPHAN_TOKEN_THRESHOLD: i64 = 250_000;

/// Message used where Python raises on a malformed value or container shape.
pub const MALFORMED_MESSAGE: &str = "malformed input value";

static NULL: Value = Value::Null;

fn malformed() -> OpsError {
    OpsError::invalid_request(MALFORMED_MESSAGE)
}

/// Python truthiness of a JSON value.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_none_or(|float| float != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `value in (None, "", [], 0, 0.0[, False])`.
fn is_dropped(value: &Value, drop_false: bool) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(flag) => drop_false && !*flag,
        Value::Number(number) => number.as_f64().is_some_and(|float| float == 0.0),
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(_) => false,
    }
}

/// Python whitespace as accepted around `int()` / `float()` text.
fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Digits with single underscores between digits, as Python numeric text allows.
fn strip_underscores(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (index, &c) in chars.iter().enumerate() {
        if c == '_' {
            let before = index.checked_sub(1).and_then(|i| chars.get(i));
            let after = chars.get(index + 1);
            let digit = |c: Option<&char>| c.is_some_and(char::is_ascii_digit);
            if !digit(before) || !digit(after) {
                return None;
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

fn parse_py_int(text: &str) -> Result<i128, OpsError> {
    let trimmed = text.trim_matches(py_space);
    let (negative, digits) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let digits = strip_underscores(digits).ok_or_else(malformed)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(malformed());
    }
    let magnitude: i128 = digits.parse().map_err(|_| malformed())?;
    Ok(if negative { -magnitude } else { magnitude })
}

fn parse_py_float(text: &str) -> Result<f64, OpsError> {
    let trimmed = text.trim_matches(py_space);
    let cleaned = strip_underscores(trimmed).ok_or_else(malformed)?;
    let parsed: f64 = cleaned.parse().map_err(|_| malformed())?;
    if parsed.is_finite() {
        Ok(parsed)
    } else {
        Err(malformed())
    }
}

/// `int(value or 0)`.
fn int_or_zero(value: &Value) -> Result<i128, OpsError> {
    match value {
        Value::Null => Ok(0),
        Value::Bool(flag) => Ok(i128::from(*flag)),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                Ok(i128::from(int))
            } else if let Some(int) = number.as_u64() {
                Ok(i128::from(int))
            } else {
                let float = number.as_f64().ok_or_else(malformed)?;
                if !float.is_finite() || float.abs() >= 1.0e38 {
                    return Err(malformed());
                }
                #[allow(clippy::cast_possible_truncation)]
                Ok(float.trunc() as i128)
            }
        }
        Value::String(text) if text.is_empty() => Ok(0),
        Value::String(text) => parse_py_int(text),
        Value::Array(items) if items.is_empty() => Ok(0),
        Value::Object(map) if map.is_empty() => Ok(0),
        Value::Array(_) | Value::Object(_) => Err(malformed()),
    }
}

/// `float(value or 0.0)`.
fn float_or_zero(value: &Value) -> Result<f64, OpsError> {
    match value {
        Value::Null => Ok(0.0),
        Value::Bool(flag) => Ok(f64::from(u8::from(*flag))),
        Value::Number(number) => number.as_f64().ok_or_else(malformed),
        Value::String(text) if text.is_empty() => Ok(0.0),
        Value::String(text) => parse_py_float(text),
        Value::Array(items) if items.is_empty() => Ok(0.0),
        Value::Object(map) if map.is_empty() => Ok(0.0),
        Value::Array(_) | Value::Object(_) => Err(malformed()),
    }
}

/// `x.get(key)` after `x = value or {}`: falsy gives `None`, a non-object
/// raises in Python.
fn lookup<'a>(value: &'a Value, key: &str) -> Result<&'a Value, OpsError> {
    if !truthy(value) {
        return Ok(&NULL);
    }
    match value {
        Value::Object(map) => Ok(map.get(key).unwrap_or(&NULL)),
        _ => Err(malformed()),
    }
}

fn require_object<'a>(value: &'a Value, field: &str) -> Result<&'a Map<String, Value>, OpsError> {
    value
        .as_object()
        .ok_or_else(|| OpsError::invalid_request(format!("{field} must be an object")))
}

/// Python `normalize_path` for string input: separators become `/`, repeated
/// separators and `.` segments collapse, a trailing separator is dropped, and
/// the result is lowercased. An empty or missing value gives `""`.
///
/// `~` is not expanded because the crate reads no environment.
pub fn normalize_path(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let unified = value.replace('\\', "/");
    let rooted = unified.starts_with('/');
    let segments: Vec<&str> = unified
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    let mut joined = segments.join("/");
    if rooted {
        joined.insert(0, '/');
    }
    if joined.is_empty() {
        joined.push('.');
    }
    joined.to_lowercase()
}

/// Python `_compact_orphan_session`.
pub fn compact_orphan_session(item: &Map<String, Value>) -> Map<String, Value> {
    let get = |key: &str| item.get(key).unwrap_or(&NULL);
    let warnings = if truthy(get("warnings")) {
        get("warnings").clone()
    } else {
        json!([])
    };
    let pairs: [(&str, Value); 9] = [
        ("session_id", get("session_id").clone()),
        ("session_file", get("session_file").clone()),
        ("total_tokens", get("total_tokens").clone()),
        ("estimated_cost_usd", get("estimated_cost_usd").clone()),
        ("size_bytes", get("size_bytes").clone()),
        ("model_provider", get("model_provider").clone()),
        ("model_name", get("model_name").clone()),
        ("warnings", warnings),
        ("missing_usage", get("missing_usage").clone()),
    ];
    pairs
        .into_iter()
        .filter(|(_, value)| !is_dropped(value, true))
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

struct Candidate<'a> {
    item: &'a Map<String, Value>,
    key: (i128, i128, f64),
}

/// Python `select_top_orphan_sessions` over already-loaded records.
pub fn select_top_orphan_sessions(
    session_records: &[Value],
    matched_session_paths: &HashSet<String>,
    top_n: i64,
    token_threshold: i128,
) -> Result<Vec<Value>, OpsError> {
    let mut candidates: Vec<Candidate<'_>> = Vec::new();
    for record in session_records {
        let item = record.as_object().ok_or_else(malformed)?;
        let session_file = match item.get("session_file").unwrap_or(&NULL) {
            Value::Null => "",
            Value::String(text) => text,
            _ => {
                return Err(OpsError::invalid_request(
                    "session_file must be a string or null",
                ))
            }
        };
        let path = normalize_path(session_file);
        if path.is_empty() || matched_session_paths.contains(&path) {
            continue;
        }
        let total_tokens = int_or_zero(item.get("total_tokens").unwrap_or(&NULL))?;
        let size_bytes = int_or_zero(item.get("size_bytes").unwrap_or(&NULL))?;
        let missing_usage = truthy(item.get("missing_usage").unwrap_or(&NULL)) || total_tokens == 0;
        if total_tokens > token_threshold || (missing_usage && size_bytes > 0) {
            let cost = float_or_zero(item.get("estimated_cost_usd").unwrap_or(&NULL))?;
            candidates.push(Candidate {
                item,
                key: (total_tokens, size_bytes, cost),
            });
        }
    }
    // Python sorts descending with a stable sort, so equal keys keep input order.
    candidates.sort_by(|a, b| {
        b.key.0.cmp(&a.key.0).then(b.key.1.cmp(&a.key.1)).then(
            b.key
                .2
                .partial_cmp(&a.key.2)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    let len = i64::try_from(candidates.len()).unwrap_or(i64::MAX);
    let end = if top_n >= 0 {
        top_n.min(len)
    } else {
        (len + top_n).max(0)
    };
    let end = usize::try_from(end).unwrap_or(0);
    Ok(candidates
        .iter()
        .take(end)
        .map(|candidate| Value::Object(compact_orphan_session(candidate.item)))
        .collect())
}

/// Python `str()` of a JSON scalar, for f-string interpolation.
fn py_str(value: &Value) -> Result<String, OpsError> {
    match value {
        Value::Null => Ok("None".to_owned()),
        Value::Bool(true) => Ok("True".to_owned()),
        Value::Bool(false) => Ok("False".to_owned()),
        Value::String(text) => Ok(text.clone()),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                Ok(int.to_string())
            } else if let Some(int) = number.as_u64() {
                Ok(int.to_string())
            } else {
                number.as_f64().map(py_float_repr).ok_or_else(malformed)
            }
        }
        Value::Array(_) | Value::Object(_) => Err(OpsError::invalid_request(
            "session_id and total_tokens must be scalars",
        )),
    }
}

/// Python `repr(float)` (shortest round-trip digits, Python's exponent rule).
fn py_float_repr(value: f64) -> String {
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .to_owned();
    }
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let decpt = exponent + 1;
    let sign = if value < 0.0 { "-" } else { "" };
    let width = i32::try_from(digits.len()).unwrap_or(i32::MAX);
    let body = if !(-3..=16).contains(&decpt) {
        let exp = decpt - 1;
        let lead = &digits[..1];
        let rest = &digits[1..];
        let frac = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        let exp_sign = if exp < 0 { '-' } else { '+' };
        format!("{lead}{frac}e{exp_sign}{:02}", exp.abs())
    } else if decpt <= 0 {
        format!(
            "0.{}{digits}",
            "0".repeat(usize::try_from(-decpt).unwrap_or(0))
        )
    } else if decpt < width {
        let split = usize::try_from(decpt).unwrap_or(0);
        format!("{}.{}", &digits[..split], &digits[split..])
    } else {
        format!(
            "{digits}{}.0",
            "0".repeat(usize::try_from(decpt - width).unwrap_or(0))
        )
    };
    format!("{sign}{body}")
}

/// Python `build_orphan_session_observation_event`.
pub fn build_orphan_session_observation_event(
    session: &Map<String, Value>,
    ts: &str,
) -> Result<Map<String, Value>, OpsError> {
    let compact = compact_orphan_session(session);
    let get = |key: &str| compact.get(key).cloned().unwrap_or(Value::Null);
    let tokens_for_note = compact.get("total_tokens").cloned().unwrap_or(json!(0));
    let note = format!(
        "session-inventory orphan_session session_id={} tokens={}",
        py_str(&get("session_id"))?,
        py_str(&tokens_for_note)?
    );
    let mut event: Vec<(&str, Value)> = vec![
        ("ts", json!(ts)),
        ("feedback_version", json!(1)),
        ("event_type", json!("observation")),
        ("project_key", json!("workspace")),
        ("controller", json!("session-inventory")),
        ("run_source", json!("cli")),
        ("result_type", json!("orphan_session")),
        ("feedback_code", json!("session_inventory_orphan_session")),
        ("feedback_note", json!(note)),
        ("session_file", get("session_file")),
        ("session_id", get("session_id")),
        ("total_tokens", get("total_tokens")),
        ("estimated_cost_usd", get("estimated_cost_usd")),
        ("model_provider", get("model_provider")),
    ];
    let warnings = get("warnings");
    if truthy(&warnings) {
        event.push(("warning_flags", warnings));
    }
    if truthy(&get("missing_usage")) {
        event.push(("missing_usage", json!(true)));
    }
    Ok(event
        .into_iter()
        .filter(|(_, value)| !is_dropped(value, false))
        .map(|(key, value)| (key.to_owned(), value))
        .collect())
}

/// Python `_sum_event_field`. `summary` must be a JSON object.
pub fn sum_event_field(summary: &Value, controller: &str, field: &str) -> Result<i128, OpsError> {
    let events = lookup(summary, "top_scan_events")?;
    let mut total: i128 = 0;
    if truthy(events) {
        let Value::Array(events) = events else {
            return Err(malformed());
        };
        for event in events {
            let event = event.as_object().ok_or_else(malformed)?;
            if event.get("controller").and_then(Value::as_str) != Some(controller) {
                continue;
            }
            // `isinstance(value, int)` is true for bool in Python.
            match event.get(field).unwrap_or(&NULL) {
                Value::Bool(flag) => total += i128::from(*flag),
                value @ Value::Number(number) if !number.is_f64() => {
                    total += int_or_zero(value)?;
                }
                _ => {}
            }
        }
    }
    let ctrl = lookup(lookup(summary, "controller_summary")?, controller)?;
    let fallback = int_or_zero(lookup(ctrl, field)?)?;
    Ok(if total == 0 { fallback } else { total })
}

/// Python `round(x, 6)`.
fn round6(value: f64) -> Result<f64, OpsError> {
    format!("{value:.6}")
        .parse::<f64>()
        .ok()
        .filter(|rounded| rounded.is_finite())
        .ok_or_else(malformed)
}

/// `value if isinstance(value, (int, float)) else 0` (bool counts as int).
fn metric_value(value: &Value) -> Value {
    match value {
        Value::Number(_) | Value::Bool(_) => value.clone(),
        _ => json!(0),
    }
}

fn metric_float(value: &Value) -> f64 {
    match value {
        Value::Bool(flag) => f64::from(u8::from(*flag)),
        other => other.as_f64().unwrap_or(0.0),
    }
}

fn controller_value(summary: &Value, controller: &str, field: &str) -> Result<Value, OpsError> {
    let data = lookup(lookup(summary, "controller_summary")?, controller)?;
    Ok(metric_value(lookup(data, field)?))
}

/// `int(value) if isinstance(value, (int, float)) else 0`, as a JSON integer.
fn join_count(summary: &Value, field: &str) -> Result<Value, OpsError> {
    let value = lookup(lookup(summary, "session_feedback_join")?, field)?;
    match value {
        Value::Number(_) | Value::Bool(_) => {
            let int = int_or_zero(value)?;
            let int = i64::try_from(int).map_err(|_| malformed())?;
            Ok(json!(int))
        }
        _ => Ok(json!(0)),
    }
}

/// `(downgraded, actionable)` counts of daemon findings.
fn daemon_ratio(summary: &Value) -> Result<(i64, i64), OpsError> {
    let health = lookup(summary, "daemon_health")?;
    let findings = lookup(health, "findings")?;
    let (mut downgraded, mut actionable) = (0, 0);
    match findings {
        Value::Array(items) => {
            for item in items {
                if let Value::Object(map) = item {
                    if truthy(map.get("downgraded").unwrap_or(&NULL)) {
                        downgraded += 1;
                    } else {
                        actionable += 1;
                    }
                }
            }
        }
        // Falsy, or iterating keys/characters that are never dicts.
        Value::Null | Value::String(_) | Value::Object(_) => {}
        Value::Bool(false) => {}
        // Python raises `TypeError` iterating a truthy number or `True`.
        Value::Bool(true) | Value::Number(_) => {
            if truthy(findings) {
                return Err(malformed());
            }
        }
    }
    Ok((downgraded, actionable))
}

/// Python `compare_log_aware_summaries`. Both summaries must be JSON objects.
pub fn compare_log_aware_summaries(baseline: &Value, current: &Value) -> Result<Value, OpsError> {
    let (base_down, base_act) = daemon_ratio(baseline)?;
    let (cur_down, cur_act) = daemon_ratio(current)?;
    let pair = |before: Result<Value, OpsError>, after: Result<Value, OpsError>| {
        before.and_then(|b| after.map(|a| (b, a)))
    };
    let sum = |summary: &Value, field: &str| -> Result<Value, OpsError> {
        Ok(json!(i64::try_from(sum_event_field(
            summary,
            "coderabbit-monitor",
            field
        )?)
        .map_err(|_| malformed())?))
    };
    let rows: Vec<(&str, (Value, Value))> = vec![
        (
            "orphan_session_count",
            pair(
                join_count(baseline, "orphan_session_count"),
                join_count(current, "orphan_session_count"),
            )?,
        ),
        (
            "coderabbit_no_candidate_rate",
            pair(
                controller_value(baseline, "coderabbit-monitor", "no_candidate_rate"),
                controller_value(current, "coderabbit-monitor", "no_candidate_rate"),
            )?,
        ),
        (
            "coderabbit_prefilter_skip_count",
            pair(
                controller_value(baseline, "coderabbit-monitor", "prefilter_skip_count"),
                controller_value(current, "coderabbit-monitor", "prefilter_skip_count"),
            )?,
        ),
        (
            "coderabbit_github_prs_checked",
            pair(
                sum(baseline, "github_prs_checked"),
                sum(current, "github_prs_checked"),
            )?,
        ),
        (
            "review_harvester_total_tokens",
            pair(
                controller_value(baseline, "review-feedback-harvester", "total_tokens"),
                controller_value(current, "review-feedback-harvester", "total_tokens"),
            )?,
        ),
        (
            "review_harvester_cooldown_skip_count",
            pair(
                controller_value(baseline, "review-feedback-harvester", "cooldown_skip_count"),
                controller_value(current, "review-feedback-harvester", "cooldown_skip_count"),
            )?,
        ),
        (
            "daemon_findings_downgraded",
            (json!(base_down), json!(cur_down)),
        ),
        (
            "daemon_findings_actionable",
            (json!(base_act), json!(cur_act)),
        ),
    ];
    let mut out = Vec::with_capacity(rows.len());
    for (metric, (before, after)) in rows {
        let delta = round6(metric_float(&after) - metric_float(&before))?;
        out.push(json!({
            "metric": metric,
            "before": before,
            "after": after,
            "delta": delta,
        }));
    }
    Ok(json!({ "comparison_rows": out }))
}

fn request_object(request: &Value) -> Result<&Map<String, Value>, OpsError> {
    request
        .as_object()
        .ok_or_else(|| OpsError::invalid_request("request must be a JSON object"))
}

fn field_object<'a>(
    request: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a Map<String, Value>, OpsError> {
    require_object(
        request
            .get(field)
            .ok_or_else(|| OpsError::invalid_request(format!("{field} is required")))?,
        field,
    )
}

fn field_string<'a>(request: &'a Map<String, Value>, field: &str) -> Result<&'a str, OpsError> {
    request
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| OpsError::invalid_request(format!("{field} must be a string")))
}

fn optional_int(request: &Map<String, Value>, field: &str, default: i64) -> Result<i64, OpsError> {
    match request.get(field) {
        None => Ok(default),
        Some(value) => value
            .as_i64()
            .ok_or_else(|| OpsError::invalid_request(format!("{field} must be an integer"))),
    }
}

fn summary_field<'a>(request: &'a Map<String, Value>, field: &str) -> Result<&'a Value, OpsError> {
    let value = request
        .get(field)
        .ok_or_else(|| OpsError::invalid_request(format!("{field} is required")))?;
    require_object(value, field)?;
    Ok(value)
}

/// Handle one stdio request. See the README for the operations.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    let request = request_object(&request)?;
    let op = request
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| OpsError::invalid_request("op must be a string"))?;
    match op {
        "default_orphan_token_threshold" => Ok(json!(DEFAULT_ORPHAN_TOKEN_THRESHOLD)),
        "normalize_path" => match request.get("value").unwrap_or(&NULL) {
            Value::Null => Ok(json!("")),
            Value::String(text) => Ok(json!(normalize_path(text))),
            _ => Err(OpsError::invalid_request("value must be a string or null")),
        },
        "compact_orphan_session" => Ok(Value::Object(compact_orphan_session(field_object(
            request, "item",
        )?))),
        "select_top_orphan_sessions" => {
            let records = request
                .get("session_records")
                .and_then(Value::as_array)
                .ok_or_else(|| OpsError::invalid_request("session_records must be an array"))?;
            let matched: HashSet<String> = match request.get("matched_session_paths") {
                None | Some(Value::Null) => HashSet::new(),
                Some(Value::Array(paths)) => paths
                    .iter()
                    .map(|path| {
                        path.as_str().map(str::to_owned).ok_or_else(|| {
                            OpsError::invalid_request("matched_session_paths must be strings")
                        })
                    })
                    .collect::<Result<_, _>>()?,
                Some(_) => {
                    return Err(OpsError::invalid_request(
                        "matched_session_paths must be an array",
                    ))
                }
            };
            let top_n = optional_int(request, "top_n", 10)?;
            let threshold =
                optional_int(request, "token_threshold", DEFAULT_ORPHAN_TOKEN_THRESHOLD)?;
            Ok(Value::Array(select_top_orphan_sessions(
                records,
                &matched,
                top_n,
                i128::from(threshold),
            )?))
        }
        "build_orphan_session_observation_event" => {
            let session = field_object(request, "session")?;
            let ts = field_string(request, "ts")?;
            Ok(Value::Object(build_orphan_session_observation_event(
                session, ts,
            )?))
        }
        "sum_event_field" => {
            let summary = summary_field(request, "summary")?;
            let controller = field_string(request, "controller")?;
            let field = field_string(request, "field")?;
            Ok(json!(i64::try_from(sum_event_field(
                summary, controller, field
            )?)
            .map_err(|_| malformed())?))
        }
        "compare_log_aware_summaries" => compare_log_aware_summaries(
            summary_field(request, "baseline")?,
            summary_field(request, "current")?,
        ),
        other => Err(OpsError::invalid_request(format!("unknown op: {other}"))),
    }
}
