#!/usr/bin/env python3
"""Regenerate parity results from log_aware_token_efficiency_common.py."""
from __future__ import annotations

import json
from pathlib import Path

import log_aware_token_efficiency_common as common


TESTDATA = Path(__file__).resolve().parent

# Python raises on malformed metric values and wrong container shapes. The Rust
# port reports every such case with this exact `invalid_request` message.
MALFORMED = {"error": {"kind": "invalid_request", "message": "malformed input value"}}


def run(request: dict) -> object:
    op = request["op"]
    if op == "default_orphan_token_threshold":
        return common.DEFAULT_ORPHAN_TOKEN_THRESHOLD
    if op == "normalize_path":
        return common.normalize_path(request.get("value"))
    if op == "compact_orphan_session":
        return common._compact_orphan_session(request["item"])
    if op == "select_top_orphan_sessions":
        kwargs = {}
        if "top_n" in request:
            kwargs["top_n"] = request["top_n"]
        if "token_threshold" in request:
            kwargs["token_threshold"] = request["token_threshold"]
        return common.select_top_orphan_sessions(
            request["session_records"],
            matched_session_paths=set(request.get("matched_session_paths") or []),
            **kwargs,
        )
    if op == "build_orphan_session_observation_event":
        return common.build_orphan_session_observation_event(request["session"], ts=request["ts"])
    if op == "sum_event_field":
        return common._sum_event_field(request["summary"], request["controller"], request["field"])
    if op == "compare_log_aware_summaries":
        return common.compare_log_aware_summaries(request["baseline"], request["current"])
    raise ValueError(f"unknown op: {op}")


def main() -> None:
    for input_path in sorted(TESTDATA.glob("*.input.json")):
        request = json.loads(input_path.read_text(encoding="utf-8"))
        try:
            expected = run(request)
        except (ValueError, TypeError, AttributeError):
            expected = MALFORMED
        output_path = input_path.with_name(input_path.name.removesuffix(".input.json") + ".expected.json")
        with output_path.open("w", encoding="utf-8", newline="\n") as output_file:
            output_file.write(json.dumps(expected, ensure_ascii=False, indent=2) + "\n")
        print(output_path.name)


if __name__ == "__main__":
    main()
