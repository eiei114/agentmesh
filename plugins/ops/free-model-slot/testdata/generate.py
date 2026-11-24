"""Generate synthetic requests and expected results using the actual Python slice."""

import argparse
import ast
import json
from pathlib import Path


def oracle(path):
    # Compile just the four unmodified function definitions. No controller
    # modules, registries, runtime state, or credentials are loaded.
    names = {"zero_cost", "passing", "ranking_key", "winners"}
    source = ast.parse(path.read_text(encoding="utf-8-sig"))
    functions = [node for node in source.body
                 if isinstance(node, ast.FunctionDef) and node.name in names]
    if {node.name for node in functions} != names:
        raise ValueError("missing Python ranking functions")
    scope = {}
    exec(compile(ast.Module(body=functions, type_ignores=[]), str(path), "exec"), scope)
    return scope


def requests():
    for name, meta in {
        "missing-cost": {},
        "null-cost": {"cost": None},
        "empty-cost": {"cost": {}},
        "missing-input": {"cost": {"output": 0}},
        "missing-output": {"cost": {"input": 0}},
        "zero": {"cost": {"input": 0, "output": 0}},
        "float-and-cache-zero": {"cost": {"input": -0.0, "output": 0.0, "cache": 0}},
        "paid-output": {"cost": {"input": 0, "output": 0.01}},
        "paid-cache": {"cost": {"input": 0, "output": 0, "cache_read": 1}},
        "false-input": {"cost": {"input": False, "output": 0}},
        "false-extra": {"cost": {"input": 0, "output": 0, "cache": False}},
        "string-zero": {"cost": {"input": 0, "output": "0"}},
        "tiers": {"cost": {"input": 0, "output": 0, "tiers": [{}]}},
        "context-tier": {"cost": {"input": 0, "output": 0, "context_over_128k": {"input": 2}}},
    }.items():
        yield f"cost-{name}", dict(op="zero_cost", meta=meta)

    common = dict(free=True, available=True, not_deprecated=True)
    external = dict(common, tool_metadata=True, not_exhausted=True)
    legacy = dict(common, smoke=True, tools=True)
    passed = [{"passed": True}, {"passed": True}]
    v1, v2 = "external-benchmarks-v1", "external-benchmarks-v2"
    for version in (v1, v2):
        suffix = version[-2:]
        for name, gates in {
            "exact": external,
            "extra": dict(external, smoke=True),
            "legacy-shape": legacy,
            "missing": {k: v for k, v in external.items() if k != "not_exhausted"},
            "numeric-true": dict(external, free=1, available=1.0),
            "string-true": dict(external, tool_metadata="true"),
            **{f"false-{key}": dict(external, **{key: False}) for key in external},
        }.items():
            yield f"passing-{suffix}-{name}", dict(op="passing", row=dict(
                ranking_method=version, gates=gates, evals=[{"passed": False}]))

    for name, row in {
        "exact": dict(gates=legacy, evals=passed),
        "unknown-method": dict(gates=legacy, evals=passed, ranking_method="synthetic"),
        "external-shape": dict(gates=external, evals=passed),
        "extra-gate": dict(gates=dict(legacy, not_exhausted=True), evals=passed),
        "missing-evals": dict(gates=legacy),
        "only-one-eval": dict(gates=legacy, evals=passed[:1]),
        "failed-second-eval": dict(gates=legacy, evals=[passed[0], {"passed": False}]),
        "failed-third-eval": dict(gates=legacy, evals=[*passed, {}]),
        "truthy-passes": dict(gates=legacy, evals=[{"passed": "yes"}, {"passed": 1}]),
        **{f"false-{key}": dict(gates=dict(legacy, **{key: False}), evals=passed)
           for key in legacy},
    }.items():
        yield f"passing-legacy-{name}", dict(op="passing", row=row)

    def row_v2(name, quality, completions, context, score):
        return dict(model=f"synthetic/{name}", ranking_method=v2, gates=external,
                    benchmark={"quality": quality}, real_runs={"completed": completions},
                    metadata={"limit": {"context": context}}, score=score)

    # Score opposes the v2 keys, and the large contexts are well past typical caps.
    tied = row_v2("a-id", .8, 2, 9007199254740993, 2)
    rows_v2 = [
        row_v2("z-id", .8, 2, 9007199254740993, 100),
        row_v2("a-context-loser", .8, 2, 9007199254740992.0, 99),
        row_v2("a-completions-loser", .8, 1, 9999999999999999, 98),
        row_v2("a-quality-loser", .7, 100, 9999999999999999, 97),
        dict(tied, marker="first"),
        row_v2("z-quality-winner", .9, 0, 0, 0),
        dict(tied, marker="second", score=999),
        dict(ranking_method=v2, gates=dict(external, not_exhausted=False)),
    ]
    yield "winners-v2", dict(op="winners", models=rows_v2)
    yield "key-v2", dict(op="ranking_key", row=tied)
    defaults = dict(model="synthetic/default", ranking_method=v2, gates=external)
    yield "key-v2-defaults", dict(op="ranking_key", row=defaults)
    yield "winners-v2-defaults", dict(op="winners", models=[
        row_v2("zero-quality", 0, 100, 100, 100),
        defaults,
        row_v2("with-evidence", .25, 1, 0, 0),
    ])

    old = dict(model="synthetic/c", score=20, gates=legacy, evals=passed)
    historical = dict(model="synthetic/b", ranking_method=v1, score=20, gates=external,
                      benchmark={"quality": .99}, real_runs={"completed": 999})
    yield "key-v1-score", dict(op="ranking_key", row=historical)
    yield "key-legacy-score", dict(op="ranking_key", row=old)
    yield "winners-mixed", dict(op="winners", models=[
        defaults, old, historical, dict(old, model="synthetic/a", score=3),
        dict(old, model="synthetic/high", score=21), dict(old, score=99, evals=passed[:1]),
    ])
    yield "winners-empty", dict(op="winners", models=[])
    yield "winners-all-rejected", dict(op="winners", models=[{}, dict(gates=external)])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="path to free_model_slot.py")
    args = parser.parse_args()
    functions = oracle(args.source)
    fixtures = Path(__file__).resolve().parent
    fields = dict(zero_cost="meta", passing="row", ranking_key="row")
    for name, request in requests():
        op = request["op"]
        result = functions[op](request if op == "winners" else request[fields[op]])
        for suffix, value in (("input", request), ("expected", result)):
            (fixtures / f"{name}.{suffix}.json").write_text(
                json.dumps(value, indent=2, allow_nan=False) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
