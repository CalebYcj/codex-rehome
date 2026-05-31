# Codex Mac-Windows Migration Handoff

Move Codex from Mac to Windows without losing conversations, memories, skills, plugins, automations, generated images, or project context.

This is a small public Codex skill for agents and users who need to reproduce a Codex collaboration workspace on another computer.

## AI Agent Quick Start

If you are an AI agent helping a user migrate Codex, read this first:

```text
Use the skill in codex-mac-windows-migration-handoff/. Package the source computer's Codex data, choose standard/full/full-with-secrets mode, include project folders when needed, transfer the package privately, run the Windows restore script after Codex is closed, then run the verifier.
```

Primary use cases:

- Mac to Windows Codex migration
- Codex conversation and session transfer
- Codex memories, skills, plugins, automations, and generated image transfer
- Project folder and old dialogue reproduction
- AI agent workspace continuity across computers
- Feishu, cloud drive, external disk, or GitHub handoff

Search keywords:

```text
Codex migration, Codex Mac to Windows, migrate Codex conversations,
Codex skills backup, Codex memory transfer, Codex project handoff,
AI agent workspace migration, OpenAI Codex desktop migration
```

This repository contains a Codex skill for migrating Codex data, conversations, memories, skills, plugins, and project context between computers, especially from macOS to Windows.

The main skill lives at:

```text
codex-mac-windows-migration-handoff/
```

Use this skill when a user asks to move Codex to another computer, preserve prior conversations, reproduce a project workspace, package Codex data on Mac, restore it on Windows, or hand off the migration package through Feishu, cloud drive, GitHub, or an external disk.

## Contents

```text
codex-mac-windows-migration-handoff/
  SKILL.md
  agents/openai.yaml
  references/path-map.md
  scripts/create_mac_codex_migration_package.sh
  scripts/restore_codex_to_windows.ps1
  scripts/collect_windows_codex_inventory.ps1
  scripts/verify_windows_codex_restore.ps1
```

There is also a backup archive:

```text
codex-mac-windows-migration-handoff-skill.zip
```

## Install For Codex

Copy the whole `codex-mac-windows-migration-handoff` folder into one of these locations:

```text
~/.codex/skills/codex-mac-windows-migration-handoff
```

or, for a project-local skill:

```text
<project>/.agents/skills/codex-mac-windows-migration-handoff
```

Then start a new Codex thread and ask:

```text
Use $codex-mac-windows-migration-handoff to migrate Codex from my Mac to my Windows computer.
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

Project folders are not automatically part of Codex data. Always decide whether to include them.

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
  Requires --i-understand-secrets. Treat the package like a password vault.
```

## Source Mac Workflow

Run the Mac package script from Terminal, ideally after closing Codex:

```bash
cd /path/to/codex-mac-windows-migration-handoff
bash scripts/create_mac_codex_migration_package.sh \
  --project "$HOME/Documents/New project"
```

For a fuller inventory without secrets:

```bash
bash scripts/create_mac_codex_migration_package.sh \
  --mode full \
  --project "$HOME/Documents/New project"
```

The script packages:

- `~/.codex`
- Codex app support folders under `~/Library/Application Support`
- optional cache folders
- optional project folders passed with `--project`
- a Windows restore script
- a Windows verification script
- a manifest and checksums
- a sensitive-file report without printing secret values

The output is a zip on the Mac Desktop by default.

The script deliberately excludes runtime/cache/dev paths that caused real Mac packaging failures, such as sockets, `vendor_imports`, `.git`, `node_modules`, and virtualenv folders.

## Windows Restore Workflow

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

The restore script backs up existing Windows Codex data before copying the migrated data.

Then verify:

```powershell
.\Verify-Codex-Windows-Restore.ps1
```

or, from the skill folder:

```powershell
powershell -ExecutionPolicy Bypass -File .\codex-mac-windows-migration-handoff\scripts\verify_windows_codex_restore.ps1
```

## Windows Inventory

To inspect a Windows machine before or after migration:

```powershell
powershell -ExecutionPolicy Bypass -File .\codex-mac-windows-migration-handoff\scripts\collect_windows_codex_inventory.ps1
```

This reports Codex data folders, approximate sizes, and likely project folders.

## Important Notes For AI Agents

- Codex data and project files are separate. Always ask whether project folders should be included.
- The package can contain sensitive data: conversations, logs, memories, generated images, local paths, and, in `full-with-secrets`, auth files or tokens.
- Do not restore browser cookies, Login Data, Local Storage, `.env`, API keys, or private keys by default.
- On Windows, old Mac paths in previous conversations may not resolve. Reopen the matching project folder from its new Windows path.
- If the Windows app fails to start after restore, remove stale `SingletonLock`, `SingletonCookie`, and `SingletonSocket` files under `%APPDATA%\Codex`.
- If login state does not transfer, ask the user to log in again. This is expected.
