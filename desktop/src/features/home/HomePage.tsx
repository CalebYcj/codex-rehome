import { useEffect, useState, type RefObject } from "react";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  ArrowRight,
  CheckCircle2,
  LoaderCircle,
  LockKeyhole,
} from "lucide-react";
import { listTransactions } from "../../lib/api";
import { useI18n } from "../../lib/i18n";
import type { CodexInventory, TransactionSummary } from "../../lib/types";
interface HomePageProps {
  headingRef: RefObject<HTMLHeadingElement | null>;
  inventory: CodexInventory | null;
  loading: boolean;
  error: string | null;
  onNavigate: (view: "send" | "receive" | "history") => void;
}
export default function HomePage({
  headingRef,
  inventory,
  loading,
  error,
  onNavigate,
}: HomePageProps) {
  const { t } = useI18n();
  const [recent, setRecent] = useState<TransactionSummary | null>(null);
  useEffect(() => {
    let active = true;
    void listTransactions()
      .then((history) => {
        if (active) setRecent(history.transactions[0] ?? null);
      })
      .catch(() => undefined);
    return () => {
      active = false;
    };
  }, []);
  return (
    <div className="page home-page">
      <header className="page-header">
        <h1 ref={headingRef} tabIndex={-1}>
          {t("把工作带到新电脑。")}
        </h1>
        <p className="page-description">
          {t("项目、对话和 Skills，带走需要的，接着做。")}
        </p>
      </header>
      <section className="home-actions" aria-label={t("迁移操作")}>
        <button
          type="button"
          className="home-action"
          aria-label={t("前往导出")}
          onClick={() => onNavigate("send")}
        >
          <span className="home-action-icon">
            <ArrowUpFromLine aria-hidden="true" />
          </span>
          <span>
            <small>{t("在旧电脑")}</small>
            <strong>{t("导出要带走的内容")}</strong>
            <span>{t("选好项目和对话，创建一个迁移包。")}</span>
          </span>
          <ArrowRight aria-hidden="true" />
        </button>
        <button
          type="button"
          className="home-action"
          aria-label={t("前往导入")}
          onClick={() => onNavigate("receive")}
        >
          <span className="home-action-icon">
            <ArrowDownToLine aria-hidden="true" />
          </span>
          <span>
            <small>{t("在新电脑")}</small>
            <strong>{t("导入已有的迁移包")}</strong>
            <span>{t("打开 .rehome 文件，恢复到这台电脑。")}</span>
          </span>
          <ArrowRight aria-hidden="true" />
        </button>
      </section>
      <div className="home-meta">
        {loading && (
          <p role="status">
            <LoaderCircle className="spin" aria-hidden="true" />
            {t("正在检测 Codex...")}
          </p>
        )}
        {error && (
          <p className="status-error" role="alert">
            {error}
          </p>
        )}
        {inventory && (
          <details className="machine-details">
            <summary>
              <CheckCircle2 aria-hidden="true" />
              {t("本机已就绪")}
              <span>{t("查看检测内容")}</span>
            </summary>
            <code>{inventory.codex_home}</code>
            <p aria-label={t("内容数量")}>
              {t("{count} 个项目", { count: inventory.counts.projects })} ·{" "}
              {t("{count} 个对话", { count: inventory.counts.conversations })} ·{" "}
              {t("{count} 个技能", { count: inventory.counts.skills })} ·{" "}
              {t("{count} 个插件", { count: inventory.counts.plugins })} ·{" "}
              {t("{count} 张生成图片", {
                count: inventory.counts.generated_images,
              })}
            </p>
          </details>
        )}
        <button
          className="icon-text-button"
          aria-label={t("前往迁移记录")}
          onClick={() => onNavigate("history")}
        >
          {t("查看迁移记录")} →
        </button>
      </div>
      {recent && (
        <p className="recent-note">
          {t("最近一次导入")} ·{" "}
          {t(
            {
              prepared: "已准备",
              applying: "导入中",
              verifying: "验证中",
              committed: "已完成",
              rolling_back: "回滚中",
              rolled_back: "已回滚",
              rollback_failed: "回滚失败",
            }[recent.status],
          )}
        </p>
      )}
      <p className="privacy-note">
        <LockKeyhole aria-hidden="true" />
        {t("离线迁移，登录信息留在原电脑。")}
      </p>
    </div>
  );
}
