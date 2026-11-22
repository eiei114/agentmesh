# Review guardrail analysis parity fixtures

Expected files are generated from the vault's read-only
`report-review-guardrail-promotions.py` implementation. The script's file name
has hyphens, so the generator loads it by path from the directory named in
`VAULT_SCRIPTS_DIR`. From the AgentMesh repository root in Git Bash, run:

```sh
PYTHONDONTWRITEBYTECODE=1 VAULT_SCRIPTS_DIR=/c/Users/Keisu/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts python plugins/ops/review-guardrail-analysis/testdata/generate_expected.py
```

That is the exact command used to generate the checked-in expected files. If
your vault checkout is elsewhere, set `VAULT_SCRIPTS_DIR` to the directory that
contains `report-review-guardrail-promotions.py`.

The generator reads every `*.input.json` here and rewrites the matching
`*.expected.json` from the Python functions (`parse_time`, `occurrence_time`
and `analyze`). Python raises on malformed timestamps, malformed registry
numbers and wrong container shapes; the generator records those cases as the
Rust port's `invalid_request` error with the message `malformed input value`.
The input files are hand-written synthetic requests; the generator never
rewrites them.

The fixtures use only synthetic guardrail keys (`K`, `alpha`, ...), repository
names (`a`, `b`, ...) and timestamps (no evidence files, review bodies, live
registry data, issue IDs, secrets or vault paths). Every `analyze` case uses
`now` of `2026-11-22T00:00:00Z` unless its name says otherwise, so the default
90 day cutoff is `2026-08-24T00:00:00+00:00`. They cover:

- a finding created long ago but observed late (excluded) and a new finding
  carrying an older observation time (included), plus the `observed_at`
  fallback when the creation time is missing, empty or null and the rule that a
  malformed creation time does not fall back;
- the exact cutoff instant (included) and one microsecond before it
  (excluded), naive timestamps read as UTC, `now` with a positive, negative and
  fractional offset, and custom, textual, fractional, zero and missing window
  days;
- the exact promotion-time instant for post-promotion recurrence, the
  `cataloged_at` fallback, promoted-over-cataloged precedence, late backfills
  that must not count as recurrence, and keys without evidence;
- thresholds (findings and repositories, custom and zero minimums, empty
  repositories), immediate candidates (only the boolean `true` counts), already
  promoted keys, unregistered keys, `unclassified` keys that are never
  candidates, duplicate rule keys, and candidate order following sorted keys
  rather than input order;
- malformed timestamps in records (skipped), and malformed registry numbers,
  promotion times, `now` and container shapes (errors).

`parse_time` fixtures avoid Python input forms the port does not accept (week
dates, basic dates such as `20261122`, comma fractions). The expected files were
generated on Windows with Python 3.14.

The Rust library and stdio binary are pure: they perform no file, shell,
network, Multica or Git operations. `load_evidence`, report and issue file
generation, `create_candidate_issues` and `main` are not ported; callers pass
loaded records. The parity test reads these fixtures from the crate's
`testdata/` directory only.
