import { useEffect, useRef, useState } from "react";
import { prepareSupport, copySupportText, openSupportIssue, recheckSupport, openPath } from "../../lib/api";
import { useI18n } from "../../lib/i18n";
import { errorMessage, type SupportSource, type SupportPreview, type RecheckReport } from "../../lib/types";

export default function SupportPanel({ source }: { source: SupportSource }) {
  const { t, locale } = useI18n();
  const [expanded, setExpanded] = useState(false);
  const [note, setNote] = useState("");
  const [preview, setPreview] = useState<SupportPreview | null>(null);
  const [mode, setMode] = useState<"codex" | "github">("codex");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [checks, setChecks] = useState<RecheckReport | null>(null);
  const generation = useRef(0);
  const sourceKey = source.kind === "incident" ? source.support_id : source.transaction_id;
  useEffect(() => { setNote(""); }, [sourceKey]);
  useEffect(() => {
    generation.current++;
    setPreview(null); setChecks(null); setStatus(null); setError(null); setBusy(false);
    return () => { generation.current++; };
  }, [sourceKey, locale]);

  async function action(task: () => Promise<void>) {
    if (busy) return;
    setBusy(true); setError(null); setStatus(null);
    const current = generation.current;
    try { await task(); }
    catch (caught) { if (current === generation.current) setError(errorMessage(caught)); }
    finally { if (current === generation.current) setBusy(false); }
  }
  async function prepare(kind: "codex" | "github") {
    const current = generation.current;
    const result = await prepareSupport(source, locale, note);
    if (current !== generation.current) return;
    setPreview(result); setMode(kind); setChecks(null);
  }
  return <section className="support-panel" aria-label={t("故障求助")}>
    <button className="icon-text-button" type="button" onClick={() => setExpanded(!expanded)} aria-expanded={expanded}>
      {t(source.kind === "incident" ? "需要帮助？" : "会话打不开？获取帮助")}
    </button>
    {expanded && <div className="support-content">
      <p>{t("ReHome 会整理本次情况，由你决定交给 Codex 或提交到 GitHub。不会自动上传或修复。")}</p>
      <label>{t("补充情况（可选，仅用于本机求助）")}<textarea maxLength={2048} value={note} disabled={busy} onChange={e => { setNote(e.target.value); setPreview(null); setStatus(null); }} /></label>
      <div className="support-actions">
        <button className="secondary-button" disabled={busy} onClick={() => void action(() => prepare("codex"))}>{t("复制到 Codex")}</button>
        <button className="secondary-button" disabled={busy} onClick={() => void action(() => prepare("github"))}>{t("到 GitHub 提交问题")}</button>
      </div>
      {preview && <>
        <h3>{t(mode === "codex" ? "将交给 Codex 的内容" : "将公开的摘要")}</h3>
        <p>{t(mode === "codex"
          ? "包含本机路径。粘贴后会交给你配置的模型服务处理，发送前可删减。请在这台电脑的 Codex 新建对话粘贴，不需要安装 Skill。"
          : "仅包含版本、步骤和受控错误摘要，不包含本机路径、原始错误或补充描述。请在 GitHub 页面确认后提交。")}</p>
        {!preview.saved && <p role="status">{t("诊断文件未能保存，以下为本次内存摘要。")}</p>}
        <textarea className="support-preview" aria-label={t("求助内容预览")} readOnly value={mode === "codex" ? preview.codex_text : preview.github_text} />
        <div className="support-actions">
          <button className="secondary-button" disabled={busy} onClick={() => void action(async () => {
            const current = generation.current;
            await copySupportText(preview.support_id, mode, locale);
            if (current === generation.current) setStatus(t(mode === "codex" ? "已复制，请到本机 Codex 新对话粘贴。" : "已复制公开摘要。"));
          })}>{t("确认复制")}</button>
          {mode === "github" && <button className="secondary-button" disabled={busy} onClick={() => void action(async () => {
            const current = generation.current;
            const result = await openSupportIssue(preview.support_id, locale);
            if (current === generation.current) setStatus(t(result === "opened" ? "已打开提交页面，请在 GitHub 确认提交。" : "摘要较长，请复制后粘贴到已打开的 GitHub 页面。"));
          })}>{t("打开 GitHub")}</button>}
          {preview.reveal_id && <button className="secondary-button" disabled={busy} onClick={() => void action(() => openPath(preview.reveal_id!))}>{t("查看本机诊断文件")}</button>}
          {preview.can_recheck && <button className="secondary-button" disabled={busy} onClick={() => void action(async () => {
            const current = generation.current;
            const result = await recheckSupport(preview.support_id);
            if (current === generation.current) setChecks(result);
          })}>{t("重新检查数据")}</button>}
        </div>
      </>}
      {busy && <p role="status">{t("正在整理...")}</p>}
      {status && <p role="status">{status}</p>}
      {error && <p role="alert">{error} {t("如无法复制，可手动选取上方文本；如无法打开浏览器，可自行前往 GitHub 仓库。")}</p>}
      {checks && <div className="support-checks">
        <p>{t("基础检查时间：{time}", { time: checks.checked_at })}</p>
        <ul>{checks.checks.map((check, i) => <li key={i}>{check.subject} — {t(statusLabels[check.status])}：{t(checkLabels[check.code] ?? "未知检查结果")}</li>)}</ul>
        {checks.omitted_sessions > 0 && <p>{t("另有 {count} 个会话未纳入检查。", { count: checks.omitted_sessions })}</p>}
        <p>{t("以上仅为基础文件与索引检查。数据库及实际续聊尚未验证，请在 Codex 中打开原对话确认。")}</p>
      </div>}
    </div>}
  </section>;
}

const statusLabels = { pass: "通过", fail: "异常", unknown: "未验证", not_applicable: "不适用" };
const checkLabels: Record<string, string> = {
  project_coverage_limited: "项目数量超过范围，部分项目未检查",
  transaction_not_committed: "事务未完成导入，请先查看迁移记录状态。",
  project_exists: "项目目录存在", project_not_directory: "项目位置不是目录", project_missing: "项目目录缺失",
  path_unavailable: "路径无法安全读取", mapping_unavailable: "缺少本次会话映射",
  time_limit: "达到检查时限，剩余内容未验证", session_header_unknown: "无法识别会话文件头",
  session_id_mismatch: "会话编号不一致", session_path_mismatch: "会话项目路径不一致",
  session_header_valid: "会话文件头匹配", session_missing: "会话文件缺失", session_unavailable: "会话无法读取或检查期间发生变化",
  database_not_checked: "本次未检查数据库", conversation_not_verified: "请实际打开原对话并续聊确认",
  coverage_limited: "记录超过范围，部分会话未检查", index_not_available: "缺少本次索引位置",
  index_entry_valid: "索引条目匹配", index_path_mismatch: "索引路径不一致", index_entry_missing: "索引条目缺失",
  index_unavailable_or_changed: "索引无法读取、超过限制或检查期间发生变化",
};
