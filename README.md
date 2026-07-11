# Codex Rehome（Codex 搬家）

Codex Rehome 是一个迁移 Codex Desktop 本地工作台的开源 Skill。它支持 Windows ↔ Mac、同系统换机，以及重装系统前后的备份恢复。

它迁移的是 Codex 对话、项目记录、Skills、Plugins、本地索引、生成物，以及你指定的项目文件。项目文件不属于 Codex 本地数据，需要明确选择后才会加入迁移包。

## 最简单的用法

1. **旧电脑**：把本仓库链接交给 Codex，告诉它使用 Codex Rehome 打包需要迁移的数据。
2. **传输**：把生成的压缩包传到新电脑。
3. **新电脑**：把压缩包和本仓库链接交给 Codex，让它恢复数据、更新路径、重新打开项目并检查结果。

一句话版本：旧电脑上的 Codex 按 Codex Rehome 的规则打包；你负责传输压缩包；新电脑上的 Codex 按同一套规则恢复和检查。

这不是官方云同步。跨系统迁移后，旧对话可以继续作为历史上下文；真正继续工作时，建议从恢复后的项目文件夹重新打开。

## 小红书 Red Skill

这个仓库同时提供中文优先的 Red Skill「Codex 搬家」。用户安装后，只需要告诉 Agent 自己处于“原电脑”“新电脑”还是“准备重装系统”，Agent 会按对应阶段继续。

- Red Skill 源文件：[redskill/SKILL.md](redskill/SKILL.md)
- 可直接分发的压缩包：`codex-rehome-redskill.zip`
- 构建脚本：[scripts/build_redskill_package.ps1](scripts/build_redskill_package.ps1)

重新生成小红书上传目录：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build_redskill_package.ps1
```

输出内容：

- `dist/redskill/codex-rehome/`：优先上传到小红书创作服务平台的 Skill 文件夹。
- `dist/redskill/codex-rehome-redskill.zip`：同内容的备份压缩包。
- `dist/redskill/redskill-upload.json`：建议填写的名称、简介、原创声明、仓库地址和标签。

这里的公开 Red Skill 包只包含说明和脚本，不包含用户对话、项目或登录信息。真正迁移时由旧电脑生成的私人迁移 ZIP 可能包含个人数据，只能私下传给新电脑，绝对不要上传到 Red Skill、GitHub 或公开笔记。

## 重要预期

跨系统迁移后，历史对话通常可以迁过去并继续查看；项目文件也可以恢复到新电脑的新路径。但是旧对话框里绑定的原始工作目录不一定能原样继续执行，尤其是 Windows 路径和 macOS 路径完全不同。

更稳妥的做法是：把旧对话当作历史记录和上下文参考，在新电脑上从恢复后的项目文件夹重新打开一个项目对话继续工作。

## 重装系统场景

如果你是在同一台电脑上重装 Windows，而且 Codex 和项目主要在 C 盘，迁移包一定不要只放在桌面、下载、`C:\Users\...` 或任何会被重装系统清掉的位置。

重装前让当前系统里的 Codex/AI 做这几件事：

1. 用 `codex-rehome` 打包 Codex 数据和需要保留的项目文件夹。
2. 把输出目录指定到非系统盘或移动硬盘，例如 `D:\Codex-Rehome-Backup`、`E:\Codex-Rehome-Backup`、`F:\Codex-Rehome-Backup`。
3. 同时保存这个 GitHub 地址、README 截图，或一段给新系统 Codex 的恢复说明文字。
4. 确认 zip、校验文件、manifest 和恢复说明都在非 C 盘后，再重装系统。

Windows 打包时可以这样指定输出位置：

```powershell
.\codex-rehome\scripts\create_windows_codex_migration_package.ps1 `
  -Project "$env:USERPROFILE\Documents\New project" `
  -Out "D:\Codex-Rehome-Backup"
```

重装后，先安装并登录 Codex，再把备份目录里的 zip 和恢复说明交给新系统里的 Codex/AI，让它解压、恢复、注册项目并运行 verifier。

可以把下面这段文字和 zip 一起保存到非 C 盘，重装后直接发给新系统里的 Codex：

```text
我刚重装完系统。这个目录里有我重装前用 codex-rehome 生成的 Codex 迁移包。
请使用 https://github.com/CalebYcj/codex-rehome 的最新说明，帮我解压这个 zip，
运行 Windows 恢复脚本 Restore-Codex-To-Windows.ps1 -RestoreProjects，
把项目恢复到 Documents\Codex-Restored-Projects，注册/打开恢复后的项目，
最后运行 Verify-Codex-Windows-Restore.ps1 -Json 并告诉我哪些对话、skills、plugins、项目恢复成功。
不要恢复 auth token、浏览器 cookies、.env、私钥、node_modules、.git 或虚拟环境。
```

## English Overview

Codex Rehome is an open-source Codex skill for moving OpenAI Codex Desktop between macOS and Windows computers. It helps package and restore Codex conversations, sessions, memories, skills, plugins, MCP/connectors, generated images, project folders, path mappings, and restore verification scripts.

Use this project when you need to migrate Codex Desktop from Mac to Windows, Windows to Mac, Windows to Windows, Mac to Mac, or preserve Codex before reinstalling the same computer's operating system; back up Codex conversations and sessions; restore Codex skills and plugins; or hand off a local AI agent workspace to another computer.

A Chinese-first Red Skill distribution package is available as `codex-rehome-redskill.zip`. Its source is under `redskill/`, and `scripts/build_redskill_package.ps1` rebuilds the upload folder and metadata. The public Red Skill package contains instructions and scripts only; never upload a user-generated private migration ZIP to Red Skill or a public repository.

## Core Lesson

Restoring Codex project conversations is not just copying `.codex` files. A visible project restore has four layers:

1. File layer: sessions, `session_index.jsonl`, skills, plugins, SQLite state, generated artifacts, and project folders.
2. Path mapping layer: source paths must become target paths in SQLite rows and session JSONL metadata.
3. Index layer: conversations must be discoverable through Codex's thread index/state, including `state_*.sqlite.threads`, `rollout_path`, `cwd`, titles, and timestamps.
4. App registration layer: Codex Desktop must register/open the restored workspace through its own entry point.

Do not treat UI project recovery as a manual JSON/SQLite patching problem. On Mac, the durable app-visible registration step is:

```bash
/Applications/Codex.app/Contents/Resources/codex app <restored-project-path>
```

Windows needs the same class of official open-workspace operation. The Windows restore script now attempts `codex app <restored-project-path>` after `-RestoreProjects`; if packaged-app permissions block that CLI call, reopen the restored folder from Codex Desktop and rerun the verifier.

## Plain User Flow

You can use this project by giving the GitHub link or a screenshot of this README to the AI/Codex on each computer.

1. Old computer: ask the AI to use `codex-rehome` to package your old Codex data. The AI should help you choose which conversations and project folders to include, then create a private zip file.
2. Transfer: send that zip to yourself through Feishu, cloud drive, external disk, LAN share, or another private channel. Do not publish the zip.
3. New computer: install and log in to Codex first. Then give the zip to the AI/Codex on the new computer. The AI should unzip it, run the restore script, map old paths to the new computer, merge the conversation index, restore project folders, and register/open the projects in Codex Desktop.
4. Check: the AI should run the verifier and tell you whether sessions, selected chats, projects, forbidden files, and app-visible project registration look correct.

In short: old computer AI packages, you move the zip, new computer AI restores and checks.

## Quick Links

- [Mac/Windows migration guide](docs/migrate-codex-between-mac-and-windows.md)
- [Mac to Windows migration guide](docs/migrate-codex-from-mac-to-windows.md)
- [Backup Codex conversations and sessions](docs/backup-codex-conversations-and-sessions.md)
- [Restore Codex skills, plugins, and projects](docs/restore-codex-skills-plugins-and-projects.md)
- [Troubleshooting Codex migration issues](docs/troubleshooting.md)
- [Feature status](docs/validation-status.md)
- [AI-readable project summary](docs/llms.txt)

## AI Agent Quick Start

If you are an AI agent helping a user migrate Codex, read this first:

```text
Use the skill in codex-rehome/. Identify the source OS and target OS, package the source computer's Codex data, choose standard/full/full-with-secrets mode, include project folders when needed, transfer the package privately, run the target OS restore script after Codex is closed, then run the matching verifier.

Default restore is merge-safe: it adds migrated sessions, skills, plugins, generated images, and session index entries while preserving the target machine's login/config identity files. Destructive full replacement requires an explicit `--replace-codex-home` or `-ReplaceCodexHome` flag.
```

Primary use cases:

- Mac to Windows Codex migration
- Windows to Mac Codex migration
- Windows to Windows Codex migration
- Mac to Mac Codex migration
- Codex conversation and session transfer
- Codex memories, skills, plugins, automations, and generated image transfer
- Project folder and old dialogue reproduction
- AI agent workspace continuity across computers
- Claude Code to Codex project handoff
- Claude Desktop agent history detection
- Feishu, cloud drive, external disk, or GitHub handoff

Search keywords:

```text
Codex migration, Codex Mac to Windows, Codex Windows to Mac,
Codex Windows to Windows, Codex Mac to Mac, migrate Codex conversations,
Codex skills backup, Codex memory transfer, Codex project handoff,
AI agent workspace migration, OpenAI Codex desktop migration,
Codex session transfer, Codex backup restore, Codex Desktop Windows restore,
Mac Codex to Windows Codex, Windows Codex to Mac Codex,
Codex generated images migration, Codex MCP connector migration,
Codex plugin restore, Claude Code to Codex, Claude to Codex handoff,
Claude Code migration, Claude Desktop agent history, Codex Agent Bridge
```

This repository contains a Codex skill for migrating Codex data, conversations, memories, skills, plugins, and project context between computers, including Mac to Windows, Windows to Mac, Windows to Windows, and Mac to Mac.

## What This Repository Is

This repository is both an agent-readable skill and a small script toolkit:

- `SKILL.md` is the main instruction file for Codex or another AI agent. It tells the agent when to use this workflow, what to migrate, what to exclude, and how to report results.
- `scripts/` contains executable helpers that package, restore, inventory, and verify the migration.
- `references/` contains extra path-mapping notes that an agent can load when needed.
- `README.md`, `README.zh-CN.md`, and `docs/` are for humans, GitHub visitors, search engines, and AI search/GEO.

So the skill is not just a shell script. It is a reusable agent workflow with scripts attached for the parts that should be deterministic.

The main skill lives at:

```text
codex-rehome/
```

Use this skill when a user asks to move Codex to another computer, preserve prior conversations, reproduce a project workspace, package Codex data on Mac or Windows, restore it on Mac or Windows, or hand off the migration package through Feishu, cloud drive, GitHub, or an external disk.

## Contents

```text
codex-rehome/
  SKILL.md
  agents/openai.yaml
  references/path-map.md
  scripts/create_mac_codex_migration_package.sh
  scripts/create_windows_codex_migration_package.ps1
  scripts/restore_codex_to_windows.ps1
  scripts/restore_codex_to_mac.sh
  scripts/collect_mac_codex_inventory.sh
  scripts/collect_windows_codex_inventory.ps1
  scripts/verify_windows_codex_restore.ps1
  scripts/verify_mac_codex_restore.sh
  scripts/inspect_claude_agent_sources.py
  scripts/export_claude_to_codex_handoff.py
  scripts/verify_agent_bridge_handoff.py
```

There is also a backup archive:

```text
codex-rehome-skill.zip
```

## Install For Codex

Copy the whole `codex-rehome` folder into one of these locations:

```text
~/.codex/skills/codex-rehome
```

or, for a project-local skill:

```text
<project>/.agents/skills/codex-rehome
```

Then start a new Codex thread and ask:

```text
Use $codex-rehome to migrate Codex from my old computer to my new computer.
```

## What Gets Migrated

This skill can help package and restore:

- Codex conversations and sessions
- Codex memories and goals
- Codex skills and plugins
- Codex config and app state
- generated images and local artifacts
- environment inventory and path mapping
- optional project folders needed to reopen old conversations
- Claude Code / Claude Desktop agent transcripts as a Codex-readable handoff package, when real local transcript files exist

Project folders are not automatically part of Codex data. Always decide whether to include them. On Mac restores, pass `--restore-projects` to copy packaged projects into `~/Documents/Codex-Restored-Projects`.

On Mac, schema v3 restore registers restored project folders with Codex Desktop by running `/Applications/Codex.app/Contents/Resources/codex app <restored-project-path>`. This is the observed durable path for making restored projects appear in the app-visible project list; editing `.codex-global-state.json` alone is not enough because the running app can overwrite it on quit.

## Known Limits / What This Cannot Guarantee

This project tries to restore the useful local Codex workspace, but some things are intentionally not promised:

- Old chat windows may not keep a live working-directory handle after a cross-OS move. The conversation text and session history can be restored, but a thread that originally worked inside `C:\...` may not be able to keep editing that same folder after it becomes `/Users/...` on Mac. Use the restored old conversation as context, then reopen the restored project folder in Codex and continue in a new project thread when needed.
- Project files are only included when they are passed with `--project` or `-Project`. Codex history and project source files are separate.
- App sidebar visibility is not guaranteed by file copy alone. The restore scripts attempt Codex Desktop project registration with `codex app <restored-project-path>`, but if the desktop app or packaged-app permissions block it, the user must manually open the restored folder from Codex Desktop.
- Login state and browser sessions are not migrated by default. Expect to log in again to Codex, GitHub, Feishu, Gmail, browser extensions, or MCP connectors.
- Secrets are excluded by default: auth files, tokens, cookies, `.env`, private keys, browser Login Data, Local Storage, `.git`, `node_modules`, and virtual environments.
- Native dependencies are not portable. Reinstall or rebuild `node_modules`, Python virtualenvs, compiled binaries, game/tool runtimes, and OS-specific dependencies on the target computer.
- Running processes, open terminal sessions, unsaved editor buffers, in-memory app state, and live GUI layout are not migrated.
- Cross-account or cross-workspace restoration can be incomplete. A conversation restored under a different Codex/OpenAI/GitHub account may be visible as local history but still need fresh authorization to access remote services.
- The verifier can check files, indexes, path mapping, selected chats, forbidden files, and best-effort project registration. It cannot guarantee that every old thread will resume exactly as if it had never moved computers.

中文边界说明：这个工具能尽量把“对话记录、索引、skills、plugins、生成物、项目文件夹和路径映射”搬过去，但不能保证旧聊天框里的原工作目录句柄跨系统复活。尤其是 Win 转 Mac / Mac 转 Win 时，旧对话更适合作为历史上下文；真正继续干活时，建议在新电脑上重新打开恢复后的项目文件夹，再开新对话接着做。

## Documentation

| Guide | What it answers |
|---|---|
| [How to migrate Codex between Mac and Windows](docs/migrate-codex-between-mac-and-windows.md) | Direction picker for Mac to Windows, Windows to Mac, Windows to Windows, and Mac to Mac |
| [How to migrate OpenAI Codex Desktop from Mac to Windows](docs/migrate-codex-from-mac-to-windows.md) | End-to-end Mac packaging, transfer, Windows restore, and verification |
| [How to back up Codex conversations and sessions](docs/backup-codex-conversations-and-sessions.md) | Where Codex stores JSONL sessions, thread SQLite state, memories, and generated images |
| [How to restore Codex skills, plugins, and projects](docs/restore-codex-skills-plugins-and-projects.md) | How to restore skills, plugin cache, generated files, and project folders |
| [Troubleshooting Codex migration](docs/troubleshooting.md) | Socket files, vendor imports, Git object permission errors, path mapping, and login issues |
| [Feature status](docs/validation-status.md) | Supported migration directions, default exclusions, and target verification steps |

## Migration Modes

```text
standard
  Default. Migrates Codex core data, sessions, memories, skills, plugins,
  generated images, selected app state, and project folders. Excludes secrets,
  browser login state, .env files, private keys, sockets, .git, node_modules,
  and virtualenvs.

full
  Includes standard data plus logs, caches, and environment inventory.
  Still excludes secrets and browser login state.

full-with-secrets
  Includes auth/token/env/login-state files only when explicitly requested.
  Requires --i-understand-secrets or -IUnderstandSecrets. Treat the package
  like a password vault.
```

## Source Mac Workflow

Run the Mac package script from Terminal, ideally after closing Codex:

```bash
cd /path/to/codex-rehome
bash scripts/create_mac_codex_migration_package.sh \
  --project "$HOME/Documents/New project"
```

To highlight specific conversations for UI-readiness checks:

```bash
bash scripts/create_mac_codex_migration_package.sh \
  --project "$HOME/Documents/New project" \
  --selected-chat "$HOME/.codex/sessions/2026/06/18/rollout-example.jsonl"
```

For a fuller inventory without secrets:

```bash
bash scripts/create_mac_codex_migration_package.sh \
  --mode full \
  --project "$HOME/Documents/New project"
```

The output is a zip on the Mac Desktop by default.

## Source Windows Workflow

Run the Windows package script from PowerShell, ideally after closing Codex:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\codex-rehome\scripts\create_windows_codex_migration_package.ps1 `
  -Project "$env:USERPROFILE\Documents\New project"
```

For a fuller inventory without secrets:

```powershell
.\codex-rehome\scripts\create_windows_codex_migration_package.ps1 `
  -Mode full `
  -Project "$env:USERPROFILE\Documents\New project"
```

The output is a zip on the Windows Desktop by default.

## Claude Code To Codex Bridge

This is for users who used Claude Code or Claude Desktop agent mode and want Codex to continue from that work.

First inspect local Claude sources:

```powershell
python .\codex-rehome\scripts\inspect_claude_agent_sources.py --json
```

If the result says `exportable_transcripts_found`, export a handoff:

```powershell
python .\codex-rehome\scripts\export_claude_to_codex_handoff.py `
  --source "<claude-jsonl-file-or-session-directory>" `
  --out "$env:USERPROFILE\Documents" `
  --title "Claude To Codex Handoff"
```

Then verify:

```powershell
python .\codex-rehome\scripts\verify_agent_bridge_handoff.py "<handoff-folder>" --json
```

Open the generated handoff folder in Codex and ask Codex to read `next-steps-for-codex.md` first.

Important boundary: if the detector reports `installed_but_no_entitled_claude_code_sessions`, Claude is installed but the current account does not have usable Claude Code sessions. On a free Claude account this commonly appears as `Claude Code requires a Pro or Max subscription`. In that case, there is no real local Claude Code history to migrate yet.

## Restore On Windows

On Windows:

1. Install Codex and open it once.
2. Close Codex completely.
3. Unzip the migration package.
4. Open PowerShell inside the unzipped folder.
5. Run:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\Restore-Codex-To-Windows.ps1 -RestoreProjects
```

The restore script backs up existing Windows Codex data, restores packaged project folders to `%USERPROFILE%\Documents\Codex-Restored-Projects` by default, imports schema v3 UI-ready metadata when present, and then merges migrated data into the target. It preserves target `auth.json`, `config.toml`, `installation_id`, `models_cache.json`, and `chrome-native-hosts-v2.json`. Use `-ProjectsDir <dir>` to choose a custom project destination, `-ReplaceCodexHome` only when you intentionally want a destructive full replacement, and `-ReplaceState` only when you want to overwrite existing state/memory/goal databases.

Then verify:

```powershell
.\Verify-Codex-Windows-Restore.ps1 -Json
```

## Restore On Mac

On Mac:

1. Install Codex and open it once.
2. Close Codex completely.
3. Unzip the migration package.
4. Open Terminal inside the unzipped folder.
5. Run:

```bash
bash ./Restore-Codex-To-Mac.sh --restore-projects
```

Then verify:

```bash
bash ./Verify-Codex-Mac-Restore.sh --json
```

The Mac and Windows verifiers report file-level restore plus schema v3 UI-ready data layers. For selected chats, readiness requires session files, `session_index.jsonl`, `state_*.sqlite.threads`, existing `rollout_path`, target `cwd` path mapping, remapped session JSONL metadata, no old source path left in selected JSONL files, and restored project paths in `.codex-global-state.json`.

## Inventory Helpers

To inspect a Windows machine before or after migration:

```powershell
powershell -ExecutionPolicy Bypass -File .\codex-rehome\scripts\collect_windows_codex_inventory.ps1
```

To inspect a Mac machine before or after migration:

```bash
bash ./codex-rehome/scripts/collect_mac_codex_inventory.sh
```

These inventory scripts report Codex data folders, approximate sizes, and likely project folders.

## Important Notes For AI Agents

- Codex data and project files are separate. Always ask whether project folders should be included.
- The package can contain sensitive data: conversations, logs, memories, generated images, local paths, and, in `full-with-secrets`, auth files or tokens.
- Do not restore browser cookies, Login Data, Local Storage, `.env`, API keys, or private keys by default.
- Restore scripts merge by default. Do not use `--replace-codex-home` / `-ReplaceCodexHome` unless the user explicitly accepts overwriting the target Codex home.
- Do not overwrite `state_*.sqlite`, `memories_*.sqlite`, or `goals_*.sqlite` by default. Use `--replace-state` / `-ReplaceState` only when replacing target state is intentional.
- Schema v3 restores prepare UI-ready project/thread data and invoke or attempt `codex app <restored-project-path>` for project registration. If the verifier reports `app_project_registration_ready=false`, run that command manually for each restored project path, or reopen the restored project folder from Codex Desktop.
- After a cross-OS restore, old absolute paths in previous conversations may not resolve. Reopen the matching project folder from its new target path.
- If the Windows app fails to start after restore, remove stale `SingletonLock`, `SingletonCookie`, and `SingletonSocket` files under `%APPDATA%\Codex`.
- If login state does not transfer, ask the user to log in again. This is expected.
- For Claude Code Bridge, never claim success unless real local transcript files were found and a handoff folder passed `verify_agent_bridge_handoff.py`.

## FAQ

### How do I migrate OpenAI Codex Desktop from Mac to Windows?

Run `scripts/create_mac_codex_migration_package.sh` on the Mac, transfer the generated zip to Windows, close Codex on Windows, run `Restore-Codex-To-Windows.ps1 -RestoreProjects`, then run `Verify-Codex-Windows-Restore.ps1 -Json`.

### Can this migrate Windows to Mac, Windows to Windows, or Mac to Mac?

Yes. Package on the source OS with `create_mac_codex_migration_package.sh` or `create_windows_codex_migration_package.ps1`, then restore on the target OS with `Restore-Codex-To-Mac.sh --restore-projects` or `Restore-Codex-To-Windows.ps1 -RestoreProjects`.

### Can this migrate Codex conversations and sessions?

Yes. The standard mode packages Codex session JSONL files, archived sessions, thread state SQLite files, memories, goals, generated images, skills, plugins, selected app state, and project folders passed with `--project` or `-Project`.

### Does it migrate Codex memories, skills, plugins, and generated images?

Yes. It includes memory databases, user skills, plugin cache/manifests, and generated images under `.codex` when those files exist on the source computer.

### Does it migrate secrets or login state?

Not by default. `standard` and `full` modes exclude auth tokens, browser cookies, Login Data, Local Storage, `.env` files, private keys, sockets, `.git`, `node_modules`, and virtual environments. The target computer should log in again manually.

### Is this an official OpenAI tool?

No. This is an independent open-source Codex skill and script toolkit for local migration workflows. It is designed to help users and AI agents handle Codex Desktop data carefully.

### What keywords describe this project?

OpenAI Codex Desktop migration, Codex Mac to Windows, Codex Windows to Mac, Codex Windows to Windows, Codex Mac to Mac, Codex conversations backup, Codex sessions restore, Codex skills migration, Codex plugin restore, AI agent workspace handoff, Mac Windows Codex transfer.
