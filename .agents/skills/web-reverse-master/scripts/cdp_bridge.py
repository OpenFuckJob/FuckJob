#!/usr/bin/env python3
"""Small Chrome DevTools Protocol bridge.

The bridge is transport-injectable so offline tests can verify message shape
without opening a browser. Live use requires a page WebSocket URL such as the
value from http://127.0.0.1:<port>/json.
"""

from __future__ import annotations

import argparse
import http.client
import json
from dataclasses import dataclass, field
from typing import Any, Protocol


class Transport(Protocol):
    def send(self, payload: str) -> None: ...
    def recv(self) -> str: ...
    def close(self) -> None: ...


@dataclass
class CDPBridge:
    ws_url: str | None = None
    transport: Transport | None = None
    seq: int = 0
    sent_messages: list[dict[str, Any]] = field(default_factory=list)

    @staticmethod
    def discover_ws_url(port: int, host: str = "127.0.0.1") -> str:
        conn = http.client.HTTPConnection(host, port, timeout=5)
        conn.request("GET", "/json")
        response = conn.getresponse()
        tabs = json.loads(response.read().decode("utf-8"))
        for tab in tabs:
            url = tab.get("webSocketDebuggerUrl")
            if url:
                return url
        raise RuntimeError(f"No webSocketDebuggerUrl found on {host}:{port}")

    @classmethod
    def from_port(cls, port: int, host: str = "127.0.0.1") -> "CDPBridge":
        return cls(ws_url=cls.discover_ws_url(port, host))

    def connect(self) -> None:
        if self.transport:
            return
        if not self.ws_url:
            raise RuntimeError("ws_url is required before connect()")
        try:
            import websocket
        except ImportError as exc:
            raise RuntimeError("Install websocket-client for live CDP use: pip install websocket-client") from exc
        self.transport = websocket.create_connection(self.ws_url, timeout=10)

    def close(self) -> None:
        if self.transport:
            self.transport.close()

    def call(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        if not self.transport:
            raise RuntimeError("Call connect() or provide a transport before sending CDP messages")
        self.seq += 1
        message = {"id": self.seq, "method": method}
        if params is not None:
            message["params"] = params
        self.sent_messages.append(message)
        self.transport.send(json.dumps(message, separators=(",", ":")))
        return json.loads(self.transport.recv())

    def enable_runtime(self) -> dict[str, Any]:
        return self.call("Runtime.enable")

    def enable_debugger(self) -> dict[str, Any]:
        return self.call("Debugger.enable")

    def evaluate(self, expression: str, return_by_value: bool = True) -> dict[str, Any]:
        return self.call(
            "Runtime.evaluate",
            {
                "expression": expression,
                "returnByValue": return_by_value,
                "awaitPromise": True,
            },
        )

    def set_breakpoint_by_url(self, url_regex: str, line_number: int = 0, column_number: int = 0) -> dict[str, Any]:
        return self.call(
            "Debugger.setBreakpointByUrl",
            {
                "urlRegex": url_regex,
                "lineNumber": line_number,
                "columnNumber": column_number,
            },
        )

    def resume(self) -> dict[str, Any]:
        return self.call("Debugger.resume")


def main() -> None:
    parser = argparse.ArgumentParser(description="Evaluate JavaScript through a CDP WebSocket")
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--ws-url", help="CDP page WebSocket URL")
    group.add_argument("--port", type=int, help="Local CDP port exposing /json")
    parser.add_argument("--expr", default="document.title", help="Expression to evaluate")
    args = parser.parse_args()

    bridge = CDPBridge.from_port(args.port) if args.port else CDPBridge(ws_url=args.ws_url)
    bridge.connect()
    try:
        print(json.dumps(bridge.evaluate(args.expr), ensure_ascii=False, indent=2))
    finally:
        bridge.close()


if __name__ == "__main__":
    main()
