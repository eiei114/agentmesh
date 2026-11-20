# Project-focus parity fixtures

The expected files are generated from the vault's `project_focus.py`; each
input passes policy contents (or `null` for a missing file) and synthetic issue
rows to the same public functions. Policy files used during generation live
only in a temporary directory.

From the repository root, run the exact generator command:

```sh
python plugins/ops/project-focus/testdata/generate_expected.py
```

The generator imports the source module from
`~/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts/` and
uses a temporary policy file to call `load`. It converts Python's `set` and
`Counter` results to sorted JSON values. Inputs cover enabled/disabled policy,
missing and invalid policy files, rows without `project_id`, configured and
absent pacing limits, and the Python behavior that excludes boolean limits.
