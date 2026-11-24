# Python parity fixtures

From the AgentMesh repository root, run the exact generation command (Git Bash):

```sh
python plugins/ops/free-model-slot/testdata/generate.py "$HOME/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts/free_model_slot.py"
```

If the source checkout is elsewhere, substitute its `free_model_slot.py` path.
The generator builds synthetic JSON requests, decodes the exact input with
Python's `json.loads`, and executes the four original Python function
definitions to produce the paired results. It does not
import the controller or read any snapshot, registry, or runtime state.

Costs and legacy gates adapt cases from `test-free-model-slot.py`. External
cases cover both exact gate sets, missing, extra and false gates, numeric
equality with `True`, and ignoring evaluations. Winner snapshots cover rejected
rows, quality/completions/context/model ordering, stable exact ties, defaults,
and historical score alongside V2 rows.

The parity test also passes the raw inputs for the adjacent-float cases through
the built stdio binary: `0.9999999999999999` must not satisfy a v1 or v2 gate,
and quality `1.0` must rank ahead of it even when model IDs sort the other way.

```sh
cargo test -p agentmesh-ops-free-model-slot --test parity
```
