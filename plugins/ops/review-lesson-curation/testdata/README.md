# Weekly curation parity

The requests are synthetic. From the repository root, with `SOURCE_SCRIPTS`
set to the directory containing the original `review-lesson-curation.py` and
`review-lessons-common.py`, regenerate the expected JSON with Python 3.11+:

```sh
python plugins/ops/review-lesson-curation/testdata/generate.py "$SOURCE_SCRIPTS"
cargo test -p agentmesh-ops-review-lesson-curation --test parity
```

The test runs without Python or the source checkout. It compares all pairs
through `check_parity` and checks that the executable returns exactly one JSON
document per request.

## JSON contract

| `op` | Input fields | Result |
| --- | --- | --- |
| `week_id` | `now` | JST ISO week string |
| `decide` | `stats`, optional `pinned` | Hint status string |
| `curate` | `index`, `previous`, `now` | `{ "curation": ..., "changed": ... }` |
| `is_visible` | `curation`, `key`, `stats`, optional `project` | Boolean |

`now` is an ISO calendar timestamp with seconds and an explicit minute-aligned
offset (`Z` or `±HH:MM`); no host clock or local timezone is consulted. For visibility,
legacy naïve timestamps (including `YYYY-MM-DDTHH:MM` and `YYYY-MM-DD HH:MM`
with omitted seconds) and date-only strings are interpreted as UTC. Evidence
must be strictly newer than `curated_at` to restore a hint by timestamp alone;
unparseable times do not restore it. These are hint decisions, not a filter on
actual findings or safety gates.
