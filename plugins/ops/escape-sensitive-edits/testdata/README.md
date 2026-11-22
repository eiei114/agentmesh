# Escape-sensitive edits parity fixtures

Expected files are generated from the vault's read-only
`escape_sensitive_edits.py` implementation. From the AgentMesh repository
root in Git Bash, run:

```sh
PYTHONPATH=/c/Users/Keisu/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts python plugins/ops/escape-sensitive-edits/testdata/generate_expected.py
```

That is the exact command used to generate the checked-in expected files. If
your vault checkout is elsewhere, substitute the directory containing
`escape_sensitive_edits.py`:

```sh
PYTHONPATH="/path/to/your/vault/4_Project/Multica-Agent-Strategy/Scripts" python plugins/ops/escape-sensitive-edits/testdata/generate_expected.py
```

The fixtures use only synthetic commands and source text. They cover path
suffix filtering (case, separators, dotfiles, trailing dots), command flags for
every unsafe shell pattern with their word-boundary rules, null commands,
escaped quotes and backslashes, physical newlines inside strings, an
unterminated string, split string assignments, and duplicate scanner findings.

The Rust library and stdio binary are pure: they perform no file, shell,
network, Multica or Git operations. `validate_js_ts_file` and its file read are
not ported; callers read the file and pass its text. The parity test reads these
fixtures from the crate's `testdata/` directory only.
