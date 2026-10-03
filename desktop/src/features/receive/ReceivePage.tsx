import { useEffect, useRef, useState, type RefObject } from "react";
import {
  AlertTriangle,
  CheckCircle2,
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
import Modal from "../../components/Modal";
import StepBar from "../../components/StepBar";
import SupportPanel from "../support/SupportPanel";
import {
  errorMessage,
  supportIdFromError,
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
  onHome: () => void;
  onHistory: () => void;
}

const verificationLabels: Array<[keyof RestoreReport["verification"], string]> =
  [
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
  onHome,
  onHistory,
}: ReceivePageProps) {
  const { t } = useI18n();
  const [step, setStep] = useState<
    "package" | "location" | "review" | "result"
  >("package");
  const [useDialog, setUseDialog] = useState(false);
  const [useChecks, setUseChecks] = useState([false, false, false]);
  const [useConfirmed, setUseConfirmed] = useState(false);
  const [opening, setOpening] = useState<string | null>(null);
  const [preview, setPreview] = useState<PackagePreview | null>(null);
  const [locations, setLocations] = useState<RestoreLocationSelection | null>(
    null,
  );
  const [plan, setPlan] = useState<RestorePlan | null>(null);
  const [conflictResolution, setConflictResolution] =
    useState<FileConflictResolution | null>(null);
  const [codexClosed, setCodexClosed] = useState(false);
  const [report, setReport] = useState<RestoreReport | null>(null);
  const [phase, setPhase] = useState<
    "idle" | "inspecting" | "selecting" | "planning" | "restoring"
  >("idle");
  const [error, setError] = useState<string | null>(null);
  const [supportId, setSupportId] = useState<string | null>(null);
  const [registrationStatuses, setRegistrationStatuses] = useState<
    Record<string, string>
  >({});
  const requestGeneration = useRef(0);

  async function choosePackage() {
    if (phase !== "idle") return;
    const generation = ++requestGeneration.current;
    setError(null);
    setSupportId(null);
    setPhase("inspecting");
    onOperationStart();
    try {
      const inspected = await inspectPackage();
      if (generation !== requestGeneration.current) return;
      if (inspected) {
        setPreview(inspected);
        setStep("package");
        clearRestoreSelection();
        if (inspected.manifest.counts.projects === 0) {
          const selected = await selectRestoreDestinations(
            inspected.selection_id,
          );
          if (generation !== requestGeneration.current) return;
          setLocations(selected);
        } else {
          setLocations(null);
        }
      }
    } catch (caught) {
      if (generation !== requestGeneration.current) return;
      setPreview(null);
      setSupportId(supportIdFromError(caught));
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
    setSupportId(null);
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
      setSupportId(supportIdFromError(caught));
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
    setSupportId(null);
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
        setCodexClosed(false);
        setStep("review");
      }
    } catch (caught) {
      if (generation !== requestGeneration.current) return;
      setPlan(null);
      setSupportId(supportIdFromError(caught));
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
    setUseConfirmed(false);
    setUseChecks([false, false, false]);
    setUseDialog(false);
  }

  async function handleRestore() {
    if (!plan || plan.conflict_count > 0 || !codexClosed || phase !== "idle")
      return;
    setError(null);
    setSupportId(null);
    setPhase("restoring");
    onOperationStart();
    try {
      const restored = await applyRestore(plan.plan_id, {
        codex_closed_confirmed: true,
        register_projects: true,
      });
      setReport(restored);
      setStep("result");
    } catch (caught) {
      // A failed attempt consumes its capability and may have rolled back writes.
      // Re-plan against the current files instead of retrying the stale snapshot.
      clearRestoreSelection();
      setStep("location");
      setSupportId(supportIdFromError(caught));
      setError(
        `${errorMessage(caught)} ${t("请重新预览导入内容后重试；如提示回滚失败，请先在迁移记录中恢复。")}`,
      );
    } finally {
      setPhase("idle");
      onOperationEnd();
    }
  }

  async function handleOpenRestored(registration: ProjectRegistration) {
    if (!report || opening) return;
    setOpening(registration.project_id);
    setError(null);
    onOperationStart();
    try {
      const status = await openRestoredThread(
        registration.project_path,
        report!.transaction_id,
      );
      setRegistrationStatuses((current) => ({
        ...current,
        [registration.project_id]: registrationStatusMessage(status, t),
      }));
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setOpening(null);
      onOperationEnd();
    }
  }

  const canPlan = Boolean(preview && locations && phase === "idle");
  const projectLocationRequired = Boolean(
    preview && preview.manifest.counts.projects > 0,
  );
  const canRestore = Boolean(
    plan &&
      plan.conflict_count === 0 &&
      codexClosed &&
      phase === "idle" &&
      !report,
  );
  const planOperations = plan
    ? [...plan.operations].sort(
        (left, right) =>
          Number(right.action === "conflict") -
          Number(left.action === "conflict"),
      )
    : [];
  const manualRegistration = report?.registrations.some(
    (registration) => !registrationIsComplete(registration.status),
  );

  useEffect(() => {
    if (!error && !supportId) headingRef.current?.focus();
  }, [step, phase, error, supportId]);
  const busy = phase !== "idle" || opening !== null;
  const stepLabel = t(
    {
      package: "导入 · 1 / 4",
      location: "导入 · 2 / 4",
      review: "导入 · 3 / 4",
      result: "导入 · 4 / 4",
    }[step],
  );
  function back() {
    if (busy) return;
    if (step === "package") onHome();
    else if (step === "location") setStep("package");
    else if (step === "review") {
      setPlan(null);
      setCodexClosed(false);
      setStep("location");
    } else onHome();
  }
  function confirmUse() {
    if (!report || !useChecks.every(Boolean)) return;
    try {
      window.localStorage.setItem(
        "rehome-use-confirmed:" + report.transaction_id,
        new Date().toISOString(),
      );
    } catch {
      /* User confirmation remains valid for this session. */
    }
    setUseConfirmed(true);
    setUseDialog(false);
  }
  if (phase === "restoring")
    return (
      <div className="page processing-page" aria-busy="true">
        <LoaderCircle className="processing-icon spin" aria-hidden="true" />
        <h1 ref={headingRef} tabIndex={-1}>
          {t("正在恢复你的工作")}
        </h1>
        <p role="status">{t("请保持 ReHome 打开，完成后再重启 Codex。")}</p>
        <p className="muted">{t("内容较多时可能需要几分钟。")}</p>
      </div>
    );
  return (
    <div className="page receive-page">
      <StepBar label={stepLabel} onBack={back} busy={busy} />
      <header className="page-header">
        <h1 ref={headingRef} tabIndex={-1}>
          {t(
            {
              package: "打开你的迁移包",
              location: "工作放在哪里？",
              review: plan?.conflict_count ? "先处理文件冲突" : "确认导入",
              result: "文件已恢复，再确认一下。",
            }[step],
          )}
        </h1>
        <p className="page-description">
          {t(
            {
              package: "选择旧电脑导出的 .rehome 文件。",
              location: "项目放到你选的文件夹，Codex 数据恢复到本机。",
              review: "确认位置与变更，保存当前工作后开始。",
              result: "重启 Codex，检查原项目和旧对话。",
            }[step],
          )}
        </p>
      </header>
      {step === "package" && (
        <section
          className="workflow-section"
          aria-labelledby="receive-package-title"
        >
          <h2 className="list-heading" id="receive-package-title">
            {t("选择迁移包")}
          </h2>
          <div className="form-row">
            <div className="form-label">
              <FileArchive aria-hidden="true" />
              <span>
                <strong>
                  {preview?.package_path.split(/[\\/]/).pop() ??
                    t("ReHome 迁移包")}
                </strong>
                <small>{preview?.package_path ?? t("尚未选择")}</small>
              </span>
            </div>
            <button
              className="secondary-button"
              onClick={() => void choosePackage()}
              disabled={busy}
            >
              <FolderOpen aria-hidden="true" />
              {t("选择迁移包")}
            </button>
          </div>
          {preview && (
            <div className="package-summary">
              <div className="summary-row">
                <span>{t("来源系统")}</span>
                <strong>{sourceOsLabel(preview.manifest.source_os)}</strong>
              </div>
              <div className="summary-row">
                <span>{t("项目与对话")}</span>
                <strong>
                  {t("{count} 个项目", {
                    count: preview.manifest.counts.projects,
                  })}{" "}
                  ·{" "}
                  {t("{count} 个对话", {
                    count: preview.manifest.counts.conversations,
                  })}
                </strong>
              </div>
              <div className="summary-row">
                <span>Skills / Plugins / {t("生成图片")}</span>
                <strong>
                  {preview.manifest.counts.skills} /{" "}
                  {preview.manifest.counts.plugins} /{" "}
                  {preview.manifest.counts.generated_images}
                </strong>
              </div>
              <p
                className={
                  preview.checksum_valid
                    ? "status status-success"
                    : "status status-error"
                }
              >
                {preview.checksum_valid ? (
                  <CheckCircle2 aria-hidden="true" />
                ) : (
                  <XCircle aria-hidden="true" />
                )}
                {t(preview.checksum_valid ? "文件完整性校验通过" : "校验失败")}
              </p>
              <details className="technical-details">
                <summary>{t("查看校验详情")}</summary>
                <code>SHA-256: {preview.archive_hash}</code>
                <p>
                  {t("禁用文件 {count}", {
                    count: preview.forbidden_files_total,
                  })}
                </p>
              </details>
            </div>
          )}
          <footer className="action-footer">
            <span className="muted">{t("迁移包不会上传。")}</span>
            <button
              className="command-button"
              disabled={!preview?.checksum_valid || busy}
              onClick={() => setStep("location")}
            >
              {t("继续")}
            </button>
          </footer>
        </section>
      )}
      {step === "location" && (
        <section
          className="workflow-section"
          aria-labelledby="receive-target-title"
        >
          <h2 className="list-heading" id="receive-target-title">
            {t("选择保存位置")}
          </h2>
          <PathPicker
            icon={HardDrive}
            label={t("Codex 数据位置")}
            value={
              locations?.target_codex_home ??
              inventory?.codex_home ??
              t("未检测")
            }
          />
          <PathPicker
            icon={FolderOpen}
            label={t("项目保存位置")}
            value={
              projectLocationRequired
                ? (locations?.projects_root ?? t("尚未选择"))
                : t("迁移包不含项目文件，无需选择")
            }
            buttonLabel={
              projectLocationRequired ? t("选择项目保存位置") : undefined
            }
            onClick={projectLocationRequired ? chooseLocations : undefined}
            disabled={busy}
          />
          <p className="privacy-note">{t("安全备份由 ReHome 自动管理")}</p>
          <footer className="action-footer">
            <span className="muted">{t("预览不会写入迁移内容。")}</span>
            <button
              className="command-button"
              disabled={!canPlan}
              onClick={() => void handlePlan()}
            >
              {phase === "planning" && (
                <LoaderCircle className="spin" aria-hidden="true" />
              )}
              {t("预览导入内容")}
            </button>
          </footer>
        </section>
      )}
      {step === "review" && plan && (
        <section
          className="workflow-section"
          aria-labelledby="restore-plan-title"
        >
          <div className="section-title-row">
            <h2 id="restore-plan-title">{t("确认导入内容")}</h2>
            <span
              className={
                plan.conflict_count
                  ? "status status-error"
                  : "status status-success"
              }
            >
              {t("冲突 {count}", { count: plan.conflict_count })}
            </span>
          </div>
          <div className="summary-row">
            <span>{t("需要空间")}</span>
            <strong>{formatBytes(plan.required_bytes)}</strong>
          </div>
          <div className="summary-row">
            <span>{t("Codex 数据位置")}</span>
            <code>{plan.target_codex_home}</code>
          </div>
          {projectLocationRequired && (
            <div className="summary-row">
              <span>{t("目标项目目录")}</span>
              <code>{plan.projects_root}</code>
            </div>
          )}
          <details
            className="technical-details"
            open={plan.conflict_count > 0 ? true : undefined}
          >
            <summary>{t("查看文件变更")}</summary>
            <div className="table-wrap">
              <table className="conflict-table">
                <thead>
                  <tr>
                    <th>{t("包内来源")}</th>
                    <th>{t("目标位置")}</th>
                    <th>{t("变更")}</th>
                  </tr>
                </thead>
                <tbody>
                  {planOperations.map((operation) => (
                    <tr key={operation.package_source + operation.target}>
                      <td>
                        <code>{operation.package_source}</code>
                      </td>
                      <td>
                        <code>{operation.target}</code>
                      </td>
                      <td>
                        <span className={"change change-" + operation.action}>
                          {changeLabel(
                            operation.action,
                            t,
                            conflictResolution !== null,
                          )}
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </details>
          {plan.conflict_count > 0 ? (
            <ConflictResolutionPanel
              count={plan.conflict_count}
              resolution={conflictResolution}
              busy={busy}
              onResolve={handlePlan}
            />
          ) : (
            <>
              {conflictResolution && (
                <p className="inline-state status-success" role="status">
                  {t(
                    conflictResolution === "keep_existing"
                      ? "已选择保留新电脑上的不同文件。"
                      : "已选择使用迁移包文件；被替换的文件会自动备份。",
                  )}
                </p>
              )}
              <label className="confirmation-row">
                <input
                  type="checkbox"
                  checked={codexClosed}
                  disabled={busy}
                  onChange={(event) => setCodexClosed(event.target.checked)}
                  aria-label={t("确认已保存当前 Codex 工作")}
                />
                <span>
                  <strong>{t("当前 Codex 工作已保存")}</strong>
                  <small>
                    {t("导入完成后请退出并重新打开 Codex，以加载迁移内容。")}
                  </small>
                </span>
              </label>
            </>
          )}
          <footer className="action-footer">
            <p className="muted">{t("替换前自动备份，发生错误保留诊断。")}</p>
            <button
              className="command-button"
              disabled={!canRestore}
              onClick={() => void handleRestore()}
            >
              <Play aria-hidden="true" />
              {t("导入到 Codex")}
            </button>
          </footer>
        </section>
      )}
      {step === "result" && report && (
        <section
          className="restore-result"
          aria-labelledby="restore-result-title"
        >
          <div className="section-title-row">
            <h2 id="restore-result-title">{t("导入完成")}</h2>
            <span className="status">
              {t("{count} 个文件", { count: report.restored_files })}
            </span>
          </div>
          <div className="result-status-row">
            <CheckCircle2 aria-hidden="true" />
            <span>{t("系统检查")}</span>
            <strong>
              {t(
                Object.entries(report.verification)
                  .filter(
                    ([key]) =>
                      key !== "app_visible_ready" &&
                      key !== "app_registration_valid",
                  )
                  .every(([, value]) => value)
                  ? "通过"
                  : "存在异常",
              )}
            </strong>
          </div>
          <div className="result-status-row">
            <CircleStatus confirmed={useConfirmed} />
            <span>{t("旧对话实际使用")}</span>
            <strong>
              {t(useConfirmed ? "已由你确认能用" : "实际使用待确认")}
            </strong>
          </div>
          <p className="manual-status">
            {t(
              "文件和索引已导入。请重启 Codex，打开原对话并继续发送一条消息，确认可以使用。",
            )}
          </p>
          <details className="technical-details">
            <summary>{t("查看系统检查详情")}</summary>
            <div className="verification-list">
              {verificationLabels.map(([key, label]) => (
                <span
                  key={key}
                  className={
                    report.verification[key]
                      ? "verification-pass"
                      : "verification-fail"
                  }
                >
                  {report.verification[key] ? (
                    <CheckCircle2 aria-hidden="true" />
                  ) : (
                    <AlertTriangle aria-hidden="true" />
                  )}
                  {t(
                    key === "app_visible_ready" && !report.verification[key]
                      ? "对话可见性待确认"
                      : label,
                  )}
                </span>
              ))}
            </div>
          </details>
          {manualRegistration && (
            <p className="manual-status" role="status">
              {t("项目文件已导入，需要在 Codex 中手动打开")}
            </p>
          )}
          {report.registrations.map((registration) => {
            const message =
              registrationStatuses[registration.project_id] ??
              (typeof registration.status === "object"
                ? registration.status.invocation_failed.message
                : null);
            return (
              <div className="registration-row" key={registration.project_id}>
                <code>{registration.project_path}</code>
                <button
                  className="secondary-button"
                  disabled={busy}
                  onClick={() => void handleOpenRestored(registration)}
                >
                  <FolderOpen aria-hidden="true" />
                  {t("在 Codex 中打开")}
                </button>
                {message && <span role="status">{message}</span>}
              </div>
            );
          })}
          <SupportPanel
            key={report.transaction_id}
            source={{
              kind: "transaction",
              transaction_id: report.transaction_id,
            }}
          />
          <footer className="action-footer">
            <button
              className="secondary-button"
              disabled={busy}
              onClick={onHistory}
            >
              {t("查看迁移记录")}
            </button>
            <button
              className="command-button"
              disabled={busy || useConfirmed}
              onClick={() => {
                setUseChecks([false, false, false]);
                setUseDialog(true);
              }}
            >
              {t("确认旧对话能用")}
            </button>
          </footer>
          <button
            className="icon-text-button new-import"
            disabled={busy}
            onClick={() => {
              setPreview(null);
              setLocations(null);
              clearRestoreSelection();
              setStep("package");
            }}
          >
            {t("导入另一个迁移包")}
          </button>
        </section>
      )}
      {phase !== "idle" && (
        <p role="status" className="inline-state">
          <LoaderCircle className="spin" aria-hidden="true" />
          {t("正在处理，请稍候…")}
        </p>
      )}
      {error && (
        <p className="inline-state status-error page-error" role="alert">
          <XCircle aria-hidden="true" />
          {error}
        </p>
      )}
      {supportId && (
        <SupportPanel
          key={supportId}
          source={{ kind: "incident", support_id: supportId }}
        />
      )}
      {useDialog && (
        <Modal
          title={t("旧对话能正常使用吗？")}
          onClose={() => setUseDialog(false)}
        >
          <p>{t("请先重启 Codex，完成下面三项检查。")}</p>
          {[
            "原项目或对话能找到",
            "旧消息仍然完整可见",
            "在原对话发送消息并收到回答",
          ].map((label, index) => (
            <label className="confirmation-row" key={label}>
              <input
                type="checkbox"
                checked={useChecks[index]}
                onChange={(event) =>
                  setUseChecks((current) =>
                    current.map((value, i) =>
                      i === index ? event.target.checked : value,
                    ),
                  )
                }
              />
              <span>{t(label)}</span>
            </label>
          ))}
          <p className="privacy-note">
            {t("这是你的使用确认，不是 ReHome 自动验证的结果。")}
          </p>
          <div className="modal-actions">
            <button
              data-autofocus
              className="secondary-button"
              onClick={() => setUseDialog(false)}
            >
              {t("还没确认")}
            </button>
            <button
              className="command-button"
              disabled={!useChecks.every(Boolean)}
              onClick={confirmUse}
            >
              {t("确认能用")}
            </button>
          </div>
        </Modal>
      )}
    </div>
  );
}
function CircleStatus({ confirmed }: { confirmed: boolean }) {
  return confirmed ? (
    <CheckCircle2 aria-hidden="true" />
  ) : (
    <AlertTriangle aria-hidden="true" />
  );
}
function PathPicker({
  icon: Icon,
  label,
  value,
  buttonLabel,
  onClick,
  disabled,
}: {
  icon: typeof FolderOpen;
  label: string;
  value: string;
  buttonLabel?: string;
  onClick?: () => Promise<void>;
  disabled?: boolean;
}) {
  return (
    <div className="form-row">
      <div className="form-label">
        <Icon aria-hidden="true" />
        <span>
          <strong>{label}</strong>
          <small>{value}</small>
        </span>
      </div>
      {buttonLabel && onClick && (
        <button
          className="secondary-button"
          disabled={disabled}
          onClick={() => void onClick()}
        >
          <FolderOpen aria-hidden="true" />
          {buttonLabel}
        </button>
      )}
    </div>
  );
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
  const [choice, setChoice] = useState<FileConflictResolution | null>(
    resolution,
  );
  useEffect(() => setChoice(resolution), [resolution]);
  return (
    <div className="conflict-resolution-panel" role="alert">
      <strong>
        {t(
          resolution !== null
            ? "仍有 {count} 个无法自动处理的结构冲突。"
            : "发现 {count} 个同名但内容不同的文件。",
          { count },
        )}
      </strong>
      <p>
        {t(
          resolution !== null
            ? "请查看上表中的冲突路径，移开对应文件或目录，或重新选择一个空的项目保存位置后再预览。"
            : "请选择如何处理这些普通文件冲突。",
        )}
      </p>
      <div
        className="conflict-resolution-actions"
        role="group"
        aria-label={t("冲突处理方式")}
      >
        <button
          className="secondary-button"
          aria-pressed={choice === "keep_existing"}
          disabled={busy}
          onClick={() => setChoice("keep_existing")}
        >
          {t("保留新电脑文件（推荐）")}
        </button>
        <button
          className="secondary-button"
          aria-pressed={choice === "use_package"}
          disabled={busy}
          onClick={() => setChoice("use_package")}
        >
          {t("使用迁移包文件")}
        </button>
      </div>
      <p>{t("保留会跳过同名文件；替换会先自动备份新电脑上的原文件。")}</p>
      <button
        className="command-button"
        disabled={busy || !choice}
        onClick={() => choice && void onResolve(choice)}
      >
        {t("应用选择并重新预览")}
      </button>
    </div>
  );
}
function sourceOsLabel(os: "windows" | "macos"): string {
  return os === "macos" ? "macOS" : "Windows";
}

function changeLabel(
  change: RestorePlan["operations"][number]["action"],
  t: (key: string) => string,
  structuralConflict: boolean,
): string {
  if (change === "conflict" && structuralConflict) return t("结构冲突");
  return t(
    {
      add: "新增",
      update: "更新",
      unchanged: "不变",
      preserve: "保留本机",
      conflict: "冲突",
    }[change],
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function registrationStatusMessage(
  status: RegistrationStatus,
  t: (key: string) => string,
): string {
  if (status === "registered") return t("已在 Codex 中登记");
  if (typeof status === "object") return status.invocation_failed.message;
  return t("项目文件已导入，需要在 Codex 中手动打开");
}
