# Vault split-plan

This crate ports the pure path, exclusion, classification, AGENTS.md cleanup,
and commit-bucket planning helpers from `vault_split_commit_common.py`. Python
remains authoritative until a separate owner decision switches a controller.

The binary reads one JSON object from stdin and writes one JSON result through
`agentmesh_ops_kit::run_stdio`. Every request has an `op` string:

| Operation | Additional request fields | Result |
| --- | --- | --- |
| `normalize_path` | `path` | normalized path string |
| `is_excluded_path` | `path` | boolean |
| `exclude_paths` | `paths` (string array) | boolean array |
| `classify_path` | `path` | bucket ID string |
| `parse_porcelain_line` | `line` | `null` or `{ "path", "raw" }` |
| `parse_porcelain_lines` | `lines` (string array) | result array |
| `agents_md_contains_multica_runtime` | `content` | boolean |
| `strip_multica_runtime_block` | `content` | `{ "content", "stripped" }` |
| `group_status_entries` | `entries` | `{ "groups", "excluded" }` |
| `build_bucket_plans` | `entries`, optional `today` | `{ "plans", "excluded" }` |

Each entry has string fields `path` and `raw`. `today`, when supplied, is an
ISO date used to keep commit messages deterministic; if omitted, the local
current date is used just as in Python. For example:

```json
{"op":"build_bucket_plans","today":"2026-11-20","entries":[{"path":"5_Structure/note.md","raw":" M 5_Structure/note.md"}]}
```

The library and binary are pure and do not call Git, access files, make network
requests, or invoke Multica.
