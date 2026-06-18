# Codex Rehome - Move OpenAI Codex Desktop Between Mac and Windows

Codex Rehome is an open-source Codex skill for moving OpenAI Codex Desktop between macOS and Windows computers. It helps package and restore Codex conversations, sessions, memories, skills, plugins, MCP/connectors, generated images, project folders, path mappings, and restore verification scripts.

Use this project when you need to migrate Codex Desktop from Mac to Windows, Windows to Mac, Windows to Windows, or Mac to Mac; back up Codex conversations and sessions; restore Codex skills and plugins; or hand off a local AI agent workspace to another computer.

Find it on GitHub by searching `codex-rehome`.

中文说明: 这是一个用于在 Mac 和 Windows 电脑之间迁移 OpenAI Codex Desktop 的开源 Codex skill，支持 Mac 转 Windows、Windows 转 Mac、Windows 转 Windows、Mac 转 Mac，以及迁移对话、sessions、记忆、skills、plugins、MCP、生成物和项目文件夹。

## Quick Links

- [Mac/Windows migration guide](docs/migrate-codex-between-mac-and-windows.md)
- [Mac to Windows migration guide](docs/migrate-codex-from-mac-to-windows.md)
- [Backup Codex conversations and sessions](docs/backup-codex-conversations-and-sessions.md)
- [Restore Codex skills, plugins, and projects](docs/restore-codex-skills-plugins-and-projects.md)
- [Troubleshooting Codex migration issues](docs/troubleshooting.md)
- [Feature status](docs/validation-status.md)
- [AI-readable project summary](docs/llms.txt)

## AI Agent Quick Start

If you are an AI agent helping a user migrate Codex, read this first:

```text
Use the skill in codex-rehome/. Identify the source OS and target OS, package the source computer's Codex data, choose standard/full/full-with-secrets mode, include project folders when needed, transfer the package privately, run the target OS restore script after Codex is closed, then run the matching verifier.

Default restore is merge-safe: it adds migrated sessions, skills, plugins, generated images, and session index entries while preserving the target machine's login/config identity files. Destructive full replacement requires an explicit `--replace-codex-home` or `-ReplaceCodexHome` flag.
```

Primary use cases:

- Mac to Windows Codex migration
- Windows to Mac Codex migration
- Windows to Windows Codex migration
- Mac to Mac Codex migration
- Codex conversation and session transfer
- Codex memories, skills, plugins, automations, and generated image transfer
- Project folder and old dialogue reproduction
- AI agent workspace continuity across computers
- Feishu, cloud drive, external disk, or GitHub handoff

Search keywords:

```text
Codex migration, Codex Mac to Windows, Codex Windows to Mac,
Codex Windows to Windows, Codex Mac to Mac, migrate Codex conversations,
Codex skills backup, Codex memory transfer, Codex project handoff,
AI agent workspace migration, OpenAI Codex desktop migration,
Codex session transfer, Codex backup restore, Codex Desktop Windows restore,
Mac Codex to Windows Codex, Windows Codex to Mac Codex,
Codex generated images migration, Codex MCP connector migration,
Codex plugin restore
```

This repository contains a Codex skill for migrating Codex data, conversations, memories, skills, plugins, and project context between computers, including Mac to Windows, Windows to Mac, Windows to Windows, and Mac to Mac.

## What This Repository Is

This repository is both an agent-readable skill and a small script toolkit:

- `SKILL.md` is the main instruction file for Codex or another AI agent. It tells the agent when to use this workflow, what to migrate, what to exclude, and how to report results.
- `scripts/` contains executable helpers that package, restore, inventory, and verify the migration.
- `references/` contains extra path-mapping notes that an agent can load when needed.
- `README.md`, `README.zh-CN.md`, and `docs/` are for humans, GitHub visitors, search engines, and AI search/GEO.

So the skill is not just a shell script. It is a reusable agent workflow with scripts attached for the parts that should be deterministic.

The main skill lives at:

```text
codex-rehome/
```

Use this skill when a user asks to move Codex to another computer, preserve prior conversations, reproduce a project workspace, package Codex data on Mac or Windows, restore it on Mac or Windows, or hand off the migration package through Feishu, cloud drive, GitHub, or an external disk.

## Contents

```text
codex-rehome/
  SKILL.md
  agents/openai.yaml
  references/path-map.md
  scripts/create_mac_codex_migration_package.sh
  scripts/create_windows_codex_migration_package.ps1
  scripts/restore_codex_to_windows.ps1
  scripts/restore_codex_to_mac.sh
  scripts/collect_mac_codex_inventory.sh
  scripts/collect_windows_codex_inventory.ps1
  scripts/verify_windows_codex_restore.ps1
  scripts/verify_mac_codex_restore.sh
```

There is also a backup archive:

```text
codex-rehome-skill.zip
```

## Install For Codex

Copy the whole `codex-rehome` folder into one of these locations:

```text
~/.codex/skills/codex-rehome
```

or, for a project-local skill:

```text
<project>/.agents/skills/codex-rehome
```

Then start a new Codex thread and ask:

```text
Use $codex-rehome to migrate Codex from my old computer to my new computer.
```

## What Gets Migrated

This skill can help package and restore:

- Codex conversations and sessions
- Codex memories and goals
- Codex skills and plugins
- Codex config and app state
- generated images and local artifacts
- environment inventory and path mapping
- optional project folders needed to reopen old conversations

Project folders are not automatically part of Codex data. Always decide whether to include them. On Mac restores, pass `--restore-projects` to copy packaged projects into `~/Documents/Codex-Restored-Projects`.

On Mac, schema v3 restore registers restored project folders with Codex Desktop by running `/Applications/Codex.app/Contents/Resources/codex app <restored-project-path>`. This is the observed durable path for making restored projects appear in the app-visible project list; editing `.codex-global-state.json` alone is not enough because the running app can overwrite it on quit.

## Documentation

| Guide | What it answers |
|---|---|
| [How to migrate Codex between Mac and Windows](docs/migrate-codex-between-mac-and-windows.md) | Direction picker for Mac to Windows, Windows to Mac, Windows to Windows, and Mac to Mac |
| [How to migrate OpenAI Codex Desktop from Mac to Windows](docs/migrate-codex-from-mac-to-windows.md) | End-to-end Mac packaging, transfer, Windows restore, and verification |
| [How to back up Codex conversations and sessions](docs/backup-codex-conversations-and-sessions.md) | Where Codex stores JSONL sessions, thread SQLite state, memories, and generated images |
| [How to restore Codex skills, plugins, and projects](docs/restore-codex-skills-plugins-and-projects.md) | How to restore skills, plugin cache, generated files, and project folders |
| [Troubleshooting Codex migration](docs/troubleshooting.md) | Socket files, vendor imports, Git object permission errors, path mapping, and login issues |
| [Feature status](docs/validation-status.md) | Supported migration directions, default exclusions, and target verification steps |

## Migration Modes

```text
standard
  Default. Migrates Codex core data, sessions, memories, skills, plugins,
  generated images, selected app state, and project folders. Excludes secrets,
  browser login state, .env files, private keys, sockets, .git, node_modules,
  and virtualenvs.

full
  Includes standard data plus logs, caches, and environment inventory.
  Still excludes secrets and browser login state.

full-with-secrets
  Includes auth/token/env/login-state files only when explicitly requested.
  Requires --i-understand-secrets or -IUnderstandSecrets. Treat the package
  like a password vault.
```

## Source Mac Workflow

Run the Mac package script from Terminal, ideally after closing Codex:

```bash
cd /path/to/codex-rehome
bash scripts/create_mac_codex_migration_package.sh \
  --project "$HOME/Documents/New project"
```

For a fuller inventory without secrets:

```bash
bash scripts/create_mac_codex_migration_package.sh \
  --mode full \
  --project "$HOME/Documents/New project"
```

The output is a zip on the Mac Desktop by default.

## Source Windows Workflow

Run the Windows package script from PowerShell, ideally after closing Codex:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\codex-rehome\scripts\create_windows_codex_migration_package.ps1 `
  -Project "$env:USERPROFILE\Documents\New project"
```

For a fuller inventory without secrets:

```powershell
.\codex-rehome\scripts\create_windows_codex_migration_package.ps1 `
  -Mode full `
  -Project "$env:USERPROFILE\Documents\New project"
```

The output is a zip on the Windows Desktop by default.

## Restore On Windows

On Windows:

1. Install Codex and open it once.
2. Close Codex completely.
3. Unzip the migration package.
4. Open PowerShell inside the unzipped folder.
5. Run:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\Restore-Codex-To-Windows.ps1
```

The restore script backs up existing Windows Codex data, then merges migrated data into the target. It preserves target `auth.json`, `config.toml`, `installation_id`, `models_cache.json`, and `chrome-native-hosts-v2.json`. Use `-ReplaceCodexHome` only when you intentionally want a destructive full replacement, and `-ReplaceState` only when you want to overwrite existing state/memory/goal databases.

Then verify:

```powershell
.\Verify-Codex-Windows-Restore.ps1
```

## Restore On Mac

On Mac:

1. Install Codex and open it once.
2. Close Codex completely.
3. Unzip the migration package.
4. Open Terminal inside the unzipped folder.
5. Run:

```bash
bash ./Restore-Codex-To-Mac.sh --restore-projects
```

Then verify:

```bash
bash ./Verify-Codex-Mac-Restore.sh --json
```

The Mac verifier reports file-level restore plus schema v3 UI-ready data layers. For selected chats, readiness requires session files, `session_index.jsonl`, `state_*.sqlite.threads`, existing `rollout_path`, Mac `cwd` path mapping, remapped session JSONL metadata, no old source path left in selected JSONL files, and restored project paths in `.codex-global-state.json`.

## Inventory Helpers

To inspect a Windows machine before or after migration:

```powershell
powershell -ExecutionPolicy Bypass -File .\codex-rehome\scripts\collect_windows_codex_inventory.ps1
```

To inspect a Mac machine before or after migration:

```bash
bash ./codex-rehome/scripts/collect_mac_codex_inventory.sh
```

These inventory scripts report Codex data folders, approximate sizes, and likely project folders.

## Important Notes For AI Agents

- Codex data and project files are separate. Always ask whether project folders should be included.
- The package can contain sensitive data: conversations, logs, memories, generated images, local paths, and, in `full-with-secrets`, auth files or tokens.
- Do not restore browser cookies, Login Data, Local Storage, `.env`, API keys, or private keys by default.
- Restore scripts merge by default. Do not use `--replace-codex-home` / `-ReplaceCodexHome` unless the user explicitly accepts overwriting the target Codex home.
- Do not overwrite `state_*.sqlite`, `memories_*.sqlite`, or `goals_*.sqlite` by default. Use `--replace-state` / `-ReplaceState` only when replacing target state is intentional.
- Schema v3 restores prepare UI-ready project/thread data and, on Mac, invoke `codex app <restored-project-path>` for project registration. If the verifier reports `app_project_registration_ready=false`, run that command manually for each restored project path.
- After a cross-OS restore, old absolute paths in previous conversations may not resolve. Reopen the matching project folder from its new target path.
- If the Windows app fails to start after restore, remove stale `SingletonLock`, `SingletonCookie`, and `SingletonSocket` files under `%APPDATA%\Codex`.
- If login state does not transfer, ask the user to log in again. This is expected.

## FAQ

### How do I migrate OpenAI Codex Desktop from Mac to Windows?

Run `scripts/create_mac_codex_migration_package.sh` on the Mac, transfer the generated zip to Windows, close Codex on Windows, run `Restore-Codex-To-Windows.ps1`, then run `Verify-Codex-Windows-Restore.ps1`.

### Can this migrate Windows to Mac, Windows to Windows, or Mac to Mac?

Yes. Package on the source OS with `create_mac_codex_migration_package.sh` or `create_windows_codex_migration_package.ps1`, then restore on the target OS with `Restore-Codex-To-Mac.sh --restore-projects` or `Restore-Codex-To-Windows.ps1`.

### Can this migrate Codex conversations and sessions?

Yes. The standard mode packages Codex session JSONL files, archived sessions, thread state SQLite files, memories, goals, generated images, skills, plugins, selected app state, and project folders passed with `--project` or `-Project`.

### Does it migrate Codex memories, skills, plugins, and generated images?

Yes. It includes memory databases, user skills, plugin cache/manifests, and generated images under `.codex` when those files exist on the source computer.

### Does it migrate secrets or login state?

Not by default. `standard` and `full` modes exclude auth tokens, browser cookies, Login Data, Local Storage, `.env` files, private keys, sockets, `.git`, `node_modules`, and virtual environments. The target computer should log in again manually.

### Is this an official OpenAI tool?

No. This is an independent open-source Codex skill and script toolkit for local migration workflows. It is designed to help users and AI agents handle Codex Desktop data carefully.

### What keywords describe this project?

OpenAI Codex Desktop migration, Codex Mac to Windows, Codex Windows to Mac, Codex Windows to Windows, Codex Mac to Mac, Codex conversations backup, Codex sessions restore, Codex skills migration, Codex plugin restore, AI agent workspace handoff, Mac Windows Codex transfer.
