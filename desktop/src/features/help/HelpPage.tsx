import type { RefObject } from "react";
import { useI18n } from "../../lib/i18n";
export default function HelpPage({
  headingRef,
}: {
  headingRef: RefObject<HTMLHeadingElement | null>;
}) {
  const { t } = useI18n();
  const items = [
    [
      "第一次迁移，怎么做？",
      "旧电脑选择导出，勾选要带走的项目、对话和 Skills。把生成的 .rehome 文件带到新电脑，选择导入，检查位置和冲突后确认。",
    ],
    [
      "导入后，怎么确认旧对话能用？",
      "退出并重新打开 Codex。找到原项目和对话，检查旧消息，再在原对话发送一条消息。ReHome 的系统检查不能代替这一步。",
    ],
    [
      "恢复失败了怎么办？",
      "查看失败页面的 Codex 排查内容。检查后复制到本机 Codex 的新对话，由你决定是否发送。不要让 Agent 覆盖整个 Codex 目录。",
    ],
    [
      "撤销导入会影响什么？",
      "迁移记录可以撤销一次导入。ReHome 会按该次备份恢复文件；导入后被修改的文件可能阻止撤销，不会强行覆盖。",
    ],
    [
      "迁移包可以删除吗？",
      "确认原项目、旧消息和实际续聊都正常后，可以删除 .rehome 文件。删除迁移包不会删除已导入内容，建议先留一份备份。",
    ],
    [
      "哪些内容不会迁移？",
      "登录凭据、密钥、.env、Git 历史、依赖目录和运行中的进程不会迁移。新电脑仍需安装环境并登录 Codex。",
    ],
  ];
  return (
    <div className="page help-page">
      <header className="page-header">
        <h1 ref={headingRef} tabIndex={-1}>
          {t("需要一点帮助？")}
        </h1>
        <p className="page-description">
          {t("从迁移到确认能用，几个常见问题。")}
        </p>
      </header>
      <section className="help-list">
        {items.map(([title, body], index) => (
          <details key={title} open={index === 0 ? true : undefined}>
            <summary>{t(title)}</summary>
            <p>{t(body)}</p>
          </details>
        ))}
      </section>
      <p className="privacy-note">
        {t("这份帮助保存在应用内，离线也能查看。")}
      </p>
    </div>
  );
}
