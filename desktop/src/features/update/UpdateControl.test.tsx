import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import UpdateControl from "./UpdateControl";
import { I18nProvider } from "../../lib/i18n";

const updater = vi.hoisted(() => ({
  checkForUpdates: vi.fn(),
  installCheckedUpdate: vi.fn(),
}));

vi.mock("../../lib/updater", () => updater);

function renderUpdateControl(migrationBusy: boolean) {
  return render(
    <I18nProvider>
      <UpdateControl migrationBusy={migrationBusy} onInstallingChange={vi.fn()} />
    </I18nProvider>,
  );
}

describe("UpdateControl", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    updater.checkForUpdates.mockResolvedValue({
      status: "available",
      currentVersion: "0.1.3",
      version: "0.1.4",
      notes: "修复恢复问题",
    });
    updater.installCheckedUpdate.mockResolvedValue(undefined);
  });

  it("checks automatically and installs a signed update after confirmation", async () => {
    const user = userEvent.setup();
    renderUpdateControl(false);

    const install = await screen.findByRole("button", { name: "更新到 0.1.4" });
    expect(screen.getByText("当前 0.1.3")).toBeInTheDocument();

    await user.click(install);

    expect(updater.installCheckedUpdate).toHaveBeenCalledOnce();
    expect(await screen.findByText("安装完成，正在重启…")).toBeInTheDocument();
  });

  it("blocks installation while a migration operation is active", async () => {
    renderUpdateControl(true);

    const install = await screen.findByRole("button", { name: "更新到 0.1.4" });
    expect(install).toBeDisabled();
    expect(screen.getByText("请先完成当前迁移")).toBeInTheDocument();
  });

  it("keeps update-check failures non-blocking and allows retry", async () => {
    const user = userEvent.setup();
    updater.checkForUpdates
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce({ status: "current", currentVersion: "0.1.4" });
    renderUpdateControl(false);

    const retry = await screen.findByRole("button", { name: "重新检查更新" });
    expect(screen.getByText("检查失败，不影响离线迁移")).toBeInTheDocument();

    await user.click(retry);

    expect(await screen.findByText("当前已是最新版")).toBeInTheDocument();
  });
});
