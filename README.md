# Codex ReHome

[中文](README.md) | [English](README.en.md) | [下载 ReHome Desktop](https://github.com/CalebYcj/codex-rehome/releases)

把 Codex Desktop 里你选择的项目、对话、Skills、Plugins 和生成内容，从一台电脑离线搬到另一台电脑。

> 从「Codex 搬家 Skill」的视频过来的？
>
> 之前需要把 Skill 发给 Codex，让 Agent 帮你执行迁移。现在这套流程已经做成 **ReHome Desktop**：普通用户直接使用 App 即可。旧 Skill 仍然保留，适合自动化和故障处理。

[下载 ReHome Desktop](https://github.com/CalebYcj/codex-rehome/releases) · [前往 Codex ReHome Skill](https://github.com/CalebYcj/codex-rehome-skill)

## 三步完成迁移

1. **原电脑**：打开 ReHome Desktop，选择要带走的项目、对话和其他内容，生成一个 `.rehome` 文件。
2. **传输**：通过网盘、私人聊天工具、局域网或移动硬盘，把 `.rehome` 文件传到新电脑。
3. **新电脑**：先安装并登录一次 Codex，然后完全退出 Codex。打开 ReHome Desktop，导入文件并确认恢复；完成后重新打开 Codex。

安装程序和迁移文件不是同一件东西：EXE 或 DMG 用来安装 ReHome Desktop；`.rehome` 用来携带你的数据。

## 可以迁移什么

- 选定项目及其项目文件
- 选定对话，以及对话在 Codex 中重新出现所需的本地索引
- Skills、Plugins 和生成图片
- 与选中内容相关的本地状态和路径映射

项目文件和对话历史是两类内容。只选对话时，项目源码不会自动被带走；选中项目时，默认会连同其子对话一起选择，但仍可以手动取消某条对话。

## 支持的场景

- Windows → Windows
- Windows → macOS
- macOS → Windows
- macOS → macOS
- 同一台电脑重装系统前后的备份与恢复

ReHome Desktop 当前处于 Beta。各方向的实际验收与已知边界见 [验证状态](docs/validation-status.md)。

## 隐私与系统影响

ReHome Desktop 是离线工具：不需要额外账号，不自动上传数据，不安装系统服务，不设置开机启动，也不需要管理员权限。

迁移包默认排除登录令牌、Cookies、`.env`、私钥、`.git`、`node_modules`、虚拟环境和运行时锁文件。不要把个人 `.rehome` 文件上传到 GitHub、公开帖子或任何公共下载链接。

## 需要知道的限制

这不是官方云同步，也不会让两台电脑每天自动保持一致。跨系统后，旧对话通常仍可作为历史上下文，但它原来绑定的工作目录可能不能继续直接使用。恢复完成后请重新打开项目；需要继续工作时，在恢复后的项目里开一个新任务最稳妥。

登录状态、浏览器会话、正在运行的终端、未保存内容和系统原生依赖不会完整迁移。使用不同账号或 workspace 时，外部服务可能需要重新登录或授权。

## 仍然想用 Skill？

[Codex ReHome Skill](https://github.com/CalebYcj/codex-rehome-skill) 保留了原来的 Agent 工作流、脚本、Red Skill、批量自动化和故障处理能力。它适合熟悉 Codex/终端的用户；普通迁移请优先使用本仓库的 Desktop App。

## 安装与帮助

- [中文安装说明](docs/desktop-install.md)
- [English installation guide](docs/desktop-install.en.md)
- [验证状态](docs/validation-status.md)
- [安全说明](SECURITY.md)

## 开发与许可证

ReHome Desktop 由本仓库的 `desktop/` 提供，ReHome Core 与 Codex Bridge 已内置，不需要额外安装。采用 [MIT License](LICENSE)。
