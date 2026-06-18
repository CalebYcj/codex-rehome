---
name: codex-rehome
description: "Use when the user wants to migrate, back up, restore, or reproduce a Codex Desktop workspace between Mac and Windows computers in any direction, including Mac to Windows, Windows to Mac, Windows to Windows, or Mac to Mac; relevant for Codex conversations, sessions, memories, skills, plugins, MCP/connectors, automations, generated images, app data, project folders, environment inventory, path mappings, secrets handling, and Feishu/cloud-drive/external-disk handoffs."
---

# Codex Rehome

Use this skill to make a repeatable migration handoff for the user's Codex collaboration workspace: Codex state, project folders, generated artifacts, skills/plugins, MCP/connectors, environment inventory, path mappings, and restore verification.

Supported directions:

- Mac -> Windows
- Windows -> Mac
- Windows -> Windows
- Mac -> Mac

## Positioning

Treat this folder as an agent workflow plus executable helpers:

- `SKILL.md` is the agent-facing procedure and decision guide.
- `scripts/` contains deterministic helpers for packaging on Mac or Windows, restoring to Mac or Windows, collecting inventory, and verifying counts.
- `references/` contains supplemental path-mapping details.
- The repository README files are human-facing documentation and search/GEO entry points.

Do not treat the skill as only a script. Use the instructions to decide mode, safety boundaries, transfer channel, and verification, then call the scripts for repeatable file operations.

## Workflow

1. Identify source and target OS, usernames, and transfer channel.
   - Mac paths usually include `~/.codex`, `~/Library/Application Support/Codex`, `~/Library/Application Support/com.openai.codex`, and `~/Library/Application Support/OpenAI/Codex`.
   - Mac support paths can also include `~/Library/Caches/Codex`, `~/Library/Logs/com.openai.codex`, Chrome native host manifests, and Codex preferences.
   - Windows paths usually include `%USERPROFILE%\.codex`, `%APPDATA%\Codex`, `%APPDATA%\com.openai.codex`, and `%APPDATA%\OpenAI\Codex`.
   - Project files are separate from Codex data. Ask for, detect, or include project folders such as `~/Documents/New project`.

2. Choose a migration mode before packaging.
   - `standard`: default. Package Codex core data, sessions, memories, skills, plugins, generated images, selected app state, and project folders while excluding auth files, browser login state, `.env`, private keys, runtime sockets, caches, `.git`, `node_modules`, and virtualenvs.
   - `full`: include standard data plus logs/caches and an environment inventory. Still exclude secrets and browser login state.
   - `full-with-secrets`: include sensitive auth/token/env/login-state files only when the user explicitly asks for it. Require `--i-understand-secrets`; treat the package like a password vault.

3. On the source computer, generate a neutral migration package.
   - Mac source: run `scripts/create_mac_codex_migration_package.sh`.
   - Windows source: run `scripts/create_windows_codex_migration_package.ps1`.
   - Best practice: install/open Codex once on the target computer, then close Codex before restoring.
   - For the cleanest package, run the packaging script after closing Codex. If running from inside Codex, tell the user that active SQLite/log files can change while copying; verify package size and rerun if needed.
   - Include optional project folders with repeated `--project /path/to/project` arguments.
   - On Windows, include highlighted chat/session JSONL files for audit with repeated `-SelectedChat <path>` arguments. These files are copied to `selected_chats/`, forced into the restorable `home/.codex/sessions` tree when needed, indexed in `home/.codex/session_index.jsonl`, and included in schema v3 metadata exports.
   - Use the script's default exclusions for runtime/cache/dev files such as `.tmp`, `process_manager`, `vendor_imports`, `.git`, `node_modules`, `.venv`, sockets, and browser login databases. These exclusions are necessary because real Mac packages can fail on socket files and unreadable Git/cache objects.

4. Transfer the generated `.zip`.
   - Feishu, cloud drive, LAN share, AirDrop-to-phone-to-PC, or external disk are all acceptable.
   - Treat the package as private: it can contain auth tokens, conversation history, memories, generated files, and logs.

5. On the target computer, unzip and run the restore script for that OS.
   - Windows target: run `Restore-Codex-To-Windows.ps1`. If execution policy blocks it, run `Set-ExecutionPolicy -Scope Process Bypass` in the same PowerShell session.
   - Mac target: run `bash Restore-Codex-To-Mac.sh --restore-projects` when the package includes project folders. Use `--projects-dir <dir>` to choose a custom project destination; otherwise projects restore to `~/Documents/Codex-Restored-Projects`.
   - Restore scripts default to merge restore, not whole-home replacement. They merge sessions, archived sessions, skills, plugin cache, generated images, and `session_index.jsonl`; they preserve target `auth.json`, `config.toml`, `installation_id`, `models_cache.json`, and `chrome-native-hosts-v2.json`.
   - Use destructive replacement only when explicitly requested: `Restore-Codex-To-Mac.sh --replace-codex-home` or `Restore-Codex-To-Windows.ps1 -ReplaceCodexHome`.
   - State databases (`state_*.sqlite`, `memories_*.sqlite`, `goals_*.sqlite`) are not overwritten by default. Use `--replace-state` / `-ReplaceState` only when the user intentionally wants package state to replace target state.
   - If Codex fails to start, close Codex and delete stale `SingletonLock`, `SingletonCookie`, and `SingletonSocket` under the target Codex app support directory.

6. Verify continuity.
   - Open Codex and check recent threads, skills, plugins, memories, generated images, and automations.
   - Windows target: run `scripts/verify_windows_codex_restore.ps1` or the package copy `Verify-Codex-Windows-Restore.ps1`.
   - Mac target: run `scripts/verify_mac_codex_restore.sh --json` or the package copy `Verify-Codex-Mac-Restore.sh --json`.
   - For Mac verification, do not call UI/sidebar readiness complete unless selected chat IDs exist both under restored `~/.codex/sessions` and in `~/.codex/session_index.jsonl`, with `forbidden_files.total == 0`.
   - For schema v3 Mac verification, also require selected chats to exist in `state_*.sqlite.threads`, have existing `rollout_path` files, have target Mac `cwd` values, have remapped session JSONL cwd metadata, have no old source path left in selected JSONL files, and have restored projects in `.codex-global-state.json`.
   - Mac project UI registration must use the bundled official entry point: `/Applications/Codex.app/Contents/Resources/codex app <restored-project-path>`. The Mac restore script invokes this automatically after `--restore-projects`; the verifier reports `project_ui_registration` and `ui_readiness.app_project_registration_ready`.
   - Do not treat hand-written `.codex-global-state.json` project entries as sufficient. A running Codex Desktop process can overwrite them on quit; `codex app <path>` is the observed durable action that makes `list_projects` include the restored project.
   - If `app_project_registration_ready=false`, run `/Applications/Codex.app/Contents/Resources/codex app <restored-project-path>` manually for each restored project, then re-check app/server `list_projects` or the visible sidebar.

## Known Source Findings

Real Mac source validation found this useful shape:

- `~/.codex/sessions` and `~/.codex/archived_sessions`: JSONL conversation sessions.
- `~/.codex/state_*.sqlite`: thread index/state.
- `~/.codex/memories_*.sqlite`: memory database.
- `~/.codex/goals_*.sqlite`: goals database.
- `~/.codex/logs_*.sqlite`: logs; useful but sensitive and large.
- `~/.codex/generated_images`: generated image files.
- `~/.codex/skills`: user and project skills.
- `~/.codex/plugins/cache`: plugin bundles/manifests.
- `~/Library/Application Support/Codex`: desktop app Chromium profile; do not restore cookies/login databases by default.
- `%USERPROFILE%\.codex`: Windows primary Codex state with the same sessions, SQLite state, memories, skills, plugins, and generated images shape.
- `%APPDATA%\Codex`: Windows desktop app profile; do not restore cookies/login databases by default.

## Feature Notes

- All directions use the same neutral package layout with target-specific restore scripts.
- Windows packages use schema version 3, forward-slash zip entries, LF/no-BOM checksums, `MANIFEST.txt`, and `MANIFEST.json` so macOS can unzip and verify them directly.
- Windows packages can include `selected_chats/` via `-SelectedChat`; Mac verification reports selected chat count, restored-session matches, `session_index.jsonl` matches, SQLite thread readiness, path mapping readiness, global project registry readiness, and Codex app project registration readiness.
- Schema v3 packages include `metadata/thread_index_export.json`, `metadata/path_map.json`, `metadata/selected_chats.json`, and `metadata/project_ui_registry_export.json`.
- Always run the target verifier before telling the user migration is complete.
- Mac restore normalizes package permissions, fails if `home/.codex` is missing, defaults to merge restore, and can restore project folders with `--restore-projects`.
- Mac restore scripts may prompt if any Codex process is running during a real restore; isolated `/tmp/codex-*` test restores continue without blocking.
- Project folders are packaged under `projects/`. On Mac, `--restore-projects` copies them to `~/Documents/Codex-Restored-Projects` by default and then calls `codex app <restored-project-path>` so Codex Desktop registers/opens each restored project.

## Scripts

- `scripts/create_mac_codex_migration_package.sh`: Run on Mac to build a neutral migration zip with Windows/Mac restore scripts, README, manifest, checksums, and optional project folders.
- `scripts/create_windows_codex_migration_package.ps1`: Run on Windows to build a Mac-friendly neutral migration zip with forward-slash entries, LF/no-BOM `SHA256SUMS.txt`, Windows/Mac restore scripts, README, manifests, checksums, optional project folders, and optional selected chat files.
- `scripts/restore_codex_to_windows.ps1`: Standalone Windows restore script. Packages also embed a copy named `Restore-Codex-To-Windows.ps1`.
- `scripts/restore_codex_to_mac.sh`: Standalone Mac restore script. Packages also embed a copy named `Restore-Codex-To-Mac.sh`.
- `scripts/collect_windows_codex_inventory.ps1`: Run on Windows before or after restore to summarize existing Codex data locations, sizes, and project folder candidates.
- `scripts/collect_mac_codex_inventory.sh`: Run on Mac before or after restore to summarize existing Codex data locations, sizes, and project folder candidates.
- `scripts/verify_windows_codex_restore.ps1`: Run on Windows after restore to verify restored paths, counts, package metadata, and project candidates.
- `scripts/verify_mac_codex_restore.sh`: Run on Mac after restore to verify restored paths, checksums, selected chats, forbidden-file counts, and restored project folders.

## Handoff Checklist

When the user wants another Codex instance on the source computer to help, send a short instruction like:

```text
Use the codex-rehome workflow. Create a standard <source OS>-to-<target OS> Codex migration package, include Codex data folders and these project folders: <paths>. Exclude auth files, browser login state, .env files, private keys, sockets, .git, node_modules, and virtualenvs. Put the zip on Desktop and tell me the zip path, size, manifest summary, sensitive-file report, checksum, and target restore command.
```

Before finalizing, report:

- Package path and size.
- Migration mode used.
- Whether projects were included or still need separate copying.
- Exact restore steps for the target OS.
- Counts for sessions, skills, plugin manifests, generated images, project files, and important SQLite files when available.
- Any caveats about login state, secrets, platform-specific paths, or live-copy consistency.
