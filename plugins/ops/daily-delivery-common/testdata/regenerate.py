"""Regenerate expected JSON from the authoritative Python module, not Rust."""

import argparse
from datetime import datetime
import json
from pathlib import Path
import sys
from types import SimpleNamespace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scripts_dir", type=Path)
    args = parser.parse_args()
    sys.path.insert(0, str(args.scripts_dir.resolve(strict=True)))
    from daily_delivery_common import daily_target, managed_delivery_scope, sdk_report_reason

    def evaluate(request):
        match request.pop("op"):
            case "managed_delivery_scope":
                project = request.get("project")
                request["project"] = None if project is None else SimpleNamespace(**project)
                return managed_delivery_scope(**request)
            case "daily_target":
                for field in ("now", "deadline"):
                    request[field] = datetime.fromisoformat(request[field].replace("Z", "+00:00"))
                return daily_target(**request)
            case "sdk_report_reason":
                return sdk_report_reason(**request)
            case name:
                raise ValueError(f"unknown op: {name}")

    inputs = sorted(Path(__file__).resolve().parent.glob("*.input.json"))
    if not inputs:
        raise ValueError("no input fixtures")
    for path in inputs:
        result = evaluate(json.loads(path.read_text(encoding="utf-8")))
        expected = path.with_name(path.name.removesuffix(".input.json") + ".expected.json")
        expected.write_bytes((json.dumps(result, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))
    print(f"Generated {len(inputs)} Python expectations")


if __name__ == "__main__":
    main()
