#!/usr/bin/env python3
"""Regenerate parity results from escape_sensitive_edits.py."""
from __future__ import annotations

import json
from pathlib import Path

import escape_sensitive_edits as common


TESTDATA = Path(__file__).resolve().parent


def run(request: dict) -> object:
    op = request["op"]
    if op == "is_js_ts_path":
        return common.is_js_ts_path(request["path"])
    if op == "command_targets_js_ts":
        return common.command_targets_js_ts(request.get("command"))
    if op == "command_mentions_escape_literal":
        return common.command_mentions_escape_literal(request.get("command"))
    if op == "flag_unsafe_js_ts_shell_edit":
        return [list(pair) for pair in common.flag_unsafe_js_ts_shell_edit(request.get("command"))]
    if op == "scan_physical_newline_in_strings":
        return common._scan_physical_newline_in_strings(request["content"])
    if op == "scan_escape_corruption":
        return common.scan_escape_corruption(request["content"], request.get("path") or "")
    raise ValueError(f"unknown op: {op}")


def main() -> None:
    for input_path in sorted(TESTDATA.glob("*.input.json")):
        request = json.loads(input_path.read_text(encoding="utf-8"))
        expected = run(request)
        output_path = input_path.with_name(input_path.name.removesuffix(".input.json") + ".expected.json")
        with output_path.open("w", encoding="utf-8", newline="\n") as output_file:
            output_file.write(json.dumps(expected, ensure_ascii=False, indent=2) + "\n")
        print(output_path.name)


if __name__ == "__main__":
    main()
