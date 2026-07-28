import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";

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
}));

vi.mock("./lib/api", () => api);

const inventory = {
  codex_home: "C:\\Users\\Me\\.codex",
  source_os: "windows",
  source_arch: "x86_64",
  source_device_id: "11111111-1111-1111-1111-111111111111",
  counts: {
    projects: 2,
    project_files: 18,
    conversations: 5,
    skills: 3,
    plugins: 2,
    generated_images: 4,
    sqlite_threads: 5,
  },
  projects: [
    {
      project_id: "22222222-2222-2222-2222-222222222222",
      name: "rehome-app",
      source_path: "C:\\Work\\rehome-app",
      archive_path: "projects/rehome-app",
      file_count: 12,
      content_bytes: 2048,
      git_remote: null,
      git_branch: "main",
      git_head: null,
    },
    {
      project_id: "33333333-3333-3333-3333-333333333333",
      name: "notes",
      source_path: "C:\\Work\\notes",
      archive_path: "projects/notes",
      file_count: 6,
      content_bytes: 1024,
      git_remote: null,
      git_branch: null,
      git_head: null,
    },
  ],
  project_paths: ["C:\\Work\\rehome-app", "C:\\Work\\notes"],
  conversations: [
    {
      task_id: "44444444-4444-4444-4444-444444444444",
      project_id: "22222222-2222-2222-2222-222222222222",
      title: "Desktop workflow",
      updated_at: "2026-07-23T08:00:00Z",
      content_hash: "abc",
      archive_path: "codex/sessions/desktop.jsonl",
      classification: null,
    },
    {
      task_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      project_id: "33333333-3333-3333-3333-333333333333",
      title: "Notes workflow",
      updated_at: "2026-07-23T07:00:00Z",
      content_hash: "def",
      archive_path: "codex/sessions/notes.jsonl",
      classification: {
        parent_task_id: "44444444-4444-4444-4444-444444444444",
        agent_path: "/root/review_notes",
        agent_nickname: "Reviewer",
        depth: 1,
      },
    },
    {
      task_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      project_id: null,
      title: "General workflow",
      updated_at: "2026-07-23T06:00:00Z",
      content_hash: "ghi",
      archive_path: "codex/sessions/general.jsonl",
      classification: null,
    },
  ],
  conversation_paths: ["C:\\Users\\Me\\.codex\\sessions\\desktop.jsonl"],
  session_index_path: "C:\\Users\\Me\\.codex\\session_index.jsonl",
  state_db_path: "C:\\Users\\Me\\.codex\\state_5.sqlite",
  skill_paths: [],
  plugin_paths: [],
  generated_image_paths: [],
  skills: [
    {
      content_id: "10101010-1010-4010-8010-101010101010",
      name: "imagegen",
      source_path: "C:\\Users\\Me\\.codex\\skills\\imagegen\\SKILL.md",
      relative_path: "imagegen",
      size_bytes: 2048,
      thumbnail_data_url: null,
      reveal_id: null,
    },
  ],
  plugins: [
    {
      content_id: "20202020-2020-4020-8020-202020202020",
      name: "computer-use",
      source_path: "C:\\Users\\Me\\.codex\\plugins\\cache\\computer-use\\plugin.json",
      relative_path: "computer-use/1.0.0",
      size_bytes: 4096,
      thumbnail_data_url: null,
      reveal_id: null,
    },
  ],
  generated_images: [
    {
      content_id: "30303030-3030-4030-8030-303030303030",
      name: "result.png",
      source_path: "C:\\Users\\Me\\.codex\\generated_images\\result.png",
      relative_path: "result.png",
      size_bytes: 8192,
      thumbnail_data_url: "data:image/png;base64,dGVzdA==",
      reveal_id: "40404040-4040-4040-8040-404040404040",
    },
  ],
  warnings: [],
};

const preview = {
  selection_id: "12121212-1212-4121-8121-121212121212",
  package_path: "C:\\Transfers\\from-mac.rehome",
  archive_hash: "4f92c9d8e1a0",
  manifest: {
    format: "codex-rehome",
    schema_version: 1,
    package_id: "55555555-5555-5555-5555-555555555555",
    created_at: "2026-07-22T08:00:00Z",
    source_os: "macos",
    source_arch: "aarch64",
    source_device_id: "66666666-6666-6666-6666-666666666666",
    mode: "full",
    parent_checkpoint: null,
    counts: {
      projects: 1,
      project_files: 12,
      conversations: 3,
      skills: 2,
      plugins: 1,
      generated_images: 4,
      sqlite_threads: 3,
    },
    projects: [],
    conversations: [],
    exclusions: { excluded_files: 6, excluded_bytes: 1200, rules: [] },
  },
  checksum_valid: true,
  entries: [],
  forbidden_files_total: 0,
};

const basePlan = {
  plan_id: "77777777-7777-7777-7777-777777777777",
  package_path: preview.package_path,
  package_id: preview.manifest.package_id,
  archive_hash: preview.archive_hash,
  target_codex_home: inventory.codex_home,
  projects_root: "C:\\Restored Projects",
  operations: [
    {
      package_source: "projects/rehome-app/README.md",
      target: "C:\\Restored Projects\\rehome-app\\README.md",
      expected_previous_hash: null,
      action: "add",
      rollback_required: true,
    },
  ],
  sessions: [],
  reference_rewrites: [],
  bridge_verification: { session_index: null, sqlite_database: null },
  conflict_count: 0,
  required_bytes: 4096,
};

const committedTransaction = {
  transaction_id: "88888888-8888-8888-8888-888888888888",
  package_id: preview.manifest.package_id,
  created_at: "2026-07-23T09:00:00Z",
  status: "committed",
  backup_root: "C:\\ReHome Backups",
  transaction_backup_path:
    "C:\\ReHome Backups\\88888888-8888-8888-8888-888888888888",
  target_codex_home: inventory.codex_home,
  projects_root: "C:\\Restored Projects",
  restored_project_paths: ["C:\\Restored Projects\\rehome-app"],
  changed_files: 8,
};

beforeEach(() => {
  vi.clearAllMocks();
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
});

async function openReceive(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "前往接收" }));
  await user.click(screen.getByRole("button", { name: "选择 ReHome 包" }));
  await screen.findByText("macOS");
  await user.click(screen.getByRole("button", { name: "选择恢复位置" }));
  await user.click(screen.getByRole("button", { name: "生成恢复计划" }));
  await screen.findByText("projects/rehome-app/README.md");
}

describe("ReHome Desktop workflows", () => {
  it("shows the detected Codex home and content counts", async () => {
    render(<App />);

    expect(await screen.findByText("C:\\Users\\Me\\.codex")).toBeInTheDocument();
    const counts = screen.getByLabelText("内容数量");
    expect(counts).toHaveTextContent("2 个项目");
    expect(counts).toHaveTextContent("5 个对话");
    expect(counts).toHaveTextContent("3 个技能");
    expect(counts).toHaveTextContent("2 个插件");
    expect(counts).toHaveTextContent("4 张生成图片");
  });

  it.each([
    ["rollback_failed", "回滚失败"],
    ["prepared", "已准备"],
  ] as const)("shows %s as %s in the recent handoff", async (status, label) => {
    api.listTransactions.mockResolvedValue({
      transactions: [{ ...committedTransaction, status }],
      warnings: [],
    });

    render(<App />);

    expect(await screen.findByText(label)).toBeInTheDocument();
  });

  it("allows project files and conversations to be selected independently", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);

    await user.click(screen.getByRole("button", { name: "前往发送" }));
    const createButton = screen.getByRole("button", { name: "创建 ReHome 包" });
    expect(createButton).toBeDisabled();

    await user.click(screen.getByRole("checkbox", { name: "选择项目 rehome-app" }));
    expect(createButton).toBeEnabled();

    await user.click(screen.getByRole("checkbox", { name: "选择项目 rehome-app" }));
    expect(createButton).toBeDisabled();
    await user.click(screen.getByRole("checkbox", { name: "选择对话 Desktop workflow" }));
    expect(createButton).toBeEnabled();
  });

  it("groups conversations under project accordions and keeps unassociated chats separate", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);

    await user.click(screen.getByRole("button", { name: "前往发送" }));
    expect(screen.queryByRole("checkbox", { name: "选择对话 Desktop workflow" })).toBeNull();

    await user.click(screen.getByRole("button", { name: "展开项目 rehome-app" }));

    expect(screen.getByRole("checkbox", { name: "选择对话 Desktop workflow" })).toBeVisible();
    expect(screen.queryByRole("checkbox", { name: "选择对话 General workflow" })).toBeNull();
    expect(screen.queryByRole("checkbox", { name: "选择对话 Notes workflow" })).toBeNull();

    await user.click(screen.getByRole("button", { name: "展开项目 未归属项目的对话" }));
    expect(screen.getByRole("checkbox", { name: "选择对话 General workflow" })).toBeVisible();
  });

  it("expands a project without selecting its files", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);

    await user.click(screen.getByRole("button", { name: "前往发送" }));

    expect(screen.getByRole("checkbox", { name: "选择项目 rehome-app" })).toBeInTheDocument();
    expect(
      screen.queryByRole("checkbox", { name: "选择对话 Desktop workflow" }),
    ).toBeNull();
    await user.click(screen.getByRole("button", { name: "展开项目 rehome-app" }));
    expect(
      screen.getByRole("checkbox", { name: "选择对话 Desktop workflow" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "选择项目 rehome-app" })).not.toBeChecked();
  });

  it("selects every project conversation by default and allows individual removal", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往发送" }));

    await user.click(screen.getByRole("checkbox", { name: "选择项目 rehome-app" }));
    const conversation = screen.getByRole("checkbox", { name: "选择对话 Desktop workflow" });
    expect(conversation).toBeChecked();

    await user.click(conversation);
    expect(conversation).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "选择项目 rehome-app" })).toBeChecked();
  });

  it("selects and clears every migration item with the global select control", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往发送" }));

    const selectAll = screen.getByRole("checkbox", { name: "全选迁移内容" });
    await user.click(selectAll);

    expect(selectAll).toBeChecked();
    expect(screen.getByRole("button", { name: "创建 ReHome 包" })).toBeEnabled();

    await user.click(selectAll);

    expect(selectAll).not.toBeChecked();
    expect(screen.getByRole("button", { name: "创建 ReHome 包" })).toBeDisabled();
  });

  it("allows a conversation-only package without selecting project files", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往发送" }));
    await user.click(screen.getByRole("button", { name: "展开项目 rehome-app" }));
    await user.click(screen.getByRole("checkbox", { name: "选择对话 Desktop workflow" }));
    await user.click(screen.getByRole("button", { name: "创建 ReHome 包" }));

    expect(api.createPackage).toHaveBeenCalledWith({
      project_ids: [],
      conversation_ids: ["44444444-4444-4444-4444-444444444444"],
      skill_ids: [],
      plugin_ids: [],
      generated_image_ids: [],
    });
  });

  it("lists optional content and packages only the selected entries", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往发送" }));

    await user.click(screen.getAllByRole("button", { name: /已选 0 \/ 1/ })[0]);
    await user.click(screen.getByRole("checkbox", { name: "全选 Skills" }));
    expect(screen.getAllByText("imagegen").length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: "创建 ReHome 包" }));
    expect(api.createPackage).toHaveBeenCalledWith({
      project_ids: [],
      conversation_ids: [],
      skill_ids: [inventory.skills[0].content_id],
      plugin_ids: [],
      generated_image_ids: [],
    });
  });

  it("labels main and subagent conversations and selects only recommended main tasks", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往发送" }));
    await user.click(screen.getByRole("button", { name: "展开项目 notes" }));

    expect(screen.getByText("子 Agent · L1")).toBeVisible();
    expect(screen.getByText("辅助记录，通常可不迁移")).toBeVisible();
    expect(screen.queryByRole("button", { name: "只选主对话" })).toBeNull();

    await user.click(screen.getByRole("button", { name: "展开项目 rehome-app" }));
    await user.click(screen.getByRole("button", { name: "只选主对话" }));
    expect(screen.getByRole("checkbox", { name: "选择对话 Desktop workflow" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "选择对话 Notes workflow" })).not.toBeChecked();
  });

  it("shows generated image thumbnails and reveals their source files", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往发送" }));
    await user.click(screen.getAllByRole("button", { name: /已选 0 \/ 1/ })[2]);

    expect(document.querySelector("img.image-thumbnail")).not.toBeNull();
    await user.click(screen.getByRole("button", { name: "在文件夹中显示 result.png" }));
    expect(api.openPath).toHaveBeenCalledWith(inventory.generated_images[0].reveal_id);
  });

  it("shows package integrity, conflicts, and destination before restore enablement", async () => {
    const user = userEvent.setup();
    api.buildRestorePlan.mockResolvedValue({
      ...basePlan,
      conflict_count: 1,
      operations: [
        {
          ...basePlan.operations[0],
          action: "conflict",
          expected_previous_hash: "different-hash",
        },
      ],
    });
    render(<App />);
    await screen.findByText(inventory.codex_home);

    await openReceive(user);

    expect(screen.getByText("macOS")).toBeInTheDocument();
    expect(screen.getByText("3 个对话")).toBeInTheDocument();
    expect(screen.getByText("校验通过")).toBeInTheDocument();
    expect(screen.getByText("禁用文件 0")).toBeInTheDocument();
    expect(screen.getByText("冲突 1")).toBeInTheDocument();
    expect(screen.queryByText("事务备份")).toBeNull();
    expect(screen.getByText("安全备份由 ReHome 自动管理")).toBeInTheDocument();
    expect(screen.getAllByText("C:\\Restored Projects").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "开始恢复" })).toBeDisabled();
  });

  it("disables every native picker while a location selection is pending", async () => {
    const user = userEvent.setup();
    const pending = deferred<Awaited<ReturnType<typeof api.selectRestoreDestinations>>>();
    api.selectRestoreDestinations.mockReturnValue(pending.promise);
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往接收" }));
    await user.click(screen.getByRole("button", { name: "选择 ReHome 包" }));
    await screen.findByText("macOS");

    await user.click(screen.getByRole("button", { name: "选择恢复位置" }));

    expect(screen.getByRole("button", { name: "选择 ReHome 包" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "选择恢复位置" })).toBeDisabled();
    pending.resolve(null);
  });

  it("ignores a stale location selection that resolves after a newer request", async () => {
    const user = userEvent.setup();
    const first = deferred<Awaited<ReturnType<typeof api.selectRestoreDestinations>>>();
    const second = deferred<Awaited<ReturnType<typeof api.selectRestoreDestinations>>>();
    api.selectRestoreDestinations
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往接收" }));
    await user.click(screen.getByRole("button", { name: "选择 ReHome 包" }));
    await screen.findByText("macOS");
    const picker = screen.getByRole("button", { name: "选择恢复位置" });

    act(() => {
      picker.click();
      picker.click();
    });
    expect(api.selectRestoreDestinations).toHaveBeenCalledTimes(2);
    await act(async () => {
      second.resolve({
        selection_id: "14141414-1414-4141-8141-141414141414",
        target_codex_home: inventory.codex_home,
        projects_root: "C:\\Newest Projects",
        backup_root: "C:\\Newest Backups",
      });
      await second.promise;
    });
    await act(async () => {
      first.resolve({
        selection_id: "15151515-1515-4151-8151-151515151515",
        target_codex_home: inventory.codex_home,
        projects_root: "C:\\Stale Projects",
        backup_root: "C:\\Stale Backups",
      });
      await first.promise;
    });

    expect(screen.getByText("C:\\Newest Projects")).toBeInTheDocument();
    expect(screen.queryByText("C:\\Stale Projects")).toBeNull();
  });

  it("clears restore results and confirmation when the selected location changes", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await openReceive(user);
    await user.click(screen.getByRole("checkbox", { name: "确认已保存当前 Codex 工作" }));
    await user.click(screen.getByRole("button", { name: "开始恢复" }));
    expect(await screen.findByText("恢复事务已提交")).toBeInTheDocument();

    api.selectRestoreDestinations.mockResolvedValueOnce({
      selection_id: "16161616-1616-4161-8161-161616161616",
      target_codex_home: inventory.codex_home,
      projects_root: "C:\\Changed Projects",
      backup_root: "C:\\Changed Backups",
    });
    await user.click(screen.getByRole("button", { name: "选择恢复位置" }));

    expect(screen.queryByText("恢复事务已提交")).toBeNull();
    expect(screen.queryByText("projects/rehome-app/README.md")).toBeNull();
    await user.click(screen.getByRole("button", { name: "生成恢复计划" }));
    expect(screen.getByRole("checkbox", { name: "确认已保存当前 Codex 工作" })).not.toBeChecked();
  });

  it("uses the exact manual-open status when registration is incomplete", async () => {
    const user = userEvent.setup();
    api.applyRestore.mockResolvedValue({
      transaction_id: committedTransaction.transaction_id,
      package_id: preview.manifest.package_id,
      completed_at: "2026-07-23T09:05:00Z",
      restored_files: 8,
      restored_bytes: 4096,
      registrations: [
        {
          project_id: "22222222-2222-2222-2222-222222222222",
          project_path: "C:\\Restored Projects\\rehome-app",
          status: "manual_open_required",
        },
      ],
      verification: {
        package_checksum_valid: true,
        files_valid: true,
        sessions_valid: true,
        session_index_valid: true,
        sqlite_threads_valid: true,
        path_mapping_valid: true,
        forbidden_files_absent: true,
        project_files_valid: true,
        app_registration_valid: false,
        app_visible_ready: false,
      },
    });
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await openReceive(user);

    await user.click(screen.getByRole("checkbox", { name: "确认已保存当前 Codex 工作" }));
    await user.click(screen.getByRole("button", { name: "开始恢复" }));

    expect(
      await screen.findByText("项目文件已恢复，需要在 Codex 中手动打开"),
    ).toBeInTheDocument();
  });

  it("shows invocation failure returned while opening a restored project", async () => {
    const user = userEvent.setup();
    api.applyRestore.mockResolvedValue(restoreReportWithRegistration("manual_open_required"));
    api.openRestoredThread.mockResolvedValue({
      invocation_failed: { message: "Codex 命令调用失败" },
    });
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await openReceive(user);
    await user.click(screen.getByRole("checkbox", { name: "确认已保存当前 Codex 工作" }));
    await user.click(screen.getByRole("button", { name: "开始恢复" }));

    await user.click(screen.getByRole("button", { name: "在 Codex 中打开" }));

    expect(await screen.findByText("Codex 命令调用失败")).toBeInTheDocument();
  });

  it("shows the exact manual status returned while opening a restored project", async () => {
    const user = userEvent.setup();
    api.applyRestore.mockResolvedValue(restoreReportWithRegistration("manual_open_required"));
    api.openRestoredThread.mockResolvedValue("manual_open_required");
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await openReceive(user);
    await user.click(screen.getByRole("checkbox", { name: "确认已保存当前 Codex 工作" }));
    await user.click(screen.getByRole("button", { name: "开始恢复" }));

    await user.click(screen.getByRole("button", { name: "在 Codex 中打开" }));

    expect(
      await screen.findAllByText("项目文件已恢复，需要在 Codex 中手动打开"),
    ).toHaveLength(2);
  });

  it("shows caught errors while opening a restored project", async () => {
    const user = userEvent.setup();
    api.applyRestore.mockResolvedValue(restoreReportWithRegistration("manual_open_required"));
    api.openRestoredThread.mockRejectedValue({ message: "无法调用 Codex" });
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await openReceive(user);
    await user.click(screen.getByRole("checkbox", { name: "确认已保存当前 Codex 工作" }));
    await user.click(screen.getByRole("button", { name: "开始恢复" }));

    await user.click(screen.getByRole("button", { name: "在 Codex 中打开" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("无法调用 Codex");
  });

  it("only enables rollback for committed transactions", async () => {
    const user = userEvent.setup();
    api.listTransactions.mockResolvedValue({
      transactions: [
        committedTransaction,
        {
          ...committedTransaction,
          transaction_id: "99999999-9999-9999-9999-999999999999",
          status: "rolled_back",
        },
      ],
      warnings: [],
    });
    render(<App />);
    await screen.findByText(inventory.codex_home);

    await user.click(screen.getByRole("button", { name: "前往历史" }));

    const committedRow = await screen.findByTestId(
      `transaction-${committedTransaction.transaction_id}`,
    );
    const rolledBackRow = screen.getByTestId(
      "transaction-99999999-9999-9999-9999-999999999999",
    );
    expect(within(committedRow).getByRole("button", { name: "回滚此事务" })).toBeEnabled();
    expect(within(rolledBackRow).getByRole("button", { name: "回滚此事务" })).toBeDisabled();
  });

  it("offers a distinct recovery action for incomplete and failed rollbacks", async () => {
    const user = userEvent.setup();
    const prepared = {
      ...committedTransaction,
      transaction_id: "99999999-9999-4999-8999-999999999999",
      status: "prepared",
    };
    api.listTransactions.mockResolvedValue({ transactions: [prepared], warnings: [] });
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往历史" }));

    const resume = await screen.findByRole("button", { name: "继续回滚事务" });
    expect(resume).toBeEnabled();
    await user.click(resume);

    expect(api.rollbackTransaction).toHaveBeenCalledWith(prepared.transaction_id, "resume");
  });

  it("shows history reveal errors", async () => {
    const user = userEvent.setup();
    api.listTransactions.mockResolvedValue({
      transactions: [committedTransaction],
      warnings: [],
    });
    api.openPath.mockRejectedValue({ message: "无法显示备份" });
    render(<App />);
    await screen.findByText(inventory.codex_home);
    await user.click(screen.getByRole("button", { name: "前往历史" }));

    await user.click(await screen.findByRole("button", { name: "显示备份" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("无法显示备份");
  });

  it("moves focus to the page heading after navigation", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);

    await user.click(screen.getByRole("button", { name: "前往历史" }));

    const heading = screen.getByRole("heading", { name: "历史记录" });
    expect(heading).toHaveAttribute("tabindex", "-1");
    expect(heading).toHaveFocus();
  });
});

function restoreReportWithRegistration(status: "manual_open_required") {
  return {
    transaction_id: committedTransaction.transaction_id,
    package_id: preview.manifest.package_id,
    completed_at: "2026-07-23T09:05:00Z",
    restored_files: 8,
    restored_bytes: 4096,
    registrations: [
      {
        project_id: "22222222-2222-2222-2222-222222222222",
        project_path: "C:\\Restored Projects\\rehome-app",
        status,
      },
    ],
    verification: {
      package_checksum_valid: true,
      files_valid: true,
      sessions_valid: true,
      session_index_valid: true,
      sqlite_threads_valid: true,
      path_mapping_valid: true,
      forbidden_files_absent: true,
      project_files_valid: true,
      app_registration_valid: false,
      app_visible_ready: false,
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}
