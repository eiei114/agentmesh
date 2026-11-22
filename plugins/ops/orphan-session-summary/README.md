# Orphan-session summary

This crate ports one deterministic group from
`log_aware_token_efficiency_common.py`: `normalize_path`,
`_compact_orphan_session`, `select_top_orphan_sessions`,
`build_orphan_session_observation_event`, `_sum_event_field`,
`compare_log_aware_summaries` and `DEFAULT_ORPHAN_TOKEN_THRESHOLD`. Filtering,
rank order, compact-field omissions, event shape and six-decimal metric deltas
match Python. Python remains authoritative until a separate owner decision
switches a controller.

Session-file scanning, event collection, live usage and daemon-log reads stay
outside the crate; callers provide already-loaded synthetic records.

The binary reads one JSON object from stdin and writes one JSON result through
`agentmesh_ops_kit::run_stdio`. Every request has an `op` string:

| Operation | Additional request fields | Result |
| --- | --- | --- |
| `default_orphan_token_threshold` | none | integer (250000) |
| `normalize_path` | `value` (string or null) | string |
| `compact_orphan_session` | `item` | object |
| `select_top_orphan_sessions` | `session_records`, optional `matched_session_paths`, `top_n` (default 10), `token_threshold` (default 250000) | array of compact sessions |
| `build_orphan_session_observation_event` | `session`, `ts` (string) | event object |
| `sum_event_field` | `summary`, `controller`, `field` | integer |
| `compare_log_aware_summaries` | `baseline`, `current` | `{ "comparison_rows": [...] }` |

`matched_session_paths` entries are compared as given against the normalized
record path, as in Python. A negative `top_n` slices from the end, as Python's
`ranked[:top_n]` does.

Where Python raises on a malformed metric value or a wrong container shape, the
port returns `invalid_request` with the message `malformed input value`.

Bounds that differ from Python: integers are held as `i128` (and output
integers must fit `i64`), `int()` text accepts ASCII digits only, `float()` text
that parses to `nan` or `inf` is rejected, `normalize_path` does not expand
`~`, and `session_file` / `session_id` must be strings or scalars rather than
any Python object.

The library and binary are pure and do not open files, run a shell, make network
requests, call Multica or touch Git.
