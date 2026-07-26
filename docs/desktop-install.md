# ReHome Desktop 安装说明

[English](desktop-install.en.md)

ReHome Desktop 是 Codex Rehome 的离线桌面版。它把 ReHome Core 和 Codex Bridge 一起打包进应用，不需要另外安装插件、CLI、Python、WSL 或数据库工具。

## 下载

从 [GitHub Releases](https://github.com/CalebYcj/codex-rehome/releases) 下载与你的电脑对应的文件：

- Windows：`ReHome-Desktop_*_x64-setup.exe`
- macOS：`ReHome-Desktop_*_universal.dmg`，同时支持 Intel 和 Apple Silicon

安装器和迁移文件不是同一种东西。安装器用于安装 ReHome Desktop；`.rehome` 或旧版 ReHome ZIP 才是两台电脑之间传递的数据包。

## Windows

1. 双击 EXE，按提示安装。
2. 安装范围仅为当前 Windows 用户，不需要管理员权限。
3. 打开 ReHome Desktop。应用会自动查找当前用户的 Codex 数据；如果 Codex 安装在自定义位置，再手动选择目录。

## macOS

1. 打开 DMG，把 ReHome Desktop 拖入 Applications。
2. 第一次打开若被 macOS 拦截，进入“系统设置 > 隐私与安全性”，确认打开该应用；也可以在 Finder 中按住 Control 点击应用并选择“打开”。
3. 应用会自动查找当前用户的 Codex 数据；如果使用了自定义目录，再手动选择。

当前公开构建未使用 Apple Developer ID 签名，因此第一次打开可能出现系统提醒。这不影响离线迁移功能。

## 使用方式

- 旧电脑：选择“发送”，选择要迁移的项目和对话，生成 `.rehome` 文件。
- 用移动硬盘、局域网、网盘或私人聊天工具把该文件传到新电脑。
- 新电脑：先安装并登录 Codex，再选择“接收”，检查恢复计划并确认执行。
- 恢复默认采用合并方式。应用会备份目标状态、映射 Windows/macOS 路径、合并对话索引，并通过 Codex 官方项目入口注册恢复的项目。

## 对系统的影响

ReHome Desktop 只在用户确认后读取或写入以下位置：

- 当前用户的 Codex 数据目录，例如 `~/.codex`
- 用户选择的项目目录、迁移包和项目恢复目录
- ReHome Desktop 自己的事务备份与恢复记录

它不安装系统服务，不设置开机启动，不请求管理员权限，不启用自动更新，也不会自动上传数据。登录令牌、Cookies、`.env`、私钥、`.git`、`node_modules`、虚拟环境和运行时锁文件默认不会进入迁移包。

卸载 ReHome Desktop 不会自动删除 Codex 数据、迁移包、已恢复项目或事务备份；这些仍由用户自行保留或删除。
