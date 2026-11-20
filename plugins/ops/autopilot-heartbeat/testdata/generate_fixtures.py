#!/usr/bin/env python3
"""Generate public synthetic parity fixtures from the authoritative Python port."""
from __future__ import annotations

import argparse
import importlib.util
import json
import sys
from datetime import datetime
from pathlib import Path
from typing import Any


FIXTURES: dict[str, dict[str, Any]] = {
    "event-round-trip": {
        "operation": "event_round_trip",
        "event": {
            "event": "tool_in_flight",
            "phase": "fixture_phase",
            "controller": "fixture-controller",
            "ts": "2026-07-13T09:00:00Z",
            "detail": "synthetic progress ✓",
            "tool": "fixture_tool",
            "session_path": "synthetic/session.jsonl",
        },
    },
    "parse-offset-timestamp": {
        "operation": "parse_dt",
        "timestamp": "2026-07-13T09:00:00+02:00",
    },
    "parse-offset-seconds-timestamp": {
        "operation": "parse_dt",
        "timestamp": "2026-07-13T09:00:00+01:00:30",
    },
    "parse-malformed-timestamp": {
        "operation": "parse_dt",
        "timestamp": "2026-07-13T09:00:00+25:00",
    },
    "parse-heartbeat-lines": {
        "operation": "parse_heartbeat_lines",
        "text": (
            'noise\nMULTICA_HEARTBEAT {"controller":"fixture-controller",'
            '"detail":"line fixture","event":"progress",'
            '"phase":"fixture_phase","ts":"2026-07-13T09:00:00Z"}\n'
            'MULTICA_HEARTBEAT {malformed}'
        ),
    },
    "normal-long-running": {
        "operation": "analyze_watchdog_activity",
        "now": "2026-07-13T09:00:00Z",
        "session_progress": {
            "has_user_progress": True,
            "has_assistant_progress": False,
        },
        "heartbeats": [
            {
                "controller": "fixture-controller",
                "detail": "synthetic scan started",
                "event": "phase_start",
                "phase": "fixture_collect",
                "ts": "2026-07-13T08:45:10+00:00",
            },
            {
                "controller": "fixture-controller",
                "detail": "synthetic scan continuing",
                "event": "progress",
                "phase": "fixture_collect",
                "ts": "2026-07-13T08:50:10+00:00",
            },
            {
                "controller": "fixture-controller",
                "detail": "synthetic classifier running",
                "event": "progress",
                "phase": "fixture_classify",
                "ts": "2026-07-13T08:55:10+00:00",
            },
        ],
    },
    "tool-in-flight": {
        "operation": "analyze_watchdog_activity",
        "now": "2026-07-13T09:20:00Z",
        "heartbeats": [
            {
                "controller": "fixture-controller",
                "detail": "synthetic tool request",
                "event": "tool_in_flight",
                "phase": "fixture_post",
                "tool": "fixture_tool",
                "ts": "2026-07-13T09:10:00Z",
            },
            {
                "controller": "fixture-controller",
                "detail": "synthetic provider response wait",
                "event": "provider_wait",
                "phase": "fixture_draft",
                "ts": "2026-07-13T09:12:00Z",
            },
        ],
        "run_messages": [
            {
                "type": "tool_use",
                "tool": "fixture_tool",
                "created_at": "2026-07-13T09:12:00Z",
            }
        ],
    },
    "true-idle": {
        "operation": "is_watchdog_idle",
        "now": "2026-07-13T09:00:00Z",
        "heartbeats": [
            {
                "controller": "fixture-controller",
                "detail": "synthetic phase start",
                "event": "phase_start",
                "phase": "fixture_collect",
                "ts": "2026-07-13T07:00:00Z",
            }
        ],
    },
    "failure-detail": {
        "operation": "build_watchdog_failure_detail",
        "now": "2026-07-13T09:00:00Z",
        "session_path": "synthetic/session.jsonl",
        "heartbeats": [
            {
                "controller": "fixture-controller",
                "detail": "synthetic phase start",
                "event": "phase_start",
                "phase": "fixture_collect",
                "ts": "2026-07-13T07:00:00Z",
            }
        ],
    },
    "terminal-only": {
        "operation": "analyze_watchdog_activity",
        "now": "2026-07-13T09:00:00Z",
        "heartbeats": [
            {
                "controller": "fixture-controller",
                "detail": "synthetic terminal result",
                "event": "terminal_result",
                "phase": "fixture_done",
                "ts": "2026-07-13T08:59:00Z",
            }
        ],
    },
    "exact-threshold": {
        "operation": "analyze_watchdog_activity",
        "now": "2026-07-13T09:00:00Z",
        "threshold_seconds": 900,
        "heartbeats": [
            {
                "controller": "fixture-controller",
                "event": "phase_start",
                "phase": "fixture_collect",
                "ts": "2026-07-13T08:45:00Z",
            }
        ],
    },
    "future-time-clamp": {
        "operation": "analyze_watchdog_activity",
        "now": "2026-07-13T09:00:00Z",
        "heartbeats": [
            {
                "controller": "fixture-controller",
                "event": "progress",
                "phase": "fixture_future",
                "ts": "2026-07-13T09:00:30Z",
            }
        ],
    },
    "events-outside-last-three": {
        "operation": "analyze_watchdog_activity",
        "now": "2026-07-13T09:00:00Z",
        "heartbeats": [
            {"controller": "fixture-controller", "event": "tool_in_flight", "phase": "a", "ts": "2026-07-13T08:59:00Z"},
            {"controller": "fixture-controller", "event": "provider_wait", "phase": "b", "ts": "2026-07-13T08:59:10Z"},
            {"controller": "fixture-controller", "event": "progress", "phase": "c", "ts": "2026-07-13T08:59:20Z"},
            {"controller": "fixture-controller", "event": "progress", "phase": "d", "ts": "2026-07-13T08:59:30Z"},
            {"controller": "fixture-controller", "event": "progress", "phase": "e", "ts": "2026-07-13T08:59:40Z"},
        ],
    },
    "tool-heartbeat-within-last-three": {
        "operation": "analyze_watchdog_activity",
        "now": "2026-07-13T09:00:00Z",
        "heartbeats": [
            {"controller": "fixture-controller", "event": "progress", "phase": "a", "ts": "2026-07-13T08:59:10Z"},
            {"controller": "fixture-controller", "event": "tool_in_flight", "phase": "b", "ts": "2026-07-13T08:59:20Z"},
            {"controller": "fixture-controller", "event": "progress", "phase": "c", "ts": "2026-07-13T08:59:30Z"},
        ],
    },
    "provider-wait-within-last-three": {
        "operation": "analyze_watchdog_activity",
        "now": "2026-07-13T09:00:00Z",
        "heartbeats": [
            {"controller": "fixture-controller", "event": "progress", "phase": "a", "ts": "2026-07-13T08:59:10Z"},
            {"controller": "fixture-controller", "event": "provider_wait", "phase": "b", "ts": "2026-07-13T08:59:20Z"},
            {"controller": "fixture-controller", "event": "progress", "phase": "c", "ts": "2026-07-13T08:59:30Z"},
        ],
    },
    "activity-helpers": {
        "operation": "activity_helpers",
        "heartbeats": [
            {"controller": "fixture-controller", "event": "terminal_result", "phase": "a", "ts": "2026-07-13T08:59:50Z"},
            {"controller": "fixture-controller", "event": "progress", "phase": "b", "ts": "2026-07-13T08:59:40Z"},
        ],
        "run_messages": [
            {"type": "assistant", "created_at": "2026-07-13T08:58:00Z"},
            {"type": "assistant", "created_at": "2026-07-13T08:59:30Z"},
        ],
    },
}


def load_python_implementation(path: Path) -> Any:
    spec = importlib.util.spec_from_file_location("autopilot_heartbeat", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot import Python implementation: {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def parse_events(hb: Any, request: dict[str, Any]) -> list[Any]:
    events = []
    lines = request.get("heartbeat_lines")
    if isinstance(lines, str):
        events.extend(hb.parse_heartbeat_lines(lines))
    elif isinstance(lines, list):
        events.extend(event for line in lines if (event := hb.HeartbeatEvent.from_line(line)))
    for value in request.get("heartbeats", []):
        event = hb.HeartbeatEvent.from_line(value) if isinstance(value, str) else hb.HeartbeatEvent.from_dict(value)
        if event:
            events.append(event)
    return events


def expected(hb: Any, request: dict[str, Any]) -> Any:
    operation = request.get("operation", "analyze_watchdog_activity")
    if operation == "parse_dt":
        date = hb.parse_dt(request.get("timestamp"))
        return date.isoformat() if date else None
    if operation == "heartbeat_event_from_dict":
        event = hb.HeartbeatEvent.from_dict(request.get("event", {}))
        return event.to_dict() if event else None
    if operation == "heartbeat_event_from_line":
        event = hb.HeartbeatEvent.from_line(request.get("line", ""))
        return event.to_dict() if event else None
    if operation == "event_round_trip":
        event = hb.HeartbeatEvent.from_dict(request["event"])
        assert event is not None
        line = event.to_line()
        parsed = hb.HeartbeatEvent.from_line(line)
        assert parsed is not None
        return {"event": event.to_dict(), "line": line, "parsed": parsed.to_dict()}
    if operation == "parse_heartbeat_lines":
        return [event.to_dict() for event in hb.parse_heartbeat_lines(request["text"])]
    heartbeats = parse_events(hb, request)
    messages = request.get("run_messages") or []
    if operation == "activity_helpers":
        latest = hb.latest_heartbeat(heartbeats)
        hb_at = hb.heartbeat_activity_at(heartbeats)
        msg_at = hb.run_message_activity_at(messages)
        return {
            "latest_heartbeat": latest.to_dict() if latest else None,
            "heartbeat_activity_at": hb_at.isoformat() if hb_at else None,
            "run_message_activity_at": msg_at.isoformat() if msg_at else None,
        }
    now = hb.parse_dt(request["now"])
    assert now is not None
    kwargs = {
        "heartbeats": heartbeats,
        "run_messages": messages,
        "session_progress": request.get("session_progress"),
        "now": now,
        "threshold_seconds": request.get("threshold_seconds", hb.DEFAULT_WATCHDOG_IDLE_THRESHOLD_SECONDS),
    }
    if operation == "is_watchdog_idle":
        return hb.is_watchdog_idle(**kwargs)
    if operation == "build_watchdog_failure_detail":
        return hb.build_watchdog_failure_detail(session_path=request.get("session_path"), **kwargs)
    return hb.analyze_watchdog_activity(**kwargs)


def write_json(path: Path, value: Any) -> None:
    text = json.dumps(value, ensure_ascii=False, indent=2) + "\n"
    with path.open("w", encoding="utf-8", newline="\n") as fixture:
        fixture.write(text)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--implementation", type=Path, required=True)
    args = parser.parse_args()
    hb = load_python_implementation(args.implementation)
    directory = Path(__file__).parent
    for name, request in FIXTURES.items():
        write_json(directory / f"{name}.input.json", request)
        result = expected(hb, request)
        write_json(directory / f"{name}.expected.json", result)
    print(f"generated {len(FIXTURES)} Python parity fixtures")


if __name__ == "__main__":
    main()
