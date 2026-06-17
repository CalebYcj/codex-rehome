# How to Restore Codex Skills, Plugins, and Projects

This guide explains how to restore Codex skills, plugin cache, generated images, and project folders after creating a migration package with `codex-rehome`.

For the full direction picker, see [How to migrate Codex between Mac and Windows](migrate-codex-between-mac-and-windows.md).

## What The Restore Scripts Copy

The generated package uses neutral folder names so the same package can be restored to Mac or Windows.

| Package folder | Windows destination | Mac destination |
|---|---|---|
| `home/.codex` | `%USERPROFILE%\.codex` | `~/.codex` |
| `appdata_roaming/Codex` | `%APPDATA%\Codex` | `~/Library/Application Support/Codex` |
| `appdata_roaming/com.openai.codex` | `%APPDATA%\com.openai.codex` | `~/Library/Application Support/com.openai.codex` |
| `appdata_roaming/OpenAI/Codex` | `%APPDATA%\OpenAI\Codex` | `~/Library/Application Support/OpenAI/Codex` |

Project folders are included under `projects/` in the migration package. Move or copy them to the desired project location on the target computer, then reopen that folder from Codex.

## Restore To Windows

After unzipping the package and closing Codex:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\Restore-Codex-To-Windows.ps1
.\Verify-Codex-Windows-Restore.ps1
```

## Restore To Mac

After unzipping the package and closing Codex:

```bash
bash ./Restore-Codex-To-Mac.sh
bash ./Verify-Codex-Mac-Restore.sh
```

## Path Mapping Notes

Old conversations may reference source-computer paths like:

```text
/Users/caleb/Documents/New project
C:\Users\Administrator\Documents\New project
```

On the target computer, reopen the matching project folder from its new location. Do not bulk-edit JSONL session files in place. Prefer restoring files first, reopening the target project folder, and letting Codex rebuild or map workspace context safely.
