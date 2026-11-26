# Completion subject parity

From the repository root, with `VAULT_ROOT` set to the root of the Notes vault
(Git Bash command):

```sh
python plugins/ops/review-completion-common/testdata/regenerate.py "$VAULT_ROOT/4_Project/Multica-Agent-Strategy/Scripts"
```

The generator imports `review-completion-common.py` and calls only
`completion_subject_plan` for expected results. The baseline and applicable
mutations are adapted from `CompletionReceiptTests` in
`test-review-completion-receipts.py`, using synthetic IDs, URLs and timestamps.
Additional cases stack failures to check first rejection and exercise timestamp
ties, strict schema/draft types, and link membership vs. ambiguity. Every
expected result includes empty `metadata`, including passing subjects.

The request is `{receipt, context}`. `context.helpers` contains precomputed
Python metadata and safety results plus URL parsing/normalization facts (using
Python's `urlparse` and `str.isdigit` semantics). The URL facts must describe the
same receipt and context; they replace the raw repo/PR/link identity inputs for
the port. These are fixture inputs, not assertions accepted from a live
producer. The Rust plan still makes the scope, SHA, link, run and managed-child
decisions in order. The supported wire shape is the projected snapshot (string
IDs/timestamps and object records), not arbitrary Python objects. The binary
does not resolve helper facts, verify receipt commands, or write any state.

Run parity without Python or the vault:

```sh
cargo test -p agentmesh-ops-review-completion-common --test parity
```
