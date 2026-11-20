# Vault split-plan parity fixtures

Expected files are generated from the vault's read-only
`vault_split_commit_common.py` implementation. From the AgentMesh repository
root in Git Bash, run:

```sh
PYTHONPATH=/c/Users/Keisu/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts python plugins/ops/vault-split-plan/testdata/generate_expected.py
```

The fixtures use only synthetic paths and content. They cover path
normalization, every exclusion pattern family, every fixed bucket in
`BUCKET_ORDER`, dynamic skill/OSS/project buckets, quoted and renamed
porcelain paths, Windows separators, grouping/deduplication, and AGENTS.md
with and without the Multica runtime block. Bucket plans use a fixed date so
the expected commit messages are reproducible.

The Rust library and stdio binary are pure: they perform no Git, filesystem,
network, or Multica operations. The parity test reads these fixtures from the
crate's `testdata/` directory only.
