"""Generate parity outputs from the owner vault's agentmesh_maintenance_requests.py."""

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
    / "agentmesh_maintenance_requests.py"
)


def load_module():
    spec = importlib.util.spec_from_file_location("agentmesh_maintenance_requests", SOURCE)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load source module: {SOURCE}")
    module = importlib.util.module_from_spec(spec)
    # The module defines a dataclass, which looks itself up in sys.modules.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def output_for(module, request):
    op = request["op"]
    try:
        if op == "slugify":
            return {"slug": module.slugify(request["value"])}
        if op == "patrol":
            action, reason = module.choose_patrol_action(
                repair_candidates=request["repair_candidates"],
                daily_exists=request["daily_exists"],
            )
            return {"action": action, "reason": reason}
        now = datetime.fromisoformat(request["now"])
        validated = module.validate_request_input(request["request"], now)
        if op == "validate":
            return validated
        if op == "render_source":
            return {
                "text": module._render_source(
                    validated, request["created_at"], request["issue_rel"]
                )
            }
        if op == "render_issue":
            return {"text": module._render_issue(validated, request["source_rel"])}
        raise RuntimeError(f"unknown op: {op}")
    except module.RequestError as err:
        return {"error": {"kind": "invalid_request", "message": str(err)}}


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
