---
name: codex-rehome
description: "Use when the user wants to migrate, back up, restore, or reproduce a Codex Desktop workspace between Mac and Windows computers, preserve Codex before reinstalling an OS, or bridge Claude Code / Claude Desktop agent history into a Codex-readable handoff; relevant for Codex sessions, projects, skills, plugins, path mappings, restore verification, and Claude-to-Codex migration checks."
---

# Codex Rehome

Use this skill to make a repeatable migration handoff for the user's Codex collaboration workspace: Codex state, project folders, generated artifacts, skills/plugins, MCP/connectors, environment inventory, path mappings, and restore verification.

Supported directions:

- Mac -> Windows
- Windows -> Mac
- Windows -> Windows
- Mac -> Mac
- Same computer OS reinstall: back up to a non-system partition or external disk before wiping the OS, then restore after reinstalling and logging in to Codex.
- Claude Code / Claude Desktop agent history -> Codex handoff package, when real local Claude transcript files exist.

## Positioning

Treat this folder as an agent workflow plus executable helpers:

- `SKILL.md` is the agent-facing procedure and decision guide.
- `scripts/` contains deterministic helpers for packaging on Mac or Windows, restoring to Mac or Windows, collecting inventory, and verifying counts.
- `references/` contains supplemental path-mapping details.
- The repository README files are human-facing documentation and search/GEO entry points.

Do not treat the skill as only a script. Use the instructions to decide mode, safety boundaries, transfer channel, and verification, then call the scripts for repeatable file operations.

## Core Model

Restoring a visible Codex project with its conversations is not a plain file copy. Treat every project/conversation migration as four layers:

1. File layer: copy sessions, `session_index.jsonl`, skills, plugins, selected SQLite files, generated artifacts, and the project folder itself.
2. Path mapping layer: rewrite source-machine paths to target-machine paths in SQLite rows and restored session JSONL metadata such as `session_meta`, `turn_context`, and workspace roots.
3. Index layer: make the conversation discoverable through Codex's thread index/state, especially `state_*.sqlite.threads`, `rollout_path`, `cwd`, title/preview, timestamps, archived state, and `session_index.jsonl`.
4. App registration layer: make Codex Desktop register/open the restored workspace through its own project-opening entry point.

The app registration layer is required. Do not treat UI project recovery as a JSON/SQLite hand-editing problem. Hand-written `.codex-global-state.json` entries and repaired `state_*.sqlite` rows can make internal thread reads work while the left sidebar still misses the project. On Mac, the observed durable command is:

```bash
/Applications/Codex.app/Contents/Resources/codex app <restored-project-path>
```

Windows needs the same class of official app/open-workspace operation. The Windows restore script attempts `codex app <restored-project-path>` after `-RestoreProjects`; if Windows packaged-app permissions block that CLI call, reopen the restored folder from Codex Desktop and rerun the verifier.

## Agent Bridge: Claude Code To Codex

Use this branch when the user says they want to move from Claude Code to Codex, bring Claude Code project history into Codex, or prepare for a Claude-to-GPT/Codex workflow switch. This is different from Codex-to-Codex rehome.

Default promise: create a Codex-readable project handoff from real local Claude transcripts. Do not claim native Codex sidebar restoration for Claude sessions.

Current supported source shapes:

- Traditional Claude Code CLI: `~/.claude/projects`.
- Claude Desktop on Windows: `%APPDATA%\Claude\claude-code-sessions`, `%APPDATA%\Claude\local-agent-mode-sessions`, and MSIX-virtualized equivalents under `%LOCALAPPDATA%\Packages\Claude_*\LocalCache\Roaming\Claude`.

First run the read-only detector:

```bash
python scripts/inspect_claude_agent_sources.py --json
```

Interpret the status before exporting:

- `exportable_transcripts_found`: real Claude JSONL transcripts exist. Continue with export.
- `installed_but_no_entitled_claude_code_sessions`: Claude Desktop/Claude Code is installed, but the current account is blocked by the Pro/Max requirement or has no usable local sessions. Tell the user this clearly. Do not fake a migration.
- `no_exportable_transcripts_found`: no supported local Claude transcript files were found. Ask the user to run Claude Code with an entitled account or provide a real local transcript package from another machine.

When real transcript files exist, generate a Codex handoff folder:

```bash
python scripts/export_claude_to_codex_handoff.py \
  --source "<claude-jsonl-file-or-session-directory>" \
  --out "<output-parent-directory>" \
  --title "Claude To Codex Handoff"
```

Use `--include-raw` only when the user understands that raw transcripts can contain prompts, code, terminal output, local paths, and secrets. By default, the exporter writes a readable handoff without copying raw JSONL files.

Verify the handoff:

```bash
python scripts/verify_agent_bridge_handoff.py "<handoff-folder>" --json
```

Then open the generated handoff folder in Codex and instruct Codex to read `next-steps-for-codex.md`, `source-manifest.json`, and `claude-transcript.md`. Treat the transcript as historical context, not as live Claude tool state.

Optional advanced backend: if the user has `cct` installed, it can be used for deeper Codex <-> Claude Code session transfer. Keep it optional and local-first; do not require it for the standard handoff flow.

Do not include ChatGPT web export/import in this branch. It is intentionally out of scope for the current Claude Code Bridge version.

## Workflow

Before running commands, tell the user which stage they are in and what they need to do next. Use plain language:

- Old computer/source stage: "I will help package your old Codex conversations, projects, skills, and local state into a zip. You choose what to include; then you transfer the zip privately to the new computer."
- Transfer stage: "Move the zip with Feishu, cloud drive, AirDrop, external disk, LAN share, or another private channel. Do not post it publicly."
- New computer/target stage: "Install and log in to Codex first. Then give me the zip; I will unzip it, run the restore script, map old paths to this computer, merge conversation indexes, restore project folders, and register/open the projects in Codex Desktop."
- Reinstall stage: "Before wiping the system drive, I will package Codex to a non-C drive or external disk and save restore instructions next to the zip. After reinstalling, install and log in to Codex, then give the zip and instructions to the new system's Codex."
- Verification stage: "I will run the verifier and tell you what came back: sessions, selected chats, projects, forbidden files, and app-visible project registration."

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
   - Same-computer Windows reinstall: do not output the package to Desktop, Downloads, `%USERPROFILE%`, or any path that will be wiped with `C:`. Use `-Out "D:\Codex-Rehome-Backup"`, another non-system partition, or an external disk. Save the GitHub URL, README screenshot, or restore prompt in the same backup folder.
   - Best practice: install/open Codex once on the target computer, then close Codex before restoring.
   - For the cleanest package, run the packaging script after closing Codex. If running from inside Codex, tell the user that active SQLite/log files can change while copying; verify package size and rerun if needed.
   - Include optional project folders with repeated `--project /path/to/project` arguments on Mac or repeated `-Project <path>` arguments on Windows.
   - Include highlighted chat/session JSONL files for audit with repeated `--selected-chat <path>` on Mac or repeated `-SelectedChat <path>` on Windows. These files are copied to `selected_chats/` and included in schema v3 metadata exports. Windows packaging also forces selected chats into the restorable `home/.codex/sessions` tree and `session_index.jsonl` when needed.
   - Use the script's default exclusions for runtime/cache/dev files such as `.tmp`, `process_manager`, `vendor_imports`, `.git`, `node_modules`, `.venv`, sockets, and browser login databases. These exclusions are necessary because real Mac packages can fail on socket files and unreadable Git/cache objects.

4. Transfer the generated `.zip`.
   - Feishu, cloud drive, LAN share, AirDrop-to-phone-to-PC, or external disk are all acceptable.
   - For same-computer OS reinstall, "transfer" means keeping the zip, manifests, checksum files, and restore instructions on a partition or external disk that will survive the reinstall. Confirm this before wiping the system drive.
   - Treat the package as private: it can contain auth tokens, conversation history, memories, generated files, and logs.

5. On the target computer, unzip and run the restore script for that OS.
   - Windows target: run `Restore-Codex-To-Windows.ps1 -RestoreProjects` when the package includes project folders. Use `-ProjectsDir <dir>` to choose a custom project destination; otherwise projects restore to `%USERPROFILE%\Documents\Codex-Restored-Projects`. If execution policy blocks it, run `Set-ExecutionPolicy -Scope Process Bypass` in the same PowerShell session.
   - Mac target: run `bash Restore-Codex-To-Mac.sh --restore-projects` when the package includes project folders. Use `--projects-dir <dir>` to choose a custom project destination; otherwise projects restore to `~/Documents/Codex-Restored-Projects`.
   - Restore scripts default to merge restore, not whole-home replacement. They merge sessions, archived sessions, skills, plugin cache, generated images, and `session_index.jsonl`; they preserve target `auth.json`, `config.toml`, `installation_id`, `models_cache.json`, and `chrome-native-hosts-v2.json`.
   - Use destructive replacement only when explicitly requested: `Restore-Codex-To-Mac.sh --replace-codex-home` or `Restore-Codex-To-Windows.ps1 -ReplaceCodexHome`.
   - State databases (`state_*.sqlite`, `memories_*.sqlite`, `goals_*.sqlite`) are not overwritten by default. Use `--replace-state` / `-ReplaceState` only when the user intentionally wants package state to replace target state.
   - If Codex fails to start, close Codex and delete stale `SingletonLock`, `SingletonCookie`, and `SingletonSocket` under the target Codex app support directory.

6. Verify continuity.
   - Open Codex and check recent threads, skills, plugins, memories, generated images, and automations.
   - Windows target: run `scripts/verify_windows_codex_restore.ps1 -Json` or the package copy `Verify-Codex-Windows-Restore.ps1 -Json`.
   - Mac target: run `scripts/verify_mac_codex_restore.sh --json` or the package copy `Verify-Codex-Mac-Restore.sh --json`.
   - For Mac and Windows verification, do not call UI/sidebar readiness complete unless selected chat IDs exist both under restored `.codex/sessions` and in `.codex/session_index.jsonl`, with forbidden files excluded.
   - For schema v3 verification, also require selected chats to exist in `state_*.sqlite.threads`, have existing `rollout_path` files, have target `cwd` values, have remapped session JSONL cwd metadata, have no old source path left in selected JSONL files, and have restored projects in `.codex-global-state.json`.
   - Mac project UI registration must use the bundled official entry point: `/Applications/Codex.app/Contents/Resources/codex app <restored-project-path>`. Windows project UI registration should use `codex app <restored-project-path>` when available. The restore scripts invoke or attempt this automatically after project restore; the verifiers report `project_ui_registration` and `ui_readiness.app_project_registration_ready`.
   - Do not treat hand-written `.codex-global-state.json` project entries as sufficient. A running Codex Desktop process can overwrite them on quit; `codex app <path>` is the observed durable action that makes `list_projects` include the restored project.
   - If `app_project_registration_ready=false`, run the relevant `codex app <restored-project-path>` command manually for each restored project, or reopen the restored project folder from Codex Desktop, then re-check app/server `list_projects` or the visible sidebar.

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
- Mac and Windows packages use schema version 3 metadata, LF/no-BOM checksums, `MANIFEST.txt`, and `MANIFEST.json`. Windows packages also use forward-slash zip entries so macOS can unzip and verify them directly.
- Mac packages can include `selected_chats/` via `--selected-chat`; Windows packages can include them via `-SelectedChat`. Mac and Windows verification report selected chat count, restored-session matches, `session_index.jsonl` matches, SQLite thread readiness, path mapping readiness, global project registry readiness, and Codex app project registration readiness.
- Schema v3 packages include `metadata/thread_index_export.json`, `metadata/path_map.json`, `metadata/selected_chats.json`, and `metadata/project_ui_registry_export.json`.
- Always run the target verifier before telling the user migration is complete.
- Mac restore normalizes package permissions, fails if `home/.codex` is missing, defaults to merge restore, and can restore project folders with `--restore-projects`.
- Windows restore defaults to merge restore, can restore project folders with `-RestoreProjects`, imports schema v3 UI-ready metadata when present, and attempts app project registration with `codex app <restored-project-path>`.
- Mac restore scripts may prompt if any Codex process is running during a real restore; isolated `/tmp/codex-*` test restores continue without blocking.
- Project folders are packaged under `projects/`. On Mac, `--restore-projects` copies them to `~/Documents/Codex-Restored-Projects` by default and then calls `codex app <restored-project-path>` so Codex Desktop registers/opens each restored project. On Windows, `-RestoreProjects` copies them to `%USERPROFILE%\Documents\Codex-Restored-Projects` by default and attempts the same `codex app <restored-project-path>` registration.

## Limits To Communicate

Set expectations before and after restore. This workflow restores the useful local Codex workspace, but it is not official cloud sync and it cannot promise that every old thread resumes exactly as if the machine never changed.

- Old chat windows may not keep a live working-directory handle after a cross-OS move. Conversation text and session history can be restored, but a thread that originally worked inside `C:\...` may not keep editing that same folder after it becomes `/Users/...` on Mac, or the reverse. Use the restored old conversation as context, then reopen the restored project folder in Codex and continue in a new project thread when needed.
- Project source files are only included when explicitly passed with `--project` on Mac or `-Project` on Windows. Codex history and project files are separate.
- App sidebar visibility is not guaranteed by file copy alone. The restore scripts invoke or attempt `codex app <restored-project-path>`, but if system permissions or Codex Desktop state block it, the user must manually open the restored project folder from Codex Desktop and rerun verification.
- Login state and browser sessions are not migrated by default. Expect to log in again to Codex, GitHub, Feishu, Gmail, browser extensions, and MCP/connectors.
- Secrets are excluded by default: auth files, tokens, cookies, `.env`, private keys, browser Login Data, Local Storage, `.git`, `node_modules`, and virtual environments.
- Native dependencies are not portable. Reinstall or rebuild `node_modules`, Python virtualenvs, compiled binaries, app/game/tool runtimes, and OS-specific dependencies on the target computer.
- Running processes, open terminal sessions, unsaved editor buffers, in-memory app state, and live GUI layout are not migrated.
- Cross-account or cross-workspace restoration can be incomplete. A conversation restored under a different Codex/OpenAI/GitHub account may be visible as local history but still need fresh authorization for remote services.
- Verifiers check files, indexes, path mapping, selected chats, forbidden files, and best-effort project registration. Passing verification means the migration package is structurally ready; it does not guarantee that every old conversation can continue editing immediately without reopening the restored project.

## Scripts

- `scripts/create_mac_codex_migration_package.sh`: Run on Mac to build a neutral migration zip with schema v3 metadata, Windows/Mac restore scripts, README, manifest, checksums, optional project folders, and optional selected chat files.
- `scripts/create_windows_codex_migration_package.ps1`: Run on Windows to build a Mac-friendly neutral migration zip with schema v3 metadata, forward-slash entries, LF/no-BOM `SHA256SUMS.txt`, Windows/Mac restore scripts, README, manifests, checksums, optional project folders, and optional selected chat files.
- `scripts/restore_codex_to_windows.ps1`: Standalone Windows restore script with merge restore, optional `-RestoreProjects`, schema v3 UI-ready metadata import, and best-effort Codex Desktop project registration. Packages also embed a copy named `Restore-Codex-To-Windows.ps1`.
- `scripts/restore_codex_to_mac.sh`: Standalone Mac restore script. Packages also embed a copy named `Restore-Codex-To-Mac.sh`.
- `scripts/collect_windows_codex_inventory.ps1`: Run on Windows before or after restore to summarize existing Codex data locations, sizes, and project folder candidates.
- `scripts/collect_mac_codex_inventory.sh`: Run on Mac before or after restore to summarize existing Codex data locations, sizes, and project folder candidates.
- `scripts/verify_windows_codex_restore.ps1`: Run on Windows after restore to verify restored paths, counts, package metadata, selected chats, thread index readiness, restored projects, and app project registration readiness.
- `scripts/verify_mac_codex_restore.sh`: Run on Mac after restore to verify restored paths, checksums, selected chats, forbidden-file counts, and restored project folders.
- `scripts/inspect_claude_agent_sources.py`: Read-only detector for Claude Code CLI and Claude Desktop agent history sources, including Windows free-account/Pro-Max entitlement failures.
- `scripts/export_claude_to_codex_handoff.py`: Converts real Claude JSONL transcripts into a Codex-readable handoff folder.
- `scripts/verify_agent_bridge_handoff.py`: Verifies the generated Claude-to-Codex handoff folder.

## Handoff Checklist

When the user wants another Codex instance on the source computer to help, send a short instruction like:

```text
Use the codex-rehome workflow. Create a standard <source OS>-to-<target OS> Codex migration package, include Codex data folders and these project folders: <paths>. Exclude auth files, browser login state, .env files, private keys, sockets, .git, node_modules, and virtualenvs. For normal cross-computer transfer, put the zip somewhere easy to find such as Desktop. For same-computer OS reinstall, put the zip on a surviving non-system partition or external disk, never Desktop, Downloads, %USERPROFILE%, or C:. Tell me the zip path, size, manifest summary, sensitive-file report, checksum, and target restore command.
```

For a same-computer reinstall, save a prompt like this next to the zip before wiping the system:

```text
I just reinstalled this computer. This folder contains a Codex migration package created with codex-rehome before reinstall. Use the latest instructions from https://github.com/CalebYcj/codex-rehome, unzip the package, run the restore script for this OS with project restore enabled, restore projects into the default Codex-Restored-Projects folder, register/open restored projects in Codex Desktop, then run the verifier and report which sessions, skills, plugins, and projects restored successfully. Do not restore auth tokens, browser cookies, .env files, private keys, node_modules, .git, or virtual environments.
```

Before finalizing, report:

- Package path and size.
- Migration mode used.
- Whether projects were included or still need separate copying.
- Exact restore steps for the target OS.
- Counts for sessions, skills, plugin manifests, generated images, project files, and important SQLite files when available.
- Any caveats about login state, secrets, platform-specific paths, or live-copy consistency.
