# Request canonical summary manifest v0

`apps/request-canonical-summary-manifest/agentmesh-app.toml` declares a pure, one-shot App that accepts one bounded AgentMesh Markdown request. It runs the shared request Markdown normalizer first and emits a stable common summary plus a separate, empty `adapter_owned_fields` object. It has no tracker-specific input fields, filesystem access, adapter calls, or Multica dependency.

## Input and output

The only input fields are `schema_version` and one complete request `markdown` string. Inputs are capped at 64 KiB UTF-8. The common output fields are:

- `request_identity_sha256`: SHA-256 of canonical JSON containing the project key, request kind, same-scope key, sequence values, and source request reference. Raw source references are never returned.
- `request_kind` and `same_scope_key`: the validated request kind and the `Same scope:` key from its Notes section.
- `normalized_title_sha256` and `normalized_summary_sha256`: SHA-256 of the normalized title and normalized `What to build` section body.
- `acceptance_criteria_digests` and `acceptance_criteria_count`: criterion-text digests in the shared normalizer's deterministic canonical order.
- `source_sha256`: SHA-256 of the shared normalizer's canonical Markdown output, not the raw bytes. This makes harmless Markdown formatting variants produce the same bounded source digest while preserving a stable fingerprint of the validated request projection.

Only hashes, the validated request kind, and the non-tracker same-scope key are emitted. Request body text, evidence values, source references, credentials, tracker identifiers, logs, and local/session paths are never echoed. The fixed `contract_version` names the summary contract. If parsing or scope validation fails, `manifest` is null and `errors` contains only normalized codes; no partial summary is returned.

## Adapter reuse

Markdown and non-Multica adapters can run this App once, retain the resulting canonical summary manifest, and supply its common fields as bounded input to downstream admission, receipt, lineage, replay, or materialization checks. Adapter-owned values remain in their own envelope and do not alter the stable common contract. An adapter must not add Multica IDs, credentials, tracker state, or local source paths to this manifest; it can associate adapter metadata outside the common fields and pass the same retained manifest to each downstream check.

## Fixtures

The fixture inputs and expected outputs are documented in `plugins/markdown-request-validator/testdata/request_canonical_summary_manifest_fixtures.md`. The successful Markdown input and a CRLF/reordered/whitespace variant have identical compact JSON output; malformed input returns a stable error code and no manifest.

Development validation:

```bash
cargo test -p agentmesh-markdown-request-validator request_canonical_summary_manifest
agentmesh app validate \
  --manifest apps/request-canonical-summary-manifest/agentmesh-app.toml \
  --toolchain-pin toolchains/agentmesh-pin.v0.example.toml
```
