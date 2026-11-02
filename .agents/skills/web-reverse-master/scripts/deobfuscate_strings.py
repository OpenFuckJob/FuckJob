#!/usr/bin/env python3
"""Decode simple JavaScript _0x string-table obfuscation.

Supported offline scenario:
  var _0xarr = ['alpha', 'beta'];
  function _0xdec(i) { return _0xarr[i - 0x10]; }
  _0xdec('0x10') -> 'alpha'

This is a deterministic first-pass helper, not a full JS interpreter. Use the
AST pipeline or browser execution for rotating arrays, RC4 tables, or runtime
dependent decoders.
"""

from __future__ import annotations

import argparse
import ast
import re
from pathlib import Path


ARRAY_RE = re.compile(r"(?:var|const|let)\s+([_$a-zA-Z][_$\w]*)\s*=\s*\[((?:.|\n)*?)\]\s*;", re.MULTILINE)


def parse_js_string(token: str) -> str | None:
    try:
        value = ast.literal_eval(token)
    except (SyntaxError, ValueError):
        return None
    return value if isinstance(value, str) else None


def parse_array(content: str) -> list[str]:
    items: list[str] = []
    for match in re.finditer(r"'(?:\\.|[^'\\])*'|\"(?:\\.|[^\"\\])*\"", content):
        value = parse_js_string(match.group(0))
        if value is not None:
            items.append(value)
    return items


def int_literal(value: str) -> int:
    value = value.strip().strip("'\"")
    return int(value, 16) if value.lower().startswith("0x") else int(value, 10)


def find_decoders(code: str, array_name: str) -> dict[str, int]:
    decoders: dict[str, int] = {}

    patterns = [
        re.compile(
            rf"function\s+([_$a-zA-Z][_$\w]*)\s*\([^)]*\)\s*\{{[^{{}}]*?return\s+{re.escape(array_name)}\s*\[[^\]]*?-\s*(0x[0-9a-fA-F]+|\d+)\s*\]",
            re.DOTALL,
        ),
        re.compile(
            rf"(?:var|let|const)\s+([_$a-zA-Z][_$\w]*)\s*=\s*function\s*\([^)]*\)\s*\{{[^{{}}]*?return\s+{re.escape(array_name)}\s*\[[^\]]*?-\s*(0x[0-9a-fA-F]+|\d+)\s*\]",
            re.DOTALL,
        ),
        re.compile(
            rf"(?:var|let|const)\s+([_$a-zA-Z][_$\w]*)\s*=\s*\([^)]*\)\s*=>\s*{re.escape(array_name)}\s*\[[^\]]*?-\s*(0x[0-9a-fA-F]+|\d+)\s*\]",
            re.DOTALL,
        ),
    ]

    for pattern in patterns:
        for match in pattern.finditer(code):
            decoders[match.group(1)] = int_literal(match.group(2))

    return decoders


def js_quote(value: str) -> str:
    escaped = value.replace("\\", "\\\\").replace("'", "\\'").replace("\n", "\\n").replace("\r", "\\r")
    return f"'{escaped}'"


def replace_decoder_calls(code: str, decoder_name: str, offset: int, strings: list[str]) -> tuple[str, int]:
    call_re = re.compile(rf"\b{re.escape(decoder_name)}\s*\(\s*(['\"]?(?:0x[0-9a-fA-F]+|\d+)['\"]?)\s*(?:,\s*[^)]*)?\)")
    count = 0

    def repl(match: re.Match[str]) -> str:
        nonlocal count
        index = int_literal(match.group(1)) - offset
        if 0 <= index < len(strings):
            count += 1
            return js_quote(strings[index])
        return match.group(0)

    return call_re.sub(repl, code), count


def extract_and_decode_strings(filepath: str, output_path: str | None = None) -> str:
    code = Path(filepath).read_text(encoding="utf-8")
    total = 0
    found_tables = 0

    for array_match in ARRAY_RE.finditer(code):
        array_name = array_match.group(1)
        strings = parse_array(array_match.group(2))
        if len(strings) < 2:
            continue

        decoders = find_decoders(code, array_name)
        if not decoders:
            continue

        found_tables += 1
        print(f"[FOUND] {array_name}: {len(strings)} strings, {len(decoders)} decoders")
        for decoder_name, offset in decoders.items():
            code, count = replace_decoder_calls(code, decoder_name, offset, strings)
            total += count
            print(f"[DONE] {decoder_name}: offset={offset} replacements={count}")

    if not found_tables:
        print("[WARN] No supported string-table decoder found")

    if output_path:
        Path(output_path).write_text(code, encoding="utf-8")
        print(f"[OUTPUT] {output_path}")

    print(f"[SUMMARY] replacements={total}")
    return code


def main() -> None:
    parser = argparse.ArgumentParser(description="Decode simple JavaScript string tables")
    parser.add_argument("input")
    parser.add_argument("output", nargs="?", help="Output file, default: input_deobf.js")
    args = parser.parse_args()

    output = args.output or str(Path(args.input).with_name(Path(args.input).stem + "_deobf.js"))
    extract_and_decode_strings(args.input, output)


if __name__ == "__main__":
    main()
