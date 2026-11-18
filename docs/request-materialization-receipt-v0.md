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

## Adapter and reviewer procedure

1. After writing the artifact, compute its byte digest and retain the original
   source digest alongside it. Do not put a file path or tracker identifier in
   the descriptor.
2. Run this App once with the admission record as supplied, the retained
   canonical summary, and that artifact descriptor. Store the JSON receipt next
   to the artifact using the adapter's own storage convention; the App itself
   does not write files.
3. A reviewer checks `receipt.verdict`, then reads `fields` and
   `reason_codes` in ordinal order. A `drifted` receipt identifies which
   comparisons disagree without disclosing request body text or tracker
   metadata. A null receipt means the input was invalid and must not be treated
   as correspondence evidence.

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
