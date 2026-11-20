//! Port of the deterministic project-focus policy helpers.
//!
//! The caller supplies the policy file contents (or `null` when the file is
//! absent), so this crate has no filesystem or Multica dependency.

use std::collections::{BTreeMap, BTreeSet};

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Map, Number, Value};

/// One project's pacing policy after loading the owner policy document.
#[derive(Debug, Clone, PartialEq)]
pub struct PacingRule {
    /// Display key returned in a pacing hold.
    pub key: String,
    /// Maximum active issue count, if configured.
    pub max_active: Value,
    /// Maximum running issue count, if configured.
    pub max_running: Value,
    /// Maximum in-review issue count, if configured.
    pub max_in_review: Value,
}

/// The loaded focus and pacing configuration.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Policy {
    /// Project ids promoted by owner focus.
    pub focus: BTreeSet<String>,
    /// Per-project pacing rules keyed by project id.
    pub paced: BTreeMap<String, PacingRule>,
}

impl Policy {
    fn as_json(&self) -> Value {
        let focus: Vec<_> = self.focus.iter().cloned().collect();
        let paced: Map<String, Value> = self
            .paced
            .iter()
            .map(|(project_id, rule)| {
                (
                    project_id.clone(),
                    json!({
                        "key": rule.key,
                        "max_active": rule.max_active,
                        "max_running": rule.max_running,
                        "max_in_review": rule.max_in_review,
                    }),
                )
            })
            .collect();
        json!({ "focus": focus, "paced": paced })
    }
}

/// Load policy text, treating absent files and malformed JSON as no focus and
/// no pacing, matching the Python implementation.
pub fn load(policy_text: Option<&str>) -> Policy {
    let Some(policy_text) = policy_text else {
        return Policy::default();
    };
    let Ok(raw) = serde_json::from_str::<Value>(policy_text) else {
        return Policy::default();
    };
    let Some(raw) = raw.as_object() else {
        return Policy::default();
    };
    if raw.get("enabled") != Some(&Value::Bool(true)) {
        return Policy::default();
    }

    let mut policy = Policy::default();
    if let Some(rows) = raw.get("focus").and_then(Value::as_array) {
        for row in rows {
            let Some(row) = row.as_object() else {
                continue;
            };
            let Some(project_id) = row.get("project_id").filter(|value| is_truthy(value)) else {
                continue;
            };
            policy.focus.insert(python_string(project_id));
        }
    }

    if let Some(rows) = raw.get("paced").and_then(Value::as_array) {
        for row in rows {
            let Some(row) = row.as_object() else {
                continue;
            };
            let Some(project_id) = row.get("project_id").filter(|value| is_truthy(value)) else {
                continue;
            };
            let project_id = python_string(project_id);
            let key = row
                .get("key")
                .filter(|value| is_truthy(value))
                .map_or_else(|| project_id.clone(), python_string);
            policy.paced.insert(
                project_id,
                PacingRule {
                    key,
                    max_active: row.get("max_active").cloned().unwrap_or(Value::Null),
                    max_running: row.get("max_running").cloned().unwrap_or(Value::Null),
                    max_in_review: row.get("max_in_review").cloned().unwrap_or(Value::Null),
                },
            );
        }
    }
    policy
}

/// Return the issue's project id as a string, or an empty string when absent.
pub fn project_of(issue: &Value) -> String {
    issue
        .get("project_id")
        .filter(|value| is_truthy(value))
        .map_or_else(String::new, python_string)
}

/// Return zero for a focused project and one for all other projects.
pub fn focus_rank(issue: &Value, policy: &Policy) -> u8 {
    u8::from(!policy.focus.contains(&project_of(issue)))
}

/// Count issues by project when their stringified status is in `statuses`.
pub fn count_by_project(issues: &[Value], statuses: &BTreeSet<String>) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for issue in issues {
        let status = issue
            .get("status")
            .filter(|value| is_truthy(value))
            .map_or_else(String::new, python_string);
        let project_id = project_of(issue);
        if statuses.contains(&status) && !project_id.is_empty() {
            *counts.entry(project_id).or_insert(0) += 1;
        }
    }
    counts
}

/// Return a pacing hold when the configured limit has been reached.
pub fn paced_out(
    issue: &Value,
    policy: &Policy,
    counts: &BTreeMap<String, u64>,
    limit_key: &str,
) -> Option<Value> {
    let project_id = project_of(issue);
    let rule = policy.paced.get(&project_id)?;
    let limit = match limit_key {
        "max_active" => &rule.max_active,
        "max_running" => &rule.max_running,
        "max_in_review" => &rule.max_in_review,
        _ => return None,
    };
    let limit = integer_limit(limit)?;
    let current = counts.get(&project_id).copied().unwrap_or(0);
    if i128::from(current) < limit {
        return None;
    }
    Some(json!({
        "project": rule.key,
        "limit_key": limit_key,
        "limit": limit_json_number(limit),
        "current": current,
    }))
}

/// Handle the stdio request shape used by this crate.
///
/// `policy_text` contains the policy file's contents, or `null` if no file was
/// available. Other fields supply the issue and issue rows read by the caller.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    let request = request
        .as_object()
        .ok_or_else(|| OpsError::invalid_request("request must be an object"))?;
    let policy_text = match request.get("policy_text") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.as_str()),
        Some(_) => {
            return Err(OpsError::invalid_request(
                "policy_text must be a string or null",
            ));
        }
    };
    let issue = request
        .get("issue")
        .filter(|value| value.is_object())
        .ok_or_else(|| OpsError::invalid_request("issue must be an object"))?;
    let issues = request
        .get("issues")
        .and_then(Value::as_array)
        .ok_or_else(|| OpsError::invalid_request("issues must be an array"))?;
    let statuses = request
        .get("statuses")
        .and_then(Value::as_array)
        .ok_or_else(|| OpsError::invalid_request("statuses must be an array"))?;
    let mut status_set = BTreeSet::new();
    for status in statuses {
        let Some(status) = status.as_str() else {
            return Err(OpsError::invalid_request(
                "statuses must contain only strings",
            ));
        };
        status_set.insert(status.to_owned());
    }
    let limit_key = request
        .get("limit_key")
        .and_then(Value::as_str)
        .ok_or_else(|| OpsError::invalid_request("limit_key must be a string"))?;

    let policy = load(policy_text);
    let counts = count_by_project(issues, &status_set);
    let counts_json: Map<String, Value> = counts
        .iter()
        .map(|(project_id, count)| (project_id.clone(), json!(count)))
        .collect();
    Ok(json!({
        "policy": policy.as_json(),
        "project_id": project_of(issue),
        "focus_rank": focus_rank(issue, &policy),
        "counts": counts_json,
        "paced_out": paced_out(issue, &policy, &counts, limit_key),
    }))
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
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
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn integer_limit(value: &Value) -> Option<i128> {
    match value {
        Value::Number(number) if number.is_i64() => number.as_i64().map(i128::from),
        Value::Number(number) if number.is_u64() => number.as_u64().map(i128::from),
        _ => None,
    }
}

fn limit_json_number(value: i128) -> Number {
    if let Ok(value) = i64::try_from(value) {
        Number::from(value)
    } else {
        Number::from(u64::try_from(value).expect("JSON integer limits fit in u64"))
    }
}
