"""Generate synthetic expected results without running the steward CLI."""

import json
from pathlib import Path

from vault_steward import BucketPlan, build_result


def main():
    directory = Path(__file__).resolve().parent
    for source in sorted(directory.glob("*.input.json")):
        request = json.loads(source.read_text(encoding="utf-8"))
        request["root"] = Path(request["root"])
        request["plans"] = [
            BucketPlan(plan["bucket_id"], tuple(plan["files"]), plan["message"])
            for plan in request["plans"]
        ]
        result = build_result(**request)
        output = source.with_name(source.name.removesuffix(".input.json") + ".expected.json")
        output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n",
                          encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
