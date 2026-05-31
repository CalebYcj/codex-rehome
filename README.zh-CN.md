# Codex Migration Handoff

把 Codex 从 Mac 迁移到 Windows，并尽量保留历史对话、记忆、skills、plugins、自动化、生成图片和项目上下文。

这是一个公开的 Codex skill，面向需要“换电脑但不想丢 Codex 上下文”的用户，也面向帮助用户执行迁移的 AI agent。

## 给 AI Agent 的快速说明

如果你是另一个 AI agent，正在帮用户迁移 Codex，请先读这一段：

```text
使用 codex-migration-handoff/ 这个 skill。先在源电脑打包 Codex 数据，必要时包含项目文件夹；通过私密渠道传输迁移包；在 Windows 上关闭 Codex 后运行恢复脚本。
```

适用场景：

- Codex 从 Mac 迁移到 Windows
- 转移 Codex 历史对话和会话上下文
- 备份/恢复 Codex memories、skills、plugins、自动化、生成图片
- 复现旧项目文件夹和旧对话环境
- 通过飞书、网盘、外接硬盘、GitHub 进行迁移包交接

搜索关键词：

```text
Codex 迁移, Codex Mac 转 Windows, Codex 历史对话迁移,
Codex skill 备份, Codex memory 转移, Codex 项目交接,
AI agent 工作区迁移, OpenAI Codex 桌面端迁移
```

这个仓库包含一个 Codex skill，用来把 Codex 的数据、历史对话、记忆、skills、plugins、自动化、生成图片，以及项目上下文从一台电脑迁移到另一台电脑。最典型的场景是：从 Mac 迁移到 Windows。

主要 skill 位于：

```text
codex-migration-handoff/
```

当用户提出以下需求时，应该使用这个 skill：

- 把 Codex 从 Mac 转移到 Windows
- 保留 Codex 历史对话和会话上下文
- 迁移 Codex memories、skills、plugins、自动化、生成图片
- 复现某个项目文件夹和旧对话环境
- 把迁移包通过飞书、网盘、GitHub、外接硬盘等方式交接给另一台电脑

## 仓库内容

```text
codex-migration-handoff/
  SKILL.md
  agents/openai.yaml
  references/path-map.md
  scripts/create_mac_codex_migration_package.sh
  scripts/restore_codex_to_windows.ps1
  scripts/collect_windows_codex_inventory.ps1
```

仓库里还保留了一份完整压缩包备份：

```text
codex-migration-handoff-skill.zip
```

## 安装到 Codex

把整个 `codex-migration-handoff` 文件夹复制到 Codex 的 skills 目录：

```text
~/.codex/skills/codex-migration-handoff
```

也可以作为项目级 skill 放到：

```text
<项目目录>/.agents/skills/codex-migration-handoff
```

然后新开一个 Codex 对话，直接说：

```text
Use $codex-migration-handoff to migrate Codex from my Mac to my Windows computer.
```

或者中文说：

```text
使用 $codex-migration-handoff，帮我把 Mac 上的 Codex 数据、对话和项目迁移到 Windows。
```

## 会迁移哪些东西

这个 skill 可以帮助打包和恢复：

- Codex 历史对话和 sessions
- Codex memories 和 goals
- Codex skills 和 plugins
- Codex 配置和应用状态
- 生成图片和本地 artifacts
- 为了重开旧对话所需的项目文件夹

注意：项目文件夹不属于 Codex 自身数据，需要单独决定是否一起打包。

## Mac 端打包流程

建议先完全退出 Mac 上的 Codex，再在终端里运行：

```bash
cd /path/to/codex-migration-handoff
bash scripts/create_mac_codex_migration_package.sh \
  --project "$HOME/Documents/New project"
```

脚本会打包：

- `~/.codex`
- Codex 在 `~/Library/Application Support` 下的应用数据
- 可选缓存目录
- 通过 `--project` 传入的项目文件夹
- Windows 端恢复脚本
- 清单和校验文件

默认输出是一个放在 Mac 桌面的 zip 迁移包。

如果要包含多个项目，可以多次传入 `--project`：

```bash
bash scripts/create_mac_codex_migration_package.sh \
  --project "$HOME/Documents/New project" \
  --project "$HOME/Documents/Another project"
```

## Windows 端恢复流程

在 Windows 上：

1. 先安装 Codex。
2. 打开一次 Codex，然后完全退出。
3. 解压 Mac 端生成的迁移包。
4. 在解压后的文件夹里打开 PowerShell。
5. 运行：

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\Restore-Codex-To-Windows.ps1
```

恢复脚本会先备份 Windows 上已有的 Codex 数据目录，再复制迁移数据。

## Windows 端盘点

如果需要检查 Windows 电脑上已有的 Codex 数据和可能的项目目录，可以运行：

```powershell
powershell -ExecutionPolicy Bypass -File .\codex-migration-handoff\scripts\collect_windows_codex_inventory.ps1
```

它会输出：

- Windows 上 Codex 数据目录是否存在
- 各目录大致大小
- Documents 下可能的项目文件夹

## 路径对应关系

Mac 端主要数据：

```text
~/.codex
~/Library/Application Support/Codex
~/Library/Application Support/com.openai.codex
~/Library/Application Support/OpenAI/Codex
```

Windows 端对应位置：

```text
%USERPROFILE%\.codex
%APPDATA%\Codex
%APPDATA%\com.openai.codex
%APPDATA%\OpenAI\Codex
```

项目文件夹不属于 Codex 数据，需要单独复制或通过 `--project` 放进迁移包。

例如：

```text
Mac:     /Users/caleb/Documents/New project
Windows: C:\Users\Administrator\Documents\New project
```

## 给 AI Agent 的注意事项

- 不要只迁移 Codex 数据，必须确认用户是否还要迁移项目文件夹。
- 迁移包可能包含敏感信息，包括登录文件、历史对话、记忆、日志、生成图片和本地路径。
- Windows 恢复后，旧对话里引用的 Mac 绝对路径可能无法直接使用，需要在 Windows 上重新打开对应项目目录。
- 如果 Windows 上 Codex 启动异常，可以关闭 Codex 后删除 `%APPDATA%\Codex` 下的 `SingletonLock`、`SingletonCookie`、`SingletonSocket`。
- 登录态不一定能跨系统迁移。如果 Codex 要求重新登录，这是正常情况。
- 如果用户要通过飞书、网盘、GitHub 传迁移包，要明确告诉用户这个包包含私人数据。
