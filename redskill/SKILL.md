---
name: codex-rehome
description: Use when 用户需要在 Mac 与 Windows 之间迁移、备份或恢复 Codex Desktop，包括换电脑、离职交接、同机重装系统、恢复历史对话与项目、迁移 skills/plugins/generated images，或遇到迁移后项目和对话未出现在 Codex 左侧栏的情况。
---

# Codex Rehome

帮助普通用户把 Codex Desktop 的历史对话、索引、skills、plugins、生成物和指定项目文件夹迁移到另一台电脑，或在重装系统前备份并在重装后恢复。

支持：

- Mac -> Windows
- Windows -> Mac
- Windows -> Windows
- Mac -> Mac
- 同一台 Windows 电脑重装系统前备份、重装后恢复

## 第一原则

先判断用户现在处于哪个阶段，再行动。不要一上来丢一串命令。

1. **原电脑**：盘点、选择项目与对话、打包、验证 ZIP。
2. **传输中**：让用户把 ZIP 私下传到新电脑。
3. **新电脑**：安装并登录 Codex、解压、合并恢复、映射路径、注册项目、验证。
4. **重装系统**：重装前把 ZIP 放到不会被清空的非系统盘或移动硬盘；重装后按新电脑阶段恢复。

每次先用一句人话告诉用户：现在在哪个阶段、接下来需要做什么。

## 开始前只确认这些

- 源系统和目标系统是什么。
- 是换电脑、离职交接，还是同机重装系统。
- 哪些项目文件夹需要带走。项目文件夹不会因为复制 Codex 数据而自动包含。
- 是否要重点选择某些对话；不确定时默认迁移全部标准 Codex 数据。
- ZIP 准备通过飞书、网盘、局域网、移动硬盘还是其他私密方式传输。

默认使用 `standard` 模式。除非用户明确要求并理解风险，不要迁移 auth token、浏览器登录态、Cookies、`.env`、API key、私钥或其他 secrets。

## 原电脑：打包

先定位本 Skill 的目录，然后调用其中的脚本。可以让用户选择多个项目和重点对话。

### Windows 原电脑

```powershell
.\scripts\create_windows_codex_migration_package.ps1 `
  -Project "C:\path\to\project-one" `
  -Project "C:\path\to\project-two"
```

重点对话可以重复添加：

```powershell
  -SelectedChat "C:\Users\<user>\.codex\sessions\...\rollout-....jsonl"
```

同机重装 Windows 时，必须指定非系统盘或移动硬盘：

```powershell
.\scripts\create_windows_codex_migration_package.ps1 `
  -Project "C:\path\to\project" `
  -Out "D:\Codex-Rehome-Backup"
```

不要把重装备份只放在 Desktop、Downloads、`%USERPROFILE%` 或任何会随 `C:` 被清空的位置。

### Mac 原电脑

```bash
bash ./scripts/create_mac_codex_migration_package.sh \
  --project "/path/to/project-one" \
  --project "/path/to/project-two"
```

重点对话可以重复添加：

```bash
  --selected-chat "$HOME/.codex/sessions/.../rollout-....jsonl"
```

### 打包后必须报告

- ZIP 的完整路径和大小。
- 使用的模式。
- 包含了哪些项目。
- sessions、skills、plugin manifests、generated images、SQLite 文件数量。
- manifest、checksum 是否存在并通过。
- 敏感文件排除结果。
- 目标电脑应该运行哪条恢复命令。

## 传输

ZIP 可能包含私人对话、记忆、日志、生成图片和本地路径，只能私下传输，不要上传到公开 GitHub 仓库或公开帖子。

同机重装系统时，“传输”指把 ZIP、manifest、checksum 和恢复说明保存在重装后仍存在的非系统盘或移动硬盘，并在重装前确认文件真实存在。

## 新电脑：恢复

先让用户安装并登录 Codex。恢复前关闭 Codex Desktop；需要重新登录属于正常情况。

默认使用 merge restore，不要整体替换目标 `.codex`。保留目标电脑的 `auth.json`、`config.toml`、`installation_id`、`models_cache.json` 和 `chrome-native-hosts-v2.json`。

### 恢复到 Windows

在解压后的迁移包目录运行：

```powershell
.\Restore-Codex-To-Windows.ps1 -RestoreProjects
.\Verify-Codex-Windows-Restore.ps1 -Json
```

如被执行策略阻止，只对当前 PowerShell 进程临时放行：

```powershell
Set-ExecutionPolicy -Scope Process Bypass
```

### 恢复到 Mac

在解压后的迁移包目录运行：

```bash
bash ./Restore-Codex-To-Mac.sh --restore-projects
bash ./Verify-Codex-Mac-Restore.sh --json
```

恢复脚本会把项目复制到默认的 `Documents/Codex-Restored-Projects`，并调用或尝试调用 `codex app <restored-project-path>`，让 Codex Desktop 正式打开和注册项目。

不要把手写 `.codex-global-state.json` 当作项目注册完成。项目未出现在左侧栏时，使用 Codex Desktop 手动打开恢复后的项目文件夹，再运行 verifier。

## 验收

至少检查：

- sessions 与 archived sessions
- `session_index.jsonl`
- selected chats 是否同时存在于 sessions、session index 和 SQLite threads
- skills、plugin manifests、generated images、SQLite 文件
- 项目文件夹是否复制成功
- 旧路径是否映射成目标路径
- `codex app` 项目注册是否成功
- forbidden files 总数是否为 0

只有文件、路径、索引和应用注册都满足时，才能说迁移准备完成。

## 必须提前说明的限制

- 这不是 OpenAI 官方云同步。
- 历史对话通常可以恢复查看，但跨系统后旧聊天框不一定继续拥有原项目目录的可用工作句柄。
- 更稳妥的续接方式是：把旧对话当作历史上下文，从恢复后的项目文件夹重新打开项目对话继续工作。
- 项目源码必须通过 `--project` 或 `-Project` 明确加入迁移包。
- 登录态、浏览器 session、远程服务授权通常需要重新登录。
- `.git`、`node_modules`、Python venv、编译产物和系统原生依赖默认不迁移，需要在目标电脑重新安装或构建。
- 打开的终端、运行中的进程、未保存内容、窗口布局和内存状态不能迁移。
- verifier 能验证文件、索引、路径和项目注册，但不能保证每个旧线程像从未换过电脑一样原地继续执行。

## 破坏性参数

只有用户明确要求并理解会覆盖目标数据时，才允许：

- Mac：`--replace-codex-home`、`--replace-state`
- Windows：`-ReplaceCodexHome`、`-ReplaceState`

真实恢复前始终先备份目标 `.codex` 和 Codex app profile。
