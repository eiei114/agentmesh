# Read-only repair reconciliation

The binary accepts one JSON request and returns the Python helper's result as
JSON. It performs no lookups or writes. The Python controller remains authoritative.

Both operations take `issue` (the issue row) and `metadata` (its decoded metadata
object). Give `pr_view` as the preloaded view for `metadata.source_pr_url`, with
`state` and `headRefOid`, or `null` if unavailable. Evidence only needs to be
present if the decision reaches the corresponding guard.

- `operation: "superseded_by"`: supply `siblings` as the ordered `issues`
  array from the source-issue-filtered lookup, including the current row if
  present. The first eligible newer row wins, not necessarily the newest one
  or one whose repair SHA matches the live head. Returns `null` or
  `"superseded_by:<identifier>@<head prefix>"`.
- `operation: "orphan_repair"`: supply `status` and `runs` for this issue.
  `runs` accepts a bare array or a `runs`/`items` response object (as in
  Python, `runs` takes precedence). Any run, including a completed one,
  suppresses the orphan decision. Returns `null` or `[status, reason]` for
  either `done` (artifact at the exact open head) or `backlog` (otherwise).

Use GitHub PR URLs with decimal ASCII numbers and ordinary GitHub/Multica JSON
field types. The caller is responsible for binding the preloaded evidence to
the issue and URL. All returned labels are inert; this is not a controller or
an authorization to apply a transition.
