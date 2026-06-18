#!/usr/bin/env bash
set -euo pipefail

PACKAGE_ROOT=""
JSON="false"
PROJECTS_DIR="$HOME/Documents/Codex-Restored-Projects"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --package-root)
      PACKAGE_ROOT="$2"
      shift 2
      ;;
    --projects-dir)
      PROJECTS_DIR="$2"
      shift 2
      ;;
    --json)
      JSON="true"
      shift
      ;;
    -h|--help)
      echo "Usage: verify_mac_codex_restore.sh [--package-root DIR] [--projects-dir DIR] [--json]"
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$PACKAGE_ROOT" ]]; then
  PACKAGE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  if [[ "$(basename "$PACKAGE_ROOT")" == "scripts" ]]; then
    PACKAGE_ROOT="$(cd "$PACKAGE_ROOT/.." && pwd)"
  fi
fi

CODEX_HOME="$HOME/.codex"
APP_SUPPORT="$HOME/Library/Application Support/Codex"
MAC_USER="${USER:-$(id -un 2>/dev/null || echo unknown)}"

count_files() {
  local path="$1"
  local name="${2:-*}"
  [[ -d "$path" ]] || { echo 0; return; }
  find "$path" -name "$name" -type f 2>/dev/null | wc -l | tr -d ' '
}

count_dirs() {
  local path="$1"
  local name="${2:-*}"
  [[ -d "$path" ]] || { echo 0; return; }
  find "$path" -name "$name" -type d 2>/dev/null | wc -l | tr -d ' '
}

size_mb() {
  local path="$1"
  [[ -e "$path" ]] || { echo "null"; return; }
  du -sk "$path" 2>/dev/null | awk '{printf "%.2f", $1 / 1024}'
}

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

json_string_array_from_lines() {
  local first="true"
  printf '['
  while IFS= read -r line; do
    [[ -n "$line" ]] || continue
    if [[ "$first" == "true" ]]; then
      first="false"
    else
      printf ','
    fi
    printf '"%s"' "$(json_escape "$line")"
  done
  printf ']'
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

checksum_status() {
  if [[ ! -f "$PACKAGE_ROOT/SHA256SUMS.txt" ]]; then
    echo "missing"
    return
  fi
  if command -v shasum >/dev/null 2>&1; then
    if (cd "$PACKAGE_ROOT" && shasum -a 256 -c SHA256SUMS.txt >/dev/null 2>&1); then
      echo "ok"
    else
      echo "failed"
    fi
  elif command -v sha256sum >/dev/null 2>&1; then
    if (cd "$PACKAGE_ROOT" && sha256sum -c SHA256SUMS.txt >/dev/null 2>&1); then
      echo "ok"
    else
      echo "failed"
    fi
  else
    echo "unavailable"
  fi
}

forbidden_count_in_root() {
  local root="$1"
  [[ -d "$root" ]] || { echo 0; return; }
  find "$root" \( \
    -name 'auth.json' -o \
    -name '.env' -o -name '.env.*' -o \
    -name '*.pem' -o -name '*.key' -o \
    -name 'id_rsa' -o -name 'id_dsa' -o -name 'id_ecdsa' -o -name 'id_ed25519' -o \
    -name 'Cookies' -o -name 'Cookies-journal' -o \
    -name 'Login Data' -o -name 'Login Data-journal' -o \
    -name 'Login Data For Account' -o -name 'Login Data For Account-journal' -o \
    -name 'Local Storage' -o -name 'Session Storage' -o \
    -name '.git' -o -name 'node_modules' -o -name '.venv' -o -name 'venv' -o \
    -name 'SingletonLock' -o -name 'SingletonCookie' -o -name 'SingletonSocket' -o \
    -name '*.sock' -o -name '*.ipc' \
  \) 2>/dev/null | wc -l | tr -d ' '
}

forbidden_count_in_codex_home() {
  [[ -d "$CODEX_HOME" ]] || { echo 0; return; }
  find "$CODEX_HOME" \( \
    \( -name 'auth.json' ! -path "$CODEX_HOME/auth.json" \) -o \
    -name '.env' -o -name '.env.*' -o \
    -name '*.pem' -o -name '*.key' -o \
    -name 'id_rsa' -o -name 'id_dsa' -o -name 'id_ecdsa' -o -name 'id_ed25519' -o \
    -name 'Cookies' -o -name 'Cookies-journal' -o \
    -name 'Login Data' -o -name 'Login Data-journal' -o \
    -name 'Login Data For Account' -o -name 'Login Data For Account-journal' -o \
    -name 'Local Storage' -o -name 'Session Storage' -o \
    -name '.git' -o -name 'node_modules' -o -name '.venv' -o -name 'venv' -o \
    -name 'SingletonLock' -o -name 'SingletonCookie' -o -name 'SingletonSocket' -o \
    -name '*.sock' -o -name '*.ipc' \
  \) 2>/dev/null | wc -l | tr -d ' '
}

selected_chats_count() {
  count_files "$PACKAGE_ROOT/selected_chats" "*.jsonl"
}

session_index_entries_count() {
  local index="$CODEX_HOME/session_index.jsonl"
  [[ -f "$index" ]] || { echo 0; return; }
  grep -cve '^[[:space:]]*$' "$index" 2>/dev/null || echo 0
}

selected_chats_in_session_index_count() {
  local selected="$PACKAGE_ROOT/selected_chats"
  local index="$CODEX_HOME/session_index.jsonl"
  [[ -d "$selected" && -f "$index" ]] || { echo 0; return; }
  local py
  if py="$(find_python3)"; then
    "$py" - "$selected" "$index" <<'PY'
import json
import sys
from pathlib import Path

selected = Path(sys.argv[1])
index = Path(sys.argv[2])

def selected_id(path):
    sid = ""
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
                if row.get("type") == "session_meta" or payload.get("type") == "session_meta":
                    sid = str(payload.get("id") or row.get("id") or sid)
                    if sid:
                        return sid
                if not sid:
                    sid = str(row.get("id") or payload.get("id") or "")
    except Exception:
        pass
    return sid or path.stem

index_ids = set()
try:
    with index.open("r", encoding="utf-8", errors="ignore") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                row = json.loads(line)
            except Exception:
                continue
            sid = str(row.get("id") or "")
            if sid:
                index_ids.add(sid)
except Exception:
    pass

count = 0
for path in selected.glob("*.jsonl"):
    if selected_id(path) in index_ids:
        count += 1
print(count)
PY
  else
    local found=0 chat first_id
    shopt -s nullglob
    for chat in "$selected"/*.jsonl; do
      first_id="$(grep -m 1 -Eo '"id"[[:space:]]*:[[:space:]]*"[^"]+"' "$chat" 2>/dev/null | head -n 1 | sed -E 's/.*"id"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/' || true)"
      if [[ -n "$first_id" ]] && grep -F "\"id\":\"$first_id\"" "$index" >/dev/null 2>&1; then
        found=$((found + 1))
      fi
    done
    shopt -u nullglob
    echo "$found"
  fi
}

selected_chats_in_sessions_count() {
  local selected="$PACKAGE_ROOT/selected_chats"
  [[ -d "$selected" && -d "$CODEX_HOME/sessions" ]] || { echo 0; return; }
  local found=0
  local chat base
  shopt -s nullglob
  for chat in "$selected"/*.jsonl; do
    base="$(basename "$chat")"
    if find "$CODEX_HOME/sessions" -type f -name "$base" 2>/dev/null | grep -q .; then
      found=$((found + 1))
      continue
    fi
    local first_id
    first_id="$(grep -m 1 -o '"id":"[^"]*"' "$chat" 2>/dev/null | head -n 1 | cut -d '"' -f 4 || true)"
    if [[ -n "$first_id" ]] && find "$CODEX_HOME/sessions" -type f -name "*$first_id*.jsonl" 2>/dev/null | grep -q .; then
      found=$((found + 1))
    fi
  done
  shopt -u nullglob
  echo "$found"
}

project_paths_lines() {
  local map="$PACKAGE_ROOT/metadata/path_map.json"
  local py
  if [[ -f "$map" ]] && py="$(find_python3)"; then
    "$py" - "$map" "$PROJECTS_DIR" <<'PY'
import json
import sys
from pathlib import Path

path_map = Path(sys.argv[1])
projects_dir = Path(sys.argv[2]).expanduser()
try:
    data = json.loads(path_map.read_text(encoding="utf-8", errors="ignore"))
except Exception:
    data = {}
seen = set()
for entry in data.get("projects", []) or []:
    name = entry.get("package_project_name") or Path(entry.get("source_path", "")).name
    if not name:
        continue
    target = str(projects_dir / name)
    if target not in seen:
        print(target)
        seen.add(target)
PY
    return
  fi
  [[ -d "$PROJECTS_DIR" ]] || return
  find "$PROJECTS_DIR" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort
}

restored_project_count() {
  local map="$PACKAGE_ROOT/metadata/path_map.json"
  local py
  if [[ -f "$map" ]] && py="$(find_python3)"; then
    "$py" - "$map" "$PROJECTS_DIR" <<'PY'
import json
import sys
from pathlib import Path

path_map = Path(sys.argv[1])
projects_dir = Path(sys.argv[2]).expanduser()
try:
    data = json.loads(path_map.read_text(encoding="utf-8", errors="ignore"))
except Exception:
    data = {}
targets = []
for entry in data.get("projects", []) or []:
    name = entry.get("package_project_name") or Path(entry.get("source_path", "")).name
    if name:
        target = str(projects_dir / name)
        if target not in targets and Path(target).is_dir():
            targets.append(target)
print(len(targets))
PY
    return
  fi
  [[ -d "$PROJECTS_DIR" ]] || { echo 0; return; }
  find "$PROJECTS_DIR" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' '
}

app_registration_kv() {
  local report="$CODEX_HOME/codex-rehome-project-registration-report.json"
  if [[ ! -f "$report" ]]; then
    cat <<'EOF'
APP_REGISTRATION_STATUS=missing
APP_REGISTRATION_COUNT=0
APP_REGISTRATION_METHOD=
APP_REGISTRATION_MESSAGE='No codex app project registration report found'
EOF
    return
  fi
  local py
  if ! py="$(find_python3)"; then
    cat <<'EOF'
APP_REGISTRATION_STATUS=unknown
APP_REGISTRATION_COUNT=0
APP_REGISTRATION_METHOD=
APP_REGISTRATION_MESSAGE='Python unavailable; cannot parse project registration report'
EOF
    return
  fi
  "$py" - "$report" <<'PY'
import json
import shlex
import sys
from pathlib import Path

path = Path(sys.argv[1])
try:
    data = json.loads(path.read_text(encoding="utf-8", errors="ignore"))
except Exception:
    data = {}
status = str(data.get("status") or "unknown")
method = str(data.get("method") or "")
message = str(data.get("message") or "")
paths = data.get("registered_project_paths") or []
if not isinstance(paths, list):
    paths = []
print(f"APP_REGISTRATION_STATUS={shlex.quote(status)}")
print(f"APP_REGISTRATION_COUNT={len([p for p in paths if p])}")
print(f"APP_REGISTRATION_METHOD={shlex.quote(method)}")
print(f"APP_REGISTRATION_MESSAGE={shlex.quote(message)}")
PY
}

ui_ready_kv() {
  local py
  if ! py="$(find_python3)"; then
    cat <<'EOF'
SELECTED_CHATS_IN_STATE_THREADS=0
SELECTED_CHATS_WITH_EXISTING_ROLLOUT_PATH=0
SELECTED_CHATS_WITH_TARGET_CWD=0
SELECTED_CHATS_WITH_SESSION_META_TARGET_CWD=0
SELECTED_CHATS_WITHOUT_SOURCE_PATH_IN_JSONL=0
SELECTED_CHATS_WITH_FRESH_UPDATED_AT=0
RESTORED_PROJECTS_IN_GLOBAL_STATE=0
RESTORED_PROJECTS_IN_THREAD_HINTS=0
EOF
    return
  fi
  "$py" - "$PACKAGE_ROOT" "$CODEX_HOME" "$PROJECTS_DIR" <<'PY'
import json
import re
import sqlite3
import sys
from pathlib import Path

package = Path(sys.argv[1])
codex_home = Path(sys.argv[2])
projects_dir = Path(sys.argv[3]).expanduser()
metadata = package / "metadata"

def read_json(path, default):
    try:
        if path.exists():
            return json.loads(path.read_text(encoding="utf-8", errors="ignore"))
    except Exception:
        pass
    return default

path_map = read_json(metadata / "path_map.json", {"projects": []})
thread_export = read_json(metadata / "thread_index_export.json", {"selected_thread_ids": [], "threads": []})
selected_meta = read_json(metadata / "selected_chats.json", {"selected_chats": []})

def selected_id(path):
    sid = ""
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
                if row.get("type") == "session_meta" or payload.get("type") == "session_meta":
                    sid = str(payload.get("id") or row.get("id") or sid)
                    if sid:
                        return sid
                if not sid:
                    sid = str(row.get("id") or payload.get("id") or "")
    except Exception:
        pass
    if sid:
        return sid
    match = re.search(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}", path.stem, re.I)
    return match.group(0) if match else path.stem

selected_ids = []
for item in selected_meta.get("selected_chats", []) or []:
    tid = item.get("id")
    if tid and tid not in selected_ids:
        selected_ids.append(str(tid))
for tid in thread_export.get("selected_thread_ids", []) or []:
    if tid and tid not in selected_ids:
        selected_ids.append(str(tid))
selected_dir = package / "selected_chats"
if selected_dir.exists():
    for path in selected_dir.glob("*.jsonl"):
        tid = selected_id(path)
        if tid and tid not in selected_ids:
            selected_ids.append(tid)
selected_ids = list(dict.fromkeys(selected_ids))

target_projects = []
source_variants = []
for entry in path_map.get("projects", []) or []:
    name = entry.get("package_project_name") or Path(entry.get("source_path", "")).name
    target = str(projects_dir / name)
    if target not in target_projects:
        target_projects.append(target)
    for old in entry.get("source_path_variants", []) or []:
        if old:
            source_variants.append(old)
    src = entry.get("source_path")
    if src:
        source_variants.extend([src, "\\\\?\\" + src if not src.startswith("\\\\?\\") else src, src.replace("\\", "/"), "/" + src])
source_variants = list(dict.fromkeys(source_variants))

def find_session(tid):
    root = codex_home / "sessions"
    if not root.exists():
        return None
    matches = list(root.rglob(f"*{tid}*.jsonl"))
    return matches[0] if matches else None

def session_meta_cwd(path):
    text = ""
    try:
        with path.open("r", encoding="utf-8", errors="ignore") as f:
            for line in f:
                text += line
                line = line.strip()
                if not line:
                    continue
                try:
                    row = json.loads(line)
                except Exception:
                    continue
                payload = row.get("payload") if isinstance(row.get("payload"), dict) else {}
                if row.get("type") == "session_meta" or payload.get("type") == "session_meta":
                    return str(payload.get("cwd") or "")
    except Exception:
        text = path.read_text(encoding="utf-8", errors="ignore") if path.exists() else ""
    for target in target_projects:
        if target and target in text:
            return target
    return ""

dbs = sorted(codex_home.glob("state_*.sqlite"), key=lambda p: p.stat().st_mtime if p.exists() else 0, reverse=True)
rows = {}
if dbs:
    try:
        con = sqlite3.connect(str(dbs[0]))
        con.row_factory = sqlite3.Row
        for tid in selected_ids:
            row = con.execute("select * from threads where id=?", (tid,)).fetchone()
            if row:
                rows[tid] = dict(row)
        con.close()
    except Exception:
        rows = {}

state_count = 0
rollout_count = 0
target_cwd_count = 0
meta_cwd_count = 0
no_source_count = 0
fresh_count = 0

for tid in selected_ids:
    row = rows.get(tid)
    if row:
        state_count += 1
        rollout = row.get("rollout_path") or ""
        if rollout and Path(str(rollout)).exists():
            rollout_count += 1
        cwd = str(row.get("cwd") or "")
        if cwd and (not target_projects or cwd in target_projects):
            target_cwd_count += 1
        updated = row.get("updated_at_ms") or row.get("updated_at") or 0
        try:
            if int(updated) > 1577836800:
                fresh_count += 1
        except Exception:
            pass
    session = find_session(tid)
    if session:
        cwd = session_meta_cwd(session)
        if cwd and (not target_projects or cwd in target_projects):
            meta_cwd_count += 1
        text = session.read_text(encoding="utf-8", errors="ignore")
        if not any(old and (old in text or old.replace("\\", "\\\\") in text) for old in source_variants):
            no_source_count += 1

global_state = read_json(codex_home / ".codex-global-state.json", {})
global_count = 0
hint_count = 0
for target in target_projects:
    if target and all(target in (global_state.get(key) or []) for key in ["electron-saved-workspace-roots", "project-order", "active-workspace-roots"]):
        global_count += 1
hints = global_state.get("thread-workspace-root-hints") or {}
for tid in selected_ids:
    if hints.get(tid) in target_projects:
        hint_count += 1

print(f"SELECTED_CHATS_IN_STATE_THREADS={state_count}")
print(f"SELECTED_CHATS_WITH_EXISTING_ROLLOUT_PATH={rollout_count}")
print(f"SELECTED_CHATS_WITH_TARGET_CWD={target_cwd_count}")
print(f"SELECTED_CHATS_WITH_SESSION_META_TARGET_CWD={meta_cwd_count}")
print(f"SELECTED_CHATS_WITHOUT_SOURCE_PATH_IN_JSONL={no_source_count}")
print(f"SELECTED_CHATS_WITH_FRESH_UPDATED_AT={fresh_count}")
print(f"RESTORED_PROJECTS_IN_GLOBAL_STATE={global_count}")
print(f"RESTORED_PROJECTS_IN_THREAD_HINTS={hint_count}")
PY
}

CHECKSUM_STATUS="$(checksum_status)"
SESSIONS="$(count_files "$CODEX_HOME/sessions" "*.jsonl")"
ARCHIVED_SESSIONS="$(count_files "$CODEX_HOME/archived_sessions" "*.jsonl")"
SKILLS="$(count_files "$CODEX_HOME/skills" "SKILL.md")"
PLUGIN_MANIFESTS="$(count_files "$CODEX_HOME/plugins/cache" "plugin.json")"
GENERATED_IMAGES="$(count_files "$CODEX_HOME/generated_images" "*")"
SQLITE_FILES="$(count_files "$CODEX_HOME" "*.sqlite")"
SESSION_INDEX_ENTRIES="$(session_index_entries_count)"
RESTORED_PROJECT_COUNT="$(restored_project_count)"
SELECTED_CHATS="$(selected_chats_count)"
SELECTED_CHATS_IN_SESSIONS="$(selected_chats_in_sessions_count)"
SELECTED_CHATS_IN_SESSION_INDEX="$(selected_chats_in_session_index_count)"
eval "$(ui_ready_kv)"
eval "$(app_registration_kv)"
FORBIDDEN_CODEX="$(forbidden_count_in_codex_home)"
FORBIDDEN_PROJECTS="$(forbidden_count_in_root "$PROJECTS_DIR")"
FORBIDDEN_TOTAL="$((FORBIDDEN_CODEX + FORBIDDEN_PROJECTS))"
if [[ "$SELECTED_CHATS" -eq 0 || "$SELECTED_CHATS_IN_SESSION_INDEX" -eq "$SELECTED_CHATS" ]]; then
  UI_LEFT_SIDEBAR_READY="true"
else
  UI_LEFT_SIDEBAR_READY="false"
fi
if [[ "$SELECTED_CHATS" -eq 0 || "$SELECTED_CHATS_IN_STATE_THREADS" -eq "$SELECTED_CHATS" ]]; then STATE_THREADS_READY="true"; else STATE_THREADS_READY="false"; fi
if [[ "$SELECTED_CHATS" -eq 0 || "$SELECTED_CHATS_WITH_EXISTING_ROLLOUT_PATH" -eq "$SELECTED_CHATS" ]]; then ROLLOUT_PATHS_READY="true"; else ROLLOUT_PATHS_READY="false"; fi
if [[ "$SELECTED_CHATS" -eq 0 || "$SELECTED_CHATS_WITH_TARGET_CWD" -eq "$SELECTED_CHATS" ]]; then PROJECT_PATH_MAPPING_READY="true"; else PROJECT_PATH_MAPPING_READY="false"; fi
if [[ "$SELECTED_CHATS" -eq 0 || "$SELECTED_CHATS_WITH_SESSION_META_TARGET_CWD" -eq "$SELECTED_CHATS" ]]; then SESSION_JSONL_PATH_MAPPING_READY="true"; else SESSION_JSONL_PATH_MAPPING_READY="false"; fi
if [[ "$SELECTED_CHATS" -eq 0 || "$SELECTED_CHATS_WITHOUT_SOURCE_PATH_IN_JSONL" -eq "$SELECTED_CHATS" ]]; then SOURCE_PATH_REMOVED_READY="true"; else SOURCE_PATH_REMOVED_READY="false"; fi
if [[ "$RESTORED_PROJECT_COUNT" -eq 0 || "$RESTORED_PROJECTS_IN_GLOBAL_STATE" -eq "$RESTORED_PROJECT_COUNT" ]]; then GLOBAL_PROJECT_REGISTRY_READY="true"; else GLOBAL_PROJECT_REGISTRY_READY="false"; fi
if [[ "$RESTORED_PROJECT_COUNT" -eq 0 || ( "$APP_REGISTRATION_STATUS" == "invoked" && "$APP_REGISTRATION_COUNT" -ge "$RESTORED_PROJECT_COUNT" ) ]]; then APP_PROJECT_REGISTRATION_READY="true"; else APP_PROJECT_REGISTRATION_READY="false"; fi

if [[ "$JSON" == "true" ]]; then
  PROJECT_PATHS_JSON="$(project_paths_lines | json_string_array_from_lines)"
  cat <<EOF
{
  "generated_at": "$(date +%Y-%m-%dT%H:%M:%S)",
  "package_root": "$(json_escape "$PACKAGE_ROOT")",
  "mac_user": "$(json_escape "$MAC_USER")",
  "checksum": {
    "sha256sums_txt": "$(json_escape "$CHECKSUM_STATUS")"
  },
  "paths": {
    "codex_home": {"path": "$(json_escape "$CODEX_HOME")", "exists": $(test -e "$CODEX_HOME" && echo true || echo false), "size_mb": $(size_mb "$CODEX_HOME")},
    "app_support_codex": {"path": "$(json_escape "$APP_SUPPORT")", "exists": $(test -e "$APP_SUPPORT" && echo true || echo false), "size_mb": $(size_mb "$APP_SUPPORT")},
    "projects_dir": {"path": "$(json_escape "$PROJECTS_DIR")", "exists": $(test -e "$PROJECTS_DIR" && echo true || echo false)}
  },
  "counts": {
    "sessions": $SESSIONS,
    "archived_sessions": $ARCHIVED_SESSIONS,
    "skills": $SKILLS,
    "plugin_manifests": $PLUGIN_MANIFESTS,
    "generated_images": $GENERATED_IMAGES,
    "sqlite_files": $SQLITE_FILES,
    "session_index_entries": $SESSION_INDEX_ENTRIES,
    "restored_project_count": $RESTORED_PROJECT_COUNT,
    "selected_chats": $SELECTED_CHATS,
    "selected_chats_in_restored_sessions": $SELECTED_CHATS_IN_SESSIONS,
    "selected_chats_in_session_index": $SELECTED_CHATS_IN_SESSION_INDEX,
    "selected_chats_in_state_threads": $SELECTED_CHATS_IN_STATE_THREADS,
    "selected_chats_with_existing_rollout_path": $SELECTED_CHATS_WITH_EXISTING_ROLLOUT_PATH,
    "selected_chats_with_target_cwd": $SELECTED_CHATS_WITH_TARGET_CWD,
    "selected_chats_with_session_meta_target_cwd": $SELECTED_CHATS_WITH_SESSION_META_TARGET_CWD,
    "selected_chats_without_source_path_in_jsonl": $SELECTED_CHATS_WITHOUT_SOURCE_PATH_IN_JSONL,
    "selected_chats_with_fresh_updated_at": $SELECTED_CHATS_WITH_FRESH_UPDATED_AT,
    "restored_projects_in_global_state": $RESTORED_PROJECTS_IN_GLOBAL_STATE,
    "restored_projects_in_thread_hints": $RESTORED_PROJECTS_IN_THREAD_HINTS
  },
  "restored_project_paths": $PROJECT_PATHS_JSON,
  "ui_readiness": {
    "selected_chats_in_sessions_ready": $(test "$SELECTED_CHATS_IN_SESSIONS" -eq "$SELECTED_CHATS" && echo true || echo false),
    "selected_chats_in_session_index_ready": $UI_LEFT_SIDEBAR_READY,
    "state_threads_ready": $STATE_THREADS_READY,
    "rollout_paths_ready": $ROLLOUT_PATHS_READY,
    "project_path_mapping_ready": $PROJECT_PATH_MAPPING_READY,
    "session_jsonl_path_mapping_ready": $SESSION_JSONL_PATH_MAPPING_READY,
    "source_path_removed_ready": $SOURCE_PATH_REMOVED_READY,
    "global_project_registry_ready": $GLOBAL_PROJECT_REGISTRY_READY,
    "app_project_registration_ready": $APP_PROJECT_REGISTRATION_READY,
    "app_restart_required": false
  },
  "project_ui_registration": {
    "status": "$(json_escape "$APP_REGISTRATION_STATUS")",
    "method": "$(json_escape "$APP_REGISTRATION_METHOD")",
    "registered_project_count": $APP_REGISTRATION_COUNT,
    "message": "$(json_escape "$APP_REGISTRATION_MESSAGE")"
  },
  "forbidden_files": {
    "codex_home": $FORBIDDEN_CODEX,
    "projects": $FORBIDDEN_PROJECTS,
    "total": $FORBIDDEN_TOTAL
  }
}
EOF
  exit 0
fi

echo "Codex Mac restore verification"
echo "Generated: $(date +%Y-%m-%dT%H:%M:%S)"
echo "Package root: $PACKAGE_ROOT"
echo "Checksum: $CHECKSUM_STATUS"
echo
for path in \
  "$CODEX_HOME" \
  "$APP_SUPPORT" \
  "$HOME/Library/Application Support/com.openai.codex" \
  "$HOME/Library/Application Support/OpenAI/Codex" \
  "$HOME/Library/Caches/Codex" \
  "$PROJECTS_DIR"; do
  if [[ -e "$path" ]]; then
    echo "  [found]   $path ($(size_mb "$path") MB)"
  else
    echo "  [missing] $path"
  fi
done
echo
echo "Counts:"
echo "  sessions: $SESSIONS"
echo "  archived_sessions: $ARCHIVED_SESSIONS"
echo "  skills: $SKILLS"
echo "  plugin_manifests: $PLUGIN_MANIFESTS"
echo "  generated_images: $GENERATED_IMAGES"
echo "  sqlite_files: $SQLITE_FILES"
echo "  session_index_entries: $SESSION_INDEX_ENTRIES"
echo "  restored_project_count: $RESTORED_PROJECT_COUNT"
echo "  selected_chats: $SELECTED_CHATS"
echo "  selected_chats_in_restored_sessions: $SELECTED_CHATS_IN_SESSIONS"
echo "  selected_chats_in_session_index: $SELECTED_CHATS_IN_SESSION_INDEX"
echo "  selected_chats_in_state_threads: $SELECTED_CHATS_IN_STATE_THREADS"
echo "  selected_chats_with_existing_rollout_path: $SELECTED_CHATS_WITH_EXISTING_ROLLOUT_PATH"
echo "  selected_chats_with_target_cwd: $SELECTED_CHATS_WITH_TARGET_CWD"
echo "  selected_chats_with_session_meta_target_cwd: $SELECTED_CHATS_WITH_SESSION_META_TARGET_CWD"
echo "  selected_chats_without_source_path_in_jsonl: $SELECTED_CHATS_WITHOUT_SOURCE_PATH_IN_JSONL"
echo "  selected_chats_with_fresh_updated_at: $SELECTED_CHATS_WITH_FRESH_UPDATED_AT"
echo "  restored_projects_in_global_state: $RESTORED_PROJECTS_IN_GLOBAL_STATE"
echo "  restored_projects_in_thread_hints: $RESTORED_PROJECTS_IN_THREAD_HINTS"
echo "  app_registration_status: $APP_REGISTRATION_STATUS"
echo "  app_registration_count: $APP_REGISTRATION_COUNT"
echo "  forbidden_files_total: $FORBIDDEN_TOTAL"
echo "  selected_chats_in_session_index_ready: $UI_LEFT_SIDEBAR_READY"
echo "  state_threads_ready: $STATE_THREADS_READY"
echo "  rollout_paths_ready: $ROLLOUT_PATHS_READY"
echo "  global_project_registry_ready: $GLOBAL_PROJECT_REGISTRY_READY"
echo "  app_project_registration_ready: $APP_PROJECT_REGISTRATION_READY"
echo
echo "Restored project paths:"
project_paths_lines | sed 's/^/  /' || true
echo "Project UI registration: $APP_REGISTRATION_STATUS via $APP_REGISTRATION_METHOD"
echo
echo "Next checks:"
echo "  1. Open Codex and confirm old threads are visible."
echo "  2. If app_project_registration_ready is false, run: /Applications/Codex.app/Contents/Resources/codex app <restored-project-path>"
echo "  3. Reconnect GitHub, Gmail, Chrome, Feishu, or other services if prompted."
