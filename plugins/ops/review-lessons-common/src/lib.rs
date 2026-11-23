//! Offline port of `review-lessons-common.py` index construction and timestamp parsing.

mod stamp;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Map, Value};
use stamp::{Stamp, DAY};

// Keys from Python's fixed ADVICE table. The advice text is not part of the index.
const ADVICE_KEYS: &[&str] = &[
    "G1-release-state-consistency",
    "G2-package-docs-consistency",
    "G3-schema-runtime-contract",
    "G4-migration-no-clobber",
    "G5-error-fail-closed",
    "G6-replay-concurrency",
    "G7-path-platform-boundary",
    "G8-ci-workflow-permissions",
    "G9-bounded-work",
    "G10-behavioral-test-contract",
    "G11-docs-implementation-contract",
];

type Object = Map<String, Value>;

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn python_scalar(value: &Value) -> Result<String, OpsError> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Null => Ok("None".into()),
        Value::Bool(true) => Ok("True".into()),
        Value::Bool(false) => Ok("False".into()),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err(OpsError::invalid_request("expected a scalar identifier")),
    }
}

fn parse_stamp(value: &Value) -> Option<Stamp> {
    stamp::parse(&python_scalar(value).ok()?)
}

#[derive(PartialEq, Eq, Hash)]
enum Identity {
    Text(String),
    Number(String),
}

fn identity(record: &Value) -> Result<Option<Identity>, OpsError> {
    let id = if truthy(&record["finding_url"]) {
        &record["finding_url"]
    } else if truthy(&record["comment_id"]) {
        return Ok(Some(Identity::Text(format!(
            "{}:{}",
            python_scalar(&record["repo"])?,
            python_scalar(&record["comment_id"])?
        ))));
    } else {
        &record["evidence_id"]
    };
    match id {
        _ if !truthy(id) => Ok(None),
        Value::String(s) => Ok(Some(Identity::Text(s.clone()))),
        Value::Bool(true) => Ok(Some(Identity::Number("1".into()))),
        Value::Number(n) => {
            // Python considers 1, 1.0 and True the same hash key. Do not
            // round integer JSON IDs through f64, where large IDs may collide.
            let key = match n.as_f64().filter(|f| n.is_f64() && f.fract() == 0.0) {
                Some(f) => format!("{f:.0}"),
                None => n.to_string(),
            };
            Ok(Some(Identity::Number(key)))
        }
        _ => Err(OpsError::invalid_request("expected a scalar identity")),
    }
}

fn repo_slug(repo: &str) -> Option<&str> {
    let slug = repo.strip_prefix("eiei114/")?;
    let is_alnum = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    (is_alnum(*slug.as_bytes().first()?) && slug.bytes().all(|c| is_alnum(c) || c == b'-'))
        .then_some(slug)
}

struct Finding<'a> {
    record: &'a Value,
    at: Stamp,
}

fn stats<'a>(items: impl Iterator<Item = &'a Finding<'a>>, now: i64) -> Value {
    let (week, month) = (now - 7 * DAY, now - 30 * DAY);
    let (mut count_7, mut count_30, mut count_90, mut open) = (0, 0, 0, 0);
    let mut last: Option<Stamp> = None;
    for Finding { record, at } in items {
        let instant = at.instant();
        count_7 += usize::from(instant >= week);
        count_30 += usize::from(instant >= month);
        count_90 += 1;
        if last.is_none_or(|previous| instant > previous.instant()) {
            last = Some(*at);
        }
        if record["resolved_at_observation"] == false
            && record["outdated_at_observation"] == false
            && parse_stamp(&record["observed_at"]).is_some_and(|t| t.instant() >= week)
        {
            open += 1;
        }
    }
    json!({
        "findings_7d": count_7, "findings_30d": count_30, "findings_90d": count_90,
        "last_finding_at": last.expect("only nonempty groups are aggregated").isoformat(),
        "recently_observed_open": open,
    })
}

fn lesson(
    key: &str,
    rule: &Value,
    items: &[Finding<'_>],
    aliases: &Object,
    now: i64,
) -> Result<Option<Value>, OpsError> {
    let mut project_index = HashMap::new();
    // Insertion order is observable when two last findings share an instant
    // but use different offsets: Python max() takes the first in group order.
    let mut projects = Vec::<(&str, Vec<&Finding<'_>>)>::new();
    for item in items {
        let repo = item
            .record
            .get("repo")
            .map_or(Some(""), Value::as_str)
            .ok_or_else(|| OpsError::invalid_request("repo must be a string"))?;
        if let Some(slug) = repo_slug(repo) {
            let project = aliases.get(slug).and_then(Value::as_str).unwrap_or(slug);
            let slot = *project_index.entry(project).or_insert_with(|| {
                projects.push((project, Vec::new()));
                projects.len() - 1
            });
            projects[slot].1.push(item);
        }
    }
    if projects.is_empty() {
        return Ok(None);
    }

    let (mut counts, mut project_stats, mut sources) =
        (Object::new(), Object::new(), Object::new());
    let mut count = 0;
    for (project, findings) in &projects {
        count += findings.len();
        counts.insert((*project).into(), json!(findings.len()));
        project_stats.insert((*project).into(), stats(findings.iter().copied(), now));
        let urls: BTreeSet<_> = findings
            .iter()
            .filter_map(|item| {
                let repo = item.record["repo"].as_str()?;
                let url = item.record["finding_url"].as_str()?;
                url.starts_with(&format!("https://github.com/{repo}/pull/"))
                    .then_some(url)
            })
            .collect();
        sources.insert(
            (*project).into(),
            json!(urls.into_iter().take(3).collect::<Vec<_>>()),
        );
    }
    Ok(Some(json!({
        "key": key,
        "attributes": rule.get("attributes").cloned().unwrap_or_else(|| json!({})),
        "project_counts": counts,
        "finding_count": count,
        "stats": stats(projects.iter().flat_map(|(_, items)| items.iter().copied()), now),
        "project_stats": project_stats,
        "immediate_risk": items.iter().any(|item| item.record["immediate_candidate"] == true),
        "repository_count": projects.len(),
        "common": count >= 3 && projects.len() >= 2,
        "scope_level": rule.get("scope_level").cloned().unwrap_or_else(|| json!("global")),
        "project_keys": rule.get("project_keys").cloned().unwrap_or_else(|| json!([])),
        "exception_adr": rule["exception_adr"],
        "sources": sources,
    })))
}

fn build_index(req: &Value) -> Result<Value, OpsError> {
    let now = req["now"]
        .as_str()
        .and_then(stamp::parse)
        .filter(|s| s.window_start_is_valid())
        .ok_or_else(|| {
            OpsError::invalid_request("now must be an ISO datetime with a valid 90-day window")
        })?;
    let records = req["records"]
        .as_array()
        .ok_or_else(|| OpsError::invalid_request("records must be an array"))?;
    let registry = req["registry"]
        .as_object()
        .ok_or_else(|| OpsError::invalid_request("registry must be an object"))?;
    let aliases = req["repo_aliases"]
        .as_object()
        .filter(|m| m.values().all(Value::is_string))
        .ok_or_else(|| OpsError::invalid_request("repo_aliases must be an object of strings"))?;
    let mut rules = HashMap::new();
    if let Some(guardrails) = registry.get("guardrails") {
        for rule in guardrails
            .as_array()
            .ok_or_else(|| OpsError::invalid_request("guardrails must be an array"))?
        {
            if !rule.is_object() {
                return Err(OpsError::invalid_request("guardrails must contain objects"));
            }
            if let Some(key) = rule["key"].as_str().filter(|k| ADVICE_KEYS.contains(k)) {
                rules.insert(key, rule);
            }
        }
    }

    let now_instant = now.instant();
    let window = now_instant - 90 * DAY..=now_instant;
    let mut seen = HashSet::new();
    let mut grouped = BTreeMap::<&str, Vec<Finding<'_>>>::new();
    for record in records {
        if !record.is_object() {
            return Err(OpsError::invalid_request("records must contain objects"));
        }
        if record["source"] == "coderabbit-monitor" && record["category"] != "inline_comment" {
            continue;
        }
        let Some(key) = record["normalized_guardrail_key"]
            .as_str()
            .filter(|k| rules.contains_key(k))
        else {
            continue;
        };
        let created = &record["finding_created_at"];
        let timestamp = if truthy(created) {
            created
        } else {
            &record["observed_at"]
        };
        let Some(at) = parse_stamp(timestamp).filter(|t| window.contains(&t.instant())) else {
            continue;
        };
        if let Some(id) = identity(record)? {
            if seen.insert(id) {
                grouped.entry(key).or_default().push(Finding { record, at });
            }
        }
    }
    let mut lessons = Vec::new();
    for (key, items) in grouped {
        if let Some(row) = lesson(key, rules[key], &items, aliases, now_instant)? {
            lessons.push(row);
        }
    }
    Ok(
        json!({"schema_version": 1, "built_at": now.isoformat(), "window_days": 90,
        "evidence_count": seen.len(), "lessons": lessons}),
    )
}

/// Dispatch one ops-kit request to `stamp` or `build_index`.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    match request["op"].as_str() {
        Some("build_index") => build_index(&request),
        Some("stamp") => request
            .get("value")
            .map(|v| json!(parse_stamp(v).map(Stamp::isoformat)))
            .ok_or_else(|| OpsError::invalid_request("stamp requires value")),
        _ => Err(OpsError::invalid_request(
            "expected op: build_index or stamp",
        )),
    }
}
