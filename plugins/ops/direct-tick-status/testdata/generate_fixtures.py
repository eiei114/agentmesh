"""Call the original assessor with synthetic inputs; never call its collectors."""
import argparse
import importlib.util
import json
from datetime import datetime
from pathlib import Path
import sys


def cases():
    spec = {"key": "synthetic", "task": "Synthetic-Tick", "interval": 10,
            "limit": 30, "hours": [9]}
    now = "2026-06-01T00:45:00+00:00"  # 09:45 JST
    old = {"finished_at": "2026-06-01T00:00:00+00:00", "ok": True}
    ready = {"state": "Ready", "last_result": 0}

    def running(start):
        return {"state": "Running", "last_run": start}

    def inputs(rows, task, *, now=now, spec=spec):
        return dict(spec=spec, rows=rows, task=task, now=now)

    yield "missing", inputs([], None)
    yield "stale-inside", inputs([old], ready)
    yield "stale-outside", inputs([old], ready, now="2026-06-01T01:45:00Z")
    yield "stale-boundary", inputs([old], ready, now="2026-06-01T00:25:00Z")
    yield "in-flight-timeout", inputs([old], running("2026-06-01T09:14:59.9999999"))
    yield "in-flight-limit-boundary", inputs([old], running("2026-06-01T09:15:00"))
    yield "offset-seven-digit-running", inputs(
        [old], running("2026-05-31T19:30:00.1234567-05:00"))
    yield "offset-seven-digit-equal-finish", inputs(
        [{**old, "finished_at": "2026-06-01T00:30:00.123456+00:00"}],
        running("2026-05-31T19:30:00.1234567-05:00"))
    yield "invalid-scheduler-stamp", inputs([old], running("not a timestamp"))
    yield "degraded-with-recovery-hold", inputs(
        [old, {"finished_at": "2026-06-01T00:40:00Z", "ok": 1, "health": "retry",
               "repair_recovery": {"held": [{"source": "synthetic-source"}]}}],
        ready, spec={**spec, "key": "work-queue"})
    yield "failures-before-running", inputs(
        [old, {"finished_at": "2026-06-01T00:01:00Z", "ok": False},
         {"finished_at": "2026-06-01T00:02:00Z", "health": "review_triggered"},
         {"finished_at": "2026-06-01T00:03:00Z", "ok": False, "reason": "失" * 201}],
        running("2026-06-01T09:30:00+09:00"), spec={**spec, "key": "review-sentinel"})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--implementation", required=True, type=Path)
    source = parser.parse_args().implementation.resolve(strict=True)
    sys.path.insert(0, str(source.parent))
    module_spec = importlib.util.spec_from_file_location("direct_tick_status", source)
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)

    directory = Path(__file__).resolve().parent
    for name, request in cases():
        assessment = module.assess(request["spec"], request["rows"], request["task"],
                                   datetime.fromisoformat(request["now"]))
        response = {"assessment": assessment, "findings": module.findings({
            "controllers": [assessment]})}
        for suffix, value in [("input", request), ("expected", response)]:
            (directory / f"{name}.{suffix}.json").write_text(
                json.dumps(value, ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
