#!/usr/bin/env python3
"""Generate parity outputs by invoking the vault's Python implementation."""

from __future__ import annotations

import dataclasses
import json
import tempfile
from pathlib import Path

from managed_projects_registry import RegistryValidationError, load_managed_projects


TESTDATA = Path(__file__).resolve().parent


def run_case(request: dict[str, object]) -> dict[str, object]:
    markdown = request["registry_markdown"]
    if not isinstance(markdown, str):
        raise TypeError("registry_markdown must be a string")
    with tempfile.TemporaryDirectory(prefix="managed-projects-registry-parity-") as temp:
        root = Path(temp)
        (root / "registry.md").write_text(markdown, encoding="utf-8")
        try:
            projects = load_managed_projects(root, Path("registry.md"))
        except RegistryValidationError as error:
            return {"error": {"kind": "failed", "message": str(error)}}
    return {key: dataclasses.asdict(project) for key, project in projects.items()}


def main() -> None:
    for input_path in sorted(TESTDATA.glob("*.input.json")):
        request = json.loads(input_path.read_text(encoding="utf-8"))
        expected_path = input_path.with_name(input_path.name.replace(".input.json", ".expected.json"))
        with expected_path.open("w", encoding="utf-8", newline="\n") as expected_file:
            expected_file.write(
                json.dumps(run_case(request), ensure_ascii=False, indent=2, sort_keys=True) + "\n"
            )
        print(expected_path.name)


if __name__ == "__main__":
    main()
