# Request Re-materialization Decision v0

`apps/request-rematerialization-decision/agentmesh-app.toml` declares a
tool-neutral, one-shot App that joins one admission decision, an ordered bounded
set of materialization receipt summaries, and one revision-lineage verdict. It
returns one deterministic `regenerate`, `skip`, or `hold` decision and redacted
per-input evidence. It never reads a request or artifact, rewrites a source,
contacts a tracker, discovers an adapter, or invokes a command.

## Input contract

Input uses the explicit `agentmesh-request-rematerialization-decision.v0`
contract. `admission_record`, each receipt entry, and `revision_lineage` are
bounded projections of their upstream App results. The adapter binds each
projection to the common `scope_key`; it supplies the receipt ordinal, revision
identity, and source digest alongside the materialization receipt verdict. It
must preserve upstream verdicts as supplied: this App does not re-run admission,
receipt comparisons, or revision resolution. Receipt entries must be ordered
with contiguous one-based ordinals, all from the admission scope, and include
exactly one receipt for the admission record's revision. At most 16 receipts
are accepted. The App caps canonicalized input at 32 KiB; the manifest's host
raw-input limit is 64 KiB.

Only fixed schema/contract versions, a scope key, SHA-256 digests, stable
ordinals, enums, and the receipt drift class are accepted. Unknown fields fail
closed. No request text, evidence values, credentials, raw logs, paths, or
tracker identifiers are in the schema.

The receipt summary preserves the receipt App's supplied `corresponded`,
`drifted`, or `unknown` verdict and the supplied drift classification:
`canonical_fields`, `adapter_owned_fields`, or `unknown`. A correspondence
receipt uses `drift_class: none`. The lineage projection names the resolved
admission revision and carries `current`, `superseded`, or `unknown`; the App
consumes that verdict rather than discovering a current revision itself.

## Decision and output

| Supplied evidence for the admitted revision | Decision effect |
| --- | --- |
| admission `admit`, receipt `corresponded`, source digests match, lineage `current` | `regenerate` |
| receipt `drifted` only in adapter-owned fields, with source digests matching and no hold condition | `skip` |
| admission `defer`, canonical-field drift, source-digest mismatch, superseded lineage, or unknown receipt/lineage staleness | `hold` |

Hold takes precedence over skip if more than one condition applies. Only the
receipt matching the admission revision determines the receipt outcome;
historical receipts for other revisions remain present in the ordered input
results with `applies_to_supplied_revision: false`. Their verdicts are not
silently substituted for a missing target receipt: absence or duplicate target
receipts is a normalized failure with no decision.

Each input result reports its role, ordinal, supplied verdict, whether it
applies to the supplied revision, SHA-256 digest of that bounded input object,
and zero or one normalized reason code. Results follow admission, receipt
ordinal, then lineage order. Top-level `reason_codes` flatten only the failing
input codes in that order (maximum 18); successful output echoes the supplied
contract version. The compact output contains no request body, evidence values,
credentials, logs, paths, or tracker identifiers. Malformed, missing,
unsupported-version, cross-scope, empty, duplicate-ordinal, missing-target,
oversized, and invalidly ordered input returns one normalized error and no
partial decision.

Each input digest is `sha256:` plus lowercase SHA-256 of the compact JSON bytes
of that input record with object keys in lexicographic order and array order
preserved. The digest is over the supplied bounded projection, not source files.

## Adapter and reviewer procedure

1. Before changing an artifact, the Markdown or non-Multica adapter obtains one
   decision using the already-produced admission result, the ordered receipt
   summaries for that scope, and the lineage resolution for the admitted
   revision. It does not re-evaluate those inputs itself.
2. It regenerates only on `regenerate`, does nothing on `skip`, and pauses for
   review on `hold`. It does not rewrite the canonical source to hide
   adapter-owned drift.
3. Store the compact decision beside the adapter artifact using the adapter's
   own storage convention. The App itself performs no I/O. A reviewer checks
   the verdict, ordered input ordinals, digests, and normalized codes. For a
   hold, the reviewer requests corrected admission/receipt/lineage evidence or
   a human resolution; the decision is not evidence to disclose request text or
   tracker metadata.
4. A canonical change that previously drifted can be regenerated after the
   source has been re-admitted and a new receipt for that revision reports
   correspondence with a matching source digest. A still-drifted canonical
   receipt is `hold`, not `regenerate`.

Fixtures under `plugins/markdown-request-validator/testdata/` pair JSON inputs
with exact expected outputs:

| Case | JSON input | Expected decision output |
| --- | --- | --- |
| Regenerate after canonical drift was refreshed; old receipt remains historical | `request_rematerialization_canonical_refresh_regenerate_input.json` | `request_rematerialization_canonical_refresh_regenerate_expected.json` |
| Skip for adapter-owned-only drift | `request_rematerialization_adapter_only_skip_input.json` | `request_rematerialization_adapter_only_skip_expected.json` |
| Hold for still-present canonical-field drift | `request_rematerialization_canonical_drift_hold_input.json` | `request_rematerialization_canonical_drift_hold_expected.json` |
| Hold for superseded revision | `request_rematerialization_superseded_hold_input.json` | `request_rematerialization_superseded_hold_expected.json` |
| Hold for deferred admission | `request_rematerialization_deferred_hold_input.json` | `request_rematerialization_deferred_hold_expected.json` |
| Hold for unknown receipt and lineage staleness | `request_rematerialization_unknown_staleness_hold_input.json` | `request_rematerialization_unknown_staleness_hold_expected.json` |
| Malformed digest | `request_rematerialization_malformed_input.json` | `request_rematerialization_malformed_expected.json` |
| Cross-scope receipt | `request_rematerialization_cross_scope_input.json` | `request_rematerialization_cross_scope_expected.json` |
| Empty receipt list | `request_rematerialization_empty_receipts_input.json` | `request_rematerialization_empty_receipts_expected.json` |
| Duplicate receipt ordinal | `request_rematerialization_duplicate_ordinal_input.json` | `request_rematerialization_duplicate_ordinal_expected.json` |

Tests also cover unsupported versions, missing inputs, oversized input,
source-digest mismatch, and deterministic byte-identical repeated output.

## Development smoke

```bash
cargo build -p agentmesh-cli -p agentmesh-markdown-request-validator --bins
agentmesh app validate \
  --manifest apps/request-rematerialization-decision/agentmesh-app.toml \
  --toolchain-pin toolchains/agentmesh-pin.v0.example.toml
agentmesh app run \
  --manifest apps/request-rematerialization-decision/agentmesh-app.toml \
  --toolchain-pin toolchains/agentmesh-pin.v0.example.toml \
  --input plugins/markdown-request-validator/testdata/request_rematerialization_canonical_refresh_regenerate_input.json \
  --sidecar-dir .agentmesh/runs \
  --mode development \
  --dev-plugin /absolute/path/to/target/debug/agentmesh-request-rematerialization-decision
```
