# Request Acceptance Evidence Coverage App v0

`apps/request-acceptance-evidence-coverage/agentmesh-app.toml` declares a
tool-neutral, one-shot App that maps evidence references onto every criterion
in one validated request summary. It never reads source files or evidence
values, contacts a tracker, discovers adapters, or writes a request.

## Input contract

The input schema bounds the JSON envelope to 64 KiB, the request to 128 ordered
criterion ordinals, and the inventory to 128 ordered references. The summary
must be marked valid and bind an `agentmesh-request.v0` request to a tool-neutral
`agentmesh:` scope and a SHA-256 request digest. It carries criterion ordinals,
not criterion text. Each reference contains a stable bounded `ev_`-namespaced
id, a SHA-256 digest, one normalized evidence class, an evidence-reference
schema version, and the criterion indices it supports. IDs are opaque local
evidence handles, never tracker identifiers or paths.

```json
{
  "schema_version": "request-acceptance-evidence-coverage-input.v0",
  "contract_version": "request-acceptance-evidence-coverage.v0",
  "request_summary": {
    "schema_version": "agentmesh-request-summary.v0",
    "request_schema_version": "agentmesh-request.v0",
    "valid": true,
    "scope": "agentmesh:app:request-acceptance-evidence-coverage",
    "request_digest": "sha256:1111111111111111111111111111111111111111111111111111111111111111",
    "acceptance_criteria": [{"ordinal": 1}, {"ordinal": 2}]
  },
  "evidence_inventory": [{
    "schema_version": "agentmesh-evidence-reference.v0",
    "id": "ev_test-run-1",
    "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "evidence_class": "test",
    "criterion_indices": [1]
  }]
}
```

A local runner can build this compact summary from a successful pinned request
parser/normalizer result: copy the stable request scope, bind the parser's
canonical projection digest, and project each canonical `requirements[*].index`
to `acceptance_criteria[*].ordinal`. Evidence producers provide only their
stable `ev_` reference, digest, class, and criterion indices; raw logs, values,
paths, and credentials stay outside this App.

## Output and downstream use

Each criterion is emitted once in ordinal order with `covered` or `uncovered`
and only the ids of supported evidence. The request verdict is `uncovered` if
any criterion is uncovered. A bounded `evidence_inventory_sha256` binds the
ordered inventory without revealing its contents. Findings contain only a
normalized code and a JSON field path; supported codes identify missing
criterion evidence, out-of-range indices, duplicate ids, and unsupported
evidence-reference versions. At most 256 findings are included, with a
truncation flag and omitted count. Malformed, missing, oversized, or unsupported
request inputs return one normalized error and `coverage: null`, never a
partial report.

The nested `coverage` summary deliberately matches the bounded
`acceptance-evidence-coverage.v0` sub-gate contract used by the request artifact
admission gate. A runner passes its schema-defined fields (`schema_version`,
`contract_version`, `scope`, `request_digest`, `verdict`,
`uncovered_criterion_ordinals`, and `reason_code`) to that App's
`acceptance_evidence` input. Materialize an artifact only after checking
`valid: true`, every criterion verdict, and `coverage.verdict: "covered"`; a
summary verdict never overrides an uncovered criterion.

## Recorded fixtures and local smoke

The fixture pairs in `plugins/markdown-request-validator/testdata/` cover full
and partial coverage, duplicate and out-of-range references, malformed input,
an unsupported evidence schema version, and an unsupported App schema version.

```bash
cargo build -p agentmesh-cli -p agentmesh-markdown-request-validator --bins
agentmesh app validate \
  --manifest apps/request-acceptance-evidence-coverage/agentmesh-app.toml \
  --toolchain-pin toolchains/agentmesh-pin.v0.example.toml
agentmesh app run \
  --manifest apps/request-acceptance-evidence-coverage/agentmesh-app.toml \
  --toolchain-pin toolchains/agentmesh-pin.v0.example.toml \
  --input plugins/markdown-request-validator/testdata/request_acceptance_evidence_coverage_partial_input.json \
  --sidecar-dir .agentmesh/runs \
  --mode development \
  --dev-plugin /absolute/path/to/target/debug/agentmesh-request-acceptance-evidence-coverage
```

The App does not authorize artifact creation itself; the local runner owns that
decision. Release, publication, tracker mutation, credentials, and production
actions are outside this contract.
