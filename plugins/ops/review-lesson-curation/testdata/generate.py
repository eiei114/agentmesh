"""Generate paired expectations by invoking the original Python functions."""

from datetime import datetime
from importlib import import_module
import json
from pathlib import Path
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(sys.argv[1]).resolve()))
source = import_module("review-lesson-curation")


def evaluate(request):
    match request["op"]:
        case "week_id":
            return source.week_id(datetime.fromisoformat(request["now"]))
        case "curate":
            snapshot, changed = source.curate(
                request["index"], request["previous"], datetime.fromisoformat(request["now"])
            )
            return {"curation": snapshot, "changed": changed}
        case "decide":
            return source.decide(request["stats"], pinned=request.get("pinned", False))
        case "is_visible":
            return source.is_visible(
                request["curation"], request["key"], request["stats"], request.get("project")
            )
        case _:
            raise ValueError(request["op"])


for path in sorted(Path(__file__).parent.glob("*.input.json")):
    expected = path.with_name(path.name.removesuffix(".input.json") + ".expected.json")
    expected.write_text(
        json.dumps(evaluate(json.loads(path.read_text(encoding="utf-8"))), indent=2) + "\n",
        encoding="utf-8",
        newline="\n",
    )
