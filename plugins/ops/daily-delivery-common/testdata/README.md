# Daily delivery parity fixtures

All inputs are synthetic. From the repository root, regenerate the paired
`*.expected.json` files with the original Python functions (Git Bash):

```bash
python -B plugins/ops/daily-delivery-common/testdata/regenerate.py "$HOME/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts"
```

Substitute the path to the source `Scripts` directory if your checkout is
elsewhere. The script only writes expected JSON in this directory. It does not
load policy files, run the supplied SDK commands, or contact Multica.

```bash
cargo test -p agentmesh-ops-daily-delivery-common --test parity
```

The suite compares every pair with `agentmesh_ops_kit::check_parity` and also
sends every input through the compiled stdio binary, checking the full result
and exit status. A separate test covers invalid requests.

## JSON requests

Send one object with an `op` field, then close stdin. A handled request exits
0 and writes one JSON scalar. Invalid JSON, request shapes, and timestamps use
the ops kit `invalid_request` error and exit 2.

| `op` | Other fields | Result |
| --- | --- | --- |
| `managed_delivery_scope` | `project` (object or null; optional string keys `status`, `workspace_path`, `multica_project_id`), `policy` (object; optional string arrays `allowed_project_statuses`, `review_delivery_extra_project_ids`) | Boolean. Status must be allowed even for explicitly listed projects. Matching is case-sensitive; backslashes in the path are treated as `/`. |
| `daily_target` | `remaining` (signed 64-bit integer), `deadline` and `now` (RFC 3339 instants with explicit offsets and microsecond precision), optional `minimum` (signed 64-bit integer, default 10) | Integer. The remaining days are rounded up and clamped to at least one; the work count is clamped at zero, divided by days, rounded up, and subject to the minimum. |
| `sdk_report_reason` | `report` (already-collected JSON, including null if unavailable), `versions` (object with string values), `cases` (array of case names, checked in order) | `null` when accepted or the first reason string when rejected. A rejection is a decision, not a transport error. |

```bash
cargo run --quiet -p agentmesh-ops-daily-delivery-common < plugins/ops/daily-delivery-common/testdata/sdk-missing-assertion-id.input.json
```

That example returns `"sdk_case_unverified:reload_cleanup"`. The report checks
do not validate external artifacts or replace independent verification of the
SDK harness. Python remains authoritative.
