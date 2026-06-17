#!/usr/bin/env bash
set -euo pipefail

size_mb() {
  local path="$1"
  [[ -e "$path" ]] || { echo ""; return; }
  du -sk "$path" 2>/dev/null | awk '{printf " (%.2f MB)", $1 / 1024}'
}

echo "Codex inventory for Mac"
echo "User: $USER"
echo

for path in \
  "$HOME/.codex" \
  "$HOME/Library/Application Support/Codex" \
  "$HOME/Library/Application Support/com.openai.codex" \
  "$HOME/Library/Application Support/OpenAI/Codex" \
  "$HOME/Library/Caches/Codex" \
  "$HOME/Library/Caches/com.openai.codex" \
  "$HOME/Library/Logs/com.openai.codex"; do
  if [[ -e "$path" ]]; then
    echo "[found]   $path$(size_mb "$path")"
  else
    echo "[missing] $path"
  fi
done

echo
echo "Likely project folders:"
for root in "$HOME/Documents" "$HOME/Desktop" "$HOME/Developer" "$HOME/Projects"; do
  [[ -d "$root" ]] || continue
  find "$root" -maxdepth 2 -type d \( -name .git -o -name .agents -o -name outputs -o -name artifacts \) 2>/dev/null |
    sed 's#/\(\.git\|\.agents\|outputs\|artifacts\)$##' |
    sort -u |
    head -n 30 |
    sed 's#^#  #'
done
