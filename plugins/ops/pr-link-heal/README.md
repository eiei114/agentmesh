# PR-link heal decision port

This crate ports the deterministic selection, wait, guard, and title-prefix
decision from `pr_link_heal.py`. The caller provides the issue (including its
metadata), an explicit `now`, and synthetic responses for a PR search and PR
view. The result contains the Python return value plus ordered `github_calls`,
`edit`, and `set_metadata` intent records. No callback is executed: this crate
does not invoke GitHub or Multica, mutate an issue or PR, touch a branch, use
the network, read files, or write files.

The stdio process reads one JSON object and writes one JSON response. Example:

```json
{
  "issue": {
    "id": "fixture-issue",
    "identifier": "DOT-3433",
    "metadata": { "project_key": "fixture-project" }
  },
  "now": "2026-11-12T18:00:00+09:00",
  "pr_search_results": [{ "number": 139 }],
  "pr_view": {
    "state": "OPEN",
    "title": "chore(deps): update a dependency",
    "headRefOid": "b8d898c0aaabcdef",
    "url": "https://github.com/fixture/fixture-project/pull/139"
  }
}
```

PR search data is consulted only when neither metadata URL nor a digit-only PR
number selects a candidate. The one-hour waiting window, unique-search rule,
open-state/head/title guards, and metadata URL precedence follow Python. See
`testdata/README.md` for the fixture generation command and coverage.
