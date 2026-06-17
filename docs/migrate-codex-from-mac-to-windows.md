# How to Migrate OpenAI Codex Desktop from Mac to Windows

This guide explains how to migrate OpenAI Codex Desktop from macOS to Windows using the `codex-rehome` skill. The workflow packages Codex conversations, sessions, memories, skills, plugins, generated images, selected app state, and project folders into a Windows-oriented migration zip.

For Windows to Mac, Windows to Windows, or Mac to Mac, start with [How to migrate Codex between Mac and Windows](migrate-codex-between-mac-and-windows.md).

## When To Use This Guide

Use this guide if you want to:

- Move Codex Desktop from Mac to Windows.
- Preserve Codex conversations and session history.
- Restore Codex memories, skills, plugins, and generated images.
- Reopen old project workspaces on a Windows computer.
- Hand off a Codex workspace through Feishu, cloud drive, external disk, or another private transfer channel.

## Source Mac Workflow

Clone or download the repository, then run the Mac package script:

```bash
cd codex-rehome
bash scripts/create_mac_codex_migration_package.sh \
  --mode standard \
  --project "$HOME/Documents/New project"
```

Add more project folders by repeating `--project`:

```bash
bash scripts/create_mac_codex_migration_package.sh \
  --mode standard \
  --project "$HOME/Documents/New project" \
  --project "$HOME/Documents/Another project"
```

The script writes a `Codex-Migration-Mac-Source-*.zip` file to the Mac Desktop by default.

## Transfer To Windows

Transfer both the migration zip and any checksum file through a private channel. Treat the package as private because it can contain conversation history, generated files, local paths, and memory data.

## Windows Restore Workflow

On Windows:

1. Install Codex.
2. Open Codex once, then close all Codex windows.
3. Unzip the migration package.
4. Open PowerShell in the unzipped folder.
5. Run:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\Restore-Codex-To-Windows.ps1
```

Then verify:

```powershell
.\Verify-Codex-Windows-Restore.ps1
```

## Standard Mode Safety

`standard` mode excludes auth tokens, browser cookies, Login Data, Local Storage, `.env` files, private keys, sockets, `.git`, `node_modules`, virtual environments, and runtime caches. Windows should log in again manually.

