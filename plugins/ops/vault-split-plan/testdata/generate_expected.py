#!/usr/bin/env python3
"""Regenerate parity results from vault_split_commit_common.py."""
from __future__ import annotations

import json
from datetime import date
from pathlib import Path

import vault_split_commit_common as common


TESTDATA = Path(__file__).resolve().parent


def run(request: dict) -> object:
    op = request["op"]
    if op == "normalize_path":
        return common.normalize_path(request["path"])
    if op == "is_excluded_path":
        return common.is_excluded_path(request["path"])
    if op == "exclude_paths":
        return [common.is_excluded_path(path) for path in request["paths"]]
    if op == "classify_path":
        return common.classify_path(request["path"])
    if op == "parse_porcelain_line":
        entry = common.parse_porcelain_line(request["line"])
        return None if entry is None else {"path": entry.path, "raw": entry.raw}
    if op == "parse_porcelain_lines":
        results = []
        for line in request["lines"]:
            entry = common.parse_porcelain_line(line)
            results.append(None if entry is None else {"path": entry.path, "raw": entry.raw})
        return results
    if op == "agents_md_contains_multica_runtime":
        return common.agents_md_contains_multica_runtime(request["content"])
    if op == "strip_multica_runtime_block":
        content, stripped = common.strip_multica_runtime_block(request["content"])
        return {"content": content, "stripped": stripped}
    entries = [common.StatusEntry(path=row["path"], raw=row["raw"]) for row in request["entries"]]
    if op == "group_status_entries":
        groups, excluded = common.group_status_entries(entries)
        return {"groups": groups, "excluded": excluded}
    if op == "build_bucket_plans":
        today = date.fromisoformat(request["today"]) if request.get("today") else None
        plans = common.build_bucket_plans(entries, today=today)
        _, excluded = common.group_status_entries(entries)
        return {
            "plans": [
                {"bucket_id": plan.bucket_id, "files": plan.files, "message": plan.message}
                for plan in plans
            ],
            "excluded": excluded,
        }
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
