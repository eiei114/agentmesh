//! One-shot, tool-neutral request source neutrality gate.
//!
//! This App scans one validated request source summary (a flat list of
//! `path`/`value` fields) for content that belongs to a downstream tool rather
//! than to the canonical request: readiness or authority hints, tracker
//! identifiers, execution or rerun commands, session or sidecar paths, and
//! credential-shaped tokens. It reports categories and field paths only and
//! never echoes a matched value. It reads nothing outside its declared input
//! and performs no side effects.

use agentmesh_fixture_support::run_fixture;
use agentmesh_proto::rpc::RunResult;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::process::ExitCode;

const APP_VERSION: &str = "request-source-neutrality.v0";
const INPUT_SCHEMA_VERSION: &str = "request-source-neutrality-input.v0";
const OUTPUT_SCHEMA_VERSION: &str = "request-source-neutrality-compact.v0";
const SOURCE_SCHEMA_VERSION: &str = "request-source-neutrality-source.v0";
const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_FIELDS: usize = 64;
const MAX_PATH_BYTES: usize = 96;
const MAX_VALUE_BYTES: usize = 4096;
const MAX_FINDINGS: usize = 32;

const REQUIRED_FIELDS: &[&str] = &["title", "scope", "request_kind", "problem"];
const REQUIRED_PREFIX: &str = "acceptance_criteria.";

const INPUT_INVALID: &str = "AGENTMESH_REQUEST_SOURCE_NEUTRALITY_INPUT_INVALID";
const INPUT_TOO_LARGE: &str = "AGENTMESH_REQUEST_SOURCE_NEUTRALITY_INPUT_TOO_LARGE";
const SCHEMA_VERSION_UNSUPPORTED: &str =
    "AGENTMESH_REQUEST_SOURCE_NEUTRALITY_SCHEMA_VERSION_UNSUPPORTED";
const REQUIRED_FIELD_MISSING: &str = "AGENTMESH_REQUEST_SOURCE_NEUTRALITY_REQUIRED_FIELD_MISSING";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    InputInvalid,
    InputTooLarge,
    SchemaVersionUnsupported,
    RequiredFieldMissing,
}

impl Failure {
    const fn code(self) -> &'static str {
        match self {
            Self::InputInvalid => INPUT_INVALID,
            Self::InputTooLarge => INPUT_TOO_LARGE,
            Self::SchemaVersionUnsupported => SCHEMA_VERSION_UNSUPPORTED,
            Self::RequiredFieldMissing => REQUIRED_FIELD_MISSING,
        }
    }
}

/// Forbidden categories in stable report order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Category {
    ReadinessHint,
    TrackerIdentifier,
    ExecutionCommand,
    SessionSidecarPath,
    CredentialToken,
}

impl Category {
    const ALL: [Self; 5] = [
        Self::ReadinessHint,
        Self::TrackerIdentifier,
        Self::ExecutionCommand,
        Self::SessionSidecarPath,
        Self::CredentialToken,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::ReadinessHint => "readiness_hint",
            Self::TrackerIdentifier => "tracker_identifier",
            Self::ExecutionCommand => "execution_command",
            Self::SessionSidecarPath => "session_sidecar_path",
            Self::CredentialToken => "credential_token",
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::ReadinessHint => "AGENTMESH_REQUEST_SOURCE_NEUTRALITY_READINESS_HINT",
            Self::TrackerIdentifier => "AGENTMESH_REQUEST_SOURCE_NEUTRALITY_TRACKER_IDENTIFIER",
            Self::ExecutionCommand => "AGENTMESH_REQUEST_SOURCE_NEUTRALITY_EXECUTION_COMMAND",
            Self::SessionSidecarPath => "AGENTMESH_REQUEST_SOURCE_NEUTRALITY_SESSION_SIDECAR_PATH",
            Self::CredentialToken => "AGENTMESH_REQUEST_SOURCE_NEUTRALITY_CREDENTIAL_TOKEN",
        }
    }
}

fn main() -> ExitCode {
    run_fixture(
        APP_VERSION,
        &[
            "request_source_neutrality_gate",
            "category_and_field_path_findings_only",
            "no_matched_value_echo",
        ],
        Box::new(|params| {
            Ok(RunResult {
                payload: check_request_source_neutrality(&params.input),
            })
        }),
    )
}

fn check_request_source_neutrality(value: &Value) -> Value {
    match validate_and_check(value) {
        Ok(neutrality) => json!({
            "schema_version": OUTPUT_SCHEMA_VERSION,
            "neutrality_version": APP_VERSION,
            "valid": true,
            "neutrality": neutrality,
            "error_count": 0,
            "errors": [],
        }),
        Err(failure) => json!({
            "schema_version": OUTPUT_SCHEMA_VERSION,
            "neutrality_version": APP_VERSION,
            "valid": false,
            "neutrality": null,
            "error_count": 1,
            "errors": [{"code": failure.code()}],
        }),
    }
}

fn validate_and_check(value: &Value) -> Result<Value, Failure> {
    let encoded = serde_json::to_vec(value).map_err(|_| Failure::InputInvalid)?;
    if encoded.len() > MAX_INPUT_BYTES {
        return Err(Failure::InputTooLarge);
    }

    let input = object(value)?;
    exact_keys(input, &["schema_version", "contract_version", "source"])?;
    let source = object(field(input, "source")?)?;
    exact_keys(source, &["schema_version", "fields"])?;
    if string(input, "schema_version")? != INPUT_SCHEMA_VERSION
        || string(input, "contract_version")? != APP_VERSION
        || string(source, "schema_version")? != SOURCE_SCHEMA_VERSION
    {
        return Err(Failure::SchemaVersionUnsupported);
    }

    let raw_fields = array(source, "fields")?;
    if raw_fields.len() > MAX_FIELDS {
        return Err(Failure::InputInvalid);
    }
    let mut fields: Vec<(&str, &str)> = Vec::with_capacity(raw_fields.len());
    let mut seen = BTreeSet::new();
    for raw in raw_fields {
        let entry = object(raw)?;
        exact_keys(entry, &["path", "value"])?;
        let path = string(entry, "path")?;
        let text = string(entry, "value")?;
        if !valid_path(path) || text.len() > MAX_VALUE_BYTES || !seen.insert(path) {
            return Err(Failure::InputInvalid);
        }
        fields.push((path, text));
    }
    for required in REQUIRED_FIELDS {
        if !seen.contains(required) {
            return Err(Failure::RequiredFieldMissing);
        }
    }
    if !seen.iter().any(|path| path.starts_with(REQUIRED_PREFIX)) {
        return Err(Failure::RequiredFieldMissing);
    }

    fields.sort_unstable_by(|left, right| left.0.cmp(right.0));
    let mut findings: Vec<(String, Category)> = Vec::new();
    let mut neutral_fields: Vec<&str> = Vec::new();
    for (index, (path, text)) in fields.iter().enumerate() {
        let categories = classify(path, text);
        if categories.is_empty() {
            neutral_fields.push(path);
            continue;
        }
        // Echo the path only when the path itself carries no forbidden shape.
        let reported = if path_is_echo_safe(path) {
            (*path).to_owned()
        } else {
            format!("fields[{index}]")
        };
        for category in categories {
            findings.push((reported.clone(), category));
        }
    }

    let finding_count = findings.len();
    let findings_truncated = finding_count > MAX_FINDINGS;
    let reported: Vec<Value> = findings
        .iter()
        .take(MAX_FINDINGS)
        .enumerate()
        .map(|(index, (path, category))| {
            json!({
                "ordinal": index + 1,
                "category": category.name(),
                "code": category.code(),
                "field_path": path,
            })
        })
        .collect();

    Ok(json!({
        "verdict": if finding_count == 0 { "neutral" } else { "non_neutral" },
        "field_count": fields.len(),
        "finding_count": finding_count,
        "findings_truncated": findings_truncated,
        "findings": reported,
        "neutral_fields": neutral_fields,
    }))
}

fn classify(path: &str, value: &str) -> Vec<Category> {
    let lower = value.to_ascii_lowercase();
    Category::ALL
        .into_iter()
        .filter(|category| match category {
            Category::ReadinessHint => readiness_hint(path, &lower),
            Category::TrackerIdentifier => tracker_identifier(path, &lower),
            Category::ExecutionCommand => execution_command(&lower),
            Category::SessionSidecarPath => session_sidecar_path(&lower),
            Category::CredentialToken => credential_token(value) || credential_token(path),
        })
        .collect()
}

const READINESS_KEYS: &[&str] = &[
    "ready",
    "ready_for_multica",
    "status",
    "pr_allowed",
    "release_allowed",
    "production_allowed",
    "work_owner",
    "authority",
    "route_mode",
];
const READINESS_MARKERS: &[&str] = &[
    "ready_for_multica",
    "pr_allowed",
    "release_allowed",
    "production_allowed",
    "work_owner",
];

fn readiness_hint(path: &str, lower: &str) -> bool {
    path.split('.')
        .any(|segment| READINESS_KEYS.contains(&segment))
        || READINESS_MARKERS
            .iter()
            .any(|marker| lower.contains(marker))
}

const TRACKER_KEYS: &[&str] = &[
    "assignee",
    "assignee_id",
    "issue_id",
    "issue_identifier",
    "workspace_id",
    "project_id",
    "parent_issue_id",
    "tracker",
    "identifier",
];
const TRACKER_MARKERS: &[&str] = &[
    "multica",
    "assignee",
    "workspace_id",
    "issue_id",
    "linear.app",
];

fn tracker_identifier(path: &str, lower: &str) -> bool {
    path.split('.')
        .any(|segment| TRACKER_KEYS.contains(&segment))
        || TRACKER_MARKERS.iter().any(|marker| lower.contains(marker))
        || has_issue_key(lower)
        || has_issue_key(path)
}

/// Detects `dot-<digits>` style tracker keys at a word boundary.
fn has_issue_key(lower: &str) -> bool {
    let bytes = lower.as_bytes();
    let mut index = 0;
    while let Some(offset) = lower[index..].find("dot-") {
        let start = index + offset;
        let digits_at = start + 4;
        let boundary = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        if boundary && bytes.get(digits_at).is_some_and(u8::is_ascii_digit) {
            return true;
        }
        index = digits_at;
    }
    false
}

const COMMAND_PROGRAMS: &[&str] = &[
    "cargo",
    "git",
    "gh",
    "npm",
    "pnpm",
    "yarn",
    "mise",
    "multica",
    "agentmesh",
    "pwsh",
    "powershell",
    "bash",
    "sh",
    "python",
    "curl",
];

fn execution_command(lower: &str) -> bool {
    if lower.contains("rerun") || lower.contains("re-run") || lower.contains("/skill:") {
        return true;
    }
    for line in lower.lines() {
        let candidate = line.trim_start_matches(['$', '>', '`', '-', '*', ' ', '\t']);
        if looks_like_command(candidate) {
            return true;
        }
    }
    let mut parts = lower.split('`');
    parts.next();
    parts.step_by(2).any(|span| looks_like_command(span.trim()))
}

fn looks_like_command(candidate: &str) -> bool {
    let mut tokens = candidate.split_whitespace();
    let first = tokens
        .next()
        .map(|token| token.trim_matches(['\'', '"', '`']));
    matches!(first, Some(program) if COMMAND_PROGRAMS.contains(&program)) && tokens.next().is_some()
}

const SESSION_MARKERS: &[&str] = &[
    "pi-sessions",
    "multica_workspaces",
    ".multica",
    "sidecar",
    "session_id",
    "sessions/",
    "sessions\\",
    "/workdir",
    "\\workdir",
];

fn session_sidecar_path(lower: &str) -> bool {
    SESSION_MARKERS.iter().any(|marker| lower.contains(marker))
        || lower.split_whitespace().any(|token| {
            let token = token.trim_matches(['\'', '"', '`', ',', ';', ')', '(']);
            token.ends_with(".jsonl") || token.ends_with(".log")
        })
}

fn credential_token(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if lower.contains("-----begin") || lower.contains("private key") {
        return true;
    }
    for assignment in ["api_key=", "apikey=", "secret=", "password=", "token="] {
        if lower.contains(assignment) {
            return true;
        }
    }
    let mut previous_bearer = false;
    for raw in value.split_whitespace() {
        let token = raw.trim_matches(['\'', '"', '`', ',', ';', ')', '(']);
        let token_lower = token.to_ascii_lowercase();
        if previous_bearer && token.len() >= 16 {
            return true;
        }
        previous_bearer = token_lower == "bearer";
        if [
            "ghp_",
            "gho_",
            "ghs_",
            "ghu_",
            "github_pat_",
            "xoxb-",
            "xoxp-",
        ]
        .iter()
        .any(|prefix| token_lower.starts_with(prefix))
            && token.len() >= 20
        {
            return true;
        }
        if token_lower.starts_with("sk-") && token.len() >= 20 {
            return true;
        }
        if token.len() == 20
            && token.starts_with("AKIA")
            && token
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            return true;
        }
    }
    false
}

fn path_is_echo_safe(path: &str) -> bool {
    !credential_token(path) && !has_issue_key(path)
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && path.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.')
        })
        && path.split('.').all(|segment| !segment.is_empty())
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
            "request_source_neutrality_neutral_input.json",
            "request_source_neutrality_neutral_expected.json",
        ),
        (
            "request_source_neutrality_tracker_leak_input.json",
            "request_source_neutrality_tracker_leak_expected.json",
        ),
        (
            "request_source_neutrality_execution_input.json",
            "request_source_neutrality_execution_expected.json",
        ),
        (
            "request_source_neutrality_malformed_input.json",
            "request_source_neutrality_malformed_expected.json",
        ),
        (
            "request_source_neutrality_missing_field_input.json",
            "request_source_neutrality_missing_field_expected.json",
        ),
        (
            "request_source_neutrality_unsupported_version_input.json",
            "request_source_neutrality_unsupported_version_expected.json",
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

    fn with_extra_field(path: &str, value: &str) -> Value {
        let mut input = load("request_source_neutrality_neutral_input.json");
        input["source"]["fields"]
            .as_array_mut()
            .unwrap()
            .push(json!({"path": path, "value": value}));
        input
    }

    fn categories_of(output: &Value) -> Vec<String> {
        output["neutrality"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|finding| finding["category"].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn recorded_fixtures_match_expected_payloads_and_repeat_byte_identically() {
        for (input_name, expected_name) in FIXTURES {
            let input = load(input_name);
            let expected = load(expected_name);
            let first = check_request_source_neutrality(&input);
            let second = check_request_source_neutrality(&input);
            assert_eq!(first, expected, "fixture {input_name}");
            assert_eq!(
                serde_json::to_vec(&first).unwrap(),
                serde_json::to_vec(&second).unwrap(),
                "fixture {input_name} changed across repeated runs"
            );
        }
    }

    #[test]
    fn field_order_in_input_does_not_change_the_report() {
        let input = load("request_source_neutrality_tracker_leak_input.json");
        let mut reversed = input.clone();
        reversed["source"]["fields"]
            .as_array_mut()
            .unwrap()
            .reverse();
        assert_eq!(
            serde_json::to_vec(&check_request_source_neutrality(&input)).unwrap(),
            serde_json::to_vec(&check_request_source_neutrality(&reversed)).unwrap()
        );
    }

    #[test]
    fn leaks_never_echo_matched_values() {
        let secret = format!("ghp_{}", "a1B2c3D4e5F6g7H8i9J0k1L2");
        for input in [
            load("request_source_neutrality_tracker_leak_input.json"),
            load("request_source_neutrality_execution_input.json"),
            with_extra_field("note", &format!("token {secret}")),
        ] {
            let output = serde_json::to_string(&check_request_source_neutrality(&input)).unwrap();
            assert!(!output.contains(&secret));
            for leaked in [
                "DOT-3474",
                "cargo test",
                "pi-sessions",
                "a.jsonl",
                "rerun",
                "assignee 01a1",
            ] {
                assert!(!output.contains(leaked), "output echoed {leaked}");
            }
        }
    }

    #[test]
    fn each_category_is_detected_by_its_own_code() {
        let secret = format!("sk-{}", "abcdefghijklmnopqrstuv");
        let cases: Vec<(String, &str)> = vec![
            ("ready_for_multica".to_owned(), "readiness_hint"),
            ("owner".to_owned(), "tracker_identifier"),
            ("verification".to_owned(), "execution_command"),
            ("evidence".to_owned(), "session_sidecar_path"),
            ("credential".to_owned(), "credential_token"),
        ];
        let values = [
            "true",
            "Assigned in DOT-1234",
            "`git push origin main`",
            "see logs/run-7.jsonl",
            &secret,
        ];
        for ((path, category), value) in cases.iter().zip(values) {
            let output = check_request_source_neutrality(&with_extra_field(path, value));
            assert_eq!(output["valid"], true, "{path}");
            assert_eq!(
                categories_of(&output),
                vec![(*category).to_owned()],
                "{path}"
            );
            assert_eq!(output["neutrality"]["verdict"], "non_neutral");
        }
    }

    #[test]
    fn ordinary_prose_is_not_flagged() {
        let output = check_request_source_neutrality(&with_extra_field(
            "notes",
            "Digit handling is digital, and the shell script catalog stays local.",
        ));
        assert_eq!(output["neutrality"]["verdict"], "neutral");
    }

    #[test]
    fn credential_shaped_path_is_reported_by_index_not_echoed() {
        let path = format!("ghp_{}", "a1b2c3d4e5f6g7h8i9j0k1l2");
        let output = check_request_source_neutrality(&with_extra_field(&path, "x"));
        let rendered = serde_json::to_string(&output).unwrap();
        assert!(!rendered.contains(&path));
        assert_eq!(categories_of(&output), vec!["credential_token"]);
    }

    #[test]
    fn findings_are_bounded_and_truncation_is_flagged() {
        let mut input = load("request_source_neutrality_neutral_input.json");
        let fields = input["source"]["fields"].as_array_mut().unwrap();
        for index in 0..(MAX_FINDINGS + 4) {
            fields.push(json!({"path": format!("leak.item{index}"), "value": "DOT-9"}));
        }
        let output = check_request_source_neutrality(&input);
        assert_eq!(output["neutrality"]["finding_count"], MAX_FINDINGS + 4);
        assert_eq!(output["neutrality"]["findings_truncated"], true);
        assert_eq!(
            output["neutrality"]["findings"].as_array().unwrap().len(),
            MAX_FINDINGS
        );
    }

    #[test]
    fn oversized_input_returns_only_a_normalized_failure() {
        let mut input = load("request_source_neutrality_neutral_input.json");
        input["padding"] = Value::String("x".repeat(MAX_INPUT_BYTES + 1));
        let output = check_request_source_neutrality(&input);
        assert_eq!(output["valid"], false);
        assert_eq!(output["neutrality"], Value::Null);
        assert_eq!(output["errors"][0]["code"], INPUT_TOO_LARGE);
    }

    #[test]
    fn duplicate_or_invalid_paths_fail_closed_without_partial_verdict() {
        for path in ["title", "Bad Path", "a..b", ""] {
            let output = check_request_source_neutrality(&with_extra_field(path, "x"));
            assert_eq!(output["valid"], false, "{path:?}");
            assert_eq!(output["neutrality"], Value::Null);
            assert_eq!(output["errors"][0]["code"], INPUT_INVALID);
        }
    }
}
