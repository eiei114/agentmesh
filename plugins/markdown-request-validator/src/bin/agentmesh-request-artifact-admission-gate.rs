//! One-shot, tool-neutral request artifact admission gate.
//!
//! This App joins already-produced neutrality, evidence-coverage, and revision
//! lineage results. It does not re-run any gate or perform materialization.

use agentmesh_evidence::sha256_prefixed;
use agentmesh_fixture_support::run_fixture;
use agentmesh_proto::rpc::RunResult;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::process::ExitCode;

const APP_VERSION: &str = "request-artifact-admission.v0";
const INPUT_SCHEMA_VERSION: &str = "request-artifact-admission-input.v0";
const OUTPUT_SCHEMA_VERSION: &str = "request-artifact-admission-compact.v0";
const REQUEST_SUMMARY_SCHEMA_VERSION: &str = "request-artifact-admission-summary.v0";
const SOURCE_NEUTRALITY_SCHEMA_VERSION: &str = "source-neutrality-result.v0";
const SOURCE_NEUTRALITY_CONTRACT_VERSION: &str = "source-neutrality.v0";
const EVIDENCE_COVERAGE_SCHEMA_VERSION: &str = "acceptance-evidence-coverage.v0";
const EVIDENCE_COVERAGE_CONTRACT_VERSION: &str = "acceptance-evidence-coverage.v0";
const REVISION_LINEAGE_SCHEMA_VERSION: &str = "revision-lineage-resolution.v0";
const REVISION_LINEAGE_CONTRACT_VERSION: &str = "revision-lineage.v0";
const MAX_INPUT_BYTES: usize = 32 * 1024;
const MAX_SCOPE_BYTES: usize = 128;
const MAX_UNCOVERED_CRITERIA: usize = 128;

const INPUT_INVALID: &str = "AGENTMESH_REQUEST_ARTIFACT_ADMISSION_INPUT_INVALID";
const INPUT_TOO_LARGE: &str = "AGENTMESH_REQUEST_ARTIFACT_ADMISSION_INPUT_TOO_LARGE";
const CONTRACT_VERSION_UNSUPPORTED: &str =
    "AGENTMESH_REQUEST_ARTIFACT_ADMISSION_CONTRACT_VERSION_UNSUPPORTED";
const SUBGATE_VERSION_UNSUPPORTED: &str =
    "AGENTMESH_REQUEST_ARTIFACT_ADMISSION_SUBGATE_VERSION_UNSUPPORTED";
const CROSS_SCOPE_INPUT: &str = "AGENTMESH_REQUEST_ARTIFACT_ADMISSION_CROSS_SCOPE_INPUT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    InputInvalid,
    InputTooLarge,
    ContractVersionUnsupported,
    SubgateVersionUnsupported,
    CrossScopeInput,
}

impl Failure {
    const fn code(self) -> &'static str {
        match self {
            Self::InputInvalid => INPUT_INVALID,
            Self::InputTooLarge => INPUT_TOO_LARGE,
            Self::ContractVersionUnsupported => CONTRACT_VERSION_UNSUPPORTED,
            Self::SubgateVersionUnsupported => SUBGATE_VERSION_UNSUPPORTED,
            Self::CrossScopeInput => CROSS_SCOPE_INPUT,
        }
    }
}

fn main() -> ExitCode {
    run_fixture(
        APP_VERSION,
        &[
            "request_artifact_admission_gate",
            "bounded_subgate_digests",
            "no_materialization_side_effects",
        ],
        Box::new(|params| {
            Ok(RunResult {
                payload: admit_request_artifact(&params.input),
            })
        }),
    )
}

fn admit_request_artifact(value: &Value) -> Value {
    match validate_and_admit(value) {
        Ok(admission) => json!({
            "schema_version": OUTPUT_SCHEMA_VERSION,
            "admission_version": APP_VERSION,
            "valid": true,
            "admission": admission,
            "error_count": 0,
            "errors": [],
        }),
        Err(failure) => json!({
            "schema_version": OUTPUT_SCHEMA_VERSION,
            "admission_version": APP_VERSION,
            "valid": false,
            "admission": null,
            "error_count": 1,
            "errors": [{"code": failure.code()}],
        }),
    }
}

fn validate_and_admit(value: &Value) -> Result<Value, Failure> {
    let encoded = serde_json::to_vec(value).map_err(|_| Failure::InputInvalid)?;
    if encoded.len() > MAX_INPUT_BYTES {
        return Err(Failure::InputTooLarge);
    }

    let input = object(value)?;
    exact_keys(
        input,
        &[
            "schema_version",
            "contract_version",
            "request_summary",
            "source_neutrality",
            "acceptance_evidence",
            "revision_lineage",
        ],
    )?;
    match string(input, "schema_version")? {
        INPUT_SCHEMA_VERSION => {}
        _ => return Err(Failure::ContractVersionUnsupported),
    }
    match string(input, "contract_version")? {
        APP_VERSION => {}
        _ => return Err(Failure::ContractVersionUnsupported),
    }

    let summary = object(field(input, "request_summary")?)?;
    exact_keys(
        summary,
        &[
            "schema_version",
            "scope",
            "request_digest",
            "revision_identity",
        ],
    )?;
    match string(summary, "schema_version")? {
        REQUEST_SUMMARY_SCHEMA_VERSION => {}
        _ => return Err(Failure::ContractVersionUnsupported),
    }
    let scope = string(summary, "scope")?;
    if !valid_scope(scope) {
        return Err(Failure::InputInvalid);
    }
    let request_digest = string(summary, "request_digest")?;
    if !valid_sha256(request_digest) {
        return Err(Failure::InputInvalid);
    }
    let revision_identity = string(summary, "revision_identity")?;
    if !valid_sha256(revision_identity) {
        return Err(Failure::InputInvalid);
    }

    let neutrality = object(field(input, "source_neutrality")?)?;
    exact_keys(
        neutrality,
        &[
            "schema_version",
            "contract_version",
            "scope",
            "request_digest",
            "verdict",
            "reason_code",
        ],
    )?;
    require_subgate_versions(
        neutrality,
        SOURCE_NEUTRALITY_SCHEMA_VERSION,
        SOURCE_NEUTRALITY_CONTRACT_VERSION,
    )?;

    let coverage = object(field(input, "acceptance_evidence")?)?;
    exact_keys(
        coverage,
        &[
            "schema_version",
            "contract_version",
            "scope",
            "request_digest",
            "verdict",
            "uncovered_criterion_ordinals",
            "reason_code",
        ],
    )?;
    require_subgate_versions(
        coverage,
        EVIDENCE_COVERAGE_SCHEMA_VERSION,
        EVIDENCE_COVERAGE_CONTRACT_VERSION,
    )?;

    let lineage = object(field(input, "revision_lineage")?)?;
    exact_keys(
        lineage,
        &[
            "schema_version",
            "contract_version",
            "scope",
            "request_digest",
            "current_revision_identity",
        ],
    )?;
    require_subgate_versions(
        lineage,
        REVISION_LINEAGE_SCHEMA_VERSION,
        REVISION_LINEAGE_CONTRACT_VERSION,
    )?;

    for subgate in [neutrality, coverage, lineage] {
        if string(subgate, "scope")? != scope
            || string(subgate, "request_digest")? != request_digest
        {
            return Err(Failure::CrossScopeInput);
        }
    }

    let neutrality_verdict = string(neutrality, "verdict")?;
    let neutrality_reason = string(neutrality, "reason_code")?;
    match (neutrality_verdict, neutrality_reason) {
        ("neutral", "none") | ("non_neutral", "source_not_neutral") => {}
        ("neutral" | "non_neutral", _) => return Err(Failure::InputInvalid),
        _ => return Err(Failure::InputInvalid),
    }

    let coverage_verdict = string(coverage, "verdict")?;
    let coverage_reason = string(coverage, "reason_code")?;
    let uncovered = array(coverage, "uncovered_criterion_ordinals")?;
    if uncovered.len() > MAX_UNCOVERED_CRITERIA {
        return Err(Failure::InputInvalid);
    }
    let mut ordinals = BTreeSet::new();
    for ordinal in uncovered {
        let value = ordinal.as_u64().ok_or(Failure::InputInvalid)?;
        if value == 0 || value > MAX_UNCOVERED_CRITERIA as u64 || !ordinals.insert(value) {
            return Err(Failure::InputInvalid);
        }
    }
    let has_uncovered = !uncovered.is_empty();
    match (coverage_verdict, has_uncovered, coverage_reason) {
        ("covered", false, "none") | ("uncovered", true, "acceptance_criteria_uncovered") => {}
        ("covered" | "uncovered", _, _) => return Err(Failure::InputInvalid),
        _ => return Err(Failure::InputInvalid),
    }

    let current_revision_identity = string(lineage, "current_revision_identity")?;
    if !valid_sha256(current_revision_identity) {
        return Err(Failure::InputInvalid);
    }
    let lineage_is_current = current_revision_identity == revision_identity;

    let gates = vec![
        gate_result(
            1,
            "source_neutrality",
            neutrality_verdict,
            if neutrality_verdict == "neutral" {
                Vec::new()
            } else {
                vec![neutrality_reason]
            },
            neutrality,
        ),
        gate_result(
            2,
            "acceptance_evidence_coverage",
            coverage_verdict,
            if coverage_verdict == "covered" {
                Vec::new()
            } else {
                vec![coverage_reason]
            },
            coverage,
        ),
        gate_result(
            3,
            "revision_lineage",
            if lineage_is_current {
                "current"
            } else {
                "stale"
            },
            if lineage_is_current {
                Vec::new()
            } else {
                vec!["revision_not_current"]
            },
            lineage,
        ),
    ];
    let admitted =
        neutrality_verdict == "neutral" && coverage_verdict == "covered" && lineage_is_current;

    Ok(json!({
        "decision": if admitted { "admit" } else { "defer" },
        "contract_version": APP_VERSION,
        "revision_identity": revision_identity,
        "gates": gates,
    }))
}

fn require_subgate_versions(
    value: &Map<String, Value>,
    schema_version: &str,
    contract_version: &str,
) -> Result<(), Failure> {
    if string(value, "schema_version")? != schema_version
        || string(value, "contract_version")? != contract_version
    {
        return Err(Failure::SubgateVersionUnsupported);
    }
    Ok(())
}

fn gate_result(
    ordinal: usize,
    name: &str,
    verdict: &str,
    reason_codes: Vec<&str>,
    input: &Map<String, Value>,
) -> Value {
    let bytes = canonical_json_bytes(&Value::Object(input.clone()));
    json!({
        "ordinal": ordinal,
        "name": name,
        "verdict": verdict,
        "reason_codes": reason_codes,
        "input_sha256": sha256_prefixed(&bytes),
    })
}

fn canonical_json_bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(&sort_json_keys(value.clone())).expect("JSON value is serializable")
}

fn sort_json_keys(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries: Vec<_> = object.into_iter().collect();
            entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
            let mut sorted = Map::new();
            for (key, nested) in entries {
                sorted.insert(key, sort_json_keys(nested));
            }
            Value::Object(sorted)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(sort_json_keys).collect()),
        scalar => scalar,
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, Failure> {
    value.as_object().ok_or(Failure::InputInvalid)
}

fn exact_keys(object: &Map<String, Value>, required: &[&str]) -> Result<(), Failure> {
    if object.len() == required.len() && required.iter().all(|key| object.contains_key(*key)) {
        Ok(())
    } else {
        Err(Failure::InputInvalid)
    }
}

fn field<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a Value, Failure> {
    object.get(key).ok_or(Failure::InputInvalid)
}

fn string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, Failure> {
    field(object, key)?.as_str().ok_or(Failure::InputInvalid)
}

fn array<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a Vec<Value>, Failure> {
    field(object, key)?.as_array().ok_or(Failure::InputInvalid)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_scope(value: &str) -> bool {
    value.len() <= MAX_SCOPE_BYTES
        && value.starts_with("agentmesh:")
        && value[10..].bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b':' | b'.' | b'_' | b'-')
        })
        && value.len() > 10
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const FIXTURES: &[(&str, &str)] = &[
        (
            "request_artifact_admission_admit_input.json",
            "request_artifact_admission_admit_expected.json",
        ),
        (
            "request_artifact_admission_uncovered_input.json",
            "request_artifact_admission_uncovered_expected.json",
        ),
        (
            "request_artifact_admission_stale_input.json",
            "request_artifact_admission_stale_expected.json",
        ),
        (
            "request_artifact_admission_non_neutral_input.json",
            "request_artifact_admission_non_neutral_expected.json",
        ),
        (
            "request_artifact_admission_malformed_input.json",
            "request_artifact_admission_malformed_expected.json",
        ),
        (
            "request_artifact_admission_cross_scope_input.json",
            "request_artifact_admission_cross_scope_expected.json",
        ),
        (
            "request_artifact_admission_unsupported_version_input.json",
            "request_artifact_admission_unsupported_version_expected.json",
        ),
        (
            "request_artifact_admission_contract_version_input.json",
            "request_artifact_admission_contract_version_expected.json",
        ),
    ];

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join(name)
    }

    #[test]
    fn recorded_fixtures_match_expected_payloads_and_repeat_byte_identically() {
        for (input_name, expected_name) in FIXTURES {
            let input: Value =
                serde_json::from_slice(&std::fs::read(fixture_path(input_name)).unwrap()).unwrap();
            let expected: Value =
                serde_json::from_slice(&std::fs::read(fixture_path(expected_name)).unwrap())
                    .unwrap();
            let first = admit_request_artifact(&input);
            let second = admit_request_artifact(&input);
            assert_eq!(first, expected, "fixture {input_name}");
            assert_eq!(
                serde_json::to_vec(&first).unwrap(),
                serde_json::to_vec(&second).unwrap(),
                "fixture {input_name} changed across repeated runs"
            );
        }
    }

    #[test]
    fn oversized_input_returns_only_a_normalized_failure() {
        let mut input: Value = serde_json::from_slice(
            &std::fs::read(fixture_path("request_artifact_admission_admit_input.json")).unwrap(),
        )
        .unwrap();
        input["padding"] = Value::String("x".repeat(MAX_INPUT_BYTES + 1));
        let output = admit_request_artifact(&input);
        assert_eq!(output["valid"], false);
        assert_eq!(output["admission"], Value::Null);
        assert_eq!(output["errors"][0]["code"], INPUT_TOO_LARGE);
    }

    #[test]
    fn valid_record_contains_no_request_scope_or_digest() {
        let input: Value = serde_json::from_slice(
            &std::fs::read(fixture_path("request_artifact_admission_admit_input.json")).unwrap(),
        )
        .unwrap();
        let output = serde_json::to_string(&admit_request_artifact(&input)).unwrap();
        assert!(!output.contains("agentmesh:app:request-artifact-admission-gate"));
        assert!(!output.contains("request_digest"));
        assert!(!output.contains("uncovered_criterion_ordinals"));
        assert!(output
            .contains("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
    }

    #[test]
    fn subgate_hash_uses_canonical_nested_key_order() {
        let first: Value = serde_json::from_str(r#"{"z":{"b":2,"a":1},"a":0}"#).unwrap();
        let second: Value = serde_json::from_str(r#"{"a":0,"z":{"a":1,"b":2}}"#).unwrap();
        assert_eq!(canonical_json_bytes(&first), canonical_json_bytes(&second));
    }
}
