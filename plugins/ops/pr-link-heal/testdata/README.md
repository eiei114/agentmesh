# PR-link heal parity fixtures

Expected output is captured by calling the vault's `pr_link_heal.py` with fake
`gh`, `edit`, and `set_md` callbacks. The callback harness records every
requested GitHub command, title edit, and ordered metadata update; it applies
none of them. All issue identifiers and PR URLs are synthetic.

From the repository root, regenerate with:

```sh
python plugins/ops/pr-link-heal/testdata/generate_expected.py
```

The script loads `pr_link_heal.py` from
`~/Projects/Notes/obsidian-note/4_Project/Multica-Agent-Strategy/Scripts/`
and consumes only the synthetic request/response JSON in this directory.
Fixtures cover metadata candidate precedence, unique/ambiguous/missing search
results, the one-hour wait boundary and fallback, invalid timestamps, closed
PRs, stale heads, prefixed titles, and successful title/metadata action plans.
