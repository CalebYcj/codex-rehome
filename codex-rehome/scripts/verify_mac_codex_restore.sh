#!/usr/bin/env bash
set -euo pipefail

PACKAGE_ROOT=""
JSON="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --package-root)
      PACKAGE_ROOT="$2"
      shift 2
      ;;
    --json)
      JSON="true"
      shift
      ;;
    -h|--help)
      echo "Usage: verify_mac_codex_restore.sh [--package-root DIR] [--json]"
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

count_files() {
  local path="$1"
  local name="${2:-*}"
  [[ -d "$path" ]] || { echo 0; return; }
  find "$path" -name "$name" -type f 2>/dev/null | wc -l | tr -d ' '
}

size_mb() {
  local path="$1"
  [[ -e "$path" ]] || { echo "null"; return; }
  du -sk "$path" 2>/dev/null | awk '{printf "%.2f", $1 / 1024}'
}

CODEX_HOME="$HOME/.codex"
APP_SUPPORT="$HOME/Library/Application Support/Codex"

if [[ "$JSON" == "true" ]]; then
  cat <<EOF
{
  "generated_at": "$(date +%Y-%m-%dT%H:%M:%S)",
  "package_root": "$PACKAGE_ROOT",
  "mac_user": "$USER",
  "paths": {
    "codex_home": {"path": "$CODEX_HOME", "exists": $(test -e "$CODEX_HOME" && echo true || echo false), "size_mb": $(size_mb "$CODEX_HOME")},
    "app_support_codex": {"path": "$APP_SUPPORT", "exists": $(test -e "$APP_SUPPORT" && echo true || echo false), "size_mb": $(size_mb "$APP_SUPPORT")}
  },
  "counts": {
    "sessions": $(count_files "$CODEX_HOME/sessions" "*.jsonl"),
    "archived_sessions": $(count_files "$CODEX_HOME/archived_sessions" "*.jsonl"),
    "skills": $(count_files "$CODEX_HOME/skills" "SKILL.md"),
    "plugin_manifests": $(count_files "$CODEX_HOME/plugins/cache" "plugin.json"),
    "generated_images": $(count_files "$CODEX_HOME/generated_images" "*"),
    "sqlite_files": $(count_files "$CODEX_HOME" "*.sqlite")
  }
}
EOF
  exit 0
fi

echo "Codex Mac restore verification"
echo "Generated: $(date +%Y-%m-%dT%H:%M:%S)"
echo "Package root: $PACKAGE_ROOT"
echo
for path in \
  "$CODEX_HOME" \
  "$APP_SUPPORT" \
  "$HOME/Library/Application Support/com.openai.codex" \
  "$HOME/Library/Application Support/OpenAI/Codex" \
  "$HOME/Library/Caches/Codex"; do
  if [[ -e "$path" ]]; then
    echo "  [found]   $path ($(size_mb "$path") MB)"
  else
    echo "  [missing] $path"
  fi
done
echo
echo "Counts:"
echo "  sessions: $(count_files "$CODEX_HOME/sessions" "*.jsonl")"
echo "  archived_sessions: $(count_files "$CODEX_HOME/archived_sessions" "*.jsonl")"
echo "  skills: $(count_files "$CODEX_HOME/skills" "SKILL.md")"
echo "  plugin_manifests: $(count_files "$CODEX_HOME/plugins/cache" "plugin.json")"
echo "  generated_images: $(count_files "$CODEX_HOME/generated_images" "*")"
echo "  sqlite_files: $(count_files "$CODEX_HOME" "*.sqlite")"
echo
echo "Next checks:"
echo "  1. Open Codex and confirm old threads are visible."
echo "  2. Reopen migrated project folders from their Mac paths."
echo "  3. Reconnect GitHub, Gmail, Chrome, Feishu, or other services if prompted."
