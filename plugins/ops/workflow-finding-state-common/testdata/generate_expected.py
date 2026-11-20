#!/usr/bin/env python3
"""Generate parity outputs with the authoritative vault Python module."""

from __future__ import annotations

import argparse
import importlib.util
import json
import sys
import tempfile
import types
from pathlib import Path


def load_authority(source_path: Path):
    sys.path.insert(0, str(source_path.parent))
    # The source imports this module only for its event-directory constant.
    # Keep oracle runs isolated from the rest of the vault and real event data.
    dependency = types.ModuleType("morning_news_common")
    dependency.FEEDBACK_EVENTS_DIR = Path("synthetic-feedback-events")
    sys.modules["morning_news_common"] = dependency
    spec = importlib.util.spec_from_file_location("workflow_finding_state_common", source_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot import Python source: {source_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--python-source", required=True, type=Path)
    args = parser.parse_args()
    authority = load_authority(args.python_source.resolve())
    testdata = Path(__file__).parent
    for input_path in sorted(testdata.glob("*.input.json")):
        request = json.loads(input_path.read_text(encoding="utf-8"))
        operation = request["operation"]
        if operation == "parse_event_dt":
            parsed = authority.parse_event_dt(request.get("value"))
            result = {"value": parsed.isoformat() if parsed is not None else None}
        elif operation == "finding_fingerprint":
            result = authority.finding_fingerprint(request["finding"])
        elif operation == "current_finding_state":
            result = authority.current_finding_state(request.get("previous"))
        elif operation == "prior_finding_state":
            since = authority.parse_event_dt(request["since"])
            if since is None:
                raise ValueError("fixture since must be a valid timestamp")
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                event_dir = root / authority.FEEDBACK_EVENTS_DIR
                event_dir.mkdir(parents=True)
                (event_dir / "synthetic.jsonl").write_text(
                    "\n".join(request["event_lines"]), encoding="utf-8"
                )
                result = authority.prior_finding_state(root, since=since)
        else:
            raise ValueError(f"unknown fixture operation: {operation}")
        output_path = input_path.with_name(input_path.name.replace(".input.json", ".expected.json"))
        with output_path.open("w", encoding="utf-8", newline="\n") as output_file:
            output_file.write(
                json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
            )
        print(f"wrote {output_path.name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
