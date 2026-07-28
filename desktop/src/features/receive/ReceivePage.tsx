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
import {
  errorMessage,
  registrationIsComplete,
  type CodexInventory,
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

export default function ReceivePage({ headingRef, inventory }: ReceivePageProps) {
  const [preview, setPreview] = useState<PackagePreview | null>(null);
  const [locations, setLocations] = useState<RestoreLocationSelection | null>(null);
  const [plan, setPlan] = useState<RestorePlan | null>(null);
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
    }
  }

  async function chooseLocations() {
    if (!preview || phase !== "idle") return;
    const generation = ++requestGeneration.current;
    setError(null);
    setPhase("selecting");
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
    }
  }

  async function handlePlan() {
    if (!preview || !locations || phase !== "idle") return;
    const generation = ++requestGeneration.current;
    setError(null);
    setReport(null);
    setPhase("planning");
    try {
      const nextPlan = await buildRestorePlan(preview.selection_id, locations.selection_id);
      if (generation === requestGeneration.current) setPlan(nextPlan);
    } catch (caught) {
      if (generation !== requestGeneration.current) return;
      setPlan(null);
      setError(errorMessage(caught));
    } finally {
      if (generation === requestGeneration.current) setPhase("idle");
    }
  }

  function clearRestoreSelection() {
    setPlan(null);
    setReport(null);
    setCodexClosed(false);
    setRegistrationStatuses({});
  }

  async function handleRestore() {
    if (!plan || plan.conflict_count > 0 || !codexClosed) return;
    setError(null);
    setPhase("restoring");
    try {
      setReport(await applyRestore(plan.plan_id, {
        codex_closed_confirmed: true,
        register_projects: true,
      }));
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setPhase("idle");
    }
  }

  async function handleOpenRestored(registration: ProjectRegistration) {
    setError(null);
    try {
      const status = await openRestoredThread(registration.project_path, report!.transaction_id);
      setRegistrationStatuses((current) => ({
        ...current,
        [registration.project_id]: registrationStatusMessage(status),
      }));
    } catch (caught) {
      setError(errorMessage(caught));
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
        <p className="eyebrow">RECEIVE</p>
        <h1 ref={headingRef} tabIndex={-1}>接收交接</h1>
        <p className="page-description">先检查包内容和目标差异，再执行本机恢复。</p>
      </header>

      <section className="workflow-section" aria-labelledby="receive-package-title">
        <div className="section-title-row"><div><span className="step-number">1</span><h2 id="receive-package-title">检查迁移包</h2></div></div>
        <div className="form-row"><div className="form-label"><FileArchive aria-hidden="true" /><span><strong>ReHome 包</strong><small>{preview?.package_path ?? "尚未选择"}</small></span></div><button className="secondary-button" type="button" onClick={() => void choosePackage()} disabled={phase !== "idle"}>{phase === "inspecting" ? <LoaderCircle className="spin" aria-hidden="true" /> : <FolderOpen aria-hidden="true" />}选择 ReHome 包</button></div>
        {preview && (
          <div className="preview-band">
            <div className="preview-facts">
              <span><small>来源系统</small><strong>{sourceOsLabel(preview.manifest.source_os)}</strong></span>
              <span><small>项目</small><strong>{preview.manifest.counts.projects}</strong></span>
              <span><small>对话</small><strong>{preview.manifest.counts.conversations} 个对话</strong></span>
              <span><small>技能 / 插件 / 图片</small><strong>{preview.manifest.counts.skills} / {preview.manifest.counts.plugins} / {preview.manifest.counts.generated_images}</strong></span>
            </div>
            <div className="integrity-row">
              <span className={preview.checksum_valid ? "status status-success" : "status status-error"}>{preview.checksum_valid ? <CheckCircle2 aria-hidden="true" /> : <XCircle aria-hidden="true" />}{preview.checksum_valid ? "校验通过" : "校验失败"}</span>
              <code className="hash-text">{preview.archive_hash}</code>
              <span>禁用文件 {preview.forbidden_files_total}</span>
            </div>
          </div>
        )}
      </section>

      <section className="workflow-section" aria-labelledby="receive-target-title">
        <div className="section-title-row"><div><span className="step-number">2</span><h2 id="receive-target-title">选择目标位置</h2></div></div>
        <PathPicker icon={HardDrive} label="Codex 数据位置" value={locations?.target_codex_home ?? inventory?.codex_home ?? "未检测"} />
        <PathPicker icon={FolderOpen} label="项目恢复位置" value={locations?.projects_root ?? "尚未选择"} buttonLabel="选择恢复位置" onClick={chooseLocations} disabled={!preview || phase !== "idle"} />
        <div className="command-row"><p>安全备份由 ReHome 自动管理</p><button className="command-button" type="button" disabled={!canPlan} onClick={() => void handlePlan()}>{phase === "planning" ? <LoaderCircle className="spin" aria-hidden="true" /> : <ShieldCheck aria-hidden="true" />}生成恢复计划</button></div>
      </section>

      {plan && (
        <section className="workflow-section" aria-labelledby="restore-plan-title">
          <div className="section-title-row"><div><span className="step-number">3</span><h2 id="restore-plan-title">确认变更</h2></div><div className="plan-badges"><span>需要 {formatBytes(plan.required_bytes)}</span><span className={plan.conflict_count ? "status status-error" : "status status-success"}>{plan.conflict_count ? <AlertTriangle aria-hidden="true" /> : <CheckCircle2 aria-hidden="true" />}冲突 {plan.conflict_count}</span></div></div>
          <div className="destination-line"><span>目标项目目录</span><code>{plan.projects_root}</code></div>
          <div className="table-wrap">
            <table className="conflict-table">
              <thead><tr><th>包内来源</th><th>目标位置</th><th>变更</th></tr></thead>
              <tbody>{plan.operations.map((operation) => <tr key={`${operation.package_source}-${operation.target}`}><td><code>{operation.package_source}</code></td><td><code>{operation.target}</code></td><td><span className={`change change-${operation.action}`}>{changeLabel(operation.action)}</span></td></tr>)}</tbody>
            </table>
          </div>
          {plan.conflict_count > 0 && <p className="inline-state status-error" role="alert"><AlertTriangle aria-hidden="true" />请先处理冲突，再重新生成恢复计划。</p>}
          <label className="confirmation-row"><input type="checkbox" checked={codexClosed} onChange={(event) => setCodexClosed(event.target.checked)} aria-label="确认已保存当前 Codex 工作" /><span><strong>当前 Codex 工作已保存</strong><small>恢复完成后请退出并重新打开 Codex，以加载迁移内容。</small></span></label>
          <div className="command-row"><ProgressSteps active={phase === "restoring"} complete={Boolean(report)} /><button className="command-button danger-command" type="button" disabled={!canRestore} onClick={() => void handleRestore()}>{phase === "restoring" ? <LoaderCircle className="spin" aria-hidden="true" /> : <Play aria-hidden="true" />}开始恢复</button></div>
        </section>
      )}

      {error && <p className="inline-state status-error page-error" role="alert"><XCircle aria-hidden="true" />{error}</p>}

      {report && (
        <section className="result-panel" aria-labelledby="restore-result-title">
          <div className="section-title-row"><div><CheckCircle2 aria-hidden="true" /><h2 id="restore-result-title">恢复事务已提交</h2></div><span className="status status-success">{report.restored_files} 个文件</span></div>
          <div className="verification-list">
            {verificationLabels.map(([key, label]) => {
              const passed = report.verification[key];
              return <span key={key} className={passed ? "verification-pass" : "verification-fail"}>{passed ? <CheckCircle2 aria-hidden="true" /> : <AlertTriangle aria-hidden="true" />}{label}</span>;
            })}
          </div>
          {manualRegistration && <p className="manual-status" role="status"><AlertTriangle aria-hidden="true" />项目文件已恢复，需要在 Codex 中手动打开</p>}
          {report.registrations.map((registration) => (
            <div className="registration-row" key={registration.project_id}><code>{registration.project_path}</code><button className="secondary-button" type="button" onClick={() => void handleOpenRestored(registration)}><FolderOpen aria-hidden="true" />在 Codex 中打开</button>{registrationStatuses[registration.project_id] && <span role="status">{registrationStatuses[registration.project_id]}</span>}</div>
          ))}
        </section>
      )}
    </div>
  );
}

function PathPicker({ icon: Icon, label, value, buttonLabel, onClick, disabled }: { icon: typeof FolderOpen; label: string; value: string; buttonLabel?: string; onClick?: () => Promise<void>; disabled?: boolean }) {
  return <div className="form-row"><div className="form-label"><Icon aria-hidden="true" /><span><strong>{label}</strong><small>{value}</small></span></div>{buttonLabel && onClick && <button className="secondary-button" type="button" disabled={disabled} onClick={() => void onClick()}><FolderOpen aria-hidden="true" />{buttonLabel}</button>}</div>;
}

function ProgressSteps({ active, complete }: { active: boolean; complete: boolean }) {
  const labels = ["检查", "备份", "写入", "验证"];
  return <div className="progress-steps" aria-label="恢复进度">{labels.map((label, index) => <span key={label} className={complete ? "complete" : active && index < 2 ? "active" : ""}>{complete ? <CheckCircle2 aria-hidden="true" /> : <Circle aria-hidden="true" />}{label}</span>)}</div>;
}

function sourceOsLabel(os: "windows" | "macos"): string {
  return os === "macos" ? "macOS" : "Windows";
}

function changeLabel(change: RestorePlan["operations"][number]["action"]): string {
  return { add: "新增", update: "更新", unchanged: "不变", conflict: "冲突" }[change];
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function registrationStatusMessage(status: RegistrationStatus): string {
  if (status === "registered") return "已在 Codex 中登记";
  if (typeof status === "object") return status.invocation_failed.message;
  return "项目文件已恢复，需要在 Codex 中手动打开";
}
