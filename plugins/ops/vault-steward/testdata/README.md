# Vault stewardship result parity

From the repository root in Git Bash, the exact generation command used was:

```sh
PYTHONDONTWRITEBYTECODE=1 PYTHONPATH="$HOME/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts" python plugins/ops/vault-steward/testdata/generate_expected.py
```

If the vault checkout is elsewhere, set `PYTHONPATH` to its directory containing
`vault_steward.py`. The generator invokes only `build_result`, not the Python
CLI. Root placeholders are single path components, so Python `str(Path(root))`
produces the same text on Windows and Unix. All paths and metadata are synthetic.

Each input is one stdio request: `root` is already-rendered text, and each
`plans` entry supplies `bucket_id`, `files`, and `message`. Counts are derived
from the supplied arrays. The shaper does not sort, deduplicate, discover paths,
or infer commit/push outcomes (including for dry runs).

Cases cover empty and populated dry-run commits, normal commits, deferred push,
skipped buckets, and errors with missing, null, empty, or populated conflict
lists. Both the Rust JSON handler and stdio binary compare against every pair:

```sh
cargo test -p agentmesh-ops-vault-steward --test parity
```
