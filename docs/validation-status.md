# Feature Status

`codex-rehome` supports moving Codex Desktop workspaces across Mac and Windows computers.

## Supported Directions

| Direction | Status | Source script | Target restore |
|---|---|---|---|
| Mac to Windows | Supported | `create_mac_codex_migration_package.sh` | `Restore-Codex-To-Windows.ps1` |
| Windows to Mac | Supported | `create_windows_codex_migration_package.ps1` | `Restore-Codex-To-Mac.sh` |
| Windows to Windows | Supported | `create_windows_codex_migration_package.ps1` | `Restore-Codex-To-Windows.ps1` |
| Mac to Mac | Supported | `create_mac_codex_migration_package.sh` | `Restore-Codex-To-Mac.sh` |

## What The Workflow Covers

The standard workflow is designed to migrate:

- Codex conversations and sessions
- archived sessions
- thread state SQLite files
- memories and goals
- user skills
- plugin cache and manifests
- generated images
- selected Codex app support state
- optional project folders
- package manifests, checksums, and restore verification helpers

## Default Safety

Standard mode excludes sensitive or machine-specific files by default:

- `auth.json`
- browser cookies and login databases
- Local Storage and Session Storage
- `.env` and `.env.*`
- private keys
- `.git`
- `node_modules`
- virtual environments
- socket, IPC, and singleton runtime files

The target computer should log in again manually when Codex, GitHub, browser integrations, Feishu, Gmail, or other external services request it.

## Target Verification

After restoring, run the verifier for the target OS:

```powershell
.\Verify-Codex-Windows-Restore.ps1
```

or:

```bash
bash ./Verify-Codex-Mac-Restore.sh
```

Then open Codex, check recent threads, and reopen migrated project folders from their new target paths.

## Platform Notes

Intel Mac and Apple Silicon should not affect the core Codex data migration because architecture-specific dependency folders and binary-heavy runtime paths are excluded by default. Reinstall or rebuild project dependencies such as `node_modules`, virtual environments, compiled artifacts, or native tools on the target machine.

Project folders are packaged under `projects/`. Restore scripts do not automatically move them into the target home directory; move or reopen them manually where you want to continue working.
