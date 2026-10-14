import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { inventory } from "./test/fixtures";

const api = vi.hoisted(() => ({
  discoverCodex: vi.fn(), listTransactions: vi.fn(), createPackage: vi.fn(),
  applyRestore: vi.fn(), buildRestorePlan: vi.fn(), inspectPackage: vi.fn(),
  openPath: vi.fn(), openRestoredThread: vi.fn(), rollbackTransaction: vi.fn(),
  selectRestoreDestinations: vi.fn(), prepareSupport: vi.fn(),
  copySupportText: vi.fn(), openSupportIssue: vi.fn(),
}));
vi.mock("./lib/api", () => api);
vi.mock("./lib/updater", () => ({
  checkForUpdates: vi.fn().mockResolvedValue({ status: "current", currentVersion: "0.1.33" }),
  installCheckedUpdate: vi.fn(),
}));
beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  api.discoverCodex.mockResolvedValue(inventory);
  api.listTransactions.mockResolvedValue({ transactions: [], warnings: [] });
});

const compressed = {
  ...inventory.conversations[0],
  task_id: "12121212-1212-4121-8121-121212121212",
  title: "Compressed old chat",
  archive_path: "codex/sessions/old.jsonl.zst",
};

describe("Compressed rollout recognition", () => {
  it("excludes compressed chats from project, recommended and bulk selection", async () => {
    const user = userEvent.setup();
    api.discoverCodex.mockResolvedValue({
      ...inventory, conversations: [inventory.conversations[0], compressed],
      unsupported_conversation_paths: ["C:/synthetic/sessions/old.jsonl.zst"],
    });
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "前往导出" }));
    expect(screen.getByText(/有 1 个压缩会话文件无法读取/)).toBeVisible();
    await user.click(screen.getByRole("checkbox", { name: "选择项目 rehome-app" }));
    const compressedBox = screen.getByRole("checkbox", { name: "选择对话 Compressed old chat" });
    expect(compressedBox).toBeDisabled();
    expect(compressedBox).not.toBeChecked();
    await user.click(screen.getByRole("checkbox", { name: "全选迁移内容" }));
    expect(screen.getByRole("checkbox", { name: "全选迁移内容" })).toBeChecked();
    await user.click(screen.getByRole("button", { name: "只选主对话" }));
    expect(compressedBox).not.toBeChecked();
    await user.click(screen.getByRole("button", { name: "继续" }));
    expect(screen.getByText(/有 1 个压缩会话文件无法读取/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "选择保存位置并创建" }));
    expect(api.createPackage.mock.calls[0][0].conversation_ids)
      .toEqual([inventory.conversations[0].task_id]);
  });

  it("explains compressed-only and unidentified files in English without enabling export", async () => {
    const user = userEvent.setup();
    api.discoverCodex.mockResolvedValue({
      ...inventory, projects: [], conversations: [{ ...compressed, project_id: null }],
      skills: [], plugins: [], generated_images: [],
      unsupported_conversation_paths: ["C:/synthetic/sessions/old.jsonl.zst", "C:/unidentified.jsonl.zst"],
    });
    localStorage.setItem("rehome.locale", "en");
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Go to Export" }));
    expect(screen.getByText(/Cannot read 2 compressed chat files/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Expand project Conversations without a project" }));
    expect(screen.getByRole("checkbox", { name: "Select conversation Compressed old chat" }))
      .toBeDisabled();
    expect(screen.getByRole("checkbox", { name: "Select all migration content" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Continue" })).toBeDisabled();
  });

  it("uses a singular English notice and derives recognition from older inventory shapes", async () => {
    api.discoverCodex.mockResolvedValue({
      ...inventory, conversations: [compressed],
    });
    localStorage.setItem("rehome.locale", "en");
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Go to Export" }));
    expect(screen.getByText(/Cannot read 1 compressed chat file;/)).toBeVisible();
  });

  it("includes readable compressed chats in recommended and bulk export", async () => {
    const user = userEvent.setup();
    api.discoverCodex.mockResolvedValue({ ...inventory,
      conversations: [inventory.conversations[0], compressed], unsupported_conversation_paths: [],
    });
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "前往导出" }));
    await user.click(screen.getByRole("checkbox", { name: "选择项目 rehome-app" }));
    const box = screen.getByRole("checkbox", { name: "选择对话 Compressed old chat" });
    expect(box).toBeEnabled();
    expect(box).toBeChecked();
    await user.click(screen.getByRole("button", { name: "只选主对话" }));
    expect(box).toBeChecked();
    await user.click(screen.getByRole("checkbox", { name: "全选迁移内容" }));
    expect(box).toBeChecked();
    await user.click(screen.getByRole("button", { name: "继续" }));
    await user.click(screen.getByRole("button", { name: "选择保存位置并创建" }));
    expect(api.createPackage.mock.calls[0][0].conversation_ids)
      .toEqual([inventory.conversations[0].task_id, compressed.task_id]);
  });
});
