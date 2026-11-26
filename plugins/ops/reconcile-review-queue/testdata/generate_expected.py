"""Run only the Python decision helpers with synthetic, read-only callbacks."""

import argparse
import importlib.util
import json
from pathlib import Path
import sys


TESTDATA = Path(__file__).resolve().parent
DEFAULT_SCRIPTS = (
    Path.home() / "Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts"
)


def load_reference(scripts):
    sys.path.insert(0, str(scripts))
    spec = importlib.util.spec_from_file_location("rrq", scripts / "reconcile-review-queue.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def decide(module, request):
    class ReadOnlyBoard:
        def m(self, *args):
            if args[:2] == ("issue", "list"):
                return {"issues": request["siblings"]}
            if args[:2] == ("issue", "runs"):
                return request["runs"]
            raise AssertionError(f"unexpected Board call: {args!r}")

    def view(args):
        assert args[:3] == ["gh", "pr", "view"], args
        return request["pr_view"]

    module.sh_json = view
    board = ReadOnlyBoard()
    issue, md = request["issue"], request["metadata"]
    if request["operation"] == "superseded_by":
        return module.superseded_by(board, issue, md)
    assert request["operation"] == "orphan_repair"
    return module.orphan_repair(board, request["status"], issue, md)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scripts", type=Path, default=DEFAULT_SCRIPTS)
    module = load_reference(parser.parse_args().scripts.resolve())
    for path in sorted(TESTDATA.glob("*.input.json")):
        request = json.loads(path.read_text(encoding="utf-8"))
        expected = path.with_name(path.name.removesuffix(".input.json") + ".expected.json")
        expected.write_text(json.dumps(decide(module, request), ensure_ascii=False) + "\n",
                            encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
