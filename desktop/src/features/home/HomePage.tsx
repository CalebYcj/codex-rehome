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
  const [recent, setRecent] = useState<TransactionSummary | null>(null);

  useEffect(() => {
    let active = true;
    void listTransactions()
      .then((transactions) => {
        if (active) setRecent(transactions[0] ?? null);
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
        <h1 ref={headingRef} tabIndex={-1}>迁移工作台</h1>
        <p className="page-description">在本机打包或恢复 Codex 工作内容。</p>
      </header>

      <section className="action-strip" aria-label="迁移操作">
        <button className="primary-action send-action" type="button" onClick={() => onNavigate("send")}>
          <ArrowUpFromLine aria-hidden="true" />
          <span><strong>发送</strong><small>创建离线 .rehome 包</small></span>
        </button>
        <button className="primary-action receive-action" type="button" onClick={() => onNavigate("receive")}>
          <ArrowDownToLine aria-hidden="true" />
          <span><strong>接收</strong><small>检查并恢复迁移包</small></span>
        </button>
      </section>

      <section className="workflow-section" aria-labelledby="detected-title">
        <div className="section-title-row">
          <div>
            <p className="section-kicker">本机检测</p>
            <h2 id="detected-title">Codex 内容</h2>
          </div>
          {inventory && <span className="status status-success"><CheckCircle2 aria-hidden="true" />已检测</span>}
        </div>

        {loading && <p className="inline-state" role="status">正在检测 Codex...</p>}
        {error && <p className="inline-state status-error" role="alert">{error}</p>}
        {inventory && (
          <>
            <div className="path-line"><span>Codex Home</span><code>{inventory.codex_home}</code></div>
            <div className="count-grid" aria-label="内容数量">
              <span><FolderKanban aria-hidden="true" /><strong>{inventory.counts.projects}</strong> 个项目</span>
              <span><MessageSquareText aria-hidden="true" /><strong>{inventory.counts.conversations}</strong> 个对话</span>
              <span><Sparkles aria-hidden="true" /><strong>{inventory.counts.skills}</strong> 个技能</span>
              <span><Puzzle aria-hidden="true" /><strong>{inventory.counts.plugins}</strong> 个插件</span>
              <span><Image aria-hidden="true" /><strong>{inventory.counts.generated_images}</strong> 张生成图片</span>
            </div>
          </>
        )}
      </section>

      <section className="workflow-section" aria-labelledby="recent-title">
        <div className="section-title-row">
          <div>
            <p className="section-kicker">事务记录</p>
            <h2 id="recent-title">最近交接</h2>
          </div>
          <Clock3 aria-hidden="true" />
        </div>
        {recent ? (
          <div className="recent-row">
            <PackageCheck aria-hidden="true" />
            <div><strong>{recent.changed_files} 个文件变更</strong><span>{recent.created_at}</span></div>
            <span className={`status status-${recent.status}`}>{recoveryStatusLabel(recent.status)}</span>
          </div>
        ) : (
          <p className="empty-state">暂无交接记录</p>
        )}
      </section>
    </div>
  );
}

function recoveryStatusLabel(status: RecoveryStatus): string {
  const labels: Record<RecoveryStatus, string> = {
    prepared: "已准备",
    applying: "恢复中",
    verifying: "验证中",
    committed: "已提交",
    rolling_back: "回滚中",
    rolled_back: "已回滚",
    rollback_failed: "回滚失败",
  };
  return labels[status];
}
