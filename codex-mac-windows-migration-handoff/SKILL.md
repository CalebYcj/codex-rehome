---
name: codex-mac-windows-migration-handoff
description: Package, transfer, restore, and verify Codex data when moving from Mac to Windows or between computers. Use when the user wants to migrate Codex conversations, sessions, memories, skills, plugins, automations, generated images, app data, project folders, developer environment inventory, path mappings, secrets handling, or reproduce a Codex workspace and previous dialogue on another machine, including Feishu/cloud-drive handoff instructions.
---

# Codex Mac-Windows Migration Handoff

Use this skill to make a repeatable migration handoff for the user's Codex collaboration workspace: Codex state, project folders, generated artifacts, skills/plugins, environment inventory, path mappings, and restore verification.

## Workflow

1. Identify source and target OS, usernames, and transfer channel.
   - Mac source paths usually include `~/.codex`, `~/Library/Application Support/Codex`, `~/Library/Application Support/com.openai.codex`, and `~/Library/Application Support/OpenAI/Codex`.
   - Mac support paths can also include `~/Library/Caches/Codex`, `~/Library/Logs/com.openai.codex`, Chrome native host manifests, and Codex preferences.
   - Windows target paths usually include `%USERPROFILE%\.codex`, `%APPDATA%\Codex`, `%APPDATA%\com.openai.codex`, and `%APPDATA%\OpenAI\Codex`.
   - Project files are separate from Codex data. Ask for, detect, or include project folders such as `~/Documents/New project`.

2. Choose a migration mode before packaging.
   - `standard`: default. Package Codex core data, sessions, memories, skills, plugins, generated images, selected app state, and project folders while excluding auth files, browser login state, `.env`, private keys, runtime sockets, caches, `.git`, `node_modules`, and virtualenvs.
   - `full`: include standard data plus logs/caches and an environment inventory. Still exclude secrets and browser login state.
   - `full-with-secrets`: include sensitive auth/token/env/login-state files only when the user explicitly asks for it. Require `--i-understand-secrets`; treat the package like a password vault.

3. On the source Mac, prefer generating a migration package with `scripts/create_mac_codex_migration_package.sh`.
   - Best practice: install/open Codex once on the Windows computer, then close Codex before restoring.
   - For the cleanest package, run the Mac script from Terminal after closing Codex. If running from inside Codex, tell the user that active SQLite/log files can change while copying; verify package size and rerun if needed.
   - Include optional project folders with repeated `--project /path/to/project` arguments.
   - Use the script's default exclusions for runtime/cache/dev files such as `.tmp`, `process_manager`, `vendor_imports`, `.git`, `node_modules`, `.venv`, sockets, and browser login databases. These exclusions are necessary because real Mac packages can fail on socket files and unreadable Git/cache objects.

4. Transfer the generated `.zip`.
   - Feishu, cloud drive, LAN share, AirDrop-to-phone-to-PC, or external disk are all acceptable.
   - Treat the package as private: it can contain auth tokens, conversation history, memories, generated files, and logs.

5. On Windows, unzip and run the included `Restore-Codex-To-Windows.ps1`.
   - The restore script backs up existing target directories before copying.
   - If execution policy blocks it, run `Set-ExecutionPolicy -Scope Process Bypass` in the same PowerShell session.
   - If Codex fails to start, close Codex and delete stale `SingletonLock`, `SingletonCookie`, and `SingletonSocket` under `%APPDATA%\Codex`.

6. Verify continuity.
   - Open Codex and check recent threads, skills, plugins, memories, generated images, and automations.
   - Run `scripts/verify_windows_codex_restore.ps1` after restore to count sessions, skills, plugin manifests, generated images, SQLite files, package metadata, and project candidates.
   - Reopen the project folder from its new Windows location if old conversations reference Mac paths like `/Users/<name>/...`.
   - If a project was included in the package, move it from `projects/` to the desired Windows folder and update/reopen the workspace in Codex.

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

## Scripts

- `scripts/create_mac_codex_migration_package.sh`: Run on Mac to build a Windows-oriented migration zip with restore script, README, manifest, checksums, and optional project folders.
- `scripts/restore_codex_to_windows.ps1`: Standalone Windows restore script. The Mac package also embeds a copy named `Restore-Codex-To-Windows.ps1`.
- `scripts/collect_windows_codex_inventory.ps1`: Run on Windows before or after restore to summarize existing Codex data locations, sizes, and project folder candidates.
- `scripts/verify_windows_codex_restore.ps1`: Run on Windows after restore to verify restored paths, counts, package metadata, and project candidates.

## Handoff Checklist

When the user wants another Codex instance on the source Mac to help, send a short instruction like:

```text
Use the codex-mac-windows-migration-handoff workflow. Create a standard Mac-to-Windows Codex migration package, include ~/.codex, Codex Application Support folders, and these project folders: <paths>. Exclude auth files, browser login state, .env files, private keys, sockets, .git, node_modules, and virtualenvs. Put the zip on Desktop and tell me the zip path, size, manifest summary, sensitive-file report, and checksum.
```

Before finalizing, report:

- Package path and size.
- Migration mode used.
- Whether projects were included or still need separate copying.
- Exact Windows restore steps.
- Counts for sessions, skills, plugin manifests, generated images, project files, and important SQLite files when available.
- Any caveats about login state, secrets, platform-specific paths, or live-copy consistency.
