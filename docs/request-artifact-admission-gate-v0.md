# Request Artifact Admission Gate App v0

`request-artifact-admission-gate` is a pure, one-shot join over three upstream
results. It does not re-run neutrality, evidence coverage, or lineage analysis;
read or write files; call a tracker; discover adapters; or rewrite a request.
Its input and output contracts are in
`apps/request-artifact-admission-gate/schemas/`.

## Input contract

The input carries an explicit `request-artifact-admission.v0` contract version
and one `request-artifact-admission-summary.v0` projected from the canonical
request, containing only:

- a tool-neutral `scope`;
- the SHA-256 digest of the canonical request fields; and
- an opaque SHA-256 `revision_identity` supplied by the adapter.

The three sub-gates must repeat the same scope and request digest. Their bounded
objects carry no request prose, criterion/evidence values, tracker IDs, paths,
credentials, or command strings. A schema or contract version not supported by
this App, missing/extra fields, malformed values, cross-scope bindings, and
inputs above 32 KiB fail closed with one normalized error code and
`admission: null`—never a partial gate list.

`source_neutrality` supplies `neutral` or `non_neutral` and its normalized
reason code. `acceptance_evidence` supplies `covered` or `uncovered`, the
uncovered criterion ordinals (never criterion text), and the corresponding
normalized reason. `revision_lineage.current_revision_identity` is the revision
selected by the existing lineage resolver; this App compares that identity to
the supplied summary identity and does not inspect revisions or recompute
lineage.

## Output and adapter use

The valid output is deterministic and orders gate results as source neutrality,
acceptance-evidence coverage, then revision lineage. Each result contains its
stable ordinal/name, the supplied or resolved verdict, at most one normalized
reason code, and the SHA-256 digest of that bounded sub-gate object. The record
includes only `admit`/`defer` and the supplied revision identity; it never
echoes the scope, request digest, request text, evidence values, or gate input.
Identical input objects serialize to byte-identical compact records.

A Markdown or non-Multica adapter should pass its canonical summary and the
three already-computed sub-gate results to this App immediately before it
materializes an artifact. It may proceed only when `valid` is `true` and
`admission.decision` is `admit`; it should not duplicate gate precedence or
short-circuit logic. For `defer`, surface only the decision, ordered gate names
and verdicts, normalized reason codes, digests, and revision identity to a
reviewer. Do not attach or copy request content, evidence values, tracker
metadata, or logs into that review record. If `valid` is false, present the
single normalized error code and rerun after repairing the inputs; there is no
admission record to act on.

## Recorded fixtures

Each input/expected-output JSON pair below is used by the Rust conformance test:

| Case | Input | Expected output |
|---|---|---|
| admitted | `request_artifact_admission_admit_input.json` | `request_artifact_admission_admit_expected.json` |
| uncovered criterion | `request_artifact_admission_uncovered_input.json` | `request_artifact_admission_uncovered_expected.json` |
| stale revision | `request_artifact_admission_stale_input.json` | `request_artifact_admission_stale_expected.json` |
| non-neutral source | `request_artifact_admission_non_neutral_input.json` | `request_artifact_admission_non_neutral_expected.json` |
| malformed sub-gate | `request_artifact_admission_malformed_input.json` | `request_artifact_admission_malformed_expected.json` |
| cross-scope sub-gate | `request_artifact_admission_cross_scope_input.json` | `request_artifact_admission_cross_scope_expected.json` |
| unsupported sub-gate version | `request_artifact_admission_unsupported_version_input.json` | `request_artifact_admission_unsupported_version_expected.json` |
| unsupported App contract | `request_artifact_admission_contract_version_input.json` | `request_artifact_admission_contract_version_expected.json` |

The fixture files live under `plugins/markdown-request-validator/testdata/`.
The test also exercises an oversized input and verifies that failures carry no
partial admission record.
