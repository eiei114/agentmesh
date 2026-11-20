# Request Materialization Receipt v0

## Purpose

`apps/request-materialization-receipt/agentmesh-app.toml` declares a tool-neutral,
one-shot App for checking a written artifact against the admission record and
canonical request summary that authorized it. The App consumes only supplied
scalars and digests. It does not re-read a source request or artifact, rewrite
files, call a tracker, or discover adapters.

## Input and output

The input contains an explicit `contract_version`, the admission gate's record,
the canonical summary, and one adapter descriptor. The descriptor records the
adapter identity, artifact kind, artifact digest, and the source digest recorded
with the artifact. That recorded source digest is required to prove the
post-write source binding; the App does not derive it from a path or file.
Input scope keys are limited to 256 ASCII characters. The host accepts at most
131,072 raw UTF-8 JSON bytes; before field parsing, the App also limits the
compact, key-sorted JSON representation to 65,536 bytes. Thus JSON whitespace
does not increase the App's semantic-size measurement. The host's raw byte
limit runs first, then the App's compact-size check, root required fields,
schema and contract versions, nested required fields, scalar validation, and
scope validation. Each App-level failure is a single normalized code with no
partial receipt.

```json
{
  "schema_version": "request-materialization-receipt-input.v0",
  "contract_version": "agentmesh-request-materialization-receipt.v0",
  "admission_record": {
    "verdict": "admit",
    "scope_key": "agentmesh:app:example",
    "request_kind": "app",
    "title_digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "criteria_count": 3,
    "source_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  },
  "request_summary": {
    "scope_key": "agentmesh:app:example",
    "request_kind": "app",
    "title_digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "criteria_count": 3,
    "source_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  },
  "artifact": {
    "adapter_id": "markdown_adapter",
    "artifact_kind": "markdown",
    "artifact_digest": "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
    "source_digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  }
}
```

The compact receipt has one fixed-order comparison for each field: `scope_key`,
`request_kind`, `title_digest`, `criteria_count`, `admission_verdict`,
`source_digest`, and `artifact_digest`. Every result includes its one-based
ordinal, a SHA-256 digest of the compared supplied values, and a normalized
code. The receipt is `corresponded` only when the admission verdict is `admit`,
the admission and summary fields agree, and all three supplied source digests
agree. A deferred verdict or field drift produces a `drifted` receipt with one
reason code per failing field, in field order. Scope keys outside the AgentMesh
namespace are rejected as `CROSS_SCOPE_INPUT`.

Malformed, missing, unsupported-version, oversized, invalid, and cross-scope
inputs return a normalized error and `receipt: null`; failures never contain a
partial receipt. Output does not echo request text, evidence values, adapter
identity, artifact kind, credentials, logs, filesystem paths, or tracker IDs.
The input schema's `scope_key.maxLength` matches the implementation's 256-byte
ASCII scope limit. `criteria_count` accepts any JSON number that is an integer
value (including `3.0`); it is normalized to an integer before comparison and
digesting.

### Compared-value digest recipe

For each field, form the following JSON array in exactly this operand order:

| Ordinal | Field | Array operands |
| --- | --- | --- |
| 1 | `scope_key` | `[admission_record.scope_key, request_summary.scope_key]` |
| 2 | `request_kind` | `[admission_record.request_kind, request_summary.request_kind]` |
| 3 | `title_digest` | `[admission_record.title_digest, request_summary.title_digest]` |
| 4 | `criteria_count` | `[admission_record.criteria_count, request_summary.criteria_count]` |
| 5 | `admission_verdict` | `[admission_record.verdict, "admit"]` |
| 6 | `source_digest` | `[admission_record.source_digest, request_summary.source_digest, artifact.source_digest]` |
| 7 | `artifact_digest` | `[artifact.artifact_digest]` |

`compared_values_digest` is `sha256:` followed by the lowercase SHA-256 hex of
the array's UTF-8 JSON bytes: compact JSON, no insignificant whitespace, array
order as listed, string values JSON-escaped, and integer counts written in base
10 without a decimal point. The inputs to these arrays are ASCII by contract.
For the artifact itself, adapters hash its exact written bytes (not a parsed or
reformatted representation) as `sha256:` plus lowercase SHA-256 hex.

## Adapter and reviewer procedure

1. After writing the artifact, hash its exact bytes and retain that artifact
   digest and the original source digest with the adapter's materialization
   record. Do not put a file path or tracker identifier in the descriptor.
2. Run this App once with the admission record as supplied, the retained
   canonical summary, and that artifact descriptor. Store the bounded input
   descriptor and JSON receipt beside the artifact using the adapter's storage
   convention; neither contains request body text or tracker metadata, and the
   App itself does not write files.
3. A reviewer checks `receipt.verdict`, then reads `fields` and
   `reason_codes` in ordinal order. To ensure the receipt still describes the
   current write, the reviewer hashes the artifact's current raw bytes and
   computes the ordinal-7 compared-value digest as described above; it must
   equal `fields[6].compared_values_digest`. A mismatch means the artifact was
   replaced or altered after the receipt and the receipt must not be trusted. A
   `drifted` receipt identifies the failing comparisons without disclosing
   request body text or tracker metadata. A null receipt means the input was
   invalid and must not be treated as correspondence evidence.

Fixtures in `plugins/markdown-request-validator/testdata/` pair each documented
JSON input with its exact expected compact output: successful correspondence,
criteria-count drift, source-digest drift, deferred admission, malformed input,
and a cross-scope input.

## Development smoke

```bash
cargo build -p agentmesh-cli -p agentmesh-markdown-request-validator --bins
agentmesh app validate \
  --manifest apps/request-materialization-receipt/agentmesh-app.toml \
  --toolchain-pin toolchains/agentmesh-pin.v0.example.toml
agentmesh app run \
  --manifest apps/request-materialization-receipt/agentmesh-app.toml \
  --toolchain-pin toolchains/agentmesh-pin.v0.example.toml \
  --input plugins/markdown-request-validator/testdata/request_materialization_receipt_success_input.json \
  --sidecar-dir .agentmesh/runs \
  --mode development \
  --dev-plugin /absolute/path/to/target/debug/agentmesh-request-materialization-receipt
```
