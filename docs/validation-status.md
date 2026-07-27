# ReHome Desktop Beta Validation

ReHome Desktop is in public beta. A migration is only complete after the package is verified, restored, and the restored project is reopened in Codex Desktop.

| Direction | Current coverage | Remaining beta boundary |
|---|---|---|
| Windows → Windows | Real-source isolated acceptance covers package creation, restore, checksums, conversations, indexes, SQLite threads, path mapping, project files, and exclusions. | A second physical Windows machine and its live sidebar registration still need final release acceptance. |
| Mac → Mac | Real package, isolated restore, and Codex project registration have passed on Intel macOS. | Apple Silicon native run remains a release check. |
| Windows → macOS | Package compatibility and target restore logic are covered. | Final current-build physical Windows-to-Mac App run remains required before stable release. |
| macOS → Windows | Package compatibility and target restore logic are covered. | Final current-build physical Mac-to-Windows App run remains required before stable release. |

## What the app verifies

- Package checksums and required files
- Selected conversation files and session indexes
- SQLite thread records and target-path mapping
- Selected project files and default exclusions
- Best-effort project registration through Codex Desktop

## What still needs a user check

Open Codex after a restore. Confirm the project is visible and reopen it if needed. Historical conversation text can be restored while an old task's original working-directory handle no longer works after a cross-platform move. Continue from the restored project in a new task when necessary.

Do not treat a successful file copy alone as a successful migration.
