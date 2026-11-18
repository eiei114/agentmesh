//! Deterministic per-criterion request evidence coverage.
//!
//! The App only consumes a validated ordinal-only request summary and an
//! ordered inventory of bounded evidence references. It never reads evidence
//! values, files, trackers, or adapter state.

use crate::adapter_compact_helpers::canonical_json_bytes;
use agentmesh_evidence::sha256_prefixed;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const APP_VERSION: &str = "request-acceptance-evidence-coverage.v0";
const INPUT_SCHEMA_VERSION: &str = "request-acceptance-evidence-coverage-input.v0";
const OUTPUT_SCHEMA_VERSION: &str = "request-acceptance-evidence-coverage-compact.v0";
const SUMMARY_SCHEMA_VERSION: &str = "agentmesh-request-summary.v0";
const REQUEST_SCHEMA_VERSION: &str = "agentmesh-request.v0";
const EVIDENCE_SCHEMA_VERSION: &str = "agentmesh-evidence-reference.v0";
const COVERAGE_SCHEMA_VERSION: &str = "acceptance-evidence-coverage.v0";
const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_CRITERIA: usize = 128;
const MAX_EVIDENCE: usize = 128;
const MAX_FINDINGS: usize = 256;
const MAX_ALL_FINDINGS: usize = 16_768;
const MAX_SCOPE_BYTES: usize = 128;

const INPUT_INVALID: &str = "AGENTMESH_REQUEST_ACCEPTANCE_EVIDENCE_COVERAGE_INPUT_INVALID";
const INPUT_TOO_LARGE: &str = "AGENTMESH_REQUEST_ACCEPTANCE_EVIDENCE_COVERAGE_INPUT_TOO_LARGE";
const SCHEMA_VERSION_UNSUPPORTED: &str =
    "AGENTMESH_REQUEST_ACCEPTANCE_EVIDENCE_COVERAGE_SCHEMA_VERSION_UNSUPPORTED";
const CONTRACT_VERSION_UNSUPPORTED: &str =
    "AGENTMESH_REQUEST_ACCEPTANCE_EVIDENCE_COVERAGE_CONTRACT_VERSION_UNSUPPORTED";
const REQUEST_SCHEMA_VERSION_UNSUPPORTED: &str =
    "AGENTMESH_REQUEST_ACCEPTANCE_EVIDENCE_COVERAGE_REQUEST_SCHEMA_VERSION_UNSUPPORTED";
const REQUEST_SUMMARY_INVALID: &str =
    "AGENTMESH_REQUEST_ACCEPTANCE_EVIDENCE_COVERAGE_REQUEST_SUMMARY_INVALID";
const REQUEST_CRITERIA_REQUIRED: &str =
    "AGENTMESH_REQUEST_ACCEPTANCE_EVIDENCE_COVERAGE_REQUEST_CRITERIA_REQUIRED";

const CRITERION_NO_EVIDENCE: &str =
    "AGENTMESH_REQUEST_ACCEPTANCE_EVIDENCE_COVERAGE_CRITERION_NO_EVIDENCE";
const CRITERION_INDEX_OUT_OF_RANGE: &str =
    "AGENTMESH_REQUEST_ACCEPTANCE_EVIDENCE_COVERAGE_CRITERION_INDEX_OUT_OF_RANGE";
const DUPLICATE_EVIDENCE_ID: &str =
    "AGENTMESH_REQUEST_ACCEPTANCE_EVIDENCE_COVERAGE_DUPLICATE_EVIDENCE_ID";
const EVIDENCE_SCHEMA_UNSUPPORTED: &str =
    "AGENTMESH_REQUEST_ACCEPTANCE_EVIDENCE_COVERAGE_EVIDENCE_SCHEMA_VERSION_UNSUPPORTED";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Failure {
    code: &'static str,
    path: &'static str,
}

#[derive(Debug)]
struct EvidenceReference {
    schema_version: String,
    id: String,
    criterion_indices: Vec<usize>,
    inventory_index: usize,
}

/// Return a deterministic coverage envelope. Invalid input never carries a
/// partial coverage report.
pub fn coverage_report(value: &Value) -> Value {
    match analyze(value) {
        Ok(coverage) => json!({
            "schema_version": OUTPUT_SCHEMA_VERSION,
            "app_version": APP_VERSION,
            "valid": true,
            "coverage": coverage,
            "error_count": 0,
            "errors": [],
        }),
        Err(failure) => json!({
            "schema_version": OUTPUT_SCHEMA_VERSION,
            "app_version": APP_VERSION,
            "valid": false,
            "coverage": null,
            "error_count": 1,
            "errors": [{"code": failure.code, "path": failure.path}],
        }),
    }
}

fn analyze(value: &Value) -> Result<Value, Failure> {
    if canonical_json_bytes(value).len() > MAX_INPUT_BYTES {
        return Err(failure(INPUT_TOO_LARGE, "$"));
    }

    let input = value
        .as_object()
        .ok_or_else(|| failure(INPUT_INVALID, "$"))?;
    exact_keys(
        input,
        &[
            "schema_version",
            "contract_version",
            "request_summary",
            "evidence_inventory",
        ],
        "$",
    )?;
    if string(input, "schema_version", "$.schema_version")? != INPUT_SCHEMA_VERSION {
        return Err(failure(SCHEMA_VERSION_UNSUPPORTED, "$.schema_version"));
    }
    if string(input, "contract_version", "$.contract_version")? != APP_VERSION {
        return Err(failure(CONTRACT_VERSION_UNSUPPORTED, "$.contract_version"));
    }

    let summary = object(input.get("request_summary"), "$.request_summary")?;
    exact_keys(
        summary,
        &[
            "schema_version",
            "request_schema_version",
            "valid",
            "scope",
            "request_digest",
            "acceptance_criteria",
        ],
        "$.request_summary",
    )?;
    if string(
        summary,
        "schema_version",
        "$.request_summary.schema_version",
    )? != SUMMARY_SCHEMA_VERSION
    {
        return Err(failure(
            REQUEST_SCHEMA_VERSION_UNSUPPORTED,
            "$.request_summary.schema_version",
        ));
    }
    if string(
        summary,
        "request_schema_version",
        "$.request_summary.request_schema_version",
    )? != REQUEST_SCHEMA_VERSION
    {
        return Err(failure(
            REQUEST_SCHEMA_VERSION_UNSUPPORTED,
            "$.request_summary.request_schema_version",
        ));
    }
    if summary.get("valid").and_then(Value::as_bool) != Some(true) {
        return Err(failure(REQUEST_SUMMARY_INVALID, "$.request_summary.valid"));
    }
    let scope = string(summary, "scope", "$.request_summary.scope")?;
    if !valid_scope(scope) {
        return Err(failure(REQUEST_SUMMARY_INVALID, "$.request_summary.scope"));
    }
    let request_digest = string(
        summary,
        "request_digest",
        "$.request_summary.request_digest",
    )?;
    if !valid_sha256(request_digest) {
        return Err(failure(
            REQUEST_SUMMARY_INVALID,
            "$.request_summary.request_digest",
        ));
    }

    let criteria = array(
        summary.get("acceptance_criteria"),
        "$.request_summary.acceptance_criteria",
    )?;
    if criteria.is_empty() {
        return Err(failure(
            REQUEST_CRITERIA_REQUIRED,
            "$.request_summary.acceptance_criteria",
        ));
    }
    if criteria.len() > MAX_CRITERIA {
        return Err(failure(
            REQUEST_SUMMARY_INVALID,
            "$.request_summary.acceptance_criteria",
        ));
    }
    for (index, criterion) in criteria.iter().enumerate() {
        let item = criterion.as_object().ok_or_else(|| {
            failure(
                REQUEST_SUMMARY_INVALID,
                "$.request_summary.acceptance_criteria",
            )
        })?;
        exact_keys(item, &["ordinal"], "$.request_summary.acceptance_criteria")?;
        let ordinal = item.get("ordinal").and_then(Value::as_u64).ok_or_else(|| {
            failure(
                REQUEST_SUMMARY_INVALID,
                "$.request_summary.acceptance_criteria",
            )
        })?;
        if ordinal != (index + 1) as u64 {
            return Err(failure(
                REQUEST_SUMMARY_INVALID,
                "$.request_summary.acceptance_criteria",
            ));
        }
    }

    let inventory = array(input.get("evidence_inventory"), "$.evidence_inventory")?;
    if inventory.len() > MAX_EVIDENCE {
        return Err(failure(INPUT_INVALID, "$.evidence_inventory"));
    }
    let mut evidence = Vec::with_capacity(inventory.len());
    let mut id_counts = BTreeMap::<String, usize>::new();
    for (index, value) in inventory.iter().enumerate() {
        let item = value
            .as_object()
            .ok_or_else(|| failure(INPUT_INVALID, "$.evidence_inventory"))?;
        exact_keys(
            item,
            &[
                "schema_version",
                "id",
                "digest",
                "evidence_class",
                "criterion_indices",
            ],
            "$.evidence_inventory",
        )?;
        let schema_version = string(
            item,
            "schema_version",
            "$.evidence_inventory.schema_version",
        )?;
        let id = string(item, "id", "$.evidence_inventory.id")?;
        if !valid_evidence_id(id)
            || !valid_sha256(string(item, "digest", "$.evidence_inventory.digest")?)
            || !valid_evidence_class(string(
                item,
                "evidence_class",
                "$.evidence_inventory.evidence_class",
            )?)
        {
            return Err(failure(INPUT_INVALID, "$.evidence_inventory"));
        }
        let indices = array(
            item.get("criterion_indices"),
            "$.evidence_inventory.criterion_indices",
        )?;
        if indices.len() > MAX_CRITERIA {
            return Err(failure(
                INPUT_INVALID,
                "$.evidence_inventory.criterion_indices",
            ));
        }
        let mut criterion_indices = Vec::with_capacity(indices.len());
        let mut seen_indices = BTreeSet::new();
        for criterion_index in indices {
            let ordinal = criterion_index
                .as_u64()
                .filter(|ordinal| *ordinal > 0 && *ordinal <= MAX_CRITERIA as u64)
                .ok_or_else(|| failure(INPUT_INVALID, "$.evidence_inventory.criterion_indices"))?
                as usize;
            if !seen_indices.insert(ordinal) {
                return Err(failure(
                    INPUT_INVALID,
                    "$.evidence_inventory.criterion_indices",
                ));
            }
            criterion_indices.push(ordinal);
        }
        *id_counts.entry(id.to_string()).or_default() += 1;
        evidence.push(EvidenceReference {
            schema_version: schema_version.to_string(),
            id: id.to_string(),
            criterion_indices,
            inventory_index: index,
        });
    }

    let inventory_digest = sha256_prefixed(&canonical_json_bytes(&Value::Array(inventory.clone())));
    let mut supports = vec![Vec::<String>::new(); criteria.len()];
    let mut findings = Vec::new();

    for reference in &evidence {
        let index = reference.inventory_index;
        let item_path = format!("$.evidence_inventory[{index}]");
        let supported_schema = reference.schema_version == EVIDENCE_SCHEMA_VERSION;
        let duplicate_id = id_counts.get(&reference.id).copied().unwrap_or_default() > 1;

        if !supported_schema {
            findings.push(finding(
                EVIDENCE_SCHEMA_UNSUPPORTED,
                format!("{item_path}.schema_version"),
            ));
        }
        if duplicate_id {
            findings.push(finding(DUPLICATE_EVIDENCE_ID, format!("{item_path}.id")));
        }

        for (reference_index, ordinal) in reference.criterion_indices.iter().enumerate() {
            if *ordinal > criteria.len() {
                findings.push(finding(
                    CRITERION_INDEX_OUT_OF_RANGE,
                    format!("{item_path}.criterion_indices[{reference_index}]"),
                ));
            } else if supported_schema && !duplicate_id {
                supports[*ordinal - 1].push(reference.id.clone());
            }
        }
    }

    let mut criterion_results = Vec::with_capacity(criteria.len());
    let mut uncovered = Vec::new();
    for (index, evidence_ids) in supports.into_iter().enumerate() {
        let ordinal = index + 1;
        let covered = !evidence_ids.is_empty();
        if !covered {
            uncovered.push(ordinal);
            findings.push(finding(
                CRITERION_NO_EVIDENCE,
                format!("$.request_summary.acceptance_criteria[{index}]"),
            ));
        }
        criterion_results.push(json!({
            "ordinal": ordinal,
            "verdict": if covered { "covered" } else { "uncovered" },
            "evidence_ids": evidence_ids,
        }));
    }

    if findings.len() > MAX_ALL_FINDINGS {
        return Err(failure(INPUT_INVALID, "$.evidence_inventory"));
    }
    let omitted_finding_count = findings.len().saturating_sub(MAX_FINDINGS);
    findings.truncate(MAX_FINDINGS);
    let verdict = if uncovered.is_empty() {
        "covered"
    } else {
        "uncovered"
    };
    Ok(json!({
        "schema_version": COVERAGE_SCHEMA_VERSION,
        "contract_version": COVERAGE_SCHEMA_VERSION,
        "scope": scope,
        "request_digest": request_digest,
        "verdict": verdict,
        "uncovered_criterion_ordinals": uncovered,
        "reason_code": if verdict == "covered" { "none" } else { "acceptance_criteria_uncovered" },
        "evidence_inventory_sha256": inventory_digest,
        "criteria": criterion_results,
        "finding_count": findings.len(),
        "findings": findings,
        "findings_truncated": omitted_finding_count > 0,
        "omitted_finding_count": omitted_finding_count,
    }))
}

fn failure(code: &'static str, path: &'static str) -> Failure {
    Failure { code, path }
}

fn finding(code: &'static str, path: String) -> Value {
    json!({"code": code, "path": path})
}

fn exact_keys(
    object: &Map<String, Value>,
    required: &[&str],
    path: &'static str,
) -> Result<(), Failure> {
    if object.len() == required.len() && required.iter().all(|key| object.contains_key(*key)) {
        Ok(())
    } else {
        Err(failure(INPUT_INVALID, path))
    }
}

fn object<'a>(
    value: Option<&'a Value>,
    path: &'static str,
) -> Result<&'a Map<String, Value>, Failure> {
    value
        .and_then(Value::as_object)
        .ok_or_else(|| failure(INPUT_INVALID, path))
}

fn array<'a>(value: Option<&'a Value>, path: &'static str) -> Result<&'a Vec<Value>, Failure> {
    value
        .and_then(Value::as_array)
        .ok_or_else(|| failure(INPUT_INVALID, path))
}

fn string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    path: &'static str,
) -> Result<&'a str, Failure> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| failure(INPUT_INVALID, path))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_scope(value: &str) -> bool {
    value.len() > 10
        && value.len() <= MAX_SCOPE_BYTES
        && value.starts_with("agentmesh:")
        && value[10..].bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b':' | b'.' | b'_' | b'-')
        })
}

fn valid_evidence_id(value: &str) -> bool {
    value.len() > 3
        && value.len() <= 63
        && value.starts_with("ev_")
        && value[3..]
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && value[3..].bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn valid_evidence_class(value: &str) -> bool {
    matches!(
        value,
        "test" | "review" | "build" | "validation" | "artifact" | "observation" | "other"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::path::PathBuf;

    const FIXTURES: &[(&str, &str)] = &[
        (
            "request_acceptance_evidence_coverage_complete_input.json",
            "request_acceptance_evidence_coverage_complete_expected.json",
        ),
        (
            "request_acceptance_evidence_coverage_partial_input.json",
            "request_acceptance_evidence_coverage_partial_expected.json",
        ),
        (
            "request_acceptance_evidence_coverage_invalid_references_input.json",
            "request_acceptance_evidence_coverage_invalid_references_expected.json",
        ),
        (
            "request_acceptance_evidence_coverage_malformed_input.json",
            "request_acceptance_evidence_coverage_malformed_expected.json",
        ),
        (
            "request_acceptance_evidence_coverage_unsupported_evidence_input.json",
            "request_acceptance_evidence_coverage_unsupported_evidence_expected.json",
        ),
        (
            "request_acceptance_evidence_coverage_unsupported_contract_input.json",
            "request_acceptance_evidence_coverage_unsupported_contract_expected.json",
        ),
    ];

    fn fixture(name: &str) -> Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join(name);
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn recorded_fixtures_match_and_repeat_byte_identically() {
        for (input_name, expected_name) in FIXTURES {
            let input = fixture(input_name);
            let expected = fixture(expected_name);
            let first = coverage_report(&input);
            let second = coverage_report(&input);
            assert_eq!(first, expected, "fixture {input_name}");
            assert_eq!(
                serde_json::to_vec(&first).unwrap(),
                serde_json::to_vec(&second).unwrap(),
                "fixture {input_name} changed across repeated runs"
            );
        }
    }

    #[test]
    fn criterion_order_and_inventory_order_are_preserved_without_echoing_values() {
        let input = fixture("request_acceptance_evidence_coverage_complete_input.json");
        let output = coverage_report(&input);
        let report = output["coverage"].as_object().unwrap();
        assert_eq!(report["criteria"][0]["ordinal"], 1);
        assert_eq!(report["criteria"][0]["evidence_ids"][0], "ev_test-run-1");
        let serialized = serde_json::to_string(&output).unwrap();
        assert!(!serialized
            .contains("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert!(!serialized
            .contains("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
    }

    #[test]
    fn unsupported_contract_and_oversized_inputs_fail_without_partial_reports() {
        let mut unsupported = fixture("request_acceptance_evidence_coverage_complete_input.json");
        unsupported["contract_version"] = json!("request-acceptance-evidence-coverage.v9");
        assert_failure(&coverage_report(&unsupported), CONTRACT_VERSION_UNSUPPORTED);

        let mut unsupported_schema =
            fixture("request_acceptance_evidence_coverage_complete_input.json");
        unsupported_schema["schema_version"] =
            json!("request-acceptance-evidence-coverage-input.v9");
        assert_failure(
            &coverage_report(&unsupported_schema),
            SCHEMA_VERSION_UNSUPPORTED,
        );

        let mut unsupported_request =
            fixture("request_acceptance_evidence_coverage_complete_input.json");
        unsupported_request["request_summary"]["request_schema_version"] =
            json!("agentmesh-request.v9");
        assert_failure(
            &coverage_report(&unsupported_request),
            REQUEST_SCHEMA_VERSION_UNSUPPORTED,
        );

        let mut oversized = fixture("request_acceptance_evidence_coverage_complete_input.json");
        oversized["evidence_inventory"][0]["id"] = json!("x".repeat(64));
        let entry = oversized["evidence_inventory"][0].clone();
        oversized["evidence_inventory"]
            .as_array_mut()
            .unwrap()
            .extend((0..MAX_EVIDENCE).map(|_| entry.clone()));
        oversized["padding"] = json!("x".repeat(MAX_INPUT_BYTES));
        let output = coverage_report(&oversized);
        assert_failure(&output, INPUT_TOO_LARGE);
    }

    #[test]
    fn oversized_inventory_and_missing_required_request_field_fail_closed() {
        let mut oversized_inventory =
            fixture("request_acceptance_evidence_coverage_complete_input.json");
        let entry = oversized_inventory["evidence_inventory"][0].clone();
        oversized_inventory["evidence_inventory"]
            .as_array_mut()
            .unwrap()
            .extend((0..MAX_EVIDENCE).map(|_| entry.clone()));
        assert_failure(&coverage_report(&oversized_inventory), INPUT_INVALID);

        let mut missing_digest =
            fixture("request_acceptance_evidence_coverage_complete_input.json");
        missing_digest["request_summary"]
            .as_object_mut()
            .unwrap()
            .remove("request_digest");
        assert_failure(&coverage_report(&missing_digest), INPUT_INVALID);
    }

    #[test]
    fn finding_count_is_bounded_and_truncation_is_explicit() {
        let mut input = fixture("request_acceptance_evidence_coverage_complete_input.json");
        input["request_summary"]["acceptance_criteria"] = json!([{"ordinal": 1}]);
        let entry = json!({
            "schema_version": EVIDENCE_SCHEMA_VERSION,
            "id": "ev_range-1",
            "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "evidence_class": "test",
            "criterion_indices": (2..=9).collect::<Vec<_>>(),
        });
        input["evidence_inventory"] = json!(vec![entry; MAX_EVIDENCE]);

        let report = coverage_report(&input)["coverage"].clone();
        assert_eq!(report["finding_count"], MAX_FINDINGS);
        assert_eq!(report["findings"].as_array().unwrap().len(), MAX_FINDINGS);
        assert_eq!(report["findings_truncated"], true);
        assert_eq!(report["omitted_finding_count"], 897);
        assert_eq!(report["criteria"][0]["verdict"], "uncovered");
        assert_eq!(report["verdict"], "uncovered");
    }

    #[test]
    fn zero_criteria_and_invalid_request_summaries_fail_closed() {
        let mut no_criteria = fixture("request_acceptance_evidence_coverage_complete_input.json");
        no_criteria["request_summary"]["acceptance_criteria"] = json!([]);
        assert_failure(&coverage_report(&no_criteria), REQUEST_CRITERIA_REQUIRED);

        let mut invalid_summary =
            fixture("request_acceptance_evidence_coverage_complete_input.json");
        invalid_summary["request_summary"]["valid"] = json!(false);
        assert_failure(&coverage_report(&invalid_summary), REQUEST_SUMMARY_INVALID);
    }

    fn assert_failure(output: &Value, code: &str) {
        assert_eq!(output["valid"], false);
        assert_eq!(output["coverage"], Value::Null);
        assert_eq!(output["error_count"], 1);
        assert_eq!(output["errors"].as_array().unwrap().len(), 1);
        assert_eq!(output["errors"][0]["code"], code);
    }
}
