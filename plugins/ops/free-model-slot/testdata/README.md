# Python parity fixtures

From the AgentMesh repository root, run the exact generation command (Git Bash):

```sh
python plugins/ops/free-model-slot/testdata/generate.py "$HOME/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts/free_model_slot.py"
```

If the source checkout is elsewhere, substitute its `free_model_slot.py` path.
The generator constructs synthetic JSON requests and executes the four original
Python function definitions to produce their paired results. It does not
import the controller or read any snapshot, registry, or runtime state.

Costs and legacy gates adapt cases from `test-free-model-slot.py`. External
cases cover both exact gate sets, missing, extra and false gates, numeric
equality with `True`, and ignoring evaluations. Winner snapshots cover rejected
rows, quality/completions/context/model ordering, stable exact ties, defaults,
and historical score alongside V2 rows.

```sh
cargo test -p agentmesh-ops-free-model-slot --test parity
```
