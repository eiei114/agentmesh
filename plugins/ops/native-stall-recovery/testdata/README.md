# Native stall evidence parity

The binary reads one JSON object. `latest_run` supplies `status`, `id`,
optional `error` text, and optional `result.output` text. `issue` supplies
`assignee_id`. Supply the `lane_health.INFRA.search` truth value as
`provider_match`, and the decoded `native_stall_recovery` marker as
`decoded_metadata` (or null if absent). `timestamp` is the explicit input to
the timestamp helper; the caller decides which run timestamp to supply.

Success returns `classification`, the timestamp in Python `isoformat()` form
(preserving its UTC offset), and `blocked_reason` (null if no matching
marker). Naive, absent, or invalid timestamps return `invalid_request` with
`native_stall_timestamp_unknown`. No selection, retry, or dispatch is made.

Run the following from the repository root, with `VAULT_SCRIPTS` set to the
vault's `4_Project/Multica-Agent-Strategy/Scripts` directory:

```sh
python plugins/ops/native-stall-recovery/testdata/generate_expected.py "$VAULT_SCRIPTS"
```

The generator calls `classification`, `timestamp`, and `blocked_reason` in
`native-stall-recovery.py`; it injects the real Python lane-health matcher
result into the request. Fixtures use only synthetic data and reuse the
classification and bound-marker cases in `test-native-stall-recovery.py`.
The Rust tests use the checked-in JSON pairs and do not need the vault or
Python installed.

```sh
cargo test -p agentmesh-ops-native-stall-recovery --test parity
cargo run --quiet -p agentmesh-ops-native-stall-recovery < plugins/ops/native-stall-recovery/testdata/marker-match.input.json
```
