# Codex ReHome

[中文](README.md) | [English](README.en.md) | [Download ReHome Desktop](https://github.com/CalebYcj/codex-rehome/releases)

Move selected Codex Desktop projects, conversations, Skills, Plugins, and generated artifacts between computers through a local, offline migration package.

> Arrived from an older “Codex ReHome Skill” video?
>
> The original workflow asked Codex to run the migration through an Agent Skill. That workflow is now available as **ReHome Desktop** for everyday use. The original Skill remains available for automation and troubleshooting.

[Download ReHome Desktop](https://github.com/CalebYcj/codex-rehome/releases) · [Open Codex ReHome Skill](https://github.com/CalebYcj/codex-rehome-skill)

## Move in three steps

1. **Source computer**: Open ReHome Desktop, choose projects, conversations, and other content, then create a `.rehome` file.
2. **Transfer**: Move the file privately by cloud drive, messaging, LAN, or external storage.
3. **Target computer**: Install and sign in to Codex once, then fully quit Codex. Open ReHome Desktop, import the file, confirm the restore, and reopen Codex afterward.

The installer and a migration package are different files. The EXE or DMG installs ReHome Desktop; a `.rehome` file carries selected data between computers.

## What it can move

- Selected projects and their files
- Selected conversations and the local indexes needed for Codex to rediscover them
- Skills (including `.codex/skills` and shared `.agents/skills`), Plugins, and generated images
- Relevant local state and path mappings for selected content

Project files and conversation history are separate. Selecting a conversation does not automatically include source files. Selecting a project includes its child conversations by default, while still allowing individual conversations to be deselected.

## Supported scenarios

- Windows to Windows
- Windows to macOS
- macOS to Windows
- macOS to macOS
- Backup and restore around an operating-system reinstall on the same computer

ReHome Desktop is currently in beta. See [validation status](docs/validation-status.md) for real-world coverage and known boundaries.

## Privacy and system impact

ReHome Desktop keeps migration offline. It requires no additional account, uploads no migration data, installs no system service, adds no autostart entry, and requests no administrator access. At launch it contacts GitHub Releases to check for a newer version. A failed check never blocks migration, and downloading or installing an update requires user confirmation.

Packages exclude login tokens, cookies, `.env` files, private keys, `.git`, `node_modules`, virtual environments, and runtime lock files by default. Never upload a personal `.rehome` file to GitHub, a public post, or a public download link.

## Important limits

This is not official cloud sync and it does not automatically keep two computers synchronized each day. After a cross-platform move, an old conversation can remain useful historical context while its original working-directory handle no longer works. Reopen the restored project, then continue in a new task when needed.

Each `.rehome` package, individual file, and single Codex conversation can currently be up to 16 GiB. Large files are streamed during creation, inspection, and restore. If a conversation exceeds that limit, split it or leave it unselected.

Login sessions, browser state, running terminals, unsaved work, and native system dependencies are not fully portable. Different accounts or workspaces may require fresh sign-in or authorization for external services.

## Need the Skill instead?

[Codex ReHome Skill](https://github.com/CalebYcj/codex-rehome-skill) keeps the original Agent workflow, scripts, Red Skill, batch automation, and troubleshooting tools. It is for advanced users; ReHome Desktop is the recommended entry point for routine migration.

## Install and help

Starting with `v0.1.4`, ReHome Desktop can check, verify, and install signed updates inside the app. Users on `v0.1.3` or earlier must install one final release manually. The updater signature prevents tampered update packages; it is separate from paid Apple or Windows publisher signing, so the operating system may still show an unknown-developer warning.

The interface starts in Chinese. Click `English` in the sidebar; ReHome remembers the choice on this device.

- [Chinese installation guide](docs/desktop-install.md)
- [English installation guide](docs/desktop-install.en.md)
- [Validation status](docs/validation-status.md)
- [Security](SECURITY.md)

## Development and license

ReHome Desktop lives in `desktop/`. ReHome Core and Codex Bridge are bundled with the app; nothing else needs to be installed. Licensed under [MIT](LICENSE).
