# ReHome Desktop Beta Validation

ReHome Desktop is in public beta. A migration is only complete after the package is verified, restored, and the restored project is reopened in Codex Desktop.

## 0.1.34 compressed-history and WAL coverage / 本轮验证

Windows local checks passed 307 core tests and 76 UI tests. Candidate CI passed 307 Windows / 298 macOS core tests and 76 UI tests on each OS, with 7 deliberate ignored tests per OS. On Intel macOS 12.7.6, Codex Desktop 26.727.51351 with bundled CLI 0.146.0-alpha.9.2 passed 23 isolated core/native checks, including 10 WAL regressions. Windows-source and Mac-source legacy/paginated packages preserved both original messages, the same thread ID and mapped paths, initially and after app-server restart. Actual Mac GUI compressed-source export, corrupt/truncated input protection and public-summary privacy were checked separately.

These fixtures contain synthetic canonical conversations in valid Zstandard frames, not a reporter's private data. GUI apply, current-build sidebar visibility, model continuation and physical Apple Silicon execution were not performed in this round. The app is not Apple distribution-signed/notarized; a signed updater is an integrity check, not OS publisher signing. Keep these boundaries separate from successful core/native restoration.

本轮 Windows 本地 307 项核心、76 项界面测试通过，候选两端 CI 分别通过 307/298 项核心和各 76 项界面测试；每端另有 7 项有意忽略。Intel/macOS 12.7.6、Codex Desktop 26.727.51351 与 CLI 0.146.0-alpha.9.2 的 23 项隔离核心/原生检查及 10 项 WAL 回归通过，四目标初次和重启后都读回原消息、ID 与本地路径。实际 GUI 压缩源导出、坏输入保护及公开摘要脱敏另行通过。未验证本轮 GUI 导入、侧栏、模型续聊或 Apple Silicon 实机；未签名/公证提示仍可能出现。

| Direction | Current coverage | Remaining beta boundary |
|---|---|---|
| Windows → Windows | Real-source isolated acceptance covers package creation, restore, checksums, conversations, indexes, SQLite threads, path mapping, project files, and exclusions. | A second physical Windows machine and its live sidebar registration still need final release acceptance. |
| Mac → Mac | Earlier project registration and current isolated compressed export, native restore/restart and WAL rollback have passed on Intel macOS. | Current-build GUI apply/sidebar/model continuation and physical Apple Silicon execution remain unverified. |
| Windows → macOS | Current Windows-source legacy/paginated packages passed isolated native restoration/restart on Intel macOS; actual App package previews also passed. | GUI apply/sidebar/model continuation remain unverified in this round. |
| macOS → Windows | Package compatibility, target restore logic and Mac-root fixture native restoration/restart are covered on Windows. | A current-build package exported on a physical Mac and applied through the Windows App remains unverified. |

## What the app verifies

- Package checksums and required files
- Selected conversation files and session indexes
- SQLite thread records and target-path mapping
- Selected project files and default exclusions
- Best-effort project registration through Codex Desktop

## What still needs a user check

Open Codex after a restore. Confirm the project is visible and reopen it if needed. Historical conversation text can be restored while an old task's original working-directory handle no longer works after a cross-platform move. Continue from the restored project in a new task when necessary.

Do not treat a successful file copy alone as a successful migration.
