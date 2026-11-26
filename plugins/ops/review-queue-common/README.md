# Review queue summary (advisory)

The library and `agentmesh-ops-review-queue-common` stdio binary summarize
already classified rows without reordering them. `next_action` labels are only
reporting data; the crate does not classify, probe, schedule, or deliver work.

The binary reads one JSON request to EOF and emits one JSON result. The request
has `rows` plus optional `complete` (boolean, defaults to `true`) and `deferred`
(nonnegative integer, defaults to `0`). Each row has a `role` (`source` or
`child`), `project`, `key`, and `reason` (strings or null), and `issue_age_hours`
(number or null). Source rows also carry `status` (string or null). Optional
`deferral_budget` and `independent_reviewed` use Python truthiness. Other row
fields are retained, and any existing `next_action` is replaced in the result.

```bash
cargo run -q -p agentmesh-ops-review-queue-common < plugins/ops/review-queue-common/testdata/complete.input.json
```
