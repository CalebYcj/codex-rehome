#!/usr/bin/env python3
"""Verify a Claude-to-Codex handoff folder."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


FORBIDDEN_NAMES = {
    "auth.json",
    ".env",
    "cookies",
    "login data",
    "local storage",
    "session storage",
    "id_rsa",
    "id_ed25519",
}


def verify(path: Path) -> dict:
    required = [
        "README.md",
        "claude-transcript.md",
        "next-steps-for-codex.md",
        "decisions.md",
        "source-manifest.json",
    ]
    files = list(path.rglob("*")) if path.exists() else []
    forbidden = [
        str(item)
        for item in files
        if item.is_file() and item.name.lower() in FORBIDDEN_NAMES
    ]
    missing = [name for name in required if not (path / name).is_file()]
    manifest = {}
    manifest_path = path / "source-manifest.json"
    if manifest_path.is_file():
        try:
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            manifest = {"parse_error": str(exc)}
    transcript_size = (path / "claude-transcript.md").stat().st_size if (path / "claude-transcript.md").is_file() else 0
    ok = not missing and not forbidden and transcript_size > 0 and not manifest.get("parse_error")
    return {
        "ok": ok,
        "path": str(path),
        "missing_required": missing,
        "forbidden_file_count": len(forbidden),
        "forbidden_files": forbidden,
        "transcript_size": transcript_size,
        "manifest": manifest,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("handoff_dir")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    report = verify(Path(args.handoff_dir).expanduser().resolve())
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        print(f"OK: {report['ok']}")
        print(f"Missing required: {', '.join(report['missing_required']) or 'none'}")
        print(f"Forbidden files: {report['forbidden_file_count']}")
        print(f"Transcript size: {report['transcript_size']}")
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
