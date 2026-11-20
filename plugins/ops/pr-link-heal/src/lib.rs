//! Deterministic port of the PR-link heal controller decision.
//!
//! Pull-request search and view data, issue data, and the current time are
//! supplied by the caller. This crate only returns the Python decision and
//! records requested GitHub/edit/metadata callbacks; it never performs them.

use std::collections::BTreeMap;

use agentmesh_ops_kit::OpsError;
use chrono::{DateTime, Duration, FixedOffset};
use serde_json::{json, Value};

const OWNER: &str = "eiei114";
const WAIT: Duration = Duration::hours(1);

/// Decide whether a PR-link heal is pending and record the requested callback
/// effects without applying them.
///
/// The request contains `issue`, an explicit `now`, and synthetic responses
/// (`pr_search_results` and `pr_view`) for the Python callback interface. The
/// response contains the Python return value and ordered callback traces.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    let request = request
        .as_object()
        .ok_or_else(|| OpsError::invalid_request("request must be an object"))?;
    let issue = request
        .get("issue")
        .filter(|value| value.is_object())
        .ok_or_else(|| OpsError::invalid_request("issue must be an object"))?;
    let now_text = request
        .get("now")
        .and_then(Value::as_str)
        .ok_or_else(|| OpsError::invalid_request("now must be an ISO-8601 string"))?;
    let now = DateTime::parse_from_rfc3339(now_text)
        .map_err(|_| OpsError::invalid_request("now must be a valid ISO-8601 timestamp"))?;
    let search_results = request
        .get("pr_search_results")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    if !search_results.is_array() {
        return Err(OpsError::invalid_request(
            "pr_search_results must be an array",
        ));
    }
    let pr_view = request.get("pr_view").cloned().unwrap_or(Value::Null);

    let mut github_calls = Vec::new();
    let mut edits = Vec::new();
    let mut metadata = Vec::new();
    let decision = attempt(
        issue,
        now,
        &search_results,
        &pr_view,
        &mut github_calls,
        &mut edits,
        &mut metadata,
    )?;

    Ok(json!({
        "result": decision,
        "actions": {
            "github_calls": github_calls,
            "edit": edits,
            "set_metadata": metadata,
        }
    }))
}

fn attempt(
    issue: &Value,
    now: DateTime<FixedOffset>,
    search_results: &Value,
    pr_view: &Value,
    github_calls: &mut Vec<Value>,
    edits: &mut Vec<Value>,
    metadata: &mut Vec<Value>,
) -> Result<Value, OpsError> {
    let key = issue
        .get("identifier")
        .filter(|value| python_truthy(value))
        .map_or_else(String::new, python_string);
    let started = md(issue, "pr_link_heal_at");
    if let Some(started) = started.filter(python_truthy) {
        let started_text = python_string(&started);
        if let Ok(started_at) = DateTime::parse_from_rfc3339(&started_text) {
            if now.signed_duration_since(started_at) < WAIT {
                return Ok(json!({
                    "requested": "waiting_for_link",
                    "since": started,
                }));
            }
        }
        // Invalid and expired timestamps both defer to the repair child.
        return Ok(json!({}));
    }

    let candidates = candidates(issue, &key, search_results, github_calls)?;
    if candidates.len() != 1 {
        return Ok(json!({}));
    }
    let (repo, number) = &candidates[0];
    github_calls.push(json!({
        "args": ["pr", "view", number.to_string(), "-R", repo, "--json", "state,title,headRefOid,url"],
    }));

    let empty_pr = Value::Null;
    let pr = if python_truthy(pr_view) {
        pr_view
    } else {
        &empty_pr
    };
    let state = pr.get("state").unwrap_or(&Value::Null);
    let head = md(issue, "pr_head_sha")
        .filter(python_truthy)
        .map_or_else(String::new, |value| python_string(&value));
    let actual_head = pr
        .get("headRefOid")
        .filter(|value| python_truthy(value))
        .map_or_else(String::new, python_string);
    let title = pr
        .get("title")
        .filter(|value| python_truthy(value))
        .map_or_else(String::new, python_string);
    if state.as_str() != Some("OPEN")
        || (!head.is_empty() && !actual_head.starts_with(&head[..head.len().min(12)]))
        || has_issue_prefix(&key, &title)
    {
        return Ok(json!({}));
    }

    edits.push(json!({
        "repo": repo,
        "number": number,
        "title": format!("{key}: {title}"),
    }));
    let issue_id = issue.get("id").cloned().ok_or_else(|| {
        OpsError::invalid_request("issue.id is required for a planned metadata update")
    })?;
    metadata.push(json!({
        "issue_id": issue_id,
        "key": "pr_link_heal_at",
        "value": python_isoformat(now),
    }));

    let pr_url = pr.get("url").cloned().unwrap_or(Value::Null);
    let existing_url = md(issue, "pr_url").unwrap_or(Value::Null);
    if !python_truthy(&existing_url) && python_truthy(&pr_url) {
        metadata.push(json!({
            "issue_id": issue.get("id").cloned().unwrap_or(Value::Null),
            "key": "pr_url",
            "value": pr_url,
        }));
    }
    Ok(json!({
        "requested": "title_prefixed",
        "url": pr_url,
    }))
}

fn candidates(
    issue: &Value,
    key: &str,
    search_results: &Value,
    github_calls: &mut Vec<Value>,
) -> Result<Vec<(String, i64)>, OpsError> {
    for field in ["pr_url", "repair_artifact_url"] {
        let Some(value) = md(issue, field).filter(python_truthy) else {
            continue;
        };
        if let Some(candidate) = find_pr_url(&python_string(&value))? {
            return Ok(vec![candidate]);
        }
    }

    let project = md(issue, "project_key")
        .filter(python_truthy)
        .map_or_else(String::new, |value| python_string(&value));
    if project.is_empty() {
        return Ok(Vec::new());
    }
    let repo = format!("{OWNER}/{project}");
    if let Some(number) = md(issue, "pr_number").filter(python_truthy) {
        let number = python_string(&number);
        if !number.is_empty() && number.chars().all(|character| character.is_ascii_digit()) {
            if let Ok(number) = number.parse::<i64>() {
                return Ok(vec![(repo, number)]);
            }
            return Err(OpsError::invalid_request(
                "metadata.pr_number is outside the supported integer range",
            ));
        }
    }

    github_calls.push(json!({
        "args": ["pr", "list", "-R", repo, "--state", "open", "--search", key, "--json", "number"],
    }));
    let rows = search_results
        .as_array()
        .expect("search results are validated as an array");
    if rows.len() != 1 {
        return Ok(Vec::new());
    }
    let raw_number = rows[0]
        .get("number")
        .ok_or_else(|| OpsError::invalid_request("a PR search result must contain number"))?;
    let number = python_int(raw_number)
        .ok_or_else(|| OpsError::invalid_request("a PR search result number must be an integer"))?;
    Ok(vec![(repo, number)])
}

fn md(issue: &Value, key: &str) -> Option<Value> {
    issue
        .get("metadata")
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get(key))
        .cloned()
}

fn find_pr_url(text: &str) -> Result<Option<(String, i64)>, OpsError> {
    let marker = "https://github.com/";
    let mut offset = 0;
    while let Some(found) = text[offset..].find(marker) {
        let start = offset + found + marker.len();
        let tail = &text[start..];
        let Some(first_slash) = tail.find('/') else {
            return Ok(None);
        };
        let owner = &tail[..first_slash];
        let tail = &tail[first_slash + 1..];
        let Some(second_slash) = tail.find('/') else {
            return Ok(None);
        };
        let repository = &tail[..second_slash];
        let route = &tail[second_slash + 1..];
        if let Some(suffix) = route.strip_prefix("pull/") {
            let digits: String = suffix.chars().take_while(char::is_ascii_digit).collect();
            if !owner.is_empty()
                && !repository.is_empty()
                && !owner.chars().any(char::is_whitespace)
                && !repository.chars().any(char::is_whitespace)
                && !digits.is_empty()
            {
                let number = digits.parse::<i64>().map_err(|_| {
                    OpsError::invalid_request(
                        "PR URL number is outside the supported integer range",
                    )
                })?;
                return Ok(Some((format!("{owner}/{repository}"), number)));
            }
        }
        offset = start;
        if offset >= text.len() {
            break;
        }
    }
    Ok(None)
}

fn has_issue_prefix(key: &str, title: &str) -> bool {
    let title = title.trim_start_matches(char::is_whitespace);
    let title = title.strip_prefix('[').unwrap_or(title);
    let Some(after_key) = title.strip_prefix(key) else {
        return false;
    };
    let Some(next) = after_key.chars().next() else {
        return !key.is_empty();
    };
    if key.is_empty() {
        return next.is_alphanumeric() || next == '_';
    }
    !(next.is_alphanumeric() || next == '_')
}

fn python_int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_u64()
                .and_then(|number| i64::try_from(number).ok())
        }),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|number| number != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

fn python_string(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(value) => if *value { "True" } else { "False" }.to_owned(),
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(python_repr)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(values) => {
            let values: BTreeMap<_, _> = values.iter().collect();
            format!(
                "{{{}}}",
                values
                    .iter()
                    .map(|(key, value)| format!(
                        "{}: {}",
                        python_repr(&Value::String((*key).clone())),
                        python_repr(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

fn python_repr(value: &Value) -> String {
    match value {
        Value::String(value) => format!(
            "'{}'",
            value
                .replace('\\', "\\\\")
                .replace('\'', "\\'")
                .replace('\n', "\\n")
        ),
        _ => python_string(value),
    }
}

fn python_isoformat(now: DateTime<FixedOffset>) -> String {
    let mut value = now.format("%Y-%m-%dT%H:%M:%S").to_string();
    let microsecond = now.timestamp_subsec_micros();
    if microsecond != 0 {
        value.push_str(&format!(".{microsecond:06}"));
    }
    let offset_seconds = now.offset().local_minus_utc();
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let absolute_offset = offset_seconds.unsigned_abs();
    value.push_str(&format!(
        "{sign}{:02}:{:02}",
        absolute_offset / 3_600,
        (absolute_offset % 3_600) / 60
    ));
    value
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn python_reference_fixtures_match() {
        let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
        let count = agentmesh_ops_kit::check_parity(&fixture_dir, handle)
            .expect("PR-link-heal fixtures must match Python output");
        assert!(
            count >= 12,
            "fixtures should cover guard and action branches"
        );
    }

    #[test]
    fn request_requires_an_explicit_now_value() {
        let error = handle(json!({ "issue": {}, "pr_search_results": [] })).unwrap_err();
        assert_eq!(error.kind, agentmesh_ops_kit::ErrorKind::InvalidRequest);
    }
}
