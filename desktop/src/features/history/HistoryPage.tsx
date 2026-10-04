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
import SupportPanel from "../support/SupportPanel";
import Modal from "../../components/Modal";
import {
  errorMessage,
  supportIdFromError,
  type RecoveryStatus,
  type RollbackAction,
  type TransactionSummary,
} from "../../lib/types";
interface HistoryPageProps {
  headingRef: RefObject<HTMLHeadingElement | null>;
  onOperationStart: () => void;
  onOperationEnd: () => void;
  onRollbackFinished: (transactionId: string, success: boolean) => void;
}
export default function HistoryPage({
  headingRef,
  onOperationStart,
  onOperationEnd,
  onRollbackFinished,
}: HistoryPageProps) {
  const { locale, t } = useI18n();
  const [transactions, setTransactions] = useState<TransactionSummary[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [rollingBack, setRollingBack] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [supportId, setSupportId] = useState<string | null>(null);
  const [failed, setFailed] = useState<Set<string>>(new Set());
  const [warnings, setWarnings] = useState<string[]>([]);
  const [confirm, setConfirm] = useState<TransactionSummary | null>(null);
  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const history = await listTransactions();
      setTransactions(history.transactions);
      setWarnings(history.warnings);
      setSelected((current) =>
        history.transactions.some((row) => row.transaction_id === current)
          ? current
          : (history.transactions[0]?.transaction_id ?? null),
      );
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  async function handleRollback(transaction: TransactionSummary) {
    if (rollingBack || failed.has(transaction.transaction_id)) return;
    const action: RollbackAction =
      transaction.status === "committed" ? "rollback" : "resume";
    setConfirm(null);
    setRollingBack(transaction.transaction_id);
    setError(null);
    setSupportId(null);
    onOperationStart();
    let success = false;
    try {
      const result = await rollbackTransaction(
        transaction.transaction_id,
        action,
      );
      if (!result.success)
        throw new Error(t("撤销未完成，请保留备份并查看诊断。"));
      success = true;
      await refresh();
    } catch (caught) {
      setError(errorMessage(caught));
      setSupportId(supportIdFromError(caught));
      setFailed((current) => new Set(current).add(transaction.transaction_id));
      await refresh();
    } finally {
      try {
        window.localStorage.removeItem(
          "rehome-use-confirmed:" + transaction.transaction_id,
        );
      } catch {
        /* A partial undo also invalidates the cosmetic use confirmation. */
      }
      onRollbackFinished(transaction.transaction_id, success);
      setRollingBack(null);
      onOperationEnd();
    }
  }
  async function reveal(path: string, id: string) {
    setError(null);
    try {
      await openPath(path, id);
    } catch (caught) {
      setError(errorMessage(caught));
    }
  }
  function confirmed(id: string) {
    try {
      return Boolean(window.localStorage.getItem("rehome-use-confirmed:" + id));
    } catch {
      return false;
    }
  }
  return (
    <div className="page history-page">
      <header className="page-header page-header-with-action">
        <div>
          <h1 ref={headingRef} tabIndex={-1}>
            {t("迁移记录")}
          </h1>
          <p className="page-description">
            {t("查看本机导入记录和自动备份。")}
          </p>
        </div>
        <button
          className="icon-button"
          aria-label={t("刷新迁移记录")}
          disabled={loading || Boolean(rollingBack)}
          onClick={() => {
            setFailed(new Set());
            setError(null);
            setSupportId(null);
            void refresh();
          }}
        >
          <RefreshCw className={loading ? "spin" : ""} aria-hidden="true" />
        </button>
      </header>
      {error && (
        <p className="inline-state status-error" role="alert">
          <AlertTriangle aria-hidden="true" />
          {error}
        </p>
      )}
      {supportId && (
        <SupportPanel
          key={supportId}
          source={{ kind: "incident", support_id: supportId }}
        />
      )}
      {warnings.length > 0 && (
        <details className="history-warnings">
          <summary className="inline-state status-warning" role="status">
            {t("有 {count} 条旧迁移记录无法读取，已安全跳过。", {
              count: warnings.length,
            })}
          </summary>
          <div className="history-warning-details">
            <strong>{t("技术详情")}</strong>
            {warnings.map((warning) => (
              <code key={warning}>{warning}</code>
            ))}
          </div>
        </details>
      )}
      {loading && !transactions.length && (
        <p className="inline-state" role="status">
          {t("正在读取迁移记录...")}
        </p>
      )}
      {!loading && !transactions.length && (
        <div className="history-empty">
          <Clock3 aria-hidden="true" />
          <strong>{t("暂无导入记录")}</strong>
          <span>{t("完成一次导入后，记录会显示在这里。")}</span>
        </div>
      )}
      <div className="transaction-list">
        {transactions.map((transaction) => {
          const committed = transaction.status === "committed",
            resumable = isResumable(transaction.status),
            busy = rollingBack === transaction.transaction_id,
            expanded = selected === transaction.transaction_id;
          return (
            <article
              className="transaction-row"
              data-testid={"transaction-" + transaction.transaction_id}
              key={transaction.transaction_id}
            >
              <div className="transaction-header">
                <button
                  className="transaction-select"
                  disabled={Boolean(rollingBack)}
                  aria-expanded={expanded}
                  aria-controls={"detail-" + transaction.transaction_id}
                  onClick={() => setSelected(transaction.transaction_id)}
                >
                  <span
                    className={"transaction-icon status-" + transaction.status}
                  >
                    {committed ? (
                      <CheckCircle2 aria-hidden="true" />
                    ) : (
                      <RotateCcw aria-hidden="true" />
                    )}
                  </span>
                  <span>
                    <strong>{statusLabel(transaction.status, t)}</strong>
                    <time>{formatDate(transaction.created_at, locale)}</time>
                  </span>
                  <span>
                    {t("{count} 个文件", { count: transaction.changed_files })}
                  </span>
                </button>
                <button
                  className="rollback-button"
                  aria-label={t(resumable ? "继续回滚事务" : "回滚此事务")}
                  disabled={
                    (!committed && !resumable) ||
                    Boolean(rollingBack) ||
                    failed.has(transaction.transaction_id)
                  }
                  onClick={() => setConfirm(transaction)}
                >
                  {busy ? (
                    <LoaderCircle className="spin" aria-hidden="true" />
                  ) : (
                    <RotateCcw aria-hidden="true" />
                  )}
                  {t(resumable ? "继续回滚" : "撤销导入")}
                </button>
              </div>
              <div
                className="transaction-detail"
                id={"detail-" + transaction.transaction_id}
                hidden={!expanded}
              >
                <p className="muted">
                  {committed
                    ? t(
                        confirmed(transaction.transaction_id)
                          ? "已由你确认能用"
                          : "实际使用待确认",
                      )
                    : t("请按本次事务状态处理，不要覆盖整个 Codex 目录。")}
                </p>
                <details className="technical-details">
                  <summary>{t("查看记录详情")}</summary>
                  <code>{transaction.transaction_id}</code>
                  <div className="summary-row">
                    <span>{t("Codex 数据位置")}</span>
                    <code>{transaction.target_codex_home}</code>
                  </div>
                  <div className="summary-row">
                    <span>{t("项目目录")}</span>
                    <code>{transaction.projects_root}</code>
                  </div>
                  <div className="summary-row">
                    <span>{t("备份目录")}</span>
                    <code>{transaction.transaction_backup_path}</code>
                  </div>
                </details>
                <div className="transaction-actions">
                  <button
                    className="icon-text-button"
                    disabled={Boolean(rollingBack)}
                    onClick={() =>
                      void reveal(
                        transaction.transaction_backup_path,
                        transaction.transaction_id,
                      )
                    }
                  >
                    <FolderOpen aria-hidden="true" />
                    {t("显示备份")}
                  </button>
                  {transaction.restored_project_paths.map((path) => (
                    <button
                      className="icon-text-button"
                      disabled={Boolean(rollingBack)}
                      aria-label={t("显示项目 {path}", { path })}
                      key={path}
                      onClick={() =>
                        void reveal(path, transaction.transaction_id)
                      }
                    >
                      <FolderOpen aria-hidden="true" />
                      {t("显示项目")}
                    </button>
                  ))}
                </div>
                {failed.has(transaction.transaction_id) && (
                  <p className="manual-status">
                    {t("撤销未完成。请保留备份，解决文件变化后刷新记录再试。")}
                  </p>
                )}
                <SupportPanel
                  source={{
                    kind: "transaction",
                    transaction_id: transaction.transaction_id,
                  }}
                />
              </div>
            </article>
          );
        })}
      </div>
      {confirm && (
        <Modal title={t("撤销这次导入？")} onClose={() => setConfirm(null)}>
          <p>
            {t(
              "按本次备份恢复文件。导入后被修改的文件可能阻止撤销，不会强行覆盖。",
            )}
          </p>
          <div className="summary-row">
            <span>{t("变更文件")}</span>
            <strong>{confirm.changed_files}</strong>
          </div>
          <code className="modal-path">{confirm.transaction_backup_path}</code>
          <p className="privacy-note">
            {t("请先保存当前工作。撤销不是删除迁移包。")}
          </p>
          <div className="modal-actions">
            <button
              data-autofocus
              className="secondary-button"
              onClick={() => setConfirm(null)}
            >
              {t("取消")}
            </button>
            <button
              className="command-button"
              onClick={() => void handleRollback(confirm)}
            >
              {t("确认撤销")}
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}
function isResumable(status: RecoveryStatus) {
  return [
    "prepared",
    "applying",
    "verifying",
    "rolling_back",
    "rollback_failed",
  ].includes(status);
}
function statusLabel(status: RecoveryStatus, t: (key: string) => string) {
  return t(
    {
      prepared: "已准备",
      applying: "导入中",
      verifying: "验证中",
      committed: "已完成",
      rolling_back: "回滚中",
      rolled_back: "已回滚",
      rollback_failed: "回滚失败",
    }[status],
  );
}
function formatDate(value: string, locale: "zh-CN" | "en") {
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? value
    : date.toLocaleString(locale, { hour12: false });
}
