# Autopilot heartbeat ops crate

This crate ports the deterministic heartbeat event and watchdog analysis
helpers from `autopilot_heartbeat.py`. It does not report heartbeats, access a
session file, call Multica, use the network, or inspect Git. The caller supplies
all event data and the `now` timestamp.

The stdio process reads one JSON object and writes one JSON response. The
default operation is `analyze_watchdog_activity`:

```json
{
  "operation": "analyze_watchdog_activity",
  "now": "2026-07-13T09:00:00Z",
  "threshold_seconds": 900,
  "heartbeat_lines": [
    "MULTICA_HEARTBEAT {\"controller\":\"fixture-controller\",\"event\":\"progress\",\"phase\":\"collect\",\"ts\":\"2026-07-13T08:59:00Z\"}"
  ],
  "heartbeats": [],
  "run_messages": [],
  "session_progress": {
    "has_user_progress": false,
    "has_assistant_progress": false
  }
}
```

`heartbeat_lines` accepts either a multiline string or an array of lines;
`heartbeats` accepts an array of event objects or heartbeat lines. Invalid
events and non-heartbeat lines are ignored, matching the Python helpers.
`threshold_seconds` is optional and defaults to 900. Analysis operations reject
a missing or malformed `now` rather than reading the system clock. Naive input
timestamps are interpreted as UTC for machine-independent results; offsets are
normalized to UTC.

Supported operations are `parse_dt`, `heartbeat_event_from_dict`,
`heartbeat_event_from_line`, `event_round_trip`, `parse_heartbeat_lines`,
`activity_helpers`, `analyze_watchdog_activity`, `is_watchdog_idle`, and
`build_watchdog_failure_detail`. The `session_path` value is opaque output data;
the crate never opens it.

Python-generated parity fixtures and their regeneration command are documented
in `testdata/README.md`.
