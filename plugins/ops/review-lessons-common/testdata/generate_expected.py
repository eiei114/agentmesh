"""Regenerate expected output from the vault implementation; never load evidence."""

import importlib
import json
import os
from pathlib import Path
import sys
from unittest.mock import patch


def main():
    sys.path.insert(0, os.environ["VAULT_SCRIPTS_DIR"])
    lessons = importlib.import_module("review-lessons-common")
    for path in sorted(Path(__file__).resolve().parent.rglob("*.input.json")):
        request = json.loads(path.read_text(encoding="utf-8"))
        if request["op"] == "stamp":
            result = lessons.stamp(request["value"])
            result = result.isoformat() if result else None
        elif request["op"] == "build_index":
            with patch.object(lessons, "REPO_TO_PROJECT_KEY_ALIASES", request["repo_aliases"]):
                result = lessons.build_index(
                    request["records"], request["registry"], lessons.stamp(request["now"])
                )
        else:
            raise ValueError(f"unknown fixture operation in {path}")
        expected = path.with_name(path.name.replace(".input.json", ".expected.json"))
        expected.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
