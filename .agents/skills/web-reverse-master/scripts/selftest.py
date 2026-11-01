#!/usr/bin/env python3
"""Offline delivery tests for web-reverse-master.

These tests use local fixtures only. They do not open browsers, call real
targets, or require network access.
"""

from __future__ import annotations

import argparse
import io
import json
import shutil
import subprocess
import sys
import uuid
from contextlib import contextmanager, redirect_stdout
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
SKILL_DIR = SCRIPT_DIR.parent
sys.path.insert(0, str(SCRIPT_DIR))


@contextmanager
def workspace_tempdir():
    root = SKILL_DIR / "selftest_tmp"
    root.mkdir(exist_ok=True)
    path = root / f"case-{uuid.uuid4().hex}"
    path.mkdir()
    try:
        yield str(path)
    finally:
        shutil.rmtree(path, ignore_errors=True)


def assert_true(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def case_scaffold() -> None:
    from new_project import create_project

    with workspace_tempdir() as tmp:
        base = Path(tmp)
        expected = {
            "sign": ["src/main.py", "src/signer.py", "tests/test_signer.py", "docs/plan.md"],
            "captcha": ["src/main.py", "src/captcha_solver.py", "tests/test_captcha_solver.py"],
            "cdp": ["src/main.py", "src/cdp_bridge.py", "tests/test_cdp_notes.py"],
            "doc": ["src/downloader.py", "tests/test_downloader_notes.py", "assets/documents"],
        }
        for project_type, paths in expected.items():
            project = create_project(f"demo-{project_type}", "https://example.test", project_type, str(base))
            for rel_path in paths:
                assert_true((project / rel_path).exists(), f"{project_type} missing {rel_path}")


def case_deobfuscate() -> None:
    from deobfuscate_strings import extract_and_decode_strings

    source = """
var _0xabc = ['alpha', 'beta', 'gamma'];
function _0xdec(i) { return _0xabc[i - 0x20]; }
const out = _0xdec('0x21') + ':' + _0xdec(0x22);
"""
    with workspace_tempdir() as tmp:
        input_path = Path(tmp) / "input.js"
        output_path = Path(tmp) / "output.js"
        input_path.write_text(source, encoding="utf-8")
        result = extract_and_decode_strings(str(input_path), str(output_path))
        assert_true("'beta'" in result, "string decoder did not replace quoted hex call")
        assert_true("'gamma'" in result, "string decoder did not replace numeric hex call")
        assert_true(output_path.exists(), "string decoder did not write output")


def case_verify() -> None:
    from verify_crypto import compare_json, compare_outputs

    assert_true(compare_outputs(b"abc\x00xyz", b"abc\x00xyz"), "byte parity comparison failed")
    assert_true(compare_json('{"a":[1,2],"b":"x"}', '{"b":"x","a":[1,2]}'), "JSON parity comparison failed")
    with redirect_stdout(io.StringIO()):
        assert_true(not compare_outputs(b"abc", b"abd"), "byte mismatch was not detected")
        assert_true(not compare_json('{"a":1}', '{"a":2}'), "JSON mismatch was not detected")


def case_env() -> None:
    from compare_env import compare_env

    browser = {
        "navigator.userAgent": {"value": "UA", "type": "string"},
        "navigator.webdriver": {"value": "undefined", "type": "undefined"},
        "crypto.randomUUID()": {"value": "random-a", "type": "string"},
    }
    patched = {
        "navigator.userAgent": {"value": "UA", "type": "string"},
        "navigator.webdriver": {"value": "undefined", "type": "undefined"},
        "crypto.randomUUID()": {"value": "random-b", "type": "string"},
    }
    with workspace_tempdir() as tmp:
        browser_path = Path(tmp) / "browser.json"
        patched_path = Path(tmp) / "patched.json"
        browser_path.write_text("[EnvDiff]\n" + json.dumps(browser), encoding="utf-8")
        patched_path.write_text(json.dumps(patched), encoding="utf-8")
        assert_true(compare_env(str(browser_path), str(patched_path)), "env comparison failed for matching stable values")

        patched["navigator.webdriver"] = {"value": "true", "type": "string"}
        patched_path.write_text(json.dumps(patched), encoding="utf-8")
        with redirect_stdout(io.StringIO()):
            assert_true(not compare_env(str(browser_path), str(patched_path)), "env mismatch was not detected")


def case_cdp() -> None:
    from cdp_bridge import CDPBridge

    class FakeTransport:
        def __init__(self):
            self.sent: list[str] = []

        def send(self, payload: str) -> None:
            self.sent.append(payload)

        def recv(self) -> str:
            message = json.loads(self.sent[-1])
            return json.dumps({"id": message["id"], "result": {"ok": True}})

        def close(self) -> None:
            pass

    transport = FakeTransport()
    bridge = CDPBridge(transport=transport)
    response = bridge.evaluate("1 + 1")
    sent = json.loads(transport.sent[-1])
    assert_true(response["result"]["ok"] is True, "CDP fake response failed")
    assert_true(sent["method"] == "Runtime.evaluate", "CDP evaluate method mismatch")
    assert_true(sent["params"]["expression"] == "1 + 1", "CDP expression mismatch")


def case_hooks() -> None:
    node = shutil.which("node")
    hook_files = [
        SCRIPT_DIR / "hooks" / "network.js",
        SCRIPT_DIR / "hooks" / "cookie_header.js",
        SCRIPT_DIR / "hooks" / "cryptojs.js",
        SCRIPT_DIR / "env_diff.js",
        SCRIPT_DIR / "deobfuscate_ast.js",
    ]
    for path in hook_files:
        assert_true(path.exists(), f"missing JS deliverable: {path.name}")

    if not node:
        print("[SKIP] node is not available; JS syntax checks skipped")
        return

    for path in hook_files:
        result = subprocess.run([node, "--check", str(path)], capture_output=True, text=True)
        assert_true(result.returncode == 0, f"node --check failed for {path.name}: {result.stderr}")

    runtime_test = f"""
const fs = require('fs');
const vm = require('vm');
const base = {json.dumps(str(SCRIPT_DIR))};

function run(code, ctx) {{
  ctx.window = ctx;
  ctx.globalThis = ctx;
  ctx.console = {{ debug() {{}} }};
  ctx.Date = Date;
  ctx.String = String;
  ctx.Array = Array;
  vm.createContext(ctx);
  vm.runInContext(fs.readFileSync(base + '/' + code, 'utf8'), ctx);
  return ctx;
}}

async function testNetwork() {{
  function XHR() {{ this.listeners = {{}}; this.status = 204; this.responseURL = '/api'; }}
  XHR.prototype.open = function () {{}};
  XHR.prototype.send = function () {{ if (this.listeners.loadend) this.listeners.loadend.call(this); }};
  XHR.prototype.addEventListener = function (name, cb) {{ this.listeners[name] = cb; }};
  const ctx = {{
    fetch(input) {{ return Promise.resolve({{ url: String(input), status: 200, ok: true }}); }},
    XMLHttpRequest: XHR
  }};
  run('hooks/network.js', ctx);
  await ctx.fetch('https://example.test/api', {{ method: 'POST' }});
  const xhr = new ctx.XMLHttpRequest();
  xhr.open('POST', '/api');
  xhr.send('body');
  if (!ctx.__webReverseLogs.some(x => x.type === 'fetch:request')) throw new Error('missing fetch log');
  if (!ctx.__webReverseLogs.some(x => x.type === 'xhr:request')) throw new Error('missing xhr log');
}}

function testCookieHeader() {{
  function Document() {{}}
  Object.defineProperty(Document.prototype, 'cookie', {{
    configurable: true,
    get() {{ return this._cookie || ''; }},
    set(value) {{ this._cookie = value; }}
  }});
  function XHR() {{}}
  XHR.prototype.setRequestHeader = function () {{}};
  const ctx = {{
    Document,
    HTMLDocument: Document,
    document: new Document(),
    XMLHttpRequest: XHR
  }};
  run('hooks/cookie_header.js', ctx);
  ctx.document.cookie = 'a=1';
  const xhr = new ctx.XMLHttpRequest();
  xhr.setRequestHeader('X-Test', 'yes');
  if (!ctx.__webReverseLogs.some(x => x.type === 'cookie:set')) throw new Error('missing cookie log');
  if (!ctx.__webReverseLogs.some(x => x.type === 'xhr:header')) throw new Error('missing header log');
}}

function testCryptoJS() {{
  const ctx = {{
    CryptoJS: {{
      MD5(value) {{ return 'md5:' + value; }},
      AES: {{
        encrypt(value, key) {{ return 'aes:' + value + ':' + key; }}
      }}
    }}
  }};
  run('hooks/cryptojs.js', ctx);
  ctx.CryptoJS.MD5('abc');
  ctx.CryptoJS.AES.encrypt('abc', 'key');
  if (!ctx.__webReverseLogs.some(x => x.type === 'cryptojs:MD5')) throw new Error('missing MD5 log');
  if (!ctx.__webReverseLogs.some(x => x.type === 'cryptojs:AES.encrypt')) throw new Error('missing AES log');
}}

(async () => {{
  await testNetwork();
  testCookieHeader();
  testCryptoJS();
}})().catch(error => {{
  console.error(error.stack || error.message);
  process.exit(1);
}});
"""
    with workspace_tempdir() as tmp:
        test_path = Path(tmp) / "hook_runtime_test.js"
        test_path.write_text(runtime_test, encoding="utf-8")
        result = subprocess.run([node, str(test_path)], capture_output=True, text=True)
        assert_true(result.returncode == 0, f"hook runtime test failed: {result.stderr}")


def case_attribution() -> None:
    from attribution_resend import classify_resend

    constant = [{"status": 403, "signal": "h"} for _ in range(5)]
    verdict, _ = classify_resend(constant)
    assert_true(verdict == "constant", f"identical resends must be 'constant', got {verdict}")

    drift = [{"status": 403, "signal": f"h{i}"} for i in range(5)]
    verdict, _ = classify_resend(drift)
    assert_true(verdict == "stateful", f"drifting resends must be 'stateful', got {verdict}")

    status_drift = [{"status": s, "signal": "h"} for s in (403, 403, 429, 403, 200)]
    verdict, _ = classify_resend(status_drift)
    assert_true(verdict == "stateful", f"status drift must be 'stateful', got {verdict}")

    verdict, _ = classify_resend(constant[:3])
    assert_true(verdict == "insufficient", f"<5 samples must be 'insufficient', got {verdict}")


def case_boundaries() -> None:
    checks = {
        SKILL_DIR / "SKILL.md": [
            "Only call tools that are actually present",
            "optional external MCP examples",
            "**Route D (default)**",
            "**No MCP available**",
        ],
        SKILL_DIR / "references" / "01-workflow.md": [
            "工具调用边界",
            "可选外部工具示例",
            "禁止编造工具调用",
            "No MCP fallback",
        ],
        SKILL_DIR / "references" / "08-tool-reference.md": [
            "调用前置条件",
            "不是当前运行环境的工具清单",
            "不要通过 shell 伪造不存在的 MCP 调用",
            "安全降级",
        ],
    }

    for path, required_phrases in checks.items():
        text = path.read_text(encoding="utf-8")
        for phrase in required_phrases:
            assert_true(phrase in text, f"{path.name} missing boundary phrase: {phrase}")


CASES = {
    "scaffold": case_scaffold,
    "deobfuscate": case_deobfuscate,
    "verify": case_verify,
    "env": case_env,
    "cdp": case_cdp,
    "hooks": case_hooks,
    "attribution": case_attribution,
    "boundaries": case_boundaries,
}


def main() -> None:
    parser = argparse.ArgumentParser(description="Run offline web-reverse-master delivery tests")
    parser.add_argument("--case", choices=sorted(CASES), action="append", help="Run only the selected case; repeatable")
    args = parser.parse_args()

    selected = args.case or list(CASES)
    failures: list[str] = []
    for name in selected:
        print(f"[RUN] {name}")
        try:
            CASES[name]()
            print(f"[PASS] {name}")
        except Exception as exc:
            failures.append(name)
            print(f"[FAIL] {name}: {exc}")

    if failures:
        print(f"[SUMMARY] failed: {', '.join(failures)}")
        raise SystemExit(1)

    print(f"[SUMMARY] all {len(selected)} cases passed")


if __name__ == "__main__":
    main()
