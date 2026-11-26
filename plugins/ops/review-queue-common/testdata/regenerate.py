"""Generate synthetic fixtures with the two Python reference functions only."""

import argparse
import ast
from collections import Counter
import json
from pathlib import Path


def cases():
    def rows(*overrides):
        return [
            {
                "key": f"SYN-{i}",
                "project": "synthetic-z",
                "role": "source",
                "status": "in_review",
                "reason": None,
                "issue_age_hours": None,
                **fields,
            }
            for i, fields in enumerate(overrides, 1)
        ]

    yield "complete", {"rows": rows(
        {"next_action": "stale-label", "inactivity_hours": 7.0},
        {"project": "synthetic-a", "reason": "unresolved_review_threads", "issue_age_hours": 2.5},
        {"status": "done", "independent_reviewed": True, "issue_age_hours": 0.0},
        {"status": "done", "issue_age_hours": 3.5},
        {"role": "child", "status": "in_review", "reason": "", "key": None},
        {"role": "child", "status": "done", "reason": "merge_conflict"},
        {"issue_age_hours": 1},
        {"project": "synthetic-a", "reason": "unresolved_review_threads"},
        {"role": "child", "reason": "merge_conflict", "issue_age_hours": 12.25},
        {"project": None, "reason": "", "status": None, "independent_reviewed": True},
    )}
    yield "budget", {"complete": True, "deferred": 6, "rows": rows(
        {"role": "child", "reason": "worker_still_active", "deferral_budget": "requests"},
        {"role": "child", "reason": "worker_still_active", "deferral_budget": {}},
        {"role": "child", "reason": "classification_deferred"},
        {"role": "child", "reason": "classification_time_deferred"},
        {"role": "child", "reason": "protected_publication_requires_separation"},
        {"role": "child", "reason": "publication_hold_invalid", "deferral_budget": {"budget": "time"}},
        {"role": "child", "reason": "publication_hold_invalid"},
        {"role": "child", "reason": None, "independent_reviewed": True},
    )}
    yield "errors", {"complete": False, "rows": []}
    yield "errors-and-budget", {"complete": False, "deferred": 2, "rows": rows(*(
        {"reason": reason, "independent_reviewed": True}
        for reason in (
            "review_repair_pending", "repair_worker_not_completed", "review_waiting_for_new_evidence",
            "independent_changes_requested", "no_action_requires_review", "no_linked_pr", "merge_conflict",
            "duplicate_repair_issue", "repair_inventory_incomplete", "two_repairs_exhausted",
            "sdk_example", "completion_example", "checkout_example", "unrecognized_synthetic_reason",
        )
    ))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", type=Path, help="path to vault review-queue-common.py")
    args = parser.parse_args()
    source = ast.parse(args.reference.read_text(encoding="utf-8"))
    names = {"next_action", "queue_summary"}
    functions = [node for node in source.body if isinstance(node, ast.FunctionDef) and node.name in names]
    if {node.name for node in functions} != names:
        parser.error("reference must define next_action and queue_summary")
    namespace = {"Counter": Counter}
    exec(compile(ast.Module(body=functions, type_ignores=[]), "review-queue-common.py", "exec"), namespace)

    directory = Path(__file__).resolve().parent
    for name, request in cases():
        # queue_summary annotates the original rows, so serialize input first.
        (directory / f"{name}.input.json").write_bytes((json.dumps(request, indent=2) + "\n").encode("utf-8"))
        result = namespace["queue_summary"](**request)
        (directory / f"{name}.expected.json").write_bytes((json.dumps(result, indent=2) + "\n").encode("utf-8"))


if __name__ == "__main__":
    main()
