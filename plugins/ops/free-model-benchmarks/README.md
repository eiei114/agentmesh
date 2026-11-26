# Free-model benchmark scoring

Offline port of `free_model_benchmarks.py`'s scoring helpers. The stdio binary
uses the ops kit contract: one JSON request, one JSON result, exit 2 for an
invalid request. It does not fetch, parse source formats, persist benchmarks,
or affect routing. Python remains authoritative.

| `op` | Request fields | Result |
| --- | --- | --- |
| `number` | `value`; optional `low` (default `0`) and `high` (default `100`), with `null` disabling a bound | Finite floating-point score; numeric strings accepted |
| `collapse` | `rows`: array of objects with exact nonblank string `id` and `score` | Rows sorted by ID with median `score`, count `measurements`, and the first row's other fields |
| `percentiles` | `entries`: same row shape | ID-to-midrank map after collapse; empty map for no rows, `0.5` for a singleton |
| `match_model` | `model`, `metadata`, `aliases`, `sources` as below | Python-shaped `status`, `identity`, per-metric `sources`, and weighted `quality` |

For `match_model`, pass an alias registry of the form `{"models": {"model-id":
{"identity": "vendor/model", "metadata": {"family": "example"}, "entries":
{"epoch_mirrorcode": "exact-row-id"}}}}`. The caller supplies current `metadata`
to compare against the alias's metadata constraints. Neither model aliases nor
entry IDs are inferred. Supply all six source records: `epoch`, `aider`,
`arena_agent`, `arena_webdev`, `llm_stats`, and `hf_lightning`. Each record has a
boolean `stale` and a `metrics` object keyed by metric name, with pre-parsed row
arrays (e.g. `{"id":"exact-row-id","score":90}`). For an unavailable source,
pass `{"stale":false,"metrics":{}}`. Missing or unmatched metrics use the `0.25`
prior without redistributing their weights. Stale evidence stays usable and is
flagged. Independent metrics use cohort percentiles; vendor-reported metrics
use the matched numeric score divided by 100, with no percentile.

For repeated measurements in an independent cohort, the percentile uses the
collapsed median, while the raw score and provenance come from the first exact
match, as in Python. See `testdata/` for reproducible synthetic examples.
