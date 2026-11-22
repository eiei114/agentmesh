//! Port of the deterministic AgentMesh maintenance request core.
//!
//! This covers request validation, slugification, the two Markdown renderers
//! and the patrol choice from the vault's `agentmesh_maintenance_requests.py`.
//! The caller supplies the raw request JSON, `now` and any relative paths, so
//! the crate never touches the filesystem, Multica, the network or git. Request
//! discovery, scope dedupe, daily caps and file writes stay in Python.
//!
//! Error messages, slug behavior and generated text are byte-compatible with
//! the Python module, which remains authoritative.

use std::fmt;

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Map, Value};

/// The only accepted request schema version.
pub const SCHEMA_VERSION: &str = "agentmesh-request.v0";
/// Accepted request kinds.
pub const ALLOWED_KINDS: [&str; 2] = ["app", "repair"];

const RELEASE_BOUNDARY_REASON: &str =
    "Private AgentMesh dev prerelease/versioning remains a separate human-owned release boundary.";

/// A request rejected by validation. The message matches Python's `RequestError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestError(pub String);

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RequestError {}

impl From<RequestError> for OpsError {
    fn from(err: RequestError) -> Self {
        OpsError::invalid_request(err.0)
    }
}

fn reject<T>(message: impl Into<String>) -> Result<T, RequestError> {
    Err(RequestError(message.into()))
}

/// A validated request, the same fields Python's `validate_request_input` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub schema_version: String,
    pub request_id: String,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub problem: String,
    pub desired_capability: String,
    pub acceptance_criteria: Vec<String>,
    pub evidence: Vec<String>,
    pub same_scope_key: String,
    pub target_app: String,
    pub user_story: String,
}

impl Request {
    /// The request as the JSON object Python returns.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "schema_version": self.schema_version,
            "request_id": self.request_id,
            "kind": self.kind,
            "title": self.title,
            "summary": self.summary,
            "problem": self.problem,
            "desired_capability": self.desired_capability,
            "acceptance_criteria": self.acceptance_criteria,
            "evidence": self.evidence,
            "same_scope_key": self.same_scope_key,
            "target_app": self.target_app,
            "user_story": self.user_story,
        })
    }
}

/// Python's `str.isspace` for one character.
fn py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Python's `str.strip()`.
fn py_strip(value: &str) -> &str {
    value.trim_matches(py_space)
}

/// Python's `value[:max]` on code points.
fn take_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

/// Python truthiness of a decoded JSON value.
fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_none_or(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn py_float_repr(number: f64) -> String {
    if number.is_infinite() {
        return if number > 0.0 { "inf" } else { "-inf" }.to_owned();
    }
    let text = format!("{number:?}");
    // Rust writes exponents as `1e16` and `1e-5`; Python as `1e+16` and `1e-05`.
    let Some((mantissa, exponent)) = text.split_once('e') else {
        return text;
    };
    let (sign, digits) = match exponent.strip_prefix('-') {
        Some(digits) => ('-', digits),
        None => ('+', exponent),
    };
    format!("{mantissa}e{sign}{digits:0>2}")
}

fn py_str_repr(value: &str) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::new();
    out.push(quote);
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python's `repr()` of a decoded JSON value.
fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => {
            if number.is_f64() {
                number.as_f64().map_or_else(String::new, py_float_repr)
            } else {
                number.to_string()
            }
        }
        Value::String(text) => py_str_repr(text),
        Value::Array(items) => {
            let parts: Vec<_> = items.iter().map(py_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(map) => {
            let parts: Vec<_> = map
                .iter()
                .map(|(key, item)| format!("{}: {}", py_str_repr(key), py_repr(item)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

/// Python's `str(value)` of a decoded JSON value.
fn py_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => py_repr(other),
    }
}

/// Python's `str(data.get(key) or default)`.
fn str_or(data: &Map<String, Value>, key: &str, default: &str) -> String {
    match data.get(key) {
        Some(value) if py_truthy(value) => py_str(value),
        _ => default.to_owned(),
    }
}

fn text(
    data: &Map<String, Value>,
    key: &str,
    minimum: usize,
    maximum: usize,
) -> Result<String, RequestError> {
    let Some(Value::String(value)) = data.get(key) else {
        return reject(format!("{key} must be a string"));
    };
    let value = py_strip(value);
    let length = value.chars().count();
    if !(minimum..=maximum).contains(&length) {
        return reject(format!("{key} length must be {minimum}..{maximum}"));
    }
    Ok(value.to_owned())
}

fn strings(
    data: &Map<String, Value>,
    key: &str,
    minimum: usize,
    maximum: usize,
) -> Result<Vec<String>, RequestError> {
    let Some(Value::Array(items)) = data.get(key) else {
        return reject(format!("{key} must contain {minimum}..{maximum} strings"));
    };
    if !(minimum..=maximum).contains(&items.len()) {
        return reject(format!("{key} must contain {minimum}..{maximum} strings"));
    }
    let mut cleaned = Vec::with_capacity(items.len());
    for item in items {
        let entry = item
            .as_str()
            .map(py_strip)
            .filter(|entry| (3..=500).contains(&entry.chars().count()));
        let Some(entry) = entry else {
            return reject(format!("{key} entries must be strings of length 3..500"));
        };
        cleaned.push(entry.to_owned());
    }
    Ok(cleaned)
}

/// Python's `slugify`.
#[must_use]
pub fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for c in value.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(c);
        } else {
            pending_dash = true;
        }
    }
    // Leading and trailing separators never reach `slug`, matching `strip("-")`.
    let truncated = take_chars(&slug, 56);
    let trimmed = truncated.trim_end_matches('-');
    if trimmed.is_empty() {
        "request".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn request_id_ok(value: &str) -> bool {
    let bytes = value.as_bytes();
    let tail = bytes.len().saturating_sub(1);
    value.is_ascii()
        && (3..=81).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes[1..=tail]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn scope_ok(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("agentmesh:") else {
        return false;
    };
    let bytes = rest.as_bytes();
    rest.is_ascii()
        && (3..=161).contains(&bytes.len())
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes[1..].iter().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(*b, b':' | b'.' | b'_' | b'-')
        })
}

/// The `YYYY-MM-DD` date of an ISO 8601 `now`, which is all Python formats.
fn date_of(now: &str) -> Result<&str, RequestError> {
    let invalid = || RequestError("now must be an ISO 8601 date-time".to_owned());
    let date = now.get(..10).ok_or_else(invalid)?;
    if !matches!(now.as_bytes().get(10), None | Some(b'T' | b' ')) {
        return Err(invalid());
    }
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = date.get(range)?;
        if part.bytes().all(|b| b.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    };
    let (Some(year), Some(month), Some(day)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return Err(invalid());
    };
    if date.as_bytes()[4] != b'-' || date.as_bytes()[7] != b'-' {
        return Err(invalid());
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(invalid()),
    };
    if year == 0 || day == 0 || day > days {
        return Err(invalid());
    }
    Ok(date)
}

/// Python's `validate_request_input`. `now` is an ISO 8601 date-time; only its
/// date is used, for the default request id.
pub fn validate_request_input(data: &Value, now: &str) -> Result<Request, RequestError> {
    let Some(data) = data.as_object() else {
        return reject("request input must be a JSON object");
    };
    let schema = str_or(data, "schema_version", SCHEMA_VERSION);
    if schema != SCHEMA_VERSION {
        return reject(format!("unsupported schema_version: {schema}"));
    }
    let kind = py_strip(&str_or(data, "kind", "")).to_owned();
    if !ALLOWED_KINDS.contains(&kind.as_str()) {
        return reject("kind must be app or repair");
    }
    let title = text(data, "title", 5, 140)?;
    let summary = text(data, "summary", 10, 500)?;
    let problem = text(data, "problem", 10, 2000)?;
    let desired = text(data, "desired_capability", 10, 2000)?;
    let criteria = strings(data, "acceptance_criteria", 2, 8)?;
    let evidence = strings(data, "evidence", 1, 8)?;
    let scope = text(data, "same_scope_key", 12, 170)?;
    if !scope_ok(&scope) {
        return reject("same_scope_key must match agentmesh:<stable-scope>");
    }
    let mut request_id = py_strip(&str_or(data, "request_id", "")).to_owned();
    if request_id.is_empty() {
        request_id = format!("{}-{}", date_of(now)?, slugify(&title));
    }
    if !request_id_ok(&request_id) {
        return reject("request_id must be lowercase ASCII kebab-case");
    }
    let target_app = take_chars(py_strip(&str_or(data, "target_app", "agentmesh")), 80);
    let user_story = take_chars(py_strip(&str_or(data, "user_story", &summary)), 500);
    Ok(Request {
        schema_version: schema,
        request_id,
        kind,
        title,
        summary,
        problem,
        desired_capability: desired,
        acceptance_criteria: criteria,
        evidence,
        same_scope_key: scope,
        target_app,
        user_story,
    })
}

/// Python's `_yaml_string`, `json.dumps(value, ensure_ascii=False)`.
#[must_use]
pub fn yaml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Python's `_render_source`.
#[must_use]
pub fn render_source(request: &Request, created_at: &str, issue_rel: &str) -> String {
    let mut lines: Vec<String> = vec![
        "---".to_owned(),
        format!("schema_version: {}", yaml_string(&request.schema_version)),
        format!("request_id: {}", yaml_string(&request.request_id)),
        format!("kind: {}", request.kind),
        "status: ready".to_owned(),
        format!("created_at: {}", yaml_string(created_at)),
        format!("same_scope_key: {}", yaml_string(&request.same_scope_key)),
        format!("target_app: {}", yaml_string(&request.target_app)),
        format!("derived_issue: {}", yaml_string(issue_rel)),
        "---".to_owned(),
        format!("# {}", request.title),
        String::new(),
        "## Summary".to_owned(),
        String::new(),
        request.summary.clone(),
        String::new(),
        "## Problem".to_owned(),
        String::new(),
        request.problem.clone(),
        String::new(),
        "## Desired capability".to_owned(),
        String::new(),
        request.desired_capability.clone(),
        String::new(),
        "## Acceptance criteria".to_owned(),
        String::new(),
    ];
    lines.extend(
        request
            .acceptance_criteria
            .iter()
            .map(|item| format!("- [ ] {item}")),
    );
    lines.extend([String::new(), "## Evidence".to_owned(), String::new()]);
    lines.extend(request.evidence.iter().map(|item| format!("- {item}")));
    lines.extend([
        String::new(),
        "## User story".to_owned(),
        String::new(),
        format!("- {}", request.user_story),
        String::new(),
    ]);
    lines.join("\n")
}

/// Python's `_render_issue`.
#[must_use]
pub fn render_issue(request: &Request, source_rel: &str) -> String {
    let task_kind = if request.kind == "repair" {
        "toolchain_repair"
    } else {
        "implementation"
    };
    let mut lines: Vec<String> = vec![
        "---".to_owned(),
        format!("title: {}", yaml_string(&request.title)),
        "ready_for_multica: true".to_owned(),
        "status: ready".to_owned(),
        "project_key: agentmesh".to_owned(),
        "issue_type: AFK".to_owned(),
        format!("source_prd: {}", yaml_string(source_rel)),
        "source_design: 4_Project/OSS/agentmesh/Docs/agentmesh-request-operations-v1.md".to_owned(),
        "source_roadmap: 4_Project/OSS/agentmesh/ROADMAP.md".to_owned(),
        "sequence_index: 1".to_owned(),
        "sequence_total: 1".to_owned(),
        "blocked_by: []".to_owned(),
        "unblocks: []".to_owned(),
        "pr_required: true".to_owned(),
        "pr_allowed: true".to_owned(),
        "pr_mode: single".to_owned(),
        "release_allowed: false".to_owned(),
        "production_allowed: false".to_owned(),
        "version_bump_required: false".to_owned(),
        "version_bump_type: none".to_owned(),
        format!(
            "version_bump_reason: {}",
            yaml_string(RELEASE_BOUNDARY_REASON)
        ),
        "package_publish_expected: false".to_owned(),
        "work_owner: ai".to_owned(),
        "route_mode: direct".to_owned(),
        "squad_candidate: false".to_owned(),
        "multi_role_required: false".to_owned(),
        "needs_design_first: false".to_owned(),
        "needs_decomposition: false".to_owned(),
        "expected_pr_count: 1".to_owned(),
        "ambiguity_level: low".to_owned(),
        "failure_streak: 0".to_owned(),
        format!("task_kind: {task_kind}"),
        "feedback_version: 1".to_owned(),
        "feedback_last_result_type: pilot_started".to_owned(),
        "feedback_last_feedback_code: agentmesh_request_materialized".to_owned(),
        format!(
            "feedback_same_scope_key: {}",
            yaml_string(&request.same_scope_key)
        ),
        "---".to_owned(),
        format!("# {}", request.title),
        String::new(),
        "## Parent".to_owned(),
        String::new(),
        format!("- Request: `{source_rel}`"),
        "- Design: [[../Docs/agentmesh-request-operations-v1]]".to_owned(),
        "- Roadmap: [[../ROADMAP]]".to_owned(),
        String::new(),
        "## What to build".to_owned(),
        String::new(),
        request.desired_capability.clone(),
        String::new(),
        format!("Problem evidence: {}", request.problem),
        String::new(),
        "## Acceptance criteria".to_owned(),
        String::new(),
    ];
    lines.extend(
        request
            .acceptance_criteria
            .iter()
            .map(|item| format!("- [ ] {item}")),
    );
    lines.extend([
        "- [ ] Run relevant AgentMesh fmt, clippy, and tests; include exact evidence in the PR."
            .to_owned(),
        "- [ ] Keep release/tag/assets and Multica authority changes out of this PR.".to_owned(),
        String::new(),
        "## Blocked by".to_owned(),
        String::new(),
        "- None - can start immediately.".to_owned(),
        String::new(),
        "## User stories covered".to_owned(),
        String::new(),
        format!("- {}", request.user_story),
        String::new(),
        "## Notes".to_owned(),
        String::new(),
        format!("- Request kind: `{}`", request.kind),
        format!("- Same scope: `{}`", request.same_scope_key),
        "- AgentMesh CLI use is issue-scoped product verification only; do not mutate Multica through AgentMesh."
            .to_owned(),
        "- Release/publish, secrets, permissions, production actions, and authority cutover remain human-owned."
            .to_owned(),
        String::new(),
    ]);
    lines.join("\n")
}

/// Python's `choose_patrol_action`: the action and its reason. `first_code` is
/// the `code` of the first repair candidate, when there is one.
#[must_use]
pub fn choose_patrol_action(
    first_repair_code: Option<Option<&str>>,
    daily_exists: bool,
) -> (&'static str, String) {
    if let Some(code) = first_repair_code {
        let reason = code
            .filter(|code| !code.is_empty())
            .unwrap_or("repair_signal");
        return ("repair_first", reason.to_owned());
    }
    if daily_exists {
        return ("no_action", "daily_app_request_exists".to_owned());
    }
    ("seed_app_request", "daily_app_request_missing".to_owned())
}

fn field<'a>(request: &'a Map<String, Value>, key: &str) -> Result<&'a Value, OpsError> {
    request
        .get(key)
        .ok_or_else(|| OpsError::invalid_request(format!("{key} is required")))
}

fn string_field<'a>(request: &'a Map<String, Value>, key: &str) -> Result<&'a str, OpsError> {
    field(request, key)?
        .as_str()
        .ok_or_else(|| OpsError::invalid_request(format!("{key} must be a string")))
}

/// Handle one stdio request. `op` selects the operation:
///
/// - `validate`: `request`, `now` -> the validated request.
/// - `slugify`: `value` -> `{ "slug" }`.
/// - `render_source`: `request`, `now`, `created_at`, `issue_rel` -> `{ "text" }`.
/// - `render_issue`: `request`, `now`, `source_rel` -> `{ "text" }`.
/// - `patrol`: `repair_candidates`, `daily_exists` -> `{ "action", "reason" }`.
///
/// A request rejected by validation fails with kind `invalid_request` and the
/// Python `RequestError` message.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    let request = request
        .as_object()
        .ok_or_else(|| OpsError::invalid_request("request must be an object"))?;
    let op = string_field(request, "op")?;
    match op {
        "validate" => {
            let validated = validate_request_input(field(request, "request")?, now(request)?)?;
            Ok(validated.to_json())
        }
        "slugify" => Ok(json!({ "slug": slugify(string_field(request, "value")?) })),
        "render_source" => {
            let validated = validate_request_input(field(request, "request")?, now(request)?)?;
            let created_at = string_field(request, "created_at")?;
            let issue_rel = string_field(request, "issue_rel")?;
            Ok(json!({ "text": render_source(&validated, created_at, issue_rel) }))
        }
        "render_issue" => {
            let validated = validate_request_input(field(request, "request")?, now(request)?)?;
            let source_rel = string_field(request, "source_rel")?;
            Ok(json!({ "text": render_issue(&validated, source_rel) }))
        }
        "patrol" => {
            let candidates = field(request, "repair_candidates")?
                .as_array()
                .ok_or_else(|| OpsError::invalid_request("repair_candidates must be an array"))?;
            let daily_exists = field(request, "daily_exists")?
                .as_bool()
                .ok_or_else(|| OpsError::invalid_request("daily_exists must be a boolean"))?;
            let first_code = match candidates.first() {
                None => None,
                Some(Value::Object(candidate)) => Some(match candidate.get("code") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(code)) => Some(code.as_str()),
                    Some(_) => {
                        return Err(OpsError::invalid_request(
                            "repair candidate code must be a string",
                        ));
                    }
                }),
                Some(_) => {
                    return Err(OpsError::invalid_request(
                        "repair_candidates must contain objects",
                    ));
                }
            };
            let (action, reason) = choose_patrol_action(first_code, daily_exists);
            Ok(json!({ "action": action, "reason": reason }))
        }
        other => Err(OpsError::invalid_request(format!("unknown op: {other}"))),
    }
}

fn now(request: &Map<String, Value>) -> Result<&str, OpsError> {
    string_field(request, "now")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-07-16T07:40:00+09:00";

    fn sample() -> Value {
        json!({
            "schema_version": "agentmesh-request.v0",
            "kind": "app",
            "title": "Markdown normalization app",
            "summary": "Normalize deterministic Markdown documents through an AgentMesh App.",
            "problem": "Markdown tools currently need orchestrator-specific glue and duplicate parsing.",
            "desired_capability": "Add a fixture-driven Markdown normalization App without Multica domain types.",
            "acceptance_criteria": ["Manifest validates through pinned CLI.", "Fixtures produce deterministic output."],
            "evidence": ["AgentMesh roadmap requests a non-Multica Markdown App."],
            "same_scope_key": "agentmesh:app:markdown-normalizer",
            "target_app": "markdown-normalizer",
            "user_story": "As a tool author, I can invoke one pinned Markdown App from different orchestrators.",
        })
    }

    fn with(key: &str, value: Value) -> Value {
        let mut data = sample();
        data[key] = value;
        data
    }

    fn message(data: &Value) -> String {
        validate_request_input(data, NOW).unwrap_err().0
    }

    #[test]
    fn valid_request_gets_default_id_from_date_and_title() {
        let request = validate_request_input(&sample(), NOW).unwrap();
        assert_eq!(request.request_id, "2026-07-16-markdown-normalization-app");
        assert_eq!(request.target_app, "markdown-normalizer");
    }

    #[test]
    fn rejects_bad_kinds_scopes_and_lengths() {
        assert_eq!(
            message(&with("kind", json!("other"))),
            "kind must be app or repair"
        );
        assert_eq!(
            message(&with("title", json!("abc"))),
            "title length must be 5..140"
        );
        assert_eq!(
            message(&with("same_scope_key", json!("agentmesh:Bad Scope"))),
            "same_scope_key must match agentmesh:<stable-scope>"
        );
        assert_eq!(
            message(&with("acceptance_criteria", json!(["only one entry"]))),
            "acceptance_criteria must contain 2..8 strings"
        );
        assert_eq!(
            message(&with("evidence", json!([5]))),
            "evidence entries must be strings of length 3..500"
        );
        assert_eq!(
            message(&with("request_id", json!("Bad_ID"))),
            "request_id must be lowercase ASCII kebab-case"
        );
        assert_eq!(message(&json!([])), "request input must be a JSON object");
    }

    #[test]
    fn falsy_schema_version_defaults_and_others_are_stringified() {
        assert!(validate_request_input(&with("schema_version", json!("")), NOW).is_ok());
        assert_eq!(
            message(&with("schema_version", json!(2))),
            "unsupported schema_version: 2"
        );
        assert_eq!(
            message(&with("schema_version", json!(true))),
            "unsupported schema_version: True"
        );
        assert_eq!(
            message(&with("schema_version", json!(["a'b", 1.5]))),
            "unsupported schema_version: [\"a'b\", 1.5]"
        );
    }

    #[test]
    fn slugify_matches_python_cases() {
        assert_eq!(slugify("  Hello,   World!  "), "hello-world");
        assert_eq!(slugify("日本語"), "request");
        assert_eq!(slugify(&"a".repeat(60)), "a".repeat(56));
        assert_eq!(slugify(&format!("{}-b", "a".repeat(55))), "a".repeat(55));
    }

    #[test]
    fn yaml_string_escapes_like_json_dumps() {
        assert_eq!(
            yaml_string("a\"b\\c\n\u{1}é\u{7f}"),
            "\"a\\\"b\\\\c\\n\\u0001é\u{7f}\""
        );
    }

    #[test]
    fn source_render_has_no_multica_marker_and_issue_render_has_one() {
        let request = validate_request_input(&sample(), NOW).unwrap();
        let source = render_source(&request, "2026-07-16T07:40:00+09:00", "Issues/x.md");
        let issue = render_issue(&request, "Requests/App/x.md");
        assert!(!source.contains("ready_for_multica"));
        assert!(source.ends_with("- As a tool author, I can invoke one pinned Markdown App from different orchestrators.\n"));
        assert!(issue.contains("ready_for_multica: true"));
        assert!(issue.contains("project_key: agentmesh"));
    }

    #[test]
    fn patrol_is_repair_first() {
        let run = |candidates: Value, daily: bool| {
            let result = handle(json!({
                "op": "patrol", "repair_candidates": candidates, "daily_exists": daily
            }))
            .unwrap();
            (result["action"].clone(), result["reason"].clone())
        };
        assert_eq!(
            run(json!([{"code": "main_ci_failed"}]), false),
            (json!("repair_first"), json!("main_ci_failed"))
        );
        assert_eq!(run(json!([{}]), true).1, json!("repair_signal"));
        assert_eq!(run(json!([]), true).0, json!("no_action"));
        assert_eq!(run(json!([]), false).0, json!("seed_app_request"));
    }

    #[test]
    fn handle_rejects_malformed_envelopes() {
        assert!(handle(json!([])).is_err());
        assert!(handle(json!({"op": "nope"})).is_err());
        assert!(handle(json!({"op": "validate", "request": {}})).is_err());
        assert_eq!(
            handle(json!({"op": "validate", "request": [], "now": NOW}))
                .unwrap_err()
                .message,
            "request input must be a JSON object"
        );
    }

    #[test]
    fn now_must_be_a_real_date_only_when_it_is_used() {
        let mut data = sample();
        assert!(validate_request_input(&data, "not a date").is_err());
        data["request_id"] = json!("explicit-id");
        assert!(validate_request_input(&data, "not a date").is_ok());
        assert!(date_of("2026-02-30T00:00:00").is_err());
        assert!(date_of("2028-02-29").is_ok());
    }
}
