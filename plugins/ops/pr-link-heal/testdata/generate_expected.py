"""Capture PR-link-heal decisions and callback intents from the Python source."""

import importlib.util
import json
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
    / "pr_link_heal.py"
)


def load_module():
    spec = importlib.util.spec_from_file_location("pr_link_heal", SOURCE)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load source module: {SOURCE}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def output_for(module, request):
    calls = []
    edits = []
    metadata = []

    def gh(args):
        calls.append({"args": args})
        if args[:2] == ["pr", "list"]:
            return request["pr_search_results"]
        if args[:2] == ["pr", "view"]:
            return request["pr_view"]
        raise AssertionError(f"unexpected synthetic gh request: {args!r}")

    def edit(repo, number, title):
        edits.append({"repo": repo, "number": number, "title": title})

    def set_md(issue_id, key, value):
        metadata.append({"issue_id": issue_id, "key": key, "value": value})

    result = module.attempt(
        request["issue"],
        gh=gh,
        edit=edit,
        set_md=set_md,
        now=datetime.fromisoformat(request["now"]),
    )
    return {
        "result": result,
        "actions": {
            "github_calls": calls,
            "edit": edits,
            "set_metadata": metadata,
        },
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
