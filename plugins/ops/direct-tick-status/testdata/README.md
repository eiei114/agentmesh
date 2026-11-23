# Direct tick status parity

From this crate directory, with `DIRECT_TICK_STATUS_PY` set to the path to
the authoritative `Scripts/direct-tick-status.py`, regenerate with:

```sh
python -B testdata/generate_fixtures.py --implementation "$DIRECT_TICK_STATUS_PY"
```

The generator imports that module and calls its `assess` and `findings`
directly with synthetic rows, snapshots, schedules and clocks. It never calls
`collect`, `ledger_rows`, `task_info` or the script's CLI. No live data is used.
The pairs cover missing tasks, JST active hours, strict staleness and runtime
boundaries, seven-digit fractions with offsets, failure priority and auxiliary
assessment fields.

Run `cargo test -p agentmesh-ops-direct-tick-status --test parity` from the
repository root to compare both the Rust handler and stdio with these pairs.
