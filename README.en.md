# Codex Rehome

[中文](README.md) | [English](README.en.md)

Move a local Codex Desktop workspace between Windows and macOS computers, including conversations, project records, Skills, Plugins, generated artifacts, and selected project folders.

Supported directions:

- Mac to Windows
- Windows to Mac
- Windows to Windows
- Mac to Mac
- Backup and restore around an operating-system reinstall on the same computer

This is not official cloud sync. It is an open-source migration Skill and script toolkit for Codex or another capable local agent.

## Quick start

### 1. Old computer: package

Install and sign in to Codex, then send it this message:

```text
Use this Skill:
https://github.com/CalebYcj/codex-rehome

I want to move Codex from this computer to another computer. Confirm the source and target operating systems, let me select the conversations and project folders to include, then create a migration package. Exclude login data, cookies, environment files, private keys, .git, node_modules, and virtual environments by default.
```

Codex inventories local data, waits for your selection, and creates a private migration ZIP.

### 2. Transfer the ZIP

Move the ZIP through a private channel such as cloud storage, a local network, an external disk, or a private messaging service. Never publish a user-generated migration ZIP in this repository, a Red Skill, or a public post.

### 3. New computer: restore

Install and sign in to Codex first. Give the ZIP and repository URL to Codex on the new computer:

```text
This is a migration package created with Codex Rehome on my old computer.
Use the latest workflow from https://github.com/CalebYcj/codex-rehome. Perform a merge-safe restore, restore project folders, map source paths to this computer, reopen/register the projects in Codex Desktop, then run the verifier and report the result.
```

## What it moves

- Codex conversations, sessions, and archived sessions
- memories, goals, and local indexes
- Skills, Plugins, and selected application state
- generated images and local artifacts
- project folders explicitly selected by the user
- path mappings, manifests, checksums, and target verification scripts

Codex history and project source files are separate. Select project folders explicitly when the destination computer needs the source files.

## Merge-safe restore

- The default is a merge-safe restore, not replacement of the destination `~/.codex` directory.
- Destination authentication, configuration, and installation identity files are preserved.
- Full Codex-home replacement and state database replacement require explicit destructive flags.
- Authentication data, cookies, browser Login Data, `.env`, private keys, `.git`, `node_modules`, virtual environments, sockets, and runtime files are excluded by default.
- The destination state is backed up before restore, and a verifier checks the result afterward.

## Why project registration matters

A visible project restore has four layers:

1. Copy sessions, indexes, Skills, Plugins, state, artifacts, and project files.
2. Map source paths to valid destination paths.
3. Make restored conversations discoverable through the thread index and state.
4. Reopen/register restored workspaces through Codex Desktop's official project entry.

File copy alone does not guarantee sidebar visibility. The restore workflow attempts `codex app <restored-project-path>` for each restored project. If operating-system permissions block it, reopen the restored folder manually in Codex Desktop and rerun verification.

## Important limits

- After a cross-OS move, an old conversation may retain its historical text while losing a live handle to its original working directory.
- The safest continuation is to keep the restored conversation as context, reopen the restored project folder, and continue in a new project task.
- Login sessions, browser state, running processes, terminals, unsaved buffers, native dependencies, and live window layouts are not portable.
- Cross-account or cross-workspace restores may require fresh authorization.
- Passing structural verification does not guarantee that every old conversation can immediately continue editing without reopening its project.

## Reinstalling the same computer

Before reinstalling Windows, store the migration package on a non-system partition or external disk that will survive the reinstall, such as `D:\Codex-Rehome-Backup`. Do not leave the only copy on Desktop, Downloads, the user profile, or the C drive.

Keep the ZIP, manifest, checksum, repository URL, and restore instructions together. After reinstalling, install and sign in to Codex before giving it the backup directory.

## Red Skill

The repository also includes the Chinese-first “Codex 搬家” Red Skill:

- Source: [redskill/SKILL.md](redskill/SKILL.md)
- Distribution archive: `codex-rehome-redskill.zip`
- Build script: [scripts/build_redskill_package.ps1](scripts/build_redskill_package.ps1)

The public Red Skill contains instructions and scripts only. A migration ZIP created from a user's computer remains private.

## Install for Codex

Give the repository URL directly to Codex, or place the `codex-rehome/` folder at:

```text
~/.codex/skills/codex-rehome
```

For project-local installation:

```text
<project>/.agents/skills/codex-rehome
```

The complete agent workflow is in [codex-rehome/SKILL.md](codex-rehome/SKILL.md).

## Documentation

- [Choose a Mac/Windows migration direction](docs/migrate-codex-between-mac-and-windows.md)
- [Mac to Windows end-to-end guide](docs/migrate-codex-from-mac-to-windows.md)
- [Back up conversations and sessions](docs/backup-codex-conversations-and-sessions.md)
- [Restore Skills, Plugins, and projects](docs/restore-codex-skills-plugins-and-projects.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Validation status](docs/validation-status.md)
- [AI-readable project summary](docs/llms.txt)

For same-computer Claude Code project and Session handoff, use [Claude Codex Handoff](https://github.com/CalebYcj/claude-codex-handoff).

## License

MIT
