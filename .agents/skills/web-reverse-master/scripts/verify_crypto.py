#!/usr/bin/env python3
"""Verify browser/local crypto parity.

The default comparison is byte-for-byte. JSON mode compares parsed structures
while still using byte diagnostics for long differing strings.
"""

from __future__ import annotations

import argparse
import importlib
import json
import os
import sys
from pathlib import Path
from typing import Any


def read_bytes(path: str) -> bytes:
    return Path(path).read_bytes()


def normalize_text(data: bytes, strip: bool) -> str:
    text = data.decode("utf-8")
    return text.strip() if strip else text


def compare_outputs(browser_output: bytes | str, local_output: bytes | str, label: str = "") -> bool:
    b_bytes = browser_output.encode("utf-8") if isinstance(browser_output, str) else browser_output
    l_bytes = local_output.encode("utf-8") if isinstance(local_output, str) else local_output

    if b_bytes == l_bytes:
        print(f"[PASS] {label}outputs match exactly ({len(b_bytes)} bytes)")
        return True

    min_len = min(len(b_bytes), len(l_bytes))
    diff_pos = next((i for i in range(min_len) if b_bytes[i] != l_bytes[i]), min_len)
    print(f"[FAIL] {label}outputs differ")
    print(f"  browser: {len(b_bytes)} bytes")
    print(f"  local:   {len(l_bytes)} bytes")
    print(f"  first diff: byte {diff_pos}")

    if diff_pos < min_len:
        print(f"  browser[{diff_pos}]: 0x{b_bytes[diff_pos]:02x}")
        print(f"  local[{diff_pos}]:   0x{l_bytes[diff_pos]:02x}")

    start = max(0, diff_pos - 20)
    end = min(max(len(b_bytes), len(l_bytes)), diff_pos + 20)
    print(f"  browser context: {b_bytes[start:end]!r}")
    print(f"  local context:   {l_bytes[start:end]!r}")
    return False


def compare_json(browser_json: str, local_json: str) -> bool:
    browser = json.loads(browser_json)
    local = json.loads(local_json)
    differences: list[str] = []

    def compare(left: Any, right: Any, path: str) -> None:
        if type(left) is not type(right):
            differences.append(f"{path}: type {type(left).__name__} != {type(right).__name__}")
            return

        if isinstance(left, dict):
            left_keys = set(left)
            right_keys = set(right)
            for key in sorted(left_keys - right_keys):
                differences.append(f"{path}.{key}: missing locally")
            for key in sorted(right_keys - left_keys):
                differences.append(f"{path}.{key}: extra locally")
            for key in sorted(left_keys & right_keys):
                compare(left[key], right[key], f"{path}.{key}")
            return

        if isinstance(left, list):
            if len(left) != len(right):
                differences.append(f"{path}: length {len(left)} != {len(right)}")
            for index, (left_item, right_item) in enumerate(zip(left, right)):
                compare(left_item, right_item, f"{path}[{index}]")
            return

        if left != right:
            differences.append(f"{path}: {left!r} != {right!r}")

    compare(browser, local, "$")
    if not differences:
        print("[PASS] JSON structures match")
        return True

    print(f"[FAIL] JSON structures differ ({len(differences)} differences)")
    for difference in differences[:20]:
        print(f"  - {difference}")
    if len(differences) > 20:
        print(f"  ... {len(differences) - 20} more")
    return False


def load_local_func(func_path: str, project_root: str | None = None):
    if ":" not in func_path:
        raise SystemExit("[ERROR] Function path must be 'module.path:function_name'")

    module_path, func_name = func_path.split(":", 1)
    root = Path(project_root).resolve() if project_root else Path.cwd().resolve()
    sys.path.insert(0, str(root))

    try:
        module = importlib.import_module(module_path)
        return getattr(module, func_name)
    except ImportError as exc:
        raise SystemExit(f"[ERROR] Cannot import module '{module_path}': {exc}") from exc
    except AttributeError as exc:
        raise SystemExit(f"[ERROR] Module '{module_path}' has no function '{func_name}'") from exc


def call_local_func(func_path: str, value: str | None, key: str | None, project_root: str | None) -> Any:
    func = load_local_func(func_path, project_root)
    if key is not None:
        return func(value, key)
    if value is not None:
        return func(value)
    return func()


def main() -> None:
    parser = argparse.ArgumentParser(description="Compare browser and local crypto outputs")
    parser.add_argument("--browser-output", required=True, help="Browser output file")
    parser.add_argument("--local-output", help="Local output file")
    parser.add_argument("--local-func", help="Local function path: module:function")
    parser.add_argument("--input", help="Input value for --local-func")
    parser.add_argument("--key", help="Key value for --local-func")
    parser.add_argument("--json", action="store_true", help="Compare as JSON structures")
    parser.add_argument("--strip", action="store_true", help="Strip leading/trailing text whitespace before comparing")
    parser.add_argument("--project-root", "-p", default=None, help="Project root for module imports")
    args = parser.parse_args()

    browser_bytes = read_bytes(args.browser_output)

    if args.local_output:
        local_bytes = read_bytes(args.local_output)
        if args.json:
            ok = compare_json(normalize_text(browser_bytes, args.strip), normalize_text(local_bytes, args.strip))
        else:
            if args.strip:
                ok = compare_outputs(normalize_text(browser_bytes, True), normalize_text(local_bytes, True))
            else:
                ok = compare_outputs(browser_bytes, local_bytes)
    elif args.local_func:
        result = call_local_func(args.local_func, args.input, args.key, args.project_root)
        if args.json:
            ok = compare_json(normalize_text(browser_bytes, args.strip), json.dumps(result, ensure_ascii=False, separators=(",", ":")))
        else:
            ok = compare_outputs(normalize_text(browser_bytes, args.strip), str(result))
    else:
        raise SystemExit("[ERROR] Specify --local-output or --local-func")

    raise SystemExit(0 if ok else 1)


if __name__ == "__main__":
    main()
