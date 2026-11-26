#!/usr/bin/env python3
"""Generate synthetic request/result pairs using the original Python subject plan."""

import argparse
from copy import deepcopy
import importlib.util
import json
from pathlib import Path
import sys
from urllib.parse import urlparse

HERE = Path(__file__).resolve().parent
HEAD = "a" * 40
REPO = "fixture-owner/synthetic-demo"
URL = f"https://github.com/{REPO}/pull/1"
OTHER_URL = f"https://github.com/{REPO}/pull/2"


def baseline():
    # Adapted from CompletionReceiptTests.setUp, without journal or command data.
    source = {"id": "source", "project_id": "project", "status": "in_review",
              "metadata": {"work_owner": "ai"}}
    producer = {**source, "id": "child", "metadata": {
        "work_owner": "ai", "task_kind": "review_fix", "source_issue_id": "source",
        "source_pr_url": URL, "dedupe_key": "managed-review-fix:source:old",
    }}
    receipt = {"schema_version": 1, "source_issue_id": "source", "producer_issue_id": "child",
               "producer_run_id": "run", "project_id": "project", "pr_url": URL, "head_sha": HEAD}
    context = {
        "source": source, "producer": producer, "project_id": "project", "project_status": "planned",
        "producer_runs": [{"id": "run", "status": "completed", "created_at": "2026-10-12T00:00:00Z"}],
        "source_runs": [], "repo": REPO, "default_branch": "main", "owner_allowlist": ["fixture-owner"],
        "pr": {"url": URL, "headRefOid": HEAD, "state": "OPEN", "baseRefName": "main",
               "isDraft": False, "author": {"login": "fixture-owner"}},
        "links": [{"html_url": URL}],
    }
    return {"receipt": receipt, "context": context}


def fixtures():
    def case(name, *changes):
        request = baseline()
        for path, value in changes:
            *parents, key = path.split(".")
            target = request
            for parent in parents:
                target = target[parent]
            target[key] = value
        return name, request

    run = baseline()["context"]["producer_runs"][0]
    stale_child = ("context.producer.metadata.source_pr_url", OTHER_URL)
    bad_head = ("context.pr.headRefOid", "d" * 40)
    yield case("managed-child-pass")
    yield case("direct-producer-pass", ("receipt.producer_issue_id", "source"),
               ("context.producer", baseline()["context"]["source"]))
    yield case("done-child-merged-pr-pass", ("context.producer.status", "done"),
               ("context.pr.state", "MERGED"), ("context.project_status", "in_progress"))
    yield case("missing-receipt", ("receipt", None), ("context.source", {}))
    yield case("boolean-schema", ("receipt.schema_version", True))
    yield case("float-schema", ("receipt.schema_version", 1.0))
    yield case("subject-before-project", ("receipt.source_issue_id", "other"),
               ("receipt.project_id", "other"))
    yield case("project-before-safety", ("context.producer.project_id", "other"),
               ("context.source.metadata.work_owner", "human"))
    yield case("source-safety-before-producer-and-status", ("context.source.metadata.work_owner", "human"),
               ("context.producer.metadata.human_review_required", True), ("context.source.status", "done"))
    yield case("protected-producer", ("context.producer.metadata.human_review_required", True))
    yield case("status-before-child-contract", ("context.source.status", "done"),
               ("context.producer.metadata.task_kind", "other"))
    yield case("child-contract-before-runs", ("context.producer.metadata.dedupe_key", "blocked-recovery:source:head"),
               ("context.producer_runs", []))
    yield case("child-contract-wrong-parent", ("context.producer.metadata.source_issue_id", "other"))
    for status in ("running", "queued", "pending", "in_progress"):
        yield case(f"producer-{status}-before-source", ("context.producer_runs", [run, {"status": status}]),
                   ("context.source_runs", [{"status": "running"}]))
    yield case("stale-run", ("context.producer_runs", [run, {**run, "id": "new", "status": "failed",
                                                           "started_at": "2026-10-13T00:00:00Z"}]))
    yield case("wrong-run-id", ("receipt.producer_run_id", "other"))
    yield case("ignored-terminal-run", ("context.producer.metadata.todo_runner_ignore_terminal_run_id", "run"))
    yield case("latest-tie-first-pass", ("context.producer_runs", [run, {**run, "id": "other",
                                                                       "status": "failed"}]))
    yield case("source-active-before-url", ("context.source_runs", [{"status": "running"}]),
               ("receipt.pr_url", URL + "?view=other"))
    yield case("outside-repo-before-head", ("context.repo", "fixture-owner/other"), bad_head)
    yield case("outside-url-query", ("receipt.pr_url", URL + "?view=other"))
    yield case("outside-url-non-pull", ("receipt.pr_url", URL.replace("/pull/", "/issues/")))
    yield case("outside-owner", ("context.owner_allowlist", ["other"]))
    yield case("outside-url-nonnumeric", ("receipt.pr_url", URL + "x"))
    yield case("head-before-scope", bad_head, ("context.pr.baseRefName", "other"))
    yield case("uppercase-head", ("receipt.head_sha", HEAD.upper()), ("context.pr.headRefOid", HEAD.upper()))
    yield case("scope-before-links", ("context.pr.baseRefName", "other"), ("context.links", []))
    yield case("scope-wrong-author", ("context.pr.author.login", "other"))
    yield case("scope-closed-pr", ("context.pr.state", "CLOSED"))
    yield case("scope-draft", ("context.pr.isDraft", True))
    yield case("scope-draft-integer", ("context.pr.isDraft", 0))
    yield case("missing-link-before-ambiguous", ("context.links", [{"url": OTHER_URL}, {}]), stale_child)
    yield case("ambiguous-link-before-parent", ("context.links", [{"url": URL}, {"url": OTHER_URL}]), stale_child)
    yield case("parent-pr-mismatch", stale_child)
    yield case("duplicate-link-pass", ("context.links", [{"html_url": URL}, {"html_url": "", "url": URL}]))


def project_for_rust(raw, python):
    wire = deepcopy(raw)
    receipt, context = wire["receipt"], wire["context"]
    producer = context["producer"]
    metadata = python.metadata
    canonical_url = python.canonical_github_url
    canonical_repo = python.canonical_repo_slug
    url = str((receipt or {}).get("pr_url") or "")
    parsed = urlparse(url)
    parts = parsed.path.strip("/").split("/")
    context["helpers"] = {
        "source_safety_reason": python.safety_reason(context["source"]),
        "producer_safety_reason": python.safety_reason(producer),
        "producer_metadata": {
            "task_kind": metadata(producer, "task_kind"),
            "source_issue_id": metadata(producer, "source_issue_id"),
            "dedupe_key": str(metadata(producer, "dedupe_key", "")),
            "todo_runner_ignore_terminal_run_id": metadata(producer, "todo_runner_ignore_terminal_run_id"),
            "canonical_source_pr_url": canonical_url(metadata(producer, "source_pr_url")),
        },
        "receipt_url": {
            "scheme": parsed.scheme, "netloc": parsed.netloc, "path": parsed.path,
            "query": parsed.query, "fragment": parsed.fragment,
            "pull_number_is_digits": len(parts) >= 4 and parts[3].isdigit(),
            "canonical_path_repo": canonical_repo("/".join(parts[:2])),
            "canonical_context_repo": canonical_repo(context.pop("repo")),
            "canonical_url": canonical_url(url),
        },
        "canonical_pr_url": canonical_url(context["pr"].pop("url")),
        "canonical_link_urls": [canonical_url(str(link.get("html_url") or link.get("url") or ""))
                                for link in context.pop("links")],
    }
    for key in ("source", "producer"):
        context[key].pop("metadata", None)
    return wire


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scripts", type=Path, help="vault's Multica-Agent-Strategy/Scripts directory")
    scripts = parser.parse_args().scripts.resolve()
    sys.dont_write_bytecode = True
    sys.path[:0] = [str(scripts), str(scripts.parents[2] / "scripts")]
    spec = importlib.util.spec_from_file_location("completion_python", scripts / "review-completion-common.py")
    python = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(python)
    for name, raw in fixtures():
        expected = python.completion_subject_plan(raw["receipt"], raw["context"])
        for suffix, value in (("input", project_for_rust(raw, python)), ("expected", expected)):
            (HERE / f"{name}.{suffix}.json").write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
