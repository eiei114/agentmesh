# Windows-subprocess parity fixtures

The expected files are generated from the vault's `windows_subprocess.py`
function `hidden_subprocess_kwargs`. Each input gives the caller-supplied
platform and process-group booleans (`is_windows`, `new_process_group`). No
process is launched; the generator only calls the pure kwargs function.

From the repository root, run the exact generator command:

```sh
python plugins/ops/windows-subprocess/testdata/generate_expected.py
```

The generator imports the source module from
`~/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts/` and
patches `os.name` for each call (and installs a stand-in `STARTUPINFO` when the
host has none). It normalizes Python's result: an empty dict becomes
`{"creationflags": null, "startupinfo": null}`, and a `STARTUPINFO` object
becomes `{"dwFlags", "wShowWindow"}`. Importing the module installs the
console window guard in the generator process only.

Cases: `non-windows` (no kwargs), `windows-no-group` (`CREATE_NO_WINDOW`),
and `windows-new-group` (`CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP`). The
Windows cases mirror `test_windows_subprocess.py`, which asserts that
`CREATE_NO_WINDOW` is set and `startupinfo` is present; the non-Windows case
mirrors its empty-dict branch. All inputs are synthetic.
