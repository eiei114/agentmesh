//! Read-only ports of two Python reconciliation helpers. No action is applied.

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Value};

/// Evaluate one decision against preloaded evidence (see the crate README).
pub fn handle(request: Value) -> Result<Value, OpsError> {
    let issue = &request["issue"];
    let md = &request["metadata"];
    if !issue.is_object() || !md.is_object() {
        return Err(OpsError::invalid_request(
            "issue and metadata must be objects",
        ));
    }
    match request["operation"].as_str() {
        Some("superseded_by") => superseded_by(&request, issue, md),
        Some("orphan_repair") => orphan_repair(&request, issue, md),
        _ => Err(OpsError::invalid_request(
            "operation must be superseded_by or orphan_repair",
        )),
    }
}

fn repair_sha(key: &str) -> Option<&str> {
    let (source, sha) = key
        .strip_suffix('\n')
        .unwrap_or(key)
        .strip_prefix("managed-review-fix:")?
        .split_once(':')?;
    (!source.is_empty()
        && sha.len() == 40
        && sha
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)))
    .then_some(sha)
}

fn has_pr_url(text: &str) -> bool {
    text.match_indices("https://github.com/")
        .any(|(i, prefix)| {
            let mut parts = text[i + prefix.len()..].split('/');
            let repo_part = |part: &str| !part.is_empty() && !part.chars().any(char::is_whitespace);
            parts.next().is_some_and(repo_part)
                && parts.next().is_some_and(repo_part)
                && parts.next() == Some("pull")
                && parts
                    .next()
                    .is_some_and(|num| num.as_bytes().first().is_some_and(u8::is_ascii_digit))
        })
}

fn open_head(request: &Value) -> Result<Option<&str>, OpsError> {
    match request.get("pr_view") {
        Some(Value::Null) => Ok(None),
        Some(pr) if pr.is_object() => Ok(pr["headRefOid"]
            .as_str()
            .filter(|head| pr["state"].as_str() == Some("OPEN") && !head.is_empty())),
        _ => Err(OpsError::invalid_request(
            "pr_view must be an object or null",
        )),
    }
}

fn head_prefix(head: &str) -> String {
    head.chars().take(12).collect()
}

fn superseded_by(request: &Value, issue: &Value, md: &Value) -> Result<Value, OpsError> {
    let Some(sha) = md["dedupe_key"].as_str().and_then(repair_sha) else {
        return Ok(Value::Null);
    };
    if !md["source_pr_url"].as_str().is_some_and(has_pr_url) {
        return Ok(Value::Null);
    }
    let Some(head) = open_head(request)?.filter(|head| *head != sha) else {
        return Ok(Value::Null);
    };
    let siblings = request["siblings"].as_array().ok_or_else(|| {
        OpsError::invalid_request("siblings must be the prefiltered source issue rows")
    })?;
    let id = &issue["id"];
    let mine = siblings
        .iter()
        .find(|row| &row["id"] == id)
        .unwrap_or(issue);
    let created = mine["created_at"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("~");
    for row in siblings {
        if &row["id"] != id
            && row["status"] != "cancelled"
            && sibling_sha(row).is_some_and(|other| other != sha)
            && row["created_at"].as_str().unwrap_or("") > created
        {
            let identifier = row["identifier"].as_str().unwrap_or("None");
            return Ok(json!(format!(
                "superseded_by:{identifier}@{}",
                head_prefix(head)
            )));
        }
    }
    Ok(Value::Null)
}

fn sibling_sha(row: &Value) -> Option<String> {
    // Match autopilot_common.metadata's nested issue and JSON-encoded metadata forms.
    let row = row
        .get("issue")
        .filter(|nested| nested.is_object())
        .unwrap_or(row);
    let md = &row["metadata"];
    let decoded_md: Value;
    let md = if let Some(text) = md.as_str() {
        decoded_md = serde_json::from_str(text).ok()?;
        &decoded_md
    } else {
        md
    };
    let key = md["dedupe_key"].as_str()?;
    if key.trim().starts_with('"') {
        let decoded_key: String = serde_json::from_str(key).ok()?;
        repair_sha(&decoded_key).map(str::to_owned)
    } else {
        repair_sha(key).map(str::to_owned)
    }
}

fn orphan_repair(request: &Value, issue: &Value, md: &Value) -> Result<Value, OpsError> {
    let status = request["status"]
        .as_str()
        .ok_or_else(|| OpsError::invalid_request("status must be a string for orphan_repair"))?;
    if status != "in_review"
        || issue["assignee_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    {
        return Ok(Value::Null);
    }
    if !md["source_pr_url"].as_str().is_some_and(has_pr_url) {
        return Ok(Value::Null);
    }
    let runs = request.get("runs").ok_or_else(|| {
        OpsError::invalid_request("runs must be supplied for an unowned review repair")
    })?;
    let runs = if runs.is_object() {
        runs.get("runs")
            .or_else(|| runs.get("items"))
            .unwrap_or(&Value::Null)
    } else {
        runs
    };
    if !runs.is_null() && runs.as_array().is_none_or(|rows| !rows.is_empty()) {
        return Ok(Value::Null);
    }
    let Some(head) = open_head(request)? else {
        return Ok(Value::Null);
    };
    let prefix = head_prefix(head);
    if md["repair_artifact_head_sha"] == head {
        Ok(json!([
            "done",
            format!("orphan_repair_artifact_at_head:{prefix}")
        ]))
    } else {
        Ok(json!([
            "backlog",
            format!("orphan_repair_requeued:{prefix}")
        ]))
    }
}
