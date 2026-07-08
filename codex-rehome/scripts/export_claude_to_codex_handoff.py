#!/usr/bin/env python3
"""Export Claude Code transcripts into a Codex-readable handoff folder."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shutil
from pathlib import Path
from typing import Any


NOISE_PARTS = {
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


def is_noise_path(path: Path) -> bool:
    return bool({p.lower() for p in path.parts} & NOISE_PARTS)


def find_jsonl_sources(source: Path) -> list[Path]:
    if source.is_file() and source.suffix.lower() == ".jsonl":
        return [source]
    if not source.exists():
        return []
    files = [
        p
        for p in source.rglob("*.jsonl")
        if p.is_file() and not is_noise_path(p)
    ]
    files.sort(key=lambda p: p.stat().st_mtime)
    return files


def stringify_content(content: Any) -> str:
    if content is None:
        return ""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts: list[str] = []
        for item in content:
            if isinstance(item, str):
                parts.append(item)
            elif isinstance(item, dict):
                text = item.get("text") or item.get("content") or item.get("input") or item.get("name")
                if text:
                    parts.append(str(text))
        return "\n".join(parts)
    if isinstance(content, dict):
        text = content.get("text") or content.get("content") or content.get("message")
        if text:
            return stringify_content(text)
    return json.dumps(content, ensure_ascii=False, sort_keys=True)


def extract_messages(obj: dict[str, Any]) -> list[dict[str, str]]:
    candidates: list[Any] = []
    if "message" in obj:
        candidates.append(obj["message"])
    if "messages" in obj and isinstance(obj["messages"], list):
        candidates.extend(obj["messages"])
    candidates.append(obj)

    messages: list[dict[str, str]] = []
    for item in candidates:
        if not isinstance(item, dict):
            continue
        role = item.get("role") or item.get("type") or item.get("speaker")
        content = item.get("content")
        if content is None:
            content = item.get("text") or item.get("message")
        text = stringify_content(content).strip()
        if role and text:
            normalized_role = str(role)
            if normalized_role == "assistant_message":
                normalized_role = "assistant"
            elif normalized_role == "user_message":
                normalized_role = "user"
            messages.append({"role": normalized_role, "text": text})
    return messages


def scan_metadata(obj: dict[str, Any]) -> dict[str, Any]:
    keys = (
        "cwd",
        "project_path",
        "workspace",
        "workspaceRoot",
        "sessionId",
        "session_id",
        "conversationId",
        "uuid",
        "timestamp",
        "created_at",
    )
    return {key: obj[key] for key in keys if key in obj}


def parse_jsonl(path: Path) -> dict[str, Any]:
    messages: list[dict[str, str]] = []
    metadata: dict[str, Any] = {}
    line_count = 0
    parse_errors = 0
    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line_count += 1
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                parse_errors += 1
                continue
            if isinstance(obj, dict):
                metadata.update(scan_metadata(obj))
                messages.extend(extract_messages(obj))
    return {
        "path": str(path),
        "line_count": line_count,
        "parse_errors": parse_errors,
        "message_count": len(messages),
        "metadata": metadata,
        "messages": messages,
    }


def safe_name(value: str) -> str:
    value = re.sub(r"[^A-Za-z0-9._ -]+", "-", value).strip(" .-")
    return value[:80] or "claude-handoff"


def write_handoff(out_dir: Path, title: str, source_files: list[dict[str, Any]], include_raw: bool) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    raw_dir = out_dir / "raw-transcripts"
    if include_raw:
        raw_dir.mkdir(exist_ok=True)

    total_messages = sum(item["message_count"] for item in source_files)
    manifest = {
        "schema_version": 1,
        "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "title": title,
        "source": "claude-code-or-claude-desktop",
        "source_file_count": len(source_files),
        "message_count": total_messages,
        "include_raw": include_raw,
        "files": [
            {
                "path": item["path"],
                "line_count": item["line_count"],
                "parse_errors": item["parse_errors"],
                "message_count": item["message_count"],
                "metadata": item["metadata"],
            }
            for item in source_files
        ],
    }
    (out_dir / "source-manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )

    readme = f"""# {title}

This folder was generated from local Claude Code / Claude Desktop transcripts
so Codex can continue the work as a project handoff.

What this is:

- A readable handoff package for Codex.
- A way to preserve Claude-side decisions, context, and next steps.

What this is not:

- It is not a native Codex sidebar/session restore.
- It does not transfer Claude login state, account membership, cookies, API keys, or subscriptions.
- It does not promise that tool calls or old working-directory handles can continue live.

Open this folder in Codex and ask Codex to read `next-steps-for-codex.md` first.
"""
    (out_dir / "README.md").write_text(readme, encoding="utf-8")

    transcript_lines = [f"# Claude Transcript\n\nSource files: {len(source_files)}\n\n"]
    for index, item in enumerate(source_files, start=1):
        transcript_lines.append(f"## Source {index}: `{Path(item['path']).name}`\n\n")
        if item["metadata"]:
            transcript_lines.append("Metadata:\n\n")
            for key, value in item["metadata"].items():
                transcript_lines.append(f"- `{key}`: `{value}`\n")
            transcript_lines.append("\n")
        for message in item["messages"]:
            role = message["role"].upper()
            transcript_lines.append(f"### {role}\n\n{message['text']}\n\n")
    (out_dir / "claude-transcript.md").write_text("".join(transcript_lines), encoding="utf-8")

    next_steps = f"""# Next Steps For Codex

1. Read `README.md` and `source-manifest.json`.
2. Read `claude-transcript.md`.
3. Summarize what the Claude-side work was trying to accomplish.
4. Identify decisions, unfinished tasks, files mentioned, and risks.
5. Continue from the restored project folder or ask the user which repository should be opened.

Important boundary:

- Treat the Claude transcript as historical context, not as a live tool state.
- Do not assume Claude's old working directory still exists.
- If project files are missing, ask the user for the project folder before editing.
"""
    (out_dir / "next-steps-for-codex.md").write_text(next_steps, encoding="utf-8")

    decisions = """# Decisions And Open Questions

Codex should fill this in after reading `claude-transcript.md`.

## Decisions

- 

## Open Questions

- 

## Files Or Paths Mentioned

- 
"""
    (out_dir / "decisions.md").write_text(decisions, encoding="utf-8")

    if include_raw:
        for item in source_files:
            source_path = Path(item["path"])
            target = raw_dir / safe_name(source_path.name)
            if target.exists():
                target = raw_dir / f"{safe_name(source_path.stem)}-{abs(hash(str(source_path))) & 0xffff:x}.jsonl"
            shutil.copy2(source_path, target)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, help="Claude JSONL file or directory to export.")
    parser.add_argument("--out", default=None, help="Output parent directory.")
    parser.add_argument("--title", default="Claude To Codex Handoff", help="Handoff title.")
    parser.add_argument("--include-raw", action="store_true", help="Copy raw JSONL transcripts into the handoff.")
    parser.add_argument("--json", action="store_true", help="Print JSON result.")
    args = parser.parse_args()

    source = Path(args.source).expanduser().resolve()
    files = find_jsonl_sources(source)
    if not files:
        result = {
            "ok": False,
            "reason": "no_exportable_jsonl_found",
            "source": str(source),
            "hint": "Run inspect_claude_agent_sources.py --json to see detected Claude sources and entitlement status.",
        }
        print(json.dumps(result, ensure_ascii=False, indent=2) if args.json else result["hint"])
        return 2

    parsed = [parse_jsonl(path) for path in files]
    parsed = [item for item in parsed if item["message_count"] > 0]
    if not parsed:
        result = {
            "ok": False,
            "reason": "jsonl_files_contained_no_supported_messages",
            "source": str(source),
        }
        print(json.dumps(result, ensure_ascii=False, indent=2) if args.json else result["reason"])
        return 3

    parent = Path(args.out).expanduser().resolve() if args.out else Path.cwd()
    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    out_dir = parent / f"{safe_name(args.title)}-{stamp}"
    write_handoff(out_dir, args.title, parsed, args.include_raw)
    result = {
        "ok": True,
        "handoff_dir": str(out_dir),
        "source_file_count": len(parsed),
        "message_count": sum(item["message_count"] for item in parsed),
        "include_raw": args.include_raw,
    }
    print(json.dumps(result, ensure_ascii=False, indent=2) if args.json else f"Handoff: {out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
