# Codex Rehome（Codex 搬家）

[中文](README.md) | [English](README.en.md)

把 Codex Desktop 的对话、项目记录、Skills、Plugins、生成物和你选择的项目文件，从一台电脑搬到另一台电脑。

支持：

- Windows → Mac
- Mac → Windows
- Windows → Windows
- Mac → Mac
- 同一台电脑重装系统前后的备份与恢复

这不是官方云同步。它是一套给 Codex/AI 使用的迁移 Skill 和脚本工具。

## 最简单的用法

### 1. 原电脑：打包

安装并登录 Codex，把下面这段话发给它：

```text
请使用这个 Skill：
https://github.com/CalebYcj/codex-rehome

我要把这台电脑上的 Codex 搬到另一台电脑。
请先确认源系统和目标系统，让我选择需要迁移的对话和项目，然后生成迁移压缩包。默认排除登录信息、Cookies、.env、私钥、.git、node_modules 和虚拟环境。
```

Codex 会检查本机数据、让你选择内容并生成私人迁移 ZIP。

### 2. 传输：移动 ZIP

使用飞书、网盘、局域网、移动硬盘或其他私人方式，把 ZIP 传到新电脑。不要把私人迁移包上传到 GitHub、Red Skill 或公开帖子。

### 3. 新电脑：恢复

先安装并登录 Codex，再把 ZIP 和仓库链接交给新电脑上的 Codex：

```text
这是旧电脑用 Codex Rehome 生成的迁移包。
请使用 https://github.com/CalebYcj/codex-rehome 的最新流程恢复数据，默认采用合并恢复，恢复项目文件夹，更新跨系统路径，重新打开项目，最后运行验证器并报告结果。
```

## 会迁移什么

- Codex conversations、sessions 和 archived sessions
- memories、goals 和本地索引
- Skills、Plugins 和部分应用状态
- generated images 和本地生成物
- 用户明确选择的项目文件夹
- 为目标电脑准备的路径映射、manifest、checksum 和验证脚本

项目文件与 Codex 历史是两类数据。需要项目源码时，必须在原电脑上明确选择项目文件夹。

## 默认安全策略

- 默认合并恢复，不整体替换目标电脑的 `~/.codex`。
- 保留目标电脑现有登录状态、配置和安装身份。
- 只有用户明确使用危险参数时，才允许整目录替换或覆盖状态数据库。
- 默认排除 auth/token、Cookies、Login Data、`.env`、私钥、`.git`、`node_modules`、虚拟环境、socket 和运行时文件。
- 恢复前自动备份目标端现有 Codex 数据，恢复后运行验证器。

## 重要限制

- 跨系统后，旧对话可以作为历史上下文，但旧聊天框绑定的原工作目录不一定还能继续操作。
- 项目文件恢复后，需要通过 Codex Desktop 的正式入口重新打开/注册。只复制文件不等于左侧栏已经恢复。
- 更稳妥的续接方式是：保留旧对话作为上下文，从恢复后的项目文件夹重新打开一个新任务继续工作。
- 登录状态、浏览器会话、正在运行的进程、终端、未保存内容和系统原生依赖不会完整迁移。
- 跨账号或跨 workspace 可能需要重新授权。

## 重装系统

同一台 Windows 电脑重装系统时，迁移包必须保存到不会被格式化的非系统盘或移动硬盘，例如 `D:\Codex-Rehome-Backup`。不要只放在桌面、下载目录、用户目录或 C 盘。

重装前让 Codex 把 ZIP、manifest、checksum、GitHub 地址和恢复说明一起放到备份目录。重装后先安装并登录 Codex，再把整个备份目录交给它恢复。

## Red Skill

仓库包含中文优先的「Codex 搬家」Red Skill：

- 源文件：[redskill/SKILL.md](redskill/SKILL.md)
- 可分发包：`codex-rehome-redskill.zip`
- 构建脚本：[scripts/build_redskill_package.ps1](scripts/build_redskill_package.ps1)

公开 Red Skill 只包含说明和脚本，不包含用户数据。原电脑生成的迁移 ZIP 始终是私人文件。

## 安装到 Codex

把仓库链接直接交给 Codex，或把 `codex-rehome/` 文件夹放到：

```text
~/.codex/skills/codex-rehome
```

也可以放到项目级目录：

```text
PROJECT_PATH/.agents/skills/codex-rehome
```

Agent 的完整执行规则位于 [codex-rehome/SKILL.md](codex-rehome/SKILL.md)。

## For AI Agents（给 AI Agent）

以 [codex-rehome/SKILL.md](codex-rehome/SKILL.md) 为执行真源。先判断用户处于原电脑、传输、新电脑还是重装系统阶段，并确认源/目标系统。默认使用 standard 模式和合并恢复；项目文件需单独选择，恢复后必须重新打开/注册项目并运行验证器。不要默认迁移登录信息或其他敏感文件。

## 详细文档

- [四种 Mac/Windows 迁移方向](docs/migrate-codex-between-mac-and-windows.md)
- [Mac → Windows 完整流程](docs/migrate-codex-from-mac-to-windows.md)
- [备份 conversations 和 sessions](docs/backup-codex-conversations-and-sessions.md)
- [恢复 Skills、Plugins 和项目](docs/restore-codex-skills-plugins-and-projects.md)
- [故障排查](docs/troubleshooting.md)
- [功能验证状态](docs/validation-status.md)
- [AI 可读项目摘要](docs/llms.txt)

Claude Code → Codex 的同电脑项目与 Session 接手已独立为 [Claude Codex Handoff](https://github.com/CalebYcj/claude-codex-handoff)。

## License

MIT
