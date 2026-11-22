# Maintenance-requests parity fixtures

The expected files are generated from the vault's
`agentmesh_maintenance_requests.py`. Each `<name>.input.json` is a stdio
request with an `op` field (`validate`, `slugify`, `render_source`,
`render_issue` or `patrol`); the generator calls the matching Python function
and writes `<name>.expected.json`. A `RequestError` becomes
`{"error": {"kind": "invalid_request", "message": ...}}`, the shape the Rust
handler returns.

From the repository root, run the exact generator command:

```sh
python plugins/ops/maintenance-requests/testdata/generate_expected.py
```

The generator imports the source module from
`~/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts/`.
It needs no vault files: every input is a synthetic request, a fixed `now`
(`2026-07-16T07:40:00+09:00`) and synthetic relative paths. The Python
`create_request` path checks, scope dedupe, daily cap and file writes are not
ported and have no fixtures.

The input files are hand-authored and committed; only the expected files are
regenerated. Inputs cover:

- validation success for both kinds, default request ids (including
  non-ASCII titles, 56-character slug cuts and a trailing dash at the cut),
  explicit ids, trimming and defaulted `target_app` and `user_story`;
- validation failures for schema version (empty, wrong, and non-string values
  that Python stringifies), kind, length bounds counted in code points, entry
  bounds, scope pattern and request id pattern;
- slugification of punctuation runs, non-ASCII text, truncation and the
  `request` fallback;
- both renderers with quotes, backslashes, newlines, control characters and
  Unicode in the generated Markdown and YAML strings;
- patrol choices for repair candidates with and without a `code`, an existing
  daily request, and neither.
