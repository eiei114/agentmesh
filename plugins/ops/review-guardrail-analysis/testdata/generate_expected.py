#!/usr/bin/env python3
"""Regenerate parity results from report-review-guardrail-promotions.py."""
from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path


TESTDATA = Path(__file__).resolve().parent

# Python raises on malformed values and wrong container shapes. The Rust port
# reports every such case with this exact `invalid_request` message.
MALFORMED = {"error": {"kind": "invalid_request", "message": "malformed input value"}}


def load_module():
    scripts = Path(os.environ["VAULT_SCRIPTS_DIR"])
    spec = importlib.util.spec_from_file_location(
        "report_review_guardrail_promotions",
        scripts / "report-review-guardrail-promotions.py",
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run(module, request: dict) -> object:
    op = request["op"]
    if op == "parse_time":
        return module.parse_time(request["value"]).isoformat()
    if op == "occurrence_time":
        when = module.occurrence_time(request["item"])
        return None if when is None else when.isoformat()
    if op == "analyze":
        return module.analyze(request["records"], request["registry"], module.parse_time(request["now"]))
    raise ValueError(f"unknown op: {op}")


def main() -> None:
    module = load_module()
    for input_path in sorted(TESTDATA.glob("*.input.json")):
        request = json.loads(input_path.read_text(encoding="utf-8"))
        try:
            expected = run(module, request)
        except (ValueError, TypeError, AttributeError, KeyError):
            expected = MALFORMED
        output_path = input_path.with_name(input_path.name.removesuffix(".input.json") + ".expected.json")
        with output_path.open("w", encoding="utf-8", newline="\n") as output_file:
            output_file.write(json.dumps(expected, ensure_ascii=False, indent=2) + "\n")
        print(output_path.name)


if __name__ == "__main__":
    main()
