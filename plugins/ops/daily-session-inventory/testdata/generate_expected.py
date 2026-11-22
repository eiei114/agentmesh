"""Generate parity outputs from the owner vault's daily-ai-kickoff.py.

The source module is loaded read-only. Only the pure group is exercised:
``daily_session_inventory_args`` and ``run_daily_session_inventory`` with a fake
runner that returns the fixture's synthetic result. The real runner
(``run_python``) is never called, so no script or subprocess runs.
"""

import importlib.util
import json
import sys
from datetime import datetime
from pathlib import Path


TESTDATA = Path(__file__).resolve().parent
SOURCE = (
    Path.home()
    / "Projects"
    / "Notes"
    / "obsidian-note"
    / "4_Project"
    / "Multica-Agent-Strategy"
    / "Scripts"
    / "daily-ai-kickoff.py"
)

# The text after this prefix is the Python json decoder's wording, which the
# Rust parser does not share. Both sides replace it before comparing.
INVALID_JSON_PREFIX = "invalid session inventory JSON: "


def load_module():
    spec = importlib.util.spec_from_file_location("daily_ai_kickoff", SOURCE)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load source module: {SOURCE}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def output_for(module, request):
    now = datetime.fromisoformat(request["now"])
    op = request["op"]
    calls = []
    if op == "args":

        def recorder(script, *args):
            calls.append((script, list(args)))
            return 0, "{}", ""

        module.run_daily_session_inventory(now, runner=recorder)
        script, args = calls[0]
        assert args == module.daily_session_inventory_args(now)
        return {"script": script, "args": args}
    if op == "decide":
        runner = request["runner"]

        def fake_runner(script, *args):
            calls.append((script, list(args)))
            return runner["status"], runner["stdout"], runner["stderr"]

        status, summary, stderr = module.run_daily_session_inventory(
            now, runner=fake_runner
        )
        if stderr.startswith(INVALID_JSON_PREFIX):
            stderr = INVALID_JSON_PREFIX + "<decoder detail>"
        return {"status": status, "summary": summary, "stderr": stderr}
    raise RuntimeError(f"unknown op: {op}")


def main():
    module = load_module()
    for input_path in sorted(TESTDATA.glob("*.input.json")):
        request = json.loads(input_path.read_text(encoding="utf-8"))
        output = output_for(module, request)
        expected_path = input_path.with_name(
            input_path.name.removesuffix(".input.json") + ".expected.json"
        )
        expected_path.write_text(
            json.dumps(output, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
            newline="\n",
        )
        print(f"generated {expected_path.name}")


if __name__ == "__main__":
    main()
