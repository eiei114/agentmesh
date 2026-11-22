# Daily session inventory parity fixtures

The expected files are generated from the vault's `daily-ai-kickoff.py`
(`daily_session_inventory_args` and `run_daily_session_inventory`). Each
`<name>.input.json` is a stdio request with an `op` field:

- `args` with `now` returns `{"script", "args"}`: the runner script name and
  the argument list Python passes to it.
- `decide` with `now` and a synthetic `runner` (`status`, `stdout`, `stderr`)
  returns `{"status", "summary", "stderr"}`, the tuple
  `run_daily_session_inventory` returns.

The generator drives the real Python functions with a fake runner that returns
the fixture's synthetic result, so `run_python` is never called and no script,
subprocess, file, Multica, network or git operation runs.

From the repository root, run the exact generator command:

```sh
python plugins/ops/daily-session-inventory/testdata/generate_expected.py
```

The generator imports the source module from
`~/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts/`.
It needs no vault data: every input is a synthetic timestamp or runner result.
The input files are hand-authored and committed; only the expected files are
regenerated. The runner-result cases mirror the vault's
`test_daily_session_inventory_flow.py`.

Inputs cover:

- UTC window arguments for JST mornings, UTC midnight, `Z`, negative and
  half-hour offsets, microseconds, month, year and leap-day boundaries;
- timestamps one second before, at, and one second after JST midnight, and the
  same instants written in UTC;
- a compact observation result, extra payload fields and stderr pass-through;
- a nonzero runner status (stdout ignored, summary empty, stderr kept);
- malformed, truncated and whitespace-only JSON, which fail closed with status
  2 and `invalid session inventory JSON: ...`;
- empty stdout and a missing, null, empty or non-object observation payload.

Known differences, kept out of the fixtures on purpose:

- The text after `invalid session inventory JSON: ` is the JSON decoder's own
  wording, so the generator and the Rust test both replace it with
  `<decoder detail>` before comparing.
- `now` must carry a UTC offset. Python reads an offset-less timestamp in the
  machine's zone; the port rejects it as an invalid request.
- Runner stdout that is valid JSON but not an object makes Python raise
  `AttributeError`; the port returns an invalid request. Python also accepts
  `NaN` and `Infinity` literals, which serde_json rejects.
