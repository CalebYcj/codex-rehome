#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ "$(basename "$ROOT")" == "scripts" ]]; then
  ROOT="$(cd "$ROOT/.." && pwd)"
fi

STAMP="$(date +%Y%m%d-%H%M%S)"

backup_if_exists() {
  local path="$1"
  if [[ -e "$path" ]]; then
    local backup="$path.backup-$STAMP"
    echo "Backing up existing data:"
    echo "  $path"
    echo "  -> $backup"
    mv "$path" "$backup"
  fi
}

restore_dir() {
  local src="$1"
  local dst="$2"
  if [[ ! -d "$src" ]]; then
    echo "Skipping missing source: $src"
    return
  fi
  mkdir -p "$(dirname "$dst")"
  backup_if_exists "$dst"
  if command -v rsync >/dev/null 2>&1; then
    rsync -aE "$src/" "$dst/"
  elif command -v ditto >/dev/null 2>&1; then
    ditto "$src" "$dst"
  else
    cp -Rp "$src" "$dst"
  fi
  echo "Restored: $dst"
}

restore_file() {
  local src="$1"
  local dst="$2"
  if [[ ! -f "$src" ]]; then
    return
  fi
  mkdir -p "$(dirname "$dst")"
  backup_if_exists "$dst"
  cp -p "$src" "$dst"
  echo "Restored: $dst"
}

if pgrep -if "Codex" >/dev/null 2>&1; then
  echo "Codex appears to be running. Close Codex before continuing."
  read -r -p "Press Enter after Codex is closed"
fi

echo "Restoring Codex data to Mac user: $USER"

restore_dir "$ROOT/home/.codex" "$HOME/.codex"
restore_dir "$ROOT/appdata_roaming/Codex" "$HOME/Library/Application Support/Codex"
restore_dir "$ROOT/appdata_roaming/com.openai.codex" "$HOME/Library/Application Support/com.openai.codex"
restore_dir "$ROOT/appdata_roaming/OpenAI/Codex" "$HOME/Library/Application Support/OpenAI/Codex"
restore_dir "$ROOT/appdata_local/Codex" "$HOME/Library/Caches/Codex"
restore_dir "$ROOT/appdata_local/com.openai.codex" "$HOME/Library/Caches/com.openai.codex"
restore_dir "$ROOT/appdata_local/com.openai.sky.CUAService" "$HOME/Library/Caches/com.openai.sky.CUAService"
restore_dir "$ROOT/appdata_local/com.openai.sky.CUAService.cli" "$HOME/Library/Caches/com.openai.sky.CUAService.cli"

restore_file "$ROOT/mac_only/Library/Preferences/com.openai.codex.plist" "$HOME/Library/Preferences/com.openai.codex.plist"
restore_file "$ROOT/mac_only/Library/Preferences/com.openai.sky.CUAService.plist" "$HOME/Library/Preferences/com.openai.sky.CUAService.plist"
restore_file "$ROOT/mac_only/Library/Preferences/com.openai.sky.CUAService.cli.plist" "$HOME/Library/Preferences/com.openai.sky.CUAService.cli.plist"

rm -f "$HOME/Library/Application Support/Codex/SingletonLock" \
  "$HOME/Library/Application Support/Codex/SingletonCookie" \
  "$HOME/Library/Application Support/Codex/SingletonSocket"

echo "Done. Open Codex and log in again if prompted."
