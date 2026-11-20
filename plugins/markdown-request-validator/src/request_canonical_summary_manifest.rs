//! Deterministic request canonical summary manifest App.
//!
//! The App projects a validated request into bounded digests and a same-scope
//! identity without returning request prose or adapter-owned source fields.

use crate::adapter_compact_helpers::sort_json_keys;
use crate::request_markdown_normalizer::normalize_request_markdown;
use agentmesh_evidence::sha256_prefixed;
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Contract version emitted by this App.
pub const CANONICAL_SUMMARY_MANIFEST_VERSION: &str = "agentmesh-request-canonical-summary.v0";
const INPUT_SCHEMA_VERSION: &str = "request-canonical-summary-manifest-input.v0";
const OUTPUT_SCHEMA_VERSION: &str = "request-canonical-summary-manifest-compact.v0";
const NORMALIZER_INPUT_SCHEMA_VERSION: &str = "request-markdown-normalizer-input.v0";
const REQUEST_SCHEMA_VERSION: &str = "agentmesh-request.v0";
const MAX_MARKDOWN_BYTES: usize = 64 * 1024;

const ERROR_INPUT_INVALID: &str = "AGENTMESH_REQUEST_CANONICAL_SUMMARY_INPUT_INVALID";
const ERROR_SCHEMA_UNSUPPORTED: &str = "AGENTMESH_REQUEST_CANONICAL_SUMMARY_SCHEMA_UNSUPPORTED";
const ERROR_SOURCE_TOO_LARGE: &str = "AGENTMESH_REQUEST_CANONICAL_SUMMARY_SOURCE_TOO_LARGE";
const ERROR_MALFORMED_MARKDOWN: &str = "AGENTMESH_REQUEST_CANONICAL_SUMMARY_MARKDOWN_MALFORMED";
const ERROR_UNSUPPORTED_REQUEST: &str = "AGENTMESH_REQUEST_CANONICAL_SUMMARY_REQUEST_UNSUPPORTED";
const ERROR_INVALID_REQUEST: &str = "AGENTMESH_REQUEST_CANONICAL_SUMMARY_REQUEST_INVALID";
const ERROR_SCOPE_MISSING: &str = "AGENTMESH_REQUEST_CANONICAL_SUMMARY_SCOPE_MISSING";
const ERROR_SCOPE_AMBIGUOUS: &str = "AGENTMESH_REQUEST_CANONICAL_SUMMARY_SCOPE_AMBIGUOUS";
const ERROR_SCOPE_INVALID: &str = "AGENTMESH_REQUEST_CANONICAL_SUMMARY_SCOPE_INVALID";

/// Build one deterministic canonical summary manifest from a Markdown request.
pub fn canonical_summary_manifest(value: &Value) -> Value {
    let Some(input) = value.as_object() else {
        return compact(None, vec![ERROR_INPUT_INVALID]);
    };
    if input
        .keys()
        .any(|key| key != "schema_version" && key != "markdown")
    {
        return compact(None, vec![ERROR_INPUT_INVALID]);
    }

    match input.get("schema_version").and_then(Value::as_str) {
        Some(INPUT_SCHEMA_VERSION) => {}
        Some(_) => return compact(None, vec![ERROR_SCHEMA_UNSUPPORTED]),
        None => return compact(None, vec![ERROR_INPUT_INVALID]),
    }

    let Some(markdown) = input.get("markdown").and_then(Value::as_str) else {
        return compact(None, vec![ERROR_INPUT_INVALID]);
    };
    if markdown.trim().is_empty() {
        return compact(None, vec![ERROR_INPUT_INVALID]);
    }
    if markdown.len() > MAX_MARKDOWN_BYTES {
        return compact(None, vec![ERROR_SOURCE_TOO_LARGE]);
    }

    let normalized = normalize_request_markdown(&json!({
        "schema_version": NORMALIZER_INPUT_SCHEMA_VERSION,
        "request_schema_version": REQUEST_SCHEMA_VERSION,
        "markdown": markdown,
    }));
    if normalized.get("valid") != Some(&Value::Bool(true)) {
        return compact(None, normalize_request_errors(&normalized["errors"]));
    }

    let projection = &normalized["projection"];
    let fields = &projection["fields"];
    let request_kind = fields["request_kind"].as_str().unwrap_or_default();
    let scope_key = match extract_scope_key(&projection["sections"]) {
        Ok(scope_key) => scope_key,
        Err(code) => return compact(None, vec![code]),
    };
    if !valid_scope_key(&scope_key, request_kind) {
        return compact(None, vec![ERROR_SCOPE_INVALID]);
    }

    let Some(normalized_markdown) = projection["normalized_markdown"].as_str() else {
        return compact(None, vec![ERROR_INVALID_REQUEST]);
    };
    if normalized_markdown.len() > MAX_MARKDOWN_BYTES {
        return compact(None, vec![ERROR_SOURCE_TOO_LARGE]);
    }

    let title = fields["title"].as_str().unwrap_or_default();
    let summary = section_body(&projection["sections"], "What to build").unwrap_or_default();
    let criteria: Vec<String> = projection["requirements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["text"].as_str().map(ToString::to_string))
        .collect();
    let identity = json!({
        "project_key": fields["project_key"],
        "request_kind": request_kind,
        "same_scope_key": scope_key,
        "sequence_index": fields["sequence_index"],
        "sequence_total": fields["sequence_total"],
        "source_prd": fields["source_prd"],
    });

    let manifest = json!({
        "common_fields": {
            "request_identity_sha256": sha256_prefixed(&serde_json::to_vec(&identity).expect("identity JSON")),
            "request_kind": request_kind,
            "same_scope_key": scope_key,
            "normalized_title_sha256": sha256_prefixed(title.as_bytes()),
            "normalized_summary_sha256": sha256_prefixed(summary.as_bytes()),
            "acceptance_criteria_digests": criteria.iter().map(|text| sha256_prefixed(text.as_bytes())).collect::<Vec<_>>(),
            "acceptance_criteria_count": criteria.len(),
            "source_sha256": sha256_prefixed(normalized_markdown.as_bytes()),
        },
        "adapter_owned_fields": {},
    });
    compact(Some(manifest), Vec::new())
}

fn compact(manifest: Option<Value>, codes: Vec<&str>) -> Value {
    let mut normalized_codes: Vec<_> = codes
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    normalized_codes.sort_unstable();
    let errors: Vec<_> = normalized_codes
        .iter()
        .map(|code| json!({"code": code, "severity": "error"}))
        .collect();
    sort_json_keys(json!({
        "schema_version": OUTPUT_SCHEMA_VERSION,
        "contract_version": CANONICAL_SUMMARY_MANIFEST_VERSION,
        "valid": manifest.is_some(),
        "manifest": manifest,
        "error_count": errors.len(),
        "errors": errors,
    }))
}

fn normalize_request_errors(errors: &Value) -> Vec<&'static str> {
    let mut has_unsupported = false;
    let mut has_malformed = false;
    for error in errors.as_array().into_iter().flatten() {
        match error["code"].as_str() {
            Some("AGENTMESH_REQUEST_MARKDOWN_NORMALIZER_UNSUPPORTED_SHAPE")
            | Some("AGENTMESH_REQUEST_MARKDOWN_NORMALIZER_SECTION_UNSUPPORTED") => {
                has_unsupported = true;
            }
            Some("AGENTMESH_REQUEST_MARKDOWN_NORMALIZER_FRONTMATTER_MALFORMED")
            | Some("AGENTMESH_REQUEST_MARKDOWN_NORMALIZER_CHECKLIST_MALFORMED") => {
                has_malformed = true;
            }
            _ => {}
        }
    }
    if has_unsupported {
        vec![ERROR_UNSUPPORTED_REQUEST]
    } else if has_malformed {
        vec![ERROR_MALFORMED_MARKDOWN]
    } else {
        vec![ERROR_INVALID_REQUEST]
    }
}

fn section_body<'a>(sections: &'a Value, heading: &str) -> Option<&'a str> {
    sections
        .as_array()?
        .iter()
        .find(|section| section["heading"].as_str() == Some(heading))?
        .get("body")?
        .as_str()
}

fn extract_scope_key(sections: &Value) -> Result<String, &'static str> {
    let Some(notes) = section_body(sections, "Notes") else {
        return Err(ERROR_SCOPE_MISSING);
    };
    let mut found = Vec::new();
    for line in notes.lines() {
        let line = line.trim();
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .or_else(|| line.strip_prefix("+ "))
            .unwrap_or(line)
            .trim();
        let Some((label, value)) = line.split_once(':') else {
            continue;
        };
        if label.trim().eq_ignore_ascii_case("same scope") {
            let value = value
                .trim()
                .trim_matches('`')
                .trim_matches('"')
                .trim_matches('\'');
            found.push(value.to_string());
        }
    }
    match found.len() {
        0 => Err(ERROR_SCOPE_MISSING),
        1 => Ok(found.remove(0)),
        _ => Err(ERROR_SCOPE_AMBIGUOUS),
    }
}

fn valid_scope_key(scope: &str, request_kind: &str) -> bool {
    if scope.len() > 200 || !scope.is_ascii() {
        return false;
    }
    let expected_prefix = format!("agentmesh:{request_kind}:");
    let Some(suffix) = scope.strip_prefix(&expected_prefix) else {
        return false;
    };
    let mut parts = suffix.split(':');
    let Some(slug) = parts.next() else {
        return false;
    };
    let version = parts.next();
    if parts.next().is_some() || !valid_scope_slug(slug) {
        return false;
    }
    version.is_none_or(|version| {
        version.strip_prefix('v').is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
    })
}

fn valid_scope_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.split(['-', '_', '.']).all(|segment| {
            let mut bytes = segment.bytes();
            bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
                && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(markdown: &str) -> Value {
        json!({
            "schema_version": INPUT_SCHEMA_VERSION,
            "markdown": markdown,
        })
    }

    fn markdown() -> &'static str {
        "---\ntitle: \"Add a deterministic request canonical summary manifest App\"\nrequest_kind: app\nissue_type: AFK\nstatus: ready\nproject_key: agentmesh-private\nsource_prd: \"synthetic://requests/request-canonical-summary-manifest\"\nsource_design: synthetic://docs/agentmesh-request-operations-v1\nsource_roadmap: synthetic://roadmaps/agentmesh-private\nblocked_by: []\nunblocks: []\nsequence_index: 1\nsequence_total: 1\n---\n# Add a deterministic request canonical summary manifest App\n\n## What to build\nBuild one tool-neutral canonical summary from each validated request.\n\n## Acceptance criteria\n- [ ] Emit a stable bounded source digest.\n- [ ] Keep adapter-owned fields separate.\n\n## Blocked by\n- None.\n\n## User stories covered\n- As an adapter maintainer, use the same summary downstream.\n\n## Notes\n- Same scope: `agentmesh:app:request-canonical-summary-manifest`\n"
    }

    fn equivalent_markdown() -> &'static str {
        "---\nsequence_total: 1\nsequence_index: 1\nunblocks: []\nblocked_by: []\nsource_roadmap: synthetic://roadmaps/agentmesh-private\nsource_design: synthetic://docs/agentmesh-request-operations-v1\nsource_prd: synthetic://requests/request-canonical-summary-manifest\nproject_key: agentmesh-private\nstatus: ready\nissue_type: AFK\nrequest_kind: app\ntitle: Add   a deterministic request canonical summary manifest App\n---\r\n## Notes\r\n* Same scope: `agentmesh:app:request-canonical-summary-manifest`\r\n\r\n## User stories covered\r\n* As an adapter maintainer, use the same summary downstream.\r\n\r\n## Blocked by\r\n* None.\r\n\r\n## Acceptance criteria\r\n* Keep adapter-owned fields separate.\r\n* [ ] Emit a stable bounded source digest.\r\n\r\n## What to build\r\nBuild   one tool-neutral canonical summary from each validated request.\r\n"
    }

    #[test]
    fn successful_and_equivalent_requests_are_byte_identical_and_bounded() {
        let first = canonical_summary_manifest(&input(markdown()));
        let second = canonical_summary_manifest(&input(equivalent_markdown()));
        assert_eq!(first["valid"], true, "{first}");
        assert_eq!(first, second);
        assert_eq!(
            serde_json::to_vec(&first).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
        assert_eq!(first["manifest"]["common_fields"]["request_kind"], "app");
        assert_eq!(
            first["manifest"]["common_fields"]["same_scope_key"],
            "agentmesh:app:request-canonical-summary-manifest"
        );
        assert_eq!(
            first["manifest"]["common_fields"]["acceptance_criteria_count"],
            2
        );
        assert_eq!(first["manifest"]["adapter_owned_fields"], json!({}));
        let output = serde_json::to_string(&first).unwrap();
        for secret in [
            "Build one tool-neutral",
            "Emit a stable",
            "synthetic://",
            "agentmesh-private",
        ] {
            assert!(!output.contains(secret));
        }
    }

    #[test]
    fn malformed_unsupported_and_oversized_inputs_have_fixed_errors_without_manifest() {
        let malformed = canonical_summary_manifest(&input("---\ntitle: [bad\n---\n"));
        assert_eq!(malformed["manifest"], Value::Null);
        assert_eq!(malformed["errors"][0]["code"], ERROR_MALFORMED_MARKDOWN);

        let unsupported =
            canonical_summary_manifest(&input(&markdown().replace("## Notes", "## Unexpected")));
        assert_eq!(unsupported["manifest"], Value::Null);
        assert_eq!(unsupported["errors"][0]["code"], ERROR_UNSUPPORTED_REQUEST);

        let oversized = canonical_summary_manifest(&input(&"x".repeat(MAX_MARKDOWN_BYTES + 1)));
        assert_eq!(oversized["manifest"], Value::Null);
        assert_eq!(oversized["errors"][0]["code"], ERROR_SOURCE_TOO_LARGE);
    }

    #[test]
    fn scope_is_required_unique_and_non_path_like() {
        let missing = canonical_summary_manifest(&input(&markdown().replace(
            "- Same scope: `agentmesh:app:request-canonical-summary-manifest`\n",
            "",
        )));
        assert_eq!(missing["errors"][0]["code"], ERROR_SCOPE_MISSING);

        let duplicate = canonical_summary_manifest(&input(&markdown().replace(
            "- Same scope: `agentmesh:app:request-canonical-summary-manifest`",
            "- Same scope: `agentmesh:app:request-canonical-summary-manifest`\n- Same scope: `agentmesh:app:another`",
        )));
        assert_eq!(duplicate["errors"][0]["code"], ERROR_SCOPE_AMBIGUOUS);

        assert!(!valid_scope_key("agentmesh:app:C:\\private", "app"));
        assert!(!valid_scope_key("agentmesh:repair:app:one", "app"));
        assert!(valid_scope_key(
            "agentmesh:repair:repo_main_behind:v10",
            "repair"
        ));
        assert!(!valid_scope_key("agentmesh:app:jira-123", "app"));
        assert!(!valid_scope_key("agentmesh:app:scope-123", "app"));
        let tracker_scope = canonical_summary_manifest(&input(&markdown().replace(
            "agentmesh:app:request-canonical-summary-manifest",
            "agentmesh:app:jira-123",
        )));
        let expected: Value = serde_json::from_str(include_str!(
            "../testdata/expected_request_canonical_summary_manifest_tracker_scope_payload.json"
        ))
        .unwrap();
        assert_eq!(tracker_scope, expected);
    }

    #[test]
    fn fixtures_match_the_documented_json_contract() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata");
        for (input_name, expected_name) in [
            (
                "request_canonical_summary_manifest_success_input.json",
                "expected_request_canonical_summary_manifest_success_payload.json",
            ),
            (
                "request_canonical_summary_manifest_equivalent_input.json",
                "expected_request_canonical_summary_manifest_equivalent_payload.json",
            ),
            (
                "request_canonical_summary_manifest_malformed_input.json",
                "expected_request_canonical_summary_manifest_malformed_payload.json",
            ),
            (
                "request_canonical_summary_manifest_tracker_scope_input.json",
                "expected_request_canonical_summary_manifest_tracker_scope_payload.json",
            ),
        ] {
            let input: Value =
                serde_json::from_slice(&std::fs::read(root.join(input_name)).unwrap()).unwrap();
            let expected: Value =
                serde_json::from_slice(&std::fs::read(root.join(expected_name)).unwrap()).unwrap();
            assert_eq!(
                canonical_summary_manifest(&input),
                expected,
                "{input_name} should match {expected_name}"
            );
        }
    }
}
