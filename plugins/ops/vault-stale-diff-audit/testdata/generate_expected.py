"""Run the original audit and finding against synthetic inputs only."""

import json
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(sys.argv[1]).resolve()))
import vault_stale_diff_audit as source


class SyntheticRoot:
    def __init__(self, request):
        self.label = request["vault_root"]
        self.mtimes = request["mtimes_unix_us"]

    def __str__(self):
        return self.label

    def __truediv__(self, path):
        mtime = self.mtimes.get(path)
        return SimpleNamespace(
            exists=lambda: mtime is not None,
            stat=lambda: SimpleNamespace(st_mtime=mtime / 1_000_000),
        )


def generate(request):
    epoch = datetime(1970, 1, 1, tzinfo=timezone.utc)
    now = epoch + timedelta(microseconds=request["now_unix_us"])
    kwargs = {k: request[k] for k in ("threshold_hours",) if k in request}
    with patch.object(
        source, "group_status_entries",
        return_value=(request["grouped"], request["excluded_paths"]),
    ):
        audit = source.audit_vault_stale_diff(
            SyntheticRoot(request), entries=[], now=now, **kwargs
        )
    return {"audit": audit, "finding": source.workflow_health_finding(audit)}


for path in sorted(Path(__file__).parent.glob("*.input.json")):
    request = json.loads(path.read_text(encoding="utf-8"))
    expected = path.with_name(path.name.replace(".input.json", ".expected.json"))
    expected.write_text(json.dumps(generate(request), indent=2) + "\n", encoding="utf-8", newline="\n")
