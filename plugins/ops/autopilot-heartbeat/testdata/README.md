# Autopilot heartbeat parity fixtures

The JSON cases are synthetic and contain no real session paths or run IDs.
They cover the round-trip, normal-work, in-flight-tool, true-idle, and failure
detail scenarios in `test_autopilot_watchdog_heartbeats.py`, plus timestamp,
terminal-event, last-three-event, future-time, and exact-threshold boundaries.

Expected outputs are produced by the authoritative Python implementation. To
regenerate them, set `AUTOPILOT_HEARTBEAT_PY` to the path of
`autopilot_heartbeat.py` from the Multica-Agent-Strategy Scripts directory,
then run this exact command from the crate directory:

```sh
python testdata/generate_fixtures.py --implementation "$AUTOPILOT_HEARTBEAT_PY"
```

The crate parity test reads each `*.input.json` and compares its stdio response
to the corresponding Python-generated `*.expected.json`.
