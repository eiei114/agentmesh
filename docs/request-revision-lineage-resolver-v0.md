# Request Revision Lineage Resolver App v0

`request-revision-lineage-resolver` is a pure, one-shot App that decides which
revision of one request scope is current. It takes a bounded list of validated
revision summaries and at most one adapter materialization reference digest,
resolves exactly one current revision, emits the ordered supersession chain,
and classifies the materialization reference as `current`, `stale` or
`unknown`. It does not read or overwrite files, call a tracker, discover
adapters, or rewrite any request source. Its contracts are in
`apps/request-revision-lineage-resolver/schemas/`.

## Input contract

The input carries explicit `request-revision-lineage.v0` and
`request-revision-lineage-input.v0` versions, one `scope_key`
(lowercase `[a-z0-9_-]` segments joined by `:`, at most 96 bytes), a
`materialization_reference_digest` (a digest or `null`), and 1 to 32
`revisions`. The whole input is limited to 64 KiB.

Each revision has exactly these fields:

| Field | Contract |
|---|---|
| `revision_id` | `[a-z0-9_.-]`, starting with a letter or digit, at most 64 bytes, unique |
| `scope_key` | must equal the top-level `scope_key` |
| `digest` | canonical digest, `sha256:` followed by 64 lowercase hex characters |
| `supersedes` | list of at most one `revision_id` that this revision replaces |

No tracker identifiers, credentials, or free-form command strings are part of
the contract, and the App never echoes adapter-owned values: the output uses
revision ids and digests supplied as canonical input, normalized codes, and
field paths only.

## Resolution

Input order does not matter. The App checks, in this fixed order, duplicate
revision ids, mixed scope keys, dangling `supersedes` references, cycles
(including a revision that supersedes itself), a revision superseded by more
than one other revision (branched lineage), and more than one unsuperseded
revision (multiple current). The first failure found is the only one reported.

When all checks pass, exactly one revision is current. The `chain` lists every
revision, newest first: `ordinal`, `revision_id`, `digest`, `role`
(`current` or `superseded`) and `superseded_by` (`null` for the current
revision). Superseded revisions are reported, never dropped.

## Materialization verdict

`lineage.materialization` classifies the supplied reference digest:

| Verdict | When | Reason code and field path |
|---|---|---|
| `current` | the reference equals the current revision digest | none |
| `stale` | the reference equals the digest of a superseded revision | `AGENTMESH_REQUEST_REVISION_LINEAGE_MATERIALIZATION_SUPERSEDED` at `revisions[<n>].digest` (input index) |
| `unknown` | the reference is `null` | `AGENTMESH_REQUEST_REVISION_LINEAGE_MATERIALIZATION_ABSENT` at `materialization_reference_digest` |
| `unknown` | the reference matches no supplied revision | `AGENTMESH_REQUEST_REVISION_LINEAGE_MATERIALIZATION_UNRECOGNIZED` at `materialization_reference_digest` |

Only `stale` and `unknown` list reasons; `current` has `reason_count: 0`.

## Failures

A failure returns `valid: false`, `lineage: null`, `error_count: 1` and one
code. There is never a partial lineage.

| Code suffix | Cause |
|---|---|
| `INPUT_INVALID` | malformed input, unknown or missing keys, bad digest, id or scope shape, empty or oversized revision list |
| `INPUT_TOO_LARGE` | input above 64 KiB |
| `SCHEMA_VERSION_UNSUPPORTED` | unsupported `schema_version` or `contract_version` |
| `DUPLICATE_REVISION_ID` | two revisions share an id |
| `SCOPE_MISMATCH` | a revision `scope_key` differs from the top-level scope |
| `DANGLING_REFERENCE` | `supersedes` names a revision that was not supplied |
| `CYCLE_DETECTED` | supersedes links form a cycle |
| `BRANCHED_LINEAGE` | two revisions supersede the same revision |
| `MULTIPLE_CURRENT` | more than one revision is not superseded |

All codes are prefixed `AGENTMESH_REQUEST_REVISION_LINEAGE_`. Identical input
serializes to byte-identical output.

## Deciding whether to regenerate a materialized artifact

A local runner or non-Multica adapter that keeps a materialized artifact stores
the canonical digest of the revision it materialized. Before the next run:

1. Project the request revisions for the scope into the input list, with the
   stored digest as `materialization_reference_digest` (or `null` if none was
   recorded).
2. Invoke the App. If `valid` is `false`, repair the revision records and
   rerun; there is no verdict to act on, and the artifact must not be reused or
   regenerated on a guess.
3. If `materialization.verdict` is `current`, reuse the artifact.
4. If it is `stale`, regenerate from `current_revision_id`. The reason
   confirms the artifact belongs to a legitimately superseded revision rather
   than a mis-ordered one. Never re-materialize any revision in the chain whose
   `role` is `superseded`.
5. If it is `unknown`, treat the artifact as unproven: regenerate from the
   current revision, and have a reviewer check the reason code and field path
   when the digest was recorded but unrecognized.

After regenerating, store `current_digest` as the new reference. The App does
not perform the regeneration or write anything itself.

## Recorded fixtures

Each input/expected-output pair is exercised by the Rust conformance test and
lives under `plugins/markdown-request-validator/testdata/`.

| Case | Input | Expected output |
|---|---|---|
| two-revision linear chain, current reference | `request_revision_lineage_linear_input.json` | `request_revision_lineage_linear_expected.json` |
| three-revision chain, stale reference | `request_revision_lineage_stale_input.json` | `request_revision_lineage_stale_expected.json` |
| supersedes cycle | `request_revision_lineage_cycle_input.json` | `request_revision_lineage_cycle_expected.json` |
| cross-scope input | `request_revision_lineage_cross_scope_input.json` | `request_revision_lineage_cross_scope_expected.json` |
| malformed input | `request_revision_lineage_malformed_input.json` | `request_revision_lineage_malformed_expected.json` |

The unit tests additionally cover input-order independence, every verdict and
reason code, dangling references, duplicate ids, branched and multiple-current
lineages, unsupported versions, oversized input, and the 32-revision bound.
