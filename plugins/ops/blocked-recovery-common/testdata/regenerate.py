"""Generate expectations with the original Python readers, not live policy."""

import json
from pathlib import Path
import sys
from unittest.mock import patch


def main(scripts: Path) -> None:
    sys.path.insert(0, str(scripts.resolve()))
    import blocked_recovery_common as source

    for path in sorted(Path(__file__).parent.glob("*.input.json")):
        request = json.loads(path.read_text(encoding="utf-8"))
        if request["op"] == "owns_blocked":
            # Supply exactly the sweeper object the loader would extract. Only
            # the loader is stubbed; owns_blocked still runs the Python checks.
            with patch.object(source, "sweep_policy", return_value=request["policy"].get("sweeper", {})):
                result = source.owns_blocked(None)
        elif request["op"] == "assessment_context":
            result = source.assessment_context(request["issue"])
        else:
            raise ValueError(f"unknown operation in {path.name}")
        expected = path.with_name(path.name.removesuffix(".input.json") + ".expected.json")
        expected.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main(Path(sys.argv[1]))
