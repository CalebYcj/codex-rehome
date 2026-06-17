# How to Back Up Codex Conversations and Sessions

Codex Desktop stores local conversations and agent state across JSONL session files, SQLite databases, generated image folders, skills, and plugin cache directories. This guide explains what the migration skill backs up and why those files matter.

## Important Codex Data Locations

| Path | Purpose |
|---|---|
| `~/.codex/sessions` | JSONL conversation session files |
| `~/.codex/archived_sessions` | Archived session JSONL files |
| `~/.codex/state_*.sqlite` | Thread and app state index |
| `~/.codex/memories_*.sqlite` | Codex memory database |
| `~/.codex/goals_*.sqlite` | Goal database |
| `~/.codex/generated_images` | Generated image files |
| `~/.codex/skills` | User and system skills |
| `~/.codex/plugins/cache` | Plugin bundles and manifests |
| `~/Library/Application Support/Codex` | Codex desktop app profile and selected state |
| `%USERPROFILE%\.codex` | Windows Codex sessions, skills, plugins, memories, and generated images |
| `%APPDATA%\Codex` | Windows Codex desktop app profile and selected state |

## Backup Command

Run standard mode on the source computer.

On Mac:

```bash
bash scripts/create_mac_codex_migration_package.sh \
  --mode standard \
  --project "$HOME/Documents/New project"
```

On Windows:

```powershell
.\codex-rehome\scripts\create_windows_codex_migration_package.ps1 `
  -Mode standard `
  -Project "$env:USERPROFILE\Documents\New project"
```

The generated package includes manifests, checksums, target restore scripts, and verification scripts.

## What Is Not Backed Up By Default

Standard mode intentionally excludes:

- `auth.json`
- Browser cookies and Login Data
- Local Storage and Session Storage
- `.env` files
- SSH private keys and API keys
- `.git`, `node_modules`, `.venv`, caches, sockets, and runtime files

This keeps the backup safer to transfer while still preserving the Codex collaboration workspace.

