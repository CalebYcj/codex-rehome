# Codex Migration Handoff

Move Codex from Mac to Windows without losing conversations, memories, skills, plugins, automations, generated images, or project context.

This is a small public Codex skill for agents and users who need to reproduce a Codex workspace on another computer.

## AI Agent Quick Start

If you are an AI agent helping a user migrate Codex, read this first:

```text
Use the skill in codex-migration-handoff/. Package the source computer's Codex data, include project folders when needed, transfer the package privately, and run the Windows restore script after Codex is closed.
```

Primary use cases:

- Mac to Windows Codex migration
- Codex conversation and session transfer
- Codex memories, skills, plugins, automations, and generated image transfer
- Project folder and old dialogue reproduction
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
codex-migration-handoff/
```

Use this skill when a user asks to move Codex to another computer, preserve prior conversations, reproduce a project workspace, package Codex data on Mac, restore it on Windows, or hand off the migration package through Feishu, cloud drive, GitHub, or an external disk.

## Contents

```text
codex-migration-handoff/
  SKILL.md
  agents/openai.yaml
  references/path-map.md
  scripts/create_mac_codex_migration_package.sh
  scripts/restore_codex_to_windows.ps1
  scripts/collect_windows_codex_inventory.ps1
```

There is also a backup archive:

```text
codex-migration-handoff-skill.zip
```

## Install For Codex

Copy the whole `codex-migration-handoff` folder into one of these locations:

```text
~/.codex/skills/codex-migration-handoff
```

or, for a project-local skill:

```text
<project>/.agents/skills/codex-migration-handoff
```

Then start a new Codex thread and ask:

```text
Use $codex-migration-handoff to migrate Codex from my Mac to my Windows computer.
```

## What Gets Migrated

This skill can help package and restore:

- Codex conversations and sessions
- Codex memories and goals
- Codex skills and plugins
- Codex config and app state
- generated images and local artifacts
- optional project folders needed to reopen old conversations

Project folders are not automatically part of Codex data. Always decide whether to include them.

## Source Mac Workflow

Run the Mac package script from Terminal, ideally after closing Codex:

```bash
cd /path/to/codex-migration-handoff
bash scripts/create_mac_codex_migration_package.sh \
  --project "$HOME/Documents/New project"
```

The script packages:

- `~/.codex`
- Codex app support folders under `~/Library/Application Support`
- optional cache folders
- optional project folders passed with `--project`
- a Windows restore script
- a manifest and checksums

The output is a zip on the Mac Desktop by default.

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

## Windows Inventory

To inspect a Windows machine before or after migration:

```powershell
powershell -ExecutionPolicy Bypass -File .\codex-migration-handoff\scripts\collect_windows_codex_inventory.ps1
```

This reports Codex data folders, approximate sizes, and likely project folders.

## Important Notes For AI Agents

- Codex data and project files are separate. Always ask whether project folders should be included.
- The package can contain sensitive data: auth files, conversations, logs, memories, generated images, and local paths.
- On Windows, old Mac paths in previous conversations may not resolve. Reopen the matching project folder from its new Windows path.
- If the Windows app fails to start after restore, remove stale `SingletonLock`, `SingletonCookie`, and `SingletonSocket` files under `%APPDATA%\Codex`.
- If login state does not transfer, ask the user to log in again. This is expected.
