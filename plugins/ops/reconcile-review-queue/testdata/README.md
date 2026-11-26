# Python parity fixtures

Run from the repository root to regenerate the checked-in expected JSON:

```sh
python plugins/ops/reconcile-review-queue/testdata/generate_expected.py
```

This loads `reconcile-review-queue.py` from the local vault's Scripts directory
(override with `--scripts` if needed) and invokes only its two decision helpers.
Preloaded synthetic evidence replaces `sh_json` and Board reads; the harness
cannot write to the Board and does not run the Python controller.

The inputs adapt `SupersededRepairTests` and `OrphanRepairTests` from
`test_reconcile_review_queue.py`. They also check response envelopes, sibling
order and current-row timestamps. All identifiers and PR URLs are synthetic.
