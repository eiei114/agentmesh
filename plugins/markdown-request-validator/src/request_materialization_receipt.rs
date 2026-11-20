//! Deterministic post-write request materialization receipt App.
//!
//! The App compares a supplied admission record and canonical request summary
//! with a bounded adapter artifact descriptor. It never reads the source or the
//! artifact, and emits only stable field ordinals, normalized codes, and hashes.

use crate::adapter_compact_helpers::canonical_json_bytes;
use agentmesh_evidence::sha256_prefixed;
use serde::de::{self, Deserializer, Visitor};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fmt;

/// Contract version accepted by this App and copied into successful receipts.
pub const RECEIPT_CONTRACT_VERSION: &str = "agentmesh-request-materialization-receipt.v0";
const INPUT_SCHEMA_VERSION: &str = "request-materialization-receipt-input.v0";
const OUTPUT_SCHEMA_VERSION: &str = "request-materialization-receipt-compact.v0";
const RECEIPT_VERSION: &str = "request-materialization-receipt.v0";
const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_CRITERIA_COUNT: u32 = 10_000;

const FIELD_ORDER: &[&str] = &[
    "scope_key",
    "request_kind",
    "title_digest",
    "criteria_count",
    "admission_verdict",
    "source_digest",
    "artifact_digest",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptInput {
    schema_version: String,
    contract_version: String,
    admission_record: AdmissionRecord,
    request_summary: RequestSummary,
    artifact: ArtifactDescriptor,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdmissionRecord {
    verdict: String,
    scope_key: String,
    request_kind: String,
    title_digest: String,
    #[serde(deserialize_with = "deserialize_criteria_count")]
    criteria_count: u32,
    source_digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestSummary {
    scope_key: String,
    request_kind: String,
    title_digest: String,
    #[serde(deserialize_with = "deserialize_criteria_count")]
    criteria_count: u32,
    source_digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactDescriptor {
    adapter_id: String,
    artifact_kind: String,
    artifact_digest: String,
    /// Digest of the request source recorded by the adapter beside its artifact.
    source_digest: String,
}

/// Produce a compact, deterministic post-write materialization receipt.
pub fn create_materialization_receipt(value: &Value) -> Value {
    if canonical_json_bytes(value).len() > MAX_INPUT_BYTES {
        return failure("INPUT_TOO_LARGE");
    }

    let Some(root) = value.as_object() else {
        return failure("MALFORMED_INPUT");
    };
    if let Some(code) = missing_required(
        root,
        &[
            "schema_version",
            "contract_version",
            "admission_record",
            "request_summary",
            "artifact",
        ],
    ) {
        return failure(code);
    }

    match root.get("schema_version").and_then(Value::as_str) {
        Some(INPUT_SCHEMA_VERSION) => {}
        Some(_) => return failure("UNSUPPORTED_SCHEMA_VERSION"),
        None => return failure("MALFORMED_INPUT"),
    }
    match root.get("contract_version").and_then(Value::as_str) {
        Some(RECEIPT_CONTRACT_VERSION) => {}
        Some(_) => return failure("UNSUPPORTED_CONTRACT_VERSION"),
        None => return failure("MALFORMED_INPUT"),
    }

    for (key, required) in [
        (
            "admission_record",
            &[
                "verdict",
                "scope_key",
                "request_kind",
                "title_digest",
                "criteria_count",
                "source_digest",
            ][..],
        ),
        (
            "request_summary",
            &[
                "scope_key",
                "request_kind",
                "title_digest",
                "criteria_count",
                "source_digest",
            ][..],
        ),
        (
            "artifact",
            &[
                "adapter_id",
                "artifact_kind",
                "artifact_digest",
                "source_digest",
            ][..],
        ),
    ] {
        let Some(object) = root.get(key).and_then(Value::as_object) else {
            return failure("MALFORMED_INPUT");
        };
        if let Some(code) = missing_required(object, required) {
            return failure(code);
        }
    }

    let Ok(input) = serde_json::from_value::<ReceiptInput>(value.clone()) else {
        return failure("MALFORMED_INPUT");
    };
    if input.schema_version != INPUT_SCHEMA_VERSION {
        return failure("UNSUPPORTED_SCHEMA_VERSION");
    }
    if input.contract_version != RECEIPT_CONTRACT_VERSION {
        return failure("UNSUPPORTED_CONTRACT_VERSION");
    }
    if !valid_record(&input) {
        return failure("INVALID_FIELD");
    }

    // Scope keys outside the AgentMesh namespace cannot be compared safely.
    // Different AgentMesh scope keys remain a reportable drift, not an error.
    if !is_agentmesh_scope(&input.admission_record.scope_key)
        || !is_agentmesh_scope(&input.request_summary.scope_key)
    {
        return failure("CROSS_SCOPE_INPUT");
    }

    let admission = &input.admission_record;
    let summary = &input.request_summary;
    let artifact = &input.artifact;
    let mut reasons = Vec::new();
    let mut fields = Vec::with_capacity(FIELD_ORDER.len());

    push_comparison(
        &mut fields,
        &mut reasons,
        "scope_key",
        json!([admission.scope_key, summary.scope_key]),
        admission.scope_key == summary.scope_key,
        "scope_key_mismatch",
    );
    push_comparison(
        &mut fields,
        &mut reasons,
        "request_kind",
        json!([admission.request_kind, summary.request_kind]),
        admission.request_kind == summary.request_kind,
        "request_kind_mismatch",
    );
    push_comparison(
        &mut fields,
        &mut reasons,
        "title_digest",
        json!([admission.title_digest, summary.title_digest]),
        admission.title_digest == summary.title_digest,
        "title_digest_mismatch",
    );
    push_comparison(
        &mut fields,
        &mut reasons,
        "criteria_count",
        json!([admission.criteria_count, summary.criteria_count]),
        admission.criteria_count == summary.criteria_count,
        "criteria_count_mismatch",
    );
    push_comparison(
        &mut fields,
        &mut reasons,
        "admission_verdict",
        json!([admission.verdict, "admit"]),
        admission.verdict == "admit",
        "admission_not_admitted",
    );
    push_comparison(
        &mut fields,
        &mut reasons,
        "source_digest",
        json!([
            admission.source_digest,
            summary.source_digest,
            artifact.source_digest
        ]),
        admission.source_digest == summary.source_digest
            && summary.source_digest == artifact.source_digest,
        "source_digest_mismatch",
    );
    fields.push(json!({
        "field": "artifact_digest",
        "ordinal": ordinal("artifact_digest"),
        "compared_values_digest": digest(&json!([artifact.artifact_digest])),
        "code": "artifact_digest_recorded"
    }));

    json!({
        "schema_version": OUTPUT_SCHEMA_VERSION,
        "valid": true,
        "receipt": {
            "receipt_version": RECEIPT_VERSION,
            "contract_version": input.contract_version,
            "verdict": if reasons.is_empty() { "corresponded" } else { "drifted" },
            "fields": fields,
            "reason_codes": reasons
        },
        "error_count": 0,
        "errors": []
    })
}

fn missing_required(
    object: &serde_json::Map<String, Value>,
    fields: &[&str],
) -> Option<&'static str> {
    fields
        .iter()
        .any(|field| !object.contains_key(*field))
        .then_some("REQUIRED_FIELD_MISSING")
}

fn deserialize_criteria_count<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    struct CriteriaCountVisitor;

    impl Visitor<'_> for CriteriaCountVisitor {
        type Value = u32;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a non-negative integral u32")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            u32::try_from(value).map_err(|_| E::custom("criteria_count exceeds u32"))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            u32::try_from(value).map_err(|_| E::custom("criteria_count is outside u32"))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if value.is_finite()
                && value.fract() == 0.0
                && value >= 0.0
                && value <= f64::from(u32::MAX)
            {
                Ok(value as u32)
            } else {
                Err(E::custom("criteria_count must be an integral u32"))
            }
        }
    }

    deserializer.deserialize_any(CriteriaCountVisitor)
}

fn valid_record(input: &ReceiptInput) -> bool {
    let admission = &input.admission_record;
    let summary = &input.request_summary;
    let artifact = &input.artifact;
    matches!(admission.verdict.as_str(), "admit" | "defer")
        && valid_scope(&admission.scope_key)
        && valid_scope(&summary.scope_key)
        && valid_kind(&admission.request_kind)
        && valid_kind(&summary.request_kind)
        && valid_digest(&admission.title_digest)
        && valid_digest(&summary.title_digest)
        && admission.criteria_count <= MAX_CRITERIA_COUNT
        && summary.criteria_count <= MAX_CRITERIA_COUNT
        && valid_digest(&admission.source_digest)
        && valid_digest(&summary.source_digest)
        && valid_identity(&artifact.adapter_id)
        && valid_identity(&artifact.artifact_kind)
        && valid_digest(&artifact.artifact_digest)
        && valid_digest(&artifact.source_digest)
}

fn valid_scope(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.chars().any(char::is_whitespace)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b":._-".contains(&byte))
}

fn is_agentmesh_scope(value: &str) -> bool {
    value.starts_with("agentmesh:") && value.len() > "agentmesh:".len()
}

fn valid_kind(value: &str) -> bool {
    matches!(value, "app" | "repair")
}

fn valid_identity(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
        && value.as_bytes()[0]
            .is_ascii_lowercase()
            .then_some(())
            .is_some()
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn push_comparison(
    fields: &mut Vec<Value>,
    reasons: &mut Vec<String>,
    name: &'static str,
    compared_values: Value,
    matches: bool,
    mismatch_code: &'static str,
) {
    let code = if matches { "matched" } else { mismatch_code };
    if !matches {
        reasons.push(mismatch_code.to_string());
    }
    fields.push(json!({
        "field": name,
        "ordinal": ordinal(name),
        "compared_values_digest": digest(&compared_values),
        "code": code
    }));
}

fn ordinal(name: &str) -> usize {
    FIELD_ORDER
        .iter()
        .position(|field| *field == name)
        .expect("field must be in the fixed receipt order")
        + 1
}

fn digest(value: &Value) -> String {
    sha256_prefixed(&canonical_json_bytes(value))
}

fn failure(code: &'static str) -> Value {
    json!({
        "schema_version": OUTPUT_SCHEMA_VERSION,
        "valid": false,
        "receipt": null,
        "error_count": 1,
        "errors": [{"code": code}]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Value {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join(name);
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn fixtures_match_their_expected_receipts() {
        for (input, expected) in [
            (
                "request_materialization_receipt_success_input.json",
                "expected_request_materialization_receipt_success_payload.json",
            ),
            (
                "request_materialization_receipt_criteria_drift_input.json",
                "expected_request_materialization_receipt_criteria_drift_payload.json",
            ),
            (
                "request_materialization_receipt_source_drift_input.json",
                "expected_request_materialization_receipt_source_drift_payload.json",
            ),
            (
                "request_materialization_receipt_deferred_input.json",
                "expected_request_materialization_receipt_deferred_payload.json",
            ),
            (
                "request_materialization_receipt_malformed_input.json",
                "expected_request_materialization_receipt_malformed_payload.json",
            ),
            (
                "request_materialization_receipt_cross_scope_input.json",
                "expected_request_materialization_receipt_cross_scope_payload.json",
            ),
        ] {
            assert_eq!(
                create_materialization_receipt(&fixture(input)),
                fixture(expected),
                "fixture {input}"
            );
        }
    }

    #[test]
    fn repeated_inputs_are_byte_identical() {
        let input = fixture("request_materialization_receipt_success_input.json");
        let first = serde_json::to_vec(&create_materialization_receipt(&input)).unwrap();
        let second = serde_json::to_vec(&create_materialization_receipt(&input)).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn integral_decimal_criteria_counts_match_integer_inputs() {
        let integer_input = fixture("request_materialization_receipt_success_input.json");
        let mut decimal_input = integer_input.clone();
        decimal_input["admission_record"]["criteria_count"] = json!(3.0);
        decimal_input["request_summary"]["criteria_count"] = json!(3.0);

        assert_eq!(
            create_materialization_receipt(&decimal_input),
            create_materialization_receipt(&integer_input)
        );
    }

    #[test]
    fn field_order_ordinals_and_reason_order_are_fixed() {
        let mut input = fixture("request_materialization_receipt_success_input.json");
        input["admission_record"]["scope_key"] = json!("agentmesh:app:other");
        input["admission_record"]["request_kind"] = json!("repair");
        input["admission_record"]["criteria_count"] = json!(1);
        input["admission_record"]["verdict"] = json!("defer");
        let receipt = create_materialization_receipt(&input)["receipt"].clone();
        let fields = receipt["fields"].as_array().unwrap();
        assert_eq!(fields.len(), FIELD_ORDER.len());
        for (index, (field, expected_name)) in fields.iter().zip(FIELD_ORDER).enumerate() {
            assert_eq!(field["field"], *expected_name);
            assert_eq!(field["ordinal"], index + 1);
        }
        assert_eq!(
            receipt["reason_codes"],
            json!([
                "scope_key_mismatch",
                "request_kind_mismatch",
                "criteria_count_mismatch",
                "admission_not_admitted"
            ])
        );
    }

    #[test]
    fn missing_and_unsupported_versions_fail_without_partial_receipts() {
        let mut missing = fixture("request_materialization_receipt_success_input.json");
        missing["artifact"]
            .as_object_mut()
            .unwrap()
            .remove("source_digest");
        assert_eq!(
            create_materialization_receipt(&missing)["errors"][0]["code"],
            "REQUIRED_FIELD_MISSING"
        );

        let mut unsupported_schema = fixture("request_materialization_receipt_success_input.json");
        unsupported_schema["schema_version"] = json!("future-input.v9");
        let output = create_materialization_receipt(&unsupported_schema);
        assert_eq!(output["errors"][0]["code"], "UNSUPPORTED_SCHEMA_VERSION");
        assert!(output["receipt"].is_null());

        let mut unsupported = fixture("request_materialization_receipt_success_input.json");
        unsupported["contract_version"] = json!("future.v9");
        let output = create_materialization_receipt(&unsupported);
        assert_eq!(output["errors"][0]["code"], "UNSUPPORTED_CONTRACT_VERSION");
        assert!(output["receipt"].is_null());

        let mut malformed = fixture("request_materialization_receipt_success_input.json");
        malformed["artifact"]["unexpected"] = json!("not-accepted");
        let output = create_materialization_receipt(&malformed);
        assert_eq!(output["errors"][0]["code"], "MALFORMED_INPUT");
        assert!(output["receipt"].is_null());
    }

    #[test]
    fn oversized_inputs_fail_with_one_normalized_code() {
        let oversized = json!({"payload": "x".repeat(MAX_INPUT_BYTES + 1)});
        let output = create_materialization_receipt(&oversized);
        assert_eq!(output["errors"][0]["code"], "INPUT_TOO_LARGE");
        assert!(output["receipt"].is_null());
    }
}
