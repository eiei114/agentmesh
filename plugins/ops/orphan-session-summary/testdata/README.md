# Orphan-session summary parity fixtures

Expected files are generated from the vault's read-only
`log_aware_token_efficiency_common.py` implementation. From the AgentMesh
repository root in Git Bash, run:

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=/c/Users/Keisu/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts python plugins/ops/orphan-session-summary/testdata/generate_expected.py
```

That is the exact command used to generate the checked-in expected files. If
your vault checkout is elsewhere, substitute the directory containing
`log_aware_token_efficiency_common.py`:

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH="/path/to/your/vault/4_Project/Multica-Agent-Strategy/Scripts" python plugins/ops/orphan-session-summary/testdata/generate_expected.py
```

The generator reads every `*.input.json` here and rewrites the matching
`*.expected.json` from the Python functions. Python raises on malformed metric
values and wrong container shapes; the generator records those cases as the
Rust port's `invalid_request` error with the message `malformed input value`.

The fixtures use only synthetic session records and summaries (no session files,
conversation text, live feedback data, real issue IDs, secrets or vault paths).
They cover the three vault tests this slice reuses
(`test_select_top_orphan_sessions_threshold_and_join_exclusion`,
`test_build_orphan_session_observation_event_is_compact`,
`test_compare_log_aware_summaries_rows`) plus:

- ties on tokens, size and cost (input order is kept), exact-threshold
  boundaries, `top_n` of zero and a negative `top_n`, and the default `top_n` of
  ten;
- missing usage with nonzero size, zero size, a truthy string flag and negative
  size;
- matched paths that differ from the record path only by case, separators, `.`
  segments or a trailing separator;
- malformed metric values (non-numeric text, text floats, lists, bad cost) and
  the rule that rows skipped as matched or without `session_file` are never
  parsed;
- absent, null and mistyped summary sections, boolean metric values, event
  totals that fall back to `controller_summary`, and six-decimal rounding
  including a half-even tie.

`normalize_path` fixtures avoid inputs whose Python result depends on the host
OS (backslashes combined with repeated separators or `.` segments, UNC paths)
and `~`. The expected files were generated on Windows with Python 3.14.

The Rust library and stdio binary are pure: they perform no file, shell,
network, Multica or Git operations. Session-file scanning, event collection,
live usage and daemon-log reads are not ported; callers pass loaded records.
The parity test reads these fixtures from the crate's `testdata/` directory
only.
