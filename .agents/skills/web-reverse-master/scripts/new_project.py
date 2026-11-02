#!/usr/bin/env python3
"""Create a focused Web reverse-engineering case workspace.

The scaffold is intentionally small: each project type gets only the files it
needs for Phase 0-4 evidence capture, implementation, and parity testing.
"""

from __future__ import annotations

import argparse
from datetime import datetime
from pathlib import Path


COMMON_DIRS = [
    "assets/js",
    "assets/screenshots",
    "docs",
    "src/output",
    "tests/fixtures",
]


TEMPLATES = {
    "sign": {
        "dirs": COMMON_DIRS + ["src/env"],
        "files": {
            "src/main.py": '''"""Request driver for {site_name}.

Target: {url}
Created: {date}
"""

from signer import sign


def main():
    sample = {"page": "1"}
    print({"sample": sample, "sign": sign(sample)})


if __name__ == "__main__":
    main()
''',
            "src/signer.py": '''"""Signing or encryption implementation.

Fill this only after Phase 2 confirms algorithm, key source, encoding, and
parameter order with browser evidence.
"""

import hashlib
import json


def canonical_payload(params: dict) -> str:
    return json.dumps(params, ensure_ascii=False, separators=(",", ":"), sort_keys=True)


def sign(params: dict, salt: str = "replace-after-hook") -> str:
    payload = canonical_payload(params)
    return hashlib.md5((payload + salt).encode("utf-8")).hexdigest()
''',
            "tests/test_signer.py": '''from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from signer import sign


def test_sign_is_stable():
    assert sign({"page": "1"}) == sign({"page": "1"})
''',
        },
    },
    "captcha": {
        "dirs": COMMON_DIRS + ["assets/captcha"],
        "files": {
            "src/main.py": '''"""Captcha research driver for {site_name}.

Target: {url}
Created: {date}
"""

from captcha_solver import build_track


def main():
    print(build_track(12, seed=1))


if __name__ == "__main__":
    main()
''',
            "src/captcha_solver.py": '''"""Captcha helper primitives.

Keep solver logic evidence-driven: capture challenge images and browser track
samples before implementing the final submitter.
"""

import random


def build_track(distance: int, seed: int | None = None) -> list[dict]:
    rng = random.Random(seed)
    x = 0
    t = 0
    track = []
    while x < distance:
        x = min(distance, x + rng.randint(1, 3))
        t += rng.randint(12, 35)
        track.append({"x": x, "y": rng.randint(-2, 2), "t": t})
    return track
''',
            "tests/test_captcha_solver.py": '''from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from captcha_solver import build_track


def test_track_reaches_distance():
    track = build_track(12, seed=1)
    assert track[-1]["x"] == 12
    assert all(step["t"] > 0 for step in track)
''',
        },
    },
    "cdp": {
        "dirs": COMMON_DIRS,
        "files": {
            "src/main.py": '''"""CDP bridge driver for {site_name}.

Target: {url}
Created: {date}
"""

from cdp_bridge import CDPBridge


def main():
    print("Provide a CDP WebSocket URL, then call CDPBridge.connect().")


if __name__ == "__main__":
    main()
''',
            "src/cdp_bridge.py": '''"""Project-local CDP bridge placeholder.

Copy or import the skill-level scripts/cdp_bridge.py when the case needs live
browser evaluation.
"""
''',
            "tests/test_cdp_notes.py": '''def test_placeholder():
    assert True
''',
        },
    },
    "doc": {
        "dirs": COMMON_DIRS + ["assets/documents"],
        "files": {
            "src/downloader.py": '''"""Document download research driver for {site_name}.

Target: {url}
Created: {date}
"""

from pathlib import Path

import requests


def download(url: str, output: str) -> Path:
    path = Path(output)
    response = requests.get(url, timeout=30)
    response.raise_for_status()
    path.write_bytes(response.content)
    return path
''',
            "tests/test_downloader_notes.py": '''def test_placeholder():
    assert True
''',
        },
    },
}


def render(content: str, site_name: str, url: str, project_type: str) -> str:
    replacements = {
        "{site_name}": site_name,
        "{url}": url,
        "{project_type}": project_type,
        "{date}": datetime.now().strftime("%Y-%m-%d"),
    }
    for needle, value in replacements.items():
        content = content.replace(needle, value)
    return content


def build_readme(site_name: str, url: str, project_type: str) -> str:
    return f"""# {site_name}

Target: {url}
Type: {project_type}

## Phase Checklist

- [ ] Phase 0: recon notes in docs/recon.md
- [ ] Phase 1: request table in docs/api.md
- [ ] Phase 2: source locations and samples in docs/crypto.md
- [ ] Phase 3: confirmed plan in docs/plan.md
- [ ] Phase 4: implementation and parity test

## Expected Evidence

- Browser request sample
- Local reproduction sample
- Byte-for-byte comparison result
- Remaining assumptions marked as `待验证`
"""


def create_project(site_name: str, url: str, project_type: str, output_dir: str | None = None) -> Path:
    if project_type not in TEMPLATES:
        raise ValueError(f"Unsupported project type: {project_type}")

    base = Path(output_dir).resolve() if output_dir else Path.cwd().resolve()
    site_dir = base / site_name
    if site_dir.exists():
        raise FileExistsError(f"Directory already exists: {site_dir}")

    template = TEMPLATES[project_type]
    for directory in template["dirs"]:
        (site_dir / directory).mkdir(parents=True, exist_ok=True)

    for rel_path, content in template["files"].items():
        path = site_dir / rel_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(render(content, site_name, url, project_type), encoding="utf-8")
        print(f"  [CREATE] {rel_path}")

    for doc_name in ["recon.md", "api.md", "crypto.md", "plan.md"]:
        doc_path = site_dir / "docs" / doc_name
        if not doc_path.exists():
            doc_path.write_text(f"# {doc_name.removesuffix('.md')}\n\n", encoding="utf-8")
            print(f"  [CREATE] docs/{doc_name}")

    readme_path = site_dir / "README.md"
    readme_path.write_text(build_readme(site_name, url, project_type), encoding="utf-8")
    print("  [CREATE] README.md")

    print(f"\n[OK] Project created: {site_dir}")
    return site_dir


def main() -> None:
    parser = argparse.ArgumentParser(description="Create a Web reverse-engineering case workspace")
    parser.add_argument("site_name", help="Project directory name")
    parser.add_argument("--url", required=True, help="Target URL")
    parser.add_argument("--type", default="sign", choices=sorted(TEMPLATES), help="Project type")
    parser.add_argument("--output-dir", "-o", default=None, help="Output directory, default: current directory")
    args = parser.parse_args()

    try:
        create_project(args.site_name, args.url, args.type, args.output_dir)
    except (FileExistsError, ValueError) as exc:
        raise SystemExit(f"[ERROR] {exc}") from exc


if __name__ == "__main__":
    main()
