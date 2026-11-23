# Synthetic Python parity

From the AgentMesh repository root in Git Bash, the exact generation command
used with Python 3.14.7 was:

```sh
PYTHONDONTWRITEBYTECODE=1 VAULT_SCRIPTS_DIR="$HOME/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts" python plugins/ops/review-lessons-common/testdata/generate_expected.py
```

Set `VAULT_SCRIPTS_DIR` to the directory containing `review-lessons-common.py`
if your vault lives elsewhere. The script calls the vault's `stamp` and
`build_index` with synthetic fixture values, temporarily replacing the repo
alias table with `repo_aliases` from each input. It reads no live ledgers and
writes only the paired `*.expected.json` files. Normal Rust tests require no
vault or Python.

The `threshold-*`, `identity-and-monitor`, `windows`, and `alias-and-order`
requests reuse the synthetic `record`, `REG`, and `NOW` from
`test-review-lessons.py`, including its cross-repository threshold, replay,
old finding with a new observation, and renamed repository cases. The fixtures
add exact/one-microsecond boundaries at 7, 30, and 90 days; future findings;
the inline monitor filter; all three deduplication identity paths; alias
coalescing; sorted and capped URLs; equal-instant timestamp tie breaking;
and the difference between counted evidence and grouped project findings.
`stamp-*` fixtures cover dates, offsets, naive UTC, fractional seconds, basic
and week dates, end-of-day midnight, and invalid input.

```sh
cargo test -p agentmesh-ops-review-lessons-common --test parity
```

`check_parity` compares the complete JSON values. It preserves lesson-list and
source-list ordering in the comparison; JSON object key order is not part of
the contract. The stdio test also submits each fixture to the actual binary.
