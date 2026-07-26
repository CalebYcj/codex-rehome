# ReHome Desktop installation

[中文](desktop-install.md)

ReHome Desktop is the offline desktop edition of Codex Rehome. ReHome Core and Codex Bridge are bundled with the app; no separate plugin, CLI, Python, WSL, or database tool is required.

## Download

Download the matching file from [GitHub Releases](https://github.com/CalebYcj/codex-rehome/releases):

- Windows: `ReHome-Desktop_*_x64-setup.exe`
- macOS: `ReHome-Desktop_*_universal.dmg`, for both Intel and Apple Silicon

An installer and a migration package are different files. The installer installs ReHome Desktop. A `.rehome` file or a legacy ReHome ZIP carries data between computers.

## Windows

1. Open the EXE and follow the installer.
2. Installation is limited to the current Windows user and does not require administrator rights.
3. Open ReHome Desktop. It automatically detects the current user's Codex data. Choose a custom directory only when Codex uses a nonstandard location.

## macOS

1. Open the DMG and drag ReHome Desktop to Applications.
2. If macOS blocks the first launch, allow it in System Settings > Privacy & Security. You can also Control-click the app in Finder and choose Open.
3. The app automatically detects the current user's Codex data. Choose a custom directory only when necessary.

Public builds are not signed with an Apple Developer ID, so macOS may show a first-launch warning. Offline migration still works normally.

## Workflow

- Old computer: choose Send, select projects and conversations, and create a `.rehome` file.
- Transfer that file privately by external drive, local network, cloud drive, or private messaging.
- New computer: install and sign in to Codex first, choose Receive, review the restore plan, and confirm.
- Restore is merge-safe by default. The app backs up destination state, maps Windows/macOS paths, merges conversation indexes, and registers restored projects through Codex's official project entry point.

## System impact

ReHome Desktop reads or writes only after confirmation, and only within:

- the current user's Codex data directory, such as `~/.codex`;
- user-selected project, package, and restore directories;
- ReHome Desktop's own transaction backups and recovery records.

It installs no system service, adds no autostart entry, requests no administrator rights, enables no automatic updater, and uploads no data automatically. Login tokens, cookies, `.env` files, private keys, `.git`, `node_modules`, virtual environments, and runtime lock files are excluded from packages by default.

Uninstalling ReHome Desktop does not automatically remove Codex data, migration packages, restored projects, or transaction backups. The user remains in control of those files.
