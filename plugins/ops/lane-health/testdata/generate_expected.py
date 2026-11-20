"""Generate parity outputs by calling the vault's lane_health.py functions."""

import importlib.util
import json
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
    / "lane_health.py"
)


def load_module():
    spec = importlib.util.spec_from_file_location("lane_health", SOURCE)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load the vault lane_health.py module")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def output_for(module, request):
    operation = request["operation"]
    if operation == "parse":
        parsed = module.parse(request["value"])
        return {"parsed": parsed.isoformat() if parsed else None}
    if operation == "since_config":
        return {"tasks": module.since_config(request["tasks"], request["updated_at"])}
    if operation == "infra_failure":
        return {
            "reason": module.infra_failure(
                request["tasks"], module.parse(request["now"])
            )
        }
    if operation == "checker":
        tasks = request["tasks"]
        try:
            tasks = module.since_config(tasks, request.get("updated_at"))
            hold = module.infra_failure(tasks, module.parse(request["now"]))
        except Exception:
            # lane_health.checker treats an unreadable history as a fail-open.
            hold = None
        return {"hold": hold}
    raise ValueError(f"unsupported operation: {operation}")


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
