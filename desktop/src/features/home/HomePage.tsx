import { useEffect, useState, type RefObject } from "react";
import {
  ArrowDownToLine,
  ArrowUpFromLine,
  CheckCircle2,
  Clock3,
  FolderKanban,
  Image,
  MessageSquareText,
  PackageCheck,
  Puzzle,
  Sparkles,
} from "lucide-react";

import { listTransactions } from "../../lib/api";
import { useI18n } from "../../lib/i18n";
import type { CodexInventory, RecoveryStatus, TransactionSummary } from "../../lib/types";

interface HomePageProps {
  headingRef: RefObject<HTMLHeadingElement | null>;
  inventory: CodexInventory | null;
  loading: boolean;
  error: string | null;
  onNavigate: (view: "send" | "receive") => void;
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
        <p className="eyebrow">CODEX WORKSPACE</p>
        <h1 ref={headingRef} tabIndex={-1}>{t("迁移工作台")}</h1>
        <p className="page-description">{t("从旧电脑导出，在新电脑导入。全程离线。")}</p>
      </header>

      <section className="action-strip" aria-label={t("迁移操作")}>
        <button className="primary-action send-action" type="button" onClick={() => onNavigate("send")}>
          <ArrowUpFromLine aria-hidden="true" />
          <span><strong>{t("导出")}</strong><small>{t("创建 .rehome 迁移包")}</small></span>
        </button>
        <button className="primary-action receive-action" type="button" onClick={() => onNavigate("receive")}>
          <ArrowDownToLine aria-hidden="true" />
          <span><strong>{t("导入")}</strong><small>{t("将迁移包导入本机 Codex")}</small></span>
        </button>
      </section>

      <section className="workflow-section" aria-labelledby="detected-title">
        <div className="section-title-row">
          <div>
            <p className="section-kicker">{t("本机检测")}</p>
            <h2 id="detected-title">{t("Codex 内容")}</h2>
          </div>
          {inventory && <span className="status status-success"><CheckCircle2 aria-hidden="true" />{t("已检测")}</span>}
        </div>

        {loading && <p className="inline-state" role="status">{t("正在检测 Codex...")}</p>}
        {error && <p className="inline-state status-error" role="alert">{error}</p>}
        {inventory && (
          <>
            <div className="path-line"><span>Codex Home</span><code>{inventory.codex_home}</code></div>
            <div className="count-grid" aria-label={t("内容数量")}>
              <span><FolderKanban aria-hidden="true" /><strong>{inventory.counts.projects}</strong> {t("{count} 个项目", { count: "" }).trim()}</span>
              <span><MessageSquareText aria-hidden="true" /><strong>{inventory.counts.conversations}</strong> {t("{count} 个对话", { count: "" }).trim()}</span>
              <span><Sparkles aria-hidden="true" /><strong>{inventory.counts.skills}</strong> {t("{count} 个技能", { count: "" }).trim()}</span>
              <span><Puzzle aria-hidden="true" /><strong>{inventory.counts.plugins}</strong> {t("{count} 个插件", { count: "" }).trim()}</span>
              <span><Image aria-hidden="true" /><strong>{inventory.counts.generated_images}</strong> {t("{count} 张生成图片", { count: "" }).trim()}</span>
            </div>
          </>
        )}
      </section>

      <section className="workflow-section" aria-labelledby="recent-title">
        <div className="section-title-row">
          <div>
            <p className="section-kicker">{t("迁移记录")}</p>
            <h2 id="recent-title">{t("最近一次迁移")}</h2>
          </div>
          <Clock3 aria-hidden="true" />
        </div>
        {recent ? (
          <div className="recent-row">
            <PackageCheck aria-hidden="true" />
            <div><strong>{t("{count} 个文件变更", { count: recent.changed_files })}</strong><span>{recent.created_at}</span></div>
            <span className={`status status-${recent.status}`}>{recoveryStatusLabel(recent.status, t)}</span>
          </div>
        ) : (
          <p className="empty-state">{t("暂无迁移记录")}</p>
        )}
      </section>
    </div>
  );
}

function recoveryStatusLabel(status: RecoveryStatus, t: (key: string) => string): string {
  const labels: Record<RecoveryStatus, string> = {
    prepared: "已准备",
    applying: "导入中",
    verifying: "验证中",
    committed: "已完成",
    rolling_back: "回滚中",
    rolled_back: "已回滚",
    rollback_failed: "回滚失败",
  };
  return t(labels[status]);
}
