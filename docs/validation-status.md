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

Restore scripts use merge restore by default. They add packaged sessions, archived sessions, skills, plugin cache, generated images, and `session_index.jsonl` entries while preserving target login/config identity files. Full `~/.codex` replacement requires `--replace-codex-home` on Mac or `-ReplaceCodexHome` on Windows. State database replacement requires `--replace-state` or `-ReplaceState`.

## Target Verification

After restoring, run the verifier for the target OS:

```powershell
.\Verify-Codex-Windows-Restore.ps1
```

or:

```bash
bash ./Verify-Codex-Mac-Restore.sh --json
```

Then open Codex, check recent threads, and reopen migrated project folders from their new target paths.

For Mac restores with `selected_chats/`, verifier readiness requires selected chat IDs to exist in restored `~/.codex/sessions`, `~/.codex/session_index.jsonl`, and target `state_*.sqlite.threads`. It also checks that rollout paths exist, thread cwd values point to restored Mac project paths, selected JSONL metadata has been path-mapped, old Windows source paths are gone from selected JSONL files, and restored projects are present in `.codex-global-state.json`.

Data-layer readiness is not the same as live desktop frontend readiness. On Mac, project folders must be registered through the bundled Codex CLI with `codex app <restored-project-path>`; hand-editing `.codex-global-state.json` alone can be overwritten by the running desktop app. Current schema v3 restore invokes `/Applications/Codex.app/Contents/Resources/codex app <restored-project-path>` after project restore and records the result in `codex-rehome-project-registration-report.json`.

## Platform Notes

Intel Mac and Apple Silicon should not affect the core Codex data migration because architecture-specific dependency folders and binary-heavy runtime paths are excluded by default. Reinstall or rebuild project dependencies such as `node_modules`, virtual environments, compiled artifacts, or native tools on the target machine.

Project folders are packaged under `projects/`. Mac restores can copy them automatically with `Restore-Codex-To-Mac.sh --restore-projects`, which defaults to `~/Documents/Codex-Restored-Projects`; pass `--projects-dir <dir>` to choose another location.

Windows-to-Mac packages are generated with schema version 3, forward-slash zip entries, LF/no-BOM `SHA256SUMS.txt`, generated `session_index.jsonl` when needed, metadata exports for thread rows/path mapping/project UI registry, and both text and JSON manifests. Mac verification checks package checksums, selected chats when present, `session_index.jsonl` readiness, SQLite thread readiness, path mapping readiness, restored project folders, global project registry readiness, and forbidden-file counts.
