# Python parity fixtures

From the repository root, regenerate the expectations with Python 3.14:

```bash
python -B plugins/ops/free-model-benchmarks/testdata/generate_expected.py "$HOME/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts"
```

The generator imports the Python authority and invokes only the four offline
scoring helpers. The fixtures use invented model IDs and pre-parsed metric
rows. They adapt cases from `test-free-model-benchmarks.py`: repeat medians from
the Arena/Aider cases, tied and singleton midranks, exact alias and metadata
drift, unverified identity, stale flags, vendor weights and the missing-evidence
prior. No downloads, production alias registry, or state are involved.

```bash
cargo test -p agentmesh-ops-free-model-benchmarks --test parity
```
