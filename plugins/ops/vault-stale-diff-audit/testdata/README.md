# Stale vault diff audit parity

The crate reads one JSON object and returns `{"audit": {…}, "finding": null | {…}}`.
The caller supplies the vault root label, already-classified `grouped` buckets,
and `excluded_paths` (passed through in the supplied order). Timestamps are UTC
integer microseconds since the Unix epoch: `now_unix_us` and the values of
`mtimes_unix_us`. Missing keys and null mtimes both mean “no readable mtime.”
`threshold_hours` is optional (default `24.0`). The finding is output data only.

Generate the checked-in `*.expected.json` from the repository root (Git Bash):

```sh
python plugins/ops/vault-stale-diff-audit/testdata/generate_expected.py "$HOME/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts"
```

The generator imports `vault_stale_diff_audit.py` and runs the original audit
and finding functions. Only the precomputed grouping and path metadata are
substituted; no vault files are read and no Git commands run. The cases cover
default threshold equality, just-below membership before rounding, future
and missing mtimes, duplicates, ties after rounding, the first ten of eleven
stale paths, and a null finding at a custom threshold.

```sh
cargo test -p agentmesh-ops-vault-stale-diff-audit --test parity
```
