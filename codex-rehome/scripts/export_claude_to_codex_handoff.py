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

FORBIDDEN_PARTS = {
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
}

FORBIDDEN_FILES = {
    "auth.json",
    ".env",
    "cookies",
    "login data",
    "local storage",
    "session storage",
    "id_rsa",
    "id_ed25519",
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


def find_skill_dirs(source: Path) -> list[Path]:
    if not source.exists():
        return []
    roots = [source] if source.is_dir() else []
    skills: list[Path] = []
    for root in roots:
        for current, dirs, files in os.walk(root, followlinks=True):
            current_path = Path(current)
            lower_parts = {part.lower() for part in current_path.parts}
            if lower_parts & FORBIDDEN_PARTS:
                dirs[:] = []
                continue
            if "SKILL.md" in files:
                skills.append(current_path)
                dirs[:] = []
    skills.sort(key=lambda p: str(p).lower())
    return skills


def ignore_forbidden(_directory: str, names: list[str]) -> set[str]:
    ignored: set[str] = set()
    for name in names:
        lower = name.lower()
        if lower in FORBIDDEN_FILES or lower in FORBIDDEN_PARTS:
            ignored.add(name)
        elif lower.startswith(".env."):
            ignored.add(name)
    return ignored


def collect_skill_sources(paths: list[str]) -> list[Path]:
    found: list[Path] = []
    seen: set[str] = set()
    for raw in paths:
        for skill in find_skill_dirs(Path(raw).expanduser().resolve()):
            key = str(skill).lower()
            if key not in seen:
                seen.add(key)
                found.append(skill)
    return found


def collect_project_sources(paths: list[str]) -> list[Path]:
    projects: list[Path] = []
    seen: set[str] = set()
    for raw in paths:
        path = Path(raw).expanduser().resolve()
        if not path.exists() or not path.is_dir():
            continue
        key = str(path).lower()
        if key in seen:
            continue
        seen.add(key)
        projects.append(path)
    return projects


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
    if obj.get("type") in {"queue-operation"}:
        return []
    candidates: list[Any] = []
    if "message" in obj:
        candidates.append(obj["message"])
    if "messages" in obj and isinstance(obj["messages"], list):
        candidates.extend(obj["messages"])
    if not candidates:
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


def write_handoff(
    out_dir: Path,
    title: str,
    source_files: list[dict[str, Any]],
    include_raw: bool,
    skill_dirs: list[Path],
    project_dirs: list[Path],
) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    raw_dir = out_dir / "raw-transcripts"
    if include_raw:
        raw_dir.mkdir(exist_ok=True)
    skills_out = out_dir / "claude-skills"
    if skill_dirs:
        skills_out.mkdir(exist_ok=True)
    projects_out = out_dir / "project-files"
    if project_dirs:
        projects_out.mkdir(exist_ok=True)

    total_messages = sum(item["message_count"] for item in source_files)
    manifest = {
        "schema_version": 1,
        "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "title": title,
        "source": "claude-code-or-claude-desktop",
        "source_file_count": len(source_files),
        "message_count": total_messages,
        "include_raw": include_raw,
        "skill_count": len(skill_dirs),
        "skills": [{"name": p.name, "path": str(p)} for p in skill_dirs],
        "project_count": len(project_dirs),
        "projects": [{"name": p.name, "path": str(p)} for p in project_dirs],
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
and/or Claude skills so Codex can continue the work as a project handoff.

What this is:

- A readable handoff package for Codex.
- A way to preserve Claude-side decisions, context, and next steps.
- A way to inspect Claude skills from Codex when skill folders were found.
- A way to carry project files that Claude worked on, when project folders were provided.

What this is not:

- It is not a native Codex sidebar/session restore.
- It does not transfer Claude login state, account membership, cookies, API keys, or subscriptions.
- It does not promise that tool calls or old working-directory handles can continue live.

Open this folder in Codex and ask Codex to read `next-steps-for-codex.md` first.
"""
    (out_dir / "README.md").write_text(readme, encoding="utf-8")

    transcript_lines = [f"# Claude Transcript\n\nSource files: {len(source_files)}\n\n"]
    if not source_files:
        transcript_lines.append(
            "No supported Claude transcript JSONL files were included in this handoff.\n\n"
        )
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

    skills_lines = [f"# Claude Skills\n\nSkill folders included: {len(skill_dirs)}\n\n"]
    for index, skill in enumerate(skill_dirs, start=1):
        target_name = safe_name(skill.name)
        target = skills_out / target_name
        if target.exists():
            target = skills_out / f"{target_name}-{abs(hash(str(skill))) & 0xffff:x}"
        shutil.copytree(skill, target, ignore=ignore_forbidden, dirs_exist_ok=True)
        skill_md = target / "SKILL.md"
        summary = ""
        if skill_md.is_file():
            text = skill_md.read_text(encoding="utf-8", errors="replace")
            summary = "\n".join(text.splitlines()[:20]).strip()
        skills_lines.append(f"## {index}. {skill.name}\n\n")
        skills_lines.append(f"- Source: `{skill}`\n")
        skills_lines.append(f"- Copied to: `claude-skills/{target.name}`\n\n")
        if summary:
            skills_lines.append("First lines of `SKILL.md`:\n\n```text\n")
            skills_lines.append(summary[:2000])
            skills_lines.append("\n```\n\n")
    (out_dir / "claude-skills-index.md").write_text("".join(skills_lines), encoding="utf-8")

    project_lines = [f"# Claude Project Files\n\nProject folders included: {len(project_dirs)}\n\n"]
    for index, project in enumerate(project_dirs, start=1):
        target_name = safe_name(project.name)
        target = projects_out / target_name
        if target.exists():
            target = projects_out / f"{target_name}-{abs(hash(str(project))) & 0xffff:x}"
        shutil.copytree(project, target, ignore=ignore_forbidden, dirs_exist_ok=True)
        project_lines.append(f"## {index}. {project.name}\n\n")
        project_lines.append(f"- Source: `{project}`\n")
        project_lines.append(f"- Copied to: `project-files/{target.name}`\n\n")
    (out_dir / "project-files-index.md").write_text("".join(project_lines), encoding="utf-8")

    next_steps = f"""# Next Steps For Codex

1. Read `README.md` and `source-manifest.json`.
2. Read `claude-transcript.md`.
3. If `claude-skills-index.md` exists, read it and inspect any relevant folder under `claude-skills/`.
4. If `project-files-index.md` exists, inspect the restored project under `project-files/`.
5. Summarize what the Claude-side work was trying to accomplish.
6. Identify decisions, unfinished tasks, files mentioned, skills, and risks.
7. Continue from the restored project folder or ask the user which repository should be opened.

Important boundary:

- Treat the Claude transcript as historical context, not as a live tool state.
- Treat Claude skills as reference material. Convert or install them into Codex only after checking compatibility.
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
    parser.add_argument(
        "--skills-source",
        action="append",
        default=[],
        help="Claude skills directory to include. Can be passed more than once.",
    )
    parser.add_argument(
        "--project-source",
        action="append",
        default=[],
        help="Project folder Claude worked on. Can be passed more than once.",
    )
    parser.add_argument("--json", action="store_true", help="Print JSON result.")
    args = parser.parse_args()

    source = Path(args.source).expanduser().resolve()
    files = find_jsonl_sources(source)
    skill_dirs = collect_skill_sources(args.skills_source)
    project_dirs = collect_project_sources(args.project_source)
    if not files and not skill_dirs and not project_dirs:
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
    if files and not parsed and not skill_dirs and not project_dirs:
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
    write_handoff(out_dir, args.title, parsed, args.include_raw, skill_dirs, project_dirs)
    result = {
        "ok": True,
        "handoff_dir": str(out_dir),
        "source_file_count": len(parsed),
        "message_count": sum(item["message_count"] for item in parsed),
        "skill_count": len(skill_dirs),
        "project_count": len(project_dirs),
        "include_raw": args.include_raw,
    }
    print(json.dumps(result, ensure_ascii=False, indent=2) if args.json else f"Handoff: {out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
