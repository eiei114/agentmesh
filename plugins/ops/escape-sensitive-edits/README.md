# Escape-sensitive edits

This crate ports the deterministic command classifiers and source scanner from
`escape_sensitive_edits.py`: `is_js_ts_path`, `command_targets_js_ts`,
`command_mentions_escape_literal`, `flag_unsafe_js_ts_shell_edit`,
`_scan_physical_newline_in_strings` and `scan_escape_corruption`, with the
module's suffix and regex constants. Issue order, line numbers,
de-duplication and message text match Python. Python remains authoritative
until a separate owner decision switches a controller.

`validate_js_ts_file` and its file read stay outside the crate.

The binary reads one JSON object from stdin and writes one JSON result through
`agentmesh_ops_kit::run_stdio`. Every request has an `op` string:

| Operation | Additional request fields | Result |
| --- | --- | --- |
| `is_js_ts_path` | `path` | boolean |
| `command_targets_js_ts` | `command` (string or null) | boolean |
| `command_mentions_escape_literal` | `command` (string or null) | boolean |
| `flag_unsafe_js_ts_shell_edit` | `command` (string or null) | array of `[pattern, reason]` |
| `scan_physical_newline_in_strings` | `content` | array of issues |
| `scan_escape_corruption` | `content`, optional `path` | array of issues |

An issue is `{ "line", "kind", "message" }`. When `path` is supplied and is not
a JS/TS path, `scan_escape_corruption` returns an empty array, as in Python.

The library and binary are pure and do not open files, run a shell, make network
requests, call Multica or touch Git.
