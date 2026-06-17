# Codex Migration Path Map

Use the neutral package layout as the bridge between Mac and Windows:

- `home/.codex`
- `appdata_roaming/Codex`
- `appdata_roaming/com.openai.codex`
- `appdata_roaming/OpenAI/Codex`
- `appdata_local/...`
- `mac_only/Library/Preferences`
- `projects/...`

This lets the same package restore to Mac or Windows.

## Mac Paths

- `~/.codex`: primary Codex state, conversations, logs, sessions, memories, skills, plugins, generated images, automations, config, auth files.
  - Common confirmed files/folders include `sessions`, `archived_sessions`, `session_index.jsonl`, `state_*.sqlite`, `memories_*.sqlite`, `goals_*.sqlite`, `logs_*.sqlite`, `generated_images`, `skills`, and `plugins/cache`.
- `~/Library/Application Support/Codex`: desktop app Chromium/profile data.
- `~/Library/Application Support/com.openai.codex`: app support data.
- `~/Library/Application Support/OpenAI/Codex`: OpenAI/Codex app support data.
- `~/Library/Caches/...`: optional caches; useful but not required.
- `~/Library/Logs/com.openai.codex`: optional logs; useful for debugging, sensitive, and potentially large.
- Chrome native host manifests such as `com.openai.codexextension.json`: useful for inventory, not directly restored cross-platform.
- `~/Library/Preferences/*.plist`: Mac-only preferences; archive for completeness but do not restore directly to Windows.

## Default Exclusions

Exclude runtime/cache/dev files by default because real Mac copies can fail on sockets and unreadable cache objects:

- `.tmp/`, `tmp/`, `process_manager/`, `vendor_imports/`
- `.git/`, `node_modules/`, `.venv/`, `venv/`, `__pycache__/`
- `*.ipc`, `*.sock`, `SingletonLock`, `SingletonCookie`, `SingletonSocket`, `RunningChromeVersion`

Exclude sensitive files unless the user explicitly chooses `full-with-secrets`:

- `~/.codex/auth.json`
- browser `Cookies`, `Login Data`, `Local Storage`, `Session Storage`
- `.env`, `.env.*`, private keys, `*.pem`, `*.key`

## Windows Paths

- `%USERPROFILE%\.codex`: primary Codex state.
- `%APPDATA%\Codex`: desktop app roaming data.
- `%APPDATA%\com.openai.codex`: app support data.
- `%APPDATA%\OpenAI\Codex`: OpenAI/Codex app support data.
- `%LOCALAPPDATA%\...`: optional cache equivalents.

## Neutral Package Mapping

| Neutral package path | Mac restore path | Windows restore path |
|---|---|---|
| `home/.codex` | `~/.codex` | `%USERPROFILE%\.codex` |
| `appdata_roaming/Codex` | `~/Library/Application Support/Codex` | `%APPDATA%\Codex` |
| `appdata_roaming/com.openai.codex` | `~/Library/Application Support/com.openai.codex` | `%APPDATA%\com.openai.codex` |
| `appdata_roaming/OpenAI/Codex` | `~/Library/Application Support/OpenAI/Codex` | `%APPDATA%\OpenAI\Codex` |
| `appdata_local/Codex` | `~/Library/Caches/Codex` | `%LOCALAPPDATA%\Codex` |
| `appdata_local/com.openai.codex` | `~/Library/Caches/com.openai.codex` | `%LOCALAPPDATA%\com.openai.codex` |
| `projects/<name>` | user-chosen project folder | user-chosen project folder |

## Project Continuity

Codex conversations may reference absolute source paths. Copy project folders separately or include them in the package under `projects/`. Reopen the project folder in Codex from its new target path, for example:

- Mac: `/Users/caleb/Documents/New project`
- Windows: `C:\Users\Administrator\Documents\New project`

Do not edit JSONL session files in place to rewrite paths. Record path mappings in the manifest and reopen the matching project folder on the target computer so newer threads resolve to the target location.
