import { useCallback, useEffect, useState, type RefObject } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Clock3,
  FolderOpen,
  LoaderCircle,
  RefreshCw,
  RotateCcw,
} from "lucide-react";

import { listTransactions, openPath, rollbackTransaction } from "../../lib/api";
import { useI18n } from "../../lib/i18n";
import { errorMessage, type RecoveryStatus, type RollbackAction, type TransactionSummary } from "../../lib/types";

interface HistoryPageProps {
  headingRef: RefObject<HTMLHeadingElement | null>;
  onOperationStart: () => void;
  onOperationEnd: () => void;
}

export default function HistoryPage({
  headingRef,
  onOperationStart,
  onOperationEnd,
}: HistoryPageProps) {
  const { locale, t } = useI18n();
  const [transactions, setTransactions] = useState<TransactionSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [rollingBack, setRollingBack] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const history = await listTransactions();
      setTransactions(history.transactions);
      setWarnings(history.warnings);
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function handleRollback(transaction: TransactionSummary, action: RollbackAction) {
    setRollingBack(transaction.transaction_id);
    setError(null);
    onOperationStart();
    try {
      await rollbackTransaction(transaction.transaction_id, action);
      await refresh();
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setRollingBack(null);
      onOperationEnd();
    }
  }

  async function handleReveal(path: string, transactionId: string) {
    setError(null);
    try {
      await openPath(path, transactionId);
    } catch (caught) {
      setError(errorMessage(caught));
    }
  }

  return (
    <div className="page history-page">
      <header className="page-header page-header-with-action">
        <div><p className="eyebrow">HISTORY</p><h1 ref={headingRef} tabIndex={-1}>{t("历史记录")}</h1><p className="page-description">{t("查看本机恢复事务和保留的备份。")}</p></div>
        <button className="icon-button" type="button" aria-label={t("刷新历史记录")} title={t("刷新历史记录")} onClick={() => void refresh()} disabled={loading}><RefreshCw className={loading ? "spin" : ""} aria-hidden="true" /></button>
      </header>

      {error && <p className="inline-state status-error" role="alert"><AlertTriangle aria-hidden="true" />{error}</p>}
      {warnings.map((warning) => <p className="inline-state status-warning" role="status" key={warning}><AlertTriangle aria-hidden="true" />{warning}</p>)}
      {loading && !transactions.length && <p className="inline-state" role="status">{t("正在读取事务记录...")}</p>}
      {!loading && !transactions.length && <div className="history-empty"><Clock3 aria-hidden="true" /><strong>{t("暂无恢复事务")}</strong><span>{t("完成一次接收后，事务会显示在这里。")}</span></div>}

      <div className="transaction-list">
        {transactions.map((transaction) => {
          const committed = transaction.status === "committed";
          const resumable = isResumable(transaction.status);
          const rollbackAction: RollbackAction = committed ? "rollback" : "resume";
          const busy = rollingBack === transaction.transaction_id;
          return (
            <article className="transaction-row" data-testid={`transaction-${transaction.transaction_id}`} key={transaction.transaction_id}>
              <div className="transaction-main">
                <span className={`transaction-icon status-${transaction.status}`}>{committed ? <CheckCircle2 aria-hidden="true" /> : <RotateCcw aria-hidden="true" />}</span>
                <div><div className="transaction-title"><strong>{statusLabel(transaction.status, t)}</strong><time>{formatDate(transaction.created_at, locale)}</time></div><code>{transaction.transaction_id}</code></div>
              </div>
              <div className="transaction-facts"><span>{t("变更文件")}<strong>{transaction.changed_files}</strong></span><span>{t("项目目录")}<strong>{transaction.projects_root}</strong></span><span>{t("备份目录")}<strong>{transaction.backup_root}</strong></span></div>
              <div className="transaction-actions">
                <button className="icon-text-button" type="button" onClick={() => void handleReveal(transaction.transaction_backup_path, transaction.transaction_id)}><FolderOpen aria-hidden="true" />{t("显示备份")}</button>
                {transaction.restored_project_paths.map((path) => (
                  <button className="icon-text-button" type="button" aria-label={t("显示项目 {path}", { path })} key={path} onClick={() => void handleReveal(path, transaction.transaction_id)}><FolderOpen aria-hidden="true" />{t("显示项目")}</button>
                ))}
                <button className="rollback-button" type="button" aria-label={t(resumable ? "继续回滚事务" : "回滚此事务")} disabled={(!committed && !resumable) || busy} onClick={() => void handleRollback(transaction, rollbackAction)}>{busy ? <LoaderCircle className="spin" aria-hidden="true" /> : <RotateCcw aria-hidden="true" />}{t(resumable ? "继续回滚" : "回滚")}</button>
              </div>
            </article>
          );
        })}
      </div>
    </div>
  );
}

function isResumable(status: RecoveryStatus): boolean {
  return ["prepared", "applying", "verifying", "rolling_back", "rollback_failed"].includes(status);
}

function statusLabel(status: RecoveryStatus, t: (key: string) => string): string {
  return t({
    prepared: "已准备",
    applying: "恢复中",
    verifying: "验证中",
    committed: "已提交",
    rolling_back: "回滚中",
    rolled_back: "已回滚",
    rollback_failed: "回滚失败",
  }[status]);
}

function formatDate(value: string, locale: "zh-CN" | "en"): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString(locale, { hour12: false });
}
