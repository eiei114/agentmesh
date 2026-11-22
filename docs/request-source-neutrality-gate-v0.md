# Request Source Neutrality Gate App v0

`request-source-neutrality-gate` is a pure, one-shot check that a canonical
request source stays tool-neutral. It scans one validated source summary for
content that belongs to a downstream tool and returns a bounded verdict with
normalized finding codes and field paths only. It does not call a tracker,
discover adapters, rewrite the source, echo matched values, or read anything
outside its declared input. Its contracts are in
`apps/request-source-neutrality-gate/schemas/`.

## Input contract

The input carries explicit `request-source-neutrality.v0` and
`request-source-neutrality-input.v0` versions and one
`request-source-neutrality-source.v0` source: a list of at most 64
`{path, value}` fields. A `path` is a lowercase dotted key
(`[a-z0-9_-]` segments, at most 96 bytes, unique); a `value` is text of at most
4096 bytes. The whole input is limited to 64 KiB.

The source must contain the neutral fields `title`, `scope`, `request_kind`,
`problem`, and at least one `acceptance_criteria.<n>` field.

## Forbidden categories and codes

Each field is checked by path (key names) and value. A finding is one
`(field path, category)` pair.

| Category | Code | Covers |
|---|---|---|
| `readiness_hint` | `AGENTMESH_REQUEST_SOURCE_NEUTRALITY_READINESS_HINT` | readiness or authority keys and markers (`status`, `ready_for_multica`, `pr_allowed`, `release_allowed`, `production_allowed`, `work_owner`, ...) |
| `tracker_identifier` | `AGENTMESH_REQUEST_SOURCE_NEUTRALITY_TRACKER_IDENTIFIER` | orchestrator or tracker names, assignee/workspace/issue identifiers, `DOT-<n>` style keys |
| `execution_command` | `AGENTMESH_REQUEST_SOURCE_NEUTRALITY_EXECUTION_COMMAND` | shell or tool command lines, inline command spans, rerun instructions |
| `session_sidecar_path` | `AGENTMESH_REQUEST_SOURCE_NEUTRALITY_SESSION_SIDECAR_PATH` | session, sidecar, workspace and raw log paths (`.jsonl`, `.log`, ...) |
| `credential_token` | `AGENTMESH_REQUEST_SOURCE_NEUTRALITY_CREDENTIAL_TOKEN` | credential-shaped tokens and secret assignments |

The detectors are deterministic substring and token rules; there is no
network, file or tracker lookup.

## Output

A valid run returns `valid: true` and a `neutrality` object with `verdict`
(`neutral` or `non_neutral`), `field_count`, `finding_count`,
`findings_truncated`, `findings`, and `neutral_fields`. Findings are ordered by
field path then category and carry `ordinal`, `category`, `code`, and
`field_path` only. At most 32 findings are listed; `finding_count` still reports
the full number and `findings_truncated` flags the cut. `neutral_fields` lists
the sorted paths with no finding. Matched values never appear. If a field path
itself looks like a credential or tracker key, it is reported as `fields[<n>]`
instead of being echoed.

Input field order does not change the output, and identical input serializes to
byte-identical output.

Malformed input, a duplicate or invalid path, an input above 64 KiB, an
unsupported schema or contract version, and a missing required neutral field
fail closed with `valid: false`, `neutrality: null`, and one normalized error
code: `..._INPUT_INVALID`, `..._INPUT_TOO_LARGE`, `..._SCHEMA_VERSION_UNSUPPORTED`,
or `..._REQUIRED_FIELD_MISSING`. There is never a partial verdict.

## Applying the gate in a local runner or non-Multica adapter

Run the gate on the canonical source summary immediately before the adapter
materializes its own downstream artifact (a local file, a tracker issue, or any
other record):

1. Project the canonical request into the `path`/`value` field list. Do not add
   adapter-only or tracker metadata to it.
2. Invoke the App. If `valid` is `false`, fix the input and rerun; there is no
   verdict to act on.
3. Proceed only when `neutrality.verdict` is `neutral`. For `non_neutral`, show
   a reviewer the ordered `category` and `field_path` pairs, repair the source,
   and rerun. Do not copy source text into the review record.
4. Only after a neutral verdict, continue to the next stage, for example the
   artifact admission gate, which accepts a neutrality verdict as one of its
   inputs.

The gate does not rewrite the source, so the repair happens in the source
document, never in the adapter.

## Recorded fixtures

Each input/expected-output pair is exercised by the Rust conformance test and
lives under `plugins/markdown-request-validator/testdata/`.

| Case | Input | Expected output |
|---|---|---|
| fully neutral request | `request_source_neutrality_neutral_input.json` | `request_source_neutrality_neutral_expected.json` |
| tracker identifier, readiness hint, session path | `request_source_neutrality_tracker_leak_input.json` | `request_source_neutrality_tracker_leak_expected.json` |
| execution instructions | `request_source_neutrality_execution_input.json` | `request_source_neutrality_execution_expected.json` |
| malformed input | `request_source_neutrality_malformed_input.json` | `request_source_neutrality_malformed_expected.json` |
| missing required field | `request_source_neutrality_missing_field_input.json` | `request_source_neutrality_missing_field_expected.json` |
| unsupported schema version | `request_source_neutrality_unsupported_version_input.json` | `request_source_neutrality_unsupported_version_expected.json` |

The unit tests additionally cover every category, credential-shaped values and
paths (built at test time so no secret-like literal is stored), finding
truncation, field-order independence, oversized input, and invalid paths.
