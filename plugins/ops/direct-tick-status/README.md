# Direct tick status

Read-only port of `parse_stamp`, `in_flight`, `assess` and `findings` from
`direct-tick-status.py`. Controller schedules, task discovery, ledger reads,
report collection and state changes stay with the caller.

Send one JSON document on stdin:

```json
{
  "spec": {"key": "synthetic", "task": "Synthetic-Tick", "interval": 10, "limit": 30, "hours": [9]},
  "rows": [{"finished_at": "2026-06-01T09:40:00+09:00", "ok": true}],
  "task": {"state": "Running", "last_run": "2026-06-01T09:42:00.1234567+09:00"},
  "now": "2026-06-01T00:45:00Z"
}
```

`interval` and `limit` are non-negative whole minutes; `hours` are JST hours
0–23. Pass rows oldest-to-newest: the last row is the newest finish, and only
JSON `true` counts as success. `health` and `reason`, when supplied, are ledger
labels (text, null or other scalar values). Supply an explicit ISO calendar
timestamp for `now` and an offset on ledger finish times. Offset-less scheduler
timestamps are interpreted as JST; fractions are truncated to microseconds.
Omitting `task` or passing `null` means missing. If the caller knows only
that scheduler data is unavailable, pass `{"state":"unknown"}` instead.

The response is `{"assessment": {...}, "findings": [...]}`. Only `failing`,
`stale` and `missing` produce warning findings. Invalid requests use the ops
kit error document and exit code 2.
