"""Generate offline JSON pairs using the vault Python oracle, never its runner."""
import importlib
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(sys.argv[1]).resolve()))
native = importlib.import_module("native-stall-recovery")
lane = importlib.import_module("lane_health")

OUT = Path(__file__).resolve().parent
OLD = "2026-11-18T12:00:00+00:00"
MARKER = {"reason": native.BLOCK_REASON, "source_run": "i-run", "agent": "owner"}


def emit(name, status="failed", error=None, result=None, stamp=OLD, marker=None):
    run = dict(id="i-run", status=status, error=error, result=result)
    issue = dict(assignee_id="owner")
    request = dict(
        issue=issue,
        latest_run=run,
        timestamp=stamp,
        provider_match=bool(lane.INFRA.search(str(error or ""))),
        decoded_metadata=marker,
    )
    python_issue = {**issue, "metadata": {native.KEY: marker}}
    try:
        expected = dict(
            classification=native.classification(run),
            timestamp=native.timestamp(stamp).isoformat(),
            blocked_reason=native.blocked_reason(python_issue, run),
        )
    except ValueError as exc:
        if str(exc) != "native_stall_timestamp_unknown":
            raise
        expected = {"error": {"kind": "invalid_request", "message": str(exc)}}
    for suffix, doc in [("input", request), ("expected", expected)]:
        (OUT / f"{name}.{suffix}.json").write_text(
            json.dumps(doc, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n"
        )


# From test-native-stall-recovery.py:test_classifies_error_and_completed_holds.
for name, status, text in [
    ("provider-free", "failed", "Free usage limit"),
    ("provider-balance", "failed", "402 insufficient balance"),
    ("runtime-session", "failed", "pi session file p is already in use"),
    ("runtime-rpc", "failed", "pi RPC exited before agent_settled"),
    ("runtime-daemon", "cancelled", "daemon shut down"),
    ("sibling-hold", "completed", "Waiting for sibling run"),
    ("identity-hold", "completed", "Native identity mismatch"),
    ("completed-hold", "completed", "Run ended without a handoff"),
    ("terminal-failure", "failed", "Unrelated error"),
]:
    emit(name, status, text, {"output": text})

# Python re.I folds dotted/dotless I; Python \b treats marks as non-word
# and all Unicode numbers as word. Separators still count toward the gap.
for name, output in [
    ("identity-dotted-i", "\u0130dentity mismatch"),
    ("sibling-dotless-i", "S\u0131BLING"),
    ("sibling-mark-after-run", "他 run\u0301待"),
    ("sibling-mark-before-run", "他\u0301run 待"),
    ("sibling-number-after-run", "他 run²待"),
    ("sibling-underscore-before-run", "他_run 待"),
    ("sibling-newline-after-run", "他 run\n待"),
    ("sibling-gap-24", "他" + "." * 23 + " run " + "." * 23 + "待"),
    ("sibling-gap-25-before", "他" + "." * 24 + " run 待"),
    ("sibling-gap-25-after", "他 run " + "." * 24 + "待"),
]:
    emit(name, "completed", result={"output": output})
emit("runtime-dotless-i", error="daemon ex\u0131ted")

# Marker binding from the fake recovery tests, plus each mismatch separately.
emit("marker-match", marker=MARKER)
emit("marker-wrong-run", marker={**MARKER, "source_run": "other-run"})
emit("marker-wrong-agent", marker={**MARKER, "agent": "other-agent"})
emit("marker-wrong-reason", marker={**MARKER, "reason": "other-reason"})
emit("marker-non-object", marker=[MARKER])

emit("completed-precedence", "completed", "daemon shut down: 429", {"output": "他の run を待つ: identity mismatch"})
emit("completed-ignores-error", "completed", "Waiting for sibling; billing", "identity mismatch")
emit("runtime-precedence", error="Daemon Stopped: 429")
emit("timestamp-naive", stamp="2026-11-18T12:00:00")
emit("timestamp-date-only", stamp="2026-11-18")
emit("timestamp-invalid", stamp="2026-02-30T12:00:00Z")
emit("timestamp-missing", stamp=None)
emit("timestamp-utc", stamp="2026-11-18T12:00:00Z")
emit("timestamp-offset", stamp="2026-11-18 21:00:00.123456789+09:00")
emit("timestamp-basic", stamp="20261118T2100-0330")
emit("timestamp-week", stamp="2026-W47-3T12:00:00+00:00")
emit("timestamp-offset-seconds", stamp="2026-11-18T12:00:00-00:00:05.25")
