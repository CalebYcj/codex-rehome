import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Download, LoaderCircle, RefreshCw, ShieldCheck } from "lucide-react";
import { version as appVersion } from "../../../package.json";

import {
  checkForUpdates,
  installCheckedUpdate,
  type UpdateCheckResult,
} from "../../lib/updater";
import Modal from "../../components/Modal";
import { useI18n } from "../../lib/i18n";

interface UpdateControlProps {
  migrationBusy: boolean;
  onInstallingChange: (installing: boolean) => void;
  footerContainer?: HTMLElement | null;
}

type UpdateState =
  | { phase: "checking" }
  | { phase: "ready"; result: UpdateCheckResult }
  | {
      phase: "installing";
      result: Extract<UpdateCheckResult, { status: "available" }>;
      percent: number | null;
    }
  | { phase: "installed" }
  | { phase: "error"; source: "check" | "install" };

export default function UpdateControl({
  migrationBusy,
  onInstallingChange,
  footerContainer,
}: UpdateControlProps) {
  const { t } = useI18n();
  const [confirming, setConfirming] = useState(false);
  const [state, setState] = useState<UpdateState>({ phase: "checking" });
  const [currentVersion, setCurrentVersion] = useState(appVersion);
  const checkInFlight = useRef(false);

  const runCheck = useCallback(async () => {
    if (checkInFlight.current) return;
    checkInFlight.current = true;
    setState({ phase: "checking" });
    try {
      const result = await checkForUpdates();
      if (result.currentVersion) setCurrentVersion(result.currentVersion);
      setState({ phase: "ready", result });
    } catch {
      setState({ phase: "error", source: "check" });
    } finally {
      checkInFlight.current = false;
    }
  }, []);

  useEffect(() => {
    void runCheck();
  }, [runCheck]);

  async function install(
    result: Extract<UpdateCheckResult, { status: "available" }>,
  ) {
    if (migrationBusy) return;
    setConfirming(false);
    onInstallingChange(true);
    setState({ phase: "installing", result, percent: null });
    try {
      await installCheckedUpdate(({ percent }) => {
        setState({ phase: "installing", result, percent });
      });
      setState({ phase: "installed" });
    } catch {
      onInstallingChange(false);
      setState({ phase: "error", source: "install" });
    }
  }

  const available =
    state.phase === "ready" && state.result.status === "available"
      ? state.result
      : null;
  const working = ["checking", "installing", "installed"].includes(state.phase);
  const unsupported =
    state.phase === "ready" && state.result.status === "unsupported";
  const shortcutLabel = available
    ? t("版本 {version}，发现更新", { version: currentVersion })
    : t("版本 {version}，检查更新", { version: currentVersion });
  const statusText =
    state.phase === "checking"
      ? t("正在检查更新")
      : state.phase === "installing"
        ? t("正在安装 {percent}", {
            percent: state.percent === null ? "…" : `${state.percent}%`,
          })
        : state.phase === "installed"
          ? t("安装完成，正在重启…")
          : state.phase === "error"
            ? state.source === "install"
              ? t("更新未完成，点击重试")
              : t("检查失败，点击重试")
            : available
              ? t("发现更新")
              : unsupported
                ? t("开发预览模式")
                : t("当前已是最新版");

  function renderDetails() {
    if (state.phase === "checking") {
      return (
        <div className="update-control" role="status">
          <LoaderCircle className="spin" aria-hidden="true" />
          <span>{t("正在检查更新")}</span>
        </div>
      );
    }

    if (state.phase === "installed") {
      return (
        <div className="update-control update-success" role="status">
          <ShieldCheck aria-hidden="true" />
          <span>{t("安装完成，正在重启…")}</span>
        </div>
      );
    }

    if (state.phase === "error") {
      return (
        <div className="update-control update-stack">
          <span>
            {state.source === "install"
              ? t("更新未完成，请重新检查后再试。")
              : t("检查失败，不影响离线迁移")}
          </span>
          <button
            type="button"
            disabled={migrationBusy}
            onClick={() => void runCheck()}
            aria-label={t("重新检查更新")}
          >
            <RefreshCw aria-hidden="true" />
            {t("重新检查")}
          </button>
        </div>
      );
    }

    if (state.phase === "installing") {
      return (
        <div className="update-control update-stack" role="status">
          <span>
            {t("正在安装 {percent}", {
              percent: state.percent === null ? "…" : `${state.percent}%`,
            })}
          </span>
          <progress
            className="update-progress"
            max={100}
            value={state.percent ?? undefined}
            aria-label={t("更新下载进度")}
          />
        </div>
      );
    }

    const { result } = state;
    if (result.status === "unsupported") {
      return (
        <div className="update-control">
          <span>{t("开发预览模式")}</span>
        </div>
      );
    }

    if (result.status === "current") {
      return (
        <div className="update-control update-stack">
          <span>{t("当前已是最新版")}</span>
          <button
            type="button"
            disabled={migrationBusy}
            onClick={() => void runCheck()}
          >
            <RefreshCw aria-hidden="true" />v{result.currentVersion}
          </button>
        </div>
      );
    }

    return (
      <div className="update-control update-available">
        <div className="update-copy">
          <strong>{t("发现新版本")}</strong>
          <span>{t("当前 {version}", { version: result.currentVersion })}</span>
          {migrationBusy && <small>{t("请先完成当前迁移")}</small>}
        </div>
        <button
          type="button"
          disabled={migrationBusy}
          onClick={() => setConfirming(true)}
          aria-label={t("更新到 {version}", { version: result.version })}
          title={
            result.notes ?? t("更新到 {version}", { version: result.version })
          }
        >
          <Download aria-hidden="true" />v{result.version}
        </button>
      </div>
    );
  }

  return (
    <>
      {renderDetails()}
      {footerContainer &&
        createPortal(
          <div className="version-shortcut-group">
            <button
              type="button"
              className={`version-shortcut${available ? " has-update" : ""}`}
              aria-label={shortcutLabel}
              title={migrationBusy ? t("请先完成当前迁移") : statusText}
              disabled={migrationBusy || working || unsupported}
              onClick={() =>
                available ? setConfirming(true) : void runCheck()
              }
            >
              {working ? (
                <LoaderCircle className="spin" aria-hidden="true" />
              ) : available ? (
                <span className="update-dot" aria-hidden="true" />
              ) : (
                <RefreshCw aria-hidden="true" />
              )}
              <span className="version-number">v{currentVersion}</span>
              <span className="version-status" role="status">
                {statusText}
              </span>
            </button>
            {state.phase === "installing" && (
              <progress
                className="update-progress"
                max={100}
                value={state.percent ?? undefined}
                aria-label={t("更新下载进度")}
              />
            )}
          </div>,
          footerContainer,
        )}
      {confirming && available && (
        <Modal
          title={t("现在更新 ReHome？")}
          onClose={() => setConfirming(false)}
        >
          <p>{t("更新会重启 ReHome。请先完成迁移并保存当前工作。")}</p>
          <p>
            {available.currentVersion} → {available.version}
          </p>
          {available.notes && <p className="update-notes">{available.notes}</p>}
          <div className="modal-actions">
            <button
              data-autofocus
              className="secondary-button"
              onClick={() => setConfirming(false)}
            >
              {t("稍后")}
            </button>
            <button
              className="command-button"
              disabled={migrationBusy}
              onClick={() => void install(available)}
            >
              {t("安装并重启")}
            </button>
          </div>
        </Modal>
      )}
    </>
  );
}
