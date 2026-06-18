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
  - after project restore, opens each restored project with the bundled
    "codex app <path>" command so Codex Desktop registers it

--replace-codex-home is destructive and replaces ~/.codex after backing it up.
Even in replace mode, target login/config identity files are preserved.
Set CODEX_REHOME_SKIP_APP_REGISTRATION=1 for isolated tests that must not open
Codex Desktop.
Set CODEX_REHOME_CODEX_APP_PATH=/path/to/codex to override the bundled CLI path
for tests or nonstandard installs.
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
  echo "Project files restored. Schema v3 metadata import will register project paths when metadata is present."
}

import_ui_ready_metadata() {
  local metadata_dir="$ROOT/metadata"
  [[ -d "$metadata_dir" ]] || { echo "No schema v3 metadata/ directory found; skipping UI-ready metadata import."; return; }
  local py
  if ! py="$(find_python3)"; then
    echo "python3 is unavailable; skipping UI-ready metadata import." >&2
    return
  fi
  "$py" - "$ROOT" "$DST_CODEX_HOME" "$PROJECTS_DIR" "$STAMP" <<'PY'
import json
import os
import shutil
import sqlite3
import sys
from pathlib import Path

root = Path(sys.argv[1])
codex_home = Path(sys.argv[2])
projects_dir = Path(sys.argv[3]).expanduser()
stamp = sys.argv[4]
metadata = root / "metadata"

def read_json(path, default):
    try:
        if path.exists():
            return json.loads(path.read_text(encoding="utf-8", errors="ignore"))
    except Exception:
        pass
    return default

path_map = read_json(metadata / "path_map.json", {"projects": []})
thread_export = read_json(metadata / "thread_index_export.json", {"threads": [], "selected_thread_ids": []})
registry_export = read_json(metadata / "project_ui_registry_export.json", {"project_registry": {}})

def target_for_project(entry):
    name = entry.get("package_project_name") or Path(entry.get("source_path", "")).name
    target = projects_dir / name
    return str(target)

path_pairs = []
target_projects = []
for entry in path_map.get("projects", []):
    target = target_for_project(entry)
    target_projects.append(target)
    for src in entry.get("source_path_variants") or []:
        if src:
            path_pairs.append((src, target))
    src = entry.get("source_path")
    if src:
        path_pairs.append((src, target))
        if not src.startswith("\\\\?\\"):
            path_pairs.append(("\\\\?\\" + src, target))
        path_pairs.append((src.replace("\\", "/"), target))
        path_pairs.append(("/" + src, target))

def map_path(value):
    if value is None:
        return value
    s = str(value)
    for old, new in path_pairs:
        if old and old in s:
            s = s.replace(old, new)
    s = s.replace("\\\\?\\/", "/")
    s = s.replace("\\\\?/", "/")
    return s

def selected_or_exported_ids():
    ids = []
    for tid in thread_export.get("selected_thread_ids", []) or []:
        if tid and tid not in ids:
            ids.append(str(tid))
    for row in thread_export.get("threads", []) or []:
        tid = row.get("id")
        if tid and tid not in ids:
            ids.append(str(tid))
    return ids

def find_session_file(thread_id):
    sessions = codex_home / "sessions"
    if not sessions.exists():
        return None
    matches = list(sessions.rglob(f"*{thread_id}*.jsonl"))
    if matches:
        return matches[0]
    return None

def rewrite_jsonl_paths():
    changed = 0
    for tid in selected_or_exported_ids():
        path = find_session_file(tid)
        if not path or not path.exists():
            continue
        text = path.read_text(encoding="utf-8", errors="ignore")
        new_text = text
        for old, new in path_pairs:
            if old:
                new_text = new_text.replace(old, new)
                new_text = new_text.replace(old.replace("\\", "\\\\"), new)
        if new_text != text:
            backup = path.with_name(path.name + f".backup-pathmap-{stamp}")
            if not backup.exists():
                shutil.copy2(path, backup)
            with path.open("w", encoding="utf-8", newline="\n") as f:
                f.write(new_text)
            changed += 1
    return changed

def newest_state_db():
    dbs = sorted(codex_home.glob("state_*.sqlite"), key=lambda p: p.stat().st_mtime if p.exists() else 0, reverse=True)
    return dbs[0] if dbs else None

def merge_sqlite_threads():
    db = newest_state_db()
    rows = thread_export.get("threads", []) or []
    if not db or not rows:
        return 0
    backup = db.with_name(db.name + f".backup-thread-import-{stamp}")
    if not backup.exists():
        shutil.copy2(db, backup)
    con = sqlite3.connect(str(db))
    con.row_factory = sqlite3.Row
    cols = [r[1] for r in con.execute("pragma table_info(threads)").fetchall()]
    if not cols:
        con.close()
        return 0
    required_defaults = {
        "rollout_path": "",
        "created_at": 0,
        "updated_at": 0,
        "source": "vscode",
        "model_provider": "openai",
        "cwd": "",
        "title": "",
        "sandbox_policy": "{}",
        "approval_mode": "on-request",
        "tokens_used": 0,
        "has_user_event": 0,
        "archived": 0,
        "cli_version": "",
        "first_user_message": "",
        "memory_mode": "enabled",
        "preview": "",
    }
    imported = 0
    for row in rows:
        tid = row.get("id")
        if not tid:
            continue
        d = dict(row)
        cwd = map_path(d.get("cwd") or "")
        if cwd:
            d["cwd"] = cwd
        session_file = find_session_file(str(tid))
        if session_file:
            d["rollout_path"] = str(session_file)
        elif d.get("rollout_path"):
            d["rollout_path"] = map_path(d.get("rollout_path"))
        for key in ["sandbox_policy", "git_origin_url", "agent_path"]:
            if d.get(key):
                d[key] = map_path(d[key])
        existing = con.execute("select * from threads where id=?", (tid,)).fetchone()
        values = {}
        for col in cols:
            if col == "id":
                values[col] = tid
            elif col in d and d[col] is not None:
                values[col] = d[col]
            elif existing is not None:
                values[col] = existing[col]
            else:
                values[col] = required_defaults.get(col)
        if existing is None:
            insert_cols = [c for c in cols if values.get(c) is not None]
            placeholders = ",".join(["?"] * len(insert_cols))
            con.execute(
                f"insert into threads ({','.join(insert_cols)}) values ({placeholders})",
                [values[c] for c in insert_cols],
            )
        else:
            update_cols = [c for c in cols if c != "id" and values.get(c) is not None]
            con.execute(
                f"update threads set {','.join([c + '=?' for c in update_cols])} where id=?",
                [values[c] for c in update_cols] + [tid],
            )
        imported += 1
    con.commit()
    con.close()
    return imported

def merge_global_state():
    state_path = codex_home / ".codex-global-state.json"
    if state_path.exists():
        try:
            data = json.loads(state_path.read_text(encoding="utf-8", errors="ignore"))
        except Exception:
            data = {}
    else:
        data = {}
    backup = state_path.with_name(state_path.name + f".backup-ui-registry-{stamp}")
    if state_path.exists() and not backup.exists():
        shutil.copy2(state_path, backup)
    for key in ["electron-saved-workspace-roots", "project-order", "active-workspace-roots"]:
        arr = data.get(key)
        if not isinstance(arr, list):
            arr = []
        for target in target_projects:
            if target and target not in arr:
                arr.append(target)
        data[key] = arr
    hints = data.get("thread-workspace-root-hints")
    if not isinstance(hints, dict):
        hints = {}
    for row in thread_export.get("threads", []) or []:
        tid = row.get("id")
        cwd = map_path(row.get("cwd") or "")
        if tid and cwd:
            hints[str(tid)] = cwd
    exported_hints = (registry_export.get("project_registry", {}) or {}).get("thread-workspace-root-hints", {}) or {}
    for tid, path in exported_hints.items():
        hints[str(tid)] = map_path(path)
    data["thread-workspace-root-hints"] = hints
    projectless = data.get("projectless-thread-ids")
    if isinstance(projectless, list):
        ids = set(selected_or_exported_ids())
        data["projectless-thread-ids"] = [tid for tid in projectless if tid not in ids]
    atom = data.get("electron-persisted-atom-state")
    if not isinstance(atom, dict):
        atom = {}
    perms = atom.get("heartbeat-thread-permissions-by-id")
    if not isinstance(perms, dict):
        perms = {}
    exported_perms = (registry_export.get("project_registry", {}) or {}).get("heartbeat-thread-permissions-by-id", {}) or {}
    for tid, value in exported_perms.items():
        perms[str(tid)] = value
    atom["heartbeat-thread-permissions-by-id"] = perms
    data["electron-persisted-atom-state"] = atom
    state_path.parent.mkdir(parents=True, exist_ok=True)
    with state_path.open("w", encoding="utf-8", newline="\n") as f:
        f.write(json.dumps(data, ensure_ascii=False, separators=(",", ":")) + "\n")
    return len(target_projects)

path_pairs = sorted(
    list(dict.fromkeys(path_pairs)),
    key=lambda item: len(item[0]) if item and item[0] else 0,
    reverse=True,
)
rewritten = rewrite_jsonl_paths()
imported = merge_sqlite_threads()
registered = merge_global_state()
report = {
    "schema": 3,
    "session_jsonl_rewritten": rewritten,
    "sqlite_threads_imported": imported,
    "restored_projects_registered": registered,
    "restart_required": True,
}
with (codex_home / "codex-rehome-ui-ready-import-report.json").open("w", encoding="utf-8", newline="\n") as f:
    f.write(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
print(json.dumps(report, ensure_ascii=False, separators=(",", ":")))
PY
}

register_restored_projects_with_codex_app() {
  if [[ "$RESTORE_PROJECTS" != "true" ]]; then
    return
  fi
  if [[ "${CODEX_REHOME_SKIP_APP_REGISTRATION:-}" == "1" ]]; then
    echo "Skipping Codex Desktop project registration because CODEX_REHOME_SKIP_APP_REGISTRATION=1"
    write_registration_report "skipped" "CODEX_REHOME_SKIP_APP_REGISTRATION=1" ""
    return
  fi
  local codex_app="${CODEX_REHOME_CODEX_APP_PATH:-/Applications/Codex.app/Contents/Resources/codex}"
  if [[ ! -x "$codex_app" ]]; then
    echo "Codex app CLI not found at $codex_app; project files restored, but app-visible project registration was not invoked."
    write_registration_report "missing_cli" "$codex_app" ""
    return
  fi
  local registered=0
  local failed=0
  local paths=()
  shopt -s nullglob
  for project_path in "$PROJECTS_DIR"/*; do
    [[ -d "$project_path" ]] || continue
    echo "Registering restored project with Codex Desktop: $project_path"
    if "$codex_app" app "$project_path"; then
      registered=$((registered + 1))
      paths+=("$project_path")
    else
      failed=$((failed + 1))
      echo "Warning: Codex Desktop project registration failed for: $project_path" >&2
    fi
  done
  shopt -u nullglob
  if [[ "$registered" -gt 0 && "$failed" -eq 0 ]]; then
    write_registration_report "invoked" "codex app registration invoked" "${paths[@]}"
  elif [[ "$registered" -gt 0 ]]; then
    write_registration_report "partial" "some codex app registrations failed" "${paths[@]}"
  else
    write_registration_report "none" "no restored project directories were registered" ""
  fi
}

write_registration_report() {
  local status="$1"
  local message="$2"
  shift 2
  local report="$DST_CODEX_HOME/codex-rehome-project-registration-report.json"
  local py
  if py="$(find_python3)"; then
    "$py" - "$report" "$status" "$message" "$@" <<'PY'
import json
import sys
from pathlib import Path

report = Path(sys.argv[1])
status = sys.argv[2]
message = sys.argv[3]
paths = [p for p in sys.argv[4:] if p]
payload = {
    "status": status,
    "message": message,
    "method": "codex app <project-path>",
    "registered_project_paths": paths,
}
report.parent.mkdir(parents=True, exist_ok=True)
with report.open("w", encoding="utf-8", newline="\n") as f:
    f.write(json.dumps(payload, ensure_ascii=False, indent=2) + "\n")
PY
  else
    printf '{"status":"%s","message":"%s","method":"codex app <project-path>","registered_project_paths":[]}\n' "$status" "$message" > "$report"
  fi
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

import_ui_ready_metadata
register_restored_projects_with_codex_app

rm -f "$HOME/Library/Application Support/Codex/SingletonLock" \
  "$HOME/Library/Application Support/Codex/SingletonCookie" \
  "$HOME/Library/Application Support/Codex/SingletonSocket"

echo "Done. Merge restore completed. If restored projects were present, Codex Desktop project registration was invoked with codex app <path>."
