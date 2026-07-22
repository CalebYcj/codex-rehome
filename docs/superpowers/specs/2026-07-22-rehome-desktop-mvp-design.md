# ReHome Desktop MVP 设计

## 1. 目标

把现有 Codex Rehome Skill 和迁移脚本升级为一个普通用户可以独立使用的桌面应用。用户只安装 **ReHome Desktop**，不需要单独安装 ReHome Core、Codex Bridge、Python 环境或 Codex Skill。

首版完成一个可交付的跨平台闭环：

1. 在来源电脑检测 Codex 数据和用户选择的项目。
2. 生成一个与目标操作系统无关的 `.rehome` 文件。
3. 用户通过微信、飞书、网盘、U 盘或局域网自行传递文件。
4. 在目标电脑预览将要恢复的内容。
5. 自动备份目标 Codex 状态，再执行合并恢复。
6. 映射 Windows/macOS 路径，恢复项目文件、对话、索引、Skills、Plugins 和生成物。
7. 通过 Codex Desktop 的项目打开入口注册恢复后的项目。
8. 验证恢复结果；失败时自动回滚。

支持 Windows -> Windows、Windows -> macOS、macOS -> Windows、macOS -> macOS，以及重装系统前后的同设备恢复。

## 2. 首版范围

### 包含

- 自动检测标准 `CODEX_HOME`、Codex/ChatGPT Desktop 应用位置和常见项目路径。
- 用户选择一个或多个项目，并确认自动匹配的相关对话。
- 生成完整快照型 `.rehome` 包。
- 接收端合并恢复，不默认整体替换目标 `~/.codex`。
- 保留目标登录态、配置、安装 ID 和模型缓存。
- 恢复前自动备份，恢复失败自动回滚，也允许用户从历史记录手动回滚。
- 对话文件、`session_index.jsonl`、相关 SQLite thread index、Skills、Plugins、生成图片和项目文件的恢复。
- Windows/macOS 路径映射与项目注册。
- Git 项目和普通文件夹项目的基础支持。
- 导入现有 Codex Rehome schema v3 ZIP，避免已有用户的迁移包失效。
- Windows 安装包和 macOS Universal DMG。

### 不包含

- ReHome 自建云盘、账号系统、域名或公网服务器。
- 后台实时同步、自动监控文件变化或开机常驻服务。
- 首版中的增量 `.rehome` 包和同一对话的追加合并。
- Claude Code、ChatGPT Web 或其他 Agent 的历史导入。
- Codex 登录凭证、浏览器 Cookie、`.env`、私钥或系统钥匙串迁移。
- 原生依赖目录迁移，例如 `node_modules`、Python 虚拟环境和编译缓存。

每日增量交接和同一对话追加属于下一阶段。首版的 manifest、稳定对象 ID 和检查点字段会为它预留兼容空间，但 UI 不展示尚未实现的入口。

## 3. 产品形式与发布

普通用户只看到一个产品：**ReHome Desktop**。

- Windows：`ReHome-Desktop-Windows-x64-Setup.exe`
- macOS：`ReHome-Desktop-macOS-Universal.dmg`
- 迁移数据：`<project>-<date>.rehome`

ReHome Core 是应用内的迁移引擎，Codex Bridge 是应用内的 Codex 检测、索引和项目注册适配层。它们是代码模块，不是额外安装项。Codex Skill 可以在后续作为可选的自然语言入口，但不影响 Desktop 独立工作。

安装包发布到 GitHub Releases。首版不要求域名、下载服务器、Apple Developer ID 或 Windows 代码签名。未签名版本必须在 README 和 Release Notes 中说明系统首次启动警告及打开方法。

macOS 安装流程为打开 DMG，把 ReHome Desktop 拖入 Applications。Windows 使用每用户安装，默认不要求管理员权限。

## 4. 技术架构

采用 Tauri 2：

- 前端：React + TypeScript + Vite。
- 桌面壳与 Core：Rust。
- 本地数据库操作：Rust `rusqlite`，使用 bundled SQLite。
- 打包与校验：Rust ZIP 和 SHA-256 库。
- 安装包：Tauri bundler，输出 Windows NSIS EXE 和 macOS Universal DMG。

选择 Rust Core 的原因是迁移过程需要大量路径、ZIP、JSONL、SQLite 和原子文件操作。把这些能力编译进应用，可避免要求用户安装 Python 或额外运行时，同时比让 UI 直接调用长 PowerShell/Bash 脚本更容易做事务、进度、取消和回滚。

现有 PowerShell/Bash 脚本继续保留：

- 兼容旧的 Skill 使用方式。
- 作为 Desktop Core 行为的回归基准。
- 在桌面应用尚未覆盖的边缘环境中提供人工救援入口。

Desktop 不直接修改这些脚本来伪装成应用。Core 会逐项移植已经验证过的打包、恢复和校验规则，并通过同一组测试夹具比较结果。

## 5. 模块边界

### Desktop Shell

负责窗口、路由、系统文件选择器、通知和更新提示。它只能通过类型化 Tauri command 调用 Core，不能直接访问 Codex 数据。

### Discovery

检测：

- 当前操作系统和 CPU 架构。
- `CODEX_HOME` 或默认 `.codex`。
- Codex/ChatGPT Desktop 应用与 CLI。
- sessions、index、SQLite、Skills、Plugins 和生成物数量。
- 用户最近使用的项目路径。

检测不到标准路径时，才要求用户手动选择。选择结果只保存在 ReHome 自己的本地设置中。

### Package Writer

把选中的 Codex 内容与项目写入 staging 目录，执行敏感文件和开发依赖排除，生成 manifest 与校验清单，再原子输出 `.rehome` 文件。

### Package Reader

在不恢复的情况下读取 manifest、验证 SHA-256、检查 schema 和排除策略，并生成接收预览。损坏、缺少核心数据或来自未来不兼容 schema 的包不得进入恢复阶段。

### Restore Engine

以事务方式执行：

1. 创建恢复计划。
2. 备份所有即将改变的目标文件。
3. 把包解压到应用私有临时目录。
4. 合并 Codex 文件和项目文件。
5. 执行路径映射、session index 和 SQLite thread index 更新。
6. 调用 Codex Bridge 注册项目。
7. 执行 verifier。
8. 成功后提交事务；失败则恢复备份。

### Codex Bridge

负责所有随 Codex 版本变化的行为：

- 识别 Codex 数据布局。
- 读取与合并 session/index/state。
- 使用目标平台可用的官方应用入口打开项目。
- 使用 `codex://threads/<id>` 打开已经恢复的对话。
- 报告“文件已恢复”和“Codex UI 已可见”两个不同结果。

Bridge 不迁移 `auth.json`，也不声称项目文件存在就等于左侧栏已经注册。

### Backup Store

保存恢复事务的元数据和必要备份，默认位于 ReHome 的应用数据目录。历史页展示时间、来源设备、项目、变更摘要和回滚状态。用户可以删除已提交且不再需要的备份。

## 6. `.rehome` 包格式

`.rehome` 是 ZIP 容器，但使用独立扩展名。所有 entry 使用 UTF-8 名称和 `/` 分隔符。

```text
manifest.json
checksums.sha256
codex/
  sessions/
  archived_sessions/
  session_index.jsonl
  skills/
  plugins/cache/
  generated_images/
  metadata/
projects/
  <stable-project-id>/
    files/
    project.json
```

`manifest.json` 至少包含：

- `format`: `codex-rehome`
- `schema_version`: `1`
- `package_id`
- `created_at`
- `source_os` 与 `source_arch`
- `source_device_id`：ReHome 随机生成的本地 ID，不使用硬件序列号
- `mode`: `full`
- 项目、对话、Skills、Plugins、SQLite 和文件计数
- 每个项目的稳定 ID、来源路径和 Git 元数据摘要
- 每个对话的 task ID、项目 ID、更新时间和内容哈希
- 排除策略摘要
- `parent_checkpoint`: 首版为 `null`，为后续增量包保留

包不包含来源电脑绝对路径之外的机器身份信息，也不包含任何登录凭证。来源路径只用于接收端生成明确的路径映射。

## 7. 项目策略

### Git 项目

首版复制工作树，但排除 `.git`。manifest 记录 remote URL、当前 branch 和 HEAD commit（存在时），用于提醒接收者代码基线是否一致。目标目录已存在时，恢复预览按文件哈希显示新增、修改、相同和冲突文件；冲突文件不静默覆盖。

### 普通文件夹项目

按相对路径和 SHA-256 比较文件。目标不存在时完整恢复；目标存在时只自动写入新增文件和内容一致的安全更新。双方都已修改的文件进入冲突列表，由用户选择保留目标、使用来源或另存副本。

项目默认恢复到用户选择的根目录；未选择时使用 `Documents/Codex-Restored-Projects`。

## 8. 对话与状态策略

首版以“把来源对话作为可见历史恢复到目标 Codex”为目标，不承诺原对话可以继续使用旧系统路径执行工具。

- session JSONL 按 task ID 去重。
- 目标已有同 ID 且哈希相同：跳过。
- 目标无同 ID：恢复 session，补齐 `session_index.jsonl` 和相关 SQLite thread row。
- 目标已有同 ID 但内容不同：首版不追加、不覆盖，恢复为带来源标记的副本任务，并在报告中标为冲突分支。
- 路径字段映射到目标项目路径，包括 session metadata、turn context、thread cwd 和 rollout path。
- 目标 auth、配置和本机 installation ID 永远保留。
- state/memory/goal 数据库不整体替换，只导入恢复任务所需的最小 thread 数据。

恢复完成后，Bridge 必须分别验证 session 文件、session index、SQLite thread、项目路径和应用注册。只有这些层都通过，才能显示“已在 Codex 中可见”。

## 9. 用户流程

### 首页

显示本机 Codex 检测状态、最近一次交接，以及两个主操作：发送、接收。

### 发送

1. 选择项目。
2. ReHome 自动列出匹配对话和将被包含的 Codex 内容。
3. 用户勾选对话并查看排除项。
4. 选择保存位置。
5. 生成 `.rehome`，显示大小、校验结果和“在文件夹中显示”。

### 接收

1. 选择 `.rehome`。
2. 显示来源系统、项目、对话、文件变化和冲突。
3. 选择项目恢复目录。
4. 用户确认后，ReHome 请求关闭 Codex；未经确认不强制终止进程。
5. 自动备份并恢复。
6. 重新打开 Codex，注册项目和恢复对话。
7. 展示逐层验证结果及必要的人工动作。

### 历史与回滚

每次接收都是一条记录。回滚只撤销该次事务写入和覆盖的内容，不删除用户在恢复后新产生的其他数据。

## 10. 安全与系统影响

ReHome Desktop 默认：

- 不请求管理员权限。
- 不安装驱动、系统服务或浏览器扩展。
- 不开放监听端口，不运行 ReHome 云服务。
- 不设置开机启动。
- 不上传遥测、项目或对话。
- 只读取用户确认的 Codex 和项目目录。
- 所有恢复均先备份、后写入。

默认排除 `auth.json`、Cookie、Login Data、Local Storage、Session Storage、`.env`、私钥、`.git`、`node_modules`、venv、socket、IPC、Singleton 文件和缓存构建目录。首版不提供“包含秘密”的开关。

## 11. 错误处理

- Codex 正在写入：阻止恢复并提示用户关闭；用户确认后才允许应用尝试关闭。
- 包损坏或 checksum 不一致：停止在预览阶段，不创建目标变更。
- 磁盘空间不足：在 staging 和备份前检查，并显示需要与可用空间。
- schema 不支持：保留包，不尝试猜测恢复。
- 项目文件冲突：进入显式冲突选择，不静默覆盖。
- 任一索引或 SQLite 更新失败：回滚本次事务。
- 项目文件已恢复但应用注册失败：保留文件恢复，报告“需要在 Codex 中手动打开项目”，不声称完全成功。
- 应用崩溃：下次启动检测未完成事务，提供继续回滚或重新验证。

## 12. 测试与验收

### 自动测试

- Rust 单元测试覆盖路径标准化、排除规则、manifest、checksum、session index 合并、SQLite thread 导入、冲突分类和回滚日志。
- 使用不含真实用户数据的固定夹具测试 Windows 路径、macOS 路径、跨系统映射和 schema v3 兼容。
- 对现有 PowerShell/Bash acceptance tests 保持通过。
- 前端测试覆盖发送、接收、冲突和失败状态。
- GitHub Actions 分别构建 Windows x64、macOS x64 和 macOS arm64，并合成 Universal DMG。

### 首版验收

在真实或隔离测试环境中验证四个方向：

- `.rehome` 在两个系统上可直接读取和校验。
- 目标登录态未被覆盖。
- 选中 sessions、session index 和 SQLite thread 均存在。
- 项目文件恢复到目标路径。
- 来源绝对路径已替换，禁止文件数量为零。
- Codex Desktop 能显示注册后的项目和恢复对话，或明确报告需要手动打开项目。
- 人为制造恢复中断后能够回滚到恢复前状态。

## 13. 后续阶段

首版稳定后再增加：

1. 基于 `parent_checkpoint`、对象哈希和文件差异的增量 `.rehome` 包。
2. 对同一 Codex 对话执行严格前缀检测和安全追加；分叉时创建 relay 对话。
3. Git bundle 或远端 Git 协作，减少代码仓库重复传输。
4. 可选 Codex Plugin，让用户通过自然语言触发 Desktop 发送/接收。
5. GitHub Releases 更新检查和可选的签名/公证发布。

