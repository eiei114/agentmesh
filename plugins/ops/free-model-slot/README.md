# Free-model snapshot ranking

This crate ports four read-only functions from `free_model_slot.py`. It does
not fetch model data, assign agents, compute health plans, or write state.
Python remains the controller authority.

The stdio binary reads one JSON document to EOF and writes one JSON result.
For example, `{"op":"zero_cost","meta":{"cost":{"input":0,"output":0}}}`
returns `true`.

| `op` | Request field | Result |
| --- | --- | --- |
| `zero_cost` | `meta` object | Boolean |
| `passing` | `row` object | Boolean |
| `ranking_key` | `row` object | Three negated metrics and a model ID, as a JSON array |
| `winners` | `models` array of rows | Full passing rows in stable rank order |

V2 uses benchmark quality, completed runs, uncapped context and model ID.
V1 and legacy snapshots keep score order with model ID as the tie-break. The
same tuple comparison applies in a mixed snapshot; there is no separate
version priority. A failing row need not have a score or model ID.

Parity applies to JSON ranking snapshots, not arbitrary Python objects or
Python exceptions. The stdio handler reports invalid request shapes through
the ops kit. Numeric input is bounded by `serde_json`'s default number
representation; the `ranking_key` JSON result uses a double if a negated
integer cannot be represented as a native JSON integer. Sorting still compares
native integer inputs without rounding them to doubles.
