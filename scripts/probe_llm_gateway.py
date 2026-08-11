#!/usr/bin/env python3
"""Probe OpenAI-compatible Chat Completions and Responses endpoints."""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from typing import Any


def probe(
    base_url: str,
    api_key: str,
    path: str,
    payload: dict[str, Any],
    timeout: float,
) -> tuple[int, dict[str, Any]]:
    request = urllib.request.Request(
        f"{base_url.rstrip('/')}/{path.lstrip('/')}",
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        },
        method="POST",
    )

    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            status = response.status
            body = response.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as error:
        status = error.code
        body = error.read().decode("utf-8", errors="replace")

    try:
        parsed = json.loads(body)
        return status, parsed if isinstance(parsed, dict) else {"body": parsed}
    except json.JSONDecodeError:
        return status, {"body": body[:300]}


def summarize(name: str, status: int, body: dict[str, Any]) -> None:
    error = body.get("error") if isinstance(body.get("error"), dict) else {}
    code = error.get("code") or body.get("code")
    kind = error.get("type") or body.get("type")
    message = error.get("message") or body.get("message")
    details = ", ".join(
        str(value) for value in (code, kind, message) if value not in (None, "")
    )
    suffix = f" - {details[:240]}" if details else ""
    print(f"{name:<18} HTTP {status}{suffix}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Check which OpenAI-compatible API protocol a gateway supports."
    )
    parser.add_argument("--base-url", default=os.getenv("OPENAI_BASE_URL"))
    parser.add_argument("--api-key", default=os.getenv("OPENAI_API_KEY"))
    parser.add_argument("--model", default=os.getenv("OPENAI_MODEL"))
    parser.add_argument("--timeout", type=float, default=20.0)
    args = parser.parse_args()

    missing = [
        name
        for name, value in (
            ("--base-url or OPENAI_BASE_URL", args.base_url),
            ("--api-key or OPENAI_API_KEY", args.api_key),
            ("--model or OPENAI_MODEL", args.model),
        )
        if not value
    ]
    if missing:
        parser.error("missing " + ", ".join(missing))
    return args


def main() -> int:
    args = parse_args()
    checks = (
        (
            "chat/completions",
            "chat/completions",
            {
                "model": args.model,
                "messages": [{"role": "user", "content": "Reply with OK."}],
                "max_tokens": 8,
                "stream": False,
            },
        ),
        (
            "responses",
            "responses",
            {
                "model": args.model,
                "input": "Reply with OK.",
                "max_output_tokens": 8,
            },
        ),
    )

    succeeded = False
    for name, path, payload in checks:
        try:
            status, body = probe(args.base_url, args.api_key, path, payload, args.timeout)
        except urllib.error.URLError as error:
            print(f"{name:<18} network error - {error.reason}")
            continue
        summarize(name, status, body)
        succeeded = succeeded or 200 <= status < 300

    return 0 if succeeded else 1


if __name__ == "__main__":
    sys.exit(main())
