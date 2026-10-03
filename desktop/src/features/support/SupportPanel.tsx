import { useEffect, useRef, useState } from "react";
import { AlertTriangle, Copy } from "lucide-react";
import {
  prepareSupport,
  copySupportText,
  openSupportIssue,
  recheckSupport,
  openPath,
} from "../../lib/api";
import { useI18n } from "../../lib/i18n";
import {
  errorMessage,
  type SupportSource,
  type SupportPreview,
  type RecheckReport,
} from "../../lib/types";

export default function SupportPanel({ source }: { source: SupportSource }) {
  const { t, locale } = useI18n();
  const isIncident = source.kind === "incident";
  const sourceKey = isIncident ? source.support_id : source.transaction_id;
  const [expanded, setExpanded] = useState(isIncident);
  const [note, setNote] = useState("");
  const [failureConfirmed, setFailureConfirmed] = useState(false);
  const [preview, setPreview] = useState<SupportPreview | null>(null);
  const [mode, setMode] = useState<"codex" | "github">("codex");
  const [busy, setBusy] = useState(isIncident);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [checks, setChecks] = useState<RecheckReport | null>(null);
  const generation = useRef(0);
  const panelRef = useRef<HTMLElement>(null);

  useEffect(() => {
    if (isIncident) panelRef.current?.scrollIntoView?.({ block: "start" });
  }, [isIncident, sourceKey]);

  useEffect(() => {
    const current = ++generation.current;
    const currentSource: SupportSource =
      source.kind === "incident"
        ? { kind: "incident", support_id: sourceKey }
        : { kind: "transaction", transaction_id: sourceKey };

    setNote("");
    setFailureConfirmed(false);
    setExpanded(source.kind === "incident");
    setPreview(null);
    setChecks(null);
    setStatus(null);
    setError(null);

    if (source.kind !== "incident") {
      setBusy(false);
      return () => {
        if (generation.current === current) generation.current++;
      };
    }

    setBusy(true);
    setMode("codex");
    void prepareSupport(currentSource, locale, "", false)
      .then((result) => {
        if (current !== generation.current) return;
        setPreview(result);
      })
      .catch((caught) => {
        if (current === generation.current) setError(errorMessage(caught));
      })
      .finally(() => {
        if (current === generation.current) setBusy(false);
      });

    return () => {
      if (generation.current === current) generation.current++;
    };
  }, [source.kind, sourceKey, locale]);

  async function action(task: () => Promise<void>) {
    if (busy) return;
    setBusy(true);
    setError(null);
    setStatus(null);
    const current = generation.current;
    try {
      await task();
    } catch (caught) {
      if (current === generation.current) setError(errorMessage(caught));
    } finally {
      if (current === generation.current) setBusy(false);
    }
  }

  async function prepare(kind: "codex" | "github") {
    const current = generation.current;
    if (kind === "codex" && source.kind === "transaction" && !failureConfirmed)
      return;
    const result = await prepareSupport(source, locale, note, failureConfirmed);
    if (current !== generation.current) return;
    setPreview(result);
    setMode(kind);
    setChecks(null);
  }

  async function copyPreview() {
    if (!preview) return;
    await action(async () => {
      const current = generation.current;
      await copySupportText(preview.support_id, mode, locale);
      if (current === generation.current)
        setStatus(
          t(
            mode === "codex"
              ? "已复制。打开本机 Codex，新建对话，粘贴并发送。"
              : "已复制公开摘要。",
          ),
        );
    });
  }

  const incidentTitle = preview
    ? "操作未完成，已备好 Codex 排查内容"
    : busy
      ? "正在整理本次失败信息…"
      : "操作未完成，可重新生成 Codex 排查内容";

  return (
    <section
      ref={panelRef}
      className={`support-panel${isIncident ? " support-panel-incident" : ""}`}
      aria-label={t("故障求助")}
    >
      {isIncident ? (
        <div className="support-panel-heading">
          <div className="support-heading-copy" aria-live="polite">
            <strong>
              <AlertTriangle aria-hidden="true" />
              {t(incidentTitle)}
            </strong>
            <span>
              {t(
                "检查下方内容，复制到 Codex 的新对话。ReHome 不会自动发送或修复。",
              )}
            </span>
          </div>
          <button
            className="icon-text-button support-collapse"
            type="button"
            onClick={() => setExpanded(!expanded)}
            aria-expanded={expanded}
          >
            {t(expanded ? "收起" : "查看求助方案")}
          </button>
        </div>
      ) : (
        <button
          className="icon-text-button"
          type="button"
          onClick={() => setExpanded(!expanded)}
          aria-expanded={expanded}
        >
          {t("项目或对话找不到、打不开？获取帮助")}
        </button>
      )}

      {expanded && (
        <div className="support-content">
          {!isIncident && (
            <div className="support-explainer">
              <p>
                {t(
                  "ReHome 会整理本次情况，由你决定交给 Codex 或提交到 GitHub。不会自动上传或修复。",
                )}
              </p>
              <p>
                {t(
                  "先生成求助内容并检查预览，再复制到本机 Codex 的新对话中发送。无需安装 Skill；Codex 会先排查，涉及关闭 Codex 的操作需要你同意。",
                )}
              </p>
            </div>
          )}

          {source.kind === "transaction" && (
            <label>
              <span>{t("补充情况（可选，仅用于本机求助）")}</span>
              <textarea
                maxLength={2048}
                placeholder={t(
                  "例如：导入后左栏找不到项目；点击原对话时显示的报错。",
                )}
                value={note}
                disabled={busy}
                onChange={(event) => {
                  setNote(event.target.value);
                  setPreview(null);
                  setChecks(null);
                  setStatus(null);
                  setError(null);
                  setMode("codex");
                }}
              />
            </label>
          )}

          {isIncident && (
            <details className="support-note">
              <summary>{t("补充情况（可选，仅用于本机求助）")}</summary>
              <textarea
                aria-label={t("补充情况（可选，仅用于本机求助）")}
                maxLength={2048}
                placeholder={t(
                  "例如：导入后左栏找不到项目；点击原对话时显示的报错。",
                )}
                value={note}
                disabled={busy}
                onChange={(event) => {
                  setNote(event.target.value);
                  setPreview(null);
                  setChecks(null);
                  setStatus(null);
                  setError(null);
                  setMode("codex");
                }}
              />
            </details>
          )}

          {source.kind === "transaction" && (
            <div className="support-actions">
              <label className="support-confirmation">
                <input
                  type="checkbox"
                  checked={failureConfirmed}
                  disabled={busy}
                  onChange={(event) => {
                    setFailureConfirmed(event.target.checked);
                    setPreview(null);
                    setChecks(null);
                    setStatus(null);
                  }}
                />
                {t("我已重启 Codex，原项目或对话仍找不到，或原对话仍打不开")}
              </label>
              <button
                className="secondary-button"
                disabled={busy || !failureConfirmed}
                onClick={() => void action(() => prepare("codex"))}
              >
                {t("生成给 Codex 的求助内容")}
              </button>
              <button
                className="secondary-button"
                disabled={busy}
                onClick={() => void action(() => prepare("github"))}
              >
                {t("预览 GitHub 问题")}
              </button>
            </div>
          )}

          {isIncident && !preview && !busy && (
            <div className="support-retry">
              <p role={error ? "alert" : "status"}>
                {error ?? t("补充情况已更改，请重新生成后再复制。")}
              </p>
              <button
                className="command-button"
                onClick={() => void action(() => prepare("codex"))}
              >
                {t("重新生成给 Codex 的求助内容")}
              </button>
            </div>
          )}

          {preview && (
            <>
              <div className="support-preview-heading">
                <h3>
                  {t(mode === "codex" ? "给 Codex 的排查内容" : "将公开的摘要")}
                </h3>
                <p>
                  {t(
                    mode === "codex"
                      ? "可能包含本机路径；复制前请检查，粘贴后由你决定是否发送。"
                      : "仅包含版本、步骤和受控错误摘要，不包含本机路径、原始错误或补充描述。请在 GitHub 页面确认后提交。",
                  )}
                </p>
              </div>
              {!preview.saved && (
                <p className="support-inline-status" role="status">
                  {t("诊断文件未能保存，以下为本次内存摘要。")}
                </p>
              )}
              {mode === "codex" && (
                <button
                  className="command-button support-copy-button"
                  disabled={busy}
                  onClick={() => void copyPreview()}
                >
                  <Copy aria-hidden="true" />
                  {t("复制给 Codex")}
                </button>
              )}
              <textarea
                className="support-preview"
                aria-label={t("求助内容预览")}
                readOnly
                value={
                  mode === "codex" ? preview.codex_text : preview.github_text
                }
              />
              <div className="support-actions support-primary-actions">
                {mode === "github" && (
                  <button
                    className="secondary-button"
                    disabled={busy}
                    onClick={() => void copyPreview()}
                  >
                    {t("复制公开摘要")}
                  </button>
                )}
                {mode === "github" && (
                  <button
                    className="secondary-button"
                    disabled={busy}
                    onClick={() =>
                      void action(async () => {
                        const current = generation.current;
                        const result = await openSupportIssue(
                          preview.support_id,
                          locale,
                        );
                        if (current === generation.current)
                          setStatus(
                            t(
                              result === "opened"
                                ? "已打开提交页面，请在 GitHub 确认提交。"
                                : "摘要较长，请复制后粘贴到已打开的 GitHub 页面。",
                            ),
                          );
                      })
                    }
                  >
                    {t("打开 GitHub")}
                  </button>
                )}
                {preview.reveal_id && (
                  <button
                    className="secondary-button"
                    disabled={busy}
                    onClick={() =>
                      void action(() => openPath(preview.reveal_id!))
                    }
                  >
                    {t("查看本机诊断文件")}
                  </button>
                )}
                {preview.can_recheck && (
                  <button
                    className="secondary-button"
                    disabled={busy}
                    onClick={() =>
                      void action(async () => {
                        const current = generation.current;
                        const result = await recheckSupport(preview.support_id);
                        if (current === generation.current) setChecks(result);
                      })
                    }
                  >
                    {t("重新检查数据")}
                  </button>
                )}
                {isIncident && mode === "codex" && (
                  <button
                    className="secondary-button"
                    disabled={busy}
                    onClick={() => void action(() => prepare("github"))}
                  >
                    {t("预览 GitHub 问题")}
                  </button>
                )}
                {isIncident && mode === "github" && (
                  <button
                    className="secondary-button"
                    disabled={busy}
                    onClick={() => setMode("codex")}
                  >
                    {t("返回 Codex 求助内容")}
                  </button>
                )}
              </div>
              {mode === "codex" && (
                <p className="support-followup">
                  {t(
                    "Codex 处理后，请确认原项目或对话可见、旧消息仍在，并打开原对话实际续聊；基础检查通过不等于恢复成功。",
                  )}
                </p>
              )}
            </>
          )}

          {busy && (
            <p className="support-inline-status" role="status">
              {t("正在整理...")}
            </p>
          )}
          {status && (
            <p className="support-inline-status" role="status">
              {status}
            </p>
          )}
          {error && preview && (
            <p className="support-inline-status" role="alert">
              {error}{" "}
              {t(
                "如无法复制，可手动选取上方文本；如无法打开浏览器，可自行前往 GitHub 仓库。",
              )}
            </p>
          )}
          {checks && (
            <div className="support-checks">
              <p>{t("基础检查时间：{time}", { time: checks.checked_at })}</p>
              <ul>
                {checks.checks.map((check, index) => (
                  <li key={index}>
                    {check.subject} — {t(statusLabels[check.status])}：
                    {t(checkLabels[check.code] ?? "未知检查结果")}
                  </li>
                ))}
              </ul>
              {checks.omitted_sessions > 0 && (
                <p>
                  {t("另有 {count} 个会话未纳入检查。", {
                    count: checks.omitted_sessions,
                  })}
                </p>
              )}
              <p>
                {t(
                  "以上仅为基础文件与索引检查。数据库及实际续聊尚未验证；请确认原项目或对话可见，打开原对话并实际续聊。",
                )}
              </p>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

const statusLabels = {
  pass: "通过",
  fail: "异常",
  unknown: "未验证",
  not_applicable: "不适用",
};
const checkLabels: Record<string, string> = {
  project_coverage_limited: "项目数量超过范围，部分项目未检查",
  transaction_not_committed: "事务未完成导入，请先查看迁移记录状态。",
  project_exists: "项目目录存在",
  project_not_directory: "项目位置不是目录",
  project_missing: "项目目录缺失",
  path_unavailable: "路径无法安全读取",
  mapping_unavailable: "缺少本次会话映射",
  time_limit: "达到检查时限，剩余内容未验证",
  session_header_unknown: "无法识别会话文件头",
  session_id_mismatch: "会话编号不一致",
  session_path_mismatch: "会话项目路径不一致",
  session_header_valid: "会话文件头匹配",
  session_missing: "会话文件缺失",
  session_unavailable: "会话无法读取或检查期间发生变化",
  database_not_checked: "本次未检查数据库",
  conversation_not_verified: "请实际打开原对话并续聊确认",
  coverage_limited: "记录超过范围，部分会话未检查",
  index_not_available: "缺少本次索引位置",
  index_entry_valid: "索引条目匹配",
  index_path_mismatch: "索引路径不一致",
  index_entry_missing: "索引条目缺失",
  index_unavailable_or_changed: "索引无法读取、超过限制或检查期间发生变化",
};
