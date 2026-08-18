import { useRef, useState, type RefObject } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Circle,
  FileArchive,
  FolderOpen,
  HardDrive,
  LoaderCircle,
  Play,
  ShieldCheck,
  XCircle,
} from "lucide-react";

import {
  applyRestore,
  buildRestorePlan,
  inspectPackage,
  openRestoredThread,
  selectRestoreDestinations,
} from "../../lib/api";
import { useI18n } from "../../lib/i18n";
import {
  errorMessage,
  registrationIsComplete,
  type CodexInventory,
  type FileConflictResolution,
  type PackagePreview,
  type ProjectRegistration,
  type RegistrationStatus,
  type RestoreLocationSelection,
  type RestorePlan,
  type RestoreReport,
} from "../../lib/types";

interface ReceivePageProps {
  headingRef: RefObject<HTMLHeadingElement | null>;
  inventory: CodexInventory | null;
  onOperationStart: () => void;
  onOperationEnd: () => void;
}

const verificationLabels: Array<[keyof RestoreReport["verification"], string]> = [
  ["package_checksum_valid", "迁移包校验"],
  ["files_valid", "文件完整性"],
  ["sessions_valid", "对话文件"],
  ["session_index_valid", "会话索引"],
  ["sqlite_threads_valid", "线程数据库"],
  ["path_mapping_valid", "跨平台路径"],
  ["forbidden_files_absent", "禁用文件隔离"],
  ["project_files_valid", "项目文件"],
  ["app_registration_valid", "Codex 项目登记"],
  ["app_visible_ready", "Codex 可见状态"],
];

export default function ReceivePage({
  headingRef,
  inventory,
  onOperationStart,
  onOperationEnd,
}: ReceivePageProps) {
  const { t } = useI18n();
  const [preview, setPreview] = useState<PackagePreview | null>(null);
  const [locations, setLocations] = useState<RestoreLocationSelection | null>(null);
  const [plan, setPlan] = useState<RestorePlan | null>(null);
  const [conflictResolution, setConflictResolution] = useState<FileConflictResolution | null>(null);
  const [codexClosed, setCodexClosed] = useState(false);
  const [report, setReport] = useState<RestoreReport | null>(null);
  const [phase, setPhase] = useState<"idle" | "inspecting" | "selecting" | "planning" | "restoring">("idle");
  const [error, setError] = useState<string | null>(null);
  const [registrationStatuses, setRegistrationStatuses] = useState<Record<string, string>>({});
  const requestGeneration = useRef(0);

  async function choosePackage() {
    if (phase !== "idle") return;
    const generation = ++requestGeneration.current;
    setError(null);
    setPhase("inspecting");
    onOperationStart();
    try {
      const inspected = await inspectPackage();
      if (generation !== requestGeneration.current) return;
      if (inspected) {
        setPreview(inspected);
        clearRestoreSelection();
        setLocations(null);
      }
    } catch (caught) {
      if (generation !== requestGeneration.current) return;
      setPreview(null);
      setError(errorMessage(caught));
    } finally {
      if (generation === requestGeneration.current) setPhase("idle");
      onOperationEnd();
    }
  }

  async function chooseLocations() {
    if (!preview || phase !== "idle") return;
    const generation = ++requestGeneration.current;
    setError(null);
    setPhase("selecting");
    onOperationStart();
    try {
      const selected = await selectRestoreDestinations(preview.selection_id);
      if (generation !== requestGeneration.current) return;
      if (selected) {
        setLocations(selected);
        clearRestoreSelection();
      }
    } catch (caught) {
      if (generation !== requestGeneration.current) return;
      setError(errorMessage(caught));
    } finally {
      if (generation === requestGeneration.current) setPhase("idle");
      onOperationEnd();
    }
  }

  async function handlePlan(resolution: FileConflictResolution | null = null) {
    if (!preview || !locations || phase !== "idle") return;
    const generation = ++requestGeneration.current;
    setError(null);
    setReport(null);
    setPhase("planning");
    onOperationStart();
    try {
      const nextPlan = await buildRestorePlan(
        preview.selection_id,
        locations.selection_id,
        resolution ?? undefined,
      );
      if (generation === requestGeneration.current) {
        setPlan(nextPlan);
        setConflictResolution(resolution);
      }
    } catch (caught) {
      if (generation !== requestGeneration.current) return;
      setPlan(null);
      setError(errorMessage(caught));
    } finally {
      if (generation === requestGeneration.current) setPhase("idle");
      onOperationEnd();
    }
  }

  function clearRestoreSelection() {
    setPlan(null);
    setConflictResolution(null);
    setReport(null);
    setCodexClosed(false);
    setRegistrationStatuses({});
  }

  async function handleRestore() {
    if (!plan || plan.conflict_count > 0 || !codexClosed) return;
    setError(null);
    setPhase("restoring");
    onOperationStart();
    try {
      setReport(await applyRestore(plan.plan_id, {
        codex_closed_confirmed: true,
        register_projects: true,
      }));
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setPhase("idle");
      onOperationEnd();
    }
  }

  async function handleOpenRestored(registration: ProjectRegistration) {
    setError(null);
    onOperationStart();
    try {
      const status = await openRestoredThread(registration.project_path, report!.transaction_id);
      setRegistrationStatuses((current) => ({
        ...current,
        [registration.project_id]: registrationStatusMessage(status, t),
      }));
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      onOperationEnd();
    }
  }

  const canPlan = Boolean(preview && locations && phase === "idle");
  const canRestore = Boolean(
    plan && plan.conflict_count === 0 && codexClosed && phase === "idle" && !report,
  );
  const manualRegistration = report?.registrations.some(
    (registration) => !registrationIsComplete(registration.status),
  );

  return (
    <div className="page receive-page">
      <header className="page-header">
        <p className="eyebrow">IMPORT</p>
        <h1 ref={headingRef} tabIndex={-1}>{t("导入 ReHome 包")}</h1>
        <p className="page-description">{t("在新电脑选择迁移包，检查内容后导入本机 Codex。")}</p>
      </header>

      <section className="workflow-section" aria-labelledby="receive-package-title">
        <div className="section-title-row"><div><span className="step-number">1</span><h2 id="receive-package-title">{t("选择迁移包")}</h2></div></div>
        <div className="form-row"><div className="form-label"><FileArchive aria-hidden="true" /><span><strong>{t("ReHome 迁移包")}</strong><small>{preview?.package_path ?? t("尚未选择")}</small></span></div><button className="secondary-button" type="button" onClick={() => void choosePackage()} disabled={phase !== "idle"}>{phase === "inspecting" ? <LoaderCircle className="spin" aria-hidden="true" /> : <FolderOpen aria-hidden="true" />}{t("选择迁移包")}</button></div>
        {preview && (
          <div className="preview-band">
            <div className="preview-facts">
              <span><small>{t("来源系统")}</small><strong>{sourceOsLabel(preview.manifest.source_os)}</strong></span>
              <span><small>{t("项目")}</small><strong>{preview.manifest.counts.projects}</strong></span>
              <span><small>{t("对话")}</small><strong>{t("{count} 个对话", { count: preview.manifest.counts.conversations })}</strong></span>
              <span><small>{t("技能 / 插件 / 图片")}</small><strong>{preview.manifest.counts.skills} / {preview.manifest.counts.plugins} / {preview.manifest.counts.generated_images}</strong></span>
            </div>
            <div className="integrity-row">
              <span className={preview.checksum_valid ? "status status-success" : "status status-error"}>{preview.checksum_valid ? <CheckCircle2 aria-hidden="true" /> : <XCircle aria-hidden="true" />}{t(preview.checksum_valid ? "校验通过" : "校验失败")}</span>
              <code className="hash-text">{preview.archive_hash}</code>
              <span>{t("禁用文件 {count}", { count: preview.forbidden_files_total })}</span>
            </div>
          </div>
        )}
      </section>

      <section className="workflow-section" aria-labelledby="receive-target-title">
        <div className="section-title-row"><div><span className="step-number">2</span><h2 id="receive-target-title">{t("选择保存位置")}</h2></div></div>
        <PathPicker icon={HardDrive} label={t("Codex 数据位置")} value={locations?.target_codex_home ?? inventory?.codex_home ?? t("未检测")} />
        <PathPicker icon={FolderOpen} label={t("项目保存位置")} value={locations?.projects_root ?? t("尚未选择")} buttonLabel={t("选择项目保存位置")} onClick={chooseLocations} disabled={!preview || phase !== "idle"} />
        <div className="command-row"><p>{t("安全备份由 ReHome 自动管理")}</p><button className="command-button" type="button" disabled={!canPlan} onClick={() => void handlePlan()}>{phase === "planning" ? <LoaderCircle className="spin" aria-hidden="true" /> : <ShieldCheck aria-hidden="true" />}{t("预览导入内容")}</button></div>
      </section>

      {plan && (
        <section className="workflow-section" aria-labelledby="restore-plan-title">
          <div className="section-title-row"><div><span className="step-number">3</span><h2 id="restore-plan-title">{t("确认导入内容")}</h2></div><div className="plan-badges"><span>{t("需要 {size}", { size: formatBytes(plan.required_bytes) })}</span><span className={plan.conflict_count ? "status status-error" : "status status-success"}>{plan.conflict_count ? <AlertTriangle aria-hidden="true" /> : <CheckCircle2 aria-hidden="true" />}{t("冲突 {count}", { count: plan.conflict_count })}</span></div></div>
          <div className="destination-line"><span>{t("目标项目目录")}</span><code>{plan.projects_root}</code></div>
          <div className="table-wrap">
            <table className="conflict-table">
              <thead><tr><th>{t("包内来源")}</th><th>{t("目标位置")}</th><th>{t("变更")}</th></tr></thead>
              <tbody>{plan.operations.map((operation) => <tr key={`${operation.package_source}-${operation.target}`}><td><code>{operation.package_source}</code></td><td><code>{operation.target}</code></td><td><span className={`change change-${operation.action}`}>{changeLabel(operation.action, t)}</span></td></tr>)}</tbody>
            </table>
          </div>
          {plan.conflict_count > 0 && (
            <ConflictResolutionPanel
              count={plan.conflict_count}
              resolution={conflictResolution}
              busy={phase === "planning"}
              onResolve={handlePlan}
            />
          )}
          {plan.conflict_count === 0 && conflictResolution && (
            <p className="inline-state status-success" role="status">
              <CheckCircle2 aria-hidden="true" />
              {t(conflictResolution === "keep_existing"
                ? "已选择保留新电脑上的不同文件。"
                : "已选择使用迁移包文件；被替换的文件会自动备份。")}
            </p>
          )}
          <label className="confirmation-row"><input type="checkbox" checked={codexClosed} onChange={(event) => setCodexClosed(event.target.checked)} aria-label={t("确认已保存当前 Codex 工作")} /><span><strong>{t("当前 Codex 工作已保存")}</strong><small>{t("导入完成后请退出并重新打开 Codex，以加载迁移内容。")}</small></span></label>
          <div className="command-row"><ProgressSteps active={phase === "restoring"} complete={Boolean(report)} /><button className="command-button danger-command" type="button" disabled={!canRestore} onClick={() => void handleRestore()}>{phase === "restoring" ? <LoaderCircle className="spin" aria-hidden="true" /> : <Play aria-hidden="true" />}{t(phase === "restoring" ? "正在导入" : "导入到 Codex")}</button></div>
        </section>
      )}

      {error && <p className="inline-state status-error page-error" role="alert"><XCircle aria-hidden="true" />{error}</p>}

      {report && (
        <section className="result-panel" aria-labelledby="restore-result-title">
          <div className="section-title-row"><div><CheckCircle2 aria-hidden="true" /><h2 id="restore-result-title">{t("导入完成")}</h2></div><span className="status status-success">{t("{count} 个文件", { count: report.restored_files })}</span></div>
          <div className="verification-list">
            {verificationLabels.map(([key, label]) => {
              const passed = report.verification[key];
              return <span key={key} className={passed ? "verification-pass" : "verification-fail"}>{passed ? <CheckCircle2 aria-hidden="true" /> : <AlertTriangle aria-hidden="true" />}{t(label)}</span>;
            })}
          </div>
          {manualRegistration && <p className="manual-status" role="status"><AlertTriangle aria-hidden="true" />{t("项目文件已导入，需要在 Codex 中手动打开")}</p>}
          {report.registrations.map((registration) => (
            <div className="registration-row" key={registration.project_id}><code>{registration.project_path}</code><button className="secondary-button" type="button" onClick={() => void handleOpenRestored(registration)}><FolderOpen aria-hidden="true" />{t("在 Codex 中打开")}</button>{registrationStatuses[registration.project_id] && <span role="status">{registrationStatuses[registration.project_id]}</span>}</div>
          ))}
        </section>
      )}
    </div>
  );
}

function PathPicker({ icon: Icon, label, value, buttonLabel, onClick, disabled }: { icon: typeof FolderOpen; label: string; value: string; buttonLabel?: string; onClick?: () => Promise<void>; disabled?: boolean }) {
  return <div className="form-row"><div className="form-label"><Icon aria-hidden="true" /><span><strong>{label}</strong><small>{value}</small></span></div>{buttonLabel && onClick && <button className="secondary-button" type="button" disabled={disabled} onClick={() => void onClick()}><FolderOpen aria-hidden="true" />{buttonLabel}</button>}</div>;
}

function ConflictResolutionPanel({
  count,
  resolution,
  busy,
  onResolve,
}: {
  count: number;
  resolution: FileConflictResolution | null;
  busy: boolean;
  onResolve: (resolution: FileConflictResolution) => Promise<void>;
}) {
  const { t } = useI18n();
  const unresolvedAfterChoice = resolution !== null;
  return (
    <div className="conflict-resolution-panel" role="alert">
      <div className="conflict-resolution-copy">
        <AlertTriangle aria-hidden="true" />
        <span>
          <strong>{t(unresolvedAfterChoice
            ? "仍有 {count} 个无法自动处理的结构冲突。"
            : "发现 {count} 个同名但内容不同的文件。", { count })}</strong>
          <small>{t(unresolvedAfterChoice
            ? "请查看上表中的冲突路径，移开对应文件或目录，或重新选择一个空的项目保存位置后再预览。"
            : "请选择如何处理这些普通文件冲突。")}</small>
        </span>
      </div>
      <div className="conflict-resolution-actions" role="group" aria-label={t("冲突处理方式")}>
        <button
          className="secondary-button"
          type="button"
          aria-pressed={resolution === "keep_existing"}
          disabled={busy || resolution === "keep_existing"}
          onClick={() => void onResolve("keep_existing")}
        >
          <ShieldCheck aria-hidden="true" />{t("保留新电脑文件（推荐）")}
        </button>
        <button
          className="secondary-button"
          type="button"
          aria-pressed={resolution === "use_package"}
          disabled={busy || resolution === "use_package"}
          onClick={() => void onResolve("use_package")}
        >
          <FileArchive aria-hidden="true" />{t("使用迁移包文件")}
        </button>
      </div>
      <p>{t("保留会跳过同名文件；替换会先自动备份新电脑上的原文件。")}</p>
    </div>
  );
}

function ProgressSteps({ active, complete }: { active: boolean; complete: boolean }) {
  const { t } = useI18n();
  const labels = ["检查", "备份", "导入", "完成"];
  return <div className="progress-steps" aria-label={t("导入进度")}>{labels.map((label, index) => <span key={label} className={complete ? "complete" : active && index < 2 ? "active" : ""}>{complete ? <CheckCircle2 aria-hidden="true" /> : <Circle aria-hidden="true" />}{t(label)}</span>)}</div>;
}

function sourceOsLabel(os: "windows" | "macos"): string {
  return os === "macos" ? "macOS" : "Windows";
}

function changeLabel(change: RestorePlan["operations"][number]["action"], t: (key: string) => string): string {
  return t({ add: "新增", update: "更新", unchanged: "不变", preserve: "保留本机", conflict: "冲突" }[change]);
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function registrationStatusMessage(status: RegistrationStatus, t: (key: string) => string): string {
  if (status === "registered") return t("已在 Codex 中登记");
  if (typeof status === "object") return status.invocation_failed.message;
  return t("项目文件已导入，需要在 Codex 中手动打开");
}
