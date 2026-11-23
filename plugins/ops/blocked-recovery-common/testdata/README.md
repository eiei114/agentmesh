# Blocked recovery parity fixtures

These pairs use synthetic policy and issue metadata only. From the repository
root, regenerate the expected results with the original Python source (Git Bash):

```bash
python -B plugins/ops/blocked-recovery-common/testdata/regenerate.py "$HOME/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts"
```

The generator replaces only Python's disk policy loader with the fixture's
`sweeper` object. It calls the original `owns_blocked` and `assessment_context`
functions, including the latter's real metadata accessor. It writes only the
expected files here and does not load live blocked recovery policy or call Multica.

```bash
cargo test -p agentmesh-ops-blocked-recovery-common --test parity
```

The test suite compares all pairs with `agentmesh_ops_kit::check_parity`, then
checks the binary against the same pairs and rejects malformed wire requests.
The large-integer cases also check stdout directly to catch rounding in either
object-valued or serialized contexts.

## JSON stdio contract

Send one JSON object and close stdin; the binary writes one JSON result.

| `op` | Required argument | Result |
| --- | --- | --- |
| `owns_blocked` | `policy`: the already-loaded policy object | `true` only if `sweeper.enabled` is boolean `true` and `sweeper.writer` is exactly `windows-tick`; otherwise `false`. |
| `assessment_context` | `issue`: an issue object with optional `metadata`, or a wrapper with an object-valued `issue` | The unchanged decoded `blocked_recovery_context` object if its writer is `windows-tick`, its mode is `assess`, and `routed_agent` is `Pi Blocked Recovery Luna Assessor`; otherwise `null`. Serialized metadata maps and values are decoded as in Python. |

Negative decisions are successful results (exit 0); malformed wire requests
use the shared kit's `invalid_request` result (exit 2). The Rust crate reads no
policy files and is not wired into any controller. Python remains authoritative.
