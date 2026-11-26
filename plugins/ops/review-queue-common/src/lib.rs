//! Advisory reporting for already classified review queue rows. No action is executed.

use std::collections::{BTreeMap, HashMap};

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Map, Number, Value};

// Optional flags in the Python reporter use truthiness, not boolean equality.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty(),
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
    }
}

/// Return the reporting label for a classified row. Precedence matches Python.
pub fn next_action(row: &Map<String, Value>) -> &'static str {
    let reason = row.get("reason").and_then(Value::as_str);
    match reason {
        _ if row.get("deferral_budget").is_some_and(truthy) => "rescan_within_scan_budget",
        Some("classification_deferred" | "classification_time_deferred") => {
            "rescan_within_scan_budget"
        }
        Some("worker_still_active" | "review_repair_pending" | "repair_worker_not_completed") => {
            "wait_for_native_worker"
        }
        Some("protected_publication_requires_separation" | "publication_hold_invalid") => {
            "static_publication_separation_then_independent_review"
        }
        _ if row.get("role").and_then(Value::as_str) == Some("child") => {
            if reason.is_some_and(|reason| !reason.is_empty()) {
                "wait_for_parent_delivery"
            } else {
                "fresh_parent_child_cleanup"
            }
        }
        Some("review_waiting_for_new_evidence") => "new_head_run_ci_or_retry_after",
        None if row.get("independent_reviewed").is_some_and(truthy) => "verified_merge_done",
        None | Some("independent_changes_requested") => "independent_review",
        Some("unresolved_review_threads") => "independent_finding_verification",
        Some("no_action_requires_review") => "independent_no_action_disposition",
        Some("no_linked_pr") => "bounded_native_linkage_or_resource_blocker",
        Some("merge_conflict") => "bounded_conflict_repair_on_pr_branch",
        Some(
            "duplicate_repair_issue" | "repair_inventory_incomplete" | "two_repairs_exhausted",
        ) => "inspect_existing_repairs_no_new_child",
        Some(reason)
            if ["sdk_", "completion_", "checkout_"]
                .iter()
                .any(|prefix| reason.starts_with(prefix)) =>
        {
            "actual_verification_or_provenance_repair"
        }
        _ => "fresh_specific_blocker_investigation",
    }
}

#[derive(Default)]
struct Group {
    oldest: Option<Number>,
    unknown: usize,
    keys: Vec<Value>,
}

impl Group {
    fn add(&mut self, key: Value, age: Option<&Number>) {
        self.keys.push(key);
        if let Some(age) = age {
            // Python compares against `oldest_issue_hours or 0`, keeping the
            // first operand on ties (including an integer 0 vs a float 0.0).
            let oldest = self
                .oldest
                .take()
                .filter(|n| n.as_f64() != Some(0.0))
                .unwrap_or_else(|| Number::from(0));
            self.oldest = Some(if age.as_f64() > oldest.as_f64() {
                age.clone()
            } else {
                oldest
            });
        } else {
            self.unknown += 1;
        }
    }
}

/// Summarize classified rows in input order, adding or replacing `next_action`.
pub fn queue_summary(
    mut rows: Vec<Value>,
    complete: bool,
    deferred: u64,
) -> Result<Value, OpsError> {
    let mut indices = HashMap::new();
    let mut groups = Vec::new();
    let (mut sources, mut children, mut in_review, mut done) = (0, 0, 0, 0);
    let (mut source_reasons, mut child_reasons) = (BTreeMap::new(), BTreeMap::new());

    for row in &mut rows {
        let invalid_row = || OpsError::invalid_request("invalid classified row");
        let row = row.as_object_mut().ok_or_else(invalid_row)?;
        let nullable_text = |field: &str| match row.get(field) {
            Some(Value::String(text)) => Ok(Some(text.as_str())),
            Some(Value::Null) => Ok(None),
            _ => Err(invalid_row()),
        };
        let project = nullable_text("project")?;
        nullable_text("key")?;
        let role = row
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(invalid_row)?;
        let reason = nullable_text("reason")?
            .filter(|r| !r.is_empty())
            .unwrap_or("ready");
        let age = match row.get("issue_age_hours") {
            Some(Value::Number(age)) => Some(age),
            Some(Value::Null) => None,
            _ => return Err(invalid_row()),
        };
        match role {
            "source" => {
                let status = nullable_text("status")?;
                sources += 1;
                in_review += usize::from(status == Some("in_review"));
                done += usize::from(status == Some("done"));
                *source_reasons.entry(reason.to_owned()).or_insert(0) += 1;
            }
            "child" => {
                children += 1;
                *child_reasons.entry(reason.to_owned()).or_insert(0) += 1;
            }
            _ => return Err(invalid_row()),
        }

        let action = next_action(row);
        let key = (
            project.map(str::to_owned),
            role.to_owned(),
            reason.to_owned(),
            action,
        );
        let index = *indices.entry(key.clone()).or_insert_with(|| {
            let index = groups.len();
            groups.push((key, Group::default()));
            index
        });
        groups[index].1.add(row["key"].clone(), age);
        row.insert("next_action".into(), json!(action));
    }

    let coverage = match (complete, deferred == 0) {
        (true, true) => "complete_classification",
        (true, false) => "partial_budget_deferred",
        (false, true) => "partial_errors",
        (false, false) => "partial_errors_and_budget_deferred",
    };
    let groups: Vec<_> = groups
        .into_iter()
        .map(|((project, role, reason, action), group)| {
            json!({
                "project": project, "role": role, "reason": reason, "next_action": action,
                "count": group.keys.len(), "oldest_issue_hours": group.oldest,
                "unknown_age": group.unknown, "keys": group.keys,
            })
        })
        .collect();
    Ok(json!({
        "coverage": coverage, "deferred_sources": deferred,
        "sources": sources, "children": children, "in_review_sources": in_review,
        "done_cleanup_sources": done, "source_reasons": source_reasons, "child_reasons": child_reasons,
        "groups": groups, "rows": rows,
        "basis": "issue_age and last_activity, not claimed exact review-entry age; no delivery counts",
    }))
}

/// Wire handler. Defaults match the Python keyword arguments.
pub fn handle(request: Value) -> Result<Value, OpsError> {
    let invalid = || OpsError::invalid_request("invalid review queue summary request");
    let mut request = match request {
        Value::Object(fields) => fields,
        _ => return Err(invalid()),
    };
    let complete = match request.get("complete") {
        Some(value) => value.as_bool().ok_or_else(invalid)?,
        None => true,
    };
    let deferred = match request.get("deferred") {
        Some(value) => value.as_u64().ok_or_else(invalid)?,
        None => 0,
    };
    let rows = match request.remove("rows") {
        Some(Value::Array(rows)) => rows,
        _ => return Err(invalid()),
    };
    queue_summary(rows, complete, deferred)
}
