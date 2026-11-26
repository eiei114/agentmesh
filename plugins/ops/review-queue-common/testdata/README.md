# Python parity fixtures

From the agentmesh repository root, with `VAULT_ROOT` set to the vault checkout
root, run this command in Bash:

```bash
python plugins/ops/review-queue-common/testdata/regenerate.py "$VAULT_ROOT/4_Project/Multica-Agent-Strategy/Scripts/review-queue-common.py"
```

The generator extracts and executes the original `next_action` and
`queue_summary` definitions without importing or running any of the Python
controller code. Every input is synthetic. The four cases cover all coverage
labels, mixed and entirely unknown ages, repeated reasons and groups, first
appearance order, and action precedence when deferral and child rules overlap.

```bash
cargo test -p agentmesh-ops-review-queue-common --test parity
```
