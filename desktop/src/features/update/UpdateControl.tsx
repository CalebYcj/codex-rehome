import { useCallback, useEffect, useState } from "react";
import { Download, LoaderCircle, RefreshCw, ShieldCheck } from "lucide-react";

import {
  checkForUpdates,
  installCheckedUpdate,
  type UpdateCheckResult,
} from "../../lib/updater";
import { useI18n } from "../../lib/i18n";

interface UpdateControlProps {
  migrationBusy: boolean;
  onInstallingChange: (installing: boolean) => void;
}

type UpdateState =
  | { phase: "checking" }
  | { phase: "ready"; result: UpdateCheckResult }
  | { phase: "installing"; result: Extract<UpdateCheckResult, { status: "available" }>; percent: number | null }
  | { phase: "installed" }
  | { phase: "error" };

export default function UpdateControl({ migrationBusy, onInstallingChange }: UpdateControlProps) {
  const { t } = useI18n();
  const [state, setState] = useState<UpdateState>({ phase: "checking" });

  const runCheck = useCallback(async () => {
    setState({ phase: "checking" });
    try {
      setState({ phase: "ready", result: await checkForUpdates() });
    } catch {
      setState({ phase: "error" });
    }
  }, []);

  useEffect(() => {
    void runCheck();
  }, [runCheck]);

  async function install(result: Extract<UpdateCheckResult, { status: "available" }>) {
    if (migrationBusy) return;
    onInstallingChange(true);
    setState({ phase: "installing", result, percent: null });
    try {
      await installCheckedUpdate(({ percent }) => {
        setState({ phase: "installing", result, percent });
      });
      setState({ phase: "installed" });
    } catch {
      onInstallingChange(false);
      setState({ phase: "error" });
    }
  }

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
        <span>{t("检查失败，不影响离线迁移")}</span>
        <button type="button" onClick={() => void runCheck()} aria-label={t("重新检查更新")}>
          <RefreshCw aria-hidden="true" />{t("重新检查")}
        </button>
      </div>
    );
  }

  if (state.phase === "installing") {
    return (
      <div className="update-control update-stack" role="status">
        <span>{t("正在安装 {percent}", { percent: state.percent === null ? "…" : `${state.percent}%` })}</span>
        <div className="update-progress" aria-hidden="true">
          <span style={{ width: `${state.percent ?? 8}%` }} />
        </div>
      </div>
    );
  }

  const { result } = state;
  if (result.status === "unsupported") {
    return <div className="update-control"><span>{t("开发预览模式")}</span></div>;
  }

  if (result.status === "current") {
    return (
      <div className="update-control update-stack">
        <span>{t("当前已是最新版")}</span>
        <button type="button" onClick={() => void runCheck()}>
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
        onClick={() => void install(result)}
        aria-label={t("更新到 {version}", { version: result.version })}
        title={result.notes ?? t("更新到 {version}", { version: result.version })}
      >
        <Download aria-hidden="true" />v{result.version}
      </button>
    </div>
  );
}
