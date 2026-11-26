"""Regenerate expected JSON using only the upstream offline scoring functions."""

import argparse
import json
from pathlib import Path
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scripts_dir", type=Path)
    args = parser.parse_args()
    sys.path.insert(0, str(args.scripts_dir.resolve()))
    import free_model_benchmarks as benchmarks

    operations = {
        "number": benchmarks.number,
        "collapse": benchmarks.collapse,
        "percentiles": benchmarks.percentiles,
        "match_model": benchmarks.match_model,
    }
    for path in sorted(Path(__file__).parent.rglob("*.input.json")):
        arguments = json.loads(path.read_text(encoding="utf-8"))
        operation = operations[arguments.pop("op")]
        try:
            result = operation(**arguments)
        except ValueError as error:
            # Normalize Python's two ValueError paths for non-numeric scores.
            message = str(error)
            if message.startswith("could not convert string to float:"):
                message = "missing/non-numeric score"
            result = {"error": {"kind": "invalid_request", "message": message}}
        path.with_name(path.name.replace(".input.json", ".expected.json")).write_text(
            json.dumps(result, sort_keys=True, indent=2, allow_nan=False) + "\n",
            encoding="utf-8",
            newline="\n",
        )


if __name__ == "__main__":
    main()
