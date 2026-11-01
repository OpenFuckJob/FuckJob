#!/usr/bin/env python3
"""Compare browser environment captures.

Inputs are JSON files produced by env_diff.js. The parser also accepts pasted
console output that contains JSON plus surrounding log text.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


DEFAULT_IGNORE = {
    "crypto.randomUUID()",
    "location.href",
    "document.cookie",
}


def load_json_loose(path: str) -> dict[str, Any]:
    text = Path(path).read_text(encoding="utf-8").strip()
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        start = text.find("{")
        end = text.rfind("}")
        if start == -1 or end == -1 or end <= start:
            raise
        return json.loads(text[start : end + 1])


def compare_env(browser_file: str, patched_file: str, ignore: set[str] | None = None) -> bool:
    browser = load_json_loose(browser_file)
    patched = load_json_loose(patched_file)
    ignored = DEFAULT_IGNORE | (ignore or set())
    diffs: list[str] = []

    for key in sorted(set(browser) | set(patched)):
        if key in ignored:
            continue
        if key not in patched:
            diffs.append(f"[MISSING] {key}: absent from patched capture")
            continue
        if key not in browser:
            diffs.append(f"[EXTRA] {key}: absent from browser capture")
            continue

        browser_item = browser[key]
        patched_item = patched[key]
        if browser_item.get("error") or patched_item.get("error"):
            if browser_item.get("error") != patched_item.get("error"):
                diffs.append(f"[ERROR] {key}: browser={browser_item.get('error')} patched={patched_item.get('error')}")
            continue

        if browser_item.get("type") != patched_item.get("type"):
            diffs.append(f"[TYPE] {key}: browser={browser_item.get('type')} patched={patched_item.get('type')}")
            continue

        if browser_item.get("value") != patched_item.get("value"):
            b_val = str(browser_item.get("value", ""))
            p_val = str(patched_item.get("value", ""))
            diffs.append(f"[VALUE] {key}: browser={b_val[:100]!r} patched={p_val[:100]!r}")

    if diffs:
        print(f"[FAIL] {len(diffs)} environment differences")
        for diff in diffs:
            print(f"  {diff}")
    else:
        print(f"[PASS] Environment captures match ({len(browser)} browser checks, ignored {len(ignored)})")

    return not diffs


def main() -> None:
    parser = argparse.ArgumentParser(description="Compare browser and patched environment JSON captures")
    parser.add_argument("browser_env")
    parser.add_argument("patched_env")
    parser.add_argument("--ignore", action="append", default=[], help="Additional key to ignore; repeatable")
    args = parser.parse_args()

    ok = compare_env(args.browser_env, args.patched_env, set(args.ignore))
    raise SystemExit(0 if ok else 1)


if __name__ == "__main__":
    main()
