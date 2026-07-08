#!/usr/bin/env python3
"""Inspect local Claude Code / Claude Desktop agent history sources.

This script is intentionally read-only. It reports whether exportable Claude
transcripts exist and whether the installed Claude app is blocked by plan
entitlement, which is common when Claude Desktop is installed with a free
account.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path
from typing import Any


def env_path(name: str) -> Path | None:
    value = os.environ.get(name)
    return Path(value) if value else None


def candidate_roots() -> list[dict[str, Any]]:
    home = Path.home()
    appdata = env_path("APPDATA")
    localappdata = env_path("LOCALAPPDATA")
    roots: list[dict[str, Any]] = [
        {
            "kind": "claude_code_cli_projects",
            "path": home / ".claude" / "projects",
            "notes": "Traditional Claude Code CLI transcripts.",
        }
    ]
    if appdata:
        roots.extend(
            [
                {
                    "kind": "claude_desktop_code_sessions",
                    "path": appdata / "Claude" / "claude-code-sessions",
                    "notes": "Claude Desktop Claude Code session storage.",
                },
                {
                    "kind": "claude_desktop_local_agent_sessions",
                    "path": appdata / "Claude" / "local-agent-mode-sessions",
                    "notes": "Claude Desktop local agent mode session storage.",
                },
            ]
        )
    if localappdata:
        packages = localappdata / "Packages"
        if packages.exists():
            for package in packages.glob("Claude_*"):
                roaming = package / "LocalCache" / "Roaming" / "Claude"
                roots.extend(
                    [
                        {
                            "kind": "msix_claude_code_sessions",
                            "path": roaming / "claude-code-sessions",
                            "notes": "MSIX-virtualized Claude Code sessions.",
                        },
                        {
                            "kind": "msix_local_agent_sessions",
                            "path": roaming / "local-agent-mode-sessions",
                            "notes": "MSIX-virtualized local agent sessions.",
                        },
                    ]
                )
    return roots


def is_noise_path(path: Path) -> bool:
    parts = {p.lower() for p in path.parts}
    noisy = {
        "skills-plugin",
        "skills",
        "node_modules",
        "cache",
        "code cache",
        "local storage",
        "session storage",
        "gpucache",
        "crashpad",
        "schemas",
    }
    return bool(parts & noisy)


def inspect_root(root: dict[str, Any]) -> dict[str, Any]:
    path = Path(root["path"])
    files: list[Path] = []
    if path.exists():
        for candidate in path.rglob("*"):
            if candidate.is_file() and candidate.suffix.lower() == ".jsonl" and not is_noise_path(candidate):
                files.append(candidate)
    files.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    total_bytes = sum(p.stat().st_size for p in files)
    return {
        "kind": root["kind"],
        "path": str(path),
        "exists": path.exists(),
        "notes": root["notes"],
        "jsonl_count": len(files),
        "total_bytes": total_bytes,
        "recent_jsonl": [
            {
                "path": str(p),
                "size": p.stat().st_size,
                "modified": int(p.stat().st_mtime),
            }
            for p in files[:10]
        ],
    }


def log_candidates() -> list[Path]:
    out: list[Path] = []
    appdata = env_path("APPDATA")
    localappdata = env_path("LOCALAPPDATA")
    if appdata:
        out.append(appdata / "Claude" / "logs" / "main.log")
    if localappdata:
        packages = localappdata / "Packages"
        if packages.exists():
            for package in packages.glob("Claude_*"):
                out.append(package / "LocalCache" / "Roaming" / "Claude" / "logs" / "main.log")
    return out


def inspect_logs() -> dict[str, Any]:
    entitlement_errors: list[str] = []
    loaded_lines: list[str] = []
    storage_missing_lines: list[str] = []
    for log in log_candidates():
        if not log.exists():
            continue
        try:
            lines = log.read_text(encoding="utf-8", errors="replace").splitlines()
        except OSError:
            continue
        for line in lines[-1000:]:
            lower = line.lower()
            if "requires a pro or max subscription" in lower:
                entitlement_errors.append(line.strip())
            elif "loaded " in lower and " persisted sessions" in lower:
                loaded_lines.append(line.strip())
            elif "session storage directory does not exist" in lower:
                storage_missing_lines.append(line.strip())
    return {
        "entitlement_error_count": len(entitlement_errors),
        "has_pro_or_max_required_error": bool(entitlement_errors),
        "entitlement_error_samples": entitlement_errors[-3:],
        "loaded_session_lines": loaded_lines[-5:],
        "missing_storage_lines": storage_missing_lines[-5:],
    }


def inspect_cct() -> dict[str, Any]:
    exe = shutil.which("cct")
    result: dict[str, Any] = {"available": bool(exe), "path": exe}
    if exe:
        try:
            completed = subprocess.run(
                [exe, "--version"],
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                timeout=10,
            )
            result["version_output"] = completed.stdout.strip()
        except Exception as exc:  # pragma: no cover - defensive reporting
            result["version_error"] = str(exc)
    return result


def build_report() -> dict[str, Any]:
    sources = [inspect_root(root) for root in candidate_roots()]
    exportable = sum(source["jsonl_count"] for source in sources)
    logs = inspect_logs()
    if exportable:
        status = "exportable_transcripts_found"
    elif logs["has_pro_or_max_required_error"]:
        status = "installed_but_no_entitled_claude_code_sessions"
    else:
        status = "no_exportable_transcripts_found"
    return {
        "schema_version": 1,
        "status": status,
        "source_count": len(sources),
        "exportable_jsonl_count": exportable,
        "sources": sources,
        "logs": logs,
        "optional_cct_backend": inspect_cct(),
    }


def print_text(report: dict[str, Any]) -> None:
    print(f"Status: {report['status']}")
    print(f"Exportable Claude JSONL transcripts: {report['exportable_jsonl_count']}")
    if report["logs"]["has_pro_or_max_required_error"]:
        print("Claude Code entitlement: blocked by Pro/Max requirement")
    print("Sources:")
    for source in report["sources"]:
        marker = "found" if source["exists"] else "missing"
        print(f"- {source['kind']}: {marker}, jsonl={source['jsonl_count']}")
        print(f"  {source['path']}")
    cct = report["optional_cct_backend"]
    print(f"Optional cct backend: {'available' if cct['available'] else 'not found'}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="Print machine-readable JSON.")
    args = parser.parse_args()
    report = build_report()
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        print_text(report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
