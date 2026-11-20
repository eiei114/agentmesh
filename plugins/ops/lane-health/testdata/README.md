# Lane-health parity fixtures

Expected values are generated from the vault's `lane_health.py` by invoking
its `parse`, `infra_failure`, and `since_config` helpers and the decision part
of `checker` with task history supplied as synthetic JSON. The fixtures use no
credentials, real issue identifiers, or vault data.

From the repository root, run the exact generator command:

```sh
python plugins/ops/lane-health/testdata/generate_expected.py
```

The generator imports `lane_health.py` from
`~/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts/` and
writes only the expected JSON files in this testdata directory.

Cases cover a three-run infra streak, too few failures, a success in the latest
streak, failures outside the three-hour window, non-infrastructure errors,
unparsable timestamps, runs preceding a configuration update, and timestamp
parsing itself. The Python check uses only an upper age bound (`now - done <=
WINDOW`), so future-dated terminal runs also count toward a streak; this
behavior is retained unchanged and covered by a synthetic case.
