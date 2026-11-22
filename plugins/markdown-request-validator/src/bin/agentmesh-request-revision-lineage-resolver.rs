//! One-shot, tool-neutral request revision lineage resolver.
//!
//! This App accepts a bounded list of validated revision summaries for one
//! request scope (revision id, canonical digest, explicit `supersedes`
//! reference) and at most one adapter materialization reference digest. It
//! resolves exactly one current revision, emits the ordered supersession chain,
//! and classifies the materialization reference as `current`, `stale` or
//! `unknown`. Output carries revision ids, digests, normalized codes and field
//! paths only. It reads nothing outside its declared input and performs no side
//! effects.

use agentmesh_fixture_support::run_fixture;
use agentmesh_proto::rpc::RunResult;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

const APP_VERSION: &str = "request-revision-lineage.v0";
const INPUT_SCHEMA_VERSION: &str = "request-revision-lineage-input.v0";
const OUTPUT_SCHEMA_VERSION: &str = "request-revision-lineage-compact.v0";
const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_REVISIONS: usize = 32;
const MAX_SCOPE_KEY_BYTES: usize = 96;
const MAX_REVISION_ID_BYTES: usize = 64;
const DIGEST_PREFIX: &str = "sha256:";
const DIGEST_HEX_LEN: usize = 64;

const REASON_ABSENT: &str = "AGENTMESH_REQUEST_REVISION_LINEAGE_MATERIALIZATION_ABSENT";
const REASON_SUPERSEDED: &str = "AGENTMESH_REQUEST_REVISION_LINEAGE_MATERIALIZATION_SUPERSEDED";
const REASON_UNRECOGNIZED: &str = "AGENTMESH_REQUEST_REVISION_LINEAGE_MATERIALIZATION_UNRECOGNIZED";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    InputInvalid,
    InputTooLarge,
    SchemaVersionUnsupported,
    DuplicateRevisionId,
    ScopeMismatch,
    DanglingReference,
    CycleDetected,
    BranchedLineage,
    MultipleCurrent,
}

impl Failure {
    const fn code(self) -> &'static str {
        match self {
            Self::InputInvalid => "AGENTMESH_REQUEST_REVISION_LINEAGE_INPUT_INVALID",
            Self::InputTooLarge => "AGENTMESH_REQUEST_REVISION_LINEAGE_INPUT_TOO_LARGE",
            Self::SchemaVersionUnsupported => {
                "AGENTMESH_REQUEST_REVISION_LINEAGE_SCHEMA_VERSION_UNSUPPORTED"
            }
            Self::DuplicateRevisionId => "AGENTMESH_REQUEST_REVISION_LINEAGE_DUPLICATE_REVISION_ID",
            Self::ScopeMismatch => "AGENTMESH_REQUEST_REVISION_LINEAGE_SCOPE_MISMATCH",
            Self::DanglingReference => "AGENTMESH_REQUEST_REVISION_LINEAGE_DANGLING_REFERENCE",
            Self::CycleDetected => "AGENTMESH_REQUEST_REVISION_LINEAGE_CYCLE_DETECTED",
            Self::BranchedLineage => "AGENTMESH_REQUEST_REVISION_LINEAGE_BRANCHED_LINEAGE",
            Self::MultipleCurrent => "AGENTMESH_REQUEST_REVISION_LINEAGE_MULTIPLE_CURRENT",
        }
    }
}

struct Revision<'a> {
    id: &'a str,
    digest: &'a str,
    supersedes: Option<&'a str>,
}

fn main() -> ExitCode {
    run_fixture(
        APP_VERSION,
        &[
            "request_revision_lineage_resolver",
            "single_current_revision_or_deterministic_failure",
            "no_adapter_value_echo",
        ],
        Box::new(|params| {
            Ok(RunResult {
                payload: resolve_request_revision_lineage(&params.input),
            })
        }),
    )
}

fn resolve_request_revision_lineage(value: &Value) -> Value {
    match validate_and_resolve(value) {
        Ok(lineage) => json!({
            "schema_version": OUTPUT_SCHEMA_VERSION,
            "lineage_version": APP_VERSION,
            "valid": true,
            "lineage": lineage,
            "error_count": 0,
            "errors": [],
        }),
        Err(failure) => json!({
            "schema_version": OUTPUT_SCHEMA_VERSION,
            "lineage_version": APP_VERSION,
            "valid": false,
            "lineage": null,
            "error_count": 1,
            "errors": [{"code": failure.code()}],
        }),
    }
}

fn validate_and_resolve(value: &Value) -> Result<Value, Failure> {
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
            "scope_key",
            "revisions",
            "materialization_reference_digest",
        ],
    )?;
    if string(input, "schema_version")? != INPUT_SCHEMA_VERSION
        || string(input, "contract_version")? != APP_VERSION
    {
        return Err(Failure::SchemaVersionUnsupported);
    }
    let scope_key = string(input, "scope_key")?;
    if !valid_scope_key(scope_key) {
        return Err(Failure::InputInvalid);
    }
    let reference = match field(input, "materialization_reference_digest")? {
        Value::Null => None,
        Value::String(digest) if valid_digest(digest) => Some(digest.as_str()),
        _ => return Err(Failure::InputInvalid),
    };

    let raw_revisions = array(input, "revisions")?;
    if raw_revisions.is_empty() || raw_revisions.len() > MAX_REVISIONS {
        return Err(Failure::InputInvalid);
    }
    let mut revisions: Vec<Revision<'_>> = Vec::with_capacity(raw_revisions.len());
    let mut scope_mismatch = false;
    for raw in raw_revisions {
        let entry = object(raw)?;
        exact_keys(entry, &["revision_id", "scope_key", "digest", "supersedes"])?;
        let id = string(entry, "revision_id")?;
        let digest = string(entry, "digest")?;
        let entry_scope = string(entry, "scope_key")?;
        if !valid_revision_id(id) || !valid_digest(digest) || !valid_scope_key(entry_scope) {
            return Err(Failure::InputInvalid);
        }
        let supersedes = array(entry, "supersedes")?;
        if supersedes.len() > 1 {
            return Err(Failure::InputInvalid);
        }
        let supersedes = match supersedes.first() {
            None => None,
            Some(Value::String(target)) if valid_revision_id(target) => Some(target.as_str()),
            Some(_) => return Err(Failure::InputInvalid),
        };
        scope_mismatch |= entry_scope != scope_key;
        revisions.push(Revision {
            id,
            digest,
            supersedes,
        });
    }

    // Fixed check order keeps the reported failure deterministic.
    let mut index_by_id: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, revision) in revisions.iter().enumerate() {
        if index_by_id.insert(revision.id, index).is_some() {
            return Err(Failure::DuplicateRevisionId);
        }
    }
    if scope_mismatch {
        return Err(Failure::ScopeMismatch);
    }
    let mut targets: Vec<Option<usize>> = Vec::with_capacity(revisions.len());
    for revision in &revisions {
        targets.push(match revision.supersedes {
            None => None,
            Some(target) => Some(*index_by_id.get(target).ok_or(Failure::DanglingReference)?),
        });
    }
    if has_cycle(&targets) {
        return Err(Failure::CycleDetected);
    }
    let mut superseded_by: Vec<Option<usize>> = vec![None; revisions.len()];
    for (index, target) in targets.iter().enumerate() {
        if let Some(target) = target {
            if superseded_by[*target].replace(index).is_some() {
                return Err(Failure::BranchedLineage);
            }
        }
    }
    let currents: Vec<usize> = (0..revisions.len())
        .filter(|index| superseded_by[*index].is_none())
        .collect();
    let [current] = currents.as_slice() else {
        return Err(Failure::MultipleCurrent);
    };
    let current = *current;

    // Acyclic, single-current and at-most-one-successor means the chain covers
    // every revision exactly once.
    let mut chain_indexes = vec![current];
    while let Some(next) = targets[*chain_indexes.last().unwrap_or(&current)] {
        chain_indexes.push(next);
    }
    let chain: Vec<Value> = chain_indexes
        .iter()
        .enumerate()
        .map(|(position, index)| {
            json!({
                "ordinal": position + 1,
                "revision_id": revisions[*index].id,
                "digest": revisions[*index].digest,
                "role": if *index == current { "current" } else { "superseded" },
                "superseded_by": superseded_by[*index].map(|by| revisions[by].id),
            })
        })
        .collect();

    Ok(json!({
        "scope_key": scope_key,
        "revision_count": revisions.len(),
        "current_revision_id": revisions[current].id,
        "current_digest": revisions[current].digest,
        "chain": chain,
        "materialization": classify_materialization(reference, &revisions, current),
    }))
}

fn has_cycle(targets: &[Option<usize>]) -> bool {
    // Each node has at most one outgoing edge, so a walk longer than the node
    // count must revisit a node.
    (0..targets.len()).any(|start| {
        let mut seen = BTreeSet::new();
        let mut cursor = Some(start);
        while let Some(node) = cursor {
            if !seen.insert(node) {
                return true;
            }
            cursor = targets[node];
        }
        false
    })
}

fn classify_materialization(
    reference: Option<&str>,
    revisions: &[Revision<'_>],
    current: usize,
) -> Value {
    let (verdict, reason) = match reference {
        None => (
            "unknown",
            Some((REASON_ABSENT, "materialization_reference_digest".to_owned())),
        ),
        Some(digest) if digest == revisions[current].digest => ("current", None),
        Some(digest) => match revisions
            .iter()
            .position(|revision| revision.digest == digest)
        {
            Some(index) => (
                "stale",
                Some((REASON_SUPERSEDED, format!("revisions[{index}].digest"))),
            ),
            None => (
                "unknown",
                Some((
                    REASON_UNRECOGNIZED,
                    "materialization_reference_digest".to_owned(),
                )),
            ),
        },
    };
    let reasons: Vec<Value> = reason
        .into_iter()
        .map(|(code, field_path)| json!({"code": code, "field_path": field_path}))
        .collect();
    json!({
        "verdict": verdict,
        "reason_count": reasons.len(),
        "reasons": reasons,
    })
}

fn valid_scope_key(scope: &str) -> bool {
    !scope.is_empty()
        && scope.len() <= MAX_SCOPE_KEY_BYTES
        && scope.split(':').all(|segment| {
            !segment.is_empty()
                && segment.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'_' | b'-')
                })
        })
}

fn valid_revision_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_REVISION_ID_BYTES
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.')
        })
        && id
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn valid_digest(digest: &str) -> bool {
    digest.strip_prefix(DIGEST_PREFIX).is_some_and(|hex| {
        hex.len() == DIGEST_HEX_LEN
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const FIXTURES: &[(&str, &str)] = &[
        (
            "request_revision_lineage_linear_input.json",
            "request_revision_lineage_linear_expected.json",
        ),
        (
            "request_revision_lineage_stale_input.json",
            "request_revision_lineage_stale_expected.json",
        ),
        (
            "request_revision_lineage_cycle_input.json",
            "request_revision_lineage_cycle_expected.json",
        ),
        (
            "request_revision_lineage_cross_scope_input.json",
            "request_revision_lineage_cross_scope_expected.json",
        ),
        (
            "request_revision_lineage_malformed_input.json",
            "request_revision_lineage_malformed_expected.json",
        ),
    ];

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join(name)
    }

    fn load(name: &str) -> Value {
        serde_json::from_slice(&std::fs::read(fixture_path(name)).unwrap()).unwrap()
    }

    fn digest(seed: char) -> String {
        format!("{DIGEST_PREFIX}{}", seed.to_string().repeat(DIGEST_HEX_LEN))
    }

    fn revision(id: &str, seed: char, supersedes: &[&str]) -> Value {
        json!({
            "revision_id": id,
            "scope_key": "agentmesh:app:lineage-test",
            "digest": digest(seed),
            "supersedes": supersedes,
        })
    }

    fn input(revisions: Vec<Value>, reference: Option<char>) -> Value {
        json!({
            "schema_version": INPUT_SCHEMA_VERSION,
            "contract_version": APP_VERSION,
            "scope_key": "agentmesh:app:lineage-test",
            "revisions": revisions,
            "materialization_reference_digest": reference.map(digest),
        })
    }

    fn error_code(output: &Value) -> &str {
        assert_eq!(output["valid"], false);
        assert_eq!(output["lineage"], Value::Null);
        assert_eq!(output["error_count"], 1);
        output["errors"][0]["code"].as_str().unwrap()
    }

    #[test]
    fn recorded_fixtures_match_expected_payloads_and_repeat_byte_identically() {
        for (input_name, expected_name) in FIXTURES {
            let input = load(input_name);
            let expected = load(expected_name);
            let first = resolve_request_revision_lineage(&input);
            let second = resolve_request_revision_lineage(&input);
            assert_eq!(first, expected, "fixture {input_name}");
            assert_eq!(
                serde_json::to_vec(&first).unwrap(),
                serde_json::to_vec(&second).unwrap(),
                "fixture {input_name} changed across repeated runs"
            );
        }
    }

    #[test]
    fn input_order_does_not_change_the_chain() {
        let forward = input(
            vec![
                revision("r1", 'a', &[]),
                revision("r2", 'b', &["r1"]),
                revision("r3", 'c', &["r2"]),
            ],
            Some('c'),
        );
        let shuffled = input(
            vec![
                revision("r2", 'b', &["r1"]),
                revision("r3", 'c', &["r2"]),
                revision("r1", 'a', &[]),
            ],
            Some('c'),
        );
        let first = resolve_request_revision_lineage(&forward);
        let second = resolve_request_revision_lineage(&shuffled);
        assert_eq!(first["lineage"]["chain"], second["lineage"]["chain"]);
        assert_eq!(first["lineage"]["current_revision_id"], "r3");
        let ids: Vec<&str> = first["lineage"]["chain"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["revision_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["r3", "r2", "r1"]);
    }

    #[test]
    fn single_revision_is_its_own_current_revision() {
        let output =
            resolve_request_revision_lineage(&input(vec![revision("r1", 'a', &[])], Some('a')));
        assert_eq!(output["valid"], true);
        assert_eq!(output["lineage"]["chain"][0]["role"], "current");
        assert_eq!(output["lineage"]["chain"][0]["superseded_by"], Value::Null);
        assert_eq!(output["lineage"]["materialization"]["verdict"], "current");
    }

    #[test]
    fn superseded_revisions_are_reported_not_dropped() {
        let output = resolve_request_revision_lineage(&input(
            vec![revision("r1", 'a', &[]), revision("r2", 'b', &["r1"])],
            None,
        ));
        let chain = output["lineage"]["chain"].as_array().unwrap();
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[1]["role"], "superseded");
        assert_eq!(chain[1]["superseded_by"], "r2");
    }

    #[test]
    fn materialization_verdicts_list_reasons_only_when_not_current() {
        let revisions = || {
            vec![
                revision("r1", 'a', &[]),
                revision("r2", 'b', &["r1"]),
                revision("r3", 'c', &["r2"]),
            ]
        };
        let current = resolve_request_revision_lineage(&input(revisions(), Some('c')));
        let materialization = &current["lineage"]["materialization"];
        assert_eq!(materialization["verdict"], "current");
        assert_eq!(materialization["reason_count"], 0);
        assert_eq!(materialization["reasons"], json!([]));

        let stale = resolve_request_revision_lineage(&input(revisions(), Some('a')));
        let materialization = &stale["lineage"]["materialization"];
        assert_eq!(materialization["verdict"], "stale");
        assert_eq!(materialization["reasons"][0]["code"], REASON_SUPERSEDED);
        assert_eq!(
            materialization["reasons"][0]["field_path"],
            "revisions[0].digest"
        );

        let unknown = resolve_request_revision_lineage(&input(revisions(), Some('f')));
        let materialization = &unknown["lineage"]["materialization"];
        assert_eq!(materialization["verdict"], "unknown");
        assert_eq!(materialization["reasons"][0]["code"], REASON_UNRECOGNIZED);

        let absent = resolve_request_revision_lineage(&input(revisions(), None));
        let materialization = &absent["lineage"]["materialization"];
        assert_eq!(materialization["verdict"], "unknown");
        assert_eq!(materialization["reasons"][0]["code"], REASON_ABSENT);
    }

    #[test]
    fn unknown_reference_is_never_echoed() {
        let output =
            resolve_request_revision_lineage(&input(vec![revision("r1", 'a', &[])], Some('9')));
        assert!(!serde_json::to_string(&output)
            .unwrap()
            .contains(&digest('9')));
    }

    #[test]
    fn invalid_lineages_fail_with_distinct_codes_and_no_partial_output() {
        let cases: Vec<(Value, Failure)> = vec![
            (
                input(
                    vec![revision("r1", 'a', &["r2"]), revision("r2", 'b', &["r1"])],
                    None,
                ),
                Failure::CycleDetected,
            ),
            (
                input(vec![revision("r1", 'a', &["r1"])], None),
                Failure::CycleDetected,
            ),
            (
                input(vec![revision("r2", 'b', &["r1"])], None),
                Failure::DanglingReference,
            ),
            (
                input(
                    vec![revision("r1", 'a', &[]), revision("r1", 'b', &[])],
                    None,
                ),
                Failure::DuplicateRevisionId,
            ),
            (
                input(
                    vec![
                        revision("r1", 'a', &[]),
                        revision("r2", 'b', &["r1"]),
                        revision("r3", 'c', &["r1"]),
                    ],
                    None,
                ),
                Failure::BranchedLineage,
            ),
            (
                input(
                    vec![revision("r1", 'a', &[]), revision("r2", 'b', &[])],
                    None,
                ),
                Failure::MultipleCurrent,
            ),
        ];
        for (case, failure) in cases {
            let output = resolve_request_revision_lineage(&case);
            assert_eq!(error_code(&output), failure.code(), "{failure:?}");
        }
    }

    #[test]
    fn mixed_scope_keys_are_rejected() {
        let mut case = input(vec![revision("r1", 'a', &[])], None);
        case["revisions"][0]["scope_key"] = json!("agentmesh:app:other");
        let output = resolve_request_revision_lineage(&case);
        assert_eq!(error_code(&output), Failure::ScopeMismatch.code());
    }

    #[test]
    fn unsupported_versions_are_rejected() {
        for key in ["schema_version", "contract_version"] {
            let mut case = input(vec![revision("r1", 'a', &[])], None);
            case[key] = json!("request-revision-lineage.v9");
            let output = resolve_request_revision_lineage(&case);
            assert_eq!(
                error_code(&output),
                Failure::SchemaVersionUnsupported.code()
            );
        }
    }

    #[test]
    fn malformed_input_is_rejected() {
        let good = || input(vec![revision("r1", 'a', &[])], None);
        let mut bad_digest = good();
        bad_digest["revisions"][0]["digest"] = json!("sha256:XYZ");
        let mut bad_id = good();
        bad_id["revisions"][0]["revision_id"] = json!("Bad Id");
        let mut extra_key = good();
        extra_key["revisions"][0]["tracker"] = json!("x");
        let mut two_targets = good();
        two_targets["revisions"][0]["supersedes"] = json!(["a", "b"]);
        let mut bad_reference = good();
        bad_reference["materialization_reference_digest"] = json!("nope");
        let mut empty = good();
        empty["revisions"] = json!([]);
        let mut too_many = good();
        too_many["revisions"] = Value::Array(
            (0..=MAX_REVISIONS)
                .map(|index| revision(&format!("r{index}"), 'a', &[]))
                .collect(),
        );
        for (name, case) in [
            ("bad_digest", bad_digest),
            ("bad_id", bad_id),
            ("extra_key", extra_key),
            ("two_targets", two_targets),
            ("bad_reference", bad_reference),
            ("empty", empty),
            ("too_many", too_many),
            ("not_object", json!([1, 2])),
        ] {
            let output = resolve_request_revision_lineage(&case);
            assert_eq!(error_code(&output), Failure::InputInvalid.code(), "{name}");
        }
    }

    #[test]
    fn oversized_input_returns_only_a_normalized_failure() {
        let mut case = input(vec![revision("r1", 'a', &[])], None);
        case["padding"] = Value::String("x".repeat(MAX_INPUT_BYTES + 1));
        let output = resolve_request_revision_lineage(&case);
        assert_eq!(error_code(&output), Failure::InputTooLarge.code());
    }

    #[test]
    fn maximum_length_chain_resolves() {
        let revisions: Vec<Value> = (0..MAX_REVISIONS)
            .map(|index| {
                if index == 0 {
                    revision("r0", 'a', &[])
                } else {
                    revision(&format!("r{index}"), 'a', &[&format!("r{}", index - 1)])
                }
            })
            .collect();
        let output = resolve_request_revision_lineage(&input(revisions, None));
        assert_eq!(output["valid"], true);
        assert_eq!(
            output["lineage"]["chain"].as_array().unwrap().len(),
            MAX_REVISIONS
        );
        assert_eq!(
            output["lineage"]["current_revision_id"],
            format!("r{}", MAX_REVISIONS - 1)
        );
    }
}
