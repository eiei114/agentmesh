# Review guardrail analysis

This crate ports the deterministic analysis core of
`report-review-guardrail-promotions.py`: `parse_time`, `occurrence_time` and
`analyze`. Timestamps without an offset are read as UTC, the finding creation
time wins over the observation time, the rolling window keeps records at or
after `now - promotion_window_days`, repository thresholds and immediate
candidates decide promotion candidates, and recurrence counts records at or
after the rule's promotion (or catalog) time. Python remains authoritative
until a separate owner decision switches a controller.

`load_evidence`, report and issue file generation, `create_candidate_issues`
and `main` stay outside the crate; callers provide already-loaded synthetic
records, a registry object and `now`.

The binary reads one JSON object from stdin and writes one JSON result through
`agentmesh_ops_kit::run_stdio`. Every request has an `op` string:

| Operation | Additional request fields | Result |
| --- | --- | --- |
| `parse_time` | `value` (string) | ISO text of the instant, as Python `isoformat()` |
| `occurrence_time` | `item` (object) | ISO text, or `null` when unparseable |
| `analyze` | `records` (array of objects), `registry` (object), `now` (string) | `{ "window_days", "cutoff", "rows", "promotion_candidates" }` |

Where Python raises on a malformed timestamp, registry number or container
shape, the port returns `invalid_request` with the message
`malformed input value`. A record whose chosen timestamp does not parse is
skipped, as in Python.

Bounds that differ from Python: timestamps accept `YYYY-MM-DD`, an optional `T`
or space and `HH[:MM[:SS[.f+]]]`, then an optional `Z`, `+HH:MM[:SS]` or
`+HHMM` offset (week dates, basic dates and comma fractions are rejected);
non-string timestamps count as unparseable; `int()` text accepts an optional
sign and ASCII digits only; and `str()` of a non-string repository, key or
status is limited to integers and booleans.

The library and binary are pure and do not open files, run a shell, make network
requests, call Multica or touch Git.
