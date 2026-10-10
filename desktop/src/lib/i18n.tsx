import {
  createContext,
  useEffect,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";

export type Locale = "zh-CN" | "en";
type Variables = Record<string, string | number>;
type Translator = (key: string, variables?: Variables) => string;

const STORAGE_KEY = "rehome.locale";

const english: Record<string, string> = {
  "导出空间不足。请检查下方的实际暂存位置和保存位置。勾选项目会带走项目文件，减少对话数量不一定能缩小包；可改存到空间充足、且不在所选项目内的文件夹。\n{message}": "Not enough space to export. Check the actual staging and save locations below. Selecting a project includes its files, so fewer chats may not make the package much smaller. Choose a folder with sufficient space outside the selected projects.\n{message}",
  "勾选项目会包含项目文件。导出需要暂存副本和迁移包的空间；通常暂存在保存位置，若保存到所选内容内或磁盘不支持私有权限，则使用本机缓存目录。": "Selecting a project includes its files. Export needs space for both a temporary snapshot and the package. The snapshot normally uses the save location; saving inside selected content or on a filesystem without private permissions uses the local cache instead.",
  "选择项目的上级文件夹，ReHome 会在其中为各项目建立目录，并更新关联对话的项目路径。导入前请在预览中检查完整目标路径。": "Choose the parent folder for your projects. ReHome creates a folder for each project there and updates associated chat paths. Check the complete target paths in the preview before importing.",
  "如何分批迁移？": "How do I migrate in batches?",
  "每次只选一部分对话，分别保存成不同的迁移包。同一项目分批迁移时，每批都保留项目勾选，并在新电脑导入到同一个项目保存位置。未变化的项目文件会跳过；有变化的文件仍需确认冲突。": "Select a subset of chats each time and save separate packages. For batches from the same project, keep the project selected in every package and import to the same project destination on the new computer. Unchanged project files are skipped; changed files still require conflict confirmation.",
  "版本 {version}，检查更新": "Version {version}, check for updates",
  "版本 {version}，发现更新": "Version {version}, update available",
  发现更新: "Update available",
  "检查失败，点击重试": "Check failed · Retry",
  "更新未完成，点击重试": "Update failed · Retry",
  "更新未完成，请重新检查后再试。":
    "Update did not finish. Check again to retry.",
  "这次导入已撤销，请重新预览后再导入。":
    "This import was undone. Preview it again before importing.",
  "撤销未完成，请先到迁移记录处理，再重新预览。":
    "Undo did not finish. Check History before previewing again.",
  返回: "Back",
  "内容较多时可能需要几分钟。": "Larger imports may take a few minutes.",
  "Plugins {plugins} · 生成图片 {images}":
    "Plugins {plugins} · Images {images}",
  "{projects} 个项目 · {conversations} 个对话 · {skills} 个 Skills":
    "{projects} projects · {conversations} chats · {skills} Skills",
  仅对话: "Chats only",
  "从迁移到确认能用，几个常见问题。":
    "A few answers, from moving your work to checking it works.",
  "体积将在创建时确定。":
    "The file size is determined when the package is created.",
  "保持简单，只设置必要的事。": "Just the essentials.",
  修改选择: "Change selection",
  先处理文件冲突: "Resolve file conflicts first",
  免费且开源: "Free and open source",
  包含项目文件: "Project files included",
  原项目或对话能找到: "I can find the original project or chat",
  取消: "Cancel",
  "哪些内容不会迁移？": "What stays behind?",
  回首页: "Back home",
  在原对话发送消息并收到回答:
    "I sent a message in the original chat and received a reply",
  在新电脑: "On the new computer",
  在旧电脑: "On the old computer",
  存在异常: "Needs attention",
  安装并重启: "Install and restart",
  实际使用待确认: "Actual use not confirmed",
  "导入 · 1 / 4": "Import · 1 / 4",
  "导入 · 2 / 4": "Import · 2 / 4",
  "导入 · 3 / 4": "Import · 3 / 4",
  "导入 · 4 / 4": "Import · 4 / 4",
  导入另一个迁移包: "Import another package",
  "导入后，怎么确认旧对话能用？": "How do I check my old chat works?",
  导入已有的迁移包: "Import a migration package",
  "导出 · 1 / 3": "Export · 1 / 3",
  "导出 · 2 / 3": "Export · 2 / 3",
  "导出 · 3 / 3": "Export · 3 / 3",
  导出要带走的内容: "Export the work you need",
  "工作放在哪里？": "Where should your work go?",
  已由你确认能用: "Confirmed usable by you",
  "带走哪些内容？": "What would you like to bring?",
  帮助: "Help",
  应用选择并重新预览: "Apply choice and preview again",
  "恢复失败了怎么办？": "What if recovery fails?",
  "打开 .rehome 文件，恢复到这台电脑。":
    "Open a .rehome file and restore it on this computer.",
  打开你的迁移包: "Open your migration package",
  "把工作带到新电脑。": "Bring your work with you.",
  "把这个文件带到新电脑，再用 ReHome 导入。":
    "Bring this file to the new computer, then import it with ReHome.",
  "按本次备份恢复文件。导入后被修改的文件可能阻止撤销，不会强行覆盖。":
    "Files will be restored from this import's backup. Files changed since import may block the undo; they will not be overwritten forcibly.",
  搜索项目或对话: "Search projects or chats",
  撤销导入: "Undo import",
  "撤销导入会影响什么？": "What does undoing an import change?",
  "撤销未完成。请保留备份，解决文件变化后刷新记录再试。":
    "Undo did not finish. Keep the backup, resolve changed files, then refresh history before retrying.",
  "撤销未完成，请保留备份并查看诊断。":
    "Undo did not finish. Keep the backup and review the diagnostics.",
  "撤销这次导入？": "Undo this import?",
  文件完整性校验通过: "Package integrity check passed",
  "文件已恢复，再确认一下。": "Files restored. Let's check they work.",
  旧对话实际使用: "Actual use of the old chat",
  "旧对话能正常使用吗？": "Does the original chat work?",
  旧消息仍然完整可见: "The original messages are still visible and complete",
  "旧电脑选择导出，勾选要带走的项目、对话和 Skills。把生成的 .rehome 文件带到新电脑，选择导入，检查位置和冲突后确认。":
    "On the old computer, choose Export and select your projects, chats and Skills. Bring the .rehome file to the new computer, choose Import, check locations and conflicts, then confirm.",
  更多内容: "More content",
  更多选项: "More options",
  更新下载进度: "Update download progress",
  "更新会重启 ReHome。请先完成迁移并保存当前工作。":
    "Updating restarts ReHome. Finish any migration and save your current work first.",
  "替换前自动备份，发生错误保留诊断。":
    "Files are backed up before replacement; diagnostics are kept if an error occurs.",
  最近一次导入: "Latest import",
  本机帮助: "Local help",
  查看保存位置与校验: "Show saved location and checksum",
  "查看失败页面的 Codex 排查内容。检查后复制到本机 Codex 的新对话，由你决定是否发送。不要让 Agent 覆盖整个 Codex 目录。":
    "Review the troubleshooting text on the failure page and copy it to a new Codex chat on this computer. You decide whether to send it. Do not let an Agent overwrite the entire Codex folder.",
  查看帮助: "View help",
  查看排除内容: "Show exclusions",
  查看文件变更: "Show file changes",
  查看校验详情: "Show integrity details",
  查看检测内容: "Show detected content",
  查看检测提示: "Show discovery notices",
  查看系统检查详情: "Show system check details",
  查看记录详情: "Show record details",
  查看迁移记录: "View migration history",
  "正在处理，请稍候…": "Working, please wait…",
  正在恢复你的工作: "Restoring your work",
  "正在执行导入。这里不会显示未经确认的进度。":
    "Import is running. No estimated progress is shown.",
  没有匹配的项目或对话: "No matching projects or chats",
  版本与更新: "Version & updates",
  "现在更新 ReHome？": "Update ReHome now?",
  界面语言: "Interface language",
  "登录信息和敏感文件不会打包。":
    "Sign-in information and sensitive files are excluded.",
  "登录凭据、密钥、.env、Git 历史、依赖目录和运行中的进程不会迁移。新电脑仍需安装环境并登录 Codex。":
    "Sign-in credentials, keys, .env, Git history, dependencies and running processes are not migrated. Set up your environment and sign in to Codex on the new computer.",
  "确认位置与变更，保存当前工作后开始。":
    "Check locations and changes, then save your current work before starting.",
  "确认原项目、旧消息和实际续聊都正常后，可以删除 .rehome 文件。删除迁移包不会删除已导入内容，建议先留一份备份。":
    "You can delete the .rehome file after confirming the original project, old messages and chat continuation work. Deleting the package does not delete imported data. Keep a backup if possible.",
  "确认后，选择迁移包的保存位置。":
    "After reviewing, choose where to save the package.",
  确认导入: "Confirm import",
  确认撤销: "Confirm undo",
  确认旧对话能用: "Confirm the old chat works",
  确认能用: "Confirm it works",
  确认要带走的内容: "Review what you're bringing",
  "离线迁移，登录信息留在原电脑。":
    "Offline migration. Sign-in information stays behind.",
  稍后: "Later",
  "第一次迁移，怎么做？": "How do I migrate for the first time?",
  系统检查: "System checks",
  继续: "Continue",
  设置: "Settings",
  设置与关于: "Settings & about",
  "请保持 ReHome 打开，完成后再重启 Codex。":
    "Keep ReHome open. Restart Codex after the import finishes.",
  "请保留迁移包，直到新电脑的旧对话确认能用。":
    "Keep the package until you've checked the old chat works on the new computer.",
  "请先保存当前工作。撤销不是删除迁移包。":
    "Save your current work first. Undoing an import is not deleting the package.",
  "请先重启 Codex，完成下面三项检查。":
    "Restart Codex first, then complete all three checks.",
  "请按本次事务状态处理，不要覆盖整个 Codex 目录。":
    "Follow this transaction's current state. Do not overwrite the entire Codex folder.",
  "迁移内容留在本机。ReHome 不会上传项目、对话或登录信息。":
    "Migration data stays local. ReHome does not upload projects, chats or sign-in information.",
  "迁移包不会上传。": "Your package is not uploaded.",
  "迁移包可以删除吗？": "Can I delete the migration package?",
  "迁移记录可以撤销一次导入。ReHome 会按该次备份恢复文件；导入后被修改的文件可能阻止撤销，不会强行覆盖。":
    "You can undo an import from migration history. ReHome restores files from that import's backup. Files changed since import may block undo; they will not be forcibly overwritten.",
  还没确认: "Not yet",
  "这份帮助保存在应用内，离线也能查看。":
    "This help is bundled with the app and works offline.",
  "这是你的使用确认，不是 ReHome 自动验证的结果。":
    "This is your confirmation of actual use, not an automatic ReHome check.",
  "退出并重新打开 Codex。找到原项目和对话，检查旧消息，再在原对话发送一条消息。ReHome 的系统检查不能代替这一步。":
    "Quit and reopen Codex. Find the original project and chat, check the old messages, then send a message in that chat. ReHome's system checks cannot replace this step.",
  "选好项目和对话，其他内容按需带走。":
    "Choose projects and chats. Bring other content only if you need it.",
  "选好项目和对话，创建一个迁移包。":
    "Choose your projects and chats to create one package.",
  选择保存位置并创建: "Choose location and create",
  "选择旧电脑导出的 .rehome 文件。":
    "Choose the .rehome file exported on the old computer.",
  "重启 Codex，检查原项目和旧对话。":
    "Restart Codex and check the original project and chat.",
  "需要一点帮助？": "Need a little help?",
  需要空间: "Space required",
  "项目、对话和 Skills，带走需要的，接着做。":
    "Your projects, chats and Skills. Bring what you need and pick up where you left off.",
  项目与对话: "Projects & chats",
  "项目放到你选的文件夹，Codex 数据恢复到本机。":
    "Projects go in your chosen folder. Codex data is restored locally.",
  "预览不会写入迁移内容。": "Preview does not write migration data.",

  故障求助: "Troubleshooting help",
  "需要帮助？": "Need help?",
  "操作未完成，已备好 Codex 排查内容":
    "The operation did not complete. Troubleshooting details for Codex are ready",
  "操作未完成，可重新生成 Codex 排查内容":
    "The operation did not complete. You can regenerate troubleshooting details for Codex",
  "正在整理本次失败信息…": "Preparing details from this failure…",
  "检查下方内容，复制到 Codex 的新对话。ReHome 不会自动发送或修复。":
    "Review the content below and copy it into a new Codex chat. ReHome will not send it or repair anything automatically.",
  收起: "Collapse",
  查看求助方案: "View help",
  "给 Codex 的排查内容": "Troubleshooting details for Codex",
  "可能包含本机路径；复制前请检查，粘贴后由你决定是否发送。":
    "May contain local paths. Review before copying; you decide whether to send after pasting.",
  "补充情况已更改，请重新生成后再复制。":
    "The added details changed. Regenerate the help text before copying.",
  "重新生成给 Codex 的求助内容": "Regenerate help for Codex",
  "返回 Codex 求助内容": "Back to Codex help",
  "项目或对话找不到、打不开？获取帮助":
    "Can't find or open a project or chat? Get help",
  "我已重启 Codex，原项目或对话仍找不到，或原对话仍打不开":
    "I restarted Codex; the original project or chat is still missing, or the chat still will not open",
  "ReHome 会整理本次情况，由你决定交给 Codex 或提交到 GitHub。不会自动上传或修复。":
    "ReHome prepares the incident for you to share with Codex or GitHub. Nothing is uploaded or repaired automatically.",
  "先生成求助内容并检查预览，再复制到本机 Codex 的新对话中发送。无需安装 Skill；Codex 会先排查，涉及关闭 Codex 的操作需要你同意。":
    "Prepare and review the help text, then copy it into a new Codex chat on this computer and send it. No Skill installation is needed. Codex will diagnose first; any step requiring Codex to close needs your consent.",
  "补充情况（可选，仅用于本机求助）":
    "Additional details (optional, local help only)",
  "例如：导入后左栏找不到项目；点击原对话时显示的报错。":
    "For example: the project is missing from the sidebar after import, or opening the original chat shows an error.",
  "生成给 Codex 的求助内容": "Prepare help for Codex",
  "预览 GitHub 问题": "Preview GitHub issue",
  "将交给 Codex 的内容": "Content to share with Codex",
  将公开的摘要: "Public summary preview",
  "包含本机路径。粘贴后会交给你配置的模型服务处理，发送前可删减。请在这台电脑的 Codex 新建对话粘贴，不需要安装 Skill。":
    "Includes local paths. Your configured model service will process what you send; edit it before sending if needed. Paste into a new Codex chat on this computer. No Skill installation is needed.",
  "仅包含版本、步骤和受控错误摘要，不包含本机路径、原始错误或补充描述。请在 GitHub 页面确认后提交。":
    "Includes only versions, steps and controlled error information, not local paths, raw errors or additional notes. Review and submit on GitHub.",
  "诊断文件未能保存，以下为本次内存摘要。":
    "The diagnostic file could not be saved. This is an in-memory summary.",
  求助内容预览: "Help content preview",
  "复制给 Codex": "Copy for Codex",
  复制公开摘要: "Copy public summary",
  "已复制。打开本机 Codex，新建对话，粘贴并发送。":
    "Copied. Open Codex on this computer, start a new chat, paste and send it.",
  "Codex 处理后，请确认原项目或对话可见、旧消息仍在，并打开原对话实际续聊；基础检查通过不等于恢复成功。":
    "After Codex works on it, confirm the original project or chat is visible and old messages remain, then open and actually continue the chat. A passing basic check does not prove recovery.",
  "已复制公开摘要。": "Public summary copied.",
  "打开 GitHub": "Open GitHub",
  "已打开提交页面，请在 GitHub 确认提交。":
    "Issue page opened. Review and submit on GitHub.",
  "摘要较长，请复制后粘贴到已打开的 GitHub 页面。":
    "The summary is long. Copy it and paste into the opened GitHub page.",
  查看本机诊断文件: "Show local diagnostic file",
  重新检查数据: "Recheck data",
  "正在整理...": "Preparing...",
  "如无法复制，可手动选取上方文本；如无法打开浏览器，可自行前往 GitHub 仓库。":
    "If copying fails, select the text above manually. If the browser cannot open, visit the GitHub repository yourself.",
  "基础检查时间：{time}": "Basic check at: {time}",
  "另有 {count} 个会话未纳入检查。":
    "{count} other conversations were not included.",
  "以上仅为基础文件与索引检查。数据库及实际续聊尚未验证；请确认原项目或对话可见，打开原对话并实际续聊。":
    "These are basic file and index checks only. The database and actual continuation remain unverified; confirm the original project or chat is visible, then open and actually continue the chat.",
  通过: "Passed",
  异常: "Problem found",
  未验证: "Unverified",
  不适用: "Not applicable",
  未知检查结果: "Unknown check result",
  "事务未完成导入，请先查看迁移记录状态。":
    "The transaction is not committed. Check its History status first.",
  项目目录存在: "Project directory exists",
  项目位置不是目录: "Project path is not a directory",
  项目目录缺失: "Project directory missing",
  路径无法安全读取: "Path cannot be read safely",
  缺少本次会话映射: "Conversation mapping unavailable",
  "达到检查时限，剩余内容未验证":
    "Check time limit reached; remaining data is unverified",
  无法识别会话文件头: "Session header not recognized",
  会话编号不一致: "Session ID mismatch",
  会话项目路径不一致: "Session project path mismatch",
  会话文件头匹配: "Session header matches",
  会话文件缺失: "Session file missing",
  会话无法读取或检查期间发生变化:
    "Session unavailable or changed during the check",
  本次未检查数据库: "Database not checked",
  请实际打开原对话并续聊确认:
    "Open the original chat and continue it to confirm",
  "记录超过范围，部分会话未检查":
    "Coverage limit reached; some chats were not checked",
  缺少本次索引位置: "Index location unavailable",
  "项目数量超过范围，部分项目未检查":
    "Coverage limit reached; some projects were not checked",
  索引条目匹配: "Index entry matches",
  索引路径不一致: "Index path mismatch",
  索引条目缺失: "Index entry missing",
  "索引无法读取、超过限制或检查期间发生变化":
    "Index unavailable, over the limit or changed during the check",
  首页: "Home",
  导出: "Export",
  导入: "Import",
  迁移记录: "History",
  前往首页: "Go to Home",
  前往导出: "Go to Export",
  前往导入: "Go to Import",
  前往迁移记录: "Go to History",
  迁移工作台: "Migration workspace",
  "导出 Codex 数据": "Export Codex Data",
  "导入 ReHome 包": "Import ReHome Package",
  "ReHome 首页": "ReHome Home",
  主导航: "Main navigation",
  离线本机迁移: "Offline device migration",
  正在检测: "Detecting",
  "未检测到 Codex": "Codex not detected",
  本机已就绪: "This device is ready",
  切换为中文: "切换为中文",
  "Switch to English": "Switch to English",
  "从旧电脑导出，在新电脑导入。全程离线。":
    "Export from the old computer and import on the new one. Fully offline.",
  迁移操作: "Migration actions",
  "创建 .rehome 迁移包": "Create a .rehome migration package",
  "将迁移包导入本机 Codex": "Import a migration package into this Codex",
  本机检测: "Device scan",
  "Codex 内容": "Codex content",
  已检测: "Detected",
  "正在检测 Codex...": "Detecting Codex...",
  内容数量: "Content counts",
  "{count} 个项目": "{count} projects",
  "{count} 个对话": "{count} conversations",
  "{count} 个技能": "{count} skills",
  "{count} 个插件": "{count} plugins",
  "{count} 张生成图片": "{count} generated images",
  最近一次迁移: "Latest migration",
  "{count} 个文件变更": "{count} changed files",
  暂无迁移记录: "No migrations yet",
  已准备: "Prepared",
  导入中: "Importing",
  验证中: "Verifying",
  已完成: "Complete",
  回滚中: "Rolling back",
  已回滚: "Rolled back",
  回滚失败: "Rollback failed",
  "在原电脑选择要带走的项目、对话和其他 Codex 内容。":
    "On the source computer, select the projects, conversations, and other Codex content to take with you.",
  全选迁移内容: "Select all migration content",
  "项目、对话和 Codex 内容": "Projects, conversations, and Codex content",
  选择项目与对话: "Select projects and conversations",
  "项目 {projects} · 对话 {conversations}":
    "Projects {projects} · Conversations {conversations}",
  "未检测到 Codex 已登记的本机项目":
    "No local projects registered with Codex were found",
  未归属项目的对话: "Conversations without a project",
  "只迁移对话，不包含项目文件":
    "Migrate conversations only, without project files",
  "其他 Codex 内容": "Other Codex content",
  都不是必选项: "All optional",
  迁移你希望在新电脑继续使用的能力: "Move the skills you want to keep using",
  "通常可以在新电脑重装，也可以选择带走":
    "Usually reinstallable, but you can include them",
  生成图片: "Generated images",
  只在需要保留历史生成物时选择:
    "Select only if you want to keep generated history",
  保存迁移包: "Save migration package",
  "ReHome 迁移包": "ReHome migration package",
  "通过系统窗口选择 .rehome 文件的保存位置":
    "Choose where to save the .rehome file in the system dialog",
  "正在创建迁移包。内容较多时可能需要几分钟，请保持 ReHome 打开。":
    "Creating the package. Large selections may take a few minutes; keep ReHome open.",
  "选择已完成，可以创建迁移包":
    "Selection complete. The package is ready to create.",
  请选择需要迁移的内容: "Select content to migrate",
  正在创建迁移包: "Creating migration package",
  创建迁移包: "Create migration package",
  迁移包已创建: "Migration package created",
  校验通过: "Verification passed",
  大小: "Size",
  内容: "Content",
  "{files} 个项目文件 / {conversations} 个对话":
    "{files} project files / {conversations} conversations",
  在文件夹中显示: "Show in folder",
  "迁移包已创建在：{path}\n但没能自动打开所在文件夹。":
    "The package was created at: {path}\nIts folder could not be opened automatically.",
  "有文件在打包过程中仍被修改。请稍等几秒后重试；如果反复出现，请先完全退出 Codex。\n{message}":
    "A file changed while it was being packaged. Wait a few seconds and retry. If this keeps happening, fully quit Codex first.\n{message}",
  "临时目录与所选目录重叠。请根据下方路径，缩小所选项目范围或更换迁移包保存文件夹；不要选择整个用户目录或磁盘。\n{message}":
    "The private staging directory overlaps a selected directory. Check the paths below, select a narrower project folder or change the package save folder. Do not select an entire user directory or drive.\n{message}",
  "所选 Skill 或插件包含符号链接。请取消选择对应的 Skill 或插件后重试；不要删除链接指向的原文件。\n{message}":
    "A selected skill or plugin contains a symbolic link. Deselect the affected skill or plugin and retry; do not delete the original files that the link points to.\n{message}",
  "选择项目 {name}": "Select project {name}",
  "项目文件夹已不存在，仅可迁移下面的对话":
    "The project folder is missing; only its conversations can be migrated",
  "收起项目 {name}": "Collapse project {name}",
  "展开项目 {name}": "Expand project {name}",
  "{count} 个文件": "{count} files",
  项目文件将在打包时统计: "Project files counted when packing",
  项目文件缺失: "Project files missing",
  "{name} 的对话": "Conversations in {name}",
  "主对话 {main} · 子 Agent {subagents} · 单独勾对话不含项目文件":
    "Main {main} · Subagents {subagents} · Selecting a conversation alone excludes project files",
  只选主对话: "Select main conversations only",
  "选择对话 {name}": "Select conversation {name}",
  "子 Agent": "Subagent",
  主对话: "Main",
  "辅助记录，通常可不迁移": "Supporting record; usually optional",
  建议迁移: "Recommended",
  这个项目下暂无可迁移对话: "No migratable conversations in this project",
  "全选 {name}": "Select all {name}",
  "已选 {selected} / {total}": "Selected {selected} / {total}",
  "选择 {name}": "Select {name}",
  "在文件夹中显示 {name}": "Show {name} in folder",
  没有检测到这类内容: "No content of this type was found",
  时间未知: "Time unknown",
  "在新电脑选择迁移包，检查内容后导入本机 Codex。":
    "On the new computer, select a migration package, review it, and import it into Codex.",
  选择迁移包: "Choose migration package",
  尚未选择: "Not selected",
  来源系统: "Source system",
  项目: "Projects",
  对话: "Conversations",
  "技能 / 插件 / 图片": "Skills / plugins / images",
  校验失败: "Verification failed",
  "禁用文件 {count}": "Forbidden files {count}",
  选择保存位置: "Choose save locations",
  "Codex 数据位置": "Codex data location",
  未检测: "Not detected",
  项目保存位置: "Project save location",
  选择项目保存位置: "Choose project save location",
  "迁移包不含项目文件，无需选择":
    "No project files in this package; no location needed",
  "安全备份由 ReHome 自动管理": "ReHome manages safety backups automatically",
  预览导入内容: "Preview import",
  "请重新预览导入内容后重试；如提示回滚失败，请先在迁移记录中恢复。":
    "Preview the import again before retrying. If rollback failed, recover it from History first.",
  确认导入内容: "Review import",
  "需要 {size}": "Requires {size}",
  "冲突 {count}": "Conflicts {count}",
  "发现 {count} 个同名但内容不同的文件。":
    "Same-name files with different content: {count}.",
  "仍有 {count} 个无法自动处理的结构冲突。":
    "{count} structural conflicts still require manual action.",
  结构冲突: "Structural conflict",
  "请选择如何处理这些普通文件冲突。":
    "Choose how ReHome should handle these regular file conflicts.",
  "请查看上表中的冲突路径，移开对应文件或目录，或重新选择一个空的项目保存位置后再预览。":
    "Review the conflict paths above, move the matching file or folder aside, or choose an empty project location and preview again.",
  冲突处理方式: "Conflict resolution",
  "保留新电脑文件（推荐）": "Keep files on this computer (recommended)",
  使用迁移包文件: "Use files from the migration package",
  "保留会跳过同名文件；替换会先自动备份新电脑上的原文件。":
    "Keeping skips matching paths. Replacing automatically backs up the current files first.",
  "已选择保留新电脑上的不同文件。":
    "Different files on this computer will be kept.",
  "已选择使用迁移包文件；被替换的文件会自动备份。":
    "Migration package files will be used, and replaced files will be backed up automatically.",
  目标项目目录: "Target project folder",
  包内来源: "Package source",
  目标位置: "Destination",
  变更: "Change",
  "确认已保存当前 Codex 工作": "Confirm current Codex work is saved",
  "当前 Codex 工作已保存": "Current Codex work is saved",
  "导入完成后请退出并重新打开 Codex，以加载迁移内容。":
    "After import, quit and reopen Codex to load migrated content.",
  正在导入: "Importing",
  "导入到 Codex": "Import into Codex",
  导入完成: "Import complete",
  迁移包校验: "Package checksum",
  文件完整性: "File integrity",
  对话文件: "Conversation files",
  会话索引: "Session index",
  线程数据库: "Thread database",
  跨平台路径: "Cross-platform paths",
  禁用文件隔离: "Forbidden-file isolation",
  项目文件: "Project files",
  "Codex 项目登记": "Codex project registration",
  "Codex 可见状态": "Codex visibility",
  对话可见性待确认: "Conversation visibility not yet verified",
  "文件和索引已导入。请重启 Codex，打开原对话并继续发送一条消息，确认可以使用。":
    "Files and indexes have been imported. Restart Codex, open the original conversation, and send another message to confirm it works.",
  "项目文件已导入，需要在 Codex 中手动打开":
    "Project files were imported and must be opened manually in Codex",
  "在 Codex 中打开": "Open in Codex",
  检查: "Inspect",
  备份: "Back up",
  完成: "Complete",
  导入进度: "Import progress",
  新增: "Add",
  更新: "Update",
  不变: "Unchanged",
  保留本机: "Keep local",
  冲突: "Conflict",
  "已在 Codex 中登记": "Registered in Codex",
  "查看本机导入记录和自动备份。": "View local imports and automatic backups.",
  刷新迁移记录: "Refresh migration history",
  "正在读取迁移记录...": "Loading migration history...",
  "有 {count} 条旧迁移记录无法读取，已安全跳过。":
    "{count} old migration records could not be read and were safely skipped.",
  技术详情: "Technical details",
  暂无导入记录: "No imports yet",
  "完成一次导入后，记录会显示在这里。": "Imports appear here after completion.",
  变更文件: "Changed files",
  项目目录: "Projects folder",
  备份目录: "Backup folder",
  显示备份: "Show backup",
  "显示项目 {path}": "Show project {path}",
  显示项目: "Show project",
  继续回滚事务: "Resume rollback transaction",
  回滚此事务: "Roll back this transaction",
  继续回滚: "Resume rollback",
  回滚: "Roll back",
  正在检查更新: "Checking for updates",
  "安装完成，正在重启…": "Installed. Restarting...",
  "检查失败，不影响离线迁移":
    "Update check failed; offline migration is unaffected",
  重新检查更新: "Check again",
  重新检查: "Check again",
  "正在安装 {percent}": "Installing {percent}",
  开发预览模式: "Development preview",
  当前已是最新版: "Up to date",
  发现新版本: "Update available",
  "当前 {version}": "Current {version}",
  请先完成当前迁移: "Finish the current migration first",
  "更新到 {version}": "Update to {version}",
};

interface I18nContextValue {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: Translator;
}

const I18nContext = createContext<I18nContextValue | null>(null);

function storedLocale(): Locale {
  try {
    return window.localStorage.getItem(STORAGE_KEY) === "en" ? "en" : "zh-CN";
  } catch {
    return "zh-CN";
  }
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(storedLocale);
  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);
  const value = useMemo<I18nContextValue>(
    () => ({
      locale,
      setLocale(next) {
        try {
          window.localStorage.setItem(STORAGE_KEY, next);
        } catch {
          /* The language still changes for this session. */
        }
        setLocaleState(next);
      },
      t(key, variables = {}) {
        const template = locale === "en" ? (english[key] ?? key) : key;
        return Object.entries(variables).reduce(
          (result, [name, value]) =>
            result.replaceAll(`{${name}}`, String(value)),
          template,
        );
      },
    }),
    [locale],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nContextValue {
  const value = useContext(I18nContext);
  if (!value) throw new Error("useI18n must be used inside I18nProvider");
  return value;
}
