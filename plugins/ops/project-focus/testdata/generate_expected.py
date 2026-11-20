"""Generate parity outputs from the owner vault's project_focus.py module."""

import importlib.util
import json
import tempfile
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
    / "project_focus.py"
)


def load_module():
    spec = importlib.util.spec_from_file_location("project_focus", SOURCE)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load source module: {SOURCE}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def output_for(module, request):
    with tempfile.TemporaryDirectory(prefix="project-focus-parity-") as temp_dir:
        policy_path = Path(temp_dir) / "policy.json"
        policy_text = request["policy_text"]
        if policy_text is not None:
            policy_path.write_text(policy_text, encoding="utf-8")
        policy = module.load(policy_path)

    issue = request["issue"]
    counts = module.count_by_project(request["issues"], set(request["statuses"]))
    pacing = module.paced_out(issue, policy, counts, request["limit_key"])
    return {
        "policy": {
            "focus": sorted(policy["focus"]),
            "paced": policy["paced"],
        },
        "project_id": module.project_of(issue),
        "focus_rank": module.focus_rank(issue, policy),
        "counts": dict(counts),
        "paced_out": pacing,
    }


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
