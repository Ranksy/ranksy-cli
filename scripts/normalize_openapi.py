#!/usr/bin/env python3
"""Normalize the Scramble OpenAPI 3.1 export into a form openapiv3 (3.0-style)
can parse, without changing API semantics.

Rules applied:
  1. `"type": ["X", "null"]`  ->  `"type": "X", "nullable": true`   (3.1 nullable)
  2. `"type": ["object", "null"]` with properties -> same rule (properties kept)
  3. `"openapi": "3.1.0"`      ->  `"openapi": "3.0.3"`             (progenitor 0.8 is 3.0.x)
  4. duplicate operationIds   ->  suffixed `_1`, `_2`, ...           (Scramble emits
     `v1.getMetric_0` for two metrics endpoints; progenitor requires unique ids)
  5. `"type": "null"`         ->  `type` key removed                 (3.1 null type has
     no 3.0 equivalent; progenitor cannot render it)
  6. array query parameters   ->  `"type": "array"` -> `"type": "string"` on the
     parameter schema (progenitor 0.8 emits `Vec::to_string()` for array query
     params and cannot compile them; the CLI wrapper does not use the generated
     methods for these two ops, so generated-client fidelity loss is acceptable)

Run after every re-sync of openapi.json from the app's Scramble export:
    python3 scripts/normalize_openapi.py
"""

import json
import sys

PATH = "openapi.json"


def normalize_type(schema: dict) -> None:
    t = schema.get("type")
    if isinstance(t, list):
        non_null = [x for x in t if x != "null"]
        schema["type"] = non_null[0] if non_null else "string"
        if "null" in t:
            schema["nullable"] = True
    elif t == "null":
        del schema["type"]


def walk(node):
    if isinstance(node, dict):
        if "type" in node:
            normalize_type(node)
        for v in node.values():
            walk(v)
    elif isinstance(node, list):
        for v in node:
            walk(v)


def main() -> None:
    with open(PATH, encoding="utf-8") as f:
        spec = json.load(f)
    if spec.get("openapi", "").startswith("3.1"):
        spec["openapi"] = "3.0.3"
    seen = {}
    for path_item in spec.get("paths", {}).values():
        for op in path_item.values():
            if not isinstance(op, dict):
                continue
            oid = op.get("operationId")
            if not oid:
                continue
            n = seen.get(oid, 0)
            if n:
                op["operationId"] = f"{oid}_{n}"
            seen[oid] = n + 1
    walk(spec)
    for path_item in spec.get("paths", {}).values():
        for op in path_item.values():
            if not isinstance(op, dict):
                continue
            for pr in op.get("parameters", []):
                schema = pr.get("schema")
                if isinstance(schema, dict) and schema.get("type") == "array":
                    schema["type"] = "string"
    with open(PATH, "w", encoding="utf-8") as f:
        json.dump(spec, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"normalized {PATH}")


if __name__ == "__main__":
    sys.exit(main())
