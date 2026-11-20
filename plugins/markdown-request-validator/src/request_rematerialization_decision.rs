//! Deterministic request re-materialization decision App.
//!
//! This module joins already-produced admission, materialization receipt, and
//! revision-lineage verdicts. It never opens request or artifact sources and
//! never recomputes any of those upstream verdicts.

use crate::adapter_compact_helpers::canonical_json_bytes;
use agentmesh_evidence::sha256_prefixed;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

/// Contract version accepted by this App and copied into decision output.
pub const DECISION_CONTRACT_VERSION: &str = "agentmesh-request-rematerialization-decision.v0";
const INPUT_SCHEMA_VERSION: &str = "request-rematerialization-decision-input.v0";
const OUTPUT_SCHEMA_VERSION: &str = "request-rematerialization-decision-compact.v0";
const ADMISSION_RECORD_SCHEMA_VERSION: &str = "request-artifact-admission-record.v0";
const ADMISSION_CONTRACT_VERSION: &str = "request-artifact-admission.v0";
const RECEIPT_RECORD_SCHEMA_VERSION: &str = "request-materialization-receipt-record.v0";
const RECEIPT_CONTRACT_VERSION: &str = "agentmesh-request-materialization-receipt.v0";
const LINEAGE_SCHEMA_VERSION: &str = "revision-lineage-resolution.v0";
const LINEAGE_CONTRACT_VERSION: &str = "revision-lineage.v0";
const MAX_INPUT_BYTES: usize = 32 * 1024;
const MAX_RECEIPTS: usize = 16;
const MAX_SCOPE_BYTES: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    InputTooLarge,
    MalformedInput,
    UnsupportedSchemaVersion,
    UnsupportedContractVersion,
    RequiredFieldMissing,
    InvalidField,
    CrossScopeInput,
    EmptyReceiptList,
    ReceiptListTooLarge,
    DuplicateReceiptOrdinal,
    ReceiptOrderInvalid,
    DuplicateTargetReceipt,
    TargetReceiptMissing,
    RevisionBindingMismatch,
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
            Self::EmptyReceiptList => "EMPTY_RECEIPT_LIST",
            Self::ReceiptListTooLarge => "RECEIPT_LIST_TOO_LARGE",
            Self::DuplicateReceiptOrdinal => "DUPLICATE_RECEIPT_ORDINAL",
            Self::ReceiptOrderInvalid => "RECEIPT_ORDER_INVALID",
            Self::DuplicateTargetReceipt => "DUPLICATE_TARGET_RECEIPT",
            Self::TargetReceiptMissing => "TARGET_RECEIPT_MISSING",
            Self::RevisionBindingMismatch => "REVISION_BINDING_MISMATCH",
        }
    }
}

/// Decide whether an adapter should regenerate, skip, or hold a request
/// artifact using only the supplied, bounded upstream records.
pub fn decide_request_rematerialization(value: &Value) -> Value {
    let supplied_contract = value
        .get("contract_version")
        .and_then(Value::as_str)
        .filter(|version| version.len() <= 128);
    match validate_and_decide(value) {
        Ok(decision) => decision,
        Err(failure) => failure_output(failure, supplied_contract),
    }
}

fn validate_and_decide(value: &Value) -> Result<Value, Failure> {
    if canonical_json_bytes(value).len() > MAX_INPUT_BYTES {
        return Err(Failure::InputTooLarge);
    }

    let root = object(value)?;
    exact_keys(
        root,
        &[
            "schema_version",
            "contract_version",
            "admission_record",
            "receipts",
            "revision_lineage",
        ],
    )?;
    if string(root, "schema_version")? != INPUT_SCHEMA_VERSION {
        return Err(Failure::UnsupportedSchemaVersion);
    }
    let contract_version = string(root, "contract_version")?;
    if contract_version != DECISION_CONTRACT_VERSION {
        return Err(Failure::UnsupportedContractVersion);
    }

    let admission = object(field(root, "admission_record")?)?;
    exact_keys(
        admission,
        &[
            "schema_version",
            "contract_version",
            "scope_key",
            "source_digest",
            "revision_identity",
            "verdict",
        ],
    )?;
    if string(admission, "schema_version")? != ADMISSION_RECORD_SCHEMA_VERSION {
        return Err(Failure::UnsupportedSchemaVersion);
    }
    if string(admission, "contract_version")? != ADMISSION_CONTRACT_VERSION {
        return Err(Failure::UnsupportedContractVersion);
    }
    let scope = string(admission, "scope_key")?;
    let source_digest = string(admission, "source_digest")?;
    let revision_identity = string(admission, "revision_identity")?;
    let admission_verdict = string(admission, "verdict")?;
    if !valid_scope(scope)
        || !valid_sha256(source_digest)
        || !valid_sha256(revision_identity)
        || !matches!(admission_verdict, "admit" | "defer")
    {
        return Err(Failure::InvalidField);
    }

    let receipts = array(root, "receipts")?;
    if receipts.is_empty() {
        return Err(Failure::EmptyReceiptList);
    }
    if receipts.len() > MAX_RECEIPTS {
        return Err(Failure::ReceiptListTooLarge);
    }

    let mut ordinals = BTreeSet::new();
    let mut target_receipt = None;
    for (index, receipt_value) in receipts.iter().enumerate() {
        let receipt = object(receipt_value)?;
        exact_keys(
            receipt,
            &[
                "schema_version",
                "contract_version",
                "ordinal",
                "scope_key",
                "revision_identity",
                "source_digest",
                "verdict",
                "drift_class",
            ],
        )?;
        if string(receipt, "schema_version")? != RECEIPT_RECORD_SCHEMA_VERSION {
            return Err(Failure::UnsupportedSchemaVersion);
        }
        if string(receipt, "contract_version")? != RECEIPT_CONTRACT_VERSION {
            return Err(Failure::UnsupportedContractVersion);
        }
        let ordinal = unsigned_integer(receipt, "ordinal")?;
        if ordinal == 0 || ordinal > MAX_RECEIPTS as u64 {
            return Err(Failure::InvalidField);
        }
        if !ordinals.insert(ordinal) {
            return Err(Failure::DuplicateReceiptOrdinal);
        }
        if ordinal != (index + 1) as u64 {
            return Err(Failure::ReceiptOrderInvalid);
        }
        let receipt_scope = string(receipt, "scope_key")?;
        let receipt_revision = string(receipt, "revision_identity")?;
        let receipt_source_digest = string(receipt, "source_digest")?;
        let receipt_verdict = string(receipt, "verdict")?;
        let drift_class = string(receipt, "drift_class")?;
        if !valid_scope(receipt_scope)
            || !valid_sha256(receipt_revision)
            || !valid_sha256(receipt_source_digest)
            || !matches!(receipt_verdict, "corresponded" | "drifted" | "unknown")
            || !matches!(
                drift_class,
                "none" | "canonical_fields" | "adapter_owned_fields" | "unknown"
            )
        {
            return Err(Failure::InvalidField);
        }
        match (receipt_verdict, drift_class) {
            ("corresponded", "none")
            | ("drifted", "canonical_fields" | "adapter_owned_fields" | "unknown")
            | ("unknown", "unknown") => {}
            _ => return Err(Failure::InvalidField),
        }
        if receipt_scope != scope {
            return Err(Failure::CrossScopeInput);
        }
        if receipt_revision == revision_identity {
            if target_receipt.is_some() {
                return Err(Failure::DuplicateTargetReceipt);
            }
            target_receipt = Some(index);
        }
    }
    let target_index = target_receipt.ok_or(Failure::TargetReceiptMissing)?;

    let lineage = object(field(root, "revision_lineage")?)?;
    exact_keys(
        lineage,
        &[
            "schema_version",
            "contract_version",
            "scope_key",
            "revision_identity",
            "verdict",
        ],
    )?;
    if string(lineage, "schema_version")? != LINEAGE_SCHEMA_VERSION {
        return Err(Failure::UnsupportedSchemaVersion);
    }
    if string(lineage, "contract_version")? != LINEAGE_CONTRACT_VERSION {
        return Err(Failure::UnsupportedContractVersion);
    }
    let lineage_scope = string(lineage, "scope_key")?;
    let lineage_revision = string(lineage, "revision_identity")?;
    let lineage_verdict = string(lineage, "verdict")?;
    if !valid_scope(lineage_scope)
        || !valid_sha256(lineage_revision)
        || !matches!(lineage_verdict, "current" | "superseded" | "unknown")
    {
        return Err(Failure::InvalidField);
    }
    if lineage_scope != scope {
        return Err(Failure::CrossScopeInput);
    }
    if lineage_revision != revision_identity {
        return Err(Failure::RevisionBindingMismatch);
    }

    let mut results = Vec::with_capacity(receipts.len() + 2);
    let admission_reasons = if admission_verdict == "defer" {
        vec!["admission_deferred"]
    } else {
        Vec::new()
    };
    results.push(input_result(
        "admission_record",
        1,
        admission_verdict,
        admission,
        true,
        admission_reasons,
    ));

    let mut adapter_drift = false;
    let mut hold = admission_verdict == "defer";
    for (index, receipt_value) in receipts.iter().enumerate() {
        let receipt = receipt_value.as_object().expect("validated receipt object");
        let is_target = index == target_index;
        let mut reasons = Vec::new();
        if is_target {
            if string(receipt, "source_digest")? != source_digest {
                reasons.push("receipt_source_digest_mismatch");
                hold = true;
            } else {
                match (string(receipt, "verdict")?, string(receipt, "drift_class")?) {
                    ("corresponded", "none") => {}
                    ("drifted", "adapter_owned_fields") => {
                        reasons.push("receipt_adapter_owned_drift");
                        adapter_drift = true;
                    }
                    ("drifted", "canonical_fields") => {
                        reasons.push("receipt_canonical_drift");
                        hold = true;
                    }
                    ("drifted", "unknown") | ("unknown", "unknown") => {
                        reasons.push("receipt_staleness_unknown");
                        hold = true;
                    }
                    _ => return Err(Failure::InvalidField),
                }
            }
        }
        results.push(input_result(
            "receipt",
            (index + 1) as u64,
            string(receipt, "verdict")?,
            receipt,
            is_target,
            reasons,
        ));
    }

    let lineage_reasons = match lineage_verdict {
        "current" => Vec::new(),
        "superseded" => {
            hold = true;
            vec!["revision_superseded"]
        }
        "unknown" => {
            hold = true;
            vec!["revision_staleness_unknown"]
        }
        _ => unreachable!("lineage verdict was validated"),
    };
    results.push(input_result(
        "revision_lineage",
        1,
        lineage_verdict,
        lineage,
        true,
        lineage_reasons,
    ));

    let reason_codes: Vec<&str> = results
        .iter()
        .flat_map(|result| result["reason_codes"].as_array().unwrap())
        .filter_map(Value::as_str)
        .collect();
    let decision = if hold {
        "hold"
    } else if adapter_drift {
        "skip"
    } else {
        "regenerate"
    };

    Ok(json!({
        "schema_version": OUTPUT_SCHEMA_VERSION,
        "valid": true,
        "contract_version": contract_version,
        "decision": decision,
        "input_results": results,
        "reason_codes": reason_codes,
        "error_count": 0,
        "errors": [],
    }))
}

fn input_result(
    role: &str,
    ordinal: u64,
    verdict: &str,
    input: &Map<String, Value>,
    applies_to_supplied_revision: bool,
    reason_codes: Vec<&str>,
) -> Value {
    let input = Value::Object(input.clone());
    json!({
        "role": role,
        "ordinal": ordinal,
        "verdict": verdict,
        "input_digest": sha256_prefixed(&canonical_json_bytes(&input)),
        "applies_to_supplied_revision": applies_to_supplied_revision,
        "reason_codes": reason_codes,
    })
}

fn failure_output(failure: Failure, contract_version: Option<&str>) -> Value {
    json!({
        "schema_version": OUTPUT_SCHEMA_VERSION,
        "valid": false,
        "contract_version": contract_version,
        "decision": null,
        "input_results": [],
        "reason_codes": [],
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

fn array<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a Vec<Value>, Failure> {
    field(object, key)?
        .as_array()
        .ok_or(Failure::MalformedInput)
}

fn unsigned_integer(object: &Map<String, Value>, key: &str) -> Result<u64, Failure> {
    field(object, key)?.as_u64().ok_or(Failure::InvalidField)
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
            "request_rematerialization_canonical_refresh_regenerate_input.json",
            "request_rematerialization_canonical_refresh_regenerate_expected.json",
        ),
        (
            "request_rematerialization_adapter_only_skip_input.json",
            "request_rematerialization_adapter_only_skip_expected.json",
        ),
        (
            "request_rematerialization_canonical_drift_hold_input.json",
            "request_rematerialization_canonical_drift_hold_expected.json",
        ),
        (
            "request_rematerialization_superseded_hold_input.json",
            "request_rematerialization_superseded_hold_expected.json",
        ),
        (
            "request_rematerialization_deferred_hold_input.json",
            "request_rematerialization_deferred_hold_expected.json",
        ),
        (
            "request_rematerialization_unknown_staleness_hold_input.json",
            "request_rematerialization_unknown_staleness_hold_expected.json",
        ),
        (
            "request_rematerialization_malformed_input.json",
            "request_rematerialization_malformed_expected.json",
        ),
        (
            "request_rematerialization_cross_scope_input.json",
            "request_rematerialization_cross_scope_expected.json",
        ),
        (
            "request_rematerialization_empty_receipts_input.json",
            "request_rematerialization_empty_receipts_expected.json",
        ),
        (
            "request_rematerialization_duplicate_ordinal_input.json",
            "request_rematerialization_duplicate_ordinal_expected.json",
        ),
    ];

    fn fixture(name: &str) -> Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join(name);
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn documented_fixtures_match_and_repeat_byte_identically() {
        for (input_name, expected_name) in FIXTURES {
            let input = fixture(input_name);
            let actual = decide_request_rematerialization(&input);
            assert_eq!(actual, fixture(expected_name), "fixture {input_name}");
            assert_eq!(
                serde_json::to_vec(&actual).unwrap(),
                serde_json::to_vec(&decide_request_rematerialization(&input)).unwrap(),
                "repeat fixture {input_name}"
            );
        }
    }

    #[test]
    fn missing_and_unsupported_contracts_have_no_partial_decision() {
        let mut missing = fixture(FIXTURES[0].0);
        missing.as_object_mut().unwrap().remove("receipts");
        let output = decide_request_rematerialization(&missing);
        assert_eq!(output["errors"][0]["code"], "REQUIRED_FIELD_MISSING");
        assert!(output["decision"].is_null());
        assert!(output["input_results"].as_array().unwrap().is_empty());

        let mut unsupported = fixture(FIXTURES[0].0);
        unsupported["contract_version"] = json!("future.v9");
        let output = decide_request_rematerialization(&unsupported);
        assert_eq!(output["errors"][0]["code"], "UNSUPPORTED_CONTRACT_VERSION");
        assert!(output["decision"].is_null());
        assert!(output["input_results"].as_array().unwrap().is_empty());
    }

    #[test]
    fn oversized_input_fails_without_partial_decision() {
        let oversized = json!({"payload": "x".repeat(MAX_INPUT_BYTES + 1)});
        let output = decide_request_rematerialization(&oversized);
        assert_eq!(output["errors"][0]["code"], "INPUT_TOO_LARGE");
        assert!(output["decision"].is_null());
        assert!(output["input_results"].as_array().unwrap().is_empty());
    }

    #[test]
    fn source_digest_mismatch_holds_and_redacts_input_values() {
        let mut input = fixture("request_rematerialization_adapter_only_skip_input.json");
        input["receipts"][0]["source_digest"] =
            json!("sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc");
        let output = decide_request_rematerialization(&input);
        assert_eq!(output["decision"], "hold");
        assert_eq!(
            output["reason_codes"],
            json!(["receipt_source_digest_mismatch"])
        );
        let encoded = serde_json::to_string(&output).unwrap();
        assert!(!encoded.contains("agentmesh:app:request-rematerialization-decision"));
        assert!(!encoded
            .contains("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
    }

    #[test]
    fn receipt_limit_bounds_result_and_reason_lists() {
        let mut input =
            fixture("request_rematerialization_canonical_refresh_regenerate_input.json");
        let first = input["receipts"][0].clone();
        let mut target = input["receipts"][1].clone();
        target["ordinal"] = json!(1);
        let mut receipts = vec![target];
        for ordinal in 2..=MAX_RECEIPTS {
            let mut receipt = first.clone();
            receipt["ordinal"] = json!(ordinal);
            receipt["revision_identity"] = json!(format!("sha256:{:064x}", ordinal + 100));
            receipts.push(receipt);
        }
        input["receipts"] = json!(receipts);
        let output = decide_request_rematerialization(&input);
        assert_eq!(output["decision"], "regenerate");
        assert_eq!(
            output["input_results"].as_array().unwrap().len(),
            MAX_RECEIPTS + 2
        );
        assert!(output["reason_codes"].as_array().unwrap().len() <= MAX_RECEIPTS + 2);

        input["receipts"].as_array_mut().unwrap().push(first);
        let output = decide_request_rematerialization(&input);
        assert_eq!(output["errors"][0]["code"], "RECEIPT_LIST_TOO_LARGE");
        assert!(output["decision"].is_null());
    }
}
