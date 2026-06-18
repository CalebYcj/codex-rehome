#!/usr/bin/env bash
set -euo pipefail

OUT_DIR="$HOME/Desktop"
PROJECTS=()
MODE="standard"
ALLOW_SECRETS="false"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      MODE="$2"
      shift 2
      ;;
    --out)
      OUT_DIR="$2"
      shift 2
      ;;
    --project)
      PROJECTS+=("$2")
      shift 2
      ;;
    --i-understand-secrets)
      ALLOW_SECRETS="true"
      shift
      ;;
    -h|--help)
      cat <<'EOF'
Usage:
  create_mac_codex_migration_package.sh [options]

Options:
  --mode standard|full|full-with-secrets
      standard: Codex core data, skills, plugins, generated images, and selected app data.
      full:     standard plus logs/caches/environment inventory, still excluding secrets.
      full-with-secrets:
                includes sensitive auth/token/env/login-state files. Requires
                --i-understand-secrets.

  --out DIR
      Output directory. Defaults to ~/Desktop.

  --project PATH
      Include a project folder. May be repeated.

  --i-understand-secrets
      Required with --mode full-with-secrets.
EOF
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

case "$MODE" in
  standard|full|full-with-secrets) ;;
  *)
    echo "Invalid --mode: $MODE" >&2
    exit 1
    ;;
esac

if [[ "$MODE" == "full-with-secrets" && "$ALLOW_SECRETS" != "true" ]]; then
  echo "Refusing full-with-secrets without --i-understand-secrets." >&2
  echo "This mode may package auth tokens, .env files, browser login state, and private keys." >&2
  exit 1
fi

STAMP="$(date +%Y%m%d-%H%M%S)"
STAGE="$OUT_DIR/Codex-Migration-Mac-Source-$STAMP"
ZIP_PATH="$OUT_DIR/Codex-Migration-Mac-Source-$STAMP.zip"
EXCLUDE_FILE="$STAGE/docs/rsync-excludes.txt"
SENSITIVE_REPORT="$STAGE/docs/SENSITIVE-FILES.txt"
ENV_REPORT="$STAGE/docs/ENV-INVENTORY.txt"

mkdir -p "$STAGE/home" \
  "$STAGE/appdata_roaming/OpenAI" \
  "$STAGE/appdata_local" \
  "$STAGE/mac_only/Library/Preferences" \
  "$STAGE/projects" \
  "$STAGE/docs"

cat > "$EXCLUDE_FILE" <<'EOF'
.DS_Store
.tmp/
tmp/
process_manager/
vendor_imports/
.git/
node_modules/
.venv/
venv/
__pycache__/
*.ipc
*.sock
SingletonLock
SingletonCookie
SingletonSocket
RunningChromeVersion
EOF

if [[ "$MODE" != "full-with-secrets" ]]; then
  cat >> "$EXCLUDE_FILE" <<'EOF'
auth.json
Cookies
Cookies-journal
Login Data
Login Data For Account
Login Data-journal
Login Data For Account-journal
Local Storage/
Session Storage/
Network/Cookies
.env
.env.*
id_rsa
id_dsa
id_ecdsa
id_ed25519
*.pem
*.key
EOF
fi

if [[ "$MODE" == "standard" ]]; then
  cat >> "$EXCLUDE_FILE" <<'EOF'
logs_*.sqlite
logs_*.sqlite*
logs/
Library/Logs/
Cache/
Caches/
GPUCache/
Code Cache/
Service Worker/CacheStorage/
EOF
fi

copy_dir() {
  local src="$1"
  local dst="$2"
  if [[ -d "$src" ]]; then
    mkdir -p "$(dirname "$dst")"
    if command -v rsync >/dev/null 2>&1; then
      rsync -aE --delete --exclude-from="$EXCLUDE_FILE" "$src/" "$dst/"
    elif command -v ditto >/dev/null 2>&1; then
      echo "Warning: rsync not found; falling back to ditto without exclude support for $src" >&2
      ditto "$src" "$dst"
    else
      echo "Neither rsync nor ditto is available." >&2
      exit 1
    fi
  fi
}

copy_file() {
  local src="$1"
  local dst="$2"
  if [[ -f "$src" ]]; then
    mkdir -p "$(dirname "$dst")"
    cp -p "$src" "$dst"
  fi
}

if command -v sqlite3 >/dev/null 2>&1; then
  for db in "$HOME/.codex"/*.sqlite; do
    [[ -f "$db" ]] && sqlite3 "$db" 'PRAGMA wal_checkpoint(PASSIVE);' >/dev/null 2>&1 || true
  done
fi

copy_dir "$HOME/.codex" "$STAGE/home/.codex"
copy_dir "$HOME/Library/Application Support/Codex" "$STAGE/appdata_roaming/Codex"
copy_dir "$HOME/Library/Application Support/com.openai.codex" "$STAGE/appdata_roaming/com.openai.codex"
copy_dir "$HOME/Library/Application Support/OpenAI/Codex" "$STAGE/appdata_roaming/OpenAI/Codex"

if [[ "$MODE" != "standard" ]]; then
  copy_dir "$HOME/Library/Caches/Codex" "$STAGE/appdata_local/Codex"
  copy_dir "$HOME/Library/Caches/com.openai.codex" "$STAGE/appdata_local/com.openai.codex"
  copy_dir "$HOME/Library/Caches/com.openai.sky.CUAService" "$STAGE/appdata_local/com.openai.sky.CUAService"
  copy_dir "$HOME/Library/Caches/com.openai.sky.CUAService.cli" "$STAGE/appdata_local/com.openai.sky.CUAService.cli"
  copy_dir "$HOME/Library/Logs/com.openai.codex" "$STAGE/mac_only/Library/Logs/com.openai.codex"
fi

copy_file "$HOME/Library/Preferences/com.openai.codex.plist" "$STAGE/mac_only/Library/Preferences/com.openai.codex.plist"
copy_file "$HOME/Library/Preferences/com.openai.sky.CUAService.plist" "$STAGE/mac_only/Library/Preferences/com.openai.sky.CUAService.plist"
copy_file "$HOME/Library/Preferences/com.openai.sky.CUAService.cli.plist" "$STAGE/mac_only/Library/Preferences/com.openai.sky.CUAService.cli.plist"

rm -f "$STAGE/appdata_roaming/Codex/SingletonLock" \
  "$STAGE/appdata_roaming/Codex/SingletonCookie" \
  "$STAGE/appdata_roaming/Codex/SingletonSocket" \
  "$STAGE/appdata_roaming/Codex/RunningChromeVersion"

for project in "${PROJECTS[@]}"; do
  if [[ -d "$project" ]]; then
    base="$(basename "$project")"
    copy_dir "$project" "$STAGE/projects/$base"
  else
    echo "Missing project: $project" >&2
  fi
done

{
  echo "Sensitive files report"
  echo "Generated: $STAMP"
  echo "Mode: $MODE"
  echo
  echo "The following paths exist or matched common sensitive patterns on the source Mac."
  echo "Contents are intentionally not printed."
  echo
  for path in \
    "$HOME/.codex/auth.json" \
    "$HOME/Library/Application Support/Codex/Cookies" \
    "$HOME/Library/Application Support/Codex/Default/Login Data" \
    "$HOME/Library/Application Support/Codex/Local Storage" \
    "$HOME/Library/Application Support/Codex/Session Storage"; do
    [[ -e "$path" ]] && echo "$path"
  done
  for project in "${PROJECTS[@]}"; do
    [[ -d "$project" ]] && find "$project" -name ".env" -o -name ".env.*" -o -name "*.pem" -o -name "*.key" 2>/dev/null | sed 's#^#project: #'
  done
  if [[ -d "$HOME/.ssh" ]]; then
    find "$HOME/.ssh" -maxdepth 1 -type f 2>/dev/null | sed 's#^#ssh: #'
  fi
} > "$SENSITIVE_REPORT"

if [[ "$MODE" != "standard" ]]; then
  {
    echo "Environment inventory"
    echo "Generated: $STAMP"
    echo
    echo "[system]"
    sw_vers 2>/dev/null || true
    uname -a 2>/dev/null || true
    echo
    echo "[tools]"
    for cmd in codex git node npm pnpm yarn python3 pip3 uv cargo rustc go brew; do
      if command -v "$cmd" >/dev/null 2>&1; then
        printf "%s: " "$cmd"
        "$cmd" --version 2>/dev/null | head -n 1 || command -v "$cmd"
      fi
    done
    echo
    echo "[git]"
    git config --global --list 2>/dev/null | sed -E 's#(token|password|secret|key)=.*#\1=<redacted>#I' || true
    echo
    echo "[shell]"
    echo "SHELL=${SHELL:-}"
    echo "PATH=$PATH" | sed -E 's#(token|password|secret|key)=[^:; ]+#\1=<redacted>#Ig'
  } > "$ENV_REPORT"
fi

cat > "$STAGE/README-Restore.txt" <<'EOF'
Codex migration package
=======================

This package uses a neutral layout so it can be restored to either Windows or Mac.

Before restoring on any target:
1. Install Codex on the target computer.
2. Open it once on the target computer, then close all Codex windows.
3. Unzip this package.

Windows restore:
1. Open PowerShell in the unzipped folder.
2. Run:
   Set-ExecutionPolicy -Scope Process Bypass
   .\Restore-Codex-To-Windows.ps1
3. Verify:
   .\Verify-Codex-Windows-Restore.ps1

Mac restore:
1. Open Terminal in the unzipped folder.
2. Run:
   bash Restore-Codex-To-Mac.sh
3. Verify:
   bash Verify-Codex-Mac-Restore.sh

Manual mapping:
home\.codex -> C:\Users\<you>\.codex
appdata_roaming\Codex -> C:\Users\<you>\AppData\Roaming\Codex
appdata_roaming\com.openai.codex -> C:\Users\<you>\AppData\Roaming\com.openai.codex
appdata_roaming\OpenAI\Codex -> C:\Users\<you>\AppData\Roaming\OpenAI\Codex

home/.codex -> ~/.codex
appdata_roaming/Codex -> ~/Library/Application Support/Codex
appdata_roaming/com.openai.codex -> ~/Library/Application Support/com.openai.codex
appdata_roaming/OpenAI/Codex -> ~/Library/Application Support/OpenAI/Codex

Project folders, if included, are under projects\. Move them to your desired project location and reopen the folder in Codex.

If Codex asks you to log in again, log in normally.

Security note:
By default this package is expected to exclude browser login state, auth.json,
.env files, and private keys. If it was created with full-with-secrets, treat it
like a password vault and transfer it only through a private channel.
EOF

cp -p "$STAGE/README-Restore.txt" "$STAGE/README-Windows-Restore.txt"

if [[ -f "$SCRIPT_DIR/restore_codex_to_windows.ps1" ]]; then
  cp -p "$SCRIPT_DIR/restore_codex_to_windows.ps1" "$STAGE/Restore-Codex-To-Windows.ps1"
else
  echo "Missing restore_codex_to_windows.ps1" >&2
  exit 1
fi

if [[ -f "$SCRIPT_DIR/verify_windows_codex_restore.ps1" ]]; then
  cp -p "$SCRIPT_DIR/verify_windows_codex_restore.ps1" "$STAGE/Verify-Codex-Windows-Restore.ps1"
fi
if [[ -f "$SCRIPT_DIR/restore_codex_to_mac.sh" ]]; then
  cp -p "$SCRIPT_DIR/restore_codex_to_mac.sh" "$STAGE/Restore-Codex-To-Mac.sh"
fi
if [[ -f "$SCRIPT_DIR/verify_mac_codex_restore.sh" ]]; then
  cp -p "$SCRIPT_DIR/verify_mac_codex_restore.sh" "$STAGE/Verify-Codex-Mac-Restore.sh"
fi
if [[ -f "$SCRIPT_DIR/collect_mac_codex_inventory.sh" ]]; then
  cp -p "$SCRIPT_DIR/collect_mac_codex_inventory.sh" "$STAGE/Collect-Mac-Codex-Inventory.sh"
fi

{
  echo "created_at=$STAMP"
  echo "source_home=$HOME"
  echo "mode=$MODE"
  echo "package=$ZIP_PATH"
  echo "projects=${PROJECTS[*]:-}"
  echo
  echo "[source_paths]"
  for path in \
    "$HOME/.codex" \
    "$HOME/Library/Application Support/Codex" \
    "$HOME/Library/Application Support/com.openai.codex" \
    "$HOME/Library/Application Support/OpenAI/Codex" \
    "$HOME/Library/Caches/Codex" \
    "$HOME/Library/Logs/com.openai.codex"; do
    if [[ -e "$path" ]]; then
      printf "%s\t" "$path"
      du -sh "$path" 2>/dev/null | awk '{print $1}' || true
    fi
  done
  echo
  echo "[counts]"
  [[ -d "$HOME/.codex/sessions" ]] && echo "sessions=$(find "$HOME/.codex/sessions" -name '*.jsonl' 2>/dev/null | wc -l | tr -d ' ')"
  [[ -d "$HOME/.codex/archived_sessions" ]] && echo "archived_sessions=$(find "$HOME/.codex/archived_sessions" -name '*.jsonl' 2>/dev/null | wc -l | tr -d ' ')"
  [[ -d "$HOME/.codex/skills" ]] && echo "skills=$(find "$HOME/.codex/skills" -name 'SKILL.md' 2>/dev/null | wc -l | tr -d ' ')"
  [[ -d "$HOME/.codex/plugins/cache" ]] && echo "plugin_manifests=$(find "$HOME/.codex/plugins/cache" -name 'plugin.json' 2>/dev/null | wc -l | tr -d ' ')"
  [[ -d "$HOME/.codex/generated_images" ]] && echo "generated_images=$(find "$HOME/.codex/generated_images" -type f 2>/dev/null | wc -l | tr -d ' ')"
  echo
  echo "[sizes]"
  du -sh "$STAGE"/* 2>/dev/null || true
} > "$STAGE/MANIFEST.txt"

cat > "$STAGE/MANIFEST.json" <<EOF
{
  "created_at": "$STAMP",
  "source_home": "$HOME",
  "mode": "$MODE",
  "package": "$ZIP_PATH",
  "projects": "$(printf '%s ' "${PROJECTS[@]:-}" | sed 's/ *$//')",
  "notes": [
    "Path mappings are recorded rather than applied to JSONL sessions in place.",
    "Use docs/SENSITIVE-FILES.txt to review suspected sensitive files without exposing values.",
    "Run the restore script for the target OS only after closing Codex on that target."
  ]
}
EOF

(cd "$STAGE" && find . -type f -print0 | xargs -0 shasum -a 256 > SHA256SUMS.txt)
(cd "$OUT_DIR" && zip -qry "$(basename "$ZIP_PATH")" "$(basename "$STAGE")")

echo "Created: $ZIP_PATH"
du -sh "$ZIP_PATH" "$STAGE"
