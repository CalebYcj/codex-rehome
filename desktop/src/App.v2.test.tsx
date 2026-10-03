import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";
import { version as appVersion } from "../package.json";

const api = vi.hoisted(() => ({
  applyRestore: vi.fn(),
  buildRestorePlan: vi.fn(),
  createPackage: vi.fn(),
  discoverCodex: vi.fn(),
  inspectPackage: vi.fn(),
  listTransactions: vi.fn(),
  openPath: vi.fn(),
  openRestoredThread: vi.fn(),
  rollbackTransaction: vi.fn(),
  selectRestoreDestinations: vi.fn(),
  prepareSupport: vi.fn(),
  copySupportText: vi.fn(),
  openSupportIssue: vi.fn(),
}));

const updater = vi.hoisted(() => ({
  checkForUpdates: vi.fn(),
  installCheckedUpdate: vi.fn(),
}));

vi.mock("./lib/api", () => api);
vi.mock("./lib/updater", () => updater);

import {
  inventory,
  preview,
  basePlan,
  committedTransaction,
} from "./test/fixtures";

beforeEach(() => {
  vi.resetAllMocks();
  window.localStorage.clear();
  api.discoverCodex.mockResolvedValue(inventory);
  api.listTransactions.mockResolvedValue({ transactions: [], warnings: [] });
  api.inspectPackage.mockResolvedValue(preview);
  api.selectRestoreDestinations.mockResolvedValue({
    selection_id: "13131313-1313-4131-8131-131313131313",
    target_codex_home: inventory.codex_home,
    projects_root: "C:\\Restored Projects",
    backup_root: "C:\\ReHome Backups",
  });
  api.buildRestorePlan.mockResolvedValue(basePlan);
  api.openPath.mockResolvedValue(undefined);
  api.prepareSupport.mockResolvedValue({
    support_id: "incident",
    codex_text: "Synthetic local diagnostic",
    github_text: "Synthetic public summary",
    saved: true,
    reveal_id: null,
    can_recheck: false,
  });
  api.openRestoredThread.mockResolvedValue("registered");
  api.rollbackTransaction.mockResolvedValue({
    transaction_id: committedTransaction.transaction_id,
    completed_at: "2026-07-23T09:10:00Z",
    restored_files: 8,
    success: true,
  });
  api.applyRestore.mockResolvedValue({
    transaction_id: committedTransaction.transaction_id,
    package_id: preview.manifest.package_id,
    completed_at: "2026-07-23T09:05:00Z",
    restored_files: 8,
    restored_bytes: 4096,
    registrations: [],
    verification: {
      package_checksum_valid: true,
      files_valid: true,
      sessions_valid: true,
      session_index_valid: true,
      sqlite_threads_valid: true,
      path_mapping_valid: true,
      forbidden_files_absent: true,
      project_files_valid: true,
      app_registration_valid: true,
      app_visible_ready: true,
    },
  });
  updater.checkForUpdates.mockResolvedValue({
    status: "current",
    currentVersion: "0.1.4",
  });
  updater.installCheckedUpdate.mockResolvedValue(undefined);
});

async function menu(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.click(screen.getByRole("button", { name: "更多选项" }));
  await user.click(screen.getByRole("menuitem", { name }));
}
async function plannedImport(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "前往导入" }));
  await user.click(screen.getByRole("button", { name: "选择迁移包" }));
  await user.click(await screen.findByRole("button", { name: "继续" }));
  await user.click(screen.getByRole("button", { name: "选择项目保存位置" }));
  await user.click(screen.getByRole("button", { name: "预览导入内容" }));
}
describe("V2 utility safeguards", () => {
  it.each([true, false])(
    "invalidates a preserved import result after undo settles with success=%s",
    async (success) => {
      const user = userEvent.setup();
      api.listTransactions.mockResolvedValue({
        transactions: [committedTransaction],
        warnings: [],
      });
      api.rollbackTransaction.mockImplementation(async () => {
        if (success)
          api.listTransactions.mockResolvedValue({
            transactions: [{ ...committedTransaction, status: "rolled_back" }],
            warnings: [],
          });
        return {
          transaction_id: committedTransaction.transaction_id,
          success,
          restored_files: 8,
          completed_at: "now",
        };
      });
      render(<App />);
      await screen.findByText(inventory.codex_home);
      await plannedImport(user);
      await user.click(
        screen.getByRole("checkbox", { name: "确认已保存当前 Codex 工作" }),
      );
      await user.click(screen.getByRole("button", { name: "导入到 Codex" }));
      await user.click(
        await screen.findByRole("button", { name: "确认旧对话能用" }),
      );
      const useDialog = screen.getByRole("dialog");
      for (const check of within(useDialog).getAllByRole("checkbox"))
        await user.click(check);
      await user.click(
        within(useDialog).getByRole("button", { name: "确认能用" }),
      );
      expect(screen.getByText("已由你确认能用")).toBeVisible();
      await user.click(screen.getByRole("button", { name: "查看迁移记录" }));
      await user.click(
        await screen.findByRole("button", { name: "回滚此事务" }),
      );
      await user.click(
        within(screen.getByRole("dialog")).getByRole("button", {
          name: "确认撤销",
        }),
      );
      await screen.findByText(
        success ? "已回滚" : "撤销未完成，请保留备份并查看诊断。",
      );
      await user.click(screen.getByRole("button", { name: "返回" }));
      expect(
        await screen.findByRole("button", { name: "预览导入内容" }),
      ).toBeVisible();
      expect(
        screen.queryByRole("button", { name: "确认旧对话能用" }),
      ).toBeNull();
      expect(screen.queryByText("已由你确认能用")).toBeNull();
      expect(
        localStorage.getItem(
          "rehome-use-confirmed:" + committedTransaction.transaction_id,
        ),
      ).toBeNull();
      expect(api.applyRestore).toHaveBeenCalledOnce();
    },
  );

  it.each(["export", "restore"])(
    "keeps the automatic %s failure solution in view instead of focusing the old heading",
    async (kind) => {
      const user = userEvent.setup();
      const failure = { message: "synthetic failure", support_id: "incident" };
      render(<App />);
      await screen.findByText(inventory.codex_home);
      if (kind === "export") {
        api.createPackage.mockRejectedValue(failure);
        await user.click(screen.getByRole("button", { name: "前往导出" }));
        await user.click(
          screen.getByRole("checkbox", { name: "选择项目 rehome-app" }),
        );
        await user.click(screen.getByRole("button", { name: "继续" }));
        await user.click(
          screen.getByRole("button", { name: "选择保存位置并创建" }),
        );
      } else {
        api.applyRestore.mockRejectedValue(failure);
        await plannedImport(user);
        await user.click(
          screen.getByRole("checkbox", { name: "确认已保存当前 Codex 工作" }),
        );
        await user.click(screen.getByRole("button", { name: "导入到 Codex" }));
      }
      const copy = await screen.findByRole("button", { name: "复制给 Codex" });
      expect(screen.getByRole("heading", { level: 1 })).not.toHaveFocus();
      const previewText = screen.getByRole("textbox", { name: "求助内容预览" });
      expect(
        copy.compareDocumentPosition(previewText) &
          Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
      expect(api.copySupportText).not.toHaveBeenCalled();
      expect(api.openSupportIssue).not.toHaveBeenCalled();
    },
  );
  it("keeps the interface usable when preference storage is unavailable and updates the document language", async () => {
    const read = vi
      .spyOn(Storage.prototype, "getItem")
      .mockImplementation(() => {
        throw new Error("storage disabled");
      });
    const write = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new Error("storage disabled");
      });
    try {
      const user = userEvent.setup();
      render(<App />);
      expect(document.documentElement.lang).toBe("zh-CN");
      await menu(user, "English");
      expect(
        screen.getByRole("heading", { name: "Bring your work with you." }),
      ).toBeVisible();
      expect(document.documentElement.lang).toBe("en");
    } finally {
      read.mockRestore();
      write.mockRestore();
    }
  });
  it("replaces the sidebar with a menu and preserves language, history and help access", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);
    expect(document.querySelector("aside.sidebar")).toBeNull();
    expect(
      screen.getByRole("heading", { name: "把工作带到新电脑。" }),
    ).toBeVisible();
    await menu(user, "帮助");
    expect(
      screen.getByRole("heading", { name: "需要一点帮助？" }),
    ).toBeVisible();
    await menu(user, "设置与关于");
    expect(screen.getByRole("heading", { name: "设置与关于" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Switch to English" }));
    expect(
      screen.getByRole("heading", { name: "Settings & about" }),
    ).toBeVisible();
  });
  it("reviews export selection before opening the native save picker", async () => {
    const user = userEvent.setup();
    api.createPackage.mockResolvedValue(null);
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往导出" }));
    await user.click(
      screen.getByRole("checkbox", { name: "选择项目 rehome-app" }),
    );
    await user.click(screen.getByRole("button", { name: "继续" }));
    expect(api.createPackage).not.toHaveBeenCalled();
    expect(
      screen.getByRole("heading", { name: "确认要带走的内容" }),
    ).toBeVisible();
    await user.click(
      screen.getByRole("button", { name: "选择保存位置并创建" }),
    );
    expect(api.createPackage).toHaveBeenCalledOnce();
    expect(
      screen.getByRole("heading", { name: "确认要带走的内容" }),
    ).toBeVisible();
  });
  it("filters projects without discarding hidden selections and uses mixed select-all", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往导出" }));
    await user.click(
      screen.getByRole("checkbox", { name: "选择项目 rehome-app" }),
    );
    expect(
      screen.getByRole("checkbox", { name: "全选迁移内容" }),
    ).toBePartiallyChecked();
    await user.type(
      screen.getByRole("searchbox", { name: "搜索项目或对话" }),
      "notes",
    );
    expect(
      screen.queryByRole("checkbox", { name: "选择项目 rehome-app" }),
    ).toBeNull();
    await user.clear(screen.getByRole("searchbox", { name: "搜索项目或对话" }));
    expect(
      screen.getByRole("checkbox", { name: "选择项目 rehome-app" }),
    ).toBeChecked();
    await menu(user, "设置与关于");
    await user.click(screen.getByRole("button", { name: "返回" }));
    expect(
      screen.getByRole("checkbox", { name: "选择项目 rehome-app" }),
    ).toBeChecked();
  });
  it("marks a project as partially selected when its files are selected but a chat is omitted", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往导出" }));
    await user.click(
      screen.getByRole("checkbox", { name: "选择项目 rehome-app" }),
    );
    await user.click(
      screen.getByRole("checkbox", { name: "选择对话 Desktop workflow" }),
    );
    expect(
      screen.getByRole("checkbox", { name: "选择项目 rehome-app" }),
    ).toBePartiallyChecked();
    await user.click(screen.getByRole("button", { name: "继续" }));
    expect(api.createPackage).not.toHaveBeenCalled();
    expect(screen.getByText("0 个对话 · 包含项目文件")).toBeVisible();
  });
  it("does not claim actual chat use from system verification and requires all three checks", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await plannedImport(user);
    await user.click(
      screen.getByRole("checkbox", { name: "确认已保存当前 Codex 工作" }),
    );
    await user.click(screen.getByRole("button", { name: "导入到 Codex" }));
    expect(await screen.findByText("实际使用待确认")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "确认旧对话能用" }));
    const dialog = screen.getByRole("dialog", { name: "旧对话能正常使用吗？" });
    const confirm = within(dialog).getByRole("button", { name: "确认能用" });
    expect(confirm).toBeDisabled();
    const checks = within(dialog).getAllByRole("checkbox");
    for (const check of checks) expect(check).not.toBeChecked();
    await user.click(checks[0]);
    await user.click(checks[1]);
    expect(confirm).toBeDisabled();
    await user.click(checks[2]);
    await user.click(confirm);
    expect(screen.getByText("已由你确认能用")).toBeVisible();
    expect(api.applyRestore).toHaveBeenCalledOnce();
  });
  it("cancels rollback safely and calls the backend only after explicit confirmation", async () => {
    const user = userEvent.setup();
    api.listTransactions.mockResolvedValue({
      transactions: [committedTransaction],
      warnings: [],
    });
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await menu(user, "迁移记录");
    await user.click(await screen.findByRole("button", { name: "回滚此事务" }));
    expect(api.rollbackTransaction).not.toHaveBeenCalled();
    let dialog = screen.getByRole("dialog", { name: "撤销这次导入？" });
    expect(within(dialog).getByRole("button", { name: "取消" })).toHaveFocus();
    await user.click(within(dialog).getByRole("button", { name: "取消" }));
    expect(api.rollbackTransaction).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "回滚此事务" }));
    dialog = screen.getByRole("dialog", { name: "撤销这次导入？" });
    await user.click(within(dialog).getByRole("button", { name: "确认撤销" }));
    expect(api.rollbackTransaction).toHaveBeenCalledWith(
      committedTransaction.transaction_id,
      "rollback",
    );
  });
  it("traps confirmation focus and restores it when Escape cancels", async () => {
    const user = userEvent.setup();
    api.listTransactions.mockResolvedValue({
      transactions: [committedTransaction],
      warnings: [],
    });
    const root = document.createElement("div");
    root.id = "root";
    document.body.appendChild(root);
    const view = render(<App />, { container: root });
    try {
      await screen.findByText(inventory.codex_home);
      await menu(user, "迁移记录");
      const undo = await screen.findByRole("button", { name: "回滚此事务" });
      await user.click(undo);
      expect(root).toHaveAttribute("inert");
      const dialog = screen.getByRole("dialog");
      const cancel = within(dialog).getByRole("button", { name: "取消" });
      const confirm = within(dialog).getByRole("button", { name: "确认撤销" });
      await user.tab({ shift: true });
      expect(confirm).toHaveFocus();
      await user.tab();
      expect(cancel).toHaveFocus();
      await user.keyboard("{Escape}");
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(root).not.toHaveAttribute("inert");
      expect(undo).toHaveFocus();
      expect(api.rollbackTransaction).not.toHaveBeenCalled();
    } finally {
      view.unmount();
      root.remove();
    }
  });
  it("does not advance an integrity-invalid package or call preview or restore", async () => {
    const user = userEvent.setup();
    api.inspectPackage.mockResolvedValue({ ...preview, checksum_valid: false });
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往导入" }));
    await user.click(screen.getByRole("button", { name: "选择迁移包" }));
    expect(await screen.findByRole("button", { name: "继续" })).toBeDisabled();
    expect(api.buildRestorePlan).not.toHaveBeenCalled();
    expect(api.applyRestore).not.toHaveBeenCalled();
  });
  it("confirms an update before any install and cancels without mutation", async () => {
    const user = userEvent.setup();
    updater.checkForUpdates.mockResolvedValue({
      status: "available",
      currentVersion: "0.1.28",
      version: "0.1.29",
      notes: "Updated interface",
    });
    render(<App />);
    await menu(user, "设置与关于");
    await user.click(
      await screen.findByRole("button", { name: "更新到 0.1.29" }),
    );
    expect(updater.installCheckedUpdate).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog", { name: "现在更新 ReHome？" });
    await user.click(within(dialog).getByRole("button", { name: "稍后" }));
    expect(updater.installCheckedUpdate).not.toHaveBeenCalled();
  });
  it("checks from the persistent version shortcut and shares the result with Settings", async () => {
    const user = userEvent.setup();
    updater.checkForUpdates
      .mockResolvedValueOnce({ status: "current", currentVersion: appVersion })
      .mockResolvedValueOnce({
        status: "available",
        currentVersion: appVersion,
        version: "0.1.30",
        notes: "Update details",
      });
    render(<App />);
    const footer = screen.getByRole("contentinfo", { name: "版本与更新" });
    const shortcut = await within(footer).findByRole("button", {
      name: `版本 ${appVersion}，检查更新`,
    });
    expect(shortcut).toHaveTextContent(`v${appVersion}`);
    expect(updater.checkForUpdates).toHaveBeenCalledTimes(1);
    shortcut.focus();
    await user.keyboard("{Enter}");
    const available = await within(footer).findByRole("button", {
      name: `版本 ${appVersion}，发现更新`,
    });
    expect(available).toHaveTextContent("发现更新");
    await user.click(available);
    const dialog = screen.getByRole("dialog", { name: "现在更新 ReHome？" });
    expect(within(dialog).getByText("Update details")).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "稍后" }));
    expect(updater.installCheckedUpdate).not.toHaveBeenCalled();
    expect(available).toHaveFocus();
    await menu(user, "设置与关于");
    expect(screen.getByRole("button", { name: "更新到 0.1.30" })).toBeEnabled();
    expect(updater.checkForUpdates).toHaveBeenCalledTimes(2);
    await user.click(screen.getByRole("button", { name: "ReHome 首页" }));
    await user.click(screen.getByRole("button", { name: "前往导出" }));
    expect(available).toBeVisible();
    expect(updater.checkForUpdates).toHaveBeenCalledTimes(2);
  });
  it("keeps the installed version visible offline and lets the footer retry in English", async () => {
    const user = userEvent.setup();
    updater.checkForUpdates
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce({ status: "current", currentVersion: appVersion });
    render(<App />);
    const footer = screen.getByRole("contentinfo", { name: "版本与更新" });
    await within(footer).findByText("检查失败，点击重试");
    expect(within(footer).getByText(`v${appVersion}`)).toBeVisible();
    await menu(user, "English");
    const retry = within(footer).getByRole("button", {
      name: `Version ${appVersion}, check for updates`,
    });
    expect(retry).toHaveTextContent("Check failed · Retry");
    await user.click(retry);
    expect(await within(footer).findByText("Up to date")).toBeVisible();
    expect(updater.checkForUpdates).toHaveBeenCalledTimes(2);
  });
  it("locks the workspace during an update started from the footer and unlocks after failure", async () => {
    const user = userEvent.setup();
    let reject!: (reason: unknown) => void;
    updater.checkForUpdates.mockResolvedValue({
      status: "available",
      currentVersion: appVersion,
      version: "0.1.30",
      notes: null,
    });
    updater.installCheckedUpdate.mockReturnValue(
      new Promise((_, fail) => {
        reject = fail;
      }),
    );
    render(<App />);
    const footer = screen.getByRole("contentinfo", { name: "版本与更新" });
    await user.click(
      await within(footer).findByRole("button", {
        name: `版本 ${appVersion}，发现更新`,
      }),
    );
    await user.click(screen.getByRole("button", { name: "安装并重启" }));
    expect(document.querySelector("main.workspace")).toHaveAttribute("inert");
    expect(within(footer).getByRole("button")).toBeDisabled();
    expect(within(footer).getByRole("progressbar")).not.toHaveAttribute(
      "value",
    );
    expect(updater.installCheckedUpdate).toHaveBeenCalledOnce();
    await act(async () => reject(new Error("download failed")));
    expect(document.querySelector("main.workspace")).not.toHaveAttribute(
      "inert",
    );
    expect(within(footer).getByRole("button")).toBeEnabled();
    expect(within(footer).getByText("更新未完成，点击重试")).toBeVisible();
  });
  it("locks navigation during an actual restore and shows no fabricated progress stages", async () => {
    const user = userEvent.setup();
    let reject!: (reason: unknown) => void;
    api.applyRestore.mockReturnValue(
      new Promise((_r, j) => {
        reject = j;
      }),
    );
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await plannedImport(user);
    await user.click(
      screen.getByRole("checkbox", { name: "确认已保存当前 Codex 工作" }),
    );
    await user.click(screen.getByRole("button", { name: "导入到 Codex" }));
    expect(screen.getByRole("button", { name: "ReHome 首页" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "更多选项" })).toBeDisabled();
    expect(
      within(screen.getByRole("contentinfo", { name: "版本与更新" })).getByRole(
        "button",
      ),
    ).toBeDisabled();
    expect(screen.queryByLabelText("导入进度")).toBeNull();
    await act(async () => reject(new Error("test stop")));
    expect(
      within(screen.getByRole("contentinfo", { name: "版本与更新" })).getByRole(
        "button",
      ),
    ).toBeEnabled();
  });
});
