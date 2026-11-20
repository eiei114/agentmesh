"""Regenerate expected files using the Python controller in the notes vault."""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
from copy import deepcopy
from pathlib import Path


HERE = Path(__file__).resolve().parent
BASE_POLICY = {
    "schema_version": 1,
    "max_requeues_per_tick": 5,
    "labels": {
        "free_model": {"name": "model/free", "id": "fallback-free"},
        "quota_hold": {"name": "provider/quota-hold", "id": "fallback-hold"},
    },
}
UNIQUE_FREE_INVENTORY = {
    "labels": [{"name": "model/free", "id": "live-free"}]
}


def synthetic_requests() -> dict[str, dict]:
    cases = {
        "unique_inventory_match": {
            "inventory": UNIQUE_FREE_INVENTORY,
            "issue": {"labels": []},
        },
        "ambiguous_inventory_fallback": {
            "inventory": {
                "labels": [
                    {"name": "model/free", "id": "first"},
                    {"name": "model/free", "id": "second"},
                ]
            },
            "issue": {"labels": [{"label_id": "fallback-free"}]},
        },
        "missing_inventory_fallback": {
            "issue": {"labels": ["model/free"]},
        },
        "malformed_inventory_fallback": {
            "inventory": {
                "labels": [{"name": "model/free", "id": "live"}, "invalid row"]
            },
            "issue": {"labels": [{"name": "model/free"}]},
        },
        "inventory_error_fallback": {
            "inventory": {"error": "synthetic inventory unavailable"},
            "issue": {"labels": ["fallback-free"]},
        },
        "existing_string_id": {
            "inventory": UNIQUE_FREE_INVENTORY,
            "issue": {"labels": ["live-free"]},
        },
        "existing_string_name": {
            "inventory": UNIQUE_FREE_INVENTORY,
            "issue": {"labels": ["model/free"]},
        },
        "existing_object_id": {
            "inventory": UNIQUE_FREE_INVENTORY,
            "issue": {"labels": [{"id": "live-free"}]},
        },
        "existing_object_name": {
            "inventory": UNIQUE_FREE_INVENTORY,
            "issue": {"labels": [{"name": "model/free"}]},
        },
        "existing_object_label_id": {
            "inventory": UNIQUE_FREE_INVENTORY,
            "issue": {"labels": [{"label_id": "live-free"}]},
        },
        "minimum_requeue_bound": {
            "policy": {"max_requeues_per_tick": 1},
            "label_key": "quota_hold",
            "inventory": {"labels": []},
            "issue": {"labels": []},
        },
        "invalid_policy_schema": {
            "policy": {"schema_version": 2},
            "inventory": {"labels": []},
            "issue": {"labels": []},
        },
        "requeue_below_bound": {
            "policy": {"max_requeues_per_tick": 0},
            "inventory": {"labels": []},
            "issue": {"labels": []},
        },
        "requeue_above_bound": {
            "policy": {"max_requeues_per_tick": 6},
            "inventory": {"labels": []},
            "issue": {"labels": []},
        },
        "free_model_empty_name": {
            "policy": {"labels": {"free_model": {"name": ""}}},
            "inventory": {"labels": []},
            "issue": {"labels": []},
        },
        "free_model_empty_id": {
            "policy": {"labels": {"free_model": {"id": None}}},
            "inventory": {"labels": []},
            "issue": {"labels": []},
        },
        "quota_hold_empty_name": {
            "policy": {"labels": {"quota_hold": {"name": ""}}},
            "label_key": "quota_hold",
            "inventory": {"labels": []},
            "issue": {"labels": []},
        },
        "quota_hold_empty_id": {
            "policy": {"labels": {"quota_hold": {"id": ""}}},
            "label_key": "quota_hold",
            "inventory": {"labels": []},
            "issue": {"labels": []},
        },
    }
    requests = {}
    for name, values in cases.items():
        request = {
            "policy": deepcopy(BASE_POLICY),
            "label_key": "free_model",
            "issue": {"labels": []},
            **deepcopy(values),
        }
        if "policy" in values:
            request["policy"] = {
                **deepcopy(BASE_POLICY),
                **deepcopy(values["policy"]),
            }
            if "labels" in values["policy"]:
                request["policy"]["labels"] = {
                    **deepcopy(BASE_POLICY["labels"]),
                    **deepcopy(values["policy"]["labels"]),
                }
        requests[name] = request
    return requests


def load_reference(vault: Path):
    source = vault / "4_Project/Multica-Agent-Strategy/Scripts/provider_quota_common.py"
    spec = importlib.util.spec_from_file_location("provider_quota_common", source)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load Python reference: {source}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def expected(reference, request: dict) -> dict:
    try:
        with tempfile.TemporaryDirectory(prefix="provider-quota-policy-") as temporary:
            root = Path(temporary)
            policy_path = root / reference.POLICY
            policy_path.parent.mkdir(parents=True)
            policy_path.write_text(
                json.dumps(request["policy"], ensure_ascii=False), encoding="utf-8"
            )
            policy = reference.load_policy(root)
        config = policy["labels"][request["label_key"]]
        label = reference.resolve_label(
            lambda _args: request.get("inventory"), config
        )
        return {
            "policy": policy,
            "label": label,
            "has_label": reference.has_label(request["issue"], label),
        }
    except (AttributeError, KeyError, TypeError, ValueError) as error:
        return {"error": {"kind": "failed", "message": str(error)}}


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: generate_fixtures.py <notes-vault-root>", file=sys.stderr)
        return 2
    reference = load_reference(Path(sys.argv[1]))
    for name, request in sorted(synthetic_requests().items()):
        input_path = HERE / f"{name}.input.json"
        output_path = HERE / f"{name}.expected.json"
        with input_path.open("w", encoding="utf-8", newline="\n") as fixture:
            fixture.write(json.dumps(request, ensure_ascii=False, indent=2) + "\n")
        with output_path.open("w", encoding="utf-8", newline="\n") as fixture:
            fixture.write(
                json.dumps(expected(reference, request), ensure_ascii=False, indent=2)
                + "\n"
            )
        print(output_path.name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
