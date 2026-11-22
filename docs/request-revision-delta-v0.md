# Request Revision Delta v0

`apps/request-revision-delta/agentmesh-app.toml` declares a tool-neutral,
one-shot App that compares two validated canonical request summaries for the
same scope and emits a deterministic revision delta. It answers one question for
a local runner or non-Multica adapter: did the contract change between these two
revisions, or only adapter-owned metadata? It never reads or overwrites files,
renders Markdown, contacts a tracker, discovers an adapter, or invokes a
command.

## Input contract

Input uses the explicit `agentmesh-request-revision-delta.v0` contract. The
root carries `schema_version` (`request-revision-delta-input.v0`),
`contract_version`, a `base` summary, and a `revised` summary. Each summary
repeats the contract version and carries:

- `schema_version`: `request-revision-summary.v0`
- `scope_key`: an `agentmesh:` scope key; both summaries must carry the same one
- `canonical_fields`: the stable contract fields, in this fixed order:
  `title`, `request_kind`, `body_summary` (optional), `acceptance_criteria`, and
  `blocked_by` (optional). Values are strings or lists of strings (at most 32
  items, 1024 bytes each).
- `adapter_extras`: adapter-owned metadata keyed by a lowercase path such as
  `adapter.label` (at most 32 entries). Values may be any JSON value.

Unknown fields fail closed. The App caps canonicalized input at 32 KiB; the
manifest's host raw-input limit is 64 KiB. The summaries are supplied by the
caller after validation; this App does not re-validate request Markdown.

## Output

The compact output (`request-revision-delta-compact.v0`) contains:

| Field | Meaning |
| --- | --- |
| `contract_affecting` | `true` only when at least one canonical field is `added`, `removed`, or `changed` |
| `canonical_changes` | all five canonical fields in fixed order, each with `change_class` (`added`, `removed`, `changed`, `unchanged`) and `base_digest` / `revised_digest` (`sha256:` of the canonical JSON of that field value, `null` when absent) |
| `adapter_extra_changes` | the union of adapter extra paths, sorted by path, each with `change_class` only |
| `diagnostics` | at most two codes: `no_change`, `canonical_contract_change`, `adapter_extras_changed` |
| `error_count` / `errors` | `0` and empty on success |

Adapter extras are reported by field path and change class only. Their values
are never echoed and never hashed, so a credential, tracker identifier, or
sidecar path stored in an extra cannot leak into the delta. Canonical fields are
reported by digest, never by value. The scope key is not echoed. Two identical
summaries produce a byte-identical delta with every class `unchanged` and
`contract_affecting: false`.

Failures return one normalized error and no partial delta (`valid: false`,
empty change lists, `contract_affecting: null`): `INPUT_TOO_LARGE`,
`MALFORMED_INPUT`, `UNSUPPORTED_SCHEMA_VERSION`, `UNSUPPORTED_CONTRACT_VERSION`,
`REQUIRED_FIELD_MISSING`, `INVALID_FIELD`, and `CROSS_SCOPE_INPUT` (the two
summaries carry different scope keys).

## Adapter and runner procedure

1. Produce a summary for the previously materialized revision and for the edited
   source, using the adapter's own validation. Pass both with the same scope key.
2. Run the App. On any error, stop: fix the input; there is no partial delta to
   act on.
3. If `contract_affecting` is `false` and `diagnostics` is `["no_change"]`, the
   revision is identical; do nothing.
4. If `contract_affecting` is `false` and `diagnostics` is
   `["adapter_extras_changed"]`, the edit touched only adapter-owned metadata.
   Regeneration and re-materialization are not required by the contract; refresh
   the adapter's own metadata if it needs it.
5. If `contract_affecting` is `true`, a canonical field changed. Read
   `canonical_changes` to see which (for example `acceptance_criteria`). The
   downstream artifact needs regeneration, and re-materialization follows once
   the revised source is admitted. Feed the resulting admission, receipt, and
   lineage records into the re-materialization decision App
   ([`request-rematerialization-decision-v0.md`](request-rematerialization-decision-v0.md))
   to decide `regenerate`, `skip`, or `hold`.
6. Store the delta beside the artifact using the adapter's own storage
   convention. The App performs no I/O.

## Fixtures

Fixtures under `plugins/markdown-request-validator/testdata/` pair JSON inputs
with exact expected outputs:

| Case | JSON input | Expected delta output |
| --- | --- | --- |
| Identical pair: all unchanged, not contract-affecting | `request_revision_delta_identical_input.json` | `request_revision_delta_identical_expected.json` |
| Canonical acceptance-criterion edit: contract-affecting | `request_revision_delta_canonical_criterion_edit_input.json` | `request_revision_delta_canonical_criterion_edit_expected.json` |
| Adapter-extra-only edit (changed, added, removed): not contract-affecting | `request_revision_delta_adapter_extra_only_input.json` | `request_revision_delta_adapter_extra_only_expected.json` |
| Cross-scope pair: `CROSS_SCOPE_INPUT` | `request_revision_delta_cross_scope_input.json` | `request_revision_delta_cross_scope_expected.json` |
| Malformed pair (missing required `title`, non-list criteria): `REQUIRED_FIELD_MISSING` | `request_revision_delta_malformed_input.json` | `request_revision_delta_malformed_expected.json` |

Tests also cover added/removed optional canonical fields, unsupported schema and
contract versions, unknown canonical fields, bounded extras, oversized input,
value redaction, and byte-identical repeated output.

## Development smoke

```bash
cargo build -p agentmesh-cli -p agentmesh-markdown-request-validator --bins
agentmesh app validate \
  --manifest apps/request-revision-delta/agentmesh-app.toml \
  --toolchain-pin toolchains/agentmesh-pin.v0.example.toml
agentmesh app run \
  --manifest apps/request-revision-delta/agentmesh-app.toml \
  --toolchain-pin toolchains/agentmesh-pin.v0.example.toml \
  --input plugins/markdown-request-validator/testdata/request_revision_delta_canonical_criterion_edit_input.json \
  --sidecar-dir .agentmesh/runs \
  --mode development \
  --dev-plugin /absolute/path/to/target/debug/agentmesh-request-revision-delta
```
