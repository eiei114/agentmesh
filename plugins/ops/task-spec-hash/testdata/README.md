# task_spec_hash.py parity fixtures

Each expected value is the JSON encoding of the output from the vault's
`task_spec_hash.py` CLI. The requests contain only synthetic Markdown; `text`
is passed verbatim on stdin and the script's `sha256:...` output is encoded as
a JSON string (or as JSON `null`).

With the vault checkout available, run this PowerShell command from the
AgentMesh repository root to regenerate all expected files:

```powershell
python -c "import json,pathlib,subprocess,sys; root=pathlib.Path('plugins/ops/task-spec-hash/testdata'); script=r'C:\Users\Keisu\Projects\Notes\obsidian-note\4_Project\Multica-Agent-Strategy\Scripts\task_spec_hash.py'; [(lambda output,value: output.write_bytes((json.dumps(None if value == 'null' else value) + '\n').encode('utf-8')))(root.joinpath(p.name[:-11] + '.expected.json'), subprocess.run([sys.executable, script, '-'], input=json.loads(p.read_text(encoding='utf-8'))['text'].encode('utf-8'), capture_output=True, check=True).stdout.decode('utf-8').strip()) for p in sorted(root.glob('*.input.json'))]"
```

The expected results intentionally preserve the Python implementation's
section-selection behavior; this port does not revise the vault's policy.
