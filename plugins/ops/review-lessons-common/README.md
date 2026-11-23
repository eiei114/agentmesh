# Review lesson index

Offline port of the vault's `review-lessons-common.py::build_index` and
`stamp`. The same `handle(Value)` function serves the library and the ops-kit
binary (`agentmesh-ops-review-lessons-common`). The binary reads one JSON
request from stdin and writes one JSON response to stdout.

```json
{"op":"build_index","now":"2026-10-11T09:00:00Z","records":[],"registry":{"guardrails":[]},"repo_aliases":{"old-slug":"project-key"}}
```

`now` is supplied by the caller, not the wall clock. `records` contains the
already-loaded finding objects. `registry.guardrails` is optional; only keys
in Python's fixed advice table are indexed. `repo_aliases` maps repository
slugs (without the owner prefix) to project keys; use `{}` for no aliases.
The result is the Python index object, including `evidence_count` before repo
filtering and per-project stats and provenance URLs after repo filtering.

```json
{"op":"stamp","value":"2026-10-11 09:00"}
```

`stamp` returns the ISO string (`"2026-10-11T09:00:00+00:00"` here) or JSON
`null` for an unparseable value. Naive timestamps are UTC; offset timestamps
are compared by instant and rendered with their original offset. Tests pin
the record timestamp formats and additional common `fromisoformat` forms
using Python 3.14; they do not claim exhaustive parity for ambiguous ISO week
date separators or Python object stringification. The JSON contract uses
objects for records and rules, string repo names, and scalar finding IDs.

No vault I/O, network, clock access, lesson selection, enrichment, or
controller wiring. Python remains authoritative. See `testdata/README.md`
to regenerate the synthetic parity fixtures.
