//! Deterministic provider quota policy validation and label decisions.
//!
//! The caller supplies the already-loaded policy and label inventory. This
//! crate deliberately performs no filesystem, network, CLI, git, or mutation
//! operations.

use agentmesh_ops_kit::OpsError;
use serde_json::{json, Map, Value};

/// Validate a loaded provider quota policy and return it unchanged.
///
/// This mirrors the schema version, requeue bound, and required label-field
/// checks performed by the Python `load_policy` helper.
pub fn load_policy(policy: &Value) -> Result<Value, OpsError> {
    let Some(policy_object) = policy.as_object() else {
        return Err(OpsError::invalid_request("policy must be an object"));
    };

    let schema_version = policy_object.get("schema_version");
    let schema_is_one = match schema_version {
        Some(Value::Bool(true)) => true,
        Some(Value::Number(number)) => number.as_f64() == Some(1.0),
        _ => false,
    };
    let requeue_count = policy_object.get("max_requeues_per_tick");
    let within_requeue_bounds = match requeue_count {
        Some(Value::Bool(true)) => true,
        Some(Value::Number(number)) => number
            .as_f64()
            .is_some_and(|value| (1.0..=5.0).contains(&value)),
        _ => false,
    };
    if !schema_is_one || !within_requeue_bounds {
        return Err(OpsError::failed("invalid_provider_quota_recovery_policy"));
    }

    let labels = policy_object.get("labels").and_then(Value::as_object);
    for key in ["free_model", "quota_hold"] {
        let config = labels
            .and_then(|entries| entries.get(key))
            .and_then(Value::as_object);
        let required_fields_are_truthy = config.is_some_and(|fields| {
            ["name", "id"]
                .iter()
                .all(|field| fields.get(*field).is_some_and(python_truthy))
        });
        if !required_fields_are_truthy {
            return Err(OpsError::failed(format!(
                "invalid_provider_quota_label:{key}"
            )));
        }
    }

    Ok(policy.clone())
}

/// Resolve a configured label against an already-loaded inventory result.
///
/// The fallback is the exact configured object, as in Python's `dict(config)`.
/// A result is accepted only when exactly one row has the configured name and
/// its id is truthy. Inventory errors and malformed shapes therefore fall
/// back without granting this crate an API dependency.
pub fn resolve_label(inventory: Option<&Value>, config: &Value) -> Value {
    let Some(config_object) = config.as_object() else {
        return config.clone();
    };
    let Some(config_name) = config_object.get("name") else {
        return config.clone();
    };
    let Some(raw_inventory) = inventory else {
        return config.clone();
    };
    let rows = match raw_inventory {
        Value::Array(rows) => Some(rows),
        Value::Object(inventory_object) => inventory_object.get("labels").and_then(Value::as_array),
        _ => None,
    };
    let Some(rows) = rows else {
        return config.clone();
    };
    if rows.iter().any(|row| !row.is_object()) {
        return config.clone();
    }

    let matching: Vec<&Map<String, Value>> = rows
        .iter()
        .filter_map(Value::as_object)
        .filter(|row| row.get("name") == Some(config_name))
        .collect();
    if matching.len() != 1 {
        return config.clone();
    }
    let Some(id) = matching[0].get("id").filter(|id| python_truthy(id)) else {
        return config.clone();
    };

    json!({"name": config_name, "id": id})
}

/// Test whether an issue's existing labels contain the resolved label.
///
/// Strings may contain either the label id or name. Object labels may use
/// `id`, `name`, or `label_id`, matching the Python helper.
pub fn has_label(issue: &Value, label: &Value) -> bool {
    let Some(issue_object) = issue.as_object() else {
        return false;
    };
    let Some(label_object) = label.as_object() else {
        return false;
    };
    let label_id = label_object.get("id");
    let label_name = label_object.get("name");
    let Some(labels) = issue_object.get("labels").and_then(Value::as_array) else {
        return false;
    };

    labels.iter().any(|item| match item {
        Value::String(_) => Some(item) == label_id || Some(item) == label_name,
        Value::Object(item) => {
            item.get("id") == label_id
                || item.get("name") == label_name
                || item.get("label_id") == label_id
        }
        _ => false,
    })
}

/// Evaluate a request containing the policy, label inventory, and issue data.
///
/// The returned `policy` and `label` values are suitable for direct JSON
/// comparison against the Python helpers' output; `has_label` is the
/// deterministic membership decision.
pub fn decide(request: Value) -> Result<Value, OpsError> {
    let request_object = request
        .as_object()
        .ok_or_else(|| OpsError::invalid_request("request must be an object"))?;
    let policy_input = request_object
        .get("policy")
        .ok_or_else(|| OpsError::invalid_request("policy is required"))?;
    let policy = load_policy(policy_input)?;

    let label_key = request_object
        .get("label_key")
        .and_then(Value::as_str)
        .ok_or_else(|| OpsError::invalid_request("label_key must be a string"))?;
    if !matches!(label_key, "free_model" | "quota_hold") {
        return Err(OpsError::invalid_request("unsupported label_key"));
    }
    let config = policy
        .pointer(&format!("/labels/{label_key}"))
        .ok_or_else(|| OpsError::invalid_request("label config is required"))?;
    let inventory = request_object.get("inventory");
    let label = resolve_label(inventory, config);
    let issue = request_object
        .get("issue")
        .ok_or_else(|| OpsError::invalid_request("issue is required"))?;
    if !issue.is_object()
        || issue
            .get("labels")
            .is_some_and(|labels| !labels.is_null() && !labels.is_array())
    {
        return Err(OpsError::invalid_request(
            "issue.labels must be an array or null",
        ));
    }

    Ok(json!({
        "policy": policy,
        "label": label,
        "has_label": has_label(issue, &label),
    }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_only_one_named_inventory_row_with_an_id() {
        let config = json!({"name": "model/free", "id": "fallback", "other": "kept on fallback"});
        assert_eq!(
            resolve_label(
                Some(&json!({"labels": [{"name": "model/free", "id": "live"}]})),
                &config
            ),
            json!({"name": "model/free", "id": "live"})
        );
        assert_eq!(
            resolve_label(
                Some(&json!({"labels": [
                    {"name": "model/free", "id": "one"},
                    {"name": "model/free", "id": "two"}
                ]})),
                &config
            ),
            config
        );
    }

    #[test]
    fn validates_policy_bounds_and_required_labels() {
        let policy = json!({
            "schema_version": 1,
            "max_requeues_per_tick": 5,
            "labels": {
                "free_model": {"name": "model/free", "id": "free"},
                "quota_hold": {"name": "provider/quota-hold", "id": "hold"}
            }
        });
        assert_eq!(load_policy(&policy), Ok(policy.clone()));

        let mut invalid = policy.clone();
        invalid["max_requeues_per_tick"] = json!(6);
        assert_eq!(
            load_policy(&invalid).unwrap_err().message,
            "invalid_provider_quota_recovery_policy"
        );
        invalid = policy.clone();
        invalid["labels"]["quota_hold"]["name"] = json!("");
        assert_eq!(
            load_policy(&invalid).unwrap_err().message,
            "invalid_provider_quota_label:quota_hold"
        );
    }

    #[test]
    fn recognizes_supported_existing_label_representations() {
        let label = json!({"id": "resolved", "name": "model/free"});
        for existing in [
            json!("resolved"),
            json!("model/free"),
            json!({"id": "resolved"}),
            json!({"name": "model/free"}),
            json!({"label_id": "resolved"}),
        ] {
            assert!(has_label(&json!({"labels": [existing]}), &label));
        }
        assert!(!has_label(&json!({"labels": []}), &label));
    }
}
