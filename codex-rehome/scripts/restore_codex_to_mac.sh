#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ "$(basename "$ROOT")" == "scripts" ]]; then
  ROOT="$(cd "$ROOT/.." && pwd)"
fi

RESTORE_PROJECTS="false"
PROJECTS_DIR="$HOME/Documents/Codex-Restored-Projects"
REPLACE_CODEX_HOME="false"
REPLACE_STATE="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --restore-projects)
      RESTORE_PROJECTS="true"
      shift
      ;;
    --projects-dir)
      PROJECTS_DIR="$2"
      shift 2
      ;;
    --replace-codex-home)
      REPLACE_CODEX_HOME="true"
      shift
      ;;
    --replace-state)
      REPLACE_STATE="true"
      shift
      ;;
    -h|--help)
      cat <<'EOF'
Usage: Restore-Codex-To-Mac.sh [--restore-projects] [--projects-dir DIR] [--replace-codex-home] [--replace-state]

Default behavior is a merge restore:
  - merges sessions, archived_sessions, skills, plugins/cache, generated_images
  - merges session_index.jsonl without deleting existing entries
  - preserves target auth.json, config.toml, installation_id, models_cache.json,
    and chrome-native-hosts-v2.json
  - does not overwrite state_*.sqlite, memories_*.sqlite, or goals_*.sqlite
    unless --replace-state is passed

--replace-codex-home is destructive and replaces ~/.codex after backing it up.
Even in replace mode, target login/config identity files are preserved.
EOF
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

STAMP="$(date +%Y%m%d-%H%M%S)"
MAC_USER="${USER:-$(id -un 2>/dev/null || echo unknown)}"
SRC_CODEX_HOME="$ROOT/home/.codex"
DST_CODEX_HOME="$HOME/.codex"

PRESERVE_FILES=(
  "auth.json"
  "config.toml"
  "installation_id"
  "models_cache.json"
  "chrome-native-hosts-v2.json"
)

normalize_package_permissions() {
  local rel
  for rel in home projects appdata_roaming appdata_local mac_only selected_chats; do
    if [[ -e "$ROOT/$rel" ]]; then
      chmod -R u+rwX "$ROOT/$rel" || {
        echo "Failed to normalize permissions for $ROOT/$rel" >&2
        exit 1
      }
    fi
  done
}

copy_dir_contents() {
  local src="$1"
  local dst="$2"
  [[ -d "$src" ]] || return 0
  if [[ ! -r "$src" || ! -x "$src" ]]; then
    echo "Source exists but is not readable/enterable: $src" >&2
    exit 1
  fi
  mkdir -p "$dst"
  if command -v rsync >/dev/null 2>&1; then
    rsync -aE "$src/" "$dst/"
  elif command -v ditto >/dev/null 2>&1; then
    ditto "$src" "$dst"
  else
    cp -Rp "$src/." "$dst/"
  fi
}

copy_file_preserve() {
  local src="$1"
  local dst="$2"
  [[ -f "$src" ]] || return 0
  mkdir -p "$(dirname "$dst")"
  cp -p "$src" "$dst"
}

find_python3() {
  local cmd path
  for cmd in python3 /usr/bin/python3 python; do
    path="$(command -v "$cmd" 2>/dev/null || true)"
    [[ -n "$path" ]] || continue
    case "$path" in
      *WindowsApps*) continue ;;
    esac
    if "$path" -c 'import json' >/dev/null 2>&1; then
      printf '%s\n' "$path"
      return 0
    fi
  done
  return 1
}

backup_copy_if_exists() {
  local path="$1"
  if [[ -e "$path" ]]; then
    local backup="$path.backup-$STAMP"
    echo "Backing up existing data copy:"
    echo "  $path"
    echo "  -> $backup"
    cp -a "$path" "$backup"
  fi
}

backup_app_profile_if_exists() {
  local path="$1"
  [[ -e "$path" ]] || return 0
  backup_copy_if_exists "$path"
}

save_preserved_files() {
  local keep_dir="$1"
  local name
  mkdir -p "$keep_dir"
  for name in "${PRESERVE_FILES[@]}"; do
    if [[ -f "$DST_CODEX_HOME/$name" ]]; then
      mkdir -p "$keep_dir/$(dirname "$name")"
      cp -p "$DST_CODEX_HOME/$name" "$keep_dir/$name"
    fi
  done
}

restore_preserved_files() {
  local keep_dir="$1"
  local name
  for name in "${PRESERVE_FILES[@]}"; do
    if [[ -f "$keep_dir/$name" ]]; then
      mkdir -p "$DST_CODEX_HOME/$(dirname "$name")"
      cp -p "$keep_dir/$name" "$DST_CODEX_HOME/$name"
    fi
  done
}

replace_codex_home() {
  local keep_dir="$HOME/.codex.preserved-$STAMP"
  save_preserved_files "$keep_dir"
  if [[ -e "$DST_CODEX_HOME" ]]; then
    local backup="$DST_CODEX_HOME.backup-$STAMP"
    echo "Replacing ~/.codex after backup:"
    echo "  $DST_CODEX_HOME"
    echo "  -> $backup"
    mv "$DST_CODEX_HOME" "$backup"
  fi
  mkdir -p "$(dirname "$DST_CODEX_HOME")"
  copy_dir_contents "$SRC_CODEX_HOME" "$DST_CODEX_HOME"
  restore_preserved_files "$keep_dir"
  rm -rf "$keep_dir"
}

merge_state_files() {
  local pattern file base dst
  for pattern in state_*.sqlite state_*.sqlite-* memories_*.sqlite memories_*.sqlite-* goals_*.sqlite goals_*.sqlite-*; do
    shopt -s nullglob
    for file in "$SRC_CODEX_HOME"/$pattern; do
      [[ -f "$file" ]] || continue
      base="$(basename "$file")"
      dst="$DST_CODEX_HOME/$base"
      if [[ "$REPLACE_STATE" == "true" || ! -e "$dst" ]]; then
        copy_file_preserve "$file" "$dst"
        echo "Restored state file: $dst"
      else
        echo "Kept existing state file: $dst"
      fi
    done
    shopt -u nullglob
  done
}

merge_session_index() {
  mkdir -p "$DST_CODEX_HOME"
  local py
  if py="$(find_python3)"; then
    "$py" - "$SRC_CODEX_HOME" "$DST_CODEX_HOME" "$ROOT/selected_chats" <<'PY'
import json
import os
import sys
from pathlib import Path

src = Path(sys.argv[1])
dst = Path(sys.argv[2])
selected = Path(sys.argv[3])
target_index = dst / "session_index.jsonl"
package_index = src / "session_index.jsonl"

def read_jsonl(path):
    if not path.exists():
        return []
    rows = []
    with path.open("r", encoding="utf-8", errors="ignore") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except Exception:
                continue
    return rows

def session_meta_from_file(path):
    session_id = ""
    thread_name = ""
    updated_at = ""
    first_user = ""
    try:
        with path.open("r", encoding="utf-8", errors="ignore") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    row = json.loads(line)
                except Exception:
                    continue
                payload = row.get("payload") if isinstance(row.get("payload"), dict) else {}
                row_id = row.get("id") or payload.get("id") or row.get("session_id") or payload.get("session_id")
                if row.get("type") == "session_meta" or payload.get("type") == "session_meta":
                    session_id = str(payload.get("id") or row_id or session_id or path.stem)
                    thread_name = str(payload.get("thread_name") or payload.get("name") or payload.get("title") or thread_name)
                if not session_id and row_id:
                    session_id = str(row_id)
                ts = row.get("timestamp") or payload.get("timestamp") or row.get("updated_at") or payload.get("updated_at")
                if ts:
                    updated_at = str(ts)
                if not first_user and isinstance(payload.get("message"), dict):
                    content = payload["message"].get("content")
                    role = payload["message"].get("role")
                    if role == "user" and isinstance(content, str):
                        first_user = content.strip().replace("\n", " ")[:80]
    except Exception:
        pass
    if not session_id:
        session_id = path.stem
    if not thread_name:
        thread_name = first_user or path.stem
    return {"id": session_id, "thread_name": thread_name, "updated_at": updated_at}

def generated_entries():
    seen = set()
    roots = [src / "sessions"]
    if selected.exists():
        roots.append(selected)
    for root in roots:
        if not root.exists():
            continue
        for path in root.rglob("*.jsonl"):
            entry = session_meta_from_file(path)
            sid = str(entry.get("id") or "")
            if sid and sid not in seen:
                seen.add(sid)
                yield entry

rows = []
ids = set()
for row in read_jsonl(target_index):
    sid = str(row.get("id") or "")
    if sid and sid not in ids:
        rows.append(row)
        ids.add(sid)

source_rows = read_jsonl(package_index)
if not source_rows:
    source_rows = list(generated_entries())

for row in source_rows:
    sid = str(row.get("id") or "")
    if sid and sid not in ids:
        rows.append({
            "id": sid,
            "thread_name": row.get("thread_name") or row.get("name") or row.get("title") or sid,
            "updated_at": row.get("updated_at") or row.get("timestamp") or ""
        })
        ids.add(sid)

target_index.parent.mkdir(parents=True, exist_ok=True)
with target_index.open("w", encoding="utf-8", newline="\n") as f:
    for row in rows:
        f.write(json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n")
PY
    echo "Merged session_index.jsonl"
  else
    local src_index="$SRC_CODEX_HOME/session_index.jsonl"
    if [[ -f "$src_index" ]]; then
      touch "$DST_CODEX_HOME/session_index.jsonl"
      cat "$src_index" >> "$DST_CODEX_HOME/session_index.jsonl"
      echo "Appended session_index.jsonl without de-duplication because python3 is unavailable"
    fi
  fi
}

merge_codex_home() {
  mkdir -p "$DST_CODEX_HOME"
  backup_copy_if_exists "$DST_CODEX_HOME"

  copy_dir_contents "$SRC_CODEX_HOME/sessions" "$DST_CODEX_HOME/sessions"
  copy_dir_contents "$SRC_CODEX_HOME/archived_sessions" "$DST_CODEX_HOME/archived_sessions"
  copy_dir_contents "$SRC_CODEX_HOME/skills" "$DST_CODEX_HOME/skills"
  copy_dir_contents "$SRC_CODEX_HOME/plugins/cache" "$DST_CODEX_HOME/plugins/cache"
  copy_dir_contents "$SRC_CODEX_HOME/generated_images" "$DST_CODEX_HOME/generated_images"

  merge_state_files
  merge_session_index

  local name
  for name in "${PRESERVE_FILES[@]}"; do
    if [[ -f "$DST_CODEX_HOME/$name" ]]; then
      echo "Preserved target file: $DST_CODEX_HOME/$name"
    fi
  done
}

merge_app_profile() {
  local src="$1"
  local dst="$2"
  [[ -d "$src" ]] || { echo "Skipping missing source: $src"; return; }
  backup_app_profile_if_exists "$dst"
  copy_dir_contents "$src" "$dst"
  echo "Merged app profile: $dst"
}

restore_projects() {
  local src_root="$ROOT/projects"
  if [[ "$RESTORE_PROJECTS" != "true" ]]; then
    echo "Project restore not requested. Pass --restore-projects to copy projects/."
    return
  fi
  if [[ ! -d "$src_root" ]]; then
    echo "Project restore requested but package has no projects/ directory." >&2
    exit 1
  fi
  mkdir -p "$PROJECTS_DIR"
  local restored=0
  local project target
  shopt -s nullglob
  for project in "$src_root"/*; do
    [[ -d "$project" ]] || continue
    target="$PROJECTS_DIR/$(basename "$project")"
    copy_dir_contents "$project" "$target"
    restored=$((restored + 1))
  done
  shopt -u nullglob
  if [[ "$restored" -eq 0 ]]; then
    echo "Project restore requested but projects/ contains no project folders." >&2
    exit 1
  fi
  echo "Restored project folders to: $PROJECTS_DIR"
  echo "Project UI registration is not automatic; reopen the restored project folder in Codex."
}

normalize_package_permissions

if [[ ! -d "$SRC_CODEX_HOME" ]]; then
  echo "Required source missing: $SRC_CODEX_HOME" >&2
  exit 1
fi
if [[ ! -r "$SRC_CODEX_HOME" || ! -x "$SRC_CODEX_HOME" ]]; then
  echo "Required source is not readable/enterable: $SRC_CODEX_HOME" >&2
  exit 1
fi

if pgrep -if "Codex" >/dev/null 2>&1; then
  if [[ "$HOME" == /tmp/codex-* || "$HOME" == /private/tmp/codex-* ]]; then
    echo "Codex appears to be running, but HOME is a temporary isolated restore target; continuing."
  else
    echo "Codex appears to be running. Close Codex before continuing."
    read -r -p "Press Enter after Codex is closed"
  fi
fi

echo "Restoring Codex data to Mac user: $MAC_USER"
echo "Restore mode: $( [[ "$REPLACE_CODEX_HOME" == "true" ]] && echo replace-codex-home || echo merge )"

if [[ "$REPLACE_CODEX_HOME" == "true" ]]; then
  replace_codex_home
  if [[ "$REPLACE_STATE" == "false" ]]; then
    echo "--replace-codex-home was used; package state files are present, but target login/config files were preserved."
  fi
else
  merge_codex_home
fi

merge_app_profile "$ROOT/appdata_roaming/Codex" "$HOME/Library/Application Support/Codex"
merge_app_profile "$ROOT/appdata_roaming/com.openai.codex" "$HOME/Library/Application Support/com.openai.codex"
merge_app_profile "$ROOT/appdata_roaming/OpenAI/Codex" "$HOME/Library/Application Support/OpenAI/Codex"
merge_app_profile "$ROOT/appdata_local/Codex" "$HOME/Library/Caches/Codex"
merge_app_profile "$ROOT/appdata_local/com.openai.codex" "$HOME/Library/Caches/com.openai.codex"
merge_app_profile "$ROOT/appdata_local/com.openai.sky.CUAService" "$HOME/Library/Caches/com.openai.sky.CUAService"
merge_app_profile "$ROOT/appdata_local/com.openai.sky.CUAService.cli" "$HOME/Library/Caches/com.openai.sky.CUAService.cli"

copy_file_preserve "$ROOT/mac_only/Library/Preferences/com.openai.codex.plist" "$HOME/Library/Preferences/com.openai.codex.plist"
copy_file_preserve "$ROOT/mac_only/Library/Preferences/com.openai.sky.CUAService.plist" "$HOME/Library/Preferences/com.openai.sky.CUAService.plist"
copy_file_preserve "$ROOT/mac_only/Library/Preferences/com.openai.sky.CUAService.cli.plist" "$HOME/Library/Preferences/com.openai.sky.CUAService.cli.plist"

restore_projects

rm -f "$HOME/Library/Application Support/Codex/SingletonLock" \
  "$HOME/Library/Application Support/Codex/SingletonCookie" \
  "$HOME/Library/Application Support/Codex/SingletonSocket"

echo "Done. Merge restore completed. Open Codex and log in again if prompted."
