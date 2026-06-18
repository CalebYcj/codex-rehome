# Codex Rehome - 在 Mac 和 Windows 之间迁移 OpenAI Codex Desktop

Codex Rehome 是一个开源 Codex skill，用来在 macOS 和 Windows 电脑之间迁移 OpenAI Codex Desktop。它帮助用户和 AI agent 打包、传输、恢复并验证 Codex 对话、sessions、记忆、skills、plugins、MCP/connectors、生成图片、项目文件夹、路径映射和本地协作现场。

它支持四种方向：

- Mac 转 Windows
- Windows 转 Mac
- Windows 转 Windows
- Mac 转 Mac

如果你正在搜索「Codex Mac 迁移 Windows」「Codex Windows 迁移 Mac」「Codex 对话迁移」「Codex sessions 备份恢复」「Codex skill 迁移工具」，这个项目就是为这个场景做的。

在 GitHub 里直接搜索 `codex-rehome` 就能找到。

这个项目不是普通的文件备份工具，而是一个面向 AI agent 的 Codex skill：目标是迁移“用户和 AI 的协作现场”。

## 核心经验

恢复 Codex 的“项目 + 对话”不是单纯复制 `.codex` 文件。真正能让左侧栏显示项目，至少要过四层：

1. 文件层：复制 sessions、`session_index.jsonl`、skills、plugins、SQLite 状态、生成物和项目文件夹。
2. 路径映射层：把源机器路径改成目标机器路径，不只改 SQLite，也要改 session JSONL 里的 `session_meta`、`turn_context`、workspace roots 等。
3. 索引层：让对话进入 Codex 的 thread index/state，包括 `state_*.sqlite.threads`、`rollout_path`、`cwd`、标题、时间戳和 `session_index.jsonl`。
4. 应用注册层：让 Codex Desktop 通过自己的打开项目入口正式注册/打开恢复后的 workspace。

不要把 UI 项目恢复当成 JSON/SQLite 手工修复问题。Mac 上已经验证过真正有效的应用注册入口是：

```bash
/Applications/Codex.app/Contents/Resources/codex app <恢复后的项目路径>
```

Windows 端也应该优先寻找同类官方打开 workspace 机制，例如 `codex app <恢复后的项目路径>` 或 Codex Desktop 对应的 open project 入口。

## 快速入口

- [如何在 Mac 和 Windows 之间迁移 Codex](docs/migrate-codex-between-mac-and-windows.md)
- [如何把 OpenAI Codex Desktop 从 Mac 迁移到 Windows](docs/migrate-codex-from-mac-to-windows.md)
- [如何备份 Codex conversations 和 sessions](docs/backup-codex-conversations-and-sessions.md)
- [如何恢复 Codex skills、plugins 和项目文件](docs/restore-codex-skills-plugins-and-projects.md)
- [Codex 迁移故障排查](docs/troubleshooting.md)
- [AI 搜索友好的项目摘要](docs/llms.txt)

## 给 AI Agent 的快速说明

如果你是另一个正在帮用户迁移 Codex 的 AI agent，先读这一段：

```text
使用 codex-rehome/ 这个 skill。先确认源电脑和目标电脑分别是 Mac 还是 Windows；在源电脑打包 Codex 数据；选择 standard/full/full-with-secrets 模式；必要时包含项目文件夹；通过私密渠道传输迁移包；在目标电脑关闭 Codex 后运行对应系统的恢复脚本；最后运行对应验证脚本。
```

适用场景：

- Codex 从 Mac 迁移到 Windows
- Codex 从 Windows 迁移到 Mac
- Codex 从 Windows 迁移到 Windows
- Codex 从 Mac 迁移到 Mac
- 转移 Codex 历史对话、sessions 和记忆
- 备份或恢复 Codex skills、plugins、自动化、生成图片
- 复现旧项目文件夹和旧对话环境
- 迁移一个用户和 AI 的完整协作现场
- 通过飞书、网盘、外接硬盘、GitHub 进行迁移包交接

搜索关键词：

```text
Codex 迁移, Codex Mac 转 Windows, Codex Windows 转 Mac,
Codex Windows 转 Windows, Codex Mac 转 Mac, Codex 历史对话迁移,
Codex skill 备份, Codex memory 转移, Codex 项目交接,
AI agent 工作区迁移, OpenAI Codex 桌面端迁移,
Codex sessions 备份, Codex 对话恢复, Codex 插件迁移,
Codex skills 迁移, Codex generated images 迁移
```

## 这个仓库到底是什么

这个仓库同时是一个给 agent 读的 skill，也是一个小型脚本工具包：

- `SKILL.md` 是 Codex 或其他 AI agent 的入口说明。它告诉 agent 什么时候该用这个流程、该迁移什么、该排除什么、如何汇报结果。
- `scripts/` 里是真正执行打包、恢复、盘点和验证的脚本。
- `references/` 里是路径映射等补充资料，agent 需要时再读取。
- `README.md`、`README.zh-CN.md` 和 `docs/` 是给人、GitHub 访客、搜索引擎和 AI 搜索/GEO 看的说明。

所以它不是单纯的 shell 脚本，也不是单纯的说明书，而是“agent 工作流 + 可执行脚本”的组合。

## 仓库内容

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
```

## 安装到 Codex

把整个 `codex-rehome` 文件夹复制到：

```text
~/.codex/skills/codex-rehome
```

也可以作为项目级 skill 放到：

```text
<项目目录>/.agents/skills/codex-rehome
```

然后新开一个 Codex 对话，说：

```text
使用 $codex-rehome，帮我把旧电脑上的 Codex 数据、对话和项目迁移到新电脑。
```

## 会迁移哪些东西

这个 skill 可以帮助打包和恢复：

- Codex 历史对话和 sessions
- Codex memories 和 goals
- Codex skills 和 plugins
- Codex 配置和应用状态
- 生成图片和本地 artifacts
- 开发环境清单和路径映射
- 为了重开旧对话所需的项目文件夹

项目文件夹不属于 Codex 自身数据，需要单独决定是否一起打包。

恢复脚本默认采用 merge restore：把迁移包里的 sessions、archived sessions、skills、plugins、generated images 和 session_index 追加/合并到目标电脑现有 Codex 数据里。默认不会整体替换目标 `~/.codex` 或 `%USERPROFILE%\.codex`，也会保留目标电脑已有的登录和配置身份文件。

只有明确传入 `--replace-codex-home`（Mac）或 `-ReplaceCodexHome`（Windows）时，才会进行破坏性的整目录替换。默认也不会覆盖 `state_*.sqlite`、`memories_*.sqlite`、`goals_*.sqlite`；只有传入 `--replace-state` 或 `-ReplaceState` 才会覆盖这些状态数据库。

## 文档

| 文档 | 解决的问题 |
|---|---|
| [How to migrate Codex between Mac and Windows](docs/migrate-codex-between-mac-and-windows.md) | Mac 转 Windows、Windows 转 Mac、Windows 转 Windows、Mac 转 Mac 的方向选择 |
| [How to migrate OpenAI Codex Desktop from Mac to Windows](docs/migrate-codex-from-mac-to-windows.md) | 从 Mac 打包、传输、Windows 恢复到验证的完整流程 |
| [How to back up Codex conversations and sessions](docs/backup-codex-conversations-and-sessions.md) | Codex JSONL sessions、thread SQLite、memories、generated images 的位置 |
| [How to restore Codex skills, plugins, and projects](docs/restore-codex-skills-plugins-and-projects.md) | 恢复 skills、plugins、生成物和项目文件夹 |
| [Troubleshooting Codex migration](docs/troubleshooting.md) | socket、vendor_imports、Git object 权限、路径映射和登录态问题 |

## 三种迁移模式

```text
standard
  默认模式。迁移 Codex 核心数据、sessions、memories、skills、plugins、
  generated images、部分应用状态和项目文件夹。
  默认排除 secrets、浏览器登录态、.env、私钥、socket、.git、
  node_modules、虚拟环境等。

full
  在 standard 基础上增加日志、缓存和环境清单。
  仍然排除 secrets 和浏览器登录态。

full-with-secrets
  只有用户明确要求时才使用。会包含 auth/token/env/login-state 相关文件。
  Mac 脚本必须加 --i-understand-secrets，Windows 脚本必须加 -IUnderstandSecrets。
  这个包要像密码保险箱一样对待。
```

## Mac 端打包流程

建议先完全退出 Mac 上的 Codex，再在终端里运行：

```bash
cd /path/to/codex-rehome
bash scripts/create_mac_codex_migration_package.sh \
  --project "$HOME/Documents/New project"
```

如果需要更完整的环境清单，但不包含 secrets：

```bash
bash scripts/create_mac_codex_migration_package.sh \
  --mode full \
  --project "$HOME/Documents/New project"
```

脚本默认会在 Mac 桌面生成 zip。

## Windows 端打包流程

建议先完全退出 Windows 上的 Codex，再在 PowerShell 里运行：

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\codex-rehome\scripts\create_windows_codex_migration_package.ps1 `
  -Project "$env:USERPROFILE\Documents\New project"
```

如果需要更完整的环境清单，但不包含 secrets：

```powershell
.\codex-rehome\scripts\create_windows_codex_migration_package.ps1 `
  -Mode full `
  -Project "$env:USERPROFILE\Documents\New project"
```

脚本默认会在 Windows 桌面生成 zip。

## Windows 端恢复流程

在 Windows 上：

1. 先安装 Codex。
2. 打开一次 Codex，然后完全退出。
3. 解压迁移包。
4. 在解压后的文件夹里打开 PowerShell。
5. 运行：

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\Restore-Codex-To-Windows.ps1
```

然后运行验证：

```powershell
.\Verify-Codex-Windows-Restore.ps1
```

默认恢复会合并到现有 Codex 数据里，不会覆盖目标机器上的 `auth.json`、`config.toml`、`installation_id`、`models_cache.json`、`chrome-native-hosts-v2.json`。如果你真的想整目录替换，才使用 `-ReplaceCodexHome`。

## Mac 端恢复流程

在 Mac 上：

1. 先安装 Codex。
2. 打开一次 Codex，然后完全退出。
3. 解压迁移包。
4. 在解压后的文件夹里打开 Terminal。
5. 运行：

```bash
bash ./Restore-Codex-To-Mac.sh --restore-projects
```

然后运行验证：

```bash
bash ./Verify-Codex-Mac-Restore.sh --json
```

Mac verifier 会区分“文件已经复制过去”和“Codex 左侧栏索引是否准备好”。如果使用了 selected chats，必须同时看到 selected chat 存在于 `~/.codex/sessions`、`~/.codex/session_index.jsonl`、`state_*.sqlite.threads`，并且 `rollout_path` 指向真实存在的 Mac JSONL、`cwd` 指向恢复后的 Mac 项目路径、selected JSONL 里不再残留旧 Windows 项目路径、恢复项目已经写入 `.codex-global-state.json`，才算数据层 UI readiness 通过。

注意：只手写 `.codex-global-state.json` 不足以让项目稳定出现在左侧栏，因为正在运行的 Codex Desktop 退出时可能用旧的内存状态覆盖这个文件。schema v3 的 Mac 恢复脚本会在恢复项目后调用 `/Applications/Codex.app/Contents/Resources/codex app <恢复后的项目路径>`，这是目前验证过能让 Codex Desktop 正式注册/打开恢复项目的关键步骤。

## 路径对应关系

Mac 端主要数据：

```text
~/.codex
~/Library/Application Support/Codex
~/Library/Application Support/com.openai.codex
~/Library/Application Support/OpenAI/Codex
```

Windows 端主要数据：

```text
%USERPROFILE%\.codex
%APPDATA%\Codex
%APPDATA%\com.openai.codex
%APPDATA%\OpenAI\Codex
```

旧对话里可能会引用旧电脑的绝对路径。不要直接批量改写 JSONL sessions。更稳妥的做法是在 manifest 里记录路径映射，然后在目标电脑重新打开对应项目目录。

## 给 AI Agent 的注意事项

- 不要只迁移 Codex 数据，要确认用户是否还要迁移项目文件夹。
- 默认不要迁移浏览器 Cookies、Login Data、Local Storage、`.env`、API key、私钥。
- 默认恢复是 merge，不是 replace。不要使用 `--replace-codex-home` 或 `-ReplaceCodexHome`，除非用户明确接受覆盖目标 Codex home 的风险。
- 默认不要覆盖 `state_*.sqlite`、`memories_*.sqlite`、`goals_*.sqlite`。只有用户明确要求时才使用 `--replace-state` 或 `-ReplaceState`。
- schema v3 会准备项目/对话的 UI-ready 数据层，并在 Mac 上调用 `codex app <恢复后的项目路径>` 注册项目；如果 verifier 显示 `app_project_registration_ready=false`，需要手动执行同一条命令。
- 如果用户要求 `full-with-secrets`，必须明确提醒风险。
- 跨系统恢复后，旧对话里的绝对路径可能不能直接使用，需要在目标电脑重新打开对应项目目录。
- 如果 Windows 上 Codex 启动异常，可以关闭 Codex 后删除 `%APPDATA%\Codex` 下的 `SingletonLock`、`SingletonCookie`、`SingletonSocket`。
- 登录态不一定能跨系统迁移。如果 Codex、GitHub、Gmail、飞书或浏览器扩展要求重新登录，这是正常情况。

## FAQ

### 如何把 OpenAI Codex Desktop 从 Mac 迁移到 Windows？

在 Mac 上运行 `scripts/create_mac_codex_migration_package.sh` 生成迁移包，把 zip 传到 Windows，关闭 Windows Codex 后运行 `Restore-Codex-To-Windows.ps1`，最后运行 `Verify-Codex-Windows-Restore.ps1` 验证。

### 可以 Windows 转 Mac、Windows 转 Windows、Mac 转 Mac 吗？

可以。在源系统上用 `create_mac_codex_migration_package.sh` 或 `create_windows_codex_migration_package.ps1` 打包，然后在目标系统上用 `Restore-Codex-To-Mac.sh --restore-projects` 或 `Restore-Codex-To-Windows.ps1` 恢复。

### 这个工具能迁移 Codex 对话和 sessions 吗？

可以。standard 模式会打包 Codex session JSONL、archived sessions、thread state SQLite、memories、goals、generated images、skills、plugins，以及通过 `--project` 或 `-Project` 指定的项目文件夹。

### 能迁移 Codex memories、skills、plugins 和生成图片吗？

可以。它会迁移 `.codex` 里的 memory 数据库、用户 skills、plugin cache/manifests 和 generated images。

### 会迁移 secrets、token、浏览器登录态吗？

默认不会。`standard` 和 `full` 模式会排除 auth token、browser cookies、Login Data、Local Storage、`.env`、私钥、socket、`.git`、`node_modules` 和虚拟环境。目标电脑需要重新登录。

### 这是 OpenAI 官方工具吗？

不是。这是一个独立开源的 Codex skill 和脚本工具包，用来帮助用户和 AI agent 更安全地处理 Codex Desktop 本地迁移。
