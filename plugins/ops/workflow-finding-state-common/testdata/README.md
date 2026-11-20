# Workflow-finding state parity fixtures

Expected JSON is generated from the authoritative vault Python implementation
and only synthetic findings/events. From this crate directory, regenerate with:

```powershell
python .\testdata\generate_expected.py --python-source "C:\Users\Keisu\Projects\Notes\obsidian-note\4_Project\Multica-Agent-Strategy\Scripts\workflow_finding_state_common.py"
```

The corpus reuses the Python tests' resolved-to-new, acknowledgement,
volatile-evidence fingerprint, and multi-month history-window cases. Additional
cases cover malformed timestamps and JSONL, Python's short timezone offset and
basic date/time spelling, unrelated controllers, and duplicate fingerprints
where the last matching event wins. The generator loads
the target Python module with its event-directory dependency stubbed to a
synthetic relative path; event lines are written only to a temporary directory
by the Python oracle. The history fold receives caller-ordered lines
representing the sorted source files and their line order.
