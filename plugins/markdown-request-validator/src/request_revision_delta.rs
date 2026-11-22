//! Deterministic request revision delta App.
//!
//! This module compares two already-validated canonical request summaries for
//! one scope. It never opens request sources, renders Markdown, discovers
//! adapters, or writes files. Stable canonical fields and adapter-owned extras
//! are classified separately; adapter extra values are never echoed or hashed.

use crate::adapter_compact_helpers::canonical_json_bytes;
use agentmesh_evidence::sha256_prefixed;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

/// Contract version accepted by this App and copied into delta output.
pub const DELTA_CONTRACT_VERSION: &str = "agentmesh-request-revision-delta.v0";
const INPUT_SCHEMA_VERSION: &str = "request-revision-delta-input.v0";
const SUMMARY_SCHEMA_VERSION: &str = "request-revision-summary.v0";
const OUTPUT_SCHEMA_VERSION: &str = "request-revision-delta-compact.v0";
const MAX_INPUT_BYTES: usize = 32 * 1024;
const MAX_SCOPE_BYTES: usize = 128;
const MAX_TEXT_BYTES: usize = 1024;
const MAX_LIST_ITEMS: usize = 32;
const MAX_EXTRAS: usize = 32;
const MAX_EXTRA_PATH_BYTES: usize = 64;

/// Stable canonical field order. The flag is true when the field is required.
const CANONICAL_FIELDS: &[(&str, bool)] = &[
    ("title", true),
    ("request_kind", true),
    ("body_summary", false),
    ("acceptance_criteria", true),
    ("blocked_by", false),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    InputTooLarge,
    MalformedInput,
    UnsupportedSchemaVersion,
    UnsupportedContractVersion,
    RequiredFieldMissing,
    InvalidField,
    CrossScopeInput,
}

impl Failure {
    const fn code(self) -> &'static str {
        match self {
            Self::InputTooLarge => "INPUT_TOO_LARGE",
            Self::MalformedInput => "MALFORMED_INPUT",
            Self::UnsupportedSchemaVersion => "UNSUPPORTED_SCHEMA_VERSION",
            Self::UnsupportedContractVersion => "UNSUPPORTED_CONTRACT_VERSION",
            Self::RequiredFieldMissing => "REQUIRED_FIELD_MISSING",
            Self::InvalidField => "INVALID_FIELD",
            Self::CrossScopeInput => "CROSS_SCOPE_INPUT",
        }
    }
}

struct Summary<'a> {
    scope: &'a str,
    canonical: &'a Map<String, Value>,
    extras: &'a Map<String, Value>,
}

/// Compute the deterministic revision delta between two canonical summaries
/// of one request scope, or one normalized failure with no partial delta.
pub fn compute_request_revision_delta(value: &Value) -> Value {
    let supplied_contract = value
        .get("contract_version")
        .and_then(Value::as_str)
        .filter(|version| version.len() <= 128);
    match validate_and_diff(value) {
        Ok(delta) => delta,
        Err(failure) => failure_output(failure, supplied_contract),
    }
}

fn validate_and_diff(value: &Value) -> Result<Value, Failure> {
    if canonical_json_bytes(value).len() > MAX_INPUT_BYTES {
        return Err(Failure::InputTooLarge);
    }
    let root = object(value)?;
    exact_keys(
        root,
        &["schema_version", "contract_version", "base", "revised"],
    )?;
    if string(root, "schema_version")? != INPUT_SCHEMA_VERSION {
        return Err(Failure::UnsupportedSchemaVersion);
    }
    let contract_version = string(root, "contract_version")?;
    if contract_version != DELTA_CONTRACT_VERSION {
        return Err(Failure::UnsupportedContractVersion);
    }

    let base = summary(field(root, "base")?)?;
    let revised = summary(field(root, "revised")?)?;
    if base.scope != revised.scope {
        return Err(Failure::CrossScopeInput);
    }

    let mut contract_affecting = false;
    let mut canonical_changes = Vec::with_capacity(CANONICAL_FIELDS.len());
    for (name, _) in CANONICAL_FIELDS {
        let before = base.canonical.get(*name);
        let after = revised.canonical.get(*name);
        let class = change_class(before, after);
        if class != "unchanged" {
            contract_affecting = true;
        }
        canonical_changes.push(json!({
            "path": name,
            "change_class": class,
            "base_digest": before.map(value_digest),
            "revised_digest": after.map(value_digest),
        }));
    }

    let paths: BTreeSet<&String> = base.extras.keys().chain(revised.extras.keys()).collect();
    let mut extras_changed = false;
    let mut extra_changes = Vec::with_capacity(paths.len());
    for path in paths {
        let class = change_class(base.extras.get(path), revised.extras.get(path));
        if class != "unchanged" {
            extras_changed = true;
        }
        extra_changes.push(json!({"path": path, "change_class": class}));
    }

    let mut diagnostics = Vec::new();
    if contract_affecting {
        diagnostics.push("canonical_contract_change");
    }
    if extras_changed {
        diagnostics.push("adapter_extras_changed");
    }
    if diagnostics.is_empty() {
        diagnostics.push("no_change");
    }

    Ok(json!({
        "schema_version": OUTPUT_SCHEMA_VERSION,
        "valid": true,
        "contract_version": contract_version,
        "contract_affecting": contract_affecting,
        "canonical_changes": canonical_changes,
        "adapter_extra_changes": extra_changes,
        "diagnostics": diagnostics,
        "error_count": 0,
        "errors": [],
    }))
}

fn summary(value: &Value) -> Result<Summary<'_>, Failure> {
    let root = object(value)?;
    exact_keys(
        root,
        &[
            "schema_version",
            "contract_version",
            "scope_key",
            "canonical_fields",
            "adapter_extras",
        ],
    )?;
    if string(root, "schema_version")? != SUMMARY_SCHEMA_VERSION {
        return Err(Failure::UnsupportedSchemaVersion);
    }
    if string(root, "contract_version")? != DELTA_CONTRACT_VERSION {
        return Err(Failure::UnsupportedContractVersion);
    }
    let scope = string(root, "scope_key")?;
    if !valid_scope(scope) {
        return Err(Failure::InvalidField);
    }

    let canonical = object(field(root, "canonical_fields")?)?;
    if canonical
        .keys()
        .any(|key| !CANONICAL_FIELDS.iter().any(|(name, _)| name == key))
    {
        return Err(Failure::MalformedInput);
    }
    for (name, required) in CANONICAL_FIELDS {
        match canonical.get(*name) {
            None if *required => return Err(Failure::RequiredFieldMissing),
            None => {}
            Some(value) => validate_canonical_value(value)?,
        }
    }

    let extras = object(field(root, "adapter_extras")?)?;
    if extras.len() > MAX_EXTRAS {
        return Err(Failure::InvalidField);
    }
    if extras.keys().any(|path| !valid_extra_path(path)) {
        return Err(Failure::InvalidField);
    }
    Ok(Summary {
        scope,
        canonical,
        extras,
    })
}

fn validate_canonical_value(value: &Value) -> Result<(), Failure> {
    match value {
        Value::String(text) if text.len() <= MAX_TEXT_BYTES => Ok(()),
        Value::Array(items)
            if items.len() <= MAX_LIST_ITEMS
                && items.iter().all(|item| {
                    item.as_str()
                        .is_some_and(|text| text.len() <= MAX_TEXT_BYTES)
                }) =>
        {
            Ok(())
        }
        _ => Err(Failure::InvalidField),
    }
}

fn change_class(before: Option<&Value>, after: Option<&Value>) -> &'static str {
    match (before, after) {
        (None, None) => "unchanged",
        (None, Some(_)) => "added",
        (Some(_), None) => "removed",
        (Some(left), Some(right)) => {
            if canonical_json_bytes(left) == canonical_json_bytes(right) {
                "unchanged"
            } else {
                "changed"
            }
        }
    }
}

fn value_digest(value: &Value) -> String {
    sha256_prefixed(&canonical_json_bytes(value))
}

fn failure_output(failure: Failure, contract_version: Option<&str>) -> Value {
    json!({
        "schema_version": OUTPUT_SCHEMA_VERSION,
        "valid": false,
        "contract_version": contract_version,
        "contract_affecting": null,
        "canonical_changes": [],
        "adapter_extra_changes": [],
        "diagnostics": [],
        "error_count": 1,
        "errors": [{"code": failure.code()}],
    })
}

fn object(value: &Value) -> Result<&Map<String, Value>, Failure> {
    value.as_object().ok_or(Failure::MalformedInput)
}

fn exact_keys(object: &Map<String, Value>, required: &[&str]) -> Result<(), Failure> {
    if required.iter().any(|key| !object.contains_key(*key)) {
        return Err(Failure::RequiredFieldMissing);
    }
    if object.len() != required.len() {
        return Err(Failure::MalformedInput);
    }
    Ok(())
}

fn field<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a Value, Failure> {
    object.get(key).ok_or(Failure::RequiredFieldMissing)
}

fn string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, Failure> {
    field(object, key)?.as_str().ok_or(Failure::MalformedInput)
}

fn valid_extra_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_EXTRA_PATH_BYTES
        && value
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn valid_scope(value: &str) -> bool {
    value.len() <= MAX_SCOPE_BYTES
        && value.starts_with("agentmesh:")
        && value.len() > 10
        && value
            .as_bytes()
            .get(10)
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && value[10..].bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b':' | b'.' | b'_' | b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const FIXTURES: &[(&str, &str)] = &[
        (
            "request_revision_delta_identical_input.json",
            "request_revision_delta_identical_expected.json",
        ),
        (
            "request_revision_delta_canonical_criterion_edit_input.json",
            "request_revision_delta_canonical_criterion_edit_expected.json",
        ),
        (
            "request_revision_delta_adapter_extra_only_input.json",
            "request_revision_delta_adapter_extra_only_expected.json",
        ),
        (
            "request_revision_delta_cross_scope_input.json",
            "request_revision_delta_cross_scope_expected.json",
        ),
        (
            "request_revision_delta_malformed_input.json",
            "request_revision_delta_malformed_expected.json",
        ),
    ];

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join(name)
    }

    fn fixture(name: &str) -> Value {
        serde_json::from_slice(&std::fs::read(fixture_path(name)).unwrap()).unwrap()
    }

    #[test]
    fn documented_fixtures_match_and_repeat_byte_identically() {
        for (input_name, expected_name) in FIXTURES {
            let input = fixture(input_name);
            let actual = compute_request_revision_delta(&input);
            assert_eq!(actual, fixture(expected_name), "fixture {input_name}");
            assert_eq!(
                serde_json::to_vec(&actual).unwrap(),
                serde_json::to_vec(&compute_request_revision_delta(&input)).unwrap(),
                "repeat fixture {input_name}"
            );
        }
    }

    #[test]
    fn identical_pair_is_all_unchanged_and_not_contract_affecting() {
        let output = compute_request_revision_delta(&fixture(FIXTURES[0].0));
        assert_eq!(output["contract_affecting"], false);
        assert_eq!(output["diagnostics"], json!(["no_change"]));
        assert!(output["canonical_changes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|change| change["change_class"] == "unchanged"));
        let paths: Vec<&str> = output["canonical_changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|change| change["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            [
                "title",
                "request_kind",
                "body_summary",
                "acceptance_criteria",
                "blocked_by"
            ]
        );
    }

    #[test]
    fn only_canonical_changes_set_the_contract_affecting_flag() {
        let canonical = compute_request_revision_delta(&fixture(FIXTURES[1].0));
        assert_eq!(canonical["contract_affecting"], true);
        assert_eq!(
            canonical["diagnostics"],
            json!(["canonical_contract_change"])
        );

        let extras = compute_request_revision_delta(&fixture(FIXTURES[2].0));
        assert_eq!(extras["contract_affecting"], false);
        assert_eq!(extras["diagnostics"], json!(["adapter_extras_changed"]));
        assert!(extras["canonical_changes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|change| change["change_class"] == "unchanged"));
        let classes: Vec<(&str, &str)> = extras["adapter_extra_changes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|change| change["change_class"] != "unchanged")
            .map(|change| {
                (
                    change["path"].as_str().unwrap(),
                    change["change_class"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            classes,
            [
                ("adapter.added_note", "added"),
                ("adapter.label", "changed"),
                ("adapter.legacy", "removed"),
            ]
        );
    }

    #[test]
    fn optional_canonical_fields_report_added_and_removed() {
        let mut input = fixture(FIXTURES[0].0);
        input["base"]["canonical_fields"]
            .as_object_mut()
            .unwrap()
            .remove("body_summary");
        input["revised"]["canonical_fields"]
            .as_object_mut()
            .unwrap()
            .remove("blocked_by");
        let output = compute_request_revision_delta(&input);
        assert_eq!(output["contract_affecting"], true);
        assert_eq!(output["canonical_changes"][2]["change_class"], "added");
        assert!(output["canonical_changes"][2]["base_digest"].is_null());
        assert_eq!(output["canonical_changes"][4]["change_class"], "removed");
        assert!(output["canonical_changes"][4]["revised_digest"].is_null());
    }

    #[test]
    fn output_never_contains_adapter_extra_values_or_scope() {
        let input = fixture(FIXTURES[2].0);
        let encoded = serde_json::to_string(&compute_request_revision_delta(&input)).unwrap();
        for secret in [
            "token-value-should-not-leak",
            "C:/sidecar/path",
            "tracker-9999",
        ] {
            assert!(
                serde_json::to_string(&input).unwrap().contains(secret),
                "fixture must carry {secret}"
            );
            assert!(!encoded.contains(secret), "leaked {secret}");
        }
        assert!(!encoded.contains("agentmesh:app:request-revision-delta"));
    }

    #[test]
    fn failures_have_no_partial_delta() {
        type Mutation = Box<dyn Fn(&mut Value)>;
        let cases: Vec<(Mutation, &str)> = vec![
            (
                Box::new(|v| {
                    v["base"]["canonical_fields"]
                        .as_object_mut()
                        .unwrap()
                        .remove("acceptance_criteria");
                }),
                "REQUIRED_FIELD_MISSING",
            ),
            (
                Box::new(|v| v["contract_version"] = json!("future.v9")),
                "UNSUPPORTED_CONTRACT_VERSION",
            ),
            (
                Box::new(|v| v["revised"]["contract_version"] = json!("future.v9")),
                "UNSUPPORTED_CONTRACT_VERSION",
            ),
            (
                Box::new(|v| v["schema_version"] = json!("request-revision-delta-input.v9")),
                "UNSUPPORTED_SCHEMA_VERSION",
            ),
            (
                Box::new(|v| v["revised"]["schema_version"] = json!("request-revision-summary.v9")),
                "UNSUPPORTED_SCHEMA_VERSION",
            ),
            (
                Box::new(|v| v["base"]["canonical_fields"]["surprise"] = json!("x")),
                "MALFORMED_INPUT",
            ),
            (
                Box::new(|v| v["base"]["canonical_fields"]["title"] = json!(7)),
                "INVALID_FIELD",
            ),
            (
                Box::new(|v| v["base"]["adapter_extras"]["Bad Path"] = json!(1)),
                "INVALID_FIELD",
            ),
            (
                Box::new(|v| v["revised"]["scope_key"] = json!("agentmesh:app:other")),
                "CROSS_SCOPE_INPUT",
            ),
            (Box::new(|v| *v = json!([1, 2])), "MALFORMED_INPUT"),
        ];
        for (mutate, code) in cases {
            let mut input = fixture(FIXTURES[0].0);
            mutate(&mut input);
            let output = compute_request_revision_delta(&input);
            assert_eq!(output["errors"][0]["code"], code);
            assert_eq!(output["valid"], false);
            assert!(output["contract_affecting"].is_null());
            assert!(output["canonical_changes"].as_array().unwrap().is_empty());
            assert!(output["adapter_extra_changes"]
                .as_array()
                .unwrap()
                .is_empty());
            assert!(output["diagnostics"].as_array().unwrap().is_empty());
        }
    }

    #[test]
    fn oversized_input_fails_without_partial_delta() {
        let oversized = json!({"payload": "x".repeat(MAX_INPUT_BYTES + 1)});
        let output = compute_request_revision_delta(&oversized);
        assert_eq!(output["errors"][0]["code"], "INPUT_TOO_LARGE");
        assert!(output["canonical_changes"].as_array().unwrap().is_empty());
    }

    #[test]
    fn extras_count_is_bounded() {
        let mut input = fixture(FIXTURES[0].0);
        for index in 0..=MAX_EXTRAS {
            input["revised"]["adapter_extras"][format!("adapter.k{index}")] = json!(1);
        }
        let output = compute_request_revision_delta(&input);
        assert_eq!(output["errors"][0]["code"], "INVALID_FIELD");
    }
}
